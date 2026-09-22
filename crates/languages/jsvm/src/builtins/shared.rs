//! `SharedArrayBuffer` and `Atomics`.
//!
//! A shared buffer is an ordinary buffer marked as shared: a structured clone
//! passes it along instead of copying it, so every context that has been sent
//! one reads and writes the same bytes. Since one context runs at a time, an
//! atomic operation is just a read and a write; `Atomics.wait` is where the
//! interpreter hands the turn to the other contexts (see `crate::workers`).
use crate::builtins::typed::elem_size;
use crate::value::*;
use crate::vm::*;
use std::cell::RefCell;
use std::rc::Rc;

/// The bytes of a shared or ordinary buffer object.
fn buffer_of(o: &Obj) -> Option<Rc<RefCell<Vec<u8>>>> {
    match &o.borrow().kind {
        Kind::ArrayBuffer(b) => Some(b.clone()),
        _ => None,
    }
}

fn sab_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Some(nt) = a.new_target.clone() else {
        return Err(vm.type_error("Constructor SharedArrayBuffer requires 'new'"));
    };
    let n = vm.to_integer(&a.arg(0))?;
    if !(0.0..=(1u64 << 30) as f64).contains(&n) {
        return Err(vm.range_error("SharedArrayBuffer allocation failed"));
    }
    let default = match vm.global.own_value("%SharedArrayBufferProto") {
        Some(Value::Obj(p)) => p,
        _ => vm.intr.arraybuffer_proto.clone(),
    };
    let proto = vm.proto_from_ctor(&nt, &default)?;
    let o = vm.obj_with(
        Some(proto),
        Kind::ArrayBuffer(Rc::new(RefCell::new(vec![0; n as usize]))),
    );
    o.set_hidden("%shared", Value::Bool(true));
    Ok(Value::Obj(o))
}

fn sab_byte_length(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        if let Some(b) = buffer_of(o) {
            let n = b.borrow().len();
            return Ok(Value::Num(n as f64));
        }
    }
    Err(vm.type_error("Receiver is not a SharedArrayBuffer"))
}

fn sab_slice(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = &a.this else {
        return Err(vm.type_error("Receiver is not a SharedArrayBuffer"));
    };
    let Some(buf) = buffer_of(o) else {
        return Err(vm.type_error("Receiver is not a SharedArrayBuffer"));
    };
    let bytes = buf.borrow().clone();
    let len = bytes.len();
    let s = vm.rel_index(&a.arg(0), len, 0)?;
    let e = vm.rel_index(&a.arg(1), len, len)?;
    let out = if s < e { bytes[s..e].to_vec() } else { vec![] };
    let proto = match vm.global.own_value("%SharedArrayBufferProto") {
        Some(Value::Obj(p)) => p,
        _ => vm.intr.arraybuffer_proto.clone(),
    };
    let copy = vm.obj_with(Some(proto), Kind::ArrayBuffer(Rc::new(RefCell::new(out))));
    copy.set_hidden("%shared", Value::Bool(true));
    Ok(Value::Obj(copy))
}

/// Where an atomic operation reads and writes: the backing store, the byte the
/// element starts at, and how it is stored.
struct Cell {
    buf: Rc<RefCell<Vec<u8>>>,
    at: usize,
    size: usize,
    signed: bool,
    shared: bool,
}

fn integer_kind(k: TypedKind) -> Option<bool> {
    match k {
        TypedKind::Int8 | TypedKind::Int16 | TypedKind::Int32 => Some(true),
        TypedKind::Uint8 | TypedKind::Uint16 | TypedKind::Uint32 => Some(false),
        _ => None,
    }
}

/// How V8 names the value in its complaints.
fn tag_of(vm: &mut Vm, v: &Value) -> String {
    match v {
        Value::Obj(o) => {
            let d = o.borrow();
            match &d.kind {
                Kind::TypedArray { kind, .. } => {
                    format!("[object {}]", crate::builtins::typed::kind_name(*kind))
                }
                Kind::Array(_) => "[object Array]".to_string(),
                Kind::ArrayBuffer(_) => "[object ArrayBuffer]".to_string(),
                _ => "#<Object>".to_string(),
            }
        }
        _ => vm.to_str(v).unwrap_or_default(),
    }
}

fn cell(vm: &mut Vm, a: &mut Args) -> JsResult<Cell> {
    let Value::Obj(o) = a.arg(0) else {
        let tag = tag_of(vm, &a.arg(0));
        return Err(vm.type_error(format!("{tag} is not an integer typed array.")));
    };
    let parts = match &o.borrow().kind {
        Kind::TypedArray {
            kind,
            buf,
            offset,
            len,
            buf_obj,
        } => Some((*kind, buf.clone(), *offset, *len, buf_obj.clone())),
        _ => None,
    };
    let Some((kind, buf, offset, len, buf_obj)) = parts else {
        let tag = tag_of(vm, &Value::Obj(o));
        return Err(vm.type_error(format!("{tag} is not an integer typed array.")));
    };
    let Some(signed) = integer_kind(kind) else {
        return Err(vm.type_error(format!(
            "[object {}] is not an integer typed array.",
            crate::builtins::typed::kind_name(kind)
        )));
    };
    let i = vm.to_integer(&a.arg(1))?;
    if !(0.0..len as f64).contains(&i) {
        return Err(vm.range_error("Invalid atomic access index"));
    }
    let size = elem_size(kind);
    let shared = buf_obj
        .as_ref()
        .map(|b| b.own_value("%shared").is_some())
        .unwrap_or(false);
    Ok(Cell {
        buf,
        at: offset + i as usize * size,
        size,
        signed,
        shared,
    })
}

impl Cell {
    fn load_raw(&self) -> u64 {
        let b = self.buf.borrow();
        let mut v: u64 = 0;
        for i in (0..self.size).rev() {
            v = (v << 8) | *b.get(self.at + i).unwrap_or(&0) as u64;
        }
        v
    }

    fn store_raw(&self, v: u64) {
        let mut b = self.buf.borrow_mut();
        for i in 0..self.size {
            if let Some(slot) = b.get_mut(self.at + i) {
                *slot = (v >> (8 * i)) as u8;
            }
        }
    }

    /// The stored bits as the number JavaScript sees.
    fn number(&self, raw: u64) -> Value {
        let bits = self.size * 8;
        if self.signed && bits < 64 && raw & (1 << (bits - 1)) != 0 {
            Value::Num(raw as f64 - (1u64 << bits) as f64)
        } else {
            Value::Num(raw as f64)
        }
    }

    /// A number as the bits this element would hold.
    fn bits(&self, x: f64) -> u64 {
        let bits = self.size * 8;
        let m = 1i128 << bits;
        let mut n = (x as i128) % m;
        if n < 0 {
            n += m;
        }
        n as u64
    }

    fn mask(&self, v: u64) -> u64 {
        let bits = self.size * 8;
        if bits >= 64 {
            v
        } else {
            v & ((1u64 << bits) - 1)
        }
    }
}

/// The read-modify-write operations, which all return the value that was there.
fn rmw(vm: &mut Vm, a: &mut Args, op: fn(u64, u64) -> u64) -> JsResult<Value> {
    let c = cell(vm, a)?;
    let x = vm.to_integer(&a.arg(2))?;
    let operand = c.bits(x);
    let old = c.load_raw();
    c.store_raw(c.mask(op(old, operand)));
    Ok(c.number(old))
}

fn a_add(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    rmw(vm, a, |o, v| o.wrapping_add(v))
}
fn a_sub(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    rmw(vm, a, |o, v| o.wrapping_sub(v))
}
fn a_and(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    rmw(vm, a, |o, v| o & v)
}
fn a_or(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    rmw(vm, a, |o, v| o | v)
}
fn a_xor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    rmw(vm, a, |o, v| o ^ v)
}
fn a_exchange(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    rmw(vm, a, |_, v| v)
}

fn a_load(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let c = cell(vm, a)?;
    let raw = c.load_raw();
    Ok(c.number(raw))
}

fn a_store(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let c = cell(vm, a)?;
    let x = vm.to_integer(&a.arg(2))?;
    c.store_raw(c.bits(x));
    // `store` gives back what was stored, as an integer, not what it became.
    Ok(Value::Num(if x == 0.0 { 0.0 } else { x }))
}

fn a_compare_exchange(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let c = cell(vm, a)?;
    let expected = vm.to_integer(&a.arg(2))?;
    let replacement = vm.to_integer(&a.arg(3))?;
    let old = c.load_raw();
    if old == c.bits(expected) {
        c.store_raw(c.bits(replacement));
    }
    Ok(c.number(old))
}

fn a_is_lock_free(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = vm.to_integer(&a.arg(0))?;
    Ok(Value::Bool(matches!(n as i64, 1 | 2 | 4 | 8)))
}

/// The checks `wait` and `waitAsync` share, and the values they were given.
fn wait_args(vm: &mut Vm, a: &mut Args) -> JsResult<(Cell, i64, f64)> {
    let c = cell(vm, a)?;
    if !c.shared {
        let tag = tag_of(vm, &a.arg(0));
        return Err(vm.type_error(format!("{tag} is not a shared typed array.")));
    }
    if c.size != 4 || !c.signed {
        let tag = tag_of(vm, &a.arg(0));
        return Err(vm.type_error(format!("{tag} is not an int32 or BigInt64 typed array.")));
    }
    let expected = vm.to_integer(&a.arg(2))?;
    let timeout = match a.arg(3) {
        Value::Undefined => f64::NAN,
        v => {
            let t = vm.to_number(&v)?;
            if t.is_nan() {
                f64::NAN
            } else {
                t.max(0.0)
            }
        }
    };
    let want = match c.number(c.bits(expected)) {
        Value::Num(n) => n as i64,
        _ => 0,
    };
    Ok((c, want, timeout))
}

fn a_wait(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (c, want, timeout) = wait_args(vm, a)?;
    let r = vm.atomics_wait(&c.buf, c.at, c.size, want, timeout)?;
    Ok(Value::str(r))
}

/// `Atomics.waitAsync`: `{ async: false, value }` when the answer is already
/// known, `{ async: true, value: Promise }` when it is not.
fn a_wait_async(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (c, want, timeout) = wait_args(vm, a)?;
    let now = {
        let raw = c.load_raw();
        match c.number(raw) {
            Value::Num(n) => n as i64,
            _ => 0,
        }
    };
    if now != want {
        let o = crate::builtins::new_obj_from(
            vm,
            vec![
                ("async", Value::Bool(false)),
                ("value", Value::str("not-equal")),
            ],
        );
        return Ok(Value::Obj(o));
    }
    if timeout == 0.0 {
        let o = crate::builtins::new_obj_from(
            vm,
            vec![
                ("async", Value::Bool(false)),
                ("value", Value::str("timed-out")),
            ],
        );
        return Ok(Value::Obj(o));
    }
    let p = vm.atomics_wait_async(&c.buf, c.at, c.size, want, timeout);
    let o = crate::builtins::new_obj_from(
        vm,
        vec![("async", Value::Bool(true)), ("value", Value::Obj(p))],
    );
    Ok(Value::Obj(o))
}

fn a_pause(_vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Undefined)
}

fn a_notify(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let c = cell(vm, a)?;
    let count = match a.arg(2) {
        Value::Undefined => f64::INFINITY,
        v => vm.to_integer(&v)?.max(0.0),
    };
    let woken = vm.atomics_notify(&c.buf, c.at, count);
    Ok(Value::Num(woken as f64))
}

pub fn install(vm: &mut Vm) {
    let tag = vm.syms.to_string_tag.clone();
    // SharedArrayBuffer: its own prototype over the object prototype, so a
    // shared buffer is not an ArrayBuffer, as in Node.
    let proto = vm.new_object();
    let ctor = vm.make_ctor("SharedArrayBuffer", 1, sab_ctor, &proto);
    vm.getter(&proto, "byteLength", sab_byte_length);
    vm.method(&proto, "slice", 2, sab_slice);
    proto.set_sym(&tag, Value::str("SharedArrayBuffer"), CONFIGURABLE);
    vm.global
        .set_hidden("%SharedArrayBufferProto", Value::Obj(proto));
    vm.set_global("SharedArrayBuffer", Value::Obj(ctor));
    // Atomics
    let atomics = vm.new_object();
    let fns: &[(&str, u32, NativeFn)] = &[
        ("add", 3, a_add),
        ("and", 3, a_and),
        ("compareExchange", 4, a_compare_exchange),
        ("exchange", 3, a_exchange),
        ("isLockFree", 1, a_is_lock_free),
        ("load", 2, a_load),
        ("or", 3, a_or),
        ("store", 3, a_store),
        ("sub", 3, a_sub),
        ("xor", 3, a_xor),
        ("pause", 0, a_pause),
        ("wait", 4, a_wait),
        ("waitAsync", 4, a_wait_async),
        ("notify", 3, a_notify),
    ];
    for (n, l, f) in fns {
        vm.method(&atomics, n, *l, *f);
    }
    atomics.set_sym(&tag, Value::str("Atomics"), CONFIGURABLE);
    vm.set_global("Atomics", Value::Obj(atomics));
}
