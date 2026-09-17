//! Deterministic portable CPU compositor. No browser, GPU, host font or clock.
//! A renderer owns disposable glyph/text caches and a retained RGBA framebuffer.
use cw_scene::{text_cell, wrap_text, Color, Damage, Node, Primitive, Rect, Scene, Transform};
use fontdue::{Font, FontSettings};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const FONT_SHA256: &str = "c805f9436dbc268644c1d9584f01a601a653e028e08fd74b9b949f6cf8304d88";
pub const UI_FONT_SHA256: &str = "ae7b7855e115a5966d8b1b3f80f254ccc117ec86f9965e202ee2940453837280";
const UI_FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
const FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Frame {
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        Some(self.rgba[i..i + 4].try_into().unwrap())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RenderStats {
    pub frames: u64,
    pub painted_pixels: u64,
    pub glyph_cache_entries: usize,
    pub text_cache_entries: usize,
}
struct Glyph {
    metrics: fontdue::Metrics,
    alpha: Vec<u8>,
}
#[derive(Clone)]
struct Mask {
    width: u32,
    height: u32,
    alpha: Vec<u8>,
    spans: Vec<(u32, u32, u32)>,
}
/// Cache limits bound resident text memory across arbitrary navigation histories.
pub struct Renderer {
    font: Font,
    ui_font: Option<Font>,
    glyphs: BTreeMap<(bool, char, u16), Arc<Glyph>>,
    texts: BTreeMap<(bool, String, u16, u32, u32), Arc<Mask>>,
    frame: Frame,
    revision: Option<u64>,
    pub stats: RenderStats,
}
impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}
impl Renderer {
    pub fn new() -> Self {
        Self {
            font: Font::from_bytes(FONT_BYTES, FontSettings::default())
                .expect("bundled font is valid"),
            ui_font: None,
            glyphs: BTreeMap::new(),
            texts: BTreeMap::new(),
            frame: Frame::default(),
            revision: None,
            stats: RenderStats::default(),
        }
    }
    /// Convenience API for validated scenes. Use `try_render` at untrusted boundaries.
    pub fn render(&mut self, scene: &Scene) -> Frame {
        self.render_full(scene);
        self.frame.clone()
    }
    pub fn try_render(&mut self, scene: &Scene) -> Result<Frame, cw_scene::SceneError> {
        scene.validate()?;
        Ok(self.render(scene))
    }
    /// Render without copying the frame; its bytes remain valid until the next mutable call.
    pub fn render_ref(&mut self, scene: &Scene) -> &Frame {
        self.render_full(scene);
        &self.frame
    }
    fn render_full(&mut self, scene: &Scene) {
        self.allocate(scene);
        let area = Rect::new(0, 0, scene.width, scene.height);
        self.paint(scene, &[area]);
        self.revision = Some(scene.revision);
    }
    /// The damage must describe every change from the prior frame. A size change or
    /// missing prior frame falls back to full repaint. Patches supply this contract.
    pub fn render_incremental(&mut self, scene: &Scene, damage: &Damage) -> &Frame {
        if self.revision.is_none()
            || self.frame.width != scene.width
            || self.frame.height != scene.height
        {
            self.render_full(scene);
        } else {
            self.paint(scene, &damage.rects);
            self.revision = Some(scene.revision);
        }
        &self.frame
    }
    pub fn frame(&self) -> &Frame {
        &self.frame
    }
    fn allocate(&mut self, scene: &Scene) {
        let len = (scene.width as u128) * (scene.height as u128) * 4;
        assert!(
            scene.width <= i32::MAX as u32
                && scene.height <= i32::MAX as u32
                && len <= cw_scene::MAX_PIXELS as u128 * 4,
            "frame exceeds dimension safety limit; call try_render for untrusted scenes"
        );
        self.frame.width = scene.width;
        self.frame.height = scene.height;
        self.frame.rgba.resize(len as usize, 0);
    }
    fn glyph(&mut self, c: char, size: u16, ui: bool) -> Arc<Glyph> {
        let size = size.clamp(1, 256);
        if let Some(g) = self.glyphs.get(&(ui, c, size)) {
            return g.clone();
        }
        if self.glyphs.len() >= 8192
            || self.glyphs.values().map(|g| g.alpha.len()).sum::<usize>() > 8 * 1024 * 1024
        {
            self.glyphs.clear()
        }
        let font = if ui {
            self.ui_font.get_or_insert_with(|| {
                Font::from_bytes(UI_FONT_BYTES, FontSettings::default())
                    .expect("bundled UI font is valid")
            })
        } else {
            &self.font
        };
        let (metrics, alpha) = font.rasterize(c, size as f32);
        let glyph = Arc::new(Glyph { metrics, alpha });
        self.glyphs.insert((ui, c, size), glyph.clone());
        glyph
    }
    fn text(&mut self, text: &str, size: u16, width: u32, height: u32, ui: bool) -> Arc<Mask> {
        let key = (ui, text.to_owned(), size, width, height);
        if let Some(mask) = self.texts.get(&key) {
            return mask.clone();
        }
        if self.texts.len() >= 128
            || self
                .texts
                .values()
                .map(|m| m.alpha.len() + m.spans.len() * 12)
                .sum::<usize>()
                > 16 * 1024 * 1024
        {
            self.texts.clear()
        }
        let len = width as u64 * height as u64;
        if len > 16 * 1024 * 1024 {
            return Arc::new(Mask {
                width: 0,
                height: 0,
                alpha: Vec::new(),
                spans: Vec::new(),
            });
        }
        let mut mask = Mask {
            width,
            height,
            alpha: vec![0; len as usize],
            spans: Vec::new(),
        };
        let size = size.clamp(1, 256);
        if ui {
            // Quantized 1/64-pixel advances avoid cumulative platform floating-point
            // layout drift. Raster positions and line spacing are integer pixels.
            let mut pen = 0i64;
            let mut baseline = size as i64;
            let line_height = size as i64 + (size as i64 + 3) / 4;
            for c in text.chars() {
                if c == '\n' {
                    pen = 0;
                    baseline += line_height;
                    continue;
                }
                if c == '\r' {
                    continue;
                }
                if baseline - size as i64 >= height as i64 {
                    break;
                }
                let g = self.glyph(if c == '\t' { ' ' } else { c }, size, true);
                let advance =
                    (g.metrics.advance_width * 64.0 + 0.5) as i64 * if c == '\t' { 4 } else { 1 };
                if pen > 0 && pen + advance > width as i64 * 64 {
                    pen = 0;
                    baseline += line_height;
                }
                let x = (pen + 32) / 64 + g.metrics.xmin as i64;
                let y = baseline - g.metrics.height as i64 - g.metrics.ymin as i64;
                for gy in 0..g.metrics.height {
                    let py = y + gy as i64;
                    if py < 0 || py >= height as i64 {
                        continue;
                    }
                    for gx in 0..g.metrics.width {
                        let px = x + gx as i64;
                        if px >= 0 && px < width as i64 {
                            let i = py as usize * width as usize + px as usize;
                            mask.alpha[i] = mask.alpha[i].max(g.alpha[gy * g.metrics.width + gx]);
                        }
                    }
                }
                pen += advance;
            }
        } else {
            let (cell, line_height) = text_cell(size);
            let columns = (width / cell).max(1) as usize;
            for (row, line) in wrap_text(text, columns).iter().enumerate() {
                let baseline = row as i64 * line_height as i64 + size as i64;
                if baseline - size as i64 >= height as i64 {
                    break;
                }
                for (col, c) in line.chars().enumerate() {
                    let g = self.glyph(c, size, false);
                    let x = col as i64 * cell as i64 + g.metrics.xmin as i64;
                    let y = baseline - g.metrics.height as i64 - g.metrics.ymin as i64;
                    for gy in 0..g.metrics.height {
                        let py = y + gy as i64;
                        if py < 0 || py >= height as i64 {
                            continue;
                        }
                        for gx in 0..g.metrics.width {
                            let px = x + gx as i64;
                            if px >= 0 && px < width as i64 {
                                let i = py as usize * width as usize + px as usize;
                                mask.alpha[i] =
                                    mask.alpha[i].max(g.alpha[gy * g.metrics.width + gx]);
                            }
                        }
                    }
                }
            }
        }
        for y in 0..height {
            let mut x = 0;
            while x < width {
                while x < width && mask.alpha[(y * width + x) as usize] == 0 {
                    x += 1
                }
                let start = x;
                while x < width && mask.alpha[(y * width + x) as usize] != 0 {
                    x += 1
                }
                if start < x {
                    mask.spans.push((y, start, x));
                }
            }
        }
        let mask = Arc::new(mask);
        if mask.alpha.len() + mask.spans.len() * 12 <= 16 * 1024 * 1024 {
            self.texts.insert(key, mask.clone());
        }
        mask
    }
    fn paint(&mut self, scene: &Scene, damage: &[Rect]) {
        let viewport = Rect::new(0, 0, scene.width, scene.height);
        let nodes = scene.ordered_nodes();
        let damage = normalize_damage(damage, viewport);
        for damage in &damage {
            let Some(area) = damage.intersection(viewport) else {
                continue;
            };
            self.clear(area, scene.background);
            for node in &nodes {
                let Some(area) = node.painted_bounds().intersection(area) else {
                    continue;
                };
                if node.opacity == 0 {
                    continue;
                }
                let text = match &node.primitive {
                    Primitive::Text { text, size, .. } => {
                        Some(self.text(text, *size, node.bounds.width, node.bounds.height, false))
                    }
                    Primitive::UiText { text, size, .. } => {
                        Some(self.text(text, *size, node.bounds.width, node.bounds.height, true))
                    }
                    _ => None,
                };
                self.paint_node(node, area, text.as_deref());
            }
        }
        self.stats.frames += 1;
        self.stats.glyph_cache_entries = self.glyphs.len();
        self.stats.text_cache_entries = self.texts.len();
    }
    fn clear(&mut self, area: Rect, c: Color) {
        let pixel = [c.0, c.1, c.2, c.3];
        for y in area.y as u32..area.y as u32 + area.height {
            let start = ((y * self.frame.width + area.x as u32) * 4) as usize;
            for dst in self.frame.rgba[start..start + area.width as usize * 4].chunks_exact_mut(4) {
                dst.copy_from_slice(&pixel)
            }
        }
    }
    fn paint_node(&mut self, node: &Node, area: Rect, text: Option<&Mask>) {
        let identity = node.transform == Transform::default();
        if identity {
            if let (Primitive::Text { color, .. } | Primitive::UiText { color, .. }, Some(mask)) =
                (&node.primitive, text)
            {
                for &(row, start, end) in &mask.spans {
                    let y = node.bounds.y as i64 + row as i64;
                    if y < area.y as i64 || y >= area.y as i64 + area.height as i64 {
                        continue;
                    }
                    let left = (node.bounds.x as i64 + start as i64).max(area.x as i64);
                    let right =
                        (node.bounds.x as i64 + end as i64).min(area.x as i64 + area.width as i64);
                    for x in left..right {
                        let alpha = mask.alpha
                            [(row * mask.width) as usize + (x - node.bounds.x as i64) as usize];
                        let color = Color(
                            color.0,
                            color.1,
                            color.2,
                            mul_alpha(mul_alpha(color.3, alpha), node.opacity),
                        );
                        if color.3 == 0 {
                            continue;
                        }
                        let i = ((y as u32 * self.frame.width + x as u32) * 4) as usize;
                        blend(&mut self.frame.rgba[i..i + 4], color);
                        self.stats.painted_pixels += 1;
                    }
                }
                return;
            }
            if let Primitive::Box {
                fill, border: None, ..
            } = &node.primitive
            {
                if fill.3 == 255 && node.opacity == 255 {
                    self.clear(area, *fill);
                    self.stats.painted_pixels += area.width as u64 * area.height as u64;
                    return;
                }
            }
        }
        for y in area.y..area.y + area.height as i32 {
            for x in area.x..area.x + area.width as i32 {
                let p = if identity {
                    Some((x, y))
                } else {
                    node.transform.inverse_point(x, y)
                };
                let Some((lx, ly)) = p else { continue };
                if !node.bounds.contains(lx, ly) {
                    continue;
                }
                let local_x = lx as i64 - node.bounds.x as i64;
                let local_y = ly as i64 - node.bounds.y as i64;
                let mut color = match &node.primitive {
                    Primitive::Region => continue,
                    Primitive::Box {
                        fill,
                        border,
                        border_width,
                    } => {
                        let bw = *border_width as i64;
                        if local_x < bw
                            || local_y < bw
                            || local_x >= node.bounds.width as i64 - bw
                            || local_y >= node.bounds.height as i64 - bw
                        {
                            border.unwrap_or(*fill)
                        } else {
                            *fill
                        }
                    }
                    Primitive::RoundedBox {
                        fill,
                        border,
                        border_width,
                        radius,
                    } => {
                        let outer = rounded_coverage(
                            local_x,
                            local_y,
                            node.bounds.width,
                            node.bounds.height,
                            *radius,
                        );
                        if outer == 0 {
                            continue;
                        }
                        let bw = (*border_width)
                            .min(node.bounds.width.div_ceil(2))
                            .min(node.bounds.height.div_ceil(2));
                        let inner = if border.is_some() && bw > 0 {
                            rounded_coverage(
                                local_x - bw as i64,
                                local_y - bw as i64,
                                node.bounds.width.saturating_sub(bw.saturating_mul(2)),
                                node.bounds.height.saturating_sub(bw.saturating_mul(2)),
                                radius.saturating_sub(bw),
                            )
                        } else {
                            outer
                        };
                        // Blend disjoint edge/interior coverage once, avoiding a dark
                        // seam where translucent antialiased fill meets its border.
                        let edge = outer.saturating_sub(inner);
                        let border = border.unwrap_or(*fill);
                        if inner == outer {
                            Color(fill.0, fill.1, fill.2, mul_alpha(fill.3, outer))
                        } else if inner == 0 {
                            Color(border.0, border.1, border.2, mul_alpha(border.3, outer))
                        } else {
                            let alpha =
                                fill.3 as u32 * inner as u32 + border.3 as u32 * edge as u32;
                            if alpha == 0 {
                                continue;
                            }
                            let channel = |f: u8, b: u8| {
                                ((f as u32 * fill.3 as u32 * inner as u32
                                    + b as u32 * border.3 as u32 * edge as u32
                                    + alpha / 2)
                                    / alpha) as u8
                            };
                            Color(
                                channel(fill.0, border.0),
                                channel(fill.1, border.1),
                                channel(fill.2, border.2),
                                ((alpha + 127) / 255) as u8,
                            )
                        }
                    }
                    Primitive::Text { color, .. } | Primitive::UiText { color, .. } => {
                        let Some(mask) = text else { continue };
                        if local_x >= mask.width as i64 || local_y >= mask.height as i64 {
                            continue;
                        }
                        let a =
                            mask.alpha[local_y as usize * mask.width as usize + local_x as usize];
                        Color(color.0, color.1, color.2, mul_alpha(color.3, a))
                    }
                    Primitive::Image {
                        width,
                        height,
                        rgba,
                    } => {
                        if *width == 0 || *height == 0 {
                            continue;
                        }
                        let sx = local_x as u64 * (*width as u64) / node.bounds.width as u64;
                        let sy = local_y as u64 * (*height as u64) / node.bounds.height as u64;
                        let i = ((sy * (*width as u64) + sx) * 4) as usize;
                        let Some(p) = rgba.get(i..i + 4) else {
                            continue;
                        };
                        Color(p[0], p[1], p[2], p[3])
                    }
                    Primitive::Path {
                        points,
                        fill,
                        stroke,
                        stroke_width,
                        closed,
                    } => {
                        let p = (local_x as i32, local_y as i32);
                        let mut color = Color::TRANSPARENT;
                        if *closed && inside_polygon(p, points) {
                            color = fill.unwrap_or(color)
                        }
                        if let Some(stroke) = stroke {
                            if on_path(p, points, *closed, *stroke_width) {
                                color = *stroke
                            }
                        }
                        color
                    }
                };
                color.3 = mul_alpha(color.3, node.opacity);
                if color.3 == 0 {
                    continue;
                }
                let i = ((y as u32 * self.frame.width + x as u32) * 4) as usize;
                blend(&mut self.frame.rgba[i..i + 4], color);
                self.stats.painted_pixels += 1;
            }
        }
    }
}
// Four-by-four fixed sample grid is deterministic across native/Wasm. Only the
// curved corner pixels pay supersampling cost; rectangular interiors are constant.
fn rounded_coverage(x: i64, y: i64, width: u32, height: u32, radius: u32) -> u8 {
    if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
        return 0;
    }
    let r = radius.min(width / 2).min(height / 2) as i64;
    if r == 0 || (x >= r && x < width as i64 - r) || (y >= r && y < height as i64 - r) {
        return 255;
    }
    let r = r as i128 * 8;
    let w = width as i128 * 8;
    let h = height as i128 * 8;
    let mut coverage = 0u32;
    for sy in [1, 3, 5, 7] {
        for sx in [1, 3, 5, 7] {
            let px = x as i128 * 8 + sx;
            let py = y as i128 * 8 + sy;
            let dx = (r - px).max(px - (w - r)).max(0);
            let dy = (r - py).max(py - (h - r)).max(0);
            if dx * dx + dy * dy <= r * r {
                coverage += 1;
            }
        }
    }
    ((coverage * 255 + 8) / 16) as u8
}
// Coalesce redundant old/new damage and contiguous rectangles before rasterization.
// Extra pixels inside a merged bounding rectangle are safe to repaint. Restrict
// merging to cases where that rectangle costs no more than the two inputs.
fn normalize_damage(input: &[Rect], viewport: Rect) -> Vec<Rect> {
    let mut rects: Vec<_> = input
        .iter()
        .filter_map(|r| r.intersection(viewport))
        .collect();
    rects.sort_by_key(|r| (r.y, r.x, r.height, r.width));
    rects.dedup();
    let mut i = 0;
    while i < rects.len() {
        let mut j = i + 1;
        while j < rects.len() {
            let a = rects[i];
            let b = rects[j];
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let right = (a.x as i64 + a.width as i64).max(b.x as i64 + b.width as i64);
            let bottom = (a.y as i64 + a.height as i64).max(b.y as i64 + b.height as i64);
            let merged = Rect::new(x, y, (right - x as i64) as u32, (bottom - y as i64) as u32);
            if merged.width as u64 * merged.height as u64
                <= a.width as u64 * a.height as u64 + b.width as u64 * b.height as u64
            {
                rects[i] = merged;
                rects.remove(j);
                j = i + 1;
            } else {
                j += 1
            }
        }
        i += 1;
    }
    rects
}
fn mul_alpha(a: u8, b: u8) -> u8 {
    ((a as u32 * b as u32 + 127) / 255) as u8
}
fn blend(dst: &mut [u8], src: Color) {
    if src.3 == 255 {
        dst.copy_from_slice(&[src.0, src.1, src.2, 255]);
        return;
    }
    let sa = src.3 as u32;
    let da = dst[3] as u32;
    let out = sa * 255 + da * (255 - sa);
    if out == 0 {
        dst.fill(0);
        return;
    }
    for (i, s) in [src.0, src.1, src.2].iter().enumerate() {
        dst[i] = (((*s as u32) * sa * 255 + dst[i] as u32 * da * (255 - sa) + out / 2) / out) as u8;
    }
    dst[3] = ((out + 127) / 255) as u8;
}
fn inside_polygon((x, y): (i32, i32), points: &[(i32, i32)]) -> bool {
    let mut inside = false;
    if points.len() < 3 {
        return false;
    }
    for i in 0..points.len() {
        let (ax, ay) = points[i];
        let (bx, by) = points[(i + 1) % points.len()];
        if (ay > y) != (by > y) {
            let lhs = (x as i128 - ax as i128) * (by as i128 - ay as i128);
            let rhs = (bx as i128 - ax as i128) * (y as i128 - ay as i128);
            if if by > ay { lhs < rhs } else { lhs > rhs } {
                inside = !inside
            }
        }
    }
    inside
}
fn on_path(p: (i32, i32), points: &[(i32, i32)], closed: bool, width: u16) -> bool {
    if width == 0 || points.len() < 2 {
        return false;
    }
    let n = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    (0..n).any(|i| {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let dx = b.0 as i128 - a.0 as i128;
        let dy = b.1 as i128 - a.1 as i128;
        let px = p.0 as i128 - a.0 as i128;
        let py = p.1 as i128 - a.1 as i128;
        let len = dx * dx + dy * dy;
        let radius = width as i128 * width as i128;
        let dot = px * dx + py * dy;
        if len == 0 || dot <= 0 {
            4 * (px * px + py * py) <= radius
        } else if dot >= len {
            let x = p.0 as i128 - b.0 as i128;
            let y = p.1 as i128 - b.1 as i128;
            4 * (x * x + y * y) <= radius
        } else {
            let cross = px * dy - py * dx;
            4 * cross * cross <= radius * len
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cw_scene::{PatchOp, ScenePatch};
    use sha2::{Digest, Sha256};
    fn sample() -> Scene {
        let mut s = Scene::new(160, 90);
        s.background = Color::rgb(10, 20, 30);
        s.nodes.push(Node::rectangle(
            1,
            Rect::new(3, 4, 100, 70),
            Color(100, 200, 30, 128),
        ));
        s.nodes.push(Node::text(
            2,
            Rect::new(8, 8, 130, 65),
            "Hello world\nDeterministic λ",
            16,
            Color::WHITE,
        ));
        s
    }
    #[test]
    fn deterministic() {
        let s = sample();
        let a = Renderer::new().render(&s);
        let b = Renderer::new().render(&s);
        assert_eq!(a, b);
        assert!(a.rgba.contains(&255));
        assert_eq!(
            format!("{:x}", Sha256::digest(&a.rgba)),
            "01455c4eaa6c1eca6900b545f69bba35ad9428fb66275a41df86268ae3595ec6"
        );
    }
    #[test]
    fn incremental_equals_full() {
        let mut s = sample();
        let mut r = Renderer::new();
        r.render(&s);
        let d = s
            .patch(ScenePatch {
                base_revision: 0,
                revision: 1,
                operations: vec![
                    PatchOp::Remove(1),
                    PatchOp::Upsert(Node::text(
                        2,
                        Rect::new(20, 10, 110, 60),
                        "Updated",
                        18,
                        Color::BLACK,
                    )),
                ],
            })
            .unwrap();
        let actual = r.render_incremental(&s, &d).clone();
        assert_eq!(actual, Renderer::new().render(&s));
    }
    #[test]
    fn transforms_clips_images_paths() {
        let mut s = Scene::new(40, 40);
        let mut n = Node::rectangle(1, Rect::new(0, 0, 20, 20), Color::BLACK);
        n.transform = Transform::translate(10, 10);
        n.clip = Some(Rect::new(10, 10, 5, 5));
        s.nodes.push(n);
        s.nodes.push(Node::new(
            2,
            Rect::new(25, 0, 10, 10),
            Primitive::Image {
                width: 1,
                height: 1,
                rgba: vec![255, 0, 0, 255],
            },
        ));
        s.nodes.push(Node::new(
            3,
            Rect::new(0, 25, 10, 10),
            Primitive::Path {
                points: vec![(0, 0), (9, 0), (0, 9)],
                fill: Some(Color::BLACK),
                stroke: None,
                stroke_width: 0,
                closed: true,
            },
        ));
        let f = Renderer::new().render(&s);
        assert_eq!(f.pixel(11, 11), Some([0, 0, 0, 255]));
        assert_eq!(f.pixel(16, 11), Some([255, 255, 255, 255]));
        assert_eq!(f.pixel(26, 1), Some([255, 0, 0, 255]));
        assert_eq!(f.pixel(1, 26), Some([0, 0, 0, 255]));
    }
    #[test]
    fn rounded_ui_golden_and_incremental() {
        let mut scene = Scene::new(220, 100);
        scene.background = Color::rgb(24, 31, 46);
        let mut panel =
            Node::rounded_rectangle(1, Rect::new(8, 8, 204, 84), Color(246, 248, 255, 230), 17);
        panel.primitive = Primitive::RoundedBox {
            fill: Color(246, 248, 255, 230),
            border: Some(Color::WHITE),
            border_width: 2,
            radius: 17,
        };
        scene.nodes.push(panel);
        scene.nodes.push(Node::ui_text(
            2,
            Rect::new(20, 17, 182, 62),
            "Window settings\nWiFi · λ · 09:41",
            17,
            Color::rgb(24, 31, 46),
        ));
        let mut renderer = Renderer::new();
        let frame = renderer.render(&scene);
        assert_eq!(frame.pixel(8, 8), Some([24, 31, 46, 255]));
        assert_eq!(frame.pixel(110, 8), Some([255, 255, 255, 255]));
        assert_eq!(frame, Renderer::new().render(&scene));
        let digest = format!("{:x}", Sha256::digest(&frame.rgba));
        assert_eq!(
            digest,
            "da928cab834c47449b5d838ccd2f3338e2e318dc63237f421dd7ac3d79324cf0"
        );
        let damage = scene
            .patch(ScenePatch {
                base_revision: 0,
                revision: 1,
                operations: vec![PatchOp::Upsert(Node::ui_text(
                    2,
                    Rect::new(20, 17, 182, 62),
                    "Connected\nWiFi · λ · 09:42",
                    17,
                    Color::BLACK,
                ))],
            })
            .unwrap();
        assert_eq!(
            renderer.render_incremental(&scene, &damage).clone(),
            Renderer::new().render(&scene)
        );
    }
    #[test]
    fn ui_proportional_advances_clipping_and_cache() {
        let mut renderer = Renderer::new();
        let narrow = renderer.text("iiii", 20, 180, 30, true);
        let wide = renderer.text("WWWW", 20, 180, 30, true);
        let right = |mask: &Mask| mask.spans.iter().map(|s| s.2).max().unwrap();
        assert!(right(&wide) > right(&narrow) * 2);
        let cached = renderer.text("iiii", 20, 180, 30, true);
        assert!(Arc::ptr_eq(&narrow, &cached));
        let mut scene = Scene::new(80, 30);
        let mut label = Node::ui_text(
            1,
            Rect::new(-5, -3, 180, 40),
            "Outside clip",
            24,
            Color::BLACK,
        );
        label.clip = Some(Rect::new(10, 5, 20, 15));
        scene.nodes.push(label);
        let frame = renderer.render(&scene);
        for y in 0..30 {
            for x in 0..80 {
                if !Rect::new(10, 5, 20, 15).contains(x, y) {
                    assert_eq!(frame.pixel(x as u32, y as u32), Some([255, 255, 255, 255]));
                }
            }
        }
    }
    #[test]
    fn rounded_coverage_symmetric_and_bounded() {
        for width in [1, 2, 13, 32] {
            for height in [1, 2, 15, 32] {
                for radius in [0, 1, 7, 500] {
                    for y in 0..height {
                        for x in 0..width {
                            let a = rounded_coverage(x as i64, y as i64, width, height, radius);
                            assert_eq!(
                                a,
                                rounded_coverage(
                                    (width - x - 1) as i64,
                                    y as i64,
                                    width,
                                    height,
                                    radius
                                )
                            );
                            assert_eq!(
                                a,
                                rounded_coverage(
                                    x as i64,
                                    (height - y - 1) as i64,
                                    width,
                                    height,
                                    radius
                                )
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(rounded_coverage(-1, 0, 20, 20, 4), 0);
        assert_eq!(rounded_coverage(20, 0, 20, 20, 4), 0);
        assert_eq!(rounded_coverage(0, 0, 20, 20, 0), 255);
    }
    #[test]
    fn font_hash() {
        assert_eq!(format!("{:x}", Sha256::digest(FONT_BYTES)), FONT_SHA256);
        assert_eq!(
            format!("{:x}", Sha256::digest(UI_FONT_BYTES)),
            UI_FONT_SHA256
        );
    }
    #[test]
    fn structured_scene_does_not_load_font() {
        let s = sample();
        assert_eq!(s.nodes.len(), 2);
        assert!(s.validate().is_ok());
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use cw_scene::{PatchOp, ScenePatch};
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(12))]
        #[test]
        fn moved_alpha_layers_incremental_equal_full(changes in prop::collection::vec((0u64..8,-10i32..60,-10i32..60,0u8..255),1..20)) {
            let mut scene=Scene::new(64,64);let mut renderer=Renderer::new();renderer.render(&scene);
            for (id,x,y,alpha) in changes {
                let n=Node::rectangle(id,Rect::new(x,y,20,20),Color(20,80,160,alpha));
                let damage=scene.patch(ScenePatch{base_revision:scene.revision,revision:scene.revision+1,operations:vec![PatchOp::Upsert(n)]}).unwrap();
                let incremental=renderer.render_incremental(&scene,&damage).clone();
                let full=renderer.render(&scene);
                prop_assert_eq!(incremental,full);
            }
        }
    }
}
