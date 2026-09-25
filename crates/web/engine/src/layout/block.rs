//! Block formatting contexts (CSS 2.1 §9, §10): widths and the over-constrained
//! equations, margin collapsing, floats and clearance, block formatting context roots
//! next to floats, absolutely and fixed positioned boxes, relative offsets, replaced
//! elements and list markers.
//!
//! Coordinates: a block's fragment rect is relative to its containing block's content
//! box while it is being placed (the parent translates it into border-box coordinates
//! when it attaches it). Floats live in a `Bfc` whose coordinates are relative to the
//! BFC root's content box.

use crate::dom::NodeId;
use crate::geom::{Au, Edges, Point, Rect, Size};
use crate::layout::boxes::{BoxId, BoxKind, Dim, Level, ReplacedBox};
use crate::layout::fragment::{Fragment, FragmentKind, Replaced, StyleSource};
use crate::layout::{inline, intrinsic, scroll, table, text, LayoutContext};
use crate::style::{
    BoxSizing, Clear, ComputedStyle, Direction, Float, LengthPercentage, LengthPercentageAuto,
    Position, Sizing, TextAlign, ZIndex,
};

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
        self.live()
            .iter()
            .filter(|f| f.rect.origin.y <= y && y < f.rect.bottom())
            .map(|f| f.rect.bottom())
            .min()
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
        self.floats
            .iter()
            .map(|f| f.rect.bottom())
            .max()
            .unwrap_or(Au::ZERO)
    }
    /// The highest top of any float (rule 5: later floats may not be above it).
    pub fn last_top(&self) -> Au {
        self.floats
            .last()
            .map(|f| f.rect.origin.y)
            .unwrap_or(Au::MIN)
    }
    /// Places a float's margin box (§9.5.1 rules 1–9) no higher than `ceiling` inside
    /// the containing block interval `[left, right]`, returning its top-left.
    pub fn place(&mut self, side: Float, size: Size, ceiling: Au, left: Au, right: Au) -> Point {
        self.place_from(side, size, ceiling, ceiling, left, right)
    }
    /// As `place`, with the flow position given apart from the ceiling: a float with
    /// `clear` has a ceiling below the floats it clears, but the in-flow content that
    /// follows it is still laid out from `flow_y`, beside those floats, so only floats
    /// that end above `flow_y` may be dropped from lookups.
    pub fn place_from(
        &mut self,
        side: Float,
        size: Size,
        ceiling: Au,
        flow_y: Au,
        left: Au,
        right: Au,
    ) -> Point {
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
                self.prune(flow_y.min(y));
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
        self.live()
            .iter()
            .filter(|f| f.rect.origin.y < y1 && y < f.rect.bottom())
            .map(|f| f.rect.bottom())
            .min()
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
#[derive(Clone, Debug)]
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
    Edges {
        top: s.padding.top.resolve(cbw),
        right: s.padding.right.resolve(cbw),
        bottom: s.padding.bottom.resolve(cbw),
        left: s.padding.left.resolve(cbw),
    }
    .non_negative()
}

trait EdgesExt {
    fn non_negative(self) -> Self;
}
impl EdgesExt for Edges {
    fn non_negative(self) -> Edges {
        Edges {
            top: self.top.max(Au::ZERO),
            right: self.right.max(Au::ZERO),
            bottom: self.bottom.max(Au::ZERO),
            left: self.left.max(Au::ZERO),
        }
    }
}

fn margin_or_zero(m: LengthPercentageAuto, base: Au) -> Au {
    m.resolve(base).unwrap_or(Au::ZERO)
}

/// Vertical margins (percentages resolve against the containing block *width*).
pub fn vertical_margins(s: &ComputedStyle, cbw: Au) -> (Au, Au) {
    (
        margin_or_zero(s.margin.top, cbw),
        margin_or_zero(s.margin.bottom, cbw),
    )
}

/// A sizing value resolved to a content-box length, or `None` for auto and for
/// percentages without a definite base.
pub fn resolve_size(v: Sizing, base: Option<Au>, edges: Au, box_sizing: BoxSizing) -> Option<Au> {
    match v {
        Sizing::Auto
        | Sizing::None
        | Sizing::MinContent
        | Sizing::MaxContent
        | Sizing::FitContent => None,
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
pub fn clamp_size(
    v: Au,
    min: Sizing,
    max: Sizing,
    base: Option<Au>,
    edges: Au,
    bs: BoxSizing,
) -> Au {
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
    clamp_size(
        h,
        s.min_height,
        s.max_height,
        cb_height,
        edges,
        s.box_sizing,
    )
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
pub fn block_horizontal(
    s: &ComputedStyle,
    cbw: Au,
    width: Option<Au>,
    edges_h: Au,
) -> (Au, Au, Au) {
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
pub fn block_width(
    ctx: &LayoutContext,
    id: BoxId,
    cbw: Au,
    avail: Au,
    edges_h: Au,
) -> (Au, Au, Au) {
    block_width_in(
        ctx,
        id,
        &Cb {
            width: cbw,
            height: None,
        },
        avail,
        edges_h,
    )
}

/// The content-box height `aspect-ratio` gives a non-replaced box of content width
/// `w`. The ratio relates the sides of the box `box-sizing` names.
pub fn ratio_height(s: &ComputedStyle, w: Au, edges_h: Au, edges_v: Au) -> Option<Au> {
    match s.box_sizing {
        BoxSizing::ContentBox => s.aspect_ratio.height_for(w),
        BoxSizing::BorderBox => s
            .aspect_ratio
            .height_for(w + edges_h)
            .map(|h| (h - edges_v).max(Au::ZERO)),
    }
}

/// The content-box width `aspect-ratio` gives a box whose `height` is definite and
/// whose `width` is `auto`.
pub fn ratio_width(s: &ComputedStyle, cb_height: Option<Au>, edges_h: Au) -> Option<Au> {
    if s.width != Sizing::Auto || s.aspect_ratio.ratio.is_none() {
        return None;
    }
    let ev = s.used_border_widths().vertical() + vertical_padding_lengths(s);
    let h = resolve_height(s, cb_height, ev)?;
    match s.box_sizing {
        BoxSizing::ContentBox => s.aspect_ratio.width_for(h),
        BoxSizing::BorderBox => s
            .aspect_ratio
            .width_for(h + ev)
            .map(|w| (w - edges_h).max(Au::ZERO)),
    }
}

/// Vertical padding when it does not depend on the containing block's width.
fn vertical_padding_lengths(s: &ComputedStyle) -> Au {
    s.padding.top.maybe_resolve(None).unwrap_or(Au::ZERO)
        + s.padding.bottom.maybe_resolve(None).unwrap_or(Au::ZERO)
}

/// `block_width` with the containing block's height known, so a definite `height`
/// and an `aspect-ratio` can give the width.
pub fn block_width_in(
    ctx: &LayoutContext,
    id: BoxId,
    cb: &Cb,
    avail: Au,
    edges_h: Au,
) -> (Au, Au, Au) {
    let cbw = cb.width;
    let s = ctx.style(id);
    let specified = match s.width {
        Sizing::Auto if ratio_width(s, cb.height, edges_h).is_some() => {
            ratio_width(s, cb.height, edges_h)
        }
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
    let clamped = clamp_size(
        w,
        s.min_width,
        s.max_width,
        Some(cbw),
        edges_h,
        s.box_sizing,
    );
    if clamped != w {
        block_horizontal(s, avail, Some(clamped), edges_h)
    } else {
        (w, ml, mr)
    }
}

/// `-webkit-line-clamp: n` on a vertical legacy box: when the box's own lines number
/// more than `n`, the `n`th ends in an ellipsis (appended where it fits the line,
/// else replacing the text that does not), the content ends at that line, and what
/// follows stays laid out but is hidden for paint, as in Blink.
fn clamp_lines(
    ctx: &LayoutContext,
    s: &ComputedStyle,
    contents: &mut ContentsResult,
    content_w: Au,
    n: usize,
) {
    let lines: Vec<usize> = contents
        .fragments
        .iter()
        .enumerate()
        .filter(|(_, f)| matches!(f.kind, FragmentKind::Line))
        .map(|(i, _)| i)
        .collect();
    if n == 0 || lines.len() <= n {
        return;
    }
    let at = lines[n - 1];
    let line = &mut contents.fragments[at];
    let content_right = line
        .children
        .iter()
        .map(|c| c.rect.right())
        .fold(Au::ZERO, Au::max);
    let ell = text::advance(&s.font, '\u{2026}');
    if line.rect.origin.x + content_right + ell <= content_w {
        append_ellipsis(ctx, &mut line.children, s);
    } else {
        // Truncate as `text-overflow: ellipsis` does, against the content edge: what
        // stays plus the ellipsis fits the box, which is where Blink cuts.
        inline::apply_ellipsis(ctx, line, content_w, s);
    }
    compute_overflow(line, false);
    contents.height = line.rect.bottom();
    for hidden in &mut contents.fragments[at + 1..] {
        hidden.hidden_for_paint = true;
    }
    contents.pending_bottom = MarginSet::default();
}

/// Appends an ellipsis after the last text run of a line (depth first), as a
/// generated run of its own in that run's style, so the text keeps its rect (a
/// `Range` over the text does not cover the ellipsis); false when the line has no text.
fn append_ellipsis(ctx: &LayoutContext, kids: &mut Vec<Fragment>, cs: &ComputedStyle) -> bool {
    for i in (0..kids.len()).rev() {
        let made = match &kids[i].kind {
            FragmentKind::Text {
                source, baseline, ..
            } => {
                let st = match source {
                    StyleSource::Before(n) => ctx.styles.before(*n),
                    StyleSource::After(n) => ctx.styles.after(*n),
                    StyleSource::Marker(n) => ctx.styles.marker(*n),
                    src => ctx.styles.get(src.node()),
                }
                .unwrap_or(cs);
                let r = kids[i].rect;
                let kind = FragmentKind::Text {
                    source: *source,
                    text: "\u{2026}".into(),
                    node: None,
                    range: (0, 0),
                    baseline: *baseline,
                    ellipsis: true,
                };
                Some(Fragment::new(
                    kind,
                    Rect::new(
                        r.right(),
                        r.origin.y,
                        text::advance(&st.font, '\u{2026}'),
                        r.size.height,
                    ),
                ))
            }
            _ => None,
        };
        if let Some(f) = made {
            kids.insert(i + 1, f);
            return true;
        }
        if matches!(kids[i].kind, FragmentKind::InlineBox { .. })
            && append_ellipsis(ctx, &mut kids[i].children, cs)
        {
            let right = kids[i]
                .children
                .iter()
                .map(|c| c.rect.right())
                .fold(Au::ZERO, Au::max);
            kids[i].rect.size.width = kids[i].rect.size.width.max(right);
            return true;
        }
    }
    false
}

/// Shrink-to-fit width (§10.3.5) of a box's border box: floats, absolutes with auto
/// width, inline-blocks, table wrappers.
pub fn shrink_to_fit(ctx: &LayoutContext, id: BoxId, available: Au) -> Au {
    let (mn, mx) = intrinsic::min_max(ctx, id);
    mn.max(available.min(mx))
}

/// Used content-box size of a replaced element (§10.3.2, §10.6.2).
/// Images, canvases, videos and SVG have a natural aspect ratio that carries one
/// specified dimension to the other; form controls, iframes and the like have a
/// natural size but no ratio, so an `auto` dimension stays at its natural length
/// (an `<input>` at `width: 100%` keeps its one-line height).
pub fn has_natural_ratio(rb: &ReplacedBox) -> bool {
    match &rb.replaced {
        Replaced::Image { .. } => true,
        Replaced::Placeholder(tag) => matches!(tag.as_str(), "canvas" | "video" | "svg"),
        _ => false,
    }
}

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
        Replaced::Image { .. } if w.is_none() && h.is_none() => Size {
            width: Au::from_px_i32(16),
            height: Au::from_px_i32(16),
        },
        Replaced::Image { .. } => Size {
            width: Au::ZERO,
            height: Au::ZERO,
        },
        _ => Size {
            width: Au::from_px_i32(300),
            height: Au::from_px_i32(150),
        },
    });
    let (iw, ih) = (intrinsic.width, intrinsic.height);
    let ratio = has_natural_ratio(rb);
    let (mut uw, mut uh) = match (w, h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (
            w,
            if ratio && iw > Au::ZERO {
                w.scale(ih.0, iw.0)
            } else {
                ih
            },
        ),
        (None, Some(h)) => (
            if ratio && ih > Au::ZERO {
                h.scale(iw.0, ih.0)
            } else {
                iw
            },
            h,
        ),
        (None, None) => (iw, ih),
    };
    let cw = clamp_size(
        uw,
        s.min_width,
        s.max_width,
        Some(cb.width),
        eh,
        s.box_sizing,
    );
    if cw != uw {
        if ratio && w.is_none() && h.is_none() && uw > Au::ZERO {
            uh = cw.scale(uh.0, uw.0);
        }
        uw = cw;
    }
    let ch = clamp_size(uh, s.min_height, s.max_height, cb.height, ev, s.box_sizing);
    if ch != uh {
        if ratio && w.is_none() && h.is_none() && uh > Au::ZERO {
            uw = ch.scale(uw.0, uh.0);
        }
        uh = ch;
    }
    Size {
        width: uw.max(Au::ZERO),
        height: uh.max(Au::ZERO),
    }
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
    for c in f.children.iter().filter(|c| !c.hidden_for_paint) {
        let co = c.overflow.translate(c.rect.origin.x, c.rect.origin.y);
        r = r.union(co);
    }
    f.overflow = r;
}

/// Structural test for a block that collapses through (§8.3.1): auto or zero height,
/// zero min-height, no vertical padding or border, no BFC, no marker, and no in-flow
/// content that produces line boxes.
pub fn is_empty_block(ctx: &LayoutContext, id: BoxId) -> bool {
    if let Some(v) = ctx
        .cache
        .borrow()
        .empty_block
        .get(id.index())
        .copied()
        .flatten()
    {
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
    if b.kind != BoxKind::Block
        || b.level != Level::Block
        || b.marker.is_some()
        || b.control.is_some()
        || b.establishes_bfc()
    {
        return false;
    }
    let s = &b.style;
    if !height_is_auto_or_zero(s, None) || !min_height_is_zero(s) {
        return false;
    }
    // `aspect-ratio` gives the box a height from its width, so it is not empty even
    // with `height: auto` and no content: a ratio-sized thumbnail must take its
    // place in the flow rather than let the next block collapse through it.
    if s.aspect_ratio.ratio.is_some() {
        return false;
    }
    if s.border.top.used_width() > Au::ZERO
        || s.border.bottom.used_width() > Au::ZERO
        || !s.padding.top.is_zero()
        || !s.padding.bottom.is_zero()
    {
        return false;
    }
    if b.inline_children {
        b.children.iter().all(|&c| inline_is_empty(ctx, c))
    } else {
        b.children
            .iter()
            .all(|&c| ctx.tree[c].is_out_of_flow() || is_empty_block(ctx, c))
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

/// Every margin of a block that collapses through (`is_empty_block`): its own top and
/// bottom margins and, since nothing separates them, those of its in-flow descendants,
/// which are all empty blocks themselves (§8.3.1). Acid2's `.empty div` carries a
/// `-6em` bottom margin this way into the set that decides the smile's clearance.
pub fn empty_block_margins(ctx: &LayoutContext, id: BoxId, cbw: Au) -> MarginSet {
    empty_block_margins_with(ctx, id, cbw, true)
}

/// `empty_block_margins`, optionally without the block's own bottom margin: the set
/// that decides where the block itself sits.
pub fn empty_block_margins_with(
    ctx: &LayoutContext,
    id: BoxId,
    cbw: Au,
    own_bottom: bool,
) -> MarginSet {
    let b = &ctx.tree[id];
    let s = &b.style;
    let mut set = MarginSet::of(margin_or_zero(s.margin.top, cbw));
    if own_bottom {
        set.add(margin_or_zero(s.margin.bottom, cbw));
    }
    if !b.inline_children && b.children.iter().any(|&c| !ctx.tree[c].is_out_of_flow()) {
        let inner_w = {
            let p = padding_edges(s, cbw);
            let bw = s.used_border_widths();
            let (w, _, _) = block_width(ctx, id, cbw, cbw, p.horizontal() + bw.horizontal());
            w
        };
        for &c in &b.children {
            if !ctx.tree[c].is_out_of_flow() {
                set = set.union(empty_block_margins(ctx, c, inner_w));
            }
        }
    }
    set
}

/// The margins that collapse with a box's top margin from inside it: its own top
/// margin, and, while it has no top border or padding and is not a BFC root, its
/// leading empty children's margins and the first non-empty child's chain.
pub fn top_margin_chain(
    ctx: &LayoutContext,
    id: BoxId,
    cbw: Au,
    bfc: &Bfc,
    y_hint: Au,
) -> MarginSet {
    let _ = y_hint;
    let b = &ctx.tree[id];
    let s = &b.style;
    let mut set = MarginSet::of(margin_or_zero(s.margin.top, cbw));
    if b.kind != BoxKind::Block
        || b.establishes_bfc()
        || s.border.top.used_width() > Au::ZERO
        || !s.padding.top.is_zero()
        || b.marker.is_some()
        || b.inline_children
    {
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
            set = set.union(empty_block_margins(ctx, c, inner_w));
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
pub fn layout_block_children(
    ctx: &LayoutContext,
    parent: BoxId,
    cb: &Cb,
    bfc: &mut Bfc,
    origin: Point,
    top_adjoining: bool,
    bottom_adjoining: bool,
) -> ContentsResult {
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
            out.abs.push(AbsRequest {
                id: c,
                static_pos: Point { x: sx, y: sy },
                fixed: cbx.style.position == Position::Fixed,
            });
            continue;
        }
        if matches!(
            cbx.kind,
            BoxKind::Col(_) | BoxKind::ColGroup(_) | BoxKind::Wbr
        ) {
            continue;
        }
        // Clearance (§9.5.2). A child with `clear` after floats resolves the flow
        // position here: its margin chain does not collapse with the parent's top.
        let mut clearance = false;
        let mut chain_cut = false;
        if cbx.style.clear != Clear::None && !bfc.floats.is_empty() {
            chain_cut = true;
            let chain = top_margin_chain(ctx, c, cb.width, bfc, origin.y + y);
            let hyp = y + if at_top {
                chain.collapse()
            } else {
                pending.union(chain).collapse()
            };
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
            // Everything inside a block that collapses through is adjoining too
            // (§8.3.1): its descendants' margins join the same set. The block itself
            // sits where the set collapses to before its own bottom margin joins (the
            // "as though it had a bottom border" position; Blink agrees, and Acid2's
            // `.empty` lands 3px below the forehead through its child's -6em).
            let yc = y + if at_top {
                Au::ZERO
            } else {
                pending
                    .union(empty_block_margins_with(ctx, c, cb.width, false))
                    .collapse()
            };
            let _ = mt;
            pending = pending.union(empty_block_margins(ctx, c, cb.width));
            let mut r = layout_block_level(ctx, c, cb, bfc, origin, yc);
            r.fragment.rect.origin.y = yc + relative_offset(&cbx.style, cb).y;
            translate_requests(
                &mut r.abs,
                r.fragment.rect.origin.x,
                r.fragment.rect.origin.y,
            );
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
        r.fragment.rect.origin.x += webkit_center_shift(ctx, parent, c, cb, &r);
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
        translate_requests(
            &mut r.abs,
            r.fragment.rect.origin.x,
            r.fragment.rect.origin.y,
        );
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

/// `text-align: -webkit-center` on a block container (what `<center>` and
/// `align=center` compute to) also centres its in-flow block-level children whose
/// horizontal margins are not `auto`, as Blink does: the child's margin box is moved
/// to the middle of the containing block when it is narrower than it.
fn webkit_center_shift(
    ctx: &LayoutContext,
    parent: BoxId,
    child: BoxId,
    cb: &Cb,
    r: &BlockResult,
) -> Au {
    if ctx.style(parent).text_align != TextAlign::WebkitCenter {
        return Au::ZERO;
    }
    let cs = &ctx.tree[child].style;
    if cs.margin.left == LengthPercentageAuto::Auto || cs.margin.right == LengthPercentageAuto::Auto
    {
        return Au::ZERO;
    }
    // The specified margins: the used right margin has already absorbed the free
    // space (§10.3.3's over-constrained rule), which is exactly what moves here.
    let ml = margin_or_zero(cs.margin.left, cb.width);
    let mr = margin_or_zero(cs.margin.right, cb.width);
    let margin_box = ml + r.fragment.rect.size.width + mr;
    let free = cb.width - margin_box;
    if free > Au::ZERO {
        free / 2
    } else {
        Au::ZERO
    }
}

/// Lays out the contents of any block container: block children or inline content.
#[allow(clippy::too_many_arguments)]
pub fn layout_contents(
    ctx: &LayoutContext,
    id: BoxId,
    cb: &Cb,
    bfc: &mut Bfc,
    origin: Point,
    top_adjoining: bool,
    bottom_adjoining: bool,
) -> ContentsResult {
    let b = &ctx.tree[id];
    if crate::layout::flex::is_flex_container(b) {
        return crate::layout::flex::layout_contents(ctx, id, cb);
    }
    if crate::layout::grid::is_grid_container(&b.style) {
        return crate::layout::grid::layout_contents(ctx, id, cb);
    }
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
pub fn layout_block_level(
    ctx: &LayoutContext,
    id: BoxId,
    cb: &Cb,
    bfc: &mut Bfc,
    cb_origin: Point,
    y: Au,
) -> BlockResult {
    let b = &ctx.tree[id];
    match &b.kind {
        BoxKind::Replaced(rb) => layout_block_replaced(ctx, id, rb, cb, y),
        BoxKind::TableWrapper => table::layout_wrapper(ctx, id, cb, bfc, cb_origin, y, None),
        BoxKind::Marker(_) => {
            let f = marker_fragment(ctx, id, None);
            BlockResult {
                fragment: Fragment::new(
                    f.kind.clone(),
                    Rect::new(Au::ZERO, y, f.rect.size.width, f.rect.size.height),
                ),
                margin: Edges::ZERO,
                bottom_margins: MarginSet::default(),
                abs: Vec::new(),
                first_baseline: None,
                last_baseline: None,
            }
        }
        _ => layout_block_box(ctx, id, cb, bfc, cb_origin, y, None),
    }
}

/// Lays out a block container box (block, list item, flow root, cell body). When
/// `forced_width` is given (cells, absolutes, floats) it is the content width.
pub fn layout_block_box(
    ctx: &LayoutContext,
    id: BoxId,
    cb: &Cb,
    bfc: &mut Bfc,
    cb_origin: Point,
    y_in: Au,
    forced_width: Option<Au>,
) -> BlockResult {
    // A box that establishes a formatting context, with no floats outside it to
    // avoid, lays out the same wherever it is placed: flex and grid layout lay
    // their items out several times (measuring, then stretching), and each of those
    // would lay the whole subtree out again. Its result is kept for the pass.
    let memo_key =
        (!ctx.cache.borrow().no_memo && bfc.floats.is_empty() && ctx.tree[id].establishes_bfc())
            .then(|| {
                (
                    id,
                    cb.width,
                    cb.height,
                    forced_width,
                    crate::layout::flex::forced_height(ctx, id),
                )
            });
    let moved = |e: &MemoEntry| {
        let mut r = e.1.clone();
        r.fragment.rect.origin.y += y_in - e.0;
        r
    };
    if let Some(k) = &memo_key {
        if let Some(e) = ctx.cache.borrow().block_memo.get(k) {
            return moved(e);
        }
    }
    // A result from an earlier pass, for the same subtree and constraints.
    let kept_key = memo_key.and_then(|(_, w, h, fw, fh)| {
        let d = *ctx.cache.borrow().digests.get(id.index())?;
        Some((d, w, h, fw, fh))
    });
    if let Some(k) = &kept_key {
        let mut cache = ctx.cache.borrow_mut();
        let hit = match cache.kept.remove(k) {
            Some(e) => Some(e),
            None => cache.kept_next.get(k).cloned(),
        };
        if let Some(e) = hit {
            cache.kept_next.insert(*k, e.clone());
            cache.block_memo.insert(memo_key.unwrap(), e.clone());
            return moved(&e);
        }
    }
    let r = layout_block_box_uncached(ctx, id, cb, bfc, cb_origin, y_in, forced_width);
    if let Some(k) = memo_key {
        let e = std::rc::Rc::new((y_in, r.clone()));
        let mut cache = ctx.cache.borrow_mut();
        // Absolutely positioned boxes still to place refer to this pass's boxes.
        if let Some(kk) = kept_key.filter(|_| r.abs.is_empty()) {
            cache.kept_next.insert(kk, e.clone());
        }
        cache.block_memo.insert(k, e);
    }
    r
}

/// A kept layout result and the block position it was laid out at.
pub type MemoEntry = std::rc::Rc<(Au, BlockResult)>;

fn layout_block_box_uncached(
    ctx: &LayoutContext,
    id: BoxId,
    cb: &Cb,
    bfc: &mut Bfc,
    cb_origin: Point,
    y_in: Au,
    forced_width: Option<Au>,
) -> BlockResult {
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
                    let (ww, mml, mmr) = block_width_in(ctx, id, cb, avail, eh);
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
                let r = block_width_in(ctx, id, cb, cb.width, eh);
                w = r.0;
                ml = r.1;
                mr = r.2;
                x = ml;
            }
        }
    }
    let (mt, mb) = vertical_margins(s, cb.width);
    let own_height = match crate::layout::flex::forced_height(ctx, id) {
        Some(forced) => forced,
        None => resolve_height(s, cb.height, ev),
    };
    // `aspect-ratio` with `height: auto`: the width gives the height, which is
    // definite for percentage children. Content taller than it grows the box (the
    // automatic minimum size) unless the box is a scroll container.
    let ratio_h = if own_height.is_none() && s.height == Sizing::Auto {
        ratio_height(s, w, eh, ev)
    } else {
        None
    };
    let ratio_grows = ratio_h.is_some() && !b.is_scroll_container() && s.min_height == Sizing::Auto;
    let own_height = if ratio_grows {
        None
    } else {
        own_height.or(ratio_h)
    };
    let quirky_root = ctx.quirks
        && b.node
            .is_some_and(|n| ctx.doc.is(n, "html") || ctx.doc.is(n, "body"));
    let child_cb_height = own_height.or(ratio_h).or(if quirky_root {
        Some(ctx.viewport.height)
    } else {
        None
    });
    let top_adjoining = !is_bfc_root && bw.top.is_zero() && p.top.is_zero() && b.marker.is_none();
    let bottom_adjoining = !is_bfc_root
        && bw.bottom.is_zero()
        && p.bottom.is_zero()
        && s.height == Sizing::Auto
        && ratio_h.is_none()
        && min_height_is_zero(s);

    // Scrollbars reserve space; `auto` is decided after a first layout.
    let (bar_x, bar_y) = scroll::reserved_bars_in(ctx, s);
    let mut reserve_v = bar_y;
    let mut reserve_h = bar_x;
    let mut attempts = 0;
    let (contents, inner_bfc, content_w) = loop {
        let content_w = (w - reserve_v).max(Au::ZERO);
        let inner_cb = Cb {
            width: content_w,
            height: child_cb_height.map(|h| (h - reserve_h).max(Au::ZERO)),
        };
        let content_origin = Point {
            x: cb_origin.x + x + bw.left + p.left,
            y: cb_origin.y + y + bw.top + p.top,
        };
        let mut inner_bfc = if is_bfc_root { Some(Bfc::new()) } else { None };
        let contents = match inner_bfc.as_mut() {
            Some(inner) => {
                layout_contents(ctx, id, &inner_cb, inner, Point::default(), false, false)
            }
            None => layout_contents(
                ctx,
                id,
                &inner_cb,
                bfc,
                content_origin,
                top_adjoining,
                bottom_adjoining,
            ),
        };
        // Reserving a bar narrows the content, which can take the other bar away
        // again, so the decision is re-made until it settles (three passes at most).
        if attempts < 3 && b.is_scroll_container() {
            let mut ch = contents.height;
            if let Some(inner) = &inner_bfc {
                ch = ch.max(inner.float_bottom());
            }
            let h_now = own_height
                .map(|h| clamp_height(s, h, cb.height, ev))
                .unwrap_or(clamp_height(s, ch, cb.height, ev));
            let content_size = scroll::content_size(&contents.fragments, content_w, ch);
            // The visible size is the scrollport before any bar is taken out of it —
            // `w`, not the already narrowed `content_w`, or a second pass would
            // subtract the same gutter twice and keep asking for another bar.
            let (nx, ny) = scroll::auto_bars_in(
                ctx,
                s,
                content_size,
                Size {
                    width: w,
                    height: h_now,
                },
                bar_x,
                bar_y,
            );
            if nx != reserve_h || ny != reserve_v {
                reserve_h = nx;
                reserve_v = ny;
                attempts += 1;
                continue;
            }
        }
        break (contents, inner_bfc, content_w);
    };
    let mut contents = contents;
    if let Some(n) = s.line_clamp.filter(|_| {
        s.box_orient_vertical
            && !matches!(
                s.display,
                crate::style::Display::Flex | crate::style::Display::InlineFlex
            )
    }) {
        clamp_lines(ctx, s, &mut contents, content_w, n as usize);
    }
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
        None => (content_h + reserve_h).max(ratio_h.unwrap_or(Au::ZERO)),
    };
    if quirky_root && own_height.is_none() {
        // Quirks mode: `<html>` and `<body>` with `height: auto` stretch to the
        // viewport (Blink's `StretchesToViewport`), less their own margins and
        // edges and, for the body, the root's margins and edges.
        let mut stretched = ctx.viewport.height - mt - mb - ev;
        if !b.is_root {
            if let Some(root) = ctx.tree.root {
                let rs = ctx.style(root);
                let (rmt, rmb) = vertical_margins(rs, ctx.viewport.width);
                let rp = padding_edges(rs, ctx.viewport.width);
                stretched -= rmt + rmb + rp.vertical() + rs.used_border_widths().vertical();
            }
        }
        h = h.max(stretched);
    }
    h = clamp_height(s, h, cb.height, ev);
    let frag_w = w + eh;
    let frag_h = h + ev;
    let rect = Rect::new(x, y, frag_w, frag_h);
    // A `<button>`'s contents sit in an anonymous box that is centred vertically in
    // the button's content box (HTML rendering §15.5.4, as every browser does): with
    // an explicit `height` taller than the label, the label is in the middle, not at
    // the top. Content that overflows stays at the top.
    let button_shift =
        if b.control == Some(crate::layout::fragment::ControlKind::Button) && h > content_h {
            (h - content_h) / 2
        } else {
            Au::ZERO
        };
    let baseline = contents
        .first_baseline
        .map(|bl| bl + bw.top + p.top + button_shift);
    let last_baseline = contents
        .last_baseline
        .map(|bl| bl + bw.top + p.top + button_shift);
    let mut frag = Fragment::new(
        FragmentKind::Box {
            source: b.source,
            padding: p,
            border: bw,
            replaced: b.control.map(Replaced::Control),
            scroll: None,
            baseline,
        },
        rect,
    );
    let cx = bw.left + p.left;
    let cy = bw.top + p.top + button_shift;
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
    let unresolved = if s.is_positioned() || b.is_root {
        resolve_absolutes(ctx, &mut frag, abs)
    } else {
        abs
    };
    finish_fragment(ctx, id, &mut frag);
    scroll::attach_scroll_info(ctx, id, &mut frag, content_w, h, reserve_h, reserve_v);
    // Self-collapsing (§8.3.1): no content, no edges, no height of its own. A box
    // sized only by `aspect-ratio` has `own_height: None` — the ratio height is kept
    // aside so content may grow past it — but `h` is the ratio's, so the box is as
    // tall as any other and must not collapse through, or the next block is laid out
    // on top of it.
    let empty_box = contents.empty
        && h <= Au::ZERO
        && own_height.is_none_or(|h| h <= Au::ZERO)
        && ev.is_zero()
        && b.marker.is_none()
        && !is_bfc_root
        && min_height_is_zero(s);
    let bottom_margins = if empty_box {
        MarginSet::of(mt)
            .union(MarginSet::of(mb))
            .union(contents.pending_bottom)
    } else {
        bottom_margins
    };
    // A forced width (flex and grid items, cells, floats, absolutes) means the
    // caller owns the horizontal margins and records them itself.
    if forced_width.is_none() {
        frag.used_margin = Some(Edges {
            top: mt,
            right: mr,
            bottom: mb,
            left: ml,
        });
    }
    BlockResult {
        fragment: frag,
        margin: Edges {
            top: mt,
            right: mr,
            bottom: mb,
            left: ml,
        },
        bottom_margins,
        abs: unresolved,
        first_baseline: baseline,
        last_baseline,
    }
}

/// A block-level replaced element (§10.3.4).
fn layout_block_replaced(
    ctx: &LayoutContext,
    id: BoxId,
    rb: &ReplacedBox,
    cb: &Cb,
    y: Au,
) -> BlockResult {
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
    BlockResult {
        fragment: frag,
        margin: Edges {
            top: mt,
            right: mr,
            bottom: mb,
            left: ml,
        },
        bottom_margins: MarginSet::of(mb),
        abs: Vec::new(),
        first_baseline: None,
        last_baseline: None,
    }
}

/// The fragment of a replaced box with this content size, at the origin.
pub fn replaced_fragment(
    ctx: &LayoutContext,
    id: BoxId,
    rb: &ReplacedBox,
    size: Size,
    p: Edges,
    bw: Edges,
) -> Fragment {
    let b = &ctx.tree[id];
    let h = size.height + p.vertical() + bw.vertical();
    let w = size.width + p.horizontal() + bw.horizontal();
    let baseline = match &rb.replaced {
        // Text-like controls sit on the text baseline; others on their bottom edge.
        Replaced::Control(
            crate::layout::fragment::ControlKind::TextInput
            | crate::layout::fragment::ControlKind::Password
            | crate::layout::fragment::ControlKind::Button
            | crate::layout::fragment::ControlKind::Submit
            | crate::layout::fragment::ControlKind::File,
        ) => {
            let fm = text::font_metrics(&b.style.font);
            let lh = size.height;
            let half = text::half_leading(lh, fm.content_height());
            Some(bw.top + p.top + half + fm.ascent)
        }
        // A menu list's text sits a pixel of internal padding below its padding
        // edge, on the font's rounded ascent (Chromium: 14 px below the border box
        // top for 13.33px Arimo, 20 for 20px Arimo).
        Replaced::Control(crate::layout::fragment::ControlKind::Select) => {
            let fm = text::font_metrics(&b.style.font);
            let ascent = Au::from_px_i32((fm.ascent.0 + 32).div_euclid(64));
            Some(bw.top + p.top + Au::from_px_i32(1) + ascent)
        }
        _ => Some(h),
    };
    let mut f = Fragment::new(
        FragmentKind::Box {
            source: b.source,
            padding: p,
            border: bw,
            replaced: Some(rb.replaced.clone()),
            scroll: None,
            baseline,
        },
        Rect::new(Au::ZERO, Au::ZERO, w, h),
    );
    finish_fragment(ctx, id, &mut f);
    if let (Replaced::Placeholder(tag), Some(node)) = (&rb.replaced, b.node) {
        if tag == "svg" && crate::svg::is_svg(ctx.doc, node) {
            svg_descendants(
                ctx,
                node,
                &mut f,
                size,
                Point {
                    x: bw.left + p.left,
                    y: bw.top + p.top,
                },
            );
        }
    }
    f
}

/// Hangs the bounding boxes of an inline `<svg>`'s descendant elements and text
/// on its fragment, as fragments that are laid out but not painted, so client
/// rects and hit-free geometry queries find them (`crate::svg` draws the content).
fn svg_descendants(ctx: &LayoutContext, svg: NodeId, f: &mut Fragment, size: Size, content: Point) {
    let built = crate::svg::build(
        ctx.doc,
        ctx.styles,
        svg,
        size.width.0 as f64 / 64.0,
        size.height.0 as f64 / 64.0,
    );
    let au = |v: f64| Au((v * 64.0).round() as i32);
    let rect = |b: crate::svg::BoxF| {
        Rect::new(
            content.x + au(b.x),
            content.y + au(b.y),
            au(b.w.max(0.0)),
            au(b.h.max(0.0)),
        )
    };
    for (node, b) in &built.boxes {
        let mut c = Fragment::new(
            FragmentKind::Box {
                source: StyleSource::Element(*node),
                padding: Edges::default(),
                border: Edges::default(),
                replaced: None,
                scroll: None,
                baseline: None,
            },
            rect(*b),
        );
        c.hidden_for_paint = true;
        f.children.push(c);
    }
    for (node, b) in &built.text_boxes {
        let Some(parent) = ctx.doc.parent(*node) else {
            continue;
        };
        let text = ctx.doc.text(*node).unwrap_or("").to_owned();
        let len = text.len();
        let mut c = Fragment::new(
            FragmentKind::Text {
                source: StyleSource::Element(parent),
                text,
                node: Some(*node),
                range: (0, len),
                baseline: Au::ZERO,
                ellipsis: false,
            },
            rect(*b),
        );
        c.hidden_for_paint = true;
        f.children.push(c);
    }
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
    let half = text::half_leading(lh, fm.content_height());
    let ascent = half + fm.ascent;
    let w = text::measure(&s.font, &txt, s.letter_spacing, s.word_spacing);
    let y = first_baseline.map(|bl| bl - ascent).unwrap_or(Au::ZERO);
    let mut f = Fragment::new(
        FragmentKind::Box {
            source: b.source,
            padding: Edges::ZERO,
            border: Edges::ZERO,
            replaced: Some(Replaced::Marker(txt)),
            scroll: None,
            baseline: Some(ascent),
        },
        Rect::new(Au::ZERO, y, w, lh),
    );
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
        Size {
            width: self.fragment.rect.size.width + self.margin.horizontal(),
            height: self.fragment.rect.size.height + self.margin.vertical(),
        }
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
    PreparedFloat {
        fragment,
        abs,
        margin: Edges {
            top: mt,
            right: mr,
            bottom: mb,
            left: ml,
        },
    }
}

/// Places a prepared float (§9.5.1) no higher than `ceiling` (content-box y of the
/// containing block). Returns the fragment positioned in the containing block's
/// content coordinates and its unresolved absolute requests.
pub fn place_float(
    ctx: &LayoutContext,
    id: BoxId,
    mut pf: PreparedFloat,
    cb: &Cb,
    bfc: &mut Bfc,
    cb_origin: Point,
    ceiling: Au,
) -> (Fragment, Vec<AbsRequest>) {
    let s = ctx.style(id);
    let size = pf.margin_size();
    let side = if s.float == Float::Right {
        Float::Right
    } else {
        Float::Left
    };
    let flow_y = cb_origin.y + ceiling;
    let mut ceil = flow_y;
    if s.clear != Clear::None {
        if let Some(cy) = bfc.clear_y(s.clear) {
            ceil = ceil.max(cy);
        }
    }
    let pos = bfc.place_from(
        side,
        size,
        ceil,
        flow_y,
        cb_origin.x,
        cb_origin.x + cb.width,
    );
    let off = relative_offset(s, cb);
    pf.fragment.rect.origin = Point {
        x: pos.x - cb_origin.x + pf.margin.left + off.x,
        y: pos.y - cb_origin.y + pf.margin.top + off.y,
    };
    (pf.fragment, pf.abs)
}

/// Lays out a float and places it. Returns the fragment positioned in the containing
/// block's content coordinates and its unresolved absolute requests.
pub fn layout_float(
    ctx: &LayoutContext,
    id: BoxId,
    cb: &Cb,
    bfc: &mut Bfc,
    cb_origin: Point,
    ceiling: Au,
) -> (Fragment, Vec<AbsRequest>) {
    let pf = prepare_float(ctx, id, cb);
    place_float(ctx, id, pf, cb, bfc, cb_origin, ceiling)
}

/// Lays out a box that establishes its own BFC and sizes itself by shrink-to-fit
/// (floats, inline-blocks, absolutes with auto width): the fragment is at the
/// origin. `avail` is the width available to its border box.
pub fn layout_standalone(
    ctx: &LayoutContext,
    id: BoxId,
    cb: &Cb,
    avail: Au,
    eh: Au,
) -> (Fragment, Vec<AbsRequest>) {
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
            let r = table::layout_wrapper(
                ctx,
                id,
                cb,
                &mut empty,
                Point::default(),
                Au::ZERO,
                Some(avail),
            );
            let mut f = r.fragment;
            f.rect.origin = Point::default();
            (f, r.abs)
        }
        _ => {
            let specified =
                resolve_size(s.width, Some(cb.width), eh, s.box_sizing).or_else(|| match s.width {
                    Sizing::MinContent => Some((intrinsic::min_max(ctx, id).0 - eh).max(Au::ZERO)),
                    Sizing::MaxContent => Some((intrinsic::min_max(ctx, id).1 - eh).max(Au::ZERO)),
                    _ => None,
                });
            let w = match specified {
                Some(w) => w,
                None => (shrink_to_fit(ctx, id, avail.max(Au::ZERO)) - eh).max(Au::ZERO),
            };
            let w = clamp_size(
                w,
                s.min_width,
                s.max_width,
                Some(cb.width),
                eh,
                s.box_sizing,
            );
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
pub fn resolve_absolutes(
    ctx: &LayoutContext,
    cbf: &mut Fragment,
    reqs: Vec<AbsRequest>,
) -> Vec<AbsRequest> {
    let mut rest = Vec::new();
    let has_transform = match cbf.source() {
        Some(src) if !src.is_anonymous() => ctx
            .tree
            .box_of(src.node())
            .is_some_and(|b| !ctx.tree[b].style.transform.is_empty()),
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
    let cb = Cb {
        width: cbw,
        height: Some(cbh),
    };
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
    let css_w = replaced_size_v
        .map(|z| z.width)
        .or_else(|| resolve_size(s.width, Some(cbw), eh, s.box_sizing));
    let css_h = replaced_size_v
        .map(|z| z.height)
        .or_else(|| resolve_size(s.height, Some(cbh), ev, s.box_sizing));

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
            let x = if !ml_auto && !mr_auto && rtl {
                cbw - r - w - eh - mr
            } else {
                l + ml
            };
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
        (None, Some(w), None) => (
            w,
            if rtl {
                cbw - static_x - w - eh - mr
            } else {
                static_x + ml
            },
        ),
        (None, Some(w), Some(r)) => (w, cbw - r - w - eh - mr),
        (Some(l), None, Some(r)) => ((cbw - l - r - eh - ml - mr).max(Au::ZERO), l + ml),
        (Some(l), Some(w), None) => (w, l + ml),
        (None, None, None) => (Au::ZERO, ml),
    };
    let clamped = clamp_size(w, s.min_width, s.max_width, Some(cbw), eh, s.box_sizing);
    let x = if clamped != w && left.is_none() && right.is_some() {
        x + (w - clamped)
    } else {
        x
    };
    w = clamped;

    // A non-replaced box with `height: auto` whose `top` and `bottom` are both set has
    // a definite height (§10.6.4 rule 5): its contents are laid out at that height, so
    // flex and grid items, percentage heights and its own absolutely positioned
    // descendants see it rather than the height of the content.
    let inset_h = match (s.inset.top.resolve(cbh), s.inset.bottom.resolve(cbh)) {
        (Some(t), Some(bo)) if css_h.is_none() && replaced_size_v.is_none() => {
            let mt = margin_or_zero(s.margin.top, cbw);
            let mb = margin_or_zero(s.margin.bottom, cbw);
            Some(clamp_height(
                s,
                (cbh - t - bo - ev - mt - mb).max(Au::ZERO),
                Some(cbh),
                ev,
            ))
        }
        _ => None,
    };

    // Lay out the contents with this width to learn the auto height.
    let mut frag = match &b.kind {
        BoxKind::Replaced(rb) => replaced_fragment(
            ctx,
            id,
            rb,
            Size {
                width: w,
                height: css_h.unwrap_or(Au::ZERO),
            },
            p,
            bw,
        ),
        BoxKind::TableWrapper => {
            let mut empty = Bfc::new();
            table::layout_wrapper(
                ctx,
                id,
                &cb,
                &mut empty,
                Point::default(),
                Au::ZERO,
                Some(w + eh),
            )
            .fragment
        }
        _ => {
            let mut empty = Bfc::new();
            if let Some(h) = inset_h {
                ctx.cache.borrow_mut().forced_height.insert(id, Some(h));
            }
            let f = layout_block_box(
                ctx,
                id,
                &cb,
                &mut empty,
                Point::default(),
                Au::ZERO,
                Some(w),
            )
            .fragment;
            if inset_h.is_some() {
                ctx.cache.borrow_mut().forced_height.remove(&id);
            }
            f
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
    // What `getComputedStyle` reports for the margins: `auto` resolved as above.
    let used_margin = Edges {
        top: mt,
        right: mr,
        bottom: mb,
        left: ml,
    };
    let h = clamp_height(s, h, Some(cbh), ev);
    if h + ev != frag.rect.size.height {
        // The content keeps its layout; the box is simply taller or shorter.
        frag.rect.size.height = h + ev;
        compute_overflow(&mut frag, b.is_scroll_container());
    }
    frag.rect.origin = Point {
        x: bl + x,
        y: bt + y,
    };
    frag.is_positioned = true;
    frag.used_margin = Some(used_margin);
    frag
}
