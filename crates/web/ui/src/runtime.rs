//! The runtime's state: the module, the document (through `cw_web`'s script-free
//! `Inner`: DOM, stylesheets, cascade, layout, hit testing, focus and form state),
//! module globals, component instances with their hooks, the mounted tree, event
//! handlers, timers and the microtask queue.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use cw_web::dom::NodeId;
use cw_web::script::{Inner, Journal, LogLevel, ScriptHostDocument};

use crate::value::*;

/// A thrown value, or the short-circuit of an optional chain.
#[derive(Debug)]
pub enum Throw {
    Value(Value),
    Short,
}

pub type R<T> = Result<T, Throw>;

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
    /// Removes the `cw.onEnv` listener with this id.
    CwOffEnv(u32),
    /// A built-in function used as a value (`xs.filter(Boolean)`, `map(Number)`).
    Builtin(crate::ir::Builtin),
    /// A built-in method bound to its receiver (`arr.map` read by an island).
    BoundMethod {
        recv: crate::value::Value,
        name: Str,
    },
    /// A `useImperativeHandle` effect: sets `r` to `create()`, returning the
    /// cleanup that sets it back to null.
    ImperativeSet {
        r: crate::value::Value,
        create: crate::value::Value,
    },
    ImperativeClear(crate::value::Value),
    /// A MediaQueryList's `addEventListener`/`addListener` (`add`) or remove.
    MediaListen {
        list: u32,
        add: bool,
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
    pub func: crate::value::ComponentFn,
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

/// A mounted app's whole state. Opaque outside the crate except for the methods
/// generated code calls (`crate::gen`).
pub struct Runtime {
    pub(crate) program: Rc<dyn crate::program::Program>,
    pub(crate) inner: Inner,
    pub(crate) globals: Vec<Value>,
    pub(crate) ctx_defaults: BTreeMap<u32, Value>,
    pub(crate) templates: Vec<TemplateInfo>,
    pub(crate) instances: BTreeMap<u32, Instance>,
    pub(crate) next_inst: u32,
    pub(crate) root: MNode,
    pub(crate) container: NodeId,
    pub(crate) handlers: BTreeMap<NodeId, Vec<(Str, Value)>>,
    pub(crate) controlled: BTreeMap<NodeId, Controlled>,
    pub(crate) form_props: BTreeMap<NodeId, crate::dom::FormProps>,
    /// The instances whose output is being reconciled, innermost last.
    pub(crate) owner: Vec<u32>,
    pub(crate) pending_work: bool,
    pub(crate) render: Vec<RenderCtx>,
    pub(crate) ctx_stack: Vec<(u32, Value)>,
    /// Instances rendered this pass, children before parents.
    pub(crate) effect_list: Vec<u32>,
    pub(crate) deleted_layout: Vec<Value>,
    pub(crate) deleted_passive: Vec<Value>,
    pub(crate) ref_detach: Vec<Value>,
    pub(crate) ref_attach: Vec<(Value, NodeId)>,
    pub(crate) autofocus: Vec<NodeId>,
    pub(crate) timers: Vec<Timer>,
    pub(crate) next_timer: u32,
    /// `requestAnimationFrame` callbacks waiting for a frame, by id.
    pub(crate) raf: Vec<(u32, Value)>,
    pub(crate) next_raf: u32,
    /// `matchMedia` lists: the query, its object, its `change` listeners.
    pub(crate) media_lists: Vec<(String, Value, Vec<Value>)>,
    /// Virtual milliseconds since boot (timers are due on this clock).
    pub(crate) clock_ms: f64,
    pub(crate) start_micros: i64,
    pub(crate) microtasks: VecDeque<Microtask>,
    pub(crate) id_counter: u32,
    /// The `cw` global's bridge to a computerworld desktop host.
    pub(crate) cw: crate::cw::CwBridge,
    /// Hole skipping and element reuse are sound for this module.
    pub(crate) pure_render: bool,
    pub(crate) booted: bool,
    /// A render threw: React unmounted the root.
    pub(crate) crashed: bool,
    /// `window`/`document` event listeners, in registration order.
    pub(crate) global_listeners: Vec<GlobalListener>,
    /// Per function: which frame slots are boxed.
    pub(crate) boxed_cache: Vec<Option<Rc<[bool]>>>,
    /// Compiled regular expressions by (pattern, flags).
    pub(crate) regex_cache: BTreeMap<(String, String), Rc<cw_regex::Regex>>,
    /// Counters for timing and tests.
    pub(crate) stats: Stats,
    /// Nesting of `fire` (script time is counted at the outermost).
    pub(crate) fire_depth: u32,
    /// A generated program's string literals, made once each (`gen::Runtime::lit`).
    pub(crate) lits: Vec<Option<Str>>,
    /// The JS VM running the app's code outside the compiled subset, when it has
    /// any (see `crate::island`).
    pub(crate) island: Option<Box<crate::island::Island>>,
    /// Templates made while running, one per host tag an island's element uses
    /// (numbered after the program's).
    pub(crate) dyn_templates: Vec<(String, crate::ir::Template)>,
    /// A restored island's stand-ins, held until their values are paired.
    pub(crate) island_keep: Vec<cw_jsvm::value::Obj>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    /// Microseconds spent in event handlers, rendering, committing and effects
    /// (what a browser reports as script time; not layout or hit testing). Not
    /// measured on wasm32, which has no clock here.
    pub script_micros: u64,
    /// The same in nanoseconds.
    pub script_nanos: u64,
    pub renders: u64,
    pub holes_evaluated: u64,
    pub holes_skipped: u64,
    pub elements_reused: u64,
}

impl Runtime {
    pub(crate) fn new(
        program: Rc<dyn crate::program::Program>,
        host: Box<dyn ScriptHostDocument>,
        url: &str,
    ) -> Runtime {
        let inner = Inner::new(host, Journal::recording(), url);
        let templates = (0..program.templates_len())
            .map(|t| crate::dom::template_info(program.template(t as u32)))
            .collect();
        let pure_render = program.pure_render();
        Runtime {
            program,
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
            raf: Vec::new(),
            next_raf: 0,
            media_lists: Vec::new(),
            clock_ms: 0.0,
            start_micros: 0,
            microtasks: VecDeque::new(),
            id_counter: 0,
            cw: Default::default(),
            pure_render,
            booted: false,
            crashed: false,
            island: None,
            dyn_templates: Vec::new(),
            island_keep: Vec::new(),
            regex_cache: BTreeMap::new(),
            boxed_cache: Vec::new(),
            global_listeners: Vec::new(),
            stats: Stats::default(),
            fire_depth: 0,
            lits: Vec::new(),
        }
    }

    pub fn log(&mut self, level: LogLevel, text: &str) {
        self.inner.log(level, text);
    }

    /// Reports an uncaught exception to the console, as a browser does.
    pub fn report(&mut self, t: Throw) {
        if let Throw::Value(v) = t {
            let text = self.thrown_text(&v);
            self.log(LogLevel::Error, &format!("Uncaught {text}"));
        }
    }

    /// How the console prints a thrown value.
    pub(crate) fn thrown_text(&mut self, v: &Value) -> String {
        match v {
            Value::Str(s) if s.contains("Error") => s.to_string(),
            Value::Error(_) => v.to_js_string(),
            // A value the island threw: its `String()` (an Error's name and
            // message), as the VM would print it.
            Value::Foreign(f) => {
                let f = f.clone();
                self.foreign_string(&f)
            }
            other => crate::interp::inspect(other),
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
    /// Runs the `requestAnimationFrame` callbacks for one frame (those asked for
    /// during it wait for the next), then settles.
    pub fn animation_frame(&mut self) {
        let t = self.performance_now();
        let cbs = std::mem::take(&mut self.raf);
        for (_, f) in cbs {
            if let Err(e) = self.call_value(&f, vec![Value::Num(t)]) {
                self.report(e);
            }
        }
        self.settle();
        if self.refresh_hover() {
            self.settle();
        }
    }

    /// `performance.now()`, as the Realm's VM gives it.
    pub(crate) fn performance_now(&self) -> f64 {
        30.0 + self.clock_ms
    }

    pub fn run_timers(&mut self, advance_ms: u32) -> bool {
        let now = self.inner.host_now_micros();
        let world_ms = (now - self.start_micros) as f64 / 1000.0;
        if world_ms > self.clock_ms {
            self.clock_ms = world_ms;
        }
        let deadline = self.clock_ms + advance_ms as f64;
        let mut ran = false;
        let mut next_frame = self.clock_ms + 16.0;
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
            // Frames as the Realm's run_until_idle has them: one per 16 ms the
            // clock advances while callbacks wait.
            let has_frames = !self.raf.is_empty();
            let mut target = match next {
                Some(n) if n <= deadline => n,
                _ if has_frames && next_frame <= deadline => next_frame,
                _ => break,
            };
            if has_frames && next_frame < target {
                target = next_frame;
            }
            self.clock_ms = self.clock_ms.max(target);
            if has_frames && target >= next_frame {
                self.animation_frame();
                next_frame = target + 16.0;
                ran = true;
            }
        }
        // Content that moved under a still pointer takes `:hover` with it (and
        // whatever its boundary events set off runs too).
        for _ in 0..4 {
            if !self.refresh_hover() {
                break;
            }
            self.settle();
            ran = true;
        }
        ran
    }
}
