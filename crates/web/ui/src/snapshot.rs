//! Snapshots: the runtime's state as data.
//!
//! Values that are shared (an array held by state and captured by a handler, a ref
//! object held by a hook and a closure) must stay shared after a restore, so values
//! are written as a heap: every array, object, closure, element, ref and promise
//! once, referenced by index. The document, form state, focus and scroll are the
//! engine's own serialisable types.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use cw_web::dom::{Document, NodeId};
use cw_web::geom::Au;
use cw_web::script::{FetchResponse, ScriptHostDocument};
use serde::{Deserialize, Serialize};

use crate::dom::FormProps;
use crate::ir::Module;
use crate::program::{Program, ProgramId};
use crate::runtime::*;
use crate::value::*;

/// A value: primitives inline, everything with identity by heap index.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum V {
    U,
    Null,
    B(bool),
    N(f64),
    /// NaN, +∞, −∞ (JSON has no spelling for them).
    NonFinite(u8),
    S(String),
    H(u32),
    Setter(u32, u32),
    Dispatch(u32, u32),
    Node(NodeId),
    Ctx(u32),
    /// A value of the app's island, by handle (see `crate::island`).
    Foreign(u32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HeapObj {
    Array(Vec<V>),
    Object(Vec<(String, V)>),
    Func(u32, Vec<V>),
    Ref(V),
    Template(u32, Vec<V>, Option<String>),
    Component(u32, V, Option<String>),
    Fragment(Vec<V>, Option<String>),
    Provider(u32, V, Vec<V>, Option<String>),
    Response(FetchResponse),
    Set(Vec<V>),
    Map(Vec<(V, V)>),
    /// Pattern, flags, `lastIndex`.
    Regex(String, String, usize),
    Error(String, String),
    Cell(V),
    /// A `useSyncExternalStore` subscription callback: instance and hook.
    StoreChanged(u32, u32),
    /// The function removing the `cw.onEnv` listener with this id.
    CwOffEnv(u32),
    /// A built-in function used as a value.
    BuiltinFn(crate::ir::Builtin),
    /// A component element of an island's component (function, props, key).
    ComponentOf(V, V, Option<String>),
    /// A built-in method bound to its receiver.
    BoundMethod(V, String),
    /// A `useImperativeHandle` effect (ref, create) and its cleanup (ref).
    ImperativeSet(V, V),
    ImperativeClear(V),
    /// A portal element: children, container, key.
    Portal(Vec<V>, NodeId, Option<String>),
    /// A MediaQueryList's listener adder (true) or remover, by list.
    MediaListen(u32, bool),
    /// A `Date`'s time value (`None` when invalid: JSON has no NaN).
    Date(Option<f64>),
    /// A promise: 0 pending, 1 fulfilled, 2 rejected; its value; its reactions
    /// (kind 0 then, 1 catch, 2 finally; handlers; the promise they settle).
    Promise(u8, Option<V>, Vec<(u8, Option<V>, Option<V>, V)>),
    /// `resolve`/`reject` of a `new Promise`: the promise, and whether it rejects.
    Resolver(V, bool),
    /// A `Promise.all` in progress: values, how many remain, its promise, done.
    AllState(Vec<V>, usize, V, bool),
    /// Element `index` of a `Promise.all` settling, and its rejection.
    AllSlot(V, usize),
    AllReject(V),
    /// An async function waiting on an `await` (none once it finished).
    Task(Option<TaskS>),
    /// The callbacks resuming a task: with the value, or throwing it.
    Resume(V, bool),
    /// Promises and events do not outlive the entry that created them; a pending
    /// promise restores as one that never settles.
    Opaque,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HookS {
    State(V),
    Reducer(V, V),
    Memo(V, Option<Vec<V>>),
    Ref(V),
    Effect(bool, Option<Vec<V>>, Option<V>),
    Context(u32),
    Id(String),
    Store(V, V, V, Option<V>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MNodeS {
    Empty,
    Text(NodeId, String),
    Template {
        tid: u32,
        key: Option<String>,
        root: NodeId,
        elem: V,
        holes: Vec<MHoleS>,
    },
    Component(u32),
    List(Vec<(ListKeyS, MNodeS)>, Option<String>, bool),
    Provider(u32, V, Vec<(ListKeyS, MNodeS)>, Option<String>),
    Portal(NodeId, Vec<(ListKeyS, MNodeS)>, Option<String>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ListKeyS {
    Key(String),
    Index(usize),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MHoleS {
    Attr(NodeId, V),
    Spread(NodeId, V),
    Ref(NodeId, V),
    Child {
        parent: NodeId,
        next_static: Option<NodeId>,
        next_hole: Option<u32>,
        value: V,
        mounted: MNodeS,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstanceS {
    pub id: u32,
    pub func: V,
    pub elem: Option<V>,
    pub props: V,
    pub hooks: Vec<HookS>,
    pub rendered: MNodeS,
    pub parent: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimerS {
    pub id: u32,
    pub due: f64,
    pub interval: Option<f64>,
    pub callback: V,
    pub args: Vec<V>,
}

/// A mounted app's complete state.
///
/// The program is named, not stored, when it was generated ahead of time: `program`
/// is its identity and `module` is absent; an interpreted app's snapshot carries its
/// IR in `module`, as it always has. See `crate::program`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<Module>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<ProgramId>,
    pub url: String,
    pub doc: Document,
    pub viewport: (u32, u32, u8, u16),
    pub focused: Option<NodeId>,
    pub focus_visible: bool,
    pub hovered: Option<NodeId>,
    /// Where the pointer last was, and whether the content changed since `hovered`
    /// was hit-tested there (so the next idle point re-hit-tests it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<(i32, i32)>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hover_stale: bool,
    pub values: Vec<(NodeId, String)>,
    pub checked: Vec<(NodeId, bool)>,
    pub indeterminate: Vec<NodeId>,
    pub selection: Vec<(NodeId, (usize, usize))>,
    /// Closed selects' type-ahead: buffer, when the last key came, the cycled key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typeahead: Vec<(NodeId, String, f64, Option<char>)>,
    pub scroll: Vec<(NodeId, i32, i32)>,
    pub images: Vec<(String, u32, u32)>,
    pub heap: Vec<HeapObj>,
    pub globals: Vec<V>,
    pub ctx_defaults: Vec<(u32, V)>,
    pub instances: Vec<InstanceS>,
    pub next_inst: u32,
    pub root: MNodeS,
    pub container: NodeId,
    pub handlers: Vec<(NodeId, Vec<(String, V)>)>,
    pub controlled: Vec<(NodeId, Option<String>, bool)>,
    pub form_props: Vec<(NodeId, [V; 5])>,
    pub timers: Vec<TimerS>,
    pub next_timer: u32,
    /// `requestAnimationFrame` callbacks waiting, and the last id given.
    #[serde(default)]
    pub raf: Vec<(u32, V)>,
    #[serde(default)]
    pub next_raf: u32,
    /// `matchMedia` lists: query, object, listeners.
    #[serde(default)]
    pub media_lists: Vec<(String, V, Vec<V>)>,
    /// Portals' top nodes, to where their portal sits in React's tree.
    #[serde(default)]
    pub portal_parents: Vec<(NodeId, NodeId)>,
    pub clock_ms: f64,
    pub start_micros: i64,
    pub id_counter: u32,
    pub booted: bool,
    pub crashed: bool,
    /// `window`/`document` listeners: window?, type, listener, capture.
    #[serde(default)]
    pub listeners: Vec<(bool, String, V, bool)>,
    /// The `cw` bridge, once the app used it, with the requests awaiting replies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cw: Option<CwS>,
    /// The island: its VM's heap image and the values crossing its boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub island: Option<IslandS>,
    /// Templates made while running (an island's host elements), by tag.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dyn_templates: Vec<String>,
}

/// An island in a snapshot. The heap image (`cw_jsvm::snapshot`, base64) is
/// readable only by the program image that wrote it; the roots it was written with
/// are, in order: the `__cw` object, the VM values of cw-ui's handles (`js`), the
/// VM stand-ins of cw-ui's values (`cw`), and the VM objects of contexts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IslandS {
    pub heap: String,
    pub js: Vec<u32>,
    pub cw: Vec<(u32, V)>,
    pub contexts: Vec<u32>,
    pub next_context: u32,
    pub exports: Vec<V>,
    pub rng: u64,
}

/// A suspended async function call (see `crate::asyncfn::encode`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskS {
    pub func: u32,
    pub frame: FrameS,
    pub stack: Vec<ContS>,
    pub result: V,
    /// What the awaited value binds: 0 the `let`, 1 the assignment, 2 nothing, 3
    /// the return value.
    pub bind: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameS {
    pub locals: Vec<V>,
    pub closure: V,
    pub inst: Option<u32>,
    pub occ: Vec<(usize, u32)>,
    pub boxed: Vec<bool>,
}

/// A continuation, by the index of its statement list or statement in the
/// function's pre-order (`crate::asyncfn::index`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ContS {
    Seq(u32, usize),
    ForOf(u32, Vec<V>, usize),
    For(u32),
    Switch(u32, usize, usize),
    /// `None` in the block; `Handler` in the handler; else in the finalizer with the
    /// completion it resumes after.
    Try(u32, Option<CompS>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CompS {
    Normal,
    Return(V),
    Throw(V),
    Break,
    Continue,
    Handler,
}

/// The `cw` bridge's state (see `crate::cw`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CwS {
    pub kind: String,
    pub argument: String,
    pub env: V,
    pub state: V,
    pub listeners: Vec<(u32, V)>,
    pub next_listener: u32,
    pub next_request: u64,
    #[serde(default)]
    pub declared: bool,
    /// Requests awaiting a reply: id, their promise, whether an HTTP response.
    #[serde(default)]
    pub pending: Vec<(u64, V, bool)>,
}

// ------------------------------------------------------------------ encoding

struct Enc {
    heap: Vec<HeapObj>,
    seen: BTreeMap<usize, u32>,
}

impl Enc {
    fn all(&mut self, state: &Rc<RefCell<AllState>>) -> V {
        match self.reserve(Rc::as_ptr(state) as *const u8 as usize) {
            Err(i) => V::H(i),
            Ok(i) => {
                let (values, remaining, result, done) = {
                    let s = state.borrow();
                    (s.values.clone(), s.remaining, s.result.clone(), s.done)
                };
                let values = values.iter().map(|v| self.v(v)).collect();
                let result = self.v(&Value::Promise(result));
                self.heap[i as usize] = HeapObj::AllState(values, remaining, result, done);
                V::H(i)
            }
        }
    }

    fn task(&mut self, cell: &Rc<RefCell<Option<crate::asyncfn::Task>>>) -> V {
        match self.reserve(Rc::as_ptr(cell) as *const u8 as usize) {
            Err(i) => V::H(i),
            Ok(i) => {
                let t = cell
                    .borrow()
                    .as_ref()
                    .map(|t| crate::asyncfn::encode(t, &mut |v| self.v(v)));
                self.heap[i as usize] = HeapObj::Task(t);
                V::H(i)
            }
        }
    }

    fn reserve(&mut self, ptr: usize) -> Result<u32, u32> {
        if let Some(i) = self.seen.get(&ptr) {
            return Err(*i);
        }
        let i = self.heap.len() as u32;
        self.heap.push(HeapObj::Opaque);
        self.seen.insert(ptr, i);
        Ok(i)
    }

    fn v(&mut self, v: &Value) -> V {
        match v {
            Value::Foreign(f) => V::Foreign(f.id),
            Value::Undefined => V::U,
            Value::Null => V::Null,
            Value::Bool(b) => V::B(*b),
            Value::Num(n) => {
                if n.is_finite() {
                    V::N(*n)
                } else if n.is_nan() {
                    V::NonFinite(0)
                } else if *n > 0.0 {
                    V::NonFinite(1)
                } else {
                    V::NonFinite(2)
                }
            }
            Value::Str(s) => V::S(s.to_string()),
            Value::Setter(a, b) => V::Setter(*a, *b),
            Value::Dispatch(a, b) => V::Dispatch(*a, *b),
            Value::Node(n) => V::Node(*n),
            Value::Context(c) => V::Ctx(*c),
            Value::Array(a) => match self.reserve(Rc::as_ptr(a) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let items = a.borrow().iter().map(|x| self.v(x)).collect();
                    self.heap[i as usize] = HeapObj::Array(items);
                    V::H(i)
                }
            },
            Value::Object(o) => match self.reserve(Rc::as_ptr(o) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let items = o
                        .borrow()
                        .iter()
                        .map(|(k, x)| (k.to_string(), self.v(x)))
                        .collect();
                    self.heap[i as usize] = HeapObj::Object(items);
                    V::H(i)
                }
            },
            Value::Func(c) => match self.reserve(Rc::as_ptr(c) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let caps = c.captures.iter().map(|x| self.v(x)).collect();
                    self.heap[i as usize] = HeapObj::Func(c.func, caps);
                    V::H(i)
                }
            },
            Value::Ref(r) => match self.reserve(Rc::as_ptr(r) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let inner = self.v(&r.borrow());
                    self.heap[i as usize] = HeapObj::Ref(inner);
                    V::H(i)
                }
            },
            Value::Elem(e) => match self.reserve(Rc::as_ptr(e) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let obj = match &**e {
                        Elem::Template { tid, holes, key } => HeapObj::Template(
                            *tid,
                            holes.iter().map(|x| self.v(x)).collect(),
                            key.as_ref().map(|k| k.to_string()),
                        ),
                        Elem::Component { func, props, key } => {
                            let f = self.v(&func.value());
                            match f {
                                V::H(fi) => HeapObj::Component(
                                    fi,
                                    self.v(props),
                                    key.as_ref().map(|k| k.to_string()),
                                ),
                                f => HeapObj::ComponentOf(
                                    f,
                                    self.v(props),
                                    key.as_ref().map(|k| k.to_string()),
                                ),
                            }
                        }
                        Elem::Fragment { children, key } => HeapObj::Fragment(
                            children.iter().map(|x| self.v(x)).collect(),
                            key.as_ref().map(|k| k.to_string()),
                        ),
                        Elem::Provider {
                            ctx,
                            value,
                            children,
                            key,
                        } => HeapObj::Provider(
                            *ctx,
                            self.v(value),
                            children.iter().map(|x| self.v(x)).collect(),
                            key.as_ref().map(|k| k.to_string()),
                        ),
                        Elem::Portal {
                            children,
                            container,
                            key,
                        } => HeapObj::Portal(
                            children.iter().map(|x| self.v(x)).collect(),
                            *container,
                            key.as_ref().map(|k| k.to_string()),
                        ),
                    };
                    self.heap[i as usize] = obj;
                    V::H(i)
                }
            },
            Value::Response(r) => match self.reserve(Rc::as_ptr(r) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    self.heap[i as usize] = HeapObj::Response((**r).clone());
                    V::H(i)
                }
            },
            Value::Set(a) => match self.reserve(Rc::as_ptr(a) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let items = a.borrow().iter().map(|x| self.v(x)).collect();
                    self.heap[i as usize] = HeapObj::Set(items);
                    V::H(i)
                }
            },
            Value::Map(m) => match self.reserve(Rc::as_ptr(m) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let items = m
                        .borrow()
                        .iter()
                        .map(|(k, x)| (self.v(k), self.v(x)))
                        .collect();
                    self.heap[i as usize] = HeapObj::Map(items);
                    V::H(i)
                }
            },
            Value::Regex(r) => match self.reserve(Rc::as_ptr(r) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    self.heap[i as usize] = HeapObj::Regex(
                        r.source.to_string(),
                        r.flags.to_string(),
                        r.last_index.get(),
                    );
                    V::H(i)
                }
            },
            Value::Date(t) => match self.reserve(Rc::as_ptr(t) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let v = t.get();
                    self.heap[i as usize] = HeapObj::Date(v.is_finite().then_some(v));
                    V::H(i)
                }
            },
            Value::Error(e) => match self.reserve(Rc::as_ptr(e) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    self.heap[i as usize] =
                        HeapObj::Error(e.name.to_string(), e.message.to_string());
                    V::H(i)
                }
            },
            Value::Cell(c) => match self.reserve(Rc::as_ptr(c) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let inner = self.v(&c.borrow());
                    self.heap[i as usize] = HeapObj::Cell(inner);
                    V::H(i)
                }
            },
            Value::Promise(p) => match self.reserve(Rc::as_ptr(p) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    let (state, value, reactions) = {
                        let pb = p.borrow();
                        let (state, value) = match &pb.state {
                            PromiseState::Pending => (0, None),
                            PromiseState::Fulfilled(v) => (1, Some(v.clone())),
                            PromiseState::Rejected(v) => (2, Some(v.clone())),
                        };
                        (state, value, pb.reactions.clone())
                    };
                    let value = value.map(|v| self.v(&v));
                    let reactions = reactions
                        .iter()
                        .map(|r| {
                            (
                                match r.kind {
                                    ReactionKind::Then => 0,
                                    ReactionKind::Catch => 1,
                                    ReactionKind::Finally => 2,
                                },
                                r.on_fulfilled.as_ref().map(|f| self.v(f)),
                                r.on_rejected.as_ref().map(|f| self.v(f)),
                                self.v(&Value::Promise(r.result.clone())),
                            )
                        })
                        .collect();
                    self.heap[i as usize] = HeapObj::Promise(state, value, reactions);
                    V::H(i)
                }
            },
            Value::Native(n) => match self.reserve(Rc::as_ptr(n) as *const u8 as usize) {
                Err(i) => V::H(i),
                Ok(i) => {
                    self.heap[i as usize] = match &**n {
                        NativeFn::Resolver { promise, reject } => {
                            HeapObj::Resolver(self.v(&Value::Promise(promise.clone())), *reject)
                        }
                        NativeFn::AllSlot { state, index } => {
                            HeapObj::AllSlot(self.all(state), *index)
                        }
                        NativeFn::AllReject(state) => HeapObj::AllReject(self.all(state)),
                        NativeFn::Resume { task, throw } => {
                            HeapObj::Resume(self.task(task), *throw)
                        }
                        NativeFn::StoreChanged { inst, hook } => {
                            HeapObj::StoreChanged(*inst, *hook)
                        }
                        NativeFn::CwOffEnv(id) => HeapObj::CwOffEnv(*id),
                        NativeFn::Builtin(b) => HeapObj::BuiltinFn(*b),
                        NativeFn::BoundMethod { recv, name } => {
                            HeapObj::BoundMethod(self.v(recv), name.to_string())
                        }
                        NativeFn::ImperativeSet { r, create } => {
                            HeapObj::ImperativeSet(self.v(r), self.v(create))
                        }
                        NativeFn::ImperativeClear(r) => HeapObj::ImperativeClear(self.v(r)),
                        NativeFn::MediaListen { list, add } => HeapObj::MediaListen(*list, *add),
                    };
                    V::H(i)
                }
            },
            Value::Event(_) => {
                let i = self.heap.len() as u32;
                self.heap.push(HeapObj::Opaque);
                V::H(i)
            }
        }
    }

    fn key(k: &ListKey) -> ListKeyS {
        match k {
            ListKey::Key(s) => ListKeyS::Key(s.to_string()),
            ListKey::Index(i) => ListKeyS::Index(*i),
        }
    }

    fn mnode(&mut self, n: &MNode) -> MNodeS {
        match n {
            MNode::Empty => MNodeS::Empty,
            MNode::Text { node, text } => MNodeS::Text(*node, text.to_string()),
            MNode::Template(t) => MNodeS::Template {
                tid: t.tid,
                key: t.key.as_ref().map(|k| k.to_string()),
                root: t.root,
                elem: self.v(&Value::Elem(t.elem.clone())),
                holes: t
                    .holes
                    .iter()
                    .map(|h| match h {
                        MHole::Attr { node, value } => MHoleS::Attr(*node, self.v(value)),
                        MHole::Spread { node, value } => MHoleS::Spread(*node, self.v(value)),
                        MHole::Ref { node, value } => MHoleS::Ref(*node, self.v(value)),
                        MHole::Child {
                            parent,
                            next_static,
                            next_hole,
                            value,
                            mounted,
                        } => MHoleS::Child {
                            parent: *parent,
                            next_static: *next_static,
                            next_hole: *next_hole,
                            value: self.v(value),
                            mounted: self.mnode(mounted),
                        },
                    })
                    .collect(),
            },
            MNode::Component { inst } => MNodeS::Component(*inst),
            MNode::List {
                children,
                key,
                fragment,
            } => MNodeS::List(
                children
                    .iter()
                    .map(|(k, c)| (Self::key(k), self.mnode(c)))
                    .collect(),
                key.as_ref().map(|k| k.to_string()),
                *fragment,
            ),
            MNode::Provider {
                ctx,
                value,
                children,
                key,
            } => MNodeS::Provider(
                *ctx,
                self.v(value),
                children
                    .iter()
                    .map(|(k, c)| (Self::key(k), self.mnode(c)))
                    .collect(),
                key.as_ref().map(|k| k.to_string()),
            ),
            MNode::Portal {
                container,
                children,
                key,
            } => MNodeS::Portal(
                *container,
                children
                    .iter()
                    .map(|(k, c)| (Self::key(k), self.mnode(c)))
                    .collect(),
                key.as_ref().map(|k| k.to_string()),
            ),
        }
    }

    fn hook(&mut self, h: &HookState) -> HookS {
        match h {
            HookState::State { value, .. } => HookS::State(self.v(value)),
            HookState::Reducer { value, reducer, .. } => {
                HookS::Reducer(self.v(value), self.v(reducer))
            }
            HookState::Memo { value, deps } => HookS::Memo(
                self.v(value),
                deps.as_ref().map(|d| d.iter().map(|x| self.v(x)).collect()),
            ),
            HookState::Ref(r) => HookS::Ref(self.v(r)),
            HookState::Effect {
                layout,
                deps,
                cleanup,
                ..
            } => HookS::Effect(
                *layout,
                deps.as_ref().map(|d| d.iter().map(|x| self.v(x)).collect()),
                cleanup.as_ref().map(|c| self.v(c)),
            ),
            HookState::Context(c) => HookS::Context(*c),
            HookState::Id(s) => HookS::Id(s.to_string()),
            HookState::Store {
                value,
                get,
                subscribe,
                unsubscribe,
                ..
            } => HookS::Store(
                self.v(value),
                self.v(get),
                self.v(subscribe),
                unsubscribe.as_ref().map(|u| self.v(u)),
            ),
        }
    }
}

pub(crate) fn save(rt: &Runtime) -> UiState {
    let mut e = Enc {
        heap: Vec::new(),
        seen: BTreeMap::new(),
    };
    let globals = rt.globals.iter().map(|g| e.v(g)).collect();
    let ctx_defaults = rt.ctx_defaults.iter().map(|(k, v)| (*k, e.v(v))).collect();
    let instances = rt
        .instances
        .iter()
        .map(|(id, i)| InstanceS {
            id: *id,
            func: e.v(&i.func.value()),
            elem: i.elem.as_ref().map(|x| e.v(&Value::Elem(x.clone()))),
            props: e.v(&i.props),
            hooks: i.hooks.iter().map(|h| e.hook(h)).collect(),
            rendered: e.mnode(&i.rendered),
            parent: i.parent,
        })
        .collect();
    let root = e.mnode(&rt.root);
    let handlers = rt
        .handlers
        .iter()
        .map(|(n, hs)| {
            (
                *n,
                hs.iter().map(|(k, v)| (k.to_string(), e.v(v))).collect(),
            )
        })
        .collect();
    let form_props = rt
        .form_props
        .iter()
        .map(|(n, p)| {
            (
                *n,
                [
                    e.v(&p.value),
                    e.v(&p.default_value),
                    e.v(&p.checked),
                    e.v(&p.default_checked),
                    e.v(&p.ty),
                ],
            )
        })
        .collect();
    let raf: Vec<(u32, V)> = rt.raf.iter().map(|(i, f)| (*i, e.v(f))).collect();
    let media_lists: Vec<(String, V, Vec<V>)> = rt
        .media_lists
        .iter()
        .map(|(m, o, ls)| (m.clone(), e.v(o), ls.iter().map(|l| e.v(l)).collect()))
        .collect();
    let timers = rt
        .timers
        .iter()
        .map(|t| TimerS {
            id: t.id,
            due: t.due,
            interval: t.interval,
            callback: e.v(&t.callback),
            args: t.args.iter().map(|a| e.v(a)).collect(),
        })
        .collect();
    let i = &rt.inner;
    let cw = rt.cw.loaded.then(|| CwS {
        kind: rt.cw.kind.clone(),
        argument: rt.cw.argument.clone(),
        env: e.v(&rt.cw.env),
        state: e.v(&rt.cw.state),
        listeners: rt.cw.listeners.iter().map(|(i, l)| (*i, e.v(l))).collect(),
        next_listener: rt.cw.next_listener,
        next_request: rt.cw.next_request,
        declared: rt.cw.declared,
        pending: rt
            .cw
            .pending
            .iter()
            .map(|(id, (p, http))| (*id, e.v(&Value::Promise(p.clone())), *http))
            .collect(),
    });
    let listeners = rt
        .global_listeners
        .iter()
        .map(|l| (l.window, l.ty.to_string(), e.v(&l.f), l.capture))
        .collect();
    let (module, program) = match rt.program.module() {
        Some(m) => (Some((**m).clone()), None),
        None => (None, Some(rt.program.id())),
    };
    let island = rt.island.as_ref().map(|i| {
        let parts = i.image();
        IslandS {
            heap: parts.heap,
            js: parts.js,
            cw: parts.cw.iter().map(|(id, v)| (*id, e.v(v))).collect(),
            contexts: parts.contexts,
            next_context: parts.next_context,
            exports: parts.exports.iter().map(|v| e.v(v)).collect(),
            rng: parts.rng,
        }
    });
    let dyn_templates = rt.dyn_templates.iter().map(|(t, _)| t.clone()).collect();
    UiState {
        module,
        program,
        url: i.url.clone(),
        doc: i.doc.clone(),
        viewport: (
            i.viewport.width,
            i.viewport.height,
            i.viewport.scale,
            i.viewport.zoom,
        ),
        focused: i.focused,
        focus_visible: i.focus_visible,
        hovered: i.hovered,
        pointer: i.pointer,
        hover_stale: i.pointer.is_some() && i.hover_generation != i.generation,
        values: i.form.values.iter().map(|(k, v)| (*k, v.clone())).collect(),
        checked: i.form.checked.iter().map(|(k, v)| (*k, *v)).collect(),
        indeterminate: i.form.indeterminate.iter().copied().collect(),
        selection: i.form.selection.iter().map(|(k, v)| (*k, *v)).collect(),
        typeahead: i
            .form
            .typeahead
            .iter()
            .map(|(k, t)| (*k, t.buffer.clone(), t.last_ms, t.repeating))
            .collect(),
        scroll: i.scroll.iter().map(|(k, (x, y))| (*k, x.0, y.0)).collect(),
        images: i
            .images
            .0
            .iter()
            .map(|(k, (w, h))| (k.clone(), *w, *h))
            .collect(),
        heap: e.heap,
        globals,
        ctx_defaults,
        instances,
        next_inst: rt.next_inst,
        root,
        container: rt.container,
        handlers,
        controlled: rt
            .controlled
            .iter()
            .map(|(n, c)| match c {
                Controlled::Value(v) => (*n, Some(v.clone()), false),
                Controlled::Checked(b) => (*n, None, *b),
            })
            .collect(),
        form_props,
        timers,
        next_timer: rt.next_timer,
        raf,
        next_raf: rt.next_raf,
        media_lists,
        portal_parents: {
            let mut p: Vec<(NodeId, NodeId)> =
                rt.portal_parents.iter().map(|(a, b)| (*a, *b)).collect();
            p.sort();
            p
        },
        clock_ms: rt.clock_ms,
        start_micros: rt.start_micros,
        id_counter: rt.id_counter,
        booted: rt.booted,
        crashed: rt.crashed,
        listeners,
        cw,
        island,
        dyn_templates,
    }
}

// ------------------------------------------------------------------ decoding

struct Dec<'a> {
    heap: &'a [HeapObj],
    done: Vec<Option<Value>>,
    program: Rc<dyn Program>,
    tasks: BTreeMap<u32, Rc<RefCell<Option<crate::asyncfn::Task>>>>,
    alls: BTreeMap<u32, Rc<RefCell<AllState>>>,
    /// The island's handles, by id.
    foreign: BTreeMap<u32, Value>,
}

impl Dec<'_> {
    fn v(&mut self, v: &V) -> Result<Value, String> {
        Ok(match v {
            V::U => Value::Undefined,
            V::Null => Value::Null,
            V::B(b) => Value::Bool(*b),
            V::N(n) => Value::Num(*n),
            V::NonFinite(0) => Value::Num(f64::NAN),
            V::NonFinite(1) => Value::Num(f64::INFINITY),
            V::NonFinite(_) => Value::Num(f64::NEG_INFINITY),
            V::S(s) => Value::str(s),
            V::Setter(a, b) => Value::Setter(*a, *b),
            V::Dispatch(a, b) => Value::Dispatch(*a, *b),
            V::Node(n) => Value::Node(*n),
            V::Ctx(c) => Value::Context(*c),
            V::Foreign(id) => self
                .foreign
                .get(id)
                .cloned()
                .ok_or("a handle of the island its snapshot does not have")?,
            V::H(i) => self.heap_value(*i)?,
        })
    }

    fn heap_value(&mut self, i: u32) -> Result<Value, String> {
        let idx = i as usize;
        if let Some(Some(v)) = self.done.get(idx) {
            return Ok(v.clone());
        }
        let obj = self
            .heap
            .get(idx)
            .ok_or_else(|| format!("heap index {i} out of range"))?;
        let v = match obj {
            HeapObj::Array(items) => {
                // Registered before its items so cycles resolve to the same array.
                let a: Arr = Rc::new(RefCell::new(Vec::new()));
                self.done[idx] = Some(Value::Array(a.clone()));
                let mut out = Vec::with_capacity(items.len());
                for x in items {
                    out.push(self.v(x)?);
                }
                *a.borrow_mut() = out;
                return Ok(Value::Array(a));
            }
            HeapObj::Object(items) => {
                let o: Obj = Rc::new(RefCell::new(Vec::new()));
                self.done[idx] = Some(Value::Object(o.clone()));
                let mut out = Vec::with_capacity(items.len());
                for (k, x) in items {
                    out.push((Rc::from(k.as_str()), self.v(x)?));
                }
                *o.borrow_mut() = out;
                return Ok(Value::Object(o));
            }
            HeapObj::Ref(x) => {
                let r = Rc::new(RefCell::new(Value::Undefined));
                self.done[idx] = Some(Value::Ref(r.clone()));
                let inner = self.v(x)?;
                *r.borrow_mut() = inner;
                return Ok(Value::Ref(r));
            }
            HeapObj::Func(f, caps) => {
                let mut c = Vec::with_capacity(caps.len());
                for x in caps {
                    c.push(self.v(x)?);
                }
                Value::Func(Rc::new(Closure {
                    func: *f,
                    captures: c,
                }))
            }
            HeapObj::Template(tid, holes, key) => {
                let mut h = Vec::with_capacity(holes.len());
                for x in holes {
                    h.push(self.v(x)?);
                }
                Value::Elem(Rc::new(Elem::Template {
                    tid: *tid,
                    holes: h,
                    key: key.as_deref().map(Rc::from),
                }))
            }
            HeapObj::Component(f, props, key) => {
                let Value::Func(func) = self.heap_value(*f)? else {
                    return Err("component element without a function".into());
                };
                Value::Elem(Rc::new(Elem::Component {
                    func: ComponentFn::Compiled(func),
                    props: self.v(props)?,
                    key: key.as_deref().map(Rc::from),
                }))
            }
            HeapObj::Fragment(children, key) => {
                let mut c = Vec::with_capacity(children.len());
                for x in children {
                    c.push(self.v(x)?);
                }
                Value::Elem(Rc::new(Elem::Fragment {
                    children: c,
                    key: key.as_deref().map(Rc::from),
                }))
            }
            HeapObj::Portal(children, container, key) => {
                let mut c = Vec::with_capacity(children.len());
                for x in children {
                    c.push(self.v(x)?);
                }
                Value::Elem(Rc::new(Elem::Portal {
                    children: c,
                    container: *container,
                    key: key.as_deref().map(Rc::from),
                }))
            }
            HeapObj::Provider(ctx, value, children, key) => {
                let value = self.v(value)?;
                let mut c = Vec::with_capacity(children.len());
                for x in children {
                    c.push(self.v(x)?);
                }
                Value::Elem(Rc::new(Elem::Provider {
                    ctx: *ctx,
                    value,
                    children: c,
                    key: key.as_deref().map(Rc::from),
                }))
            }
            HeapObj::Response(r) => Value::Response(Rc::new(r.clone())),
            HeapObj::Error(name, message) => Value::error(name, message),
            HeapObj::Cell(x) => {
                let c = Rc::new(RefCell::new(Value::Undefined));
                self.done[idx] = Some(Value::Cell(c.clone()));
                let inner = self.v(x)?;
                *c.borrow_mut() = inner;
                return Ok(Value::Cell(c));
            }
            HeapObj::Set(items) => {
                let a: Arr = Rc::new(RefCell::new(Vec::new()));
                self.done[idx] = Some(Value::Set(a.clone()));
                let mut out = Vec::with_capacity(items.len());
                for x in items {
                    out.push(self.v(x)?);
                }
                *a.borrow_mut() = out;
                return Ok(Value::Set(a));
            }
            HeapObj::Map(items) => {
                let m = Rc::new(RefCell::new(Vec::new()));
                self.done[idx] = Some(Value::Map(m.clone()));
                let mut out = Vec::with_capacity(items.len());
                for (k, x) in items {
                    out.push((self.v(k)?, self.v(x)?));
                }
                *m.borrow_mut() = out;
                return Ok(Value::Map(m));
            }
            HeapObj::Regex(source, flags, last) => {
                let mut fl = cw_regex::Flags::default();
                for c in flags.chars() {
                    match c {
                        'i' => fl.ignore_case = true,
                        'm' => fl.multiline = true,
                        's' => fl.dot_all = true,
                        'u' | 'v' => fl.unicode = true,
                        _ => {}
                    }
                }
                let re = cw_regex::Regex::new(source, cw_regex::Flavor::JavaScript, fl)
                    .map_err(|e| e.message)?;
                Value::Regex(Rc::new(RegexObj {
                    source: Rc::from(source.as_str()),
                    flags: Rc::from(flags.as_str()),
                    re: Rc::new(re),
                    last_index: Cell::new(*last),
                }))
            }
            HeapObj::StoreChanged(inst, hook) => Value::Native(Rc::new(NativeFn::StoreChanged {
                inst: *inst,
                hook: *hook,
            })),
            HeapObj::CwOffEnv(id) => Value::Native(Rc::new(NativeFn::CwOffEnv(*id))),
            HeapObj::BuiltinFn(b) => Value::Native(Rc::new(NativeFn::Builtin(*b))),
            HeapObj::BoundMethod(recv, name) => Value::Native(Rc::new(NativeFn::BoundMethod {
                recv: self.v(recv)?,
                name: Rc::from(name.as_str()),
            })),
            HeapObj::ComponentOf(f, props, key) => {
                let func =
                    ComponentFn::of(self.v(f)?).ok_or("component element without a function")?;
                Value::Elem(Rc::new(Elem::Component {
                    func,
                    props: self.v(props)?,
                    key: key.as_deref().map(Rc::from),
                }))
            }
            HeapObj::ImperativeSet(r, create) => Value::Native(Rc::new(NativeFn::ImperativeSet {
                r: self.v(r)?,
                create: self.v(create)?,
            })),
            HeapObj::MediaListen(list, add) => Value::Native(Rc::new(NativeFn::MediaListen {
                list: *list,
                add: *add,
            })),
            HeapObj::ImperativeClear(r) => {
                Value::Native(Rc::new(NativeFn::ImperativeClear(self.v(r)?)))
            }
            HeapObj::Date(t) => Value::Date(Rc::new(Cell::new(t.unwrap_or(f64::NAN)))),
            HeapObj::Promise(state, value, reactions) => {
                // Registered first: reactions and values may lead back to it.
                let p = crate::interp::new_promise();
                self.done[idx] = Some(Value::Promise(p.clone()));
                let value = match value {
                    Some(v) => self.v(v)?,
                    None => Value::Undefined,
                };
                let mut rs = Vec::with_capacity(reactions.len());
                for (kind, ok, bad, result) in reactions {
                    rs.push(Reaction {
                        kind: match kind {
                            0 => ReactionKind::Then,
                            1 => ReactionKind::Catch,
                            _ => ReactionKind::Finally,
                        },
                        on_fulfilled: ok.as_ref().map(|f| self.v(f)).transpose()?,
                        on_rejected: bad.as_ref().map(|f| self.v(f)).transpose()?,
                        result: self.promise(result)?,
                    });
                }
                {
                    let mut pb = p.borrow_mut();
                    pb.state = match state {
                        0 => PromiseState::Pending,
                        1 => PromiseState::Fulfilled(value),
                        _ => PromiseState::Rejected(value),
                    };
                    pb.reactions = rs;
                }
                return Ok(Value::Promise(p));
            }
            HeapObj::Resolver(p, reject) => Value::Native(Rc::new(NativeFn::Resolver {
                promise: self.promise(p)?,
                reject: *reject,
            })),
            HeapObj::AllSlot(state, index) => Value::Native(Rc::new(NativeFn::AllSlot {
                state: self.all(state)?,
                index: *index,
            })),
            HeapObj::AllReject(state) => {
                Value::Native(Rc::new(NativeFn::AllReject(self.all(state)?)))
            }
            HeapObj::Resume(task, throw) => Value::Native(Rc::new(NativeFn::Resume {
                task: self.task(task)?,
                throw: *throw,
            })),
            HeapObj::AllState(..) | HeapObj::Task(_) => {
                return Err("internal state where a value belongs".into())
            }
            HeapObj::Opaque => Value::Promise(crate::interp::new_promise()),
        };
        self.done[idx] = Some(v.clone());
        Ok(v)
    }

    fn promise(&mut self, v: &V) -> Result<Rc<RefCell<Promise>>, String> {
        match self.v(v)? {
            Value::Promise(p) => Ok(p),
            _ => Err("expected a promise".into()),
        }
    }

    fn all(&mut self, v: &V) -> Result<Rc<RefCell<AllState>>, String> {
        let V::H(i) = v else {
            return Err("expected a Promise.all".into());
        };
        if let Some(a) = self.alls.get(i) {
            return Ok(a.clone());
        }
        let Some(HeapObj::AllState(values, remaining, result, done)) = self.heap.get(*i as usize)
        else {
            return Err("expected a Promise.all".into());
        };
        let result_p = self.promise(result)?;
        let a = Rc::new(RefCell::new(AllState {
            values: Vec::new(),
            remaining: *remaining,
            result: result_p,
            done: *done,
        }));
        self.alls.insert(*i, a.clone());
        let values = values.iter().map(|x| self.v(x)).collect::<Result<_, _>>()?;
        a.borrow_mut().values = values;
        Ok(a)
    }

    fn task(&mut self, v: &V) -> Result<Rc<RefCell<Option<crate::asyncfn::Task>>>, String> {
        let V::H(i) = v else {
            return Err("expected a task".into());
        };
        if let Some(t) = self.tasks.get(i) {
            return Ok(t.clone());
        }
        let Some(HeapObj::Task(t)) = self.heap.get(*i as usize) else {
            return Err("expected a task".into());
        };
        let cell = Rc::new(RefCell::new(None));
        self.tasks.insert(*i, cell.clone());
        if let Some(t) = t {
            let program = self.program.clone();
            let task = crate::asyncfn::decode(&program, t, &mut |x| self.v(x))?;
            *cell.borrow_mut() = Some(task);
        }
        Ok(cell)
    }

    fn func(&mut self, v: &V) -> Result<ComponentFn, String> {
        ComponentFn::of(self.v(v)?).ok_or_else(|| "expected a function".into())
    }

    fn elem(&mut self, v: &V) -> Result<Rc<Elem>, String> {
        match self.v(v)? {
            Value::Elem(e) => Ok(e),
            _ => Err("expected an element".into()),
        }
    }

    fn key(k: &ListKeyS) -> ListKey {
        match k {
            ListKeyS::Key(s) => ListKey::Key(Rc::from(s.as_str())),
            ListKeyS::Index(i) => ListKey::Index(*i),
        }
    }

    fn mnode(&mut self, n: &MNodeS) -> Result<MNode, String> {
        Ok(match n {
            MNodeS::Empty => MNode::Empty,
            MNodeS::Text(node, text) => MNode::Text {
                node: *node,
                text: Rc::from(text.as_str()),
            },
            MNodeS::Template {
                tid,
                key,
                root,
                elem,
                holes,
            } => {
                let mut hs = Vec::with_capacity(holes.len());
                for h in holes {
                    hs.push(match h {
                        MHoleS::Attr(node, v) => MHole::Attr {
                            node: *node,
                            value: self.v(v)?,
                        },
                        MHoleS::Spread(node, v) => MHole::Spread {
                            node: *node,
                            value: self.v(v)?,
                        },
                        MHoleS::Ref(node, v) => MHole::Ref {
                            node: *node,
                            value: self.v(v)?,
                        },
                        MHoleS::Child {
                            parent,
                            next_static,
                            next_hole,
                            value,
                            mounted,
                        } => MHole::Child {
                            parent: *parent,
                            next_static: *next_static,
                            next_hole: *next_hole,
                            value: self.v(value)?,
                            mounted: self.mnode(mounted)?,
                        },
                    });
                }
                MNode::Template(Box::new(MTemplate {
                    tid: *tid,
                    key: key.as_deref().map(Rc::from),
                    root: *root,
                    elem: self.elem(elem)?,
                    holes: hs,
                }))
            }
            MNodeS::Component(i) => MNode::Component { inst: *i },
            MNodeS::List(children, key, fragment) => {
                let mut c = Vec::with_capacity(children.len());
                for (k, m) in children {
                    c.push((Self::key(k), self.mnode(m)?));
                }
                MNode::List {
                    children: c,
                    key: key.as_deref().map(Rc::from),
                    fragment: *fragment,
                }
            }
            MNodeS::Provider(ctx, value, children, key) => {
                let value = self.v(value)?;
                let mut c = Vec::with_capacity(children.len());
                for (k, m) in children {
                    c.push((Self::key(k), self.mnode(m)?));
                }
                MNode::Provider {
                    ctx: *ctx,
                    value,
                    children: c,
                    key: key.as_deref().map(Rc::from),
                }
            }
            MNodeS::Portal(container, children, key) => {
                let mut c = Vec::with_capacity(children.len());
                for (k, m) in children {
                    c.push((Self::key(k), self.mnode(m)?));
                }
                MNode::Portal {
                    container: *container,
                    children: c,
                    key: key.as_deref().map(Rc::from),
                }
            }
        })
    }
}

/// The program a snapshot was taken of, when it can say: its own IR, or a
/// registered generated program it names.
pub(crate) fn own_program(s: &UiState) -> Result<Rc<dyn Program>, String> {
    if let Some(m) = &s.module {
        if m.version != crate::ir::IR_VERSION {
            return Err(format!("IR version {}", m.version));
        }
        return Ok(Rc::new(crate::program::IrProgram::new(m.clone())));
    }
    match &s.program {
        Some(id) => match crate::program::find(id) {
            Some(p) => Ok(Rc::new(crate::program::StaticProgram(p))),
            None => Err(format!(
                "program {} ({}) is not registered: restore it with UiApp::restore_with",
                id.name, id.hash
            )),
        },
        None => Err("a snapshot with neither IR nor a program".into()),
    }
}

/// Whether `program` is the program the snapshot was taken of.
pub(crate) fn check_program(s: &UiState, program: &dyn Program) -> Result<(), String> {
    let hash = match (&s.program, &s.module) {
        (Some(id), _) => id.hash.clone(),
        (None, Some(m)) => {
            if m.version != crate::ir::IR_VERSION {
                return Err(format!("IR version {}", m.version));
            }
            crate::program::hash_hex(crate::program::ir_hash(m))
        }
        (None, None) => return Err("a snapshot with neither IR nor a program".into()),
    };
    let ours = crate::program::hash_hex(program.ir_hash());
    if hash != ours {
        return Err(format!(
            "the snapshot is of IR {hash}, the program is IR {ours}"
        ));
    }
    Ok(())
}

pub(crate) fn load(
    s: &UiState,
    program: Rc<dyn Program>,
    host: Box<dyn ScriptHostDocument>,
) -> Result<Runtime, String> {
    let mut rt = Runtime::new(program.clone(), host, &s.url);
    // The island's VM first: cw-ui's values name its objects by handle.
    let mut foreign = BTreeMap::new();
    if let Some(is) = &s.island {
        foreign = rt.load_island(
            &is.heap,
            &is.js,
            &is.cw,
            &is.contexts,
            is.next_context,
            is.rng,
        )?;
    }
    for tag in &s.dyn_templates {
        rt.dyn_template(tag);
    }
    let mut d = Dec {
        heap: &s.heap,
        done: vec![None; s.heap.len()],
        program,
        tasks: BTreeMap::new(),
        alls: BTreeMap::new(),
        foreign,
    };
    if let Some(is) = &s.island {
        let cw = is
            .cw
            .iter()
            .map(|(id, v)| Ok((*id, d.v(v)?)))
            .collect::<Result<Vec<_>, String>>()?;
        let exports = is
            .exports
            .iter()
            .map(|v| d.v(v))
            .collect::<Result<Vec<_>, _>>()?;
        rt.finish_island(cw, exports);
    }
    rt.globals = s.globals.iter().map(|g| d.v(g)).collect::<Result<_, _>>()?;
    for (k, v) in &s.ctx_defaults {
        let v = d.v(v)?;
        rt.ctx_defaults.insert(*k, v);
    }
    for i in &s.instances {
        let mut hooks = Vec::with_capacity(i.hooks.len());
        for h in &i.hooks {
            hooks.push(match h {
                HookS::State(v) => HookState::State {
                    value: d.v(v)?,
                    queue: Vec::new(),
                },
                HookS::Reducer(v, r) => HookState::Reducer {
                    value: d.v(v)?,
                    reducer: d.v(r)?,
                    queue: Vec::new(),
                },
                HookS::Memo(v, deps) => HookState::Memo {
                    value: d.v(v)?,
                    deps: match deps {
                        Some(ds) => Some(ds.iter().map(|x| d.v(x)).collect::<Result<_, _>>()?),
                        None => None,
                    },
                },
                HookS::Ref(r) => HookState::Ref(d.v(r)?),
                HookS::Effect(layout, deps, cleanup) => HookState::Effect {
                    layout: *layout,
                    deps: match deps {
                        Some(ds) => Some(ds.iter().map(|x| d.v(x)).collect::<Result<_, _>>()?),
                        None => None,
                    },
                    pending: None,
                    cleanup: match cleanup {
                        Some(c) => Some(d.v(c)?),
                        None => None,
                    },
                },
                HookS::Context(c) => HookState::Context(*c),
                HookS::Id(s) => HookState::Id(Rc::from(s.as_str())),
                HookS::Store(v, g, s, u) => HookState::Store {
                    value: d.v(v)?,
                    get: d.v(g)?,
                    subscribe: d.v(s)?,
                    unsubscribe: match u {
                        Some(u) => Some(d.v(u)?),
                        None => None,
                    },
                    needs_subscribe: false,
                },
            });
        }
        let inst = Instance {
            func: d.func(&i.func)?,
            elem: match &i.elem {
                Some(e) => Some(d.elem(e)?),
                None => None,
            },
            props: d.v(&i.props)?,
            hooks,
            rendered: d.mnode(&i.rendered)?,
            parent: i.parent,
            dirty: false,
            subtree_dirty: false,
            context_changed: false,
            cache: ElemCache::default(),
        };
        rt.instances.insert(i.id, inst);
    }
    rt.next_inst = s.next_inst;
    rt.root = d.mnode(&s.root)?;
    rt.container = s.container;
    for (n, hs) in &s.handlers {
        let mut list = Vec::with_capacity(hs.len());
        for (k, v) in hs {
            list.push((Rc::from(k.as_str()), d.v(v)?));
        }
        rt.handlers.insert(*n, list);
    }
    for (n, v, b) in &s.controlled {
        rt.controlled.insert(
            *n,
            match v {
                Some(v) => Controlled::Value(v.clone()),
                None => Controlled::Checked(*b),
            },
        );
    }
    for (n, p) in &s.form_props {
        rt.form_props.insert(
            *n,
            FormProps {
                value: d.v(&p[0])?,
                default_value: d.v(&p[1])?,
                checked: d.v(&p[2])?,
                default_checked: d.v(&p[3])?,
                ty: d.v(&p[4])?,
            },
        );
    }
    for t in &s.timers {
        rt.timers.push(Timer {
            id: t.id,
            due: t.due,
            interval: t.interval,
            callback: d.v(&t.callback)?,
            args: t.args.iter().map(|a| d.v(a)).collect::<Result<_, _>>()?,
        });
    }
    rt.next_timer = s.next_timer;
    for (i, f) in &s.raf {
        rt.raf.push((*i, d.v(f)?));
    }
    rt.next_raf = s.next_raf;
    rt.portal_parents = s.portal_parents.iter().copied().collect();
    for (m, o, ls) in &s.media_lists {
        let ls = ls.iter().map(|l| d.v(l)).collect::<Result<Vec<_>, _>>()?;
        rt.media_lists.push((m.clone(), d.v(o)?, ls));
    }
    rt.clock_ms = s.clock_ms;
    rt.start_micros = s.start_micros;
    rt.id_counter = s.id_counter;
    rt.booted = s.booted;
    rt.crashed = s.crashed;
    if let Some(c) = &s.cw {
        rt.cw = crate::cw::CwBridge {
            loaded: true,
            kind: c.kind.clone(),
            argument: c.argument.clone(),
            env: d.v(&c.env)?,
            state: d.v(&c.state)?,
            listeners: c
                .listeners
                .iter()
                .map(|(i, l)| Ok((*i, d.v(l)?)))
                .collect::<Result<_, String>>()?,
            next_listener: c.next_listener,
            next_request: c.next_request,
            declared: c.declared,
            pending: c
                .pending
                .iter()
                .map(|(id, p, http)| match d.v(p)? {
                    Value::Promise(p) => Ok((*id, (p, *http))),
                    _ => Err("a cw request without its promise".to_owned()),
                })
                .collect::<Result<_, String>>()?,
        };
    }
    for (window, ty, f, capture) in &s.listeners {
        rt.global_listeners.push(GlobalListener {
            window: *window,
            ty: Rc::from(ty.as_str()),
            f: d.v(f)?,
            capture: *capture,
        });
    }
    let i = &mut rt.inner;
    i.doc = s.doc.clone();
    i.url = s.url.clone();
    i.viewport = cw_web::Viewport {
        width: s.viewport.0,
        height: s.viewport.1,
        scale: s.viewport.2,
        zoom: s.viewport.3,
    };
    i.ready_state = "complete".into();
    i.focused = s.focused;
    i.focus_visible = s.focus_visible;
    i.hovered = s.hovered;
    i.pointer = s.pointer;
    i.form.values = s.values.iter().cloned().collect();
    i.form.checked = s.checked.iter().cloned().collect();
    i.form.indeterminate = s.indeterminate.iter().copied().collect();
    i.form.selection = s.selection.iter().cloned().collect();
    i.form.typeahead = s
        .typeahead
        .iter()
        .map(|(k, b, t, r)| {
            (
                *k,
                cw_web::script::inner::TypeAhead {
                    buffer: b.clone(),
                    last_ms: *t,
                    repeating: *r,
                },
            )
        })
        .collect();
    i.scroll = s
        .scroll
        .iter()
        .map(|(n, x, y)| (*n, (Au(*x), Au(*y))))
        .collect();
    for (src, w, h) in &s.images {
        i.images.0.insert(src.clone(), (*w, *h));
    }
    i.touch();
    // A stale hover is hit-tested again at the next idle point, as it would have
    // been had the app not been snapshotted.
    i.hover_generation = if s.hover_stale {
        u64::MAX
    } else {
        i.generation
    };
    let _ = Cell::new(0);
    Ok(rt)
}

impl UiState {
    /// The state as JSON (what a checkpoint stores).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("state serialises")
    }
    pub fn from_json(s: &str) -> Result<UiState, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}

#[allow(dead_code)]
fn _doc_is_serde(d: &Document) -> Document {
    d.clone()
}
