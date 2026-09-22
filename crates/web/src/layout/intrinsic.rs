//! Min-content and max-content widths (§10.3.5, css-sizing-3) for every box type,
//! cached per box in the `LayoutCache` the entry point owns.
//!
//! Widths are border-box widths (content plus padding and border) without margins;
//! percentage padding and widths count as zero, as they have no base here.

use crate::geom::Au;
use crate::layout::block::{self, Cb};
use crate::layout::boxes::{BoxId, BoxKind};
use crate::layout::{inline, table, text, LayoutContext};
use crate::style::{BoxSizing, LengthPercentage, Sizing};

/// `(min-content, max-content)` border-box widths of a box.
pub fn min_max(ctx: &LayoutContext, id: BoxId) -> (Au, Au) {
    if let Some(v) = ctx
        .cache
        .borrow()
        .intrinsic
        .get(id.index())
        .copied()
        .flatten()
    {
        return v;
    }
    let v = compute(ctx, id);
    if let Some(slot) = ctx.cache.borrow_mut().intrinsic.get_mut(id.index()) {
        *slot = Some(v);
    }
    v
}

fn length_of(v: Sizing) -> Option<Au> {
    match v {
        Sizing::Set(LengthPercentage::Length(l)) => Some(l),
        Sizing::Set(LengthPercentage::Calc(l, _)) => Some(l),
        Sizing::Set(v) if !v.has_percent() => Some(v.resolve(Au::ZERO)),
        _ => None,
    }
}

/// Horizontal padding plus border, with percentages as zero.
pub fn edges_h(ctx: &LayoutContext, id: BoxId) -> Au {
    let s = ctx.style(id);
    let p = block::padding_edges(s, Au::ZERO);
    p.horizontal() + s.used_border_widths().horizontal()
}

/// Horizontal margins with percentages as zero.
pub fn margins_h(ctx: &LayoutContext, id: BoxId) -> Au {
    let s = ctx.style(id);
    s.margin.left.resolve(Au::ZERO).unwrap_or(Au::ZERO)
        + s.margin.right.resolve(Au::ZERO).unwrap_or(Au::ZERO)
}

/// Content-box `(min, max)` of a block container's contents, ignoring its own
/// `width`: the inline content's runs, or the widest child.
pub fn content_min_max(ctx: &LayoutContext, id: BoxId) -> (Au, Au) {
    let b = &ctx.tree[id];
    if crate::layout::flex::is_flex_container(b) {
        return crate::layout::flex::content_min_max(ctx, id);
    }
    if crate::layout::grid::is_grid_container(&b.style) {
        return crate::layout::grid::content_min_max(ctx, id);
    }
    if b.inline_children {
        let (mut mn, mut mx) = inline::intrinsic_widths(ctx, id);
        if let Some(m) = b.marker {
            if matches!(ctx.tree[m].kind, BoxKind::Inline) {
                // Inside markers are part of the inline content already.
                let _ = m;
            }
        }
        if ctx.style(id).white_space == crate::style::WhiteSpace::NoWrap {
            mn = mn.max(mx);
        }
        if mx < mn {
            mx = mn;
        }
        (mn, mx)
    } else {
        let mut mn = Au::ZERO;
        let mut mx = Au::ZERO;
        let mut float_run = Au::ZERO;
        for &c in &b.children {
            let cb = &ctx.tree[c];
            if cb.is_abs() || matches!(cb.kind, BoxKind::Col(_) | BoxKind::ColGroup(_)) {
                continue;
            }
            let (cmn, cmx) = min_max(ctx, c);
            let m = margins_h(ctx, c);
            mn = mn.max(cmn + m);
            if cb.is_float() {
                // Consecutive floats sit side by side.
                float_run += cmx + m;
                mx = mx.max(float_run);
            } else {
                float_run = Au::ZERO;
                mx = mx.max(cmx + m);
            }
        }
        (mn, mx)
    }
}

fn compute(ctx: &LayoutContext, id: BoxId) -> (Au, Au) {
    let b = &ctx.tree[id];
    let s = &b.style;
    match &b.kind {
        BoxKind::Text(_)
        | BoxKind::Inline
        | BoxKind::Br(_)
        | BoxKind::Wbr
        | BoxKind::Col(_)
        | BoxKind::ColGroup(_) => (Au::ZERO, Au::ZERO),
        BoxKind::Marker(t) => {
            let w = text::measure(&s.font, t, s.letter_spacing, s.word_spacing);
            (w, w)
        }
        BoxKind::Replaced(rb) => {
            let size = block::replaced_size(
                ctx,
                id,
                rb,
                &Cb {
                    width: Au::ZERO,
                    height: None,
                },
            );
            let e = edges_h(ctx, id);
            (size.width + e, size.width + e)
        }
        BoxKind::TableWrapper => table::intrinsic_widths(ctx, id),
        BoxKind::Table => table::grid_intrinsic_widths(ctx, id),
        BoxKind::Row | BoxKind::RowGroup => {
            let mut mn = Au::ZERO;
            let mut mx = Au::ZERO;
            for &c in &b.children {
                let (a, z) = min_max(ctx, c);
                mn += a;
                mx += z;
            }
            (mn, mx)
        }
        BoxKind::Block | BoxKind::InlineBlock | BoxKind::Cell(_) | BoxKind::Caption => {
            let e = edges_h(ctx, id);
            let content_e = match s.box_sizing {
                BoxSizing::BorderBox => Au::ZERO,
                BoxSizing::ContentBox => e,
            };
            let (mut mn, mut mx) = match length_of(s.width) {
                Some(w) => {
                    let bb = (w + content_e).max(e);
                    (bb, bb)
                }
                None => {
                    let (cmn, cmx) = content_min_max(ctx, id);
                    match s.width {
                        Sizing::MinContent => (cmn + e, cmn + e),
                        Sizing::MaxContent => (cmx + e, cmx + e),
                        _ => (cmn + e, cmx + e),
                    }
                }
            };
            if let Some(mxw) = length_of(s.max_width) {
                let bb = (mxw + content_e).max(e);
                mn = mn.min(bb);
                mx = mx.min(bb);
            }
            if let Some(mnw) = length_of(s.min_width) {
                let bb = (mnw + content_e).max(e);
                mn = mn.max(bb);
                mx = mx.max(bb);
            }
            let (bar_x, bar_y) = crate::layout::scroll::reserved_bars_in(ctx, s);
            let _ = bar_x;
            (mn + bar_y, mx + bar_y)
        }
    }
}
