//! Form control state, focus/hover state, and the host objects hanging off an
//! element: `dataset`, `classList`-style token lists and `attributes`.

use cw_jsvm::value::{Args, HostHooks, JsResult, Key, Obj, Value};
use cw_jsvm::vm::Vm;

use super::dom::{set_attribute_value, wrap_node};
use super::{arg_node, arg_num, arg_str, inner, node_array, node_of, opt_node, string_val};
use crate::dom::NodeId;

// ---------------------------------------------------------------- control state

fn value_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let v = inner(vm).borrow().control_value(n);
    Ok(string_val(v))
}

fn value_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let v = arg_str(vm, a, 1)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    if i.doc.is(n, "select") {
        let options = i.options_of(n);
        let hit = options.iter().copied().find(|o| i.option_value(*o) == v);
        for o in options {
            i.form.checked.insert(o, Some(o) == hit);
        }
        i.touch_state(n);
    } else {
        i.set_value(n, &v);
    }
    Ok(Value::Undefined)
}

/// Whether the control's value was set by script or the user (the dirty flag).
fn value_dirty(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let d = inner(vm).borrow().form.values.contains_key(&n);
    Ok(Value::Bool(d))
}

fn checked_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let c = inner(vm).borrow().is_checked(n);
    Ok(Value::Bool(c))
}

fn checked_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let on = a.arg(1).truthy();
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    if i.doc.is(n, "option") {
        i.set_option_selected(n, on);
    } else {
        i.set_checked(n, on);
    }
    Ok(Value::Undefined)
}

fn indeterminate(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    if a.len() > 1 {
        if a.arg(1).truthy() {
            i.form.indeterminate.insert(n);
        } else {
            i.form.indeterminate.remove(&n);
        }
        i.touch_state(n);
    }
    Ok(Value::Bool(i.form.indeterminate.contains(&n)))
}

fn selected_index(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    if a.len() > 1 {
        let idx = arg_num_raw(&a.arg(1));
        let options = i.options_of(n);
        for (k, o) in options.iter().enumerate() {
            i.form.checked.insert(*o, k as f64 == idx);
        }
        i.touch_state(n);
        return Ok(Value::Undefined);
    }
    let options = i.options_of(n);
    let selected = i.selected_options(n);
    let idx = selected.first().and_then(|s| options.iter().position(|o| o == s));
    Ok(Value::Num(idx.map(|x| x as f64).unwrap_or(-1.0)))
}

fn arg_num_raw(v: &Value) -> f64 {
    match v {
        Value::Num(n) => *n,
        _ => -1.0,
    }
}

fn form_owner(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let f = inner(vm).borrow().form_owner(n);
    Ok(opt_node(vm, f))
}

fn form_data_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let form = arg_node(vm, a, 0)?;
    let submitter = node_of(&a.arg(1));
    let pairs = inner(vm).borrow().form_data_set(form, submitter);
    let items: Vec<Value> = pairs.into_iter().map(|(k, v)| vm.arr(vec![string_val(k), string_val(v)])).collect();
    Ok(vm.arr(items))
}

fn reset_form(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let form = arg_node(vm, a, 0)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let els = i.form_elements(form);
    for e in els {
        i.form.values.remove(&e);
        i.form.checked.remove(&e);
        i.form.indeterminate.remove(&e);
        for o in i.options_of(e) {
            i.form.checked.remove(&o);
        }
        i.touch_state(e);
    }
    Ok(Value::Undefined)
}

fn is_disabled(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let d = inner(vm).borrow().is_disabled(n);
    Ok(Value::Bool(d))
}

fn selection(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let len = i.control_value(n).chars().count();
    if a.len() > 2 {
        let s = (arg_num_raw(&a.arg(1)).max(0.0) as usize).min(len);
        let e = (arg_num_raw(&a.arg(2)).max(0.0) as usize).min(len);
        i.form.selection.insert(n, (s.min(e), e));
        return Ok(Value::Undefined);
    }
    let (s, e) = i.form.selection.get(&n).copied().unwrap_or((len, len));
    drop(i);
    Ok(vm.arr(vec![Value::Num(s.min(len) as f64), Value::Num(e.min(len) as f64)]))
}

fn custom_validity(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    if a.len() > 1 {
        let m = arg_str(vm, a, 1)?;
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        if m.is_empty() {
            i.form.custom_validity.remove(&n);
        } else {
            i.form.custom_validity.insert(n, m);
        }
        i.touch_state(n);
        return Ok(Value::Undefined);
    }
    let m = inner(vm).borrow().form.custom_validity.get(&n).cloned().unwrap_or_default();
    Ok(string_val(m))
}

// ---------------------------------------------------------------- focus, hover

fn set_focused(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = node_of(&a.arg(0));
    let visible = a.arg(1).truthy();
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let old = i.focused;
    if old != n {
        if let Some(o) = old {
            i.touch_state(o);
        }
        if let Some(x) = n {
            i.touch_state(x);
        }
        i.focused = n;
    }
    i.focus_visible = visible;
    Ok(Value::Undefined)
}

fn is_focusable(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let f = inner(vm).borrow().is_focusable(n);
    Ok(Value::Bool(f))
}

fn focus_order(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let nodes = inner(vm).borrow().focus_order();
    Ok(node_array(vm, &nodes))
}

// ---------------------------------------------------------------- dataset

fn data_attr(prop: &str) -> String {
    let mut s = String::from("data-");
    for c in prop.chars() {
        if c.is_ascii_uppercase() {
            s.push('-');
            s.push(c.to_ascii_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

fn data_prop(attr: &str) -> Option<String> {
    let rest = attr.strip_prefix("data-")?;
    let mut out = String::new();
    let mut up = false;
    for c in rest.chars() {
        if c == '-' {
            up = true;
        } else if up {
            out.push(c.to_ascii_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn dataset_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let (Key::Str(s), Some(id)) = (k, o.host_id()) else { return Ok(None) };
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(i.doc.attr(NodeId(id), &data_attr(s)).map(Value::str))
}
fn dataset_set(vm: &mut Vm, o: &Obj, k: &Key, v: &Value) -> JsResult<Option<bool>> {
    let (Key::Str(s), Some(id)) = (k, o.host_id()) else { return Ok(None) };
    let val = vm.to_string(v)?.to_string();
    set_attribute_value(vm, NodeId(id), &data_attr(s), Some(&val))?;
    Ok(Some(true))
}
fn dataset_delete(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<bool>> {
    let (Key::Str(s), Some(id)) = (k, o.host_id()) else { return Ok(None) };
    set_attribute_value(vm, NodeId(id), &data_attr(s), None)?;
    Ok(Some(true))
}
fn dataset_keys(vm: &mut Vm, o: &Obj) -> JsResult<Vec<Key>> {
    let Some(id) = o.host_id() else { return Ok(Vec::new()) };
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(i.doc.attrs(NodeId(id)).iter().filter_map(|a| data_prop(&a.name)).map(|p| Key::str(&p)).collect())
}
pub static DATASET_HOOKS: HostHooks = HostHooks { class: "DOMStringMap", get: dataset_get, set: dataset_set, delete: dataset_delete, keys: dataset_keys };

// ---------------------------------------------------------------- token lists

fn tokens_of(vm: &mut Vm, o: &Obj) -> Vec<String> {
    let (Some(id), Some(Value::Str(attr))) = (o.host_id(), o.host_slot(1)) else { return Vec::new() };
    let rc = inner(vm);
    let i = rc.borrow();
    let mut out: Vec<String> = Vec::new();
    for t in i.doc.attr(NodeId(id), &attr).unwrap_or("").split_ascii_whitespace() {
        if !out.iter().any(|x| x == t) {
            out.push(t.to_owned());
        }
    }
    out
}
fn tokens_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    if let Some(idx) = k.array_index() {
        return Ok(tokens_of(vm, o).get(idx as usize).map(|s| Value::str(s)));
    }
    if k.as_str() == Some("length") {
        return Ok(Some(Value::Num(tokens_of(vm, o).len() as f64)));
    }
    Ok(None)
}
fn tokens_keys(vm: &mut Vm, o: &Obj) -> JsResult<Vec<Key>> {
    Ok((0..tokens_of(vm, o).len()).map(|i| Key::str(&i.to_string())).collect())
}
fn no_set(_vm: &mut Vm, _o: &Obj, _k: &Key, _v: &Value) -> JsResult<Option<bool>> {
    Ok(None)
}
fn no_delete(_vm: &mut Vm, _o: &Obj, _k: &Key) -> JsResult<Option<bool>> {
    Ok(None)
}
pub static TOKENS_HOOKS: HostHooks = HostHooks { class: "DOMTokenList", get: tokens_get, set: no_set, delete: no_delete, keys: tokens_keys };

// ---------------------------------------------------------------- attributes map

fn attrs_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let (Key::Str(s), Some(id)) = (k, o.host_id()) else { return Ok(None) };
    let n = NodeId(id);
    let name = {
        let rc = inner(vm);
        let i = rc.borrow();
        if let Some(idx) = k.array_index() {
            i.doc.attrs(n).get(idx as usize).map(|a| a.name.clone())
        } else if s.as_str() == "length" {
            return Ok(Some(Value::Num(i.doc.attrs(n).len() as f64)));
        } else if i.doc.has_attr(n, s) && !matches!(s.as_str(), "item" | "constructor") {
            Some(s.to_string())
        } else {
            None
        }
    };
    let Some(name) = name else { return Ok(None) };
    let Some(hooks) = vm.global.own_value("%hooks") else { return Ok(None) };
    let f = vm.get_str(&hooks, "attrNode")?;
    let el = wrap_node(vm, n);
    Ok(Some(vm.call(&f, hooks, vec![el, string_val(name)])?))
}
fn attrs_keys(vm: &mut Vm, o: &Obj) -> JsResult<Vec<Key>> {
    let Some(id) = o.host_id() else { return Ok(Vec::new()) };
    let n = inner(vm).borrow().doc.attrs(NodeId(id)).len();
    Ok((0..n).map(|i| Key::str(&i.to_string())).collect())
}
pub static ATTRS_HOOKS: HostHooks = HostHooks { class: "NamedNodeMap", get: attrs_get, set: no_set, delete: no_delete, keys: attrs_keys };

/// `W.elementPart(el, kind, protoName, attr?)`: `dataset`, `tokens`, `attributes`.
fn element_part(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let kind = arg_str(vm, a, 1)?;
    let proto_name = arg_str(vm, a, 2)?;
    let extra = a.arg(3);
    let proto = inner(vm).borrow().protos.get(&proto_name).cloned();
    let hooks: &'static HostHooks = match kind.as_str() {
        "dataset" => &DATASET_HOOKS,
        "tokens" => &TOKENS_HOOKS,
        _ => &ATTRS_HOOKS,
    };
    Ok(Value::Obj(vm.host_obj(proto, hooks, vec![Value::Num(n.0 as f64), extra])))
}

fn tokens(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else { return Ok(vm.arr(vec![])) };
    let t = tokens_of(vm, &o);
    Ok(super::str_array(vm, &t))
}

/// The element and attribute name of a token list or dataset part.
fn part_owner(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else { return Ok(Value::Null) };
    let Some(id) = o.host_id() else { return Ok(Value::Null) };
    Ok(wrap_node(vm, NodeId(id)))
}

fn unused(_vm: &mut Vm, a: &mut Args) -> JsResult<f64> {
    arg_num(_vm, a, 0)
}

pub fn install(vm: &mut Vm, w: &Obj) {
    let _ = unused;
    vm.method(w, "value", 1, value_get);
    vm.method(w, "setValue", 2, value_set);
    vm.method(w, "valueDirty", 1, value_dirty);
    vm.method(w, "checked", 1, checked_get);
    vm.method(w, "setChecked", 2, checked_set);
    vm.method(w, "indeterminate", 2, indeterminate);
    vm.method(w, "selectedIndex", 2, selected_index);
    vm.method(w, "formOwner", 1, form_owner);
    vm.method(w, "formDataSet", 2, form_data_set);
    vm.method(w, "resetForm", 1, reset_form);
    vm.method(w, "isDisabled", 1, is_disabled);
    vm.method(w, "selection", 3, selection);
    vm.method(w, "customValidity", 2, custom_validity);
    vm.method(w, "setFocused", 2, set_focused);
    vm.method(w, "isFocusable", 1, is_focusable);
    vm.method(w, "focusOrder", 0, focus_order);
    vm.method(w, "elementPart", 4, element_part);
    vm.method(w, "tokens", 1, tokens);
    vm.method(w, "partOwner", 1, part_owner);
}
