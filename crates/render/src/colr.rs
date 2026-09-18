//! Colour glyphs from `COLR`/`CPAL` fonts (Noto Color Emoji's COLRv1 build).
//!
//! `ttf-parser` walks the paint graph — layers, solid fills, linear, radial and sweep
//! gradients, transforms, clip boxes and composite layers, for COLRv0 and COLRv1 —
//! and calls back into [`Canvas`], which does the drawing here:
//!
//! * outlines are flattened and filled with an exact nonzero-winding scanline
//!   rasterizer: 16 sub-scanlines per pixel, horizontal coverage in 1/256 pixel,
//!   integer accumulation;
//! * paint is premultiplied RGBA in `f32`, composited with the Porter-Duff operators
//!   and the separable blend modes of the COLRv1 specification (W3C compositing
//!   formulas), inside the current clip;
//! * gradients follow the COLRv1 geometry: linear gradients are skewed by their
//!   rotation point, radial gradients are two-point conical, sweeps are clockwise
//!   in the flipped raster space, and every extend mode (pad, repeat, reflect) is
//!   honoured, interpolating in premultiplied space.
//!
//! **Determinism.** Only IEEE-754 `+ - * /`, `sqrt`, `floor` and conversions are
//! used, never a platform `sin`/`atan2`: those are evaluated here by polynomials.
//! Rust does not fuse multiply-adds, so native and Wasm builds compute the same
//! bits and the colour glyph is pixel-identical on every target.
use rustybuzz::ttf_parser::{
    self,
    colr::{self, ClipBox, CompositeMode, GradientExtend, Paint},
    GlyphId, OutlineBuilder, RgbaColor,
};

/// A rasterized colour glyph: premultiplied RGBA, placed relative to the pen
/// origin (`left` pixels right of it, top row `top` pixels above the baseline).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorGlyph {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Paint `glyph` of `face` at `size` pixels per em with palette 0. `None` when the
/// glyph has no colour definition. `foreground` fills palette entry 0xFFFF.
pub fn rasterize(
    face: &ttf_parser::Face<'_>,
    glyph: GlyphId,
    size: f32,
    foreground: RgbaColor,
) -> Option<ColorGlyph> {
    if !face.is_color_glyph(glyph) {
        return None;
    }
    let upem = f32::from(face.units_per_em());
    let scale = size / upem;
    let advance = f32::from(face.glyph_hor_advance(glyph).unwrap_or(0));
    let ascent = f32::from(face.ascender()).max(upem * 0.8);
    let descent = f32::from(face.descender()).min(-upem * 0.2);
    // A generous em box around the advance; the result is cropped to its ink.
    let left = (-0.25 * upem * scale).floor();
    let right = ((advance + 0.25 * upem) * scale).ceil();
    let top = ((ascent + 0.1 * upem) * scale).ceil();
    let bottom = ((descent - 0.1 * upem) * scale).floor();
    let width = (right - left).max(1.0) as usize;
    let height = (top - bottom).max(1.0) as usize;
    if width * height > 1 << 22 {
        return None;
    }
    let base = Affine {
        a: scale,
        b: 0.0,
        c: 0.0,
        d: -scale,
        e: -left,
        f: top,
    };
    let mut canvas = Canvas {
        face,
        width,
        height,
        transforms: vec![base],
        path: Vec::new(),
        clips: vec![vec![255; width * height]],
        layers: vec![(CompositeMode::SourceOver, vec![[0.0; 4]; width * height])],
    };
    face.paint_color_glyph(glyph, 0, foreground, &mut canvas)?;
    let pixels = &canvas.layers[0].1;
    // Crop to the painted pixels.
    let quantized: Vec<[u8; 4]> = pixels.iter().map(|p| p.map(to_u8)).collect();
    let rows: Vec<usize> = (0..height)
        .filter(|&y| {
            quantized[y * width..(y + 1) * width]
                .iter()
                .any(|p| p[3] != 0)
        })
        .collect();
    let cols: Vec<usize> = (0..width)
        .filter(|&x| (0..height).any(|y| quantized[y * width + x][3] != 0))
        .collect();
    let (Some(&y0), Some(&y1), Some(&x0), Some(&x1)) =
        (rows.first(), rows.last(), cols.first(), cols.last())
    else {
        return Some(ColorGlyph {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            rgba: Vec::new(),
        });
    };
    let mut rgba = Vec::with_capacity((x1 - x0 + 1) * (y1 - y0 + 1) * 4);
    for y in y0..=y1 {
        for x in x0..=x1 {
            rgba.extend_from_slice(&quantized[y * width + x]);
        }
    }
    Some(ColorGlyph {
        left: left as i32 + x0 as i32,
        top: top as i32 - y0 as i32,
        width: (x1 - x0 + 1) as u32,
        height: (y1 - y0 + 1) as u32,
        rgba,
    })
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

/// Row-major 2x3 affine map, `x' = a x + c y + e`, `y' = b x + d y + f`.
#[derive(Clone, Copy, Debug)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}
impl Affine {
    /// `self` after `t`: points are mapped by `t` first.
    fn then(self, t: Affine) -> Affine {
        Affine {
            a: self.a * t.a + self.c * t.b,
            b: self.b * t.a + self.d * t.b,
            c: self.a * t.c + self.c * t.d,
            d: self.b * t.c + self.d * t.d,
            e: self.a * t.e + self.c * t.f + self.e,
            f: self.b * t.e + self.d * t.f + self.f,
        }
    }
    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
    fn invert(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }
}

/// `sin` and `cos` of `turns * π` (COLRv1 angles are in half-turns), by range
/// reduction and Taylor polynomials: no platform libm.
fn sin_cos_half_turns(turns: f32) -> (f32, f32) {
    // Reduce to [-1, 1) half-turns, then fold into [-0.5, 0.5], where
    // sin(π t) = sin(π (±1 - t)) and cos(π t) = -cos(π (±1 - t)).
    let t = turns - 2.0 * ((turns + 1.0) * 0.5).floor();
    let (t, negate_cos) = if t > 0.5 {
        (1.0 - t, true)
    } else if t < -0.5 {
        (-1.0 - t, true)
    } else {
        (t, false)
    };
    let x = t * std::f32::consts::PI;
    let x2 = x * x;
    let sin = x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))));
    let cos = 1.0 - x2 / 2.0 * (1.0 - x2 / 12.0 * (1.0 - x2 / 30.0 * (1.0 - x2 / 56.0)));
    (sin, if negate_cos { -cos } else { cos })
}

/// `atan2(y, x)` in half-turns (`[-1, 1]`), by octant reduction and a minimax
/// polynomial for `atan` on `[0, 1]` (error below 1e-5 rad).
fn atan2_half_turns(y: f32, x: f32) -> f32 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let (ax, ay) = (x.abs(), y.abs());
    let (num, den, swap) = if ay > ax {
        (ax, ay, true)
    } else {
        (ay, ax, false)
    };
    let z = num / den;
    let z2 = z * z;
    let mut a = z
        * (0.999_866
            + z2 * (-0.330_299_5 + z2 * (0.180_141 + z2 * (-0.085_133 + z2 * 0.020_835_1))));
    if swap {
        a = std::f32::consts::FRAC_PI_2 - a;
    }
    if x < 0.0 {
        a = std::f32::consts::PI - a;
    }
    if y < 0.0 {
        a = -a;
    }
    a / std::f32::consts::PI
}

struct Canvas<'f, 'a> {
    face: &'f ttf_parser::Face<'a>,
    width: usize,
    height: usize,
    transforms: Vec<Affine>,
    /// The current outline, as segments in raster pixels.
    path: Vec<[(f32, f32); 2]>,
    clips: Vec<Vec<u8>>,
    layers: Vec<(CompositeMode, Vec<[f32; 4]>)>,
}

struct Flatten<'p> {
    t: Affine,
    out: &'p mut Vec<[(f32, f32); 2]>,
    start: (f32, f32),
    pen: (f32, f32),
}
impl Flatten<'_> {
    fn to(&mut self, p: (f32, f32)) {
        if p != self.pen {
            self.out.push([self.pen, p]);
        }
        self.pen = p;
    }
}
impl OutlineBuilder for Flatten<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.start = self.t.apply(x, y);
        self.pen = self.start;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.t.apply(x, y);
        self.to(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.pen, self.t.apply(x1, y1), self.t.apply(x, y));
        let dev = ((p0.0 - 2.0 * p1.0 + p2.0).abs()).max((p0.1 - 2.0 * p1.1 + p2.1).abs());
        let n = ((dev * 2.0).sqrt().ceil() as usize).clamp(1, 16);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            self.to((
                u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
                u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
            ));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, p1, p2, p3) = (
            self.pen,
            self.t.apply(x1, y1),
            self.t.apply(x2, y2),
            self.t.apply(x, y),
        );
        let dev = [
            (p0.0 - 2.0 * p1.0 + p2.0).abs(),
            (p0.1 - 2.0 * p1.1 + p2.1).abs(),
            (p1.0 - 2.0 * p2.0 + p3.0).abs(),
            (p1.1 - 2.0 * p2.1 + p3.1).abs(),
        ]
        .into_iter()
        .fold(0.0f32, f32::max);
        let n = ((dev * 3.0).sqrt().ceil() as usize).clamp(1, 24);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.to((
                a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
                a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
            ));
        }
    }
    fn close(&mut self) {
        let start = self.start;
        self.to(start);
    }
}

/// Nonzero-winding coverage of `segments` over a `width`×`height` grid.
fn fill(segments: &[[(f32, f32); 2]], width: usize, height: usize) -> Vec<u8> {
    const SUB: i64 = 16; // sub-scanlines per pixel
    const ONE: i64 = 256; // fixed-point units per pixel
    let fixed = |v: f32| (v * ONE as f32 + 0.5).floor() as i64;
    let lines = height * SUB as usize;
    let mut crossings: Vec<Vec<(i64, i32)>> = vec![Vec::new(); lines];
    for [(x0, y0), (x1, y1)] in segments {
        let (x0, y0, x1, y1) = (fixed(*x0), fixed(*y0), fixed(*x1), fixed(*y1));
        if y0 == y1 {
            continue;
        }
        let (dir, (xa, ya, xb, yb)) = if y1 > y0 {
            (1, (x0, y0, x1, y1))
        } else {
            (-1, (x1, y1, x0, y0))
        };
        // Sub-scanline k samples y = k * 16 + 8 (fixed units); half-open [ya, yb).
        let ceil = |a: i64| (a + 15).div_euclid(16);
        let (first, last) = (ceil(ya - 8).max(0), ceil(yb - 8).min(lines as i64));
        for k in first..last {
            let sy = k * 16 + 8;
            let x = xa + (sy - ya) * (xb - xa) / (yb - ya);
            crossings[k as usize].push((x, dir));
        }
    }
    let mut acc = vec![0i64; width * height];
    let limit = width as i64 * ONE;
    for (k, row) in crossings.iter_mut().enumerate() {
        if row.is_empty() {
            continue;
        }
        row.sort_unstable();
        let y = k / SUB as usize;
        let cells = &mut acc[y * width..(y + 1) * width];
        let mut winding = 0;
        for w in 0..row.len() - 1 {
            winding += row[w].1;
            if winding == 0 {
                continue;
            }
            let (a, b) = (row[w].0.clamp(0, limit), row[w + 1].0.clamp(0, limit));
            if a >= b {
                continue;
            }
            let (pa, pb) = ((a / ONE) as usize, ((b - 1) / ONE) as usize);
            if pa == pb {
                cells[pa] += b - a;
            } else {
                cells[pa] += (pa as i64 + 1) * ONE - a;
                for c in &mut cells[pa + 1..pb] {
                    *c += ONE;
                }
                cells[pb] += b - pb as i64 * ONE;
            }
        }
    }
    let full = SUB * ONE;
    acc.into_iter()
        .map(|a| ((a.min(full) * 255 + full / 2) / full) as u8)
        .collect()
}

impl Canvas<'_, '_> {
    fn transform(&self) -> Affine {
        *self.transforms.last().expect("base transform")
    }
    fn clip(&self) -> &[u8] {
        self.clips.last().expect("root clip")
    }
    fn push_mask(&mut self, mask: Vec<u8>) {
        let clip = self
            .clip()
            .iter()
            .zip(mask)
            .map(|(&a, b)| ((u32::from(a) * u32::from(b) + 127) / 255) as u8)
            .collect();
        self.clips.push(clip);
    }
    fn push(&mut self, t: Affine) {
        let next = self.transform().then(t);
        self.transforms.push(next);
    }
}

fn premultiply(c: RgbaColor) -> [f32; 4] {
    let a = f32::from(c.alpha) / 255.0;
    [
        f32::from(c.red) / 255.0 * a,
        f32::from(c.green) / 255.0 * a,
        f32::from(c.blue) / 255.0 * a,
        a,
    ]
}

/// A gradient's colour line: stops (offset, premultiplied colour) and extend mode.
struct ColorLine {
    stops: Vec<(f32, [f32; 4])>,
    extend: GradientExtend,
}
impl ColorLine {
    fn new(stops: impl Iterator<Item = colr::ColorStop>, extend: GradientExtend) -> Self {
        let mut stops: Vec<(f32, [f32; 4])> = stops
            .map(|s| (s.stop_offset, premultiply(s.color)))
            .collect();
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        Self { stops, extend }
    }
    fn at(&self, t: f32) -> [f32; 4] {
        let (Some(first), Some(last)) = (self.stops.first(), self.stops.last()) else {
            return [0.0; 4];
        };
        if !t.is_finite() {
            return [0.0; 4];
        }
        let (lo, hi) = (first.0, last.0);
        let span = hi - lo;
        let t = if span <= 0.0 {
            t
        } else {
            match self.extend {
                GradientExtend::Pad => t,
                GradientExtend::Repeat => lo + (t - lo) - span * ((t - lo) / span).floor(),
                GradientExtend::Reflect => {
                    let u = (t - lo) / span;
                    let m = u - 2.0 * (u * 0.5).floor();
                    lo + span * if m > 1.0 { 2.0 - m } else { m }
                }
            }
        };
        if t <= lo {
            return first.1;
        }
        if t >= hi {
            return last.1;
        }
        for pair in self.stops.windows(2) {
            let ((a, ca), (b, cb)) = (pair[0], pair[1]);
            if t >= a && t <= b {
                if b <= a {
                    return cb;
                }
                let f = (t - a) / (b - a);
                return [0, 1, 2, 3].map(|i| ca[i] + (cb[i] - ca[i]) * f);
            }
        }
        last.1
    }
}

/// W3C separable blend functions on unpremultiplied channels.
fn blend_channel(mode: CompositeMode, cb: f32, cs: f32) -> f32 {
    match mode {
        CompositeMode::Multiply => cb * cs,
        CompositeMode::Screen => cb + cs - cb * cs,
        CompositeMode::Overlay => blend_channel(CompositeMode::HardLight, cs, cb),
        CompositeMode::Darken => cb.min(cs),
        CompositeMode::Lighten => cb.max(cs),
        CompositeMode::ColorDodge => {
            if cb == 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        CompositeMode::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        CompositeMode::HardLight => {
            if cs <= 0.5 {
                cb * 2.0 * cs
            } else {
                let s = 2.0 * cs - 1.0;
                cb + s - cb * s
            }
        }
        CompositeMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    cb.sqrt()
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        CompositeMode::Difference => (cb - cs).abs(),
        CompositeMode::Exclusion => cb + cs - 2.0 * cb * cs,
        // Non-separable modes (hue, saturation, color, luminosity) are not used by
        // the bundled font; they composite as the source.
        _ => cs,
    }
}

/// Composite premultiplied `s` over backdrop `d` with a COLRv1 composite mode.
fn composite(mode: CompositeMode, s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    let (sa, da) = (s[3], d[3]);
    let porter = |fa: f32, fb: f32| [0, 1, 2, 3].map(|i| s[i] * fa + d[i] * fb);
    match mode {
        CompositeMode::Clear => [0.0; 4],
        CompositeMode::Source => s,
        CompositeMode::Destination => d,
        CompositeMode::SourceOver => porter(1.0, 1.0 - sa),
        CompositeMode::DestinationOver => porter(1.0 - da, 1.0),
        CompositeMode::SourceIn => porter(da, 0.0),
        CompositeMode::DestinationIn => porter(0.0, sa),
        CompositeMode::SourceOut => porter(1.0 - da, 0.0),
        CompositeMode::DestinationOut => porter(0.0, 1.0 - sa),
        CompositeMode::SourceAtop => porter(da, 1.0 - sa),
        CompositeMode::DestinationAtop => porter(1.0 - da, sa),
        CompositeMode::Xor => porter(1.0 - da, 1.0 - sa),
        CompositeMode::Plus => [0, 1, 2, 3].map(|i| (s[i] + d[i]).min(1.0)),
        blend => {
            let mut out = [0.0; 4];
            for i in 0..3 {
                let cs = if sa > 0.0 { s[i] / sa } else { 0.0 };
                let cb = if da > 0.0 { d[i] / da } else { 0.0 };
                out[i] = s[i] * (1.0 - da)
                    + d[i] * (1.0 - sa)
                    + sa * da * blend_channel(blend, cb, cs).clamp(0.0, 1.0);
            }
            out[3] = sa + da * (1.0 - sa);
            out
        }
    }
}

impl<'a> colr::Painter<'a> for Canvas<'_, 'a> {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        self.path.clear();
        let mut flatten = Flatten {
            t: self.transform(),
            out: &mut self.path,
            start: (0.0, 0.0),
            pen: (0.0, 0.0),
        };
        self.face.outline_glyph(glyph_id, &mut flatten);
        flatten.close();
    }

    fn paint(&mut self, paint: Paint<'a>) {
        let (w, h) = (self.width, self.height);
        let inverse = self.transform().invert();
        let clip = self.clips.last().expect("root clip").clone();
        let source: Box<dyn Fn(f32, f32) -> Option<[f32; 4]>> = match paint {
            Paint::Solid(color) => {
                let c = premultiply(color);
                Box::new(move |_, _| Some(c))
            }
            Paint::LinearGradient(g) => {
                let Some(inv) = inverse else { return };
                let line = ColorLine::new(g.stops(0, &[]), g.extend);
                // The gradient runs from p0 to the projection p3 of p1 onto the
                // line through p0 perpendicular to p0→p2.
                let (d1x, d1y) = (g.x1 - g.x0, g.y1 - g.y0);
                let (px, py) = (g.y2 - g.y0, -(g.x2 - g.x0));
                let pp = px * px + py * py;
                let (vx, vy) = if pp == 0.0 {
                    (d1x, d1y)
                } else {
                    let k = (d1x * px + d1y * py) / pp;
                    (px * k, py * k)
                };
                let len = vx * vx + vy * vy;
                if len == 0.0 {
                    return;
                }
                let (x0, y0) = (g.x0, g.y0);
                Box::new(move |x, y| {
                    let (u, v) = inv.apply(x, y);
                    Some(line.at(((u - x0) * vx + (v - y0) * vy) / len))
                })
            }
            Paint::RadialGradient(g) => {
                let Some(inv) = inverse else { return };
                let line = ColorLine::new(g.stops(0, &[]), g.extend);
                let (c0x, c0y, r0) = (g.x0, g.y0, g.r0);
                let (cdx, cdy, dr) = (g.x1 - g.x0, g.y1 - g.y0, g.r1 - g.r0);
                let a = cdx * cdx + cdy * cdy - dr * dr;
                Box::new(move |x, y| {
                    let (u, v) = inv.apply(x, y);
                    let (pdx, pdy) = (u - c0x, v - c0y);
                    let b = pdx * cdx + pdy * cdy + r0 * dr;
                    let c = pdx * pdx + pdy * pdy - r0 * r0;
                    let valid = |t: f32| r0 + t * dr >= 0.0;
                    let t = if a.abs() < 1e-6 {
                        if b == 0.0 {
                            return None;
                        }
                        let t = c / (2.0 * b);
                        valid(t).then_some(t)?
                    } else {
                        let disc = b * b - a * c;
                        if disc < 0.0 {
                            return None;
                        }
                        let sq = disc.sqrt();
                        let (t1, t2) = ((b + sq) / a, (b - sq) / a);
                        let (hi, lo) = if t1 > t2 { (t1, t2) } else { (t2, t1) };
                        if valid(hi) {
                            hi
                        } else if valid(lo) {
                            lo
                        } else {
                            return None;
                        }
                    };
                    Some(line.at(t))
                })
            }
            Paint::SweepGradient(g) => {
                let Some(inv) = inverse else { return };
                let line = ColorLine::new(g.stops(0, &[]), g.extend);
                let (cx, cy) = (g.center_x, g.center_y);
                // Angles are in half-turns, counter-clockwise in font space.
                let (start, end) = (g.start_angle, g.end_angle);
                let span = end - start;
                Box::new(move |x, y| {
                    let (u, v) = inv.apply(x, y);
                    let mut angle = atan2_half_turns(v - cy, u - cx);
                    if angle < 0.0 {
                        angle += 2.0;
                    }
                    if span == 0.0 {
                        return None;
                    }
                    Some(line.at((angle - start) / span))
                })
            }
        };
        let layer = &mut self.layers.last_mut().expect("root layer").1;
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let coverage = clip[i];
                if coverage == 0 {
                    continue;
                }
                let Some(color) = source(x as f32 + 0.5, y as f32 + 0.5) else {
                    continue;
                };
                let k = f32::from(coverage) / 255.0;
                let s = color.map(|c| c * k);
                layer[i] = composite(CompositeMode::SourceOver, s, layer[i]);
            }
        }
    }

    fn push_clip(&mut self) {
        let mask = fill(&self.path, self.width, self.height);
        self.push_mask(mask);
    }

    fn push_clip_box(&mut self, clipbox: ClipBox) {
        let t = self.transform();
        let corners = [
            (clipbox.x_min, clipbox.y_min),
            (clipbox.x_max, clipbox.y_min),
            (clipbox.x_max, clipbox.y_max),
            (clipbox.x_min, clipbox.y_max),
        ]
        .map(|(x, y)| t.apply(x, y));
        let segments: Vec<[(f32, f32); 2]> =
            (0..4).map(|i| [corners[i], corners[(i + 1) % 4]]).collect();
        let mask = fill(&segments, self.width, self.height);
        self.push_mask(mask);
    }

    fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    fn push_layer(&mut self, mode: CompositeMode) {
        self.layers
            .push((mode, vec![[0.0; 4]; self.width * self.height]));
    }

    fn pop_layer(&mut self) {
        if self.layers.len() < 2 {
            return;
        }
        let (mode, source) = self.layers.pop().expect("checked");
        let clip = self.clips.last().expect("root clip").clone();
        let dest = &mut self.layers.last_mut().expect("checked").1;
        for (i, d) in dest.iter_mut().enumerate() {
            let k = f32::from(clip[i]) / 255.0;
            if k == 0.0 {
                continue;
            }
            let out = composite(mode, source[i], *d);
            *d = [0, 1, 2, 3].map(|c| d[c] + (out[c] - d[c]) * k);
        }
    }

    fn push_translate(&mut self, tx: f32, ty: f32) {
        self.push(Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: ty,
        });
    }

    fn push_scale(&mut self, sx: f32, sy: f32) {
        self.push(Affine {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: 0.0,
            f: 0.0,
        });
    }

    fn push_rotate(&mut self, angle: f32) {
        let (sin, cos) = sin_cos_half_turns(angle);
        self.push(Affine {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        });
    }

    fn push_skew(&mut self, skew_x: f32, skew_y: f32) {
        let tan = |turns: f32| {
            let (s, c) = sin_cos_half_turns(turns);
            if c == 0.0 {
                0.0
            } else {
                s / c
            }
        };
        // COLRv1 skews are clockwise in x (counter-clockwise angles lean left).
        self.push(Affine {
            a: 1.0,
            b: tan(skew_y),
            c: -tan(skew_x),
            d: 1.0,
            e: 0.0,
            f: 0.0,
        });
    }

    fn push_transform(&mut self, t: ttf_parser::Transform) {
        self.push(Affine {
            a: t.a,
            b: t.b,
            c: t.c,
            d: t.d,
            e: t.e,
            f: t.f,
        });
    }

    fn pop_transform(&mut self) {
        if self.transforms.len() > 1 {
            self.transforms.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polynomial_trigonometry_is_accurate() {
        for i in -40..=40 {
            let turns = i as f32 / 10.0;
            let (s, c) = sin_cos_half_turns(turns);
            let x = f64::from(turns) * std::f64::consts::PI;
            assert!((f64::from(s) - x.sin()).abs() < 1e-4, "{turns}");
            assert!((f64::from(c) - x.cos()).abs() < 1e-4, "{turns}");
        }
        for (y, x) in [
            (1.0, 1.0),
            (1.0, -2.0),
            (-3.0, -1.0),
            (-0.5, 4.0),
            (0.0, -1.0),
        ] {
            let a = f64::from(atan2_half_turns(y, x)) * std::f64::consts::PI;
            assert!(
                (a - f64::from(y).atan2(f64::from(x))).abs() < 1e-4,
                "{y} {x}"
            );
        }
    }

    #[test]
    fn scanline_fill_is_exact_for_rectangles_and_holes() {
        let rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
            vec![
                [(x0, y0), (x1, y0)],
                [(x1, y0), (x1, y1)],
                [(x1, y1), (x0, y1)],
                [(x0, y1), (x0, y0)],
            ]
        };
        let mask = fill(&rect(1.0, 1.0, 3.5, 3.0), 5, 4);
        assert_eq!(&mask[5..10], &[0, 255, 255, 128, 0]);
        assert_eq!(&mask[0..5], &[0; 5]);
        // An inner square wound the other way is a hole; the same way, nonzero fill.
        let mut ring = rect(0.0, 0.0, 6.0, 6.0);
        ring.extend(rect(2.0, 4.0, 4.0, 2.0));
        let mask = fill(&ring, 6, 6);
        assert_eq!(mask[3 * 6 + 3], 0);
        assert_eq!(mask[6], 255);
        let mut doubled = rect(0.0, 0.0, 6.0, 6.0);
        doubled.extend(rect(2.0, 2.0, 4.0, 4.0));
        assert_eq!(fill(&doubled, 6, 6)[3 * 6 + 3], 255);
    }

    #[test]
    fn composite_modes_follow_porter_duff() {
        let red = [1.0, 0.0, 0.0, 1.0];
        let half_blue = [0.0, 0.0, 0.5, 0.5];
        assert_eq!(
            composite(CompositeMode::SourceOver, half_blue, red),
            [0.5, 0.0, 0.5, 1.0]
        );
        assert_eq!(
            composite(CompositeMode::SourceIn, red, half_blue),
            [0.5, 0.0, 0.0, 0.5]
        );
        assert_eq!(
            composite(CompositeMode::DestinationOut, red, half_blue),
            [0.0, 0.0, 0.0, 0.0]
        );
        // Soft light with mid grey leaves the backdrop unchanged.
        let grey = [0.5, 0.5, 0.5, 1.0];
        let out = composite(CompositeMode::SoftLight, grey, red);
        assert!((out[0] - 1.0).abs() < 1e-6 && out[1].abs() < 1e-6);
    }
}
