//! The native half of the Web API: a `%web` binding object of functions the prelude
//! (`web.js`) calls, native accessors installed on the prelude's prototypes for the
//! hot DOM and geometry properties, and the host-object hooks that give collections,
//! style declarations, storage and datasets their named and indexed properties.
//!
//! Conventions: a DOM node is a `Kind::Host` object whose first slot is its
//! `NodeId`; natives take wrappers (or `null`) and return wrappers, strings, numbers
//! and plain arrays. A native never holds the realm's `RefCell` borrow across a call
//! back into JavaScript.

pub mod canvas_bind;
pub mod dom;
pub mod dom_forms;
pub mod dom_query;
pub mod events;
pub mod layout;
pub mod misc;
pub mod style;

use std::cell::RefCell;
use std::rc::Rc;

use cw_jsvm::value::{Args, Ctl, HostHooks, JsResult, Key, NativeFn, Obj, Value, ALL, HIDDEN};
use cw_jsvm::vm::{ErrKind, Vm};

use super::inner::Inner;
use crate::dom::NodeId;

/// The realm state behind a VM.
pub fn inner(vm: &Vm) -> Rc<RefCell<Inner>> {
    vm.embedder::<RefCell<Inner>>().expect("realm state")
}

pub fn node_of(v: &Value) -> Option<NodeId> {
    match v {
        Value::Obj(o) => o.host_id().map(NodeId),
        _ => None,
    }
}

/// The node of `this` or a type error.
pub fn this_node(vm: &mut Vm, a: &Args) -> JsResult<NodeId> {
    match node_of(&a.this) {
        Some(n) => Ok(n),
        None => Err(vm.type_error("Illegal invocation")),
    }
}

pub fn arg_node(vm: &mut Vm, a: &Args, i: usize) -> JsResult<NodeId> {
    match node_of(&a.arg(i)) {
        Some(n) => Ok(n),
        None => Err(vm.type_error(format!("parameter {} is not of type 'Node'", i + 1))),
    }
}

pub fn arg_str(vm: &mut Vm, a: &Args, i: usize) -> JsResult<String> {
    let v = a.arg(i);
    if v.is_undefined() {
        return Ok(String::new());
    }
    Ok(vm.to_string(&v)?.to_string())
}

pub fn arg_num(vm: &mut Vm, a: &Args, i: usize) -> JsResult<f64> {
    let v = a.arg(i);
    if v.is_undefined() {
        return Ok(0.0);
    }
    vm.to_number(&v)
}

pub fn arg_bool(a: &Args, i: usize) -> bool {
    a.arg(i).truthy()
}

pub fn str_val(s: &str) -> Value {
    Value::str(s)
}

pub fn string_val(s: String) -> Value {
    Value::string(s)
}

pub fn opt_node(vm: &mut Vm, n: Option<NodeId>) -> Value {
    match n {
        Some(n) => dom::wrap_node(vm, n),
        None => Value::Null,
    }
}

pub fn node_array(vm: &mut Vm, nodes: &[NodeId]) -> Value {
    let v: Vec<Value> = nodes.iter().map(|n| dom::wrap_node(vm, *n)).collect();
    vm.arr(v)
}

pub fn str_array(vm: &mut Vm, items: &[String]) -> Value {
    let v: Vec<Value> = items.iter().map(|s| Value::str(s)).collect();
    vm.arr(v)
}

/// Throws a `DOMException` with the given name.
pub fn dom_exception(vm: &mut Vm, name: &str, msg: &str) -> Ctl {
    let ctor = vm.global.own_value("DOMException");
    if let Some(ctor) = ctor {
        if let Ok(e) = vm.construct(&ctor, vec![Value::str(msg), Value::str(name)], None) {
            return Ctl::Throw(e);
        }
    }
    vm.error(ErrKind::Error, format!("{name}: {msg}"))
}

/// Defines a native method on an object.
pub fn method(vm: &Vm, target: &Obj, name: &str, len: u32, f: NativeFn) {
    vm.method(target, name, len, f);
}

/// A native getter (and optional setter) on a prototype.
pub fn accessor(vm: &Vm, target: &Obj, name: &str, get: NativeFn, set: Option<NativeFn>) {
    vm.accessor(target, name, get, set);
}

/// Reads a JS array (or array-like) into values.
pub fn array_values(vm: &mut Vm, v: &Value) -> JsResult<Vec<Value>> {
    match v {
        Value::Obj(o) if o.is_array() => Ok(match &o.borrow().kind {
            cw_jsvm::value::Kind::Array(items) => items.clone(),
            _ => Vec::new(),
        }),
        Value::Obj(_) => vm.iterable_to_vec(v),
        _ => Ok(Vec::new()),
    }
}

/// Plain hooks for host objects with no exotic properties.
fn plain_get(_vm: &mut Vm, _o: &Obj, _k: &Key) -> JsResult<Option<Value>> {
    Ok(None)
}
fn plain_set(_vm: &mut Vm, _o: &Obj, _k: &Key, _v: &Value) -> JsResult<Option<bool>> {
    Ok(None)
}
fn plain_delete(_vm: &mut Vm, _o: &Obj, _k: &Key) -> JsResult<Option<bool>> {
    Ok(None)
}
fn plain_keys(_vm: &mut Vm, _o: &Obj) -> JsResult<Vec<Key>> {
    Ok(Vec::new())
}

pub static PLAIN_HOOKS: HostHooks = HostHooks {
    class: "Object",
    get: plain_get,
    set: plain_set,
    delete: plain_delete,
    keys: plain_keys,
    plain: true,
};

/// A host object holding an opaque id (a stylesheet, a canvas context).
pub fn handle_obj(vm: &mut Vm, proto_name: &str, id: u32, extra: Vec<Value>) -> Value {
    let proto = inner(vm).borrow().protos.get(proto_name).cloned();
    let mut data = vec![Value::Num(id as f64)];
    data.extend(extra);
    Value::Obj(vm.host_obj(proto, &PLAIN_HOOKS, data))
}

/// Installs the `%web` binding object and the prelude hooks.
pub fn install(vm: &mut Vm) {
    let w = vm.new_object();
    dom::install(vm, &w);
    style::install(vm, &w);
    layout::install(vm, &w);
    misc::install(vm, &w);
    vm.method(&w, "registerProtos", 1, register_protos);
    vm.global.set_hidden("%web", Value::Obj(w));
    // The browser realm has no Node process surface.
    for name in ["process", "require", "module", "Buffer", "global"] {
        let key = Key::str(name);
        vm.global.borrow_mut().props.remove(&key);
    }
}

/// `W.registerProtos({Node: Node.prototype, ...})`: remembers the prelude's
/// prototypes (for wrapper creation) and installs the native accessors and methods
/// on them.
fn register_protos(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let map = a.arg(0);
    let Value::Obj(m) = &map else {
        return Ok(Value::Undefined);
    };
    let keys = vm.own_enum_keys(m)?;
    let mut protos = Vec::new();
    for k in keys {
        let v = vm.get_str(&map, &k)?;
        if let Value::Obj(p) = v {
            protos.push((k.to_string(), p));
        }
    }
    {
        let rc = inner(vm);
        let mut inner = rc.borrow_mut();
        for (k, p) in &protos {
            inner.protos.insert(k.clone(), p.clone());
        }
    }
    let get = |name: &str| {
        protos
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, p)| p.clone())
    };
    if let Some(p) = get("Node") {
        dom::install_node_accessors(vm, &p);
    }
    if let Some(p) = get("Element") {
        dom::install_element_accessors(vm, &p);
        layout::install_element_accessors(vm, &p);
    }
    if let Some(p) = get("HTMLElement") {
        layout::install_html_element_accessors(vm, &p);
    }
    if let Some(p) = get("CharacterData") {
        dom::install_character_data_accessors(vm, &p);
    }
    Ok(Value::Undefined)
}

/// Sets a hidden data property (non-enumerable) on a wrapper.
pub fn set_hidden(o: &Obj, k: &str, v: Value) {
    o.borrow_mut()
        .props
        .insert(Key::str(k), cw_jsvm::value::Prop::data(v, HIDDEN));
}

pub fn set_data(o: &Obj, k: &str, v: Value) {
    o.borrow_mut()
        .props
        .insert(Key::str(k), cw_jsvm::value::Prop::data(v, ALL));
}
