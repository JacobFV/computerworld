//! Tables (CSS 2.1 §17): the wrapper and grid boxes, the automatic (§17.5.2.2) and
//! fixed (§17.5.2.1) column algorithms, row heights with rowspan distribution,
//! `vertical-align` in cells, `border-spacing`, `border-collapse: collapse` with
//! the conflict resolution of §17.6.2.1 and half-border geometry, `empty-cells`,
//! captions, and `height` on tables and rows.

// The grid algorithms index several parallel arrays by row and column.
#![allow(clippy::needless_range_loop)]

use crate::geom::{Au, Edges, Point, Rect, Size};
use crate::layout::block::{self, AbsRequest, Bfc, BlockResult, Cb, MarginSet};
use crate::layout::boxes::{BoxId, BoxKind, Dim};
use crate::layout::fragment::{CollapsedBorders, Fragment, FragmentKind};
use crate::layout::{intrinsic, LayoutContext};
use crate::style::{BorderCollapse, BorderSide, BorderStyle, BoxSizing, Direction, EmptyCells, LengthPercentage, Sizing, TableLayout, VerticalAlign};

/// A cell placed in the grid.
#[derive(Clone, Copy, Debug)]
pub struct GridCell {
    pub id: BoxId,
    pub row: usize,
    pub col: usize,
    pub rowspan: usize,
    pub colspan: usize,
}

#[derive(Clone, Debug)]
pub struct RowInfo {
    pub id: BoxId,
    pub group: BoxId,
    pub first_in_group: bool,
    pub last_in_group: bool,
}

/// The grid of a table: rows, cells with their spans, and the column count.
#[derive(Clone, Debug, Default)]
pub struct Structure {
    pub rows: Vec<RowInfo>,
    pub cells: Vec<GridCell>,
    pub ncols: usize,
    /// Slot (row, col) → index into `cells`.
    pub slots: Vec<Vec<Option<usize>>>,
    /// Per column: the `<col>` box, when one covers it, and its group.
    pub cols: Vec<Option<BoxId>>,
    pub colgroups: Vec<Option<BoxId>>,
}

impl Structure {
    pub fn slot(&self, r: usize, c: usize) -> Option<usize> {
        self.slots.get(r).and_then(|row| row.get(c).copied().flatten())
    }
}

/// Builds the grid (§17.5.1, HTML's table processing model for spans).
pub fn structure(ctx: &LayoutContext, grid: BoxId) -> Structure {
    let mut st = Structure::default();
    let mut cols: Vec<Option<BoxId>> = Vec::new();
    let mut colgroups: Vec<Option<BoxId>> = Vec::new();
    for &c in ctx.tree.children(grid) {
        match &ctx.tree[c].kind {
            BoxKind::ColGroup(cg) => {
                let kids = ctx.tree.children(c);
                if kids.is_empty() {
                    for _ in 0..cg.span {
                        cols.push(None);
                        colgroups.push(Some(c));
                    }
                } else {
                    for &k in kids {
                        if let BoxKind::Col(cb) = &ctx.tree[k].kind {
                            for _ in 0..cb.span {
                                cols.push(Some(k));
                                colgroups.push(Some(c));
                            }
                        }
                    }
                }
            }
            BoxKind::Col(cb) => {
                for _ in 0..cb.span {
                    cols.push(Some(c));
                    colgroups.push(None);
                }
            }
            _ => {}
        }
    }
    for &g in ctx.tree.children(grid) {
        if ctx.tree[g].kind != BoxKind::RowGroup {
            continue;
        }
        let rows: Vec<BoxId> = ctx.tree.children(g).iter().copied().filter(|r| ctx.tree[*r].kind == BoxKind::Row).collect();
        let n = rows.len();
        for (i, r) in rows.into_iter().enumerate() {
            st.rows.push(RowInfo { id: r, group: g, first_in_group: i == 0, last_in_group: i + 1 == n });
        }
    }
    let nrows = st.rows.len();
    let mut slots: Vec<Vec<Option<usize>>> = vec![Vec::new(); nrows];
    for r in 0..nrows {
        let row = st.rows[r].id;
        let mut c = 0usize;
        for &cell in ctx.tree.children(row) {
            let BoxKind::Cell(cb) = &ctx.tree[cell].kind else { continue };
            while slots[r].get(c).copied().flatten().is_some() {
                c += 1;
            }
            let colspan = cb.colspan.max(1) as usize;
            let rowspan = (cb.rowspan.max(1) as usize).min(nrows - r);
            let idx = st.cells.len();
            st.cells.push(GridCell { id: cell, row: r, col: c, rowspan, colspan });
            for rr in r..r + rowspan {
                if slots[rr].len() < c + colspan {
                    slots[rr].resize(c + colspan, None);
                }
                for cc in c..c + colspan {
                    if slots[rr][cc].is_none() {
                        slots[rr][cc] = Some(idx);
                    }
                }
            }
            c += colspan;
        }
    }
    st.ncols = slots.iter().map(|r| r.len()).max().unwrap_or(0).max(cols.len());
    for r in &mut slots {
        r.resize(st.ncols, None);
    }
    st.slots = slots;
    cols.resize(st.ncols, None);
    colgroups.resize(st.ncols, None);
    st.cols = cols;
    st.colgroups = colgroups;
    st
}

/// Per-column constraints from cells and `<col>`s.
#[derive(Clone, Copy, Debug, Default)]
pub struct ColInfo {
    pub min: Au,
    pub max: Au,
    /// Percentage in 1/100 %, when any cell or col in the column has one.
    pub percent: Option<i32>,
    /// A specified length width from a cell or col.
    pub fixed: Option<Au>,
}

/// Borders resolved for every grid edge in the collapsing model.
#[derive(Clone, Debug, Default)]
pub struct CollapsedGrid {
    /// `[row 0..=nrows][col]`: the horizontal edge above row `r`.
    pub horizontal: Vec<Vec<BorderSide>>,
    /// `[row][col 0..=ncols]`: the vertical edge left of column `c`.
    pub vertical: Vec<Vec<BorderSide>>,
}

fn style_rank(s: BorderStyle) -> u8 {
    match s {
        BorderStyle::Hidden => 10,
        BorderStyle::Double => 8,
        BorderStyle::Solid => 7,
        BorderStyle::Dashed => 6,
        BorderStyle::Dotted => 5,
        BorderStyle::Ridge => 4,
        BorderStyle::Outset => 3,
        BorderStyle::Groove => 2,
        BorderStyle::Inset => 1,
        BorderStyle::None => 0,
    }
}

/// §17.6.2.1: picks the winning border of two candidates; `origin` ranks the source
/// (cell 6 > row 5 > row group 4 > column 3 > column group 2 > table 1).
fn resolve(a: (BorderSide, u8), b: (BorderSide, u8)) -> (BorderSide, u8) {
    let (sa, sb) = (a.0.style, b.0.style);
    if sa == BorderStyle::Hidden {
        return a;
    }
    if sb == BorderStyle::Hidden {
        return b;
    }
    if sa == BorderStyle::None {
        return b;
    }
    if sb == BorderStyle::None {
        return a;
    }
    if a.0.width != b.0.width {
        return if a.0.width > b.0.width { a } else { b };
    }
    if style_rank(sa) != style_rank(sb) {
        return if style_rank(sa) > style_rank(sb) { a } else { b };
    }
    if a.1 >= b.1 {
        a
    } else {
        b
    }
}

fn none_side() -> BorderSide {
    BorderSide { width: Au::ZERO, style: BorderStyle::None, color: cw_scene::Color(0, 0, 0, 255) }
}

/// Resolves the collapsed borders of every edge.
pub fn collapse_borders(ctx: &LayoutContext, grid: BoxId, st: &Structure) -> CollapsedGrid {
    let nrows = st.rows.len();
    let ncols = st.ncols;
    let ts = ctx.style(grid);
    let mut horizontal = vec![vec![none_side(); ncols]; nrows + 1];
    let mut vertical = vec![vec![none_side(); ncols + 1]; nrows];
    for r in 0..=nrows {
        for c in 0..ncols {
            let mut best = (none_side(), 0u8);
            // Cell below (top border) and cell above (bottom border).
            if r < nrows {
                if let Some(i) = st.slot(r, c) {
                    let cell = st.cells[i];
                    if cell.row == r {
                        best = resolve(best, (ctx.style(cell.id).border.top, 6));
                    }
                }
            }
            if r > 0 {
                if let Some(i) = st.slot(r - 1, c) {
                    let cell = st.cells[i];
                    if cell.row + cell.rowspan == r {
                        best = resolve(best, (ctx.style(cell.id).border.bottom, 6));
                    }
                }
            }
            if r < nrows {
                let row = &st.rows[r];
                best = resolve(best, (ctx.style(row.id).border.top, 5));
                if row.first_in_group {
                    best = resolve(best, (ctx.style(row.group).border.top, 4));
                }
            }
            if r > 0 {
                let row = &st.rows[r - 1];
                best = resolve(best, (ctx.style(row.id).border.bottom, 5));
                if row.last_in_group {
                    best = resolve(best, (ctx.style(row.group).border.bottom, 4));
                }
            }
            if r == 0 {
                if let Some(col) = st.cols[c] {
                    best = resolve(best, (ctx.style(col).border.top, 3));
                }
                if let Some(cg) = st.colgroups[c] {
                    best = resolve(best, (ctx.style(cg).border.top, 2));
                }
                best = resolve(best, (ts.border.top, 1));
            }
            if r == nrows {
                if let Some(col) = st.cols[c] {
                    best = resolve(best, (ctx.style(col).border.bottom, 3));
                }
                if let Some(cg) = st.colgroups[c] {
                    best = resolve(best, (ctx.style(cg).border.bottom, 2));
                }
                best = resolve(best, (ts.border.bottom, 1));
            }
            horizontal[r][c] = best.0;
        }
    }
    for r in 0..nrows {
        for c in 0..=ncols {
            let mut best = (none_side(), 0u8);
            if c < ncols {
                if let Some(i) = st.slot(r, c) {
                    let cell = st.cells[i];
                    if cell.col == c {
                        best = resolve(best, (ctx.style(cell.id).border.left, 6));
                    }
                }
            }
            if c > 0 {
                if let Some(i) = st.slot(r, c - 1) {
                    let cell = st.cells[i];
                    if cell.col + cell.colspan == c {
                        best = resolve(best, (ctx.style(cell.id).border.right, 6));
                    }
                }
            }
            if c < ncols {
                if let Some(col) = st.cols[c] {
                    best = resolve(best, (ctx.style(col).border.left, 3));
                }
                if let Some(cg) = st.colgroups[c] {
                    if c == 0 || st.colgroups[c - 1] != Some(cg) {
                        best = resolve(best, (ctx.style(cg).border.left, 2));
                    }
                }
            }
            if c > 0 {
                if let Some(col) = st.cols[c - 1] {
                    best = resolve(best, (ctx.style(col).border.right, 3));
                }
                if let Some(cg) = st.colgroups[c - 1] {
                    if c == ncols || st.colgroups[c] != Some(cg) {
                        best = resolve(best, (ctx.style(cg).border.right, 2));
                    }
                }
            }
            let row = &st.rows[r];
            if c == 0 {
                best = resolve(best, (ctx.style(row.id).border.left, 5));
                best = resolve(best, (ctx.style(row.group).border.left, 4));
                best = resolve(best, (ts.border.left, 1));
            }
            if c == ncols {
                best = resolve(best, (ctx.style(row.id).border.right, 5));
                best = resolve(best, (ctx.style(row.group).border.right, 4));
                best = resolve(best, (ts.border.right, 1));
            }
            vertical[r][c] = best.0;
        }
    }
    CollapsedGrid { horizontal, vertical }
}

fn widest(sides: impl Iterator<Item = BorderSide>) -> BorderSide {
    let mut best = none_side();
    let mut hidden = None;
    for s in sides {
        if s.style == BorderStyle::Hidden {
            hidden = Some(s);
        }
        if s.style.is_visible() && (!best.style.is_visible() || s.width > best.width) {
            best = s;
        }
    }
    match hidden {
        Some(h) if !best.style.is_visible() => BorderSide { width: Au::ZERO, ..h },
        _ => best,
    }
}

fn half(w: Au) -> Au {
    w / 2
}

/// The four resolved borders of a cell in the collapsing model (widest along each side).
fn cell_collapsed(cg: &CollapsedGrid, cell: &GridCell) -> CollapsedBorders {
    let top = widest((cell.col..cell.col + cell.colspan).map(|c| cg.horizontal[cell.row][c]));
    let bottom = widest((cell.col..cell.col + cell.colspan).map(|c| cg.horizontal[cell.row + cell.rowspan][c]));
    let left = widest((cell.row..cell.row + cell.rowspan).map(|r| cg.vertical[r][cell.col]));
    let right = widest((cell.row..cell.row + cell.rowspan).map(|r| cg.vertical[r][cell.col + cell.colspan]));
    CollapsedBorders { top, right, bottom, left }
}

fn cell_border_used(cb: &CollapsedBorders) -> Edges {
    Edges { top: half(cb.top.used_width()), right: half(cb.right.used_width()), bottom: half(cb.bottom.used_width()), left: half(cb.left.used_width()) }
}

/// Everything about a table's geometry decided before rows are laid out.
struct Prep {
    st: Structure,
    collapsed: Option<CollapsedGrid>,
    cell_borders: Vec<Edges>,
    cell_collapsed: Vec<Option<CollapsedBorders>>,
    cols: Vec<ColInfo>,
    /// The grid box's padding and border (half outer borders when collapsing).
    padding: Edges,
    border: Edges,
    outer: Option<CollapsedBorders>,
    spacing: (Au, Au),
}

fn prepare(ctx: &LayoutContext, grid: BoxId) -> Prep {
    let s = ctx.style(grid);
    let st = structure(ctx, grid);
    let collapse = s.border_collapse == BorderCollapse::Collapse;
    let collapsed = if collapse { Some(collapse_borders(ctx, grid, &st)) } else { None };
    let mut cell_borders = Vec::with_capacity(st.cells.len());
    let mut cell_collapsed = Vec::with_capacity(st.cells.len());
    for cell in &st.cells {
        match &collapsed {
            Some(cg) => {
                let cb = cell_collapsed_of(cg, cell);
                cell_borders.push(cell_border_used(&cb));
                cell_collapsed.push(Some(cb));
            }
            None => {
                cell_borders.push(ctx.style(cell.id).used_border_widths());
                cell_collapsed.push(None);
            }
        }
    }
    let (padding, border, outer, spacing) = match &collapsed {
        Some(cg) => {
            let nrows = st.rows.len();
            let ncols = st.ncols;
            let top = widest((0..ncols).map(|c| cg.horizontal[0][c]));
            let bottom = widest((0..ncols).map(|c| cg.horizontal[nrows][c]));
            let left = widest((0..nrows).map(|r| cg.vertical[r][0]));
            let right = widest((0..nrows).map(|r| cg.vertical[r][ncols]));
            let outer = CollapsedBorders { top, right, bottom, left };
            // With no rows the table's own borders apply.
            let b = if nrows == 0 || ncols == 0 {
                s.used_border_widths()
            } else {
                Edges { top: half(top.used_width()), right: half(right.used_width()), bottom: half(bottom.used_width()), left: half(left.used_width()) }
            };
            (Edges::ZERO, b, Some(outer), (Au::ZERO, Au::ZERO))
        }
        None => (block::padding_edges(s, Au::ZERO), s.used_border_widths(), None, s.border_spacing),
    };
    let cols = column_constraints(ctx, &st, &cell_borders, spacing.0);
    Prep { st, collapsed, cell_borders, cell_collapsed, cols, padding, border, outer, spacing }
}

fn cell_collapsed_of(cg: &CollapsedGrid, cell: &GridCell) -> CollapsedBorders {
    cell_collapsed(cg, cell)
}

/// Column min/max/percent/fixed constraints (§17.5.2.2 steps 1–3).
fn column_constraints(ctx: &LayoutContext, st: &Structure, cell_borders: &[Edges], hspacing: Au) -> Vec<ColInfo> {
    let n = st.ncols;
    let mut cols = vec![ColInfo::default(); n];
    for (c, col) in cols.iter_mut().enumerate() {
        for b in [st.cols[c], st.colgroups[c]].into_iter().flatten() {
            let cs = ctx.style(b);
            match cs.width {
                Sizing::Set(LengthPercentage::Length(w)) => {
                    col.fixed = Some(col.fixed.map_or(w, |f| f.max(w)));
                    col.min = col.min.max(w);
                    col.max = col.max.max(w);
                }
                Sizing::Set(LengthPercentage::Percent(p)) => col.percent = Some(col.percent.map_or(p, |q| q.max(p))),
                _ => {
                    if let BoxKind::Col(cb) | BoxKind::ColGroup(cb) = &ctx.tree[b].kind {
                        match cb.width {
                            Some(Dim::Px(w)) => {
                                col.fixed = Some(col.fixed.map_or(w, |f| f.max(w)));
                                col.min = col.min.max(w);
                                col.max = col.max.max(w);
                            }
                            Some(Dim::Percent(p)) => col.percent = Some(col.percent.map_or(p, |q| q.max(p))),
                            None => {}
                        }
                    }
                }
            }
        }
    }
    // Single-column cells first.
    let mut spanning = Vec::new();
    for (i, cell) in st.cells.iter().enumerate() {
        let (mn, mx, spec, pct) = cell_widths(ctx, cell.id, cell_borders[i]);
        if cell.colspan == 1 {
            let col = &mut cols[cell.col];
            col.min = col.min.max(mn);
            col.max = col.max.max(mx);
            if let Some(w) = spec {
                col.fixed = Some(col.fixed.map_or(w, |f| f.max(w)));
            }
            if let Some(p) = pct {
                col.percent = Some(col.percent.map_or(p, |q| q.max(p)));
            }
        } else {
            spanning.push((i, mn, mx, pct));
        }
    }
    // Spanning cells distribute their excess over the spanned columns.
    spanning.sort_by_key(|(i, _, _, _)| st.cells[*i].colspan);
    for (i, mn, mx, pct) in spanning {
        let cell = st.cells[i];
        let range = cell.col..(cell.col + cell.colspan).min(n);
        let span_spacing = hspacing * (cell.colspan as i32 - 1);
        let sum_min: Au = range.clone().map(|c| cols[c].min).fold(Au::ZERO, |a, b| a + b);
        let sum_max: Au = range.clone().map(|c| cols[c].max).fold(Au::ZERO, |a, b| a + b);
        let need_min = mn - span_spacing - sum_min;
        if need_min > Au::ZERO {
            distribute(&mut cols, range.clone(), need_min, true);
        }
        let sum_max2: Au = range.clone().map(|c| cols[c].max).fold(Au::ZERO, |a, b| a + b);
        let need_max = mx - span_spacing - sum_max2.max(sum_max);
        if need_max > Au::ZERO {
            distribute(&mut cols, range.clone(), need_max, false);
        }
        if let Some(p) = pct {
            let have: i32 = range.clone().filter_map(|c| cols[c].percent).sum();
            if p > have {
                let free: Vec<usize> = range.clone().filter(|c| cols[*c].percent.is_none()).collect();
                if !free.is_empty() {
                    let each = (p - have) / free.len() as i32;
                    for c in free {
                        cols[c].percent = Some(each);
                    }
                }
            }
        }
    }
    for c in &mut cols {
        c.max = c.max.max(c.min);
    }
    cols
}

/// Adds `extra` to the columns in `range`, proportionally to their max widths
/// (evenly when all are zero); `to_min` also raises the min widths.
fn distribute(cols: &mut [ColInfo], range: std::ops::Range<usize>, extra: Au, to_min: bool) {
    let idx: Vec<usize> = range.collect();
    if idx.is_empty() {
        return;
    }
    let total: Au = idx.iter().map(|&c| cols[c].max).fold(Au::ZERO, |a, b| a + b);
    let mut given = Au::ZERO;
    let n = idx.len();
    for (k, &c) in idx.iter().enumerate() {
        let share = if k + 1 == n {
            extra - given
        } else if total > Au::ZERO {
            extra.scale(cols[c].max.0, total.0)
        } else {
            extra / n as i32
        };
        given += share;
        if to_min {
            cols[c].min += share;
            cols[c].max = cols[c].max.max(cols[c].min);
        } else {
            cols[c].max += share;
        }
    }
}

/// `(min, max, specified length, percent)` border-box widths of a cell.
fn cell_widths(ctx: &LayoutContext, id: BoxId, border: Edges) -> (Au, Au, Option<Au>, Option<i32>) {
    let s = ctx.style(id);
    let p = block::padding_edges(s, Au::ZERO);
    let e = p.horizontal() + border.horizontal();
    let (cmn, cmx) = intrinsic::content_min_max(ctx, id);
    let mut mn = cmn + e;
    let mut mx = cmx + e;
    let mut spec = None;
    let mut pct = None;
    match s.width {
        Sizing::Set(LengthPercentage::Length(w)) => {
            let bb = match s.box_sizing {
                BoxSizing::BorderBox => w.max(e),
                BoxSizing::ContentBox => w + e,
            };
            mn = mn.max(bb);
            mx = mx.max(bb);
            spec = Some(bb);
        }
        Sizing::Set(LengthPercentage::Percent(pc)) => pct = Some(pc),
        Sizing::Set(LengthPercentage::Calc(l, pc)) => {
            mn = mn.max(l + e);
            mx = mx.max(l + e);
            pct = Some(pc);
        }
        _ => {}
    }
    if s.white_space == crate::style::WhiteSpace::NoWrap {
        mn = mn.max(mx);
    }
    if let Some(mxw) = match s.max_width {
        Sizing::Set(LengthPercentage::Length(w)) => Some(w + e),
        _ => None,
    } {
        mx = mx.min(mxw).max(mn);
    }
    (mn, mx.max(mn), spec, pct)
}

/// `(min, max)` border-box widths of the grid box.
pub fn grid_intrinsic_widths(ctx: &LayoutContext, grid: BoxId) -> (Au, Au) {
    let prep = prepare(ctx, grid);
    let s = ctx.style(grid);
    let extra = prep.padding.horizontal() + prep.border.horizontal() + prep.spacing.0 * (prep.cols.len() as i32 + 1);
    let extra = if prep.cols.is_empty() { prep.padding.horizontal() + prep.border.horizontal() } else { extra };
    let sum_min: Au = prep.cols.iter().map(|c| c.min).fold(Au::ZERO, |a, b| a + b);
    let sum_max: Au = prep.cols.iter().map(|c| c.max).fold(Au::ZERO, |a, b| a + b);
    let (mut mn, mut mx) = (sum_min + extra, sum_max + extra);
    if let Sizing::Set(LengthPercentage::Length(w)) = s.width {
        mn = mn.max(w);
        mx = mn;
    }
    // Percentage columns widen the max-content width so they get their share.
    let pct_total: i32 = prep.cols.iter().filter_map(|c| c.percent).sum::<i32>().min(10_000);
    if pct_total > 0 && pct_total < 10_000 {
        let pct_max: Au = prep.cols.iter().filter(|c| c.percent.is_some()).map(|c| c.max).fold(Au::ZERO, |a, b| a + b);
        let non_pct: Au = prep.cols.iter().filter(|c| c.percent.is_none()).map(|c| c.max).fold(Au::ZERO, |a, b| a + b);
        let by_pct = pct_max.scale(10_000, pct_total);
        let by_rest = non_pct.scale(10_000, 10_000 - pct_total);
        mx = mx.max(by_pct.max(by_rest) + extra);
    }
    (mn, mx.max(mn))
}

/// `(min, max)` of the wrapper: the grid and its captions.
pub fn intrinsic_widths(ctx: &LayoutContext, wrapper: BoxId) -> (Au, Au) {
    let mut mn = Au::ZERO;
    let mut mx = Au::ZERO;
    for &c in ctx.tree.children(wrapper) {
        let (a, z) = intrinsic::min_max(ctx, c);
        mn = mn.max(a);
        mx = mx.max(z);
    }
    (mn, mx)
}

/// Used border-box width of the grid (§17.5.2): specified width or shrink-to-fit
/// within `avail`, never below the minimum.
fn used_table_width(ctx: &LayoutContext, grid: BoxId, cb: &Cb, avail: Au) -> Au {
    let s = ctx.style(grid);
    let (mn, mx) = intrinsic::min_max(ctx, grid);
    match s.width {
        Sizing::Set(lp) => lp.resolve(cb.width).max(mn),
        Sizing::MinContent => mn,
        Sizing::MaxContent => mx,
        _ => mn.max(avail.min(mx)),
    }
}

/// Column widths for a used inner width (the grid's content width minus spacing).
fn distribute_columns(cols: &[ColInfo], inner: Au, fixed_layout: bool) -> Vec<Au> {
    let n = cols.len();
    if n == 0 {
        return Vec::new();
    }
    let mut w = vec![Au::ZERO; n];
    if fixed_layout {
        // §17.5.2.1: fixed and percent columns as specified, the rest share equally.
        let mut used = Au::ZERO;
        let mut auto = Vec::new();
        for (i, c) in cols.iter().enumerate() {
            if let Some(p) = c.percent {
                w[i] = inner.percent_of(p);
            } else if let Some(f) = c.fixed {
                w[i] = f;
            } else {
                auto.push(i);
                continue;
            }
            used += w[i];
        }
        if !auto.is_empty() {
            let rem = (inner - used).max(Au::ZERO);
            let each = rem / auto.len() as i32;
            let mut given = Au::ZERO;
            for (k, &i) in auto.iter().enumerate() {
                w[i] = if k + 1 == auto.len() { rem - given } else { each };
                given += w[i];
            }
        } else if used < inner {
            // Extra goes to the columns proportionally.
            let extra = inner - used;
            let mut given = Au::ZERO;
            for i in 0..n {
                let share = if i + 1 == n { extra - given } else if used > Au::ZERO { extra.scale(w[i].0, used.0) } else { extra / n as i32 };
                w[i] += share;
                given += share;
            }
        }
        return w;
    }
    // Automatic layout.
    let pct_total: i32 = cols.iter().filter_map(|c| c.percent).sum();
    let scale = if pct_total > 10_000 { 10_000 } else { pct_total };
    let mut pct_targets = Au::ZERO;
    for (i, c) in cols.iter().enumerate() {
        if let Some(p) = c.percent {
            let p = if pct_total > 10_000 { (p as i64 * scale as i64 / pct_total as i64) as i32 } else { p };
            w[i] = inner.percent_of(p).max(c.min);
            pct_targets += w[i];
        }
    }
    let np: Vec<usize> = (0..n).filter(|&i| cols[i].percent.is_none()).collect();
    let sum_min: Au = np.iter().map(|&i| cols[i].min).fold(Au::ZERO, |a, b| a + b);
    let sum_max: Au = np.iter().map(|&i| cols[i].max).fold(Au::ZERO, |a, b| a + b);
    let mut rem = inner - pct_targets;
    if rem < sum_min {
        // Shrink percentage columns towards their minimums.
        let short = sum_min - rem;
        let mut slack: Au = (0..n).filter(|&i| cols[i].percent.is_some()).map(|i| w[i] - cols[i].min).fold(Au::ZERO, |a, b| a + b);
        let mut left = short;
        for i in (0..n).filter(|&i| cols[i].percent.is_some()) {
            if slack <= Au::ZERO || left <= Au::ZERO {
                break;
            }
            let give = (w[i] - cols[i].min).min(left);
            w[i] -= give;
            left -= give;
            slack -= give;
        }
        rem = inner - (0..n).filter(|&i| cols[i].percent.is_some()).map(|i| w[i]).fold(Au::ZERO, |a, b| a + b);
    }
    if np.is_empty() {
        // Only percentage columns: leftover goes to them proportionally.
        let total: Au = w.iter().copied().fold(Au::ZERO, |a, b| a + b);
        if total < inner && total > Au::ZERO {
            let extra = inner - total;
            let mut given = Au::ZERO;
            for i in 0..n {
                let share = if i + 1 == n { extra - given } else { extra.scale(w[i].0, total.0) };
                w[i] += share;
                given += share;
            }
        } else if total < inner {
            let each = inner / n as i32;
            for (i, v) in w.iter_mut().enumerate() {
                *v = if i + 1 == n { inner - each * (n as i32 - 1) } else { each };
            }
        }
        return w;
    }
    if rem >= sum_max {
        for &i in &np {
            w[i] = cols[i].max;
        }
        let extra = rem - sum_max;
        if extra > Au::ZERO {
            // To auto columns proportionally to max, else fixed ones, else percent ones.
            let auto: Vec<usize> = np.iter().copied().filter(|&i| cols[i].fixed.is_none()).collect();
            let targets: Vec<usize> = if !auto.is_empty() {
                auto
            } else {
                np.clone()
            };
            let total: Au = targets.iter().map(|&i| w[i]).fold(Au::ZERO, |a, b| a + b);
            let mut given = Au::ZERO;
            let m = targets.len();
            for (k, &i) in targets.iter().enumerate() {
                let share = if k + 1 == m { extra - given } else if total > Au::ZERO { extra.scale(w[i].0, total.0) } else { extra / m as i32 };
                w[i] += share;
                given += share;
            }
        }
    } else if rem >= sum_min {
        let range = sum_max - sum_min;
        let extra = rem - sum_min;
        let mut given = Au::ZERO;
        let m = np.len();
        for (k, &i) in np.iter().enumerate() {
            let share = if k + 1 == m { extra - given } else if range > Au::ZERO { extra.scale((cols[i].max - cols[i].min).0, range.0) } else { extra / m as i32 };
            w[i] = cols[i].min + share;
            given += share;
        }
    } else {
        for &i in &np {
            w[i] = cols[i].min;
        }
    }
    w
}

/// A laid-out cell before its row height is known.
struct CellLayout {
    idx: usize,
    fragment: Fragment,
    content_height: Au,
    baseline: Option<Au>,
    abs: Vec<AbsRequest>,
    is_empty: bool,
}

fn layout_cell(ctx: &LayoutContext, cell: &GridCell, border: Edges, width: Au, collapsed: Option<CollapsedBorders>) -> CellLayout {
    let id = cell.id;
    let b = &ctx.tree[id];
    let s = &b.style;
    let p = block::padding_edges(s, width);
    let content_w = (width - p.horizontal() - border.horizontal()).max(Au::ZERO);
    let mut bfc = Bfc::new();
    let cb = Cb { width: content_w, height: None };
    let contents = block::layout_contents(ctx, id, &cb, &mut bfc, Point::default(), false, false);
    let mut content_h = contents.height.max(bfc.float_bottom());
    let ev = p.vertical() + border.vertical();
    if let Some(h) = block::resolve_height(s, None, ev) {
        content_h = content_h.max(h);
    }
    content_h = block::clamp_height(s, content_h, None, ev);
    let baseline = contents.first_baseline.map(|bl| bl + border.top + p.top);
    let mut frag = Fragment::new(FragmentKind::Box { source: b.source, padding: p, border, replaced: None, scroll: None, baseline }, Rect::new(Au::ZERO, Au::ZERO, width, content_h + ev));
    let cx = border.left + p.left;
    let cy = border.top + p.top;
    let mut abs = contents.abs;
    block::translate_requests(&mut abs, cx, cy);
    let is_empty = contents.empty && contents.fragments.is_empty();
    for mut c in contents.fragments {
        c.rect.origin.x += cx;
        c.rect.origin.y += cy;
        frag.children.push(c);
    }
    frag.collapsed_borders = collapsed.map(Box::new);
    CellLayout { idx: 0, fragment: frag, content_height: content_h + ev, baseline, abs, is_empty }
}

/// The laid-out grid box at the origin, with its unresolved absolute requests and
/// the first row's baseline.
struct GridLayout {
    fragment: Fragment,
    abs: Vec<AbsRequest>,
    baseline: Option<Au>,
}

fn layout_grid(ctx: &LayoutContext, grid: BoxId, used_width: Au, cb: &Cb) -> GridLayout {
    let s = ctx.style(grid);
    let prep = prepare(ctx, grid);
    let st = &prep.st;
    let ncols = st.ncols;
    let nrows = st.rows.len();
    let (hs, vs) = prep.spacing;
    let edges_h = prep.padding.horizontal() + prep.border.horizontal();
    let inner = (used_width - edges_h - hs * (ncols as i32 + 1)).max(Au::ZERO);
    let fixed_layout = s.table_layout == TableLayout::Fixed && !matches!(s.width, Sizing::Auto);
    let widths = distribute_columns(&prep.cols, inner, fixed_layout);
    let col_sum: Au = widths.iter().copied().fold(Au::ZERO, |a, b| a + b);
    let content_w = if ncols == 0 { (used_width - edges_h).max(Au::ZERO) } else { (col_sum + hs * (ncols as i32 + 1)).max(used_width - edges_h) };
    let rtl = s.direction == Direction::Rtl;
    // Column x offsets (content-box relative), left to right in the visual order.
    let mut col_x = Vec::with_capacity(ncols + 1);
    let mut x = hs;
    for c in 0..ncols {
        let vc = if rtl { ncols - 1 - c } else { c };
        col_x.push(x);
        x += widths[vc] + hs;
    }
    col_x.push(x);
    let col_left = |c: usize, span: usize| -> (Au, Au) {
        // Visual span of columns c..c+span.
        if rtl {
            let vstart = ncols - (c + span);
            let x0 = col_x[vstart];
            let x1 = col_x[vstart + span] - hs;
            (x0, x1 - x0)
        } else {
            let x0 = col_x[c];
            let x1 = col_x[c + span] - hs;
            (x0, x1 - x0)
        }
    };

    // Lay out the cells.
    let mut cells: Vec<CellLayout> = Vec::with_capacity(st.cells.len());
    for (i, cell) in st.cells.iter().enumerate() {
        let (_, w) = col_left(cell.col, cell.colspan.min(ncols - cell.col));
        let mut cl = layout_cell(ctx, cell, prep.cell_borders[i], w, prep.cell_collapsed[i].clone());
        cl.idx = i;
        cells.push(cl);
    }

    // Row heights (§17.5.3).
    let mut row_h = vec![Au::ZERO; nrows];
    let mut row_baseline: Vec<Option<Au>> = vec![None; nrows];
    for (r, row) in st.rows.iter().enumerate() {
        let rs = ctx.style(row.id);
        if let Sizing::Set(LengthPercentage::Length(h)) = rs.height {
            row_h[r] = h;
        }
        let hide_empty = s.empty_cells == EmptyCells::Hide && prep.collapsed.is_none();
        let mut all_empty = true;
        let mut any = false;
        for cl in cells.iter().filter(|c| st.cells[c.idx].row == r) {
            any = true;
            if !cl.is_empty {
                all_empty = false;
            }
            let cell = st.cells[cl.idx];
            if cell.rowspan != 1 {
                continue;
            }
            let va = ctx.style(cell.id).vertical_align;
            if va == VerticalAlign::Baseline {
                if let Some(b) = cl.baseline {
                    row_baseline[r] = Some(row_baseline[r].map_or(b, |x| x.max(b)));
                }
            }
        }
        for cl in cells.iter().filter(|c| st.cells[c.idx].row == r && st.cells[c.idx].rowspan == 1) {
            let cell = st.cells[cl.idx];
            let va = ctx.style(cell.id).vertical_align;
            let h = match (va, cl.baseline, row_baseline[r]) {
                (VerticalAlign::Baseline, Some(b), Some(rb)) => cl.content_height + (rb - b),
                _ => cl.content_height,
            };
            row_h[r] = row_h[r].max(h);
        }
        if hide_empty && any && all_empty {
            row_h[r] = Au::ZERO;
        }
    }
    // Row-spanning cells.
    for cl in &cells {
        let cell = st.cells[cl.idx];
        if cell.rowspan <= 1 {
            continue;
        }
        let end = (cell.row + cell.rowspan).min(nrows);
        let total: Au = row_h[cell.row..end].iter().copied().fold(Au::ZERO, |a, b| a + b) + vs * (end as i32 - cell.row as i32 - 1);
        if cl.content_height > total {
            let extra = cl.content_height - total;
            let sum: Au = row_h[cell.row..end].iter().copied().fold(Au::ZERO, |a, b| a + b);
            let mut given = Au::ZERO;
            let n = end - cell.row;
            for (k, r) in (cell.row..end).enumerate() {
                let share = if k + 1 == n { extra - given } else if sum > Au::ZERO { extra.scale(row_h[r].0, sum.0) } else { extra / n as i32 };
                row_h[r] += share;
                given += share;
            }
        }
    }
    // Table height: extra space goes to the rows.
    let edges_v = prep.padding.vertical() + prep.border.vertical();
    let rows_total: Au = row_h.iter().copied().fold(Au::ZERO, |a, b| a + b) + if nrows > 0 { vs * (nrows as i32 + 1) } else { Au::ZERO };
    let content_h = match block::resolve_size(s.height, cb.height, edges_v, BoxSizing::BorderBox) {
        Some(h) if h > rows_total && nrows > 0 => {
            let extra = h - rows_total;
            let sum: Au = row_h.iter().copied().fold(Au::ZERO, |a, b| a + b);
            let mut given = Au::ZERO;
            for r in 0..nrows {
                let share = if r + 1 == nrows { extra - given } else if sum > Au::ZERO { extra.scale(row_h[r].0, sum.0) } else { extra / nrows as i32 };
                row_h[r] += share;
                given += share;
            }
            h
        }
        Some(h) => h.max(rows_total),
        None => rows_total,
    };
    // Row y offsets.
    let mut row_y = Vec::with_capacity(nrows + 1);
    let mut y = vs;
    for r in 0..nrows {
        row_y.push(y);
        y += row_h[r] + vs;
    }
    row_y.push(y);

    // Build fragments: cells into rows, rows into groups, groups into the grid.
    let cx = prep.border.left + prep.padding.left;
    let cy = prep.border.top + prep.padding.top;
    let mut grid_frag = Fragment::new(
        FragmentKind::Box { source: ctx.tree[grid].source, padding: prep.padding, border: prep.border, replaced: None, scroll: None, baseline: None },
        Rect::new(Au::ZERO, Au::ZERO, content_w + edges_h, content_h + edges_v),
    );
    grid_frag.collapsed_borders = prep.outer.map(Box::new);
    let mut abs_all = Vec::new();
    let mut cells: Vec<Option<CellLayout>> = cells.into_iter().map(Some).collect();
    // Column boxes: for paint (backgrounds), zero-height rows of columns.
    for &c in ctx.tree.children(grid) {
        let cbx = &ctx.tree[c];
        match &cbx.kind {
            BoxKind::ColGroup(_) | BoxKind::Col(_) => {
                let (first, span) = column_range(st, c);
                if let Some(first) = first {
                    let (x0, w) = col_left(first, span.min(ncols - first));
                    let f = Fragment::new(FragmentKind::Box { source: cbx.source, padding: Edges::ZERO, border: Edges::ZERO, replaced: None, scroll: None, baseline: None }, Rect::new(cx + x0, cy + vs, w, content_h - vs * 2));
                    grid_frag.children.push(f);
                    if cbx.kind != BoxKind::Col(crate::layout::boxes::ColBox { span: 0, width: None }) {
                        for &k in ctx.tree.children(c) {
                            let (kf, ks) = column_range(st, k);
                            if let Some(kf) = kf {
                                let (kx, kw) = col_left(kf, ks.min(ncols - kf));
                                let f = Fragment::new(FragmentKind::Box { source: ctx.tree[k].source, padding: Edges::ZERO, border: Edges::ZERO, replaced: None, scroll: None, baseline: None }, Rect::new(cx + kx, cy + vs, kw, content_h - vs * 2));
                                grid_frag.children.push(f);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut r = 0;
    let mut first_baseline = None;
    while r < nrows {
        let g = st.rows[r].group;
        let start = r;
        while r < nrows && st.rows[r].group == g {
            r += 1;
        }
        let end = r;
        let gy = row_y[start];
        let gh = row_y[end] - vs - gy;
        let mut gf = Fragment::new(FragmentKind::Box { source: ctx.tree[g].source, padding: Edges::ZERO, border: Edges::ZERO, replaced: None, scroll: None, baseline: None }, Rect::new(cx, cy + gy, content_w, gh.max(Au::ZERO)));
        for rr in start..end {
            let row = &st.rows[rr];
            let ry = row_y[rr] - gy;
            let mut rf = Fragment::new(FragmentKind::Box { source: ctx.tree[row.id].source, padding: Edges::ZERO, border: Edges::ZERO, replaced: None, scroll: None, baseline: None }, Rect::new(Au::ZERO, ry, content_w, row_h[rr]));
            for slot in cells.iter_mut() {
                let Some(cl) = slot else { continue };
                let cell = st.cells[cl.idx];
                if cell.row != rr {
                    continue;
                }
                let mut cl = slot.take().unwrap();
                let end_r = (cell.row + cell.rowspan).min(nrows);
                let h = row_y[end_r] - vs - row_y[cell.row];
                let (x0, _) = col_left(cell.col, cell.colspan.min(ncols - cell.col));
                let va = ctx.style(cell.id).vertical_align;
                let inner_h = cl.content_height;
                let shift = match va {
                    VerticalAlign::Baseline => match (cl.baseline, row_baseline[cell.row]) {
                        (Some(b), Some(rb)) if cell.rowspan == 1 => rb - b,
                        _ => Au::ZERO,
                    },
                    VerticalAlign::Middle => (h - inner_h) / 2,
                    VerticalAlign::Bottom | VerticalAlign::TextBottom => h - inner_h,
                    _ => Au::ZERO,
                }
                .max(Au::ZERO);
                if !shift.is_zero() {
                    for c in &mut cl.fragment.children {
                        c.rect.origin.y += shift;
                    }
                    block::translate_requests(&mut cl.abs, Au::ZERO, shift);
                }
                cl.fragment.rect = Rect::new(x0, Au::ZERO, cl.fragment.rect.size.width, h);
                if let FragmentKind::Box { baseline, .. } = &mut cl.fragment.kind {
                    *baseline = cl.baseline.map(|b| b + shift);
                }
                if rr == 0 && first_baseline.is_none() {
                    first_baseline = cl.baseline.map(|b| b + shift + cy + row_y[0]);
                }
                block::finish_fragment(ctx, cell.id, &mut cl.fragment);
                let mut abs = cl.abs;
                let unresolved = if ctx.style(cell.id).is_positioned() { block::resolve_absolutes(ctx, &mut cl.fragment, abs) } else { std::mem::take(&mut abs) };
                let mut unresolved = unresolved;
                block::translate_requests(&mut unresolved, x0 + cx, cy + row_y[cell.row]);
                abs_all.extend(unresolved);
                rf.children.push(cl.fragment);
            }
            block::finish_fragment(ctx, row.id, &mut rf);
            gf.children.push(rf);
        }
        block::finish_fragment(ctx, g, &mut gf);
        grid_frag.children.push(gf);
    }
    block::finish_fragment(ctx, grid, &mut grid_frag);
    GridLayout { fragment: grid_frag, abs: abs_all, baseline: first_baseline }
}

/// The first column index and span covered by a `<col>`/`<colgroup>` box.
fn column_range(st: &Structure, b: BoxId) -> (Option<usize>, usize) {
    let mut first = None;
    let mut span = 0;
    for c in 0..st.ncols {
        if st.cols[c] == Some(b) || st.colgroups[c] == Some(b) {
            if first.is_none() {
                first = Some(c);
            }
            span += 1;
        }
    }
    (first, span)
}

/// Lays out a table wrapper box as a block-level box (§17.4): the wrapper takes the
/// margins and floats, the grid the width; captions above and below. `avail` is the
/// shrink-to-fit availability for floats, absolutes and inline tables.
#[allow(clippy::too_many_arguments)]
pub fn layout_wrapper(ctx: &LayoutContext, wrapper: BoxId, cb: &Cb, bfc: &mut Bfc, cb_origin: Point, y_in: Au, avail: Option<Au>) -> BlockResult {
    let wb = &ctx.tree[wrapper];
    let ws = &wb.style;
    let grid = ctx.tree.children(wrapper).iter().copied().find(|c| ctx.tree[*c].kind == BoxKind::Table);
    let (mt, mb) = block::vertical_margins(ws, cb.width);
    let ml0 = ws.margin.left.resolve(cb.width).unwrap_or(Au::ZERO);
    let mr0 = ws.margin.right.resolve(cb.width).unwrap_or(Au::ZERO);
    let mut y = y_in;
    // Placement next to floats (BFC root).
    let mut x_base = Au::ZERO;
    let mut avail_w = avail.unwrap_or(cb.width);
    if avail.is_none() && !bfc.floats.is_empty() {
        let (mn, _) = intrinsic::min_max(ctx, wrapper);
        let mut tries = 0;
        loop {
            let (l, r) = bfc.available(cb_origin.y + y, cb_origin.x, cb_origin.x + cb.width);
            let a = r - l;
            if a >= mn + ml0 + mr0 || a >= cb.width || tries > 64 {
                x_base = l - cb_origin.x;
                avail_w = a;
                break;
            }
            tries += 1;
            match bfc.next_change(cb_origin.y + y) {
                Some(ny) => y = ny - cb_origin.y,
                None => {
                    x_base = l - cb_origin.x;
                    avail_w = a;
                    break;
                }
            }
        }
    }
    let used_w = match grid {
        Some(g) => used_table_width(ctx, g, cb, (avail_w - ml0 - mr0).max(Au::ZERO)),
        None => Au::ZERO,
    };
    // Captions can be wider than the grid.
    let mut caps: Vec<(BoxId, bool)> = Vec::new();
    for &c in ctx.tree.children(wrapper) {
        if ctx.tree[c].kind == BoxKind::Caption {
            caps.push((c, ctx.style(c).caption_side == crate::style::CaptionSide::Bottom));
        }
    }
    let wrapper_w = used_w;
    let (_, ml, mr) = block::block_horizontal(ws, avail_w, Some(wrapper_w), Au::ZERO);
    let mut frag = Fragment::new(FragmentKind::Box { source: wb.source, padding: Edges::ZERO, border: Edges::ZERO, replaced: None, scroll: None, baseline: None }, Rect::new(x_base + ml, y, wrapper_w, Au::ZERO));
    let mut cy = Au::ZERO;
    let mut abs = Vec::new();
    let mut baseline = None;
    let cap_cb = Cb { width: wrapper_w, height: None };
    let lay_caption = |c: BoxId, cy: &mut Au, frag: &mut Fragment, abs: &mut Vec<AbsRequest>| {
        let mut b2 = Bfc::new();
        let r = block::layout_block_level(ctx, c, &cap_cb, &mut b2, Point::default(), *cy + r_margin_top(ctx, c, wrapper_w));
        let mut f = r.fragment;
        let bottom = f.rect.bottom() + r.margin.bottom;
        block::translate_requests(&mut abs.clone(), Au::ZERO, Au::ZERO);
        let mut a = r.abs;
        block::translate_requests(&mut a, f.rect.origin.x, f.rect.origin.y);
        abs.extend(a);
        f.rect.origin.x += Au::ZERO;
        frag.children.push(f);
        *cy = bottom;
    };
    for &(c, bottom) in caps.iter().filter(|(_, b)| !*b) {
        let _ = bottom;
        lay_caption(c, &mut cy, &mut frag, &mut abs);
    }
    if let Some(g) = grid {
        let gl = layout_grid(ctx, g, used_w, cb);
        let mut gf = gl.fragment;
        gf.rect.origin = Point { x: Au::ZERO, y: cy };
        baseline = gl.baseline.map(|b| b + cy);
        let mut a = gl.abs;
        block::translate_requests(&mut a, Au::ZERO, cy);
        abs.extend(a);
        cy += gf.rect.size.height;
        frag.children.push(gf);
    }
    for &(c, _) in caps.iter().filter(|(_, b)| *b) {
        lay_caption(c, &mut cy, &mut frag, &mut abs);
    }
    frag.rect.size.height = cy;
    let unresolved = if ws.is_positioned() { block::resolve_absolutes(ctx, &mut frag, abs) } else { abs };
    block::finish_fragment(ctx, wrapper, &mut frag);
    BlockResult { fragment: frag, margin: Edges { top: mt, right: mr, bottom: mb, left: ml }, bottom_margins: MarginSet::of(mb), abs: unresolved, first_baseline: baseline, last_baseline: baseline }
}

fn r_margin_top(ctx: &LayoutContext, c: BoxId, cbw: Au) -> Au {
    block::vertical_margins(ctx.style(c), cbw).0
}

#[allow(dead_code)]
fn _unused(_: Size) {}
