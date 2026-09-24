//! The runtime's state: the module, the document (through `cw_web`'s script-free
//! `Inner`: DOM, stylesheets, cascade, layout, hit testing, focus and form state),
//! module globals, component instances with their hooks, the mounted tree, event
//! handlers, timers and the microtask queue.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use cw_web::dom::NodeId;
use cw_web::script::{Inner, Journal, LogLevel, ScriptHostDocument};

use crate::ir::Module;
use crate::value::*;

/// A thrown value, or the short-circuit of an optional chain.
#[derive(Debug)]
pub(crate) enum Throw {
    Value(Value),
    Short,
}

pub(crate) type R<T> = Result<T, Throw>;

pub(crate) fn type_error<T>(msg: impl Into<String>) -> R<T> {
    Err(Throw::Value(Value::error("TypeError", &msg.into())))
}

/// A thrown `Error` of the given name.
pub(crate) fn js_error<T>(name: &str, msg: impl Into<String>) -> R<T> {
    Err(Throw::Value(Value::error(name, &msg.into())))
}

/// The functions the runtime itself provides.
#[derive(Debug)]
pub enum NativeFn {
    /// `resolve`/`reject` of `new Promise(executor)`.
    Resolver {
        promise: Rc<RefCell<Promise>>,
        reject: bool,
    },
    /// Element `index` of a `Promise.all` settling.
    AllSlot {
        state: Rc<RefCell<AllState>>,
        index: usize,
    },
    AllReject(Rc<RefCell<AllState>>),
    /// The callback a `useSyncExternalStore` subscription is given.
    StoreChanged {
        inst: u32,
        hook: u32,
    },
    /// An async function resumed after an `await` (with the value, or throwing it).
    Resume {
        task: Rc<RefCell<Option<crate::asyncfn::Task>>>,
        throw: bool,
    },
}

#[derive(Debug)]
pub struct AllState {
    pub values: Vec<Value>,
    pub remaining: usize,
    pub result: Rc<RefCell<Promise>>,
    pub done: bool,
}

/// A hook's state, in call order.
#[derive(Debug)]
pub(crate) enum HookState {
    State {
        value: Value,
        queue: Vec<Update>,
    },
    Reducer {
        value: Value,
        reducer: Value,
        queue: Vec<Value>,
    },
    Memo {
        value: Value,
        deps: Option<Vec<Value>>,
    },
    Ref(Value),
    Effect {
        layout: bool,
        deps: Option<Vec<Value>>,
        /// The create function of the last render, when it must run at commit.
        pending: Option<Value>,
        cleanup: Option<Value>,
    },
    Context(u32),
    Id(Str),
    /// `useSyncExternalStore`: the snapshot, its getter, the subscribe function
    /// and what it returned.
    Store {
        value: Value,
        get: Value,
        subscribe: Value,
        unsubscribe: Option<Value>,
        needs_subscribe: bool,
    },
}

/// A listener `window.addEventListener` or `document.addEventListener` registered.
#[derive(Debug, Clone)]
pub(crate) struct GlobalListener {
    pub window: bool,
    pub ty: Str,
    pub f: Value,
    pub capture: bool,
}

#[derive(Debug)]
pub(crate) enum Update {
    /// A value computed eagerly (or passed directly).
    Value(Value),
    /// An updater function, applied at render.
    Fn(Value),
}

/// One template element (or component element) a component's own frame produced
/// last render, so the next render can skip holes whose inputs did not change.
#[derive(Debug, Default)]
pub(crate) struct ElemCache {
    pub entries: BTreeMap<(usize, u32), CacheEntry>,
}

#[derive(Debug)]
pub(crate) struct CacheEntry {
    /// Per hole: the dependency values it was evaluated with.
    pub deps: Vec<Vec<Value>>,
    pub holes: Vec<Value>,
    pub elem: Rc<Elem>,
}

#[derive(Debug)]
pub(crate) struct Instance {
    pub func: Rc<Closure>,
    pub elem: Option<Rc<Elem>>,
    pub props: Value,
    pub hooks: Vec<HookState>,
    pub rendered: MNode,
    pub parent: Option<u32>,
    /// State updates are queued.
    pub dirty: bool,
    /// A descendant is dirty.
    pub subtree_dirty: bool,
    /// A context this instance reads changed.
    pub context_changed: bool,
    pub cache: ElemCache,
}

/// The mounted tree: what each rendered value became in the DOM.
#[derive(Debug, Default)]
pub(crate) enum MNode {
    #[default]
    Empty,
    Text {
        node: NodeId,
        text: Str,
    },
    Template(Box<MTemplate>),
    Component {
        inst: u32,
    },
    /// An array or a fragment: children with their keys.
    List {
        children: Vec<(ListKey, MNode)>,
        /// A keyed `<Fragment key>` element (vs a bare array).
        key: Option<Str>,
        fragment: bool,
    },
    Provider {
        ctx: u32,
        value: Value,
        children: Vec<(ListKey, MNode)>,
        key: Option<Str>,
    },
}

/// How a list child is matched against the previous render: an explicit key or its
/// index (React's implicit key).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ListKey {
    Key(Str),
    Index(usize),
}

#[derive(Debug)]
pub(crate) struct MTemplate {
    pub tid: u32,
    pub key: Option<Str>,
    pub root: NodeId,
    pub elem: Rc<Elem>,
    pub holes: Vec<MHole>,
}

#[derive(Debug)]
pub(crate) enum MHole {
    Attr {
        node: NodeId,
        value: Value,
    },
    Spread {
        node: NodeId,
        value: Value,
    },
    Ref {
        node: NodeId,
        value: Value,
    },
    Child {
        parent: NodeId,
        /// The static node after this hole in its parent, if the next sibling is one.
        next_static: Option<NodeId>,
        /// The hole after this one, if the next sibling is a hole.
        next_hole: Option<u32>,
        value: Value,
        mounted: MNode,
    },
}

/// Per-template facts computed once at load: where each hole lives.
#[derive(Debug, Clone)]
pub(crate) struct TemplateInfo {
    /// Per hole: the index (in creation order) of the element it belongs to, and for
    /// child holes where it sits.
    pub sites: Vec<HoleSite>,
    /// Prop name per attribute hole.
    pub props: Vec<Option<Str>>,
    /// Tag per created element index.
    pub tags: Vec<Str>,
}

#[derive(Debug, Clone)]
pub(crate) enum HoleSite {
    Attr,
    Child {
        next_static: Option<usize>,
        next_hole: Option<u32>,
    },
}

#[derive(Debug)]
pub(crate) struct Timer {
    pub id: u32,
    pub due: f64,
    pub interval: Option<f64>,
    pub callback: Value,
    pub args: Vec<Value>,
}

#[derive(Debug)]
pub(crate) enum Microtask {
    Reaction(Reaction, PromiseState),
}

/// A controlled form control: what React will restore after each event.
#[derive(Debug, Clone)]
pub(crate) enum Controlled {
    Value(String),
    Checked(bool),
}

/// The render in progress.
#[derive(Debug)]
pub(crate) struct RenderCtx {
    pub inst: u32,
    pub cursor: usize,
    pub state_changed: bool,
    pub rerender: bool,
    /// Last render's element cache (entries are taken as they are reused).
    pub old_cache: ElemCache,
    pub new_cache: ElemCache,
}

pub(crate) struct Runtime {
    pub module: Rc<Module>,
    pub inner: Inner,
    pub globals: Vec<Value>,
    pub ctx_defaults: BTreeMap<u32, Value>,
    pub templates: Vec<TemplateInfo>,
    pub instances: BTreeMap<u32, Instance>,
    pub next_inst: u32,
    pub root: MNode,
    pub container: NodeId,
    pub handlers: BTreeMap<NodeId, Vec<(Str, Value)>>,
    pub controlled: BTreeMap<NodeId, Controlled>,
    pub form_props: BTreeMap<NodeId, crate::dom::FormProps>,
    /// The instances whose output is being reconciled, innermost last.
    pub owner: Vec<u32>,
    pub pending_work: bool,
    pub render: Vec<RenderCtx>,
    pub ctx_stack: Vec<(u32, Value)>,
    /// Instances rendered this pass, children before parents.
    pub effect_list: Vec<u32>,
    pub deleted_layout: Vec<Value>,
    pub deleted_passive: Vec<Value>,
    pub ref_detach: Vec<Value>,
    pub ref_attach: Vec<(Value, NodeId)>,
    pub autofocus: Vec<NodeId>,
    pub timers: Vec<Timer>,
    pub next_timer: u32,
    /// Virtual milliseconds since boot (timers are due on this clock).
    pub clock_ms: f64,
    pub start_micros: i64,
    pub microtasks: VecDeque<Microtask>,
    pub id_counter: u32,
    /// Hole skipping and element reuse are sound for this module.
    pub pure_render: bool,
    pub booted: bool,
    /// A render threw: React unmounted the root.
    pub crashed: bool,
    /// `window`/`document` event listeners, in registration order.
    pub global_listeners: Vec<GlobalListener>,
    /// Per function: which frame slots are boxed.
    pub boxed_cache: Vec<Option<Rc<[bool]>>>,
    /// Compiled regular expressions by (pattern, flags).
    pub regex_cache: BTreeMap<(String, String), Rc<cw_regex::Regex>>,
    /// Counters for timing and tests.
    pub stats: Stats,
    /// Nesting of `fire` (script time is counted at the outermost).
    pub fire_depth: u32,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    /// Microseconds spent in event handlers, rendering, committing and effects
    /// (what a browser reports as script time; not layout or hit testing). Not
    /// measured on wasm32, which has no clock here.
    pub script_micros: u64,
    pub renders: u64,
    pub holes_evaluated: u64,
    pub holes_skipped: u64,
    pub elements_reused: u64,
}

impl Runtime {
    pub fn new(module: Rc<Module>, host: Box<dyn ScriptHostDocument>, url: &str) -> Runtime {
        let inner = Inner::new(host, Journal::recording(), url);
        let templates = module
            .templates
            .iter()
            .map(crate::dom::template_info)
            .collect();
        let pure_render = crate::render::is_pure(&module);
        Runtime {
            module,
            inner,
            globals: Vec::new(),
            ctx_defaults: BTreeMap::new(),
            templates,
            instances: BTreeMap::new(),
            next_inst: 1,
            root: MNode::Empty,
            container: NodeId(0),
            handlers: BTreeMap::new(),
            controlled: BTreeMap::new(),
            form_props: BTreeMap::new(),
            owner: Vec::new(),
            pending_work: false,
            render: Vec::new(),
            ctx_stack: Vec::new(),
            effect_list: Vec::new(),
            deleted_layout: Vec::new(),
            deleted_passive: Vec::new(),
            ref_detach: Vec::new(),
            ref_attach: Vec::new(),
            autofocus: Vec::new(),
            timers: Vec::new(),
            next_timer: 1,
            clock_ms: 0.0,
            start_micros: 0,
            microtasks: VecDeque::new(),
            id_counter: 0,
            pure_render,
            booted: false,
            crashed: false,
            regex_cache: BTreeMap::new(),
            boxed_cache: Vec::new(),
            global_listeners: Vec::new(),
            stats: Stats::default(),
            fire_depth: 0,
        }
    }

    pub fn log(&mut self, level: LogLevel, text: &str) {
        self.inner.log(level, text);
    }

    /// Reports an uncaught exception to the console, as a browser does.
    pub fn report(&mut self, t: Throw) {
        if let Throw::Value(v) = t {
            let text = match &v {
                Value::Str(s) if s.contains("Error") => s.to_string(),
                Value::Error(_) => v.to_js_string(),
                other => crate::interp::inspect(other),
            };
            self.log(LogLevel::Error, &format!("Uncaught {text}"));
        }
    }

    /// Keeps the host journal from growing: the runtime snapshots its state, not a
    /// replay record.
    pub fn trim_journal(&mut self) {
        self.inner.journal.entries.clear();
    }

    /// Milliseconds on the world clock right now, by the runtime's virtual clock.
    pub fn now_ms(&self) -> f64 {
        self.start_micros as f64 / 1000.0 + self.clock_ms
    }

    /// Fires the timers due on the world clock, then advances the virtual clock by
    /// up to `advance_ms` to fire later ones (as the Realm's `run_until_idle`).
    pub fn run_timers(&mut self, advance_ms: u32) -> bool {
        let now = self.inner.host_now_micros();
        let world_ms = (now - self.start_micros) as f64 / 1000.0;
        if world_ms > self.clock_ms {
            self.clock_ms = world_ms;
        }
        let deadline = self.clock_ms + advance_ms as f64;
        let mut ran = false;
        self.settle();
        loop {
            loop {
                let due = self
                    .timers
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.due <= self.clock_ms)
                    .min_by(|(_, a), (_, b)| a.due.total_cmp(&b.due).then(a.id.cmp(&b.id)))
                    .map(|(i, _)| i);
                let Some(i) = due else { break };
                let (callback, args) = {
                    let t = &mut self.timers[i];
                    (t.callback.clone(), t.args.clone())
                };
                match self.timers[i].interval {
                    Some(iv) => self.timers[i].due += iv,
                    None => {
                        self.timers.remove(i);
                    }
                }
                if let Err(e) = self.call_value(&callback, args) {
                    self.report(e);
                }
                self.settle();
                ran = true;
            }
            let next = self
                .timers
                .iter()
                .map(|t| t.due)
                .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.min(d))));
            match next {
                Some(n) if n <= deadline => self.clock_ms = self.clock_ms.max(n),
                _ => break,
            }
        }
        ran
    }
}
