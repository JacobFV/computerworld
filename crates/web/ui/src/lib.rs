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
mod dom;
mod events;
mod interp;
mod json;
mod render;
mod runtime;
mod snapshot;

use std::rc::Rc;

use cw_web::dom::{Document, NodeId};
use cw_web::layout::FragmentTree;
use cw_web::script::{DefaultAction, Inner, LogEntry, ScriptHostDocument, UiEvent};
use cw_web::style::StyleSet;

pub use runtime::Stats;
pub use snapshot::UiState;

use crate::interp::Frame;
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
        mut doc: Document,
        url: &str,
        host: Box<dyn ScriptHostDocument>,
    ) -> Result<UiApp, UiError> {
        if module.version != ir::IR_VERSION {
            return Err(UiError::Version(module.version));
        }
        let Some(root) = &module.root else {
            return Err(UiError::NoContainer(String::new()));
        };
        let Some(container) = doc.by_id(&root.container_id).first().copied() else {
            return Err(UiError::NoContainer(root.container_id.clone()));
        };
        doc.url = url.to_owned();
        let mut rt = Runtime::new(Rc::new(module), host, url);
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
        let module = rt.module.clone();
        rt.globals = vec![Value::Undefined; module.globals.len()];
        for (i, g) in module.globals.iter().enumerate() {
            match &g.init {
                ir::GlobalInit::Function(f) => {
                    rt.globals[i] = Value::Func(Rc::new(Closure {
                        func: *f,
                        captures: Vec::new(),
                    }));
                }
                ir::GlobalInit::Context(_) => rt.globals[i] = Value::Context(i as u32),
                _ => {}
            }
        }
        let mut frame = Frame::bare();
        for (i, g) in module.globals.iter().enumerate() {
            let r = match &g.init {
                ir::GlobalInit::Expr(e) => rt.eval(&mut frame, e).map(|v| rt.globals[i] = v),
                ir::GlobalInit::Context(e) => rt.eval(&mut frame, e).map(|v| {
                    rt.ctx_defaults.insert(i as u32, v);
                }),
                _ => Ok(()),
            };
            if let Err(e) = r {
                rt.report(e);
                rt.crashed = true;
                return;
            }
        }
        let root = module.root.as_ref().expect("checked in new");
        let element = rt.call_closure(
            &Rc::new(Closure {
                func: root.element,
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

    /// Whether a render threw and React's semantics unmounted the app.
    pub fn crashed(&self) -> bool {
        self.rt.crashed
    }

    /// The app's state, for a snapshot.
    pub fn snapshot(&self) -> UiState {
        snapshot::save(&self.rt)
    }

    /// Rebuilds an app from a snapshot; afterwards it is live on `host`.
    pub fn restore(state: &UiState, host: Box<dyn ScriptHostDocument>) -> Result<UiApp, UiError> {
        Ok(UiApp {
            rt: snapshot::load(state, host).map_err(UiError::State)?,
        })
    }
}
