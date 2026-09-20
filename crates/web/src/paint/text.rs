//! Text runs, decorations, selection and shadows.
//!
//! A run becomes one `UiText`/`UiTextBold` node in the run's typeface, size, slant
//! and language. The renderer places a UI text node's first baseline `size` pixels
//! below the node's top (see `cw_render`), so the node's top is the fragment's
//! baseline minus the font size in pixels; its height is the renderer's line height
//! (`size + (size + 3) / 4`) and its width the measured advance plus slack so it
//! never wraps.
//!
//! `letter-spacing` has no scene parameter, so a spaced run is painted one node per
//! character, each advanced by `metrics::advance` plus the spacing.
//!
//! Decorations: underline `1px` below the baseline, line-through at `0.35em` above
//! it, overline at the ascent (`1em`), all `max(1, size / 16)` thick, in the
//! decoration colour or the text colour; `double` draws two lines, `dotted` and
//! `dashed` a run of boxes, `wavy` falls back to solid. `text-decoration` is read
//! from the run's style and, when the document is available, from every ancestor
//! element of the text node, since the property propagates to descendants rather
//! than inheriting.
//!
//! `text-shadow` paints offset copies of the run in the shadow colour before the
//! text; blur has no text primitive, so a blurred shadow is the same copy at half
//! alpha (its 1 px spread is what the renderer's edge would have shown).
//!
//! The `::selection` highlight is a box behind the selected part of the run, found
//! by summing glyph advances up to the selection's byte offsets.

use cw_scene::{Color, Primitive, Rect as SRect};

use super::{abs_rect, parts, px, Painter, State};
use crate::dom::NodeId;
use crate::layout::fragment::{Fragment, FragmentKind};
use crate::style::computed::{ComputedStyle, Font, TextDecoration, TextDecorationStyle};

pub(crate) const SELECTION: Color = Color(180, 213, 255, 255);

/// The renderer's line height for a UI text node of `size` px.
pub(crate) fn line_height_px(size: u16) -> u32 {
    size as u32 + (size as u32).div_ceil(4)
}

/// Width of `text` in `font`, whole pixels rounded up.
pub(crate) fn width_px(font: &Font, text: &str) -> u32 {
    cw_scene::metrics::text_width(font.typeface, font.scene_style(), text, font.size_px())
}

/// The bounds a text node needs so the renderer draws its baseline at `baseline`.
pub(crate) fn text_bounds(x: i32, baseline: i32, width: u32, size: u16) -> SRect {
    SRect::new(x, baseline - size as i32, width + 2, line_height_px(size))
}

/// Emits one text node with its baseline at `baseline`; returns the node index.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_text(p: &mut Painter, state: &State, id: u64, x: i32, baseline: i32, text: &str, font: &Font, color: Color) -> usize {
    let size = font.size_px();
    let w = width_px(font, text);
    let bounds = text_bounds(x, baseline, w, size);
    p.emit(state, id, bounds, Primitive::ui_text_face(text, color, size, font.scene_style(), font.typeface))
}

/// The decoration in force for a text run: the run's style, plus (when the DOM is
/// available) those of the text node's ancestors.
fn decoration_of(p: &Painter, style: &ComputedStyle, node: Option<NodeId>) -> Vec<TextDecoration> {
    let mut out = vec![style.text_decoration];
    if let (Some(doc), Some(n)) = (p.doc, node) {
        for a in doc.ancestors(n) {
            if let Some(s) = p.styles.get(a) {
                if s.text_decoration.any_line() && s.text_decoration != style.text_decoration {
                    out.push(s.text_decoration);
                }
            }
        }
    }
    out
}

/// Byte range of the selection within a text node's data, if it is selected.
fn selection_in(p: &Painter, node: NodeId, len: usize) -> Option<(usize, usize)> {
    let sel = p.ctx.selection?;
    let order = |n: NodeId| p.semantics.order.get(&n).copied().unwrap_or(n.0);
    let (s, e) = (order(sel.start.0), order(sel.end.0));
    let me = order(node);
    if me < s || me > e {
        return None;
    }
    let from = if node == sel.start.0 { sel.start.1 } else { 0 };
    let to = if node == sel.end.0 { sel.end.1 } else { len };
    (from < to).then_some((from.min(len), to.min(len)))
}

/// Pixel offset of byte `at` within `text`, by summing advances.
fn offset_px(font: &Font, text: &str, at: usize) -> i32 {
    let size = font.size_px();
    let style = font.scene_style();
    let sum: i64 = text[..at.min(text.len())].chars().map(|c| cw_scene::metrics::advance(font.typeface, style, c, size)).sum();
    ((sum + 32) / 64) as i32
}

/// Paints one text run fragment.
pub(crate) fn paint_run(p: &mut Painter, f: &Fragment, state: &State) {
    let FragmentKind::Text { source, text, node, range, baseline, ellipsis } = &f.kind else { return };
    if text.is_empty() && !*ellipsis {
        // A preserved newline's zero-width run: nothing to draw.
        return;
    }
    let Some(key) = p.key(f) else { return };
    let style = p.style(*source).clone();
    let rect = abs_rect(state, f);
    let srect = super::snap(rect);
    let font = &style.font;
    let size = font.size_px();
    let x = px(rect.origin.x);
    let baseline_px = px(rect.origin.y + *baseline);
    let mut shown = text.clone();
    if *ellipsis && !shown.ends_with('\u{2026}') {
        shown.push('\u{2026}');
    }
    let width = width_px(font, &shown);
    let bounds = text_bounds(x, baseline_px, width, size);
    p.record_hit(state, source.node(), srect, 0, matches!(style.pointer_events, crate::style::computed::PointerEvents::None));

    // Selection highlight.
    if let Some(n) = node {
        if let Some((from, to)) = selection_in(p, *n, text.len()) {
            let (lo, hi) = (from.max(range.0), to.min(range.1));
            if lo < hi && hi - range.0 <= text.len() {
                let x0 = x + offset_px(font, text, lo - range.0);
                let x1 = x + offset_px(font, text, hi - range.0);
                let id = p.id(key, parts::SELECTION);
                p.emit(state, id, SRect::new(x0, srect.y, (x1 - x0).max(1) as u32, srect.height.max(line_height_px(size))), Primitive::Box { fill: SELECTION, border: None, border_width: 0 });
            }
        }
    }

    // Shadows, last first so the first listed is on top.
    for sh in style.text_shadow.iter().rev() {
        if sh.color.3 == 0 {
            continue;
        }
        let mut color = sh.color;
        if sh.blur > crate::geom::Au::ZERO {
            color.3 /= 2;
        }
        let part = p.next_part(key);
        let id = p.id(key, part);
        draw_spaced(p, state, key, id, x + px(sh.offset_x), baseline_px + px(sh.offset_y), &shown, font, color, style.letter_spacing, false);
    }

    // The text itself.
    let id = p.id(key, parts::TEXT);
    let idx = draw_spaced(p, state, key, id, x, baseline_px, &shown, font, style.color, style.letter_spacing, true);
    if let Some(i) = idx {
        p.nodes[i].semantic = Some(cw_scene::Semantic { role: "text".into(), label: shown.clone(), value: None, disabled: false, focusable: false });
    }

    // Decorations.
    let thickness = (size as u32 / 16).max(1);
    let total_width = if style.letter_spacing.is_zero() { width } else { (bounds.width - 2) + shown.chars().count() as u32 * px(style.letter_spacing).max(0) as u32 };
    for deco in decoration_of(p, &style, *node) {
        let color = deco.color.unwrap_or(style.color);
        let lines = [
            (deco.underline, parts::UNDERLINE, baseline_px + 1),
            (deco.line_through, parts::LINE_THROUGH, baseline_px - (size as i32 * 35 / 100)),
            (deco.overline, parts::OVERLINE, baseline_px - size as i32),
        ];
        for (on, part, y) in lines {
            if !on {
                continue;
            }
            let line = SRect::new(x, y, total_width, thickness);
            let first = p.id(key, part);
            match deco.style {
                TextDecorationStyle::Double => {
                    p.emit(state, first, line, Primitive::Box { fill: color, border: None, border_width: 0 });
                    let second = p.next_part(key);
                    let id = p.id(key, second);
                    p.emit(state, id, SRect::new(x, y + 2 * thickness as i32, total_width, thickness), Primitive::Box { fill: color, border: None, border_width: 0 });
                }
                TextDecorationStyle::Dotted | TextDecorationStyle::Dashed => {
                    let dash = if deco.style == TextDecorationStyle::Dotted { thickness } else { 3 * thickness };
                    let mut at = 0;
                    let mut n = 0;
                    while at < total_width && n < 2048 {
                        let part = if n == 0 { part } else { p.next_part(key) };
                        let id = p.id(key, part);
                        p.emit(state, id, SRect::new(x + at as i32, y, dash.min(total_width - at), thickness), Primitive::Box { fill: color, border: None, border_width: 0 });
                        at += 2 * dash;
                        n += 1;
                    }
                }
                TextDecorationStyle::Solid | TextDecorationStyle::Wavy => {
                    p.emit(state, first, line, Primitive::Box { fill: color, border: None, border_width: 0 });
                }
            }
        }
    }
}

/// Draws `text` as one node, or one node per character when `spacing` is non-zero.
/// Returns the index of the single node when there is one.
#[allow(clippy::too_many_arguments)]
fn draw_spaced(p: &mut Painter, state: &State, key: (NodeId, u32), id: u64, x: i32, baseline: i32, text: &str, font: &Font, color: Color, spacing: crate::geom::Au, main: bool) -> Option<usize> {
    if spacing.is_zero() {
        return Some(draw_text(p, state, id, x, baseline, text, font, color));
    }
    let size = font.size_px();
    let style = font.scene_style();
    let sp = px(spacing);
    let mut pen: i64 = (x as i64) * 64;
    let mut first = true;
    for ch in text.chars() {
        let s = ch.to_string();
        let part_id = if first && main { id } else {
            let part = p.next_part(key);
            p.id(key, part)
        };
        first = false;
        let gx = ((pen + 32) / 64) as i32;
        let w = cw_scene::metrics::text_width(font.typeface, style, &s, size);
        p.emit(state, part_id, text_bounds(gx, baseline, w, size), Primitive::ui_text_face(s, color, size, style, font.typeface));
        pen += cw_scene::metrics::advance(font.typeface, style, ch, size) + sp as i64 * 64;
    }
    None
}
