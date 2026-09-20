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
//! `z-index`); the fragment's `z_index` orders it. Positioned boxes with
//! `z-index: auto` are painted atomically here rather than letting their positioned
//! descendants join the parent context: a simplification that only shows when a
//! `z-index: auto` box holds a negative-`z-index` child.
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
    order: usize,
}

pub(crate) fn establishes_context(f: &Fragment, style: &ComputedStyle) -> bool {
    if f.establishes_stacking_context {
        return true;
    }
    if matches!(f.kind, FragmentKind::Box { .. }) {
        return style.establishes_stacking_context(false) && !f.source().is_some_and(StyleSource::is_anonymous);
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
    if !style.transform.is_empty() {
        let srect = snap(rect);
        let ox = srect.x + px(style.transform_origin.0.resolve(rect.size.width));
        let oy = srect.y + px(style.transform_origin.1.resolve(rect.size.height));
        let local = transform_matrix(&style.transform, rect.size, (ox, oy));
        s.transform = compose(&s.transform, &local);
        s.transformed = true;
    }
    if let FragmentKind::Box { padding, border, scroll, source, .. } = &f.kind {
        let clips = !source.is_anonymous()
            && (!matches!(style.overflow_x, Overflow::Visible) || !matches!(style.overflow_y, Overflow::Visible));
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
                    s.rounded_clip = Some(RoundedClip { rect: snap(padding_box), radius: r });
                }
            }
        }
        if let Some(info) = scroll {
            let node = source.node();
            let off = p.scroll_of(node, info);
            s.origin.x -= off.x;
            s.origin.y -= off.y;
            s.scrolled.x += off.x;
            s.scrolled.y += off.y;
            if register_scroll {
                let padding_box = border.inset(rect);
                let content_box = padding.inset(padding_box);
                let target = format!("pane:{}", p.doc.map(|d| semantics::interaction_id(d, node)).unwrap_or_else(|| format!("n{}", node.0)));
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
pub(crate) fn transform_matrix(ops: &[TransformOp], size: crate::geom::Size, origin: (i32, i32)) -> Transform {
    let mut m = Transform::default();
    for op in ops {
        let t = match *op {
            TransformOp::Translate(x, y) => Transform::translate(px(x.resolve(size.width)), px(y.resolve(size.height))),
            TransformOp::Scale(sx, sy) => Transform { a: scale_1024(sx), b: 0, c: 0, d: scale_1024(sy), tx: 0, ty: 0 },
            TransformOp::Rotate(deg) => {
                let (s, c) = (super::trig::sin_1024(deg), super::trig::cos_1024(deg));
                Transform { a: c, b: s, c: -s, d: c, tx: 0, ty: 0 }
            }
            TransformOp::SkewX(deg) => Transform { a: 1024, b: 0, c: super::trig::tan_1024(deg), d: 1024, tx: 0, ty: 0 },
            TransformOp::SkewY(deg) => Transform { a: 1024, b: super::trig::tan_1024(deg), c: 0, d: 1024, tx: 0, ty: 0 },
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
    let (oa, ob, oc, od) = (outer.a as i64, outer.b as i64, outer.c as i64, outer.d as i64);
    let (ia, ib, ic, id) = (inner.a as i64, inner.b as i64, inner.c as i64, inner.d as i64);
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
    state.origin = Point { x: -scroll.x, y: -scroll.y };
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
    paint_context(p, &tree.root, &state);
}

/// Paints `f` and its subtree as one stacking context (or atomically, which is the
/// same order).
pub(crate) fn paint_context<'a>(p: &mut Painter<'a>, f: &'a Fragment, state: &State) {
    // 1. Own background and borders, then the replaced content of an atomic inline,
    //    floated, positioned or stacking-context replaced box (an `<img>` in a line, a
    //    positioned picture), which no child bucket would otherwise paint.
    paint_own(p, f, state);
    if matches!(&f.kind, FragmentKind::Box { replaced: Some(_), .. }) && p.style_of(f).visibility == Visibility::Visible {
        replaced::paint(p, f, state);
    }
    let child_state = enter(p, f, state, true);
    let mut b = Buckets::default();
    for c in &f.children {
        collect(p, c, &child_state, false, &mut b);
    }
    if has_outline(p, f) {
        b.outlines.push(Item { frag: f, state: state.clone() });
    }
    // 2. Negative z-index contexts.
    b.negative.sort_by_key(|(z, order, _)| (*z, *order));
    for (_, _, it) in &b.negative {
        paint_context(p, it.frag, &it.state);
    }
    // 3. Block backgrounds and borders.
    for it in &b.blocks {
        paint_own(p, it.frag, &it.state);
    }
    // 4. Floats.
    for it in &b.floats {
        paint_context(p, it.frag, &it.state);
    }
    // 5. Inline content.
    for it in &b.inline {
        match it {
            Inline::Box(it) => paint_own(p, it.frag, &it.state),
            Inline::Text(it) => {
                if p.style_of(it.frag).visibility == Visibility::Visible {
                    text::paint_run(p, it.frag, &it.state);
                }
            }
            Inline::Content(it) => {
                if p.style_of(it.frag).visibility == Visibility::Visible {
                    replaced::paint(p, it.frag, &it.state);
                }
            }
            Inline::Atomic(it) => paint_context(p, it.frag, &it.state),
        }
    }
    // 6. Layer zero: positioned z-index auto/0 in tree order.
    for (_, it) in &b.layer_zero {
        paint_context(p, it.frag, &it.state);
    }
    // 7. Positive z-index contexts.
    b.positive.sort_by_key(|(z, order, _)| (*z, *order));
    for (_, _, it) in &b.positive {
        paint_context(p, it.frag, &it.state);
    }
    // 8. Outlines.
    for it in &b.outlines {
        paint_outline(p, it.frag, &it.state);
    }
}

fn has_outline(p: &Painter, f: &Fragment) -> bool {
    match &f.kind {
        FragmentKind::Box { source, .. } | FragmentKind::InlineBox { source, .. } => {
            !source.is_anonymous() && {
                let s = p.style(*source);
                s.outline.style.is_visible() && s.outline.width > Au::ZERO && s.visibility == Visibility::Visible
            }
        }
        _ => false,
    }
}

fn paint_outline(p: &mut Painter, f: &Fragment, state: &State) {
    let Some(key) = p.key(f) else { return };
    let rect = snap(box_rect(p, f, state));
    let style = p.style_of(f).clone();
    border::paint_outline(p, key, state, &style, rect);
}

/// Sorts the subtree under `f` (the fragment itself included) into the buckets of the
/// current stacking context. `inline` says whether we are inside a line box.
fn collect<'a>(p: &mut Painter<'a>, f: &'a Fragment, state: &State, inline: bool, b: &mut Buckets<'a>) {
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
            b.inline.push(Inline::Text(Item { frag: f, state: state.clone() }));
        }
        FragmentKind::InlineBox { .. } => {
            b.inline.push(Inline::Box(Item { frag: f, state: state.clone() }));
            if has_outline(p, f) {
                b.outlines.push(Item { frag: f, state: state.clone() });
            }
            let s = enter(p, f, state, true);
            for c in &f.children {
                collect(p, c, &s, true, b);
            }
        }
        FragmentKind::Box { replaced, .. } => {
            let style = p.style_of(f);
            let item = Item { frag: f, state: state.clone() };
            if establishes_context(f, style) {
                match f.z_index {
                    z if z < 0 => b.negative.push((z, order, item)),
                    0 => b.layer_zero.push((true, item)),
                    z => b.positive.push((z, order, item)),
                }
                return;
            }
            if f.is_positioned || style.is_positioned() {
                b.layer_zero.push((false, item));
                return;
            }
            if f.is_float {
                b.floats.push(item);
                return;
            }
            if inline {
                b.inline.push(Inline::Atomic(item));
                return;
            }
            b.blocks.push(item);
            if has_outline(p, f) {
                b.outlines.push(Item { frag: f, state: state.clone() });
            }
            if replaced.is_some() {
                b.inline.push(Inline::Content(Item { frag: f, state: state.clone() }));
            }
            let s = enter(p, f, state, true);
            for c in &f.children {
                collect(p, c, &s, false, b);
            }
        }
    }
}

/// Paints a box's or inline box's own decoration: the interaction region, outer
/// shadows, background, inset shadows, borders. Records the hit-test item.
pub(crate) fn paint_own(p: &mut Painter, f: &Fragment, state: &State) {
    let (source, padding, border, first, last) = match &f.kind {
        FragmentKind::Box { source, padding, border, .. } => (*source, *padding, *border, true, true),
        FragmentKind::InlineBox { source, padding, border, first, last } => (*source, *padding, *border, *first, *last),
        _ => return,
    };
    let Some(key) = p.key(f) else { return };
    let rect = box_rect(p, f, state);
    let srect = snap(rect);
    let style = p.style(source).clone();
    let hidden = style.visibility != Visibility::Visible;
    if !source.is_anonymous() {
        if !hidden {
            semantics::paint_region(p, key, state, source, srect);
        }
        let radii = border::radii_px(&style, srect);
        let radius = border::uniform_radius(&radii).unwrap_or(0);
        p.record_hit(state, source.node(), srect, radius, hidden || matches!(style.pointer_events, crate::style::computed::PointerEvents::None));
    }
    if hidden || source.is_anonymous() {
        return;
    }
    // The element's own decoration belongs to its opacity group.
    let own_state;
    let state = if style.opacity < 255 {
        own_state = State { opacity: super::mul_opacity(state.opacity, style.opacity), ..state.clone() };
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
    border::paint_box_shadows(p, key, state, &style, srect, false);
    background::paint_background(p, key, state, &style, rect, border.inset(rect), content_box, first, last);
    border::paint_box_shadows(p, key, state, &style, srect, true);
    border::paint_borders(p, key, state, &style, srect, border, first, last);
}
