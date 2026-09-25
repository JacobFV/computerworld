//! `cw-ui`: runs React-syntax apps compiled by `cw-tsx` without a JS VM.
//!
//! A [`UiApp`] is one mounted app: the IR (`ir::Module`) instantiated onto a
//! `cw_web` document parsed from the page's HTML shell. The document, its styles,
//! layout, hit testing, focus and form state are the engine's own (`cw_web::script::
//! Inner`, the same state a JS `Realm` keeps); only the script layer is replaced.
//! Updates follow React 18: state updates inside an event are batched and rendered
//! when its dispatch ends, layout effects run before passive effects, and a keyed
//! list keeps each item's state and DOM through reorders.
//!
//! The host is the Realm's own [`ScriptHostDocument`] (network, world clock,
//! entropy, storage, console), so anything that can host a Realm can host an app.
//!
//! Snapshots are the state itself ([`UiState`]: document, component tree, hook
//! values, handlers, timers), serialised with shared values kept shared; restoring
//! deserialises it, with no replay.
//!
//! ```ignore
//! let mut app = UiApp::new(module, html, "https://site.test/", Box::new(host))?;
//! app.boot();
//! app.dispatch(UiEvent::Click { x, y, button: 0, modifiers, detail: 1 });
//! let state = app.snapshot();
//! let again = UiApp::restore(&state, Box::new(host))?;
//! ```

pub mod ir;
pub mod value;

mod asyncfn;
mod cw;
mod dom;
mod events;
pub mod gen;
mod geometry;
mod history;
mod interp;
pub mod island;
mod json;
pub mod program;
mod render;
mod runtime;
mod snapshot;

use std::rc::Rc;

use cw_web::dom::{Document, NodeId};
use cw_web::layout::FragmentTree;
use cw_web::script::{DefaultAction, Inner, LogEntry, ScriptHostDocument, UiEvent};
use cw_web::style::StyleSet;

pub use program::{GenProgram, IrProgram, Program, ProgramId};
pub use runtime::Stats;
pub use snapshot::UiState;

use crate::runtime::Runtime;
use crate::value::{Closure, Value};

/// The DOM attribute React writes for a host-element prop (`className` → `class`).
/// Names React passes through keep their spelling; the document lower-cases HTML
/// attribute names itself.
pub fn dom_attr_name(prop: &str) -> String {
    match prop {
        "className" => "class".into(),
        "htmlFor" => "for".into(),
        "acceptCharset" => "accept-charset".into(),
        "httpEquiv" => "http-equiv".into(),
        // SVG presentation attributes React spells in camelCase.
        "strokeWidth" => "stroke-width".into(),
        "strokeLinecap" => "stroke-linecap".into(),
        "strokeLinejoin" => "stroke-linejoin".into(),
        "strokeDasharray" => "stroke-dasharray".into(),
        "strokeDashoffset" => "stroke-dashoffset".into(),
        "strokeOpacity" => "stroke-opacity".into(),
        "fillOpacity" => "fill-opacity".into(),
        "fillRule" => "fill-rule".into(),
        "clipRule" => "clip-rule".into(),
        "clipPath" => "clip-path".into(),
        "fontSize" => "font-size".into(),
        "fontFamily" => "font-family".into(),
        "fontWeight" => "font-weight".into(),
        "textAnchor" => "text-anchor".into(),
        "dominantBaseline" => "dominant-baseline".into(),
        "stopColor" => "stop-color".into(),
        "stopOpacity" => "stop-opacity".into(),
        "xlinkHref" => "xlink:href".into(),
        n => n.to_owned(),
    }
}

/// Why an app could not be created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiError {
    /// The IR is from another compiler version.
    Version(u32),
    /// The IR does not parse.
    Ir(String),
    /// The module renders nowhere, or its container is not in the page.
    NoContainer(String),
    /// A snapshot does not decode.
    State(String),
}

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UiError::Version(v) => write!(f, "IR version {v}, runtime expects {}", ir::IR_VERSION),
            UiError::Ir(e) => write!(f, "IR does not parse: {e}"),
            UiError::NoContainer(id) => write!(f, "no element #{id} to render into"),
            UiError::State(e) => write!(f, "snapshot does not decode: {e}"),
        }
    }
}

/// One mounted app. See the crate documentation.
pub struct UiApp {
    rt: Runtime,
}

impl UiApp {
    /// Parses the IR from its JSON form (`<name>.ui.json`).
    pub fn parse_ir(json: &str) -> Result<ir::Module, UiError> {
        let m: ir::Module = serde_json::from_str(json).map_err(|e| UiError::Ir(e.to_string()))?;
        if m.version != ir::IR_VERSION {
            return Err(UiError::Version(m.version));
        }
        Ok(m)
    }

    /// An app for `module` in the page `html` (its HTML shell: styles and the
    /// container element; its scripts are not run). Nothing renders until `boot`.
    pub fn new(
        module: ir::Module,
        html: &str,
        url: &str,
        host: Box<dyn ScriptHostDocument>,
    ) -> Result<UiApp, UiError> {
        let doc = cw_web::html::parse(html);
        Self::with_document(module, doc, url, host)
    }

    /// An app rendering into an existing document (a desktop host builds its own).
    pub fn with_document(
        module: ir::Module,
        doc: Document,
        url: &str,
        host: Box<dyn ScriptHostDocument>,
    ) -> Result<UiApp, UiError> {
        if module.version != ir::IR_VERSION {
            return Err(UiError::Version(module.version));
        }
        Self::with_program(Rc::new(program::IrProgram::new(module)), doc, url, host)
    }

    /// An app running a generated program (`cw-tsx build --emit rust`) in the page
    /// `html`.
    pub fn generated(
        program: &'static program::GenProgram,
        html: &str,
        url: &str,
        host: Box<dyn ScriptHostDocument>,
    ) -> Result<UiApp, UiError> {
        let doc = cw_web::html::parse(html);
        Self::with_program(Rc::new(program::StaticProgram(program)), doc, url, host)
    }

    /// An app running `program`, interpreted or generated, in a document.
    pub fn with_program(
        program: Rc<dyn Program>,
        mut doc: Document,
        url: &str,
        host: Box<dyn ScriptHostDocument>,
    ) -> Result<UiApp, UiError> {
        let Some(container_id) = program.container_id() else {
            return Err(UiError::NoContainer(String::new()));
        };
        let Some(container) = doc.by_id(container_id).first().copied() else {
            return Err(UiError::NoContainer(container_id.to_owned()));
        };
        doc.url = url.to_owned();
        let mut rt = Runtime::new(program, host, url);
        rt.inner.doc = doc;
        rt.inner.ready_state = "complete".into();
        rt.container = container;
        Ok(UiApp { rt })
    }

    /// Initialises the module and renders it: globals in order, the root element
    /// into its container, then layout and passive effects.
    pub fn boot(&mut self) {
        let rt = &mut self.rt;
        if rt.booted {
            return;
        }
        rt.booted = true;
        rt.start_micros = rt.inner.host_now_micros();
        let program = rt.program.clone();
        rt.globals = vec![Value::Undefined; program.globals_len()];
        // The island first: compiled globals initialise from its exports.
        if let Some(script) = program.island_script() {
            if let Err(e) = rt.start_island(script) {
                rt.report(e);
                rt.crashed = true;
                return;
            }
        }
        if let Err(e) = program.boot_globals(rt) {
            rt.report(e);
            rt.crashed = true;
            return;
        }
        let element = rt.call_closure(
            &Rc::new(Closure {
                func: program.root_element(),
                captures: Vec::new(),
            }),
            Vec::new(),
            None,
        );
        match element {
            Ok(v) => rt.mount_root(v),
            Err(e) => {
                rt.report(e);
                rt.crashed = true;
                return;
            }
        }
        rt.settle();
        rt.trim_journal();
    }

    /// Delivers a browser action and reports the default action left for the
    /// browser (a submission, a navigation, a focus change).
    pub fn dispatch(&mut self, ev: UiEvent) -> DefaultAction {
        let a = self.rt.dispatch_ui(ev);
        self.rt.settle();
        self.rt.trim_journal();
        a
    }

    /// `form.requestSubmit()`: the `submit` event through the app's handlers, then
    /// the submission unless one prevented it.
    pub fn request_submit(&mut self, form: NodeId) -> DefaultAction {
        let a = self.rt.submit_form(form, None);
        self.rt.settle();
        self.rt.trim_journal();
        a
    }

    /// Runs timers due on the world clock, advancing the virtual clock by up to
    /// `advance_ms` to reach later ones. Returns whether anything ran.
    pub fn run_until_idle(&mut self, advance_ms: u32) -> bool {
        let r = self.rt.run_timers(advance_ms);
        self.rt.trim_journal();
        r
    }

    /// World-clock microseconds of the earliest pending timer.
    pub fn next_timer_micros(&self) -> Option<i64> {
        self.rt
            .timers
            .iter()
            .map(|t| t.due)
            .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.min(d))))
            .map(|ms| self.rt.start_micros + (ms * 1000.0) as i64)
    }

    pub fn document(&self) -> &Document {
        &self.rt.inner.doc
    }

    /// The engine state behind the document: styles, layout, scroll, focus, form
    /// values. What a browser paints and hit-tests from.
    pub fn inner(&mut self) -> &mut Inner {
        &mut self.rt.inner
    }

    pub fn styles(&mut self) -> &StyleSet {
        self.rt.inner.ensure_styles();
        &self.rt.inner.styles
    }

    pub fn fragment_tree(&mut self) -> &FragmentTree {
        self.rt.inner.ensure_layout();
        self.rt.inner.tree.as_ref().expect("laid out")
    }

    pub fn focused(&self) -> Option<NodeId> {
        self.rt.inner.focused
    }

    pub fn hovered(&self) -> Option<NodeId> {
        self.rt.inner.hovered
    }

    pub fn form_values(&self) -> std::collections::BTreeMap<NodeId, String> {
        self.rt.inner.form.values.clone()
    }

    pub fn title(&self) -> String {
        self.rt.inner.title()
    }

    pub fn url(&self) -> String {
        self.rt.inner.url.clone()
    }

    /// Console output and uncaught errors so far.
    pub fn logs(&self) -> Vec<LogEntry> {
        self.rt.inner.logs.clone()
    }

    /// Render counters (renders, holes evaluated and skipped).
    pub fn stats(&self) -> Stats {
        self.rt.stats
    }

    /// The first element in document order matching a CSS selector.
    pub fn query_selector(&self, selector: &str) -> Option<NodeId> {
        let list = cw_web::css::selector::parse_selector_list(selector).ok()?;
        let ctx = cw_web::css::MatchContext::new();
        let doc = &self.rt.inner.doc;
        doc.descendants(Document::ROOT).find(|n| {
            doc.is_element(*n) && cw_web::css::matching::matches_list(doc, *n, &list, &ctx)
        })
    }

    /// The centre of an element's first box, in viewport pixels (where a click on
    /// it lands).
    pub fn centre_of(&mut self, node: NodeId) -> Option<(i32, i32)> {
        let rects = self.rt.inner.rects_of(node);
        let (sx, sy) = self.rt.inner.window_scroll();
        rects.first().map(|r| {
            (
                (r.origin.x - sx + r.size.width.scale(1, 2)).to_px_round(),
                (r.origin.y - sy + r.size.height.scale(1, 2)).to_px_round(),
            )
        })
    }

    /// Replies to the app's `cw` requests, as the JSON array bridge.js's
    /// `__cw_deliver` takes (`[{"id": 1, "value": …} | {"id": 2, "error": "…"}]`);
    /// the app then settles. See `crate::cw`.
    pub fn cw_deliver(&mut self, replies: &str) -> Result<(), UiError> {
        let r = self.rt.cw_deliver(replies).map_err(UiError::State);
        self.rt.trim_journal();
        r
    }

    /// The app's environment changed (bridge.js's `__cw_env`): `cw.env` becomes
    /// `env` (JSON) and every `cw.onEnv` listener runs; the app then settles. The host
    /// applies the theme to the document itself.
    pub fn cw_env(&mut self, env: &str) -> Result<(), UiError> {
        let r = self.rt.cw_env(env).map_err(UiError::State);
        self.rt.trim_journal();
        r
    }

    /// Whether the app declared its state through `cw.state.set`: then that state
    /// is what a host keeps for it, and booting it again with that state restores
    /// it, as on the JS backend.
    pub fn declares_state(&self) -> bool {
        self.rt.cw.declared
    }

    /// Whether a render threw and React's semantics unmounted the app.
    pub fn crashed(&self) -> bool {
        self.rt.crashed
    }

    /// The app's state, for a snapshot.
    pub fn snapshot(&self) -> UiState {
        snapshot::save(&self.rt)
    }

    /// Rebuilds an app from a snapshot; afterwards it is live on `host`. The program
    /// is the snapshot's own IR, or the registered generated program it names
    /// (`program::register`).
    pub fn restore(state: &UiState, host: Box<dyn ScriptHostDocument>) -> Result<UiApp, UiError> {
        let program = snapshot::own_program(state).map_err(UiError::State)?;
        Ok(UiApp {
            rt: snapshot::load(state, program, host).map_err(UiError::State)?,
        })
    }

    /// Rebuilds an app from a snapshot on `program`, which must be the same IR the
    /// snapshot was taken of, interpreted or generated: an interpreted app's
    /// snapshot restores on the generated program of its IR, and a generated app's
    /// on the interpreter given that IR.
    pub fn restore_with(
        state: &UiState,
        program: Rc<dyn Program>,
        host: Box<dyn ScriptHostDocument>,
    ) -> Result<UiApp, UiError> {
        snapshot::check_program(state, &*program).map_err(UiError::State)?;
        Ok(UiApp {
            rt: snapshot::load(state, program, host).map_err(UiError::State)?,
        })
    }

    /// Which program this app runs.
    pub fn program_id(&self) -> program::ProgramId {
        self.rt.program.id()
    }

    /// Whether this app runs generated code (else the interpreter).
    pub fn is_generated(&self) -> bool {
        self.rt.program.module().is_none()
    }
}
