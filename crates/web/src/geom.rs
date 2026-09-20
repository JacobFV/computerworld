//! Fixed-point geometry. `Au` is an "app unit", 1/64 of a CSS pixel, stored in an
//! `i32`, as Gecko and Servo do. Layout uses nothing else, so every platform produces
//! the same positions.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Au(pub i32);

impl Au {
    pub const ZERO: Au = Au(0);
    pub const PER_PX: i32 = 64;
    pub const MAX: Au = Au(i32::MAX / 2);
    pub const MIN: Au = Au(i32::MIN / 2);

    pub const fn from_px_i32(px: i32) -> Au {
        Au(px.saturating_mul(Self::PER_PX))
    }
    /// From a decimal CSS number such as `12.5`. Rounds half away from zero. Used only
    /// when parsing literal values; layout arithmetic stays in `Au`.
    pub fn from_f64_px(px: f64) -> Au {
        let v = px * Self::PER_PX as f64;
        let r = if v >= 0.0 { (v + 0.5).floor() } else { (v - 0.5).ceil() };
        Au(r.clamp(Self::MIN.0 as f64, Self::MAX.0 as f64) as i32)
    }
    /// Nearest whole pixel, half away from zero.
    pub const fn to_px_round(self) -> i32 {
        let h = Self::PER_PX / 2;
        if self.0 >= 0 {
            (self.0 + h) / Self::PER_PX
        } else {
            -((-self.0 + h) / Self::PER_PX)
        }
    }
    pub const fn to_px_floor(self) -> i32 {
        self.0.div_euclid(Self::PER_PX)
    }
    pub const fn to_px_ceil(self) -> i32 {
        -((-self.0).div_euclid(Self::PER_PX))
    }
    pub fn to_f64_px(self) -> f64 {
        self.0 as f64 / Self::PER_PX as f64
    }
    pub fn min(self, o: Au) -> Au {
        if self <= o { self } else { o }
    }
    pub fn max(self, o: Au) -> Au {
        if self >= o { self } else { o }
    }
    pub fn clamp(self, lo: Au, hi: Au) -> Au {
        self.max(lo).min(hi)
    }
    pub fn abs(self) -> Au {
        Au(self.0.saturating_abs())
    }
    /// `self * num / den` in i64, rounded half away from zero; den != 0.
    pub fn scale(self, num: i32, den: i32) -> Au {
        debug_assert!(den != 0);
        let v = self.0 as i64 * num as i64;
        let d = den as i64;
        let r = if (v >= 0) == (d > 0) { (v + d.abs() / 2) / d } else { (v - d.abs() / 2) / d };
        Au(r.clamp(Self::MIN.0 as i64, Self::MAX.0 as i64) as i32)
    }
    /// A percentage (in 1/100 of a percent, so 50% is 5000) of a base length,
    /// truncated towards zero like Blink's `LayoutUnit(base * percent)`: a
    /// `10.638%` of 470px is 49.98px there and must not round up to 50.02px here, or
    /// Acid1's floats no longer fit side by side.
    pub fn percent_of(self, per_myriad: i32) -> Au {
        let v = self.0 as i64 * per_myriad as i64 / 10_000;
        Au(v.clamp(Self::MIN.0 as i64, Self::MAX.0 as i64) as i32)
    }
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl Add for Au {
    type Output = Au;
    fn add(self, o: Au) -> Au {
        Au(self.0.saturating_add(o.0))
    }
}
impl Sub for Au {
    type Output = Au;
    fn sub(self, o: Au) -> Au {
        Au(self.0.saturating_sub(o.0))
    }
}
impl AddAssign for Au {
    fn add_assign(&mut self, o: Au) {
        *self = *self + o;
    }
}
impl SubAssign for Au {
    fn sub_assign(&mut self, o: Au) {
        *self = *self - o;
    }
}
impl Neg for Au {
    type Output = Au;
    fn neg(self) -> Au {
        Au(self.0.saturating_neg())
    }
}
impl Mul<i32> for Au {
    type Output = Au;
    fn mul(self, k: i32) -> Au {
        Au(self.0.saturating_mul(k))
    }
}
impl Div<i32> for Au {
    type Output = Au;
    fn div(self, k: i32) -> Au {
        self.scale(1, k)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: Au,
    pub y: Au,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Size {
    pub width: Au,
    pub height: Au,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub fn new(x: Au, y: Au, width: Au, height: Au) -> Rect {
        Rect { origin: Point { x, y }, size: Size { width, height } }
    }
    pub fn right(&self) -> Au {
        self.origin.x + self.size.width
    }
    pub fn bottom(&self) -> Au {
        self.origin.y + self.size.height
    }
    pub fn translate(self, dx: Au, dy: Au) -> Rect {
        Rect { origin: Point { x: self.origin.x + dx, y: self.origin.y + dy }, size: self.size }
    }
    pub fn contains(&self, x: Au, y: Au) -> bool {
        x >= self.origin.x && x < self.right() && y >= self.origin.y && y < self.bottom()
    }
    pub fn intersection(self, o: Rect) -> Option<Rect> {
        let x0 = self.origin.x.max(o.origin.x);
        let y0 = self.origin.y.max(o.origin.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 > x0 && y1 > y0 { Some(Rect::new(x0, y0, x1 - x0, y1 - y0)) } else { None }
    }
    pub fn union(self, o: Rect) -> Rect {
        let x0 = self.origin.x.min(o.origin.x);
        let y0 = self.origin.y.min(o.origin.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
    /// Snap to whole device pixels for painting: edges rounded, so adjacent boxes stay
    /// adjacent.
    pub fn to_scene(self) -> cw_scene::Rect {
        let x0 = self.origin.x.to_px_round();
        let y0 = self.origin.y.to_px_round();
        let x1 = self.right().to_px_round();
        let y1 = self.bottom().to_px_round();
        cw_scene::Rect { x: x0, y: y0, width: (x1 - x0).max(0) as u32, height: (y1 - y0).max(0) as u32 }
    }
}

/// Four sides, in the CSS order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Edges {
    pub top: Au,
    pub right: Au,
    pub bottom: Au,
    pub left: Au,
}

impl Edges {
    pub const ZERO: Edges = Edges { top: Au::ZERO, right: Au::ZERO, bottom: Au::ZERO, left: Au::ZERO };
    pub fn uniform(v: Au) -> Edges {
        Edges { top: v, right: v, bottom: v, left: v }
    }
    pub fn horizontal(&self) -> Au {
        self.left + self.right
    }
    pub fn vertical(&self) -> Au {
        self.top + self.bottom
    }
    pub fn inset(&self, r: Rect) -> Rect {
        Rect::new(r.origin.x + self.left, r.origin.y + self.top, (r.size.width - self.horizontal()).max(Au::ZERO), (r.size.height - self.vertical()).max(Au::ZERO))
    }
}

impl Add for Edges {
    type Output = Edges;
    fn add(self, o: Edges) -> Edges {
        Edges { top: self.top + o.top, right: self.right + o.right, bottom: self.bottom + o.bottom, left: self.left + o.left }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_is_half_away_from_zero() {
        assert_eq!(Au(32).to_px_round(), 1);
        assert_eq!(Au(31).to_px_round(), 0);
        assert_eq!(Au(-32).to_px_round(), -1);
        assert_eq!(Au(-31).to_px_round(), 0);
        assert_eq!(Au::from_f64_px(0.5), Au(32));
        assert_eq!(Au::from_f64_px(-0.5), Au(-32));
        assert_eq!(Au::from_px_i32(3).to_px_floor(), 3);
        assert_eq!(Au(-1).to_px_floor(), -1);
        assert_eq!(Au(1).to_px_ceil(), 1);
    }

    #[test]
    fn scale_and_percent() {
        assert_eq!(Au::from_px_i32(100).percent_of(5000), Au::from_px_i32(50));
        assert_eq!(Au::from_px_i32(10).scale(1, 3), Au(213));
        assert_eq!(Au::from_px_i32(-10).scale(1, 3), Au(-213));
        assert_eq!(Au::MAX + Au::MAX + Au::MAX, Au(i32::MAX));
    }

    #[test]
    fn rect_edges_snap_together() {
        let a = Rect::new(Au(0), Au(0), Au(100), Au(100));
        let b = Rect::new(Au(100), Au(0), Au(100), Au(100));
        let (sa, sb) = (a.to_scene(), b.to_scene());
        assert_eq!(sa.x + sa.width as i32, sb.x);
        assert_eq!(a.union(b), Rect::new(Au(0), Au(0), Au(200), Au(100)));
        assert!(a.intersection(b).is_none());
    }
}
