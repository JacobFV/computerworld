//! CSS Grid Layout Level 1 (css-grid-1; subgrid and masonry excluded): the explicit
//! and implicit grids (§7) with `repeat(auto-fill|auto-fit)` expansion and named
//! lines and areas, line-based and automatic placement (§8), the track sizing
//! algorithm (§11) in fixed point, content and self alignment (css-align), item sizing
//! (§6.6 automatic minimums), absolutely positioned children (§9) and the fragments of
//! a grid container's contents.
//!
//! Entered from `block::layout_contents` for a grid container (the block code owns the
//! container's own width, height, scrollbars and edges), from
//! `intrinsic::content_min_max` for the container's min/max-content widths, and from
//! the box builder to wrap a container's children into grid items.
//!
//! `fr` arithmetic is done in `i64` with flex factors in 1/1000 units and lengths in
//! `Au`; nothing here uses floating point. Line numbers follow the spec: the explicit
//! grid's lines are 1..=N+1, implicit lines before it are 0, -1, … and after it N+2, ….
//! Track indices are `usize` offsets from the implicit grid's first line.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::geom::{Au, Edges, Point, Rect};
use crate::layout::block::{self, AbsRequest, Bfc, Cb, ContentsResult, MarginSet};
use crate::layout::boxes::{BoxId, BoxKind, LayoutBox, Level};
use crate::layout::fragment::{Fragment, FragmentKind, StyleSource};
use crate::layout::{intrinsic, text, LayoutContext};
use crate::style::{
    AlignContent, AlignItems, AlignSelf, AutoRepeat, ComputedStyle, Display, GridAutoFlow,
    GridLine, JustifyContent, LengthPercentage, LengthPercentageAuto, Sizing, TrackBreadth,
    TrackList, TrackSize,
};

/// "Infinite" growth limit.
const INF: Au = Au::MAX;

pub fn is_grid_container(s: &ComputedStyle) -> bool {
    matches!(s.display, Display::Grid | Display::InlineGrid)
}

// Box generation.

/// Wraps a grid container's children into grid items (§6): every in-flow child box is
/// blockified, each contiguous run of text (with `<br>`/`<wbr>`) becomes one
/// anonymous block item, and runs of collapsible white space are dropped. Absolutely
/// positioned children stay as they are (the container positions them, §9).
pub fn wrap_grid_items(
    boxes: &mut Vec<LayoutBox>,
    container: BoxId,
    kids: Vec<BoxId>,
) -> Vec<BoxId> {
    let parent_style = boxes[container.index()].style.clone();
    let anon = StyleSource::Anonymous(boxes[container.index()].source.node());
    let mut out = Vec::new();
    let mut run: Vec<BoxId> = Vec::new();
    fn flush(
        boxes: &mut Vec<LayoutBox>,
        container: BoxId,
        run: &mut Vec<BoxId>,
        out: &mut Vec<BoxId>,
        parent_style: &Rc<ComputedStyle>,
        source: StyleSource,
    ) {
        if run.is_empty() {
            return;
        }
        let has_content = run.iter().any(|k| match &boxes[k.index()].kind {
            BoxKind::Text(t) => {
                !text::is_collapsible_whitespace(&t.text, boxes[k.index()].style.white_space)
            }
            _ => true,
        });
        if !has_content {
            run.clear();
            return;
        }
        let mut s = ComputedStyle::inherit_from(parent_style);
        s.display = Display::Block;
        let id = BoxId(boxes.len() as u32);
        // Clone the container's box so new fields keep compiling; every field that
        // matters is reset below.
        let mut b = boxes[container.index()].clone();
        b.kind = BoxKind::Block;
        b.style = Rc::new(s);
        b.source = source;
        b.node = None;
        b.level = Level::Block;
        b.children = std::mem::take(run);
        b.inline_children = true;
        b.marker = None;
        b.control = None;
        b.is_root = false;
        b.split_first = true;
        b.split_last = true;
        b.is_item = true;
        boxes.push(b);
        out.push(id);
    }
    for k in kids {
        if matches!(
            boxes[k.index()].kind,
            BoxKind::Text(_) | BoxKind::Br(_) | BoxKind::Wbr
        ) {
            run.push(k);
            continue;
        }
        flush(boxes, container, &mut run, &mut out, &parent_style, anon);
        let has_block_child = boxes[k.index()]
            .children
            .iter()
            .any(|c| boxes[c.index()].level == Level::Block && !boxes[c.index()].is_out_of_flow());
        let b = &mut boxes[k.index()];
        b.level = Level::Block;
        match b.kind {
            BoxKind::Inline => {
                b.kind = BoxKind::Block;
                b.inline_children = !has_block_child;
                b.split_first = true;
                b.split_last = true;
            }
            BoxKind::InlineBlock => b.kind = BoxKind::Block,
            _ => {}
        }
        if !b.is_abs() {
            b.is_item = true;
        }
        out.push(k);
    }
    flush(boxes, container, &mut run, &mut out, &parent_style, anon);
    out
}

// Sizing constraints and track sizing functions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Avail {
    Definite(Au),
    MinContent,
    MaxContent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MinFn {
    Fixed(Au),
    Auto,
    MinContent,
    MaxContent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MaxFn {
    Fixed(Au),
    Auto,
    MinContent,
    MaxContent,
    /// Flex factor in 1/1000.
    Flex(i32),
    FitContent(Au),
}

#[derive(Clone, Debug)]
struct Track {
    min: MinFn,
    max: MaxFn,
    base: Au,
    limit: Au,
    inf_growable: bool,
    /// An empty `auto-fit` track: fixed at zero, its gutters collapsed.
    collapsed: bool,
}

impl Track {
    fn new(min: MinFn, max: MaxFn) -> Track {
        Track {
            min,
            max,
            base: Au::ZERO,
            limit: INF,
            inf_growable: false,
            collapsed: false,
        }
    }
    fn intrinsic_min(&self) -> bool {
        !matches!(self.min, MinFn::Fixed(_))
    }
    fn flex(&self) -> Option<i32> {
        match self.max {
            MaxFn::Flex(f) => Some(f.max(0)),
            _ => None,
        }
    }
    /// An intrinsic max sizing function; a `fit-content()` one only while its growth
    /// limit (plus `extra`) is under the argument (§11.5.1).
    fn intrinsic_max(&self, extra: Au) -> bool {
        match self.max {
            MaxFn::Auto | MaxFn::MinContent | MaxFn::MaxContent => true,
            MaxFn::FitContent(arg) => self.limit == INF || self.limit + extra < arg,
            _ => false,
        }
    }
    /// A max-content (or `auto`, treated as max-content) max sizing function.
    fn max_content_max(&self, extra: Au) -> bool {
        match self.max {
            MaxFn::Auto | MaxFn::MaxContent => true,
            MaxFn::FitContent(arg) => self.limit == INF || self.limit + extra < arg,
            _ => false,
        }
    }
}

fn breadth_min(b: TrackBreadth, base: Option<Au>) -> MinFn {
    match b {
        TrackBreadth::Fixed(lp) => match lp.maybe_resolve(base) {
            Some(v) => MinFn::Fixed(v.max(Au::ZERO)),
            None => MinFn::Auto,
        },
        TrackBreadth::Flex(_) | TrackBreadth::Auto => MinFn::Auto,
        TrackBreadth::MinContent => MinFn::MinContent,
        TrackBreadth::MaxContent => MinFn::MaxContent,
    }
}

fn breadth_max(b: TrackBreadth, base: Option<Au>) -> MaxFn {
    match b {
        TrackBreadth::Fixed(lp) => match lp.maybe_resolve(base) {
            Some(v) => MaxFn::Fixed(v.max(Au::ZERO)),
            None => MaxFn::Auto,
        },
        TrackBreadth::Flex(f) => MaxFn::Flex(f),
        TrackBreadth::Auto => MaxFn::Auto,
        TrackBreadth::MinContent => MaxFn::MinContent,
        TrackBreadth::MaxContent => MaxFn::MaxContent,
    }
}

/// The min and max track sizing functions of a track size (§7.2.1); percentages
/// against `base`, or `auto` when it is indefinite.
fn track_fns(t: &TrackSize, base: Option<Au>) -> (MinFn, MaxFn) {
    match *t {
        TrackSize::Fixed(lp) => match lp.maybe_resolve(base) {
            Some(v) => (MinFn::Fixed(v.max(Au::ZERO)), MaxFn::Fixed(v.max(Au::ZERO))),
            None => (MinFn::Auto, MaxFn::Auto),
        },
        TrackSize::Flex(f) => (MinFn::Auto, MaxFn::Flex(f)),
        TrackSize::Auto => (MinFn::Auto, MaxFn::Auto),
        TrackSize::MinContent => (MinFn::MinContent, MaxFn::MinContent),
        TrackSize::MaxContent => (MinFn::MaxContent, MaxFn::MaxContent),
        TrackSize::MinMax(a, b) => (breadth_min(a, base), breadth_max(b, base)),
        TrackSize::FitContent(lp) => match lp.maybe_resolve(base) {
            Some(v) => (MinFn::Auto, MaxFn::FitContent(v.max(Au::ZERO))),
            None => (MinFn::Auto, MaxFn::MaxContent),
        },
    }
}

// The explicit grid (§7.1–7.3).

#[derive(Clone, Debug, Default)]
struct ExplicitAxis {
    tracks: Vec<TrackSize>,
    /// Names of line `i` (0-based; `names.len() == tracks.len() + 1`).
    names: Vec<Vec<String>>,
    /// Track index range produced by the auto repeat, for `auto-fit` collapsing.
    repeat: Option<(usize, usize)>,
    auto_fit: bool,
}

/// A track's size for counting auto repetitions (§7.2.3.2): its max sizing function
/// when definite, else its min sizing function, floored by the min.
fn fixed_size(t: &TrackSize, base: Option<Au>) -> Au {
    let fixed = |lp: LengthPercentage| lp.maybe_resolve(base).unwrap_or(Au::ZERO).max(Au::ZERO);
    match *t {
        TrackSize::Fixed(lp) => fixed(lp),
        TrackSize::MinMax(a, b) => {
            let mn = match a {
                TrackBreadth::Fixed(lp) => fixed(lp),
                _ => Au::ZERO,
            };
            match b {
                TrackBreadth::Fixed(lp) => fixed(lp).max(mn),
                _ => mn,
            }
        }
        _ => Au::ZERO,
    }
}

/// The number of repetitions of an `auto-fill`/`auto-fit` repeat (§7.2.3.2) given the
/// axis's definite (or max) size, else its min size, else 1.
fn auto_repeat_count(
    tl: &TrackList,
    rep: &AutoRepeat,
    avail: Option<Au>,
    min_size: Option<Au>,
    gap: Au,
    base: Option<Au>,
) -> usize {
    let outside: i64 = tl.tracks.iter().map(|t| fixed_size(t, base).0 as i64).sum();
    let outside_n = tl.tracks.len() as i64;
    let rep_sum: i64 = rep
        .tracks
        .iter()
        .map(|t| fixed_size(t, base).0 as i64)
        .sum();
    let rep_n = rep.tracks.len() as i64;
    let g = gap.0 as i64;
    let a = outside + g * (outside_n - 1);
    let denom = rep_sum + g * rep_n;
    if denom <= 0 {
        return 1;
    }
    if let Some(av) = avail {
        let n = (av.0 as i64 - a).div_euclid(denom);
        return n.max(1) as usize;
    }
    if let Some(mn) = min_size {
        let need = mn.0 as i64 - a;
        let n = if need <= 0 {
            1
        } else {
            (need + denom - 1) / denom
        };
        return n.max(1) as usize;
    }
    1
}

/// Expands a track list's auto repeat `count` times, merging line names at the seams.
fn expand(tl: &TrackList, count: usize) -> ExplicitAxis {
    let mut out = ExplicitAxis::default();
    let mut cur: Vec<String> = Vec::new();
    let names_at = |i: usize| -> Vec<String> { tl.line_names.get(i).cloned().unwrap_or_default() };
    let insert_repeat = |out: &mut ExplicitAxis, cur: &mut Vec<String>, rep: &AutoRepeat| {
        let start = out.tracks.len();
        for _ in 0..count {
            for (j, t) in rep.tracks.iter().enumerate() {
                if let Some(n) = rep.line_names.get(j) {
                    cur.extend(n.iter().cloned());
                }
                out.names.push(std::mem::take(cur));
                out.tracks.push(*t);
            }
            if let Some(n) = rep.line_names.get(rep.tracks.len()) {
                cur.extend(n.iter().cloned());
            }
        }
        out.repeat = Some((start, out.tracks.len()));
        out.auto_fit = !rep.fill;
    };
    for (i, t) in tl.tracks.iter().enumerate() {
        if let Some(rep) = &tl.auto_repeat {
            if rep.at == i {
                insert_repeat(&mut out, &mut cur, rep);
            }
        }
        cur.extend(names_at(i));
        out.names.push(std::mem::take(&mut cur));
        out.tracks.push(*t);
    }
    if let Some(rep) = &tl.auto_repeat {
        if rep.at >= tl.tracks.len() {
            insert_repeat(&mut out, &mut cur, rep);
        }
    }
    cur.extend(names_at(tl.tracks.len()));
    out.names.push(cur);
    out
}

/// Named lines of one axis: explicit names plus the implicit `*-start`/`*-end` lines
/// of the template areas, in line order (§7.3.2).
#[derive(Clone, Debug, Default)]
struct Names {
    lines: BTreeMap<String, Vec<i32>>,
    /// Number of explicit tracks; the explicit lines are `1..=explicit + 1`.
    explicit: i32,
}

impl Names {
    fn build(
        axis: &ExplicitAxis,
        areas: &BTreeMap<String, (i32, i32, i32, i32)>,
        rows: bool,
        explicit: i32,
    ) -> Names {
        let mut lines: BTreeMap<String, Vec<i32>> = BTreeMap::new();
        for (i, ns) in axis.names.iter().enumerate() {
            for n in ns {
                lines.entry(n.clone()).or_default().push(i as i32 + 1);
            }
        }
        for (name, &(r0, r1, c0, c1)) in areas {
            let (a, b) = if rows { (r0, r1) } else { (c0, c1) };
            lines.entry(format!("{name}-start")).or_default().push(a);
            lines.entry(format!("{name}-end")).or_default().push(b);
        }
        for v in lines.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        Names { lines, explicit }
    }
    fn named(&self, name: &str) -> &[i32] {
        self.lines.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }
    /// `<integer> <name>?` (§8.3): the nth line, counting from the end when negative;
    /// lines beyond the explicit grid all carry every name.
    fn by_number(&self, n: i32, name: Option<&str>) -> i32 {
        match name {
            None => {
                if n > 0 {
                    n
                } else {
                    self.explicit + 2 + n
                }
            }
            Some(nm) => {
                let l = self.named(nm);
                let m = l.len() as i32;
                if n > 0 {
                    if n <= m {
                        l[(n - 1) as usize]
                    } else {
                        self.explicit + 1 + (n - m)
                    }
                } else {
                    let k = -n;
                    if k <= m {
                        l[(m - k) as usize]
                    } else {
                        1 - (k - m)
                    }
                }
            }
        }
    }
    /// `span <n> <name>?` from a definite line, forwards or backwards.
    fn nth_from(&self, from: i32, n: i32, name: Option<&str>, forward: bool) -> i32 {
        let n = n.max(1);
        match name {
            None => {
                if forward {
                    from.saturating_add(n)
                } else {
                    from.saturating_sub(n)
                }
            }
            Some(nm) => {
                let l = self.named(nm);
                let mut found = 0;
                if forward {
                    for &x in l {
                        if x > from {
                            found += 1;
                            if found == n {
                                return x;
                            }
                        }
                    }
                    from.max(self.explicit + 1) + (n - found)
                } else {
                    for &x in l.iter().rev() {
                        if x < from {
                            found += 1;
                            if found == n {
                                return x;
                            }
                        }
                    }
                    from.min(1) - (n - found)
                }
            }
        }
    }
}

// Placement (§8).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Res {
    /// Definite start and end lines.
    Definite(i32, i32),
    /// Auto-placed with this span.
    Span(i32),
}

/// Resolves one axis of an item's placement from its two placement properties
/// (§8.3.1).
fn resolve_axis(names: &Names, start: &GridLine, end: &GridLine) -> Res {
    enum P {
        Auto,
        Line(i32),
        Span(i32, Option<String>),
    }
    let conv = |g: &GridLine, is_start: bool| -> P {
        match g {
            GridLine::Auto => P::Auto,
            GridLine::Line(0, _) => P::Auto,
            GridLine::Line(n, nm) => P::Line(names.by_number(*n, nm.as_deref())),
            GridLine::Span(n, nm) => P::Span((*n).clamp(1, 10_000) as i32, nm.clone()),
            GridLine::Name(s) => {
                let key = if is_start {
                    format!("{s}-start")
                } else {
                    format!("{s}-end")
                };
                if let Some(&x) = names.named(&key).first() {
                    return P::Line(x);
                }
                P::Line(names.by_number(1, Some(s)))
            }
        }
    };
    match (conv(start, true), conv(end, false)) {
        (P::Line(a), P::Line(b)) => {
            if a == b {
                Res::Definite(a, a + 1)
            } else if b < a {
                Res::Definite(b, a)
            } else {
                Res::Definite(a, b)
            }
        }
        (P::Line(a), P::Span(n, nm)) => Res::Definite(a, names.nth_from(a, n, nm.as_deref(), true)),
        (P::Span(n, nm), P::Line(b)) => {
            Res::Definite(names.nth_from(b, n, nm.as_deref(), false), b)
        }
        (P::Line(a), P::Auto) => Res::Definite(a, a + 1),
        (P::Auto, P::Line(b)) => Res::Definite(b - 1, b),
        (P::Span(n, nm), P::Auto) | (P::Auto, P::Span(n, nm)) | (P::Span(n, nm), P::Span(_, _)) => {
            Res::Span(if nm.is_some() { 1 } else { n })
        }
        (P::Auto, P::Auto) => Res::Span(1),
    }
}

/// Occupancy of the implicit grid during auto-placement, in (major, minor) cells
/// offset from the implicit grid's first lines; grows on demand.
struct Occupancy {
    major0: i32,
    minor0: i32,
    cells: Vec<Vec<bool>>,
}

impl Occupancy {
    fn free(&self, major: (i32, i32), minor: (i32, i32)) -> bool {
        for r in major.0..major.1 {
            let ri = (r - self.major0) as usize;
            let Some(row) = self.cells.get(ri) else {
                continue;
            };
            for c in minor.0..minor.1 {
                let ci = (c - self.minor0) as usize;
                if row.get(ci).copied().unwrap_or(false) {
                    return false;
                }
            }
        }
        true
    }
    fn fill(&mut self, major: (i32, i32), minor: (i32, i32)) {
        let need_rows = (major.1 - self.major0).max(0) as usize;
        if self.cells.len() < need_rows {
            self.cells.resize(need_rows, Vec::new());
        }
        let need_cols = (minor.1 - self.minor0).max(0) as usize;
        for r in major.0..major.1 {
            let row = &mut self.cells[(r - self.major0) as usize];
            if row.len() < need_cols {
                row.resize(need_cols, false);
            }
            for c in minor.0..minor.1 {
                row[(c - self.minor0) as usize] = true;
            }
        }
    }
}

/// An item's definite `((major start, end), (minor start, end))` lines.
type Placed = ((i32, i32), (i32, i32));

/// The auto-placement algorithm (§8.5) over (major, minor) axes: the major axis is
/// the one that grows (rows for `grid-auto-flow: row`). Returns the definite
/// `((major start, end), (minor start, end))` of every item.
fn auto_place(items: &[(Res, Res)], explicit_minor: i32, dense: bool) -> Vec<Placed> {
    let mut minor0 = 1;
    let mut minor_end = explicit_minor + 1;
    let mut major0 = 1;
    let mut max_span = 1;
    for (maj, min) in items {
        match *min {
            Res::Definite(a, b) => {
                minor0 = minor0.min(a);
                minor_end = minor_end.max(b);
            }
            Res::Span(k) => max_span = max_span.max(k),
        }
        if let Res::Definite(a, _) = *maj {
            major0 = major0.min(a);
        }
    }
    if minor_end - minor0 < max_span {
        minor_end = minor0 + max_span;
    }
    let mut occ = Occupancy {
        major0,
        minor0,
        cells: Vec::new(),
    };
    let mut out: Vec<Option<Placed>> = vec![None; items.len()];
    // Step 1: items with definite positions in both axes.
    for (i, (maj, min)) in items.iter().enumerate() {
        if let (Res::Definite(a, b), Res::Definite(c, d)) = (*maj, *min) {
            out[i] = Some(((a, b), (c, d)));
            occ.fill((a, b), (c, d));
        }
    }
    // Step 2: items locked in the major axis.
    let mut row_cursor: BTreeMap<i32, i32> = BTreeMap::new();
    for (i, (maj, min)) in items.iter().enumerate() {
        if let (Res::Definite(a, b), Res::Span(k)) = (*maj, *min) {
            let mut c = if dense {
                minor0
            } else {
                row_cursor.get(&a).copied().unwrap_or(minor0)
            };
            while !occ.free((a, b), (c, c + k)) {
                c += 1;
            }
            out[i] = Some(((a, b), (c, c + k)));
            occ.fill((a, b), (c, c + k));
            row_cursor.insert(a, c + k);
        }
    }
    // Step 3: the auto-placement cursor.
    let (mut cm, mut cn) = (major0, minor0);
    for (i, (maj, min)) in items.iter().enumerate() {
        if out[i].is_some() {
            continue;
        }
        match (*maj, *min) {
            (Res::Span(k), Res::Definite(a, b)) => {
                if dense {
                    cm = major0;
                } else if a < cn {
                    cm += 1;
                }
                cn = a;
                while !occ.free((cm, cm + k), (a, b)) {
                    cm += 1;
                }
                out[i] = Some(((cm, cm + k), (a, b)));
                occ.fill((cm, cm + k), (a, b));
            }
            (Res::Span(k), Res::Span(j)) => {
                if dense {
                    cm = major0;
                    cn = minor0;
                }
                loop {
                    if cn + j > minor_end {
                        cm += 1;
                        cn = minor0;
                        continue;
                    }
                    if occ.free((cm, cm + k), (cn, cn + j)) {
                        break;
                    }
                    cn += 1;
                }
                out[i] = Some(((cm, cm + k), (cn, cn + j)));
                occ.fill((cm, cm + k), (cn, cn + j));
            }
            _ => {}
        }
    }
    out.into_iter()
        .map(|o| o.unwrap_or(((1, 2), (1, 2))))
        .collect()
}

// Alignment.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Start,
    End,
    Center,
    Stretch,
    Baseline,
}

fn resolve_self(self_v: AlignSelf, items_v: AlignItems) -> Align {
    match self_v {
        AlignSelf::Auto => match items_v {
            AlignItems::Stretch => Align::Stretch,
            AlignItems::FlexStart | AlignItems::Start | AlignItems::SelfStart => Align::Start,
            AlignItems::FlexEnd | AlignItems::End | AlignItems::SelfEnd => Align::End,
            AlignItems::Center => Align::Center,
            AlignItems::Baseline => Align::Baseline,
        },
        AlignSelf::Stretch => Align::Stretch,
        AlignSelf::FlexStart | AlignSelf::Start => Align::Start,
        AlignSelf::FlexEnd | AlignSelf::End => Align::End,
        AlignSelf::Center => Align::Center,
        AlignSelf::Baseline => Align::Baseline,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dist {
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Stretch,
}

/// `justify-content` on a grid container. The computed value's initial is
/// `flex-start` (the enum has no `normal`), which behaves as `normal`, i.e. `stretch`.
fn dist_justify(j: JustifyContent) -> Dist {
    match j {
        JustifyContent::FlexStart | JustifyContent::Stretch => Dist::Stretch,
        JustifyContent::Start | JustifyContent::Left => Dist::Start,
        JustifyContent::FlexEnd | JustifyContent::End | JustifyContent::Right => Dist::End,
        JustifyContent::Center => Dist::Center,
        JustifyContent::SpaceBetween => Dist::SpaceBetween,
        JustifyContent::SpaceAround => Dist::SpaceAround,
        JustifyContent::SpaceEvenly => Dist::SpaceEvenly,
    }
}

fn dist_align(a: AlignContent) -> Dist {
    match a {
        AlignContent::Normal | AlignContent::Stretch => Dist::Stretch,
        AlignContent::FlexStart | AlignContent::Start => Dist::Start,
        AlignContent::FlexEnd | AlignContent::End => Dist::End,
        AlignContent::Center => Dist::Center,
        AlignContent::SpaceBetween => Dist::SpaceBetween,
        AlignContent::SpaceAround => Dist::SpaceAround,
        AlignContent::SpaceEvenly => Dist::SpaceEvenly,
    }
}

// Track sizing (§11).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Contrib {
    Minimum,
    MinContent,
    MaxContent,
    /// Min/max-content contribution limited by the tracks' fixed max sizing functions
    /// and floored by the minimum contribution (§11.5, under a size constraint).
    LimitedMinContent,
    LimitedMaxContent,
}

/// An item's outer contribution of `kind` in the axis being sized. The third argument
/// is the sum of the spanned tracks' fixed max sizing functions (plus gutters) when
/// every spanned track has one: the clamp on the automatic minimum size (§6.6).
type ContribFn<'x> = dyn FnMut(usize, Contrib, Option<Au>) -> Au + 'x;

fn split(space: Au, n: usize, i: usize) -> Au {
    let n = n.max(1) as i32;
    let base = space.0 / n;
    let rem = space.0 % n;
    Au(base + if (i as i32) < rem { 1 } else { 0 })
}

/// Gutters inside a span: one gap between each pair of consecutive non-collapsed
/// tracks.
fn gaps_between(tracks: &[Track], gap: Au, s: usize, e: usize) -> Au {
    let n = tracks[s..e].iter().filter(|t| !t.collapsed).count();
    if n >= 2 {
        gap * (n as i32 - 1)
    } else {
        Au::ZERO
    }
}

fn total_size(tracks: &[Track], gap: Au) -> Au {
    let mut sum = Au::ZERO;
    for t in tracks {
        if !t.collapsed {
            sum += t.base;
        }
    }
    sum + gaps_between(tracks, gap, 0, tracks.len())
}

/// The spanned tracks' fixed max sizes plus gutters when all are fixed (optionally
/// counting `fit-content()` arguments as fixed).
fn fixed_area(tracks: &[Track], gap: Au, s: usize, e: usize, allow_fit: bool) -> Option<Au> {
    let mut sum = Au::ZERO;
    for t in &tracks[s..e] {
        match t.max {
            MaxFn::Fixed(v) => sum += v,
            MaxFn::FitContent(v) if allow_fit => sum += v,
            _ => return None,
        }
    }
    Some(sum + gaps_between(tracks, gap, s, e))
}

fn item_contrib(
    tracks: &[Track],
    gap: Au,
    s: usize,
    e: usize,
    idx: usize,
    kind: Contrib,
    contrib: &mut ContribFn,
) -> Au {
    let fa = fixed_area(tracks, gap, s, e, false);
    let limited = |k: Contrib, contrib: &mut ContribFn| -> Au {
        let c = contrib(idx, k, fa);
        let mn = contrib(idx, Contrib::Minimum, fa);
        match fixed_area(tracks, gap, s, e, true) {
            Some(f) => c.min(f).max(mn),
            None => c,
        }
    };
    match kind {
        Contrib::Minimum | Contrib::MinContent | Contrib::MaxContent => contrib(idx, kind, fa),
        Contrib::LimitedMinContent => limited(Contrib::MinContent, contrib),
        Contrib::LimitedMaxContent => limited(Contrib::MaxContent, contrib),
    }
}

/// Distributes extra space to accommodate spanning items (§11.5.1): to base sizes
/// (`to_limits == false`) or growth limits, among the spanned tracks accepted by
/// `affected`.
#[allow(clippy::too_many_arguments)]
fn distribute(
    tracks: &mut [Track],
    gap: Au,
    items: &[(usize, usize, usize)],
    kind: Contrib,
    to_limits: bool,
    affected: &dyn Fn(&Track) -> bool,
    contrib: &mut ContribFn,
) {
    let n = tracks.len();
    let mut planned = vec![Au::ZERO; n];
    let mut incurred = vec![Au::ZERO; n];
    let mut rooms = vec![Au::ZERO; n];
    for &(s, e, idx) in items {
        let aff: Vec<usize> = (s..e)
            .filter(|&k| !tracks[k].collapsed && affected(&tracks[k]))
            .collect();
        if aff.is_empty() {
            continue;
        }
        let c = item_contrib(tracks, gap, s, e, idx, kind, contrib);
        let size = |t: &Track| {
            if to_limits {
                if t.limit == INF {
                    t.base
                } else {
                    t.limit
                }
            } else {
                t.base
            }
        };
        let mut space = c
            - tracks[s..e].iter().map(size).fold(Au::ZERO, |a, b| a + b)
            - gaps_between(tracks, gap, s, e);
        if space <= Au::ZERO {
            continue;
        }
        for v in &mut incurred[s..e] {
            *v = Au::ZERO;
        }
        for &k in &aff {
            let t = &tracks[k];
            rooms[k] = if to_limits {
                if t.inf_growable || t.limit == INF {
                    match t.max {
                        MaxFn::FitContent(arg) => (arg - size(t)).max(Au::ZERO),
                        _ => INF,
                    }
                } else {
                    Au::ZERO
                }
            } else {
                let l = match t.max {
                    MaxFn::FitContent(arg) => t.limit.min(arg),
                    _ => t.limit,
                };
                if l == INF {
                    INF
                } else {
                    (l - t.base).max(Au::ZERO)
                }
            };
        }
        // Distribute space up to limits.
        let mut active = aff.clone();
        loop {
            active.retain(|&k| rooms[k] == INF || incurred[k] < rooms[k]);
            if active.is_empty() || space <= Au::ZERO {
                break;
            }
            let total = space;
            let m = active.len();
            let mut progressed = false;
            for (i, &k) in active.iter().enumerate() {
                let give = split(total, m, i);
                let g = if rooms[k] == INF {
                    give
                } else {
                    give.min(rooms[k] - incurred[k])
                };
                incurred[k] += g;
                space -= g;
                progressed |= g > Au::ZERO;
            }
            if !progressed {
                break;
            }
        }
        // Distribute space beyond limits.
        if space > Au::ZERO {
            let mut targets: Vec<usize> = aff
                .iter()
                .copied()
                .filter(|&k| {
                    let t = &tracks[k];
                    if to_limits {
                        true
                    } else {
                        match kind {
                            Contrib::MaxContent | Contrib::LimitedMaxContent => {
                                t.max_content_max(incurred[k])
                            }
                            _ => t.intrinsic_max(incurred[k]),
                        }
                    }
                })
                .collect();
            if targets.is_empty() {
                targets = aff.clone();
            }
            let m = targets.len();
            let total = space;
            for (i, &k) in targets.iter().enumerate() {
                incurred[k] += split(total, m, i);
            }
        }
        for k in s..e {
            if incurred[k] > planned[k] {
                planned[k] = incurred[k];
            }
        }
    }
    for k in 0..n {
        if planned[k] <= Au::ZERO {
            continue;
        }
        let t = &mut tracks[k];
        if to_limits {
            if t.limit == INF {
                t.limit = t.base + planned[k];
                t.inf_growable = true;
            } else {
                t.limit += planned[k];
            }
        } else {
            t.base += planned[k];
            if t.limit != INF && t.limit < t.base {
                t.limit = t.base;
            }
        }
    }
}

/// §11.5 step 4: items crossing flexible tracks grow the flexible tracks' base sizes
/// in proportion to their flex factors (all other tracks treated as fixed).
fn distribute_flex(
    tracks: &mut [Track],
    gap: Au,
    items: &[(usize, usize, usize)],
    kind: Contrib,
    contrib: &mut ContribFn,
) {
    let n = tracks.len();
    let mut planned = vec![Au::ZERO; n];
    for &(s, e, idx) in items {
        let aff: Vec<(usize, i64)> = (s..e)
            .filter_map(|k| {
                tracks[k]
                    .flex()
                    .filter(|_| !tracks[k].collapsed && tracks[k].min == MinFn::Auto)
                    .map(|f| (k, f as i64))
            })
            .collect();
        if aff.is_empty() {
            continue;
        }
        let c = item_contrib(tracks, gap, s, e, idx, kind, contrib);
        let space = c
            - tracks[s..e]
                .iter()
                .map(|t| t.base)
                .fold(Au::ZERO, |a, b| a + b)
            - gaps_between(tracks, gap, s, e);
        if space <= Au::ZERO {
            continue;
        }
        let sum_f: i64 = aff.iter().map(|&(_, f)| f).sum();
        let m = aff.len();
        for (i, &(k, f)) in aff.iter().enumerate() {
            let inc = if sum_f >= 1000 {
                Au(((space.0 as i64) * f / sum_f).clamp(0, Au::MAX.0 as i64) as i32)
            } else {
                // Less than one fr in total: that proportion by ratio, the rest equally.
                let by_ratio = (space.0 as i64) * f / 1000;
                let rest = space.0 as i64 - (space.0 as i64) * sum_f / 1000;
                Au(
                    (by_ratio + split(Au(rest as i32), m, i).0 as i64).clamp(0, Au::MAX.0 as i64)
                        as i32,
                )
            };
            planned[k] = planned[k].max(inc);
        }
    }
    for k in 0..n {
        if planned[k] > Au::ZERO {
            tracks[k].base += planned[k];
        }
    }
}

/// Finds the size of an fr (§11.7.1) for the tracks `s..e` and a space to fill.
fn find_fr(tracks: &[Track], gap: Au, s: usize, e: usize, space: Au) -> Au {
    let mut leftover: i64 = space.0 as i64 - gaps_between(tracks, gap, s, e).0 as i64;
    let mut inflexible = vec![false; e - s];
    for (i, t) in tracks[s..e].iter().enumerate() {
        if t.flex().is_none() || t.collapsed {
            inflexible[i] = true;
            leftover -= t.base.0 as i64;
        }
    }
    loop {
        let sum_f: i64 = tracks[s..e]
            .iter()
            .enumerate()
            .filter(|(i, _)| !inflexible[*i])
            .map(|(_, t)| t.flex().unwrap_or(0) as i64)
            .sum();
        let sum_f = sum_f.max(1000);
        let hyp = leftover * 1000 / sum_f;
        let mut changed = false;
        for (i, t) in tracks[s..e].iter().enumerate() {
            if inflexible[i] {
                continue;
            }
            let f = t.flex().unwrap_or(0) as i64;
            if t.base.0 as i64 > hyp * f / 1000 {
                inflexible[i] = true;
                leftover -= t.base.0 as i64;
                changed = true;
            }
        }
        if !changed {
            return Au(hyp.clamp(0, Au::MAX.0 as i64) as i32);
        }
    }
}

/// Expands flexible tracks (§11.7).
fn expand_flex(
    tracks: &mut [Track],
    gap: Au,
    spans: &[(usize, usize)],
    avail: Avail,
    contrib: &mut ContribFn,
) {
    let n = tracks.len();
    let flex: Vec<usize> = (0..n)
        .filter(|&k| tracks[k].flex().is_some() && !tracks[k].collapsed)
        .collect();
    if flex.is_empty() {
        return;
    }
    let fr = match avail {
        Avail::MinContent => Au::ZERO,
        Avail::Definite(a) => {
            let free = a - total_size(tracks, gap);
            if free <= Au::ZERO {
                Au::ZERO
            } else {
                find_fr(tracks, gap, 0, n, a)
            }
        }
        Avail::MaxContent => {
            let mut fr = Au::ZERO;
            for &k in &flex {
                let f = tracks[k].flex().unwrap_or(0);
                let v = if f > 1000 {
                    tracks[k].base.scale(1000, f)
                } else {
                    tracks[k].base
                };
                fr = fr.max(v);
            }
            for (idx, &(s, e)) in spans.iter().enumerate() {
                if tracks[s..e]
                    .iter()
                    .any(|t| t.flex().is_some() && !t.collapsed)
                {
                    let c = item_contrib(tracks, gap, s, e, idx, Contrib::MaxContent, contrib);
                    fr = fr.max(find_fr(tracks, gap, s, e, c));
                }
            }
            fr
        }
    };
    for &k in &flex {
        let f = tracks[k].flex().unwrap_or(0);
        let v = fr.scale(f, 1000);
        if v > tracks[k].base {
            tracks[k].base = v;
        }
    }
}

/// The track sizing algorithm (§11.3–11.8) for one axis. `spans[i]` is item `i`'s
/// track range; `stretch` says the axis's content distribution is `normal`/`stretch`.
fn size_axis(
    tracks: &mut [Track],
    gap: Au,
    spans: &[(usize, usize)],
    avail: Avail,
    stretch: bool,
    contrib: &mut ContribFn,
) {
    // §11.4 Initialise track sizes.
    for t in tracks.iter_mut() {
        t.base = match t.min {
            MinFn::Fixed(v) => v,
            _ => Au::ZERO,
        };
        t.limit = match t.max {
            MaxFn::Fixed(v) => v.max(t.base),
            _ => INF,
        };
        t.inf_growable = false;
    }
    let constrained = !matches!(avail, Avail::Definite(_));

    // §11.5 step 2: size tracks to fit non-spanning items.
    for (idx, &(s, e)) in spans.iter().enumerate() {
        if e != s + 1 || tracks[s].collapsed {
            continue;
        }
        let (min, max) = (tracks[s].min, tracks[s].max);
        let fa = fixed_area(tracks, gap, s, e, false);
        let minimum = contrib(idx, Contrib::Minimum, fa);
        let minc = contrib(idx, Contrib::MinContent, fa);
        let maxc = contrib(idx, Contrib::MaxContent, fa);
        let lim = |c: Au| match max {
            MaxFn::Fixed(v) | MaxFn::FitContent(v) => c.min(v).max(minimum),
            _ => c,
        };
        let t = &mut tracks[s];
        match min {
            MinFn::Fixed(_) => {}
            MinFn::MinContent => t.base = t.base.max(minc),
            MinFn::MaxContent => t.base = t.base.max(maxc),
            MinFn::Auto => {
                t.base = t.base.max(match avail {
                    Avail::MinContent => lim(minc),
                    Avail::MaxContent => lim(maxc),
                    Avail::Definite(_) => minimum,
                })
            }
        }
        let grow = |t: &mut Track, v: Au| t.limit = if t.limit == INF { v } else { t.limit.max(v) };
        match max {
            MaxFn::MinContent => grow(t, minc),
            MaxFn::MaxContent | MaxFn::Auto => grow(t, maxc),
            MaxFn::FitContent(arg) => grow(t, maxc.min(arg)),
            MaxFn::Fixed(_) | MaxFn::Flex(_) => {}
        }
        if t.limit != INF && t.limit < t.base {
            t.limit = t.base;
        }
    }

    // §11.5 step 3: spanning items not crossing flexible tracks, by span size.
    let mut groups: BTreeMap<usize, Vec<(usize, usize, usize)>> = BTreeMap::new();
    let mut flex_items: Vec<(usize, usize, usize)> = Vec::new();
    for (idx, &(s, e)) in spans.iter().enumerate() {
        if e < s + 2 {
            continue;
        }
        if tracks[s..e]
            .iter()
            .any(|t| t.flex().is_some() && !t.collapsed)
        {
            flex_items.push((s, e, idx));
        } else {
            groups.entry(e - s).or_default().push((s, e, idx));
        }
    }
    let min_kind = if constrained {
        Contrib::LimitedMinContent
    } else {
        Contrib::Minimum
    };
    for group in groups.values() {
        distribute(
            tracks,
            gap,
            group,
            min_kind,
            false,
            &|t| t.intrinsic_min(),
            contrib,
        );
        distribute(
            tracks,
            gap,
            group,
            Contrib::MinContent,
            false,
            &|t| matches!(t.min, MinFn::MinContent | MinFn::MaxContent),
            contrib,
        );
        distribute(
            tracks,
            gap,
            group,
            Contrib::MaxContent,
            false,
            &|t| t.min == MinFn::MaxContent,
            contrib,
        );
        if avail == Avail::MaxContent {
            distribute(
                tracks,
                gap,
                group,
                Contrib::LimitedMaxContent,
                false,
                &|t| t.min == MinFn::Auto,
                contrib,
            );
        }
        distribute(
            tracks,
            gap,
            group,
            Contrib::MinContent,
            true,
            &|t| t.intrinsic_max(Au::ZERO),
            contrib,
        );
        distribute(
            tracks,
            gap,
            group,
            Contrib::MaxContent,
            true,
            &|t| t.max_content_max(Au::ZERO),
            contrib,
        );
        for t in tracks.iter_mut() {
            t.inf_growable = false;
        }
    }
    // §11.5 step 4: items crossing flexible tracks.
    if !flex_items.is_empty() {
        distribute_flex(tracks, gap, &flex_items, min_kind, contrib);
    }
    // §11.5 step 5.
    for t in tracks.iter_mut() {
        if t.limit == INF {
            t.limit = t.base;
        }
    }

    // §11.6 Maximise tracks.
    match avail {
        Avail::Definite(a) => {
            let mut free = a - total_size(tracks, gap);
            if free > Au::ZERO {
                let mut active: Vec<usize> = (0..tracks.len())
                    .filter(|&k| !tracks[k].collapsed)
                    .collect();
                loop {
                    active.retain(|&k| tracks[k].base < tracks[k].limit);
                    if active.is_empty() || free <= Au::ZERO {
                        break;
                    }
                    let total = free;
                    let m = active.len();
                    let mut progressed = false;
                    for (i, &k) in active.iter().enumerate() {
                        let g = split(total, m, i).min(tracks[k].limit - tracks[k].base);
                        tracks[k].base += g;
                        free -= g;
                        progressed |= g > Au::ZERO;
                    }
                    if !progressed {
                        break;
                    }
                }
            }
        }
        Avail::MaxContent => {
            for t in tracks.iter_mut() {
                t.base = t.base.max(t.limit);
            }
        }
        Avail::MinContent => {}
    }

    // §11.7 Expand flexible tracks.
    expand_flex(tracks, gap, spans, avail, contrib);

    // §11.8 Stretch auto tracks.
    if stretch {
        if let Avail::Definite(a) = avail {
            let free = a - total_size(tracks, gap);
            if free > Au::ZERO {
                let auto: Vec<usize> = (0..tracks.len())
                    .filter(|&k| tracks[k].max == MaxFn::Auto && !tracks[k].collapsed)
                    .collect();
                let m = auto.len();
                for (i, &k) in auto.iter().enumerate() {
                    tracks[k].base += split(free, m, i);
                }
            }
        }
    }
}

/// Track start and end positions with gutters and content distribution (css-align
/// §5). Collapsed tracks sit at the previous track's end with no gutter.
fn positions(tracks: &[Track], gap: Au, avail: Option<Au>, dist: Dist) -> (Vec<Au>, Vec<Au>) {
    let n = tracks.len();
    let live = tracks.iter().filter(|t| !t.collapsed).count() as i32;
    let total = total_size(tracks, gap);
    let free = avail.map(|a| a - total).unwrap_or(Au::ZERO);
    let (offset, between) = match dist {
        Dist::Start | Dist::Stretch => (Au::ZERO, Au::ZERO),
        Dist::End => (free, Au::ZERO),
        Dist::Center => (free / 2, Au::ZERO),
        Dist::SpaceBetween => {
            if free > Au::ZERO && live > 1 {
                (Au::ZERO, free / (live - 1))
            } else {
                (Au::ZERO, Au::ZERO)
            }
        }
        Dist::SpaceAround => {
            if free > Au::ZERO && live > 0 {
                let each = free / live;
                (each / 2, each)
            } else {
                (Au::ZERO, Au::ZERO)
            }
        }
        Dist::SpaceEvenly => {
            if free > Au::ZERO && live > 0 {
                let each = free / (live + 1);
                (each, each)
            } else {
                (Au::ZERO, Au::ZERO)
            }
        }
    };
    let mut starts = vec![Au::ZERO; n];
    let mut ends = vec![Au::ZERO; n];
    let mut cur = offset;
    let mut first = true;
    for (k, t) in tracks.iter().enumerate() {
        if t.collapsed {
            starts[k] = cur;
            ends[k] = cur;
            continue;
        }
        if !first {
            cur += gap + between;
        }
        first = false;
        starts[k] = cur;
        cur += t.base;
        ends[k] = cur;
    }
    (starts, ends)
}

// The grid container.

#[derive(Clone, Debug)]
struct GItem {
    id: BoxId,
    col: (usize, usize),
    row: (usize, usize),
    /// Participates in the row's baseline alignment context.
    baseline_group: bool,
}

/// Per-item measurements from the column pass: margins, used inline size, and the
/// block size and baseline of the item laid out at that size.
#[derive(Clone, Debug, Default)]
struct Measure {
    margin: Edges,
    /// Auto margins: left, right, top, bottom.
    auto: [bool; 4],
    content_w: Au,
    ev: Au,
    /// Border-box height.
    height: Au,
    /// Baseline from the margin-box top (synthesised from the bottom edge otherwise).
    ascent: Au,
    shim: Au,
}

struct Grid<'c, 'a> {
    ctx: &'c LayoutContext<'a>,
    id: BoxId,
    items: Vec<GItem>,
    abs: Vec<BoxId>,
    cols: Vec<Track>,
    rows: Vec<Track>,
    col_gap: Au,
    row_gap: Au,
    col_names: Names,
    row_names: Names,
    col_min: i32,
    row_min: i32,
    /// The container's content width when definite (percent margins of items in
    /// contributions resolve against it).
    cw: Option<Au>,
}

fn margins(s: &ComputedStyle, base: Au) -> (Edges, [bool; 4]) {
    let f = |m: LengthPercentageAuto| -> (Au, bool) {
        match m {
            LengthPercentageAuto::Auto => (Au::ZERO, true),
            LengthPercentageAuto::Set(lp) => (lp.resolve(base), false),
        }
    };
    let (l, la) = f(s.margin.left);
    let (r, ra) = f(s.margin.right);
    let (t, ta) = f(s.margin.top);
    let (b, ba) = f(s.margin.bottom);
    (
        Edges {
            top: t,
            right: r,
            bottom: b,
            left: l,
        },
        [la, ra, ta, ba],
    )
}

fn auto_track(auto: &[TrackSize], i: usize) -> TrackSize {
    if auto.is_empty() {
        TrackSize::Auto
    } else {
        auto[i % auto.len()]
    }
}

/// Builds one axis's tracks: the explicit ones (template tracks, then `grid-auto-*`
/// sizes for explicit tracks that only the areas define) and the implicit ones
/// cycling the `grid-auto-*` list forwards after and backwards before (§7.6).
fn build_tracks(
    exp: &ExplicitAxis,
    explicit: usize,
    auto: &[TrackSize],
    line_min: i32,
    count: usize,
    base: Option<Au>,
    spans: &[(usize, usize)],
) -> Vec<Track> {
    let first = (1 - line_min).max(0) as usize;
    let n_auto = auto.len().max(1);
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let ts = if k >= first && k < first + explicit {
            let e = k - first;
            if e < exp.tracks.len() {
                exp.tracks[e]
            } else {
                auto_track(auto, e - exp.tracks.len())
            }
        } else if k < first {
            let d = first - k;
            auto_track(auto, (n_auto - (d % n_auto)) % n_auto)
        } else {
            auto_track(auto, k - first - explicit)
        };
        let (mn, mx) = track_fns(&ts, base);
        out.push(Track::new(mn, mx));
    }
    if exp.auto_fit {
        if let Some((a, b)) = exp.repeat {
            let mut covered = vec![false; count];
            for &(s, e) in spans {
                for c in covered.iter_mut().take(e.min(count)).skip(s) {
                    *c = true;
                }
            }
            for k in a..b {
                let idx = first + k;
                if idx < count && !covered[idx] {
                    out[idx].collapsed = true;
                    out[idx].min = MinFn::Fixed(Au::ZERO);
                    out[idx].max = MaxFn::Fixed(Au::ZERO);
                }
            }
        }
    }
    out
}

impl<'c, 'a> Grid<'c, 'a> {
    /// Reads the container's style and children, expands the explicit grid, places
    /// every item and builds the tracks. `cb` is `None` for intrinsic sizing.
    fn prepare(ctx: &'c LayoutContext<'a>, id: BoxId, cb: Option<&Cb>) -> Grid<'c, 'a> {
        let b = &ctx.tree[id];
        let s = &b.style;
        let cw = cb.map(|c| c.width);
        let ch = cb.and_then(|c| c.height);
        let col_gap = s
            .column_gap
            .maybe_resolve(cw)
            .unwrap_or(Au::ZERO)
            .max(Au::ZERO);
        let row_gap = s
            .row_gap
            .maybe_resolve(ch)
            .unwrap_or(Au::ZERO)
            .max(Au::ZERO);

        // The explicit grid, with auto repeats counted against the container's
        // definite size, else its max size, else its min size.
        let p = block::padding_edges(s, cw.unwrap_or(Au::ZERO));
        let bw = s.used_border_widths();
        let eh = p.horizontal() + bw.horizontal();
        let ev = p.vertical() + bw.vertical();
        let max_w = block::resolve_size(s.max_width, None, eh, s.box_sizing);
        let min_w = block::resolve_size(s.min_width, None, eh, s.box_sizing);
        let max_h = block::resolve_size(s.max_height, None, ev, s.box_sizing);
        let min_h = block::resolve_size(s.min_height, None, ev, s.box_sizing);
        let col_count = match &s.grid_template_columns.auto_repeat {
            Some(rep) => auto_repeat_count(
                &s.grid_template_columns,
                rep,
                cw.or(max_w),
                min_w,
                col_gap,
                cw,
            ),
            None => 0,
        };
        let row_count = match &s.grid_template_rows.auto_repeat {
            Some(rep) => {
                auto_repeat_count(&s.grid_template_rows, rep, ch.or(max_h), min_h, row_gap, ch)
            }
            None => 0,
        };
        let cols_exp = expand(&s.grid_template_columns, col_count);
        let rows_exp = expand(&s.grid_template_rows, row_count);

        // Areas: the bounding box of each name's cells (§7.3).
        let mut areas: BTreeMap<String, (i32, i32, i32, i32)> = BTreeMap::new();
        let mut area_cols = 0usize;
        for (ri, row) in s.grid_template_areas.iter().enumerate() {
            area_cols = area_cols.max(row.len());
            for (ci, name) in row.iter().enumerate() {
                if name.starts_with('.') || name.is_empty() {
                    continue;
                }
                let (r0, r1, c0, c1) = (ri as i32 + 1, ri as i32 + 2, ci as i32 + 1, ci as i32 + 2);
                areas
                    .entry(name.clone())
                    .and_modify(|a| {
                        a.0 = a.0.min(r0);
                        a.1 = a.1.max(r1);
                        a.2 = a.2.min(c0);
                        a.3 = a.3.max(c1);
                    })
                    .or_insert((r0, r1, c0, c1));
            }
        }
        let explicit_cols = cols_exp.tracks.len().max(area_cols);
        let explicit_rows = rows_exp.tracks.len().max(s.grid_template_areas.len());
        let col_names = Names::build(&cols_exp, &areas, false, explicit_cols as i32);
        let row_names = Names::build(&rows_exp, &areas, true, explicit_rows as i32);

        // Items in order-modified document order; absolutely positioned children apart.
        let mut kids: Vec<(i32, usize, BoxId)> = Vec::new();
        let mut abs = Vec::new();
        for (i, &c) in b.children.iter().enumerate() {
            let cb = &ctx.tree[c];
            if cb.is_abs() {
                abs.push(c);
                continue;
            }
            if matches!(
                cb.kind,
                BoxKind::Col(_)
                    | BoxKind::ColGroup(_)
                    | BoxKind::Wbr
                    | BoxKind::Br(_)
                    | BoxKind::Text(_)
            ) {
                continue;
            }
            kids.push((cb.style.order, i, c));
        }
        kids.sort();

        // Placement.
        let flow = s.grid_auto_flow;
        let dense = matches!(flow, GridAutoFlow::RowDense | GridAutoFlow::ColumnDense);
        let row_major = matches!(flow, GridAutoFlow::Row | GridAutoFlow::RowDense);
        let resolved: Vec<(Res, Res)> = kids
            .iter()
            .map(|&(_, _, c)| {
                let cs = ctx.style(c);
                let row = resolve_axis(&row_names, &cs.grid_row_start, &cs.grid_row_end);
                let col = resolve_axis(&col_names, &cs.grid_column_start, &cs.grid_column_end);
                if row_major {
                    (row, col)
                } else {
                    (col, row)
                }
            })
            .collect();
        let placed = auto_place(
            &resolved,
            if row_major {
                explicit_cols as i32
            } else {
                explicit_rows as i32
            },
            dense,
        );
        let placed: Vec<((i32, i32), (i32, i32))> = placed
            .into_iter()
            .map(|(maj, min)| if row_major { (maj, min) } else { (min, maj) })
            .collect();

        let mut col_min = 1;
        let mut col_end = explicit_cols as i32 + 1;
        let mut row_min = 1;
        let mut row_end = explicit_rows as i32 + 1;
        for &((r0, r1), (c0, c1)) in &placed {
            col_min = col_min.min(c0);
            col_end = col_end.max(c1);
            row_min = row_min.min(r0);
            row_end = row_end.max(r1);
        }
        let ncols = (col_end - col_min).max(0) as usize;
        let nrows = (row_end - row_min).max(0) as usize;
        let items: Vec<GItem> = kids
            .iter()
            .zip(placed.iter())
            .map(|(&(_, _, c), &((r0, r1), (c0, c1)))| {
                let cs = ctx.style(c);
                let row = ((r0 - row_min) as usize, (r1 - row_min) as usize);
                let baseline_group = resolve_self(cs.align_self, s.align_items) == Align::Baseline
                    && row.1 == row.0 + 1;
                GItem {
                    id: c,
                    col: ((c0 - col_min) as usize, (c1 - col_min) as usize),
                    row,
                    baseline_group,
                }
            })
            .collect();
        let col_spans: Vec<(usize, usize)> = items.iter().map(|it| it.col).collect();
        let row_spans: Vec<(usize, usize)> = items.iter().map(|it| it.row).collect();
        let cols = build_tracks(
            &cols_exp,
            explicit_cols,
            &s.grid_auto_columns,
            col_min,
            ncols,
            cw,
            &col_spans,
        );
        let rows = build_tracks(
            &rows_exp,
            explicit_rows,
            &s.grid_auto_rows,
            row_min,
            nrows,
            ch,
            &row_spans,
        );
        Grid {
            ctx,
            id,
            items,
            abs,
            cols,
            rows,
            col_gap,
            row_gap,
            col_names,
            row_names,
            col_min,
            row_min,
            cw,
        }
    }

    fn style(&self) -> &ComputedStyle {
        self.ctx.style(self.id)
    }

    fn justify_self(&self, item: BoxId) -> Align {
        let b = &self.ctx.tree[item];
        let a = resolve_self(b.style.justify_self, self.style().justify_items);
        if matches!(b.kind, BoxKind::Replaced(_)) && a == Align::Stretch {
            Align::Start
        } else {
            a
        }
    }

    fn align_self(&self, item: BoxId) -> Align {
        let b = &self.ctx.tree[item];
        let a = resolve_self(b.style.align_self, self.style().align_items);
        if matches!(b.kind, BoxKind::Replaced(_)) && a == Align::Stretch {
            Align::Start
        } else {
            a
        }
    }

    /// An item's inline-axis contribution (§11.5) from its intrinsic widths.
    fn inline_contrib(&self, it: &GItem, kind: Contrib, fixed: Option<Au>, over: Option<Au>) -> Au {
        if let Some(w) = over {
            return w;
        }
        let ctx = self.ctx;
        let b = &ctx.tree[it.id];
        let s = &b.style;
        let (m, _) = margins(s, self.cw.unwrap_or(Au::ZERO));
        let mh = m.horizontal();
        let (mn, mx) = intrinsic::min_max(ctx, it.id);
        match kind {
            Contrib::MinContent | Contrib::LimitedMinContent => mn + mh,
            Contrib::MaxContent | Contrib::LimitedMaxContent => mx + mh,
            Contrib::Minimum => {
                if matches!(
                    s.width,
                    Sizing::Set(LengthPercentage::Length(_))
                        | Sizing::MinContent
                        | Sizing::MaxContent
                ) {
                    return mn + mh;
                }
                let eh = intrinsic::edges_h(ctx, it.id);
                match s.min_width {
                    Sizing::Auto => {
                        if b.is_scroll_container() {
                            eh + mh
                        } else {
                            let v = mn + mh;
                            match fixed {
                                Some(f) => v.min(f).max(eh + mh),
                                None => v,
                            }
                        }
                    }
                    other => block::resolve_size(other, None, eh, s.box_sizing)
                        .map(|v| v + eh + mh)
                        .unwrap_or(eh + mh),
                }
            }
        }
    }

    /// An item's block-axis contribution from its measured height.
    fn block_contrib(&self, it: &GItem, ms: &Measure, kind: Contrib, fixed: Option<Au>) -> Au {
        let b = &self.ctx.tree[it.id];
        let s = &b.style;
        let mv = ms.margin.vertical();
        let outer = ms.height + mv + ms.shim;
        match kind {
            Contrib::Minimum => {
                if matches!(s.height, Sizing::Set(LengthPercentage::Length(_))) {
                    return outer;
                }
                match s.min_height {
                    Sizing::Auto => {
                        if b.is_scroll_container() {
                            ms.ev + mv
                        } else {
                            match fixed {
                                Some(f) => outer.min(f).max(ms.ev + mv),
                                None => outer,
                            }
                        }
                    }
                    other => block::resolve_size(other, None, ms.ev, s.box_sizing)
                        .map(|v| v + ms.ev + mv)
                        .unwrap_or(ms.ev + mv),
                }
            }
            _ => outer,
        }
    }

    /// Runs the track sizing algorithm for the columns.
    fn size_columns(&self, avail: Avail, over: &BTreeMap<usize, Au>) -> Vec<Track> {
        let mut cols = self.cols.clone();
        let spans: Vec<(usize, usize)> = self.items.iter().map(|it| it.col).collect();
        let stretch = dist_justify(self.style().justify_content) == Dist::Stretch;
        let mut f = |i: usize, k: Contrib, fixed: Option<Au>| {
            self.inline_contrib(&self.items[i], k, fixed, over.get(&i).copied())
        };
        size_axis(&mut cols, self.col_gap, &spans, avail, stretch, &mut f);
        cols
    }

    fn size_rows(&self, avail: Avail, measures: &[Measure]) -> Vec<Track> {
        let mut rows = self.rows.clone();
        let spans: Vec<(usize, usize)> = self.items.iter().map(|it| it.row).collect();
        let stretch = dist_align(self.style().align_content) == Dist::Stretch;
        let mut f = |i: usize, k: Contrib, fixed: Option<Au>| {
            self.block_contrib(&self.items[i], &measures[i], k, fixed)
        };
        size_axis(&mut rows, self.row_gap, &spans, avail, stretch, &mut f);
        rows
    }

    /// The used inline size of an item in its grid area (css-align §6, §6.6):
    /// `(content width, margins, auto margin flags)`.
    fn used_width(
        &self,
        it: &GItem,
        area_w: Au,
        area_h: Option<Au>,
        fixed: Option<Au>,
    ) -> (Au, Edges, [bool; 4]) {
        let ctx = self.ctx;
        let b = &ctx.tree[it.id];
        let s = &b.style;
        let (m, auto) = margins(s, area_w);
        let p = block::padding_edges(s, area_w);
        let eh = p.horizontal() + s.used_border_widths().horizontal();
        let free = area_w - m.horizontal();
        if let BoxKind::Replaced(rb) = &b.kind {
            let size = block::replaced_size(
                ctx,
                it.id,
                rb,
                &Cb {
                    width: area_w,
                    height: area_h,
                },
            );
            return (size.width, m, auto);
        }
        let (mn, mx) = intrinsic::min_max(ctx, it.id);
        let stretch = self.justify_self(it.id) == Align::Stretch && !auto[0] && !auto[1];
        let content = match s.width {
            Sizing::Set(lp) => match s.box_sizing {
                crate::style::BoxSizing::ContentBox => lp.resolve(area_w),
                crate::style::BoxSizing::BorderBox => lp.resolve(area_w) - eh,
            },
            Sizing::MinContent => mn - eh,
            Sizing::MaxContent => mx - eh,
            Sizing::Auto | Sizing::None | Sizing::FitContent => {
                if stretch && s.width == Sizing::Auto {
                    free - eh
                } else {
                    mn.max(free.min(mx)) - eh
                }
            }
        }
        .max(Au::ZERO);
        let min_w = match s.min_width {
            Sizing::Auto => {
                if b.is_scroll_container() || !matches!(s.width, Sizing::Auto | Sizing::FitContent)
                {
                    Au::ZERO
                } else {
                    let c = (mn - eh).max(Au::ZERO);
                    match fixed {
                        Some(f) => c.min((f - m.horizontal() - eh).max(Au::ZERO)),
                        None => c,
                    }
                }
            }
            other => block::resolve_size(other, Some(area_w), eh, s.box_sizing).unwrap_or(Au::ZERO),
        };
        let mut w = content;
        if let Some(mx) = block::resolve_size(s.max_width, Some(area_w), eh, s.box_sizing) {
            w = w.min(mx);
        }
        w = w.max(min_w);
        (w, m, auto)
    }

    /// Lays out an item at a content width in an area, at the origin. `forced_h` is a
    /// stretched content height.
    fn layout_item(
        &self,
        id: BoxId,
        area_w: Au,
        area_h: Option<Au>,
        content_w: Au,
        forced_h: Option<Au>,
    ) -> (Fragment, Vec<AbsRequest>) {
        let ctx = self.ctx;
        let b = &ctx.tree[id];
        let s = &b.style;
        let cb = Cb {
            width: area_w,
            height: area_h,
        };
        match &b.kind {
            BoxKind::Replaced(rb) => {
                let p = block::padding_edges(s, area_w);
                let bw = s.used_border_widths();
                let mut size = block::replaced_size(ctx, id, rb, &cb);
                if size.width != content_w && s.height == Sizing::Auto && size.width > Au::ZERO {
                    size.height = size.height.scale(content_w.0, size.width.0);
                }
                size.width = content_w;
                (
                    block::replaced_fragment(ctx, id, rb, size, p, bw),
                    Vec::new(),
                )
            }
            BoxKind::TableWrapper => {
                let eh = intrinsic::edges_h(ctx, id);
                block::layout_standalone(ctx, id, &cb, content_w + eh, eh)
            }
            _ => {
                if let Some(h) = forced_h {
                    ctx.cache.borrow_mut().forced_height.insert(id, Some(h));
                }
                let r = block::layout_block_box(
                    ctx,
                    id,
                    &cb,
                    &mut Bfc::new(),
                    Point::default(),
                    Au::ZERO,
                    Some(content_w),
                );
                if forced_h.is_some() {
                    ctx.cache.borrow_mut().forced_height.remove(&id);
                }
                let mut f = r.fragment;
                f.rect.origin = Point::default();
                (f, r.abs)
            }
        }
    }

    /// Sizes every item in its column area and lays it out to learn its height and
    /// baseline; computes the baseline shims of each row's alignment context.
    fn measure(&self, cols: &[Track], starts: &[Au], ends: &[Au]) -> Vec<Measure> {
        let mut out: Vec<Measure> = Vec::with_capacity(self.items.len());
        for it in &self.items {
            let area_w = ends[it.col.1 - 1] - starts[it.col.0];
            let fixed = fixed_area(cols, self.col_gap, it.col.0, it.col.1, false);
            let (content_w, margin, auto) = self.used_width(it, area_w, None, fixed);
            let (frag, _) = self.layout_item(it.id, area_w, None, content_w, None);
            let s = self.ctx.style(it.id);
            let ev = block::padding_edges(s, area_w).vertical() + s.used_border_widths().vertical();
            let height = frag.rect.size.height;
            let ascent = margin.top
                + match &frag.kind {
                    FragmentKind::Box {
                        baseline: Some(bl), ..
                    } => *bl,
                    _ => height,
                };
            out.push(Measure {
                margin,
                auto,
                content_w,
                ev,
                height,
                ascent,
                shim: Au::ZERO,
            });
        }
        // Baseline alignment contexts per row (§11.5 step 1).
        let mut max_ascent: BTreeMap<usize, Au> = BTreeMap::new();
        for (it, ms) in self.items.iter().zip(out.iter()) {
            if it.baseline_group {
                let e = max_ascent.entry(it.row.0).or_insert(Au::MIN);
                *e = (*e).max(ms.ascent);
            }
        }
        for (it, ms) in self.items.iter().zip(out.iter_mut()) {
            if it.baseline_group {
                ms.shim = max_ascent[&it.row.0] - ms.ascent;
            }
        }
        out
    }

    /// Second-pass column contributions (§12 step 3) for replaced items whose width
    /// depends on their now-definite row area height. Returns whether any changed.
    fn replaced_overrides(
        &self,
        measures: &[Measure],
        row_starts: &[Au],
        row_ends: &[Au],
        col_starts: &[Au],
        col_ends: &[Au],
        over: &mut BTreeMap<usize, Au>,
    ) -> bool {
        let mut changed = false;
        for (i, it) in self.items.iter().enumerate() {
            let b = &self.ctx.tree[it.id];
            let BoxKind::Replaced(rb) = &b.kind else {
                continue;
            };
            let s = &b.style;
            let pct = |z: Sizing| matches!(z, Sizing::Set(v) if v.has_percent());
            if !(pct(s.height) || pct(s.min_height) || pct(s.max_height)) {
                continue;
            }
            let area_w = col_ends[it.col.1 - 1] - col_starts[it.col.0];
            let area_h = row_ends[it.row.1 - 1] - row_starts[it.row.0];
            let size = block::replaced_size(
                self.ctx,
                it.id,
                rb,
                &Cb {
                    width: area_w,
                    height: Some(area_h),
                },
            );
            let w =
                size.width + intrinsic::edges_h(self.ctx, it.id) + measures[i].margin.horizontal();
            let before = self.inline_contrib(it, Contrib::MinContent, None, over.get(&i).copied());
            if w != before {
                over.insert(i, w);
                changed = true;
            }
        }
        changed
    }
}

/// The min-content and max-content widths of a grid container's contents
/// (§11 under a min-/max-content constraint): the sum of the columns and gutters.
pub fn content_min_max(ctx: &LayoutContext, id: BoxId) -> (Au, Au) {
    let g = Grid::prepare(ctx, id, None);
    let none = BTreeMap::new();
    let mn = g.size_columns(Avail::MinContent, &none);
    let mx = g.size_columns(Avail::MaxContent, &none);
    let a = total_size(&mn, g.col_gap).max(Au::ZERO);
    let b = total_size(&mx, g.col_gap).max(Au::ZERO);
    (a, b.max(a))
}

/// Lays out a grid container's contents in its content box (`cb`): tracks, items and
/// absolutely positioned children. Fragments are relative to the content box.
pub fn layout_contents(ctx: &LayoutContext, id: BoxId, cb: &Cb) -> ContentsResult {
    let g = Grid::prepare(ctx, id, Some(cb));
    let s = g.style();
    let col_avail = Avail::Definite(cb.width);
    let row_avail = cb.height.map(Avail::Definite).unwrap_or(Avail::MaxContent);
    let jdist = dist_justify(s.justify_content);
    let adist = dist_align(s.align_content);
    let mut over: BTreeMap<usize, Au> = BTreeMap::new();
    let mut pass = 0;
    let (cols, col_starts, col_ends, rows, row_starts, row_ends, measures) = loop {
        let cols = g.size_columns(col_avail, &over);
        let (cs, ce) = positions(&cols, g.col_gap, Some(cb.width), jdist);
        let measures = g.measure(&cols, &cs, &ce);
        let rows = g.size_rows(row_avail, &measures);
        let (rs, re) = positions(&rows, g.row_gap, cb.height, adist);
        if pass == 0 && g.replaced_overrides(&measures, &rs, &re, &cs, &ce, &mut over) {
            pass += 1;
            continue;
        }
        break (cols, cs, ce, rows, rs, re, measures);
    };
    let _ = (&cols, &rows);

    let mut fragments = Vec::new();
    let mut abs_out: Vec<AbsRequest> = Vec::new();
    let mut row0_group: Option<Au> = None;
    let mut row0_first: Option<Au> = None;
    for (it, ms) in g.items.iter().zip(measures.iter()) {
        let b = &ctx.tree[it.id];
        let is = &b.style;
        let x0 = col_starts[it.col.0];
        let area_w = col_ends[it.col.1 - 1] - x0;
        let y0 = row_starts[it.row.0];
        let area_h = row_ends[it.row.1 - 1] - y0;
        let align = g.align_self(it.id);
        let stretch_h = align == Align::Stretch
            && is.height == Sizing::Auto
            && !ms.auto[2]
            && !ms.auto[3]
            && !matches!(b.kind, BoxKind::Replaced(_) | BoxKind::TableWrapper);
        let forced = if stretch_h {
            Some((area_h - ms.margin.vertical() - ms.ev).max(Au::ZERO))
        } else {
            None
        };
        let (mut frag, mut abs) = g.layout_item(it.id, area_w, Some(area_h), ms.content_w, forced);
        let w = frag.rect.size.width;
        let h = frag.rect.size.height;
        let free_x = area_w - w - ms.margin.horizontal();
        let x = match (ms.auto[0], ms.auto[1]) {
            (true, true) => free_x.max(Au::ZERO) / 2,
            (true, false) => free_x.max(Au::ZERO),
            (false, true) => Au::ZERO,
            (false, false) => match g.justify_self(it.id) {
                Align::Start | Align::Stretch | Align::Baseline => Au::ZERO,
                Align::End => free_x,
                Align::Center => free_x / 2,
            },
        } + ms.margin.left;
        let free_y = area_h - h - ms.margin.vertical();
        let y = match (ms.auto[2], ms.auto[3]) {
            (true, true) => free_y.max(Au::ZERO) / 2,
            (true, false) => free_y.max(Au::ZERO),
            (false, true) => Au::ZERO,
            (false, false) => match align {
                Align::Start | Align::Stretch => Au::ZERO,
                Align::End => free_y,
                Align::Center => free_y / 2,
                Align::Baseline => {
                    if it.baseline_group {
                        ms.shim
                    } else {
                        Au::ZERO
                    }
                }
            },
        } + ms.margin.top;
        let off = block::relative_offset(
            is,
            &Cb {
                width: area_w,
                height: Some(area_h),
            },
        );
        frag.rect.origin = Point {
            x: x0 + x + off.x,
            y: y0 + y + off.y,
        };
        // Used margins: an `auto` side takes the free space of its area.
        let mut used = ms.margin;
        if ms.auto[0] {
            used.left = x;
        }
        if ms.auto[1] {
            used.right = (area_w - w - x).max(Au::ZERO);
        }
        if ms.auto[2] {
            used.top = y;
        }
        if ms.auto[3] {
            used.bottom = (area_h - h - y).max(Au::ZERO);
        }
        frag.used_margin = Some(used);
        frag.is_float = false;
        frag.establishes_stacking_context |= is.establishes_stacking_context(true);
        block::translate_requests(&mut abs, frag.rect.origin.x, frag.rect.origin.y);
        abs_out.extend(abs);
        if it.row.0 == 0 {
            let bl = match &frag.kind {
                FragmentKind::Box {
                    baseline: Some(b), ..
                } => Some(frag.rect.origin.y + *b),
                _ => None,
            };
            if it.baseline_group && row0_group.is_none() {
                row0_group = bl.or(Some(frag.rect.bottom()));
            }
            if row0_first.is_none() {
                row0_first = bl.or(Some(frag.rect.bottom()));
            }
        }
        fragments.push(frag);
    }

    // Absolutely positioned children (§9): against the grid area named by their
    // lines when this container is their containing block, else the static position.
    let pad = block::padding_edges(s, cb.width);
    let height = row_ends.last().copied().unwrap_or(Au::ZERO).max(Au::ZERO);
    for &a in &g.abs {
        let ab = &ctx.tree[a];
        let fixed = ab.style.position == crate::style::Position::Fixed;
        if !s.is_positioned() || fixed {
            abs_out.push(AbsRequest {
                id: a,
                static_pos: Point::default(),
                fixed,
            });
            continue;
        }
        let as_ = &ab.style;
        let line_pos =
            |starts: &[Au], ends: &[Au], line: i32, min: i32, end: bool, lo: Au, hi: Au| -> Au {
                let n = starts.len() as i32;
                let idx = line - min;
                if idx < 0 || idx > n {
                    return if end { hi } else { lo };
                }
                if end {
                    if idx == 0 {
                        starts.first().copied().unwrap_or(Au::ZERO)
                    } else {
                        ends[(idx - 1) as usize]
                    }
                } else if idx == n {
                    ends.last().copied().unwrap_or(Au::ZERO)
                } else {
                    starts[idx as usize]
                }
            };
        let (cx0, cx1) =
            match resolve_axis(&g.col_names, &as_.grid_column_start, &as_.grid_column_end) {
                Res::Definite(l0, l1) => {
                    let x0 = if as_.grid_column_start == GridLine::Auto {
                        -pad.left
                    } else {
                        line_pos(
                            &col_starts,
                            &col_ends,
                            l0,
                            g.col_min,
                            false,
                            -pad.left,
                            cb.width + pad.right,
                        )
                    };
                    let x1 = if as_.grid_column_end == GridLine::Auto {
                        cb.width + pad.right
                    } else {
                        line_pos(
                            &col_starts,
                            &col_ends,
                            l1,
                            g.col_min,
                            true,
                            -pad.left,
                            cb.width + pad.right,
                        )
                    };
                    (x0, x1.max(x0))
                }
                Res::Span(_) => (-pad.left, cb.width + pad.right),
            };
        let inner_h = cb.height.unwrap_or(height);
        let (cy0, cy1) = match resolve_axis(&g.row_names, &as_.grid_row_start, &as_.grid_row_end) {
            Res::Definite(l0, l1) => {
                let y0 = if as_.grid_row_start == GridLine::Auto {
                    -pad.top
                } else {
                    line_pos(
                        &row_starts,
                        &row_ends,
                        l0,
                        g.row_min,
                        false,
                        -pad.top,
                        inner_h + pad.bottom,
                    )
                };
                let y1 = if as_.grid_row_end == GridLine::Auto {
                    inner_h + pad.bottom
                } else {
                    line_pos(
                        &row_starts,
                        &row_ends,
                        l1,
                        g.row_min,
                        true,
                        -pad.top,
                        inner_h + pad.bottom,
                    )
                };
                (y0, y1.max(y0))
            }
            Res::Span(_) => (-pad.top, inner_h + pad.bottom),
        };
        let cbf = Fragment::new(
            FragmentKind::Box {
                source: ctx.tree[id].source,
                padding: Edges::ZERO,
                border: Edges::ZERO,
                replaced: None,
                scroll: None,
                baseline: None,
            },
            Rect::new(cx0, cy0, cx1 - cx0, cy1 - cy0),
        );
        let req = AbsRequest {
            id: a,
            static_pos: Point { x: -cx0, y: -cy0 },
            fixed: false,
        };
        let mut f = block::layout_absolute(ctx, &cbf, &req);
        f.rect.origin.x += cx0;
        f.rect.origin.y += cy0;
        fragments.push(f);
    }

    let first_baseline = row0_group.or(row0_first);
    ContentsResult {
        fragments,
        height,
        pending_bottom: MarginSet::default(),
        empty: g.items.is_empty() && height.is_zero(),
        first_baseline,
        last_baseline: first_baseline,
        abs: abs_out,
    }
}

#[cfg(test)]
#[path = "grid_tests.rs"]
mod tests;
