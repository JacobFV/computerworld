//! What runs a scripted document: the JS `Realm`, or a compiled TSX app (`cw_ui`).
//!
//! A page declares a compiled app by giving the script that loads its React
//! fallback a `data-cw-ui` attribute naming the app's IR:
//!
//! ```html
//! <script src="/vendor/react.production.min.js"></script>
//! <script src="/vendor/react-dom.production.min.js"></script>
//! <script src="app.js" data-cw-ui="app.ui.json"></script>
//! ```
//!
//! Chrome ignores the attribute and runs React. This browser fetches the IR; when it
//! parses and its version is the runtime's, the page runs on `cw_ui` and none of its
//! scripts run; otherwise (no IR, a 404, another version, a page with more than one
//! app) it runs on the Realm as any other page. See docs/tsx-apps.md.
//!
//! `PageScript` has the methods the browser calls on a realm, under the same names,
//! so the drivers treat both alike; a compiled app has no `requestAnimationFrame`,
//! observers or same-document history of its own, and those calls are no-ops.

use std::cell::Ref;
use std::ops::Deref;

use cw_ui::{UiApp, UiState};
use cw_web::dom::{Document, NodeId};
use cw_web::layout::FragmentTree;
use cw_web::paint::RgbaImage;
use cw_web::script::{DefaultAction, Inner, Realm, RealmState, ScriptHostDocument, UiEvent};

/// The attribute a page's app script carries to name its compiled IR.
pub const UI_ATTRIBUTE: &str = "data-cw-ui";

pub enum PageScript {
    Js(Box<Realm>),
    Ui(Box<UiApp>),
}

/// A scripted document's serialisable state.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptState {
    Js(RealmState),
    Ui(Box<UiState>),
}

/// A borrow of the engine state behind the document.
pub enum InnerRef<'a> {
    Js(Ref<'a, Inner>),
    Ui(&'a Inner),
}

impl Deref for InnerRef<'_> {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        match self {
            InnerRef::Js(r) => r,
            InnerRef::Ui(r) => r,
        }
    }
}

pub enum DocRef<'a> {
    Js(Ref<'a, Document>),
    Ui(&'a Document),
}

impl Deref for DocRef<'_> {
    type Target = Document;
    fn deref(&self) -> &Document {
        match self {
            DocRef::Js(r) => r,
            DocRef::Ui(r) => r,
        }
    }
}

pub enum TreeRef<'a> {
    Js(Ref<'a, FragmentTree>),
    Ui(&'a FragmentTree),
}

impl Deref for TreeRef<'_> {
    type Target = FragmentTree;
    fn deref(&self) -> &FragmentTree {
        match self {
            TreeRef::Js(r) => r,
            TreeRef::Ui(r) => r,
        }
    }
}

/// The IR a page names with `data-cw-ui`, resolved against `base`, if it names
/// exactly one.
pub fn declared_ui(doc: &Document, base: &str) -> Option<String> {
    let mut found = doc
        .descendants(Document::ROOT)
        .filter(|n| doc.is(*n, "script"))
        .filter_map(|n| doc.attr(n, UI_ATTRIBUTE));
    let first = found.next()?.trim().to_owned();
    if found.next().is_some() || first.is_empty() {
        return None;
    }
    url::Url::parse(base)
        .ok()?
        .join(&first)
        .ok()
        .map(|u| u.to_string())
}

impl PageScript {
    pub fn is_compiled(&self) -> bool {
        matches!(self, PageScript::Ui(_))
    }

    /// The compiled app, when that is what runs the page.
    pub fn ui(&mut self) -> Option<&mut UiApp> {
        match self {
            PageScript::Ui(app) => Some(app),
            PageScript::Js(_) => None,
        }
    }

    /// Runs the page: the realm parses it and runs its scripts; a compiled app
    /// renders.
    pub fn run_document(&mut self) {
        match self {
            PageScript::Js(r) => r.run_document(),
            PageScript::Ui(a) => a.boot(),
        }
    }

    pub fn dispatch(&mut self, event: UiEvent) -> DefaultAction {
        match self {
            PageScript::Js(r) => r.dispatch(event),
            PageScript::Ui(a) => a.dispatch(event),
        }
    }

    pub fn run_until_idle(&mut self, advance_ms: u32) -> bool {
        match self {
            PageScript::Js(r) => r.run_until_idle(advance_ms),
            PageScript::Ui(a) => a.run_until_idle(advance_ms),
        }
    }

    pub fn after_layout(&mut self) {
        if let PageScript::Js(r) = self {
            r.after_layout();
        }
    }

    pub fn animation_frame(&mut self) {
        match self {
            PageScript::Js(r) => r.animation_frame(),
            PageScript::Ui(a) => a.animation_frame(),
        }
    }

    pub fn wants_animation_frame(&self) -> bool {
        match self {
            PageScript::Js(r) => r.wants_animation_frame(),
            PageScript::Ui(a) => a.wants_animation_frame(),
        }
    }

    /// Styles and layout flushed: what paint and hit testing read.
    pub fn layout(&mut self) -> InnerRef<'_> {
        match self {
            PageScript::Js(r) => InnerRef::Js(r.layout()),
            PageScript::Ui(a) => {
                a.inner().ensure_layout();
                InnerRef::Ui(a.inner())
            }
        }
    }

    pub fn fragment_tree(&mut self) -> TreeRef<'_> {
        match self {
            PageScript::Js(r) => TreeRef::Js(r.fragment_tree()),
            PageScript::Ui(a) => TreeRef::Ui(a.fragment_tree()),
        }
    }

    pub fn document(&self) -> DocRef<'_> {
        match self {
            PageScript::Js(r) => DocRef::Js(r.document()),
            PageScript::Ui(a) => DocRef::Ui(a.document()),
        }
    }

    pub fn set_image_sizes(&mut self, sizes: Vec<(String, u32, u32)>) {
        match self {
            PageScript::Js(r) => r.set_image_sizes(sizes),
            PageScript::Ui(a) => {
                let inner = a.inner();
                for (src, w, h) in sizes {
                    inner.images.0.insert(src, (w, h));
                }
                inner.touch();
            }
        }
    }

    pub fn title(&self) -> String {
        match self {
            PageScript::Js(r) => r.title(),
            PageScript::Ui(a) => a.title(),
        }
    }

    pub fn url(&self) -> String {
        match self {
            PageScript::Js(r) => r.url(),
            PageScript::Ui(a) => a.url(),
        }
    }

    pub fn focused(&self) -> Option<NodeId> {
        match self {
            PageScript::Js(r) => r.focused(),
            PageScript::Ui(a) => a.focused(),
        }
    }

    /// Evaluates script in the page. A compiled app has no script to evaluate.
    pub fn eval(&mut self, source: &str) -> Result<String, String> {
        match self {
            PageScript::Js(r) => r.eval(source),
            PageScript::Ui(_) => Err("a compiled app evaluates no script".into()),
        }
    }

    /// `form.requestSubmit()`: validation, `submit`, and the submission unless a
    /// handler prevented it.
    pub fn request_submit(&mut self, form: NodeId, form_index: usize) -> DefaultAction {
        match self {
            PageScript::Js(r) => {
                let _ = r.eval(&format!("document.forms[{form_index}].requestSubmit()"));
                DefaultAction::None
            }
            PageScript::Ui(a) => a.request_submit(form),
        }
    }

    pub fn canvases(&self) -> Vec<(NodeId, RgbaImage)> {
        match self {
            PageScript::Js(r) => r.canvases(),
            PageScript::Ui(_) => Vec::new(),
        }
    }

    pub fn next_timer_micros(&self) -> Option<i64> {
        match self {
            PageScript::Js(r) => r.next_timer_micros(),
            PageScript::Ui(a) => a.next_timer_micros(),
        }
    }

    pub fn history_position(&self) -> (usize, usize) {
        match self {
            PageScript::Js(r) => r.history_position(),
            PageScript::Ui(_) => (0, 1),
        }
    }

    pub fn set_step_budget(&mut self, steps: u64) {
        if let PageScript::Js(r) = self {
            r.set_step_budget(steps);
        }
    }

    pub fn snapshot(&self) -> ScriptState {
        match self {
            PageScript::Js(r) => ScriptState::Js(r.snapshot()),
            PageScript::Ui(a) => ScriptState::Ui(Box::new(a.snapshot())),
        }
    }

    /// Rebuilds from a snapshot: a realm by replay, a compiled app by decoding.
    pub fn restore(state: &ScriptState, host: Box<dyn ScriptHostDocument>) -> PageScript {
        match state {
            ScriptState::Js(s) => PageScript::Js(Box::new(Realm::restore(s, host))),
            ScriptState::Ui(s) => match UiApp::restore(s, host) {
                Ok(a) => PageScript::Ui(Box::new(a)),
                // A state this build cannot decode (another IR version) is a page
                // that renders nothing, not a crash.
                Err(_) => PageScript::Js(Box::new(Realm::new(
                    "",
                    "about:blank",
                    Box::new(cw_web::script::MemoryHost::new()),
                ))),
            },
        }
    }

    /// How many inputs a restore replays (0 for a compiled app, which does not).
    pub fn journal_len(state: &ScriptState) -> usize {
        match state {
            ScriptState::Js(s) => s.inputs.len(),
            ScriptState::Ui(_) => 0,
        }
    }
}
