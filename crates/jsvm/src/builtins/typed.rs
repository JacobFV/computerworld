//! ArrayBuffer, typed arrays, TextEncoder / TextDecoder.

use super::*;
use crate::bigint::BigInt;
use crate::value::*;
use crate::vm::Vm;
use std::cell::RefCell;
use std::rc::Rc;

pub const KINDS: [(TypedKind, &str, usize); 11] = [
    (TypedKind::Int8, "Int8Array", 1),
    (TypedKind::Uint8, "Uint8Array", 1),
    (TypedKind::Uint8Clamped, "Uint8ClampedArray", 1),
    (TypedKind::Int16, "Int16Array", 2),
    (TypedKind::Uint16, "Uint16Array", 2),
    (TypedKind::Int32, "Int32Array", 4),
    (TypedKind::Uint32, "Uint32Array", 4),
    (TypedKind::Float32, "Float32Array", 4),
    (TypedKind::Float64, "Float64Array", 8),
    (TypedKind::BigInt64, "BigInt64Array", 8),
    (TypedKind::BigUint64, "BigUint64Array", 8),
];

pub fn kind_index(k: TypedKind) -> usize {
    KINDS.iter().position(|(x, _, _)| *x == k).unwrap()
}

pub fn elem_size(k: TypedKind) -> usize {
    KINDS[kind_index(k)].2
}

pub fn kind_name(k: TypedKind) -> &'static str {
    KINDS[kind_index(k)].1
}

fn read(k: TypedKind, b: &[u8]) -> Value {
    match k {
        TypedKind::Int8 => Value::Num(b[0] as i8 as f64),
        TypedKind::Uint8 | TypedKind::Uint8Clamped => Value::Num(b[0] as f64),
        TypedKind::Int16 => Value::Num(i16::from_le_bytes([b[0], b[1]]) as f64),
        TypedKind::Uint16 => Value::Num(u16::from_le_bytes([b[0], b[1]]) as f64),
        TypedKind::Int32 => Value::Num(i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64),
        TypedKind::Uint32 => Value::Num(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64),
        TypedKind::Float32 => Value::Num(f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64),
        TypedKind::Float64 => Value::Num(f64::from_le_bytes(b[..8].try_into().unwrap())),
        TypedKind::BigInt64 => Value::BigInt(Rc::new(BigInt::from_i64(i64::from_le_bytes(
            b[..8].try_into().unwrap(),
        )))),
        TypedKind::BigUint64 => Value::BigInt(Rc::new(BigInt::from_u64(u64::from_le_bytes(
            b[..8].try_into().unwrap(),
        )))),
    }
}

fn encode(vm: &mut Vm, k: TypedKind, v: &Value) -> JsResult<Vec<u8>> {
    Ok(match k {
        TypedKind::BigInt64 | TypedKind::BigUint64 => {
            let b = crate::builtins::number::to_bigint(vm, v)?;
            b.to_u64_mod().to_le_bytes().to_vec()
        }
        _ => {
            let n = vm.to_number(v)?;
            match k {
                TypedKind::Int8 => vec![crate::numconv::to_int32(n) as i8 as u8],
                TypedKind::Uint8 => vec![crate::numconv::to_uint32(n) as u8],
                TypedKind::Uint8Clamped => {
                    let c = if n.is_nan() { 0.0 } else { n.clamp(0.0, 255.0) };
                    // Round half to even.
                    let f = c.floor();
                    let r = if c - f > 0.5 || (c - f == 0.5 && f % 2.0 != 0.0) {
                        f + 1.0
                    } else {
                        f
                    };
                    vec![r as u8]
                }
                TypedKind::Int16 => (crate::numconv::to_int32(n) as i16).to_le_bytes().to_vec(),
                TypedKind::Uint16 => (crate::numconv::to_uint32(n) as u16).to_le_bytes().to_vec(),
                TypedKind::Int32 => crate::numconv::to_int32(n).to_le_bytes().to_vec(),
                TypedKind::Uint32 => crate::numconv::to_uint32(n).to_le_bytes().to_vec(),
                TypedKind::Float32 => (n as f32).to_le_bytes().to_vec(),
                _ => n.to_le_bytes().to_vec(),
            }
        }
    })
}

impl<'h> Vm<'h> {
    pub fn typed_get(&mut self, o: &Obj, i: usize) -> Option<Value> {
        let d = o.borrow();
        if let Kind::TypedArray {
            kind,
            buf,
            offset,
            len,
            ..
        } = &d.kind
        {
            if i >= *len {
                return None;
            }
            let sz = elem_size(*kind);
            let b = buf.borrow();
            let at = offset + i * sz;
            if at + sz > b.len() {
                return None;
            }
            return Some(read(*kind, &b[at..at + sz]));
        }
        None
    }

    pub fn typed_set(&mut self, o: &Obj, i: usize, v: &Value) -> JsResult<()> {
        let (kind, buf, offset, len) = match &o.borrow().kind {
            Kind::TypedArray {
                kind,
                buf,
                offset,
                len,
                ..
            } => (*kind, buf.clone(), *offset, *len),
            _ => return Ok(()),
        };
        let bytes = encode(self, kind, v)?;
        if i < len {
            let at = offset + i * bytes.len();
            let mut b = buf.borrow_mut();
            if at + bytes.len() <= b.len() {
                b[at..at + bytes.len()].copy_from_slice(&bytes);
            }
        }
        Ok(())
    }

    pub fn typed_bytes(&self, o: &Obj) -> Option<Vec<u8>> {
        let d = o.borrow();
        match &d.kind {
            Kind::TypedArray {
                kind,
                buf,
                offset,
                len,
                ..
            } => {
                let b = buf.borrow();
                let sz = elem_size(*kind);
                Some(b[*offset..(*offset + len * sz).min(b.len())].to_vec())
            }
            Kind::ArrayBuffer(b) => Some(b.borrow().clone()),
            _ => None,
        }
    }

    pub fn new_typed(&mut self, kind: TypedKind, bytes: Vec<u8>, proto: Option<Obj>) -> Obj {
        let sz = elem_size(kind);
        let len = bytes.len() / sz;
        let buf = Rc::new(RefCell::new(bytes));
        let proto = proto.unwrap_or_else(|| self.intr.typed_protos[kind_index(kind)].clone());
        self.obj_with(
            Some(proto),
            Kind::TypedArray {
                kind,
                buf,
                offset: 0,
                len,
                buf_obj: None,
            },
        )
    }
}

fn array_buffer_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Some(nt) = a.new_target.clone() else {
        return Err(vm.type_error("Constructor ArrayBuffer requires 'new'"));
    };
    let n = vm.to_integer(&a.arg(0))?;
    if !(0.0..=(1u64 << 30) as f64).contains(&n) {
        return Err(vm.range_error("Array buffer allocation failed"));
    }
    let ap = vm.intr.arraybuffer_proto.clone();
    let proto = vm.proto_from_ctor(&nt, &ap)?;
    Ok(Value::Obj(vm.obj_with(
        Some(proto),
        Kind::ArrayBuffer(Rc::new(RefCell::new(vec![0; n as usize]))),
    )))
}

fn byte_length(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        match &o.borrow().kind {
            Kind::ArrayBuffer(b) => return Ok(Value::Num(b.borrow().len() as f64)),
            Kind::TypedArray { kind, len, .. } => {
                return Ok(Value::Num((len * elem_size(*kind)) as f64))
            }
            _ => {}
        }
    }
    Err(vm.type_error("Receiver is not an ArrayBuffer or typed array"))
}

fn ab_slice(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = &a.this else {
        return Err(vm.type_error("not an ArrayBuffer"));
    };
    let bytes = match &o.borrow().kind {
        Kind::ArrayBuffer(b) => b.borrow().clone(),
        _ => return Err(vm.type_error("not an ArrayBuffer")),
    };
    let len = bytes.len();
    let s = vm.rel_index(&a.arg(0), len, 0)?;
    let e = vm.rel_index(&a.arg(1), len, len)?;
    let out = if s < e { bytes[s..e].to_vec() } else { vec![] };
    Ok(Value::Obj(vm.obj_with(
        Some(vm.intr.arraybuffer_proto.clone()),
        Kind::ArrayBuffer(Rc::new(RefCell::new(out))),
    )))
}

fn typed_kind_of_callee(a: &Args) -> TypedKind {
    let n = Vm::func_name(&a.callee);
    KINDS
        .iter()
        .find(|(_, name, _)| *name == n)
        .map(|x| x.0)
        .unwrap_or(TypedKind::Uint8)
}

pub fn typed_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let kind = typed_kind_of_callee(a);
    let Some(nt) = a.new_target.clone() else {
        return Err(vm.type_error(format!("Constructor {} requires 'new'", kind_name(kind))));
    };
    let default = vm.intr.typed_protos[kind_index(kind)].clone();
    let proto = vm.proto_from_ctor(&nt, &default)?;
    let sz = elem_size(kind);
    let arg = a.arg(0);
    match &arg {
        Value::Obj(o) => {
            let ab = match &o.borrow().kind {
                Kind::ArrayBuffer(b) => Some(b.clone()),
                _ => None,
            };
            if let Some(buf) = ab {
                let off = vm.to_integer(&a.arg(1))?.max(0.0) as usize;
                let total = buf.borrow().len();
                if !off.is_multiple_of(sz) {
                    return Err(vm.range_error(format!(
                        "start offset of {} should be a multiple of {sz}",
                        kind_name(kind)
                    )));
                }
                let len = if a.arg(2).is_undefined() {
                    if (total - off.min(total)) % sz != 0 {
                        return Err(vm.range_error(format!(
                            "byte length of {} should be a multiple of {sz}",
                            kind_name(kind)
                        )));
                    }
                    (total - off.min(total)) / sz
                } else {
                    vm.to_integer(&a.arg(2))? as usize
                };
                if off + len * sz > total {
                    return Err(vm.range_error(format!("Invalid typed array length: {len}")));
                }
                return Ok(Value::Obj(vm.obj_with(
                    Some(proto),
                    Kind::TypedArray {
                        kind,
                        buf,
                        offset: off,
                        len,
                        buf_obj: Some(o.clone()),
                    },
                )));
            }
            let items = if o.is_array()
                || matches!(o.borrow().kind, Kind::TypedArray { .. })
                || !vm
                    .get(&arg, &Key::Sym(vm.syms.iterator.clone()))?
                    .is_nullish()
            {
                vm.iterable_to_vec(&arg)?
            } else {
                let n = vm.length_of(&arg)?;
                let mut v = vec![];
                for i in 0..n {
                    v.push(vm.get_index(&arg, i)?);
                }
                v
            };
            let t = vm.new_typed(kind, vec![0; items.len() * sz], Some(proto));
            for (i, it) in items.iter().enumerate() {
                vm.typed_set(&t, i, it)?;
            }
            Ok(Value::Obj(t))
        }
        _ => {
            let n = if arg.is_undefined() {
                0.0
            } else {
                vm.to_integer(&arg)?
            };
            if !(0.0..=(1u64 << 30) as f64).contains(&n) {
                let d = crate::numconv::number_to_string(n);
                return Err(vm.range_error(format!("Invalid typed array length: {d}")));
            }
            Ok(Value::Obj(vm.new_typed(
                kind,
                vec![0; n as usize * sz],
                Some(proto),
            )))
        }
    }
}

fn this_typed(vm: &mut Vm, a: &Args) -> JsResult<(Obj, TypedKind, usize)> {
    if let Value::Obj(o) = &a.this {
        if let Kind::TypedArray { kind, len, .. } = &o.borrow().kind {
            return Ok((o.clone(), *kind, *len));
        }
    }
    Err(vm.type_error("this is not a typed array."))
}

fn t_length(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (_, _, len) = this_typed(vm, a)?;
    Ok(Value::Num(len as f64))
}

fn t_byte_offset(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, _, _) = this_typed(vm, a)?;
    let off = match &o.borrow().kind {
        Kind::TypedArray { offset, .. } => *offset,
        _ => 0,
    };
    Ok(Value::Num(off as f64))
}

fn t_buffer(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, _, _) = this_typed(vm, a)?;
    let (existing, buf) = match &o.borrow().kind {
        Kind::TypedArray { buf_obj, buf, .. } => (buf_obj.clone(), buf.clone()),
        _ => unreachable!(),
    };
    if let Some(b) = existing {
        return Ok(Value::Obj(b));
    }
    let b = vm.obj_with(
        Some(vm.intr.arraybuffer_proto.clone()),
        Kind::ArrayBuffer(buf),
    );
    if let Kind::TypedArray { buf_obj, .. } = &mut o.borrow_mut().kind {
        *buf_obj = Some(b.clone());
    }
    Ok(Value::Obj(b))
}

fn t_values(vm: &mut Vm, a: &Args) -> JsResult<Vec<Value>> {
    let (o, _, len) = this_typed(vm, a)?;
    Ok((0..len)
        .map(|i| vm.typed_get(&o, i).unwrap_or(Value::Undefined))
        .collect())
}

fn t_subarray(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, kind, len) = this_typed(vm, a)?;
    let s = vm.rel_index(&a.arg(0), len, 0)?;
    let e = vm.rel_index(&a.arg(1), len, len)?;
    let (buf, offset, bo) = match &o.borrow().kind {
        Kind::TypedArray {
            buf,
            offset,
            buf_obj,
            ..
        } => (buf.clone(), *offset, buf_obj.clone()),
        _ => unreachable!(),
    };
    let proto = o.proto();
    Ok(Value::Obj(vm.obj_with(
        proto,
        Kind::TypedArray {
            kind,
            buf,
            offset: offset + s * elem_size(kind),
            len: e.saturating_sub(s),
            buf_obj: bo,
        },
    )))
}

fn t_slice(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, kind, len) = this_typed(vm, a)?;
    let s = vm.rel_index(&a.arg(0), len, 0)?;
    let e = vm.rel_index(&a.arg(1), len, len)?;
    let bytes = vm.typed_bytes(&o).unwrap_or_default();
    let sz = elem_size(kind);
    let out = if s < e {
        bytes[s * sz..e * sz].to_vec()
    } else {
        vec![]
    };
    let proto = o.proto();
    Ok(Value::Obj(vm.new_typed(kind, out, proto)))
}

fn t_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, _, len) = this_typed(vm, a)?;
    let src = vm.iterable_to_vec(&a.arg(0))?;
    let off = vm.to_integer(&a.arg(1))?.max(0.0) as usize;
    if off + src.len() > len {
        return Err(vm.range_error("offset is out of bounds"));
    }
    for (i, v) in src.iter().enumerate() {
        vm.typed_set(&o, off + i, v)?;
    }
    Ok(Value::Undefined)
}

fn t_fill(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, _, len) = this_typed(vm, a)?;
    let s = vm.rel_index(&a.arg(1), len, 0)?;
    let e = vm.rel_index(&a.arg(2), len, len)?;
    let v = a.arg(0);
    for i in s..e {
        vm.typed_set(&o, i, &v)?;
    }
    Ok(a.this.clone())
}

fn t_join(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let vals = t_values(vm, a)?;
    let sep = if a.arg(0).is_undefined() {
        ",".to_string()
    } else {
        vm.to_str(&a.arg(0))?
    };
    let mut parts = vec![];
    for v in vals {
        parts.push(vm.to_str(&v)?);
    }
    Ok(Value::string(parts.join(&sep)))
}

/// Array-method bridge: runs the Array.prototype method on a plain copy.
fn bridge(vm: &mut Vm, a: &mut Args, name: &str, wrap: bool) -> JsResult<Value> {
    let (o, kind, _) = this_typed(vm, a)?;
    let vals = t_values(vm, a)?;
    let arr = vm.arr(vals);
    let ap = vm.intr.array_proto.clone();
    let f = vm.get_str(&Value::Obj(ap), name)?;
    let r = vm.call(&f, arr, a.args.clone())?;
    if wrap {
        let items = vm.iterable_to_vec(&r)?;
        let t = vm.new_typed(kind, vec![0; items.len() * elem_size(kind)], o.proto());
        for (i, v) in items.iter().enumerate() {
            vm.typed_set(&t, i, v)?;
        }
        return Ok(Value::Obj(t));
    }
    Ok(r)
}

macro_rules! bridged {
    ($name:ident, $js:expr, $wrap:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
            bridge(vm, a, $js, $wrap)
        }
    };
}
bridged!(t_map, "map", true);
bridged!(t_filter, "filter", true);
bridged!(t_for_each, "forEach", false);
bridged!(t_reduce, "reduce", false);
bridged!(t_index_of, "indexOf", false);
bridged!(t_includes, "includes", false);
bridged!(t_find, "find", false);
bridged!(t_find_index, "findIndex", false);
bridged!(t_every, "every", false);
bridged!(t_some, "some", false);
bridged!(t_at, "at", false);

fn t_sort(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, _, _) = this_typed(vm, a)?;
    let mut vals = t_values(vm, a)?;
    let cmp = a.arg(0);
    if cmp.is_callable() {
        crate::builtins::array::sort_values(vm, &mut vals, &cmp)?;
    } else {
        vals.sort_by(|x, y| match (x, y) {
            (Value::Num(p), Value::Num(q)) => p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal),
            (Value::BigInt(p), Value::BigInt(q)) => p.cmp(q),
            _ => std::cmp::Ordering::Equal,
        });
    }
    for (i, v) in vals.iter().enumerate() {
        vm.typed_set(&o, i, v)?;
    }
    Ok(a.this.clone())
}

fn t_reverse(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, _, _) = this_typed(vm, a)?;
    let mut vals = t_values(vm, a)?;
    vals.reverse();
    for (i, v) in vals.iter().enumerate() {
        vm.typed_set(&o, i, v)?;
    }
    Ok(a.this.clone())
}

fn t_iter(vm: &mut Vm, a: &mut Args, k: IterKind) -> JsResult<Value> {
    let (o, _, _) = this_typed(vm, a)?;
    Ok(crate::builtins::array::make_array_iter(
        vm,
        Value::Obj(o),
        k,
    ))
}
fn t_values_m(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    t_iter(vm, a, IterKind::Values)
}
fn t_keys(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    t_iter(vm, a, IterKind::Keys)
}
fn t_entries(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    t_iter(vm, a, IterKind::Entries)
}

fn t_from(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let items = vm.iterable_to_vec(&a.arg(0))?;
    let items = if a.arg(1).is_callable() {
        let f = a.arg(1);
        let mut out = vec![];
        for (i, v) in items.into_iter().enumerate() {
            out.push(vm.call(&f, Value::Undefined, vec![v, Value::Num(i as f64)])?);
        }
        out
    } else {
        items
    };
    let arr = vm.arr(items);
    vm.construct(&a.this, vec![arr], None)
}

fn t_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let arr = vm.arr(a.args.clone());
    vm.construct(&a.this, vec![arr], None)
}

fn t_tag(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        if let Kind::TypedArray { kind, .. } = &o.borrow().kind {
            return Ok(Value::str(kind_name(*kind)));
        }
    }
    Ok(Value::Undefined)
}

fn text_encoder_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.new_object();
    if let Some(nt) = &a.new_target {
        if let Value::Obj(p) = vm.get_str(&Value::Obj(nt.clone()), "prototype")? {
            o.borrow_mut().proto = Some(p);
        }
    }
    Ok(Value::Obj(o))
}

fn text_encode(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = if a.arg(0).is_undefined() {
        JsStr::new("")
    } else {
        str_arg(vm, a, 0)?
    };
    Ok(Value::Obj(vm.new_typed(
        TypedKind::Uint8,
        s.as_bytes().to_vec(),
        None,
    )))
}

fn text_decode(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let bytes = match a.arg(0) {
        Value::Obj(o) => vm.typed_bytes(&o).unwrap_or_default(),
        _ => vec![],
    };
    Ok(Value::string(String::from_utf8_lossy(&bytes).into_owned()))
}

fn encoding_getter(_vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::str("utf-8"))
}

pub fn install(vm: &mut Vm) {
    let tag = vm.syms.to_string_tag.clone();
    let abp = vm.intr.arraybuffer_proto.clone();
    let abc = vm.make_ctor("ArrayBuffer", 1, array_buffer_ctor, &abp);
    vm.getter(&abp, "byteLength", byte_length);
    vm.method(&abp, "slice", 2, ab_slice);
    abp.set_sym(&tag, Value::str("ArrayBuffer"), CONFIGURABLE);
    vm.set_global("ArrayBuffer", Value::Obj(abc));
    // %TypedArray%
    let tap = vm.new_object();
    let fs: &[(&str, u32, NativeFn)] = &[
        ("subarray", 2, t_subarray),
        ("slice", 2, t_slice),
        ("set", 1, t_set),
        ("fill", 1, t_fill),
        ("join", 1, t_join),
        ("map", 1, t_map),
        ("filter", 1, t_filter),
        ("forEach", 1, t_for_each),
        ("reduce", 1, t_reduce),
        ("indexOf", 1, t_index_of),
        ("includes", 1, t_includes),
        ("find", 1, t_find),
        ("findIndex", 1, t_find_index),
        ("every", 1, t_every),
        ("some", 1, t_some),
        ("at", 1, t_at),
        ("sort", 1, t_sort),
        ("reverse", 0, t_reverse),
        ("keys", 0, t_keys),
        ("entries", 0, t_entries),
    ];
    for (n, l, f) in fs {
        vm.method(&tap, n, *l, *f);
    }
    let vals = vm.method(&tap, "values", 0, t_values_m);
    let it = vm.syms.iterator.clone();
    tap.set_sym(&it, Value::Obj(vals), HIDDEN);
    vm.getter(&tap, "length", t_length);
    vm.getter(&tap, "byteLength", byte_length);
    vm.getter(&tap, "byteOffset", t_byte_offset);
    vm.getter(&tap, "buffer", t_buffer);
    let tagf = vm.native_fn("get [Symbol.toStringTag]", 0, t_tag);
    tap.borrow_mut().props.insert(
        Key::Sym(tag.clone()),
        Prop {
            slot: Slot::Accessor(Some(tagf), None),
            flags: CONFIGURABLE,
        },
    );
    let ap = vm.intr.array_proto.clone();
    if let Some(ts) = ap.own_value("toString") {
        tap.set_hidden("toString", ts);
    }
    let mut protos = vec![];
    for (kind, name, sz) in KINDS {
        let p = vm.obj_with(Some(tap.clone()), Kind::Ordinary);
        let c = vm.make_ctor(name, 3, typed_ctor, &p);
        vm.constant(&c, "BYTES_PER_ELEMENT", Value::Num(sz as f64));
        vm.constant(&p, "BYTES_PER_ELEMENT", Value::Num(sz as f64));
        vm.method(&c, "from", 1, t_from);
        vm.method(&c, "of", 0, t_of);
        let _ = kind;
        vm.set_global(name, Value::Obj(c));
        protos.push(p);
    }
    vm.intr.typed_protos = protos;
    // TextEncoder / TextDecoder
    let tep = vm.new_object();
    let tec = vm.make_ctor("TextEncoder", 0, text_encoder_ctor, &tep);
    vm.method(&tep, "encode", 1, text_encode);
    vm.getter(&tep, "encoding", encoding_getter);
    vm.set_global("TextEncoder", Value::Obj(tec));
    let tdp = vm.new_object();
    let tdc = vm.make_ctor("TextDecoder", 0, text_encoder_ctor, &tdp);
    vm.method(&tdp, "decode", 1, text_decode);
    vm.getter(&tdp, "encoding", encoding_getter);
    vm.set_global("TextDecoder", Value::Obj(tdc));
}
