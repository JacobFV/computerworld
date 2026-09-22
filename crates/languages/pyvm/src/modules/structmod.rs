//! `_struct`: packing values to C layouts (the native half of `struct`).
use super::{new_module, set_fn, set_val};
use crate::builtins::*;
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

#[derive(Clone, Copy)]
struct Item {
    code: char,
    count: usize,
}
struct Format {
    items: Vec<Item>,
    native: bool,
    big: bool,
}

fn serr(vm: &mut Vm, msg: impl Into<String>) -> Box<PyErr> {
    let cls = vm
        .modules
        .borrow()
        .get_str("_struct")
        .and_then(|m| match m {
            Value::Module(m) => m.dict.borrow().get_str("error"),
            _ => None,
        });
    match cls {
        Some(Value::Class(c)) => {
            let v = vm.new_exception(&c, vec![Value::string(msg.into())]);
            PyErr::from_exc(v, false)
        }
        _ => err("ValueError", msg.into()),
    }
}

fn parse(vm: &mut Vm, fmt: &str) -> PyResult<Format> {
    let mut chars = fmt.chars().peekable();
    let (native, big) = match chars.peek() {
        Some('@') => {
            chars.next();
            (true, false)
        }
        Some('=') | Some('<') => {
            chars.next();
            (false, false)
        }
        Some('>') | Some('!') => {
            chars.next();
            (false, true)
        }
        _ => (true, false),
    };
    let mut items = vec![];
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        let mut count = None;
        let mut c = c;
        if c.is_ascii_digit() {
            let mut n = c.to_digit(10).unwrap() as usize;
            loop {
                match chars.next() {
                    Some(d) if d.is_ascii_digit() => n = n * 10 + d.to_digit(10).unwrap() as usize,
                    Some(d) => {
                        c = d;
                        break;
                    }
                    None => return Err(serr(vm, "repeat count given without format specifier")),
                }
            }
            count = Some(n);
        }
        if !"xcbB?hHiIlLqQnNefdspP".contains(c) || (!native && "nNP".contains(c)) {
            return Err(serr(vm, "bad char in struct format"));
        }
        items.push(Item {
            code: c,
            count: count.unwrap_or(1),
        });
    }
    Ok(Format { items, native, big })
}

fn size_of(code: char, native: bool) -> usize {
    match code {
        'x' | 'c' | 'b' | 'B' | '?' | 's' | 'p' => 1,
        'h' | 'H' | 'e' => 2,
        'i' | 'I' | 'f' => 4,
        'l' | 'L' => {
            if native {
                8
            } else {
                4
            }
        }
        'q' | 'Q' | 'd' | 'n' | 'N' | 'P' => 8,
        _ => 0,
    }
}

fn calcsize(f: &Format) -> usize {
    let mut size: usize = 0;
    for it in &f.items {
        let s = size_of(it.code, f.native);
        if f.native && !"xcbB?sp".contains(it.code) && s > 1 {
            size = size.div_ceil(s) * s;
        }
        size += match it.code {
            's' | 'p' => it.count,
            _ => s * it.count,
        };
    }
    size
}

fn f16_bits(x: f64) -> u16 {
    let bits = (x as f32).to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mant != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = (mant | 0x80_0000) >> (1 - e);
        let round = if (m & 0x1fff) > 0x1000 || ((m & 0x1fff) == 0x1000 && (m & 0x2000) != 0) {
            1
        } else {
            0
        };
        return sign | ((m >> 13) as u16 + round);
    }
    let m = mant >> 13;
    let rest = mant & 0x1fff;
    let mut v = sign as u32 | ((e as u32) << 10) | m;
    if rest > 0x1000 || (rest == 0x1000 && (m & 1) != 0) {
        v += 1;
    }
    v as u16
}
fn f16_value(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f64;
    if e == 0 {
        sign * m * 2f64.powi(-24)
    } else if e == 0x1f {
        if m == 0.0 {
            sign * f64::INFINITY
        } else {
            f64::NAN
        }
    } else {
        sign * (1.0 + m / 1024.0) * 2f64.powi(e - 15)
    }
}

fn int_of(vm: &mut Vm, v: &Value) -> PyResult<i128> {
    match v {
        Value::Int(i) => Ok(*i as i128),
        Value::Bool(b) => Ok(*b as i128),
        Value::Big(b) => Ok(b.to_f64().unwrap_or(f64::MAX) as i128),
        Value::Instance(_) if vm.getattr_str(v, "__index__").is_ok() => {
            let r = vm.index_of(v)?;
            Ok(r as i128)
        }
        _ => Err(serr(vm, "required argument is not an integer")),
    }
}

fn pack_into(vm: &mut Vm, f: &Format, args: &[Value]) -> PyResult<Vec<u8>> {
    let needed: usize = f
        .items
        .iter()
        .map(|it| match it.code {
            'x' => 0,
            's' | 'p' => 1,
            _ => it.count,
        })
        .sum();
    if needed != args.len() {
        return Err(serr(
            vm,
            format!(
                "pack expected {needed} items for packing (got {})",
                args.len()
            ),
        ));
    }
    let mut out: Vec<u8> = vec![];
    let mut ai = 0;
    for it in &f.items {
        let s = size_of(it.code, f.native);
        if f.native && !"xcbB?sp".contains(it.code) && s > 1 {
            while !out.len().is_multiple_of(s) {
                out.push(0);
            }
        }
        if it.code == 'x' {
            out.extend(std::iter::repeat_n(0, it.count));
            continue;
        }
        if it.code == 's' || it.code == 'p' {
            let v = &args[ai];
            ai += 1;
            let b = match vm.base_value(v) {
                Value::Bytes(b) => (*b).clone(),
                Value::ByteArray(b) => b.borrow().clone(),
                _ => {
                    return Err(serr(
                        vm,
                        format!("argument for '{}' must be a bytes object", it.code),
                    ))
                }
            };
            let mut field = vec![0u8; it.count];
            if it.code == 's' {
                let n = b.len().min(it.count);
                field[..n].copy_from_slice(&b[..n]);
            } else if it.count > 0 {
                let n = b.len().min(it.count - 1).min(255);
                field[0] = n as u8;
                field[1..1 + n].copy_from_slice(&b[..n]);
            }
            out.extend(field);
            continue;
        }
        for _ in 0..it.count {
            let v = &args[ai];
            ai += 1;
            let bytes: Vec<u8> = match it.code {
                'c' => match vm.base_value(v) {
                    Value::Bytes(b) if b.len() == 1 => vec![b[0]],
                    Value::ByteArray(b) if b.borrow().len() == 1 => vec![b.borrow()[0]],
                    _ => return Err(serr(vm, "char format requires a bytes object of length 1")),
                },
                '?' => vec![vm.truthy(v)? as u8],
                'e' | 'f' | 'd' => {
                    let x = match v {
                        Value::Int(i) => *i as f64,
                        Value::Bool(b) => *b as i64 as f64,
                        _ => match crate::bfuncs::float_from(vm, v) {
                            Ok(x) => x,
                            Err(_) => return Err(serr(vm, "required argument is not a float")),
                        },
                    };
                    match it.code {
                        'e' => {
                            let h = f16_bits(x);
                            if h & 0x7c00 == 0x7c00 && x.is_finite() {
                                return Err(err(
                                    "OverflowError",
                                    "float too large to pack with e format",
                                ));
                            }
                            h.to_le_bytes().to_vec()
                        }
                        'f' => (x as f32).to_le_bytes().to_vec(),
                        _ => x.to_le_bytes().to_vec(),
                    }
                }
                code => {
                    let n = int_of(vm, v)?;
                    let (lo, hi): (i128, i128) = match code {
                        'b' => (-128, 127),
                        'B' => (0, 255),
                        'h' => (-32768, 32767),
                        'H' => (0, 65535),
                        'i' => (-(1 << 31), (1 << 31) - 1),
                        'I' => (0, (1 << 32) - 1),
                        'l' if s == 4 => (-(1 << 31), (1 << 31) - 1),
                        'L' if s == 4 => (0, (1 << 32) - 1),
                        'l' | 'q' | 'n' => (-(1 << 63), (1 << 63) - 1),
                        _ => (0, (1 << 64) - 1),
                    };
                    if n < lo || n > hi {
                        return Err(serr(
                            vm,
                            format!("'{code}' format requires {lo} <= number <= {hi}"),
                        ));
                    }
                    (n as u128 as u64).to_le_bytes()[..s].to_vec()
                }
            };
            let mut bytes = bytes;
            if f.big && bytes.len() > 1 {
                bytes.reverse();
            }
            out.extend(bytes);
        }
    }
    Ok(out)
}

fn unpack_from(vm: &mut Vm, f: &Format, data: &[u8]) -> PyResult<Vec<Value>> {
    let mut out = vec![];
    let mut pos: usize = 0;
    for it in &f.items {
        let s = size_of(it.code, f.native);
        if f.native && !"xcbB?sp".contains(it.code) && s > 1 {
            pos = pos.div_ceil(s) * s;
        }
        if it.code == 'x' {
            pos += it.count;
            continue;
        }
        if it.code == 's' {
            out.push(Value::Bytes(Rc::new(data[pos..pos + it.count].to_vec())));
            pos += it.count;
            continue;
        }
        if it.code == 'p' {
            let n = if it.count > 0 {
                (data[pos] as usize).min(it.count - 1)
            } else {
                0
            };
            out.push(Value::Bytes(Rc::new(data[pos + 1..pos + 1 + n].to_vec())));
            pos += it.count;
            continue;
        }
        for _ in 0..it.count {
            let mut raw = data[pos..pos + s].to_vec();
            pos += s;
            if f.big && raw.len() > 1 {
                raw.reverse();
            }
            let mut buf = [0u8; 8];
            buf[..s].copy_from_slice(&raw);
            let u = u64::from_le_bytes(buf);
            let v = match it.code {
                'c' => Value::Bytes(Rc::new(vec![raw[0]])),
                '?' => Value::Bool(raw[0] != 0),
                'e' => Value::Float(f16_value(u as u16)),
                'f' => Value::Float(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64),
                'd' => Value::Float(f64::from_le_bytes(buf)),
                'b' | 'h' | 'i' | 'l' | 'q' | 'n' => {
                    let shift = 64 - 8 * s as u32;
                    Value::Int(((u << shift) as i64) >> shift)
                }
                _ => {
                    if u > i64::MAX as u64 {
                        Value::Big(Rc::new(crate::bigint::BigInt::from_u64(u)))
                    } else {
                        Value::Int(u as i64)
                    }
                }
            };
            out.push(v);
        }
    }
    let _ = vm;
    Ok(out)
}

fn fmt_arg(vm: &mut Vm, v: &Value) -> PyResult<String> {
    match vm.base_value(v) {
        Value::Str(s) => Ok(s.s.clone()),
        Value::Bytes(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
        other => Err(type_err(format!(
            "Struct() argument 1 must be a str or bytes object, not {}",
            vm.type_name(&other)
        ))),
    }
}
fn buf_arg(vm: &mut Vm, v: &Value) -> PyResult<Vec<u8>> {
    match vm.base_value(v) {
        Value::Bytes(b) => Ok((*b).clone()),
        Value::ByteArray(b) => Ok(b.borrow().clone()),
        other => Err(type_err(format!(
            "a bytes-like object is required, not '{}'",
            vm.type_name(&other)
        ))),
    }
}

pub fn make(vm: &mut Vm) -> Value {
    let m = new_module("_struct");
    let exc = new_class("error", vec![vm.t.exc("Exception")], Kind::Object, false);
    exc.dict
        .borrow_mut()
        .set_str("__module__", Value::str("struct"));
    set_val(&m, "error", Value::Class(exc));
    set_fn(&m, "calcsize", |vm, a| {
        let fs = fmt_arg(vm, &a.args[0])?;
        let f = parse(vm, &fs)?;
        Ok(Value::Int(calcsize(&f) as i64))
    });
    set_fn(&m, "pack", |vm, a| {
        let fs = fmt_arg(vm, &a.args[0])?;
        let f = parse(vm, &fs)?;
        let out = pack_into(vm, &f, &a.args[1..])?;
        Ok(Value::Bytes(Rc::new(out)))
    });
    set_fn(&m, "unpack_from", |vm, a| {
        let fs = fmt_arg(vm, &a.args[0])?;
        let f = parse(vm, &fs)?;
        let data = buf_arg(vm, &a.args[1])?;
        let offset = match a.args.get(2) {
            Some(v) => to_int_arg(vm, v)?,
            None => 0,
        };
        let exact = matches!(a.args.get(3), Some(Value::Bool(true)));
        let size = calcsize(&f);
        let off = if offset < 0 {
            (data.len() as i64 + offset).max(0) as usize
        } else {
            offset as usize
        };
        if exact && data.len() != size {
            return Err(serr(
                vm,
                format!("unpack requires a buffer of {size} bytes"),
            ));
        }
        if !exact && data.len() < off + size {
            return Err(serr(
                vm,
                format!(
                    "unpack_from requires a buffer of at least {} bytes for unpacking {size} bytes at offset {off} (actual buffer size is {})",
                    off + size,
                    data.len()
                ),
            ));
        }
        let vals = unpack_from(vm, &f, &data[off..])?;
        Ok(Value::tuple(vals))
    });
    Value::Module(m)
}
