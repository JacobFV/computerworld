//! Network (`fetch`/XHR transport), storage, cookies, history, navigation, console
//! output to the host, dialogs and the deterministic entropy.

use cw_jsvm::value::{Args, HostHooks, JsResult, Key, Obj, TypedKind, Value};
use cw_jsvm::vm::Vm;

use super::{arg_num, arg_str, array_values, inner, str_array, string_val};
use crate::script::inner::{HistoryEntry, LogLevel};
use crate::script::{FetchRequest, StorageArea};

// ---------------------------------------------------------------- fetch

/// `W.fetch(url, method, [[k,v]...], body)` → `{status, statusText, url,
/// headers: [[k,v]...], body: Uint8Array}` or `null` on a network error.
fn fetch(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let url = arg_str(vm, a, 0)?;
    let method = arg_str(vm, a, 1)?;
    let method = if method.is_empty() { "GET".to_owned() } else { method.to_ascii_uppercase() };
    let mut headers = Vec::new();
    for pair in array_values(vm, &a.arg(2))? {
        let items = array_values(vm, &pair)?;
        if items.len() == 2 {
            let k = vm.to_string(&items[0])?.to_string();
            let v = vm.to_string(&items[1])?.to_string();
            headers.push((k.to_ascii_lowercase(), v));
        }
    }
    let body = match a.arg(3) {
        Value::Undefined | Value::Null => None,
        Value::Obj(o) => vm.typed_bytes(&o).or_else(|| match &o.borrow().kind {
            cw_jsvm::value::Kind::ArrayBuffer(b) => Some(b.borrow().clone()),
            _ => None,
        }),
        v => Some(vm.to_string(&v)?.as_bytes().to_vec()),
    };
    let url = inner(vm).borrow().resolve_url(&url);
    let r = inner(vm).borrow_mut().host_fetch(&FetchRequest { url, method, headers, body });
    match r {
        Ok(resp) => {
            let hs: Vec<Value> = resp.headers.iter().map(|(k, v)| vm.arr(vec![string_val(k.to_ascii_lowercase()), Value::str(v)])).collect();
            let hv = vm.arr(hs);
            let body = vm.new_typed(TypedKind::Uint8, resp.body.clone(), None);
            let o = cw_jsvm::builtins::new_obj_from(vm, vec![("status", Value::Num(resp.status as f64)), ("statusText", string_val(resp.status_text.clone())), ("url", string_val(resp.url.clone())), ("headers", hv), ("body", Value::Obj(body))]);
            Ok(Value::Obj(o))
        }
        Err(_) => Ok(Value::Null),
    }
}

fn resolve_url(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let href = arg_str(vm, a, 0)?;
    let base = a.arg(1);
    let r = if base.is_nullish() {
        inner(vm).borrow().resolve_url(&href)
    } else {
        let b = vm.to_string(&base)?.to_string();
        crate::script::inner::resolve_url(&b, &href)
    };
    Ok(string_val(r))
}

// ---------------------------------------------------------------- storage

fn area_of(o: &Obj) -> StorageArea {
    match o.host_id() {
        Some(1) => StorageArea::Session,
        _ => StorageArea::Local,
    }
}

fn storage_member(name: &str) -> bool {
    matches!(name, "length" | "key" | "getItem" | "setItem" | "removeItem" | "clear" | "constructor" | "then" | "toString" | "valueOf" | "toJSON" | "hasOwnProperty") || name.starts_with("__")
}

fn storage_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let Key::Str(s) = k else { return Ok(None) };
    if storage_member(s) {
        return Ok(None);
    }
    let v = inner(vm).borrow_mut().host_storage_get(area_of(o), s);
    Ok(v.map(string_val))
}
fn storage_set(vm: &mut Vm, o: &Obj, k: &Key, v: &Value) -> JsResult<Option<bool>> {
    let Key::Str(s) = k else { return Ok(None) };
    if storage_member(s) {
        return Ok(None);
    }
    let val = vm.to_string(v)?.to_string();
    inner(vm).borrow_mut().host_storage_set(area_of(o), s, &val);
    Ok(Some(true))
}
fn storage_delete(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<bool>> {
    let Key::Str(s) = k else { return Ok(None) };
    if storage_member(s) {
        return Ok(None);
    }
    inner(vm).borrow_mut().host_storage_remove(area_of(o), s);
    Ok(Some(true))
}
fn storage_keys(vm: &mut Vm, o: &Obj) -> JsResult<Vec<Key>> {
    let keys = inner(vm).borrow_mut().host_storage_keys(area_of(o));
    Ok(keys.iter().map(|k| Key::str(k)).collect())
}
pub static STORAGE_HOOKS: HostHooks = HostHooks { class: "Storage", get: storage_get, set: storage_set, delete: storage_delete, keys: storage_keys };

/// `W.storage(area)`: the `Storage` object (0 local, 1 session).
fn storage(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let area = arg_num(vm, a, 0)? as u32;
    let proto = inner(vm).borrow().protos.get("Storage").cloned();
    Ok(Value::Obj(vm.host_obj(proto, &STORAGE_HOOKS, vec![Value::Num(area as f64)])))
}

/// `W.storageOp(area, op, key, value)`.
fn storage_op(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let area = if arg_num(vm, a, 0)? as u32 == 1 { StorageArea::Session } else { StorageArea::Local };
    let op = arg_str(vm, a, 1)?;
    let key = arg_str(vm, a, 2)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    Ok(match op.as_str() {
        "get" => i.host_storage_get(area, &key).map(string_val).unwrap_or(Value::Null),
        "set" => {
            let v = arg_str(vm, a, 3)?;
            i.host_storage_set(area, &key, &v);
            Value::Undefined
        }
        "remove" => {
            i.host_storage_remove(area, &key);
            Value::Undefined
        }
        "keys" => {
            let keys = i.host_storage_keys(area);
            drop(i);
            str_array(vm, &keys)
        }
        "clear" => {
            for k in i.host_storage_keys(area) {
                i.host_storage_remove(area, &k);
            }
            Value::Undefined
        }
        _ => Value::Undefined,
    })
}

// ---------------------------------------------------------------- history, navigation

fn history_op(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let op = arg_str(vm, a, 0)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    Ok(match op.as_str() {
        "length" => Value::Num(i.history.len() as f64),
        "index" => Value::Num(i.history_index as f64),
        "state" => i.history.get(i.history_index).and_then(|e| e.state.clone()).map(string_val).unwrap_or(Value::Null),
        "push" | "replace" => {
            let state = if a.arg(1).is_nullish() { None } else { Some(vm.to_string(&a.arg(1))?.to_string()) };
            let url = arg_str(vm, a, 2)?;
            let url = if url.is_empty() { i.url.clone() } else { i.resolve_url(&url) };
            let entry = HistoryEntry { url: url.clone(), state };
            if op == "push" {
                let idx = i.history_index + 1;
                i.history.truncate(idx);
                i.history.push(entry);
                i.history_index = idx;
            } else {
                let idx = i.history_index;
                i.history[idx] = entry;
            }
            set_url(&mut i, &url);
            Value::Undefined
        }
        "go" => {
            let delta = arg_num(vm, a, 1)? as i64;
            let target = i.history_index as i64 + delta;
            if target < 0 || target >= i.history.len() as i64 || delta == 0 {
                return Ok(Value::Null);
            }
            i.history_index = target as usize;
            let url = i.history[target as usize].url.clone();
            let state = i.history[target as usize].state.clone();
            set_url(&mut i, &url);
            drop(i);
            vm.arr(vec![string_val(url), state.map(string_val).unwrap_or(Value::Null)])
        }
        "setUrl" => {
            let url = arg_str(vm, a, 1)?;
            let url = i.resolve_url(&url);
            set_url(&mut i, &url);
            Value::Undefined
        }
        _ => Value::Undefined,
    })
}

fn set_url(i: &mut crate::script::inner::Inner, url: &str) {
    i.url = url.to_owned();
    i.doc.url = url.to_owned();
    let target = url.split_once('#').map(|(_, h)| h.to_owned()).filter(|h| !h.is_empty());
    if target != i.target_id {
        i.target_id = target;
        if let Some(root) = i.doc.document_element() {
            i.touch_state(root);
        }
        i.sheet_changed();
    }
}

fn navigate(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let url = arg_str(vm, a, 0)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let url = i.resolve_url(&url);
    i.host_navigate(&url);
    Ok(Value::Undefined)
}

/// `W.submitForm(form, submitter, action, method, enctype)`: a script-initiated
/// submission goes to the host.
fn submit_form(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let form = super::arg_node(vm, a, 0)?;
    let submitter = super::node_of(&a.arg(1));
    let action = arg_str(vm, a, 2)?;
    let method = arg_str(vm, a, 3)?;
    let enctype = arg_str(vm, a, 4)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let data = i.form_data_set(form, submitter);
    let action = i.resolve_url(&action);
    i.host_submit_form(&action, &method, &enctype, &data);
    Ok(Value::Undefined)
}

// ---------------------------------------------------------------- output, dialogs, entropy

fn log(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let level = arg_str(vm, a, 0)?;
    let text = arg_str(vm, a, 1)?;
    let level = match level.as_str() {
        "error" => LogLevel::Error,
        "warn" => LogLevel::Warn,
        "info" => LogLevel::Info,
        "debug" => LogLevel::Debug,
        _ => LogLevel::Log,
    };
    // The VM's console writes to its streams; the prelude routes page-level
    // messages (errors, alerts) here directly.
    if level == LogLevel::Error {
        vm.stderr.push_str(&text);
        vm.stderr.push('\n');
    } else {
        vm.stdout.push_str(&text);
        vm.stdout.push('\n');
    }
    Ok(Value::Undefined)
}

fn alert(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let kind = arg_str(vm, a, 0)?;
    let text = arg_str(vm, a, 1)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    i.alerts.push(format!("{kind}: {text}"));
    Ok(Value::Undefined)
}

fn alerts(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let list = inner(vm).borrow().alerts.clone();
    Ok(str_array(vm, &list))
}

fn random_bytes(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_num(vm, a, 0)? as usize;
    let mut out = Vec::with_capacity(n.min(65536));
    while out.len() < n.min(65536) {
        let r = (vm.random() * 4294967296.0) as u32;
        out.extend_from_slice(&r.to_le_bytes());
    }
    out.truncate(n.min(65536));
    Ok(Value::Obj(vm.new_typed(TypedKind::Uint8, out, None)))
}

fn now_micros(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let t = inner(vm).borrow_mut().host_now_micros();
    Ok(Value::Num(t as f64))
}

fn url_info(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let u = inner(vm).borrow().url.clone();
    Ok(string_val(u))
}

/// Decodes UTF-8 bytes (a `Uint8Array`) to a string, lossily.
fn utf8_decode(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else { return Ok(Value::str("")) };
    let bytes = vm.typed_bytes(&o).unwrap_or_default();
    Ok(string_val(String::from_utf8_lossy(&bytes).into_owned()))
}

fn utf8_encode(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = arg_str(vm, a, 0)?;
    Ok(Value::Obj(vm.new_typed(TypedKind::Uint8, s.into_bytes(), None)))
}

fn set_referrer(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = arg_str(vm, a, 0)?;
    inner(vm).borrow_mut().referrer = s;
    Ok(Value::Undefined)
}

pub fn install(vm: &mut Vm, w: &Obj) {
    vm.method(w, "fetch", 4, fetch);
    vm.method(w, "resolveUrl", 2, resolve_url);
    vm.method(w, "storage", 1, storage);
    vm.method(w, "storageOp", 4, storage_op);
    vm.method(w, "history", 3, history_op);
    vm.method(w, "navigate", 1, navigate);
    vm.method(w, "submitForm", 5, submit_form);
    vm.method(w, "log", 2, log);
    vm.method(w, "alert", 2, alert);
    vm.method(w, "alerts", 0, alerts);
    vm.method(w, "randomBytes", 1, random_bytes);
    vm.method(w, "nowMicros", 0, now_micros);
    vm.method(w, "url", 0, url_info);
    vm.method(w, "utf8Decode", 1, utf8_decode);
    vm.method(w, "utf8Encode", 1, utf8_encode);
    vm.method(w, "setReferrer", 1, set_referrer);
    super::canvas_bind::install(vm, w);
}
