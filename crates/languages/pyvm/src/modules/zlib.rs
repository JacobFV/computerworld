//! `_zlib`: the native half of `zlib` (whose Python half, `lib/zlib.py`, defines
//! the `Compress` / `Decompress` classes). Compression is `cw-zlib`'s port of the
//! library CPython links, driven with CPython's own buffer strategy, so outputs
//! match `zlib.compress` byte for byte.
use super::{new_module, set_fn, set_val};
use crate::builtins::*;
use crate::value::*;
use crate::vm::*;
use cw_zlib::{Deflater, Flush, HashVariant, Inflater, OutputSchedule, Progress, ZError};
use std::rc::Rc;

fn bytes_arg(vm: &Vm, v: &Value, what: &str) -> PyResult<Vec<u8>> {
    match vm.base_value(v) {
        Value::Bytes(b) => Ok((*b).clone()),
        Value::ByteArray(b) => Ok(b.borrow().clone()),
        other => Err(type_err(format!(
            "{what}a bytes-like object is required, not '{}'",
            vm.type_name(&other)
        ))),
    }
}
fn int(vm: &mut Vm, a: &Args, i: usize, default: i64) -> PyResult<i64> {
    match a.args.get(i) {
        None | Some(Value::None) => Ok(default),
        Some(v) => to_int_arg(vm, v),
    }
}
fn bytes(b: Vec<u8>) -> Value {
    Value::Bytes(Rc::new(b))
}

/// Raises `zlib.error(msg)`.
fn zerror(vm: &mut Vm, msg: String) -> Box<PyErr> {
    let cls = vm.modules.borrow().get_str("_zlib").and_then(|m| match m {
        Value::Module(m) => m.dict.borrow().get_str("error"),
        _ => None,
    });
    match cls {
        Some(Value::Class(c)) => {
            let v = vm.new_exception(&c, vec![Value::string(msg)]);
            PyErr::from_exc(v, false)
        }
        _ => err("RuntimeError", msg),
    }
}
fn describe(e: &ZError, doing: &str) -> String {
    match e {
        ZError::Data(m) => format!(
            "Error -3 while {doing}: {}",
            if m.is_empty() {
                "invalid input data"
            } else {
                m
            }
        ),
        ZError::Buf => format!("Error -5 while {doing}: incomplete or truncated stream"),
        ZError::Stream => format!("Error -2 while {doing}: inconsistent stream state"),
        ZError::NeedDict(_) => format!("Error 2 while {doing}"),
    }
}

fn boxed(vm: &Vm, data: Box<dyn std::any::Any>) -> Value {
    Value::Native(Rc::new(Native {
        class: vm.t.object.clone(),
        data: std::cell::RefCell::new(NativeKind::Boxed(data)),
    }))
}
fn with<T: 'static, R>(v: &Value, f: impl FnOnce(&mut T) -> R) -> PyResult<R> {
    if let Value::Native(n) = v {
        if let NativeKind::Boxed(b) = &mut *n.data.borrow_mut() {
            if let Some(t) = b.downcast_mut::<T>() {
                return Ok(f(t));
            }
        }
    }
    Err(type_err("invalid zlib object"))
}

struct Decomp {
    inf: Inflater,
    zdict: Option<Vec<u8>>,
    raw: bool,
}
/// What one `decompress` call produced: output, end-of-stream, the data after the
/// stream, and the input left over when an output limit stopped it.
type Decompressed = (Vec<u8>, bool, Vec<u8>, Vec<u8>);

pub fn make(vm: &mut Vm) -> Value {
    let m = new_module("_zlib");
    let exc = new_class("error", vec![vm.t.exc("Exception")], Kind::Object, false);
    exc.dict
        .borrow_mut()
        .set_str("__module__", Value::str("zlib"));
    set_val(&m, "error", Value::Class(exc));
    // compress(data, level, wbits)
    set_fn(&m, "compress", |vm, a| {
        let data = bytes_arg(vm, &a.args[0], "")?;
        let level = int(vm, &a, 1, -1)?;
        let wbits = int(vm, &a, 2, 15)?;
        if !(-1..=9).contains(&level) {
            return Err(zerror(vm, "Bad compression level".into()));
        }
        match cw_zlib::compress(
            &data,
            level as i32,
            wbits as i32,
            8,
            0,
            HashVariant::Canonical,
            &OutputSchedule::cpython(),
        ) {
            Ok(o) => Ok(bytes(o)),
            Err(ZError::Stream) => Err(zerror(vm, "Bad compression level".into())),
            Err(e) => Err(zerror(vm, describe(&e, "compressing data"))),
        }
    });
    // decompress(data, wbits, bufsize)
    set_fn(&m, "decompress", |vm, a| {
        let data = bytes_arg(vm, &a.args[0], "")?;
        let wbits = int(vm, &a, 1, 15)?;
        let mut inf = match Inflater::new(wbits as i32) {
            Ok(i) => i,
            Err(e) => return Err(zerror(vm, describe(&e, "preparing to decompress data"))),
        };
        let mut out = vec![];
        match inf.inflate(&data, &mut out, usize::MAX) {
            Ok(Progress::End) => Ok(bytes(out)),
            Ok(Progress::NeedDict(_)) => Err(zerror(vm, "Error 2 while decompressing data".into())),
            Ok(_) => Err(zerror(vm, describe(&ZError::Buf, "decompressing data"))),
            Err(e) => Err(zerror(vm, describe(&e, "decompressing data"))),
        }
    });
    // compressobj(level, method, wbits, memLevel, strategy, zdict) -> handle
    set_fn(&m, "compressobj", |vm, a| {
        let level = int(vm, &a, 0, -1)?;
        let method = int(vm, &a, 1, 8)?;
        let wbits = int(vm, &a, 2, 15)?;
        let mem = int(vm, &a, 3, 8)?;
        let strategy = int(vm, &a, 4, 0)?;
        if method != 8 {
            return Err(value_err("Invalid initialization option"));
        }
        let mut d = match Deflater::new(
            level as i32,
            wbits as i32,
            mem as i32,
            strategy as i32,
            HashVariant::Canonical,
        ) {
            Ok(d) => d,
            Err(_) => return Err(value_err("Invalid initialization option")),
        };
        if let Some(z) = a.args.get(5).filter(|v| !v.is_none()) {
            let dict = bytes_arg(vm, z, "")?;
            if d.set_dictionary(&dict).is_err() {
                return Err(value_err("Invalid dictionary"));
            }
        }
        Ok(boxed(vm, Box::new(d)))
    });
    // c_compress(handle, data) / c_flush(handle, mode)
    set_fn(&m, "c_compress", |vm, a| {
        let data = bytes_arg(vm, &a.args[1], "")?;
        let r = with(&a.args[0], |d: &mut Deflater| {
            let mut call = 0;
            cw_zlib::deflate_all(d, &data, Flush::None, &OutputSchedule::cpython(), &mut call)
        })?;
        r.map(bytes)
            .map_err(|e| zerror(vm, describe(&e, "compressing data")))
    });
    set_fn(&m, "c_flush", |vm, a| {
        let mode = int(vm, &a, 1, 4)?;
        if mode == 0 {
            return Ok(bytes(vec![]));
        }
        let flush = Flush::from_i32(mode as i32).ok_or_else(|| value_err("invalid flush mode"))?;
        let r = with(&a.args[0], |d: &mut Deflater| {
            let mut call = 0;
            cw_zlib::deflate_all(d, &[], flush, &OutputSchedule::cpython(), &mut call)
        })?;
        r.map(bytes)
            .map_err(|e| zerror(vm, describe(&e, "flushing")))
    });
    set_fn(&m, "c_copy", |vm, a| {
        let d = with(&a.args[0], |d: &mut Deflater| d.clone())?;
        Ok(boxed(vm, Box::new(d)))
    });
    // decompressobj(wbits, zdict) -> handle
    set_fn(&m, "decompressobj", |vm, a| {
        let wbits = int(vm, &a, 0, 15)?;
        let inf =
            Inflater::new(wbits as i32).map_err(|_| value_err("Invalid initialization option"))?;
        let zdict = match a.args.get(1).filter(|v| !v.is_none()) {
            Some(z) => Some(bytes_arg(vm, z, "")?),
            None => None,
        };
        let mut d = Decomp {
            inf,
            zdict,
            raw: wbits < 0,
        };
        if d.raw {
            if let Some(z) = d.zdict.clone() {
                let _ = d.inf.set_dictionary(&z);
            }
        }
        Ok(boxed(vm, Box::new(d)))
    });
    // d_decompress(handle, data, max_length) -> (out, eof, unused_data, unconsumed_tail)
    set_fn(&m, "d_decompress", |vm, a| {
        let data = bytes_arg(vm, &a.args[1], "")?;
        let max = int(vm, &a, 2, 0)?;
        let limit = if max <= 0 { usize::MAX } else { max as usize };
        let r = with(
            &a.args[0],
            |d: &mut Decomp| -> Result<Decompressed, ZError> {
                let mut out = vec![];
                let mut p = d.inf.inflate(&data, &mut out, limit)?;
                if let Progress::NeedDict(_) = p {
                    match d.zdict.clone() {
                        Some(z) => {
                            d.inf.set_dictionary(&z)?;
                            let room = limit - out.len().min(limit);
                            p = d.inf.inflate(&[], &mut out, room)?;
                        }
                        None => return Err(ZError::NeedDict(0)),
                    }
                }
                Ok(match p {
                    Progress::End => (out, true, d.inf.take_unconsumed(), vec![]),
                    Progress::OutputFull => {
                        let tail = d.inf.take_unconsumed();
                        (out, false, vec![], tail)
                    }
                    _ => (out, false, vec![], vec![]),
                })
            },
        )?;
        match r {
            Ok((out, eof, unused, tail)) => Ok(Value::tuple(vec![
                bytes(out),
                Value::Bool(eof),
                bytes(unused),
                bytes(tail),
            ])),
            Err(e) => Err(zerror(vm, describe(&e, "decompressing data"))),
        }
    });
    set_fn(&m, "d_copy", |vm, a| {
        let d = with(&a.args[0], |d: &mut Decomp| Decomp {
            inf: d.inf.clone(),
            zdict: d.zdict.clone(),
            raw: d.raw,
        })?;
        Ok(boxed(vm, Box::new(d)))
    });
    set_fn(&m, "crc32", |vm, a| {
        let data = bytes_arg(vm, &a.args[0], "")?;
        let start = int(vm, &a, 1, 0)?;
        Ok(Value::Int(cw_zlib::crc32(start as u32, &data) as i64))
    });
    set_fn(&m, "adler32", |vm, a| {
        let data = bytes_arg(vm, &a.args[0], "")?;
        let start = int(vm, &a, 1, 1)?;
        Ok(Value::Int(cw_zlib::adler32(start as u32, &data) as i64))
    });
    Value::Module(m)
}
