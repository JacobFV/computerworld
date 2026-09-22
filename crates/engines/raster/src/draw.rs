//! Painting: brush dabs and strokes, shapes, flood fill and mask stamping. Everything
//! paints through an optional selection, and returns the rectangle it touched so the
//! document can record exactly that much for undo.
use crate::blend::{self, BlendMode};
use crate::fmath::{div255, isqrt};
use crate::mask::{coverage, in_ellipse, in_polygon, near_segment, sub16_bounds, Mask, P16};
use crate::{Canvas, IRect, Rgba};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushKind {
    #[default]
    Paint,
    /// Removes alpha instead of adding colour.
    Erase,
}

/// A round brush tip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brush {
    pub kind: BrushKind,
    pub color: Rgba,
    /// Diameter in pixels.
    pub size: u32,
    /// 100 is a hard edge; 0 falls off linearly from the centre.
    pub hardness: u8,
    /// Ceiling on what one stroke can add, 0..=255. Overlapping dabs never exceed it.
    pub opacity: u8,
    /// False for a pencil: whole pixels, no partial coverage.
    pub antialias: bool,
    /// How the stroke composites onto the layer (a highlighter multiplies).
    pub blend: BlendMode,
    /// Distance between dabs as a percentage of the diameter.
    pub spacing: u8,
}
impl Default for Brush {
    fn default() -> Self {
        Self {
            kind: BrushKind::Paint,
            color: crate::BLACK,
            size: 5,
            hardness: 100,
            opacity: 255,
            antialias: true,
            blend: BlendMode::Normal,
            spacing: 20,
        }
    }
}
impl Brush {
    pub const MAX_SIZE: u32 = 1000;
    fn radius16(&self) -> i64 {
        i64::from(self.size.clamp(1, Self::MAX_SIZE)) * 8
    }
    /// Distance between dabs in sub16 units, never less than a pixel.
    pub fn spacing16(&self) -> i64 {
        (i64::from(self.size.clamp(1, Self::MAX_SIZE)) * 16 * i64::from(self.spacing.max(1)) / 100)
            .max(16)
    }
    /// Coverage of one dab centred at `(cx, cy)` (sub16) on pixel `(x, y)`.
    #[inline]
    pub fn dab_coverage(&self, cx: i64, cy: i64, x: i32, y: i32) -> u8 {
        let (px, py) = (i64::from(x) * 16 + 8, i64::from(y) * 16 + 8);
        let r = self.radius16();
        if !self.antialias {
            // A pencil dab is centred on a pixel, so a one-pixel pencil is one pixel.
            let (sx, sy) = (cx.div_euclid(16) * 16 + 8, cy.div_euclid(16) * 16 + 8);
            let (dx, dy) = (px - sx, py - sy);
            return if dx * dx + dy * dy <= r * r { 255 } else { 0 };
        }
        let (dx, dy) = (px - cx, py - cy);
        let d = isqrt((dx * dx + dy * dy) as u64) as i64;
        let outer = r + 8;
        let inner = (r * i64::from(self.hardness.min(100)) / 100)
            .min(r - 8)
            .max(0);
        if d <= inner {
            255
        } else if d >= outer {
            0
        } else {
            (255 * (outer - d) / (outer - inner)) as u8
        }
    }
    /// Pixels one dab at `(cx, cy)` can touch.
    pub fn dab_bounds(&self, cx: i64, cy: i64) -> IRect {
        let r = self.radius16() + 16;
        let x0 = (cx - r).div_euclid(16) as i32;
        let y0 = (cy - r).div_euclid(16) as i32;
        let x1 = (cx + r).div_euclid(16) as i32 + 1;
        let y1 = (cy + r).div_euclid(16) as i32 + 1;
        IRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }
    /// Stamp one dab into a stroke's coverage mask (maximum, never accumulating).
    pub fn dab(&self, mask: &mut Mask, cx: i64, cy: i64) -> Option<IRect> {
        let r = self.dab_bounds(cx, cy).clip(mask.width(), mask.height())?;
        let mut hit: Option<IRect> = None;
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let c = self.dab_coverage(cx, cy, x, y);
                if c > 0 {
                    mask.raise(x, y, c);
                    let p = IRect::new(x, y, 1, 1);
                    hit = Some(hit.map_or(p, |h| h.union(&p)));
                }
            }
        }
        hit
    }
}

/// Dab positions along `from`-`to`, carrying the distance travelled since the last
/// dab so spacing stays even across pointer events. Returns the positions and the new
/// carry.
pub fn dabs_along(from: P16, to: P16, spacing: i64, carry: i64) -> (Vec<P16>, i64) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = isqrt((dx * dx + dy * dy) as u64) as i64;
    if len == 0 {
        return (vec![], carry);
    }
    let mut out = vec![];
    let mut t = spacing - carry;
    let mut last = 0;
    while t <= len {
        out.push((from.0 + dx * t / len, from.1 + dy * t / len));
        last = t;
        t += spacing;
    }
    let carry = if out.is_empty() {
        carry + len
    } else {
        len - last
    };
    (out, carry)
}

/// Write `color` through a coverage function into `layer`, restricted by `selection`,
/// over the rectangle `area`. `Erase` in `color`'s place removes alpha.
#[allow(clippy::too_many_arguments)]
pub fn apply_coverage(
    layer: &mut Canvas,
    before: Option<&Canvas>,
    area: IRect,
    selection: Option<&Mask>,
    brush_kind: BrushKind,
    color: Rgba,
    opacity: u8,
    mode: BlendMode,
    cov: impl Fn(i32, i32) -> u8,
) -> Option<IRect> {
    apply_colors(
        layer,
        before,
        area,
        selection,
        brush_kind,
        opacity,
        mode,
        |x, y| (cov(x, y), color),
    )
}

/// Like [`apply_coverage`], with a colour of its own at every pixel: a gradient, or the
/// pixels a clone stamp copies.
#[allow(clippy::too_many_arguments)]
pub fn apply_colors(
    layer: &mut Canvas,
    before: Option<&Canvas>,
    area: IRect,
    selection: Option<&Mask>,
    brush_kind: BrushKind,
    opacity: u8,
    mode: BlendMode,
    cov_color: impl Fn(i32, i32) -> (u8, Rgba),
) -> Option<IRect> {
    let area = area.clip(layer.width(), layer.height())?;
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let (cv, color) = cov_color(x, y);
            let mut c = u32::from(cv);
            if let Some(sel) = selection {
                c = div255(c * u32::from(sel.get(x, y)));
            }
            let base = before.map_or_else(|| layer.get(x, y), |b| b.get(x, y));
            let out = if c == 0 {
                base
            } else {
                match brush_kind {
                    BrushKind::Paint => {
                        let src = [
                            color[0],
                            color[1],
                            color[2],
                            div255(u32::from(color[3]) * c) as u8,
                        ];
                        blend::composite(base, src, opacity, mode)
                    }
                    BrushKind::Erase => blend::erase(base, div255(c * u32::from(opacity))),
                }
            };
            layer.set(x, y, out);
        }
    }
    Some(area)
}

/// Flood fill from `(x, y)`: the region is found on `sample` (the layer itself, or the
/// merged image) and painted onto `layer`.
pub fn bucket_fill(
    layer: &mut Canvas,
    sample: &Canvas,
    x: i32,
    y: i32,
    color: Rgba,
    tolerance: u8,
    selection: Option<&Mask>,
) -> Option<IRect> {
    let region = Mask::flood(sample, x, y, tolerance);
    let area = region.bounds()?;
    apply_coverage(
        layer,
        None,
        area,
        selection,
        BrushKind::Paint,
        color,
        255,
        BlendMode::Normal,
        |px, py| region.get(px, py),
    )
}

/// Composite a coverage mask (text, a pasted alpha) in `color`, its top-left at `(x, y)`.
pub fn stamp(
    layer: &mut Canvas,
    x: i32,
    y: i32,
    alpha: &Mask,
    color: Rgba,
    selection: Option<&Mask>,
) -> Option<IRect> {
    apply_coverage(
        layer,
        None,
        IRect::new(x, y, alpha.width(), alpha.height()),
        selection,
        BrushKind::Paint,
        color,
        255,
        BlendMode::Normal,
        |px, py| alpha.get(px - x, py - y),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Line,
    /// A line with an arrowhead at its end, as Markup and Paint's arrow tool draw.
    Arrow,
    Rectangle,
    RoundedRectangle,
    Ellipse,
    /// Free polygon through the points the pointer placed.
    Polygon,
    Triangle,
    RightTriangle,
    Diamond,
    Pentagon,
    Hexagon,
    RightArrow,
    LeftArrow,
    UpArrow,
    DownArrow,
    Star,
    Heart,
    /// A cubic Bézier: `points` are the start, two control points and the end (Paint's
    /// Curve). Two points draw it straight.
    Curve,
    /// A closed shape through freehand points (Pinta's Freeform Shape).
    Freeform,
    /// An open line through every point (a stroked path, Pinta's Line/Curve).
    Polyline,
}
impl ShapeKind {
    pub const ALL: [ShapeKind; 20] = [
        Self::Line,
        Self::Arrow,
        Self::Rectangle,
        Self::RoundedRectangle,
        Self::Ellipse,
        Self::Polygon,
        Self::Triangle,
        Self::RightTriangle,
        Self::Diamond,
        Self::Pentagon,
        Self::Hexagon,
        Self::RightArrow,
        Self::LeftArrow,
        Self::UpArrow,
        Self::DownArrow,
        Self::Star,
        Self::Heart,
        Self::Curve,
        Self::Freeform,
        Self::Polyline,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Arrow => "arrow",
            Self::Rectangle => "rectangle",
            Self::RoundedRectangle => "rounded-rectangle",
            Self::Ellipse => "ellipse",
            Self::Polygon => "polygon",
            Self::Triangle => "triangle",
            Self::RightTriangle => "right-triangle",
            Self::Diamond => "diamond",
            Self::Pentagon => "pentagon",
            Self::Hexagon => "hexagon",
            Self::RightArrow => "right-arrow",
            Self::LeftArrow => "left-arrow",
            Self::UpArrow => "up-arrow",
            Self::DownArrow => "down-arrow",
            Self::Star => "star",
            Self::Heart => "heart",
            Self::Curve => "curve",
            Self::Freeform => "freeform",
            Self::Polyline => "polyline",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
    /// Line-like shapes have an outline and no interior.
    pub fn open(self) -> bool {
        matches!(
            self,
            Self::Line | Self::Arrow | Self::Curve | Self::Polyline
        )
    }
}

/// A shape in sub16 coordinates. Two-point shapes use `points[0]` and `points[1]` as the
/// ends of a line or opposite corners of their box; `Polygon` uses every point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    pub kind: ShapeKind,
    pub points: Vec<P16>,
    pub outline: Option<Rgba>,
    pub fill: Option<Rgba>,
    /// Outline width in pixels.
    pub width: u32,
    pub antialias: bool,
}

/// Vertex fractions (per mille of the box) of the box-derived polygons.
fn template(kind: ShapeKind) -> &'static [(i64, i64)] {
    match kind {
        ShapeKind::Triangle => &[(500, 0), (1000, 1000), (0, 1000)],
        ShapeKind::RightTriangle => &[(0, 0), (1000, 1000), (0, 1000)],
        ShapeKind::Diamond => &[(500, 0), (1000, 500), (500, 1000), (0, 500)],
        ShapeKind::Pentagon => &[(500, 0), (1000, 382), (809, 1000), (191, 1000), (0, 382)],
        ShapeKind::Hexagon => &[
            (250, 0),
            (750, 0),
            (1000, 500),
            (750, 1000),
            (250, 1000),
            (0, 500),
        ],
        ShapeKind::RightArrow => &[
            (0, 250),
            (600, 250),
            (600, 0),
            (1000, 500),
            (600, 1000),
            (600, 750),
            (0, 750),
        ],
        ShapeKind::LeftArrow => &[
            (1000, 250),
            (400, 250),
            (400, 0),
            (0, 500),
            (400, 1000),
            (400, 750),
            (1000, 750),
        ],
        ShapeKind::UpArrow => &[
            (250, 1000),
            (250, 400),
            (0, 400),
            (500, 0),
            (1000, 400),
            (750, 400),
            (750, 1000),
        ],
        ShapeKind::DownArrow => &[
            (250, 0),
            (250, 600),
            (0, 600),
            (500, 1000),
            (1000, 600),
            (750, 600),
            (750, 0),
        ],
        // A regular five-pointed star in its bounding box.
        ShapeKind::Star => &[
            (500, 0),
            (618, 363),
            (1000, 363),
            (691, 588),
            (809, 951),
            (500, 727),
            (191, 951),
            (309, 588),
            (0, 363),
            (382, 363),
        ],
        // Two lobes and a point, as a sixteen-sided outline.
        ShapeKind::Heart => &[
            (500, 250),
            (580, 100),
            (700, 20),
            (850, 30),
            (960, 150),
            (990, 320),
            (900, 520),
            (700, 750),
            (500, 1000),
            (300, 750),
            (100, 520),
            (10, 320),
            (40, 150),
            (150, 30),
            (300, 20),
            (420, 100),
        ],
        _ => &[],
    }
}

impl Shape {
    fn corners(&self) -> Option<(i64, i64, i64, i64)> {
        let (a, b) = (self.points.first()?, self.points.get(1)?);
        Some((a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)))
    }
    /// The polygon a closed non-rectangular, non-elliptic shape is drawn as.
    pub fn outline_points(&self) -> Vec<P16> {
        match self.kind {
            ShapeKind::Polygon | ShapeKind::Freeform | ShapeKind::Polyline => {
                return self.points.clone()
            }
            ShapeKind::Curve => {
                let p = &self.points;
                return match p.len() {
                    0 | 1 => p.clone(),
                    2 => p.clone(),
                    3 => crate::path::cubic(p[0], p[1], p[1], p[2]),
                    _ => crate::path::cubic(p[0], p[1], p[2], p[3]),
                };
            }
            _ => {}
        }
        let Some((x0, y0, x1, y1)) = self.corners() else {
            return vec![];
        };
        let (w, h) = (x1 - x0, y1 - y0);
        template(self.kind)
            .iter()
            .map(|(fx, fy)| (x0 + w * fx / 1000, y0 + h * fy / 1000))
            .collect()
    }
    fn half16(&self) -> i64 {
        i64::from(self.width.max(1)) * 8
    }
    /// Pixel area the shape can touch.
    fn area(&self, w: u32, h: u32) -> Option<IRect> {
        let pad = self.half16() + 16 * i64::from(self.width.max(1)) * 4;
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for (x, y) in &self.points {
            x0 = x0.min(*x);
            y0 = y0.min(*y);
            x1 = x1.max(*x);
            y1 = y1.max(*y);
        }
        if self.points.is_empty() {
            return None;
        }
        sub16_bounds(x0 - pad, y0 - pad, x1 + pad, y1 + pad, w, h)
    }
    /// Arrowhead triangle for an `Arrow` from `a` to `b`, and where its shaft ends.
    fn arrowhead(&self, a: P16, b: P16) -> (Vec<P16>, P16) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = isqrt((dx * dx + dy * dy) as u64).max(1) as i64;
        let head = (i64::from(self.width.max(1)) * 16 * 3).max(16 * 8).min(len);
        let (ux, uy) = (dx * head / len, dy * head / len);
        let base = (b.0 - ux, b.1 - uy);
        let (px, py) = (-uy * 6 / 10, ux * 6 / 10);
        (
            vec![b, (base.0 + px, base.1 + py), (base.0 - px, base.1 - py)],
            (b.0 - ux / 2, b.1 - uy / 2),
        )
    }
    /// Whether a sub16 point is in the outline or the interior.
    fn hit(&self, x: i64, y: i64) -> (bool, bool) {
        let half = self.half16();
        match self.kind {
            ShapeKind::Line | ShapeKind::Arrow => {
                let (Some(a), Some(b)) = (self.points.first(), self.points.get(1)) else {
                    return (false, false);
                };
                if self.kind == ShapeKind::Arrow {
                    let (head, shaft_end) = self.arrowhead(*a, *b);
                    (
                        near_segment(*a, shaft_end, half, x, y) || in_polygon(&head, x, y),
                        false,
                    )
                } else {
                    (near_segment(*a, *b, half, x, y), false)
                }
            }
            ShapeKind::Polyline | ShapeKind::Curve => {
                // Curves are flattened to a polyline before drawing (see `draw`).
                let pts = &self.points;
                let edge = pts.len() == 1 && near_segment(pts[0], pts[0], half, x, y)
                    || pts.windows(2).any(|w| near_segment(w[0], w[1], half, x, y));
                (edge, false)
            }
            ShapeKind::Rectangle | ShapeKind::RoundedRectangle => {
                let Some((x0, y0, x1, y1)) = self.corners() else {
                    return (false, false);
                };
                let radius = if self.kind == ShapeKind::RoundedRectangle {
                    ((x1 - x0).min(y1 - y0) / 6).max(0)
                } else {
                    0
                };
                let inside = |x0: i64, y0: i64, x1: i64, y1: i64, r: i64| {
                    if x < x0 || x >= x1 || y < y0 || y >= y1 {
                        return false;
                    }
                    if r <= 0 {
                        return true;
                    }
                    let cx = x.clamp(x0 + r, x1 - r);
                    let cy = y.clamp(y0 + r, y1 - r);
                    (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r
                };
                let outer = inside(x0 - half, y0 - half, x1 + half, y1 + half, radius + half);
                let inner = inside(x0 + half, y0 + half, x1 - half, y1 - half, radius - half);
                (outer && !inner, inside(x0, y0, x1, y1, radius))
            }
            ShapeKind::Ellipse => {
                let Some((x0, y0, x1, y1)) = self.corners() else {
                    return (false, false);
                };
                let outer = in_ellipse(x0 - half, y0 - half, x1 + half, y1 + half, x, y);
                let inner = in_ellipse(x0 + half, y0 + half, x1 - half, y1 - half, x, y);
                (outer && !inner, in_ellipse(x0, y0, x1, y1, x, y))
            }
            _ => {
                let pts = self.outline_points();
                let n = pts.len();
                let edge = (0..n).any(|i| near_segment(pts[i], pts[(i + 1) % n], half, x, y));
                (edge, in_polygon(&pts, x, y))
            }
        }
    }
    /// Paint the shape onto `layer`: its fill, then its outline over it.
    pub fn draw(&self, layer: &mut Canvas, selection: Option<&Mask>) -> Option<IRect> {
        if self.kind == ShapeKind::Curve {
            let flat = Shape {
                kind: ShapeKind::Polyline,
                points: self.outline_points(),
                ..self.clone()
            };
            return flat.draw(layer, selection);
        }
        if self.kind == ShapeKind::Polyline {
            // Segment by segment into one coverage mask: a long path costs its length,
            // not its length times its bounding box.
            let color = self.outline?;
            let half = self.half16();
            let mut mask = Mask::empty(layer.width(), layer.height());
            let mut touched: Option<IRect> = None;
            let segments: Vec<(P16, P16)> = if self.points.len() == 1 {
                vec![(self.points[0], self.points[0])]
            } else {
                self.points.windows(2).map(|w| (w[0], w[1])).collect()
            };
            for (a, b) in segments {
                let pad = half + 32;
                let Some(r) = sub16_bounds(
                    a.0.min(b.0) - pad,
                    a.1.min(b.1) - pad,
                    a.0.max(b.0) + pad,
                    a.1.max(b.1) + pad,
                    layer.width(),
                    layer.height(),
                ) else {
                    continue;
                };
                for y in r.y..r.bottom() {
                    for x in r.x..r.right() {
                        let c = coverage(x, y, self.antialias, &|sx, sy| {
                            near_segment(a, b, half, sx, sy)
                        });
                        if c > 0 {
                            mask.raise(x, y, c);
                        }
                    }
                }
                touched = Some(touched.map_or(r, |t| t.union(&r)));
            }
            return apply_coverage(
                layer,
                None,
                touched?,
                selection,
                BrushKind::Paint,
                color,
                255,
                BlendMode::Normal,
                |x, y| mask.get(x, y),
            );
        }
        let area = self.area(layer.width(), layer.height())?;
        let fill = self.fill.filter(|_| !self.kind.open());
        let mut touched: Option<IRect> = None;
        let mut paint = |color: Rgba, which: usize, layer: &mut Canvas| {
            let r = apply_coverage(
                layer,
                None,
                area,
                selection,
                BrushKind::Paint,
                color,
                255,
                BlendMode::Normal,
                |x, y| {
                    coverage(x, y, self.antialias, &|sx, sy| {
                        let hit = self.hit(sx, sy);
                        if which == 0 {
                            hit.1
                        } else {
                            hit.0
                        }
                    })
                },
            );
            if let Some(r) = r {
                touched = Some(touched.map_or(r, |t| t.union(&r)));
            }
        };
        if let Some(color) = fill {
            paint(color, 0, layer);
        }
        if let Some(color) = self.outline {
            paint(color, 1, layer);
        }
        touched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BLACK, TRANSPARENT, WHITE};
    fn px(x: i64, y: i64) -> P16 {
        (x * 16 + 8, y * 16 + 8)
    }
    #[test]
    fn a_one_pixel_pencil_dab_is_one_pixel() {
        let brush = Brush {
            size: 1,
            antialias: false,
            ..Brush::default()
        };
        let mut mask = Mask::empty(5, 5);
        brush.dab(&mut mask, 2 * 16 + 3, 2 * 16 + 13);
        assert_eq!(mask.bounds(), Some(IRect::new(2, 2, 1, 1)));
        assert_eq!(mask.get(2, 2), 255);
        // A 3px pencil covers the 3x3 block: the diagonal centres are sqrt(2) < 1.5 away.
        let brush = Brush { size: 3, ..brush };
        let mut mask = Mask::empty(5, 5);
        brush.dab(&mut mask, px(2, 2).0, px(2, 2).1);
        assert_eq!(mask.data().iter().filter(|v| **v == 255).count(), 9);
    }
    #[test]
    fn soft_brushes_fall_off_and_hard_ones_do_not() {
        let hard = Brush {
            size: 9,
            ..Brush::default()
        };
        let soft = Brush {
            hardness: 0,
            ..hard
        };
        let (cx, cy) = px(10, 10);
        assert_eq!(hard.dab_coverage(cx, cy, 10, 10), 255);
        // A 9px hard tip covers nine whole pixels across; the half-pixel ramp beyond
        // its 4.5px radius starts at the tenth pixel's centre, five pixels out.
        assert_eq!(hard.dab_coverage(cx, cy, 14, 10), 255);
        assert_eq!(hard.dab_coverage(cx, cy, 15, 10), 0);
        let wide = Brush { size: 10, ..hard };
        // Radius 5 (80 sub16), ramp 72..88: a centre exactly 5px out is half covered.
        assert_eq!(wide.dab_coverage(cx, cy, 15, 10), 127);
        assert_eq!(soft.dab_coverage(cx, cy, 10, 10), 255);
        // Soft: linear from the centre (0) to 4.5 + 0.5 px (80 sub16): 255*(80-32)/80.
        assert_eq!(soft.dab_coverage(cx, cy, 12, 10), 153);
    }
    #[test]
    fn dabs_are_evenly_spaced_across_segments() {
        let (a, carry) = dabs_along((0, 0), (40, 0), 16, 0);
        assert_eq!(a, vec![(16, 0), (32, 0)]);
        assert_eq!(carry, 8);
        let (b, carry) = dabs_along((40, 0), (60, 0), 16, carry);
        assert_eq!(b, vec![(48, 0)]);
        assert_eq!(carry, 12);
        let (c, carry) = dabs_along((60, 0), (62, 0), 16, carry);
        assert!(c.is_empty());
        assert_eq!(carry, 14);
    }
    #[test]
    fn shapes_fill_and_outline_where_they_should() {
        let mut c = Canvas::filled(20, 20, WHITE);
        let rect = Shape {
            kind: ShapeKind::Rectangle,
            points: vec![(2 * 16, 2 * 16), (12 * 16, 8 * 16)],
            outline: Some(BLACK),
            fill: Some([255, 0, 0, 255]),
            width: 2,
            antialias: false,
        };
        let r = rect.draw(&mut c, None).unwrap();
        assert!(r.contains(1, 1) && r.contains(12, 8));
        // A 2px outline straddles the edge: one pixel out, one pixel in.
        assert_eq!(c.get(1, 5), BLACK);
        assert_eq!(c.get(2, 5), BLACK);
        assert_eq!(c.get(3, 5), [255, 0, 0, 255]);
        assert_eq!(c.get(0, 5), WHITE);
        assert_eq!(c.get(7, 7), BLACK);
        assert_eq!(c.get(7, 6), [255, 0, 0, 255]);
        // A line with no fill stays open even when a fill colour is set.
        let mut c = Canvas::new(10, 3);
        Shape {
            kind: ShapeKind::Line,
            points: vec![px(1, 1), px(8, 1)],
            outline: Some(BLACK),
            fill: Some(WHITE),
            width: 1,
            antialias: false,
        }
        .draw(&mut c, None);
        assert_eq!(c.get(1, 1), BLACK);
        assert_eq!(c.get(8, 1), BLACK);
        assert_eq!(c.get(4, 0), TRANSPARENT);
        assert_eq!(c.get(4, 2), TRANSPARENT);
        // Every template shape draws something inside its box.
        for kind in ShapeKind::ALL {
            let mut c = Canvas::new(40, 40);
            let shape = Shape {
                kind,
                points: vec![(4 * 16, 4 * 16), (36 * 16, 30 * 16), (4 * 16, 30 * 16)],
                outline: Some(BLACK),
                fill: Some(WHITE),
                width: 1,
                antialias: true,
            };
            assert!(shape.draw(&mut c, None).is_some(), "{kind:?}");
            assert!(c.opaque_bounds().is_some(), "{kind:?}");
            assert_eq!(ShapeKind::parse(kind.id()), Some(kind));
        }
    }
    #[test]
    fn bucket_fill_respects_tolerance_and_the_selection() {
        let mut c = Canvas::filled(6, 1, WHITE);
        c.set(3, 0, BLACK);
        let sample = c.clone();
        bucket_fill(&mut c, &sample, 0, 0, [0, 0, 255, 255], 0, None).unwrap();
        assert_eq!(c.get(2, 0), [0, 0, 255, 255]);
        assert_eq!(c.get(4, 0), WHITE);
        let mut c = sample.clone();
        let sel = Mask::rect(6, 1, IRect::new(0, 0, 2, 1));
        bucket_fill(&mut c, &sample, 0, 0, [0, 0, 255, 255], 0, Some(&sel)).unwrap();
        assert_eq!(c.get(1, 0), [0, 0, 255, 255]);
        assert_eq!(c.get(2, 0), WHITE);
        let mut alpha = Mask::empty(2, 1);
        alpha.set(0, 0, 255);
        alpha.set(1, 0, 128);
        let mut c = Canvas::filled(4, 1, WHITE);
        stamp(&mut c, 1, 0, &alpha, BLACK, None);
        assert_eq!(c.get(1, 0), BLACK);
        assert_eq!(c.get(2, 0), [127, 127, 127, 255]);
    }
}
