//! Block formatting contexts (CSS 2.1 §9, §10): widths and the over-constrained
//! equations, margin collapsing, floats and clearance, block formatting context roots
//! next to floats, absolutely and fixed positioned boxes, relative offsets, replaced
//! elements and list markers.
//!
//! Coordinates: a block's fragment rect is relative to its containing block's content
//! box while it is being placed (the parent translates it into border-box coordinates
//! when it attaches it). Floats live in a `Bfc` whose coordinates are relative to the
//! BFC root's content box.

use crate::geom::{Au, Edges, Point, Rect, Size};
use crate::layout::boxes::{BoxId, BoxKind, Dim, Level, ReplacedBox};
use crate::layout::fragment::{Fragment, FragmentKind, Replaced, StyleSource};
use crate::layout::{inline, intrinsic, scroll, table, text, LayoutContext};
use crate::style::{BoxSizing, Clear, ComputedStyle, Direction, Float, LengthPercentage, LengthPercentageAuto, Position, Sizing, ZIndex};

/// The containing block a box is laid out in: its content width, and its height when
/// definite (percentage heights resolve against it; otherwise they are `auto`).
#[derive(Clone, Copy, Debug)]
pub struct Cb {
    pub width: Au,
    pub height: Option<Au>,
}

/// Margins waiting to collapse (§8.3.1): the largest positive and the most negative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarginSet {
    pub pos: Au,
    pub neg: Au,
}

impl MarginSet {
    pub fn of(m: Au) -> MarginSet {
        let mut s = MarginSet::default();
        s.add(m);
        s
    }
    pub fn add(&mut self, m: Au) {
        if m >= Au::ZERO {
            self.pos = self.pos.max(m);
        } else {
            self.neg = self.neg.min(m);
        }
    }
    pub fn union(mut self, o: MarginSet) -> MarginSet {
        self.pos = self.pos.max(o.pos);
        self.neg = self.neg.min(o.neg);
        self
    }
    pub fn collapse(self) -> Au {
        self.pos + self.neg
    }
    pub fn is_empty(self) -> bool {
        self.pos.is_zero() && self.neg.is_zero()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlacedFloat {
    /// Margin box, in BFC coordinates.
    pub rect: Rect,
    pub side: Float,
}

/// The float state of one block formatting context (§9.5).
#[derive(Clone, Debug, Default)]
pub struct Bfc {
    pub floats: Vec<PlacedFloat>,
    /// Floats below this y are the only ones that can still matter; earlier ones are
    /// skipped so lookups stay bounded on long documents.
    live_from: usize,
}

impl Bfc {
    pub fn new() -> Bfc {
        Bfc::default()
    }
    fn live(&self) -> &[PlacedFloat] {
        &self.floats[self.live_from..]
    }
    /// The interval free of floats at `y`, within `[left, right]`.
    pub fn available(&self, y: Au, left: Au, right: Au) -> (Au, Au) {
        let mut l = left;
        let mut r = right;
        for f in self.live() {
            if f.rect.origin.y <= y && y < f.rect.bottom() {
                match f.side {
                    Float::Left => l = l.max(f.rect.right()),
                    Float::Right => r = r.min(f.rect.origin.x),
                    Float::None => {}
                }
            }
        }
        (l, r.max(l))
    }
    /// The interval free of floats over the whole band `[y, y + height)`.
    pub fn available_band(&self, y: Au, height: Au, left: Au, right: Au) -> (Au, Au) {
        let mut l = left;
        let mut r = right;
        let y1 = y + height.max(Au(1));
        for f in self.live() {
            if f.rect.origin.y < y1 && y < f.rect.bottom() {
                match f.side {
                    Float::Left => l = l.max(f.rect.right()),
                    Float::Right => r = r.min(f.rect.origin.x),
                    Float::None => {}
                }
            }
        }
        (l, r.max(l))
    }
    /// The next y below `y` at which the available interval changes (a float ends).
    pub fn next_change(&self, y: Au) -> Option<Au> {
        self.live().iter().filter(|f| f.rect.origin.y <= y && y < f.rect.bottom()).map(|f| f.rect.bottom()).min()
    }
    /// The y a box with this `clear` must not be above.
    pub fn clear_y(&self, clear: Clear) -> Option<Au> {
        self.floats
            .iter()
            .filter(|f| match clear {
                Clear::None => false,
                Clear::Left => f.side == Float::Left,
                Clear::Right => f.side == Float::Right,
                Clear::Both => true,
            })
            .map(|f| f.rect.bottom())
            .max()
    }
    /// The lowest margin-bottom edge of any float.
    pub fn float_bottom(&self) -> Au {
        self.floats.iter().map(|f| f.rect.bottom()).max().unwrap_or(Au::ZERO)
    }
    /// The highest top of any float (rule 5: later floats may not be above it).
    pub fn last_top(&self) -> Au {
        self.floats.last().map(|f| f.rect.origin.y).unwrap_or(Au::MIN)
    }
    /// Places a float's margin box (§9.5.1 rules 1–9) no higher than `ceiling` inside
    /// the containing block interval `[left, right]`, returning its top-left.
    pub fn place(&mut self, side: Float, size: Size, ceiling: Au, left: Au, right: Au) -> Point {
        let mut y = ceiling.max(self.last_top());
        loop {
            let (l, r) = self.available_band(y, size.height, left, right);
            let fits = r - l >= size.width || {
                // Move down until it fits or no float constrains this band any more.
                let (l0, r0) = (left, right);
                (l, r) == (l0, r0)
            };
            if fits {
                let x = match side {
                    Float::Right => r - size.width,
                    _ => l,
                };
                let rect = Rect::new(x, y, size.width, size.height);
                self.floats.push(PlacedFloat { rect, side });
                self.prune(y);
                return Point { x, y };
            }
            match self.next_change_band(y, size.height) {
                Some(ny) if ny > y => y = ny,
                _ => {
                    let x = match side {
                        Float::Right => right - size.width,
                        _ => left,
                    };
                    let rect = Rect::new(x, y, size.width, size.height);
                    self.floats.push(PlacedFloat { rect, side });
                    return Point { x, y };
                }
            }
        }
    }
    fn next_change_band(&self, y: Au, height: Au) -> Option<Au> {
        let y1 = y + height.max(Au(1));
        self.live().iter().filter(|f| f.rect.origin.y < y1 && y < f.rect.bottom()).map(|f| f.rect.bottom()).min()
    }
    /// Floats entirely above `y` cannot affect anything placed at or below `y`, since
    /// later floats and lines never move up; drop them from lookups.
    fn prune(&mut self, y: Au) {
        while self.live_from < self.floats.len() && self.floats[self.live_from].rect.bottom() <= y {
            self.live_from += 1;
        }
    }
}

/// An absolutely positioned box waiting for its containing block.
#[derive(Clone, Debug)]
pub struct AbsRequest {
    pub id: BoxId,
    /// Static position (where the box's margin-box top-left would be in flow), relative
    /// to the border box of the fragment currently carrying the request.
    pub static_pos: Point,
    pub fixed: bool,
}

pub fn translate_requests(reqs: &mut [AbsRequest], dx: Au, dy: Au) {
    for r in reqs {
        r.static_pos.x += dx;
        r.static_pos.y += dy;
    }
}

/// The result of laying out one block-level box in a flow.
#[derive(Debug)]
pub struct BlockResult {
    /// Positioned relative to the containing block's content box.
    pub fragment: Fragment,
    pub margin: Edges,
    /// Bottom margins that collapse out of the box (its own, and its last children's
    /// when adjoining).
    pub bottom_margins: MarginSet,
    pub abs: Vec<AbsRequest>,
    pub first_baseline: Option<Au>,
    pub last_baseline: Option<Au>,
}

/// Result of laying out a block container's contents.
#[derive(Debug, Default)]
pub struct ContentsResult {
    /// Children, positioned relative to the content box.
    pub fragments: Vec<Fragment>,
    /// The content height without pending bottom margins.
    pub height: Au,
    pub pending_bottom: MarginSet,
    /// No in-flow content: margins collapse through.
    pub empty: bool,
    pub first_baseline: Option<Au>,
    pub last_baseline: Option<Au>,
    pub abs: Vec<AbsRequest>,
}

// Resolution helpers.

pub fn resolve_lp(v: LengthPercentage, base: Au) -> Au {
    v.resolve(base)
}

pub fn padding_edges(s: &ComputedStyle, cbw: Au) -> Edges {
    Edges { top: s.padding.top.resolve(cbw), right: s.padding.right.resolve(cbw), bottom: s.padding.bottom.resolve(cbw), left: s.padding.left.resolve(cbw) }
        .non_negative()
}

trait EdgesExt {
    fn non_negative(self) -> Self;
}
impl EdgesExt for Edges {
    fn non_negative(self) -> Edges {
        Edges { top: self.top.max(Au::ZERO), right: self.right.max(Au::ZERO), bottom: self.bottom.max(Au::ZERO), left: self.left.max(Au::ZERO) }
    }
}

fn margin_or_zero(m: LengthPercentageAuto, base: Au) -> Au {
    m.resolve(base).unwrap_or(Au::ZERO)
}

/// Vertical margins (percentages resolve against the containing block *width*).
pub fn vertical_margins(s: &ComputedStyle, cbw: Au) -> (Au, Au) {
    (margin_or_zero(s.margin.top, cbw), margin_or_zero(s.margin.bottom, cbw))
}

/// A sizing value resolved to a content-box length, or `None` for auto and for
/// percentages without a definite base.
pub fn resolve_size(v: Sizing, base: Option<Au>, edges: Au, box_sizing: BoxSizing) -> Option<Au> {
    match v {
        Sizing::Auto | Sizing::None | Sizing::MinContent | Sizing::MaxContent | Sizing::FitContent => None,
        Sizing::Set(lp) => {
            let v = lp.maybe_resolve(base)?;
            Some(match box_sizing {
                BoxSizing::ContentBox => v,
                BoxSizing::BorderBox => (v - edges).max(Au::ZERO),
            })
        }
    }
}

/// Clamps a content-box length by `min`/`max` (the min wins).
pub fn clamp_size(v: Au, min: Sizing, max: Sizing, base: Option<Au>, edges: Au, bs: BoxSizing) -> Au {
    let mut r = v;
    if let Some(mx) = resolve_size(max, base, edges, bs) {
        r = r.min(mx);
    }
    if let Some(mn) = resolve_size(min, base, edges, bs) {
        r = r.max(mn);
    }
    r.max(Au::ZERO)
}

/// Resolves `height` for a box with these edges; `None` is auto.
pub fn resolve_height(s: &ComputedStyle, cb_height: Option<Au>, edges: Au) -> Option<Au> {
    resolve_size(s.height, cb_height, edges, s.box_sizing)
}

pub fn clamp_height(s: &ComputedStyle, h: Au, cb_height: Option<Au>, edges: Au) -> Au {
    clamp_size(h, s.min_height, s.max_height, cb_height, edges, s.box_sizing)
}

/// Whether the box's `min-height` is zero (for margin collapsing through).
fn min_height_is_zero(s: &ComputedStyle) -> bool {
    match s.min_height {
        Sizing::Auto | Sizing::None => true,
        Sizing::Set(lp) => lp.is_zero(),
        _ => false,
    }
}

/// Whether `height` is auto or zero (for margin collapsing through).
fn height_is_auto_or_zero(s: &ComputedStyle, cb_height: Option<Au>) -> bool {
    match s.height {
        Sizing::Auto => true,
        Sizing::Set(lp) => lp.maybe_resolve(cb_height).is_none_or(|v| v <= Au::ZERO),
        _ => false,
    }
}

/// The used horizontal values of an in-flow block-level box (§10.3.3) given the
/// content width when known (`Some`), else auto. Returns `(content width, margin-left,
/// margin-right)`.
pub fn block_horizontal(s: &ComputedStyle, cbw: Au, width: Option<Au>, edges_h: Au) -> (Au, Au, Au) {
    let ml = s.margin.left;
    let mr = s.margin.right;
    match width {
        None => {
            let ml = margin_or_zero(ml, cbw);
            let mr = margin_or_zero(mr, cbw);
            ((cbw - ml - mr - edges_h).max(Au::ZERO), ml, mr)
        }
        Some(w) => {
            let rem = cbw - w - edges_h;
            let (ml_auto, mr_auto) = (ml.is_auto(), mr.is_auto());
            let mut mlv = margin_or_zero(ml, cbw);
            let mut mrv = margin_or_zero(mr, cbw);
            if rem < Au::ZERO && (ml_auto || mr_auto) {
                // Auto margins are treated as zero and the box overflows.
                if s.direction == Direction::Rtl {
                    mlv = rem - mrv;
                } else {
                    mrv = rem - mlv;
                }
            } else if ml_auto && mr_auto {
                mlv = rem / 2;
                mrv = rem - mlv;
            } else if ml_auto {
                mlv = rem - mrv;
            } else if mr_auto {
                mrv = rem - mlv;
            } else if s.direction == Direction::Rtl {
                mlv = rem - mrv;
            } else {
                mrv = rem - mlv;
            }
            (w, mlv, mrv)
        }
    }
}

/// Width of an in-flow block-level non-replaced box with min/max clamping (§10.4).
/// `avail` is the width available to the border box plus margins (the containing
/// block width, or the interval next to floats for BFC roots).
pub fn block_width(ctx: &LayoutContext, id: BoxId, cbw: Au, avail: Au, edges_h: Au) -> (Au, Au, Au) {
    let s = ctx.style(id);
    let specified = match s.width {
        Sizing::Set(lp) => Some(match s.box_sizing {
            BoxSizing::ContentBox => lp.resolve(cbw),
            BoxSizing::BorderBox => (lp.resolve(cbw) - edges_h).max(Au::ZERO),
        }),
        Sizing::MinContent => Some((intrinsic::min_max(ctx, id).0 - edges_h).max(Au::ZERO)),
        Sizing::MaxContent => Some((intrinsic::min_max(ctx, id).1 - edges_h).max(Au::ZERO)),
        Sizing::FitContent => {
            let (mn, mx) = intrinsic::min_max(ctx, id);
            let ml = margin_or_zero(s.margin.left, cbw);
            let mr = margin_or_zero(s.margin.right, cbw);
            Some((mx.min((avail - ml - mr).max(mn)) - edges_h).max(Au::ZERO))
        }
        Sizing::Auto | Sizing::None => None,
    };
    let (w, ml, mr) = block_horizontal(s, avail, specified, edges_h);
    let clamped = clamp_size(w, s.min_width, s.max_width, Some(cbw), edges_h, s.box_sizing);
    if clamped != w {
        block_horizontal(s, avail, Some(clamped), edges_h)
    } else {
        (w, ml, mr)
    }
}

/// Shrink-to-fit width (§10.3.5) of a box's border box: floats, absolutes with auto
/// width, inline-blocks, table wrappers.
pub fn shrink_to_fit(ctx: &LayoutContext, id: BoxId, available: Au) -> Au {
    let (mn, mx) = intrinsic::min_max(ctx, id);
    mn.max(available.min(mx))
}

/// Used content-box size of a replaced element (§10.3.2, §10.6.2).
pub fn replaced_size(ctx: &LayoutContext, id: BoxId, rb: &ReplacedBox, cb: &Cb) -> Size {
    let s = ctx.style(id);
    let p = padding_edges(s, cb.width);
    let b = s.used_border_widths();
    let eh = p.horizontal() + b.horizontal();
    let ev = p.vertical() + b.vertical();
    let css_w = resolve_size(s.width, Some(cb.width), eh, s.box_sizing);
    let css_h = resolve_size(s.height, cb.height, ev, s.box_sizing);
    let attr = |d: Option<Dim>, base: Option<Au>| -> Option<Au> {
        match d? {
            Dim::Px(a) => Some(a),
            Dim::Percent(p) => base.map(|b| b.percent_of(p)),
        }
    };
    let w = css_w.or_else(|| attr(rb.attr_width, Some(cb.width)));
    let h = css_h.or_else(|| attr(rb.attr_height, cb.height));
    let intrinsic = rb.intrinsic.unwrap_or(match &rb.replaced {
        Replaced::Image { .. } if w.is_none() && h.is_none() => Size { width: Au::from_px_i32(16), height: Au::from_px_i32(16) },
        Replaced::Image { .. } => Size { width: Au::ZERO, height: Au::ZERO },
        _ => Size { width: Au::from_px_i32(300), height: Au::from_px_i32(150) },
    });
    let (iw, ih) = (intrinsic.width, intrinsic.height);
    let (mut uw, mut uh) = match (w, h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, if iw > Au::ZERO { w.scale(ih.0, iw.0) } else { ih }),
        (None, Some(h)) => (if ih > Au::ZERO { h.scale(iw.0, ih.0) } else { iw }, h),
        (None, None) => (iw, ih),
    };
    let cw = clamp_size(uw, s.min_width, s.max_width, Some(cb.width), eh, s.box_sizing);
    if cw != uw {
        if w.is_none() && h.is_none() && uw > Au::ZERO {
            uh = cw.scale(uh.0, uw.0);
        }
        uw = cw;
    }
    let ch = clamp_size(uh, s.min_height, s.max_height, cb.height, ev, s.box_sizing);
    if ch != uh {
        if w.is_none() && h.is_none() && uh > Au::ZERO {
            uw = ch.scale(uw.0, uh.0);
        }
        uh = ch;
    }
    Size { width: uw.max(Au::ZERO), height: uh.max(Au::ZERO) }
}

/// The relative-position offset of a box (§9.4.3).
pub fn relative_offset(s: &ComputedStyle, cb: &Cb) -> Point {
    if s.position != Position::Relative && s.position != Position::Sticky {
        return Point::default();
    }
    if s.position == Position::Sticky {
        return Point::default();
    }
    let left = s.inset.left.resolve(cb.width);
    let right = s.inset.right.resolve(cb.width);
    let top = match s.inset.top {
        LengthPercentageAuto::Auto => None,
        LengthPercentageAuto::Set(lp) => lp.maybe_resolve(cb.height),
    };
    let bottom = match s.inset.bottom {
        LengthPercentageAuto::Auto => None,
        LengthPercentageAuto::Set(lp) => lp.maybe_resolve(cb.height),
    };
    let x = match (left, right) {
        (Some(l), _) if s.direction == Direction::Ltr => l,
        (_, Some(r)) if s.direction == Direction::Rtl => -r,
        (Some(l), None) => l,
        (None, Some(r)) => -r,
        _ => Au::ZERO,
    };
    let y = match (top, bottom) {
        (Some(t), _) => t,
        (None, Some(b)) => -b,
        _ => Au::ZERO,
    };
    Point { x, y }
}

/// Marks paint-order facts on a fragment from the box's style and computes its
/// overflow rect from its children.
pub fn finish_fragment(ctx: &LayoutContext, id: BoxId, f: &mut Fragment) {
    let b = &ctx.tree[id];
    let s = &b.style;
    f.is_float = s.is_floating();
    f.is_positioned = s.is_positioned();
    f.z_index = match s.z_index {
        ZIndex::Int(i) => i,
        ZIndex::Auto => 0,
    };
    f.establishes_stacking_context = b.is_root || s.establishes_stacking_context(false);
    compute_overflow(f, b.is_scroll_container());
}

/// The overflow rect: the union of the border box and the descendants' overflow,
/// unless the box clips.
pub fn compute_overflow(f: &mut Fragment, clips: bool) {
    let own = Rect::new(Au::ZERO, Au::ZERO, f.rect.size.width, f.rect.size.height);
    if clips {
        f.overflow = own;
        return;
    }
    let mut r = own;
    for c in &f.children {
        let co = c.overflow.translate(c.rect.origin.x, c.rect.origin.y);
        r = r.union(co);
    }
    f.overflow = r;
}

/// Structural test for a block that collapses through (§8.3.1): auto or zero height,
/// zero min-height, no vertical padding or border, no BFC, no marker, and no in-flow
/// content that produces line boxes.
pub fn is_empty_block(ctx: &LayoutContext, id: BoxId) -> bool {
    if let Some(v) = ctx.cache.borrow().empty_block.get(id.index()).copied().flatten() {
        return v;
    }
    let v = compute_empty_block(ctx, id);
    if let Some(slot) = ctx.cache.borrow_mut().empty_block.get_mut(id.index()) {
        *slot = Some(v);
    }
    v
}

fn compute_empty_block(ctx: &LayoutContext, id: BoxId) -> bool {
    let b = &ctx.tree[id];
    if b.kind != BoxKind::Block || b.level != Level::Block || b.marker.is_some() || b.control.is_some() || b.establishes_bfc() {
        return false;
    }
    let s = &b.style;
    if !height_is_auto_or_zero(s, None) || !min_height_is_zero(s) {
        return false;
    }
    if s.border.top.used_width() > Au::ZERO || s.border.bottom.used_width() > Au::ZERO || !s.padding.top.is_zero() || !s.padding.bottom.is_zero() {
        return false;
    }
    if b.inline_children {
        b.children.iter().all(|&c| inline_is_empty(ctx, c))
    } else {
        b.children.iter().all(|&c| ctx.tree[c].is_out_of_flow() || is_empty_block(ctx, c))
    }
}

fn inline_is_empty(ctx: &LayoutContext, id: BoxId) -> bool {
    let b = &ctx.tree[id];
    match &b.kind {
        BoxKind::Text(t) => text::is_collapsible_whitespace(&t.text, b.style.white_space),
        BoxKind::Inline => {
            let s = &b.style;
            let edges_zero = s.used_border_widths().horizontal().is_zero()
                && s.used_border_widths().vertical().is_zero()
                && s.padding.left.is_zero()
                && s.padding.right.is_zero()
                && s.padding.top.is_zero()
                && s.padding.bottom.is_zero()
                && margin_or_zero(s.margin.left, Au::ZERO).is_zero()
                && margin_or_zero(s.margin.right, Au::ZERO).is_zero();
            edges_zero && b.children.iter().all(|&c| inline_is_empty(ctx, c))
        }
        BoxKind::Wbr => true,
        _ => b.is_out_of_flow(),
    }
}

/// The margins that collapse with a box's top margin from inside it: its own top
/// margin, and, while it has no top border or padding and is not a BFC root, its
/// leading empty children's margins and the first non-empty child's chain.
pub fn top_margin_chain(ctx: &LayoutContext, id: BoxId, cbw: Au, bfc: &Bfc, y_hint: Au) -> MarginSet {
    let _ = y_hint;
    let b = &ctx.tree[id];
    let s = &b.style;
    let mut set = MarginSet::of(margin_or_zero(s.margin.top, cbw));
    if b.kind != BoxKind::Block || b.establishes_bfc() || s.border.top.used_width() > Au::ZERO || !s.padding.top.is_zero() || b.marker.is_some() || b.inline_children {
        return set;
    }
    let inner_w = {
        let p = padding_edges(s, cbw);
        let bw = s.used_border_widths();
        let (w, _, _) = block_width(ctx, id, cbw, cbw, p.horizontal() + bw.horizontal());
        w
    };
    let mut saw_float = !bfc.floats.is_empty();
    for &c in &b.children {
        let cb = &ctx.tree[c];
        if cb.is_out_of_flow() {
            saw_float |= cb.is_float();
            continue;
        }
        // A child that may get clearance resolves the position here (as LayoutNG
        // does): its margin does not join the chain.
        if cb.style.clear != Clear::None && saw_float {
            return set;
        }
        if is_empty_block(ctx, c) {
            set.add(margin_or_zero(cb.style.margin.top, inner_w));
            set.add(margin_or_zero(cb.style.margin.bottom, inner_w));
            continue;
        }
        return set.union(top_margin_chain(ctx, c, inner_w, bfc, y_hint));
    }
    set
}

/// Lays out the in-flow block-level children of a block container (§9.4.1, §8.3.1).
/// `origin` is the BFC coordinate of the content box. `top_adjoining` says the
/// container's top margin was collapsed with its first child's chain by the caller;
/// `bottom_adjoining` that the last child's bottom margin collapses out.
#[allow(clippy::too_many_arguments)]
pub fn layout_block_children(ctx: &LayoutContext, parent: BoxId, cb: &Cb, bfc: &mut Bfc, origin: Point, top_adjoining: bool, bottom_adjoining: bool) -> ContentsResult {
    let children = ctx.tree.children(parent);
    let mut out = ContentsResult::default();
    let mut pending = MarginSet::default();
    let mut y = Au::ZERO;
    let mut at_top = top_adjoining;
    let mut any_content = false;
    let rtl = ctx.style(parent).direction == Direction::Rtl;
    for &c in children {
        let cbx = &ctx.tree[c];
        if cbx.is_float() {
            let ceiling = y + if at_top { Au::ZERO } else { pending.collapse() };
            let (frag, mut abs) = layout_float(ctx, c, cb, bfc, origin, ceiling);
            translate_requests(&mut abs, frag.rect.origin.x, frag.rect.origin.y);
            out.abs.extend(abs);
            out.fragments.push(frag);
            continue;
        }
        if cbx.is_abs() {
            let sy = y + if at_top { Au::ZERO } else { pending.collapse() };
            let sx = if rtl { cb.width } else { Au::ZERO };
            out.abs.push(AbsRequest { id: c, static_pos: Point { x: sx, y: sy }, fixed: cbx.style.position == Position::Fixed });
            continue;
        }
        if matches!(cbx.kind, BoxKind::Col(_) | BoxKind::ColGroup(_) | BoxKind::Wbr) {
            continue;
        }
        // Clearance (§9.5.2). A child with `clear` after floats resolves the flow
        // position here: its margin chain does not collapse with the parent's top.
        let mut clearance = false;
        let mut chain_cut = false;
        if cbx.style.clear != Clear::None && !bfc.floats.is_empty() {
            chain_cut = true;
            let chain = top_margin_chain(ctx, c, cb.width, bfc, origin.y + y);
            let hyp = y + if at_top { chain.collapse() } else { pending.union(chain).collapse() };
            match bfc.clear_y(cbx.style.clear) {
                Some(cy) if cy - origin.y > hyp => {
                    clearance = true;
                    y = cy - origin.y;
                }
                _ => y = hyp,
            }
            pending = MarginSet::default();
            at_top = false;
        }
        if !chain_cut && is_empty_block(ctx, c) {
            let mt = margin_or_zero(cbx.style.margin.top, cb.width);
            let mb = margin_or_zero(cbx.style.margin.bottom, cb.width);
            let yc = y + if at_top { Au::ZERO } else { pending.union(MarginSet::of(mt)).collapse() };
            pending.add(mt);
            pending.add(mb);
            let mut r = layout_block_level(ctx, c, cb, bfc, origin, yc);
            r.fragment.rect.origin.y = yc + relative_offset(&cbx.style, cb).y;
            translate_requests(&mut r.abs, r.fragment.rect.origin.x, r.fragment.rect.origin.y);
            out.abs.extend(r.abs);
            out.fragments.push(r.fragment);
            continue;
        }
        let yc = if chain_cut {
            y
        } else {
            let chain = top_margin_chain(ctx, c, cb.width, bfc, origin.y + y);
            if at_top {
                Au::ZERO
            } else {
                y + pending.union(chain).collapse()
            }
        };
        let _ = clearance;
        at_top = false;
        let mut r = layout_block_level(ctx, c, cb, bfc, origin, yc);
        let frag_y = r.fragment.rect.origin.y;
        y = frag_y + r.fragment.rect.size.height;
        pending = r.bottom_margins;
        if !any_content {
            out.first_baseline = r.first_baseline.map(|b| b + frag_y);
        }
        if let Some(lb) = r.last_baseline {
            out.last_baseline = Some(lb + frag_y);
        }
        any_content = true;
        let off = relative_offset(&cbx.style, cb);
        r.fragment.rect.origin.x += off.x;
        r.fragment.rect.origin.y += off.y;
        translate_requests(&mut r.abs, r.fragment.rect.origin.x, r.fragment.rect.origin.y);
        out.abs.extend(r.abs);
        out.fragments.push(r.fragment);
    }
    if any_content {
        if bottom_adjoining {
            out.height = y;
            out.pending_bottom = pending;
        } else {
            out.height = y + pending.collapse();
            out.pending_bottom = MarginSet::default();
        }
        out.empty = false;
    } else {
        out.height = Au::ZERO;
        out.pending_bottom = pending;
        out.empty = true;
    }
    out
}

/// Lays out the contents of any block container: block children or inline content.
#[allow(clippy::too_many_arguments)]
pub fn layout_contents(ctx: &LayoutContext, id: BoxId, cb: &Cb, bfc: &mut Bfc, origin: Point, top_adjoining: bool, bottom_adjoining: bool) -> ContentsResult {
    let b = &ctx.tree[id];
    if b.inline_children {
        let r = inline::layout_inline_content(ctx, id, cb, bfc, origin);
        ContentsResult {
            fragments: r.fragments,
            height: r.height,
            pending_bottom: MarginSet::default(),
            empty: r.empty,
            first_baseline: r.first_baseline,
            last_baseline: r.last_baseline,
            abs: r.abs,
        }
    } else {
        layout_block_children(ctx, id, cb, bfc, origin, top_adjoining, bottom_adjoining)
    }
}

/// Lays out one in-flow block-level box at tentative content-box offset `y` in its
/// containing block, whose content box sits at `cb_origin` in BFC coordinates.
pub fn layout_block_level(ctx: &LayoutContext, id: BoxId, cb: &Cb, bfc: &mut Bfc, cb_origin: Point, y: Au) -> BlockResult {
    let b = &ctx.tree[id];
    match &b.kind {
        BoxKind::Replaced(rb) => layout_block_replaced(ctx, id, rb, cb, y),
        BoxKind::TableWrapper => table::layout_wrapper(ctx, id, cb, bfc, cb_origin, y, None),
        BoxKind::Marker(_) => {
            let f = marker_fragment(ctx, id, None);
            BlockResult { fragment: Fragment::new(f.kind.clone(), Rect::new(Au::ZERO, y, f.rect.size.width, f.rect.size.height)), margin: Edges::ZERO, bottom_margins: MarginSet::default(), abs: Vec::new(), first_baseline: None, last_baseline: None }
        }
        _ => layout_block_box(ctx, id, cb, bfc, cb_origin, y, None),
    }
}

/// Lays out a block container box (block, list item, flow root, cell body). When
/// `forced_width` is given (cells, absolutes, floats) it is the content width.
pub fn layout_block_box(ctx: &LayoutContext, id: BoxId, cb: &Cb, bfc: &mut Bfc, cb_origin: Point, y_in: Au, forced_width: Option<Au>) -> BlockResult {
    let b = &ctx.tree[id];
    let s = &b.style;
    let p = padding_edges(s, cb.width);
    let bw = s.used_border_widths();
    let eh = p.horizontal() + bw.horizontal();
    let ev = p.vertical() + bw.vertical();
    let is_bfc_root = b.establishes_bfc();
    let mut y = y_in;
    let x;
    let (w, ml, mr);
    match forced_width {
        Some(fw) => {
            w = fw;
            ml = Au::ZERO;
            mr = Au::ZERO;
            x = Au::ZERO;
        }
        None => {
            if is_bfc_root && !bfc.floats.is_empty() {
                // §9.5: a BFC root's border box must not overlap floats: narrow or move down.
                let cb_left = cb_origin.x;
                let cb_right = cb_origin.x + cb.width;
                let mut tries = 0;
                loop {
                    let (l, r) = bfc.available(cb_origin.y + y, cb_left, cb_right);
                    let avail = r - l;
                    let (ww, mml, mmr) = block_width(ctx, id, cb.width, avail, eh);
                    let needs = ww + eh + mml.max(Au::ZERO);
                    if needs <= avail || avail >= cb.width || tries > 64 {
                        w = ww;
                        ml = mml;
                        mr = mmr;
                        x = (l - cb_left) + ml;
                        break;
                    }
                    tries += 1;
                    match bfc.next_change(cb_origin.y + y) {
                        Some(ny) => y = ny - cb_origin.y,
                        None => {
                            w = ww;
                            ml = mml;
                            mr = mmr;
                            x = (l - cb_left) + ml;
                            break;
                        }
                    }
                }
            } else {
                let r = block_width(ctx, id, cb.width, cb.width, eh);
                w = r.0;
                ml = r.1;
                mr = r.2;
                x = ml;
            }
        }
    }
    let (mt, mb) = vertical_margins(s, cb.width);
    let own_height = resolve_height(s, cb.height, ev);
    let quirky_root = ctx.quirks && b.node.is_some_and(|n| ctx.doc.is(n, "html") || ctx.doc.is(n, "body"));
    let child_cb_height = own_height.or(if quirky_root { Some(ctx.viewport.height) } else { None });
    let top_adjoining = !is_bfc_root && bw.top.is_zero() && p.top.is_zero() && b.marker.is_none();
    let bottom_adjoining = !is_bfc_root && bw.bottom.is_zero() && p.bottom.is_zero() && s.height == Sizing::Auto && min_height_is_zero(s);

    // Scrollbars reserve space; `auto` is decided after a first layout.
    let (bar_x, bar_y) = scroll::reserved_bars(s);
    let mut reserve_v = bar_y;
    let mut reserve_h = bar_x;
    let mut attempts = 0;
    let (contents, inner_bfc, content_w) = loop {
        let content_w = (w - reserve_v).max(Au::ZERO);
        let inner_cb = Cb { width: content_w, height: child_cb_height.map(|h| (h - reserve_h).max(Au::ZERO)) };
        let content_origin = Point { x: cb_origin.x + x + bw.left + p.left, y: cb_origin.y + y + bw.top + p.top };
        let mut inner_bfc = if is_bfc_root { Some(Bfc::new()) } else { None };
        let contents = match inner_bfc.as_mut() {
            Some(inner) => layout_contents(ctx, id, &inner_cb, inner, Point::default(), false, false),
            None => layout_contents(ctx, id, &inner_cb, bfc, content_origin, top_adjoining, bottom_adjoining),
        };
        if attempts == 0 && b.is_scroll_container() {
            let mut ch = contents.height;
            if let Some(inner) = &inner_bfc {
                ch = ch.max(inner.float_bottom());
            }
            let h_now = own_height.map(|h| clamp_height(s, h, cb.height, ev)).unwrap_or(clamp_height(s, ch, cb.height, ev));
            let content_size = scroll::content_size(&contents.fragments, content_w, ch);
            let (nx, ny) = scroll::auto_bars(s, content_size, Size { width: content_w, height: h_now }, bar_x, bar_y);
            if nx != reserve_h || ny != reserve_v {
                reserve_h = nx;
                reserve_v = ny;
                attempts += 1;
                continue;
            }
        }
        break (contents, inner_bfc, content_w);
    };
    let mut content_h = contents.height;
    if let Some(inner) = &inner_bfc {
        content_h = content_h.max(inner.float_bottom());
    }
    let mut bottom_margins = MarginSet::of(mb);
    if bottom_adjoining && !contents.empty {
        bottom_margins = bottom_margins.union(contents.pending_bottom);
    } else if !bottom_adjoining && contents.empty {
        // Margins of an empty box with a fixed height stay separate.
    }
    let mut h = match own_height {
        Some(h) => h,
        None => content_h + reserve_h,
    };
    h = clamp_height(s, h, cb.height, ev);
    let frag_w = w + eh;
    let frag_h = h + ev;
    let rect = Rect::new(x, y, frag_w, frag_h);
    let baseline = contents.first_baseline.map(|bl| bl + bw.top + p.top);
    let last_baseline = contents.last_baseline.map(|bl| bl + bw.top + p.top);
    let mut frag = Fragment::new(
        FragmentKind::Box { source: b.source, padding: p, border: bw, replaced: b.control.map(Replaced::Control), scroll: None, baseline },
        rect,
    );
    let cx = bw.left + p.left;
    let cy = bw.top + p.top;
    let mut abs = contents.abs;
    translate_requests(&mut abs, cx, cy);
    for mut c in contents.fragments {
        c.rect.origin.x += cx;
        c.rect.origin.y += cy;
        frag.children.push(c);
    }
    if let Some(m) = b.marker {
        if matches!(ctx.tree[m].kind, BoxKind::Marker(_)) {
            let mut mf = marker_fragment(ctx, m, baseline);
            mf.rect.origin.x = cx - mf.rect.size.width;
            frag.children.push(mf);
        }
    }
    let unresolved = if s.is_positioned() || b.is_root { resolve_absolutes(ctx, &mut frag, abs) } else { abs };
    finish_fragment(ctx, id, &mut frag);
    scroll::attach_scroll_info(ctx, id, &mut frag, content_w, h, reserve_h, reserve_v);
    let empty_box = contents.empty && own_height.is_none_or(|h| h <= Au::ZERO) && ev.is_zero() && b.marker.is_none() && !is_bfc_root && min_height_is_zero(s);
    let bottom_margins = if empty_box { MarginSet::of(mt).union(MarginSet::of(mb)).union(contents.pending_bottom) } else { bottom_margins };
    BlockResult { fragment: frag, margin: Edges { top: mt, right: mr, bottom: mb, left: ml }, bottom_margins, abs: unresolved, first_baseline: baseline, last_baseline }
}

/// A block-level replaced element (§10.3.4).
fn layout_block_replaced(ctx: &LayoutContext, id: BoxId, rb: &ReplacedBox, cb: &Cb, y: Au) -> BlockResult {
    let b = &ctx.tree[id];
    let s = &b.style;
    let p = padding_edges(s, cb.width);
    let bw = s.used_border_widths();
    let eh = p.horizontal() + bw.horizontal();
    let size = replaced_size(ctx, id, rb, cb);
    let (w, ml, mr) = block_horizontal(s, cb.width, Some(size.width), eh);
    let (mt, mb) = vertical_margins(s, cb.width);
    let mut frag = replaced_fragment(ctx, id, rb, size, p, bw);
    frag.rect.origin = Point { x: ml, y };
    let _ = w;
    BlockResult { fragment: frag, margin: Edges { top: mt, right: mr, bottom: mb, left: ml }, bottom_margins: MarginSet::of(mb), abs: Vec::new(), first_baseline: None, last_baseline: None }
}

/// The fragment of a replaced box with this content size, at the origin.
pub fn replaced_fragment(ctx: &LayoutContext, id: BoxId, rb: &ReplacedBox, size: Size, p: Edges, bw: Edges) -> Fragment {
    let b = &ctx.tree[id];
    let h = size.height + p.vertical() + bw.vertical();
    let w = size.width + p.horizontal() + bw.horizontal();
    let baseline = match &rb.replaced {
        // Text-like controls sit on the text baseline; others on their bottom edge.
        Replaced::Control(crate::layout::fragment::ControlKind::TextInput | crate::layout::fragment::ControlKind::Password | crate::layout::fragment::ControlKind::Select | crate::layout::fragment::ControlKind::Button | crate::layout::fragment::ControlKind::Submit | crate::layout::fragment::ControlKind::File) => {
            let fm = text::font_metrics(&b.style.font);
            let lh = size.height;
            let half = (lh - fm.content_height()) / 2;
            Some(bw.top + p.top + half + fm.ascent)
        }
        _ => Some(h),
    };
    let mut f = Fragment::new(FragmentKind::Box { source: b.source, padding: p, border: bw, replaced: Some(rb.replaced.clone()), scroll: None, baseline }, Rect::new(Au::ZERO, Au::ZERO, w, h));
    finish_fragment(ctx, id, &mut f);
    f
}

/// The marker box of a list item: text in the marker font, baseline-aligned with the
/// first line when there is one.
pub fn marker_fragment(ctx: &LayoutContext, m: BoxId, first_baseline: Option<Au>) -> Fragment {
    let b = &ctx.tree[m];
    let txt = match &b.kind {
        BoxKind::Marker(t) => t.clone(),
        _ => String::new(),
    };
    let s = &b.style;
    let fm = text::font_metrics(&s.font);
    let lh = s.line_height_au(fm.normal_line_height());
    let half = (lh - fm.content_height()) / 2;
    let ascent = half + fm.ascent;
    let w = text::measure(&s.font, &txt, s.letter_spacing, s.word_spacing);
    let y = first_baseline.map(|bl| bl - ascent).unwrap_or(Au::ZERO);
    let mut f = Fragment::new(FragmentKind::Box { source: b.source, padding: Edges::ZERO, border: Edges::ZERO, replaced: Some(Replaced::Marker(txt)), scroll: None, baseline: Some(ascent) }, Rect::new(Au::ZERO, y, w, lh));
    finish_fragment(ctx, m, &mut f);
    f
}

/// A float laid out but not yet placed: its fragment at the origin and its margins.
#[derive(Debug)]
pub struct PreparedFloat {
    pub fragment: Fragment,
    pub abs: Vec<AbsRequest>,
    pub margin: Edges,
}

impl PreparedFloat {
    pub fn margin_size(&self) -> Size {
        Size { width: self.fragment.rect.size.width + self.margin.horizontal(), height: self.fragment.rect.size.height + self.margin.vertical() }
    }
}

/// Sizes a float (§10.3.5, §10.6.6) without placing it.
pub fn prepare_float(ctx: &LayoutContext, id: BoxId, cb: &Cb) -> PreparedFloat {
    let b = &ctx.tree[id];
    let s = &b.style;
    let p = padding_edges(s, cb.width);
    let bw = s.used_border_widths();
    let eh = p.horizontal() + bw.horizontal();
    let ml = margin_or_zero(s.margin.left, cb.width);
    let mr = margin_or_zero(s.margin.right, cb.width);
    let (mt, mb) = vertical_margins(s, cb.width);
    let (fragment, abs) = layout_standalone(ctx, id, cb, cb.width - ml - mr, eh);
    PreparedFloat { fragment, abs, margin: Edges { top: mt, right: mr, bottom: mb, left: ml } }
}

/// Places a prepared float (§9.5.1) no higher than `ceiling` (content-box y of the
/// containing block). Returns the fragment positioned in the containing block's
/// content coordinates and its unresolved absolute requests.
pub fn place_float(ctx: &LayoutContext, id: BoxId, mut pf: PreparedFloat, cb: &Cb, bfc: &mut Bfc, cb_origin: Point, ceiling: Au) -> (Fragment, Vec<AbsRequest>) {
    let s = ctx.style(id);
    let size = pf.margin_size();
    let side = if s.float == Float::Right { Float::Right } else { Float::Left };
    let mut ceil = cb_origin.y + ceiling;
    if s.clear != Clear::None {
        if let Some(cy) = bfc.clear_y(s.clear) {
            ceil = ceil.max(cy);
        }
    }
    let pos = bfc.place(side, size, ceil, cb_origin.x, cb_origin.x + cb.width);
    let off = relative_offset(s, cb);
    pf.fragment.rect.origin = Point { x: pos.x - cb_origin.x + pf.margin.left + off.x, y: pos.y - cb_origin.y + pf.margin.top + off.y };
    (pf.fragment, pf.abs)
}

/// Lays out a float and places it. Returns the fragment positioned in the containing
/// block's content coordinates and its unresolved absolute requests.
pub fn layout_float(ctx: &LayoutContext, id: BoxId, cb: &Cb, bfc: &mut Bfc, cb_origin: Point, ceiling: Au) -> (Fragment, Vec<AbsRequest>) {
    let pf = prepare_float(ctx, id, cb);
    place_float(ctx, id, pf, cb, bfc, cb_origin, ceiling)
}

/// Lays out a box that establishes its own BFC and sizes itself by shrink-to-fit
/// (floats, inline-blocks, absolutes with auto width): the fragment is at the
/// origin. `avail` is the width available to its border box.
pub fn layout_standalone(ctx: &LayoutContext, id: BoxId, cb: &Cb, avail: Au, eh: Au) -> (Fragment, Vec<AbsRequest>) {
    let b = &ctx.tree[id];
    let s = &b.style;
    match &b.kind {
        BoxKind::Replaced(rb) => {
            let p = padding_edges(s, cb.width);
            let bw = s.used_border_widths();
            let size = replaced_size(ctx, id, rb, cb);
            (replaced_fragment(ctx, id, rb, size, p, bw), Vec::new())
        }
        BoxKind::TableWrapper => {
            let mut empty = Bfc::new();
            let r = table::layout_wrapper(ctx, id, cb, &mut empty, Point::default(), Au::ZERO, Some(avail));
            let mut f = r.fragment;
            f.rect.origin = Point::default();
            (f, r.abs)
        }
        _ => {
            let specified = resolve_size(s.width, Some(cb.width), eh, s.box_sizing).or_else(|| match s.width {
                Sizing::MinContent => Some((intrinsic::min_max(ctx, id).0 - eh).max(Au::ZERO)),
                Sizing::MaxContent => Some((intrinsic::min_max(ctx, id).1 - eh).max(Au::ZERO)),
                _ => None,
            });
            let w = match specified {
                Some(w) => w,
                None => (shrink_to_fit(ctx, id, avail.max(Au::ZERO)) - eh).max(Au::ZERO),
            };
            let w = clamp_size(w, s.min_width, s.max_width, Some(cb.width), eh, s.box_sizing);
            let mut empty = Bfc::new();
            let r = layout_block_box(ctx, id, cb, &mut empty, Point::default(), Au::ZERO, Some(w));
            let mut f = r.fragment;
            f.rect.origin = Point::default();
            (f, r.abs)
        }
    }
}

/// Resolves absolutely positioned descendants against this fragment (the containing
/// block: its padding box), appending their fragments; returns the requests that
/// must travel further up (fixed ones, unless this box has a transform).
pub fn resolve_absolutes(ctx: &LayoutContext, cbf: &mut Fragment, reqs: Vec<AbsRequest>) -> Vec<AbsRequest> {
    let mut rest = Vec::new();
    let has_transform = match cbf.source() {
        Some(src) if !src.is_anonymous() => ctx.tree.box_of(src.node()).is_some_and(|b| !ctx.tree[b].style.transform.is_empty()),
        _ => false,
    };
    let is_root = matches!(cbf.kind, FragmentKind::Box { source: StyleSource::Anonymous(n), .. } if n == crate::dom::Document::ROOT);
    for r in reqs {
        if r.fixed && !has_transform && !is_root {
            rest.push(r);
            continue;
        }
        let frag = layout_absolute(ctx, cbf, &r);
        cbf.children.push(frag);
    }
    rest
}

/// Lays out one absolutely positioned box against its containing block fragment
/// (§10.3.7, §10.6.4). The result is relative to the containing block's border box.
pub fn layout_absolute(ctx: &LayoutContext, cbf: &Fragment, req: &AbsRequest) -> Fragment {
    let id = req.id;
    let b = &ctx.tree[id];
    let s = &b.style;
    let (bl, bt, br_, bb) = match &cbf.kind {
        FragmentKind::Box { border, .. } => (border.left, border.top, border.right, border.bottom),
        _ => (Au::ZERO, Au::ZERO, Au::ZERO, Au::ZERO),
    };
    let cbw = (cbf.rect.size.width - bl - br_).max(Au::ZERO);
    let cbh = (cbf.rect.size.height - bt - bb).max(Au::ZERO);
    let cb = Cb { width: cbw, height: Some(cbh) };
    let p = padding_edges(s, cbw);
    let bw = s.used_border_widths();
    let eh = p.horizontal() + bw.horizontal();
    let ev = p.vertical() + bw.vertical();
    let static_x = req.static_pos.x - bl;
    let static_y = req.static_pos.y - bt;
    let rtl = s.direction == Direction::Rtl;

    let replaced_size_v = match &b.kind {
        BoxKind::Replaced(rb) => Some(replaced_size(ctx, id, rb, &cb)),
        _ => None,
    };
    let css_w = replaced_size_v.map(|z| z.width).or_else(|| resolve_size(s.width, Some(cbw), eh, s.box_sizing));
    let css_h = replaced_size_v.map(|z| z.height).or_else(|| resolve_size(s.height, Some(cbh), ev, s.box_sizing));

    // Horizontal (§10.3.7 / §10.3.8): left + ml + eh + w + mr + right = cbw.
    let mut left = s.inset.left.resolve(cbw);
    let mut right = s.inset.right.resolve(cbw);
    let (ml_auto, mr_auto) = (s.margin.left.is_auto(), s.margin.right.is_auto());
    let mut ml = margin_or_zero(s.margin.left, cbw);
    let mut mr = margin_or_zero(s.margin.right, cbw);
    let shrink = |avail: Au| -> Au {
        match replaced_size_v {
            Some(z) => z.width,
            None => (shrink_to_fit(ctx, id, avail.max(Au::ZERO)) - eh).max(Au::ZERO),
        }
    };
    if left.is_none() && css_w.is_none() && right.is_none() {
        if rtl {
            right = Some(cbw - static_x);
        } else {
            left = Some(static_x);
        }
    }
    let (mut w, x) = match (left, css_w, right) {
        (Some(l), Some(w), Some(r)) => {
            let rem = cbw - l - w - r - eh;
            if ml_auto && mr_auto {
                if rem >= Au::ZERO {
                    ml = rem / 2;
                    mr = rem - ml;
                } else if rtl {
                    mr = Au::ZERO;
                    ml = rem;
                } else {
                    ml = Au::ZERO;
                    mr = rem;
                }
            } else if ml_auto {
                ml = rem - mr;
            } else if mr_auto {
                mr = rem - ml;
            }
            // Over-constrained: `right` is ignored in ltr, `left` in rtl.
            let x = if !ml_auto && !mr_auto && rtl { cbw - r - w - eh - mr } else { l + ml };
            (w, x)
        }
        (None, None, Some(r)) => {
            let w = shrink(cbw - r - ml - mr - eh);
            (w, cbw - r - w - eh - mr)
        }
        (Some(l), None, None) => {
            let w = shrink(cbw - l - ml - mr - eh);
            (w, l + ml)
        }
        (None, Some(w), None) => (w, if rtl { cbw - static_x - w - eh - mr } else { static_x + ml }),
        (None, Some(w), Some(r)) => (w, cbw - r - w - eh - mr),
        (Some(l), None, Some(r)) => ((cbw - l - r - eh - ml - mr).max(Au::ZERO), l + ml),
        (Some(l), Some(w), None) => (w, l + ml),
        (None, None, None) => (Au::ZERO, ml),
    };
    let clamped = clamp_size(w, s.min_width, s.max_width, Some(cbw), eh, s.box_sizing);
    let x = if clamped != w && left.is_none() && right.is_some() { x + (w - clamped) } else { x };
    w = clamped;

    // Lay out the contents with this width to learn the auto height.
    let mut frag = match &b.kind {
        BoxKind::Replaced(rb) => replaced_fragment(ctx, id, rb, Size { width: w, height: css_h.unwrap_or(Au::ZERO) }, p, bw),
        BoxKind::TableWrapper => {
            let mut empty = Bfc::new();
            table::layout_wrapper(ctx, id, &cb, &mut empty, Point::default(), Au::ZERO, Some(w + eh)).fragment
        }
        _ => {
            let mut empty = Bfc::new();
            layout_block_box(ctx, id, &cb, &mut empty, Point::default(), Au::ZERO, Some(w)).fragment
        }
    };
    let content_h = (frag.rect.size.height - ev).max(Au::ZERO);

    // Vertical (§10.6.4 / §10.6.5): top + mt + ev + h + mb + bottom = cbh.
    let mut top = s.inset.top.resolve(cbh);
    let bottom = s.inset.bottom.resolve(cbh);
    let (mt_auto, mb_auto) = (s.margin.top.is_auto(), s.margin.bottom.is_auto());
    let mut mt = margin_or_zero(s.margin.top, cbw);
    let mut mb = margin_or_zero(s.margin.bottom, cbw);
    if top.is_none() && css_h.is_none() && bottom.is_none() {
        top = Some(static_y);
    }
    let (h, y) = match (top, css_h, bottom) {
        (Some(t), Some(h), Some(bo)) => {
            let rem = cbh - t - h - bo - ev;
            if mt_auto && mb_auto {
                mt = rem / 2;
                mb = rem - mt;
            } else if mt_auto {
                mt = rem - mb;
            } else if mb_auto {
                mb = rem - mt;
            }
            (h, t + mt)
        }
        (None, None, Some(bo)) => (content_h, cbh - bo - content_h - ev - mb),
        (Some(t), None, None) => (content_h, t + mt),
        (None, Some(h), None) => (h, static_y + mt),
        (None, Some(h), Some(bo)) => (h, cbh - bo - h - ev - mb),
        (Some(t), None, Some(bo)) => ((cbh - t - bo - ev - mt - mb).max(Au::ZERO), t + mt),
        (Some(t), Some(h), None) => (h, t + mt),
        (None, None, None) => (content_h, mt),
    };
    let _ = mb;
    let h = clamp_height(s, h, Some(cbh), ev);
    if h + ev != frag.rect.size.height {
        // The content keeps its layout; the box is simply taller or shorter.
        frag.rect.size.height = h + ev;
        compute_overflow(&mut frag, b.is_scroll_container());
    }
    frag.rect.origin = Point { x: bl + x, y: bt + y };
    frag.is_positioned = true;
    frag
}
