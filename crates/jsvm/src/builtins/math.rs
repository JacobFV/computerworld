//! `Math`.

use crate::numconv::{to_int32, to_uint32};
use crate::value::*;
use crate::vm::Vm;

fn nums(vm: &mut Vm, a: &Args) -> JsResult<Vec<f64>> {
    let mut out = Vec::with_capacity(a.args.len());
    for v in &a.args {
        out.push(match v {
            Value::Num(n) => *n,
            other => vm.to_number(other)?,
        });
    }
    Ok(out)
}

fn n0(vm: &mut Vm, a: &Args) -> JsResult<f64> {
    match a.args.first() {
        Some(Value::Num(n)) => Ok(*n),
        Some(v) => {
            let v = v.clone();
            vm.to_number(&v)
        }
        None => Ok(f64::NAN),
    }
}

macro_rules! unary {
    ($name:ident, $f:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
            let x = n0(vm, a)?;
            let f: fn(f64) -> f64 = $f;
            Ok(Value::Num(f(x)))
        }
    };
}

unary!(abs, |x| x.abs());
unary!(acos, |x| x.acos());
unary!(acosh, |x| x.acosh());
unary!(asin, |x| x.asin());
unary!(asinh, |x| x.asinh());
unary!(atan, |x| x.atan());
unary!(atanh, |x| x.atanh());
unary!(cbrt, |x| x.cbrt());
unary!(ceil, |x| x.ceil());
unary!(cos, |x| x.cos());
unary!(cosh, |x| x.cosh());
unary!(exp, |x| x.exp());
unary!(expm1, |x| x.exp_m1());
unary!(floor, |x| x.floor());
unary!(fround, |x| (x as f32) as f64);
unary!(log, |x| x.ln());
unary!(log1p, |x| x.ln_1p());
unary!(log10, |x| x.log10());
unary!(log2, |x| x.log2());
unary!(sin, |x| x.sin());
unary!(sinh, |x| x.sinh());
unary!(sqrt, |x| x.sqrt());
unary!(tan, |x| x.tan());
unary!(tanh, |x| x.tanh());
unary!(trunc, |x| x.trunc());
unary!(sign, |x| if x.is_nan() {
    f64::NAN
} else if x > 0.0 {
    1.0
} else if x < 0.0 {
    -1.0
} else {
    x
});
unary!(round, |x| {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    if x > 0.0 && x < 0.5 {
        return 0.0;
    }
    if (-0.5..0.0).contains(&x) {
        return -0.0;
    }
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
});
unary!(clz32, |x| to_uint32(x).leading_zeros() as f64);

fn atan2(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = nums(vm, a)?;
    let y = v.first().copied().unwrap_or(f64::NAN);
    let x = v.get(1).copied().unwrap_or(f64::NAN);
    Ok(Value::Num(y.atan2(x)))
}

fn pow(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = nums(vm, a)?;
    let x = v.first().copied().unwrap_or(f64::NAN);
    let y = v.get(1).copied().unwrap_or(f64::NAN);
    Ok(Value::Num(crate::conv::js_pow(x, y)))
}

fn imul(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = nums(vm, a)?;
    let x = to_int32(v.first().copied().unwrap_or(0.0));
    let y = to_int32(v.get(1).copied().unwrap_or(0.0));
    Ok(Value::Num(x.wrapping_mul(y) as f64))
}

fn max(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = nums(vm, a)?;
    let mut r = f64::NEG_INFINITY;
    for x in v {
        if x.is_nan() {
            return Ok(Value::Num(f64::NAN));
        }
        if x > r || (x == 0.0 && r == 0.0 && !x.is_sign_negative()) {
            r = x;
        }
    }
    Ok(Value::Num(r))
}

fn min(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = nums(vm, a)?;
    let mut r = f64::INFINITY;
    for x in v {
        if x.is_nan() {
            return Ok(Value::Num(f64::NAN));
        }
        if x < r || (x == 0.0 && r == 0.0 && x.is_sign_negative()) {
            r = x;
        }
    }
    Ok(Value::Num(r))
}

fn hypot(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = nums(vm, a)?;
    if v.iter().any(|x| x.is_infinite()) {
        return Ok(Value::Num(f64::INFINITY));
    }
    if v.iter().any(|x| x.is_nan()) {
        return Ok(Value::Num(f64::NAN));
    }
    let m = v.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    if m == 0.0 {
        return Ok(Value::Num(0.0));
    }
    let s: f64 = v.iter().map(|x| (x / m) * (x / m)).sum();
    Ok(Value::Num(s.sqrt() * m))
}

fn random(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Num(vm.random()))
}

pub fn install(vm: &mut Vm) {
    let m = vm.new_object();
    let tag = vm.syms.to_string_tag.clone();
    m.set_sym(&tag, Value::str("Math"), CONFIGURABLE);
    for (n, v) in [
        ("E", std::f64::consts::E),
        ("LN10", std::f64::consts::LN_10),
        ("LN2", std::f64::consts::LN_2),
        ("LOG10E", std::f64::consts::LOG10_E),
        ("LOG2E", std::f64::consts::LOG2_E),
        ("PI", std::f64::consts::PI),
        ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
        ("SQRT2", std::f64::consts::SQRT_2),
    ] {
        vm.constant(&m, n, Value::Num(v));
    }
    let fs: &[(&str, u32, NativeFn)] = &[
        ("abs", 1, abs),
        ("acos", 1, acos),
        ("acosh", 1, acosh),
        ("asin", 1, asin),
        ("asinh", 1, asinh),
        ("atan", 1, atan),
        ("atanh", 1, atanh),
        ("atan2", 2, atan2),
        ("ceil", 1, ceil),
        ("cbrt", 1, cbrt),
        ("expm1", 1, expm1),
        ("clz32", 1, clz32),
        ("cos", 1, cos),
        ("cosh", 1, cosh),
        ("exp", 1, exp),
        ("floor", 1, floor),
        ("fround", 1, fround),
        ("hypot", 2, hypot),
        ("imul", 2, imul),
        ("log", 1, log),
        ("log1p", 1, log1p),
        ("log2", 1, log2),
        ("log10", 1, log10),
        ("max", 2, max),
        ("min", 2, min),
        ("pow", 2, pow),
        ("random", 0, random),
        ("round", 1, round),
        ("sign", 1, sign),
        ("sin", 1, sin),
        ("sinh", 1, sinh),
        ("sqrt", 1, sqrt),
        ("tan", 1, tan),
        ("tanh", 1, tanh),
        ("trunc", 1, trunc),
    ];
    for (n, l, f) in fs {
        vm.method(&m, n, *l, *f);
    }
    vm.set_global("Math", Value::Obj(m));
}
