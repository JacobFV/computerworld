//! Desktop applications written as web apps: a window whose frame is the platform's
//! own (the desktop scene paints it, per OS skin) and whose content is a cw-web
//! document laid out and painted by the web engine and driven by a script layer.
//!
//! **Script.** A `runtime::AppRuntime` runs the application: React on the JS VM
//! (`JsRuntime`) today, a compiled TSX runtime when one is available. The host speaks
//! to either only in `UiEvent`s in, `Reply`s to requests in, and an `Outbox` out.
//!
//! **Input.** A click on a control is a click on the element with that id (the scene
//! names every interactive element by its id, as a browser page's does); typed text
//! goes to the focused text control; keys go to the focused element. Scrolling is the
//! platform's, as for every native application: a scroll container with an id is a
//! pane of the window (`pane:<id>`), its offset kept in the window's `Scroll` and
//! applied when the document is painted, with the platform's own scroll bar.
//!
//! **Effects.** What the application asks of the machine through the `cw` global
//! becomes an `AppEffect` of the window, mediated by the environment exactly like a
//! native application's, and the answer comes back as a reply that settles the
//! promise it asked with. Nothing reaches the host's own files or network.
//!
//! **Snapshots.** An application is its declared state (`cw.state.set`). A handle
//! shares the live runtime between clones (a VM heap can be neither cloned nor
//! serialised) the way a browser tab shares its realm: each handle records the epoch
//! it last saw, and a handle the live runtime has moved on from boots the same code
//! with its own state the first time it is used. A restore boot is silent: requests it
//! makes are dropped, so a restored window and the live one it was taken from behave
//! alike. A runtime on the JS VM is also rebooted from state once its journal holds
//! `COMPACT_AFTER` entries (and no request is outstanding), so a long session costs
//! no more than a short one.

mod catalog;
mod paint;
mod project;
pub mod runtime;

use std::sync::{Arc, Mutex, MutexGuard};

use cw_sdk::WebSource;
use cw_web::dom::{Document, NodeId};
use cw_web::paint::semantics;
use cw_web::script::{Modifiers, UiEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::desktop_scene::{DesktopTheme, Painter};
use crate::AppEffect;
pub use catalog::{define, get as definition};
use runtime::{AppRuntime, Boot, Chrome, Env, JsRuntime, Outbox, Reply, Request, UiRuntime};

/// Journal entries (`Realm::journal_len`: inputs and host answers) a runtime on the
/// JS VM holds before it is rebooted from declared state. The journal stays on,
/// since it is what lets a window saved with a request outstanding restore exactly,
/// and it is small (about 108 bytes an entry, five entries a keystroke into Notes);
/// what the reboot really bounds is the VM heap, which grows about 41 KB a keystroke
/// with journaling on or off. 2,000 entries is about 400 keystrokes, about 16 MB,
/// for one reboot of about 10 ms.
pub const COMPACT_AFTER: usize = 2000;
/// Console lines a window keeps.
const CONSOLE_LIMIT: usize = 200;
/// Rounds of immediately answered requests one entry may chain.
const IMMEDIATE_ROUNDS: usize = 16;

/// A request waiting for the machine's answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Waiting {
    List(String),
    Write(String),
    /// `ReadFiles` and `Http` answer under the tag they were sent with.
    Tagged(String),
}

/// The live runtime and what belongs to it. `Realm` is full of `Rc`s.
///
/// SAFETY: every `Rc` a runtime holds is created inside it and dropped with it; no
/// reference into it leaves the mutex guarding the cell (`WebApp` hands the runtime
/// only to closures that cannot keep it), so its reference counts are only touched by
/// the thread holding that mutex.
struct Cell {
    runtime: Option<Box<dyn AppRuntime>>,
    epoch: u64,
    env: Env,
    waiting: Vec<(u64, Waiting)>,
    /// Effects asked for while answering something that returns none (a transport
    /// failure), handed out with the next entry's.
    deferred: Vec<AppEffect>,
    console: Vec<String>,
}
unsafe impl Send for Cell {}

/// A window saved while requests it made are outstanding: the runtime's complete
/// state (the promises and the code awaiting them), which requests are waiting for
/// which answers, and the effects not yet handed out. Declared state cannot carry a
/// pending promise, so this is what such a window restores from, and the machine's
/// answer then reaches the code that asked for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inflight {
    runtime: Value,
    waiting: Vec<(u64, Waiting)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deferred: Vec<AppEffect>,
}

struct Local {
    cell: Arc<Mutex<Cell>>,
    epoch: u64,
    /// This handle's work in flight as of its epoch, when the shared runtime has
    /// moved on (or was never booted here) and it had requests outstanding.
    captured: Option<Arc<Inflight>>,
    /// The window this application runs in, and the latest world clock it was told.
    window: u64,
    clock: u64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// A web application's window content. See the module documentation.
pub struct WebApp {
    kind: &'static str,
    version: u32,
    /// What it was launched on; only a first boot reads it.
    argument: String,
    /// The state the application declared: what a snapshot keeps.
    state: Value,
    chrome: Chrome,
    local: Mutex<Local>,
}

/// The environment a theme and a content size give an application.
pub fn env_for(theme: DesktopTheme, width: u32, height: u32) -> Env {
    Env {
        platform: theme.platform().to_owned(),
        mobile: theme.mobile(),
        width: width.max(1),
        height: height.max(1),
        css: paint::theme_css(theme),
    }
}

fn boot(
    kind: &str,
    argument: &str,
    state: Option<&Value>,
    env: &Env,
    clock: u64,
) -> Result<Box<dyn AppRuntime>, String> {
    let entry = catalog::get(kind).ok_or_else(|| format!("no web application {kind}"))?;
    let boot = Boot {
        kind,
        argument,
        state,
        env,
    };
    match &entry.app.source {
        WebSource::Script {
            script,
            style,
            react,
        } => Ok(Box::new(JsRuntime::boot(
            style, script, *react, &boot, clock,
        ))),
        WebSource::Compiled { ir, script, style } => {
            match UiRuntime::boot(ir, style, &boot, state, clock) {
                Ok(runtime) => Ok(Box::new(runtime)),
                // A first launch the IR cannot mount runs the same source on React;
                // cw-ui's own snapshot can only be restored by cw-ui.
                Err(_) if state.is_none_or(Value::is_null) => {
                    Ok(Box::new(JsRuntime::boot(style, script, true, &boot, clock)))
                }
                Err(reason) => Err(format!("{kind} cannot be restored: {reason}")),
            }
        }
    }
}

/// Rebuilds a runtime from the state `AppRuntime::suspend` saved.
fn resume(kind: &str, saved: &Value, env: &Env, clock: u64) -> Result<Box<dyn AppRuntime>, String> {
    let entry = catalog::get(kind).ok_or_else(|| format!("no web application {kind}"))?;
    let boot = Boot {
        kind,
        argument: "",
        state: None,
        env,
    };
    match &entry.app.source {
        WebSource::Script {
            script,
            style,
            react,
        } => Ok(Box::new(JsRuntime::resume(
            style, script, *react, saved, &boot, clock,
        )?)),
        // A compiled app's saved state is cw-ui's; one that fell back to React at
        // launch saved the Realm's.
        WebSource::Compiled { script, style, .. } => match UiRuntime::resume(saved, &boot, clock) {
            Ok(runtime) => Ok(Box::new(runtime)),
            Err(_) => Ok(Box::new(JsRuntime::resume(
                style, script, true, saved, &boot, clock,
            )?)),
        },
    }
}

/// Text controls take typed text: a textarea, a text-like input, an editable element.
pub(crate) fn is_text_control(doc: &Document, node: NodeId) -> bool {
    match doc.tag(node) {
        Some("textarea") => true,
        Some("input") => !matches!(
            doc.attr(node, "type")
                .unwrap_or("text")
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "checkbox"
                | "radio"
                | "submit"
                | "button"
                | "reset"
                | "hidden"
                | "range"
                | "file"
                | "color"
                | "image"
        ),
        _ => doc
            .attr(node, "contenteditable")
            .is_some_and(|v| v != "false"),
    }
}

/// The element a control id names: its `id`, else the path the scene named it by.
pub(crate) fn node_for(doc: &Document, target: &str) -> Option<NodeId> {
    // The id index can still hold elements a render removed from the document.
    let connected = |n: &NodeId| doc.ancestors(*n).any(|a| a == Document::ROOT);
    if let Some(node) = doc.by_id(target).iter().find(|n| connected(n)) {
        return Some(*node);
    }
    if !target.starts_with('/') {
        return None;
    }
    doc.descendants(Document::ROOT)
        .find(|n| doc.is_element(*n) && semantics::path_id(doc, *n) == target)
}

/// The element pane `name` of the window is: the one marked `data-cw-pane="<name>"`,
/// else the one whose id it is.
pub(crate) fn pane_node(doc: &Document, name: &str) -> Option<NodeId> {
    doc.descendants(Document::ROOT)
        .find(|n| doc.attr(*n, "data-cw-pane") == Some(name))
        .or_else(|| node_for(doc, name))
}

/// A key as the desktop names it (`Enter`, `Ctrl+s`, `Shift+Tab`, `a`) as a DOM key
/// value and its modifiers.
fn parse_key(key: &str) -> (String, Modifiers) {
    let mut modifiers = Modifiers::default();
    let mut rest = key;
    while let Some((head, tail)) = rest.split_once('+') {
        if tail.is_empty() {
            break;
        }
        match head.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers.ctrl = true,
            "shift" => modifiers.shift = true,
            "alt" | "option" => modifiers.alt = true,
            "meta" | "cmd" | "command" | "super" => modifiers.meta = true,
            _ => break,
        }
        rest = tail;
    }
    let key = match rest {
        "Esc" => "Escape",
        "Space" | "space" => " ",
        "Return" => "Enter",
        "Del" => "Delete",
        other => other,
    };
    (key.to_owned(), modifiers)
}

impl WebApp {
    /// Launch `kind` on `argument` in window `window`: boot it and hand out what its
    /// first render asked for.
    pub fn launch(
        kind: &str,
        argument: &str,
        window: u64,
        clock_us: u64,
        theme: DesktopTheme,
    ) -> Result<(Self, Vec<AppEffect>), String> {
        let entry = catalog::get(kind).ok_or_else(|| format!("no web application {kind}"))?;
        let (width, height) = theme.native_screen();
        let env = env_for(theme, width, height);
        let runtime = boot(kind, argument, None, &env, clock_us)?;
        let cell = Cell {
            runtime: Some(runtime),
            epoch: 1,
            env,
            waiting: vec![],
            deferred: vec![],
            console: vec![],
        };
        let mut app = Self {
            kind: entry.kind,
            version: entry.app.version,
            argument: argument.to_owned(),
            state: Value::Null,
            chrome: Chrome::default(),
            local: Mutex::new(Local {
                cell: Arc::new(Mutex::new(cell)),
                epoch: 1,
                captured: None,
                window,
                clock: clock_us,
            }),
        };
        let effects = app.enter(Some(window), Some(clock_us), |_, _| Ok(()))?.1;
        Ok((app, effects))
    }

    /// A handle on `kind` in `state`, as a snapshot holds it; the application boots
    /// the first time it is used.
    pub fn restored(
        kind: &str,
        version: u32,
        argument: String,
        state: Value,
        chrome: Chrome,
        inflight: Option<Inflight>,
    ) -> Result<Self, String> {
        let entry = catalog::get(kind).ok_or_else(|| format!("no web application {kind}"))?;
        if entry.app.version != version {
            return Err(format!(
                "web application {kind} is version {}, the snapshot needs {version}",
                entry.app.version
            ));
        }
        let cell = Cell {
            runtime: None,
            epoch: 0,
            env: env_for(DesktopTheme::Macos, 800, 600),
            waiting: vec![],
            deferred: vec![],
            console: vec![],
        };
        Ok(Self {
            kind: entry.kind,
            version,
            argument,
            state,
            chrome,
            local: Mutex::new(Local {
                cell: Arc::new(Mutex::new(cell)),
                epoch: 0,
                captured: inflight.map(Arc::new),
                window: 0,
                clock: 0,
            }),
        })
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn argument(&self) -> &str {
        &self.argument
    }
    /// The state the application declared.
    pub fn state(&self) -> &Value {
        &self.state
    }
    pub fn chrome_facts(&self) -> &Chrome {
        &self.chrome
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        let Some(entry) = catalog::get(self.kind) else {
            return self.kind.to_owned();
        };
        let titles = &entry.app.titles;
        titles
            .get(theme.platform())
            .or_else(|| titles.get("*"))
            .cloned()
            .unwrap_or_else(|| self.kind.to_owned())
    }
    pub fn document(&self) -> String {
        self.chrome.document.clone()
    }
    pub fn caption(&self) -> String {
        self.chrome.caption.clone()
    }
    pub fn modified(&self) -> bool {
        self.chrome.modified
    }
    /// The application's console: `console.*` lines and uncaught errors, newest last.
    pub fn console(&self) -> Vec<String> {
        let local = lock(&self.local);
        let cell = lock(&local.cell);
        cell.console.clone()
    }

    /// Makes sure `local`'s cell holds the runtime of `local`'s epoch, booting one
    /// from declared state when the shared runtime moved on (or never ran here).
    fn ready(kind: &str, argument: &str, state: &Value, local: &mut Local) -> Result<(), String> {
        let (in_sync, env) = {
            let cell = lock(&local.cell);
            (
                cell.epoch == local.epoch && cell.runtime.is_some(),
                cell.env.clone(),
            )
        };
        if in_sync {
            return Ok(());
        }
        let (mut runtime, waiting, deferred) = match local.captured.as_deref() {
            // Work in flight resumes exactly where it was.
            Some(inflight) => (
                resume(kind, &inflight.runtime, &env, local.clock)?,
                inflight.waiting.clone(),
                inflight.deferred.clone(),
            ),
            None => (
                boot(kind, argument, Some(state), &env, local.clock)?,
                vec![],
                vec![],
            ),
        };
        // A restore boot is silent (see the module documentation).
        let out = runtime.drain();
        let fresh = Cell {
            runtime: Some(runtime),
            epoch: local.epoch,
            env,
            waiting,
            deferred,
            console: out.logs,
        };
        if Arc::strong_count(&local.cell) == 1 {
            *lock(&local.cell) = fresh;
        } else {
            local.cell = Arc::new(Mutex::new(fresh));
        }
        Ok(())
    }

    /// Enters the runtime to change it: runs `f`, turns what the application asked
    /// for into effects, and records the state it declared. A refusal is the error.
    fn enter<T>(
        &mut self,
        window: Option<u64>,
        clock: Option<u64>,
        f: impl FnOnce(&mut dyn AppRuntime, u64) -> Result<T, String>,
    ) -> Result<(T, Vec<AppEffect>), String> {
        let local = match self.local.get_mut() {
            Ok(l) => l,
            Err(p) => p.into_inner(),
        };
        if let Some(window) = window {
            local.window = window;
        }
        if let Some(clock) = clock {
            local.clock = local.clock.max(clock);
        }
        Self::ready(self.kind, &self.argument, &self.state, local)?;
        // The live runtime is this handle's state from here on.
        local.captured = None;
        let (window, now) = (local.window, local.clock);
        let cell_arc = local.cell.clone();
        let mut cell = lock(&cell_arc);
        let cell = &mut *cell;
        let runtime = cell.runtime.as_mut().expect("ready");
        let out = f(runtime.as_mut(), now);
        let mut effects = std::mem::take(&mut cell.deferred);
        let mut refusal = None;
        for _ in 0..IMMEDIATE_ROUNDS {
            let outbox = cell.runtime.as_mut().expect("ready").drain();
            let immediate = Self::absorb(
                outbox,
                window,
                cell,
                &mut self.state,
                &mut self.chrome,
                &mut effects,
                &mut refusal,
            );
            if immediate.is_empty() {
                break;
            }
            cell.runtime
                .as_mut()
                .expect("ready")
                .deliver(&immediate, now);
        }
        // A backend that snapshots itself is its own state.
        if let Some(state) = cell.runtime.as_mut().and_then(|r| r.snapshot()) {
            self.state = state;
        }
        cell.epoch += 1;
        local.epoch = cell.epoch;
        // A long session is folded back into its state (see the module docs).
        if cell.waiting.is_empty()
            && cell.deferred.is_empty()
            && cell
                .runtime
                .as_ref()
                .is_some_and(|r| r.weight() > COMPACT_AFTER)
        {
            cell.runtime = None;
        }
        let value = out?;
        match refusal {
            Some(reason) => Err(reason),
            None => Ok((value, effects)),
        }
    }

    /// Takes in one outbox: declared state, window facts, console lines, and requests,
    /// which become effects or are answered at once. Returns the immediate answers.
    fn absorb(
        outbox: Outbox,
        window: u64,
        cell: &mut Cell,
        state: &mut Value,
        chrome: &mut Chrome,
        effects: &mut Vec<AppEffect>,
        refusal: &mut Option<String>,
    ) -> Vec<Reply> {
        if let Some(value) = outbox.state {
            *state = value;
        }
        if let Some(facts) = outbox.chrome {
            *chrome = facts;
        }
        if outbox.refusal.is_some() {
            *refusal = outbox.refusal;
        }
        cell.console.extend(outbox.logs);
        if cell.console.len() > CONSOLE_LIMIT {
            let excess = cell.console.len() - CONSOLE_LIMIT;
            cell.console.drain(..excess);
        }
        let mut immediate = vec![];
        for (id, request) in outbox.requests {
            let tag = format!("web:{id}");
            match request {
                Request::Read { path } => {
                    cell.waiting.push((id, Waiting::Tagged(tag.clone())));
                    effects.push(AppEffect::ReadFiles {
                        window,
                        tag,
                        paths: vec![path],
                    });
                }
                Request::Write { path, content } => {
                    cell.waiting.push((id, Waiting::Write(path.clone())));
                    effects.push(AppEffect::WriteFile {
                        window,
                        path,
                        content,
                    });
                }
                Request::List { path } => {
                    cell.waiting.push((id, Waiting::List(path.clone())));
                    effects.push(AppEffect::ListTree {
                        window,
                        path,
                        depth: 1,
                    });
                }
                Request::Mkdir { path } => {
                    effects.push(AppEffect::CreateDirectory { window, path });
                    immediate.push(Reply::ok(id, Value::Null));
                }
                Request::Http { url, method, body } => {
                    let effect = AppEffect::Http {
                        window,
                        tag: tag.clone(),
                        method,
                        url,
                        body,
                    };
                    match effect.validate() {
                        Ok(()) => {
                            cell.waiting.push((id, Waiting::Tagged(tag)));
                            effects.push(effect);
                        }
                        Err(reason) => immediate.push(Reply::err(id, reason)),
                    }
                }
                Request::Launch { app, argument } => {
                    effects.push(AppEffect::Launch {
                        window,
                        kind: app,
                        argument,
                    });
                    immediate.push(Reply::ok(id, Value::Null));
                }
                Request::Emit { name, data } => {
                    effects.push(AppEffect::Emit { window, name, data });
                    immediate.push(Reply::ok(id, Value::Null));
                }
            }
        }
        immediate
    }

    /// Answers the first request waiting on something `matches` accepts.
    fn answer(
        &mut self,
        window: u64,
        matches: impl Fn(&Waiting) -> bool,
        reply: impl FnOnce(u64) -> Reply,
    ) -> Result<Vec<AppEffect>, String> {
        let id = {
            let local = match self.local.get_mut() {
                Ok(l) => l,
                Err(p) => p.into_inner(),
            };
            // A restored window's requests wait in the runtime it resumes.
            Self::ready(self.kind, &self.argument, &self.state, local)?;
            let mut cell = lock(&local.cell);
            if cell.epoch != local.epoch {
                return Err("the application is not waiting for that".into());
            }
            let index = cell
                .waiting
                .iter()
                .position(|(_, w)| matches(w))
                .ok_or("the application is not waiting for that")?;
            cell.waiting.remove(index).0
        };
        let reply = reply(id);
        self.enter(Some(window), None, |runtime, now| {
            runtime.deliver(&[reply], now);
            Ok(())
        })
        .map(|((), effects)| effects)
    }

    /// A folder listing asked for with `cw.fs.list`.
    pub fn tree_listed(
        &mut self,
        window: u64,
        path: &str,
        result: Result<Vec<String>, String>,
    ) -> Result<Vec<AppEffect>, String> {
        self.answer(
            window,
            |w| matches!(w, Waiting::List(p) if p == path),
            |id| match result {
                Ok(names) => Reply::ok(id, Value::from(names)),
                Err(reason) => Reply::err(id, reason),
            },
        )
    }
    /// A file read asked for with `cw.fs.readFile`.
    pub fn files_read(
        &mut self,
        window: u64,
        tag: &str,
        files: Vec<(String, Result<String, String>)>,
    ) -> Result<Vec<AppEffect>, String> {
        let result = files
            .into_iter()
            .next()
            .map(|(_, r)| r)
            .unwrap_or_else(|| Err("nothing was read".into()));
        self.answer(
            window,
            |w| matches!(w, Waiting::Tagged(t) if t == tag),
            |id| match result {
                Ok(text) => Reply::ok(id, Value::from(text)),
                Err(reason) => Reply::err(id, reason),
            },
        )
    }
    /// A write asked for with `cw.fs.writeFile` reached the disk.
    pub fn written(&mut self, window: u64, path: &str) -> Result<Vec<AppEffect>, String> {
        self.answer(
            window,
            |w| matches!(w, Waiting::Write(p) if p == path),
            |id| Reply::ok(id, Value::Null),
        )
    }
    /// A service answered a `cw.fetch`.
    pub fn http(
        &mut self,
        window: u64,
        tag: &str,
        status: u16,
        body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let body = body.to_owned();
        self.answer(
            window,
            |w| matches!(w, Waiting::Tagged(t) if t == tag),
            |id| Reply::ok(id, serde_json::json!({ "status": status, "body": body })),
        )
    }
    /// A request never reached its service. Effects the application asks for in
    /// answer go out with its next entry.
    pub fn offline(&mut self, tag: &str, reason: &str) {
        let window = lock(&self.local).window;
        if let Ok(effects) = self.answer(
            window,
            |w| matches!(w, Waiting::Tagged(t) if t == tag),
            |id| Reply::err(id, reason),
        ) {
            let local = match self.local.get_mut() {
                Ok(l) => l,
                Err(p) => p.into_inner(),
            };
            lock(&local.cell).deferred.extend(effects);
        }
    }

    /// Reads the document without changing what the application can observe; the
    /// runtime is booted first if this handle's is not live.
    fn read<T>(&self, f: impl FnOnce(&mut Cell) -> T) -> Result<T, String> {
        let mut local = lock(&self.local);
        Self::ready(self.kind, &self.argument, &self.state, &mut local)?;
        let cell_arc = local.cell.clone();
        drop(local);
        let mut cell = lock(&cell_arc);
        Ok(f(&mut cell))
    }

    /// Tells a live runtime its environment when it changed. Requests and state the
    /// application makes in answer are dropped: an environment is not an input.
    fn sync_env(cell: &mut Cell, env: &Env, now: u64) {
        if cell.env == *env {
            return;
        }
        cell.env = env.clone();
        if let Some(runtime) = cell.runtime.as_mut() {
            runtime.set_env(env, now);
            let out = runtime.drain();
            cell.console.extend(out.logs);
        }
    }

    /// The element id of the focused text control, if one has the focus.
    pub fn text_field(&self) -> Option<String> {
        self.read(|cell| {
            let mut field = None;
            if let Some(runtime) = cell.runtime.as_mut() {
                runtime.view(&mut |v| {
                    field = v
                        .focused
                        .filter(|n| is_text_control(v.doc, *n))
                        .map(|n| semantics::interaction_id(v.doc, n));
                });
            }
            field
        })
        .ok()
        .flatten()
    }
    /// The first element marked `data-cw-<attribute>`, by id: `back` is where a
    /// phone's Back goes, `refresh` what pulling a list down does.
    pub fn marked(&self, attribute: &str) -> Option<String> {
        let name = format!("data-cw-{attribute}");
        self.read(|cell| {
            let mut found = None;
            if let Some(runtime) = cell.runtime.as_mut() {
                runtime.view(&mut |v| {
                    found = v
                        .doc
                        .descendants(Document::ROOT)
                        .find(|n| v.doc.is_element(*n) && v.doc.has_attr(*n, &name))
                        .map(|n| semantics::interaction_id(v.doc, n));
                });
            }
            found
        })
        .ok()
        .flatten()
    }

    /// A click on the element `target` names.
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        self.click_detail(window, target, clock_us, 1)
    }
    /// What the desktop calls opening a control: on a phone a tap, which is one click;
    /// on a desktop a double click, two clicks (the second with `detail` 2, then
    /// `dblclick`), the second only if the first left the control in the document.
    pub fn activate(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let mut effects = self.click_detail(window, target, clock_us, 1)?;
        let (mobile, still) = self
            .read(|cell| {
                let mut still = false;
                if let Some(runtime) = cell.runtime.as_mut() {
                    runtime.view(&mut |v| still = node_for(v.doc, target).is_some());
                }
                (cell.env.mobile, still)
            })
            .unwrap_or((true, false));
        if !mobile && still {
            effects.extend(self.click_detail(window, target, clock_us, 2)?);
        }
        Ok(effects)
    }
    fn click_detail(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
        detail: u32,
    ) -> Result<Vec<AppEffect>, String> {
        let kind = self.kind;
        self.enter(Some(window), Some(clock_us), |runtime, now| {
            let mut node = None;
            let mut disabled = false;
            runtime.view(&mut |v| {
                node = node_for(v.doc, target);
                disabled = node.is_some_and(|n| semantics::is_disabled(v.doc, n));
            });
            let node = node.ok_or_else(|| format!("{kind} has no control {target}"))?;
            if disabled {
                return Err(format!("{target} is disabled"));
            }
            runtime.dispatch(
                UiEvent::ClickNode {
                    node,
                    modifiers: Modifiers::default(),
                    detail,
                },
                now,
            );
            Ok(())
        })
        .map(|((), effects)| effects)
    }
    /// Typed text, into the focused text control. With none focused the keys go to
    /// the document, whose own handlers may take them; text that changes nothing
    /// the application declares is refused, as a native application refuses it.
    pub fn text_effects(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
        let kind = self.kind;
        let before = self.state.clone();
        let mut field = false;
        let effects = self
            .enter(Some(window), None, |runtime, now| {
                runtime.view(&mut |v| {
                    field = v.focused.is_some_and(|n| is_text_control(v.doc, n));
                });
                runtime.dispatch(
                    UiEvent::TypeText {
                        text: text.to_owned(),
                    },
                    now,
                );
                Ok(())
            })
            .map(|((), effects)| effects)?;
        if !field && effects.is_empty() && self.state == before {
            return Err(format!("nothing in {kind} is taking text"));
        }
        Ok(effects)
    }
    /// Typed text where the caller has no window to hand (effects go out with the
    /// next entry).
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        let window = lock(&self.local).window;
        let effects = self.text_effects(window, text)?;
        let local = match self.local.get_mut() {
            Ok(l) => l,
            Err(p) => p.into_inner(),
        };
        lock(&local.cell).deferred.extend(effects);
        Ok(())
    }
    /// A key, to the focused element (the document's body when nothing has focus).
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        let (key, modifiers) = parse_key(key);
        self.enter(Some(window), Some(clock_us), |runtime, now| {
            runtime.dispatch(
                UiEvent::Key {
                    key,
                    code: String::new(),
                    modifiers,
                    repeat: false,
                },
                now,
            );
            Ok(())
        })
        .map(|((), effects)| effects)
    }

    /// The semantic page: every element with an id, in document order.
    pub fn page(&self, page: &mut cw_protocol::Page) {
        let _ = self.read(|cell| {
            if let Some(runtime) = cell.runtime.as_mut() {
                runtime.view(&mut |v| project::project(v, page));
            }
        });
    }

    /// Paints the document into the window's content.
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let wanted = env_for(env.theme, env.width, env.height);
        let clock = env.clock_us;
        let result = self.read(|cell| {
            Self::sync_env(cell, &wanted, clock);
            if let Some(runtime) = cell.runtime.as_mut() {
                runtime.view(&mut |v| paint::paint(v, p, env));
            }
        });
        if let Err(reason) = result {
            let l = crate::apps::look::look(env.theme);
            p.scene.background = l.surface;
            crate::apps::look::notice(p, env.width, 40, &reason);
        }
    }
}

impl WebApp {
    /// This handle's work in flight, if requests it made are outstanding: taken from
    /// the live runtime when this handle is the one in step with it.
    pub fn inflight(&self) -> Option<Inflight> {
        let local = lock(&self.local);
        let mut cell = lock(&local.cell);
        let cell = &mut *cell;
        match cell.runtime.as_mut() {
            Some(runtime) if cell.epoch == local.epoch => {
                if cell.waiting.is_empty() && cell.deferred.is_empty() {
                    return None;
                }
                Some(Inflight {
                    runtime: runtime.suspend().ok()?,
                    waiting: cell.waiting.clone(),
                    deferred: cell.deferred.clone(),
                })
            }
            _ => local.captured.as_deref().cloned(),
        }
    }
}

impl Clone for WebApp {
    fn clone(&self) -> Self {
        // A copy of a window with requests outstanding keeps their state, and so does
        // this handle, whichever of the two the live runtime then follows.
        let captured = self.inflight().map(Arc::new);
        let mut local = lock(&self.local);
        if captured.is_some() {
            local.captured = captured.clone();
        }
        Self {
            kind: self.kind,
            version: self.version,
            argument: self.argument.clone(),
            state: self.state.clone(),
            chrome: self.chrome.clone(),
            local: Mutex::new(Local {
                cell: local.cell.clone(),
                epoch: local.epoch,
                captured,
                window: local.window,
                clock: local.clock,
            }),
        }
    }
}
impl PartialEq for WebApp {
    fn eq(&self, o: &Self) -> bool {
        self.kind == o.kind
            && self.version == o.version
            && self.argument == o.argument
            && self.state == o.state
            && self.chrome == o.chrome
    }
}
impl Eq for WebApp {}
impl std::fmt::Debug for WebApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebApp")
            .field("kind", &self.kind)
            .field("state", &self.state)
            .finish()
    }
}

#[derive(Serialize)]
struct WebAppRef<'a> {
    kind: &'a str,
    version: u32,
    #[serde(skip_serializing_if = "str::is_empty")]
    argument: &'a str,
    state: &'a Value,
    #[serde(skip_serializing_if = "is_default_chrome")]
    chrome: &'a Chrome,
    #[serde(skip_serializing_if = "Option::is_none")]
    inflight: Option<Inflight>,
}
fn is_default_chrome(c: &&Chrome) -> bool {
    **c == Chrome::default()
}
#[derive(Deserialize)]
struct WebAppOwned {
    kind: String,
    version: u32,
    #[serde(default)]
    argument: String,
    state: Value,
    #[serde(default)]
    chrome: Chrome,
    #[serde(default)]
    inflight: Option<Inflight>,
}
impl Serialize for WebApp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        WebAppRef {
            kind: self.kind,
            version: self.version,
            argument: &self.argument,
            state: &self.state,
            chrome: &self.chrome,
            inflight: self.inflight(),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for WebApp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = WebAppOwned::deserialize(d)?;
        WebApp::restored(
            &o.kind, o.version, o.argument, o.state, o.chrome, o.inflight,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
