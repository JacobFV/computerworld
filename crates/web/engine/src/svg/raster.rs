//! Anti-aliased filling and stroking of flattened SVG geometry into an RGBA raster.
//!
//! Filling accumulates each edge's signed area into a per-pixel buffer and sums it
//! along the row (the signed-area scan converter font rasterisers use): the result
//! is the exact coverage of each pixel by the polygon, with windings adding up, so
//! the nonzero rule is `min(1, |sum|)` and the even-odd rule folds the sum into
//! `[0, 1]`. Strokes are outlined as polygons (a quad per segment, joins and caps),
//! all wound the same way, and filled nonzero, which unions them.
//!
//! Colour is composited source-over in premultiplied `f32` and written out as
//! straight-alpha RGBA bytes for `Primitive::Image`.

use super::geom::{sin_cos, Affine, Poly, Pt};
use cw_scene::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// In device pixels.
    pub width: f64,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
    /// Dash and gap lengths in device pixels; empty for a solid line.
    pub dashes: Vec<f64>,
    pub dash_offset: f64,
}

/// How a covered pixel is coloured.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Color),
    /// `t` along the gradient from the pixel centre mapped through `inverse` (into
    /// the gradient's own unit space), then the stops.
    Linear {
        inverse: Affine,
        from: Pt,
        to: Pt,
        stops: Vec<(f64, [f64; 4])>,
    },
    Radial {
        inverse: Affine,
        centre: Pt,
        radius: f64,
        stops: Vec<(f64, [f64; 4])>,
    },
}

/// A premultiplied RGBA canvas in device pixels.
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    px: Vec<[f32; 4]>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Canvas {
        Canvas {
            width,
            height,
            px: vec![[0.0; 4]; width * height],
        }
    }

    pub fn is_blank(&self) -> bool {
        self.px.iter().all(|p| p[3] == 0.0)
    }

    /// Straight-alpha RGBA bytes.
    pub fn into_rgba(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in self.px {
            let a = p[3].clamp(0.0, 1.0);
            if a <= 0.0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let un = |c: f32| ((c / a).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            out.extend_from_slice(&[un(p[0]), un(p[1]), un(p[2]), (a * 255.0 + 0.5) as u8]);
        }
        out
    }

    /// Fills `polys` with `paint` at `opacity`.
    pub fn fill(&mut self, polys: &[Poly], rule: FillRule, paint: &Paint, opacity: f64) {
        let Some(mask) = Mask::of(polys, self.width, self.height, rule) else {
            return;
        };
        self.composite(&mask, paint, opacity);
    }

    pub fn stroke(&mut self, polys: &[Poly], stroke: &Stroke, paint: &Paint, opacity: f64) {
        let outline = stroke_outline(polys, stroke);
        self.fill(&outline, FillRule::NonZero, paint, opacity);
    }

    fn composite(&mut self, mask: &Mask, paint: &Paint, opacity: f64) {
        let opacity = opacity.clamp(0.0, 1.0) as f32;
        for y in 0..mask.h {
            let row = &mask.cov[y * mask.w..(y + 1) * mask.w];
            for (x, &c) in row.iter().enumerate() {
                if c <= 0.0 {
                    continue;
                }
                let (dx, dy) = (mask.x0 + x, mask.y0 + y);
                let src = paint_at(paint, dx as f64 + 0.5, dy as f64 + 0.5);
                let k = c * opacity * src[3];
                if k <= 0.0 {
                    continue;
                }
                let d = &mut self.px[dy * self.width + dx];
                let inv = 1.0 - k;
                d[0] = src[0] * k + d[0] * inv;
                d[1] = src[1] * k + d[1] * inv;
                d[2] = src[2] * k + d[2] * inv;
                d[3] = k + d[3] * inv;
            }
        }
    }
}

/// Straight RGB in 0..1 and alpha.
fn paint_at(paint: &Paint, x: f64, y: f64) -> [f32; 4] {
    let lerp = |stops: &[(f64, [f64; 4])], t: f64| -> [f32; 4] {
        let t = t.clamp(0.0, 1.0);
        let Some(first) = stops.first() else {
            return [0.0; 4];
        };
        if t <= first.0 {
            return first.1.map(|v| v as f32);
        }
        for w in stops.windows(2) {
            let ((t0, c0), (t1, c1)) = (w[0], w[1]);
            if t <= t1 {
                let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
                // Interpolated premultiplied, as Skia draws SVG gradients.
                let a = c0[3] + (c1[3] - c0[3]) * f;
                let mut out = [0.0f32; 4];
                for i in 0..3 {
                    let v = c0[i] * c0[3] + (c1[i] * c1[3] - c0[i] * c0[3]) * f;
                    out[i] = if a > 0.0 {
                        (v / a) as f32
                    } else {
                        c1[i] as f32
                    };
                }
                out[3] = a as f32;
                return out;
            }
        }
        stops.last().unwrap().1.map(|v| v as f32)
    };
    match paint {
        Paint::Solid(c) => [
            c.0 as f32 / 255.0,
            c.1 as f32 / 255.0,
            c.2 as f32 / 255.0,
            c.3 as f32 / 255.0,
        ],
        Paint::Linear {
            inverse,
            from,
            to,
            stops,
        } => {
            let p = inverse.apply(Pt::new(x, y));
            let (vx, vy) = (to.x - from.x, to.y - from.y);
            let len2 = vx * vx + vy * vy;
            let t = if len2 > 0.0 {
                ((p.x - from.x) * vx + (p.y - from.y) * vy) / len2
            } else {
                1.0
            };
            lerp(stops, t)
        }
        Paint::Radial {
            inverse,
            centre,
            radius,
            stops,
        } => {
            let p = inverse.apply(Pt::new(x, y));
            let d =
                ((p.x - centre.x) * (p.x - centre.x) + (p.y - centre.y) * (p.y - centre.y)).sqrt();
            let t = if *radius > 0.0 { d / radius } else { 1.0 };
            lerp(stops, t)
        }
    }
}

/// Coverage of a region of the canvas, `w × h` pixels from `(x0, y0)`.
struct Mask {
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    cov: Vec<f32>,
}

impl Mask {
    fn of(polys: &[Poly], width: usize, height: usize, rule: FillRule) -> Option<Mask> {
        let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in polys {
            for q in &p.pts {
                minx = minx.min(q.x);
                miny = miny.min(q.y);
                maxx = maxx.max(q.x);
                maxy = maxy.max(q.y);
            }
        }
        if minx > maxx {
            return None;
        }
        let x0 = minx.floor().max(0.0) as usize;
        let y0 = miny.floor().max(0.0) as usize;
        let x1 = (maxx.ceil().max(0.0) as usize).min(width);
        let y1 = (maxy.ceil().max(0.0) as usize).min(height);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let (w, h) = (x1 - x0, y1 - y0);
        // Two guard cells a row: edges clipped to the right edge write at `w`.
        let stride = w + 2;
        let mut acc = vec![0.0f32; stride * h];
        let (ox, oy) = (x0 as f64, y0 as f64);
        for p in polys {
            let n = p.pts.len();
            if n < 2 {
                continue;
            }
            for i in 0..n {
                let a = p.pts[i];
                let b = p.pts[(i + 1) % n];
                edge(
                    &mut acc,
                    stride,
                    w as f64,
                    h,
                    Pt::new(a.x - ox, a.y - oy),
                    Pt::new(b.x - ox, b.y - oy),
                );
            }
        }
        let mut cov = vec![0.0f32; w * h];
        for y in 0..h {
            let mut sum = 0.0f32;
            for x in 0..w {
                sum += acc[y * stride + x];
                let c = match rule {
                    FillRule::NonZero => sum.abs().min(1.0),
                    FillRule::EvenOdd => {
                        let m = sum.abs() % 2.0;
                        if m > 1.0 {
                            2.0 - m
                        } else {
                            m
                        }
                    }
                };
                cov[y * w + x] = c;
            }
        }
        Some(Mask { x0, y0, w, h, cov })
    }
}

/// Adds the signed area of the edge `a → b` to the accumulation buffer, clipped to
/// `[0, w]` horizontally (the part left of 0 counts at column 0, which keeps the
/// winding) and to the rows that exist.
fn edge(acc: &mut [f32], stride: usize, w: f64, h: usize, a: Pt, b: Pt) {
    // Split at x = 0 and x = w so each piece lies in one horizontal band.
    let mut pieces = vec![(a, b)];
    for cut in [0.0, w] {
        let mut next = Vec::with_capacity(pieces.len() + 1);
        for (p, q) in pieces {
            if (p.x < cut && q.x > cut) || (p.x > cut && q.x < cut) {
                let t = (cut - p.x) / (q.x - p.x);
                let m = Pt::new(cut, p.y + (q.y - p.y) * t);
                next.push((p, m));
                next.push((m, q));
            } else {
                next.push((p, q));
            }
        }
        pieces = next;
    }
    for (p, q) in pieces {
        let clamp = |v: Pt| Pt::new(v.x.clamp(0.0, w), v.y);
        line(acc, stride, h, clamp(p), clamp(q));
    }
}

/// The signed-area accumulation of one line (Raph Levien's font-rs scan converter).
fn line(acc: &mut [f32], stride: usize, h: usize, p0: Pt, p1: Pt) {
    if (p0.y - p1.y).abs() < 1e-9 {
        return;
    }
    let (dir, p0, p1) = if p0.y < p1.y {
        (1.0f32, p0, p1)
    } else {
        (-1.0f32, p1, p0)
    };
    let (x0p, y0p, x1p, y1p) = (p0.x as f32, p0.y as f32, p1.x as f32, p1.y as f32);
    let dxdy = (x1p - x0p) / (y1p - y0p);
    let mut x = x0p;
    let ystart = if y0p < 0.0 {
        x -= y0p * dxdy;
        0usize
    } else {
        y0p as usize
    };
    let yend = (y1p.ceil().max(0.0) as usize).min(h);
    for y in ystart..yend {
        let row = y * stride;
        let dy = ((y + 1) as f32).min(y1p) - (y as f32).max(y0p);
        let xnext = x + dxdy * dy;
        let d = dy * dir;
        let (xa, xb) = if x < xnext { (x, xnext) } else { (xnext, x) };
        let xa_floor = xa.floor();
        let xai = xa_floor as isize;
        let xb_ceil = xb.ceil();
        let xbi = xb_ceil as isize;
        let at = |i: isize| row + i.max(0) as usize;
        if xbi <= xai + 1 {
            let xmf = 0.5 * (x + xnext) - xa_floor;
            acc[at(xai)] += d - d * xmf;
            acc[at(xai + 1)] += d * xmf;
        } else {
            let s = 1.0 / (xb - xa);
            let xaf = xa - xa_floor;
            let a0 = 0.5 * s * (1.0 - xaf) * (1.0 - xaf);
            let xbf = xb - xb_ceil + 1.0;
            let am = 0.5 * s * xbf * xbf;
            acc[at(xai)] += d * a0;
            if xbi == xai + 2 {
                acc[at(xai + 1)] += d * (1.0 - a0 - am);
            } else {
                let a1 = s * (1.5 - xaf);
                acc[at(xai + 1)] += d * (a1 - a0);
                for xi in xai + 2..xbi - 1 {
                    acc[at(xi)] += d * s;
                }
                let a2 = a1 + (xbi - xai - 3) as f32 * s;
                acc[at(xbi - 1)] += d * (1.0 - a2 - am);
            }
            acc[at(xbi)] += d * am;
        }
        x = xnext;
    }
}

fn signed_area(pts: &[Pt]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        a += p.x * q.y - q.x * p.y;
    }
    a / 2.0
}

/// A closed polygon wound positively (so overlapping pieces add up).
fn piece(mut pts: Vec<Pt>) -> Poly {
    if signed_area(&pts) < 0.0 {
        pts.reverse();
    }
    Poly { pts, closed: true }
}

/// A circle as a polygon fine enough that its chords stay within 0.05 px.
fn disc(c: Pt, r: f64) -> Poly {
    let n = if r <= 0.0 {
        return Poly::default();
    } else {
        // Chord sagitta r(1 - cos(π/n)) ≤ 0.05 ⇒ n ≥ π / acos(1 - 0.05/r).
        ((std::f64::consts::PI * (r / 0.1).sqrt()).ceil() as usize).clamp(8, 128)
    };
    let pts = (0..n)
        .map(|i| {
            let (s, co) = sin_cos(2.0 * std::f64::consts::PI * i as f64 / n as f64);
            Pt::new(c.x + r * co, c.y + r * s)
        })
        .collect();
    piece(pts)
}

fn dedup(pts: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out
            .last()
            .is_none_or(|q| (q.x - p.x).abs() > 1e-9 || (q.y - p.y).abs() > 1e-9)
        {
            out.push(p);
        }
    }
    out
}

/// Splits polylines into dashes.
fn dash(polys: &[Poly], dashes: &[f64], offset: f64) -> Vec<Poly> {
    let total: f64 = dashes.iter().sum();
    if total <= 0.0 || dashes.iter().any(|d| *d < 0.0) {
        return polys.to_vec();
    }
    let mut out = Vec::new();
    for p in polys {
        let mut pts = p.pts.clone();
        if p.closed && pts.len() > 1 {
            pts.push(pts[0]);
        }
        // Position in the pattern.
        let mut idx = 0;
        let mut left = dashes[0];
        let mut off = offset.rem_euclid(total);
        while off > 0.0 {
            if off >= left {
                off -= left;
                idx = (idx + 1) % dashes.len();
                left = dashes[idx];
            } else {
                left -= off;
                off = 0.0;
            }
        }
        let mut cur: Vec<Pt> = Vec::new();
        let on = |i: usize| i.is_multiple_of(2);
        if on(idx) {
            if let Some(&s) = pts.first() {
                cur.push(s);
            }
        }
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let seg = ((b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y)).sqrt();
            let mut pos = 0.0;
            while seg - pos > left {
                pos += left;
                let t = pos / seg;
                let m = Pt::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                if on(idx) {
                    cur.push(m);
                    out.push(Poly {
                        pts: std::mem::take(&mut cur),
                        closed: false,
                    });
                } else {
                    cur = vec![m];
                }
                idx = (idx + 1) % dashes.len();
                left = dashes[idx];
            }
            left -= seg - pos;
            if on(idx) {
                cur.push(b);
            }
        }
        if on(idx) && cur.len() > 1 {
            out.push(Poly {
                pts: cur,
                closed: false,
            });
        }
    }
    out
}

/// The outline of a stroke as positively wound pieces.
pub fn stroke_outline(polys: &[Poly], s: &Stroke) -> Vec<Poly> {
    let hw = s.width / 2.0;
    if hw <= 0.0 {
        return Vec::new();
    }
    let dashed;
    let polys = if s.dashes.is_empty() {
        polys
    } else {
        dashed = dash(polys, &s.dashes, s.dash_offset);
        &dashed[..]
    };
    let mut out = Vec::new();
    for p in polys {
        let mut pts = dedup(&p.pts);
        // A closed subpath that returns to its start explicitly has no closing segment.
        if p.closed && pts.len() > 2 {
            let (a, b) = (pts[0], pts[pts.len() - 1]);
            if (a.x - b.x).abs() <= 1e-9 && (a.y - b.y).abs() <= 1e-9 {
                pts.pop();
            }
        }
        let closed = p.closed && pts.len() > 2;
        if pts.len() == 1 {
            // A zero-length subpath shows only its caps.
            match s.cap {
                Cap::Round => out.push(disc(pts[0], hw)),
                Cap::Square => {
                    let c = pts[0];
                    out.push(piece(vec![
                        Pt::new(c.x - hw, c.y - hw),
                        Pt::new(c.x + hw, c.y - hw),
                        Pt::new(c.x + hw, c.y + hw),
                        Pt::new(c.x - hw, c.y + hw),
                    ]));
                }
                Cap::Butt => {}
            }
            continue;
        }
        let n = pts.len();
        let segs = if closed { n } else { n - 1 };
        let dir = |i: usize| -> Pt {
            let (a, b) = (pts[i % n], pts[(i + 1) % n]);
            let len = ((b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y)).sqrt();
            Pt::new((b.x - a.x) / len, (b.y - a.y) / len)
        };
        for i in 0..segs {
            let (mut a, mut b) = (pts[i], pts[(i + 1) % n]);
            let d = dir(i);
            if !closed && s.cap == Cap::Square {
                if i == 0 {
                    a = Pt::new(a.x - d.x * hw, a.y - d.y * hw);
                }
                if i == segs - 1 {
                    b = Pt::new(b.x + d.x * hw, b.y + d.y * hw);
                }
            }
            let nx = -d.y * hw;
            let ny = d.x * hw;
            out.push(piece(vec![
                Pt::new(a.x + nx, a.y + ny),
                Pt::new(b.x + nx, b.y + ny),
                Pt::new(b.x - nx, b.y - ny),
                Pt::new(a.x - nx, a.y - ny),
            ]));
        }
        // Joins at every vertex between two segments.
        let joints: Vec<usize> = if closed {
            (0..n).collect()
        } else {
            (1..n - 1).collect()
        };
        for j in joints {
            let v = pts[j];
            let d0 = dir((j + n - 1) % n);
            let d1 = dir(j);
            let cross = d0.x * d1.y - d0.y * d1.x;
            if cross.abs() < 1e-9 && d0.x * d1.x + d0.y * d1.y > 0.0 {
                continue;
            }
            match s.join {
                Join::Round => out.push(disc(v, hw)),
                Join::Miter | Join::Bevel => {
                    // The outer side is the one the path turns away from.
                    let side = if cross > 0.0 { -1.0 } else { 1.0 };
                    let n0 = Pt::new(-d0.y * hw * side, d0.x * hw * side);
                    let n1 = Pt::new(-d1.y * hw * side, d1.x * hw * side);
                    let p0 = Pt::new(v.x + n0.x, v.y + n0.y);
                    let p1 = Pt::new(v.x + n1.x, v.y + n1.y);
                    let cos = (d0.x * d1.x + d0.y * d1.y).clamp(-1.0, 1.0);
                    // Miter length / stroke width = 1 / sin(θ/2), θ the angle between
                    // the segments; sin(θ/2) = √((1 - cos φ)/2) with φ the turn.
                    let sin_half = ((1.0 + cos) / 2.0).sqrt();
                    let miter_ok =
                        s.join == Join::Miter && sin_half > 0.0 && 1.0 / sin_half <= s.miter_limit;
                    if miter_ok {
                        let bis = Pt::new(n0.x + n1.x, n0.y + n1.y);
                        let bl = (bis.x * bis.x + bis.y * bis.y).sqrt();
                        let ml = hw / sin_half;
                        let m = Pt::new(v.x + bis.x / bl * ml, v.y + bis.y / bl * ml);
                        out.push(piece(vec![v, p0, m, p1]));
                    } else {
                        out.push(piece(vec![v, p0, p1]));
                    }
                }
            }
        }
        if !closed && s.cap == Cap::Round {
            out.push(disc(pts[0], hw));
            out.push(disc(pts[n - 1], hw));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Poly {
        Poly {
            pts: vec![
                Pt::new(x, y),
                Pt::new(x + s, y),
                Pt::new(x + s, y + s),
                Pt::new(x, y + s),
            ],
            closed: true,
        }
    }

    #[test]
    fn a_pixel_aligned_square_covers_exactly_its_pixels() {
        let mut c = Canvas::new(8, 8);
        c.fill(
            &[square(2.0, 2.0, 4.0)],
            FillRule::NonZero,
            &Paint::Solid(Color(255, 0, 0, 255)),
            1.0,
        );
        let rgba = c.into_rgba();
        let at = |x: usize, y: usize| rgba[(y * 8 + x) * 4 + 3];
        assert_eq!(at(2, 2), 255);
        assert_eq!(at(5, 5), 255);
        assert_eq!(at(1, 2), 0);
        assert_eq!(at(6, 5), 0);
    }

    #[test]
    fn half_covered_pixels_are_half_alpha() {
        let mut c = Canvas::new(4, 4);
        c.fill(
            &[square(0.5, 0.0, 2.0)],
            FillRule::NonZero,
            &Paint::Solid(Color(0, 0, 0, 255)),
            1.0,
        );
        let rgba = c.into_rgba();
        assert_eq!(rgba[3], 128);
        assert_eq!(rgba[4 + 3], 255);
        assert_eq!(rgba[2 * 4 + 3], 128);
    }

    #[test]
    fn a_stroked_line_is_as_wide_as_its_width() {
        let mut c = Canvas::new(10, 10);
        let line = Poly {
            pts: vec![Pt::new(1.0, 5.0), Pt::new(9.0, 5.0)],
            closed: false,
        };
        let s = Stroke {
            width: 2.0,
            cap: Cap::Butt,
            join: Join::Miter,
            miter_limit: 4.0,
            dashes: vec![],
            dash_offset: 0.0,
        };
        c.stroke(&[line], &s, &Paint::Solid(Color(0, 0, 0, 255)), 1.0);
        let rgba = c.into_rgba();
        let at = |x: usize, y: usize| rgba[(y * 10 + x) * 4 + 3];
        assert_eq!(at(5, 4), 255);
        assert_eq!(at(5, 5), 255);
        assert_eq!(at(5, 3), 0);
        assert_eq!(at(5, 6), 0);
        assert_eq!(at(0, 5), 0);
    }

    #[test]
    fn overlapping_round_joins_do_not_cancel() {
        let mut c = Canvas::new(20, 20);
        let vee = Poly {
            pts: vec![Pt::new(2.0, 2.0), Pt::new(10.0, 18.0), Pt::new(18.0, 2.0)],
            closed: false,
        };
        let s = Stroke {
            width: 4.0,
            cap: Cap::Round,
            join: Join::Round,
            miter_limit: 4.0,
            dashes: vec![],
            dash_offset: 0.0,
        };
        c.stroke(&[vee], &s, &Paint::Solid(Color(0, 0, 0, 255)), 1.0);
        let rgba = c.into_rgba();
        assert_eq!(rgba[(17 * 20 + 10) * 4 + 3], 255, "the joint is solid");
    }
}
