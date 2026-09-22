//! Global functions: isNaN, isFinite, URI coding, structuredClone, eval.

use super::*;
use crate::value::*;
use crate::vm::{ErrKind, Vm};

fn is_nan(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = num_arg(vm, a, 0)?;
    Ok(Value::Bool(n.is_nan()))
}

fn is_finite(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = num_arg(vm, a, 0)?;
    Ok(Value::Bool(n.is_finite()))
}

fn uri_error(vm: &mut Vm) -> Ctl {
    vm.error(ErrKind::URIError, "URI malformed")
}

fn encode(vm: &mut Vm, s: &JsStr, keep: &str) -> JsResult<Value> {
    let mut out = String::new();
    let units = s.utf16();
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        let c = if (0xd800..0xdc00).contains(&u) {
            if i + 1 < units.len() && (0xdc00..0xe000).contains(&units[i + 1]) {
                let cp = 0x10000 + (((u as u32) - 0xd800) << 10) + ((units[i + 1] as u32) - 0xdc00);
                i += 1;
                char::from_u32(cp).unwrap()
            } else {
                return Err(uri_error(vm));
            }
        } else if (0xdc00..0xe000).contains(&u) {
            return Err(uri_error(vm));
        } else {
            char::from_u32(u as u32).unwrap()
        };
        i += 1;
        if c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c) || keep.contains(c) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    Ok(Value::string(out))
}

fn decode(vm: &mut Vm, s: &str, reserved: &str) -> JsResult<Value> {
    let b = s.as_bytes();
    let mut out: Vec<u8> = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() {
                return Err(uri_error(vm));
            }
            let h = std::str::from_utf8(&b[i + 1..(i + 3).min(b.len())]).unwrap_or("");
            let Ok(v) = u8::from_str_radix(h, 16) else {
                return Err(uri_error(vm));
            };
            if h.len() != 2 {
                return Err(uri_error(vm));
            }
            if v < 0x80 && reserved.contains(v as char) {
                out.extend_from_slice(&b[i..i + 3]);
            } else {
                out.push(v);
            }
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    match String::from_utf8(out) {
        Ok(s) => Ok(Value::string(s)),
        Err(_) => Err(uri_error(vm)),
    }
}

fn encode_uri_component(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    encode(vm, &s, "")
}
fn encode_uri(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    encode(vm, &s, ";,/?:@&=+$#")
}
fn decode_uri_component(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    decode(vm, &s, "")
}
fn decode_uri(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    decode(vm, &s, ";/?:@&=+$,#")
}

fn escape(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    let mut out = String::new();
    for u in s.utf16() {
        let c = char::from_u32(u as u32).unwrap_or('?');
        if u < 128 && (c.is_ascii_alphanumeric() || "@*_+-./".contains(c)) {
            out.push(c);
        } else if u < 256 {
            out.push_str(&format!("%{u:02X}"));
        } else {
            out.push_str(&format!("%u{u:04X}"));
        }
    }
    Ok(Value::string(out))
}

fn unescape(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    let u = s.utf16();
    let mut out: Vec<u16> = vec![];
    let mut i = 0;
    let hex = |xs: &[u16]| -> Option<u16> {
        let st: String = xs
            .iter()
            .map(|&x| char::from_u32(x as u32).unwrap_or('?'))
            .collect();
        u16::from_str_radix(&st, 16).ok()
    };
    while i < u.len() {
        if u[i] == b'%' as u16 {
            if i + 6 <= u.len() && u[i + 1] == b'u' as u16 {
                if let Some(v) = hex(&u[i + 2..i + 6]) {
                    out.push(v);
                    i += 6;
                    continue;
                }
            }
            if i + 3 <= u.len() {
                if let Some(v) = hex(&u[i + 1..i + 3]) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(u[i]);
        i += 1;
    }
    Ok(Value::Str(JsStr::from_utf16(&out)))
}

fn data_clone_error(vm: &mut Vm, what: &str) -> Ctl {
    let msg = format!("{what} could not be cloned.");
    let ctor = vm.global.own_value("DOMException");
    let e = match ctor {
        Some(c) => match vm.construct(
            &c,
            vec![Value::string(msg.clone()), Value::str("DataCloneError")],
            None,
        ) {
            Ok(v) => v,
            Err(e) => return e,
        },
        None => {
            let e = vm.make_error(ErrKind::Error, &msg);
            e.set_hidden("name", Value::str("DataCloneError"));
            Value::Obj(e)
        }
    };
    if let Value::Obj(o) = &e {
        if let Kind::Error(ed) = &mut o.borrow_mut().kind {
            let user: Vec<String> = ed
                .frames
                .iter()
                .skip_while(|f| !f.starts_with("structuredClone"))
                .skip(1)
                .cloned()
                .collect();
            let mut frames = vec![
                "new DOMException (node:internal/per_context/domexception:76:18)".to_string(),
                "structuredClone (node:internal/worker/js_transferable:127:26)".to_string(),
            ];
            frames.extend(user);
            frames.truncate(10);
            ed.frames = frames;
            ed.arrow = Some("node:internal/worker/js_transferable:127\n  const serializedData = nativeStructuredClone(value, idlOptions);\n                         ^\n".into());
        }
    }
    Ctl::Throw(e)
}

/// A structured clone, as `structuredClone` and `postMessage` make one: every
/// value is copied, except a `SharedArrayBuffer`, which both sides go on
/// sharing, and a function, which cannot cross at all.
pub fn structured_clone_value(vm: &mut Vm, v: &Value) -> JsResult<Value> {
    let mut memo = vec![];
    clone_value(vm, v, &mut memo)
}

/// Another handle on a shared buffer: a new object over the same bytes, which
/// is what a structured clone makes of a `SharedArrayBuffer`.
fn share_buffer(vm: &mut Vm, o: &Obj) -> Obj {
    let buf = match &o.borrow().kind {
        Kind::ArrayBuffer(b) => Some(b.clone()),
        _ => None,
    };
    let Some(b) = buf else { return o.clone() };
    let proto = o.proto();
    let copy = vm.obj_with(proto, Kind::ArrayBuffer(b));
    copy.set_hidden("%shared", Value::Bool(true));
    copy
}

fn clone_value(vm: &mut Vm, v: &Value, memo: &mut Vec<(usize, Obj)>) -> JsResult<Value> {
    if let Value::Sym(s) = v {
        let d = crate::builtins::symbol::symbol_descriptive(s);
        return Err(data_clone_error(vm, &d));
    }
    let Value::Obj(o) = v else {
        return Ok(v.clone());
    };
    if let Some((_, c)) = memo.iter().find(|(a, _)| *a == o.addr()) {
        return Ok(Value::Obj(c.clone()));
    }
    enum K {
        Arr(Vec<Value>),
        Plain,
        Date(f64),
        Map(Vec<(Value, Value)>),
        Set(Vec<Value>),
        Err,
        Prim(Value),
        Bad,
    }
    // Buffers: a shared one keeps its bytes, the rest are copied.
    if o.own_value("%shared").is_some() {
        let copy = share_buffer(vm, o);
        memo.push((o.addr(), copy.clone()));
        return Ok(Value::Obj(copy));
    }
    let buffer = {
        let d = o.borrow();
        match &d.kind {
            Kind::ArrayBuffer(b) => Some(b.borrow().clone()),
            _ => None,
        }
    };
    if let Some(bytes) = buffer {
        let proto = vm.intr.arraybuffer_proto.clone();
        let copy = vm.obj_with(
            Some(proto),
            Kind::ArrayBuffer(std::rc::Rc::new(std::cell::RefCell::new(bytes))),
        );
        memo.push((o.addr(), copy.clone()));
        return Ok(Value::Obj(copy));
    }
    let typed = {
        let d = o.borrow();
        match &d.kind {
            Kind::TypedArray {
                kind,
                buf,
                offset,
                len,
                buf_obj,
            } => Some((*kind, buf.clone(), *offset, *len, buf_obj.clone())),
            _ => None,
        }
    };
    if let Some((kind, buf, offset, len, buf_obj)) = typed {
        let shared = buf_obj
            .as_ref()
            .map(|b| b.own_value("%shared").is_some())
            .unwrap_or(false);
        let proto = vm.intr.typed_protos[crate::builtins::typed::kind_index(kind)].clone();
        let copy = if shared {
            let buf_obj = buf_obj.as_ref().map(|b| share_buffer(vm, b));
            vm.obj_with(
                Some(proto),
                Kind::TypedArray {
                    kind,
                    buf,
                    offset,
                    len,
                    buf_obj,
                },
            )
        } else {
            let size = crate::builtins::typed::elem_size(kind);
            let bytes = {
                let b = buf.borrow();
                b[offset..(offset + len * size).min(b.len())].to_vec()
            };
            vm.new_typed(kind, bytes, Some(proto))
        };
        memo.push((o.addr(), copy.clone()));
        return Ok(Value::Obj(copy));
    }
    let k = {
        let d = o.borrow();
        match &d.kind {
            Kind::Array(a) => K::Arr(a.clone()),
            Kind::Ordinary | Kind::Arguments => K::Plain,
            Kind::Date(t) => K::Date(*t),
            Kind::Map(m) => K::Map(m.entries.iter().flatten().cloned().collect()),
            Kind::Set(m) => K::Set(m.entries.iter().flatten().map(|(k, _)| k.clone()).collect()),
            Kind::Error(_) => K::Err,
            Kind::Boolean(b) => K::Prim(Value::Bool(*b)),
            Kind::Number(n) => K::Prim(Value::Num(*n)),
            Kind::String(s) => K::Prim(Value::Str(s.clone())),
            _ => K::Bad,
        }
    };
    let out = match k {
        K::Arr(items) => {
            let a = vm.new_array(vec![]);
            memo.push((o.addr(), a.clone()));
            let mut nv = vec![];
            for x in items {
                nv.push(if let Value::Empty = x {
                    x
                } else {
                    clone_value(vm, &x, memo)?
                });
            }
            if let Kind::Array(v) = &mut a.borrow_mut().kind {
                *v = nv;
            }
            a
        }
        K::Plain | K::Err => {
            let n = if matches!(k, K::Err) {
                let msg = vm.get_str(v, "message")?;
                let m = vm.to_str(&msg)?;
                let e = vm.make_error(ErrKind::Error, &m);
                // The copy keeps what kind of error it was and the trace it
                // was made with, as V8's structured clone does.
                let proto = o.proto();
                if let Some(p) = proto {
                    e.borrow_mut().proto = Some(p);
                }
                let frames = match &o.borrow().kind {
                    Kind::Error(d) => Some((d.frames.clone(), d.site.clone(), d.arrow.clone())),
                    _ => None,
                };
                if let Some((frames, site, arrow)) = frames {
                    if let Kind::Error(d) = &mut e.borrow_mut().kind {
                        d.frames = frames;
                        d.site = site;
                        d.arrow = arrow;
                    }
                }
                e
            } else {
                vm.new_object()
            };
            memo.push((o.addr(), n.clone()));
            let keys = vm.own_enum_keys(o)?;
            for key in keys {
                let x = vm.get(v, &Key::Str(key.clone()))?;
                let c = clone_value(vm, &x, memo)?;
                n.set_prop(&key, c, ALL);
            }
            n
        }
        K::Date(t) => vm.obj_with(Some(vm.intr.date_proto.clone()), Kind::Date(t)),
        K::Map(entries) => {
            let m = vm.obj_with(Some(vm.intr.map_proto.clone()), Kind::Map(Box::default()));
            memo.push((o.addr(), m.clone()));
            for (a, b) in entries {
                let ca = clone_value(vm, &a, memo)?;
                let cb = clone_value(vm, &b, memo)?;
                if let Kind::Map(md) = &mut m.borrow_mut().kind {
                    md.set(ca, cb);
                }
            }
            m
        }
        K::Set(items) => {
            let s = vm.obj_with(Some(vm.intr.set_proto.clone()), Kind::Set(Box::default()));
            memo.push((o.addr(), s.clone()));
            for a in items {
                let ca = clone_value(vm, &a, memo)?;
                if let Kind::Set(md) = &mut s.borrow_mut().kind {
                    md.set(ca, Value::Undefined);
                }
            }
            s
        }
        K::Prim(p) => vm.to_object(&p)?,
        K::Bad => {
            let src = if o.is_callable() {
                crate::builtins::function::func_to_string(vm, o)
            } else {
                "#<Object>".to_string()
            };
            return Err(data_clone_error(vm, &src));
        }
    };
    Ok(Value::Obj(out))
}

fn structured_clone(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let mut memo = vec![];
    clone_value(vm, &a.arg(0), &mut memo)
}

fn eval(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Str(s) = a.arg(0) else {
        return Ok(a.arg(0));
    };
    vm.eval_source(&s, "eval", true)
}

pub fn install(vm: &mut Vm) {
    let g = vm.global.clone();
    let fs: &[(&str, u32, NativeFn)] = &[
        ("isNaN", 1, is_nan),
        ("isFinite", 1, is_finite),
        ("encodeURIComponent", 1, encode_uri_component),
        ("encodeURI", 1, encode_uri),
        ("decodeURIComponent", 1, decode_uri_component),
        ("decodeURI", 1, decode_uri),
        ("escape", 1, escape),
        ("unescape", 1, unescape),
        ("structuredClone", 1, structured_clone),
        ("eval", 1, eval),
    ];
    for (n, l, f) in fs {
        vm.method(&g, n, *l, *f);
    }
}
