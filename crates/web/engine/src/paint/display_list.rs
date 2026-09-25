//! Painting order: CSS 2.1 Appendix E.
//!
//! Every stacking context is painted in this order:
//!
//! 1. the background and borders of the element establishing the context;
//! 2. child stacking contexts with negative `z-index`, most negative first;
//! 3. in-flow, non-positioned, block-level descendants: backgrounds and borders in
//!    tree order;
//! 4. non-positioned floats, each painted atomically (as if it were a stacking
//!    context with `z-index: 0`);
//! 5. in-flow inline-level content in tree order: inline boxes' backgrounds and
//!    borders, text runs, replaced content, and atomic inline-level boxes
//!    (inline-blocks, inline tables) painted atomically;
//! 6. positioned descendants with `z-index: auto` (painted atomically) or `0`
//!    (stacking contexts), in tree order;
//! 7. child stacking contexts with positive `z-index`, least positive first;
//! 8. outlines of everything in the context.
//!
//! A fragment establishes a stacking context when layout marked it so, or when its
//! computed style says so (`opacity < 1`, a `transform`, positioned with a numeric
//! `z-index`); the fragment's `z_index` orders it. Floats, atomic inlines and
//! positioned boxes with `z-index: auto` are painted atomically "as if" they were
//! stacking contexts, but are not: their positioned descendants and the descendants
//! that really establish contexts belong to the enclosing context (Appendix E, the
//! note under steps 4, 5 and 8), so they are handed up when the box is collected and
//! take their place in the enclosing context's layers 2, 6 and 7 in document order.
//! Acid2's eyes depend on it: `.eyes` (absolute, inside the `z-index: auto`
//! `.picture`) must paint over a fixed `<p>` that precedes it in the document.
//!
//! A scroll container that reserved a gutter draws a classic bar in it, after the
//! contents of the context it was collected in and in its own coordinate space, so
//! the bar neither scrolls nor is clipped by the scrollport. A host that overlays
//! its scrollbars reserves no gutter and so draws none, and the viewport's bars are
//! the shell's.
//!
//! Group `opacity` has no scene primitive, so every node of the group carries the
//! product of its ancestors' opacities. Overflow clips are `Node::clip` (and
//! `rounded_clip` when the clipping box has radii) intersected down the tree.
//! `transform` composes 2-D affine matrices in the scene's 1/1024 units, all of
//! translate, scale, rotate and skew (the scene's `Transform` is a full affine map);
//! a transformed ancestor's overflow clip is applied as the transformed bounding box
//! (the scene clips in untransformed coordinates), which is exact for translations
//! and axis-aligned scales and a conservative box for rotations.

use cw_scene::{Rect as SRect, RoundedClip, ScrollArea, Transform};

use super::{abs_rect, background, border, px, replaced, semantics, snap, text, Painter, State};
use crate::geom::{Au, Point, Rect};
use crate::layout::fragment::{Fragment, FragmentKind, StyleSource};
use crate::style::computed::{ComputedStyle, Overflow, Position, TransformOp, Visibility};

/// One deferred paint job: a fragment and the state of its parent's coordinate space.
struct Item<'a> {
    frag: &'a Fragment,
    state: State,
    /// For a box painted atomically without being a stacking context (a float, an
    /// atomic inline, a positioned box with `z-index: auto`): its own layers,
    /// collected when it was met, less the positioned and stacking-context
    /// descendants that were handed to the enclosing context.
    inner: Option<Box<Buckets<'a>>>,
}

impl<'a> Item<'a> {
    fn new(frag: &'a Fragment, state: &State) -> Item<'a> {
        Item {
            frag,
            state: state.clone(),
            inner: None,
        }
    }
}

enum Inline<'a> {
    /// An inline box's background and borders.
    Box(Item<'a>),
    Text(Item<'a>),
    /// Replaced content of a box (an image, a control) at the content phase.
    Content(Item<'a>),
    /// An atomic inline-level box, painted as its own stacking context.
    Atomic(Item<'a>),
}

#[derive(Default)]
struct Buckets<'a> {
    negative: Vec<(i32, usize, Item<'a>)>,
    blocks: Vec<Item<'a>>,
    floats: Vec<Item<'a>>,
    inline: Vec<Inline<'a>>,
    /// `z-index: auto` positioned boxes and `z-index: 0` contexts, in tree order.
    layer_zero: Vec<(bool, Item<'a>)>,
    positive: Vec<(i32, usize, Item<'a>)>,
    outlines: Vec<Item<'a>>,
    /// Scroll containers that reserved a gutter, painted last so their bar sits over
    /// their own contents.
    bars: Vec<Item<'a>>,
    order: usize,
}

pub(crate) fn establishes_context(f: &Fragment, style: &ComputedStyle) -> bool {
    if f.establishes_stacking_context {
        return true;
    }
    if matches!(f.kind, FragmentKind::Box { .. }) {
        return style.establishes_stacking_context(false)
            && !f.source().is_some_and(StyleSource::is_anonymous);
    }
    false
}

/// The absolute border box of a fragment in `Au`, with `position: fixed` boxes
/// pinned to the viewport.
pub(crate) fn box_rect(p: &Painter, f: &Fragment, state: &State) -> Rect {
    let r = abs_rect(state, f);
    if p.style_of(f).position == Position::Fixed {
        r.translate(state.scrolled.x, state.scrolled.y)
    } else {
        r
    }
}

/// Folds the element's own `transform` (about its `transform-origin`) into `s`.
fn apply_transform(style: &ComputedStyle, rect: Rect, s: &mut State) {
    if style.transform.is_empty() {
        return;
    }
    let srect = snap(rect);
    let ox = srect.x + px(style.transform_origin.0.resolve(rect.size.width));
    let oy = srect.y + px(style.transform_origin.1.resolve(rect.size.height));
    let local = transform_matrix(&style.transform, rect.size, (ox, oy));
    s.transform = compose(&s.transform, &local);
    s.transformed = true;
}

/// The state a fragment paints *itself* in: its parent's, with its own `transform`
/// applied. A transform moves the element's background, borders, replaced content
/// and outline along with its children; `enter` only covers the children.
pub(crate) fn own_state(p: &Painter, f: &Fragment, state: &State) -> State {
    let mut s = state.clone();
    let style = p.style_of(f);
    if !style.transform.is_empty() && f.source().is_some_and(|src| !src.is_anonymous()) {
        let rect = box_rect(p, f, state);
        apply_transform(style, rect, &mut s);
    }
    s
}

/// The state a fragment's children inherit: origin at its border box, its opacity,
/// transform and overflow clip folded in, its scroll offset applied.
pub(crate) fn enter(p: &mut Painter, f: &Fragment, state: &State, register_scroll: bool) -> State {
    let rect = box_rect(p, f, state);
    let style = p.style_of(f).clone();
    let mut s = state.clone();
    s.origin = rect.origin;
    if style.position == Position::Fixed {
        s.scrolled = Point::default();
    }
    if style.opacity < 255 {
        s.opacity = super::mul_opacity(s.opacity, style.opacity);
    }
    apply_transform(&style, rect, &mut s);
    if let FragmentKind::Box {
        padding,
        border,
        scroll,
        source,
        ..
    } = &f.kind
    {
        let clips = !source.is_anonymous()
            && (!matches!(style.overflow_x, Overflow::Visible)
                || !matches!(style.overflow_y, Overflow::Visible));
        if clips {
            let padding_box = border.inset(rect);
            let mut clip = snap(padding_box);
            if s.transformed {
                clip = s.transform.bounds(clip);
            }
            s = s.clipped(clip);
            let radii = border::radii_px(&style, snap(rect));
            if let Some(r) = border::uniform_radius(&radii) {
                if r > 0 {
                    s.rounded_clip = Some(RoundedClip {
                        rect: snap(padding_box),
                        radius: r,
                    });
                }
            }
        }
        // The root fragment's scroll is the document's: `paint_root` applied it from
        // `PaintContext::scroll` and registered its scroll area, so applying the
        // offset layout recorded on the root would scroll the page twice (the root's
        // inner scroll area is still registered, at offset 0, as it always was).
        let is_root =
            matches!(source, StyleSource::Anonymous(n) if *n == crate::dom::Document::ROOT);
        if let Some(info) = scroll {
            let node = source.node();
            let off = if is_root {
                Point::default()
            } else {
                p.scroll_of(node, info)
            };
            s.origin.x -= off.x;
            s.origin.y -= off.y;
            s.scrolled.x += off.x;
            s.scrolled.y += off.y;
            if register_scroll {
                let padding_box = border.inset(rect);
                let content_box = padding.inset(padding_box);
                let target = format!(
                    "pane:{}",
                    p.doc
                        .map(|d| semantics::interaction_id(d, node))
                        .unwrap_or_else(|| format!("n{}", node.0))
                );
                let view = snap(padding_box);
                p.scrolls.push(ScrollArea {
                    target: target.clone(),
                    window: None,
                    bounds: view,
                    offset: px(off.y),
                    extent: super::upx(info.content_height).max(view.height),
                    title: None,
                    title_height: 0,
                    horizontal: false,
                });
                if info.content_width > content_box.size.width || info.shows_x_bar {
                    p.scrolls.push(ScrollArea {
                        target,
                        window: None,
                        bounds: view,
                        offset: px(off.x),
                        extent: super::upx(info.content_width).max(view.width),
                        title: None,
                        title_height: 0,
                        horizontal: true,
                    });
                }
            }
        }
    }
    s
}

/// The affine matrix of a `transform` list about `origin` (scene pixels).
pub(crate) fn transform_matrix(
    ops: &[TransformOp],
    size: crate::geom::Size,
    origin: (i32, i32),
) -> Transform {
    let mut m = Transform::default();
    for op in ops {
        let t = match *op {
            TransformOp::Translate(x, y) => {
                Transform::translate(px(x.resolve(size.width)), px(y.resolve(size.height)))
            }
            TransformOp::Scale(sx, sy) => Transform {
                a: scale_1024(sx),
                b: 0,
                c: 0,
                d: scale_1024(sy),
                tx: 0,
                ty: 0,
            },
            TransformOp::Rotate(deg) => {
                let (s, c) = (super::trig::sin_1024(deg), super::trig::cos_1024(deg));
                Transform {
                    a: c,
                    b: s,
                    c: -s,
                    d: c,
                    tx: 0,
                    ty: 0,
                }
            }
            TransformOp::SkewX(deg) => Transform {
                a: 1024,
                b: 0,
                c: super::trig::tan_1024(deg),
                d: 1024,
                tx: 0,
                ty: 0,
            },
            TransformOp::SkewY(deg) => Transform {
                a: 1024,
                b: super::trig::tan_1024(deg),
                c: 0,
                d: 1024,
                tx: 0,
                ty: 0,
            },
        };
        m = compose(&m, &t);
    }
    let to = Transform::translate(origin.0, origin.1);
    let from = Transform::translate(-origin.0, -origin.1);
    compose(&compose(&to, &m), &from)
}

fn scale_1024(v: i32) -> i32 {
    ((v as i64 * 1024 + 500) / 1000).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// `outer ∘ inner`: apply `inner` first, then `outer`.
pub(crate) fn compose(outer: &Transform, inner: &Transform) -> Transform {
    let (oa, ob, oc, od) = (
        outer.a as i64,
        outer.b as i64,
        outer.c as i64,
        outer.d as i64,
    );
    let (ia, ib, ic, id) = (
        inner.a as i64,
        inner.b as i64,
        inner.c as i64,
        inner.d as i64,
    );
    let (itx, ity) = (inner.tx as i64, inner.ty as i64);
    let div = |v: i64| (v + 512).div_euclid(1024);
    Transform {
        a: div(oa * ia + oc * ib) as i32,
        b: div(ob * ia + od * ib) as i32,
        c: div(oa * ic + oc * id) as i32,
        d: div(ob * ic + od * id) as i32,
        tx: (div(oa * itx + oc * ity) + outer.tx as i64) as i32,
        ty: (div(ob * itx + od * ity) + outer.ty as i64) as i32,
    }
}

/// Paints the whole tree: the canvas, the root scroll area, then the root fragment
/// as the outermost stacking context.
pub(crate) fn paint_root(p: &mut Painter, root_state: &State) {
    let vp = SRect::new(0, 0, p.viewport.width, p.viewport.height);
    let scroll = p.ctx.scroll;
    let mut state = root_state.clone();
    state.origin = Point {
        x: -scroll.x,
        y: -scroll.y,
    };
    state.scrolled = scroll;
    // Canvas background: from the root element, else from the body.
    background::paint_canvas(p, &state);
    p.scrolls.push(ScrollArea {
        target: "pane:page".into(),
        window: None,
        bounds: vp,
        offset: px(scroll.y),
        extent: super::upx(p.tree.content_height).max(vp.height),
        title: None,
        title_height: 0,
        horizontal: false,
    });
    if p.tree.content_width > p.tree.viewport_width {
        p.scrolls.push(ScrollArea {
            target: "pane:page".into(),
            window: None,
            bounds: vp,
            offset: px(scroll.x),
            extent: super::upx(p.tree.content_width).max(vp.width),
            title: None,
            title_height: 0,
            horizontal: true,
        });
    }
    let tree: &'_ crate::layout::fragment::FragmentTree = p.tree;
    // The root element's stacking context is the document's: boxes whose containing
    // block is the viewport (fixed boxes, absolutes with no positioned ancestor) are
    // the root fragment's later children in the fragment tree, but they belong to
    // the root element's context and interleave with its layers in document order.
    match tree.root.children.split_first() {
        Some((html, extra))
            if matches!(tree.root.kind, FragmentKind::Box { source: StyleSource::Anonymous(n), .. } if n == crate::dom::Document::ROOT)
                && matches!(
                    html.kind,
                    FragmentKind::Box {
                        source: StyleSource::Element(_),
                        ..
                    }
                )
                && !extra.is_empty() =>
        {
            paint_own(p, &tree.root, &state);
            let inner = enter(p, &tree.root, &state, true);
            // Fixed boxes that belong to an inner stacking context are painted there.
            let mut own: Vec<&Fragment> = Vec::new();
            for f in extra {
                match f.stacking_parent {
                    Some(n) => p.adopted.entry(n).or_default().push(f),
                    None => own.push(f),
                }
            }
            p.root_inner = Some(inner.clone());
            paint_context_with(p, html, &inner, &own, &inner);
        }
        _ => paint_context(p, &tree.root, &state),
    }
}

/// Paints `f` and its subtree as one stacking context (or atomically, which is the
/// same order).
pub(crate) fn paint_context<'a>(p: &mut Painter<'a>, f: &'a Fragment, state: &State) {
    paint_context_with(p, f, state, &[], state);
}

/// `paint_context`, with `extra` fragments (positioned in `extra_state`'s space)
/// collected into the same context after `f`'s own children.
fn paint_context_with<'a>(
    p: &mut Painter<'a>,
    f: &'a Fragment,
    state: &State,
    extra: &[&'a Fragment],
    extra_state: &State,
) {
    // 1. Own background and borders, then the replaced content of an atomic inline,
    //    floated, positioned or stacking-context replaced box (an `<img>` in a line, a
    //    positioned picture), which no child bucket would otherwise paint.
    paint_own(p, f, state);
    if matches!(
        &f.kind,
        FragmentKind::Box {
            replaced: Some(_),
            ..
        }
    ) && p.style_of(f).visibility == Visibility::Visible
    {
        replaced::paint(p, f, &own_state(p, f, state));
    }
    let child_state = enter(p, f, state, true);
    let mut b = Buckets::default();
    for c in &f.children {
        collect(p, c, &child_state, false, &mut b);
    }
    for c in extra {
        collect(p, c, extra_state, false, &mut b);
    }
    // The fixed boxes this context adopted, positioned in the root's space but with
    // the context's opacity.
    let adopted = f
        .source()
        .filter(|s| !s.is_anonymous())
        .and_then(|s| p.adopted.remove(&s.node()));
    if let (Some(list), Some(root)) = (adopted, p.root_inner.clone()) {
        let mut s = root;
        s.opacity = child_state.opacity;
        for c in list {
            collect(p, c, &s, false, &mut b);
        }
    }
    if has_outline(p, f) {
        b.outlines.push(Item::new(f, state));
    }
    paint_buckets(p, b);
    paint_scrollbars(p, f, state);
}

/// Classic scrollbars in the gutter a scroll container reserved. A host that
/// overlays its bars reserves no gutter, so `shows_*_bar` is false there and nothing
/// is drawn; the viewport's own bars are the shell's, painted over the page by the
/// browser chrome, so the root is skipped. The bar is painted after the contents and
/// in the container's own coordinate space, so it neither scrolls nor is clipped by
/// the scrollport.
fn has_bars(f: &Fragment) -> bool {
    match &f.kind {
        FragmentKind::Box {
            source,
            scroll: Some(info),
            ..
        } => {
            !matches!(source, StyleSource::Anonymous(n) if *n == crate::dom::Document::ROOT)
                && (info.shows_x_bar || info.shows_y_bar)
        }
        _ => false,
    }
}

fn paint_scrollbars(p: &mut Painter, f: &Fragment, state: &State) {
    if p.hits_only {
        return;
    }
    let FragmentKind::Box {
        source,
        padding,
        border,
        scroll: Some(info),
        ..
    } = &f.kind
    else {
        return;
    };
    if matches!(source, StyleSource::Anonymous(n) if *n == crate::dom::Document::ROOT)
        || !(info.shows_x_bar || info.shows_y_bar)
    {
        return;
    }
    let (source, info) = (*source, *info);
    let Some(key) = p.key(f) else { return };
    let style = p.style_rc(source);
    if style.visibility != Visibility::Visible {
        return;
    }
    let view = snap(padding.inset(border.inset(box_rect(p, f, state))));
    let bar = super::upx(crate::layout::scroll::bar_thickness(&style));
    if bar == 0 {
        return;
    }
    let (vbar, hbar) = (info.shows_y_bar, info.shows_x_bar);
    let (corner_w, corner_h) = (if vbar { bar } else { 0 }, if hbar { bar } else { 0 });
    // The offset from the start of the scrollable area, which is what a thumb shows.
    let along = |track: SRect, horizontal: bool| -> (SRect, SRect) {
        let (len, content, off) = if horizontal {
            (
                track.width,
                super::upx(info.content_width),
                px(info.scroll_x - info.origin_x),
            )
        } else {
            (
                track.height,
                super::upx(info.content_height),
                px(info.scroll_y - info.origin_y),
            )
        };
        let span = content.max(len);
        let thumb = ((u64::from(len) * u64::from(len) / u64::from(span.max(1))) as u32)
            .clamp(bar.min(len), len);
        let room = span - len;
        let at = if room == 0 {
            0
        } else {
            ((len - thumb) as i64 * off.clamp(0, room as i32) as i64 / room as i64) as i32
        };
        let t = if horizontal {
            SRect::new(
                track.x + at,
                track.y + 2,
                thumb,
                track.height.saturating_sub(4),
            )
        } else {
            SRect::new(
                track.x + 2,
                track.y + at,
                track.width.saturating_sub(4),
                thumb,
            )
        };
        (track, t)
    };
    let emit = |p: &mut Painter, track: SRect, thumb: SRect| {
        if track.width == 0 || track.height == 0 {
            return;
        }
        let part = p.next_part(key);
        let id = p.id(key, part);
        p.emit(
            state,
            id,
            track,
            cw_scene::Primitive::Box {
                fill: super::SCROLLBAR_TRACK,
                border: None,
                border_width: 0,
            },
        );
        let part = p.next_part(key);
        let id = p.id(key, part);
        p.emit(
            state,
            id,
            thumb,
            cw_scene::Primitive::RoundedBox {
                fill: super::SCROLLBAR_THUMB,
                border: None,
                border_width: 0,
                radius: thumb.width.min(thumb.height) / 2,
            },
        );
    };
    if vbar {
        let track = SRect::new(
            view.right() - bar as i32,
            view.y,
            bar,
            view.height.saturating_sub(corner_h),
        );
        let (track, thumb) = along(track, false);
        emit(p, track, thumb);
    }
    if hbar {
        let track = SRect::new(
            view.x,
            view.bottom() - bar as i32,
            view.width.saturating_sub(corner_w),
            bar,
        );
        let (track, thumb) = along(track, true);
        emit(p, track, thumb);
    }
}

/// Paints an item: a real stacking context, or a box painted atomically whose
/// layers were collected ahead (`Item::inner`).
fn paint_item<'a>(p: &mut Painter<'a>, it: Item<'a>) {
    match it.inner {
        None => paint_context(p, it.frag, &it.state),
        Some(inner) => {
            paint_own(p, it.frag, &it.state);
            if matches!(
                &it.frag.kind,
                FragmentKind::Box {
                    replaced: Some(_),
                    ..
                }
            ) && p.style_of(it.frag).visibility == Visibility::Visible
            {
                replaced::paint(p, it.frag, &own_state(p, it.frag, &it.state));
            }
            paint_buckets(p, *inner);
            paint_scrollbars(p, it.frag, &it.state);
        }
    }
}

/// Steps 2 to 8 over collected layers.
fn paint_buckets<'a>(p: &mut Painter<'a>, mut b: Buckets<'a>) {
    // 2. Negative z-index contexts.
    sort_by_tree_order(p, &mut b.negative, |(_, _, it)| it.frag);
    b.negative.sort_by_key(|(z, _, _)| *z);
    for (_, _, it) in b.negative {
        paint_item(p, it);
    }
    // 3. Block backgrounds and borders.
    for it in &b.blocks {
        paint_own(p, it.frag, &it.state);
    }
    // 4. Floats.
    for it in b.floats {
        paint_item(p, it);
    }
    // 5. Inline content.
    for it in b.inline {
        match it {
            Inline::Box(it) => paint_own(p, it.frag, &it.state),
            Inline::Text(it) => {
                if p.style_of(it.frag).visibility == Visibility::Visible {
                    text::paint_run(p, it.frag, &it.state);
                }
            }
            Inline::Content(it) => {
                if p.style_of(it.frag).visibility == Visibility::Visible {
                    replaced::paint(p, it.frag, &own_state(p, it.frag, &it.state));
                }
            }
            Inline::Atomic(it) => paint_item(p, it),
        }
    }
    // 6. Layer zero: positioned z-index auto/0 in tree order. Tree order is the
    //    document's, not the fragment tree's: an absolutely or fixed positioned box
    //    is a child of its containing block's fragment (a fixed box of the root),
    //    which can put it after boxes that follow it in the document. The sort is
    //    stable, so fragments without a document position keep their place.
    sort_by_tree_order(p, &mut b.layer_zero, |(_, it)| it.frag);
    for (_, it) in b.layer_zero {
        paint_item(p, it);
    }
    // 7. Positive z-index contexts.
    sort_by_tree_order(p, &mut b.positive, |(_, _, it)| it.frag);
    b.positive.sort_by_key(|(z, _, _)| *z);
    for (_, _, it) in b.positive {
        paint_item(p, it);
    }
    // 8. Outlines.
    for it in &b.outlines {
        paint_outline(p, it.frag, &own_state(p, it.frag, &it.state));
    }
    // 9. The scrollbars of the scroll containers collected here.
    for it in &b.bars {
        paint_scrollbars(p, it.frag, &it.state);
    }
}

/// Stable-sorts a stacking layer into document order. Only fragments the document
/// places are compared; if any item has no position (a hand-built fragment tree, an
/// anonymous box) the layer keeps the fragment-tree order it was collected in.
fn sort_by_tree_order<'a, T>(
    p: &Painter<'a>,
    items: &mut Vec<T>,
    frag: impl Fn(&T) -> &'a Fragment,
) {
    if items.len() < 2 {
        return;
    }
    let keys: Option<Vec<(u32, u8)>> = items.iter().map(|it| p.tree_order(frag(it))).collect();
    let Some(keys) = keys else { return };
    let mut keyed: Vec<((u32, u8), T)> = keys.into_iter().zip(items.drain(..)).collect();
    keyed.sort_by_key(|(k, _)| *k);
    items.extend(keyed.into_iter().map(|(_, it)| it));
}

fn has_outline(p: &Painter, f: &Fragment) -> bool {
    match &f.kind {
        FragmentKind::Box { source, .. } | FragmentKind::InlineBox { source, .. } => {
            !source.is_anonymous() && {
                let s = p.style(*source);
                s.outline.style.is_visible()
                    && s.outline.width > Au::ZERO
                    && s.visibility == Visibility::Visible
            }
        }
        _ => false,
    }
}

fn paint_outline(p: &mut Painter, f: &Fragment, state: &State) {
    if p.hits_only {
        return;
    }
    let Some(key) = p.key(f) else { return };
    let rect = snap(box_rect(p, f, state));
    let style = p.style_of(f).clone();
    border::paint_outline(p, key, state, &style, rect);
}

/// Sorts the subtree under `f` (the fragment itself included) into the buckets of the
/// current stacking context. `inline` says whether we are inside a line box.
fn collect<'a>(
    p: &mut Painter<'a>,
    f: &'a Fragment,
    state: &State,
    inline: bool,
    b: &mut Buckets<'a>,
) {
    // Laid out but not rendered (the lines after a `-webkit-line-clamp`).
    if f.hidden_for_paint {
        return;
    }
    let order = b.order;
    b.order += 1;
    match &f.kind {
        FragmentKind::Line => {
            let mut s = state.clone();
            s.origin = abs_rect(state, f).origin;
            for c in &f.children {
                collect(p, c, &s, true, b);
            }
        }
        FragmentKind::Text { .. } => {
            b.inline.push(Inline::Text(Item::new(f, state)));
        }
        FragmentKind::InlineBox { .. } => {
            b.inline.push(Inline::Box(Item::new(f, state)));
            if has_outline(p, f) {
                b.outlines.push(Item::new(f, state));
            }
            let s = enter(p, f, state, true);
            for c in &f.children {
                collect(p, c, &s, true, b);
            }
        }
        FragmentKind::Box { replaced, .. } => {
            let style = p.style_of(f);
            if establishes_context(f, style) {
                let item = Item::new(f, state);
                match f.z_index {
                    z if z < 0 => b.negative.push((z, order, item)),
                    0 => b.layer_zero.push((true, item)),
                    z => b.positive.push((z, order, item)),
                }
                return;
            }
            let positioned = f.is_positioned || style.is_positioned();
            if positioned || f.is_float || inline {
                let is_float = f.is_float;
                let (item, mut hoisted) = atomic_item(p, f, state, b.order);
                b.order = hoisted.order;
                if positioned {
                    b.layer_zero.push((false, item));
                } else if is_float {
                    b.floats.push(item);
                } else {
                    b.inline.push(Inline::Atomic(item));
                }
                // The box's own place comes before its descendants'.
                b.negative.append(&mut hoisted.negative);
                b.layer_zero.append(&mut hoisted.layer_zero);
                b.positive.append(&mut hoisted.positive);
                return;
            }
            b.blocks.push(Item::new(f, state));
            if has_outline(p, f) {
                b.outlines.push(Item::new(f, state));
            }
            if has_bars(f) {
                b.bars.push(Item::new(f, state));
            }
            if replaced.is_some() {
                b.inline.push(Inline::Content(Item::new(f, state)));
            }
            let s = enter(p, f, state, true);
            for c in &f.children {
                collect(p, c, &s, false, b);
            }
        }
    }
}

/// A box painted atomically that is not a stacking context: collects its layers now
/// and splits them into what it paints itself when its turn comes (the item) and the
/// positioned and stacking-context descendants that belong to the enclosing context
/// (the second value, which also carries the advanced `order` counter).
fn atomic_item<'a>(
    p: &mut Painter<'a>,
    f: &'a Fragment,
    state: &State,
    order: usize,
) -> (Item<'a>, Buckets<'a>) {
    let child_state = enter(p, f, state, true);
    let mut inner = Buckets {
        order,
        ..Buckets::default()
    };
    for c in &f.children {
        collect(p, c, &child_state, false, &mut inner);
    }
    if has_outline(p, f) {
        inner.outlines.push(Item::new(f, state));
    }
    let hoisted = Buckets {
        negative: std::mem::take(&mut inner.negative),
        layer_zero: std::mem::take(&mut inner.layer_zero),
        positive: std::mem::take(&mut inner.positive),
        order: inner.order,
        ..Buckets::default()
    };
    (
        Item {
            frag: f,
            state: state.clone(),
            inner: Some(Box::new(inner)),
        },
        hoisted,
    )
}

/// Paints a box's or inline box's own decoration: the interaction region, outer
/// shadows, background, inset shadows, borders. Records the hit-test item.
pub(crate) fn paint_own(p: &mut Painter, f: &Fragment, state: &State) {
    let (source, padding, border, first, last) = match &f.kind {
        FragmentKind::Box {
            source,
            padding,
            border,
            ..
        } => (*source, *padding, *border, true, true),
        FragmentKind::InlineBox {
            source,
            padding,
            border,
            first,
            last,
        } => (*source, *padding, *border, *first, *last),
        _ => return,
    };
    let Some(key) = p.key(f) else { return };
    let rect = box_rect(p, f, state);
    let srect = snap(rect);
    let style = p.style_rc(source);
    let transformed_state;
    let state = if style.transform.is_empty() {
        state
    } else {
        transformed_state = own_state(p, f, state);
        &transformed_state
    };
    let hidden = style.visibility != Visibility::Visible;
    if !source.is_anonymous() {
        if !hidden {
            semantics::paint_region(p, key, state, source, srect);
        }
        let radii = border::radii_px(&style, srect);
        let radius = border::uniform_radius(&radii).unwrap_or(0);
        p.record_hit(
            state,
            source.node(),
            srect,
            radius,
            hidden
                || matches!(
                    style.pointer_events,
                    crate::style::computed::PointerEvents::None
                ),
        );
    }
    if hidden || source.is_anonymous() || p.hits_only {
        return;
    }
    // The element's own decoration belongs to its opacity group.
    let own_state;
    let state = if style.opacity < 255 {
        own_state = State {
            opacity: super::mul_opacity(state.opacity, style.opacity),
            ..state.clone()
        };
        &own_state
    } else {
        state
    };
    if p.canvas_source == Some(source.node()) && matches!(source, StyleSource::Element(_)) {
        // The canvas took this element's background; borders still paint.
        border::paint_borders(p, key, state, &style, srect, border, first, last);
        return;
    }
    let content_box = padding.inset(border.inset(rect));
    if style.backdrop_blur > Au::ZERO && srect.width > 0 && srect.height > 0 {
        // `backdrop-filter: blur(σ)` blurs what is already painted under the border
        // box; the renderer runs three box blurs of radius r, whose variance
        // r(r + 1) matches the Gaussian's σ² at r = round(√(σ² + ¼) - ½).
        let sigma = style.backdrop_blur.0 as f64 / 64.0;
        let r = ((sigma * sigma + 0.25).sqrt() - 0.5).round().max(1.0) as u32;
        let radius = border::uniform_radius(&border::radii_px(&style, srect)).unwrap_or(0);
        let id = p.id(key, super::parts::BACKDROP);
        p.emit(
            state,
            id,
            srect,
            cw_scene::Primitive::Backdrop { radius, blur: r },
        );
    }
    border::paint_box_shadows(p, key, state, &style, srect, false);
    background::paint_background(
        p,
        key,
        state,
        &style,
        rect,
        border.inset(rect),
        content_box,
        first,
        last,
    );
    border::paint_box_shadows(p, key, state, &style, srect, true);
    border::paint_borders(p, key, state, &style, srect, border, first, last);
}
