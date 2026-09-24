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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiState {
    pub module: Module,
    pub url: String,
    pub doc: Document,
    pub viewport: (u32, u32, u8, u16),
    pub focused: Option<NodeId>,
    pub focus_visible: bool,
    pub hovered: Option<NodeId>,
    pub values: Vec<(NodeId, String)>,
    pub checked: Vec<(NodeId, bool)>,
    pub indeterminate: Vec<NodeId>,
    pub selection: Vec<(NodeId, (usize, usize))>,
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
    pub clock_ms: f64,
    pub start_micros: i64,
    pub id_counter: u32,
    pub booted: bool,
    pub crashed: bool,
}

// ------------------------------------------------------------------ encoding

struct Enc {
    heap: Vec<HeapObj>,
    seen: BTreeMap<usize, u32>,
}

impl Enc {
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
                            let f = self.v(&Value::Func(func.clone()));
                            let V::H(fi) = f else { unreachable!() };
                            HeapObj::Component(
                                fi,
                                self.v(props),
                                key.as_ref().map(|k| k.to_string()),
                            )
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
            Value::Event(_) | Value::Promise(_) => {
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
            func: e.v(&Value::Func(i.func.clone())),
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
    UiState {
        module: (*rt.module).clone(),
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
        values: i.form.values.iter().map(|(k, v)| (*k, v.clone())).collect(),
        checked: i.form.checked.iter().map(|(k, v)| (*k, *v)).collect(),
        indeterminate: i.form.indeterminate.iter().copied().collect(),
        selection: i.form.selection.iter().map(|(k, v)| (*k, *v)).collect(),
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
        clock_ms: rt.clock_ms,
        start_micros: rt.start_micros,
        id_counter: rt.id_counter,
        booted: rt.booted,
        crashed: rt.crashed,
    }
}

// ------------------------------------------------------------------ decoding

struct Dec<'a> {
    heap: &'a [HeapObj],
    done: Vec<Option<Value>>,
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
                    func,
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
            HeapObj::Opaque => Value::Promise(crate::interp::new_promise()),
        };
        self.done[idx] = Some(v.clone());
        Ok(v)
    }

    fn func(&mut self, v: &V) -> Result<Rc<Closure>, String> {
        match self.v(v)? {
            Value::Func(c) => Ok(c),
            _ => Err("expected a function".into()),
        }
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
        })
    }
}

pub(crate) fn load(s: &UiState, host: Box<dyn ScriptHostDocument>) -> Result<Runtime, String> {
    if s.module.version != crate::ir::IR_VERSION {
        return Err(format!("IR version {}", s.module.version));
    }
    let mut rt = Runtime::new(Rc::new(s.module.clone()), host, &s.url);
    let mut d = Dec {
        heap: &s.heap,
        done: vec![None; s.heap.len()],
    };
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
    rt.clock_ms = s.clock_ms;
    rt.start_micros = s.start_micros;
    rt.id_counter = s.id_counter;
    rt.booted = s.booted;
    rt.crashed = s.crashed;
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
    i.form.values = s.values.iter().cloned().collect();
    i.form.checked = s.checked.iter().cloned().collect();
    i.form.indeterminate = s.indeterminate.iter().copied().collect();
    i.form.selection = s.selection.iter().cloned().collect();
    i.scroll = s
        .scroll
        .iter()
        .map(|(n, x, y)| (*n, (Au(*x), Au(*y))))
        .collect();
    for (src, w, h) in &s.images {
        i.images.0.insert(src.clone(), (*w, *h));
    }
    i.touch();
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
