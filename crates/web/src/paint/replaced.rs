//! Replaced content: images, form controls, list markers and placeholders.
//!
//! The box's own background and borders come from the UA sheet through the normal
//! box path; this module draws only what goes inside the content box.
//!
//! Images are `Primitive::Image` from the cache, sized per `object-fit` (`fill`
//! stretches to the content box, `contain`/`scale-down` fit inside it centred,
//! `cover` fills it centred and clipped, `none` is the intrinsic size centred and
//! clipped); the renderer samples nearest-neighbour to the node bounds. A missing
//! image is a 1 px inset grey border box with the `alt` text, as browsers draw it.
//!
//! Controls are drawn from DOM state in the theme-neutral look the UA sheet implies:
//! text inputs show their value (or placeholder in grey) and a caret when focused;
//! password fields show bullets; a checkbox is a 13 px rounded square with a check
//! polyline; a radio a 13 px circle with a dot; a button its label centred; a select
//! its chosen option and a chevron; a textarea its value wrapped to the content
//! width; a range a 4 px track with a 12 px thumb at the value. Disabled controls
//! grey their text. `<iframe>`, `<canvas>`, `<video>`, `<svg>` and `<object>` are a
//! light grey box with the tag name in muted text.

use cw_scene::{Color, Primitive, Rect as SRect};

use super::{display_list::box_rect, parts, px, semantics, snap, text, upx, Painter, State};
use crate::dom::NodeId;
use crate::geom::Rect;
use crate::layout::fragment::{ControlKind, Fragment, FragmentKind, Replaced};
use crate::style::computed::{ComputedStyle, ObjectFit};

const PLACEHOLDER_TEXT: Color = Color(117, 117, 117, 255);
const DISABLED_TEXT: Color = Color(109, 109, 109, 255);
const CONTROL_BORDER: Color = Color(118, 118, 118, 255);
const CONTROL_FILL: Color = Color(255, 255, 255, 255);
const CHECK: Color = Color(16, 16, 16, 255);
const ACCENT: Color = Color(0, 96, 223, 255);
const PLACEHOLDER_BOX: Color = Color(238, 238, 238, 255);
const PLACEHOLDER_EDGE: Color = Color(200, 200, 200, 255);
const MISSING_EDGE: Color = Color(192, 192, 192, 255);

pub(crate) fn paint(p: &mut Painter, f: &Fragment, state: &State) {
    let FragmentKind::Box { source, padding, border, replaced: Some(replaced), baseline, .. } = &f.kind else { return };
    let Some(key) = p.key(f) else { return };
    let rect = box_rect(p, f, state);
    let content = padding.inset(border.inset(rect));
    let style = p.style(*source).clone();
    let node = source.node();
    let disabled = p.doc.is_some_and(|d| semantics::is_disabled(d, node));
    match replaced {
        Replaced::Image { src, alt } => paint_image(p, key, state, &style, content, src, alt),
        Replaced::Control(kind) => paint_control(p, key, state, &style, node, *kind, content, snap(rect).y, disabled, *baseline),
        Replaced::Placeholder(tag) => {
            let r = snap(content);
            if r.width == 0 || r.height == 0 {
                return;
            }
            let id = p.id(key, parts::CONTENT);
            p.emit(state, id, r, Primitive::Box { fill: PLACEHOLDER_BOX, border: Some(PLACEHOLDER_EDGE), border_width: 1 });
            let label = tag.clone();
            let mut font = style.font.clone();
            font.size = crate::geom::Au::from_px_i32(12);
            font.weight = 400;
            let w = text::width_px(&font, &label);
            if w + 8 <= r.width && r.height >= 14 {
                let id = p.id(key, parts::CONTENT_TEXT);
                let x = r.x + (r.width - w) as i32 / 2;
                let baseline = r.y + (r.height as i32 + 12) / 2 - 1;
                text::draw_text(p, &state.clipped(r), id, x, baseline, &label, &font, PLACEHOLDER_TEXT);
            }
        }
        Replaced::Marker(marker) => {
            let r = snap(content);
            let baseline_px = px(rect.origin.y + baseline.unwrap_or(crate::geom::Au::ZERO));
            let baseline_px = if baseline.is_some() { baseline_px } else { r.y + style.font.size_px() as i32 };
            let w = text::width_px(&style.font, marker);
            // Markers sit at the end of their box (outside markers hang left of the
            // content), so right-align within the marker box.
            let x = r.right() - w as i32;
            let id = p.id(key, parts::CONTENT_TEXT);
            let i = text::draw_text(p, state, id, x.max(r.x), baseline_px, marker, &style.font, style.color);
            p.nodes[i].semantic = Some(semantics::text_semantic(marker));
        }
    }
}

fn paint_image(p: &mut Painter, key: (NodeId, u32), state: &State, style: &ComputedStyle, content: Rect, src: &str, alt: &str) {
    let r = snap(content);
    if r.width == 0 || r.height == 0 {
        return;
    }
    let Some(img) = p.ctx.images.image(src) else {
        // Missing: an inset border and the alt text.
        let id = p.id(key, parts::CONTENT);
        p.emit(state, id, r, Primitive::Box { fill: Color::TRANSPARENT, border: Some(MISSING_EDGE), border_width: 1 });
        if !alt.is_empty() && r.width > 6 && r.height as i32 > style.font.size_px() as i32 + 2 {
            let clipped = state.clipped(SRect::new(r.x + 1, r.y + 1, r.width - 2, r.height - 2));
            let shown = cw_scene::metrics::ellipsize(style.font.typeface, style.font.scene_style(), alt, style.font.size_px(), r.width - 4);
            let id = p.id(key, parts::CONTENT_TEXT);
            let baseline = r.y + 2 + style.font.size_px() as i32;
            let i = text::draw_text(p, &clipped, id, r.x + 2, baseline, &shown, &style.font, style.color);
            p.nodes[i].semantic = Some(semantics::text_semantic(alt));
        }
        return;
    };
    if img.width == 0 || img.height == 0 {
        return;
    }
    let (iw, ih) = (img.width as u64, img.height as u64);
    let (cw, ch) = (r.width as u64, r.height as u64);
    let fit = |cover: bool| -> (u64, u64) {
        let by_w = (cw, (cw * ih + iw / 2) / iw);
        let by_h = ((ch * iw + ih / 2) / ih, ch);
        if (by_w.1 >= ch) == cover {
            by_w
        } else {
            by_h
        }
    };
    let (dw, dh, clip) = match style.object_fit {
        ObjectFit::Fill => (cw, ch, false),
        ObjectFit::Contain => {
            let (w, h) = fit(false);
            (w, h, false)
        }
        ObjectFit::Cover => {
            let (w, h) = fit(true);
            (w, h, true)
        }
        ObjectFit::None => (iw, ih, true),
        ObjectFit::ScaleDown => {
            if iw <= cw && ih <= ch {
                (iw, ih, false)
            } else {
                let (w, h) = fit(false);
                (w, h, false)
            }
        }
    };
    let dx = r.x + ((cw as i64 - dw as i64) / 2) as i32;
    let dy = r.y + ((ch as i64 - dh as i64) / 2) as i32;
    let bounds = SRect::new(dx, dy, dw.min(u32::MAX as u64) as u32, dh.min(u32::MAX as u64) as u32);
    let s = if clip { state.clipped(r) } else { state.clone() };
    let id = p.id(key, parts::CONTENT);
    let rgba = img.rgba.clone();
    let (width, height) = (img.width, img.height);
    let i = p.emit(&s, id, bounds, Primitive::Image { width, height, rgba });
    if !alt.is_empty() {
        p.nodes[i].semantic = Some(cw_scene::Semantic { role: "img".into(), label: alt.to_owned(), value: None, disabled: false, focusable: false });
    }
}

fn control_text_color(style: &ComputedStyle, disabled: bool) -> Color {
    if disabled {
        DISABLED_TEXT
    } else {
        style.color
    }
}

/// Places `text` in a box, vertically centred: returns the baseline.
fn centred_baseline(r: SRect, size: u16) -> i32 {
    r.y + ((r.height as i32 - size as i32) / 2).max(0) + size as i32 - 1
}

#[allow(clippy::too_many_arguments)]
fn paint_control(p: &mut Painter, key: (NodeId, u32), state: &State, style: &ComputedStyle, node: NodeId, kind: ControlKind, content: Rect, rect_y: i32, disabled: bool, baseline: Option<crate::geom::Au>) {
    let r = snap(content);
    if r.width == 0 || r.height == 0 || kind == ControlKind::Hidden {
        return;
    }
    let doc = p.doc;
    let value = semantics::value_of(p, node).unwrap_or_default();
    let placeholder = doc.and_then(|d| d.attr(node, "placeholder")).map(semantics::collapse).unwrap_or_default();
    let focused = p.ctx.focused == Some(node);
    let font = style.font.clone();
    let size = font.size_px();
    let ink = control_text_color(style, disabled);
    let clipped = state.clipped(r);
    match kind {
        ControlKind::TextInput | ControlKind::Password | ControlKind::File | ControlKind::Color => {
            let shown = if kind == ControlKind::Password { "\u{2022}".repeat(value.chars().count()) } else { value.clone() };
            let (text_shown, color) = if shown.is_empty() { (placeholder.clone(), PLACEHOLDER_TEXT) } else { (shown.clone(), ink) };
            // Layout's baseline is from the top of the border box; fall back to
            // centring the line in the content box.
            let baseline = baseline.map(|b| rect_y + px(b)).filter(|b| *b >= r.y && *b <= r.bottom()).unwrap_or_else(|| centred_baseline(r, size));
            if !text_shown.is_empty() {
                let one_line: String = text_shown.split('\n').next().unwrap_or("").to_owned();
                let id = p.id(key, parts::CONTENT_TEXT);
                text::draw_text(p, &clipped, id, r.x, baseline, &one_line, &font, color);
            }
            if focused && !disabled {
                let at = p.ctx.caret.unwrap_or(usize::MAX).min(shown.chars().count());
                let prefix: String = shown.chars().take(at).collect();
                let cx = r.x + text::width_px(&font, &prefix) as i32;
                paint_caret(p, key, &clipped, cx, baseline, size, ink, 0, at as u32);
            }
        }
        ControlKind::TextArea => {
            let (text_shown, color) = if value.is_empty() { (placeholder.clone(), PLACEHOLDER_TEXT) } else { (value.clone(), ink) };
            let lh = text::line_height_px(size) as i32;
            let lines = cw_scene::metrics::wrap(font.typeface, font.scene_style(), &text_shown, size, r.width.max(1));
            let mut y = r.y + size as i32;
            let mut last = (r.x, y, 0u32, 0u32);
            for (i, line) in lines.iter().enumerate() {
                let line_no = i as u32;
                if y - size as i32 >= r.bottom() {
                    break;
                }
                let part = if i == 0 { parts::CONTENT_TEXT } else { p.next_part(key) };
                let id = p.id(key, part);
                let shown = line.trim_end();
                if !shown.is_empty() {
                    text::draw_text(p, &clipped, id, r.x, y, shown, &font, color);
                }
                last = (r.x + text::width_px(&font, shown) as i32, y, line_no, shown.chars().count() as u32);
                y += lh;
            }
            if focused && !disabled {
                let (cx, cy, line, col) = if value.is_empty() { (r.x, r.y + size as i32, 0, 0) } else { last };
                paint_caret(p, key, &clipped, cx, cy, size, ink, line, col);
            }
        }
        ControlKind::Checkbox | ControlKind::Radio => {
            let checked = doc.is_some_and(|d| d.has_attr(node, "checked") || d.attr(node, "aria-checked") == Some("true"));
            let d = 13u32.min(r.width).min(r.height);
            let bx = r.x + (r.width - d) as i32 / 2;
            let by = r.y + (r.height - d) as i32 / 2;
            let b = SRect::new(bx, by, d, d);
            let fill = if disabled { PLACEHOLDER_BOX } else { CONTROL_FILL };
            let id = p.id(key, parts::CONTENT);
            let radius = if kind == ControlKind::Radio { d / 2 } else { 2 };
            p.emit(state, id, b, Primitive::RoundedBox { fill, border: Some(CONTROL_BORDER), border_width: 1, radius });
            if checked {
                let id = p.id(key, parts::CONTENT_GLYPH);
                let mark = if disabled { DISABLED_TEXT } else { CHECK };
                if kind == ControlKind::Radio {
                    let inset = (d / 4).max(1);
                    let dot = SRect::new(bx + inset as i32, by + inset as i32, d - 2 * inset, d - 2 * inset);
                    p.emit(state, id, dot, Primitive::RoundedBox { fill: mark, border: None, border_width: 0, radius: (d - 2 * inset) / 2 });
                } else {
                    let (x, y, s) = (bx, by, d as i32);
                    let pts = vec![(x + s * 3 / 13, y + s * 7 / 13), (x + s * 6 / 13, y + s * 10 / 13), (x + s * 11 / 13, y + s * 3 / 13)];
                    p.emit(state, id, b, Primitive::Path { points: pts, fill: None, stroke: Some(mark), stroke_width: 2, closed: false });
                }
            }
        }
        ControlKind::Button | ControlKind::Submit => {
            let label = doc.map(|d| {
                let t = semantics::collapse(&d.text_content(node));
                if !t.is_empty() {
                    t
                } else if let Some(v) = d.attr(node, "value") {
                    semantics::collapse(v)
                } else if kind == ControlKind::Submit {
                    "Submit".into()
                } else {
                    String::new()
                }
            });
            let Some(label) = label else { return };
            // `<button>` children are laid out as normal content; only the input
            // kinds draw their label here.
            if doc.is_some_and(|d| d.is(node, "button")) {
                return;
            }
            if label.is_empty() {
                return;
            }
            let shown = cw_scene::metrics::ellipsize(font.typeface, font.scene_style(), &label, size, r.width);
            let w = text::width_px(&font, &shown);
            let x = r.x + (r.width.saturating_sub(w) / 2) as i32;
            let id = p.id(key, parts::CONTENT_TEXT);
            text::draw_text(p, &clipped, id, x, centred_baseline(r, size), &shown, &font, ink);
        }
        ControlKind::Select => {
            let chevron_w = 16u32.min(r.width);
            let text_w = r.width.saturating_sub(chevron_w);
            if !value.is_empty() && text_w > 0 {
                let shown = cw_scene::metrics::ellipsize(font.typeface, font.scene_style(), &value, size, text_w);
                let id = p.id(key, parts::CONTENT_TEXT);
                text::draw_text(p, &state.clipped(SRect::new(r.x, r.y, text_w, r.height)), id, r.x, centred_baseline(r, size), &shown, &font, ink);
            }
            let cx = r.right() - chevron_w as i32 / 2;
            let cy = r.y + r.height as i32 / 2;
            let id = p.id(key, parts::CONTENT_GLYPH);
            let pts = vec![(cx - 4, cy - 2), (cx, cy + 2), (cx + 4, cy - 2)];
            p.emit(state, id, r, Primitive::Path { points: pts, fill: None, stroke: Some(ink), stroke_width: 2, closed: false });
        }
        ControlKind::Range => {
            let (min, max, val) = range_values(doc, node, &value);
            let track_h = 4u32.min(r.height);
            let track = SRect::new(r.x, r.y + (r.height - track_h) as i32 / 2, r.width, track_h);
            let id = p.id(key, parts::CONTENT);
            p.emit(state, id, track, Primitive::RoundedBox { fill: PLACEHOLDER_EDGE, border: None, border_width: 0, radius: 2 });
            let thumb = 12u32.min(r.height).max(2);
            let span = r.width.saturating_sub(thumb) as i64;
            let frac = if max > min { (val - min).clamp(0, max - min) } else { 0 };
            let tx = r.x + if max > min { (span * frac / (max - min)) as i32 } else { 0 };
            let ty = r.y + (r.height - thumb) as i32 / 2;
            let id = p.id(key, parts::CONTENT_GLYPH);
            p.emit(state, id, SRect::new(tx, ty, thumb, thumb), Primitive::RoundedBox { fill: if disabled { DISABLED_TEXT } else { ACCENT }, border: None, border_width: 0, radius: thumb / 2 });
        }
        ControlKind::Hidden => {}
    }
}

fn range_values(doc: Option<&crate::dom::Document>, node: NodeId, value: &str) -> (i64, i64, i64) {
    let num = |s: Option<&str>| s.and_then(|v| v.trim().parse::<f64>().ok()).map(|v| v.round() as i64);
    let min = doc.and_then(|d| num(d.attr(node, "min"))).unwrap_or(0);
    let max = doc.and_then(|d| num(d.attr(node, "max"))).unwrap_or(100);
    let val = num(Some(value)).unwrap_or((min + max) / 2);
    (min, max, val)
}

/// A 1 px caret at `x` spanning the text line, and the scene focus caret.
#[allow(clippy::too_many_arguments)]
fn paint_caret(p: &mut Painter, key: (NodeId, u32), state: &State, x: i32, baseline: i32, size: u16, color: Color, line: u32, column: u32) {
    let bounds = SRect::new(x, baseline - size as i32, 1, text::line_height_px(size));
    let id = p.id(key, parts::CARET);
    p.emit(state, id, bounds, Primitive::Box { fill: color, border: None, border_width: 0 });
    if let Some(f) = &mut p.focus {
        f.caret = Some(cw_scene::Caret { bounds, line, column, offset: column });
    }
}

#[allow(dead_code)]
pub(crate) fn upx_(a: crate::geom::Au) -> u32 {
    upx(a)
}
