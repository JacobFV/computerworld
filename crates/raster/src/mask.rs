//! Selections: an 8-bit coverage per pixel. 255 is fully selected; the antialiased edge
//! of an ellipse or lasso is partial, as it is in every editor that feathers an edge.
//!
//! Geometry is in 1/16-pixel units ("sub16"), so a pointer landing between pixels at a
//! high zoom keeps its precision. Antialiased coverage takes a 4x4 grid of samples at
//! `16x + 4i + 2`; aliased coverage takes the single sample at the pixel centre.
use crate::{Canvas, IRect, Rgba};
use serde::{Deserialize, Serialize};

/// How a new selection combines with the one already there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectMode {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}
impl SelectMode {
    pub const ALL: [SelectMode; 4] = [Self::Replace, Self::Add, Self::Subtract, Self::Intersect];
    pub fn id(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Add => "add",
            Self::Subtract => "subtract",
            Self::Intersect => "intersect",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Mask {
    width: u32,
    height: u32,
    data: Vec<u8>,
}
impl std::fmt::Debug for Mask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Mask({}x{}, {:?})",
            self.width,
            self.height,
            self.bounds()
        )
    }
}

/// A point in sub16 units.
pub type P16 = (i64, i64);

/// Inside the ellipse inscribed in the sub16 box `(x0, y0)-(x1, y1)`.
pub fn in_ellipse(x0: i64, y0: i64, x1: i64, y1: i64, x: i64, y: i64) -> bool {
    let (rx2, ry2) = (i128::from(x1 - x0), i128::from(y1 - y0));
    if rx2 <= 0 || ry2 <= 0 {
        return false;
    }
    let dx = i128::from(2 * x - (x0 + x1));
    let dy = i128::from(2 * y - (y0 + y1));
    dx * dx * ry2 * ry2 + dy * dy * rx2 * rx2 <= rx2 * rx2 * ry2 * ry2
}

/// Even-odd point-in-polygon, exact in integers.
pub fn in_polygon(points: &[P16], x: i64, y: i64) -> bool {
    let n = points.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    for i in 0..n {
        let (ax, ay) = points[i];
        let (bx, by) = points[(i + 1) % n];
        if (ay > y) != (by > y) {
            let lhs = i128::from(x - ax) * i128::from(by - ay);
            let rhs = i128::from(y - ay) * i128::from(bx - ax);
            if (by > ay && lhs < rhs) || (by < ay && lhs > rhs) {
                inside = !inside;
            }
        }
    }
    inside
}

/// Within `radius` (sub16) of the segment `a`-`b`: a stroke with round caps.
pub fn near_segment(a: P16, b: P16, radius: i64, x: i64, y: i64) -> bool {
    let (dx, dy) = (i128::from(b.0 - a.0), i128::from(b.1 - a.1));
    let (px, py) = (i128::from(x - a.0), i128::from(y - a.1));
    let r2 = i128::from(radius) * i128::from(radius);
    let len2 = dx * dx + dy * dy;
    let dot = px * dx + py * dy;
    if len2 == 0 || dot <= 0 {
        return px * px + py * py <= r2;
    }
    if dot >= len2 {
        let (qx, qy) = (i128::from(x - b.0), i128::from(y - b.1));
        return qx * qx + qy * qy <= r2;
    }
    let cross = px * dy - py * dx;
    cross * cross <= r2 * len2
}

/// Coverage (0..=255) of pixel `(x, y)` under `inside`.
#[inline]
pub fn coverage(x: i32, y: i32, antialias: bool, inside: &impl Fn(i64, i64) -> bool) -> u8 {
    let (bx, by) = (i64::from(x) * 16, i64::from(y) * 16);
    if !antialias {
        return if inside(bx + 8, by + 8) { 255 } else { 0 };
    }
    let mut hits = 0u32;
    for j in 0..4 {
        for i in 0..4 {
            if inside(bx + 4 * i + 2, by + 4 * j + 2) {
                hits += 1;
            }
        }
    }
    ((hits * 255 + 8) / 16) as u8
}

/// The pixels a sub16 box can touch, clipped to the canvas.
pub fn sub16_bounds(x0: i64, y0: i64, x1: i64, y1: i64, w: u32, h: u32) -> Option<IRect> {
    let px0 = x0.min(x1).div_euclid(16) - 1;
    let py0 = y0.min(y1).div_euclid(16) - 1;
    let px1 = x0.max(x1).div_euclid(16) + 2;
    let py1 = y0.max(y1).div_euclid(16) + 2;
    let clampi = |v: i64| v.clamp(-1, i64::from(i32::MAX / 2)) as i32;
    IRect::new(
        clampi(px0),
        clampi(py0),
        (clampi(px1) - clampi(px0)).max(0) as u32,
        (clampi(py1) - clampi(py0)).max(0) as u32,
    )
    .clip(w, h)
}

fn similar(a: Rgba, b: Rgba, tolerance: u8) -> bool {
    if a[3] == 0 && b[3] == 0 {
        return true;
    }
    (0..4).all(|c| a[c].abs_diff(b[c]) <= tolerance)
}

impl Mask {
    pub fn empty(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; width as usize * height as usize],
        }
    }
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![255; width as usize * height as usize],
        }
    }
    pub fn from_data(width: u32, height: u32, data: Vec<u8>) -> Result<Self, String> {
        if data.len() != width as usize * height as usize {
            return Err("mask data does not match its size".into());
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 0;
        }
        self.data[y as usize * self.width as usize + x as usize]
    }
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, v: u8) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        self.data[y as usize * self.width as usize + x as usize] = v;
    }
    /// Raise the coverage at `(x, y)` to at least `v`.
    #[inline]
    pub fn raise(&mut self, x: i32, y: i32, v: u8) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = y as usize * self.width as usize + x as usize;
        self.data[i] = self.data[i].max(v);
    }
    /// Fill from a predicate over sub16 points, visiting only `area`.
    pub fn from_shape(
        width: u32,
        height: u32,
        area: Option<IRect>,
        antialias: bool,
        inside: impl Fn(i64, i64) -> bool,
    ) -> Self {
        let mut mask = Self::empty(width, height);
        if let Some(area) = area.and_then(|a| a.clip(width, height)) {
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    mask.set(x, y, coverage(x, y, antialias, &inside));
                }
            }
        }
        mask
    }
    /// Every pixel whose centre lies in the pixel rectangle `r`.
    pub fn rect(width: u32, height: u32, r: IRect) -> Self {
        let mut mask = Self::empty(width, height);
        if let Some(r) = r.clip(width, height) {
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    mask.set(x, y, 255);
                }
            }
        }
        mask
    }
    /// The antialiased ellipse inscribed in the pixel rectangle `r`.
    pub fn ellipse(width: u32, height: u32, r: IRect) -> Self {
        let (x0, y0) = (i64::from(r.x) * 16, i64::from(r.y) * 16);
        let (x1, y1) = (i64::from(r.right()) * 16, i64::from(r.bottom()) * 16);
        Self::from_shape(width, height, Some(r), true, |x, y| {
            in_ellipse(x0, y0, x1, y1, x, y)
        })
    }
    /// The antialiased polygon through sub16 `points` (a lasso).
    pub fn polygon(width: u32, height: u32, points: &[P16]) -> Self {
        if points.len() < 3 {
            return Self::empty(width, height);
        }
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for (x, y) in points {
            x0 = x0.min(*x);
            y0 = y0.min(*y);
            x1 = x1.max(*x);
            y1 = y1.max(*y);
        }
        let area = sub16_bounds(x0, y0, x1, y1, width, height);
        Self::from_shape(width, height, area, true, |x, y| in_polygon(points, x, y))
    }
    /// Magic wand: the pixels connected to `(x, y)` (4-connected) whose colour is within
    /// `tolerance` of the seed's on every channel. Transparent pixels match each other
    /// whatever colour they nominally hold.
    pub fn flood(canvas: &Canvas, x: i32, y: i32, tolerance: u8) -> Self {
        let (w, h) = (canvas.width(), canvas.height());
        let mut mask = Self::empty(w, h);
        if !canvas.bounds().contains(x, y) {
            return mask;
        }
        let seed = canvas.get(x, y);
        let mut stack = vec![(x, y)];
        while let Some((sx, sy)) = stack.pop() {
            if mask.get(sx, sy) != 0 || !similar(canvas.get(sx, sy), seed, tolerance) {
                continue;
            }
            // Walk the whole run on this row, then seed the rows above and below.
            let mut left = sx;
            while left > 0
                && mask.get(left - 1, sy) == 0
                && similar(canvas.get(left - 1, sy), seed, tolerance)
            {
                left -= 1;
            }
            let mut right = sx;
            while right + 1 < w as i32
                && mask.get(right + 1, sy) == 0
                && similar(canvas.get(right + 1, sy), seed, tolerance)
            {
                right += 1;
            }
            for cx in left..=right {
                mask.set(cx, sy, 255);
            }
            for ny in [sy - 1, sy + 1] {
                if ny < 0 || ny >= h as i32 {
                    continue;
                }
                let mut cx = left;
                while cx <= right {
                    if mask.get(cx, ny) == 0 && similar(canvas.get(cx, ny), seed, tolerance) {
                        stack.push((cx, ny));
                        // Skip the rest of this matching run; one seed covers it.
                        while cx <= right && similar(canvas.get(cx, ny), seed, tolerance) {
                            cx += 1;
                        }
                    } else {
                        cx += 1;
                    }
                }
            }
        }
        mask
    }
    pub fn is_empty(&self) -> bool {
        self.data.iter().all(|v| *v == 0)
    }
    pub fn invert(&mut self) {
        for v in &mut self.data {
            *v = 255 - *v;
        }
    }
    /// Combine `other` into this selection.
    pub fn combine(&mut self, other: &Mask, mode: SelectMode) {
        for (a, b) in self.data.iter_mut().zip(&other.data) {
            *a = match mode {
                SelectMode::Replace => *b,
                SelectMode::Add => (*a).max(*b),
                SelectMode::Subtract => {
                    crate::fmath::div255(u32::from(*a) * (255 - u32::from(*b))) as u8
                }
                SelectMode::Intersect => (*a).min(*b),
            };
        }
    }
    /// The tight box around every partly selected pixel.
    pub fn bounds(&self) -> Option<IRect> {
        let w = self.width as usize;
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for (i, v) in self.data.iter().enumerate() {
            if *v != 0 {
                let (x, y) = (i % w, i / w);
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        (x0 != usize::MAX).then(|| {
            IRect::new(
                x0 as i32,
                y0 as i32,
                (x1 - x0 + 1) as u32,
                (y1 - y0 + 1) as u32,
            )
        })
    }
    /// A copy restricted to `r`, for cropping.
    pub fn region(&self, r: IRect) -> Mask {
        let mut out = Mask::empty(r.w, r.h);
        for y in 0..r.h as i32 {
            for x in 0..r.w as i32 {
                out.set(x, y, self.get(r.x + x, r.y + y));
            }
        }
        out
    }
    /// Pixels of the selection's outline: selected pixels with an unselected
    /// 4-neighbour. What an interface draws as the marching ants.
    pub fn outline(&self) -> Vec<(i32, i32)> {
        let mut out = vec![];
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                if self.get(x, y) >= 128
                    && [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                        .iter()
                        .any(|(nx, ny)| self.get(*nx, *ny) < 128)
                {
                    out.push((x, y));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rectangles_ellipses_and_lassos_cover_exactly() {
        let r = Mask::rect(10, 10, IRect::new(2, 3, 4, 2));
        assert_eq!(r.bounds(), Some(IRect::new(2, 3, 4, 2)));
        assert_eq!(r.data().iter().filter(|v| **v == 255).count(), 8);
        let e = Mask::ellipse(20, 20, IRect::new(0, 0, 20, 20));
        assert_eq!(e.get(10, 10), 255);
        assert_eq!(e.get(0, 0), 0);
        // The edge is antialiased: partially covered, not all-or-nothing.
        let partial = e.data().iter().filter(|v| **v > 0 && **v < 255).count();
        assert!(partial > 20, "{partial}");
        // Symmetric in both axes.
        for y in 0..20 {
            for x in 0..20 {
                assert_eq!(e.get(x, y), e.get(19 - x, y));
                assert_eq!(e.get(x, y), e.get(x, 19 - y));
            }
        }
        // A right triangle on the pixel grid covers half of each diagonal pixel.
        let t = Mask::polygon(8, 8, &[(0, 0), (128, 0), (0, 128)]);
        assert_eq!(t.get(0, 0), 255);
        assert_eq!(t.get(7, 7), 0);
        assert_eq!(t.get(3, 4), 96, "the diagonal pixel is partly covered");
        let total: u32 = t.data().iter().map(|v| u32::from(*v)).sum();
        // Exactly half the 8x8 area, give or take the sampling of the diagonal.
        assert!((total as i32 - 32 * 255).abs() <= 255, "{total}");
    }
    #[test]
    fn selections_combine_invert_and_bound() {
        let mut a = Mask::rect(10, 1, IRect::new(0, 0, 6, 1));
        let b = Mask::rect(10, 1, IRect::new(4, 0, 6, 1));
        let mut add = a.clone();
        add.combine(&b, SelectMode::Add);
        assert_eq!(add.bounds(), Some(IRect::new(0, 0, 10, 1)));
        let mut sub = a.clone();
        sub.combine(&b, SelectMode::Subtract);
        assert_eq!(sub.bounds(), Some(IRect::new(0, 0, 4, 1)));
        let mut both = a.clone();
        both.combine(&b, SelectMode::Intersect);
        assert_eq!(both.bounds(), Some(IRect::new(4, 0, 2, 1)));
        a.invert();
        assert_eq!(a.bounds(), Some(IRect::new(6, 0, 4, 1)));
        assert!(Mask::empty(3, 3).is_empty());
        assert_eq!(Mask::empty(3, 3).bounds(), None);
        let outline = Mask::rect(5, 5, IRect::new(1, 1, 3, 3)).outline();
        assert_eq!(outline.len(), 8, "a 3x3 square has 8 edge pixels");
    }
    #[test]
    fn magic_wand_follows_connected_similar_colour() {
        let mut c = Canvas::filled(6, 3, [255, 255, 255, 255]);
        // A wall of red splits the canvas; a near-white pixel sits on the left.
        for y in 0..3 {
            c.set(3, y, [255, 0, 0, 255]);
        }
        c.set(1, 1, [250, 250, 250, 255]);
        let strict = Mask::flood(&c, 0, 0, 0);
        assert_eq!(strict.data().iter().filter(|v| **v == 255).count(), 8);
        assert_eq!(strict.get(1, 1), 0);
        let loose = Mask::flood(&c, 0, 0, 10);
        assert_eq!(loose.get(1, 1), 255);
        assert_eq!(loose.get(4, 0), 0, "the wall stops the fill");
        assert!(Mask::flood(&c, 9, 9, 0).is_empty());
        assert!(near_segment((0, 0), (160, 0), 16, 80, 16));
        assert!(!near_segment((0, 0), (160, 0), 16, 80, 17));
        assert!(near_segment((0, 0), (160, 0), 16, 170, 0));
        assert!(in_polygon(&[(0, 0), (10, 0), (10, 10), (0, 10)], 5, 5));
    }
}
