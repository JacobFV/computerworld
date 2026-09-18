//! Deterministic elementary functions and engineering-notation numbers.
//!
//! Every function here is built only from IEEE-754 `+ - * /`, `sqrt` and exact bit
//! manipulation, which are correctly rounded on every target Rust supports. Platform
//! `libm` (`f64::exp`, `f64::sin`, …) is not: glibc, macOS, MSVC and the Wasm runtimes
//! disagree in the last bit, and a simulator whose waveforms depend on the host would
//! hash differently on each. Rust never contracts `a * b + c` into a fused multiply-add
//! on its own, so the evaluation order written here is the one that runs.

// Split constants for Cody–Waite reduction, written to the digits fdlibm publishes
// them with; the extra digits pin the exact double each one must be.
#[allow(clippy::excessive_precision)]
const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-1;
#[allow(clippy::excessive_precision)]
const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
const INV_LN2: f64 = std::f64::consts::LOG2_E;
pub const PI: f64 = std::f64::consts::PI;
#[allow(clippy::excessive_precision)]
const PIO2_1: f64 = 1.570_796_326_734_125_614_17;
#[allow(clippy::excessive_precision)]
const PIO2_2: f64 = 6.077_100_506_303_965_976_60e-11;
#[allow(clippy::excessive_precision)]
const PIO2_2T: f64 = 2.022_266_248_795_950_631_25e-21;

/// 2^k for integer k in the normal range, built from the exponent bits.
fn pow2i(k: i32) -> f64 {
    if k > 1023 {
        return f64::INFINITY;
    }
    if k < -1022 {
        // Subnormal results: scale in two exact steps.
        if k < -1074 {
            return 0.0;
        }
        return pow2i(k + 1000) * pow2i(-1000);
    }
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// Round half away from zero to an integer-valued float, without `f64::round`'s libm.
fn round_int(x: f64) -> f64 {
    if x >= 0.0 {
        (x + 0.5) as i64 as f64
    } else {
        -((-x + 0.5) as i64 as f64)
    }
}

/// e^x, correct to about one unit in the last place.
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if x < -745.133_219_101_941_1 {
        return 0.0;
    }
    let k = round_int(x * INV_LN2);
    let hi = x - k * LN2_HI;
    let lo = k * LN2_LO;
    let r = hi - lo;
    // Taylor series of e^r for |r| <= ln2/2 ≈ 0.347: 18 terms reach 1e-17 relative.
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut n = 1.0;
    while n < 19.0 {
        term = term * r / n;
        sum += term;
        n += 1.0;
    }
    sum * pow2i(k as i32)
}

/// Natural logarithm. `ln(0)` is -inf and a negative argument is NaN.
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return x;
    }
    let mut bits = x.to_bits();
    let mut e: i32 = 0;
    if (bits >> 52) & 0x7ff == 0 {
        // Subnormal: normalise first.
        let scaled = x * pow2i(54);
        bits = scaled.to_bits();
        e -= 54;
    }
    e += ((bits >> 52) & 0x7ff) as i32 - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    // Centre the mantissa on 1 so the series argument stays small.
    if m > std::f64::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let mut term = s;
    let mut sum = 0.0;
    let mut k = 1.0;
    while k < 40.0 {
        sum += term / k;
        term *= s2;
        k += 2.0;
    }
    let ef = e as f64;
    2.0 * sum + ef * LN2_LO + ef * LN2_HI
}

pub fn log10(x: f64) -> f64 {
    ln(x) * std::f64::consts::LOG10_E
}

/// x^y for x > 0 through exp and ln; integer powers of any sign are multiplied out.
pub fn pow(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        return 1.0;
    }
    if y == (y as i32) as f64 && y.abs() <= 64.0 {
        let mut n = (y as i32).unsigned_abs();
        let mut base = x;
        let mut acc = 1.0;
        while n > 0 {
            if n & 1 == 1 {
                acc *= base;
            }
            base *= base;
            n >>= 1;
        }
        return if y < 0.0 { 1.0 / acc } else { acc };
    }
    if x <= 0.0 {
        return if x == 0.0 { 0.0 } else { f64::NAN };
    }
    exp(y * ln(x))
}

pub fn pow10(y: f64) -> f64 {
    pow(10.0, y)
}

fn sin_kernel(x: f64) -> f64 {
    // |x| <= pi/4: Taylor to x^21.
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    let mut n = 1.0;
    while n < 21.0 {
        term = -term * x2 / ((n + 1.0) * (n + 2.0));
        sum += term;
        n += 2.0;
    }
    sum
}
fn cos_kernel(x: f64) -> f64 {
    let x2 = x * x;
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut n = 0.0;
    while n < 20.0 {
        term = -term * x2 / ((n + 1.0) * (n + 2.0));
        sum += term;
        n += 2.0;
    }
    sum
}
/// Reduce x by multiples of pi/2 (Cody–Waite, three parts): the quadrant and remainder.
/// The first part carries 33 bits, so `q * PIO2_1` is exact while |q| < 2^20.
fn reduce(x: f64) -> (i64, f64) {
    let q = round_int(x / (PI / 2.0));
    let r = (x - q * PIO2_1) - q * PIO2_2 - q * PIO2_2T;
    (q as i64, r)
}
pub fn sin(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let (q, r) = reduce(x);
    match q.rem_euclid(4) {
        0 => sin_kernel(r),
        1 => cos_kernel(r),
        2 => -sin_kernel(r),
        _ => -cos_kernel(r),
    }
}
pub fn cos(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let (q, r) = reduce(x);
    match q.rem_euclid(4) {
        0 => cos_kernel(r),
        1 => -sin_kernel(r),
        2 => -cos_kernel(r),
        _ => sin_kernel(r),
    }
}

/// arctangent on [0, 1] by argument halving and a short series.
fn atan_unit(x: f64) -> f64 {
    // atan(x) = 2 atan(x / (1 + sqrt(1 + x^2))), applied twice brings x below 0.2.
    let mut v = x;
    let mut scale = 1.0;
    for _ in 0..2 {
        v /= 1.0 + (1.0 + v * v).sqrt();
        scale *= 2.0;
    }
    let v2 = v * v;
    let mut term = v;
    let mut sum = 0.0;
    let mut k = 1.0;
    while k < 40.0 {
        sum += term / k;
        term = -term * v2;
        k += 2.0;
    }
    sum * scale
}
pub fn atan(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let a = x.abs();
    let r = if a <= 1.0 {
        atan_unit(a)
    } else if a.is_infinite() {
        PI / 2.0
    } else {
        PI / 2.0 - atan_unit(1.0 / a)
    };
    if x < 0.0 {
        -r
    } else {
        r
    }
}
pub fn atan2(y: f64, x: f64) -> f64 {
    if x > 0.0 {
        atan(y / x)
    } else if x < 0.0 {
        if y >= 0.0 {
            atan(y / x) + PI
        } else {
            atan(y / x) - PI
        }
    } else if y > 0.0 {
        PI / 2.0
    } else if y < 0.0 {
        -PI / 2.0
    } else {
        0.0
    }
}
pub fn tanh(x: f64) -> f64 {
    if x > 20.0 {
        return 1.0;
    }
    if x < -20.0 {
        return -1.0;
    }
    let e = exp(2.0 * x);
    (e - 1.0) / (e + 1.0)
}

/// Parse a SPICE/KiCad engineering value: `4k7`, `10u`, `1meg`, `2.2nF`, `1e-3`, `100mil`.
/// Unit letters after the multiplier (`F`, `Ohm`, `H`, `V`, `A`, `Hz`, `s`) are ignored,
/// as SPICE does. Returns `None` when no number leads the text.
pub fn parse_value(text: &str) -> Option<f64> {
    let t = text.trim();
    let bytes = t.as_bytes();
    let mut end = 0;
    let mut seen_digit = false;
    let mut seen_e = false;
    let mut seen_dot = false;
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_digit() {
            seen_digit = true;
        } else if (c == b'+' || c == b'-')
            && (end == 0 || matches!(bytes[end - 1], b'e' | b'E') && seen_e)
        {
        } else if c == b'.' && !seen_dot && !seen_e {
            seen_dot = true;
        } else if (c == b'e' || c == b'E')
            && seen_digit
            && !seen_e
            && bytes
                .get(end + 1)
                .is_some_and(|n| n.is_ascii_digit() || *n == b'-' || *n == b'+')
        {
            seen_e = true;
        } else {
            break;
        }
        end += 1;
    }
    if !seen_digit {
        return None;
    }
    let mantissa: f64 = t[..end].parse().ok()?;
    let rest = t[end..].to_ascii_lowercase();
    // "4k7" style: a multiplier letter standing in for the decimal point.
    let (mult, after) = multiplier(&rest);
    if mult != 1.0 || rest.starts_with('r') {
        let tail: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !tail.is_empty() && !t[..end].contains('.') && !seen_e {
            let frac: f64 = format!("0.{tail}").parse().ok()?;
            return Some((mantissa + frac) * mult);
        }
    }
    Some(mantissa * mult)
}
fn multiplier(rest: &str) -> (f64, &str) {
    for (prefix, value) in [
        ("meg", 1e6),
        ("mil", 25.4e-6),
        ("t", 1e12),
        ("g", 1e9),
        ("k", 1e3),
        ("m", 1e-3),
        ("u", 1e-6),
        ("µ", 1e-6),
        ("n", 1e-9),
        ("p", 1e-12),
        ("f", 1e-15),
        ("r", 1.0),
    ] {
        if let Some(after) = rest.strip_prefix(prefix) {
            return (value, after);
        }
    }
    (1.0, rest)
}

/// Engineering notation with an SI prefix and `digits` significant figures:
/// `4.70k`, `1.00m`, `-12.5µ`. Deterministic: Rust's own float formatting.
pub fn format_si(value: f64, digits: usize, unit: &str) -> String {
    if value == 0.0 || !value.is_finite() {
        return if value.is_finite() {
            format!("0{unit}")
        } else {
            format!("{value}{unit}")
        };
    }
    let prefixes = [
        (1e12, "T"),
        (1e9, "G"),
        (1e6, "M"),
        (1e3, "k"),
        (1.0, ""),
        (1e-3, "m"),
        (1e-6, "µ"),
        (1e-9, "n"),
        (1e-12, "p"),
        (1e-15, "f"),
    ];
    let a = value.abs();
    let (scale, prefix) = prefixes
        .iter()
        .copied()
        .find(|(s, _)| a >= *s * 0.9995)
        .unwrap_or((1e-15, "f"));
    let scaled = value / scale;
    let whole = scaled.abs() as u64;
    let int_digits = if whole >= 100 {
        3
    } else if whole >= 10 {
        2
    } else {
        1
    };
    let decimals = digits.saturating_sub(int_digits);
    let mut s = format!("{scaled:.decimals$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    format!("{s}{prefix}{unit}")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1e-300)
    }
    #[test]
    fn elementary_functions_match_known_values() {
        assert_eq!(exp(0.0), 1.0);
        assert!(close(exp(1.0), std::f64::consts::E, 4e-16));
        assert!(close(exp(-20.5), 1.250_152_866_386_743_5e-9, 1e-14));
        assert!(close(exp(40.0), 2.353_852_668_370_199_8e17, 1e-14));
        assert!(close(ln(10.0), std::f64::consts::LN_10, 4e-16));
        assert!(close(ln(1e-300), -690.775_527_898_213_7, 1e-15));
        assert!(close(ln(0.75), -0.287_682_072_451_780_9, 1e-15));
        assert!(close(sin(1.0), 0.841_470_984_807_896_5, 1e-15));
        assert!(close(cos(1.0), 0.540_302_305_868_139_8, 1e-15));
        assert!(close(sin(1000.0), 0.826_879_540_532_002_6, 1e-12));
        assert!(sin(PI).abs() < 1e-15);
        assert!(close(atan2(1.0, 1.0), PI / 4.0, 1e-15));
        assert!(close(atan2(-1.0, -1.0), -3.0 * PI / 4.0, 1e-15));
        assert!(close(pow(2.0, 0.5), std::f64::consts::SQRT_2, 1e-15));
        assert!(close(log10(1000.0), 3.0, 1e-15));
    }
    #[test]
    fn engineering_values_parse_like_spice() {
        assert_eq!(parse_value("10k"), Some(10_000.0));
        assert_eq!(parse_value("4k7"), Some(4_700.0));
        assert_eq!(parse_value("1meg"), Some(1e6));
        assert_eq!(parse_value("1M"), Some(1e-3), "SPICE reads M as milli");
        assert!(close(parse_value("2.2nF").unwrap(), 2.2e-9, 1e-15));
        assert_eq!(parse_value("1e-3"), Some(1e-3));
        assert_eq!(parse_value("-5"), Some(-5.0));
        assert_eq!(parse_value("100"), Some(100.0));
        assert_eq!(parse_value("x"), None);
        assert_eq!(format_si(4700.0, 3, "Ω"), "4.7kΩ");
        assert_eq!(format_si(0.001, 3, "s"), "1ms");
        assert_eq!(format_si(-1.5e-6, 3, "A"), "-1.5µA");
    }
}
