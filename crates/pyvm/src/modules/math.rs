//! The `math` module.
use super::{new_module, set_fn, set_val};
use crate::bigint::BigInt;
use crate::builtins::*;
use crate::ops::Num;
use crate::value::*;
use crate::vm::*;

fn f(vm: &mut Vm, a: &Args, i: usize, fname: &str) -> PyResult<f64> {
    let v = a.args.get(i).ok_or_else(|| {
        type_err(format!(
            "math.{fname}() takes exactly one argument (0 given)"
        ))
    })?;
    match vm.as_num(v) {
        Some(Num::C(..)) => Err(type_err("must be real number, not complex")),
        Some(n) => vm.num_to_f64(&n),
        None => {
            if let Some(r) = vm.call_special(v, "__float__", vec![])? {
                if let Value::Float(x) = r {
                    return Ok(x);
                }
            }
            if let Some(r) = vm.call_special(v, "__index__", vec![])? {
                return crate::bfuncs::float_from(vm, &r);
            }
            Err(type_err(format!(
                "must be real number, not {}",
                vm.type_name(v)
            )))
        }
    }
}
fn domain() -> Box<PyErr> {
    value_err("math domain error")
}
fn range_err() -> Box<PyErr> {
    err("OverflowError", "math range error")
}
fn checked(x: f64, input_finite: bool) -> PyResult<Value> {
    if x.is_nan() && input_finite {
        return Err(domain());
    }
    if x.is_infinite() && input_finite {
        return Err(range_err());
    }
    Ok(Value::Float(x))
}

macro_rules! unary {
    ($m:expr, $name:expr, $body:expr) => {
        set_fn($m, $name, |vm, a| {
            let x = f(vm, &a, 0, $name)?;
            let g: fn(f64) -> PyResult<Value> = $body;
            g(x)
        });
    };
}

fn to_big(vm: &mut Vm, v: &Value, fname: &str) -> PyResult<BigInt> {
    match v {
        Value::Int(i) => Ok(BigInt::from_i64(*i)),
        Value::Bool(b) => Ok(BigInt::from_i64(*b as i64)),
        Value::Big(b) => Ok((**b).clone()),
        Value::Float(_) => Err(type_err(format!(
            "'float' object cannot be interpreted as an integer"
        ))),
        other => {
            let _ = fname;
            Ok(BigInt::from_i64(vm.index_of(other)?))
        }
    }
}

fn gcd_big(a: BigInt, b: BigInt) -> BigInt {
    let (mut a, mut b) = (a.abs(), b.abs());
    while !b.is_zero() {
        let r = a.divmod_floor(&b).1;
        a = b;
        b = r;
    }
    a
}

/// Exact float summation (Shewchuk), as math.fsum.
fn fsum(values: &[f64]) -> PyResult<f64> {
    let mut partials: Vec<f64> = vec![];
    let mut special = 0.0;
    let mut inf_sum = 0.0;
    for &x0 in values {
        let mut x = x0;
        if !x.is_finite() {
            if x.is_infinite() {
                inf_sum += x;
            }
            special += x;
            continue;
        }
        let mut i = 0;
        for j in 0..partials.len() {
            let mut y = partials[j];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let lo = y - (hi - x);
            if lo != 0.0 {
                partials[i] = lo;
                i += 1;
            }
            x = hi;
        }
        partials.truncate(i);
        partials.push(x);
    }
    if special != 0.0 {
        if inf_sum.is_nan() {
            return Err(value_err("-inf + inf in fsum"));
        }
        return Ok(special);
    }
    let mut hi = 0.0;
    if let Some(mut n) = partials.len().checked_sub(1) {
        hi = partials[n];
        let mut lo = 0.0;
        while n > 0 {
            let x = hi;
            n -= 1;
            let y = partials[n];
            hi = x + y;
            let yr = hi - x;
            lo = y - yr;
            if lo != 0.0 {
                break;
            }
        }
        if n > 0 && ((lo < 0.0 && partials[n - 1] < 0.0) || (lo > 0.0 && partials[n - 1] > 0.0)) {
            let y = lo * 2.0;
            let x = hi + y;
            let yr = x - hi;
            if y == yr {
                hi = x;
            }
        }
    }
    Ok(hi)
}

fn lgamma(x: f64) -> f64 {
    // Lanczos approximation.
    if x < 0.5 {
        let s = (std::f64::consts::PI / (std::f64::consts::PI * x).sin())
            .abs()
            .ln();
        return s - lgamma(1.0 - x);
    }
    let g = 7.0;
    let c = [
        0.999_999_999_999_809_9,
        676.5203681218851,
        -1259.1392167224028,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507343278686905,
        -0.13857109526572012,
        9.984_369_578_019_572e-6,
        1.5056327351493116e-7,
    ];
    let x = x - 1.0;
    let mut a = c[0];
    let t = x + g + 0.5;
    for (i, ci) in c.iter().enumerate().skip(1) {
        a += ci / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}
fn gamma(x: f64) -> PyResult<f64> {
    if x.fract() == 0.0 && x <= 0.0 {
        return Err(domain());
    }
    if x.fract() == 0.0 && x <= 23.0 {
        let mut r = 1.0;
        for i in 2..(x as i64) {
            r *= i as f64;
        }
        return Ok(r);
    }
    if x < 0.5 {
        return Ok(std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma(1.0 - x)?));
    }
    let r = lgamma(x).exp();
    if r.is_infinite() {
        return Err(range_err());
    }
    Ok(r)
}
fn erf(x: f64) -> f64 {
    // Abramowitz-Stegun 7.1.26 is too coarse; use a series / continued fraction.
    if x.abs() < 2.5 {
        let mut sum = x;
        let mut term = x;
        let x2 = x * x;
        let mut n = 0.0;
        loop {
            n += 1.0;
            term *= -x2 / n;
            let add = term / (2.0 * n + 1.0);
            sum += add;
            if add.abs() < 1e-17 * sum.abs() {
                break;
            }
            if n > 200.0 {
                break;
            }
        }
        sum * 2.0 / std::f64::consts::PI.sqrt()
    } else {
        1.0f64.copysign(x) - erfc_cf(x.abs()) * if x < 0.0 { -1.0 } else { 1.0 }
    }
}
fn erfc_cf(x: f64) -> f64 {
    // Continued fraction for erfc, x > 0.
    let mut f = 0.0;
    for k in (1..60).rev() {
        f = k as f64 / 2.0 / (x + f);
    }
    (-x * x).exp() / (std::f64::consts::PI.sqrt() * (x + f))
}

pub fn make(vm: &mut Vm) -> Value {
    let m = new_module("math");
    set_val(&m, "pi", Value::Float(std::f64::consts::PI));
    set_val(&m, "e", Value::Float(std::f64::consts::E));
    set_val(&m, "tau", Value::Float(std::f64::consts::TAU));
    set_val(&m, "inf", Value::Float(f64::INFINITY));
    set_val(&m, "nan", Value::Float(f64::NAN));
    unary!(&m, "sqrt", |x| if x < 0.0 {
        Err(domain())
    } else {
        Ok(Value::Float(x.sqrt()))
    });
    unary!(&m, "cbrt", |x| Ok(Value::Float(x.cbrt())));
    unary!(&m, "exp", |x| checked(x.exp(), x.is_finite()));
    unary!(&m, "exp2", |x| checked(x.exp2(), x.is_finite()));
    unary!(&m, "expm1", |x| checked(x.exp_m1(), x.is_finite()));
    unary!(&m, "log2", |x| if x <= 0.0 {
        Err(domain())
    } else {
        Ok(Value::Float(x.log2()))
    });
    unary!(&m, "log10", |x| if x <= 0.0 {
        Err(domain())
    } else {
        Ok(Value::Float(x.log10()))
    });
    unary!(&m, "log1p", |x| if x <= -1.0 {
        Err(domain())
    } else {
        Ok(Value::Float(x.ln_1p()))
    });
    unary!(&m, "sin", |x| if x.is_infinite() {
        Err(domain())
    } else {
        Ok(Value::Float(x.sin()))
    });
    unary!(&m, "cos", |x| if x.is_infinite() {
        Err(domain())
    } else {
        Ok(Value::Float(x.cos()))
    });
    unary!(&m, "tan", |x| if x.is_infinite() {
        Err(domain())
    } else {
        Ok(Value::Float(x.tan()))
    });
    unary!(&m, "asin", |x| if x.abs() > 1.0 {
        Err(domain())
    } else {
        Ok(Value::Float(x.asin()))
    });
    unary!(&m, "acos", |x| if x.abs() > 1.0 {
        Err(domain())
    } else {
        Ok(Value::Float(x.acos()))
    });
    unary!(&m, "atan", |x| Ok(Value::Float(x.atan())));
    unary!(&m, "sinh", |x| checked(x.sinh(), x.is_finite()));
    unary!(&m, "cosh", |x| checked(x.cosh(), x.is_finite()));
    unary!(&m, "tanh", |x| Ok(Value::Float(x.tanh())));
    unary!(&m, "asinh", |x| Ok(Value::Float(x.asinh())));
    unary!(&m, "acosh", |x| if x < 1.0 {
        Err(domain())
    } else {
        Ok(Value::Float(x.acosh()))
    });
    unary!(&m, "atanh", |x| if x.abs() >= 1.0 {
        Err(domain())
    } else {
        Ok(Value::Float(x.atanh()))
    });
    unary!(&m, "degrees", |x| Ok(Value::Float(x.to_degrees())));
    unary!(&m, "radians", |x| Ok(Value::Float(x.to_radians())));
    unary!(&m, "fabs", |x| Ok(Value::Float(x.abs())));
    unary!(&m, "isfinite", |x| Ok(Value::Bool(x.is_finite())));
    unary!(&m, "isinf", |x| Ok(Value::Bool(x.is_infinite())));
    unary!(&m, "isnan", |x| Ok(Value::Bool(x.is_nan())));
    unary!(&m, "erf", |x| Ok(Value::Float(erf(x))));
    unary!(&m, "erfc", |x| Ok(Value::Float(if x > 2.5 {
        erfc_cf(x)
    } else {
        1.0 - erf(x)
    })));
    unary!(&m, "gamma", |x| gamma(x).map(Value::Float));
    unary!(&m, "lgamma", |x| if x.fract() == 0.0 && x <= 0.0 {
        Err(domain())
    } else {
        Ok(Value::Float(lgamma(x)))
    });
    unary!(&m, "ulp", |x| {
        if !x.is_finite() {
            return Ok(Value::Float(x.abs()));
        }
        let x = x.abs();
        let next = f64::from_bits(x.to_bits() + 1);
        if next.is_infinite() {
            return Ok(Value::Float(x - f64::from_bits(x.to_bits() - 1)));
        }
        Ok(Value::Float(next - x))
    });
    set_fn(&m, "log", |vm, a| {
        let x = f(vm, &a, 0, "log")?;
        // Exact log for big ints beyond float range.
        if let Some(Value::Big(b)) = a.args.first() {
            if x.is_infinite() {
                let bits = b.bit_length();
                let shifted = b.shr(bits - 60).to_f64().unwrap_or(1.0);
                let ln = shifted.ln() + (bits - 60) as f64 * std::f64::consts::LN_2;
                let base = match a.args.get(1) {
                    Some(_) => f(vm, &a, 1, "log")?.ln(),
                    None => 1.0,
                };
                return Ok(Value::Float(ln / base));
            }
        }
        if x <= 0.0 {
            return Err(domain());
        }
        match a.args.get(1) {
            Some(_) => {
                let b = f(vm, &a, 1, "log")?;
                if b <= 0.0 || b == 1.0 {
                    if b == 1.0 {
                        return Err(err("ZeroDivisionError", "float division by zero"));
                    }
                    return Err(domain());
                }
                Ok(Value::Float(x.ln() / b.ln()))
            }
            None => Ok(Value::Float(x.ln())),
        }
    });
    set_fn(&m, "pow", |vm, a| {
        let x = f(vm, &a, 0, "pow")?;
        let y = f(vm, &a, 1, "pow")?;
        if x == 0.0 && y < 0.0 {
            return Err(domain());
        }
        if x < 0.0 && y.fract() != 0.0 && y.is_finite() {
            return Err(domain());
        }
        checked(x.powf(y), x.is_finite() && y.is_finite())
    });
    set_fn(&m, "atan2", |vm, a| {
        let y = f(vm, &a, 0, "atan2")?;
        let x = f(vm, &a, 1, "atan2")?;
        Ok(Value::Float(y.atan2(x)))
    });
    set_fn(&m, "copysign", |vm, a| {
        let x = f(vm, &a, 0, "copysign")?;
        let y = f(vm, &a, 1, "copysign")?;
        Ok(Value::Float(x.copysign(y)))
    });
    set_fn(&m, "fmod", |vm, a| {
        let x = f(vm, &a, 0, "fmod")?;
        let y = f(vm, &a, 1, "fmod")?;
        if y == 0.0 || x.is_infinite() {
            return Err(domain());
        }
        Ok(Value::Float(x % y))
    });
    set_fn(&m, "remainder", |vm, a| {
        let x = f(vm, &a, 0, "remainder")?;
        let y = f(vm, &a, 1, "remainder")?;
        if y == 0.0 {
            return Err(domain());
        }
        let n = (x / y).round_ties_even();
        Ok(Value::Float(x - n * y))
    });
    set_fn(&m, "modf", |vm, a| {
        let x = f(vm, &a, 0, "modf")?;
        Ok(Value::tuple(vec![
            Value::Float(x.fract()),
            Value::Float(x.trunc()),
        ]))
    });
    set_fn(&m, "frexp", |vm, a| {
        let x = f(vm, &a, 0, "frexp")?;
        let (mm, e) = frexp(x);
        Ok(Value::tuple(vec![Value::Float(mm), Value::Int(e as i64)]))
    });
    set_fn(&m, "ldexp", |vm, a| {
        let x = f(vm, &a, 0, "ldexp")?;
        let e = to_int_arg(vm, &a.args[1])?;
        checked(x * 2f64.powi(e.clamp(-2000, 2000) as i32), x.is_finite())
    });
    set_fn(&m, "hypot", |vm, a| {
        let mut acc = 0.0f64;
        for i in 0..a.args.len() {
            let x = f(vm, &a, i, "hypot")?;
            acc = acc.hypot(x);
        }
        Ok(Value::Float(acc))
    });
    set_fn(&m, "dist", |vm, a| {
        let p = vm.iterate(&a.args[0])?;
        let q = vm.iterate(&a.args[1])?;
        if p.len() != q.len() {
            return Err(value_err(
                "both points must have the same number of dimensions",
            ));
        }
        let mut acc = 0.0f64;
        for (x, y) in p.iter().zip(q.iter()) {
            let x = crate::bfuncs::float_from(vm, x)?;
            let y = crate::bfuncs::float_from(vm, y)?;
            acc = acc.hypot(x - y);
        }
        Ok(Value::Float(acc))
    });
    fn int_round(vm: &mut Vm, a: Args, mode: u8) -> PyResult<Value> {
        let v = a.args.first().cloned().unwrap_or(Value::None);
        match &v {
            Value::Int(_) | Value::Big(_) => return Ok(v),
            Value::Bool(b) => return Ok(Value::Int(*b as i64)),
            Value::Float(x) => {
                let r = match mode {
                    0 => x.floor(),
                    1 => x.ceil(),
                    _ => x.trunc(),
                };
                return float_to_int(r);
            }
            _ => {}
        }
        let dunder = match mode {
            0 => "__floor__",
            1 => "__ceil__",
            _ => "__trunc__",
        };
        if let Some(r) = vm.call_special(&v, dunder, vec![])? {
            return Ok(r);
        }
        let x = crate::bfuncs::float_from(vm, &v)?;
        let r = match mode {
            0 => x.floor(),
            1 => x.ceil(),
            _ => x.trunc(),
        };
        float_to_int(r)
    }
    set_fn(&m, "floor", |vm, a| int_round(vm, a, 0));
    set_fn(&m, "ceil", |vm, a| int_round(vm, a, 1));
    set_fn(&m, "trunc", |vm, a| int_round(vm, a, 2));
    set_fn(&m, "factorial", |vm, a| {
        let v = a.args.first().cloned().unwrap_or(Value::None);
        let n = match &v {
            Value::Int(i) => *i,
            Value::Bool(b) => *b as i64,
            Value::Float(_) => {
                return Err(type_err(
                    "'float' object cannot be interpreted as an integer",
                ))
            }
            other => vm.index_of(other)?,
        };
        if n < 0 {
            return Err(value_err("factorial() not defined for negative values"));
        }
        if n > 20000 {
            return Err(err(
                "OverflowError",
                "factorial() argument should not exceed 20000 in this simulation",
            ));
        }
        let mut acc = BigInt::from_i64(1);
        let mut small: u64 = 1;
        for i in 2..=n as u64 {
            if let Some(p) = small.checked_mul(i) {
                small = p;
            } else {
                acc = acc.mul(&BigInt::from_u64(small));
                small = i;
            }
        }
        acc = acc.mul(&BigInt::from_u64(small));
        Ok(Value::big(acc))
    });
    set_fn(&m, "gcd", |vm, a| {
        let mut acc = BigInt::zero();
        for v in &a.args {
            let b = to_big(vm, v, "gcd")?;
            acc = gcd_big(acc, b);
        }
        Ok(Value::big(acc))
    });
    set_fn(&m, "lcm", |vm, a| {
        let mut acc = BigInt::from_i64(1);
        for v in &a.args {
            let b = to_big(vm, v, "lcm")?;
            if b.is_zero() {
                return Ok(Value::Int(0));
            }
            let g = gcd_big(acc.clone(), b.clone());
            acc = acc.mul(&b).abs().divmod_floor(&g).0;
        }
        Ok(Value::big(acc))
    });
    set_fn(&m, "isqrt", |vm, a| {
        let b = to_big(vm, a.args.first().unwrap_or(&Value::Int(0)), "isqrt")?;
        if b.is_negative() {
            return Err(value_err("isqrt() argument must be nonnegative"));
        }
        Ok(Value::big(b.isqrt()))
    });
    set_fn(&m, "comb", |vm, a| {
        let n = to_big(vm, &a.args[0], "comb")?;
        let k = to_big(vm, &a.args[1], "comb")?;
        if n.is_negative() {
            return Err(value_err("n must be a non-negative integer"));
        }
        if k.is_negative() {
            return Err(value_err("k must be a non-negative integer"));
        }
        let (n, k) = (
            n.to_i64().unwrap_or(i64::MAX),
            k.to_i64().unwrap_or(i64::MAX),
        );
        if k > n {
            return Ok(Value::Int(0));
        }
        let k = k.min(n - k);
        let mut acc = BigInt::from_i64(1);
        for i in 0..k {
            acc = acc
                .mul(&BigInt::from_i64(n - i))
                .divmod_floor(&BigInt::from_i64(i + 1))
                .0;
        }
        Ok(Value::big(acc))
    });
    set_fn(&m, "perm", |vm, a| {
        let n = to_big(vm, &a.args[0], "perm")?.to_i64().unwrap_or(i64::MAX);
        let k = match a.args.get(1) {
            Some(Value::None) | None => n,
            Some(v) => to_big(vm, v, "perm")?.to_i64().unwrap_or(i64::MAX),
        };
        if n < 0 {
            return Err(value_err("n must be a non-negative integer"));
        }
        if k < 0 {
            return Err(value_err("k must be a non-negative integer"));
        }
        if k > n {
            return Ok(Value::Int(0));
        }
        let mut acc = BigInt::from_i64(1);
        for i in 0..k {
            acc = acc.mul(&BigInt::from_i64(n - i));
        }
        Ok(Value::big(acc))
    });
    set_fn(&m, "prod", |vm, mut a| {
        let start = a.kw("start").unwrap_or(Value::Int(1));
        let items = vm.iterate(&a.args[0])?;
        let mut acc = start;
        for it in items {
            acc = vm.binary_op(&acc, &it, crate::ast::BinOp::Mul)?;
        }
        Ok(acc)
    });
    set_fn(&m, "fsum", |vm, a| {
        let items = vm.iterate(&a.args[0])?;
        let mut vals = vec![];
        for it in items {
            vals.push(crate::bfuncs::float_from(vm, &it)?);
        }
        Ok(Value::Float(fsum(&vals)?))
    });
    set_fn(&m, "isclose", |vm, mut a| {
        let rel = match a.kw("rel_tol") {
            Some(v) => crate::bfuncs::float_from(vm, &v)?,
            None => 1e-9,
        };
        let abs = match a.kw("abs_tol") {
            Some(v) => crate::bfuncs::float_from(vm, &v)?,
            None => 0.0,
        };
        if rel < 0.0 || abs < 0.0 {
            return Err(value_err("tolerances must be non-negative"));
        }
        let x = f(vm, &a, 0, "isclose")?;
        let y = f(vm, &a, 1, "isclose")?;
        if x == y {
            return Ok(Value::Bool(true));
        }
        if x.is_infinite() || y.is_infinite() {
            return Ok(Value::Bool(false));
        }
        let d = (x - y).abs();
        Ok(Value::Bool(
            d <= (rel * y).abs() || d <= (rel * x).abs() || d <= abs,
        ))
    });
    set_fn(&m, "nextafter", |vm, a| {
        let x = f(vm, &a, 0, "nextafter")?;
        let y = f(vm, &a, 1, "nextafter")?;
        if x.is_nan() || y.is_nan() {
            return Ok(Value::Float(f64::NAN));
        }
        if x == y {
            return Ok(Value::Float(y));
        }
        if x == 0.0 {
            return Ok(Value::Float(f64::from_bits(1).copysign(y)));
        }
        let bits = x.to_bits();
        let up = (y > x) == (x > 0.0);
        Ok(Value::Float(f64::from_bits(if up {
            bits + 1
        } else {
            bits - 1
        })))
    });
    let _ = vm;
    Value::Module(m)
}
