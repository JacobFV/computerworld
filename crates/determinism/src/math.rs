//! Transcendental functions built from IEEE-754 basic operations only.
//!
//! `f64::exp`, `ln`, `powf` and the trigonometric methods call the platform's libm, and
//! glibc, musl and the Wasm port do not agree to the last bit. Anything that feeds a
//! simulated result (a spreadsheet cell, a SQL value) must be identical on every target,
//! so it uses these instead. Addition, subtraction, multiplication, division and square
//! root are correctly rounded everywhere and Rust never contracts them into fused
//! operations, so everything below is bit-for-bit reproducible.
//!
//! `exp`, the sine/cosine kernels, the argument reduction and `atan` follow fdlibm (Sun
//! Microsystems, freely redistributable); `ln` and `pow` go through a double-double
//! logarithm so `pow` stays within about one ulp even for large exponents.
// The constants are fdlibm's, written with every digit it publishes so each one names
// its exact double; truncating them or swapping in `std::f64::consts` would change
// nothing numerically but would hide where they come from.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

/// Error-free sum: `a + b == hi + lo` exactly.
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}
/// `a + b` renormalised when `|a| >= |b|`.
fn fast_two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    (s, b - (s - a))
}
/// Veltkamp split into two halves whose products are exact.
fn split(a: f64) -> (f64, f64) {
    if a.abs() > 6.696_928_794_914_17e299 {
        let (h, l) = split(a * 3.725_290_298_461_914e-9);
        return (h * 268_435_456.0, l * 268_435_456.0);
    }
    let t = 134_217_729.0 * a;
    let hi = t - (t - a);
    (hi, a - hi)
}
/// Error-free product without a fused multiply-add: `a * b == hi + lo` exactly.
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    if !p.is_finite() {
        return (p, 0.0);
    }
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}
fn dd_mul(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (p, e) = two_prod(a.0, b.0);
    if !p.is_finite() {
        return (p, 0.0);
    }
    fast_two_sum(p, e + (a.0 * b.1 + a.1 * b.0))
}
fn dd_div(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let q = a.0 / b.0;
    let (p, e) = two_prod(q, b.0);
    let r = ((a.0 - p) - e + a.1 - q * b.1) / b.0;
    fast_two_sum(q, r)
}

const LN2_HI: f64 = 6.931_471_803_691_238_164_9e-1;
const LN2_LO: f64 = 1.908_214_929_270_587_7e-10;

/// e^x, within one ulp.
pub fn exp(x: f64) -> f64 {
    const O_THRESHOLD: f64 = 7.097_827_128_933_839_730_96e2;
    const U_THRESHOLD: f64 = -7.451_332_191_019_411_084_2e2;
    const INVLN2: f64 = 1.442_695_040_888_963_387;
    const P1: f64 = 1.666_666_666_666_660_190_37e-1;
    const P2: f64 = -2.777_777_777_701_559_338_42e-3;
    const P3: f64 = 6.613_756_321_437_934_361_17e-5;
    const P4: f64 = -1.653_390_220_546_525_153_9e-6;
    const P5: f64 = 4.138_136_797_057_238_460_39e-8;
    if x.is_nan() {
        return x;
    }
    if x > O_THRESHOLD {
        return f64::INFINITY;
    }
    if x < U_THRESHOLD {
        return 0.0;
    }
    let ax = x.abs();
    let (hi, lo, k, x) = if ax > 0.5 * LN2_HI {
        let (hi, lo, k) = if ax < 1.5 * LN2_HI {
            if x > 0.0 {
                (x - LN2_HI, LN2_LO, 1)
            } else {
                (x + LN2_HI, -LN2_LO, -1)
            }
        } else {
            let half = if x < 0.0 { -0.5 } else { 0.5 };
            let k = (INVLN2 * x + half) as i32;
            let t = f64::from(k);
            (x - t * LN2_HI, t * LN2_LO, k)
        };
        (hi, lo, k, hi - lo)
    } else if ax < 3.725_290_298_461_914e-9 {
        return 1.0 + x;
    } else {
        (0.0, 0.0, 0, x)
    };
    let t = x * x;
    let c = x - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    if k == 0 {
        return 1.0 - ((x * c) / (c - 2.0) - x);
    }
    let y = 1.0 - ((lo - (x * c) / (2.0 - c)) - hi);
    scale(y, k)
}
/// `y * 2^k` for the range `exp` produces.
fn scale(y: f64, k: i32) -> f64 {
    if k > 1023 {
        return scale(y, k - 1) * 2.0;
    }
    if k >= -1021 {
        f64::from_bits((y.to_bits() as i64 + (i64::from(k) << 52)) as u64)
    } else {
        // Subnormal result: move most of the exponent exactly, round once at the end.
        let twom1000 = f64::from_bits(((0x3ff - 1000) as u64) << 52);
        f64::from_bits((y.to_bits() as i64 + (i64::from(k + 1000) << 52)) as u64) * twom1000
    }
}
/// Natural logarithm as an unevaluated double-double sum, for finite `x > 0`.
fn ln_dd(x: f64) -> (f64, f64) {
    let mut bits = x.to_bits();
    let mut k: i32 = 0;
    if bits < 0x0010_0000_0000_0000 {
        bits = (x * 1.801_439_850_948_198_4e16).to_bits();
        k -= 54;
    }
    k += ((bits >> 52) as i32) - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 {
        m *= 0.5;
        k += 1;
    }
    // m in (sqrt(1/2), sqrt(2)], so m - 1 is exact (Sterbenz).
    let f = m - 1.0;
    let s = dd_div((f, 0.0), two_sum(2.0, f));
    let z = s.0 * s.0;
    // 2 atanh(s) = 2s + s·z·(2/3 + 2z/5 + 2z²/7 + …) with |z| < 0.0295.
    let mut poly = 0.0;
    let mut n = 25.0;
    while n >= 3.0 {
        poly = 2.0 / n + z * poly;
        n -= 2.0;
    }
    let tail = s.0 * z * poly;
    let (hi, lo) = fast_two_sum(2.0 * s.0, 2.0 * s.1 + tail);
    let kf = f64::from(k);
    let (h, l) = two_sum(kf * LN2_HI, hi);
    let (h, l) = fast_two_sum(h, l + lo + kf * LN2_LO);
    (h, l)
}
/// Natural logarithm. `ln(0)` is -inf and a negative argument is NaN, as in IEEE-754.
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() || x == 1.0 {
        return if x == 1.0 { 0.0 } else { x };
    }
    let (h, l) = ln_dd(x);
    h + l
}
/// Base-10 logarithm; exact on powers of ten.
pub fn log10(x: f64) -> f64 {
    log_base(x, (2.302_585_092_994_045_7, -2.170_756_223_382_249_2e-16))
}
/// Base-2 logarithm; exact on powers of two.
pub fn log2(x: f64) -> f64 {
    let bits = x.to_bits();
    if x > 0.0 && x.is_finite() && bits & 0x000f_ffff_ffff_ffff == 0 && bits >> 52 != 0 {
        return ((bits >> 52) as i64 - 1023) as f64;
    }
    log_base(x, fast_two_sum(LN2_HI, LN2_LO))
}
fn log_base(x: f64, ln_base: (f64, f64)) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return x;
    }
    let (h, l) = dd_div(ln_dd(x), ln_base);
    let r = h + l;
    // An integer answer is exact when the double-double quotient is that close to it,
    // so log10(1000) is 3 and not 2.9999999999999996.
    let n = r.round();
    if ((h - n) + l).abs() <= 1e-28 * n.abs().max(1.0) {
        return n;
    }
    r
}
fn is_integer(y: f64) -> bool {
    y.is_finite() && y == y.trunc()
}
/// `x^y` with IEEE-754 special cases. Integer exponents up to 1024 are computed by
/// double-double repeated squaring, so exactly representable powers come out exact.
pub fn pow(x: f64, y: f64) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if y == 1.0 {
        return x;
    }
    let odd = is_integer(y) && y.abs() < 9.007_199_254_740_992e15 && (y % 2.0).abs() == 1.0;
    if x == 0.0 {
        let r = if y < 0.0 { f64::INFINITY } else { 0.0 };
        return if x.is_sign_negative() && odd { -r } else { r };
    }
    if y.is_infinite() {
        let ax = x.abs();
        return if ax == 1.0 {
            1.0
        } else if (ax > 1.0) == (y > 0.0) {
            f64::INFINITY
        } else {
            0.0
        };
    }
    if x.is_infinite() {
        let r = if y < 0.0 { 0.0 } else { f64::INFINITY };
        return if x < 0.0 && odd { -r } else { r };
    }
    if x < 0.0 && !is_integer(y) {
        return f64::NAN;
    }
    if y == 0.5 {
        return x.sqrt();
    }
    let ax = x.abs();
    let r = if is_integer(y) && y.abs() <= 1024.0 {
        let mut n = y.abs() as u32;
        let mut base = (ax, 0.0);
        let mut acc = (1.0, 0.0);
        while n > 0 {
            if n & 1 == 1 {
                acc = dd_mul(acc, base);
            }
            n >>= 1;
            if n > 0 {
                base = dd_mul(base, base);
            }
        }
        if y < 0.0 {
            if acc.0.is_infinite() {
                0.0
            } else if acc.0 == 0.0 {
                f64::INFINITY
            } else {
                let q = dd_div((1.0, 0.0), acc);
                q.0 + q.1
            }
        } else {
            acc.0 + acc.1
        }
    } else {
        let l = ln_dd(ax);
        let (p, e) = two_prod(y, l.0);
        let (hi, lo) = fast_two_sum(p, e + y * l.1);
        if hi > 709.8 {
            f64::INFINITY
        } else if hi < -745.2 {
            0.0
        } else {
            let base = exp(hi);
            base + base * lo
        }
    };
    if x < 0.0 && odd {
        -r
    } else {
        r
    }
}

/// pi/2 as the double nearest it.
pub const PIO2_HI: f64 = 1.570_796_326_794_896_558;

/// Reduce `x` by multiples of pi/2: `(n mod 4, y0, y1)` with `x - n·pi/2 = y0 + y1`.
/// Valid below [`TRIG_LIMIT`].
fn rem_pio2(x: f64) -> (i32, f64, f64) {
    const INVPIO2: f64 = 6.366_197_723_675_813_824_33e-1;
    const PIO2_1: f64 = 1.570_796_326_734_125_614_17;
    const PIO2_1T: f64 = 6.077_100_506_506_192_249_32e-11;
    const PIO2_2: f64 = 6.077_100_506_303_965_976_6e-11;
    const PIO2_2T: f64 = 2.022_266_248_795_950_631_54e-21;
    const PIO2_3: f64 = 2.022_266_248_711_166_455_8e-21;
    const PIO2_3T: f64 = 8.478_427_660_368_899_569_97e-32;
    if x.abs() <= std::f64::consts::FRAC_PI_4 {
        return (0, x, 0.0);
    }
    let n = (x * INVPIO2).round();
    let mut r = x - n * PIO2_1;
    let mut w = n * PIO2_1T;
    let mut y0 = r - w;
    let exponent = |v: f64| ((v.to_bits() >> 52) & 0x7ff) as i32;
    let j = exponent(x);
    if j - exponent(y0) > 16 {
        let t = r;
        w = n * PIO2_2;
        r = t - w;
        w = n * PIO2_2T - ((t - r) - w);
        y0 = r - w;
        if j - exponent(y0) > 49 {
            let t = r;
            w = n * PIO2_3;
            r = t - w;
            w = n * PIO2_3T - ((t - r) - w);
            y0 = r - w;
        }
    }
    let y1 = (r - y0) - w;
    ((n as i64).rem_euclid(4) as i32, y0, y1)
}
fn kernel_sin(x: f64, y: f64) -> f64 {
    const S1: f64 = -1.666_666_666_666_663_243_48e-1;
    const S2: f64 = 8.333_333_333_322_489_461_24e-3;
    const S3: f64 = -1.984_126_982_985_794_931_34e-4;
    const S4: f64 = 2.755_731_370_707_006_767_89e-6;
    const S5: f64 = -2.505_076_025_340_686_341_95e-8;
    const S6: f64 = 1.589_690_995_211_550_102_21e-10;
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    x - ((z * (0.5 * y - v * r) - y) - v * S1)
}
fn kernel_cos(x: f64, y: f64) -> f64 {
    const C1: f64 = 4.166_666_666_666_660_190_37e-2;
    const C2: f64 = -1.388_888_888_887_410_957_49e-3;
    const C3: f64 = 2.480_158_728_947_672_941_78e-5;
    const C4: f64 = -2.755_731_435_139_066_330_35e-7;
    const C5: f64 = 2.087_572_321_298_174_827_9e-9;
    const C6: f64 = -1.135_964_755_778_819_482_65e-11;
    let z = x * x;
    let w = z * z;
    let r = z * (C1 + z * (C2 + z * C3)) + w * w * (C4 + z * (C5 + z * C6));
    let hz = 0.5 * z;
    let w = 1.0 - hz;
    w + (((1.0 - w) - hz) + (z * r - x * y))
}
/// Largest argument the trigonometric functions reduce (2^27, the spreadsheet limit);
/// at or beyond it they return NaN rather than a meaningless value.
pub const TRIG_LIMIT: f64 = 134_217_728.0;
pub fn sin(x: f64) -> f64 {
    if !x.is_finite() || x.abs() >= TRIG_LIMIT {
        return f64::NAN;
    }
    let (n, a, b) = rem_pio2(x);
    match n {
        0 => kernel_sin(a, b),
        1 => kernel_cos(a, b),
        2 => -kernel_sin(a, b),
        _ => -kernel_cos(a, b),
    }
}
pub fn cos(x: f64) -> f64 {
    if !x.is_finite() || x.abs() >= TRIG_LIMIT {
        return f64::NAN;
    }
    let (n, a, b) = rem_pio2(x);
    match n {
        0 => kernel_cos(a, b),
        1 => -kernel_sin(a, b),
        2 => -kernel_cos(a, b),
        _ => kernel_sin(a, b),
    }
}
pub fn tan(x: f64) -> f64 {
    if !x.is_finite() || x.abs() >= TRIG_LIMIT {
        return f64::NAN;
    }
    let (n, a, b) = rem_pio2(x);
    let (s, c) = (kernel_sin(a, b), kernel_cos(a, b));
    if n % 2 == 0 {
        s / c
    } else {
        -c / s
    }
}
pub fn atan(x: f64) -> f64 {
    const ATANHI: [f64; 4] = [
        4.636_476_090_008_060_935_15e-1,
        7.853_981_633_974_482_789_99e-1,
        9.827_937_232_473_290_540_82e-1,
        1.570_796_326_794_896_558,
    ];
    const ATANLO: [f64; 4] = [
        2.269_877_745_296_168_709_24e-17,
        3.061_616_997_868_383_017_93e-17,
        1.390_331_103_123_099_845_16e-17,
        6.123_233_995_736_766_035_87e-17,
    ];
    const AT: [f64; 11] = [
        3.333_333_333_333_293_180_27e-1,
        -1.999_999_999_987_648_324_76e-1,
        1.428_571_427_250_346_637_11e-1,
        -1.111_111_040_546_235_578_8e-1,
        9.090_887_133_436_506_561_96e-2,
        -7.691_876_205_044_829_994_95e-2,
        6.661_073_137_387_531_206_69e-2,
        -5.833_570_133_790_573_486_45e-2,
        4.976_877_994_615_932_360_17e-2,
        -3.653_157_274_421_691_552_7e-2,
        1.628_582_011_536_578_236_23e-2,
    ];
    if x.is_nan() {
        return x;
    }
    let negative = x < 0.0;
    let mut ax = x.abs();
    if ax >= 7.378_697_629_483_821e19 {
        let r = ATANHI[3] + ATANLO[3];
        return if negative { -r } else { r };
    }
    let id: i32 = if ax < 0.4375 {
        if ax < 3.725_290_298_461_914e-9 {
            return x;
        }
        -1
    } else if ax < 1.1875 {
        if ax < 0.6875 {
            ax = (2.0 * ax - 1.0) / (2.0 + ax);
            0
        } else {
            ax = (ax - 1.0) / (ax + 1.0);
            1
        }
    } else if ax < 2.4375 {
        ax = (ax - 1.5) / (1.0 + 1.5 * ax);
        2
    } else {
        ax = -1.0 / ax;
        3
    };
    let z = ax * ax;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    let r = if id < 0 {
        ax - ax * (s1 + s2)
    } else {
        let i = id as usize;
        ATANHI[i] - ((ax * (s1 + s2) - ATANLO[i]) - ax)
    };
    if negative {
        -r
    } else {
        r
    }
}
/// Angle of the point `(x, y)`, in [-pi, pi].
pub fn atan2(y: f64, x: f64) -> f64 {
    const PI: f64 = std::f64::consts::PI;
    const PI_LO: f64 = 1.224_646_799_147_353_207_2e-16;
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        return if x > 0.0 || (x == 0.0 && !x.is_sign_negative()) {
            y
        } else if y.is_sign_negative() {
            -PI
        } else {
            PI
        };
    }
    if x == 0.0 || y.is_infinite() && x.is_finite() {
        return if y > 0.0 { PIO2_HI } else { -PIO2_HI };
    }
    if x.is_infinite() {
        let base = if y.is_infinite() {
            if x > 0.0 {
                std::f64::consts::FRAC_PI_4
            } else {
                3.0 * std::f64::consts::FRAC_PI_4
            }
        } else if x > 0.0 {
            0.0
        } else {
            PI
        };
        return if y < 0.0 { -base } else { base };
    }
    let z = atan((y / x).abs());
    match (x > 0.0, y > 0.0) {
        (true, true) => z,
        (true, false) => -z,
        (false, true) => PI - (z - PI_LO),
        (false, false) => (z - PI_LO) - PI,
    }
}
pub fn asin(x: f64) -> f64 {
    if x.is_nan() || x.abs() > 1.0 {
        return f64::NAN;
    }
    if x.abs() == 1.0 {
        return if x > 0.0 { PIO2_HI } else { -PIO2_HI };
    }
    atan(x / ((1.0 - x) * (1.0 + x)).sqrt())
}
pub fn acos(x: f64) -> f64 {
    if x.is_nan() || x.abs() > 1.0 {
        return f64::NAN;
    }
    if x == 1.0 {
        return 0.0;
    }
    if x == -1.0 {
        return std::f64::consts::PI;
    }
    2.0 * atan(((1.0 - x) / (1.0 + x)).sqrt())
}
pub fn sinh(x: f64) -> f64 {
    if x.abs() < 1e-5 {
        return x + x * x * x / 6.0;
    }
    let e = exp(x);
    (e - 1.0 / e) / 2.0
}
pub fn cosh(x: f64) -> f64 {
    let e = exp(x);
    (e + 1.0 / e) / 2.0
}
pub fn tanh(x: f64) -> f64 {
    if x.abs() > 22.0 {
        return x.signum();
    }
    if x.abs() < 1e-5 {
        return x - x * x * x / 3.0;
    }
    let e = exp(2.0 * x);
    (e - 1.0) / (e + 1.0)
}
/// `ln(1 + x)`, accurate for tiny `x`.
pub fn ln_1p(x: f64) -> f64 {
    let u = 1.0 + x;
    if u == 1.0 {
        return x;
    }
    ln(u) * (x / (u - 1.0))
}
/// Round-trip-stable decimal text for a finite double with at most `digits`
/// significant digits (1..=17), the way `%.*g` prints it: no exponent between 1e-5 and
/// 10^digits, trailing zeros removed. Uses only `core` formatting, which is exact.
pub fn format_significant(x: f64, digits: usize) -> String {
    if x == 0.0 {
        return "0".into();
    }
    if !x.is_finite() {
        return if x.is_nan() {
            "NaN".into()
        } else if x > 0.0 {
            "Inf".into()
        } else {
            "-Inf".into()
        };
    }
    let digits = digits.clamp(1, 17);
    let sci = format!("{:.*e}", digits - 1, x);
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits_only: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let trimmed = digits_only.trim_end_matches('0');
    let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if exponent < -5 || exponent >= digits as i32 {
        out.push_str(&trimmed[..1]);
        if trimmed.len() > 1 {
            out.push('.');
            out.push_str(&trimmed[1..]);
        }
        out.push_str(&format!(
            "e{}{:02}",
            if exponent < 0 { '-' } else { '+' },
            exponent.abs()
        ));
    } else if exponent < 0 {
        out.push_str("0.");
        for _ in 0..(-exponent - 1) {
            out.push('0');
        }
        out.push_str(trimmed);
    } else {
        let int_len = exponent as usize + 1;
        if trimmed.len() <= int_len {
            out.push_str(trimmed);
            for _ in trimmed.len()..int_len {
                out.push('0');
            }
        } else {
            out.push_str(&trimmed[..int_len]);
            out.push('.');
            out.push_str(&trimmed[int_len..]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ulps(a: f64, b: f64) -> u64 {
        if a == b {
            return 0;
        }
        (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs()
    }
    fn sweep() -> impl Iterator<Item = f64> {
        (0..4000).map(|i| {
            let t = f64::from(i) / 4000.0;
            (t * 2.0 - 1.0) * 40.0 + f64::from(i % 7) * 1e-3
        })
    }
    #[test]
    fn exp_and_ln_track_the_platform_within_an_ulp() {
        for x in sweep() {
            assert!(ulps(exp(x), x.exp()) <= 1, "exp({x})");
            let y = x.abs() * 3.7 + 1e-9;
            assert!(ulps(ln(y), y.ln()) <= 1, "ln({y})");
        }
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(1000.0), f64::INFINITY);
        assert_eq!(exp(-1000.0), 0.0);
        assert!(ln(-1.0).is_nan());
        assert_eq!(ln(0.0), f64::NEG_INFINITY);
        assert!(ulps(exp(-740.0), (-740.0f64).exp()) <= 1);
        assert!(ulps(exp(709.5), 709.5f64.exp()) <= 1);
        assert!(ulps(ln(5e-320), 5e-320f64.ln()) <= 1);
    }
    #[test]
    fn exact_powers_are_exact() {
        assert_eq!(pow(2.0, 10.0), 1024.0);
        assert_eq!(pow(10.0, 15.0), 1e15);
        assert_eq!(pow(10.0, -2.0), 0.01);
        assert_eq!(pow(-2.0, 3.0), -8.0);
        assert_eq!(pow(9.0, 0.5), 3.0);
        assert_eq!(pow(1.05, 0.0), 1.0);
        assert!(pow(-8.0, 1.0 / 3.0).is_nan());
        assert_eq!(pow(0.0, -1.0), f64::INFINITY);
        assert_eq!(log10(1000.0), 3.0);
        assert_eq!(log10(1e-5), -5.0);
        assert_eq!(log2(1024.0), 10.0);
        for (x, y) in [(1.004_166_666_666_666_6, 360.0), (1.07, 12.5), (3.3, -2.25)] {
            assert!(ulps(pow(x, y), x.powf(y)) <= 2, "pow({x},{y})");
        }
    }
    #[test]
    fn trigonometry_tracks_the_platform() {
        for x in sweep() {
            assert!(ulps(sin(x), x.sin()) <= 1, "sin({x})");
            assert!(ulps(cos(x), x.cos()) <= 1, "cos({x})");
            assert!(ulps(atan(x), x.atan()) <= 1, "atan({x})");
            assert!(ulps(atan2(x, 1.3), x.atan2(1.3)) <= 2, "atan2({x})");
            assert!(ulps(tan(x), x.tan()) <= 4, "tan({x})");
        }
        for i in -99..100 {
            let x = f64::from(i) / 100.0;
            assert!((asin(x) - x.asin()).abs() < 1e-15, "asin({x})");
            assert!((acos(x) - x.acos()).abs() < 1e-15, "acos({x})");
        }
        assert!(sin(1e10).is_nan());
    }
    #[test]
    fn significant_digits_print_like_printf_g() {
        assert_eq!(format_significant(0.1, 15), "0.1");
        assert_eq!(format_significant(1.0 / 3.0, 15), "0.333333333333333");
        assert_eq!(format_significant(1234.5, 15), "1234.5");
        assert_eq!(format_significant(1e20, 15), "1e+20");
        assert_eq!(format_significant(-2.5e-7, 15), "-2.5e-07");
        assert_eq!(format_significant(100.0, 15), "100");
        assert_eq!(format_significant(0.1 + 0.2, 15), "0.3");
        assert_eq!(format_significant(0.1 + 0.2, 17), "0.30000000000000004");
    }
}
