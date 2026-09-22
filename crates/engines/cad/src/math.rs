//! Deterministic scalar and vector math.
//!
//! Everything a model depends on must come out bit-identical on x86_64, aarch64 and
//! wasm32. IEEE-754 guarantees that for `+ - * /` and `sqrt` (all correctly rounded,
//! and Rust never contracts `a * b + c` into a fused multiply-add on its own), but not
//! for the transcendental functions, whose `std` versions come from each platform's
//! libm. So this module carries its own: fdlibm's argument reduction and minimax
//! polynomials, evaluated with nothing but correctly rounded operations. `mul_add` is
//! never used anywhere in the kernel.
use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

pub const PI: f64 = std::f64::consts::PI;
pub const TAU: f64 = 2.0 * PI;
pub const FRAC_PI_2: f64 = std::f64::consts::FRAC_PI_2;
const FRAC_2_PI: f64 = std::f64::consts::FRAC_2_PI;

/// IEEE-754 square root: correctly rounded on every target, so `std`'s is already exact.
#[inline]
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

const PIO2_1: f64 = 1.570_796_326_734_125_6;
const PIO2_2: f64 = 6.077_100_506_303_966e-11;
const PIO2_3: f64 = 2.022_266_248_711_166_5e-21;

fn kernel_sin(x: f64) -> f64 {
    const S1: f64 = -1.666_666_666_666_663_2e-1;
    const S2: f64 = 8.333_333_333_322_49e-3;
    const S3: f64 = -1.984_126_982_985_795e-4;
    const S4: f64 = 2.755_731_370_707_006_8e-6;
    const S5: f64 = -2.505_076_025_340_686_3e-8;
    const S6: f64 = 1.589_690_995_211_55e-10;
    let z = x * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    x + x * z * (S1 + z * r)
}
fn kernel_cos(x: f64) -> f64 {
    const C1: f64 = 4.166_666_666_666_66e-2;
    const C2: f64 = -1.388_888_888_887_411e-3;
    const C3: f64 = 2.480_158_728_947_673e-5;
    const C4: f64 = -2.755_731_435_139_066_3e-7;
    const C5: f64 = 2.087_572_321_298_175e-9;
    const C6: f64 = -1.135_964_755_778_819_5e-11;
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    let hz = 0.5 * z;
    let w = 1.0 - hz;
    w + (((1.0 - w) - hz) + z * r)
}

/// Sine and cosine together, from one argument reduction.
pub fn sin_cos(x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (f64::NAN, f64::NAN);
    }
    if x.abs() <= PI / 4.0 {
        return (kernel_sin(x), kernel_cos(x));
    }
    let k = (x * FRAC_2_PI).round();
    let r = ((x - k * PIO2_1) - k * PIO2_2) - k * PIO2_3;
    let (s, c) = (kernel_sin(r), kernel_cos(r));
    match (k.rem_euclid(4.0)) as i32 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}
pub fn sin(x: f64) -> f64 {
    sin_cos(x).0
}
pub fn cos(x: f64) -> f64 {
    sin_cos(x).1
}
pub fn tan(x: f64) -> f64 {
    let (s, c) = sin_cos(x);
    s / c
}

/// Arctangent, fdlibm's reduction into four intervals and an odd polynomial.
pub fn atan(x: f64) -> f64 {
    const ATANHI: [f64; 4] = [
        4.636_476_090_008_061e-1,
        std::f64::consts::FRAC_PI_4,
        9.827_937_232_473_29e-1,
        std::f64::consts::FRAC_PI_2,
    ];
    const ATANLO: [f64; 4] = [
        2.269_877_745_296_168_7e-17,
        3.061_616_997_868_383e-17,
        1.390_331_103_123_099_8e-17,
        6.123_233_995_736_766e-17,
    ];
    const AT: [f64; 11] = [
        3.333_333_333_333_293e-1,
        -1.999_999_999_987_648_3e-1,
        1.428_571_427_250_346_6e-1,
        -1.111_111_040_546_235_6e-1,
        9.090_887_133_436_507e-2,
        -7.691_876_205_044_83e-2,
        6.661_073_137_387_531e-2,
        -5.833_570_133_790_573_5e-2,
        4.976_877_994_615_932_4e-2,
        -3.653_157_274_421_691_6e-2,
        1.628_582_011_536_578_2e-2,
    ];
    if x.is_nan() {
        return x;
    }
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let mut t = x.abs();
    if t >= 7.378_697_629_483_821e19 {
        return sign * (ATANHI[3] + ATANLO[3]);
    }
    let id: i32 = if t < 0.4375 {
        if t < 1e-29 {
            return x;
        }
        -1
    } else if t < 1.1875 {
        if t < 0.6875 {
            t = (2.0 * t - 1.0) / (2.0 + t);
            0
        } else {
            t = (t - 1.0) / (t + 1.0);
            1
        }
    } else if t < 2.4375 {
        t = (t - 1.5) / (1.0 + 1.5 * t);
        2
    } else {
        t = -1.0 / t;
        3
    };
    let z = t * t;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    if id < 0 {
        return sign * (t - t * (s1 + s2));
    }
    let i = id as usize;
    sign * (ATANHI[i] - ((t * (s1 + s2) - ATANLO[i]) - t))
}

/// Four-quadrant arctangent in `(-π, π]`.
pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return if y > 0.0 {
            FRAC_PI_2
        } else if y < 0.0 {
            -FRAC_PI_2
        } else {
            0.0
        };
    }
    if y == 0.0 {
        return if x > 0.0 { 0.0 } else { PI };
    }
    let a = atan((y / x).abs());
    match (x > 0.0, y > 0.0) {
        (true, true) => a,
        (true, false) => -a,
        (false, true) => PI - a,
        (false, false) => a - PI,
    }
}
pub fn acos(x: f64) -> f64 {
    let x = x.clamp(-1.0, 1.0);
    atan2(sqrt((1.0 - x) * (1.0 + x)), x)
}
pub fn asin(x: f64) -> f64 {
    let x = x.clamp(-1.0, 1.0);
    atan2(x, sqrt((1.0 - x) * (1.0 + x)))
}
pub fn hypot(x: f64, y: f64) -> f64 {
    sqrt(x * x + y * y)
}
pub fn degrees(radians: f64) -> f64 {
    radians * (180.0 / PI)
}
pub fn radians(degrees: f64) -> f64 {
    degrees * (PI / 180.0)
}
/// Angle wrapped into `(-π, π]`.
pub fn wrap_angle(a: f64) -> f64 {
    let mut a = a % TAU;
    if a <= -PI {
        a += TAU;
    } else if a > PI {
        a -= TAU;
    }
    a
}
/// Angle wrapped into `[0, 2π)`.
pub fn wrap_positive(a: f64) -> f64 {
    let a = a % TAU;
    if a < 0.0 {
        a + TAU
    } else {
        a
    }
}

/// A number printed the way a CAD front end shows a length: up to `decimals`, with
/// trailing zeros dropped, and never `-0`.
pub fn fmt_num(v: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, v);
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    };
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}
pub const fn v2(x: f64, y: f64) -> V2 {
    V2 { x, y }
}
impl V2 {
    pub const ZERO: V2 = v2(0.0, 0.0);
    pub fn dot(self, o: V2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: V2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn len(self) -> f64 {
        sqrt(self.dot(self))
    }
    pub fn norm(self) -> V2 {
        let l = self.len();
        if l > 0.0 {
            self / l
        } else {
            self
        }
    }
    pub fn perp(self) -> V2 {
        v2(-self.y, self.x)
    }
    pub fn dist(self, o: V2) -> f64 {
        (self - o).len()
    }
    pub fn angle(self) -> f64 {
        atan2(self.y, self.x)
    }
    pub fn polar(angle: f64, r: f64) -> V2 {
        let (s, c) = sin_cos(angle);
        v2(c * r, s * r)
    }
    pub fn lerp(self, o: V2, t: f64) -> V2 {
        self + (o - self) * t
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}
impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        v2(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        v2(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for V2 {
    type Output = V2;
    fn mul(self, s: f64) -> V2 {
        v2(self.x * s, self.y * s)
    }
}
impl Div<f64> for V2 {
    type Output = V2;
    fn div(self, s: f64) -> V2 {
        v2(self.x / s, self.y / s)
    }
}
impl Neg for V2 {
    type Output = V2;
    fn neg(self) -> V2 {
        v2(-self.x, -self.y)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}
impl V3 {
    pub const ZERO: V3 = v3(0.0, 0.0, 0.0);
    pub const X: V3 = v3(1.0, 0.0, 0.0);
    pub const Y: V3 = v3(0.0, 1.0, 0.0);
    pub const Z: V3 = v3(0.0, 0.0, 1.0);
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn len(self) -> f64 {
        sqrt(self.dot(self))
    }
    pub fn len2(self) -> f64 {
        self.dot(self)
    }
    pub fn norm(self) -> V3 {
        let l = self.len();
        if l > 0.0 {
            self / l
        } else {
            self
        }
    }
    pub fn dist(self, o: V3) -> f64 {
        (self - o).len()
    }
    pub fn lerp(self, o: V3, t: f64) -> V3 {
        self + (o - self) * t
    }
    pub fn min(self, o: V3) -> V3 {
        v3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    pub fn max(self, o: V3) -> V3 {
        v3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    /// Any unit vector perpendicular to this one, chosen deterministically.
    pub fn any_perp(self) -> V3 {
        let a = if self.x.abs() < 0.9 { V3::X } else { V3::Y };
        self.cross(a).norm()
    }
    pub fn get(self, axis: usize) -> f64 {
        match axis {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
}
impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}
impl SubAssign for V3 {
    fn sub_assign(&mut self, o: V3) {
        *self = *self - o;
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, s: f64) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Div<f64> for V3 {
    type Output = V3;
    fn div(self, s: f64) -> V3 {
        v3(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

/// Rigid placement: an origin and a right-handed orthonormal basis. Sketches live in the
/// plane spanned by `x` and `y`; `z` is the plane's normal.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub origin: V3,
    pub x: V3,
    pub y: V3,
    pub z: V3,
}
impl Default for Frame {
    fn default() -> Self {
        Self::XY
    }
}
impl Frame {
    pub const XY: Frame = Frame {
        origin: V3::ZERO,
        x: V3::X,
        y: V3::Y,
        z: V3::Z,
    };
    /// FreeCAD's XZ_Plane: sketch x along global X, sketch y along global Z, normal -Y.
    pub const XZ: Frame = Frame {
        origin: V3::ZERO,
        x: V3::X,
        y: V3::Z,
        z: v3(0.0, -1.0, 0.0),
    };
    /// FreeCAD's YZ_Plane: sketch x along global Y, sketch y along global Z, normal +X.
    pub const YZ: Frame = Frame {
        origin: V3::ZERO,
        x: V3::Y,
        y: V3::Z,
        z: V3::X,
    };
    /// A frame from an origin, a normal and a preferred in-plane x direction.
    pub fn from_normal(origin: V3, normal: V3, x_hint: V3) -> Frame {
        let z = normal.norm();
        let mut x = x_hint - z * x_hint.dot(z);
        if x.len() < 1e-9 {
            x = z.any_perp();
        }
        let x = x.norm();
        let y = z.cross(x).norm();
        Frame { origin, x, y, z }
    }
    pub fn to_world(&self, p: V2) -> V3 {
        self.origin + self.x * p.x + self.y * p.y
    }
    pub fn point(&self, p: V3) -> V3 {
        self.origin + self.x * p.x + self.y * p.y + self.z * p.z
    }
    pub fn dir(&self, d: V3) -> V3 {
        self.x * d.x + self.y * d.y + self.z * d.z
    }
    pub fn to_local(&self, p: V3) -> V3 {
        let d = p - self.origin;
        v3(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }
    pub fn to_local2(&self, p: V3) -> V2 {
        let d = p - self.origin;
        v2(d.dot(self.x), d.dot(self.y))
    }
    pub fn offset(&self, along_normal: f64) -> Frame {
        Frame {
            origin: self.origin + self.z * along_normal,
            ..*self
        }
    }
}

/// Affine transform: a 3x3 linear part (rows) and a translation. Used for mirrors and
/// pattern instances, so it may carry a reflection.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Xform {
    pub m: [[f64; 3]; 3],
    pub t: V3,
}
impl Xform {
    pub const IDENTITY: Xform = Xform {
        m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        t: V3::ZERO,
    };
    pub fn translate(t: V3) -> Xform {
        Xform {
            t,
            ..Self::IDENTITY
        }
    }
    /// Rotation by `angle` about the axis through `origin` along `axis` (Rodrigues).
    pub fn rotate(origin: V3, axis: V3, angle: f64) -> Xform {
        let k = axis.norm();
        let (s, c) = sin_cos(angle);
        let t = 1.0 - c;
        let m = [
            [
                t * k.x * k.x + c,
                t * k.x * k.y - s * k.z,
                t * k.x * k.z + s * k.y,
            ],
            [
                t * k.x * k.y + s * k.z,
                t * k.y * k.y + c,
                t * k.y * k.z - s * k.x,
            ],
            [
                t * k.x * k.z - s * k.y,
                t * k.y * k.z + s * k.x,
                t * k.z * k.z + c,
            ],
        ];
        let lin = Xform { m, t: V3::ZERO };
        Xform {
            m,
            t: origin - lin.dir(origin),
        }
    }
    /// Reflection in the plane through `origin` with `normal`.
    pub fn mirror(origin: V3, normal: V3) -> Xform {
        let n = normal.norm();
        let m = [
            [1.0 - 2.0 * n.x * n.x, -2.0 * n.x * n.y, -2.0 * n.x * n.z],
            [-2.0 * n.x * n.y, 1.0 - 2.0 * n.y * n.y, -2.0 * n.y * n.z],
            [-2.0 * n.x * n.z, -2.0 * n.y * n.z, 1.0 - 2.0 * n.z * n.z],
        ];
        let lin = Xform { m, t: V3::ZERO };
        Xform {
            m,
            t: origin - lin.dir(origin),
        }
    }
    pub fn dir(&self, d: V3) -> V3 {
        let m = &self.m;
        v3(
            m[0][0] * d.x + m[0][1] * d.y + m[0][2] * d.z,
            m[1][0] * d.x + m[1][1] * d.y + m[1][2] * d.z,
            m[2][0] * d.x + m[2][1] * d.y + m[2][2] * d.z,
        )
    }
    pub fn point(&self, p: V3) -> V3 {
        self.dir(p) + self.t
    }
    pub fn det(&self) -> f64 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
    /// `self` after `first`: the transform that applies `first`, then `self`.
    pub fn after(&self, first: &Xform) -> Xform {
        let mut m = [[0.0; 3]; 3];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = self.m[i][0] * first.m[0][j]
                    + self.m[i][1] * first.m[1][j]
                    + self.m[i][2] * first.m[2][j];
            }
        }
        Xform {
            m,
            t: self.point(first.t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trig_matches_the_platform_to_an_ulp_or_two() {
        let mut worst: f64 = 0.0;
        for i in -2000..=2000 {
            let x = i as f64 * 0.013_7;
            let (s, c) = sin_cos(x);
            worst = worst.max((s - x.sin()).abs()).max((c - x.cos()).abs());
            worst = worst.max((atan(x) - x.atan()).abs());
            let y = (i as f64 * 0.37).sin() * 3.0;
            worst = worst.max((atan2(y, x) - y.atan2(x)).abs());
        }
        assert!(worst < 1e-15, "worst error {worst}");
        for i in -100..=100 {
            let x = i as f64 / 100.0;
            assert!((acos(x) - x.acos()).abs() < 1e-14);
            assert!((asin(x) - x.asin()).abs() < 1e-14);
        }
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        assert!((sin(PI / 6.0) - 0.5).abs() < 1e-16);
    }
    #[test]
    fn transforms_compose_and_reflect() {
        let r = Xform::rotate(v3(1.0, 0.0, 0.0), V3::Z, FRAC_PI_2);
        let p = r.point(v3(2.0, 0.0, 0.0));
        assert!((p - v3(1.0, 1.0, 0.0)).len() < 1e-12);
        let m = Xform::mirror(V3::ZERO, V3::X);
        assert!((m.det() + 1.0).abs() < 1e-12);
        assert!((m.point(v3(3.0, 2.0, 1.0)) - v3(-3.0, 2.0, 1.0)).len() < 1e-12);
        let both = m.after(&r);
        assert!((both.point(v3(2.0, 0.0, 0.0)) - v3(-1.0, 1.0, 0.0)).len() < 1e-12);
    }
    #[test]
    fn numbers_print_without_noise() {
        assert_eq!(fmt_num(10.0, 2), "10");
        assert_eq!(fmt_num(2.5, 2), "2.5");
        assert_eq!(fmt_num(-0.0001, 2), "0");
        assert_eq!(fmt_num(1.23456, 3), "1.235");
    }
}
