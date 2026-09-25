//! The script layer: bindings from the deterministic JavaScript VM (`cw_jsvm`) to the
//! engine's DOM, CSSOM, events, timers and layout, for one document.
//!
//! A [`Realm`] owns one VM instance and the document it scripts. The browser drives
//! it through a few entry points: [`Realm::run_document`] parses the page and runs
//! its scripts in document order (pausing the parser at every `</script>`, so
//! `document.write` works), [`Realm::run_until_idle`] drains microtasks and the
//! timers due on the world clock, [`Realm::dispatch`] turns the browser's pointer,
//! key and scroll actions into the DOM event sequences and reports the default
//! action left for the browser, [`Realm::after_layout`] delivers `ResizeObserver` and
//! `IntersectionObserver` records, and [`Realm::animation_frame`] runs the
//! `requestAnimationFrame` callbacks for one painted frame. Everything the realm
//! needs from the outside (network, clock, entropy, storage, cookies, console
//! output) comes through the [`ScriptHostDocument`] trait the browser implements.
//!
//! Layout on demand: geometry reads (`offsetWidth`, `getBoundingClientRect`,
//! `getComputedStyle`, `elementFromPoint`) flush pending style and layout through the
//! engine's own `style::cascade`/`restyle` and `layout::layout_with`, tracked by a
//! generation counter every mutation bumps; the browser reads the same styled tree
//! back from the realm to paint.
//!
//! The Web API surface is implemented as native functions on the VM's object model
//! (`bindings`) plus a JavaScript prelude (`web.js`) that defines the class
//! hierarchy, the event system, observers and the fetch/XHR/form objects on top of
//! those natives, exactly as the VM's Node flavour is built on its `bootstrap.js`.
//!
//! Snapshots: the VM heap is not serialisable, so a realm snapshots as its inputs
//! (the page, every entry-point call in order) plus a journal of the host's answers
//! (`RealmState`); `Realm::restore` replays them into a fresh VM, which is
//! deterministic, and continues live from there.
//!
//! Documented approximations: `attachShadow` creates an open `ShadowRoot` whose
//! content is rendered as light content (styles are not scoped); `<iframe>` has no
//! nested browsing context (`contentWindow`/`contentDocument` are `null`);
//! `innerText` is a layout-aware approximation (block breaks from computed
//! `display`); canvas text is rasterised as glyph boxes from the metrics tables, not
//! glyph outlines; `getComputedStyle` reports computed (not used) values for `auto`
//! lengths; a `CSSStyleDeclaration` over the `style` attribute keeps declarations as
//! written, so a shorthand set through it is not readable through its longhands.

pub mod bindings;
pub mod bridge;
pub mod canvas;
pub mod inner;
pub mod journal;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use cw_jsvm::value::{Ctl, Value};
use cw_jsvm::vm::Vm;
use serde::{Deserialize, Serialize};

use crate::dom::{Document, NodeId};
use crate::layout::FragmentTree;
use crate::paint::RgbaImage;
use crate::style::StyleSet;
use crate::Viewport;

pub use cw_jsvm::profile::{ProfileOptions, ProfileReport};
pub use inner::{Inner, LogEntry, LogLevel};
pub use journal::{Journal, JournalEntry};

/// A request the page makes through `fetch`, `XMLHttpRequest`, a `<script src>`, a
/// stylesheet `<link>`, an `@import` or a module import.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// What the transport answered. Synchronous at the host boundary; the page sees it
/// asynchronously (a promise resolved from the event loop).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The final URL after redirects.
    pub url: String,
}

impl FetchResponse {
    pub fn ok(url: &str, content_type: &str, body: &[u8]) -> FetchResponse {
        FetchResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("content-type".into(), content_type.into())],
            body: body.to_vec(),
            url: url.to_owned(),
        }
    }
    pub fn not_found(url: &str) -> FetchResponse {
        FetchResponse {
            status: 404,
            status_text: "Not Found".into(),
            headers: vec![],
            body: Vec::new(),
            url: url.to_owned(),
        }
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageArea {
    Local,
    Session,
}

/// The callbacks the browser implements for one document. Every method has a
/// default so a test host only implements what it needs; reads are journaled for
/// snapshot replay, writes are skipped while a restore is replaying.
pub trait ScriptHostDocument {
    /// Performs a request over the world's transport. `Err` is a network error
    /// (the page's `fetch` rejects with a `TypeError`).
    fn fetch(&mut self, request: &FetchRequest) -> Result<FetchResponse, String> {
        let _ = request;
        Err("network unavailable".into())
    }
    /// The page asked to navigate (`location.assign`, a link's default action is
    /// reported separately through `DefaultAction::Navigate`).
    fn navigate(&mut self, url: &str) {
        let _ = url;
    }
    /// A form submitted by script (`form.submit()`, `element.click()` on a submit
    /// button): the browser performs the request. The default navigates to the
    /// action with the data as a query string.
    fn submit_form(
        &mut self,
        action: &str,
        method: &str,
        enctype: &str,
        data: &[(String, String)],
    ) {
        let _ = enctype;
        if method == "post" {
            self.navigate(action);
        } else {
            let query: Vec<String> = data
                .iter()
                .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
                .collect();
            let base = action.split(['?', '#']).next().unwrap_or(action);
            self.navigate(&format!("{base}?{}", query.join("&")));
        }
    }
    /// The world clock, in microseconds.
    fn now_micros(&self) -> i64 {
        0
    }
    /// The world's seeded entropy.
    fn random_u64(&mut self) -> u64 {
        0x9E37_79B9_7F4A_7C15
    }
    fn viewport(&self) -> Viewport {
        Viewport::default()
    }
    /// Layout-affecting state changed; the browser will repaint.
    fn request_relayout(&mut self) {}
    fn storage_get(&self, area: StorageArea, key: &str) -> Option<String> {
        let _ = (area, key);
        None
    }
    fn storage_set(&mut self, area: StorageArea, key: &str, value: &str) {
        let _ = (area, key, value);
    }
    fn storage_remove(&mut self, area: StorageArea, key: &str) {
        let _ = (area, key);
    }
    fn storage_keys(&self, area: StorageArea) -> Vec<String> {
        let _ = area;
        Vec::new()
    }
    fn cookie_get(&self) -> String {
        String::new()
    }
    fn cookie_set(&mut self, cookie: &str) {
        let _ = cookie;
    }
    /// Console output and page errors, one line each.
    fn log(&mut self, level: LogLevel, text: &str) {
        let _ = (level, text);
    }
    /// A call from the page to its embedder: script calls
    /// `__cw_host(name, payload)` (both strings) and gets the `Ok` string back
    /// synchronously, or an `Error` with the `Err` message thrown. What names and
    /// payloads mean is the embedder's (a web application's host, for one); a host
    /// that does not implement it refuses every call. Answers are journaled like
    /// the other host calls, so a restore replays them.
    fn host_call(&mut self, name: &str, payload: &str) -> Result<String, String> {
        let _ = (name, payload);
        Err(format!("the host does not answer `{name}`"))
    }
}

/// `application/x-www-form-urlencoded` encoding of one value.
pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A host with in-memory storage and a static table of URLs, for tests and simple
/// embeddings.
#[derive(Default)]
pub struct MemoryHost {
    pub responses: std::collections::BTreeMap<String, FetchResponse>,
    pub local: std::collections::BTreeMap<String, String>,
    pub session: std::collections::BTreeMap<String, String>,
    pub cookies: String,
    pub now_micros: i64,
    pub seed: u64,
    pub viewport: Viewport,
    pub navigations: Vec<String>,
    pub logs: Vec<(LogLevel, String)>,
    pub requests: Vec<FetchRequest>,
    pub relayouts: u32,
}

impl MemoryHost {
    pub fn new() -> MemoryHost {
        MemoryHost {
            seed: 0x1234_5678_9ABC_DEF1,
            ..Default::default()
        }
    }
    pub fn with_response(mut self, url: &str, content_type: &str, body: &str) -> MemoryHost {
        self.responses.insert(
            url.to_owned(),
            FetchResponse::ok(url, content_type, body.as_bytes()),
        );
        self
    }
    fn area(&self, area: StorageArea) -> &std::collections::BTreeMap<String, String> {
        match area {
            StorageArea::Local => &self.local,
            StorageArea::Session => &self.session,
        }
    }
    fn area_mut(&mut self, area: StorageArea) -> &mut std::collections::BTreeMap<String, String> {
        match area {
            StorageArea::Local => &mut self.local,
            StorageArea::Session => &mut self.session,
        }
    }
}

impl ScriptHostDocument for MemoryHost {
    fn fetch(&mut self, request: &FetchRequest) -> Result<FetchResponse, String> {
        self.requests.push(request.clone());
        let key = request.url.split('#').next().unwrap_or("");
        match self.responses.get(key) {
            Some(r) => Ok(r.clone()),
            None => match self.responses.get(&request.url) {
                Some(r) => Ok(r.clone()),
                None => Ok(FetchResponse::not_found(&request.url)),
            },
        }
    }
    fn navigate(&mut self, url: &str) {
        self.navigations.push(url.to_owned());
    }
    fn now_micros(&self) -> i64 {
        self.now_micros
    }
    fn random_u64(&mut self) -> u64 {
        let mut x = self.seed | 1;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        x
    }
    fn viewport(&self) -> Viewport {
        self.viewport
    }
    fn request_relayout(&mut self) {
        self.relayouts += 1;
    }
    fn storage_get(&self, area: StorageArea, key: &str) -> Option<String> {
        self.area(area).get(key).cloned()
    }
    fn storage_set(&mut self, area: StorageArea, key: &str, value: &str) {
        self.area_mut(area).insert(key.to_owned(), value.to_owned());
    }
    fn storage_remove(&mut self, area: StorageArea, key: &str) {
        self.area_mut(area).remove(key);
    }
    fn storage_keys(&self, area: StorageArea) -> Vec<String> {
        self.area(area).keys().cloned().collect()
    }
    fn cookie_get(&self) -> String {
        self.cookies.clone()
    }
    fn cookie_set(&mut self, cookie: &str) {
        // name=value; attributes: keep the pair, replace an existing name.
        let pair = cookie.split(';').next().unwrap_or("").trim();
        let name = pair.split('=').next().unwrap_or("").trim();
        let mut pairs: Vec<String> = self
            .cookies
            .split("; ")
            .filter(|p| !p.is_empty() && p.split('=').next().unwrap_or("").trim() != name)
            .map(str::to_owned)
            .collect();
        if !pair.is_empty()
            && !cookie.contains("max-age=0")
            && !cookie
                .to_ascii_lowercase()
                .contains("expires=thu, 01 jan 1970")
        {
            pairs.push(pair.to_owned());
        }
        self.cookies = pairs.join("; ");
    }
    fn log(&mut self, level: LogLevel, text: &str) {
        self.logs.push((level, text.to_owned()));
    }
}

/// Keyboard modifier state of a UI event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// What the browser's agent did; `dispatch` turns each into the DOM's event
/// sequence. Coordinates are CSS pixels in the viewport.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiEvent {
    /// The pointer moved: `mousemove`, and `mouseover`/`mouseout`/`mouseenter`/
    /// `mouseleave` when the hovered element changed (`:hover` follows).
    PointerMove {
        x: i32,
        y: i32,
        modifiers: Modifiers,
    },
    /// `pointerdown`/`mousedown` at a point, focus change, `pointerup`/`mouseup`, then
    /// `click` (and `dblclick` when `detail` is 2) with the activation behaviour.
    Click {
        x: i32,
        y: i32,
        button: u8,
        modifiers: Modifiers,
        detail: u32,
    },
    /// The same sequence targeted at an element (the agent's element actions).
    ClickNode {
        node: NodeId,
        modifiers: Modifiers,
        detail: u32,
    },
    /// Only the press or release half of a click.
    PointerDown {
        x: i32,
        y: i32,
        button: u8,
        modifiers: Modifiers,
    },
    PointerUp {
        x: i32,
        y: i32,
        button: u8,
        modifiers: Modifiers,
    },
    /// A key press to the focused element: `keydown`, `keypress` for printable keys,
    /// `beforeinput`, the value edit, `input`, then `keyup`. `key` is the DOM key
    /// value (`"a"`, `"Enter"`, `"Backspace"`), `code` the physical code (empty to
    /// derive it from `key`).
    Key {
        key: String,
        code: String,
        modifiers: Modifiers,
        repeat: bool,
    },
    /// Only `keydown` (`down: true`) or `keyup`.
    KeyHalf {
        key: String,
        code: String,
        modifiers: Modifiers,
        down: bool,
    },
    /// Types a string into the focused control, one key at a time.
    TypeText {
        text: String,
    },
    /// Sets a control's value as if the user edited it, firing `input` (and `change`
    /// when `commit`).
    SetValue {
        node: NodeId,
        value: String,
        commit: bool,
    },
    /// Scrolls a scroll container (`None` is the viewport) to the offsets; `scroll`
    /// fires on the element or the document.
    Scroll {
        node: Option<NodeId>,
        x: i32,
        y: i32,
    },
    /// A wheel at a point; the nearest scroll container scrolls unless prevented.
    Wheel {
        x: i32,
        y: i32,
        delta_x: i32,
        delta_y: i32,
        modifiers: Modifiers,
    },
    Focus {
        node: Option<NodeId>,
    },
    /// The viewport changed: `resize` and `matchMedia` change events.
    Resize {
        width: u32,
        height: u32,
    },
    /// The browser changed the URL fragment (back/forward, address bar).
    HashChange {
        hash: String,
    },
    /// The browser went back or forward in the realm's own history entries.
    HistoryGo {
        delta: i32,
    },
    Visibility {
        hidden: bool,
    },
    /// `pageshow` after the document became the shown page.
    PageShow,
    /// `beforeunload`, `pagehide`, `unload`.
    Unload,
}

/// The default action left for the browser after `dispatch`, unless the page
/// called `preventDefault`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefaultAction {
    None,
    /// The page prevented the default.
    Prevented,
    /// Follow a link (resolved URL).
    Navigate(String),
    /// Submit a form: the encoded data set is ready for the browser's transport.
    Submit {
        form: NodeId,
        action: String,
        method: String,
        enctype: String,
        data: Vec<(String, String)>,
    },
    /// A checkbox, radio, `<details>` or `<dialog>` toggled (already applied to the
    /// document; the browser repaints).
    Toggle(NodeId),
    /// Focus moved to the element (already applied; the browser paints the caret).
    Focus(NodeId),
    /// The page asked to close a `beforeunload` prompt: the returned message.
    ConfirmUnload(String),
}

/// One entry-point call, recorded for snapshot replay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Input {
    RunDocument,
    RunUntilIdle {
        advance_ms: u32,
    },
    Dispatch(UiEvent),
    AfterLayout,
    AnimationFrame,
    Eval(String),
    /// The browser told the realm the intrinsic sizes of pictures it fetched
    /// (`src` as written, width, height), which layout (and so script) can see.
    ImageSizes(Vec<(String, u32, u32)>),
}

/// A realm's serialisable state: its page and every input and host answer since it
/// started. `Realm::restore` rebuilds an identical realm from it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealmState {
    pub html: String,
    pub url: String,
    pub inputs: Vec<Input>,
    pub journal: Vec<JournalEntry>,
    /// VM steps each entry-point call (and each `<script>`) may spend before it is
    /// interrupted with a `TimeoutError`; 0 keeps the VM's own lifetime budget.
    /// Part of the state so a restore replays under the same limit.
    #[serde(default)]
    pub step_budget: u64,
    /// The host draws scrollbars over the content (overlay scrollbars, or a
    /// headless browser that hides them), so scroll containers give no space to
    /// them; see `Realm::set_overlay_scrollbars`.
    #[serde(default)]
    pub overlay_scrollbars: bool,
    /// A heap snapshot of this realm as it was after some prefix of `inputs`
    /// (`Realm::heap_image`), when one was taken: `Realm::restore` reads it and
    /// replays only the inputs after it. It is a cache of what replay would
    /// rebuild, held in memory only (never serialised, and ignored by
    /// comparisons), since it is readable only by the program image that wrote it.
    #[serde(skip)]
    pub image: HeapImage,
}

/// A realm's heap image, shared between the states that carry it.
#[derive(Clone, Default)]
pub struct HeapImage(pub Option<std::sync::Arc<ImageData>>);

/// The bytes of a realm's heap image, and how far into its state it was taken.
pub struct ImageData {
    pub bytes: Vec<u8>,
    /// The long source texts the bytes name (shared by a realm's images).
    pub sources: Vec<std::sync::Arc<str>>,
    /// The entry-point calls, and the journal entries, the realm had received.
    pub inputs: usize,
    pub journal: usize,
}

impl PartialEq for HeapImage {
    fn eq(&self, _: &HeapImage) -> bool {
        true
    }
}
impl Eq for HeapImage {}
impl std::fmt::Debug for HeapImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(i) => write!(
                f,
                "HeapImage({} bytes after {} inputs)",
                i.bytes.len(),
                i.inputs
            ),
            None => f.write_str("HeapImage(none)"),
        }
    }
}

/// VM steps a realm runs between two heap images its snapshots carry (see
/// `Realm::snapshot`): about 10 ms of script to replay at most, against the 2 to
/// 4 ms an image of a framework page takes to write.
pub const IMAGE_EVERY_STEPS: u64 = 2_000_000;

/// What `Realm::heap_image` writes: the VM heap and the state outside it, as the
/// heap's length (8 bytes, little-endian), the heap, and the outside state in
/// postcard.
struct RealmImage {
    heap: Vec<u8>,
    inner: inner::image::InnerImage,
}

impl RealmImage {
    fn write(heap: &[u8], inner: &inner::image::InnerImage) -> Result<Vec<u8>, String> {
        let tail = postcard::to_allocvec(inner).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(8 + heap.len() + tail.len());
        out.extend_from_slice(&(heap.len() as u64).to_le_bytes());
        out.extend_from_slice(heap);
        out.extend_from_slice(&tail);
        Ok(out)
    }

    /// The heap's bytes and the outside state.
    fn read(bytes: &[u8]) -> Result<(&[u8], inner::image::InnerImage), String> {
        let n = bytes
            .get(..8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()) as usize)
            .ok_or("not a realm image")?;
        let heap = bytes.get(8..8 + n).ok_or("realm image truncated")?;
        let inner = postcard::from_bytes(&bytes[8 + n..]).map_err(|e| e.to_string())?;
        Ok((heap, inner))
    }
}

/// One document's script environment. See the module documentation.
pub struct Realm {
    vm: Vm<'static>,
    // Dropped after `vm`, which borrows it (field order is drop order).
    _bridge: Box<bridge::Bridge>,
    pub(crate) inner: Rc<RefCell<Inner>>,
    state: RealmState,
    /// Frees the realm's cyclic garbage once everything above (the VM and the
    /// document's handles on JS values) has dropped.
    _reclaim: Option<cw_jsvm::gc::Reclaim>,
    /// This realm is the second half of a snapshot check (see
    /// `set_verify_snapshots`), not itself checked.
    verifying: bool,
    /// The last heap image of this realm (taken, or restored from) and the VM's
    /// step count then: what its snapshots carry (see `snapshot`).
    image: RefCell<(Option<std::sync::Arc<ImageData>>, u64)>,
    /// The shared copies of the VM's long source texts its images carry.
    shared_sources: RefCell<Vec<(Rc<str>, std::sync::Arc<str>)>>,
}

const PRELUDE: &str = concat!(
    include_str!("web/01-core.js"),
    include_str!("web/02-node.js"),
    include_str!("web/03-element.js"),
    include_str!("web/04-html-elements.js"),
    include_str!("web/05-cssom.js"),
    include_str!("web/06-window.js")
);

impl Realm {
    /// Creates the realm for `html` at `url`; nothing runs until `run_document`.
    pub fn new(html: &str, url: &str, host: Box<dyn ScriptHostDocument>) -> Realm {
        Self::build(html, url, host, Journal::recording(), Vec::new())
    }

    /// Rebuilds a realm from a snapshot: from its heap image when it carries one
    /// this program can read, else by replaying its inputs against the journal;
    /// afterwards the realm is live on `host`.
    pub fn restore(state: &RealmState, host: Box<dyn ScriptHostDocument>) -> Realm {
        let Some(image) = &state.image.0 else {
            return Self::replay(state, host);
        };
        if image.inputs > state.inputs.len() || image.journal > state.journal.len() {
            return Self::replay(state, host);
        }
        let mut realm = match Self::from_image(state, image, host) {
            Ok(realm) => realm,
            Err((_, host)) => return Self::replay(state, host),
        };
        // The inputs after the image, answered from the journal.
        {
            let mut j = Journal::replay(state.journal.clone());
            j.replay_pos = image.journal;
            j.recording = realm.inner.borrow().journal.recording;
            realm.inner.borrow_mut().journal = j;
        }
        realm.apply_inputs(state.inputs[image.inputs..].to_vec());
        realm.inner.borrow_mut().journal.replaying = false;
        realm
    }

    /// Rebuilds a realm from a snapshot by replaying its inputs.
    pub fn replay(state: &RealmState, host: Box<dyn ScriptHostDocument>) -> Realm {
        let inputs = state.inputs.clone();
        let mut realm = Self::build(
            &state.html,
            &state.url,
            host,
            Journal::replay(state.journal.clone()),
            Vec::new(),
        );
        realm.state.step_budget = state.step_budget;
        realm.set_overlay_scrollbars(state.overlay_scrollbars);
        realm.apply_inputs(inputs);
        realm.inner.borrow_mut().journal.replaying = false;
        realm
    }

    /// Runs recorded entry-point calls again, in order.
    fn apply_inputs(&mut self, inputs: Vec<Input>) {
        let realm = self;
        for input in inputs {
            match input {
                Input::RunDocument => realm.run_document(),
                Input::RunUntilIdle { advance_ms } => {
                    realm.run_until_idle(advance_ms);
                }
                Input::Dispatch(ev) => {
                    realm.dispatch(ev);
                }
                Input::AfterLayout => realm.after_layout(),
                Input::AnimationFrame => realm.animation_frame(),
                Input::Eval(src) => {
                    let _ = realm.eval(&src);
                }
                Input::ImageSizes(sizes) => realm.set_image_sizes(sizes),
            }
        }
    }

    fn build(
        html: &str,
        url: &str,
        host: Box<dyn ScriptHostDocument>,
        journal: Journal,
        inputs: Vec<Input>,
    ) -> Realm {
        let inner = Rc::new(RefCell::new(Inner::new(host, journal, url)));
        let mut bridge = Box::new(bridge::Bridge {
            inner: inner.clone(),
        });
        // SAFETY: the bridge lives in a box the realm keeps until after the VM is
        // dropped (declared after `vm`, so dropped after it), its address is stable,
        // and it is reached only through `vm.host` from here on.
        let host_ref: &'static mut bridge::Bridge =
            unsafe { &mut *(&mut *bridge as *mut bridge::Bridge) };
        let (mut vm, booted) = match Self::from_template(host_ref, &inner) {
            Some(vm) => (vm, true),
            None => {
                // SAFETY: as above; `from_template` gave its reference up.
                let host_ref: &'static mut bridge::Bridge =
                    unsafe { &mut *(&mut *bridge as *mut bridge::Bridge) };
                let mut vm = Vm::new(host_ref, vec!["/usr/bin/browser".into()], Vec::new(), None);
                vm.stack_limit = 20;
                (vm, false)
            }
        };
        let any: Rc<dyn std::any::Any> = inner.clone();
        vm.embedder = Some(any);
        if !booted {
            bindings::install(&mut vm);
        }
        let reclaim = vm.reclaim.take();
        let mut realm = Realm {
            vm,
            _bridge: bridge,
            inner,
            state: RealmState {
                html: html.to_owned(),
                url: url.to_owned(),
                inputs,
                journal: Vec::new(),
                step_budget: 0,
                overlay_scrollbars: false,
                image: HeapImage::default(),
            },
            _reclaim: reclaim,
            verifying: false,
            image: RefCell::new((None, 0)),
            shared_sources: RefCell::new(Vec::new()),
        };
        if !booted {
            realm.run_prelude();
        }
        realm.image.borrow_mut().1 = realm.vm.steps;
        realm
    }

    /// A VM booted with the prelude, read from this thread's template (see
    /// `set_realm_templates`) instead of running it: the heap the prelude leaves
    /// does not depend on the page, its URL or its host, so one image of it serves
    /// every realm. The host is asked what `Vm::new` would ask it (the clock, then
    /// entropy), in that order, and the prelude's handles are put into `inner`.
    fn from_template(
        host_ref: &'static mut bridge::Bridge,
        inner: &Rc<RefCell<Inner>>,
    ) -> Option<Vm<'static>> {
        let t = template()?;
        let mut opts = bindings::snapshot_options();
        opts.sources_in = &t.sources;
        let (mut vm, roots) = Vm::from_heap_snapshot(host_ref, &t.heap, opts).ok()?;
        let mut i = inner.borrow_mut();
        vm.start_micros = i.host_now_micros();
        vm.rng_state = i.host_random_u64() | 1;
        let obj = |v: &Value| match v {
            Value::Obj(o) => Some(o.clone()),
            _ => None,
        };
        i.wrappers = roots[..t.wrappers].iter().map(obj).collect();
        i.protos = t
            .protos
            .iter()
            .cloned()
            .zip(&roots[t.wrappers..])
            .filter_map(|(k, v)| obj(v).map(|o| (k, o)))
            .collect();
        drop(i);
        Some(vm)
    }

    /// Writes the realm, between two entry points, as a heap snapshot: the VM
    /// heap (`cw_jsvm::snapshot`) and the realm state outside it (the document,
    /// stylesheet sources, interaction, form and loader state; styles and layout
    /// are recomputed after a restore). Deterministic: the same realm writes the
    /// same bytes, and a realm restored from them writes them again. `Err` when
    /// the realm holds something a snapshot cannot carry (a stylesheet script
    /// edited through the CSSOM); its state then restores by replay.
    pub fn heap_image(&self) -> Result<Vec<u8>, String> {
        self.write_image(None)
    }

    /// `heap_image`, with the long source texts put in `sources` instead (see
    /// `cw_jsvm::snapshot::Options::sources_out`).
    fn write_image(&self, sources: Option<&RefCell<Vec<Rc<str>>>>) -> Result<Vec<u8>, String> {
        let (img, roots) = self.inner.borrow().image()?;
        let mut opts = bindings::snapshot_options();
        opts.sources_out = sources;
        let heap = self
            .vm
            .heap_snapshot(&roots, opts)
            .map_err(|e| e.to_string())?;
        RealmImage::write(&heap, &img)
    }

    /// The realm's snapshot with a heap image of the realm as it is now (see
    /// `RealmState::image`), or with none when the realm cannot be imaged.
    pub fn snapshot_with_image(&self) -> RealmState {
        self.take_image();
        let mut s = self.snapshot_state();
        s.image = HeapImage(self.image.borrow().0.clone());
        s
    }

    /// Writes a heap image of the realm as it is now and keeps it for the
    /// snapshots that follow; keeps none when the realm cannot be imaged.
    fn take_image(&self) {
        let out = RefCell::new(Vec::new());
        let image = self.write_image(Some(&out)).ok().map(|bytes| {
            // The realm's source texts are shared by all its images.
            let mut shared = self.shared_sources.borrow_mut();
            let sources = out
                .into_inner()
                .into_iter()
                .map(|rc| {
                    if let Some((_, a)) = shared.iter().find(|(r, _)| Rc::ptr_eq(r, &rc)) {
                        return a.clone();
                    }
                    let a: std::sync::Arc<str> = std::sync::Arc::from(&*rc);
                    shared.push((rc, a.clone()));
                    a
                })
                .collect();
            let inner = self.inner.borrow();
            std::sync::Arc::new(ImageData {
                bytes,
                sources,
                inputs: self.state.inputs.len(),
                journal: inner.journal.entries.len(),
            })
        });
        let mut slot = self.image.borrow_mut();
        // A realm that cannot be imaged now keeps its last image (a prefix of its
        // state all the same) and is not tried again for as long.
        if image.is_some() {
            slot.0 = image;
        }
        slot.1 = self.vm.steps;
    }

    /// Rebuilds a realm from `state` and its heap image `bytes` (no script runs,
    /// and the host is not asked anything it would not be by replay). `Err` gives
    /// the host back when the image cannot be read.
    #[allow(clippy::result_large_err)]
    pub fn from_image(
        state: &RealmState,
        image: &std::sync::Arc<ImageData>,
        host: Box<dyn ScriptHostDocument>,
    ) -> Result<Realm, (String, Box<dyn ScriptHostDocument>)> {
        let (heap, image_inner) = match RealmImage::read(&image.bytes) {
            Ok(i) => i,
            Err(e) => return Err((e, host)),
        };
        let journal = Journal {
            entries: state.journal[..image.journal.min(state.journal.len())].to_vec(),
            replay_pos: 0,
            replaying: false,
            recording: true,
        };
        let inner = Rc::new(RefCell::new(Inner::new(host, journal, &state.url)));
        let mut bridge = Box::new(bridge::Bridge {
            inner: inner.clone(),
        });
        // SAFETY: as in `build`.
        let host_ref: &'static mut bridge::Bridge =
            unsafe { &mut *(&mut *bridge as *mut bridge::Bridge) };
        let give_back = |e: String, inner: Rc<RefCell<Inner>>| {
            let host = std::mem::replace(&mut inner.borrow_mut().host, Box::new(MemoryHost::new()));
            (e, host)
        };
        let sources: Vec<Rc<str>> = image.sources.iter().map(|a| Rc::from(&**a)).collect();
        let mut opts = bindings::snapshot_options();
        opts.sources_in = &sources;
        let (mut vm, roots) = match Vm::from_heap_snapshot(host_ref, heap, opts) {
            Ok(v) => v,
            Err(e) => {
                drop(bridge);
                return Err(give_back(e.to_string(), inner));
            }
        };
        let any: Rc<dyn std::any::Any> = inner.clone();
        vm.embedder = Some(any);
        let applied = inner.borrow_mut().apply_image(image_inner, &roots);
        drop(roots);
        if let Err(e) = applied {
            drop(vm);
            drop(bridge);
            return Err(give_back(e, inner));
        }
        {
            let mut i = inner.borrow_mut();
            i.layout_cache.overlay_scrollbars = state.overlay_scrollbars;
        }
        let reclaim = vm.reclaim.take();
        let steps = vm.steps;
        Ok(Realm {
            vm,
            _bridge: bridge,
            inner,
            state: RealmState {
                html: state.html.clone(),
                url: state.url.clone(),
                inputs: state.inputs[..image.inputs.min(state.inputs.len())].to_vec(),
                journal: Vec::new(),
                step_budget: state.step_budget,
                overlay_scrollbars: state.overlay_scrollbars,
                image: HeapImage::default(),
            },
            _reclaim: reclaim,
            verifying: false,
            image: RefCell::new((Some(image.clone()), steps)),
            shared_sources: RefCell::new(
                sources
                    .into_iter()
                    .zip(image.sources.iter().cloned())
                    .collect(),
            ),
        })
    }

    fn run_prelude(&mut self) {
        let r = self.vm.eval_source(PRELUDE, "web.js", false);
        if let Err(e) = r {
            let msg = self.error_text(&e);
            panic!("web.js prelude failed: {msg}");
        }
        self.flush_console();
    }

    fn error_text(&mut self, e: &Ctl) -> String {
        match e {
            Ctl::Throw(v) | Ctl::Fatal(v) => {
                let stack = self.vm.get_str(v, "stack").ok();
                match stack {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => self
                        .vm
                        .inspect_default(v)
                        .unwrap_or_else(|_| "error".into()),
                }
            }
            Ctl::Exit(c) => format!("exit {c}"),
        }
    }

    /// Limits every later entry-point call (and each `<script>` of the document) to
    /// `steps` VM steps; a script that never yields is interrupted with a
    /// `TimeoutError` reported to the console, and the realm stays usable. 0 removes
    /// the per-call limit.
    pub fn set_step_budget(&mut self, steps: u64) {
        self.state.step_budget = steps;
    }

    /// Starts the VM's profiler (see `cw_jsvm::profile`). It only observes: the
    /// realm behaves identically with it on, and it is not part of the snapshot.
    pub fn profile_start(&mut self, opts: ProfileOptions) {
        self.vm.profile_start(opts);
    }

    /// Stops the profiler and returns what it recorded since `profile_start`.
    pub fn profile_stop(&mut self) -> Option<ProfileReport> {
        self.vm.profile_stop()
    }

    /// Whether the host's scrollbars overlay the content (as on macOS, on phones,
    /// or in a headless Chromium launched with `--hide-scrollbars`, which is how
    /// the parity dumps are taken) instead of taking 15 px from each scroll
    /// container. A host setting, kept in the snapshot; layout redoes itself.
    /// Resolves `font-family` lists as on a device with these fonts installed (the
    /// parity harnesses use `LinuxBaseline`). Not journaled: a harness setting, made
    /// before the page runs.
    pub fn set_font_environment(&mut self, env: crate::css::FontEnvironment) {
        let mut i = self.inner.borrow_mut();
        if i.font_env != env {
            i.font_env = env;
            i.sheets_dirty = true;
            i.layout_cache.invalidate_all();
            i.touch();
        }
    }

    pub fn set_overlay_scrollbars(&mut self, on: bool) {
        self.state.overlay_scrollbars = on;
        let mut i = self.inner.borrow_mut();
        if i.layout_cache.overlay_scrollbars != on {
            i.layout_cache.overlay_scrollbars = on;
            i.layout_cache.invalidate_all();
            i.touch();
        }
    }

    /// Records an entry-point call for `snapshot`/`restore`, unless journaling is
    /// off.
    fn record_input(&mut self, input: Input) {
        if self.inner.borrow().journal.recording {
            self.state.inputs.push(input);
        }
    }

    /// Turns the input journal on or off (it is on from creation). An embedder
    /// that restores its realms from state of its own, rather than with
    /// `Realm::restore`, turns it off so the realm's memory does not grow with
    /// every input and host answer; a snapshot taken while it is off does not
    /// restore the realm.
    pub fn set_journaling(&mut self, on: bool) {
        self.inner.borrow_mut().journal.recording = on;
    }

    /// How many entries the journal holds: the entry-point calls recorded plus the
    /// host answers recorded (what `snapshot` carries and `restore` replays).
    pub fn journal_len(&self) -> usize {
        self.state.inputs.len() + self.inner.borrow().journal.entries.len()
    }

    /// Re-arms the VM's step limit for one entry-point call.
    fn arm(&mut self) {
        if self.state.step_budget > 0 {
            self.vm.budget = self.vm.steps.saturating_add(self.state.step_budget);
        }
    }

    /// Tells layout the intrinsic sizes of pictures the browser fetched, keyed by
    /// the `src` as written. Recorded as an input, so a restore sees the same layout.
    fn set_image_sizes_step(&mut self, sizes: Vec<(String, u32, u32)>) {
        self.record_input(Input::ImageSizes(sizes.clone()));
        let mut inner = self.inner.borrow_mut();
        for (src, w, h) in sizes {
            inner.images.0.insert(src, (w, h));
        }
        inner.touch();
    }

    /// The world-clock time (microseconds) the earliest pending timer is due at.
    pub fn next_timer_micros(&self) -> Option<i64> {
        let start = self.vm.start_micros;
        self.vm
            .timers
            .iter()
            .map(|t| if t.immediate { 0.0 } else { t.when })
            .fold(None, |m: Option<f64>, w| Some(m.map_or(w, |m| m.min(w))))
            .map(|ms| start + (ms * 1000.0) as i64)
    }

    /// Whether the page has `requestAnimationFrame` callbacks waiting for a frame.
    pub fn wants_animation_frame(&self) -> bool {
        self.inner.borrow().has_raf
    }

    /// The realm's same-document history: `(index, length)` (`pushState` entries).
    pub fn history_position(&self) -> (usize, usize) {
        let i = self.inner.borrow();
        (i.history_index, i.history.len())
    }

    /// The document (parsed once `run_document` ran).
    pub fn document(&self) -> std::cell::Ref<'_, Document> {
        std::cell::Ref::map(self.inner.borrow(), |i| &i.doc)
    }

    pub fn document_mut(&self) -> std::cell::RefMut<'_, Document> {
        std::cell::RefMut::map(self.inner.borrow_mut(), |i| &mut i.doc)
    }

    /// Flushes pending style and layout and returns the styled, laid-out state for
    /// painting: the browser paints from this.
    pub fn layout(&mut self) -> std::cell::Ref<'_, Inner> {
        self.inner.borrow_mut().ensure_layout();
        self.inner.borrow()
    }

    /// The computed styles after flushing pending restyles.
    pub fn styles(&mut self) -> std::cell::Ref<'_, StyleSet> {
        self.inner.borrow_mut().ensure_styles();
        std::cell::Ref::map(self.inner.borrow(), |i| &i.styles)
    }

    /// The fragment tree after flushing pending layout.
    pub fn fragment_tree(&mut self) -> std::cell::Ref<'_, FragmentTree> {
        self.inner.borrow_mut().ensure_layout();
        std::cell::Ref::map(self.inner.borrow(), |i| i.tree.as_ref().expect("laid out"))
    }

    /// The realm's state for a snapshot. It carries a heap image of the realm
    /// (see `RealmState::image`): the last one, or a new one when the realm has
    /// run `IMAGE_EVERY_STEPS` VM steps since, so a restore replays at most that
    /// much script. Writing one observes the realm without changing it.
    pub fn snapshot(&self) -> RealmState {
        let due = self.vm.steps.saturating_sub(self.image.borrow().1) >= IMAGE_EVERY_STEPS;
        let recording = self.inner.borrow().journal.recording;
        if due && recording && !self.verifying {
            self.take_image();
        }
        let mut s = self.snapshot_state();
        if recording {
            s.image = HeapImage(self.image.borrow().0.clone());
        }
        s
    }

    /// The realm's serialisable state, without an image.
    fn snapshot_state(&self) -> RealmState {
        let mut s = self.state.clone();
        s.journal = self.inner.borrow().journal.entries.clone();
        s.image = HeapImage::default();
        s
    }

    /// Console output and page errors so far (also delivered to the host's `log`).
    pub fn logs(&self) -> Vec<LogEntry> {
        self.inner.borrow().logs.clone()
    }

    /// The `alert`/`confirm`/`prompt` calls so far (`kind: message`).
    pub fn alerts(&self) -> Vec<String> {
        self.inner.borrow().alerts.clone()
    }

    pub fn focused(&self) -> Option<NodeId> {
        self.inner.borrow().focused
    }

    pub fn hovered(&self) -> Option<NodeId> {
        self.inner.borrow().hovered
    }

    /// The live values of form controls (what the user typed or script set), for
    /// `PaintContext::values`.
    pub fn form_values(&self) -> std::collections::BTreeMap<NodeId, String> {
        self.inner.borrow().form.values.clone()
    }

    /// The document's title, as `document.title` reads it.
    pub fn title(&self) -> String {
        self.inner.borrow().title()
    }

    /// The current URL (after `pushState`, hash changes).
    pub fn url(&self) -> String {
        self.inner.borrow().url.clone()
    }

    /// The raster of a `<canvas>` element's 2D context, if the page drew on it: the
    /// browser hands it to paint as the canvas's replaced content.
    pub fn canvas_pixels(&self, node: NodeId) -> Option<RgbaImage> {
        self.inner.borrow().canvases.get(&node).map(|c| c.image())
    }

    /// Every canvas with pixels, keyed by node.
    pub fn canvases(&self) -> Vec<(NodeId, RgbaImage)> {
        self.inner
            .borrow()
            .canvases
            .iter()
            .map(|(n, c)| (*n, c.image()))
            .collect()
    }

    /// Parses the page, running its scripts as the parser reaches them (`defer`
    /// after the parse, `async` after that, modules through the VM's loader), then
    /// fires `DOMContentLoaded` and `load`.
    fn run_document_step(&mut self) {
        self.record_input(Input::RunDocument);
        self.arm();
        let html = self.state.html.clone();
        let placeholder = Document::new();
        let start_doc = std::mem::replace(&mut self.inner.borrow_mut().doc, placeholder);
        let mut parser = crate::html::Parser::new(start_doc, &html);
        loop {
            let script = parser.next_script();
            std::mem::swap(parser.document(), &mut self.inner.borrow_mut().doc);
            match script {
                Some(script) => {
                    self.inner.borrow_mut().parsing = true;
                    self.run_parser_script(script);
                    let writes = std::mem::take(&mut self.inner.borrow_mut().write_buffer);
                    if !writes.is_empty() {
                        parser.write(&writes);
                    }
                    std::mem::swap(parser.document(), &mut self.inner.borrow_mut().doc);
                }
                None => break,
            }
        }
        drop(parser);
        {
            let mut inner = self.inner.borrow_mut();
            inner.parsing = false;
            inner.touch();
            inner.sheets_dirty = true;
        }
        self.call_hook("parsed", vec![]);
        // Async scripts: fetch-completion order made deterministic by document order.
        let async_scripts = std::mem::take(&mut self.inner.borrow_mut().async_scripts);
        for s in async_scripts {
            self.run_script_element(s, true);
        }
        self.inner.borrow_mut().ready_state = "interactive".into();
        self.call_hook("readyState", vec![]);
        let deferred = std::mem::take(&mut self.inner.borrow_mut().deferred_scripts);
        for s in deferred {
            self.run_script_element(s, true);
        }
        self.drain_microtasks();
        self.call_hook("domContentLoaded", vec![]);
        self.drain_microtasks();
        self.inner.borrow_mut().ready_state = "complete".into();
        self.call_hook("readyState", vec![]);
        self.call_hook("load", vec![]);
        self.drain_microtasks();
        self.flush_console();
        self.inner.borrow_mut().host.request_relayout();
    }

    fn run_parser_script(&mut self, script: NodeId) {
        let (is_async, is_defer, has_src, is_module) = {
            let inner = self.inner.borrow();
            let d = &inner.doc;
            let ty = d
                .attr(script, "type")
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            (
                d.has_attr(script, "async"),
                d.has_attr(script, "defer"),
                d.has_attr(script, "src"),
                ty == "module",
            )
        };
        if is_module || (is_defer && has_src) {
            self.inner.borrow_mut().deferred_scripts.push(script);
            return;
        }
        if is_async && has_src {
            self.inner.borrow_mut().async_scripts.push(script);
            return;
        }
        self.run_script_element(script, false);
    }

    /// Runs a `<script>` element now (inline text or fetched `src`), unless its type
    /// is not a JavaScript type or it is `nomodule`. `from_queue` scripts (deferred,
    /// async, dynamically inserted) run outside the parser.
    pub(crate) fn run_script_element(&mut self, script: NodeId, from_queue: bool) {
        let _ = from_queue;
        let (ty, src, text, nomodule, already) = {
            let mut inner = self.inner.borrow_mut();
            let already = !inner.executed_scripts.insert(script);
            let d = &inner.doc;
            let ty = d
                .attr(script, "type")
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            let src = d.attr(script, "src").map(|s| inner.resolve_url(s));
            let text = d.text_content(script);
            (ty, src, text, d.has_attr(script, "nomodule"), already)
        };
        if already {
            return;
        }
        self.arm();
        let is_js = ty.is_empty()
            || matches!(
                ty.as_str(),
                "text/javascript"
                    | "application/javascript"
                    | "text/ecmascript"
                    | "application/ecmascript"
                    | "module"
                    | "text/jscript"
                    | "text/x-javascript"
                    | "text/babel"
            );
        if !is_js || nomodule || ty == "text/babel" {
            return;
        }
        let is_module = ty == "module";
        let (source, name) = match &src {
            Some(url) => {
                let r = self.inner.borrow_mut().host_fetch(&FetchRequest {
                    url: url.clone(),
                    method: "GET".into(),
                    headers: vec![],
                    body: None,
                });
                match r {
                    Ok(resp) if resp.status < 400 => (
                        String::from_utf8_lossy(&resp.body).into_owned(),
                        url.clone(),
                    ),
                    _ => {
                        let w = self.wrap(script);
                        self.call_hook("scriptError", vec![w]);
                        return;
                    }
                }
            }
            None => (
                text,
                format!("{}#inline-{}", self.inner.borrow().url, script.0),
            ),
        };
        {
            let mut inner = self.inner.borrow_mut();
            inner.current_script = Some(script);
        }
        if is_module {
            self.run_module(&source, &name);
        } else {
            self.exec(&source, &name);
        }
        {
            let mut inner = self.inner.borrow_mut();
            inner.current_script = None;
        }
        let w = self.wrap(script);
        self.call_hook("scriptLoad", vec![w]);
        self.drain_microtasks();
    }

    fn run_module(&mut self, source: &str, url: &str) {
        let path = bridge::module_path(url);
        self.inner
            .borrow_mut()
            .module_sources
            .insert(path.clone(), source.to_owned());
        let dir = match path.rfind('/') {
            Some(i) => path[..i].to_owned(),
            None => "/".into(),
        };
        let r = self.vm.import_namespace(&path, &dir, url);
        match r {
            Ok(_) => {}
            Err(e) => self.report_error(e),
        }
        self.flush_console();
    }

    /// Evaluates a classic script in the realm's global scope.
    fn eval_step(&mut self, source: &str) -> Result<String, String> {
        self.record_input(Input::Eval(source.to_owned()));
        self.arm();
        let r = self.vm.eval_source_with(source, "eval", false, true);
        let out = match r {
            Ok(v) => match &v {
                Value::Str(s) => Ok(s.to_string()),
                Value::Undefined => Ok(String::new()),
                other => self
                    .vm
                    .inspect_default(other)
                    .map_err(|_| "inspect failed".to_owned()),
            },
            Err(e) => {
                let text = self.error_text(&e);
                self.report_error(e);
                Err(text)
            }
        };
        self.drain_microtasks();
        self.flush_console();
        out
    }

    fn exec(&mut self, source: &str, name: &str) {
        let r = self.vm.eval_source_with(source, name, false, true);
        if let Err(e) = r {
            self.report_error(e);
        }
        self.flush_console();
    }

    /// Reports an uncaught exception: `error` on `window`, then `console.error`.
    fn report_error(&mut self, e: Ctl) {
        match e {
            Ctl::Throw(v) => {
                let _ = self.call_hook_result("uncaught", vec![v]);
            }
            Ctl::Fatal(v) => {
                let text = self.error_text(&Ctl::Fatal(v));
                self.inner
                    .borrow_mut()
                    .log(LogLevel::Error, &format!("Fatal: {text}"));
            }
            Ctl::Exit(_) => {}
        }
        self.flush_console();
    }

    fn wrap(&mut self, node: NodeId) -> Value {
        bindings::dom::wrap_node(&mut self.vm, node)
    }

    /// Calls a prelude hook (`%hooks.<name>`), reporting an exception it throws.
    fn call_hook(&mut self, name: &str, args: Vec<Value>) -> Value {
        match self.call_hook_result(name, args) {
            Ok(v) => v,
            Err(e) => {
                self.report_error(e);
                Value::Undefined
            }
        }
    }

    fn call_hook_result(&mut self, name: &str, args: Vec<Value>) -> Result<Value, Ctl> {
        let hooks = self.vm.global.own_value("%hooks");
        let Some(hooks) = hooks else {
            return Ok(Value::Undefined);
        };
        let f = self.vm.get_str(&hooks, name)?;
        if !f.is_callable() {
            return Ok(Value::Undefined);
        }
        self.vm.call(&f, hooks, args)
    }

    /// Drains microtasks (and reports unhandled rejections through
    /// `unhandledrejection`).
    fn drain_microtasks(&mut self) {
        loop {
            let r = self.vm.run_microtasks();
            if let Err(e) = r {
                self.report_error(e);
                continue;
            }
            let pending = std::mem::take(&mut self.vm.pending_rejections);
            let mut any = false;
            for p in pending {
                let (st, val) = match self.vm.promise_state(&p) {
                    Some(s) => s,
                    None => continue,
                };
                let handled =
                    matches!(&p.borrow().kind, cw_jsvm::value::Kind::Promise(pd) if pd.handled);
                if st != cw_jsvm::value::PromiseState::Rejected || handled {
                    continue;
                }
                any = true;
                let _ = self.call_hook_result("unhandledRejection", vec![val, Value::Obj(p)]);
            }
            if !any {
                break;
            }
        }
        self.flush_console();
    }

    fn flush_console(&mut self) {
        let out = std::mem::take(&mut self.vm.stdout);
        let err = std::mem::take(&mut self.vm.stderr);
        let mut inner = self.inner.borrow_mut();
        for line in out.split_inclusive('\n') {
            inner.log(LogLevel::Log, line.trim_end_matches('\n'));
        }
        for line in err.split_inclusive('\n') {
            inner.log(LogLevel::Error, line.trim_end_matches('\n'));
        }
    }

    /// Syncs the VM's virtual clock to the world clock.
    fn sync_clock(&mut self) {
        let now = self.inner.borrow_mut().host_now_micros();
        let start = self.vm.start_micros;
        let world_ms = (now - start) as f64 / 1000.0;
        let cur = self.vm.clock();
        if world_ms > cur {
            self.vm.elapsed_ms = world_ms;
        }
    }

    /// Drains microtasks, the timers due on the world clock, `MessagePort` tasks and
    /// queued script tasks; then, while `advance_ms` of virtual time remain, advances
    /// the clock to the next timer and fires it (`requestAnimationFrame` callbacks
    /// run once per 16ms of advancement). Returns true when work ran.
    fn run_until_idle_step(&mut self, advance_ms: u32) -> bool {
        self.record_input(Input::RunUntilIdle { advance_ms });
        self.arm();
        self.sync_clock();
        let mut ran = false;
        let deadline = self.vm.clock() + advance_ms as f64;
        let mut next_frame = self.vm.clock() + 16.0;
        loop {
            ran |= self.run_due();
            let next = self
                .vm
                .timers
                .iter()
                .filter(|t| !t.immediate)
                .map(|t| t.when)
                .fold(None, |m: Option<f64>, w| Some(m.map_or(w, |m| m.min(w))));
            let has_frames = self.inner.borrow().has_raf;
            let mut target = match next {
                Some(w) if w <= deadline => w,
                _ if has_frames && next_frame <= deadline => next_frame,
                _ => break,
            };
            if has_frames && next_frame < target {
                target = next_frame;
            }
            let cur = self.vm.clock();
            if target > cur {
                self.vm.elapsed_ms = target;
            }
            if has_frames && target >= next_frame {
                self.animation_frame_inner();
                next_frame = target + 16.0;
                ran = true;
            }
        }
        // Content that moved under a still pointer takes `:hover` with it (and
        // whatever its boundary events set off runs too).
        for _ in 0..4 {
            if !bindings::events::refresh_hover(self) {
                break;
            }
            self.run_due();
            ran = true;
        }
        self.flush_console();
        // Idle: a safe point to reclaim cyclic garbage (see `cw_jsvm::gc`).
        cw_jsvm::gc::maybe_collect();
        ran
    }

    /// Fires everything due right now (no clock advance).
    fn run_due(&mut self) -> bool {
        let mut ran = false;
        loop {
            self.drain_microtasks();
            let now = self.vm.clock();
            let mut fired = false;
            while let Some(i) = self.vm.next_due_timer(now) {
                let r = self.vm.fire_timer(i);
                if let Err(e) = r {
                    self.report_error(e);
                }
                self.drain_microtasks();
                fired = true;
                ran = true;
            }
            // Immediates (`setImmediate`, used internally for port tasks).
            let ids: Vec<u64> = self
                .vm
                .timers
                .iter()
                .filter(|t| t.immediate)
                .map(|t| t.id)
                .collect();
            for id in ids {
                if let Some(i) = self.vm.timers.iter().position(|t| t.id == id) {
                    let r = self.vm.fire_timer(i);
                    if let Err(e) = r {
                        self.report_error(e);
                    }
                    self.drain_microtasks();
                    fired = true;
                    ran = true;
                }
            }
            let pending_scripts = std::mem::take(&mut self.inner.borrow_mut().pending_scripts);
            for s in pending_scripts {
                self.run_script_element(s, true);
                fired = true;
                ran = true;
            }
            // Layout observers see each new layout (the browser also calls
            // `after_layout` when it paints).
            let observe = {
                let mut i = self.inner.borrow_mut();
                if i.has_layout_observers {
                    i.ensure_layout();
                    i.generation != i.observed_generation
                } else {
                    false
                }
            };
            if observe {
                {
                    let mut i = self.inner.borrow_mut();
                    i.observed_generation = i.generation;
                }
                self.call_hook("afterLayout", vec![]);
                self.drain_microtasks();
                fired = true;
                ran = true;
            }
            if self.pump_animations() {
                fired = true;
                ran = true;
            }
            if !fired {
                break;
            }
        }
        ran
    }

    /// Turns the transitions and animations the last style flush started into DOM
    /// events on the world clock: `transitionrun`/`transitionstart`/`transitionend`
    /// and `animationstart`/`animationend`, each scheduled with the realm's timers so
    /// it lands `transition-delay` and `transition-duration` later. The engine does
    /// not interpolate; the property jumps to its new value and the end event arrives
    /// when the declared time is up, which is what `<transition>` in Vue and
    /// `svelte/transition` wait for.
    fn pump_animations(&mut self) -> bool {
        {
            let mut i = self.inner.borrow_mut();
            if i.doc.document_element().is_some() {
                i.ensure_styles();
            }
        }
        let starts = std::mem::take(&mut self.inner.borrow_mut().pending_animations);
        if starts.is_empty() {
            return false;
        }
        for a in starts {
            let target = self.wrap(a.node);
            let hook = if a.is_animation {
                "cssAnimation"
            } else {
                "cssTransition"
            };
            self.call_hook(
                hook,
                vec![
                    target,
                    Value::str(&a.name),
                    Value::Num(a.delay_ms as f64),
                    Value::Num(a.duration_ms as f64),
                    a.iterations.map(Value::Num).unwrap_or(Value::Null),
                    Value::Bool(a.cancelled),
                ],
            );
        }
        self.drain_microtasks();
        true
    }

    /// Runs the `requestAnimationFrame` callbacks for one painted frame, then drains
    /// microtasks. The browser calls this once per frame it paints.
    fn animation_frame_step(&mut self) {
        self.record_input(Input::AnimationFrame);
        self.arm();
        self.sync_clock();
        self.animation_frame_inner();
        self.flush_console();
    }

    fn animation_frame_inner(&mut self) {
        let t = self.vm.perf_now();
        self.call_hook("animationFrame", vec![Value::Num(t)]);
        self.drain_microtasks();
        if bindings::events::refresh_hover(self) {
            self.drain_microtasks();
        }
    }

    /// Delivers `ResizeObserver` and `IntersectionObserver` records against the
    /// current layout. The browser calls this after it laid out and painted.
    fn after_layout_step(&mut self) {
        self.record_input(Input::AfterLayout);
        self.arm();
        {
            let mut i = self.inner.borrow_mut();
            i.ensure_layout();
            i.observed_generation = i.generation;
        }
        self.call_hook("afterLayout", vec![]);
        self.drain_microtasks();
        self.flush_console();
    }

    /// Dispatches a browser action as DOM events and reports the default action.
    fn dispatch_step(&mut self, event: UiEvent) -> DefaultAction {
        self.record_input(Input::Dispatch(event.clone()));
        self.arm();
        self.sync_clock();
        let action = bindings::events::dispatch(self, event);
        self.drain_microtasks();
        self.pump_animations();
        self.flush_console();
        cw_jsvm::gc::maybe_collect();
        self.inner.borrow_mut().host.request_relayout();
        action
    }

    pub(crate) fn vm(&mut self) -> &mut Vm<'static> {
        &mut self.vm
    }
}

// ---------------------------------------------------------------- templates

/// A realm's VM just after the prelude, as a heap snapshot (see
/// `Realm::from_template`).
struct Template {
    heap: Vec<u8>,
    sources: Vec<Rc<str>>,
    /// Names of the prelude's prototypes, and how many node wrappers it made: the
    /// heap roots, wrappers first.
    protos: Vec<String>,
    wrappers: usize,
}

thread_local! {
    static TEMPLATE: RefCell<Option<Rc<Template>>> = const { RefCell::new(None) };
    static TEMPLATES: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Whether new realms on this thread start from a template of the booted prelude
/// (on by default) or run the prelude themselves. Either way they are the same
/// realm; the template is made by running it once, on first use.
pub fn set_realm_templates(on: bool) {
    TEMPLATES.with(|t| t.set(on));
}

fn template() -> Option<Rc<Template>> {
    if !TEMPLATES.with(|t| t.get()) {
        return None;
    }
    if let Some(t) = TEMPLATE.with(|t| t.borrow().clone()) {
        return Some(t);
    }
    TEMPLATES.with(|t| t.set(false));
    let r = Realm::new("", "about:blank", Box::new(MemoryHost::new()));
    TEMPLATES.with(|t| t.set(true));
    let (_, roots) = r.inner.borrow().image().ok()?;
    let sources = RefCell::new(Vec::new());
    let mut opts = bindings::snapshot_options();
    opts.sources_out = Some(&sources);
    let heap = r.vm.heap_snapshot(&roots, opts).ok()?;
    let (protos, wrappers) = {
        let i = r.inner.borrow();
        (i.protos.keys().cloned().collect(), i.wrappers.len())
    };
    let t = Rc::new(Template {
        heap,
        sources: sources.into_inner(),
        protos,
        wrappers,
    });
    TEMPLATE.with(|s| *s.borrow_mut() = Some(t.clone()));
    Some(t)
}

// ---------------------------------------------------------------- entry points

thread_local! {
    static VERIFY_SNAPSHOTS: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    /// Entry points checked, and entry points not checked because the realm could
    /// not be imaged, on this thread.
    static VERIFIED: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// How many entry points the snapshot check has checked on this thread, and how
/// many it skipped because the realm could not be written (see `Realm::heap_image`).
pub fn verified_snapshots() -> (u64, u64) {
    VERIFIED.with(|v| v.get())
}

/// Turns the snapshot check on or off for this thread (it starts on when the
/// environment has `CW_WEB_VERIFY_SNAPSHOTS=1`): every entry point of every realm
/// then writes the realm's heap image before it runs, restores a second realm from
/// it, runs the same entry point there (answering its host calls from the first
/// realm's journal), and panics unless both return the same and write the same
/// image afterwards.
pub fn set_verify_snapshots(on: bool) {
    VERIFY_SNAPSHOTS.with(|v| v.set(Some(on)));
}

fn verify_snapshots() -> bool {
    VERIFY_SNAPSHOTS.with(|v| match v.get() {
        Some(on) => on,
        None => {
            let on = std::env::var("CW_WEB_VERIFY_SNAPSHOTS").is_ok_and(|v| v == "1");
            v.set(Some(on));
            on
        }
    })
}

/// A realm's image before an entry point, for the snapshot check.
struct Verify {
    image: std::sync::Arc<ImageData>,
    state: RealmState,
    journal_at: usize,
}

impl Realm {
    /// Parses the page, running its scripts as the parser reaches them (`defer`
    /// after the parse, `async` after that, modules through the VM's loader), then
    /// fires `DOMContentLoaded` and `load`.
    pub fn run_document(&mut self) {
        let v = self.verify_begin();
        self.run_document_step();
        self.verify_end(v, Input::RunDocument, String::new());
    }

    /// Evaluates a classic script in the realm's global scope.
    pub fn eval(&mut self, source: &str) -> Result<String, String> {
        let v = self.verify_begin();
        let r = self.eval_step(source);
        self.verify_end(v, Input::Eval(source.to_owned()), format!("{r:?}"));
        r
    }

    /// Drains microtasks and runs the timers due within `advance_ms` of the world
    /// clock; returns whether anything ran.
    pub fn run_until_idle(&mut self, advance_ms: u32) -> bool {
        let v = self.verify_begin();
        let r = self.run_until_idle_step(advance_ms);
        self.verify_end(v, Input::RunUntilIdle { advance_ms }, format!("{r}"));
        r
    }

    /// Runs the `requestAnimationFrame` callbacks for one painted frame.
    pub fn animation_frame(&mut self) {
        let v = self.verify_begin();
        self.animation_frame_step();
        self.verify_end(v, Input::AnimationFrame, String::new());
    }

    /// Delivers `ResizeObserver` and `IntersectionObserver` records after a layout.
    pub fn after_layout(&mut self) {
        let v = self.verify_begin();
        self.after_layout_step();
        self.verify_end(v, Input::AfterLayout, String::new());
    }

    /// Turns a browser action into its DOM event sequence; returns the default
    /// action left for the browser.
    pub fn dispatch(&mut self, event: UiEvent) -> DefaultAction {
        let v = self.verify_begin();
        let r = self.dispatch_step(event.clone());
        self.verify_end(v, Input::Dispatch(event), format!("{r:?}"));
        r
    }

    /// Tells layout the intrinsic sizes of pictures the browser fetched, keyed by
    /// the `src` as written. Recorded as an input, so a restore sees the same layout.
    pub fn set_image_sizes(&mut self, sizes: Vec<(String, u32, u32)>) {
        let v = self.verify_begin();
        self.set_image_sizes_step(sizes.clone());
        self.verify_end(v, Input::ImageSizes(sizes), String::new());
    }

    fn verify_begin(&self) -> Option<Verify> {
        if self.verifying || !verify_snapshots() {
            return None;
        }
        let inner = self.inner.borrow();
        if inner.journal.replaying || !inner.journal.recording {
            return None;
        }
        let journal_at = inner.journal.entries.len();
        drop(inner);
        // A realm that cannot be imaged is not checked (but counted).
        let image = self.heap_image();
        VERIFIED.with(|v| {
            let (a, b) = v.get();
            v.set(if image.is_ok() {
                (a + 1, b)
            } else {
                (a, b + 1)
            })
        });
        let image = image.ok()?;
        let state = self.snapshot_state();
        Some(Verify {
            image: std::sync::Arc::new(ImageData {
                bytes: image,
                sources: Vec::new(),
                inputs: state.inputs.len(),
                journal: journal_at,
            }),
            state,
            journal_at,
        })
    }

    fn verify_end(&mut self, v: Option<Verify>, input: Input, out: String) {
        let Some(v) = v else {
            return;
        };
        let mut shadow = match Realm::from_image(&v.state, &v.image, Box::new(MemoryHost::new())) {
            Ok(r) => r,
            Err((e, _)) => panic!("snapshot check: the image does not restore: {e}"),
        };
        shadow.verifying = true;
        {
            let entries = self.inner.borrow().journal.entries.clone();
            let mut j = Journal::replay(entries);
            j.replay_pos = v.journal_at;
            shadow.inner.borrow_mut().journal = j;
        }
        let what = format!("{input:?}");
        let what = &what[..what.len().min(120)];
        let out2 = match input {
            Input::RunDocument => {
                shadow.run_document_step();
                String::new()
            }
            Input::RunUntilIdle { advance_ms } => {
                format!("{}", shadow.run_until_idle_step(advance_ms))
            }
            Input::Dispatch(ev) => format!("{:?}", shadow.dispatch_step(ev)),
            Input::AfterLayout => {
                shadow.after_layout_step();
                String::new()
            }
            Input::AnimationFrame => {
                shadow.animation_frame_step();
                String::new()
            }
            Input::Eval(src) => format!("{:?}", shadow.eval_step(&src)),
            Input::ImageSizes(sizes) => {
                shadow.set_image_sizes_step(sizes);
                String::new()
            }
        };
        assert_eq!(
            out, out2,
            "snapshot check: {what} returned differently after a restore"
        );
        {
            let j = &shadow.inner.borrow().journal;
            assert_eq!(
                j.replay_pos,
                j.entries.len(),
                "snapshot check: {what} asked the host differently after a restore"
            );
        }
        let a = self.heap_image();
        let b = shadow.heap_image();
        match (a, b) {
            (Ok(a), Ok(b)) => {
                if a != b {
                    panic!(
                        "snapshot check: {what} left a different realm after a restore: {}",
                        image_difference(&a, &b)
                    );
                }
            }
            (Err(_), Err(_)) => {}
            (a, b) => panic!(
                "snapshot check: {what}: one realm images and the other does not ({:?} / {:?})",
                a.err(),
                b.err()
            ),
        }
    }
}

/// Where two realm images first differ, for the snapshot check's report.
fn image_difference(a: &[u8], b: &[u8]) -> String {
    let (Ok(x), Ok(y)) = (RealmImage::read(a), RealmImage::read(b)) else {
        return "undecodable".into();
    };
    let x = RealmImage {
        heap: x.0.to_vec(),
        inner: x.1,
    };
    let y = RealmImage {
        heap: y.0.to_vec(),
        inner: y.1,
    };
    let first = |p: &[u8], q: &[u8]| p.iter().zip(q).position(|(m, n)| m != n);
    if x.heap != y.heap {
        return format!(
            "heaps differ ({} / {} bytes) from byte {:?}",
            x.heap.len(),
            y.heap.len(),
            first(&x.heap, &y.heap)
        );
    }
    let xi = serde_json::to_value(&x.inner).unwrap_or_default();
    let yi = serde_json::to_value(&y.inner).unwrap_or_default();
    if let (serde_json::Value::Object(xm), serde_json::Value::Object(ym)) = (&xi, &yi) {
        for (k, v) in xm {
            if ym.get(k) != Some(v) {
                let s = |v: Option<&serde_json::Value>| {
                    let t = v.map(|v| v.to_string()).unwrap_or_default();
                    t[..t.len().min(300)].to_owned()
                };
                return format!("`{k}` differs: {} / {}", s(Some(v)), s(ym.get(k)));
            }
        }
    }
    "the realm states differ".into()
}

/// Parses the page and runs its scripts; see [`Realm::run_document`].
pub fn run_document(realm: &mut Realm) {
    realm.run_document();
}

/// Drains the event loop; see [`Realm::run_until_idle`].
pub fn run_until_idle(realm: &mut Realm, advance_ms: u32) -> bool {
    realm.run_until_idle(advance_ms)
}

/// Dispatches a browser action; see [`Realm::dispatch`].
pub fn dispatch(realm: &mut Realm, event: UiEvent) -> DefaultAction {
    realm.dispatch(event)
}

/// The pixels of a `<canvas>` the page drew on; see [`Realm::canvas_pixels`].
pub fn canvas_pixels(realm: &Realm, node: NodeId) -> Option<RgbaImage> {
    realm.canvas_pixels(node)
}
