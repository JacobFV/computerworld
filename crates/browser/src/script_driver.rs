//! How the browser drives a scripted document: every action becomes a `UiEvent`
//! dispatched to the realm first, and the `DefaultAction` that comes back decides
//! what the browser itself does. After each dispatch the realm's event loop runs
//! until idle, layout observers are delivered, and the tab's mirrors (field values,
//! focus, scroll, URL, title) are read back. Navigations the page asked for are
//! performed once the event is over, never inside it.
//!
//! Time: `tick` advances the realm's timers with the world clock (each timer at its
//! own due time across the elapsed window, `requestAnimationFrame` once per 16 ms of
//! it) for the visible tab; a background tab's timers run at most once per second.
//! Every entry is bounded by the realm's step budget (`scripted::STEP_BUDGET`): a
//! script that never yields is interrupted, the error is on the console, and the
//! page stays usable.

use cw_determinism::Determinism;
use cw_protocol::{HttpRequest, HttpResponse, Page, Result, SimError};
use cw_web::dom::{Document, NodeId};
use cw_web::script::{DefaultAction, Modifiers, Realm, UiEvent};
use cw_web::Viewport;
use std::sync::Arc;
use url::Url;

use crate::scripted::{HostEnv, PendingNav, MAX_CONSOLE_LINES};
use crate::{BrowserState, ConsoleEntry, Content, WebDocument};

type Transport<'a, 't> = &'a mut (dyn FnMut(HttpRequest) -> Result<HttpResponse> + 't);
/// The transport type of an action that carries none.
pub(crate) type NoTransport = fn(HttpRequest) -> Result<HttpResponse>;

/// The longest stretch of world time one tick replays timer by timer, and the
/// stretch a page with a running `requestAnimationFrame` loop gets (frames are
/// 16 ms apart, so this bounds a tick to 15 of them).
const TICK_WINDOW_MS: u64 = 60_000;
const FRAME_WINDOW_MS: u64 = 250;
/// Background tabs run their timers this often, as browsers throttle them.
const BACKGROUND_INTERVAL: u64 = 1_000_000;
/// Navigations a page may chain from its own load before the browser stops it.
const MAX_SCRIPT_REDIRECTS: u32 = 8;

fn not_scripted() -> SimError {
    SimError::invalid("the tab shows no scripted document")
}

impl BrowserState {
    /// The console of the tab on show: what its pages logged, uncaught errors and
    /// budget interruptions, oldest first.
    pub fn console(&self) -> &[ConsoleEntry] {
        &self.tab().console
    }

    /// The world clock page scripts read (`Date.now`, `performance.now`, timers).
    pub fn set_clock(&mut self, now: u64) {
        self.clock = self.clock.max(now);
    }

    /// The world seed and the name page-script entropy streams live under; each tab
    /// draws from `<scope>/tab/<id>/page-script`.
    pub fn set_entropy(&mut self, seed: u64, scope: &str) {
        if self.entropy_seed != seed || self.entropy_scope != scope {
            self.entropy_seed = seed;
            self.entropy_scope = scope.to_owned();
        }
    }

    /// The content viewport in scene px. A scripted page on show sees `resize`.
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        let next = Some((width.max(1), height.max(1)));
        if self.viewport != next {
            self.viewport = next;
            self.sync_script_viewport();
        }
    }

    fn css_viewport(&self) -> (u32, u32) {
        let (w, h) = self.viewport.unwrap_or((1280, 800));
        let z = u64::from(self.zoom().clamp(10, 500));
        let css = |v: u32| ((u64::from(v) * 100).div_ceil(z)).max(1) as u32;
        (css(w), css(h))
    }

    pub(crate) fn sync_script_viewport(&mut self) {
        let (w, h) = self.css_viewport();
        let index = self.active;
        let differs = self.document().and_then(WebDocument::scripted).is_some_and(|s| {
            s.read(|realm| {
                let inner = realm.layout();
                (inner.viewport.width, inner.viewport.height) != (w, h)
            })
        });
        if differs {
            self.with_script_at(index, None, |realm| {
                realm.dispatch(UiEvent::Resize { width: w, height: h });
            });
        }
    }

    // ---------------------------------------------------------------------------
    // Entering the realm
    // ---------------------------------------------------------------------------

    /// Moves what the host needs out of the browser for one entry.
    fn host_env(&mut self, index: usize, url: &str) -> HostEnv {
        let (w, h) = self.css_viewport();
        let seed = self.entropy_seed;
        let scope = if self.entropy_scope.is_empty() { "browser" } else { self.entropy_scope.as_str() };
        let tab = &mut self.tabs[index];
        let stream = format!("{scope}/tab/{}/page-script", tab.id);
        HostEnv {
            cookies: std::mem::take(&mut self.cookies),
            storage: std::mem::take(&mut self.storage),
            session: std::mem::take(&mut tab.session_storage),
            now: self.clock,
            entropy: Some(tab.entropy.take().unwrap_or_else(|| Determinism::new(seed))),
            stream,
            viewport: Viewport { width: w, height: h, scale: 1, zoom: 100 },
            url: url.to_owned(),
            ..HostEnv::default()
        }
    }

    /// Takes back what the page did with it; returns the navigations it queued.
    fn take_env(&mut self, index: usize, mut env: HostEnv) -> Vec<PendingNav> {
        self.cookies = std::mem::take(&mut env.cookies);
        self.storage = std::mem::take(&mut env.storage);
        let tab = &mut self.tabs[index];
        tab.session_storage = std::mem::take(&mut env.session);
        // An untouched stream is not state worth keeping.
        tab.entropy = env.entropy.take().filter(|d| *d != Determinism::new(d.seed()));
        tab.console.append(&mut env.console);
        if tab.console.len() > MAX_CONSOLE_LINES {
            let drop = tab.console.len() - MAX_CONSOLE_LINES;
            tab.console.drain(..drop);
        }
        std::mem::take(&mut env.navs)
    }

    /// One entry into `web`'s realm on behalf of tab `index`, then the pictures the
    /// document now references are fetched and their sizes handed to its layout.
    fn enter_doc<T>(&mut self, web: &mut WebDocument, index: usize, mut transport: Option<Transport<'_, '_>>, f: impl FnOnce(&mut Realm) -> T) -> Option<(T, Vec<PendingNav>)> {
        if !web.is_scripted() {
            return None;
        }
        let mut env = self.host_env(index, &web.url);
        let out = env.with_transport(transport.as_deref_mut(), |env| web.script_enter(env, f));
        let now = env.now;
        let mut navs = self.take_env(index, env);
        if let Some(s) = web.scripted() {
            s.update_mirror(|m| m.last_run = now);
        }
        if let Some(mut t) = transport.take() {
            let wanted = web.script_image_references();
            if !wanted.is_empty() {
                let mut added = Vec::new();
                for (reference, resolved) in wanted {
                    match Url::parse(&resolved) {
                        Ok(target) => match self.load_image_from(target, &mut t, false, None) {
                            Ok(asset) => {
                                web.add_image(&reference, &resolved, asset);
                                added.push(reference);
                            }
                            Err(error) => web.add_image_error(&reference, &resolved, &error.code),
                        },
                        Err(_) => web.add_image_error(&reference, &resolved, "invalid"),
                    }
                }
                let mut env = self.host_env(index, &web.url);
                env.with_transport(Some(&mut t), |env| web.script_set_image_sizes(env, &added));
                navs.extend(self.take_env(index, env));
            }
        }
        out.map(|o| (o, navs))
    }

    /// Enters the realm of the document tab `index` shows, and mirrors the realm's
    /// form values, focus, scroll and URL back into the tab.
    pub(crate) fn with_script_at<T>(&mut self, index: usize, transport: Option<Transport<'_, '_>>, f: impl FnOnce(&mut Realm) -> T) -> Option<(T, Vec<PendingNav>)> {
        let tab = self.tabs.get_mut(index)?;
        let position = tab.position;
        let entry = tab.history.get_mut(position)?;
        if !entry.web().is_some_and(WebDocument::is_scripted) {
            return None;
        }
        let Content::Web(mut web) = std::mem::replace(&mut entry.content, Content::Page(Page::new(""))) else { unreachable!("checked above") };
        let out = self.enter_doc(&mut web, index, transport, f);
        let fields = web.script_fields();
        let (focused, scroll_y) = web.script_focus_and_scroll();
        let tab = &mut self.tabs[index];
        tab.fields = fields;
        tab.focused = focused;
        tab.scroll_y = scroll_y;
        let entry = &mut tab.history[position];
        entry.url = web.url.clone();
        entry.content = Content::Web(web);
        out
    }

    fn with_script<T>(&mut self, transport: Option<Transport<'_, '_>>, f: impl FnOnce(&mut Realm) -> T) -> Result<(T, Vec<PendingNav>)> {
        self.with_script_at(self.active, transport, f).ok_or_else(not_scripted)
    }

    // ---------------------------------------------------------------------------
    // Loading and leaving
    // ---------------------------------------------------------------------------

    /// A document with script: the realm parses it, runs its scripts in order
    /// (fetching `<script src>`, stylesheets and imports through the transport),
    /// fires `DOMContentLoaded` and `load`, and idles.
    pub(crate) fn load_scripted<F>(&mut self, html: &str, url: &Url, transport: &mut F, _fresh_images: bool) -> WebDocument
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let mut web = WebDocument::new_scripted(html, url.as_str(), self.css_viewport(), self.clock);
        let index = self.active;
        let navs = self.enter_doc(&mut web, index, Some(transport), |realm| {
            realm.run_document();
            realm.dispatch(UiEvent::PageShow);
        });
        // Queued until the entry is committed (`script_after_load`).
        self.load_navs = navs.map(|(_, n)| n).unwrap_or_default();
        web
    }

    pub(crate) fn script_after_load<F>(&mut self, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let fields = self.document().map(WebDocument::initial_fields).unwrap_or_default();
        let (focused, scroll_y) = self.document().map(WebDocument::script_focus_and_scroll).unwrap_or((None, 0));
        let url = self.document().map(|w| w.url.clone());
        let tab = self.tab_mut();
        tab.fields = fields;
        tab.focused = focused;
        tab.scroll_y = scroll_y;
        if let (Some(url), Some(entry)) = (url, self.entry_mut()) {
            entry.url = url;
        }
        let navs = std::mem::take(&mut self.load_navs);
        self.perform_navs(navs, transport)
    }

    /// The document on show is being left: `beforeunload` (its answer ignored),
    /// `pagehide`, `unload`; then its VM is dropped and only its journal kept.
    pub(crate) fn script_leave<F>(&mut self, transport: &mut F)
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let index = self.active;
        if self.with_script_at(index, Some(transport), |realm| realm.dispatch(UiEvent::Unload)).is_some() {
            if let Some(s) = self.document().and_then(WebDocument::scripted) {
                s.suspend();
            }
        }
    }

    pub(crate) fn script_unload(&mut self, index: usize) {
        let _ = self.with_script_at(index, None, |realm| realm.dispatch(UiEvent::Unload));
    }

    /// History traversal arrived at a scripted document: `pageshow`.
    pub(crate) fn script_arrive<F>(&mut self, transport: &mut F)
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let index = self.active;
        let _ = self.with_script_at(index, Some(transport), |realm| realm.dispatch(UiEvent::PageShow));
    }

    pub(crate) fn script_visibility(&mut self, index: usize, hidden: bool) {
        let known = self.tabs.get(index).and_then(|t| t.history.get(t.position)).and_then(|e| e.web()).and_then(WebDocument::scripted).map(|s| s.mirror().hidden);
        if known.is_some_and(|k| k != hidden) {
            let _ = self.with_script_at(index, None, |realm| realm.dispatch(UiEvent::Visibility { hidden }));
            if let Some(s) = self.tabs[index].history.get(self.tabs[index].position).and_then(|e| e.web()).and_then(WebDocument::scripted) {
                s.update_mirror(|m| m.hidden = hidden);
            }
        }
    }

    /// Back or forward inside the page's own history (`pushState` entries, fragment
    /// changes): `popstate`/`hashchange`, no request. False when the page has no
    /// entry that way and the tab's history should move instead.
    pub(crate) fn script_history_go<F>(&mut self, delta: i32, transport: &mut F) -> Result<bool>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let Some((index, len)) = self.document().and_then(WebDocument::scripted).map(|s| s.mirror().history) else { return Ok(false) };
        let target = index as i64 + i64::from(delta);
        if target < 0 || target >= len as i64 {
            return Ok(false);
        }
        let (_, navs) = self.with_script(Some(transport), |realm| realm.dispatch(UiEvent::HistoryGo { delta }))?;
        self.perform_navs(navs, transport)?;
        Ok(true)
    }

    // ---------------------------------------------------------------------------
    // Navigation the page asked for
    // ---------------------------------------------------------------------------

    /// Performs the last navigation the page queued (a later one supersedes an
    /// earlier one, as in a browser). `location.reload()` and a navigation to the
    /// current URL reload in place.
    fn perform_navs<F>(&mut self, navs: Vec<PendingNav>, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let Some(nav) = navs.into_iter().next_back() else { return Ok(()) };
        if self.nav_depth >= MAX_SCRIPT_REDIRECTS {
            self.tab_mut().console.push(ConsoleEntry { level: "error".into(), text: "navigation stopped: the page kept redirecting".into() });
            return Ok(());
        }
        self.nav_depth += 1;
        let result = match nav {
            PendingNav::Navigate { url, new_tab } => {
                if new_tab {
                    self.new_tab();
                    self.navigate_url(&url, transport)
                } else if self.url() == Some(url.as_str()) {
                    self.reload(transport)
                } else {
                    self.navigate_url(&url, transport)
                }
            }
            PendingNav::Submit { action, method, enctype, data } => match self.resolve(&action) {
                Ok(url) => {
                    let entries: Vec<(String, String, bool)> = data.into_iter().map(|(k, v)| (k, v, false)).collect();
                    let request = crate::web_document::encode_submission(url, &method.to_ascii_uppercase(), &enctype.to_ascii_lowercase(), &entries);
                    self.request(request, transport, false)
                }
                Err(e) => Err(e),
            },
        };
        self.nav_depth -= 1;
        result
    }

    fn after_action<F>(&mut self, action: DefaultAction, new_tab: bool, mut navs: Vec<PendingNav>, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        match action {
            DefaultAction::Navigate(url) => navs.push(PendingNav::Navigate { url, new_tab }),
            DefaultAction::Submit { action, method, enctype, data, .. } => navs.push(PendingNav::Submit { action, method, enctype, data }),
            _ => {}
        }
        self.perform_navs(navs, transport)
    }

    /// Without a transport a queued navigation cannot be performed; say so.
    fn drop_navs(&mut self, navs: Vec<PendingNav>, action: DefaultAction) {
        if !navs.is_empty() || matches!(action, DefaultAction::Navigate(_) | DefaultAction::Submit { .. }) {
            self.tab_mut().console.push(ConsoleEntry { level: "warn".into(), text: "navigation dropped: this action carries no network".into() });
        }
    }

    fn finish<F>(&mut self, action: DefaultAction, new_tab: bool, navs: Vec<PendingNav>, transport: Option<&mut F>) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        match transport {
            Some(t) => self.after_action(action, new_tab, navs, t),
            None => {
                self.drop_navs(navs, action);
                Ok(())
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Actions
    // ---------------------------------------------------------------------------

    fn script_node(&self, id: &str) -> Result<NodeId> {
        self.document().and_then(|w| w.node_for(id)).ok_or_else(|| SimError::not_found(format!("element {id}")))
    }

    /// Whether activating `node` opens a new tab (`<a target=_blank>`).
    fn opens_new_tab(&self, node: NodeId) -> bool {
        self.document().is_some_and(|w| {
            w.with_document(|d| {
                std::iter::once(node).chain(d.ancestors(node)).find(|n| (d.is(*n, "a") || d.is(*n, "area")) && d.has_attr(*n, "href")).is_some_and(|a| d.attr(a, "target").is_some_and(|t| t.trim().eq_ignore_ascii_case("_blank")))
            })
        })
    }

    pub(crate) fn script_click<F>(&mut self, id: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let node = self.script_node(id)?;
        let new_tab = self.opens_new_tab(node);
        let (action, navs) = self.with_script(Some(transport), |realm| realm.dispatch(UiEvent::ClickNode { node, modifiers: Modifiers::default(), detail: 1 }))?;
        self.after_action(action, new_tab, navs, transport)
    }

    pub(crate) fn script_click_at<F>(&mut self, x: i32, y: i32, width: u32, height: u32, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        self.set_viewport(width, height);
        let inputs = self.inputs(width, height);
        let hit = self.document().and_then(|w| w.hit(x, y, inputs)).ok_or_else(|| SimError::not_found("nothing to click there"))?;
        let new_tab = self.opens_new_tab(hit);
        let (cx, cy) = WebDocument::css_point(x, y, self.zoom());
        let (action, navs) = self.with_script(Some(transport), |realm| realm.dispatch(UiEvent::Click { x: cx, y: cy, button: 0, modifiers: Modifiers::default(), detail: 1 }))?;
        self.after_action(action, new_tab, navs, transport)
    }

    /// `fill` with the network at hand (an `input` handler may fetch suggestions).
    pub fn fill_with<F>(&mut self, id: &str, value: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_fill(id, value, Some(transport));
        }
        self.fill(id, value)
    }

    pub(crate) fn script_fill<F>(&mut self, id: &str, value: &str, mut transport: Option<&mut F>) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let node = self.document().and_then(|w| w.node_for(id)).ok_or_else(|| SimError::not_found(format!("input {id}")))?;
        #[derive(PartialEq)]
        enum Kind {
            Check(bool),
            Value,
            Other,
        }
        let kind = self.document().and_then(WebDocument::scripted).map_or(Kind::Other, |s| {
            s.read(|realm| {
                let inner = realm.layout();
                let d = &inner.doc;
                if d.is(node, "input") && matches!(crate::web_document::input_type(d, node).as_str(), "checkbox" | "radio") {
                    Kind::Check(inner.is_checked(node))
                } else if d.is(node, "select") || inner.is_text_control(node) || d.is(node, "input") {
                    Kind::Value
                } else {
                    Kind::Other
                }
            })
        });
        let events: Vec<UiEvent> = match kind {
            Kind::Other => return Err(SimError::not_found(format!("input {id}"))),
            Kind::Check(now) => {
                let on = matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on" | "checked" | "yes");
                if on == now {
                    vec![UiEvent::Focus { node: Some(node) }]
                } else {
                    vec![UiEvent::ClickNode { node, modifiers: Modifiers::default(), detail: 1 }]
                }
            }
            Kind::Value => vec![UiEvent::Focus { node: Some(node) }, UiEvent::SetValue { node, value: value.to_owned(), commit: true }],
        };
        let t: Option<Transport<'_, '_>> = match transport.as_deref_mut() {
            Some(t) => Some(t),
            None => None,
        };
        let (action, navs) = self.with_script(t, |realm| {
            let mut last = DefaultAction::None;
            for e in events {
                last = realm.dispatch(e);
            }
            last
        })?;
        self.finish(action, false, navs, transport)
    }

    /// `text` with the network at hand.
    pub fn text_with<F>(&mut self, text: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_text(text, Some(transport));
        }
        self.text(text)
    }

    pub(crate) fn script_text<F>(&mut self, text: &str, mut transport: Option<&mut F>) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let editable = self.document().and_then(WebDocument::scripted).is_some_and(|s| {
            s.read(|realm| {
                let inner = realm.layout();
                inner.focused.is_some_and(|f| inner.is_text_control(f) || inner.doc.has_attr(f, "contenteditable"))
            })
        });
        if !editable {
            return Err(SimError::invalid("no focused input"));
        }
        let t: Option<Transport<'_, '_>> = match transport.as_deref_mut() {
            Some(t) => Some(t),
            None => None,
        };
        let (action, navs) = self.with_script(t, |realm| realm.dispatch(UiEvent::TypeText { text: text.to_owned() }))?;
        self.finish(action, false, navs, transport)
    }

    pub(crate) fn script_key<F>(&mut self, key: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let mut modifiers = Modifiers::default();
        let mut name = key;
        loop {
            if let Some(rest) = name.strip_prefix("Shift+") {
                modifiers.shift = true;
                name = rest;
            } else if let Some(rest) = name.strip_prefix("Ctrl+").or_else(|| name.strip_prefix("Control+")) {
                modifiers.ctrl = true;
                name = rest;
            } else if let Some(rest) = name.strip_prefix("Alt+") {
                modifiers.alt = true;
                name = rest;
            } else if let Some(rest) = name.strip_prefix("Meta+").or_else(|| name.strip_prefix("Cmd+")) {
                modifiers.meta = true;
                name = rest;
            } else {
                break;
            }
        }
        let dom_key = match name {
            "Space" => " ",
            "Esc" => "Escape",
            "Return" => "Enter",
            "Left" => "ArrowLeft",
            "Right" => "ArrowRight",
            "Up" => "ArrowUp",
            "Down" => "ArrowDown",
            other => other,
        }
        .to_owned();
        let focused = self.document().and_then(WebDocument::scripted).and_then(|s| s.read(|realm| realm.focused()));
        let new_tab = focused.is_some_and(|f| self.opens_new_tab(f));
        let (action, navs) = self.with_script(Some(transport), |realm| realm.dispatch(UiEvent::Key { key: dom_key, code: String::new(), modifiers, repeat: false }))?;
        self.after_action(action, new_tab, navs, transport)
    }

    pub(crate) fn script_submit<F>(&mut self, id: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let node = self.document().and_then(|w| w.node_for(id)).ok_or_else(|| SimError::not_found("form"))?;
        // The submitter: `id` itself when it is a submit button, else the form's
        // default button; a form with neither is submitted through `requestSubmit`.
        let plan = self.document().and_then(WebDocument::scripted).and_then(|s| {
            s.read(|realm| {
                let inner = realm.layout();
                let d = &inner.doc;
                let is_submit = |n: NodeId| match d.tag(n) {
                    Some("button") => d.attr(n, "type").is_none_or(|t| t.trim().eq_ignore_ascii_case("submit")),
                    Some("input") => matches!(crate::web_document::input_type(d, n).as_str(), "submit" | "image"),
                    _ => false,
                };
                let form = if d.is(node, "form") { Some(node) } else { inner.form_owner(node) }?;
                if node != form && is_submit(node) {
                    return Some((form, Some(node)));
                }
                let button = d.descendants(Document::ROOT).find(|n| is_submit(*n) && !inner.is_disabled(*n) && inner.form_owner(*n) == Some(form));
                Some((form, button))
            })
        });
        let Some((form, button)) = plan else { return Err(SimError::not_found("form")) };
        let form_index = self.document().map_or(0, |w| w.with_document(|d| d.descendants(Document::ROOT).filter(|n| d.is(*n, "form")).position(|n| n == form).unwrap_or(0)));
        let (action, navs) = self.with_script(Some(transport), |realm| match button {
            Some(b) => realm.dispatch(UiEvent::ClickNode { node: b, modifiers: Modifiers::default(), detail: 1 }),
            None => {
                let _ = realm.eval(&format!("document.forms[{form_index}].requestSubmit()"));
                DefaultAction::None
            }
        })?;
        self.after_action(action, false, navs, transport)
    }

    /// `hover_at` with the network at hand.
    pub fn hover_at_with<F>(&mut self, x: i32, y: i32, width: u32, height: u32, transport: &mut F) -> Option<&'static str>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_hover_at(x, y, width, height, Some(transport));
        }
        self.hover_at(x, y, width, height)
    }

    pub(crate) fn script_hover_at<F>(&mut self, x: i32, y: i32, width: u32, height: u32, mut transport: Option<&mut F>) -> Option<&'static str>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        self.set_viewport(width, height);
        let (cx, cy) = WebDocument::css_point(x, y, self.zoom());
        let t: Option<Transport<'_, '_>> = match transport.as_deref_mut() {
            Some(t) => Some(t),
            None => None,
        };
        let (action, navs) = self.with_script(t, |realm| realm.dispatch(UiEvent::PointerMove { x: cx, y: cy, modifiers: Modifiers::default() })).ok()?;
        let _ = self.finish(action, false, navs, transport);
        self.cursor_at(x, y, width, height)
    }

    /// `scroll_pane` with the network at hand (infinite lists fetch on `scroll`).
    pub fn scroll_pane_with<F>(&mut self, pane: &str, offset: i32, horizontal: bool, transport: &mut F) -> bool
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_scroll(pane, offset, horizontal, Some(transport));
        }
        self.scroll_pane(pane, offset, horizontal)
    }

    pub(crate) fn script_scroll<F>(&mut self, pane: &str, offset: i32, horizontal: bool, mut transport: Option<&mut F>) -> bool
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let offset = offset.max(0);
        let id = pane.strip_prefix("row:").unwrap_or(pane);
        let node = if id == "page" { None } else { self.document().and_then(|w| w.node_for(id)) };
        if id != "page" && node.is_none() {
            return false;
        }
        let key = node.unwrap_or(Document::ROOT);
        let read = |b: &BrowserState| {
            b.document().and_then(WebDocument::scripted).map_or((0, 0), |s| {
                s.read(|realm| {
                    let inner = realm.layout();
                    inner.scroll.get(&key).map_or((0, 0), |(x, y)| (x.to_px_floor(), y.to_px_floor()))
                })
            })
        };
        let before = read(self);
        let (x, y) = if horizontal { (offset, before.1) } else { (before.0, offset) };
        let t: Option<Transport<'_, '_>> = match transport.as_deref_mut() {
            Some(t) => Some(t),
            None => None,
        };
        let Ok((action, navs)) = self.with_script(t, |realm| realm.dispatch(UiEvent::Scroll { node, x, y })) else { return false };
        let _ = self.finish(action, false, navs, transport);
        read(self) != before
    }

    // ---------------------------------------------------------------------------
    // Time
    // ---------------------------------------------------------------------------

    /// Whether a scripted page wants to run at `now`: the visible tab has a timer due
    /// or a `requestAnimationFrame` callback waiting; a background tab has a timer
    /// due and its once-per-second turn has come.
    pub(crate) fn script_pending(&self, now: u64) -> bool {
        self.tabs.iter().enumerate().any(|(index, _)| self.script_due(index, now))
    }

    fn script_due(&self, index: usize, now: u64) -> bool {
        let Some(m) = self.tabs.get(index).and_then(|t| t.history.get(t.position)).and_then(|e| e.web()).and_then(WebDocument::scripted).map(|s| s.mirror()) else { return false };
        let timer = m.next_timer.is_some_and(|t| t <= now.min(i64::MAX as u64) as i64);
        if index == self.active {
            timer || m.wants_frame
        } else {
            timer && now >= m.last_run.saturating_add(BACKGROUND_INTERVAL)
        }
    }

    /// Advances page script to `now` (world microseconds): due timers fire in order at
    /// their own times, the visible tab gets its animation frames, background tabs
    /// their throttled turn. Bounded by the step budget per tab.
    pub fn tick<F>(&mut self, now: u64, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        self.set_clock(now);
        let now = self.clock;
        for index in 0..self.tabs.len() {
            if !self.script_due(index, now) {
                continue;
            }
            let Some(m) = self.tabs[index].history.get(self.tabs[index].position).and_then(|e| e.web()).and_then(WebDocument::scripted).map(|s| s.mirror()) else { continue };
            let visible = index == self.active;
            let elapsed_ms = now.saturating_sub(m.last_run) / 1_000;
            let window = if !visible {
                0
            } else if m.wants_frame {
                elapsed_ms.min(FRAME_WINDOW_MS)
            } else {
                elapsed_ms.min(TICK_WINDOW_MS)
            };
            // The realm starts the window at `now - window` and walks its timers up
            // to `now`, so an interval that came due three times fires three times.
            self.clock = now - window * 1_000;
            let result = self.with_script_at(index, Some(transport), |realm| {
                realm.run_until_idle(window as u32);
                if visible && window < 16 && realm.wants_animation_frame() {
                    realm.animation_frame();
                }
            });
            self.clock = now;
            if let Some(s) = self.tabs[index].history.get(self.tabs[index].position).and_then(|e| e.web()).and_then(WebDocument::scripted) {
                s.update_mirror(|m| m.last_run = now);
            }
            if let Some((_, navs)) = result {
                if index == self.active {
                    self.perform_navs(navs, transport)?;
                }
            }
        }
        Ok(())
    }

    /// The realm state of the document on show, for measuring what a snapshot holds.
    pub fn script_state(&self) -> Option<Arc<cw_web::script::RealmState>> {
        self.document().and_then(WebDocument::scripted).map(|s| s.state())
    }
}
