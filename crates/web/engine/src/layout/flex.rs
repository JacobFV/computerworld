//! CSS Flexible Box Layout Level 1 (css-flexbox-1): flex container box generation
//! (§4), the line-length and flexible-length algorithm (§9.2–9.7), cross sizing and
//! alignment (§9.4–9.6, §8), intrinsic sizes of flex containers (§9.9), and the
//! static position of absolutely positioned children (§4.1).
//!
//! How it plugs in: a flex container is a block container box whose `display` is
//! `flex` or `inline-flex`; `block::layout_block_box` sizes it, reserves scrollbars,
//! resolves its absolutes and attaches its fragment exactly as for any block, and asks
//! `layout_contents` here for the children. Items are laid out through
//! `block::layout_block_box` with a forced content width and (through
//! `LayoutCache::forced_height`) a forced content height, so the rest of the engine
//! (percent resolution, scroll containers, nested flex, tables, replaced elements)
//! works unchanged inside items. Everything is `Au`; the ratios of the flexible-length
//! loop and the alignment distributions are computed in `i128` and rounded once, with
//! the sum of the shares always equal to the space distributed.
//!
//! Out of scope, documented here: `writing-mode` other than horizontal (the main axis
//! of `row` is horizontal; `direction: rtl` reverses it), `visibility: collapse`
//! items (laid out as `hidden`, no strut), the transferred-size suggestion of §4.5 for
//! aspect-ratio items (their `aspect-ratio` height comes from `block::ratio_height`
//! when the item is laid out, which covers a column item of definite width), and
//! percentage `min-height`/`max-height` of a column container
//! whose own height is indefinite (they are ignored there, lengths apply).

use std::rc::Rc;

use crate::dom::{Document, NodeId};
use crate::geom::{Au, Edges, Point, Size};
use crate::layout::block::{self, AbsRequest, Bfc, Cb, ContentsResult};
use crate::layout::boxes::{BoxId, BoxKind, LayoutBox, Level, ReplacedBox};
use crate::layout::fragment::{Fragment, FragmentKind, StyleSource};
use crate::layout::{intrinsic, table, text, LayoutContext};
use crate::style::{
    AlignContent, AlignItems, AlignSelf, BoxSizing, ComputedStyle, Direction, Display,
    FlexDirection, FlexWrap, JustifyContent, LengthPercentage, LengthPercentageAuto, Overflow,
    Position, Sizing, StyleSet,
};

// Box generation (§4).

/// A block container box that lays its children out as flex items.
pub fn is_flex_container(b: &LayoutBox) -> bool {
    matches!(b.style.display, Display::Flex | Display::InlineFlex)
        && matches!(b.kind, BoxKind::Block | BoxKind::InlineBlock)
}

/// Whether the element's box is a flex or grid item: its parent (through
/// `display: contents` ancestors) is a flex or grid container.
pub fn is_flex_or_grid_item(doc: &Document, styles: &StyleSet, node: NodeId) -> bool {
    let mut p = doc.parent(node);
    while let Some(par) = p {
        match styles.get(par).map(|s| s.display) {
            Some(Display::Contents) => p = doc.parent(par),
            Some(Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid) => {
                return true
            }
            _ => return false,
        }
    }
    false
}

/// Blockification of a flex or grid item's `display` (css-display §2.7): inline-level
/// values become their block-level counterparts and table-internal values `block`.
pub fn blockify_item_display(d: Display) -> Display {
    match d {
        Display::Inline
        | Display::InlineBlock
        | Display::TableRowGroup
        | Display::TableHeaderGroup
        | Display::TableFooterGroup
        | Display::TableRow
        | Display::TableCell
        | Display::TableColumnGroup
        | Display::TableColumn
        | Display::TableCaption => Display::Block,
        Display::InlineFlex => Display::Flex,
        Display::InlineGrid => Display::Grid,
        Display::InlineTable => Display::Table,
        d => d,
    }
}

/// Turns the children boxes of a flex container into flex items (§4): runs of
/// inline-level boxes (text) are wrapped in anonymous block items, runs of only
/// collapsible white space are dropped, every in-flow box is marked an item, and
/// absolutely positioned boxes stay as children without being items.
pub fn wrap_flex_items(
    boxes: &mut Vec<LayoutBox>,
    container: BoxId,
    kids: Vec<BoxId>,
) -> Vec<BoxId> {
    let parent_style = boxes[container.index()].style.clone();
    let anon = StyleSource::Anonymous(boxes[container.index()].source.node());
    let mut out = Vec::new();
    let mut run: Vec<BoxId> = Vec::new();
    let flush = |boxes: &mut Vec<LayoutBox>, run: &mut Vec<BoxId>, out: &mut Vec<BoxId>| {
        if run.is_empty() {
            return;
        }
        let has_content = run.iter().any(|k| {
            let b = &boxes[k.index()];
            match &b.kind {
                BoxKind::Text(t) => !text::is_collapsible_whitespace(&t.text, b.style.white_space),
                BoxKind::Wbr | BoxKind::Br(_) => false,
                _ => true,
            }
        });
        if has_content {
            let id = BoxId(boxes.len() as u32);
            boxes.push(LayoutBox {
                kind: BoxKind::Block,
                style: crate::layout::boxes::anon_style(&parent_style, Display::Block),
                source: anon,
                node: None,
                level: Level::Block,
                children: std::mem::take(run),
                inline_children: true,
                marker: None,
                control: None,
                is_root: false,
                split_first: true,
                split_last: true,
                is_item: true,
                is_fieldset: false,
            });
            out.push(id);
        } else {
            run.clear();
        }
    };
    for k in kids {
        let (inline, abs) = {
            let b = &boxes[k.index()];
            (b.level == Level::Inline, b.is_abs())
        };
        if inline && !abs {
            run.push(k);
        } else {
            flush(boxes, &mut run, &mut out);
            boxes[k.index()].is_item = !abs;
            out.push(k);
        }
    }
    flush(boxes, &mut run, &mut out);
    out
}

/// The content height imposed on a box by the flex (or grid) algorithm, if any:
/// `Some(None)` means "lay out as if `height: auto`".
pub fn forced_height(ctx: &LayoutContext, id: BoxId) -> Option<Option<Au>> {
    ctx.cache.borrow().forced_height.get(&id).copied()
}

// Arithmetic.

/// `a * num / den` in `i128`, rounded half away from zero; zero when `den` is zero.
fn mul_div(a: Au, num: i128, den: i128) -> Au {
    if den == 0 {
        return Au::ZERO;
    }
    let v = a.0 as i128 * num;
    let r = if (v >= 0) == (den > 0) {
        (v + den.abs() / 2) / den
    } else {
        (v - den.abs() / 2) / den
    };
    Au(r.clamp(Au::MIN.0 as i128, Au::MAX.0 as i128) as i32)
}

/// A length-only sizing value as a content-box length (percentages have no base here).
fn length_size(v: Sizing, edges: Au, bs: BoxSizing) -> Option<Au> {
    let l = match v {
        Sizing::Set(LengthPercentage::Length(l)) => l,
        Sizing::Set(v) if !v.has_percent() => v.resolve(Au::ZERO),
        _ => return None,
    };
    Some(match bs {
        BoxSizing::ContentBox => l,
        BoxSizing::BorderBox => (l - edges).max(Au::ZERO),
    })
}

// The algorithm.

#[derive(Clone, Copy, Debug)]
struct Axes {
    /// The main axis is horizontal.
    row: bool,
    /// `row-reverse` or `column-reverse`.
    dir_rev: bool,
    /// `wrap-reverse`.
    wrap_rev: bool,
    /// Physical: main-start is the right/bottom edge.
    main_rev: bool,
    /// Physical: cross-start is the bottom/right edge.
    cross_rev: bool,
    multi: bool,
}

/// How an item's height is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum H {
    /// The item's own `height` property.
    Auto,
    /// As if `height: auto`, for content-based sizes.
    Content,
    /// A forced content-box height.
    Forced(Au),
}

/// Flex-relative positional alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pos {
    Start,
    End,
    Center,
    Between,
    Around,
    Evenly,
    Baseline,
}

struct Item {
    id: BoxId,
    style: Rc<ComputedStyle>,
    order: i32,
    /// Resolved margins, `auto` as zero until distributed.
    margin: Edges,
    /// `auto` margins: top, right, bottom, left.
    auto: [bool; 4],
    /// Padding plus border.
    pb: Edges,
    replaced: bool,
    scroll: bool,
    grow: i32,
    shrink: i32,
    align: AlignSelf,
    base: Au,
    hyp_main: Au,
    min_main: Au,
    max_main: Option<Au>,
    target: Au,
    frozen: bool,
    violation: Au,
    hyp_cross: Au,
    cross: Au,
    laid: Option<(Au, H)>,
    fragment: Option<Fragment>,
    abs: Vec<AbsRequest>,
    /// First baseline from the border-box top, when the content has one.
    baseline: Option<Au>,
    /// Margin-box start offsets in flex-relative coordinates.
    main_pos: Au,
    cross_pos: Au,
    /// The placed border-box top, relative to the container's content box.
    frag_y: Au,
}

impl Item {
    fn new(ctx: &LayoutContext, id: BoxId, cb: &Cb, container: &ComputedStyle) -> Item {
        let b = &ctx.tree[id];
        let s = b.style.clone();
        let p = block::padding_edges(&s, cb.width);
        let bw = s.used_border_widths();
        let m = |v: LengthPercentageAuto| v.resolve(cb.width).unwrap_or(Au::ZERO);
        let align = match s.align_self {
            AlignSelf::Auto => match container.align_items {
                AlignItems::Stretch => AlignSelf::Stretch,
                AlignItems::FlexStart => AlignSelf::FlexStart,
                AlignItems::FlexEnd => AlignSelf::FlexEnd,
                AlignItems::Center => AlignSelf::Center,
                AlignItems::Baseline => AlignSelf::Baseline,
                AlignItems::Start | AlignItems::SelfStart => AlignSelf::Start,
                AlignItems::End | AlignItems::SelfEnd => AlignSelf::End,
            },
            v => v,
        };
        let is_scroll =
            |o: Overflow| matches!(o, Overflow::Hidden | Overflow::Scroll | Overflow::Auto);
        Item {
            id,
            order: s.order,
            margin: Edges {
                top: m(s.margin.top),
                right: m(s.margin.right),
                bottom: m(s.margin.bottom),
                left: m(s.margin.left),
            },
            auto: [
                s.margin.top.is_auto(),
                s.margin.right.is_auto(),
                s.margin.bottom.is_auto(),
                s.margin.left.is_auto(),
            ],
            pb: p + bw,
            replaced: matches!(b.kind, BoxKind::Replaced(_)),
            scroll: is_scroll(s.overflow_x) || is_scroll(s.overflow_y),
            grow: s.flex_grow.max(0),
            shrink: s.flex_shrink.max(0),
            align,
            style: s,
            base: Au::ZERO,
            hyp_main: Au::ZERO,
            min_main: Au::ZERO,
            max_main: None,
            target: Au::ZERO,
            frozen: false,
            violation: Au::ZERO,
            hyp_cross: Au::ZERO,
            cross: Au::ZERO,
            laid: None,
            fragment: None,
            abs: Vec::new(),
            baseline: None,
            main_pos: Au::ZERO,
            cross_pos: Au::ZERO,
            frag_y: Au::ZERO,
        }
    }
    fn edges_main(&self, a: Axes) -> Au {
        if a.row {
            self.pb.horizontal()
        } else {
            self.pb.vertical()
        }
    }
    fn edges_cross(&self, a: Axes) -> Au {
        if a.row {
            self.pb.vertical()
        } else {
            self.pb.horizontal()
        }
    }
    fn margin_main(&self, a: Axes) -> Au {
        if a.row {
            self.margin.horizontal()
        } else {
            self.margin.vertical()
        }
    }
    fn margin_cross(&self, a: Axes) -> Au {
        if a.row {
            self.margin.vertical()
        } else {
            self.margin.horizontal()
        }
    }
    fn outer_main(&self, a: Axes, m: Au) -> Au {
        m + self.edges_main(a) + self.margin_main(a)
    }
    fn outer_cross(&self, a: Axes, c: Au) -> Au {
        c + self.edges_cross(a) + self.margin_cross(a)
    }
    /// `auto` margins in the main axis, physical order (left, right) or (top, bottom).
    fn auto_main(&self, a: Axes) -> (bool, bool) {
        if a.row {
            (self.auto[3], self.auto[1])
        } else {
            (self.auto[0], self.auto[2])
        }
    }
    fn auto_cross(&self, a: Axes) -> (bool, bool) {
        if a.row {
            (self.auto[0], self.auto[2])
        } else {
            (self.auto[3], self.auto[1])
        }
    }
    fn has_auto_cross(&self, a: Axes) -> bool {
        let (s, e) = self.auto_cross(a);
        s || e
    }
    fn set_main_margins(&mut self, a: Axes, start: Au, end: Au) {
        if a.row {
            self.margin.left = start;
            self.margin.right = end;
        } else {
            self.margin.top = start;
            self.margin.bottom = end;
        }
    }
    fn set_cross_margins(&mut self, a: Axes, start: Au, end: Au) {
        if a.row {
            self.margin.top = start;
            self.margin.bottom = end;
        } else {
            self.margin.left = start;
            self.margin.right = end;
        }
    }
    fn frag_height(&self) -> Au {
        self.fragment
            .as_ref()
            .map(|f| f.rect.size.height)
            .unwrap_or(Au::ZERO)
    }
    fn content_height(&self) -> Au {
        (self.frag_height() - self.pb.vertical()).max(Au::ZERO)
    }
    /// The alignment baseline from the border-box top: the first baseline, or the
    /// border box's bottom edge when there is none (css-align §9.1).
    fn baseline_or_synth(&self) -> Au {
        self.baseline.unwrap_or_else(|| self.frag_height())
    }
    /// The cross-axis size property is `auto` (stretch may apply).
    fn cross_is_auto(&self, a: Axes) -> bool {
        let v = if a.row {
            self.style.height
        } else {
            self.style.width
        };
        matches!(v, Sizing::Auto)
    }
}

/// The used height of a replaced item at a content width, ignoring `height` when
/// `content` is set (the aspect ratio transfers the width when there is one).
fn replaced_height_for_width(
    ctx: &LayoutContext,
    it: &Item,
    rb: &ReplacedBox,
    cb: &Cb,
    width: Au,
    content: bool,
) -> Au {
    let s = &it.style;
    let ev = it.pb.vertical();
    let css_h = if content {
        None
    } else {
        block::resolve_size(s.height, cb.height, ev, s.box_sizing)
    };
    let h = match css_h {
        Some(h) => h,
        None => match rb.intrinsic {
            Some(z)
                if block::has_natural_ratio(rb)
                    && z.width > Au::ZERO
                    && rb.attr_height.is_none() =>
            {
                width.scale(z.height.0, z.width.0)
            }
            _ => block::replaced_size(ctx, it.id, rb, cb).height,
        },
    };
    block::clamp_size(h, s.min_height, s.max_height, cb.height, ev, s.box_sizing)
}

/// Lays out an item with a content width and a height rule, keeping the result when
/// the same layout was already done.
fn layout_item(ctx: &LayoutContext, cb: &Cb, it: &mut Item, width: Au, h: H) {
    if it.laid == Some((width, h)) {
        return;
    }
    if let (H::Forced(fh), Some((w0, h0))) = (h, it.laid) {
        if w0 == width
            && h0 != H::Forced(fh)
            && !matches!(h0, H::Forced(_))
            && it.content_height() == fh
        {
            it.laid = Some((width, h));
            return;
        }
    }
    let b = &ctx.tree[it.id];
    let s = it.style.clone();
    let p = block::padding_edges(&s, cb.width);
    let bw = s.used_border_widths();
    let eh = p.horizontal() + bw.horizontal();
    let ev = p.vertical() + bw.vertical();
    let (mut frag, abs, baseline) = match &b.kind {
        BoxKind::Replaced(rb) => {
            let hh = match h {
                H::Forced(v) => v,
                other => replaced_height_for_width(ctx, it, rb, cb, width, other == H::Content),
            };
            let f = block::replaced_fragment(ctx, it.id, rb, Size { width, height: hh }, p, bw);
            let bl = match &f.kind {
                FragmentKind::Box { baseline, .. } => *baseline,
                _ => None,
            };
            (f, Vec::new(), bl)
        }
        BoxKind::TableWrapper => {
            let mut bfc = Bfc::new();
            let r = table::layout_wrapper(
                ctx,
                it.id,
                cb,
                &mut bfc,
                Point::default(),
                Au::ZERO,
                Some(width + eh),
            );
            let mut f = r.fragment;
            if let H::Forced(v) = h {
                f.rect.size.height = v + ev;
                block::compute_overflow(&mut f, false);
            }
            (f, r.abs, r.first_baseline)
        }
        _ => {
            let forced = match h {
                H::Auto => None,
                H::Content => Some(None),
                H::Forced(v) => Some(Some(v)),
            };
            if let Some(f) = forced {
                ctx.cache.borrow_mut().forced_height.insert(it.id, f);
            }
            let mut bfc = Bfc::new();
            let r = block::layout_block_box(
                ctx,
                it.id,
                cb,
                &mut bfc,
                Point::default(),
                Au::ZERO,
                Some(width),
            );
            ctx.cache.borrow_mut().forced_height.remove(&it.id);
            (r.fragment, r.abs, r.first_baseline)
        }
    };
    frag.rect.origin = Point::default();
    // `float` does not apply to flex items (§3).
    frag.is_float = false;
    if s.establishes_stacking_context(true) {
        frag.establishes_stacking_context = true;
    }
    it.fragment = Some(frag);
    it.abs = abs;
    it.baseline = baseline;
    it.laid = Some((width, h));
}

/// The cross-axis (width) content size of an item in a column container: its
/// `width`, else stretched to `stretch_to` (the line's cross size) when it stretches,
/// else fit-content in the container's width.
fn column_width(ctx: &LayoutContext, cb: &Cb, it: &Item, a: Axes, stretch_to: Option<Au>) -> Au {
    let s = &it.style;
    let eh = it.pb.horizontal();
    let bs = s.box_sizing;
    let mh = it.margin_cross(a);
    let v = match block::resolve_size(s.width, Some(cb.width), eh, bs) {
        Some(w) => w,
        None => match s.width {
            Sizing::MinContent => (intrinsic::min_max(ctx, it.id).0 - eh).max(Au::ZERO),
            Sizing::MaxContent => (intrinsic::min_max(ctx, it.id).1 - eh).max(Au::ZERO),
            _ => {
                let b = &ctx.tree[it.id];
                let stretch = it.align == AlignSelf::Stretch && !it.has_auto_cross(a);
                match (stretch_to, &b.kind) {
                    (Some(line), _) if stretch => (line - mh - eh).max(Au::ZERO),
                    (_, BoxKind::Replaced(rb)) => block::replaced_size(ctx, it.id, rb, cb).width,
                    _ => (block::shrink_to_fit(ctx, it.id, (cb.width - mh).max(Au::ZERO)) - eh)
                        .max(Au::ZERO),
                }
            }
        },
    };
    block::clamp_size(v, s.min_width, s.max_width, Some(cb.width), eh, bs)
}

/// Flex base size, automatic minimum, max and hypothetical main size (§9.2 step 3,
/// §4.5).
fn compute_main_sizes(ctx: &LayoutContext, cb: &Cb, it: &mut Item, a: Axes, main_def: Option<Au>) {
    let s = it.style.clone();
    let bs = s.box_sizing;
    let em = it.edges_main(a);
    let main_prop = if a.row { s.width } else { s.height };
    let basis = if s.flex_basis == Sizing::Auto {
        main_prop
    } else {
        s.flex_basis
    };
    let max_prop = if a.row { s.max_width } else { s.max_height };
    it.max_main = block::resolve_size(max_prop, main_def, em, bs);
    let definite = match basis {
        Sizing::Set(lp) => lp.maybe_resolve(main_def).map(|v| match bs {
            BoxSizing::ContentBox => v,
            BoxSizing::BorderBox => (v - em).max(Au::ZERO),
        }),
        _ => None,
    };
    let stretch_to = if a.multi { None } else { Some(cb.width) };
    let column_w = if a.row {
        Au::ZERO
    } else {
        column_width(ctx, cb, it, a, stretch_to)
    };
    it.base = match definite {
        Some(v) => v.max(Au::ZERO),
        None => {
            if a.row {
                // Content-based: the content's own sizes, not `width` (which is either
                // `auto` here or overridden by `flex-basis: content`).
                let (mn, mx) = if it.replaced {
                    let (mn, mx) = intrinsic::min_max(ctx, it.id);
                    ((mn - em).max(Au::ZERO), (mx - em).max(Au::ZERO))
                } else {
                    intrinsic::content_min_max(ctx, it.id)
                };
                match basis {
                    Sizing::MinContent => mn,
                    Sizing::FitContent => mx.min((cb.width - it.margin_main(a) - em).max(mn)),
                    _ => mx,
                }
            } else {
                layout_item(ctx, cb, it, column_w, H::Content);
                it.content_height()
            }
        }
    };
    let min_prop = if a.row { s.min_width } else { s.min_height };
    it.min_main = match block::resolve_size(min_prop, main_def, em, bs) {
        Some(v) => v,
        None if matches!(min_prop, Sizing::Auto) && !it.scroll => {
            // The automatic minimum size (§4.5): the smaller of the content size
            // suggestion and the specified size suggestion, clamped by the max.
            let content = if a.row {
                if it.replaced {
                    (intrinsic::min_max(ctx, it.id).0 - em).max(Au::ZERO)
                } else {
                    intrinsic::content_min_max(ctx, it.id).0
                }
            } else {
                layout_item(ctx, cb, it, column_w, H::Content);
                it.content_height()
            };
            let specified = block::resolve_size(main_prop, main_def, em, bs);
            let mut v = match specified {
                Some(sp) => sp.min(content),
                None => content,
            };
            if let Some(mx) = it.max_main {
                v = v.min(mx);
            }
            v.max(Au::ZERO)
        }
        None => Au::ZERO,
    };
    it.hyp_main = clamp_main(it, it.base);
}

fn clamp_main(it: &Item, v: Au) -> Au {
    let mut r = v;
    if let Some(mx) = it.max_main {
        r = r.min(mx);
    }
    r.max(it.min_main).max(Au::ZERO)
}

/// Collects items into flex lines (§9.3).
fn break_lines(items: &[Item], a: Axes, avail_main: Au, gap: Au) -> Vec<Vec<usize>> {
    if items.is_empty() {
        return Vec::new();
    }
    if !a.multi {
        return vec![(0..items.len()).collect()];
    }
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut line: Vec<usize> = Vec::new();
    let mut sum = Au::ZERO;
    for (i, it) in items.iter().enumerate() {
        let outer = it.outer_main(a, it.hyp_main);
        let add = if line.is_empty() { outer } else { gap + outer };
        if !line.is_empty() && sum + add > avail_main {
            lines.push(std::mem::take(&mut line));
            sum = Au::ZERO;
            line.push(i);
            sum += outer;
        } else {
            line.push(i);
            sum += add;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Resolves the flexible lengths of one line (§9.7). Each pass freezes at least one
/// item, so the loop runs at most `line.len() + 1` times.
fn resolve_flexible_lengths(items: &mut [Item], line: &[usize], a: Axes, inner_main: Au, gap: Au) {
    let n = line.len() as i32;
    let gaps = gap * (n - 1).max(0);
    let sum_hyp: Au = line
        .iter()
        .map(|&i| items[i].outer_main(a, items[i].hyp_main))
        .fold(Au::ZERO, |acc, v| acc + v);
    let grow = sum_hyp + gaps < inner_main;
    for &i in line {
        let it = &mut items[i];
        it.target = it.base;
        it.frozen = false;
        let inflexible = if grow {
            it.grow == 0 || it.base > it.hyp_main
        } else {
            it.shrink == 0 || it.base < it.hyp_main
        };
        if inflexible {
            it.target = it.hyp_main;
            it.frozen = true;
        }
    }
    let used = |items: &[Item]| -> Au {
        line.iter()
            .map(|&i| {
                items[i].outer_main(
                    a,
                    if items[i].frozen {
                        items[i].target
                    } else {
                        items[i].base
                    },
                )
            })
            .fold(Au::ZERO, |acc, v| acc + v)
    };
    let initial_free = inner_main - gaps - used(items);
    for _ in 0..=line.len() {
        if line.iter().all(|&i| items[i].frozen) {
            break;
        }
        let remaining = inner_main - gaps - used(items);
        let unfrozen: Vec<usize> = line.iter().copied().filter(|&i| !items[i].frozen).collect();
        let sum_factors: i128 = unfrozen
            .iter()
            .map(|&i| if grow { items[i].grow } else { items[i].shrink } as i128)
            .sum();
        let mut free = remaining;
        if sum_factors < 1000 {
            let p = mul_div(initial_free, sum_factors, 1000);
            if p.abs() < remaining.abs() {
                free = p;
            }
        }
        if free.0 != 0 {
            if grow {
                let mut cum: i128 = 0;
                let mut prev = Au::ZERO;
                for &i in &unfrozen {
                    cum += items[i].grow as i128;
                    let upto = mul_div(free, cum, sum_factors);
                    items[i].target = items[i].base + (upto - prev);
                    prev = upto;
                }
            } else {
                let scaled: Vec<i128> = unfrozen
                    .iter()
                    .map(|&i| items[i].shrink as i128 * items[i].base.0 as i128)
                    .collect();
                let sum_scaled: i128 = scaled.iter().sum();
                let mut cum: i128 = 0;
                let mut prev = Au::ZERO;
                for (k, &i) in unfrozen.iter().enumerate() {
                    cum += scaled[k];
                    let upto = mul_div(free, cum, sum_scaled);
                    items[i].target = items[i].base + (upto - prev);
                    prev = upto;
                }
            }
        } else {
            for &i in &unfrozen {
                items[i].target = items[i].base;
            }
        }
        let mut total = Au::ZERO;
        for &i in &unfrozen {
            let it = &mut items[i];
            let clamped = clamp_main(it, it.target);
            it.violation = clamped - it.target;
            it.target = clamped;
            total += it.violation;
        }
        for &i in &unfrozen {
            let it = &mut items[i];
            let freeze = match total.0.signum() {
                0 => true,
                1 => it.violation > Au::ZERO,
                _ => it.violation < Au::ZERO,
            };
            if freeze {
                it.frozen = true;
            }
        }
    }
}

/// Maps `justify-content` to the flex-relative position, given the axes (§8.2;
/// `start`/`end` follow the writing mode, `left`/`right` are physical).
fn justify_pos(j: JustifyContent, a: Axes) -> Pos {
    let flip = |p: Pos, rev: bool| {
        if rev {
            if p == Pos::Start {
                Pos::End
            } else {
                Pos::Start
            }
        } else {
            p
        }
    };
    match j {
        JustifyContent::FlexStart | JustifyContent::Stretch => Pos::Start,
        JustifyContent::FlexEnd => Pos::End,
        JustifyContent::Center => Pos::Center,
        JustifyContent::SpaceBetween => Pos::Between,
        JustifyContent::SpaceAround => Pos::Around,
        JustifyContent::SpaceEvenly => Pos::Evenly,
        JustifyContent::Start => flip(Pos::Start, a.dir_rev),
        JustifyContent::End => flip(Pos::End, a.dir_rev),
        JustifyContent::Left => {
            if a.row {
                flip(Pos::Start, a.main_rev)
            } else {
                flip(Pos::Start, a.dir_rev)
            }
        }
        JustifyContent::Right => {
            if a.row {
                flip(Pos::End, a.main_rev)
            } else {
                flip(Pos::Start, a.dir_rev)
            }
        }
    }
}

fn align_content_pos(c: AlignContent, a: Axes) -> Pos {
    let flip = |p: Pos, rev: bool| {
        if rev {
            if p == Pos::Start {
                Pos::End
            } else {
                Pos::Start
            }
        } else {
            p
        }
    };
    match c {
        AlignContent::Normal | AlignContent::Stretch | AlignContent::FlexStart => Pos::Start,
        AlignContent::FlexEnd => Pos::End,
        AlignContent::Center => Pos::Center,
        AlignContent::SpaceBetween => Pos::Between,
        AlignContent::SpaceAround => Pos::Around,
        AlignContent::SpaceEvenly => Pos::Evenly,
        AlignContent::Start => flip(Pos::Start, a.wrap_rev),
        AlignContent::End => flip(Pos::End, a.wrap_rev),
    }
}

fn align_self_pos(s: AlignSelf, a: Axes) -> Pos {
    let flip = |p: Pos, rev: bool| {
        if rev {
            if p == Pos::Start {
                Pos::End
            } else {
                Pos::Start
            }
        } else {
            p
        }
    };
    match s {
        AlignSelf::Auto | AlignSelf::Stretch | AlignSelf::FlexStart => Pos::Start,
        AlignSelf::FlexEnd => Pos::End,
        AlignSelf::Center => Pos::Center,
        AlignSelf::Baseline => {
            if a.row {
                Pos::Baseline
            } else {
                Pos::Start
            }
        }
        AlignSelf::Start => flip(Pos::Start, a.wrap_rev),
        AlignSelf::End => flip(Pos::End, a.wrap_rev),
    }
}

/// The offset of the `k`-th of `n` boxes distributed in `free` space (which may be
/// negative) by a content-distribution value; the fallbacks are `flex-start` for
/// `space-between` and `center` for `space-around`/`space-evenly` (§8.2).
fn distribute(p: Pos, free: Au, k: usize, n: usize) -> Au {
    let (k, n) = (k as i128, n as i128);
    match p {
        Pos::Start | Pos::Baseline => Au::ZERO,
        Pos::End => free,
        Pos::Center => free / 2,
        Pos::Between => {
            if free <= Au::ZERO || n <= 1 {
                Au::ZERO
            } else {
                mul_div(free, k, n - 1)
            }
        }
        Pos::Around => {
            if free < Au::ZERO {
                free / 2
            } else {
                mul_div(free, 2 * k + 1, 2 * n)
            }
        }
        Pos::Evenly => {
            if free < Au::ZERO {
                free / 2
            } else {
                mul_div(free, k + 1, n + 1)
            }
        }
    }
}

/// Lays out the items of a flex container whose content box is `cb` (§9). Fragments
/// are positioned relative to the content box; `height` is the content height the
/// container gets when its own is `auto`.
pub fn layout_contents(ctx: &LayoutContext, id: BoxId, cb: &Cb) -> ContentsResult {
    let b = &ctx.tree[id];
    let s = b.style.clone();
    let rtl = s.direction == Direction::Rtl;
    let row = matches!(
        s.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let dir_rev = matches!(
        s.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let wrap_rev = s.flex_wrap == FlexWrap::WrapReverse;
    let a = Axes {
        row,
        dir_rev,
        wrap_rev,
        main_rev: dir_rev ^ (row && rtl),
        cross_rev: wrap_rev ^ (!row && rtl),
        multi: s.flex_wrap != FlexWrap::NoWrap,
    };
    let main_def = if row { Some(cb.width) } else { cb.height };
    let cross_def = if row { cb.height } else { Some(cb.width) };
    // The container's own min/max height (lengths only: its containing block is not
    // known here), for the indefinite axis.
    let cp = block::padding_edges(&s, cb.width);
    let cev = cp.vertical() + s.used_border_widths().vertical();
    let min_h = length_size(s.min_height, cev, s.box_sizing);
    let max_h = length_size(s.max_height, cev, s.box_sizing);
    let main_gap = if row {
        s.column_gap.resolve(cb.width)
    } else {
        s.row_gap.maybe_resolve(cb.height).unwrap_or(Au::ZERO)
    };
    let cross_gap = if row {
        s.row_gap.maybe_resolve(cb.height).unwrap_or(Au::ZERO)
    } else {
        s.column_gap.resolve(cb.width)
    };

    // Items in order-modified document order (§5.4); absolutes aside (§4.1).
    let mut items: Vec<Item> = Vec::new();
    let mut abs_ids: Vec<BoxId> = Vec::new();
    for &c in &b.children {
        let cbx = &ctx.tree[c];
        if cbx.is_abs() {
            abs_ids.push(c);
            continue;
        }
        if matches!(
            cbx.kind,
            BoxKind::Col(_) | BoxKind::ColGroup(_) | BoxKind::Wbr | BoxKind::Marker(_)
        ) {
            continue;
        }
        items.push(Item::new(ctx, c, cb, &s));
    }
    items.sort_by_key(|it| it.order);

    // §9.2: flex base sizes and hypothetical main sizes.
    for it in &mut items {
        compute_main_sizes(ctx, cb, it, a, main_def);
    }

    // §9.3: flex lines and the container's main size.
    let avail_main = main_def
        .or(if row { None } else { max_h })
        .unwrap_or(Au::MAX);
    let lines = break_lines(&items, a, avail_main, main_gap);
    let line_hyp_sum = |line: &Vec<usize>| -> Au {
        let n = line.len() as i32;
        line.iter()
            .map(|&i| items[i].outer_main(a, items[i].hyp_main))
            .fold(Au::ZERO, |acc, v| acc + v)
            + main_gap * (n - 1).max(0)
    };
    let inner_main = match main_def {
        Some(m) => m,
        None => {
            let mut m = lines.iter().map(line_hyp_sum).max().unwrap_or(Au::ZERO);
            if let Some(mx) = max_h {
                m = m.min(mx);
            }
            if let Some(mn) = min_h {
                m = m.max(mn);
            }
            m
        }
    };
    for line in &lines {
        resolve_flexible_lengths(&mut items, line, a, inner_main, main_gap);
    }

    // §9.4 step 7: hypothetical cross sizes.
    for it in &mut items {
        if row {
            layout_item(ctx, cb, it, it.target, H::Auto);
            it.hyp_cross = it.content_height();
        } else {
            it.hyp_cross =
                column_width(ctx, cb, it, a, if a.multi { None } else { Some(cb.width) });
        }
    }

    // Step 8: line cross sizes; baseline groups measured from the cross-start edge.
    let above_of = |it: &Item| -> Au {
        let phys = it.margin.top + it.baseline_or_synth();
        if a.cross_rev {
            it.outer_cross(a, it.hyp_cross) - phys
        } else {
            phys
        }
    };
    let participates = |it: &Item| row && it.align == AlignSelf::Baseline && !it.has_auto_cross(a);
    let mut line_cross: Vec<Au> = Vec::with_capacity(lines.len());
    let mut line_above: Vec<Au> = Vec::with_capacity(lines.len());
    for line in &lines {
        let mut max_above = Au::ZERO;
        let mut max_below = Au::ZERO;
        let mut max_outer = Au::ZERO;
        for &i in line {
            let it = &items[i];
            let outer = it.outer_cross(a, it.hyp_cross);
            if participates(it) {
                let ab = above_of(it);
                max_above = max_above.max(ab);
                max_below = max_below.max(outer - ab);
            } else {
                max_outer = max_outer.max(outer);
            }
        }
        let mut lc = match (a.multi, cross_def) {
            (false, Some(c)) => c,
            _ => (max_above + max_below).max(max_outer),
        };
        if !a.multi && row {
            if let Some(mx) = max_h {
                lc = lc.min(mx);
            }
            if let Some(mn) = min_h {
                lc = lc.max(mn);
            }
        }
        line_cross.push(lc);
        line_above.push(max_above);
    }
    // Step 9: `align-content: stretch` shares extra cross space among the lines.
    let n_lines = lines.len();
    let cross_gaps = cross_gap * (n_lines as i32 - 1).max(0);
    // A multi-line row container with an indefinite height is as tall as its lines,
    // clamped by its min- and max-height: extra height from a `min-height` is still
    // space for `align-content: stretch` (react-admin's filter form: `flex-wrap:
    // wrap; align-items: flex-end; min-height: 64px`, whose field sits at the bottom).
    let clamped_cross = if a.multi && row && cross_def.is_none() {
        let used: Au = line_cross.iter().fold(Au::ZERO, |acc, v| acc + *v) + cross_gaps;
        let mut c = used;
        if let Some(mx) = max_h {
            c = c.min(mx);
        }
        if let Some(mn) = min_h {
            c = c.max(mn);
        }
        (c > used).then_some(c)
    } else {
        None
    };
    if let Some(c) = cross_def.or(clamped_cross) {
        if matches!(
            s.align_content,
            AlignContent::Normal | AlignContent::Stretch
        ) && n_lines > 0
        {
            let used: Au = line_cross.iter().fold(Au::ZERO, |acc, v| acc + *v);
            let free = c - used - cross_gaps;
            if free > Au::ZERO {
                let mut prev = Au::ZERO;
                for (li, lc) in line_cross.iter_mut().enumerate() {
                    let upto = mul_div(free, li as i128 + 1, n_lines as i128);
                    *lc += upto - prev;
                    prev = upto;
                }
            }
        }
    }
    // Step 11: used cross sizes and the final layout of every item.
    for (li, line) in lines.iter().enumerate() {
        let lc = line_cross[li];
        for &i in line {
            let it = &mut items[i];
            let stretch =
                it.align == AlignSelf::Stretch && it.cross_is_auto(a) && !it.has_auto_cross(a);
            if stretch {
                let v = (lc - it.margin_cross(a) - it.edges_cross(a)).max(Au::ZERO);
                let st = it.style.clone();
                it.cross = if row {
                    block::clamp_size(
                        v,
                        st.min_height,
                        st.max_height,
                        cb.height,
                        it.pb.vertical(),
                        st.box_sizing,
                    )
                } else {
                    block::clamp_size(
                        v,
                        st.min_width,
                        st.max_width,
                        Some(cb.width),
                        it.pb.horizontal(),
                        st.box_sizing,
                    )
                };
            } else {
                it.cross = it.hyp_cross;
            }
            if row {
                let h = if stretch {
                    H::Forced(it.cross)
                } else {
                    H::Auto
                };
                layout_item(ctx, cb, it, it.target, h);
            } else {
                layout_item(ctx, cb, it, it.cross, H::Forced(it.target));
            }
        }
    }

    // §9.5: main-axis alignment.
    let jc = justify_pos(s.justify_content, a);
    for line in &lines {
        let n = line.len();
        let used: Au = line
            .iter()
            .map(|&i| items[i].outer_main(a, items[i].target))
            .fold(Au::ZERO, |acc, v| acc + v);
        let mut free = inner_main - used - main_gap * (n as i32 - 1).max(0);
        let auto_count: i128 = line
            .iter()
            .map(|&i| {
                let (s, e) = items[i].auto_main(a);
                s as i128 + e as i128
            })
            .sum();
        if auto_count > 0 {
            if free > Au::ZERO {
                let each = mul_div(free, 1, auto_count);
                let mut left = free;
                let mut k = 0;
                for &i in line {
                    let it = &mut items[i];
                    let (sa, ea) = it.auto_main(a);
                    let mut take = |auto: bool| -> Au {
                        if !auto {
                            return Au::ZERO;
                        }
                        k += 1;
                        let v = if k == auto_count { left } else { each };
                        left -= v;
                        v
                    };
                    let (ms, me) = (take(sa), take(ea));
                    let (cs, ce) = if a.row {
                        (it.margin.left, it.margin.right)
                    } else {
                        (it.margin.top, it.margin.bottom)
                    };
                    it.set_main_margins(a, if sa { ms } else { cs }, if ea { me } else { ce });
                }
            }
            free = Au::ZERO.min(free);
        }
        let mut prefix = Au::ZERO;
        for (k, &i) in line.iter().enumerate() {
            let it = &mut items[i];
            let om = it.outer_main(a, it.target);
            it.main_pos = prefix + main_gap * k as i32 + distribute(jc, free, k, n);
            prefix += om;
        }
    }

    // §9.6: cross-axis alignment of lines (align-content) and items (align-self).
    let lines_used: Au = line_cross.iter().fold(Au::ZERO, |acc, v| acc + *v);
    let total_cross = cross_def.unwrap_or(lines_used + cross_gaps);
    let free_c = total_cross - lines_used - cross_gaps;
    let ac = align_content_pos(s.align_content, a);
    let mut line_pos: Vec<Au> = Vec::with_capacity(n_lines);
    let mut prefix = Au::ZERO;
    for (li, lc) in line_cross.iter().enumerate() {
        line_pos.push(prefix + cross_gap * li as i32 + distribute(ac, free_c, li, n_lines));
        prefix += *lc;
    }
    for (li, line) in lines.iter().enumerate() {
        let lc = line_cross[li];
        for &i in line {
            let it = &mut items[i];
            let free_i = lc - it.outer_cross(a, it.cross);
            let (sa, ea) = it.auto_cross(a);
            let (cs, ce) = if a.row {
                (it.margin.top, it.margin.bottom)
            } else {
                (it.margin.left, it.margin.right)
            };
            let offset = if sa || ea {
                if free_i > Au::ZERO {
                    let (ms, me) = match (sa, ea) {
                        (true, true) => (free_i / 2, free_i - free_i / 2),
                        (true, false) => (free_i, ce),
                        _ => (cs, free_i),
                    };
                    it.set_cross_margins(a, if sa { ms } else { cs }, if ea { me } else { ce });
                }
                Au::ZERO
            } else {
                match align_self_pos(it.align, a) {
                    Pos::Baseline => {
                        let ab = {
                            let phys = it.margin.top + it.baseline_or_synth();
                            if a.cross_rev {
                                it.outer_cross(a, it.cross) - phys
                            } else {
                                phys
                            }
                        };
                        line_above[li] - ab
                    }
                    p => distribute(p, free_i, 0, 1),
                }
            };
            it.cross_pos = line_pos[li] + offset;
        }
    }

    // Physical placement, relative offsets, fragments in order.
    let mut out = ContentsResult {
        empty: items.is_empty(),
        ..Default::default()
    };
    for it in &mut items {
        let om = it.outer_main(a, it.target);
        let oc = it.outer_cross(a, it.cross);
        let main_phys = if a.main_rev {
            inner_main - it.main_pos - om
        } else {
            it.main_pos
        };
        let cross_phys = if a.cross_rev {
            total_cross - it.cross_pos - oc
        } else {
            it.cross_pos
        };
        let (x, y) = if row {
            (main_phys + it.margin.left, cross_phys + it.margin.top)
        } else {
            (cross_phys + it.margin.left, main_phys + it.margin.top)
        };
        let off = block::relative_offset(&it.style, cb);
        it.frag_y = y + off.y;
        // The container's baselines are read after the fragments have moved out, so
        // an item without a baseline synthesises its own (the border box's bottom
        // edge) while its fragment still says how tall it is.
        it.baseline = Some(it.baseline_or_synth());
        if let Some(mut f) = it.fragment.take() {
            f.rect.origin = Point {
                x: x + off.x,
                y: y + off.y,
            };
            f.used_margin = Some(it.margin);
            let mut abs = std::mem::take(&mut it.abs);
            block::translate_requests(&mut abs, f.rect.origin.x, f.rect.origin.y);
            out.abs.extend(abs);
            out.fragments.push(f);
        }
    }
    // The container's baselines (§8.5): the first line's baseline-aligned item, else
    // its first item, synthesized from the item's border box when it has none.
    let line_baseline = |line: &Vec<usize>, last: bool| -> Option<Au> {
        let pick = line
            .iter()
            .copied()
            .find(|&i| participates(&items[i]))
            .or(if last {
                line.last().copied()
            } else {
                line.first().copied()
            })?;
        let it = &items[pick];
        Some(it.frag_y + it.baseline_or_synth())
    };
    out.first_baseline = lines.first().and_then(|l| line_baseline(l, false));
    out.last_baseline = lines.last().and_then(|l| line_baseline(l, true));
    out.height = if row { total_cross } else { inner_main };

    // §4.1: absolutely positioned children take their static position as the sole
    // item of the container.
    let al_default = s.align_items;
    for c in abs_ids {
        let cs = ctx.tree[c].style.clone();
        let align = match cs.align_self {
            AlignSelf::Auto => match al_default {
                AlignItems::FlexEnd => AlignSelf::FlexEnd,
                AlignItems::Center => AlignSelf::Center,
                AlignItems::Start | AlignItems::SelfStart => AlignSelf::Start,
                AlignItems::End | AlignItems::SelfEnd => AlignSelf::End,
                _ => AlignSelf::FlexStart,
            },
            v => v,
        };
        let jp = jc;
        let ap = align_self_pos(align, a);
        let needs_size = !matches!(jp, Pos::Start | Pos::Between)
            || !matches!(ap, Pos::Start | Pos::Baseline)
            || a.main_rev
            || a.cross_rev;
        let (ow, oh) = if needs_size {
            let p = block::padding_edges(&cs, cb.width);
            let bw = cs.used_border_widths();
            let eh = p.horizontal() + bw.horizontal();
            let ml = cs.margin.left.resolve(cb.width).unwrap_or(Au::ZERO);
            let mr = cs.margin.right.resolve(cb.width).unwrap_or(Au::ZERO);
            let (mt, mb) = block::vertical_margins(&cs, cb.width);
            let (f, _) =
                block::layout_standalone(ctx, c, cb, (cb.width - ml - mr).max(Au::ZERO), eh);
            (f.rect.size.width + ml + mr, f.rect.size.height + mt + mb)
        } else {
            (Au::ZERO, Au::ZERO)
        };
        let (om, oc) = if row { (ow, oh) } else { (oh, ow) };
        let mp = distribute(jp, inner_main - om, 0, 1);
        let cpos = distribute(ap, total_cross - oc, 0, 1);
        let main_phys = if a.main_rev { inner_main - mp - om } else { mp };
        let cross_phys = if a.cross_rev {
            total_cross - cpos - oc
        } else {
            cpos
        };
        let (x, y) = if row {
            (main_phys, cross_phys)
        } else {
            (cross_phys, main_phys)
        };
        out.abs.push(AbsRequest {
            id: c,
            static_pos: Point { x, y },
            fixed: cs.position == Position::Fixed,
            paint_in: None,
        });
    }
    out
}

/// Content-box `(min-content, max-content)` widths of a flex container (§9.9, as
/// browsers implement it): a row container sums its items' max-content contributions
/// and, when single-line, their min-content contributions (multi-line: the largest);
/// a column container takes the largest of each. Gaps count.
pub fn content_min_max(ctx: &LayoutContext, id: BoxId) -> (Au, Au) {
    let b = &ctx.tree[id];
    let s = &b.style;
    let row = matches!(
        s.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let multi = s.flex_wrap != FlexWrap::NoWrap;
    let gap = match if row { s.column_gap } else { s.row_gap } {
        LengthPercentage::Length(l) => l,
        LengthPercentage::Calc(l, _) => l,
        v => v.resolve(Au::ZERO),
    };
    let mut mn = Au::ZERO;
    let mut mx = Au::ZERO;
    let mut n = 0;
    for &c in &b.children {
        let cb = &ctx.tree[c];
        if cb.is_abs()
            || matches!(
                cb.kind,
                BoxKind::Col(_) | BoxKind::ColGroup(_) | BoxKind::Wbr | BoxKind::Marker(_)
            )
        {
            continue;
        }
        let (cmn, cmx) = intrinsic::min_max(ctx, c);
        let m = intrinsic::margins_h(ctx, c);
        if row {
            mx += cmx + m;
            if multi {
                mn = mn.max(cmn + m);
            } else {
                mn += cmn + m;
            }
        } else {
            mn = mn.max(cmn + m);
            mx = mx.max(cmx + m);
        }
        n += 1;
    }
    if row && n > 1 {
        mx += gap * (n - 1);
        if !multi {
            mn += gap * (n - 1);
        }
    }
    (mn, mx.max(mn))
}

#[cfg(test)]
#[path = "flex_tests.rs"]
mod tests;
