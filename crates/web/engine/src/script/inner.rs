//! The realm's state outside the VM heap: the document, its stylesheets (document
//! and constructed), computed styles and layout with their dirty tracking, the
//! interaction state selector matching needs (hover, focus, active, `:target`), form
//! control state, wrapper caches, canvases and the journaled host.

use std::collections::{BTreeMap, BTreeSet};

use cw_jsvm::value::Obj;
use serde::{Deserialize, Serialize};

use super::canvas::CanvasState;
use super::journal::{Journal, JournalEntry};
use super::{FetchRequest, FetchResponse, ScriptHostDocument, StorageArea};
use crate::css::{self, FormState, MatchContext, Media, Origin, Stylesheet};
use crate::dom::{Document, Mutation, NodeId, NodeKind};
use crate::geom::Au;
use crate::layout::{self, FragmentTree, ImageSizeMap, LayoutCache, LayoutOptions, ScrollState};
use crate::style::{self, StyleSet};
use crate::{Strictness, Viewport};

pub(crate) mod image;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Log,
    Info,
    Warn,
    Error,
    Debug,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub level: LogLevel,
    pub text: String,
}

/// Where a stylesheet came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SheetOwner {
    /// A `<style>` element (its text) or a `<link rel=stylesheet>` (its href).
    Element(NodeId),
    /// `new CSSStyleSheet()`.
    Constructed,
    /// Expanded from an `@import` of another sheet.
    Import(u32),
}

#[derive(Clone, Debug)]
pub struct SheetEntry {
    pub id: u32,
    pub owner: SheetOwner,
    pub sheet: Stylesheet,
    pub disabled: bool,
    pub media: String,
    pub href: Option<String>,
    /// The source text the sheet was parsed from, to detect `<style>` text changes.
    pub source: String,
}

/// Form control state that lives outside the DOM attributes.
#[derive(Clone, Debug, Default)]
pub struct FormData {
    /// Dirty values of inputs and textareas.
    pub values: BTreeMap<NodeId, String>,
    /// Dirty checkedness of checkboxes and radios; selectedness of options.
    pub checked: BTreeMap<NodeId, bool>,
    pub indeterminate: BTreeSet<NodeId>,
    /// Selection `(start, end)` of text controls.
    pub selection: BTreeMap<NodeId, (usize, usize)>,
    pub custom_validity: BTreeMap<NodeId, String>,
    /// Type-to-select state of each closed select (Blink's `TypeAhead`).
    pub typeahead: BTreeMap<NodeId, TypeAhead>,
}

/// What a closed select has been typed so far: the search buffer, when the last
/// key came (virtual ms), and the character being cycled through, if any.
#[derive(Clone, Debug, Default)]
pub struct TypeAhead {
    pub buffer: String,
    pub last_ms: f64,
    pub repeating: Option<char>,
}

impl FormState for FormData {
    fn checked(&self, doc: &Document, element: NodeId) -> bool {
        if let Some(c) = self.checked.get(&element) {
            return *c;
        }
        if doc.is(element, "option") {
            if doc.has_attr(element, "selected") {
                return true;
            }
            // The first option of a single-select is selected by default.
            let Some(select) = doc.ancestors(element).find(|a| doc.is(*a, "select")) else {
                return false;
            };
            if doc.has_attr(select, "multiple")
                || doc
                    .attr(select, "size")
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(1)
                    > 1
            {
                return false;
            }
            let mut first = None;
            for o in doc.descendants(select) {
                if o != select && doc.is(o, "option") {
                    if first.is_none() {
                        first = Some(o);
                    }
                    if doc.has_attr(o, "selected") || self.checked.get(&o) == Some(&true) {
                        return false;
                    }
                    if self.checked.contains_key(&o) {
                        // Script deselected everything explicitly.
                        return false;
                    }
                }
            }
            first == Some(element)
        } else {
            doc.has_attr(element, "checked")
        }
    }
    fn indeterminate(&self, _doc: &Document, element: NodeId) -> bool {
        self.indeterminate.contains(&element)
    }
    fn value(&self, doc: &Document, element: NodeId) -> String {
        if let Some(v) = self.values.get(&element) {
            return v.clone();
        }
        if doc.is(element, "textarea") {
            doc.text_content(element)
        } else {
            doc.attr(element, "value").unwrap_or("").to_owned()
        }
    }
}

/// The selector-matching state (hover, focus, form state, ...) styles are computed
/// against.
fn match_context<'a>(
    doc: &Document,
    form: &'a FormData,
    (hovered, active, focused): (Option<NodeId>, Option<NodeId>, Option<NodeId>),
    focus_visible: bool,
    target_id: &Option<String>,
) -> MatchContext<'a> {
    let mut ctx = MatchContext::new();
    ctx.set_hovered(doc, hovered);
    ctx.set_active(doc, active);
    ctx.focused = focused;
    ctx.focus_visible = focus_visible;
    ctx.target_id = target_id.clone();
    ctx.document_lang = doc
        .document_element()
        .and_then(|h| doc.attr(h, "lang"))
        .unwrap_or("")
        .to_owned();
    ctx.form = Some(form);
    ctx
}

/// A history entry of the realm (`pushState`).
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub url: String,
    /// The serialised state (JSON) or `None`.
    pub state: Option<String>,
}

pub struct Inner {
    pub host: Box<dyn ScriptHostDocument>,
    pub journal: Journal,
    pub doc: Document,
    pub url: String,
    pub viewport: Viewport,
    pub sheets: Vec<SheetEntry>,
    pub constructed: BTreeMap<u32, SheetEntry>,
    pub adopted: Vec<u32>,
    next_sheet_id: u32,
    pub sheets_dirty: bool,
    pub styles: StyleSet,
    styles_valid: bool,
    /// The cascade engine built from the current sheets (see `ensure_styles`).
    style_engine: Option<style::StyleEngine>,
    /// Whether the document or a style layout reads changed since the last layout
    /// (see `ensure_layout`).
    layout_dirty: bool,
    /// The viewport, scroll offsets, image sizes and scrollbar mode the current
    /// tree was laid out with.
    laid_out: Option<(Viewport, ScrollState, ImageSizeMap, bool)>,
    /// Bumped when the fragment tree is replaced, and when a style flush changed
    /// a style hit testing reads: the key of the hit-test list.
    hit_epoch: u64,
    hit_list: Option<(u64, crate::paint::hit::HitList)>,
    /// Elements whose matching state (hover, focus, active) changed since the last
    /// restyle.
    state_changed: Vec<NodeId>,
    pub tree: Option<FragmentTree>,
    tree_generation: u64,
    styles_generation: u64,
    /// Bumped by every mutation, style change, scroll or viewport change.
    pub generation: u64,
    pub(crate) layout_cache: LayoutCache,
    pub images: ImageSizeMap,
    pub scroll: ScrollState,
    pub hovered: Option<NodeId>,
    /// Where the pointer last was (viewport CSS pixels), so `:hover` can follow
    /// content that moves under a pointer that stays still.
    pub pointer: Option<(i32, i32)>,
    /// The generation `hovered` was last hit-tested at: while nothing has changed
    /// since, the element under a still pointer cannot have changed either.
    pub hover_generation: u64,
    /// Whether any element has an `on*` content attribute, with the
    /// (generation, node count) it was computed at (`W.inlineHandlers`).
    pub inline_handlers: (u64, usize, bool),
    pub active: Option<NodeId>,
    pub focused: Option<NodeId>,
    pub focus_visible: bool,
    pub target_id: Option<String>,
    pub form: FormData,
    /// JS wrappers per node, created on first access.
    pub wrappers: Vec<Option<Obj>>,
    /// Prototypes per element local name (and `*element`, `*svg`, `text`, ...),
    /// registered by the prelude.
    pub protos: BTreeMap<String, Obj>,
    pub logs: Vec<LogEntry>,
    pub canvases: BTreeMap<NodeId, CanvasState>,
    pub module_sources: BTreeMap<String, String>,
    pub parsing: bool,
    pub write_buffer: String,
    pub ready_state: String,
    pub current_script: Option<NodeId>,
    pub deferred_scripts: Vec<NodeId>,
    pub async_scripts: Vec<NodeId>,
    /// Dynamically inserted `<script src>` elements waiting for their task.
    pub pending_scripts: Vec<NodeId>,
    pub executed_scripts: BTreeSet<NodeId>,
    pub has_raf: bool,
    /// Whether `ResizeObserver`/`IntersectionObserver` instances are observing.
    pub has_layout_observers: bool,
    /// The layout generation observers were last delivered against.
    pub observed_generation: u64,
    /// Whether any `MutationObserver` observes (so mutation natives notify).
    pub observing: bool,
    /// Custom element names the page defined (attribute changes notify).
    pub custom_defined: BTreeSet<String>,
    pub history: Vec<HistoryEntry>,
    pub history_index: usize,
    pub hidden: bool,
    pub alerts: Vec<String>,
    /// Inline `style` attribute declaration cache: attribute text and parsed block.
    pub inline_cache: BTreeMap<NodeId, (String, Vec<css::Declaration>)>,
    pub referrer: String,
    pub cookie_cache: Option<String>,
    pub dom_mutations_since_styles: usize,
    /// Per-element transition and animation bookkeeping: the computed style the last
    /// style flush left and the transitioned values read off it.
    anim_state: BTreeMap<NodeId, AnimState>,
    /// Transitions and animations the last style flush started; the realm turns each
    /// into DOM events on the world clock (see `Realm::pump_animations`).
    pub pending_animations: Vec<AnimationStart>,
}

/// What the previous style flush left on an element that can transition or animate.
struct AnimState {
    style: std::rc::Rc<style::ComputedStyle>,
    /// The serialized value of each transitioned property, in `transition-property`
    /// order, so the next flush can tell which of them changed.
    values: Vec<(String, String)>,
    /// The `animation-name`s that were running.
    names: Vec<String>,
}

/// A transition or animation a style change started.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationStart {
    pub node: NodeId,
    /// The transitioned property, or the `@keyframes` name for an animation.
    pub name: String,
    pub is_animation: bool,
    pub delay_ms: i32,
    pub duration_ms: i32,
    /// `None` is `infinite`; the end event never fires.
    pub iterations: Option<f64>,
    /// An animation whose name left `animation-name`: it is cancelled, not started.
    pub cancelled: bool,
}

/// What `transition-property: all` covers: the properties frameworks actually
/// transition. (The full animatable set would cost a serialization per property per
/// element on every style flush for no gain.)
const TRANSITION_ALL: &[&str] = &[
    "opacity",
    "color",
    "background-color",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "outline-color",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "top",
    "right",
    "bottom",
    "left",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
    "font-size",
    "font-weight",
    "letter-spacing",
    "line-height",
    "transform",
    "box-shadow",
    "visibility",
    "fill",
    "stroke",
    "flex-basis",
    "flex-grow",
    "flex-shrink",
    "gap",
    "z-index",
];

impl Inner {
    pub fn new(host: Box<dyn ScriptHostDocument>, journal: Journal, url: &str) -> Inner {
        let mut inner = Inner {
            host,
            journal,
            doc: Document::new(),
            url: url.to_owned(),
            viewport: Viewport::default(),
            sheets: Vec::new(),
            constructed: BTreeMap::new(),
            adopted: Vec::new(),
            next_sheet_id: 1,
            sheets_dirty: true,
            styles: StyleSet::new(),
            styles_valid: false,
            style_engine: None,
            layout_dirty: true,
            laid_out: None,
            hit_epoch: 0,
            hit_list: None,
            state_changed: Vec::new(),
            tree: None,
            tree_generation: u64::MAX,
            styles_generation: u64::MAX,
            generation: 1,
            layout_cache: LayoutCache {
                keep_across_passes: true,
                ..LayoutCache::default()
            },
            images: ImageSizeMap::default(),
            scroll: ScrollState::new(),
            hovered: None,
            pointer: None,
            hover_generation: u64::MAX,
            inline_handlers: (u64::MAX, 0, true),
            active: None,
            focused: None,
            focus_visible: false,
            target_id: None,
            form: FormData::default(),
            wrappers: Vec::new(),
            protos: BTreeMap::new(),
            logs: Vec::new(),
            canvases: BTreeMap::new(),
            module_sources: BTreeMap::new(),
            parsing: false,
            write_buffer: String::new(),
            ready_state: "loading".into(),
            current_script: None,
            deferred_scripts: Vec::new(),
            async_scripts: Vec::new(),
            pending_scripts: Vec::new(),
            executed_scripts: BTreeSet::new(),
            has_raf: false,
            has_layout_observers: false,
            observed_generation: 0,
            observing: false,
            custom_defined: BTreeSet::new(),
            history: vec![HistoryEntry {
                url: url.to_owned(),
                state: None,
            }],
            history_index: 0,
            hidden: false,
            alerts: Vec::new(),
            inline_cache: BTreeMap::new(),
            referrer: String::new(),
            cookie_cache: None,
            dom_mutations_since_styles: 0,
            anim_state: BTreeMap::new(),
            pending_animations: Vec::new(),
        };
        inner.doc.url = url.to_owned();
        inner.viewport = inner.host_viewport();
        inner.target_id = url
            .split_once('#')
            .map(|(_, h)| h.to_owned())
            .filter(|h| !h.is_empty());
        inner
    }

    // ------------------------------------------------------------ host (journaled)

    pub fn host_now_micros(&mut self) -> i64 {
        if let Some(JournalEntry::Now(t)) = self.journal.next_replayed() {
            return *t;
        }
        let t = self.host.now_micros();
        self.journal.record(JournalEntry::Now(t));
        t
    }

    pub fn host_random_u64(&mut self) -> u64 {
        if let Some(JournalEntry::Random(r)) = self.journal.next_replayed() {
            return *r;
        }
        let r = self.host.random_u64();
        self.journal.record(JournalEntry::Random(r));
        r
    }

    pub fn host_viewport(&mut self) -> Viewport {
        if let Some(JournalEntry::Viewport(w, h, s, z)) = self.journal.next_replayed() {
            return Viewport {
                width: *w,
                height: *h,
                scale: *s,
                zoom: *z,
            };
        }
        let v = self.host.viewport();
        self.journal
            .record(JournalEntry::Viewport(v.width, v.height, v.scale, v.zoom));
        v
    }

    pub fn host_fetch(&mut self, req: &FetchRequest) -> Result<FetchResponse, String> {
        if let Some(JournalEntry::Fetch(r)) = self.journal.next_replayed() {
            return r.clone();
        }
        let r = self.host.fetch(req);
        self.journal.record(JournalEntry::Fetch(r.clone()));
        r
    }

    pub fn host_storage_get(&mut self, area: StorageArea, key: &str) -> Option<String> {
        if let Some(JournalEntry::StorageGet(v)) = self.journal.next_replayed() {
            return v.clone();
        }
        let v = self.host.storage_get(area, key);
        self.journal.record(JournalEntry::StorageGet(v.clone()));
        v
    }

    pub fn host_storage_keys(&mut self, area: StorageArea) -> Vec<String> {
        if let Some(JournalEntry::StorageKeys(v)) = self.journal.next_replayed() {
            return v.clone();
        }
        let v = self.host.storage_keys(area);
        self.journal.record(JournalEntry::StorageKeys(v.clone()));
        v
    }

    pub fn host_cookie_get(&mut self) -> String {
        if let Some(JournalEntry::Cookie(v)) = self.journal.next_replayed() {
            return v.clone();
        }
        let v = self.host.cookie_get();
        self.journal.record(JournalEntry::Cookie(v.clone()));
        v
    }

    /// A call to the embedder (`__cw_host`), journaled.
    pub fn host_call(&mut self, name: &str, payload: &str) -> Result<String, String> {
        if let Some(JournalEntry::HostCall(r)) = self.journal.next_replayed() {
            return r.clone();
        }
        let r = self.host.host_call(name, payload);
        self.journal.record(JournalEntry::HostCall(r.clone()));
        r
    }

    /// A host write: skipped during replay (the host already did it).
    fn host_write(&mut self, f: impl FnOnce(&mut dyn ScriptHostDocument)) {
        if self.journal.next_replayed().is_some() {
            return;
        }
        self.journal.record(JournalEntry::Write);
        f(&mut *self.host);
    }

    pub fn host_storage_set(&mut self, area: StorageArea, key: &str, value: &str) {
        self.host_write(|h| h.storage_set(area, key, value));
    }
    pub fn host_storage_remove(&mut self, area: StorageArea, key: &str) {
        self.host_write(|h| h.storage_remove(area, key));
    }
    pub fn host_cookie_set(&mut self, cookie: &str) {
        self.host_write(|h| h.cookie_set(cookie));
    }
    pub fn host_navigate(&mut self, url: &str) {
        self.host_write(|h| h.navigate(url));
    }
    pub fn host_submit_form(
        &mut self,
        action: &str,
        method: &str,
        enctype: &str,
        data: &[(String, String)],
    ) {
        self.host_write(|h| h.submit_form(action, method, enctype, data));
    }

    pub fn log(&mut self, level: LogLevel, text: &str) {
        self.logs.push(LogEntry {
            level,
            text: text.to_owned(),
        });
        self.host.log(level, text);
    }

    // ------------------------------------------------------------ urls

    /// Resolves `href` against the document URL (RFC 3986 relative resolution for
    /// the forms pages use).
    pub fn resolve_url(&self, href: &str) -> String {
        resolve_url(&self.url, href)
    }

    pub fn title(&self) -> String {
        let t = self
            .doc
            .descendants(Document::ROOT)
            .find(|n| self.doc.is(*n, "title"));
        match t {
            Some(t) => collapse_ws(&self.doc.text_content(t)),
            None => String::new(),
        }
    }

    // ------------------------------------------------------------ dirty tracking

    /// Marks the document changed (layout and styles will be recomputed lazily).
    pub fn touch(&mut self) {
        self.generation += 1;
    }

    pub fn touch_state(&mut self, node: NodeId) {
        self.state_changed.push(node);
        self.generation += 1;
    }

    pub fn media(&self) -> Media {
        let z = self.viewport.zoom.max(1) as i64;
        Media {
            width_px: (self.viewport.width as i64 * 100 / z) as i32,
            height_px: (self.viewport.height as i64 * 100 / z) as i32,
            dppx: css::token::Number::from_i64(self.viewport.scale.max(1) as i64),
            ..Media::default()
        }
    }

    // ------------------------------------------------------------ stylesheets

    fn alloc_sheet_id(&mut self) -> u32 {
        let id = self.next_sheet_id;
        self.next_sheet_id += 1;
        id
    }

    /// Rebuilds the document sheet list from `<style>` and `<link rel=stylesheet>`
    /// elements in tree order, keeping entries whose source did not change (so CSSOM
    /// edits survive), and expanding `@import`s.
    fn rebuild_sheets(&mut self) {
        let mut old: Vec<SheetEntry> = std::mem::take(&mut self.sheets);
        let mut out: Vec<SheetEntry> = Vec::new();
        let nodes: Vec<NodeId> = self
            .doc
            .descendants(Document::ROOT)
            .filter(|n| self.doc.is(*n, "style") || self.doc.is(*n, "link"))
            .collect();
        for n in nodes {
            if self.doc.is(n, "style") {
                let ty = self
                    .doc
                    .attr(n, "type")
                    .unwrap_or("text/css")
                    .trim()
                    .to_ascii_lowercase();
                if !ty.is_empty() && ty != "text/css" {
                    continue;
                }
                let text = self.doc.text_content(n);
                let media = self.doc.attr(n, "media").unwrap_or("").to_owned();
                if let Some(pos) = old.iter().position(|e| e.owner == SheetOwner::Element(n)) {
                    let mut e = old.remove(pos);
                    if e.source != text {
                        e.sheet = css::parse_stylesheet(&text, Origin::Author, Strictness::Lenient)
                            .unwrap_or_default();
                        e.source = text;
                    }
                    e.media = media;
                    self.push_with_imports(&mut out, e);
                } else {
                    let sheet = css::parse_stylesheet(&text, Origin::Author, Strictness::Lenient)
                        .unwrap_or_default();
                    let id = self.alloc_sheet_id();
                    self.push_with_imports(
                        &mut out,
                        SheetEntry {
                            id,
                            owner: SheetOwner::Element(n),
                            sheet,
                            disabled: false,
                            media,
                            href: None,
                            source: text,
                        },
                    );
                }
            } else {
                let rel = self.doc.attr(n, "rel").unwrap_or("").to_ascii_lowercase();
                if !rel.split_ascii_whitespace().any(|r| r == "stylesheet") {
                    continue;
                }
                let Some(href) = self.doc.attr(n, "href").map(|h| self.resolve_url(h)) else {
                    continue;
                };
                let media = self.doc.attr(n, "media").unwrap_or("").to_owned();
                if let Some(pos) = old.iter().position(|e| {
                    e.owner == SheetOwner::Element(n) && e.href.as_deref() == Some(href.as_str())
                }) {
                    let mut e = old.remove(pos);
                    e.media = media;
                    self.push_with_imports(&mut out, e);
                } else {
                    let r = self.host_fetch(&FetchRequest {
                        url: href.clone(),
                        method: "GET".into(),
                        headers: vec![],
                        body: None,
                    });
                    let text = match r {
                        Ok(resp) if resp.status < 400 => {
                            String::from_utf8_lossy(&resp.body).into_owned()
                        }
                        _ => String::new(),
                    };
                    let sheet = css::parse_stylesheet(&text, Origin::Author, Strictness::Lenient)
                        .unwrap_or_default();
                    let id = self.alloc_sheet_id();
                    let disabled = self.doc.has_attr(n, "disabled");
                    self.push_with_imports(
                        &mut out,
                        SheetEntry {
                            id,
                            owner: SheetOwner::Element(n),
                            sheet,
                            disabled,
                            media,
                            href: Some(href),
                            source: text,
                        },
                    );
                }
            }
        }
        // Constructed sheets that were adopted stay in `constructed`; imports of
        // removed sheets are dropped with them.
        self.sheets = out;
        self.sheets_dirty = false;
    }

    fn push_with_imports(&mut self, out: &mut Vec<SheetEntry>, entry: SheetEntry) {
        let imports: Vec<(String, String)> = entry
            .sheet
            .imports()
            .map(|(u, m)| (u.to_owned(), m.to_string()))
            .collect();
        for (url, media) in imports {
            let url = resolve_url(entry.href.as_deref().unwrap_or(&self.url), &url);
            if out.iter().any(|e| e.href.as_deref() == Some(url.as_str())) {
                continue;
            }
            let r = self.host_fetch(&FetchRequest {
                url: url.clone(),
                method: "GET".into(),
                headers: vec![],
                body: None,
            });
            let text = match r {
                Ok(resp) if resp.status < 400 => String::from_utf8_lossy(&resp.body).into_owned(),
                _ => String::new(),
            };
            let sheet = css::parse_stylesheet(&text, Origin::Author, Strictness::Lenient)
                .unwrap_or_default();
            let id = self.alloc_sheet_id();
            let imported = SheetEntry {
                id,
                owner: SheetOwner::Import(entry.id),
                sheet,
                disabled: false,
                media,
                href: Some(url),
                source: text,
            };
            self.push_with_imports(out, imported);
        }
        out.push(entry);
    }

    /// The sheet with this id, in the document list or the constructed map.
    pub fn sheet(&self, id: u32) -> Option<&SheetEntry> {
        self.sheets
            .iter()
            .find(|s| s.id == id)
            .or_else(|| self.constructed.get(&id))
    }
    pub fn sheet_mut(&mut self, id: u32) -> Option<&mut SheetEntry> {
        if let Some(i) = self.sheets.iter().position(|s| s.id == id) {
            return self.sheets.get_mut(i);
        }
        self.constructed.get_mut(&id)
    }
    /// The sheet id of a `<style>`/`<link>` element, building the list if needed.
    pub fn sheet_of_element(&mut self, node: NodeId) -> Option<u32> {
        if self.sheets_dirty {
            self.rebuild_sheets();
        }
        self.sheets
            .iter()
            .find(|s| s.owner == SheetOwner::Element(node))
            .map(|s| s.id)
    }
    /// The document's sheets (`document.styleSheets`): element-owned ones in order.
    pub fn document_sheet_ids(&mut self) -> Vec<u32> {
        if self.sheets_dirty {
            self.rebuild_sheets();
        }
        self.sheets
            .iter()
            .filter(|s| matches!(s.owner, SheetOwner::Element(_)))
            .map(|s| s.id)
            .collect()
    }
    pub fn new_constructed_sheet(&mut self, text: &str) -> u32 {
        let id = self.alloc_sheet_id();
        let sheet =
            css::parse_stylesheet(text, Origin::Author, Strictness::Lenient).unwrap_or_default();
        self.constructed.insert(
            id,
            SheetEntry {
                id,
                owner: SheetOwner::Constructed,
                sheet,
                disabled: false,
                media: String::new(),
                href: None,
                source: text.to_owned(),
            },
        );
        id
    }
    /// Marks a sheet edited through the CSSOM: a full cascade follows.
    pub fn sheet_changed(&mut self) {
        self.styles_valid = false;
        self.generation += 1;
    }

    // ------------------------------------------------------------ styles and layout

    /// Whether the stylesheets contain anything at all (documents without CSS still
    /// cascade the UA sheet).
    fn effective_sheets(&self) -> Vec<Stylesheet> {
        let media = self.media();
        let mut out = Vec::new();
        for e in &self.sheets {
            if e.disabled {
                continue;
            }
            if !e.media.trim().is_empty() && !css::MediaQueryList::parse(&e.media).evaluate(&media)
            {
                continue;
            }
            out.push(e.sheet.clone());
        }
        for id in &self.adopted {
            if let Some(e) = self.constructed.get(id) {
                if !e.disabled {
                    out.push(e.sheet.clone());
                }
            }
        }
        out
    }

    /// The selector-matching state (hover, focus, form state, ...) styles are
    /// computed against.
    fn match_context(&self) -> MatchContext<'_> {
        match_context(
            &self.doc,
            &self.form,
            (self.hovered, self.active, self.focused),
            self.focus_visible,
            &self.target_id,
        )
    }

    /// Checks the incrementally maintained styles and fragment tree against a
    /// from-scratch cascade and layout of the same document and state, panicking
    /// with the first difference (`style::profile::verifying`).
    fn verify_incremental(&self) {
        let media = self.media();
        let sheets = self.effective_sheets();
        let ctx = self.match_context();
        let fresh = style::cascade(&self.doc, &sheets, &media, &ctx, Strictness::Lenient)
            .unwrap_or_else(|_| StyleSet::new());
        if let Some(d) = self.styles.diff(&fresh, &self.doc) {
            panic!(
                "incremental restyle differs from a full cascade at {}: {d}",
                self.url
            );
        }
        let Some(tree) = &self.tree else {
            return;
        };
        let mut cache = LayoutCache {
            overlay_scrollbars: self.layout_cache.overlay_scrollbars,
            ..LayoutCache::default()
        };
        let opts = LayoutOptions {
            images: &self.images,
            scroll: &self.scroll,
        };
        let fresh_tree = layout::layout_with(&self.doc, &fresh, self.viewport, opts, &mut cache);
        if *tree != fresh_tree {
            panic!(
                "incremental layout differs from a full layout at {}",
                self.url
            );
        }
    }

    /// Flushes pending style work: a full cascade when sheets changed (or none ran),
    /// else an incremental restyle from the document's mutation log and the changed
    /// matching state.
    pub fn ensure_styles(&mut self) {
        if self.sheets_dirty {
            self.rebuild_sheets();
            self.styles_valid = false;
        }
        let up_to_date = self.styles_valid
            && self.doc.mutations.is_empty()
            && self.state_changed.is_empty()
            && self.styles_generation == self.generation;
        if up_to_date {
            return;
        }
        let media = self.media();
        let quirks = self.doc.quirks == crate::dom::QuirksMode::Quirks;
        // The cascade engine lives as long as the sheets and media it was built
        // from: every sheet change (and a viewport change) clears `styles_valid`.
        if !self.styles_valid
            || !self
                .style_engine
                .as_ref()
                .is_some_and(|e| e.is_for(&media, quirks, Strictness::Lenient))
        {
            self.styles_valid = false;
            let sheets_t = style::profile::span(style::profile::Phase::Sheets);
            let sheets = self.effective_sheets();
            drop(sheets_t);
            self.style_engine =
                style::StyleEngine::build(&sheets, &media, quirks, Strictness::Lenient).ok();
        }
        let mutations: Vec<Mutation> = self.doc.drain_mutations();
        let changed: Vec<NodeId> = std::mem::take(&mut self.state_changed);
        let ctx = match_context(
            &self.doc,
            &self.form,
            (self.hovered, self.active, self.focused),
            self.focus_visible,
            &self.target_id,
        );
        // Layout reads the document: a mutation relayouts unless it is an
        // attribute layout reads only through style; a style change relayouts
        // only when it can move a box (`Restyled::layout_changed`).
        self.layout_dirty |= mutations.iter().any(|m| match m {
            Mutation::AttributeChanged { node, name, .. } => {
                layout::boxes::attribute_affects_layout(&self.doc, &self.styles, *node, name)
            }
            _ => true,
        });
        let restyled = match &self.style_engine {
            None => {
                self.styles = StyleSet::new();
                style::Restyled {
                    changed: true,
                    layout_changed: true,
                    hits_changed: true,
                }
            }
            Some(engine) if !self.styles_valid => {
                self.styles = engine
                    .cascade(&self.doc, &ctx)
                    .unwrap_or_else(|_| StyleSet::new());
                style::Restyled {
                    changed: true,
                    layout_changed: true,
                    hits_changed: true,
                }
            }
            Some(engine) => engine
                .update(&self.doc, &mut self.styles, &mutations, &changed, &ctx)
                .unwrap_or(style::Restyled {
                    changed: true,
                    layout_changed: true,
                    hits_changed: true,
                }),
        };
        self.layout_dirty |= restyled.layout_changed;
        if restyled.hits_changed {
            self.hit_epoch += 1;
        }
        self.styles_valid = true;
        self.styles_generation = self.generation;
        let _t = style::profile::span(style::profile::Phase::StyleOther);
        self.inline_cache.clear();
        self.detect_animations();
    }

    /// Compares each element's transitioned properties and `animation-name` with what
    /// the previous style flush left, and records the transitions and animations the
    /// change started. CSS Transitions §3: a transition starts when a transitionable
    /// property's computed value changes while `transition-duration` is non-zero; an
    /// element seen for the first time transitions nothing.
    fn detect_animations(&mut self) {
        let mut fresh: BTreeMap<NodeId, AnimState> = BTreeMap::new();
        let mut props: Vec<(String, String)> = Vec::new();
        for idx in 0..self.styles.styles.len() {
            let Some(style) = self.styles.styles[idx].clone() else {
                continue;
            };
            let transitions = style.transitions.duration.iter().any(|d| *d > 0);
            let animations = style.animations.name.iter().any(|n| n != "none");
            let node = NodeId(idx as u32);
            if !transitions && !animations {
                if let Some(p) = self.anim_state.remove(&node) {
                    for gone in p.names {
                        self.pending_animations.push(AnimationStart {
                            node,
                            name: gone,
                            is_animation: true,
                            delay_ms: 0,
                            duration_ms: 0,
                            iterations: None,
                            cancelled: true,
                        });
                    }
                }
                continue;
            }
            // Text nodes inherit a computed style; only elements transition.
            if !self.doc.is_element(node) {
                continue;
            }
            let prev = self.anim_state.get(&node);
            // The cascade shares one `Rc` per unchanged element, so an untouched
            // element costs a pointer comparison.
            if let Some(p) = prev {
                if std::rc::Rc::ptr_eq(&p.style, &style) {
                    let keep = self.anim_state.remove(&node).unwrap();
                    fresh.insert(node, keep);
                    continue;
                }
            }
            props.clear();
            if transitions {
                for t in style.transitions.items() {
                    if t.duration_ms <= 0 {
                        continue;
                    }
                    let names: &[&str] = if t.property == "all" {
                        TRANSITION_ALL
                    } else {
                        &[t.property.as_str()]
                    };
                    for name in names {
                        if props.iter().any(|(p, _)| p == name) {
                            continue;
                        }
                        let Some(value) = style.serialize(name) else {
                            continue;
                        };
                        if let Some(old) =
                            prev.and_then(|p| p.values.iter().find(|(p, _)| p == name))
                        {
                            if old.1 != value {
                                self.pending_animations.push(AnimationStart {
                                    node,
                                    name: (*name).to_owned(),
                                    is_animation: false,
                                    delay_ms: t.delay_ms,
                                    duration_ms: t.duration_ms,
                                    iterations: Some(1.0),
                                    cancelled: false,
                                });
                            }
                        }
                        props.push(((*name).to_owned(), value));
                    }
                }
            }
            let mut names: Vec<String> = Vec::new();
            if animations {
                for a in style.animations.items() {
                    if a.name == "none" || names.contains(&a.name) {
                        continue;
                    }
                    if !prev.map(|p| p.names.contains(&a.name)).unwrap_or(false)
                        && a.duration_ms > 0
                    {
                        self.pending_animations.push(AnimationStart {
                            node,
                            name: a.name.clone(),
                            is_animation: true,
                            delay_ms: a.delay_ms,
                            duration_ms: a.duration_ms,
                            iterations: a.iteration_count.map(|c| c as f64 / 1000.0),
                            cancelled: false,
                        });
                    }
                    names.push(a.name.clone());
                }
            }
            // An animation whose name is gone stops: CSS Animations §4 cancels it.
            if let Some(p) = prev {
                for gone in p.names.iter().filter(|n| !names.contains(n)) {
                    self.pending_animations.push(AnimationStart {
                        node,
                        name: gone.clone(),
                        is_animation: true,
                        delay_ms: 0,
                        duration_ms: 0,
                        iterations: None,
                        cancelled: true,
                    });
                }
            }
            fresh.insert(
                node,
                AnimState {
                    style,
                    values: std::mem::take(&mut props),
                    names,
                },
            );
        }
        self.anim_state = fresh;
    }

    /// Flushes style and layout.
    pub fn ensure_layout(&mut self) {
        self.ensure_styles();
        if self.tree.is_some() && self.tree_generation == self.generation {
            return;
        }
        // Nothing layout reads changed (a hover or focus change that restyled
        // nothing, or only colours): the tree stands.
        let same_inputs = self.laid_out.as_ref().is_some_and(|(v, s, i, o)| {
            *v == self.viewport
                && *s == self.scroll
                && *i == self.images
                && *o == self.layout_cache.overlay_scrollbars
        });
        if self.tree.is_some() && !self.layout_dirty && same_inputs {
            self.tree_generation = self.generation;
            if style::profile::verifying() {
                self.verify_incremental();
            }
            return;
        }
        let opts = LayoutOptions {
            images: &self.images,
            scroll: &self.scroll,
        };
        let tree = layout::layout_with(
            &self.doc,
            &self.styles,
            self.viewport,
            opts,
            &mut self.layout_cache,
        );
        self.clamp_scroll(&tree);
        if self.tree_generation != self.generation {
            self.tree = Some(tree);
            self.tree_generation = self.generation;
        }
        self.layout_dirty = false;
        self.laid_out = Some((
            self.viewport,
            self.scroll.clone(),
            self.images.clone(),
            self.layout_cache.overlay_scrollbars,
        ));
        self.hit_epoch += 1;
        if style::profile::verifying() {
            self.verify_incremental();
        }
    }

    /// Clamps every scroll offset to the laid-out scrollable range.
    fn clamp_scroll(&mut self, tree: &FragmentTree) {
        let mut changed = false;
        for (node, (x, y)) in self.scroll.iter_mut() {
            let (max_x, max_y) = if *node == Document::ROOT {
                (
                    (tree.content_width - tree.viewport_width).max(Au::ZERO),
                    (tree.content_height - tree.viewport_height).max(Au::ZERO),
                )
            } else {
                match fragment_of(tree, *node) {
                    Some((f, _)) => match &f.kind {
                        layout::FragmentKind::Box {
                            scroll: Some(s),
                            border,
                            ..
                        } => {
                            let bar_w = if s.shows_y_bar {
                                Au::from_px_i32(15)
                            } else {
                                Au::ZERO
                            };
                            let bar_h = if s.shows_x_bar {
                                Au::from_px_i32(15)
                            } else {
                                Au::ZERO
                            };
                            let inner_w = f.rect.size.width - border.horizontal() - bar_w;
                            let inner_h = f.rect.size.height - border.vertical() - bar_h;
                            (
                                (s.content_width - inner_w).max(Au::ZERO),
                                (s.content_height - inner_h).max(Au::ZERO),
                            )
                        }
                        _ => (Au::ZERO, Au::ZERO),
                    },
                    None => (Au::ZERO, Au::ZERO),
                }
            };
            let nx = (*x).clamp(Au::ZERO, max_x);
            let ny = (*y).clamp(Au::ZERO, max_y);
            if nx != *x || ny != *y {
                *x = nx;
                *y = ny;
                changed = true;
            }
        }
        if changed {
            // The clamped offsets are what the next layout should use; this layout
            // already placed content with the requested ones, so redo it.
            self.generation += 1;
            let opts = LayoutOptions {
                images: &self.images,
                scroll: &self.scroll,
            };
            let t2 = layout::layout_with(
                &self.doc,
                &self.styles,
                self.viewport,
                opts,
                &mut self.layout_cache,
            );
            self.tree = Some(t2);
            self.tree_generation = self.generation;
        }
    }

    /// Absolute border-box rects of the element's fragments (document coordinates).
    pub fn rects_of(&mut self, node: NodeId) -> Vec<crate::geom::Rect> {
        self.ensure_layout();
        self.tree
            .as_ref()
            .map(|t| t.rects_of(node))
            .unwrap_or_default()
    }

    /// The document scroll offset in Au.
    pub fn window_scroll(&self) -> (Au, Au) {
        self.scroll
            .get(&Document::ROOT)
            .copied()
            .unwrap_or((Au::ZERO, Au::ZERO))
    }

    pub fn set_scroll(&mut self, node: NodeId, x: Au, y: Au) {
        let cur = self
            .scroll
            .get(&node)
            .copied()
            .unwrap_or((Au::ZERO, Au::ZERO));
        let nx = x.max(Au::ZERO);
        let ny = y.max(Au::ZERO);
        if cur == (nx, ny) {
            return;
        }
        self.scroll.insert(node, (nx, ny));
        self.generation += 1;
    }

    /// The topmost element at viewport coordinates, through the painting order.
    pub fn element_from_point(&mut self, x: i32, y: i32) -> Option<NodeId> {
        self.ensure_layout();
        let tree = self.tree.as_ref()?;
        if x < 0 || y < 0 || x >= self.viewport.width as i32 || y >= self.viewport.height as i32 {
            return None;
        }
        let mut ctx = crate::paint::PaintContext::default();
        let (sx, sy) = self.window_scroll();
        ctx.scroll = crate::geom::Point { x: sx, y: sy };
        for (n, (ox, oy)) in &self.scroll {
            if *n != Document::ROOT {
                ctx.scroll_offsets
                    .insert(*n, crate::geom::Point { x: *ox, y: *oy });
            }
        }
        if self.hit_list.as_ref().map(|(e, _)| *e) != Some(self.hit_epoch) {
            let list = crate::paint::hit::HitList::build(tree, &self.styles, self.viewport, &ctx);
            if style::profile::verifying() {
                let painted = crate::paint::hit::HitList::build_by_painting(
                    tree,
                    &self.styles,
                    self.viewport,
                    &ctx,
                );
                if list != painted {
                    panic!("a hits-only traversal differs from a paint at {}", self.url);
                }
            }
            self.hit_list = Some((self.hit_epoch, list));
        } else if style::profile::verifying() {
            let fresh = crate::paint::hit::HitList::build(tree, &self.styles, self.viewport, &ctx);
            if self.hit_list.as_ref().map(|(_, l)| l) != Some(&fresh) {
                panic!("a reused hit list differs from a fresh one at {}", self.url);
            }
        }
        let hit = self.hit_list.as_ref().and_then(|(_, l)| l.at(x, y));
        match hit {
            Some(n) => {
                // Text nodes map to their element.
                let mut n = n;
                while !self.doc.is_element(n) {
                    n = self.doc.parent(n)?;
                }
                Some(n)
            }
            None => self.doc.document_element(),
        }
    }

    // ------------------------------------------------------------ text

    /// `innerText`: the rendered text approximation (block breaks by computed
    /// `display`, `display: none` skipped, whitespace collapsed per `white-space`).
    pub fn inner_text(&mut self, node: NodeId) -> String {
        self.ensure_styles();
        let mut out = String::new();
        self.inner_text_into(node, &mut out);
        let trimmed = out.trim_matches('\n').to_owned();
        trimmed
    }

    fn inner_text_into(&self, node: NodeId, out: &mut String) {
        match self.doc.kind(node) {
            NodeKind::Text(t) => {
                let parent = self.doc.parent(node);
                let ws = parent
                    .and_then(|p| self.styles.get(p))
                    .map(|s| s.white_space)
                    .unwrap_or(style::WhiteSpace::Normal);
                if ws.collapses() {
                    let collapsed = collapse_ws_keep_edges(t);
                    if out.ends_with('\n') || out.is_empty() {
                        out.push_str(collapsed.trim_start());
                    } else {
                        out.push_str(&collapsed);
                    }
                } else {
                    out.push_str(t);
                }
            }
            NodeKind::Element { tag, .. } => {
                let style = self.styles.get(node);
                let display = style.map(|s| s.display).unwrap_or(style::Display::Inline);
                if display.is_none()
                    || matches!(
                        tag.as_str(),
                        "script" | "style" | "template" | "noscript" | "head"
                    )
                {
                    return;
                }
                if tag == "br" {
                    out.push('\n');
                    return;
                }
                let block = !display.is_inline_level() || tag == "li";
                if block && !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                if tag == "p" && !out.is_empty() && !out.ends_with("\n\n") {
                    out.push('\n');
                }
                let kids: Vec<NodeId> = self.doc.children(node).collect();
                for k in kids {
                    self.inner_text_into(k, out);
                }
                if block && !out.ends_with('\n') {
                    out.push('\n');
                }
                if tag == "p" && !out.ends_with("\n\n") {
                    out.push('\n');
                }
            }
            NodeKind::DocumentFragment | NodeKind::Document => {
                let kids: Vec<NodeId> = self.doc.children(node).collect();
                for k in kids {
                    self.inner_text_into(k, out);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ forms

    /// The current value of a control (dirty value, else the attribute/content).
    pub fn control_value(&self, node: NodeId) -> String {
        if let Some(v) = self.form.values.get(&node) {
            return v.clone();
        }
        if self.doc.is(node, "textarea") {
            return self.doc.text_content(node);
        }
        if self.doc.is(node, "select") {
            let selected = self.selected_options(node);
            return selected
                .first()
                .map(|o| self.option_value(*o))
                .unwrap_or_default();
        }
        if self.doc.is(node, "option") {
            return self.option_value(node);
        }
        self.doc.attr(node, "value").unwrap_or("").to_owned()
    }

    pub fn option_value(&self, option: NodeId) -> String {
        match self.doc.attr(option, "value") {
            Some(v) => v.to_owned(),
            None => collapse_ws(&self.doc.text_content(option)),
        }
    }

    pub fn options_of(&self, select: NodeId) -> Vec<NodeId> {
        self.doc
            .descendants(select)
            .filter(|n| *n != select && self.doc.is(*n, "option"))
            .collect()
    }

    pub fn is_checked(&self, node: NodeId) -> bool {
        self.form.checked(&self.doc, node)
    }

    /// Selected options of a select, applying the single-select rule (the last
    /// selected wins; the first option when none is).
    pub fn selected_options(&self, select: NodeId) -> Vec<NodeId> {
        let options = self.options_of(select);
        let multiple = self.doc.has_attr(select, "multiple");
        let selected: Vec<NodeId> = options
            .iter()
            .copied()
            .filter(|o| self.is_checked(*o))
            .collect();
        if multiple {
            return selected;
        }
        match selected.last() {
            Some(o) => vec![*o],
            None => Vec::new(),
        }
    }

    pub fn set_checked(&mut self, node: NodeId, checked: bool) {
        let ty = self
            .doc
            .attr(node, "type")
            .unwrap_or("")
            .to_ascii_lowercase();
        if checked && ty == "radio" {
            let name = self.doc.attr(node, "name").map(str::to_owned);
            if let Some(name) = name {
                let form = self.form_owner(node);
                let group: Vec<NodeId> = self
                    .doc
                    .descendants(Document::ROOT)
                    .filter(|n| {
                        *n != node
                            && self.doc.is(*n, "input")
                            && self
                                .doc
                                .attr(*n, "type")
                                .map(|t| t.eq_ignore_ascii_case("radio"))
                                .unwrap_or(false)
                            && self.doc.attr(*n, "name") == Some(name.as_str())
                            && self.form_owner(*n) == form
                    })
                    .collect();
                for g in group {
                    self.form.checked.insert(g, false);
                    self.state_changed.push(g);
                }
            }
        }
        self.form.checked.insert(node, checked);
        self.form.indeterminate.remove(&node);
        self.touch_state(node);
    }

    pub fn set_option_selected(&mut self, option: NodeId, selected: bool) {
        let select = self
            .doc
            .ancestors(option)
            .find(|a| self.doc.is(*a, "select"));
        if selected {
            if let Some(select) = select {
                if !self.doc.has_attr(select, "multiple") {
                    for o in self.options_of(select) {
                        if o != option {
                            self.form.checked.insert(o, false);
                            self.state_changed.push(o);
                        }
                    }
                }
            }
        }
        self.form.checked.insert(option, selected);
        self.touch_state(option);
    }

    pub fn set_value(&mut self, node: NodeId, value: &str) {
        let value = if self.doc.is(node, "input")
            && !matches!(
                self.doc
                    .attr(node, "type")
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "hidden" | "textarea"
            ) {
            value.replace(['\n', '\r'], "")
        } else {
            value.to_owned()
        };
        let len = value.chars().count();
        self.form.values.insert(node, value);
        self.form.selection.insert(node, (len, len));
        self.touch_state(node);
    }

    /// The form an element belongs to (`form` attribute or the nearest ancestor).
    pub fn form_owner(&self, node: NodeId) -> Option<NodeId> {
        if let Some(id) = self.doc.attr(node, "form") {
            if let Some(f) = self.doc.by_id(id).first() {
                if self.doc.is(*f, "form") {
                    return Some(*f);
                }
            }
        }
        self.doc.ancestors(node).find(|a| self.doc.is(*a, "form"))
    }

    /// The listed elements of a form (`form.elements`), in tree order.
    pub fn form_elements(&self, form: NodeId) -> Vec<NodeId> {
        let mut out: Vec<NodeId> = Vec::new();
        for n in self.doc.descendants(Document::ROOT) {
            if n == form || !self.doc.is_element(n) {
                continue;
            }
            let tag = self.doc.tag(n).unwrap_or("");
            let listed = matches!(
                tag,
                "button" | "fieldset" | "input" | "object" | "output" | "select" | "textarea"
            ) || self.custom_defined.contains(tag);
            if !listed {
                continue;
            }
            if tag == "input"
                && self
                    .doc
                    .attr(n, "type")
                    .map(|t| t.eq_ignore_ascii_case("image"))
                    .unwrap_or(false)
            {
                continue;
            }
            if self.form_owner(n) == Some(form) {
                out.push(n);
            }
        }
        out
    }

    /// Constructs the form data set (name, value) for submission.
    pub fn form_data_set(&self, form: NodeId, submitter: Option<NodeId>) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for n in self.form_elements(form) {
            let tag = self.doc.tag(n).unwrap_or("");
            if self.is_disabled(n) {
                continue;
            }
            let Some(name) = self.doc.attr(n, "name").map(str::to_owned) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            match tag {
                "input" => {
                    let ty = self
                        .doc
                        .attr(n, "type")
                        .unwrap_or("text")
                        .to_ascii_lowercase();
                    match ty.as_str() {
                        "checkbox" | "radio" => {
                            if self.is_checked(n) {
                                out.push((
                                    name,
                                    self.doc.attr(n, "value").unwrap_or("on").to_owned(),
                                ));
                            }
                        }
                        "submit" | "reset" | "button" | "image" => {
                            if Some(n) == submitter {
                                out.push((
                                    name,
                                    self.doc.attr(n, "value").unwrap_or("").to_owned(),
                                ));
                            }
                        }
                        "file" => {}
                        _ => out.push((name, self.control_value(n))),
                    }
                }
                "textarea" => out.push((name, self.control_value(n))),
                "select" => {
                    for o in self.selected_options(n) {
                        if !self.is_disabled(o) {
                            out.push((name.clone(), self.option_value(o)));
                        }
                    }
                }
                "button" => {
                    if Some(n) == submitter {
                        out.push((name, self.doc.attr(n, "value").unwrap_or("").to_owned()));
                    }
                }
                "output" => out.push((name, self.doc.text_content(n))),
                _ => {}
            }
        }
        if let Some(s) = submitter {
            if self.doc.tag(s) == Some("button") || self.doc.tag(s) == Some("input") {
                let name = self.doc.attr(s, "name").unwrap_or("");
                if !name.is_empty() && !self.form_elements(form).contains(&s) {
                    out.push((
                        name.to_owned(),
                        self.doc.attr(s, "value").unwrap_or("").to_owned(),
                    ));
                }
            }
        }
        out
    }

    pub fn is_disabled(&self, node: NodeId) -> bool {
        if self.doc.has_attr(node, "disabled") {
            return true;
        }
        if self.doc.is(node, "option") {
            return self
                .doc
                .ancestors(node)
                .any(|a| self.doc.is(a, "optgroup") && self.doc.has_attr(a, "disabled"));
        }
        self.doc.ancestors(node).any(|a| {
            self.doc.is(a, "fieldset")
                && self.doc.has_attr(a, "disabled")
                && !self
                    .doc
                    .children(a)
                    .find(|c| self.doc.is(*c, "legend"))
                    .map(|l| self.doc.ancestors(node).any(|x| x == l))
                    .unwrap_or(false)
        })
    }

    /// Whether the element can take focus.
    pub fn is_focusable(&self, node: NodeId) -> bool {
        if !self.doc.is_element(node) || self.is_disabled(node) {
            return false;
        }
        let tag = self.doc.tag(node).unwrap_or("");
        if let Some(t) = self.doc.attr(node, "tabindex") {
            if t.trim().parse::<i32>().is_ok() {
                return true;
            }
        }
        match tag {
            "input" => !self
                .doc
                .attr(node, "type")
                .map(|t| t.eq_ignore_ascii_case("hidden"))
                .unwrap_or(false),
            "textarea" | "select" | "button" | "summary" | "iframe" => true,
            "a" | "area" => self.doc.has_attr(node, "href"),
            _ => self
                .doc
                .attr(node, "contenteditable")
                .map(|c| c != "false")
                .unwrap_or(false),
        }
    }

    /// The caret position a click at viewport point (`x`, `y`) puts in the text
    /// control `node`: the character boundary nearest the point, as painting
    /// lays the value out (from the content box's left edge; a textarea's lines
    /// wrapped to its width). `None` when the control has no box.
    pub fn caret_from_point(&mut self, node: NodeId, x: i32, y: i32) -> Option<usize> {
        self.ensure_layout();
        let (sx, sy) = self.window_scroll();
        let (content, font) = {
            let tree = self.tree.as_ref()?;
            let (f, abs) = fragment_of(tree, node)?;
            let layout::FragmentKind::Box {
                padding, border, ..
            } = &f.kind
            else {
                return None;
            };
            let left = abs.origin.x + border.left + padding.left;
            let top = abs.origin.y + border.top + padding.top;
            let width = abs.size.width - border.left - border.right - padding.left - padding.right;
            (
                (
                    left.to_px_round(),
                    top.to_px_round(),
                    width.to_px_round().max(1),
                ),
                self.styles.get(node)?.font.clone(),
            )
        };
        let px = x + sx.to_px_round();
        let py = y + sy.to_px_round();
        let value = self.control_value(node);
        let password = self.doc.is(node, "input")
            && self
                .doc
                .attr(node, "type")
                .is_some_and(|t| t.eq_ignore_ascii_case("password"));
        let shown: Vec<char> = if password {
            vec!['\u{2022}'; value.chars().count()]
        } else {
            value.chars().collect()
        };
        // The boundary in `line` (a run of `shown` starting at `start`) nearest
        // `px`.
        let nearest = |start: usize, line: &[char]| -> usize {
            let mut best = (i32::MAX, 0);
            let mut prefix = String::new();
            for i in 0..=line.len() {
                if i > 0 {
                    prefix.push(line[i - 1]);
                }
                let at = content.0 + crate::paint::text::width_px(&font, &prefix) as i32;
                let d = (at - px).abs();
                if d < best.0 {
                    best = (d, i);
                }
                if at > px {
                    break;
                }
            }
            start + best.1
        };
        if !self.doc.is(node, "textarea") {
            return Some(nearest(0, &shown));
        }
        // Visual lines: each hard line wrapped to the content width, with the
        // character offset where it starts.
        let size = font.size_px();
        let mut lines: Vec<(usize, Vec<char>)> = Vec::new();
        let mut at = 0;
        for hard in value.split('\n') {
            let hard_chars: Vec<char> = hard.chars().collect();
            let wrapped = cw_scene::metrics::wrap(
                font.typeface,
                font.scene_style(),
                hard,
                size,
                content.2 as u32,
            );
            let mut pos = 0;
            for w in wrapped.iter().filter(|w| !w.is_empty()) {
                let w: Vec<char> = w.chars().collect();
                // Soft breaks drop the spaces they break at.
                while pos < hard_chars.len() && hard_chars[pos] == ' ' && w.first() != Some(&' ') {
                    pos += 1;
                }
                let len = w.len().min(hard_chars.len() - pos);
                lines.push((at + pos, hard_chars[pos..pos + len].to_vec()));
                pos += len;
            }
            if wrapped.iter().all(|w| w.is_empty()) {
                lines.push((at, Vec::new()));
            }
            at += hard_chars.len() + 1;
        }
        let lh = crate::paint::text::line_height_px(size) as i32;
        let row = ((py - content.1) / lh.max(1)).clamp(0, lines.len().saturating_sub(1) as i32);
        let (start, line) = &lines[row as usize];
        Some(nearest(*start, line))
    }

    pub fn is_text_control(&self, node: NodeId) -> bool {
        if self.doc.is(node, "textarea") {
            return true;
        }
        if self.doc.is(node, "input") {
            let ty = self
                .doc
                .attr(node, "type")
                .unwrap_or("text")
                .to_ascii_lowercase();
            return matches!(
                ty.as_str(),
                "text"
                    | "search"
                    | "url"
                    | "tel"
                    | "email"
                    | "password"
                    | "number"
                    | "date"
                    | "time"
                    | "datetime-local"
                    | "month"
                    | "week"
                    | "color"
                    | "range"
                    | ""
            );
        }
        self.doc
            .attr(node, "contenteditable")
            .map(|c| c != "false")
            .unwrap_or(false)
    }

    /// Focusable elements in document order (for Tab).
    pub fn focus_order(&self) -> Vec<NodeId> {
        let mut with_index: Vec<(i32, usize, NodeId)> = Vec::new();
        for (i, n) in self.doc.descendants(Document::ROOT).enumerate() {
            if !self.is_focusable(n) {
                continue;
            }
            let ti: i32 = self
                .doc
                .attr(n, "tabindex")
                .and_then(|t| t.trim().parse().ok())
                .unwrap_or(0);
            if ti < 0 {
                continue;
            }
            with_index.push((if ti == 0 { i32::MAX } else { ti }, i, n));
        }
        with_index.sort();
        with_index.into_iter().map(|(_, _, n)| n).collect()
    }
}

/// The fragment of an element (its first box), with its absolute rect.
pub fn fragment_of(
    tree: &FragmentTree,
    node: NodeId,
) -> Option<(&layout::Fragment, crate::geom::Rect)> {
    let mut out = None;
    tree.root.walk(crate::geom::Point::default(), &mut |f, r| {
        if out.is_some() {
            return;
        }
        if let Some(s) = f.source() {
            if !s.is_anonymous()
                && s.node() == node
                && matches!(f.kind, layout::FragmentKind::Box { .. })
            {
                out = Some((f, r));
            }
        }
    });
    out
}

pub fn collapse_ws(s: &str) -> String {
    s.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

fn collapse_ws_keep_edges(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// Resolves a reference against a base URL (scheme, authority, path, query,
/// fragment), enough for the forms pages use.
pub fn resolve_url(base: &str, href: &str) -> String {
    let href = href.trim();
    if href.is_empty() {
        return base.split('#').next().unwrap_or("").to_owned();
    }
    if has_scheme(href) {
        return href.to_owned();
    }
    let (scheme, rest) = match base.split_once("://") {
        Some((s, r)) => (s, r),
        None => return href.to_owned(),
    };
    let (authority, path_qf) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if let Some(r) = href.strip_prefix("//") {
        return format!("{scheme}://{r}");
    }
    let base_path_q = path_qf.split('#').next().unwrap_or("/");
    let base_path = base_path_q.split('?').next().unwrap_or("/");
    if let Some(h) = href.strip_prefix('#') {
        return format!("{scheme}://{authority}{base_path_q}#{h}");
    }
    if let Some(q) = href.strip_prefix('?') {
        return format!("{scheme}://{authority}{base_path}?{q}");
    }
    let (href_path, tail) = match href.find(['?', '#']) {
        Some(i) => (&href[..i], &href[i..]),
        None => (href, ""),
    };
    let merged = if href_path.starts_with('/') {
        href_path.to_owned()
    } else {
        let dir = match base_path.rfind('/') {
            Some(i) => &base_path[..=i],
            None => "/",
        };
        format!("{dir}{href_path}")
    };
    let mut segs: Vec<&str> = Vec::new();
    for seg in merged.split('/') {
        match seg {
            "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    let mut path = segs.join("/");
    if merged.ends_with("/.") || merged.ends_with("/..") {
        path.push('/');
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    format!("{scheme}://{authority}{path}{tail}")
}

fn has_scheme(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
    }
    false
}
