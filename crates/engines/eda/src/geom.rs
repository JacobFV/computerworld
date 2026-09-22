//! Integer geometry. Schematic points are mils; board points are nanometres. Nothing
//! here rounds a coordinate, so a design saved and loaded is the design that was drawn.
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Pt {
    pub x: i64,
    pub y: i64,
}
// `add`/`sub` are named methods so call sites read `a.add(b)` without importing
// `std::ops`; they are exactly vector addition and subtraction.
#[allow(clippy::should_implement_trait)]
impl Pt {
    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }
    pub fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
    pub fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
    /// Snap to a grid of `step`, rounding to the nearest line.
    pub fn snap(self, step: i64) -> Pt {
        let s = |v: i64| (v + step / 2).div_euclid(step) * step;
        Pt::new(s(self.x), s(self.y))
    }
    pub fn dist(self, o: Pt) -> f64 {
        let dx = (self.x - o.x) as f64;
        let dy = (self.y - o.y) as f64;
        (dx * dx + dy * dy).sqrt()
    }
    pub fn manhattan(self, o: Pt) -> i64 {
        (self.x - o.x).abs() + (self.y - o.y).abs()
    }
}

/// A 2×2 integer transform with entries in {-1, 0, 1}: the eight orientations a symbol
/// or footprint may take. Applied to a local offset it gives a page/board offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Xf(pub [i8; 4]);
impl Default for Xf {
    fn default() -> Self {
        Self::IDENTITY
    }
}
impl Xf {
    pub const IDENTITY: Xf = Xf([1, 0, 0, 1]);
    pub fn apply(self, p: Pt) -> Pt {
        let [a, b, c, d] = self.0;
        Pt::new(
            a as i64 * p.x + b as i64 * p.y,
            c as i64 * p.x + d as i64 * p.y,
        )
    }
    /// Apply `self`, then `next`.
    pub fn then(self, next: Xf) -> Xf {
        let [a, b, c, d] = self.0;
        let [e, f, g, h] = next.0;
        Xf([e * a + f * c, e * b + f * d, g * a + h * c, g * b + h * d])
    }
    /// Rotate 90° counter-clockwise as seen on a Y-down screen.
    pub const ROT_CCW: Xf = Xf([0, 1, -1, 0]);
    /// Mirror about the horizontal axis (flip vertically).
    pub const MIRROR_X: Xf = Xf([1, 0, 0, -1]);
    /// Mirror about the vertical axis (flip horizontally).
    pub const MIRROR_Y: Xf = Xf([-1, 0, 0, 1]);
    pub fn rotation(quarter_turns_ccw: u8) -> Xf {
        let mut x = Self::IDENTITY;
        for _ in 0..quarter_turns_ccw % 4 {
            x = x.then(Self::ROT_CCW);
        }
        x
    }
    /// Decompose into KiCad's `(at … angle)` plus an optional `(mirror x|y)`, applied as
    /// rotation first, then mirror.
    pub fn decompose(self) -> (u16, Option<char>) {
        for q in 0..4u8 {
            for (m, mx) in [
                (None, Self::IDENTITY),
                (Some('x'), Self::MIRROR_X),
                (Some('y'), Self::MIRROR_Y),
            ] {
                if Self::rotation(q).then(mx) == self {
                    return (q as u16 * 90, m);
                }
            }
        }
        (0, None)
    }
    pub fn compose(angle: i64, mirror: Option<char>) -> Xf {
        let q = (angle.rem_euclid(360) / 90) as u8;
        let r = Self::rotation(q);
        match mirror {
            Some('x') => r.then(Self::MIRROR_X),
            Some('y') => r.then(Self::MIRROR_Y),
            _ => r,
        }
    }
}

/// Axis-aligned rectangle, inclusive of its edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub min: Pt,
    pub max: Pt,
}
impl Rect {
    pub fn new(a: Pt, b: Pt) -> Self {
        Self {
            min: Pt::new(a.x.min(b.x), a.y.min(b.y)),
            max: Pt::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }
    pub fn around(c: Pt, half_w: i64, half_h: i64) -> Self {
        Self::new(
            Pt::new(c.x - half_w, c.y - half_h),
            Pt::new(c.x + half_w, c.y + half_h),
        )
    }
    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            min: Pt::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            max: Pt::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        }
    }
    pub fn inflate(&self, d: i64) -> Rect {
        Rect {
            min: Pt::new(self.min.x - d, self.min.y - d),
            max: Pt::new(self.max.x + d, self.max.y + d),
        }
    }
    /// Strictly overlapping interiors (touching edges do not count).
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.min.x < o.max.x && o.min.x < self.max.x && self.min.y < o.max.y && o.min.y < self.max.y
    }
    pub fn width(&self) -> i64 {
        self.max.x - self.min.x
    }
    pub fn height(&self) -> i64 {
        self.max.y - self.min.y
    }
    pub fn center(&self) -> Pt {
        Pt::new((self.min.x + self.max.x) / 2, (self.min.y + self.max.y) / 2)
    }
}

/// Whether `p` lies on segment a–b (inclusive of the ends).
pub fn on_segment(p: Pt, a: Pt, b: Pt) -> bool {
    let cross =
        (b.x - a.x) as i128 * (p.y - a.y) as i128 - (b.y - a.y) as i128 * (p.x - a.x) as i128;
    cross == 0
        && p.x >= a.x.min(b.x)
        && p.x <= a.x.max(b.x)
        && p.y >= a.y.min(b.y)
        && p.y <= a.y.max(b.y)
}

/// Distance from point to segment.
pub fn point_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (px, py) = (p.x as f64, p.y as f64);
    let (ax, ay) = (a.x as f64, a.y as f64);
    let (bx, by) = (b.x as f64, b.y as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt()
}

fn orient(a: Pt, b: Pt, c: Pt) -> i128 {
    let v = (b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128;
    v.signum()
}
/// Whether two closed segments share a point.
pub fn segments_intersect(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if o1 != o2 && o3 != o4 {
        return true;
    }
    (o1 == 0 && on_segment(c, a, b))
        || (o2 == 0 && on_segment(d, a, b))
        || (o3 == 0 && on_segment(a, c, d))
        || (o4 == 0 && on_segment(b, c, d))
}
pub fn segment_segment(a: Pt, b: Pt, c: Pt, d: Pt) -> f64 {
    if segments_intersect(a, b, c, d) {
        return 0.0;
    }
    point_segment(a, c, d)
        .min(point_segment(b, c, d))
        .min(point_segment(c, a, b))
        .min(point_segment(d, a, b))
}
/// Distance from a segment to a rectangle (0 when they touch or overlap).
pub fn segment_rect(a: Pt, b: Pt, r: &Rect) -> f64 {
    if r.contains(a) || r.contains(b) {
        return 0.0;
    }
    let corners = [
        r.min,
        Pt::new(r.max.x, r.min.y),
        r.max,
        Pt::new(r.min.x, r.max.y),
    ];
    let mut best = f64::INFINITY;
    for i in 0..4 {
        best = best.min(segment_segment(a, b, corners[i], corners[(i + 1) % 4]));
    }
    best
}
pub fn rect_rect(a: &Rect, b: &Rect) -> f64 {
    let dx = (b.min.x - a.max.x).max(a.min.x - b.max.x).max(0) as f64;
    let dy = (b.min.y - a.max.y).max(a.min.y - b.max.y).max(0) as f64;
    (dx * dx + dy * dy).sqrt()
}

/// Point in polygon by the even–odd rule; points on an edge count as inside.
pub fn in_polygon(p: Pt, poly: &[Pt]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if on_segment(p, a, b) {
            return true;
        }
        if (a.y > p.y) != (b.y > p.y) {
            // x of the edge at p.y, compared exactly with 128-bit arithmetic.
            let lhs = (p.x - a.x) as i128 * (b.y - a.y) as i128;
            let rhs = (b.x - a.x) as i128 * (p.y - a.y) as i128;
            let crosses = if b.y > a.y { lhs < rhs } else { lhs > rhs };
            if crosses {
                inside = !inside;
            }
        }
    }
    inside
}
/// Distance from a point to a polygon's boundary.
pub fn polygon_edge_distance(p: Pt, poly: &[Pt]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| point_segment(p, poly[i], poly[(i + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}

/// Format an integer number of `unit`-per-millimetre as a millimetre decimal, trimmed:
/// `mm(1_270_000, 1_000_000)` is "1.27".
pub fn mm(value: i64, per_mm: i64) -> String {
    let neg = value < 0;
    let v = value.unsigned_abs();
    let per = per_mm as u64;
    let whole = v / per;
    let mut frac = v % per;
    let mut digits = 0;
    let mut p = per;
    while p > 1 {
        p /= 10;
        digits += 1;
    }
    let mut s = format!("{}{whole}", if neg && v != 0 { "-" } else { "" });
    if frac != 0 {
        let mut f = format!("{frac:0digits$}");
        while f.ends_with('0') {
            f.pop();
        }
        s.push('.');
        s.push_str(&f);
        frac = 0;
    }
    let _ = frac;
    s
}
/// Parse a millimetre decimal exactly into integer units of `1/per_mm` mm, rounding the
/// digits beyond the unit to nearest.
pub fn parse_mm(text: &str, per_mm: i64) -> Option<i64> {
    let t = text.trim();
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (whole, frac) = t.split_once('.').unwrap_or((t, ""));
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let w: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut units = w.checked_mul(per_mm)?;
    let mut scale = per_mm;
    let mut rest = 0i64;
    for (i, ch) in frac.chars().enumerate() {
        let d = ch as i64 - '0' as i64;
        if scale >= 10 {
            scale /= 10;
            units += d * scale;
        } else {
            if i < 30 && rest == 0 {
                rest = d;
            }
            break;
        }
    }
    if rest >= 5 {
        units += 1;
    }
    Some(if neg { -units } else { units })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn orientations_compose_and_decompose() {
        for q in 0..4 {
            for m in [None, Some('x'), Some('y')] {
                let x = Xf::compose(q as i64 * 90, m);
                let (angle, mirror) = x.decompose();
                assert_eq!(Xf::compose(angle as i64, mirror), x);
            }
        }
        // A CCW quarter turn takes "right" to "up" on a Y-down page.
        assert_eq!(Xf::ROT_CCW.apply(Pt::new(1, 0)), Pt::new(0, -1));
        assert_eq!(Xf::rotation(4), Xf::IDENTITY);
    }
    #[test]
    fn millimetres_format_and_parse_exactly() {
        assert_eq!(mm(1_270_000, 1_000_000), "1.27");
        assert_eq!(mm(-500_000, 1_000_000), "-0.5");
        assert_eq!(mm(25_400_000, 1_000_000), "25.4");
        assert_eq!(parse_mm("1.27", 1_000_000), Some(1_270_000));
        assert_eq!(parse_mm("-0.0005", 1_000_000), Some(-500));
        assert_eq!(parse_mm("2.54", 10_000), Some(25_400));
        assert_eq!(parse_mm("x", 10), None);
    }
    #[test]
    fn distances_and_containment() {
        assert_eq!(
            segment_segment(Pt::new(0, 0), Pt::new(10, 0), Pt::new(5, -5), Pt::new(5, 5)),
            0.0
        );
        assert_eq!(
            point_segment(Pt::new(5, 3), Pt::new(0, 0), Pt::new(10, 0)),
            3.0
        );
        let r = Rect::new(Pt::new(0, 0), Pt::new(10, 10));
        assert_eq!(segment_rect(Pt::new(13, 0), Pt::new(13, 10), &r), 3.0);
        let sq = [
            Pt::new(0, 0),
            Pt::new(10, 0),
            Pt::new(10, 10),
            Pt::new(0, 10),
        ];
        assert!(in_polygon(Pt::new(5, 5), &sq));
        assert!(in_polygon(Pt::new(10, 5), &sq));
        assert!(!in_polygon(Pt::new(11, 5), &sq));
    }
}
