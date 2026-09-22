//! `Number`, `Boolean`, `BigInt` and locale number formatting.

use super::*;
use crate::bigint::BigInt;
use crate::numconv::*;
use crate::value::*;
use crate::vm::Vm;
use std::rc::Rc;

fn this_num(vm: &mut Vm, a: &Args, method: &str) -> JsResult<f64> {
    match &a.this {
        Value::Num(n) => Ok(*n),
        Value::Obj(o) => match &o.borrow().kind {
            Kind::Number(n) => Ok(*n),
            _ => Err(vm.type_error(format!(
                "Number.prototype.{method} requires that 'this' be a Number"
            ))),
        },
        _ => Err(vm.type_error(format!(
            "Number.prototype.{method} requires that 'this' be a Number"
        ))),
    }
}

fn number_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = if a.args.is_empty() {
        0.0
    } else {
        match vm.to_numeric(&a.args[0])? {
            Value::BigInt(b) => b.to_f64().unwrap_or(f64::INFINITY),
            Value::Num(n) => n,
            _ => f64::NAN,
        }
    };
    match &a.new_target {
        None => Ok(Value::Num(n)),
        Some(nt) => {
            let np = vm.intr.number_proto.clone();
            let proto = vm.proto_from_ctor(nt, &np)?;
            Ok(Value::Obj(vm.obj_with(Some(proto), Kind::Number(n))))
        }
    }
}

fn to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_num(vm, a, "toString")?;
    let r = if a.arg(0).is_undefined() {
        10.0
    } else {
        vm.to_integer(&a.arg(0))?
    };
    if !(2.0..=36.0).contains(&r) {
        return Err(vm.range_error("toString() radix must be between 2 and 36"));
    }
    Ok(Value::string(if r == 10.0 {
        number_to_string(n)
    } else {
        to_radix(n, r as u32)
    }))
}

fn to_fixed_m(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_num(vm, a, "toFixed")?;
    let f = vm.to_integer(&a.arg(0))?;
    if !(0.0..=100.0).contains(&f) {
        return Err(vm.range_error("toFixed() digits argument must be between 0 and 100"));
    }
    Ok(Value::string(to_fixed(n, f as usize)))
}

fn to_precision_m(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_num(vm, a, "toPrecision")?;
    if a.arg(0).is_undefined() {
        return Ok(Value::string(number_to_string(n)));
    }
    let p = vm.to_integer(&a.arg(0))?;
    if !n.is_finite() {
        return Ok(Value::string(number_to_string(n)));
    }
    if !(1.0..=100.0).contains(&p) {
        return Err(vm.range_error("toPrecision() argument must be between 1 and 100"));
    }
    Ok(Value::string(to_precision(n, p as usize)))
}

fn to_exponential_m(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_num(vm, a, "toExponential")?;
    let f = if a.arg(0).is_undefined() {
        None
    } else {
        Some(vm.to_integer(&a.arg(0))?)
    };
    if !n.is_finite() {
        return Ok(Value::string(number_to_string(n)));
    }
    if let Some(f) = f {
        if !(0.0..=100.0).contains(&f) {
            return Err(vm.range_error("toExponential() argument must be between 0 and 100"));
        }
    }
    Ok(Value::string(to_exponential(n, f.map(|x| x as usize))))
}

fn value_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Num(this_num(vm, a, "valueOf")?))
}

/// en-US grouping + fraction digits (halfExpand rounding).
pub fn format_locale(x: f64, min_frac: usize, max_frac: usize, grouping: bool) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "∞".into() } else { "-∞".into() };
    }
    let neg = x < 0.0;
    let s = if x.abs() >= 1e21 {
        format!("{:.*}", max_frac, x.abs())
    } else {
        to_fixed(x.abs(), max_frac)
    };
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (s.clone(), String::new()),
    };
    let mut frac = frac;
    while frac.len() > min_frac && frac.ends_with('0') {
        frac.pop();
    }
    let int = if grouping { group3(&int) } else { int };
    let mut out = String::new();
    let is_zero = int.chars().all(|c| c == '0' || c == ',') && frac.chars().all(|c| c == '0');
    if neg && !is_zero {
        out.push('-');
    }
    out.push_str(&int);
    if !frac.is_empty() {
        out.push('.');
        out.push_str(&frac);
    }
    out
}

pub fn group3(int: &str) -> String {
    let b = int.as_bytes();
    let mut out = String::new();
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*c as char);
    }
    out
}

/// Formats per Intl.NumberFormat-style options (subset).
pub fn format_with_options(vm: &mut Vm, x: f64, opts: &Value) -> JsResult<String> {
    let mut style = "decimal".to_string();
    let mut currency = "USD".to_string();
    let mut min_frac: Option<usize> = None;
    let mut max_frac: Option<usize> = None;
    let mut grouping = true;
    let mut max_sig: Option<usize> = None;
    if let Value::Obj(_) = opts {
        let s = vm.get_str(opts, "style")?;
        if !s.is_undefined() {
            style = vm.to_str(&s)?;
        }
        let c = vm.get_str(opts, "currency")?;
        if !c.is_undefined() {
            currency = vm.to_str(&c)?.to_uppercase();
        }
        let v = vm.get_str(opts, "minimumFractionDigits")?;
        if !v.is_undefined() {
            let n = vm.to_integer(&v)?;
            if !(0.0..=100.0).contains(&n) {
                return Err(vm.range_error("minimumFractionDigits value is out of range."));
            }
            min_frac = Some(n as usize);
        }
        let v = vm.get_str(opts, "maximumFractionDigits")?;
        if !v.is_undefined() {
            let n = vm.to_integer(&v)?;
            if !(0.0..=100.0).contains(&n) {
                return Err(vm.range_error("maximumFractionDigits value is out of range."));
            }
            max_frac = Some(n as usize);
        }
        let v = vm.get_str(opts, "maximumSignificantDigits")?;
        if !v.is_undefined() {
            max_sig = Some(vm.to_integer(&v)? as usize);
        }
        let g = vm.get_str(opts, "useGrouping")?;
        if !g.is_undefined() {
            grouping = g.truthy();
        }
    }
    let (dmin, dmax) = match style.as_str() {
        "currency" => (2, 2),
        "percent" => (0, 0),
        _ => (0, 3),
    };
    let mut minf = min_frac.unwrap_or(dmin);
    let mut maxf = max_frac.unwrap_or(dmax.max(minf));
    if minf > maxf {
        if min_frac.is_some() && max_frac.is_none() {
            maxf = minf;
        } else {
            minf = maxf;
        }
    }
    let val = if style == "percent" { x * 100.0 } else { x };
    let body = if let Some(sig) = max_sig {
        let p = to_precision(val.abs(), sig.clamp(1, 21));
        let v: f64 = p.parse().unwrap_or(val.abs());
        let s = format_locale(v, 0, 20, grouping);
        if val < 0.0 {
            format!("-{s}")
        } else {
            s
        }
    } else {
        format_locale(val, minf, maxf, grouping)
    };
    Ok(match style.as_str() {
        "currency" => {
            let sym = match currency.as_str() {
                "USD" => "$",
                "EUR" => "€",
                "GBP" => "£",
                "JPY" => "¥",
                "INR" => "₹",
                "CNY" => "CN¥",
                "CAD" => "CA$",
                "AUD" => "A$",
                _ => "",
            };
            let (neg, digits) = match body.strip_prefix('-') {
                Some(d) => (true, d.to_string()),
                None => (false, body.clone()),
            };
            let cur = if sym.is_empty() {
                format!("{currency}\u{a0}")
            } else {
                sym.to_string()
            };
            format!("{}{cur}{digits}", if neg { "-" } else { "" })
        }
        "percent" => format!("{body}%"),
        _ => body,
    })
}

fn to_locale_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_num(vm, a, "toLocaleString")?;
    let s = format_with_options(vm, n, &a.arg(1))?;
    Ok(Value::string(s))
}

fn is_integer(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(
        matches!(a.arg(0), Value::Num(n) if n.is_finite() && n.fract() == 0.0),
    ))
}
fn is_safe_integer(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(
        matches!(a.arg(0), Value::Num(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0),
    ))
}
fn is_finite_n(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(
        matches!(a.arg(0), Value::Num(n) if n.is_finite()),
    ))
}
fn is_nan_n(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(matches!(a.arg(0), Value::Num(n) if n.is_nan())))
}

pub fn parse_float_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    Ok(Value::Num(parse_float(&s)))
}

pub fn parse_int_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    let r = vm.to_i32(&a.arg(1))?;
    Ok(Value::Num(parse_int(&s, r)))
}

// ---------------------------------------------------------------- Boolean

fn boolean_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let b = a.arg(0).truthy();
    match &a.new_target {
        None => Ok(Value::Bool(b)),
        Some(nt) => {
            let bp = vm.intr.boolean_proto.clone();
            let proto = vm.proto_from_ctor(nt, &bp)?;
            Ok(Value::Obj(vm.obj_with(Some(proto), Kind::Boolean(b))))
        }
    }
}

fn this_bool(vm: &mut Vm, a: &Args) -> JsResult<bool> {
    match &a.this {
        Value::Bool(b) => Ok(*b),
        Value::Obj(o) => match &o.borrow().kind {
            Kind::Boolean(b) => Ok(*b),
            _ => Err(vm.type_error("Boolean.prototype.valueOf requires that 'this' be a Boolean")),
        },
        _ => Err(vm.type_error("Boolean.prototype.valueOf requires that 'this' be a Boolean")),
    }
}

fn bool_to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::str(if this_bool(vm, a)? { "true" } else { "false" }))
}
fn bool_value_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(this_bool(vm, a)?))
}

// ---------------------------------------------------------------- BigInt

pub fn to_bigint(vm: &mut Vm, v: &Value) -> JsResult<BigInt> {
    let p = vm.to_primitive(v, crate::conv::Hint::Number)?;
    match &p {
        Value::BigInt(b) => Ok((**b).clone()),
        Value::Bool(b) => Ok(BigInt::from_i64(*b as i64)),
        Value::Str(s) => match crate::conv::parse_bigint_str(s) {
            Some(b) => Ok(b),
            None => Err(vm.syntax_error(format!("Cannot convert {} to a BigInt", s.as_str()))),
        },
        Value::Num(n) => {
            let d = number_to_string(*n);
            Err(vm.type_error(format!("Cannot convert {d} to a BigInt")))
        }
        Value::Undefined => Err(vm.type_error("Cannot convert undefined to a BigInt")),
        Value::Null => Err(vm.type_error("Cannot convert null to a BigInt")),
        _ => Err(vm.type_error("Cannot convert value to a BigInt")),
    }
}

fn bigint_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if a.new_target.is_some() {
        return Err(vm.type_error("BigInt is not a constructor"));
    }
    let v = a.arg(0);
    let p = vm.to_primitive(&v, crate::conv::Hint::Number)?;
    if let Value::Num(n) = p {
        if !n.is_finite() || n.fract() != 0.0 {
            let d = number_to_string(n);
            return Err(vm.range_error(format!(
                "The number {d} cannot be converted to a BigInt because it is not an integer"
            )));
        }
        return Ok(Value::BigInt(Rc::new(BigInt::from_f64(n))));
    }
    Ok(Value::BigInt(Rc::new(to_bigint(vm, &p)?)))
}

fn this_big(vm: &mut Vm, a: &Args) -> JsResult<Rc<BigInt>> {
    match &a.this {
        Value::BigInt(b) => Ok(b.clone()),
        Value::Obj(o) => match &o.borrow().kind {
            Kind::BigInt(b) => Ok(b.clone()),
            _ => Err(vm.type_error("BigInt.prototype.valueOf requires that 'this' be a BigInt")),
        },
        _ => Err(vm.type_error("BigInt.prototype.valueOf requires that 'this' be a BigInt")),
    }
}

fn big_to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let b = this_big(vm, a)?;
    let r = if a.arg(0).is_undefined() {
        10.0
    } else {
        vm.to_integer(&a.arg(0))?
    };
    if !(2.0..=36.0).contains(&r) {
        return Err(vm.range_error("toString() radix must be between 2 and 36"));
    }
    Ok(Value::string(b.to_str_radix(r as u32)))
}

fn big_to_locale_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let b = this_big(vm, a)?;
    let s = b.to_str_radix(10);
    let (neg, d) = match s.strip_prefix('-') {
        Some(d) => (true, d.to_string()),
        None => (false, s),
    };
    Ok(Value::string(format!(
        "{}{}",
        if neg { "-" } else { "" },
        group3(&d)
    )))
}

fn big_value_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::BigInt(this_big(vm, a)?))
}

fn as_int_n(vm: &mut Vm, a: &mut Args, signed: bool) -> JsResult<Value> {
    let bits = vm.to_integer(&a.arg(0))?.max(0.0) as u64;
    let b = to_bigint(vm, &a.arg(1))?;
    let m = BigInt::from_i64(1).shl(bits);
    let mut r = b.divmod_floor(&m).1;
    if signed && bits > 0 {
        let half = BigInt::from_i64(1).shl(bits - 1);
        if r.cmp(&half) != std::cmp::Ordering::Less {
            r = r.sub(&m);
        }
    }
    Ok(Value::BigInt(Rc::new(r)))
}

fn as_int_n_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    as_int_n(vm, a, true)
}
fn as_uint_n_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    as_int_n(vm, a, false)
}

pub fn install(vm: &mut Vm) {
    let proto = vm.intr.number_proto.clone();
    let ctor = vm.make_ctor("Number", 1, number_ctor, &proto);
    vm.set_global("Number", Value::Obj(ctor.clone()));
    vm.method(&proto, "toString", 1, to_string);
    vm.method(&proto, "toFixed", 1, to_fixed_m);
    vm.method(&proto, "toPrecision", 1, to_precision_m);
    vm.method(&proto, "toExponential", 1, to_exponential_m);
    vm.method(&proto, "valueOf", 0, value_of);
    vm.method(&proto, "toLocaleString", 0, to_locale_string);
    vm.method(&ctor, "isInteger", 1, is_integer);
    vm.method(&ctor, "isSafeInteger", 1, is_safe_integer);
    vm.method(&ctor, "isFinite", 1, is_finite_n);
    vm.method(&ctor, "isNaN", 1, is_nan_n);
    let pf = vm.method(&ctor, "parseFloat", 1, parse_float_fn);
    let pi = vm.method(&ctor, "parseInt", 2, parse_int_fn);
    vm.set_global("parseFloat", Value::Obj(pf));
    vm.set_global("parseInt", Value::Obj(pi));
    for (n, v) in [
        ("EPSILON", f64::EPSILON),
        ("MAX_SAFE_INTEGER", 9007199254740991.0),
        ("MIN_SAFE_INTEGER", -9007199254740991.0),
        ("MAX_VALUE", f64::MAX),
        ("MIN_VALUE", 5e-324),
        ("NaN", f64::NAN),
        ("POSITIVE_INFINITY", f64::INFINITY),
        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
    ] {
        vm.constant(&ctor, n, Value::Num(v));
    }
    let g = vm.global.clone();
    vm.constant(&g, "NaN", Value::Num(f64::NAN));
    vm.constant(&g, "Infinity", Value::Num(f64::INFINITY));
    vm.constant(&g, "undefined", Value::Undefined);

    let bproto = vm.intr.boolean_proto.clone();
    let bctor = vm.make_ctor("Boolean", 1, boolean_ctor, &bproto);
    vm.set_global("Boolean", Value::Obj(bctor));
    vm.method(&bproto, "toString", 0, bool_to_string);
    vm.method(&bproto, "valueOf", 0, bool_value_of);

    let biproto = vm.intr.bigint_proto.clone();
    let bictor = vm.make_ctor("BigInt", 1, bigint_ctor, &biproto);
    vm.set_global("BigInt", Value::Obj(bictor.clone()));
    vm.method(&biproto, "toString", 0, big_to_string);
    vm.method(&biproto, "toLocaleString", 0, big_to_locale_string);
    vm.method(&biproto, "valueOf", 0, big_value_of);
    vm.method(&bictor, "asIntN", 2, as_int_n_fn);
    vm.method(&bictor, "asUintN", 2, as_uint_n_fn);
    let tag = vm.syms.to_string_tag.clone();
    biproto.set_sym(&tag, Value::str("BigInt"), CONFIGURABLE);
}
