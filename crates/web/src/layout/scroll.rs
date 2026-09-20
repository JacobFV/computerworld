//! Scroll containers and the root: `overflow` clipping, content sizes, scrollbar
//! reservation (`scrollbar-width` px — 15 by default, half that for `thin`, none at
//! all for `none` — for `scroll`, and for `auto` when the content overflows; the
//! decision is re-made until it settles, since reserving one bar can take the other
//! away), scroll offsets from the caller's `ScrollState`, the viewport as the root
//! scroll container with `overflow` propagated from `<body>`, and the
//! `position: sticky` post-pass.
//!
//! The scrollable area runs from `ScrollInfo::origin_*` to the far edge of the
//! content. The origin is zero for a box whose content only overflows the end edge;
//! a reversed flex container packs from the end, so its earlier items sit above (or
//! left of) the padding box and the origin is negative. The caller's offset is
//! measured from the start of that area, as `scrollTop` is, and `scroll_*` is what
//! the painter subtracts — zero being where layout put the content, which for a
//! reversed column is the end, the position such a list opens at.

use crate::dom::Document;
use crate::geom::{Au, Point, Rect, Size};
use crate::layout::block::{self, Bfc, Cb};
use crate::layout::boxes::BoxId;
use crate::layout::fragment::{Fragment, FragmentKind, ScrollInfo, StyleSource};
use crate::layout::{FragmentTree, LayoutContext};
use crate::style::computed::ScrollbarWidth;
use crate::style::{ComputedStyle, LengthPercentageAuto, Overflow, Position};

/// Scrollbar thickness in CSS px.
pub const BAR: Au = Au(15 * 64);

/// `reserved_bars` on this host: nothing when its scrollbars are overlaid
/// (`LayoutCache::overlay_scrollbars`).
pub fn reserved_bars_in(ctx: &LayoutContext, s: &ComputedStyle) -> (Au, Au) {
    if ctx.cache.borrow().overlay_scrollbars {
        (Au::ZERO, Au::ZERO)
    } else {
        reserved_bars(s)
    }
}

/// `auto_bars` on this host: overlaid scrollbars never take space.
pub fn auto_bars_in(ctx: &LayoutContext, s: &ComputedStyle, content: Size, visible: Size, bar_x: Au, bar_y: Au) -> (Au, Au) {
    if ctx.cache.borrow().overlay_scrollbars {
        (Au::ZERO, Au::ZERO)
    } else {
        auto_bars(s, content, visible, bar_x, bar_y)
    }
}

/// The thickness of this box's bars: `scrollbar-width: none` gives it none at all,
/// `thin` half of one (css-scrollbars-1 leaves the value to the UA).
pub fn bar_thickness(s: &ComputedStyle) -> Au {
    match s.scrollbar_width {
        ScrollbarWidth::None => Au::ZERO,
        ScrollbarWidth::Thin => Au(BAR.0 / 2),
        ScrollbarWidth::Auto => BAR,
    }
}

/// Space reserved before layout: `(horizontal bar height, vertical bar width)`.
pub fn reserved_bars(s: &ComputedStyle) -> (Au, Au) {
    let bar = bar_thickness(s);
    let h = if s.overflow_x == Overflow::Scroll { bar } else { Au::ZERO };
    let v = if s.overflow_y == Overflow::Scroll { bar } else { Au::ZERO };
    (h, v)
}

/// Bars needed after a first layout: `auto` shows a bar when the content overflows.
pub fn auto_bars(s: &ComputedStyle, content: Size, visible: Size, bar_x: Au, bar_y: Au) -> (Au, Au) {
    let bar = bar_thickness(s);
    if bar.is_zero() {
        return (Au::ZERO, Au::ZERO);
    }
    let mut h = bar_x;
    let mut v = bar_y;
    if s.overflow_y == Overflow::Auto && content.height > visible.height {
        v = bar;
    }
    if s.overflow_x == Overflow::Auto && content.width > visible.width - v {
        h = bar;
    }
    if s.overflow_y == Overflow::Auto && v.is_zero() && content.height > visible.height - h {
        v = bar;
    }
    (h, v)
}

/// The scrollable size of content fragments positioned in content-box coordinates.
/// Content that runs off the start edge counts too: a `column-reverse` flex
/// container fills from the bottom, so its earlier items sit above the content box
/// and the area to scroll is taller than the last item's bottom edge.
pub fn content_size(fragments: &[Fragment], w: Au, h: Au) -> Size {
    let (mut right, mut bottom) = (w, h);
    let (mut left, mut top) = (Au::ZERO, Au::ZERO);
    for f in fragments {
        let o = f.overflow.translate(f.rect.origin.x, f.rect.origin.y);
        right = right.max(o.right());
        bottom = bottom.max(o.bottom());
        left = left.min(o.origin.x);
        top = top.min(o.origin.y);
    }
    Size { width: right - left, height: bottom - top }
}

/// Fills `ScrollInfo` on a scroll container's fragment.
pub fn attach_scroll_info(ctx: &LayoutContext, id: BoxId, frag: &mut Fragment, content_w: Au, h: Au, reserve_h: Au, reserve_v: Au) {
    let b = &ctx.tree[id];
    if !b.is_scroll_container() {
        return;
    }
    let (padding, border) = match &frag.kind {
        FragmentKind::Box { padding, border, .. } => (*padding, *border),
        _ => return,
    };
    // Children are in border-box coordinates; the scrollable area is the padding box.
    let cx = border.left;
    let cy = border.top;
    let pad_w = content_w + padding.horizontal() + reserve_v;
    let pad_h = h + padding.vertical();
    let mut right = pad_w;
    let mut bottom = pad_h;
    // How far the content runs off the start edges, as a non-positive offset. A
    // reversed flex container packs from the end, so its earlier items sit above (or
    // left of) the padding box; that part of the scrollable overflow region is as
    // reachable as the part past the end edge.
    let (mut origin_x, mut origin_y) = (Au::ZERO, Au::ZERO);
    for c in &frag.children {
        if c.is_positioned && c.establishes_stacking_context && matches!(ctx.style_of_source(c.source()), Some(s) if s.position == Position::Fixed) {
            continue;
        }
        let o = c.overflow.translate(c.rect.origin.x - cx, c.rect.origin.y - cy);
        right = right.max(o.right() + padding.right);
        bottom = bottom.max(o.bottom() + padding.bottom);
        origin_x = origin_x.min(o.origin.x - padding.left);
        origin_y = origin_y.min(o.origin.y - padding.top);
    }
    let (content_w, content_h) = (right - origin_x, bottom - origin_y);
    let visible_w = (pad_w - reserve_v).max(Au::ZERO);
    let visible_h = (pad_h - reserve_h).max(Au::ZERO);
    // The caller's offset is measured from the start of the scrollable area, the way
    // `scrollTop` is, so it is shifted by `origin` to give the offset the painter
    // subtracts; with none recorded the box stays where layout put it, which for a
    // reversed column is at the end — what Chromium shows when such a list opens.
    let max_x = (content_w - visible_w).max(Au::ZERO);
    let max_y = (content_h - visible_h).max(Au::ZERO);
    let (sx, sy) = match b.node.and_then(|n| ctx.scroll.get(&n).copied()) {
        Some((x, y)) => (x.clamp(Au::ZERO, max_x) + origin_x, y.clamp(Au::ZERO, max_y) + origin_y),
        None => (Au::ZERO, Au::ZERO),
    };
    if let FragmentKind::Box { scroll, .. } = &mut frag.kind {
        *scroll = Some(ScrollInfo { content_width: content_w, content_height: content_h, scroll_x: sx, scroll_y: sy, origin_x, origin_y, shows_x_bar: !reserve_h.is_zero(), shows_y_bar: !reserve_v.is_zero() });
    }
}

impl LayoutContext<'_> {
    /// The style a fragment paints with, when it came from an element.
    pub fn style_of_source(&self, src: Option<StyleSource>) -> Option<&ComputedStyle> {
        match src? {
            StyleSource::Element(n) => self.styles.get(n),
            StyleSource::Before(n) => self.styles.before(n),
            StyleSource::After(n) => self.styles.after(n),
            StyleSource::Marker(n) => self.styles.marker(n),
            StyleSource::Anonymous(_) => None,
        }
    }
}

/// The viewport takes its `overflow` from `<html>`, or from `<body>` when the root's
/// is `visible`; the element it came from then behaves as `visible` (css-overflow-3
/// §3.3). Returns the viewport's `(overflow-x, overflow-y)`.
pub fn propagate_root_overflow(doc: &Document, tree: &mut crate::layout::boxes::BoxTree) -> (Overflow, Overflow) {
    let Some(html) = tree.root else { return (Overflow::Auto, Overflow::Auto) };
    let mut from = html;
    let visible = |b: BoxId, t: &crate::layout::boxes::BoxTree| t[b].style.overflow_x == Overflow::Visible && t[b].style.overflow_y == Overflow::Visible;
    if visible(html, tree) {
        match doc.body().and_then(|b| tree.box_of(b)) {
            Some(body) if !visible(body, tree) => from = body,
            _ => return (Overflow::Auto, Overflow::Auto),
        }
    }
    let (ox, oy) = (tree[from].style.overflow_x, tree[from].style.overflow_y);
    let mut st = (*tree[from].style).clone();
    st.overflow_x = Overflow::Visible;
    st.overflow_y = Overflow::Visible;
    tree.boxes[from.index()].style = std::rc::Rc::new(st);
    let fix = |o: Overflow| if o == Overflow::Visible { Overflow::Auto } else { o };
    (fix(ox), fix(oy))
}

/// Lays out the document in the initial containing block. The viewport's scrollbars
/// are overlay bars, as Chromium's are on the platforms the parity dumps come from:
/// they take no space from the layout viewport, so `<html>` is always the viewport
/// wide; `ScrollInfo::shows_y_bar` still says whether one should be drawn.
pub fn layout_root(ctx: &LayoutContext) -> FragmentTree {
    let vw = ctx.viewport.width;
    let vh = ctx.viewport.height;
    let (ox, oy) = ctx.root_overflow;
    let mut root = Fragment::new(
        FragmentKind::Box { source: StyleSource::Anonymous(Document::ROOT), padding: crate::geom::Edges::ZERO, border: crate::geom::Edges::ZERO, replaced: None, scroll: None, baseline: None },
        Rect::new(Au::ZERO, Au::ZERO, vw, vh),
    );
    root.establishes_stacking_context = true;
    let mut content = Size { width: vw, height: vh };
    if let Some(html) = ctx.tree.root {
        let cb = Cb { width: vw, height: Some(vh) };
        let mut bfc = Bfc::new();
        let s = ctx.style(html);
        let (mt, mb) = block::vertical_margins(s, cb.width);
        let mut r = block::layout_block_level(ctx, html, &cb, &mut bfc, Point::default(), mt);
        let off = block::relative_offset(s, &cb);
        r.fragment.rect.origin.x += off.x;
        r.fragment.rect.origin.y += off.y;
        block::translate_requests(&mut r.abs, r.fragment.rect.origin.x, r.fragment.rect.origin.y);
        let o = r.fragment.overflow.translate(r.fragment.rect.origin.x, r.fragment.rect.origin.y);
        let doc_h = (r.fragment.rect.bottom() + mb).max(o.bottom()).max(Au::ZERO);
        let doc_w = o.right().max(Au::ZERO);
        root.children.push(r.fragment);
        let rest = block::resolve_absolutes(ctx, &mut root, r.abs);
        debug_assert!(rest.is_empty());
        content = Size { width: doc_w.max(vw), height: doc_h.max(vh) };
        for c in &root.children[1..] {
            let o = c.overflow.translate(c.rect.origin.x, c.rect.origin.y);
            content.width = content.width.max(o.right());
            content.height = content.height.max(o.bottom());
        }
    }
    // `overflow: hidden` on the viewport stops the user scrolling it, not the
    // program: a fragment navigation (`#top`) or `scrollTo` still moves it, so the
    // caller's offset applies, clamped to the scrollable range; only `clip` pins it.
    let scrollable = !matches!(oy, Overflow::Clip);
    let (sx, sy) = ctx.scroll.get(&Document::ROOT).copied().unwrap_or((Au::ZERO, Au::ZERO));
    let sx = if matches!(ox, Overflow::Clip) { Au::ZERO } else { sx.clamp(Au::ZERO, (content.width - vw).max(Au::ZERO)) };
    let sy = if scrollable { sy.clamp(Au::ZERO, (content.height - vh).max(Au::ZERO)) } else { Au::ZERO };
    // Overlay bars are drawn over the content when the axis can scroll.
    let shows_x_bar = ox == Overflow::Scroll || ox == Overflow::Auto && content.width > vw;
    let shows_y_bar = oy == Overflow::Scroll || oy == Overflow::Auto && content.height > vh;
    if let FragmentKind::Box { scroll, .. } = &mut root.kind {
        *scroll = Some(ScrollInfo { content_width: content.width, content_height: content.height, scroll_x: sx, scroll_y: sy, origin_x: Au::ZERO, origin_y: Au::ZERO, shows_x_bar, shows_y_bar });
    }
    root.overflow = Rect::new(Au::ZERO, Au::ZERO, content.width, content.height);
    apply_sticky(ctx, &mut root);
    FragmentTree { root, content_width: content.width, content_height: content.height, viewport_width: vw, viewport_height: vh }
}

/// `position: sticky` (css-position-3 §3.4): each sticky box is shifted so it stays
/// inside its scroll container's scrollport (inset by its `top`/`left`/… values),
/// limited to its containing block's content box. Runs over the finished tree with
/// the scroll offsets already known.
pub fn apply_sticky(ctx: &LayoutContext, root: &mut Fragment) {
    let (sx, sy) = match &root.kind {
        FragmentKind::Box { scroll: Some(s), .. } => (s.scroll_x, s.scroll_y),
        _ => (Au::ZERO, Au::ZERO),
    };
    let port = Rect::new(sx, sy, root.rect.size.width, root.rect.size.height);
    let cb = port;
    let origin = root.rect.origin;
    let (_, content_box) = boxes_of(root, origin);
    walk_sticky(ctx, root, origin, port, cb.union(content_box));
}

fn boxes_of(f: &Fragment, abs_origin: Point) -> (Rect, Rect) {
    let (p, b) = match &f.kind {
        FragmentKind::Box { padding, border, .. } => (*padding, *border),
        _ => (crate::geom::Edges::ZERO, crate::geom::Edges::ZERO),
    };
    let bb = Rect::new(abs_origin.x, abs_origin.y, f.rect.size.width, f.rect.size.height);
    let pad = b.inset(bb);
    let content = p.inset(pad);
    (pad, content)
}

fn walk_sticky(ctx: &LayoutContext, f: &mut Fragment, abs_origin: Point, port: Rect, cb: Rect) {
    let (pad, content) = boxes_of(f, abs_origin);
    // A scroll container starts a new scrollport for its descendants.
    let inner_port = match &f.kind {
        FragmentKind::Box { scroll: Some(s), .. } => Rect::new(pad.origin.x + s.scroll_x, pad.origin.y + s.scroll_y, pad.size.width, pad.size.height),
        _ => port,
    };
    let is_block = matches!(f.kind, FragmentKind::Box { .. });
    let inner_cb = if is_block { content } else { cb };
    for c in &mut f.children {
        let c_origin = Point { x: abs_origin.x + c.rect.origin.x, y: abs_origin.y + c.rect.origin.y };
        if let Some(s) = ctx.style_of_source(c.source()) {
            if s.position == Position::Sticky && matches!(c.kind, FragmentKind::Box { .. }) {
                let shift = sticky_shift(s, Rect::new(c_origin.x, c_origin.y, c.rect.size.width, c.rect.size.height), inner_port, inner_cb);
                c.rect.origin.x += shift.x;
                c.rect.origin.y += shift.y;
            }
        }
        let c_origin = Point { x: abs_origin.x + c.rect.origin.x, y: abs_origin.y + c.rect.origin.y };
        walk_sticky(ctx, c, c_origin, inner_port, inner_cb);
    }
}

fn sticky_shift(s: &ComputedStyle, rect: Rect, port: Rect, cb: Rect) -> Point {
    let inset = |v: LengthPercentageAuto, base: Au| -> Option<Au> {
        match v {
            LengthPercentageAuto::Auto => None,
            LengthPercentageAuto::Set(lp) => Some(lp.resolve(base)),
        }
    };
    let mut dx = Au::ZERO;
    let mut dy = Au::ZERO;
    if let Some(t) = inset(s.inset.top, port.size.height) {
        let min_y = port.origin.y + t;
        if rect.origin.y < min_y {
            let max_shift = (cb.bottom() - rect.bottom()).max(Au::ZERO);
            dy = (min_y - rect.origin.y).min(max_shift);
        }
    }
    if let Some(b) = inset(s.inset.bottom, port.size.height) {
        let max_y = port.bottom() - b;
        if rect.bottom() + dy > max_y {
            let max_shift = (rect.origin.y - cb.origin.y).max(Au::ZERO);
            dy = dy.min(Au::ZERO).max(-(rect.bottom() - max_y).min(max_shift));
        }
    }
    if let Some(l) = inset(s.inset.left, port.size.width) {
        let min_x = port.origin.x + l;
        if rect.origin.x < min_x {
            let max_shift = (cb.right() - rect.right()).max(Au::ZERO);
            dx = (min_x - rect.origin.x).min(max_shift);
        }
    }
    if let Some(r) = inset(s.inset.right, port.size.width) {
        let max_x = port.right() - r;
        if rect.right() + dx > max_x {
            let max_shift = (rect.origin.x - cb.origin.x).max(Au::ZERO);
            dx = dx.min(Au::ZERO).max(-(rect.right() - max_x).min(max_shift));
        }
    }
    Point { x: dx, y: dy }
}
