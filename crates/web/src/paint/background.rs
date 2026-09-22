//! Backgrounds: colour, gradients, images, and the canvas.
//!
//! The background colour fills the box `background-clip` names (the border box
//! unless the last layer says otherwise; the border box for inline fragments),
//! as a `RoundedBox` when the radii are uniform and as a rounded `Path` polygon
//! otherwise. Layers paint bottom-up (the last listed first), each positioned in its
//! `background-origin` box, sized by `background-size` (`cover`/`contain` scale by
//! integer nearest-neighbour resampling here, so the pixels are the same on every
//! platform), tiled by `background-repeat` as repeated `Image` nodes clipped to the
//! painting area (capped at 4096 tiles), and `background-attachment: fixed` layers
//! are positioned in the viewport.
//!
//! Gradients are rasterised into `Primitive::Image` at the box size with integer
//! arithmetic: a linear gradient projects each pixel centre onto the gradient line
//! (angle from `trig`), a radial one measures the distance to the centre (circle) or
//! the elliptical distance (ellipse), both `farthest-corner`. An axis-aligned linear
//! gradient is rasterised as a one-pixel strip and stretched by the renderer, which
//! samples images nearest-neighbour to the node bounds; other gradients are capped at
//! 512 px on the longer side for the same reason. Colour stops are resolved as the
//! spec says: missing positions interpolated between neighbours, out-of-order
//! positions clamped to the previous. The computed form has no separate hint entry,
//! so a hint arrives as an ordinary stop. Rasters are cached by (size, gradient)
//! within one paint.
//!
//! The canvas takes the root element's background; if the root has none, the body's
//! (`Scene::background` is the colour; image layers are painted over the viewport).
//! The element whose background propagated paints none of its own.

use std::rc::Rc;

use cw_scene::{Color, Primitive, Rect as SRect};

use super::{border, parts, px, snap, upx, Painter, RgbaImage, State};
use crate::dom::NodeId;
use crate::geom::{Au, Rect};
use crate::style::computed::{
    BackgroundBox, BackgroundImage, BackgroundLayer, BackgroundRepeat, BackgroundSize,
    ComputedStyle, GradientStop, LengthPercentageAuto,
};

const MAX_TILES: usize = 4096;
const MAX_GRADIENT_SIDE: u32 = 512;

fn has_background(s: &ComputedStyle) -> bool {
    s.background_color.3 != 0
        || s.background
            .iter()
            .any(|l| !matches!(l.image, BackgroundImage::None))
}

/// Canvas background propagation. Sets `Scene::background` and paints the root
/// layers over the viewport.
pub(crate) fn paint_canvas(p: &mut Painter, state: &State) {
    let Some(doc) = p.doc else { return };
    let html = doc.document_element();
    let body = doc.body();
    let source = match (html, body) {
        (Some(h), _) if p.styles.get(h).is_some_and(has_background) => Some(h),
        (_, Some(b)) if p.styles.get(b).is_some_and(has_background) => Some(b),
        _ => None,
    };
    let Some(src) = source else { return };
    let Some(style) = p.styles.get(src).cloned() else {
        return;
    };
    p.canvas_source = Some(src);
    if style.background_color.3 != 0 {
        p.background = style.background_color;
    }
    if style
        .background
        .iter()
        .all(|l| matches!(l.image, BackgroundImage::None))
    {
        return;
    }
    // Layers are positioned relative to the root element's box; the viewport is the
    // painting area.
    let root_rect = p
        .tree
        .rects_of(html.unwrap_or(src))
        .first()
        .copied()
        .unwrap_or(Rect::new(
            Au::ZERO,
            Au::ZERO,
            p.tree.viewport_width,
            p.tree.viewport_height,
        ))
        .translate(state.origin.x, state.origin.y);
    let vp = SRect::new(0, 0, p.viewport.width, p.viewport.height);
    let key = (src, u32::MAX >> 20);
    let clipped = state.clipped(vp);
    for layer in style.background.iter().rev() {
        paint_layer(p, key, &clipped, layer, root_rect, root_rect, root_rect, vp);
    }
}

/// The background of one box fragment. `border_box`, `padding_box` and
/// `content_box` are absolute (`Au`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_background(
    p: &mut Painter,
    key: (NodeId, u32),
    state: &State,
    style: &ComputedStyle,
    border_box: Rect,
    padding_box: Rect,
    content_box: Rect,
    first: bool,
    last: bool,
) {
    let srect = snap(border_box);
    let pick = |b: BackgroundBox| match b {
        BackgroundBox::BorderBox => border_box,
        BackgroundBox::PaddingBox => padding_box,
        BackgroundBox::ContentBox => content_box,
    };
    let color_clip = style
        .background
        .last()
        .map(|l| l.clip)
        .unwrap_or(BackgroundBox::BorderBox);
    if style.background_color.3 != 0 {
        let area = snap(pick(color_clip));
        if area.width > 0 && area.height > 0 {
            let radii = border::radii_px(style, srect);
            let id = p.id(key, parts::BACKGROUND);
            let fill = style.background_color;
            let radii = if !(first && last) {
                // An inline fragment cut at a line end has square cut ends.
                let mut r = radii;
                if !first {
                    r.top_left = 0;
                    r.bottom_left = 0;
                }
                if !last {
                    r.top_right = 0;
                    r.bottom_right = 0;
                }
                r
            } else {
                radii
            };
            match border::uniform_radius(&radii) {
                Some(0) => {
                    p.emit(
                        state,
                        id,
                        area,
                        Primitive::Box {
                            fill,
                            border: None,
                            border_width: 0,
                        },
                    );
                }
                Some(r) if area == srect => {
                    p.emit(
                        state,
                        id,
                        area,
                        Primitive::RoundedBox {
                            fill,
                            border: None,
                            border_width: 0,
                            radius: r,
                        },
                    );
                }
                _ => {
                    let pts = border::rounded_polygon(area, radii);
                    p.emit_path(state, id, area, pts, Some(fill), None, 0, true);
                }
            }
        }
    }
    let vp = SRect::new(0, 0, p.viewport.width, p.viewport.height);
    for layer in style.background.iter().rev() {
        if matches!(layer.image, BackgroundImage::None) {
            continue;
        }
        let painting = snap(pick(layer.clip));
        let radii = border::radii_px(style, srect);
        let mut clipped = state.clipped(painting);
        if let Some(r) = border::uniform_radius(&radii) {
            if r > 0 && clipped.rounded_clip.is_none() {
                clipped.rounded_clip = Some(cw_scene::RoundedClip {
                    rect: srect,
                    radius: r,
                });
            }
        }
        paint_layer(
            p,
            key,
            &clipped,
            layer,
            pick(layer.origin),
            border_box,
            padding_box,
            vp,
        );
    }
}

/// Paints one image layer positioned in `origin_box` and clipped by `state`.
#[allow(clippy::too_many_arguments)]
fn paint_layer(
    p: &mut Painter,
    key: (NodeId, u32),
    state: &State,
    layer: &BackgroundLayer,
    origin_box: Rect,
    _border_box: Rect,
    _padding_box: Rect,
    viewport: SRect,
) {
    let area = if layer.attachment_fixed {
        viewport
    } else {
        snap(origin_box)
    };
    if area.width == 0 || area.height == 0 {
        return;
    }
    // Intrinsic size.
    let intrinsic: Option<(u32, u32)> = match &layer.image {
        BackgroundImage::Url(url) => p.ctx.images.image(url).map(|i| (i.width, i.height)),
        BackgroundImage::LinearGradient { .. } | BackgroundImage::RadialGradient { .. } => None,
        BackgroundImage::None => return,
    };
    if matches!(layer.image, BackgroundImage::Url(_)) && intrinsic.is_none() {
        return; // not fetched yet: browsers paint nothing
    }
    let (tw, th) = tile_size(layer.size, intrinsic, (area.width, area.height));
    if tw == 0 || th == 0 {
        return;
    }
    // Position within the positioning area: percentages align the same fraction of
    // image and area.
    let pos_x = resolve_position(layer.position.0, area.width as i64, tw as i64);
    let pos_y = resolve_position(layer.position.1, area.height as i64, th as i64);
    let pixels: Rc<RgbaImage> = match &layer.image {
        BackgroundImage::Url(url) => {
            let img = p.ctx.images.image(url).unwrap();
            if img.width == tw && img.height == th {
                Rc::new(img.clone())
            } else {
                Rc::new(resample(img, tw, th))
            }
        }
        g => rasterize_gradient(p, tw, th, g),
    };
    let (repeat_x, repeat_y) = match layer.repeat {
        BackgroundRepeat::Repeat | BackgroundRepeat::Space | BackgroundRepeat::Round => {
            (true, true)
        }
        BackgroundRepeat::RepeatX => (true, false),
        BackgroundRepeat::RepeatY => (false, true),
        BackgroundRepeat::NoRepeat => (false, false),
    };
    let Some(clip) = state.clip else { return };
    let xs = tile_positions(
        area.x as i64 + pos_x,
        tw,
        repeat_x,
        clip.x as i64,
        clip.right() as i64,
    );
    let ys = tile_positions(
        area.y as i64 + pos_y,
        th,
        repeat_y,
        clip.y as i64,
        clip.bottom() as i64,
    );
    let mut n = 0;
    for &y in &ys {
        for &x in &xs {
            if n >= MAX_TILES {
                return;
            }
            n += 1;
            let part = p.next_part(key);
            let id = p.id(key, part);
            let bounds = SRect::new(x as i32, y as i32, tw, th);
            if bounds.intersection(clip).is_none() {
                continue;
            }
            p.emit(
                state,
                id,
                bounds,
                Primitive::Image {
                    width: pixels.width,
                    height: pixels.height,
                    rgba: pixels.rgba.clone(),
                },
            );
        }
    }
}

/// Tile origins along one axis that intersect `[lo, hi)`.
fn tile_positions(start: i64, size: u32, repeat: bool, lo: i64, hi: i64) -> Vec<i64> {
    if !repeat {
        return vec![start];
    }
    let s = size as i64;
    // First tile at or before `lo`.
    let first = start - ((start - lo).div_euclid(s)) * s;
    let mut out = Vec::new();
    let mut x = first;
    while x < hi && out.len() < MAX_TILES {
        out.push(x);
        x += s;
    }
    out
}

fn resolve_position(v: crate::style::computed::LengthPercentage, area: i64, size: i64) -> i64 {
    match v {
        crate::style::computed::LengthPercentage::Length(l) => px(l) as i64,
        crate::style::computed::LengthPercentage::Percent(pc) => {
            ((area - size) * pc as i64 + 5000) / 10_000
        }
        crate::style::computed::LengthPercentage::Calc(l, pc) => {
            px(l) as i64 + ((area - size) * pc as i64 + 5000) / 10_000
        }
        v @ crate::style::computed::LengthPercentage::Clamp { .. } => {
            px(v.resolve(crate::geom::Au::from_px_i32((area - size) as i32))) as i64
        }
    }
}

/// The size of one tile per `background-size`.
fn tile_size(size: BackgroundSize, intrinsic: Option<(u32, u32)>, area: (u32, u32)) -> (u32, u32) {
    let (aw, ah) = area;
    let (iw, ih) = intrinsic.unwrap_or(area);
    match size {
        BackgroundSize::Auto => (iw, ih),
        BackgroundSize::Cover | BackgroundSize::Contain => {
            if iw == 0 || ih == 0 {
                return (0, 0);
            }
            // Scale so the image covers (or fits) the area, preserving aspect.
            let by_w = (
                aw as u64,
                (aw as u64 * ih as u64 + iw as u64 / 2) / iw as u64,
            );
            let by_h = (
                (ah as u64 * iw as u64 + ih as u64 / 2) / ih as u64,
                ah as u64,
            );
            let cover = matches!(size, BackgroundSize::Cover);
            let pick = if (by_w.1 >= ah as u64) == cover {
                by_w
            } else {
                by_h
            };
            (
                pick.0.min(u32::MAX as u64) as u32,
                pick.1.min(u32::MAX as u64) as u32,
            )
        }
        BackgroundSize::Explicit(w, h) => {
            let aw_au = Au::from_px_i32(aw as i32);
            let ah_au = Au::from_px_i32(ah as i32);
            let w_px = match w {
                LengthPercentageAuto::Auto => None,
                LengthPercentageAuto::Set(v) => Some(upx(v.resolve(aw_au))),
            };
            let h_px = match h {
                LengthPercentageAuto::Auto => None,
                LengthPercentageAuto::Set(v) => Some(upx(v.resolve(ah_au))),
            };
            match (w_px, h_px) {
                (Some(w), Some(h)) => (w, h),
                (Some(w), None) => (
                    w,
                    if iw == 0 {
                        ih
                    } else {
                        (w as u64 * ih as u64 / iw as u64) as u32
                    },
                ),
                (None, Some(h)) => (
                    if ih == 0 {
                        iw
                    } else {
                        (h as u64 * iw as u64 / ih as u64) as u32
                    },
                    h,
                ),
                (None, None) => (iw, ih),
            }
        }
    }
}

/// Nearest-neighbour resampling to `w × h`.
pub(crate) fn resample(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    if w == 0 || h == 0 || img.width == 0 || img.height == 0 {
        return RgbaImage {
            width: w,
            height: h,
            rgba: vec![0; (w as usize) * (h as usize) * 4],
        };
    }
    let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for y in 0..h {
        let sy = (y as u64 * img.height as u64 / h as u64) as usize;
        for x in 0..w {
            let sx = (x as u64 * img.width as u64 / w as u64) as usize;
            let i = (sy * img.width as usize + sx) * 4;
            out.extend_from_slice(&img.rgba[i..i + 4]);
        }
    }
    RgbaImage {
        width: w,
        height: h,
        rgba: out,
    }
}

/// Colour stops resolved to `(offset in 1/65536 of the gradient line, colour)`.
fn resolve_stops(stops: &[GradientStop], line_len_au: Au) -> Vec<(i64, Color)> {
    if stops.is_empty() {
        return vec![(0, Color::TRANSPARENT), (65_536, Color::TRANSPARENT)];
    }
    let to_frac = |pos: crate::style::computed::LengthPercentage| -> i64 {
        match pos {
            crate::style::computed::LengthPercentage::Percent(pc) => pc as i64 * 65_536 / 10_000,
            crate::style::computed::LengthPercentage::Length(l) => {
                if line_len_au.0 == 0 {
                    0
                } else {
                    l.0 as i64 * 65_536 / line_len_au.0 as i64
                }
            }
            crate::style::computed::LengthPercentage::Calc(l, pc) => {
                let a = if line_len_au.0 == 0 {
                    0
                } else {
                    l.0 as i64 * 65_536 / line_len_au.0 as i64
                };
                a + pc as i64 * 65_536 / 10_000
            }
            v @ crate::style::computed::LengthPercentage::Clamp { .. } => {
                if line_len_au.0 == 0 {
                    0
                } else {
                    v.resolve(line_len_au).0 as i64 * 65_536 / line_len_au.0 as i64
                }
            }
        }
    };
    let n = stops.len();
    let mut pos: Vec<Option<i64>> = stops.iter().map(|s| s.position.map(to_frac)).collect();
    if pos[0].is_none() {
        pos[0] = Some(0);
    }
    if pos[n - 1].is_none() {
        pos[n - 1] = Some(65_536);
    }
    // Clamp to the running maximum.
    let mut max = i64::MIN;
    for p in pos.iter_mut().flatten() {
        if *p < max {
            *p = max;
        }
        max = *p;
    }
    // Interpolate runs of missing positions.
    let mut i = 0;
    while i < n {
        if pos[i].is_none() {
            let start = i - 1;
            let mut end = i;
            while pos[end].is_none() {
                end += 1;
            }
            let (a, b) = (pos[start].unwrap(), pos[end].unwrap());
            let gaps = (end - start) as i64;
            for (k, slot) in pos.iter_mut().enumerate().take(end).skip(start + 1) {
                *slot = Some(a + (b - a) * (k - start) as i64 / gaps);
            }
            i = end;
        }
        i += 1;
    }
    stops
        .iter()
        .zip(pos)
        .map(|(s, p)| (p.unwrap(), s.color))
        .collect()
}

fn color_at(stops: &[(i64, Color)], t: i64) -> Color {
    if t <= stops[0].0 {
        return stops[0].1;
    }
    let last = stops[stops.len() - 1];
    if t >= last.0 {
        return last.1;
    }
    for w in stops.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if t >= a && t <= b {
            if b == a {
                return cb;
            }
            let f = (t - a) * 256 / (b - a);
            let mix = |x: u8, y: u8| ((x as i64 * (256 - f) + y as i64 * f) / 256) as u8;
            return Color(
                mix(ca.0, cb.0),
                mix(ca.1, cb.1),
                mix(ca.2, cb.2),
                mix(ca.3, cb.3),
            );
        }
    }
    last.1
}

/// Rasterises a gradient at `w × h` (the tile size), cached by size and gradient
/// within this paint.
pub(crate) fn rasterize_gradient(
    p: &mut Painter,
    w: u32,
    h: u32,
    g: &BackgroundImage,
) -> Rc<RgbaImage> {
    let key = (w, h, g.clone());
    if let Some(cached) = p.gradients.get(&key) {
        let bytes = cached.clone();
        return Rc::new(image_from_cached(w, h, &bytes, g));
    }
    let raster = rasterize_gradient_uncached(w, h, g);
    p.gradients.insert(key, Rc::new(raster.rgba.clone()));
    let mut r = raster;
    r.width = r.width.max(1);
    Rc::new(r)
}

fn image_from_cached(w: u32, h: u32, bytes: &[u8], g: &BackgroundImage) -> RgbaImage {
    let (rw, rh) = raster_size(w, h, g);
    RgbaImage {
        width: rw,
        height: rh,
        rgba: bytes.to_vec(),
    }
}

/// The raster's own size: a strip for axis-aligned linear gradients, otherwise the
/// tile size capped on its longer side.
fn raster_size(w: u32, h: u32, g: &BackgroundImage) -> (u32, u32) {
    match g {
        BackgroundImage::LinearGradient {
            angle_centi_deg, ..
        } => {
            let a = angle_centi_deg.rem_euclid(36_000);
            if a == 0 || a == 18_000 {
                (1, h.clamp(1, MAX_GRADIENT_SIDE * 8))
            } else if a == 9000 || a == 27_000 {
                (w.clamp(1, MAX_GRADIENT_SIDE * 8), 1)
            } else {
                capped(w, h)
            }
        }
        _ => capped(w, h),
    }
}

fn capped(w: u32, h: u32) -> (u32, u32) {
    let m = w.max(h);
    if m <= MAX_GRADIENT_SIDE {
        (w.max(1), h.max(1))
    } else {
        (
            (w as u64 * MAX_GRADIENT_SIDE as u64 / m as u64).max(1) as u32,
            (h as u64 * MAX_GRADIENT_SIDE as u64 / m as u64).max(1) as u32,
        )
    }
}

pub(crate) fn rasterize_gradient_uncached(w: u32, h: u32, g: &BackgroundImage) -> RgbaImage {
    let (rw, rh) = raster_size(w, h, g);
    let mut rgba = Vec::with_capacity((rw as usize) * (rh as usize) * 4);
    match g {
        BackgroundImage::LinearGradient {
            angle_centi_deg,
            stops,
        } => {
            let s = super::trig::sin_1024(*angle_centi_deg) as i64;
            let c = super::trig::cos_1024(*angle_centi_deg) as i64;
            // Gradient line length in 1/1024 px over the box size.
            let (bw, bh) = (w as i64, h as i64);
            let len_1024 = (bw * s).abs() + (bh * c).abs();
            let stops = resolve_stops(stops, Au((len_1024 * 64 / 1024) as i32));
            for py in 0..rh {
                for pxl in 0..rw {
                    // Pixel centre in doubled box coordinates relative to the centre.
                    let x2 = if rw == 1 {
                        0
                    } else {
                        (2 * pxl as i64 + 1) * bw / rw as i64 - bw
                    };
                    let y2 = if rh == 1 {
                        0
                    } else {
                        (2 * py as i64 + 1) * bh / rh as i64 - bh
                    };
                    let proj = x2 * s - y2 * c; // in 2 * 1/1024 px units
                    let t = if len_1024 == 0 {
                        32_768
                    } else {
                        ((proj + len_1024) * 65_536) / (2 * len_1024)
                    };
                    let col = color_at(&stops, t);
                    rgba.extend_from_slice(&[col.0, col.1, col.2, col.3]);
                }
            }
        }
        BackgroundImage::RadialGradient { circle, stops } => {
            let (bw, bh) = (w as i128, h as i128);
            // Farthest-corner radii, doubled: circle r2 = sqrt(w²+h²); ellipse
            // rx2 = w√2, ry2 = h√2.
            let r2 = super::trig::isqrt(bw * bw + bh * bh).max(1);
            let ray_au = if *circle {
                Au((r2 * 32) as i32)
            } else {
                Au((super::trig::isqrt(2 * bw * bw) * 32) as i32)
            };
            let stops = resolve_stops(stops, ray_au);
            for py in 0..rh {
                for pxl in 0..rw {
                    let x2 = (2 * pxl as i128 + 1) * bw / rw as i128 - bw;
                    let y2 = (2 * py as i128 + 1) * bh / rh as i128 - bh;
                    let t = if *circle {
                        super::trig::isqrt(x2 * x2 + y2 * y2) * 65_536 / r2
                    } else if bw == 0 || bh == 0 {
                        65_536
                    } else {
                        // t² = x²/(2w²) + y²/(2h²)
                        let num = x2 * x2 * bh * bh + y2 * y2 * bw * bw;
                        let den = 2 * bw * bw * bh * bh;
                        super::trig::isqrt(num * 65_536 * 65_536 / den)
                    };
                    let col = color_at(&stops, t as i64);
                    rgba.extend_from_slice(&[col.0, col.1, col.2, col.3]);
                }
            }
        }
        _ => {
            rgba.resize((rw as usize) * (rh as usize) * 4, 0);
        }
    }
    RgbaImage {
        width: rw,
        height: rh,
        rgba,
    }
}
