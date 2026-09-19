//! Deterministic arithmetic. Transcendental functions from a host `libm` differ between
//! native targets and Wasm in the last bit, which is enough to move a pixel. Everything
//! here is built from IEEE-754 basic operations (`+ - * /`, `floor`, bit casts), which
//! are correctly rounded on every target, so the same input gives the same bits.

const LN2: f64 = std::f64::consts::LN_2;
const SQRT2: f64 = std::f64::consts::SQRT_2;
pub const PI: f64 = std::f64::consts::PI;

/// Integer square root, `floor(sqrt(n))`.
pub fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    // Newton's method from an over-estimate converges monotonically downwards.
    let mut x = 1u64 << (64 - n.leading_zeros()).div_ceil(2);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// `2^k` for an integer exponent in the normal range.
fn pow2i(k: i64) -> f64 {
    let k = k.clamp(-1022, 1023);
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// `e^x`, by range reduction to `|r| <= ln2/2` and a Taylor series.
pub fn exp(x: f64) -> f64 {
    if x < -700.0 {
        return 0.0;
    }
    let x = x.min(700.0);
    let k = (x / LN2 + 0.5).floor();
    let r = x - k * LN2;
    let mut term = 1.0;
    let mut sum = 1.0;
    for i in 1..=22 {
        term = term * r / i as f64;
        sum += term;
    }
    sum * pow2i(k as i64)
}

/// Natural logarithm of a positive number; 0 and negatives give a large negative value.
pub fn ln(x: f64) -> f64 {
    if x <= 0.0 {
        return -745.0;
    }
    let bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mut m = f64::from_bits((bits & ((1u64 << 52) - 1)) | (1023u64 << 52));
    if m > SQRT2 {
        m /= 2.0;
        e += 1;
    }
    // ln(m) = 2 atanh((m - 1) / (m + 1)), |s| < 0.172 so the series converges fast.
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let mut term = s;
    let mut sum = 0.0;
    for i in 0..24 {
        sum += term / (2 * i + 1) as f64;
        term *= s2;
    }
    2.0 * sum + e as f64 * LN2
}

/// `x^y` for `x >= 0`.
pub fn powf(x: f64, y: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if y == 0.0 {
        return 1.0;
    }
    exp(y * ln(x))
}

/// Sine of an angle in radians.
pub fn sin(x: f64) -> f64 {
    let two_pi = 2.0 * PI;
    let mut r = x - (x / two_pi).floor() * two_pi; // [0, 2π)
    if r > PI {
        r -= two_pi; // (-π, π]
    }
    // Fold into [-π/2, π/2] where the series is most accurate.
    if r > PI / 2.0 {
        r = PI - r;
    } else if r < -PI / 2.0 {
        r = -PI - r;
    }
    let r2 = r * r;
    let mut term = r;
    let mut sum = r;
    for i in 1..=12 {
        term = -term * r2 / ((2 * i) as f64 * (2 * i + 1) as f64);
        sum += term;
    }
    sum
}

pub fn cos(x: f64) -> f64 {
    sin(x + PI / 2.0)
}

/// Arctangent, by reduction to `|x| <= tan(π/12)` and a Taylor series.
pub fn atan(x: f64) -> f64 {
    if x < 0.0 {
        return -atan(-x);
    }
    if x > 1.0 {
        return PI / 2.0 - atan(1.0 / x);
    }
    const SQRT3: f64 = 1.732_050_807_568_877_2;
    const TAN_PI_12: f64 = 0.267_949_192_431_122_7;
    let (base, r) = if x > TAN_PI_12 {
        (PI / 6.0, (x * SQRT3 - 1.0) / (x + SQRT3))
    } else {
        (0.0, x)
    };
    let r2 = r * r;
    let mut term = r;
    let mut sum = 0.0;
    for i in 0..20 {
        sum += term / (2 * i + 1) as f64;
        term = -term * r2;
    }
    base + sum
}

/// The angle of `(x, y)` from the positive x axis, in `(-π, π]`.
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

/// Round half away from zero, without relying on a platform `round`.
pub fn round(x: f64) -> f64 {
    if x >= 0.0 {
        (x + 0.5).floor()
    } else {
        -((-x + 0.5).floor())
    }
}

/// Round to the nearest integer and clamp into a byte.
pub fn to_u8(x: f64) -> u8 {
    round(x).clamp(0.0, 255.0) as u8
}

/// `(x + 127) / 255`: exact rounding division of a product of two bytes.
#[inline]
pub fn div255(x: u32) -> u32 {
    (x + 127) / 255
}

#[inline]
pub fn mul255(a: u8, b: u8) -> u8 {
    div255(u32::from(a) * u32::from(b)) as u8
}

/// Percent (0..=100) to a byte (0..=255), rounded.
#[inline]
pub fn percent255(percent: u8) -> u8 {
    ((u32::from(percent.min(100)) * 255 + 50) / 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn square_roots_are_exact_floors() {
        for n in [0u64, 1, 2, 3, 4, 15, 16, 17, 99, 100, 1 << 40, u64::MAX] {
            let r = isqrt(n);
            assert!(r * r <= n, "{n}");
            assert!((r + 1).checked_mul(r + 1).is_none_or(|s| s > n), "{n}");
        }
    }
    #[test]
    fn transcendental_functions_are_accurate() {
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs().max(1.0);
        assert!(close(exp(1.0), std::f64::consts::E));
        assert!(close(exp(-3.5), 0.030_197_383_422_318_5));
        assert!(close(ln(10.0), std::f64::consts::LN_10));
        assert!(close(powf(2.0, 0.5), SQRT2));
        assert!(close(powf(0.5, 2.4), 0.18946457081379978));
        assert!(close(sin(PI / 6.0), 0.5));
        assert!(close(cos(PI / 3.0), 0.5));
        assert!(close(sin(-PI / 2.0), -1.0));
        assert!(sin(PI).abs() < 1e-12);
        assert!(close(cos(7.0 * PI / 4.0), SQRT2 / 2.0));
        assert!(close(atan(1.0), PI / 4.0));
        assert!(close(atan(0.5), 0.463_647_609_000_806_1));
        assert!(close(atan(-20.0), -1.520_837_931_072_953_7));
        assert!(close(atan2(1.0, -1.0), 3.0 * PI / 4.0));
        assert!(close(atan2(-1.0, -1.0), -3.0 * PI / 4.0));
        assert!(close(atan2(-2.0, 0.0), -PI / 2.0));
        assert_eq!(atan2(0.0, 0.0), 0.0);
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -3.0);
        assert_eq!(div255(255 * 255), 255);
        assert_eq!(mul255(128, 255), 128);
        assert_eq!(mul255(200, 100), 78);
        assert_eq!(percent255(50), 128);
    }
}
