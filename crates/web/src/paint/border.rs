//! Borders, outlines, shadows and corner radii.
//!
//! Every side has its own width, style and colour. `solid` is a box strip per side
//! (mitred corners come out as overlapping strips, top and bottom spanning the full
//! width); `double` is two strips of a third each; `dashed` and `dotted` are runs
//! of boxes along the side (dash `3w` with gap `w`; dots `w` square with gap `w`);
//! `groove`, `ridge`, `inset` and `outset` are two-tone solids (the light half at
//! 40% towards white, the dark half at 40% towards black, swapped between the
//! top/left and bottom/right sides and, for groove/ridge, between the outer and
//! inner halves).
//!
//! With a uniform radius and a uniform solid border the whole ring is one
//! `RoundedBox`; with radii but differing sides each side is a `Path` stroke that
//! follows the straight edge and half of each adjacent corner arc (arcs are integer
//! polylines from [`super::trig`]). Dash patterns ignore radii.
//!
//! `outline` is drawn outside the border box, pushed out by `outline-offset`, and
//! does not affect layout. Outer `box-shadow` is a `Shadow` primitive (its bounds
//! include the blur padding, as the scene requires); an inset shadow is four shadow
//! strips just outside the padding box, clipped to it, which darkens the edges the
//! way a real inset shadow does without a dedicated primitive.

use cw_scene::{Color, Primitive, Rect as SRect};

use super::{parts, px, upx, Painter, State};
use crate::dom::NodeId;
use crate::geom::Edges;
use crate::style::computed::{BorderSide, BorderStyle, ComputedStyle, Corners};

/// Corner radii in scene pixels, each the smaller of the corner's two radii, scaled
/// down together when they would overlap (CSS Backgrounds §5.5).
pub(crate) fn radii_px(style: &ComputedStyle, rect: SRect) -> Corners<u32> {
    let w = crate::geom::Au::from_px_i32(rect.width as i32);
    let h = crate::geom::Au::from_px_i32(rect.height as i32);
    let one = |(a, b): (crate::style::computed::LengthPercentage, crate::style::computed::LengthPercentage)| {
        let ra = upx(a.resolve(w));
        let rb = upx(b.resolve(h));
        ra.min(rb)
    };
    let r = style.border_radius;
    let mut c = Corners { top_left: one(r.top_left), top_right: one(r.top_right), bottom_right: one(r.bottom_right), bottom_left: one(r.bottom_left) };
    // Scale so adjacent radii never exceed the side length.
    let limit = |a: &mut u32, b: &mut u32, len: u32| {
        let sum = *a + *b;
        if sum > len && sum > 0 {
            *a = (*a as u64 * len as u64 / sum as u64) as u32;
            *b = (*b as u64 * len as u64 / sum as u64) as u32;
        }
    };
    let (mut tl, mut tr, mut br, mut bl) = (c.top_left, c.top_right, c.bottom_right, c.bottom_left);
    limit(&mut tl, &mut tr, rect.width);
    limit(&mut bl, &mut br, rect.width);
    limit(&mut tl, &mut bl, rect.height);
    limit(&mut tr, &mut br, rect.height);
    c = Corners { top_left: tl, top_right: tr, bottom_right: br, bottom_left: bl };
    c
}

pub(crate) fn uniform_radius(c: &Corners<u32>) -> Option<u32> {
    if c.top_left == c.top_right && c.top_right == c.bottom_right && c.bottom_right == c.bottom_left {
        Some(c.top_left)
    } else {
        None
    }
}

pub(crate) fn any_radius(c: &Corners<u32>) -> bool {
    c.top_left > 0 || c.top_right > 0 || c.bottom_right > 0 || c.bottom_left > 0
}

/// The outline of a rectangle with per-corner radii, clockwise from the top-left
/// corner's end of the top edge.
pub(crate) fn rounded_polygon(r: SRect, c: Corners<u32>) -> Vec<(i32, i32)> {
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let mut pts = Vec::new();
    let corner = |pts: &mut Vec<(i32, i32)>, radius: u32, cx: i32, cy: i32, start: i32, fallback: (i32, i32)| {
        if radius == 0 {
            pts.push(fallback);
        } else {
            pts.extend(super::trig::quarter_arc(cx, cy, radius, start));
        }
    };
    // Top-right: centre (x1 - r, y0 + r), from 270 to 360.
    let tr = c.top_right;
    corner(&mut pts, tr, x1 - tr as i32, y0 + tr as i32, 27_000, (x1, y0));
    let br = c.bottom_right;
    corner(&mut pts, br, x1 - br as i32, y1 - br as i32, 0, (x1, y1));
    let bl = c.bottom_left;
    corner(&mut pts, bl, x0 + bl as i32, y1 - bl as i32, 9000, (x0, y1));
    let tl = c.top_left;
    corner(&mut pts, tl, x0 + tl as i32, y0 + tl as i32, 18_000, (x0, y0));
    pts.dedup();
    pts
}

fn lighten(c: Color, pct: u32) -> Color {
    let f = |v: u8| (v as u32 + (255 - v as u32) * pct / 100) as u8;
    Color(f(c.0), f(c.1), f(c.2), c.3)
}

fn darken(c: Color, pct: u32) -> Color {
    let f = |v: u8| (v as u32 * (100 - pct) / 100) as u8;
    Color(f(c.0), f(c.1), f(c.2), c.3)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    fn part(self) -> u32 {
        match self {
            Side::Top => parts::BORDER_TOP,
            Side::Right => parts::BORDER_RIGHT,
            Side::Bottom => parts::BORDER_BOTTOM,
            Side::Left => parts::BORDER_LEFT,
        }
    }
    fn is_top_left(self) -> bool {
        matches!(self, Side::Top | Side::Left)
    }
}

/// The strip a side occupies inside `rect` (border box), given all four widths, so
/// the top and bottom strips span the full width and the sides sit between them.
fn side_strip(rect: SRect, widths: [u32; 4], side: Side) -> SRect {
    let [t, r, b, l] = widths;
    match side {
        Side::Top => SRect::new(rect.x, rect.y, rect.width, t.min(rect.height)),
        Side::Bottom => SRect::new(rect.x, rect.bottom() - b.min(rect.height) as i32, rect.width, b.min(rect.height)),
        Side::Left => {
            let inner = rect.height.saturating_sub(t + b);
            SRect::new(rect.x, rect.y + t as i32, l.min(rect.width), inner)
        }
        Side::Right => {
            let inner = rect.height.saturating_sub(t + b);
            SRect::new(rect.right() - r.min(rect.width) as i32, rect.y + t as i32, r.min(rect.width), inner)
        }
    }
}

fn box_node(fill: Color) -> Primitive {
    Primitive::Box { fill, border: None, border_width: 0 }
}

/// Paints the four borders of a box. `first`/`last` say whether an inline box
/// fragment is the first/last piece on its line: the left/right borders are omitted
/// on open ends.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_borders(p: &mut Painter, key: (NodeId, u32), state: &State, style: &ComputedStyle, rect: SRect, border: Edges, first: bool, last: bool) {
    let sides = [
        (Side::Top, style.border.top, upx(border.top)),
        (Side::Right, style.border.right, if last { upx(border.right) } else { 0 }),
        (Side::Bottom, style.border.bottom, upx(border.bottom)),
        (Side::Left, style.border.left, if first { upx(border.left) } else { 0 }),
    ];
    let widths = [sides[0].2, sides[1].2, sides[2].2, sides[3].2];
    if widths.iter().all(|w| *w == 0) || rect.width == 0 || rect.height == 0 {
        return;
    }
    let radii = radii_px(style, rect);
    let uniform = sides.iter().all(|(_, s, w)| *w == widths[0] && s.style == sides[0].1.style && s.color == sides[0].1.color);
    if let (Some(r), true, BorderStyle::Solid) = (uniform_radius(&radii), uniform, sides[0].1.style) {
        if r > 0 && sides[0].1.style.is_visible() {
            let id = p.id(key, parts::BORDER_TOP);
            p.emit(state, id, rect, Primitive::RoundedBox { fill: Color::TRANSPARENT, border: Some(sides[0].1.color), border_width: widths[0], radius: r });
            return;
        }
    }
    let rounded = any_radius(&radii);
    for (side, bs, w) in sides {
        if w == 0 || !bs.style.is_visible() || bs.color.3 == 0 {
            continue;
        }
        if rounded && matches!(bs.style, BorderStyle::Solid | BorderStyle::Double | BorderStyle::Groove | BorderStyle::Ridge | BorderStyle::Inset | BorderStyle::Outset) {
            paint_rounded_side(p, key, state, rect, radii, side, bs, w);
            continue;
        }
        let strip = side_strip(rect, widths, side);
        match bs.style {
            BorderStyle::Solid => {
                let id = p.id(key, side.part());
                p.emit(state, id, strip, box_node(bs.color));
            }
            BorderStyle::Double => {
                let third = (w / 3).max(1);
                let (a, b) = split_strip(strip, side, third);
                let id = p.id(key, side.part());
                p.emit(state, id, a, box_node(bs.color));
                let id2 = p.next_part(key);
                let id2 = p.id(key, id2);
                p.emit(state, id2, b, box_node(bs.color));
            }
            BorderStyle::Dashed | BorderStyle::Dotted => {
                let dotted = bs.style == BorderStyle::Dotted;
                let (dash, gap) = if dotted { (w, w) } else { (3 * w, w) };
                let horizontal = matches!(side, Side::Top | Side::Bottom);
                let len = if horizontal { strip.width } else { strip.height };
                let mut at = 0u32;
                let mut count = 0;
                while at < len && count < 4096 {
                    let d = dash.min(len - at);
                    let r = if horizontal { SRect::new(strip.x + at as i32, strip.y, d, strip.height) } else { SRect::new(strip.x, strip.y + at as i32, strip.width, d) };
                    let part = if count == 0 { side.part() } else { p.next_part(key) };
                    let id = p.id(key, part);
                    if dotted && w > 1 {
                        p.emit(state, id, r, Primitive::RoundedBox { fill: bs.color, border: None, border_width: 0, radius: w / 2 });
                    } else {
                        p.emit(state, id, r, box_node(bs.color));
                    }
                    at += dash + gap;
                    count += 1;
                }
            }
            BorderStyle::Groove | BorderStyle::Ridge | BorderStyle::Inset | BorderStyle::Outset => {
                let (outer, inner) = two_tone(bs, side);
                match bs.style {
                    BorderStyle::Inset | BorderStyle::Outset => {
                        let id = p.id(key, side.part());
                        p.emit(state, id, strip, box_node(outer));
                    }
                    _ => {
                        let half = (w / 2).max(1);
                        let (a, b) = split_strip(strip, side, half);
                        let id = p.id(key, side.part());
                        p.emit(state, id, a, box_node(outer));
                        let id2 = p.next_part(key);
                        let id2 = p.id(key, id2);
                        p.emit(state, id2, b, box_node(inner));
                    }
                }
            }
            BorderStyle::None | BorderStyle::Hidden => {}
        }
    }
}

/// The outer and inner colours of a 3-D border style on one side.
fn two_tone(bs: BorderSide, side: Side) -> (Color, Color) {
    let light = lighten(bs.color, 40);
    let dark = darken(bs.color, 40);
    let top_left = side.is_top_left();
    match bs.style {
        BorderStyle::Inset => (if top_left { dark } else { light }, if top_left { dark } else { light }),
        BorderStyle::Outset => (if top_left { light } else { dark }, if top_left { light } else { dark }),
        BorderStyle::Groove => (if top_left { dark } else { light }, if top_left { light } else { dark }),
        BorderStyle::Ridge => (if top_left { light } else { dark }, if top_left { dark } else { light }),
        _ => (bs.color, bs.color),
    }
}

/// The outer and inner strips of a side, each `t` thick.
fn split_strip(strip: SRect, side: Side, t: u32) -> (SRect, SRect) {
    match side {
        Side::Top => (SRect::new(strip.x, strip.y, strip.width, t), SRect::new(strip.x, strip.bottom() - t as i32, strip.width, t)),
        Side::Bottom => (SRect::new(strip.x, strip.bottom() - t as i32, strip.width, t), SRect::new(strip.x, strip.y, strip.width, t)),
        Side::Left => (SRect::new(strip.x, strip.y, t, strip.height), SRect::new(strip.right() - t as i32, strip.y, t, strip.height)),
        Side::Right => (SRect::new(strip.right() - t as i32, strip.y, t, strip.height), SRect::new(strip.x, strip.y, t, strip.height)),
    }
}

/// One side of a rounded border as a stroked path along the middle of the border
/// strip: the straight edge plus half of each adjacent corner arc.
#[allow(clippy::too_many_arguments)]
fn paint_rounded_side(p: &mut Painter, key: (NodeId, u32), state: &State, rect: SRect, radii: Corners<u32>, side: Side, bs: BorderSide, w: u32) {
    let half = (w / 2) as i32;
    let inset = SRect::new(rect.x + half, rect.y + half, rect.width.saturating_sub(w), rect.height.saturating_sub(w));
    let shrink = |r: u32| r.saturating_sub(w / 2);
    let (x0, y0, x1, y1) = (inset.x, inset.y, inset.right(), inset.bottom());
    let half_arc = |cx: i32, cy: i32, r: u32, start: i32| -> Vec<(i32, i32)> {
        if r == 0 {
            return Vec::new();
        }
        let arc = super::trig::quarter_arc(cx, cy, r, start);
        let n = arc.len();
        arc.into_iter().skip(n / 2).collect()
    };
    let first_half = |cx: i32, cy: i32, r: u32, start: i32| -> Vec<(i32, i32)> {
        if r == 0 {
            return Vec::new();
        }
        let arc = super::trig::quarter_arc(cx, cy, r, start);
        let n = arc.len();
        arc.into_iter().take(n / 2 + 1).collect()
    };
    let mut pts: Vec<(i32, i32)> = Vec::new();
    match side {
        Side::Top => {
            let (tl, tr) = (shrink(radii.top_left), shrink(radii.top_right));
            pts.extend(half_arc(x0 + tl as i32, y0 + tl as i32, tl, 18_000));
            if tl == 0 {
                pts.push((x0, y0));
            }
            if tr == 0 {
                pts.push((x1, y0));
            }
            pts.extend(first_half(x1 - tr as i32, y0 + tr as i32, tr, 27_000));
        }
        Side::Right => {
            let (tr, br) = (shrink(radii.top_right), shrink(radii.bottom_right));
            pts.extend(half_arc(x1 - tr as i32, y0 + tr as i32, tr, 27_000));
            if tr == 0 {
                pts.push((x1, y0));
            }
            if br == 0 {
                pts.push((x1, y1));
            }
            pts.extend(first_half(x1 - br as i32, y1 - br as i32, br, 0));
        }
        Side::Bottom => {
            let (br, bl) = (shrink(radii.bottom_right), shrink(radii.bottom_left));
            pts.extend(half_arc(x1 - br as i32, y1 - br as i32, br, 0));
            if br == 0 {
                pts.push((x1, y1));
            }
            if bl == 0 {
                pts.push((x0, y1));
            }
            pts.extend(first_half(x0 + bl as i32, y1 - bl as i32, bl, 9000));
        }
        Side::Left => {
            let (bl, tl) = (shrink(radii.bottom_left), shrink(radii.top_left));
            pts.extend(half_arc(x0 + bl as i32, y1 - bl as i32, bl, 9000));
            if bl == 0 {
                pts.push((x0, y1));
            }
            if tl == 0 {
                pts.push((x0, y0));
            }
            pts.extend(first_half(x0 + tl as i32, y0 + tl as i32, tl, 18_000));
        }
    }
    pts.dedup();
    let color = match bs.style {
        BorderStyle::Groove | BorderStyle::Ridge | BorderStyle::Inset | BorderStyle::Outset => two_tone(bs, side).0,
        _ => bs.color,
    };
    let id = p.id(key, side.part());
    p.emit(state, id, rect, Primitive::Path { points: pts, fill: None, stroke: Some(color), stroke_width: w.min(u16::MAX as u32) as u16, closed: false });
}

/// `outline` around the border box, offset outwards by `outline-offset`.
pub(crate) fn paint_outline(p: &mut Painter, key: (NodeId, u32), state: &State, style: &ComputedStyle, rect: SRect) {
    let o = style.outline;
    if !o.style.is_visible() || o.width <= crate::geom::Au::ZERO || o.color.3 == 0 {
        return;
    }
    let w = upx(o.width).max(1);
    let off = px(style.outline_offset);
    let grow = w as i32 + off;
    let outer = SRect::new(rect.x - grow, rect.y - grow, (rect.width as i64 + 2 * grow as i64).max(0) as u32, (rect.height as i64 + 2 * grow as i64).max(0) as u32);
    if outer.width == 0 || outer.height == 0 {
        return;
    }
    let radii = radii_px(style, rect);
    let id = p.id(key, parts::OUTLINE);
    match uniform_radius(&radii) {
        Some(r) if r > 0 => {
            p.emit(state, id, outer, Primitive::RoundedBox { fill: Color::TRANSPARENT, border: Some(o.color), border_width: w, radius: r + grow.max(0) as u32 });
        }
        _ => {
            let widths = [w; 4];
            let mut first = true;
            for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
                let strip = side_strip(outer, widths, side);
                let part = if first { parts::OUTLINE } else { p.next_part(key) };
                first = false;
                let id = p.id(key, part);
                match o.style {
                    BorderStyle::Dashed | BorderStyle::Dotted => {
                        let dotted = o.style == BorderStyle::Dotted;
                        let (dash, gap) = if dotted { (w, w) } else { (3 * w, w) };
                        let horizontal = matches!(side, Side::Top | Side::Bottom);
                        let len = if horizontal { strip.width } else { strip.height };
                        let mut at = 0u32;
                        let mut n = 0;
                        while at < len && n < 4096 {
                            let d = dash.min(len - at);
                            let r = if horizontal { SRect::new(strip.x + at as i32, strip.y, d, strip.height) } else { SRect::new(strip.x, strip.y + at as i32, strip.width, d) };
                            let part = p.next_part(key);
                            let id = p.id(key, part);
                            p.emit(state, id, r, box_node(o.color));
                            at += dash + gap;
                            n += 1;
                        }
                    }
                    _ => {
                        p.emit(state, id, strip, box_node(o.color));
                    }
                }
            }
        }
    }
}

/// `box-shadow`: the outer shadows when `inset` is false, the inset ones when true.
/// Shadows are painted last-to-first so the first listed ends up on top.
pub(crate) fn paint_box_shadows(p: &mut Painter, key: (NodeId, u32), state: &State, style: &ComputedStyle, rect: SRect, inset: bool) {
    let radii = radii_px(style, rect);
    let radius = uniform_radius(&radii).unwrap_or(radii.top_left.max(radii.top_right).max(radii.bottom_left).max(radii.bottom_right));
    for sh in style.box_shadow.iter().rev() {
        if sh.inset != inset || sh.color.3 == 0 {
            continue;
        }
        let blur = upx(sh.blur);
        let spread = px(sh.spread);
        let (dx, dy) = (px(sh.offset_x), px(sh.offset_y));
        if !inset {
            let grown = SRect::new(rect.x + dx - spread, rect.y + dy - spread, (rect.width as i64 + 2 * spread as i64).max(0) as u32, (rect.height as i64 + 2 * spread as i64).max(0) as u32);
            if grown.width == 0 || grown.height == 0 {
                continue;
            }
            let bounds = SRect::new(grown.x - blur as i32, grown.y - blur as i32, grown.width + 2 * blur, grown.height + 2 * blur);
            let part = p.next_part(key);
            let id = p.id(key, part);
            p.emit(state, id, bounds, Primitive::Shadow { color: sh.color, radius: (radius as i64 + spread as i64).max(0) as u32, blur });
        } else {
            // Four strips just outside the padding box, shifted by the offset and
            // pulled in by the spread, clipped to the box.
            let clipped = state.clipped(rect);
            let t = (blur.max(1) + spread.max(0) as u32).max(1);
            let strips = [
                SRect::new(rect.x - t as i32 + dx, rect.y - t as i32 + dy, rect.width + 2 * t, t),
                SRect::new(rect.x - t as i32 + dx, rect.bottom() - spread + dy, rect.width + 2 * t, t),
                SRect::new(rect.x - t as i32 + dx, rect.y - t as i32 + dy, t, rect.height + 2 * t),
                SRect::new(rect.right() - spread + dx, rect.y - t as i32 + dy, t, rect.height + 2 * t),
            ];
            for s in strips {
                let bounds = SRect::new(s.x - blur as i32, s.y - blur as i32, s.width + 2 * blur, s.height + 2 * blur);
                let part = p.next_part(key);
                let id = p.id(key, part);
                p.emit(&clipped, id, bounds, Primitive::Shadow { color: sh.color, radius: 0, blur });
            }
        }
    }
}
