//! A recording, software-rasterised `CanvasRenderingContext2D`: paths flattened to
//! polygons and filled by scanline (non-zero or even-odd winding, no
//! anti-aliasing), strokes as per-segment quads, linear gradients, image blits,
//! `getImageData`/`putImageData`, text as glyph boxes sized from the metrics tables
//! (an approximation: no glyph outlines are available here), and a PNG encoder
//! (stored deflate) for `toDataURL`. Arithmetic is `f64` with only the basic
//! operations, so results are identical on every platform.

use crate::paint::RgbaImage;
use crate::style::values::{parse_color, ColorSpec, Parser};
use cw_scene::Color;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Paint {
    Solid(Color),
    Linear {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        stops: Vec<(f64, Color)>,
    },
    Radial {
        x: f64,
        y: f64,
        r: f64,
        stops: Vec<(f64, Color)>,
    },
}

impl Paint {
    fn at(&self, x: f64, y: f64) -> Color {
        match self {
            Paint::Solid(c) => *c,
            Paint::Linear {
                x0,
                y0,
                x1,
                y1,
                stops,
            } => {
                let dx = x1 - x0;
                let dy = y1 - y0;
                let len2 = dx * dx + dy * dy;
                let t = if len2 == 0.0 {
                    0.0
                } else {
                    ((x - x0) * dx + (y - y0) * dy) / len2
                };
                stop_color(stops, t)
            }
            Paint::Radial {
                x: cx,
                y: cy,
                r,
                stops,
            } => {
                let d = ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt();
                let t = if *r <= 0.0 { 1.0 } else { d / r };
                stop_color(stops, t)
            }
        }
    }
}

fn stop_color(stops: &[(f64, Color)], t: f64) -> Color {
    if stops.is_empty() {
        return Color(0, 0, 0, 0);
    }
    let t = t.clamp(0.0, 1.0);
    if t <= stops[0].0 {
        return stops[0].1;
    }
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            let mix = |a: u8, b: u8| {
                ((a as f64) + (b as f64 - a as f64) * f)
                    .round()
                    .clamp(0.0, 255.0) as u8
            };
            return Color(
                mix(c0.0, c1.0),
                mix(c0.1, c1.1),
                mix(c0.2, c1.2),
                mix(c0.3, c1.3),
            );
        }
    }
    stops[stops.len() - 1].1
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawState {
    pub fill: Paint,
    pub stroke: Paint,
    pub fill_text: String,
    pub stroke_text: String,
    pub line_width: f64,
    pub global_alpha: f64,
    pub font: String,
    pub font_size: f64,
    pub font_bold: bool,
    pub font_italic: bool,
    pub font_family: String,
    pub text_align: String,
    pub text_baseline: String,
    /// `[a, b, c, d, e, f]`: x' = a x + c y + e, y' = b x + d y + f.
    pub transform: [f64; 6],
    pub clip: Option<(f64, f64, f64, f64)>,
    pub line_cap: String,
    pub line_join: String,
    pub composite: String,
    pub image_smoothing: bool,
    pub line_dash: Vec<f64>,
    pub shadow_blur: f64,
    pub shadow_color: String,
    pub miter_limit: f64,
}

impl Default for DrawState {
    fn default() -> Self {
        DrawState {
            fill: Paint::Solid(Color(0, 0, 0, 255)),
            stroke: Paint::Solid(Color(0, 0, 0, 255)),
            fill_text: "#000000".into(),
            stroke_text: "#000000".into(),
            line_width: 1.0,
            global_alpha: 1.0,
            font: "10px sans-serif".into(),
            font_size: 10.0,
            font_bold: false,
            font_italic: false,
            font_family: "sans-serif".into(),
            text_align: "start".into(),
            text_baseline: "alphabetic".into(),
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            clip: None,
            line_cap: "butt".into(),
            line_join: "miter".into(),
            composite: "source-over".into(),
            image_smoothing: true,
            line_dash: Vec::new(),
            shadow_blur: 0.0,
            shadow_color: "rgba(0, 0, 0, 0)".into(),
            miter_limit: 10.0,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CanvasState {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub state: DrawState,
    pub stack: Vec<DrawState>,
    /// Sub-paths in device space (already transformed), with their closed flag.
    pub path: Vec<(Vec<(f64, f64)>, bool)>,
    /// The current point in user space.
    cur: Option<(f64, f64)>,
    /// Drawing commands happened (the canvas is not blank).
    pub dirty: bool,
}

impl CanvasState {
    pub fn new(width: u32, height: u32) -> CanvasState {
        let w = width.clamp(0, 16384);
        let h = height.clamp(0, 16384);
        CanvasState {
            width: w,
            height: h,
            pixels: vec![0; (w * h * 4) as usize],
            ..Default::default()
        }
    }

    pub fn image(&self) -> RgbaImage {
        RgbaImage::new(self.width, self.height, self.pixels.clone())
    }

    /// Setting `width`/`height` resets the bitmap and state.
    pub fn resize(&mut self, width: u32, height: u32) {
        *self = CanvasState::new(width, height);
    }

    // ------------------------------------------------------------ transforms

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let t = self.state.transform;
        (t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5])
    }

    pub fn transform(&mut self, m: [f64; 6]) {
        let t = self.state.transform;
        self.state.transform = [
            t[0] * m[0] + t[2] * m[1],
            t[1] * m[0] + t[3] * m[1],
            t[0] * m[2] + t[2] * m[3],
            t[1] * m[2] + t[3] * m[3],
            t[0] * m[4] + t[2] * m[5] + t[4],
            t[1] * m[4] + t[3] * m[5] + t[5],
        ];
    }
    pub fn set_transform(&mut self, m: [f64; 6]) {
        self.state.transform = m;
    }
    pub fn translate(&mut self, x: f64, y: f64) {
        self.transform([1.0, 0.0, 0.0, 1.0, x, y]);
    }
    pub fn scale(&mut self, x: f64, y: f64) {
        self.transform([x, 0.0, 0.0, y, 0.0, 0.0]);
    }
    pub fn rotate(&mut self, angle: f64) {
        let (s, c) = sin_cos(angle);
        self.transform([c, s, -s, c, 0.0, 0.0]);
    }
    pub fn save(&mut self) {
        self.stack.push(self.state.clone());
    }
    pub fn restore(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }

    // ------------------------------------------------------------ paths

    pub fn begin_path(&mut self) {
        self.path.clear();
        self.cur = None;
    }
    pub fn move_to(&mut self, x: f64, y: f64) {
        let p = self.apply(x, y);
        self.path.push((vec![p], false));
        self.cur = Some((x, y));
    }
    pub fn line_to(&mut self, x: f64, y: f64) {
        if self.path.is_empty() || self.cur.is_none() {
            self.move_to(x, y);
            return;
        }
        let p = self.apply(x, y);
        self.path.last_mut().unwrap().0.push(p);
        self.cur = Some((x, y));
    }
    pub fn close_path(&mut self) {
        if let Some(last) = self.path.last_mut() {
            last.1 = true;
            if let Some(first) = last.0.first().copied() {
                let t = self.state.transform;
                // Back to user space for the current point.
                let det = t[0] * t[3] - t[1] * t[2];
                if det != 0.0 {
                    let x = ((first.0 - t[4]) * t[3] - (first.1 - t[5]) * t[2]) / det;
                    let y = ((first.1 - t[5]) * t[0] - (first.0 - t[4]) * t[1]) / det;
                    self.cur = Some((x, y));
                }
                let sub = (vec![first], false);
                self.path.push(sub);
            }
        }
    }
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close_path();
        self.cur = Some((x, y));
    }
    pub fn arc(&mut self, cx: f64, cy: f64, r: f64, start: f64, end: f64, anticlockwise: bool) {
        self.ellipse(cx, cy, r, r, 0.0, start, end, anticlockwise);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(
        &mut self,
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        rotation: f64,
        start: f64,
        end: f64,
        anticlockwise: bool,
    ) {
        let tau = std::f64::consts::PI * 2.0;
        let mut sweep = end - start;
        if anticlockwise {
            if sweep >= tau {
                sweep = -tau;
            } else {
                sweep = sweep.rem_euclid(tau);
                if sweep > 0.0 {
                    sweep -= tau;
                }
                if sweep == 0.0 && end != start {
                    sweep = -tau;
                }
            }
        } else if sweep >= tau {
            sweep = tau;
        } else {
            sweep = sweep.rem_euclid(tau);
            if sweep == 0.0 && end != start {
                sweep = tau;
            }
        }
        let steps = ((sweep.abs() * rx.max(ry).max(1.0) / 3.0).ceil() as usize).clamp(8, 720);
        let (rs, rc) = sin_cos(rotation);
        for i in 0..=steps {
            let a = start + sweep * (i as f64) / (steps as f64);
            let (s, c) = sin_cos(a);
            let px = rx * c;
            let py = ry * s;
            let x = cx + px * rc - py * rs;
            let y = cy + px * rs + py * rc;
            if i == 0 {
                if self.path.is_empty() || self.cur.is_none() {
                    self.move_to(x, y);
                } else {
                    self.line_to(x, y);
                }
            } else {
                self.line_to(x, y);
            }
        }
    }
    pub fn arc_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, _r: f64) {
        self.line_to(x1, y1);
        self.line_to(x2, y2);
    }
    pub fn quadratic_curve_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        let (x0, y0) = self.cur.unwrap_or((cx, cy));
        for i in 1..=16 {
            let t = i as f64 / 16.0;
            let u = 1.0 - t;
            let px = u * u * x0 + 2.0 * u * t * cx + t * t * x;
            let py = u * u * y0 + 2.0 * u * t * cy + t * t * y;
            self.line_to(px, py);
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bezier_curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        let (x0, y0) = self.cur.unwrap_or((c1x, c1y));
        for i in 1..=24 {
            let t = i as f64 / 24.0;
            let u = 1.0 - t;
            let px = u * u * u * x0 + 3.0 * u * u * t * c1x + 3.0 * u * t * t * c2x + t * t * t * x;
            let py = u * u * u * y0 + 3.0 * u * u * t * c1y + 3.0 * u * t * t * c2y + t * t * t * y;
            self.line_to(px, py);
        }
    }

    pub fn fill(&mut self, even_odd: bool) {
        let polys: Vec<Vec<(f64, f64)>> = self
            .path
            .iter()
            .filter(|(p, _)| p.len() >= 3)
            .map(|(p, _)| p.clone())
            .collect();
        let paint = self.state.fill.clone();
        self.fill_polygons(&polys, even_odd, &paint);
    }

    pub fn stroke(&mut self) {
        let w = self.state.line_width.max(0.0);
        if w == 0.0 {
            return;
        }
        // Device-space half width from the transform's scale.
        let t = self.state.transform;
        let sx = (t[0] * t[0] + t[1] * t[1]).sqrt();
        let sy = (t[2] * t[2] + t[3] * t[3]).sqrt();
        let hw = w * (sx + sy) / 4.0;
        let mut quads: Vec<Vec<(f64, f64)>> = Vec::new();
        for (pts, closed) in &self.path {
            let n = pts.len();
            if n == 0 {
                continue;
            }
            if n == 1 {
                continue;
            }
            let segs = if *closed { n } else { n - 1 };
            for i in 0..segs {
                let (x0, y0) = pts[i];
                let (x1, y1) = pts[(i + 1) % n];
                let dx = x1 - x0;
                let dy = y1 - y0;
                let len = (dx * dx + dy * dy).sqrt();
                if len == 0.0 {
                    continue;
                }
                let nx = -dy / len * hw;
                let ny = dx / len * hw;
                quads.push(vec![
                    (x0 + nx, y0 + ny),
                    (x1 + nx, y1 + ny),
                    (x1 - nx, y1 - ny),
                    (x0 - nx, y0 - ny),
                ]);
                // A square joint at each vertex fills the gap between segments.
                if hw > 0.75 {
                    quads.push(vec![
                        (x1 - hw, y1 - hw),
                        (x1 + hw, y1 - hw),
                        (x1 + hw, y1 + hw),
                        (x1 - hw, y1 + hw),
                    ]);
                    if i == 0 && !*closed {
                        quads.push(vec![
                            (x0 - hw, y0 - hw),
                            (x0 + hw, y0 - hw),
                            (x0 + hw, y0 + hw),
                            (x0 - hw, y0 + hw),
                        ]);
                    }
                }
            }
        }
        let paint = self.state.stroke.clone();
        for q in quads {
            self.fill_polygons(&[q], false, &paint);
        }
    }

    pub fn clip(&mut self) {
        // Approximation: the bounding box of the current path.
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for (pts, _) in &self.path {
            for (x, y) in pts {
                min_x = min_x.min(*x);
                min_y = min_y.min(*y);
                max_x = max_x.max(*x);
                max_y = max_y.max(*y);
            }
        }
        if min_x < max_x && min_y < max_y {
            let c = match self.state.clip {
                Some((cx0, cy0, cx1, cy1)) => (
                    cx0.max(min_x),
                    cy0.max(min_y),
                    cx1.min(max_x),
                    cy1.min(max_y),
                ),
                None => (min_x, min_y, max_x, max_y),
            };
            self.state.clip = Some(c);
        }
    }

    pub fn is_point_in_path(&self, x: f64, y: f64, even_odd: bool) -> bool {
        let polys: Vec<&Vec<(f64, f64)>> = self
            .path
            .iter()
            .filter(|(p, _)| p.len() >= 3)
            .map(|(p, _)| p)
            .collect();
        let mut winding = 0i32;
        let mut crossings = 0u32;
        for poly in polys {
            let n = poly.len();
            for i in 0..n {
                let (x0, y0) = poly[i];
                let (x1, y1) = poly[(i + 1) % n];
                if (y0 <= y) != (y1 <= y) {
                    let xi = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                    if xi > x {
                        crossings += 1;
                        winding += if y1 > y0 { 1 } else { -1 };
                    }
                }
            }
        }
        if even_odd {
            crossings % 2 == 1
        } else {
            winding != 0
        }
    }

    // ------------------------------------------------------------ rectangles

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let poly = vec![
            self.apply(x, y),
            self.apply(x + w, y),
            self.apply(x + w, y + h),
            self.apply(x, y + h),
        ];
        let paint = self.state.fill.clone();
        self.fill_polygons(&[poly], false, &paint);
    }
    pub fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let saved = self.path.clone();
        let cur = self.cur;
        self.begin_path();
        self.rect(x, y, w, h);
        self.stroke();
        self.path = saved;
        self.cur = cur;
    }
    pub fn clear_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let poly = vec![
            self.apply(x, y),
            self.apply(x + w, y),
            self.apply(x + w, y + h),
            self.apply(x, y + h),
        ];
        self.fill_polygons_with(&[poly], false, &mut |_, _| Some(Color(0, 0, 0, 0)), true);
    }

    fn fill_polygons(&mut self, polys: &[Vec<(f64, f64)>], even_odd: bool, paint: &Paint) {
        let alpha = self.state.global_alpha.clamp(0.0, 1.0);
        let p = paint.clone();
        self.fill_polygons_with(
            polys,
            even_odd,
            &mut |x, y| {
                let c = p.at(x, y);
                let a = (c.3 as f64 * alpha).round().clamp(0.0, 255.0) as u8;
                Some(Color(c.0, c.1, c.2, a))
            },
            false,
        );
    }

    /// Scanline fill: for each row, the crossings of every edge with the row's
    /// centre line, sorted; spans are filled by the winding rule.
    fn fill_polygons_with(
        &mut self,
        polys: &[Vec<(f64, f64)>],
        even_odd: bool,
        color: &mut dyn FnMut(f64, f64) -> Option<Color>,
        replace: bool,
    ) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        self.dirty = true;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for poly in polys {
            for (_, y) in poly {
                min_y = min_y.min(*y);
                max_y = max_y.max(*y);
            }
        }
        if min_y >= max_y {
            return;
        }
        let (cx0, cy0, cx1, cy1) =
            self.state
                .clip
                .unwrap_or((0.0, 0.0, self.width as f64, self.height as f64));
        let row0 = (min_y.max(cy0).max(0.0)).floor() as i64;
        let row1 = (max_y.min(cy1).min(self.height as f64)).ceil() as i64;
        let mut xs: Vec<(f64, i32)> = Vec::new();
        for row in row0..row1 {
            let y = row as f64 + 0.5;
            xs.clear();
            for poly in polys {
                let n = poly.len();
                for i in 0..n {
                    let (x0, y0) = poly[i];
                    let (x1, y1) = poly[(i + 1) % n];
                    if (y0 <= y) != (y1 <= y) {
                        let xi = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                        xs.push((xi, if y1 > y0 { 1 } else { -1 }));
                    }
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut winding = 0i32;
            for i in 0..xs.len() - 1 {
                winding += if even_odd { 1 } else { xs[i].1 };
                let inside = if even_odd {
                    winding % 2 == 1
                } else {
                    winding != 0
                };
                if !inside {
                    continue;
                }
                let xa = xs[i].0.max(cx0).max(0.0);
                let xb = xs[i + 1].0.min(cx1).min(self.width as f64);
                let px0 = (xa - 0.5).ceil() as i64;
                let px1 = (xb - 0.5).floor() as i64;
                for px in px0..=px1 {
                    if px < 0 || px >= self.width as i64 {
                        continue;
                    }
                    let Some(c) = color(px as f64 + 0.5, y) else {
                        continue;
                    };
                    let idx = ((row as u32 * self.width + px as u32) * 4) as usize;
                    if replace {
                        self.pixels[idx..idx + 4].copy_from_slice(&[c.0, c.1, c.2, c.3]);
                    } else {
                        blend(&mut self.pixels[idx..idx + 4], c);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ images

    /// Draws `src` (nearest-neighbour) into the destination rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image(
        &mut self,
        src: &RgbaImage,
        sx: f64,
        sy: f64,
        sw: f64,
        sh: f64,
        dx: f64,
        dy: f64,
        dw: f64,
        dh: f64,
    ) {
        if src.width == 0 || src.height == 0 || sw <= 0.0 || sh <= 0.0 || dw == 0.0 || dh == 0.0 {
            return;
        }
        let poly = vec![
            self.apply(dx, dy),
            self.apply(dx + dw, dy),
            self.apply(dx + dw, dy + dh),
            self.apply(dx, dy + dh),
        ];
        let t = self.state.transform;
        let det = t[0] * t[3] - t[1] * t[2];
        if det == 0.0 {
            return;
        }
        let alpha = self.state.global_alpha.clamp(0.0, 1.0);
        let img = src.clone();
        self.fill_polygons_with(
            &[poly],
            false,
            &mut |x, y| {
                // Device to user space, then to source pixels.
                let ux = ((x - t[4]) * t[3] - (y - t[5]) * t[2]) / det;
                let uy = ((y - t[5]) * t[0] - (x - t[4]) * t[1]) / det;
                let fx = (ux - dx) / dw;
                let fy = (uy - dy) / dh;
                let px = (sx + fx * sw).floor();
                let py = (sy + fy * sh).floor();
                if px < 0.0 || py < 0.0 || px >= img.width as f64 || py >= img.height as f64 {
                    return None;
                }
                let i = ((py as u32 * img.width + px as u32) * 4) as usize;
                let a = (img.rgba[i + 3] as f64 * alpha).round() as u8;
                Some(Color(img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], a))
            },
            false,
        );
    }

    pub fn get_image_data(&self, x: i64, y: i64, w: u32, h: u32) -> RgbaImage {
        let mut out = vec![0u8; (w * h * 4) as usize];
        for row in 0..h as i64 {
            for col in 0..w as i64 {
                let sx = x + col;
                let sy = y + row;
                if sx < 0 || sy < 0 || sx >= self.width as i64 || sy >= self.height as i64 {
                    continue;
                }
                let si = ((sy as u32 * self.width + sx as u32) * 4) as usize;
                let di = ((row as u32 * w + col as u32) * 4) as usize;
                out[di..di + 4].copy_from_slice(&self.pixels[si..si + 4]);
            }
        }
        RgbaImage::new(w, h, out)
    }

    pub fn put_image_data(&mut self, img: &RgbaImage, x: i64, y: i64) {
        self.dirty = true;
        for row in 0..img.height as i64 {
            for col in 0..img.width as i64 {
                let dx = x + col;
                let dy = y + row;
                if dx < 0 || dy < 0 || dx >= self.width as i64 || dy >= self.height as i64 {
                    continue;
                }
                let si = ((row as u32 * img.width + col as u32) * 4) as usize;
                let di = ((dy as u32 * self.width + dx as u32) * 4) as usize;
                self.pixels[di..di + 4].copy_from_slice(&img.rgba[si..si + 4]);
            }
        }
    }

    // ------------------------------------------------------------ text

    fn typeface(&self) -> (cw_scene::Typeface, cw_scene::Style) {
        let tf = cw_scene::fonts::resolve_family(&self.state.font_family);
        (
            tf,
            cw_scene::Style::new(
                self.state.font_bold,
                self.state.font_italic,
                cw_scene::Lang::default(),
            )
            .for_web(),
        )
    }

    /// The advance width of `text` in the current font, in CSS px.
    pub fn measure_text(&self, text: &str) -> f64 {
        let (tf, st) = self.typeface();
        let size = self.state.font_size.round().clamp(1.0, 4096.0) as u16;
        cw_scene::metrics::text_width(tf, st, text, size) as f64
    }

    /// Draws text as glyph boxes: each glyph a filled rectangle of its advance width
    /// (inset a little) and cap height, placed by `textAlign` and `textBaseline`.
    pub fn fill_text(&mut self, text: &str, x: f64, y: f64, max_width: Option<f64>, stroke: bool) {
        let (tf, st) = self.typeface();
        let size_px = self.state.font_size.round().clamp(1.0, 4096.0) as u16;
        let total = self.measure_text(text);
        let scale = match max_width {
            Some(m) if m > 0.0 && total > m => m / total,
            _ => 1.0,
        };
        let width = total * scale;
        let start_x = match self.state.text_align.as_str() {
            "center" => x - width / 2.0,
            "right" | "end" => x - width,
            _ => x,
        };
        let size = self.state.font_size;
        let ascent = size * 0.75;
        let descent = size * 0.25;
        let baseline = match self.state.text_baseline.as_str() {
            "top" | "hanging" => y + ascent,
            "middle" => y + ascent / 2.0 - descent / 2.0 + descent * 0.5,
            "bottom" | "ideographic" => y - descent,
            _ => y,
        };
        let paint = if stroke {
            self.state.stroke.clone()
        } else {
            self.state.fill.clone()
        };
        let mut cx = start_x;
        for ch in text.chars() {
            let adv = cw_scene::metrics::advance(tf, st, ch, size_px) as f64 / 64.0 * scale;
            if !ch.is_whitespace() {
                let inset = (adv * 0.12).min(1.0);
                let glyph_h = if ch.is_lowercase() {
                    size * 0.5
                } else {
                    size * 0.7
                };
                let x0 = cx + inset;
                let x1 = cx + adv - inset;
                let y0 = baseline - glyph_h;
                let y1 = baseline
                    + if matches!(ch, 'g' | 'j' | 'p' | 'q' | 'y') {
                        descent * 0.8
                    } else {
                        0.0
                    };
                let poly = vec![
                    self.apply(x0, y0),
                    self.apply(x1, y0),
                    self.apply(x1, y1),
                    self.apply(x0, y1),
                ];
                if stroke {
                    let saved = std::mem::take(&mut self.path);
                    let cur = self.cur;
                    self.path = vec![(poly, true)];
                    self.stroke();
                    self.path = saved;
                    self.cur = cur;
                } else {
                    self.fill_polygons(&[poly], false, &paint);
                }
            }
            cx += adv;
        }
    }

    /// `toDataURL('image/png')`.
    pub fn to_data_url(&self) -> String {
        let png = encode_png(self.width, self.height, &self.pixels);
        format!("data:image/png;base64,{}", base64(&png))
    }
}

fn blend(dst: &mut [u8], src: Color) {
    let sa = src.3 as u32;
    if sa == 0 {
        return;
    }
    if sa == 255 {
        dst.copy_from_slice(&[src.0, src.1, src.2, 255]);
        return;
    }
    let da = dst[3] as u32;
    let out_a = sa + da * (255 - sa) / 255;
    if out_a == 0 {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }
    let ch = |s: u8, d: u8| -> u8 {
        ((s as u32 * sa + d as u32 * da * (255 - sa) / 255) / out_a).min(255) as u8
    };
    dst[0] = ch(src.0, dst[0]);
    dst[1] = ch(src.1, dst[1]);
    dst[2] = ch(src.2, dst[2]);
    dst[3] = out_a as u8;
}

/// `(sin, cos)` from a Taylor series after range reduction: only basic
/// operations, so identical everywhere.
pub fn sin_cos(a: f64) -> (f64, f64) {
    let tau = std::f64::consts::PI * 2.0;
    let mut x = a % tau;
    if x > std::f64::consts::PI {
        x -= tau;
    } else if x < -std::f64::consts::PI {
        x += tau;
    }
    // Reduce further to [-pi/2, pi/2] for convergence.
    let (mut x, sign) = if x > std::f64::consts::FRAC_PI_2 {
        (std::f64::consts::PI - x, (1.0, -1.0))
    } else if x < -std::f64::consts::FRAC_PI_2 {
        (-std::f64::consts::PI - x, (1.0, -1.0))
    } else {
        (x, (1.0, 1.0))
    };
    if x == 0.0 {
        x = 0.0;
    }
    let x2 = x * x;
    let mut s = x;
    let mut term = x;
    let mut c = 1.0;
    let mut cterm = 1.0;
    for i in 1..12 {
        let k = (2 * i) as f64;
        term *= -x2 / ((k) * (k + 1.0));
        s += term;
        cterm *= -x2 / ((k - 1.0) * k);
        c += cterm;
    }
    (s * sign.0, c * sign.1)
}

/// Parses a CSS colour string as canvas `fillStyle` takes it.
pub fn parse_css_color(s: &str) -> Option<Color> {
    let values = crate::css::parser::parse_component_value_list(s.trim());
    let mut p = Parser::new(&values);
    match parse_color(&mut p)? {
        ColorSpec::Rgba(c) => Some(c),
        ColorSpec::CurrentColor => Some(Color(0, 0, 0, 255)),
        ColorSpec::Mix { .. } => Some(Color(0, 0, 0, 255)),
    }
}

/// Serialises a colour as canvas reads `fillStyle` back (`#rrggbb` or `rgba()`).
pub fn color_string(c: Color) -> String {
    if c.3 == 255 {
        format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
    } else {
        // The shortest decimal that maps back to the same alpha byte.
        let a = c.3 as f64 / 255.0;
        let mut s = String::new();
        for digits in 1..=3 {
            let cand = format!("{:.*}", digits, a);
            let back = (cand.parse::<f64>().unwrap_or(0.0) * 255.0).round() as u8;
            if back == c.3 {
                s = cand;
                break;
            }
        }
        if s.is_empty() {
            s = format!("{:.3}", a);
        }
        while s.contains('.') && s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
        format!("rgba({}, {}, {}, {})", c.0, c.1, c.2, s)
    }
}

/// Parses a canvas `font` shorthand: `[style] [weight] size[/lh] family`.
pub fn parse_font(s: &str) -> Option<(f64, bool, bool, String)> {
    let mut size = None;
    let mut bold = false;
    let mut italic = false;
    let mut family = Vec::new();
    for word in s.split_whitespace() {
        if size.is_some() {
            family.push(word.to_owned());
            continue;
        }
        let w = word.to_ascii_lowercase();
        let sz = w.split('/').next().unwrap_or("");
        if let Some(px) = sz.strip_suffix("px") {
            if let Ok(v) = px.parse::<f64>() {
                size = Some(v);
                continue;
            }
        }
        if let Some(pt) = sz.strip_suffix("pt") {
            if let Ok(v) = pt.parse::<f64>() {
                size = Some(v * 4.0 / 3.0);
                continue;
            }
        }
        if let Some(em) = sz.strip_suffix("em") {
            if let Ok(v) = em.parse::<f64>() {
                size = Some(v * 16.0);
                continue;
            }
        }
        match w.as_str() {
            "bold" | "bolder" | "600" | "700" | "800" | "900" => bold = true,
            "italic" | "oblique" => italic = true,
            "normal" | "small-caps" | "lighter" | "100" | "200" | "300" | "400" | "500" => {}
            _ => return None,
        }
    }
    let size = size?;
    let family = family.join(" ");
    if family.is_empty() {
        return None;
    }
    Some((size, bold, italic, family))
}

// ---------------------------------------------------------------- PNG

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for chunk in data.chunks(5000) {
        for x in chunk {
            a += *x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut c = Vec::with_capacity(4 + data.len());
    c.extend_from_slice(ty);
    c.extend_from_slice(data);
    out.extend_from_slice(&c);
    out.extend_from_slice(&crc32(&c).to_be_bytes());
}

/// Encodes RGBA pixels as a PNG with stored (uncompressed) deflate blocks.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in 0..height as usize {
        raw.push(0);
        let start = row * width as usize * 4;
        raw.extend_from_slice(&rgba[start..start + width as usize * 4]);
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    if raw.is_empty() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(b) = blocks.next() {
        let last = blocks.peek().is_none();
        z.push(if last { 1 } else { 0 });
        let len = b.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
