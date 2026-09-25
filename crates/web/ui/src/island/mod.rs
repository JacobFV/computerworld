//! Islands: the part of a compiled app that runs on the JS VM.
//!
//! An app's code outside the compiled subset, and the npm packages it imports,
//! run on a `cw_jsvm` VM inside the app, beside the compiled code, over one
//! document and one React: cw-ui's. The VM's `react` is a shim (`shim.js`) whose
//! hooks are cw-ui's hooks of the component cw-ui is rendering and whose elements
//! cw-ui reconciles, so a component on either side can render one from the other,
//! share context with it, and pass it callbacks, refs and children.
//!
//! The boundary.
//!
//! * Primitives cross as themselves.
//! * A VM object or function held by cw-ui is a `Value::Foreign`: a handle to it,
//!   canonical per object (the same object is always the same handle, so `===`
//!   and React's dependency comparisons see identity), read and called on the VM
//!   every time (`get_member`, calls, `invoke`, iteration all go to the VM), so it
//!   is always the object as it is now.
//! * A cw-ui function held by the VM is a VM function calling it; any other cw-ui
//!   object (an array, an object, a DOM node, a ref, an event) is a VM `Proxy`
//!   whose traps read and write it where it lives. Both are canonical per value.
//! * Elements convert: a VM element (`{ $$typeof, type, props, key, ref }`) is,
//!   for cw-ui, a host element (a template of one element made for its tag), a
//!   component of either side, a fragment or a provider; a cw-ui element is, for
//!   the VM, the element it stands for, with its props (a compiled template's
//!   holes and attributes are its props again), so a package may inspect or clone
//!   it.
//!
//! Re-entrancy. cw-ui calls into the VM, which may call back into cw-ui (a hook,
//! a compiled callback), which may call into the VM again. The runtime the VM's
//! natives reach is a raw pointer set by every entry into the VM, valid for that
//! entry (the runtime does not move while it runs); the VM is reached only
//! through the island, one call at a time on this thread.
//!
//! Determinism. The VM's clock is the app's world clock and its entropy a stream
//! seeded from the host's when the island starts; its timers are cw-ui's timers
//! and its promise jobs run whenever cw-ui settles, so a session replays exactly.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use cw_jsvm::value::{Args, Ctl, FuncImpl, JsResult, Kind, Obj, ObjData, Value as Js};
use cw_jsvm::vm::Vm;
use cw_script_host::{FileStat, FsError, FsErrorKind, ScriptHost};

use crate::runtime::{Runtime, Throw, R};
use crate::value::{Elem, Foreign, Str, Value};

const SHIM: &str = include_str!("shim.js");

/// The island's view of the world: the app's clock and a seeded entropy stream.
struct IslandHost {
    now: Rc<Cell<i64>>,
    rng: u64,
}

impl ScriptHost for IslandHost {
    fn read_file(&mut self, _path: &str) -> Result<Vec<u8>, FsError> {
        Err(FsError::new(FsErrorKind::NotFound))
    }
    fn write_file(&mut self, _path: &str, _data: &[u8], _append: bool) -> Result<(), FsError> {
        Err(FsError::new(FsErrorKind::PermissionDenied))
    }
    fn stat(&mut self, _path: &str) -> Result<FileStat, FsError> {
        Err(FsError::new(FsErrorKind::NotFound))
    }
    fn list_dir(&mut self, _path: &str) -> Result<Vec<String>, FsError> {
        Err(FsError::new(FsErrorKind::NotFound))
    }
    fn mkdir(&mut self, _path: &str, _parents: bool) -> Result<(), FsError> {
        Err(FsError::new(FsErrorKind::PermissionDenied))
    }
    fn remove(&mut self, _path: &str, _recursive: bool) -> Result<(), FsError> {
        Err(FsError::new(FsErrorKind::PermissionDenied))
    }
    fn rename(&mut self, _from: &str, _to: &str) -> Result<(), FsError> {
        Err(FsError::new(FsErrorKind::PermissionDenied))
    }
    fn cwd(&self) -> String {
        "/".into()
    }
    fn chdir(&mut self, _path: &str) -> Result<(), FsError> {
        Ok(())
    }
    fn resolve(&self, path: &str) -> String {
        if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        }
    }
    fn now_micros(&self) -> i64 {
        self.now.get()
    }
    fn random_u64(&mut self) -> u64 {
        // xorshift64*: a fixed function of the seed the island started with.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn user(&self) -> String {
        "user".into()
    }
    fn hostname(&self) -> String {
        "browser".into()
    }
    fn pid(&self) -> u64 {
        1
    }
}

/// What the VM's natives reach: the runtime of the current entry.
pub(crate) struct Link {
    rt: Cell<*mut Runtime>,
}

/// The VM of an app and the two tables of values crossing its boundary.
pub struct Island {
    vm: *mut Vm<'static>,
    host: *mut IslandHost,
    now: Rc<Cell<i64>>,
    link: Rc<Link>,
    /// The `__cw` object the shim and the natives share.
    cw: Obj,
    /// VM values cw-ui holds, by handle id; the handles' drops queue here.
    js_vals: Vec<Option<Js>>,
    js_free: Vec<u32>,
    /// VM object (by address) to its handle, while both live.
    js_ids: HashMap<usize, (Weak<RefCell<ObjData>>, Weak<Foreign>)>,
    drops: Rc<RefCell<Vec<u32>>>,
    /// cw-ui values the VM holds (functions and proxies), by id, with the VM
    /// object that stands for each (weakly: when it is gone, so is the entry).
    cw_vals: Vec<Option<(Value, Weak<RefCell<ObjData>>)>>,
    cw_free: Vec<u32>,
    cw_ids: HashMap<Identity, u32>,
    /// The VM context objects of cw-ui's contexts, by id.
    contexts: HashMap<u32, Obj>,
    /// Ids for contexts the VM creates (after every compiled one).
    next_context: u32,
    /// The island's exports compiled code imports, in the program's order.
    pub exports: Vec<Value>,
}

impl Drop for Island {
    fn drop(&mut self) {
        // SAFETY: both were made by `Box::into_raw` in `Island::new`; the VM borrows
        // the host, so it goes first.
        unsafe {
            drop(Box::from_raw(self.vm));
            drop(Box::from_raw(self.host));
        }
    }
}

impl std::fmt::Debug for Island {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Island")
            .field("js_vals", &self.js_vals.len())
            .field("cw_vals", &self.cw_vals.len())
            .finish()
    }
}

/// The island of the runtime a native of the VM runs for.
fn rt_of(vm: &Vm) -> &'static mut Runtime {
    let link = vm.embedder::<Link>().expect("an island's VM");
    // SAFETY: set by the entry into the VM that is running this native (see the
    // module documentation); the runtime outlives the entry.
    unsafe { &mut *link.rt.get() }
}

fn slot_id(a: &Args) -> u32 {
    match &a.callee.borrow().kind {
        Kind::Function(fd) => match &fd.imp {
            FuncImpl::Native { slots, .. } => match slots.first() {
                Some(Js::Num(n)) => *n as u32,
                _ => u32::MAX,
            },
            _ => u32::MAX,
        },
        _ => u32::MAX,
    }
}

/// A cw-ui error as a VM exception, and back.
fn throw_to_js(rt: &mut Runtime, t: Throw) -> Ctl {
    match t {
        Throw::Value(v) => Ctl::Throw(rt.js_value(&v)),
        Throw::Short => Ctl::Throw(Js::Undefined),
    }
}

impl Runtime {
    /// Starts the island: a VM with the React shim, running `script`, whose
    /// `__cw_exports` array are the values compiled code imports.
    pub(crate) fn start_island(&mut self, script: &str) -> R<()> {
        let now = Rc::new(Cell::new(self.inner.host_now_micros()));
        let seed = self.inner.host_random_u64() | 1;
        let host = Box::into_raw(Box::new(IslandHost {
            now: now.clone(),
            rng: seed,
        }));
        // SAFETY: the host lives until the island drops, after the VM.
        let host_ref: &'static mut IslandHost = unsafe { &mut *host };
        let vm = Box::into_raw(Box::new(Vm::new(
            host_ref,
            vec!["/island".into()],
            Vec::new(),
            None,
        )));
        let link = Rc::new(Link {
            rt: Cell::new(std::ptr::null_mut()),
        });
        // SAFETY: just made; the island owns it from here.
        let vmr = unsafe { &mut *vm };
        let any: Rc<dyn std::any::Any> = link.clone();
        vmr.embedder = Some(any);
        let cw = vmr.new_object();
        for (name, len, f) in NATIVES {
            vmr.method(&cw, name, *len, *f);
        }
        vmr.set_global("__cw", Js::Obj(cw.clone()));
        self.island = Some(Box::new(Island {
            vm,
            host,
            now,
            link,
            cw,
            js_vals: Vec::new(),
            js_free: Vec::new(),
            js_ids: HashMap::new(),
            drops: Rc::new(RefCell::new(Vec::new())),
            cw_vals: Vec::new(),
            cw_free: Vec::new(),
            cw_ids: HashMap::new(),
            contexts: HashMap::new(),
            next_context: 1 << 24,
            exports: Vec::new(),
        }));
        self.js_eval(SHIM, "cw-island-shim.js")?;
        let exports = self.js_eval(&format!("{script}\n;__cw_exports"), "island.js")?;
        let list = self.js_to_array(&exports)?;
        let values: Vec<Value> = list.iter().map(|v| self.cw_value(v)).collect();
        if let Some(i) = self.island.as_mut() {
            i.exports = values;
        }
        Ok(())
    }

    /// The island's VM, for one call. See the module documentation.
    fn vm(&mut self) -> &'static mut Vm<'static> {
        let me = self as *mut Runtime;
        let clock = self.start_micros + (self.clock_ms * 1000.0) as i64;
        let island = self.island.as_ref().expect("an island");
        island.link.rt.set(me);
        island.now.set(clock);
        // SAFETY: the VM lives as long as the island, which the runtime keeps; it is
        // used for one call at a time on this thread.
        unsafe { &mut *island.vm }
    }

    fn island_mut(&mut self) -> &mut Island {
        self.island.as_mut().expect("an island")
    }

    fn js_eval(&mut self, src: &str, file: &str) -> R<Js> {
        let vm = self.vm();
        let r = vm.eval_source_with(src, file, true, true);
        let r = self.js_result(r)?;
        self.run_js_jobs();
        Ok(r)
    }

    /// A VM result as a cw-ui one.
    fn js_result(&mut self, r: JsResult<Js>) -> R<Js> {
        match r {
            Ok(v) => Ok(v),
            Err(Ctl::Throw(e)) | Err(Ctl::Fatal(e)) => Err(Throw::Value(self.cw_value(&e))),
            Err(Ctl::Exit(_)) => Err(Throw::Value(Value::error("Error", "process.exit"))),
        }
    }

    /// Runs the VM's promise jobs (cw-ui's settle loop calls this).
    pub(crate) fn run_js_jobs(&mut self) -> bool {
        if self.island.is_none() {
            return false;
        }
        let vm = self.vm();
        if vm.microtasks.is_empty() && vm.ticks.is_empty() {
            return false;
        }
        if let Err(Ctl::Throw(e)) = vm.run_microtasks() {
            let e = self.cw_value(&e);
            self.report(Throw::Value(e));
        }
        self.collect_island();
        true
    }

    fn js_helper(&mut self, name: &str) -> Js {
        let cw = Js::Obj(self.island.as_ref().expect("an island").cw.clone());
        let vm = self.vm();
        vm.get_str(&cw, name).unwrap_or(Js::Undefined)
    }

    /// Calls a helper the shim put on `__cw`.
    fn js_call_helper(&mut self, name: &str, args: Vec<Js>) -> R<Js> {
        let f = self.js_helper(name);
        let vm = self.vm();
        let r = vm.call(&f, Js::Undefined, args);
        self.js_result(r)
    }

    fn js_to_array(&mut self, v: &Js) -> R<Vec<Js>> {
        let arr = self.js_call_helper("toArray", vec![v.clone()])?;
        match &arr {
            Js::Obj(o) => match &o.borrow().kind {
                Kind::Array(items) => Ok(items
                    .iter()
                    .map(|x| match x {
                        Js::Empty => Js::Undefined,
                        x => x.clone(),
                    })
                    .collect()),
                _ => Ok(Vec::new()),
            },
            _ => Ok(Vec::new()),
        }
    }

    /// Frees the handles cw-ui dropped and the entries of VM objects that died.
    pub(crate) fn collect_island(&mut self) {
        let Some(island) = self.island.as_mut() else {
            return;
        };
        let dropped: Vec<u32> = std::mem::take(&mut *island.drops.borrow_mut());
        for id in dropped {
            if let Some(Some(v)) = island.js_vals.get(id as usize) {
                if let Js::Obj(o) = v {
                    // Unless a newer handle names the object now.
                    let stale = island
                        .js_ids
                        .get(&o.addr())
                        .is_some_and(|(_, f)| f.upgrade().is_none());
                    if stale {
                        island.js_ids.remove(&o.addr());
                    }
                }
                island.js_vals[id as usize] = None;
                island.js_free.push(id);
            }
        }
        let mut dead = Vec::new();
        for (i, e) in island.cw_vals.iter().enumerate() {
            if let Some((_, w)) = e {
                if w.strong_count() == 0 {
                    dead.push(i);
                }
            }
        }
        for i in dead {
            if let Some((v, _)) = island.cw_vals[i].take() {
                if let Some(k) = identity(&v) {
                    island.cw_ids.remove(&k);
                }
            }
            island.cw_free.push(i as u32);
        }
    }

    // ------------------------------------------------------------ marshalling

    /// A VM value as cw-ui sees it.
    pub(crate) fn cw_value(&mut self, v: &Js) -> Value {
        match v {
            Js::Undefined | Js::Empty => Value::Undefined,
            Js::Null => Value::Null,
            Js::Bool(b) => Value::Bool(*b),
            Js::Num(n) => Value::Num(*n),
            Js::Str(s) => Value::str(s.as_str()),
            Js::Obj(o) => {
                // One of ours: the cw-ui value it stands for.
                if let Some(v) = self.unwrap_js(o) {
                    return v;
                }
                if let Some(e) = self.js_element(o) {
                    return e;
                }
                if let Kind::Date(t) = o.borrow().kind {
                    return Value::Date(Rc::new(Cell::new(t)));
                }
                self.foreign(v.clone())
            }
            other => self.foreign(other.clone()),
        }
    }

    /// The handle of a VM value, the same for the same object.
    fn foreign(&mut self, v: Js) -> Value {
        let callable = matches!(&v, Js::Obj(o) if o.is_callable());
        let array = matches!(&v, Js::Obj(o) if o.is_array_or_proxy());
        let island = self.island_mut();
        let key = match &v {
            Js::Obj(o) => Some((o.addr(), Rc::downgrade(&o.0))),
            _ => None,
        };
        if let Some((addr, _)) = &key {
            if let Some((w, f)) = island.js_ids.get(addr) {
                if let (Some(_), Some(f)) = (w.upgrade(), f.upgrade()) {
                    return Value::Foreign(f);
                }
            }
        }
        let id = match island.js_free.pop() {
            Some(id) => {
                island.js_vals[id as usize] = Some(v);
                id
            }
            None => {
                island.js_vals.push(Some(v));
                island.js_vals.len() as u32 - 1
            }
        };
        let f = Rc::new(Foreign {
            id,
            callable,
            array,
            drops: Some(island.drops.clone()),
        });
        if let Some((addr, w)) = key {
            island.js_ids.insert(addr, (w, Rc::downgrade(&f)));
        }
        Value::Foreign(f)
    }

    /// The VM value a handle names.
    pub(crate) fn js_of(&mut self, f: &Foreign) -> Js {
        self.island
            .as_ref()
            .and_then(|i| i.js_vals.get(f.id as usize).cloned().flatten())
            .unwrap_or(Js::Undefined)
    }

    /// The cw-ui value a VM function or proxy of ours stands for.
    fn unwrap_js(&mut self, o: &Obj) -> Option<Value> {
        let id = if o.is_callable() {
            match &o.borrow().kind {
                Kind::Function(fd) => match &fd.imp {
                    FuncImpl::Native { f, slots }
                        if std::ptr::eq(*f as *const (), n_call_cw as *const ()) =>
                    {
                        match slots.first() {
                            Some(Js::Num(n)) => Some(*n as u32),
                            _ => None,
                        }
                    }
                    _ => None,
                },
                _ => None,
            }
        } else if matches!(o.borrow().kind, Kind::Proxy { .. }) {
            match self.js_call_helper("idOf", vec![Js::Obj(o.clone())]) {
                Ok(Js::Num(n)) if n >= 0.0 => Some(n as u32),
                _ => None,
            }
        } else {
            None
        }?;
        self.island
            .as_ref()
            .and_then(|i| i.cw_vals.get(id as usize).cloned().flatten())
            .map(|(v, _)| v)
    }

    /// A cw-ui value as the VM sees it.
    pub(crate) fn js_value(&mut self, v: &Value) -> Js {
        match v {
            Value::Undefined => Js::Undefined,
            Value::Null => Js::Null,
            Value::Bool(b) => Js::Bool(*b),
            Value::Num(n) => Js::Num(*n),
            Value::Str(s) => Js::str(s),
            Value::Foreign(f) => self.js_of(f),
            Value::Cell(c) => {
                let inner = c.borrow().clone();
                self.js_value(&inner)
            }
            Value::Elem(e) => self.elem_to_js(e),
            Value::Context(id) => Js::Obj(self.js_context(*id)),
            // A cw-ui promise is a VM promise that settles with it, so VM code
            // awaits it as its own.
            Value::Promise(_) => {
                let proxy = self.wrap_cw(v);
                self.js_call_helper("fromCw", vec![proxy])
                    .unwrap_or(Js::Undefined)
            }
            // A date crosses as a date (a copy: its time, not its identity).
            Value::Date(d) => {
                let t = d.get();
                let vm = self.vm();
                Js::Obj(vm.obj_with(Some(vm.intr.date_proto.clone()), Kind::Date(t)))
            }
            v => self.wrap_cw(v),
        }
    }

    /// The VM function or proxy standing for a cw-ui value (one per value).
    fn wrap_cw(&mut self, v: &Value) -> Js {
        let key = identity(v);
        if let Some(k) = key {
            let island = self.island.as_ref().expect("an island");
            if let Some(id) = island.cw_ids.get(&k) {
                if let Some(Some((_, w))) = island.cw_vals.get(*id as usize) {
                    if let Some(o) = w.upgrade() {
                        return Js::Obj(Obj(o));
                    }
                }
            }
        }
        let callable = v.type_of() == "function";
        let is_array = matches!(v, Value::Array(_));
        let id = {
            let island = self.island_mut();
            match island.cw_free.pop() {
                Some(id) => id,
                None => {
                    island.cw_vals.push(None);
                    island.cw_vals.len() as u32 - 1
                }
            }
        };
        let obj = if callable {
            let vm = self.vm();
            let f = vm.native_fn_slots("", 0, n_call_cw, vec![Js::Num(id as f64)]);
            if let Value::Func(c) = v {
                if self.program.forward_ref(c.func) {
                    f.set_hidden("__cwForwardRef", Js::Bool(true));
                }
            }
            f
        } else {
            match self.js_call_helper("proxy", vec![Js::Num(id as f64), Js::Bool(is_array)]) {
                Ok(Js::Obj(o)) => o,
                _ => {
                    let vm = self.vm();
                    vm.new_object()
                }
            }
        };
        let island = self.island_mut();
        island.cw_vals[id as usize] = Some((v.clone(), Rc::downgrade(&obj.0)));
        if let Some(k) = key {
            island.cw_ids.insert(k, id);
        }
        Js::Obj(obj)
    }

    fn cw_by_id(&self, id: u32) -> Value {
        self.island
            .as_ref()
            .and_then(|i| i.cw_vals.get(id as usize).cloned().flatten())
            .map(|(v, _)| v)
            .unwrap_or_default()
    }

    /// The VM context object of cw-ui context `id` (one per context).
    fn js_context(&mut self, id: u32) -> Obj {
        if let Some(o) = self.island.as_ref().and_then(|i| i.contexts.get(&id)) {
            return o.clone();
        }
        let o = match self.js_call_helper("contextOf", vec![Js::Num(id as f64)]) {
            Ok(Js::Obj(o)) => o,
            _ => {
                let vm = self.vm();
                vm.new_object()
            }
        };
        self.island_mut().contexts.insert(id, o.clone());
        o
    }

    // ------------------------------------------------------------ elements

    fn js_symbol(&mut self, name: &str) -> Js {
        let syms = self.js_helper("symbols");
        let vm = self.vm();
        vm.get_str(&syms, name).unwrap_or(Js::Undefined)
    }

    fn js_get(&mut self, o: &Js, k: &str) -> Js {
        let vm = self.vm();
        match vm.get_str(o, k) {
            Ok(v) => v,
            Err(_) => Js::Undefined,
        }
    }

    /// A VM element as a cw-ui element, or `None` when `o` is no element.
    fn js_element(&mut self, o: &Obj) -> Option<Value> {
        if o.is_callable() || !matches!(o.borrow().kind, Kind::Ordinary) {
            return None;
        }
        let ov = Js::Obj(o.clone());
        let tag = self.js_get(&ov, "$$typeof");
        let portal = self.js_symbol("PORTAL");
        if js_same(&tag, &portal) {
            // `createPortal(children, container, key)`.
            let children_js = self.js_get(&ov, "children");
            let children = match self.cw_value(&children_js) {
                Value::Array(a) => a.borrow().clone(),
                Value::Foreign(f) if f.array => {
                    let f = f.clone();
                    self.foreign_items(&f).unwrap_or_default()
                }
                Value::Undefined => Vec::new(),
                v => vec![v],
            };
            let container_js = self.js_get(&ov, "containerInfo");
            let key = match self.js_get(&ov, "key") {
                Js::Null | Js::Undefined => None,
                k => {
                    let vm = self.vm();
                    vm.to_str(&k).ok().map(|s| Rc::from(s.as_str()))
                }
            };
            return Some(match self.cw_value(&container_js) {
                Value::Node(container) => Value::Elem(Rc::new(Elem::Portal {
                    children,
                    container,
                    key,
                })),
                _ => Value::error("Error", "Target container is not a DOM element."),
            });
        }
        let element = self.js_symbol("ELEMENT");
        if !js_same(&tag, &element) {
            return None;
        }
        let ty = self.js_get(&ov, "type");
        let key = match self.js_get(&ov, "key") {
            Js::Null | Js::Undefined => None,
            k => {
                let vm = self.vm();
                vm.to_str(&k).ok().map(|s| Rc::from(s.as_str()))
            }
        };
        let r = self.js_get(&ov, "ref");
        let props = self.js_get(&ov, "props");
        let children_js = self.js_get(&props, "children");
        let children = self.cw_value(&children_js);
        match &ty {
            Js::Str(tag) => {
                let tid = self.dyn_template(tag.as_str());
                // The props but `children` (which fill the template's child hole).
                let mut own: Vec<(Str, Value)> = Vec::new();
                let keys = self.js_call_helper("keys", vec![props.clone()]).ok();
                for k in keys
                    .map(|k| self.js_to_array(&k).unwrap_or_default())
                    .unwrap_or_default()
                {
                    let name = match &k {
                        Js::Str(s) => s.as_str().to_owned(),
                        _ => continue,
                    };
                    if name == "children" {
                        continue;
                    }
                    let v = self.js_get(&props, &name);
                    let cv = self.cw_value(&v);
                    own.push((Rc::from(name.as_str()), cv));
                }
                let r = match &r {
                    Js::Null | Js::Undefined => Value::Undefined,
                    r => self.cw_value(r),
                };
                let mut holes = vec![Value::object(own), r];
                if !is_void(tag.as_str()) {
                    holes.push(children);
                }
                Some(Value::Elem(Rc::new(Elem::Template { tid, holes, key })))
            }
            Js::Sym(_) => {
                // Fragment, StrictMode, Suspense, Profiler: the children.
                let list = match children {
                    Value::Array(a) => a.borrow().clone(),
                    Value::Undefined => Vec::new(),
                    v => vec![v],
                };
                Some(Value::Elem(Rc::new(Elem::Fragment {
                    children: list,
                    key,
                })))
            }
            Js::Obj(t) if !t.is_callable() => {
                // A provider: `{ $$typeof: PROVIDER, _context }`.
                let ctx = self.js_get(&ty, "_context");
                let id = match self.js_get(&ctx, "_cw") {
                    Js::Num(n) => n as u32,
                    _ => return Some(Value::error("Error", "unsupported element type")),
                };
                let value_js = self.js_get(&props, "value");
                let value = self.cw_value(&value_js);
                let list = match children {
                    Value::Array(a) => a.borrow().clone(),
                    Value::Undefined => Vec::new(),
                    v => vec![v],
                };
                Some(Value::Elem(Rc::new(Elem::Provider {
                    ctx: id,
                    value,
                    children: list,
                    key,
                })))
            }
            _ => {
                // A component, of either side; `ref` travels as a prop and goes
                // to a forwardRef component as its second argument.
                let func = self.cw_value(&ty);
                let mut own: Vec<(Str, Value)> = Vec::new();
                let keys = self.js_call_helper("keys", vec![props.clone()]).ok();
                for k in keys
                    .map(|k| self.js_to_array(&k).unwrap_or_default())
                    .unwrap_or_default()
                {
                    let name = match &k {
                        Js::Str(s) => s.as_str().to_owned(),
                        _ => continue,
                    };
                    let v = self.js_get(&props, &name);
                    let cv = self.cw_value(&v);
                    own.push((Rc::from(name.as_str()), cv));
                }
                if !matches!(r, Js::Null | Js::Undefined) {
                    let rv = self.cw_value(&r);
                    own.push((Rc::from("ref"), rv));
                }
                Some(Value::Elem(Rc::new(Elem::Component {
                    func: crate::value::ComponentFn::of(func)?,
                    props: Value::object(own),
                    key,
                })))
            }
        }
    }

    /// A cw-ui element as the VM's element.
    fn elem_to_js(&mut self, e: &Rc<Elem>) -> Js {
        if let Elem::Portal {
            children,
            container,
            key,
        } = &**e
        {
            let c = self.js_value(&Value::array(children.clone()));
            let n = self.js_value(&Value::Node(*container));
            let k = match key {
                Some(k) => Js::str(k),
                None => Js::Null,
            };
            return self
                .js_call_helper("portal", vec![c, n, k])
                .unwrap_or(Js::Undefined);
        }
        let (ty, props, key): (Js, Vec<(String, Value)>, Option<Str>) = match &**e {
            Elem::Template { tid, holes, key } => {
                return self.template_to_js(*tid, holes, key.clone());
            }
            Elem::Component { func, props, key } => {
                let f = func.value();
                let mut ps = Vec::new();
                if let Value::Object(o) = props {
                    for (k, v) in o.borrow().iter() {
                        ps.push((k.to_string(), v.clone()));
                    }
                }
                (self.js_value(&f), ps, key.clone())
            }
            Elem::Fragment { children, key } => {
                let fragment = self.js_symbol("FRAGMENT");
                (
                    fragment,
                    vec![("children".into(), Value::array(children.clone()))],
                    key.clone(),
                )
            }
            Elem::Portal { .. } => unreachable!("converted above"),
            Elem::Provider {
                ctx,
                value,
                children,
                key,
            } => {
                let c = self.js_context(*ctx);
                let p = self.js_get(&Js::Obj(c), "Provider");
                (
                    p,
                    vec![
                        ("value".into(), value.clone()),
                        ("children".into(), Value::array(children.clone())),
                    ],
                    key.clone(),
                )
            }
        };
        self.make_js_element(ty, props, key, None)
    }

    fn make_js_element(
        &mut self,
        ty: Js,
        props: Vec<(String, Value)>,
        key: Option<Str>,
        r: Option<Value>,
    ) -> Js {
        let element = self.js_symbol("ELEMENT");
        let mut r_js = Js::Null;
        let vm_props = {
            let vm = self.vm();
            vm.new_object()
        };
        for (k, v) in props {
            if k == "ref" {
                r_js = self.js_value(&v);
                continue;
            }
            let jv = self.js_value(&v);
            vm_props.set_prop(&k, jv, cw_jsvm::value::ALL);
        }
        if let Some(r) = r {
            if !r.is_nullish() {
                r_js = self.js_value(&r);
            }
        }
        let vm = self.vm();
        let o = vm.new_object();
        o.set_prop("$$typeof", element, cw_jsvm::value::ALL);
        o.set_prop("type", ty, cw_jsvm::value::ALL);
        o.set_prop(
            "key",
            match key {
                Some(k) => Js::str(&k),
                None => Js::Null,
            },
            cw_jsvm::value::ALL,
        );
        o.set_prop("ref", r_js, cw_jsvm::value::ALL);
        o.set_prop("props", Js::Obj(vm_props), cw_jsvm::value::ALL);
        o.set_prop("_owner", Js::Null, cw_jsvm::value::ALL);
        Js::Obj(o)
    }

    /// A compiled template's element as React elements again: its attributes and
    /// holes are props, its static children nested elements.
    fn template_to_js(&mut self, tid: u32, holes: &[Value], key: Option<Str>) -> Js {
        let root = self.template_tree(tid);
        self.tnode_to_js(&root, holes, key).unwrap_or(Js::Undefined)
    }

    fn tnode_to_js(
        &mut self,
        n: &crate::ir::TNode,
        holes: &[Value],
        key: Option<Str>,
    ) -> Option<Js> {
        use crate::ir::{TAttr, TNode};
        match n {
            TNode::Text(s) => Some(Js::str(s)),
            TNode::Hole(h) => Some(self.js_value(&holes[*h as usize])),
            TNode::Element {
                tag,
                attrs,
                children,
            } => {
                let mut props: Vec<(String, Value)> = Vec::new();
                let mut r = None;
                for a in attrs {
                    match a {
                        TAttr::Static(dom, v) => props.push((react_prop(dom), Value::str(v))),
                        TAttr::Dynamic(p, h) => props.push((p.clone(), holes[*h as usize].clone())),
                        TAttr::Spread(h) => {
                            if let Value::Object(o) = &holes[*h as usize] {
                                for (k, v) in o.borrow().iter() {
                                    props.push((k.to_string(), v.clone()));
                                }
                            }
                        }
                        TAttr::Ref(h) => r = Some(holes[*h as usize].clone()),
                    }
                }
                let mut kids: Vec<Js> = Vec::new();
                for c in children {
                    if let Some(k) = self.tnode_to_js(c, holes, None) {
                        kids.push(k);
                    }
                }
                let children_js = match kids.len() {
                    0 => None,
                    1 => kids.pop(),
                    _ => {
                        let vm = self.vm();
                        Some(vm.arr(kids))
                    }
                };
                let el = self.make_js_element(Js::str(tag), props, key, r);
                if let (Some(c), Js::Obj(o)) = (children_js, &el) {
                    let p = self.js_get(&Js::Obj(o.clone()), "props");
                    if let Js::Obj(p) = p {
                        p.set_prop("children", c, cw_jsvm::value::ALL);
                    }
                }
                Some(el)
            }
        }
    }
}

/// `===` on two VM values that are symbols or objects.
fn js_same(a: &Js, b: &Js) -> bool {
    match (a, b) {
        (Js::Sym(x), Js::Sym(y)) => Rc::ptr_eq(x, y),
        (Js::Obj(x), Js::Obj(y)) => x.ptr_eq(y),
        _ => false,
    }
}

/// Elements with no children (the template of one gets no child hole).
fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}

/// The React prop a DOM attribute comes from (`class` → `className`).
fn react_prop(dom: &str) -> String {
    match dom {
        "class" => "className".into(),
        "for" => "htmlFor".into(),
        "tabindex" => "tabIndex".into(),
        "readonly" => "readOnly".into(),
        "maxlength" => "maxLength".into(),
        "colspan" => "colSpan".into(),
        "rowspan" => "rowSpan".into(),
        other => other.into(),
    }
}

/// What makes a cw-ui value the same one again: its allocation, or for the
/// values that are not allocations, a tag and their fields.
type Identity = (u8, usize, usize);

fn identity(v: &Value) -> Option<Identity> {
    let at = |p: usize| (0u8, p, 0usize);
    Some(match v {
        Value::Array(a) => at(Rc::as_ptr(a) as *const u8 as usize),
        Value::Object(o) => at(Rc::as_ptr(o) as *const u8 as usize),
        Value::Func(f) => at(Rc::as_ptr(f) as *const u8 as usize),
        Value::Ref(r) => at(Rc::as_ptr(r) as *const u8 as usize),
        Value::Event(e) => at(Rc::as_ptr(e) as *const u8 as usize),
        Value::Promise(p) => at(Rc::as_ptr(p) as *const u8 as usize),
        Value::Native(n) => at(Rc::as_ptr(n) as *const u8 as usize),
        Value::Set(s) => at(Rc::as_ptr(s) as *const u8 as usize),
        Value::Map(m) => at(Rc::as_ptr(m) as *const u8 as usize),
        Value::Date(d) => at(Rc::as_ptr(d) as *const u8 as usize),
        Value::Regex(r) => at(Rc::as_ptr(r) as *const u8 as usize),
        Value::Error(e) => at(Rc::as_ptr(e) as *const u8 as usize),
        Value::Response(r) => at(Rc::as_ptr(r) as *const u8 as usize),
        // Nodes and setters are values, not allocations: keyed apart.
        Value::Node(n) => (1, n.0 as usize, 0),
        Value::Setter(i, h) => (2, *i as usize, *h as usize),
        Value::Dispatch(i, h) => (3, *i as usize, *h as usize),
        _ => return None,
    })
}

// ---------------------------------------------------------------- natives

type Native = fn(&mut Vm, &mut Args) -> JsResult<Js>;

const NATIVES: &[(&str, u32, Native)] = &[
    ("pget", 2, n_pget),
    ("pset", 3, n_pset),
    ("phas", 2, n_phas),
    ("pkeys", 1, n_pkeys),
    ("pdel", 2, n_pdel),
    ("hook", 1, n_hook),
    ("newContext", 1, n_new_context),
    ("log", 2, n_log),
    ("inspect", 1, n_inspect),
    ("timer", 4, n_timer),
    ("clearTimer", 1, n_clear_timer),
    ("g", 1, n_global),
    ("builtin", 2, n_builtin),
];

/// The page builtins the shim's `document` and `window` call:
/// `__cw.builtin(name, args)`.
fn n_builtin(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    use crate::ir::Builtin as B;
    let name = match a.arg(0) {
        Js::Str(s) => s.as_str().to_owned(),
        _ => return Ok(Js::Undefined),
    };
    let b = match name.as_str() {
        "GetElementById" => B::GetElementById,
        "QuerySelector" => B::QuerySelector,
        "QuerySelectorAll" => B::QuerySelectorAll,
        "DocumentBody" => B::DocumentBody,
        "DocumentElement" => B::DocumentElement,
        "ActiveElement" => B::ActiveElement,
        "DocumentTitle" => B::DocumentTitle,
        "DocumentAddListener" => B::DocumentAddListener,
        "DocumentRemoveListener" => B::DocumentRemoveListener,
        "WindowAddListener" => B::WindowAddListener,
        "WindowRemoveListener" => B::WindowRemoveListener,
        "StorageGet" => B::StorageGet,
        "StorageSet" => B::StorageSet,
        "StorageRemove" => B::StorageRemove,
        "StorageClear" => B::StorageClear,
        "StorageKey" => B::StorageKey,
        "StorageLength" => B::StorageLength,
        "LocationPart" => B::LocationPart,
        "InnerWidth" => B::InnerWidth,
        "InnerHeight" => B::InnerHeight,
        "ScrollX" => B::ScrollX,
        "ScrollY" => B::ScrollY,
        "WindowScrollTo" => B::WindowScrollTo,
        "WindowScrollBy" => B::WindowScrollBy,
        "Alert" => B::Alert,
        "Fetch" => B::Fetch,
        "RequestAnimationFrame" => B::RequestAnimationFrame,
        "CancelAnimationFrame" => B::CancelAnimationFrame,
        "PerformanceNow" => B::PerformanceNow,
        "MatchMedia" => B::MatchMedia,
        "LocationSet" => B::LocationSet,
        "LocationAssign" => B::LocationAssign,
        "LocationReplace" => B::LocationReplace,
        "LocationReload" => B::LocationReload,
        "HistoryLength" => B::HistoryLength,
        "HistoryState" => B::HistoryState,
        "HistoryPush" => B::HistoryPush,
        "HistoryReplace" => B::HistoryReplace,
        "HistoryGo" => B::HistoryGo,
        _ => return Ok(Js::Undefined),
    };
    let rt = rt_of(vm);
    let list = a.arg(1);
    let items = rt.js_to_array(&list).unwrap_or_default();
    let mut args: Vec<Value> = items.iter().map(|v| rt.cw_value(v)).collect();
    if matches!(b, B::Fetch) {
        // `fetch(url, init)`: the init object and its headers as cw-ui objects.
        if let Some(init) = args.get(1).cloned() {
            let init = rt.plain_object(&init).unwrap_or_default();
            if let Value::Object(o) = &init {
                let fixed: Vec<(Str, Value)> = o
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut out = Vec::new();
                for (k, v) in fixed {
                    let v = if &*k == "headers" {
                        rt.plain_object(&v).unwrap_or_default()
                    } else {
                        v
                    };
                    out.push((k, v));
                }
                args[1] = Value::object(out);
            }
        }
    }
    while args.last().is_some_and(|v| matches!(v, Value::Undefined)) {
        args.pop();
    }
    match rt.builtin(b, args) {
        Ok(v) => Ok(rt.js_value(&v)),
        Err(t) => Err(throw_to_js(rt, t)),
    }
}

/// A compiled module's global, read by the island's modules of the app:
/// `__cw.g(slot)`.
fn n_global(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let slot = match a.arg(0) {
        Js::Num(n) => n as usize,
        _ => return Ok(Js::Undefined),
    };
    let rt = rt_of(vm);
    let v = rt.globals.get(slot).cloned().unwrap_or(Value::Undefined);
    Ok(rt.js_value(&v))
}

/// A cw-ui function called from the VM.
fn n_call_cw(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let id = slot_id(a);
    let args: Vec<Js> = std::mem::take(&mut a.args);
    let _ = vm;
    let rt = rt_of(vm);
    let f = rt.cw_by_id(id);
    let args: Vec<Value> = args.iter().map(|v| rt.cw_value(v)).collect();
    match rt.call_value(&f, args) {
        Ok(v) => Ok(rt.js_value(&v)),
        Err(t) => Err(throw_to_js(rt, t)),
    }
}

fn key_name(vm: &mut Vm, v: &Js) -> JsResult<String> {
    vm.to_str(v)
}

fn n_pget(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let id = match a.arg(0) {
        Js::Num(n) => n as u32,
        _ => return Ok(Js::Undefined),
    };
    let k = key_name(vm, &a.arg(1))?;
    let rt = rt_of(vm);
    let target = rt.cw_by_id(id);
    let v = match rt.proxy_get(&target, &k) {
        Ok(v) => v,
        Err(t) => return Err(throw_to_js(rt, t)),
    };
    Ok(rt.js_value(&v))
}

fn n_pset(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let id = match a.arg(0) {
        Js::Num(n) => n as u32,
        _ => return Ok(Js::Bool(false)),
    };
    let k = key_name(vm, &a.arg(1))?;
    let v = a.arg(2);
    let rt = rt_of(vm);
    let target = rt.cw_by_id(id);
    let v = rt.cw_value(&v);
    match rt.set_index(&target, &proxy_key(&target, &k), v) {
        Ok(()) => Ok(Js::Bool(true)),
        Err(t) => Err(throw_to_js(rt, t)),
    }
}

fn n_phas(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let id = match a.arg(0) {
        Js::Num(n) => n as u32,
        _ => return Ok(Js::Bool(false)),
    };
    let k = key_name(vm, &a.arg(1))?;
    let rt = rt_of(vm);
    let target = rt.cw_by_id(id);
    Ok(Js::Bool(rt.proxy_keys(&target).iter().any(|x| **x == *k)))
}

fn n_pkeys(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let id = match a.arg(0) {
        Js::Num(n) => n as u32,
        _ => return Ok(vm.arr(vec![])),
    };
    let rt = rt_of(vm);
    let target = rt.cw_by_id(id);
    let keys: Vec<Js> = rt.proxy_keys(&target).iter().map(|k| Js::str(k)).collect();
    Ok(vm.arr(keys))
}

fn n_pdel(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let id = match a.arg(0) {
        Js::Num(n) => n as u32,
        _ => return Ok(Js::Bool(false)),
    };
    let k = key_name(vm, &a.arg(1))?;
    let rt = rt_of(vm);
    let target = rt.cw_by_id(id);
    match rt.builtin(crate::ir::Builtin::Delete, vec![target, Value::str(&k)]) {
        Ok(_) => Ok(Js::Bool(true)),
        Err(t) => Err(throw_to_js(rt, t)),
    }
}

/// A property key from the VM as cw-ui indexes `target` with it: an array's
/// element by its number (the VM spells every key as a string, `"0"`).
fn proxy_key(target: &Value, k: &str) -> Value {
    if matches!(target, Value::Array(_)) {
        if let Ok(i) = k.parse::<u32>() {
            if i.to_string() == k {
                return Value::Num(i as f64);
            }
        }
    }
    Value::str(k)
}

/// A hook of the component cw-ui is rendering: `__cw.hook(kind, ...args)`.
fn n_hook(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    use crate::ir::Hook;
    let kind = vm.to_str(&a.arg(0))?;
    let raw: Vec<Js> = a.args.iter().skip(1).cloned().collect();
    let rt = rt_of(vm);
    let mut args: Vec<Value> = raw.iter().map(|v| rt.cw_value(v)).collect();
    let h = match kind.as_str() {
        "state" => Hook::State,
        "reducer" => Hook::Reducer,
        "memo" => Hook::Memo,
        "callback" => Hook::Callback,
        "ref" => Hook::Ref,
        "effect" => Hook::Effect,
        "layout" => Hook::LayoutEffect,
        "context" => {
            // The VM's context object: its cw-ui id.
            let ctx = raw.first().cloned().unwrap_or(Js::Undefined);
            let id = match vm.get_str(&ctx, "_cw")? {
                Js::Num(n) => n as u32,
                _ => return Err(vm.type_error("useContext needs a context")),
            };
            args = vec![Value::Context(id)];
            Hook::Context
        }
        "id" => Hook::Id,
        "store" => Hook::SyncExternalStore,
        "imperative" => Hook::ImperativeHandle,
        other => return Err(vm.type_error(format!("unknown hook {other}"))),
    };
    let rt = rt_of(vm);
    let n = args.len();
    let r = rt.hook_with(h, n, &mut |_, i| Ok(args[i].clone()));
    match r {
        Ok(v) => {
            // `useState`/`useReducer` give an array the VM destructures.
            if let Value::Array(items) = &v {
                let items: Vec<Value> = items.borrow().clone();
                let js: Vec<Js> = items.iter().map(|x| rt.js_value(x)).collect();
                return Ok(vm.arr(js));
            }
            Ok(rt.js_value(&v))
        }
        Err(t) => Err(throw_to_js(rt, t)),
    }
}

fn n_new_context(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let d = a.arg(0);
    let rt = rt_of(vm);
    let dv = rt.cw_value(&d);
    let island = rt.island_mut();
    let id = island.next_context;
    island.next_context += 1;
    rt.ctx_defaults.insert(id, dv);
    Ok(Js::Num(id as f64))
}

fn n_log(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let level = match a.arg(0) {
        Js::Num(n) => n as u32,
        _ => 0,
    };
    let text = vm.to_str(&a.arg(1))?;
    let rt = rt_of(vm);
    let level = match level {
        1 => cw_web::script::LogLevel::Warn,
        2 => cw_web::script::LogLevel::Error,
        _ => cw_web::script::LogLevel::Log,
    };
    rt.log(level, &text);
    Ok(Js::Undefined)
}

fn n_inspect(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let v = a.arg(0);
    // cw-ui's own formatting for what came from the compiled side, the VM's for
    // the rest.
    let s = vm.inspect(&v, &cw_jsvm::inspect::Opts::default())?;
    Ok(Js::string(s))
}

/// `setTimeout`/`setInterval` of the VM: cw-ui's timers.
fn n_timer(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let interval = matches!(a.arg(0), Js::Num(n) if n == 1.0);
    let f = a.arg(1);
    let ms = a.arg(2);
    let extra = a.arg(3);
    let rt = rt_of(vm);
    let f = rt.cw_value(&f);
    let ms = rt.cw_value(&ms);
    let extra = match rt.cw_value(&extra) {
        Value::Array(items) => items.borrow().clone(),
        Value::Foreign(_) => Vec::new(),
        _ => Vec::new(),
    };
    let mut args = vec![f, ms];
    args.extend(extra);
    let b = if interval {
        crate::ir::Builtin::SetInterval
    } else {
        crate::ir::Builtin::SetTimeout
    };
    match rt.builtin(b, args) {
        Ok(v) => Ok(rt.js_value(&v)),
        Err(t) => Err(throw_to_js(rt, t)),
    }
}

fn n_clear_timer(vm: &mut Vm, a: &mut Args) -> JsResult<Js> {
    let id = a.arg(0);
    let rt = rt_of(vm);
    let id = rt.cw_value(&id);
    let _ = rt.builtin(crate::ir::Builtin::ClearTimeout, vec![id]);
    Ok(Js::Undefined)
}

// ---------------------------------------------------------------- the runtime side

impl Runtime {
    /// A property of a cw-ui value, read through a VM proxy (`get` trap): a
    /// method is a function bound to the value.
    fn proxy_get(&mut self, target: &Value, k: &str) -> R<Value> {
        let v = self.get_index(target, &proxy_key(target, k))?;
        if matches!(v, Value::Undefined) && !matches!(target, Value::Object(_)) {
            // A built-in method (`arr.map`, `el.focus`): a function invoking it.
            if crate::interp::has_builtin_method(target, k) {
                return Ok(Value::Native(Rc::new(
                    crate::runtime::NativeFn::BoundMethod {
                        recv: target.clone(),
                        name: Rc::from(k),
                    },
                )));
            }
        }
        Ok(v)
    }

    /// The keys a VM proxy of a cw-ui value lists.
    fn proxy_keys(&mut self, target: &Value) -> Vec<Str> {
        match target {
            Value::Object(o) => crate::interp::object_keys(&o.borrow()),
            Value::Array(a) => (0..a.borrow().len())
                .map(|i| Rc::from(i.to_string().as_str()))
                .collect(),
            Value::Ref(_) => vec![Rc::from("current")],
            _ => Vec::new(),
        }
    }

    // ------------------------------------------------------------ Foreign values

    /// `f.name`.
    pub(crate) fn foreign_get(&mut self, f: &Foreign, k: &str) -> R<Value> {
        let o = self.js_of(f);
        let vm = self.vm();
        let r = vm.get_str(&o, k);
        let v = self.js_result(r)?;
        Ok(self.cw_value(&v))
    }

    /// `f.name = v`.
    pub(crate) fn foreign_set(&mut self, f: &Foreign, k: &str, v: Value) -> R<()> {
        let o = self.js_of(f);
        let v = self.js_value(&v);
        let vm = self.vm();
        let r = vm.set_str(&o, k, v).map(|_| Js::Undefined);
        self.js_result(r)?;
        Ok(())
    }

    /// `f(args)`.
    pub(crate) fn foreign_call(&mut self, f: &Foreign, args: Vec<Value>) -> R<Value> {
        let o = self.js_of(f);
        let args: Vec<Js> = args.iter().map(|a| self.js_value(a)).collect();
        let vm = self.vm();
        let r = vm.call(&o, Js::Undefined, args);
        let v = self.js_result(r)?;
        self.run_js_jobs();
        Ok(self.cw_value(&v))
    }

    /// `f.name(args)`, with `this` the object.
    pub(crate) fn foreign_invoke(&mut self, f: &Foreign, name: &str, args: Vec<Value>) -> R<Value> {
        let o = self.js_of(f);
        let args: Vec<Js> = args.iter().map(|a| self.js_value(a)).collect();
        let list = {
            let vm = self.vm();
            vm.arr(args)
        };
        let v = self.js_call_helper("method", vec![o, Js::str(name), list])?;
        self.run_js_jobs();
        Ok(self.cw_value(&v))
    }

    /// `new f(args)`.
    pub(crate) fn foreign_construct(&mut self, f: &Foreign, args: Vec<Value>) -> R<Value> {
        let o = self.js_of(f);
        let args: Vec<Js> = args.iter().map(|a| self.js_value(a)).collect();
        let list = {
            let vm = self.vm();
            vm.arr(args)
        };
        let v = self.js_call_helper("construct", vec![o, list])?;
        Ok(self.cw_value(&v))
    }

    /// The items of an iterable VM value.
    pub(crate) fn foreign_items(&mut self, f: &Foreign) -> R<Vec<Value>> {
        let o = self.js_of(f);
        let list = self.js_to_array(&o)?;
        Ok(list.iter().map(|v| self.cw_value(v)).collect())
    }

    /// A VM object's own enumerable properties, for a spread.
    pub(crate) fn foreign_entries(&mut self, f: &Foreign) -> R<Vec<(Str, Value)>> {
        let o = self.js_of(f);
        let keys = self.js_call_helper("keys", vec![o.clone()])?;
        let keys = self.js_to_array(&keys)?;
        let mut out = Vec::new();
        for k in keys {
            let name = match &k {
                Js::Str(s) => s.as_str().to_owned(),
                _ => continue,
            };
            let v = self.js_get(&o, &name);
            let cv = self.cw_value(&v);
            out.push((Rc::from(name.as_str()), cv));
        }
        Ok(out)
    }

    /// `String(f)`.
    pub(crate) fn foreign_string(&mut self, f: &Foreign) -> String {
        let o = self.js_of(f);
        match self.js_call_helper("str", vec![o]) {
            Ok(Js::Str(s)) => s.as_str().to_owned(),
            _ => "[object Object]".into(),
        }
    }

    /// `Number(f)`.
    pub(crate) fn foreign_number(&mut self, f: &Foreign) -> f64 {
        let o = self.js_of(f);
        match self.js_call_helper("num", vec![o]) {
            Ok(Js::Num(n)) => n,
            _ => f64::NAN,
        }
    }

    /// `JSON.stringify(f)` (the VM's, for a VM value).
    pub(crate) fn foreign_json(&mut self, f: &Foreign) -> Option<String> {
        let o = self.js_of(f);
        match self.js_call_helper("json", vec![o, Js::Undefined]) {
            Ok(Js::Str(s)) => Some(s.as_str().to_owned()),
            _ => None,
        }
    }

    /// `f instanceof <global name>`.
    pub(crate) fn foreign_instance(&mut self, f: &Foreign, name: &str) -> bool {
        let o = self.js_of(f);
        matches!(
            self.js_call_helper("instance", vec![o, Js::str(name)]),
            Ok(Js::Bool(true))
        )
    }

    /// Whether a VM value is a thenable (a promise to adopt).
    pub(crate) fn foreign_thenable(&mut self, f: &Foreign) -> bool {
        let o = self.js_of(f);
        matches!(
            self.js_call_helper("isThenable", vec![o]),
            Ok(Js::Bool(true))
        )
    }

    /// `f.then(ok, bad)`.
    pub(crate) fn foreign_then(&mut self, f: &Foreign, ok: Value, bad: Value) -> R<()> {
        let o = self.js_of(f);
        let ok = self.js_value(&ok);
        let bad = self.js_value(&bad);
        self.js_call_helper("then", vec![o, ok, bad])?;
        Ok(())
    }

    /// Whether a VM function is a `forwardRef` render function.
    /// The function cw-ui calls to render VM component `f`: `f` itself, or for a
    /// class component the shim's function component running it (one per class).
    pub(crate) fn class_host(&mut self, f: &Rc<Foreign>) -> Rc<Foreign> {
        let o = self.js_of(f);
        match self.js_call_helper("classHost", vec![o.clone()]) {
            Ok(h) if !js_same(&h, &o) => match self.cw_value(&h) {
                Value::Foreign(h) => h,
                _ => f.clone(),
            },
            _ => f.clone(),
        }
    }

    pub(crate) fn foreign_forward_ref(&mut self, f: &Foreign) -> bool {
        let o = self.js_of(f);
        matches!(self.js_get(&o, "__cwForwardRef"), Js::Bool(true))
    }

    /// `v` as an object of cw-ui's (a VM object's own enumerable properties), for
    /// a spread or a rest pattern; anything else as it is.
    pub(crate) fn plain_object(&mut self, v: &Value) -> R<Value> {
        match v {
            Value::Foreign(f) if !f.callable => {
                let f = f.clone();
                Ok(Value::object(self.foreign_entries(&f)?))
            }
            v => Ok(v.clone()),
        }
    }

    /// `String(v)`, a VM value's by its own conversion.
    pub(crate) fn string_of(&mut self, v: &Value) -> String {
        match v {
            Value::Foreign(f) => {
                let f = f.clone();
                self.foreign_string(&f)
            }
            v => v.to_js_string(),
        }
    }

    /// An operator with a VM value on either side, as the VM evaluates it (its
    /// conversions are the VM's); `None` for one cw-ui evaluates itself.
    pub(crate) fn foreign_binary(
        &mut self,
        op: crate::ir::BinaryOp,
        a: &Value,
        b: &Value,
    ) -> R<Option<Value>> {
        use crate::ir::BinaryOp as B;
        let sym = match op {
            B::Add => "+",
            B::Sub => "-",
            B::Mul => "*",
            B::Div => "/",
            B::Rem => "%",
            B::Exp => "**",
            B::Eq => "==",
            B::NotEq => "!=",
            B::Lt => "<",
            B::LtEq => "<=",
            B::Gt => ">",
            B::GtEq => ">=",
            B::In => "in",
            _ => return Ok(None),
        };
        let (x, y) = (self.js_value(a), self.js_value(b));
        let v = self.js_call_helper("op", vec![Js::str(sym), x, y])?;
        Ok(Some(self.cw_value(&v)))
    }

    /// `new f(args)` for a value that may be an island's constructor.
    pub(crate) fn construct(&mut self, f: &Value, args: Vec<Value>) -> R<Value> {
        match f {
            Value::Foreign(x) if x.callable => {
                let x = x.clone();
                self.foreign_construct(&x, args)
            }
            other => crate::runtime::type_error(format!(
                "{} is not a constructor",
                crate::interp::inspect(other)
            )),
        }
    }

    /// The export `i` of the island, as compiled code imports it.
    pub fn island_export(&mut self, i: u32) -> R<Value> {
        // Each export is a function returning the value, called when the global it
        // initialises is (so an app module on the island has run by then).
        let f = match self.island.as_ref().and_then(|x| x.exports.get(i as usize)) {
            Some(v) => v.clone(),
            None => return crate::runtime::type_error("the app's island has no such export"),
        };
        let r = self.call_value(&f, Vec::new());
        self.run_js_jobs();
        r
    }
}

// ---------------------------------------------------------------- snapshots

/// An island written for a snapshot (see `snapshot::IslandS`).
pub(crate) struct IslandImage {
    pub heap: String,
    pub js: Vec<u32>,
    pub cw: Vec<(u32, Value)>,
    pub contexts: Vec<u32>,
    pub next_context: u32,
    pub exports: Vec<Value>,
    pub rng: u64,
}

fn snapshot_options() -> cw_jsvm::snapshot::Options<'static> {
    cw_jsvm::snapshot::Options::new(
        &[],
        cw_jsvm::snapshot::fingerprint_of(&[n_call_cw, n_pget, n_hook, n_timer]),
    )
}

impl Island {
    /// The island's heap image and tables, between two entries into its VM.
    pub(crate) fn image(&self) -> IslandImage {
        let mut roots = vec![Js::Obj(self.cw.clone())];
        let mut js = Vec::new();
        for (id, v) in self.js_vals.iter().enumerate() {
            if let Some(v) = v {
                js.push(id as u32);
                roots.push(v.clone());
            }
        }
        let mut cw = Vec::new();
        for (id, e) in self.cw_vals.iter().enumerate() {
            if let Some((v, w)) = e {
                if let Some(o) = w.upgrade() {
                    cw.push((id as u32, v.clone()));
                    roots.push(Js::Obj(Obj(o)));
                }
            }
        }
        let mut contexts: Vec<u32> = self.contexts.keys().copied().collect();
        contexts.sort_unstable();
        for c in &contexts {
            roots.push(Js::Obj(self.contexts[c].clone()));
        }
        // SAFETY: the VM is the island's; no entry is running (snapshots are taken
        // between entries).
        let vm = unsafe { &*self.vm };
        let heap = match vm.heap_snapshot(&roots, snapshot_options()) {
            Ok(bytes) => base64_encode(&bytes),
            Err(e) => format!("!{e}"),
        };
        // SAFETY: as above.
        let rng = unsafe { (*self.host).rng };
        IslandImage {
            heap,
            js,
            cw,
            contexts,
            next_context: self.next_context,
            exports: self.exports.clone(),
            rng,
        }
    }
}

impl Runtime {
    /// Reads an island's VM back from its image (`Island::image`); returns the
    /// handles of the VM values cw-ui held, by id. `finish_island` adds the rest.
    pub(crate) fn load_island(
        &mut self,
        heap: &str,
        js: &[u32],
        cw: &[(u32, crate::snapshot::V)],
        contexts: &[u32],
        next_context: u32,
        rng: u64,
    ) -> Result<std::collections::BTreeMap<u32, Value>, String> {
        if let Some(e) = heap.strip_prefix('!') {
            return Err(format!("the island could not be imaged: {e}"));
        }
        let bytes = base64_decode(heap).ok_or("a bad island image")?;
        let now = Rc::new(Cell::new(self.inner.host_now_micros()));
        let host = Box::into_raw(Box::new(IslandHost {
            now: now.clone(),
            rng,
        }));
        // SAFETY: the host lives until the island drops, after the VM.
        let host_ref: &'static mut IslandHost = unsafe { &mut *host };
        let (mut vm, roots) = match Vm::from_heap_snapshot(host_ref, &bytes, snapshot_options()) {
            Ok(v) => v,
            Err(e) => {
                // SAFETY: nothing borrows it any more.
                drop(unsafe { Box::from_raw(host) });
                return Err(format!("the island's heap image: {e}"));
            }
        };
        let link = Rc::new(Link {
            rt: Cell::new(std::ptr::null_mut()),
        });
        let any: Rc<dyn std::any::Any> = link.clone();
        vm.embedder = Some(any);
        let mut it = roots.into_iter();
        let cw_obj = match it.next() {
            Some(Js::Obj(o)) => o,
            _ => return Err("the island image has no __cw".into()),
        };
        let vm = Box::into_raw(Box::new(vm));
        let size = js.iter().max().map(|m| *m as usize + 1).unwrap_or(0);
        let mut js_vals: Vec<Option<Js>> = vec![None; size];
        let mut handles = std::collections::BTreeMap::new();
        let drops = Rc::new(RefCell::new(Vec::new()));
        let mut js_ids = HashMap::new();
        for id in js {
            let v = it.next().unwrap_or(Js::Undefined);
            let (callable, array) = match &v {
                Js::Obj(o) => (o.is_callable(), o.is_array_or_proxy()),
                _ => (false, false),
            };
            let f = Rc::new(Foreign {
                id: *id,
                callable,
                array,
                drops: Some(drops.clone()),
            });
            if let Js::Obj(o) = &v {
                js_ids.insert(o.addr(), (Rc::downgrade(&o.0), Rc::downgrade(&f)));
            }
            handles.insert(*id, Value::Foreign(f));
            js_vals[*id as usize] = Some(v);
        }
        let js_free = (0..size as u32)
            .rev()
            .filter(|i| js_vals[*i as usize].is_none())
            .collect();
        let cw_size = cw.iter().map(|(id, _)| *id as usize + 1).max().unwrap_or(0);
        let mut cw_vals: Vec<Option<(Value, Weak<RefCell<ObjData>>)>> = vec![None; cw_size];
        let mut cw_objs = Vec::new();
        for (id, _) in cw {
            let o = match it.next() {
                Some(Js::Obj(o)) => o,
                _ => return Err("the island image lost a stand-in".into()),
            };
            cw_objs.push((*id, o));
        }
        let mut ctx_map = HashMap::new();
        for c in contexts {
            if let Some(Js::Obj(o)) = it.next() {
                ctx_map.insert(*c, o);
            }
        }
        // The stand-ins are kept alive by the island until `finish_island` pairs
        // them with their values.
        for (id, o) in &cw_objs {
            cw_vals[*id as usize] = Some((Value::Undefined, Rc::downgrade(&o.0)));
        }
        self.island = Some(Box::new(Island {
            vm,
            host,
            now,
            link,
            cw: cw_obj,
            js_vals,
            js_free,
            js_ids,
            drops,
            cw_vals,
            cw_free: Vec::new(),
            cw_ids: HashMap::new(),
            contexts: ctx_map,
            next_context,
            exports: Vec::new(),
        }));
        self.island_keep = cw_objs.into_iter().map(|(_, o)| o).collect();
        Ok(handles)
    }

    /// Pairs the restored stand-ins with their cw-ui values.
    pub(crate) fn finish_island(&mut self, cw: Vec<(u32, Value)>, exports: Vec<Value>) {
        let Some(island) = self.island.as_mut() else {
            return;
        };
        for (id, v) in cw {
            if let Some(Some((slot, _))) = island.cw_vals.get_mut(id as usize) {
                if let Some(k) = identity(&v) {
                    island.cw_ids.insert(k, id);
                }
                *slot = v;
            }
        }
        island.cw_free = (0..island.cw_vals.len() as u32)
            .rev()
            .filter(|i| island.cw_vals[*i as usize].is_none())
            .collect();
        island.exports = exports;
        self.island_keep.clear();
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let val = |c: u8| B64.iter().position(|x| *x == c).map(|p| p as u32);
    for c in s.as_bytes().chunks(4) {
        if c.len() != 4 {
            return None;
        }
        let a = val(c[0])?;
        let b = val(c[1])?;
        let n = a << 18 | b << 12;
        if c[2] == b'=' {
            out.push((n >> 16) as u8);
            continue;
        }
        let n = n | val(c[2])? << 6;
        if c[3] == b'=' {
            out.push((n >> 16) as u8);
            out.push((n >> 8) as u8);
            continue;
        }
        let n = n | val(c[3])?;
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
        out.push(n as u8);
    }
    Some(out)
}
