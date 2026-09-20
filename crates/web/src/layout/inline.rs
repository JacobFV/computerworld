//! Inline formatting contexts (CSS 2.1 §9.4.2, §10.8; CSS Text Level 3): white-space
//! processing, soft wrap opportunities, line breaking, the strut, `vertical-align`,
//! inline boxes with edges, atomic inlines, floats and absolutes met in inline
//! content, `text-align` including `justify`, `text-indent`, `text-overflow` and
//! `direction: rtl` (reversed line direction; no bidi reordering of mixed runs).

use crate::geom::{Au, Edges, Point, Rect, Size};
use crate::layout::block::{self, AbsRequest, Bfc, Cb};
use crate::layout::boxes::{BoxId, BoxKind, Level};
use crate::layout::fragment::{Fragment, FragmentKind, StyleSource};
use crate::layout::text::{self, CharKind, CollapseState, FontMetrics};
use crate::layout::LayoutContext;
use crate::style::{Clear, ComputedStyle, Direction, Overflow, OverflowWrap, Position, TextAlign, TextOverflow, VerticalAlign, WhiteSpace, WordBreak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKind {
    Word,
    /// `collapsible`: removed at line starts and ends; `hang`: preserved but hangs at
    /// a line end (pre-wrap).
    Space { collapsible: bool, hang: bool },
    Tab,
    /// A preserved segment break.
    Newline,
    Br(Clear),
    Open(BoxId),
    Close(BoxId),
    /// Index into the atomic layouts.
    Atomic(usize),
    Float(BoxId),
    Abs(BoxId),
}

/// The smallest piece inline layout places: a word (or part of one), a space, an
/// inline box edge, an atomic inline, or an out-of-flow box.
#[derive(Clone, Debug)]
pub struct Unit {
    pub kind: UnitKind,
    /// The text box (words, spaces) or the inline box (edges) this came from.
    pub owner: BoxId,
    pub text: String,
    pub range: (usize, usize),
    pub width: Au,
    /// A soft wrap opportunity exists before this unit.
    pub break_before: bool,
    pub face: u8,
}

/// A laid-out atomic inline (inline-block, replaced, inline-table).
#[derive(Debug)]
pub struct AtomicLayout {
    pub id: BoxId,
    pub fragment: Fragment,
    pub margin: Edges,
    pub abs: Vec<AbsRequest>,
    /// Baseline from the margin-box top, or `None` for the bottom margin edge.
    pub baseline: Option<Au>,
}

impl AtomicLayout {
    fn margin_height(&self) -> Au {
        self.fragment.rect.size.height + self.margin.vertical()
    }
    fn above(&self) -> Au {
        match self.baseline {
            Some(b) => b,
            None => self.margin_height(),
        }
    }
    fn below(&self) -> Au {
        self.margin_height() - self.above()
    }
}

/// Everything collected from an inline formatting context before line breaking.
#[derive(Debug, Default)]
pub struct InlineContent {
    pub units: Vec<Unit>,
    pub atomics: Vec<AtomicLayout>,
}

#[derive(Debug, Default)]
pub struct InlineResult {
    pub fragments: Vec<Fragment>,
    pub height: Au,
    pub empty: bool,
    pub first_baseline: Option<Au>,
    pub last_baseline: Option<Au>,
    pub abs: Vec<AbsRequest>,
}

struct Collector<'c, 'a> {
    ctx: &'c LayoutContext<'a>,
    cb: Cb,
    content: InlineContent,
    state: CollapseState,
    pending_break: bool,
    /// Kind of the last content unit (not an edge), for break opportunities.
    last_content: Option<UnitKind>,
    last_char: Option<char>,
    last_owner: Option<BoxId>,
    /// Whether the last content unit's white-space allows wrapping.
    last_wraps: bool,
    /// Whether atomic inlines are laid out (false for intrinsic sizing, which only
    /// needs their intrinsic widths).
    layout_atomics: bool,
}

/// Collects the units of an inline formatting context, laying out atomic inlines.
pub fn collect(ctx: &LayoutContext, container: BoxId, cb: &Cb, layout_atomics: bool) -> InlineContent {
    let mut c = Collector { ctx, cb: *cb, content: InlineContent::default(), state: CollapseState { after_space: false, at_start: true }, pending_break: false, last_content: None, last_char: None, last_owner: None, last_wraps: true, layout_atomics };
    c.walk(container);
    c.fix_edge_breaks();
    c.content
}

impl Collector<'_, '_> {
    fn push(&mut self, u: Unit) {
        self.content.units.push(u);
    }

    fn walk(&mut self, id: BoxId) {
        let kids: Vec<BoxId> = self.ctx.tree.children(id).to_vec();
        for k in kids {
            let b = &self.ctx.tree[k];
            match &b.kind {
                BoxKind::Text(t) => {
                    let txt = t.text.clone();
                    self.text_units(k, &txt);
                }
                BoxKind::Inline => {
                    let s = &b.style;
                    let p = block::padding_edges(s, self.cb.width);
                    let bw = s.used_border_widths();
                    let ml = s.margin.left.resolve(self.cb.width).unwrap_or(Au::ZERO);
                    let mr = s.margin.right.resolve(self.cb.width).unwrap_or(Au::ZERO);
                    let start = if b.split_first { ml + bw.left + p.left } else { Au::ZERO };
                    let end = if b.split_last { mr + bw.right + p.right } else { Au::ZERO };
                    let bb = self.break_before_content(true);
                    self.push(Unit { kind: UnitKind::Open(k), owner: k, text: String::new(), range: (0, 0), width: start, break_before: bb, face: 0 });
                    self.walk(k);
                    self.push(Unit { kind: UnitKind::Close(k), owner: k, text: String::new(), range: (0, 0), width: end, break_before: false, face: 0 });
                }
                BoxKind::Br(clear) => {
                    let clear = *clear;
                    self.push(Unit { kind: UnitKind::Br(clear), owner: k, text: String::new(), range: (0, 0), width: Au::ZERO, break_before: false, face: 0 });
                    self.state = CollapseState { after_space: false, at_start: true };
                    self.last_content = None;
                    self.last_char = None;
                    self.pending_break = false;
                }
                BoxKind::Wbr => {
                    self.pending_break = true;
                }
                _ if b.is_float() => {
                    self.push(Unit { kind: UnitKind::Float(k), owner: k, text: String::new(), range: (0, 0), width: Au::ZERO, break_before: false, face: 0 });
                }
                _ if b.is_abs() => {
                    self.push(Unit { kind: UnitKind::Abs(k), owner: k, text: String::new(), range: (0, 0), width: Au::ZERO, break_before: false, face: 0 });
                }
                BoxKind::InlineBlock | BoxKind::Replaced(_) | BoxKind::TableWrapper | BoxKind::Block | BoxKind::Table | BoxKind::Cell(_) | BoxKind::Caption | BoxKind::Row | BoxKind::RowGroup => {
                    self.atomic(k);
                }
                BoxKind::Marker(_) | BoxKind::Col(_) | BoxKind::ColGroup(_) => {}
            }
        }
    }

    fn wraps(&self, s: &ComputedStyle) -> bool {
        s.white_space.wraps()
    }

    /// Whether a break may occur before the next content unit, given what came before.
    fn break_before_content(&mut self, _is_edge: bool) -> bool {
        self.pending_break || matches!(self.last_content, Some(UnitKind::Space { .. } | UnitKind::Atomic(_))) && self.last_wraps
    }

    fn atomic(&mut self, k: BoxId) {
        let ctx = self.ctx;
        let b = &ctx.tree[k];
        let s = b.style.clone();
        let wraps = self.wraps(&s);
        let mut bb = self.break_before_content(false) || (matches!(self.last_content, Some(UnitKind::Word)) && self.last_wraps && wraps);
        if !wraps {
            bb = self.pending_break;
        }
        let p = block::padding_edges(&s, self.cb.width);
        let bw = s.used_border_widths();
        let eh = p.horizontal() + bw.horizontal();
        let ml = s.margin.left.resolve(self.cb.width).unwrap_or(Au::ZERO);
        let mr = s.margin.right.resolve(self.cb.width).unwrap_or(Au::ZERO);
        let (mt, mb) = block::vertical_margins(&s, self.cb.width);
        let margin = Edges { top: mt, right: mr, bottom: mb, left: ml };
        let (fragment, abs) = if self.layout_atomics {
            block::layout_standalone(ctx, k, &self.cb, self.cb.width - ml - mr, eh)
        } else {
            let (mn, mx) = crate::layout::intrinsic::min_max(ctx, k);
            let _ = mn;
            (Fragment::new(FragmentKind::Line, Rect::new(Au::ZERO, Au::ZERO, mx, Au::ZERO)), Vec::new())
        };
        let baseline = atomic_baseline(&s, &fragment, &margin, matches!(b.kind, BoxKind::Replaced(_)));
        let idx = self.content.atomics.len();
        let width = fragment.rect.size.width + ml + mr;
        self.content.atomics.push(AtomicLayout { id: k, fragment, margin, abs, baseline });
        self.push(Unit { kind: UnitKind::Atomic(idx), owner: k, text: String::new(), range: (0, 0), width, break_before: bb, face: 0 });
        self.state.after_space = false;
        self.state.at_start = false;
        self.last_content = Some(UnitKind::Atomic(idx));
        self.last_char = None;
        self.last_owner = Some(k);
        self.last_wraps = wraps;
        self.pending_break = false;
    }

    fn text_units(&mut self, owner: BoxId, txt: &str) {
        let s = self.ctx.tree[owner].style.clone();
        let ws = s.white_space;
        let wraps = self.wraps(&s);
        let chars = text::process(txt, ws, s.text_transform, &mut self.state);
        let font = &s.font;
        let ls = s.letter_spacing;
        let wsp = s.word_spacing;
        let space_w = text::advance(font, ' ') + ls + wsp;
        let tab_w = (text::advance(font, ' ') + ls) * s.tab_size.max(1) as i32;
        let mut i = 0;
        while i < chars.len() {
            let pc = chars[i];
            match pc.kind {
                CharKind::Space | CharKind::PreservedSpace => {
                    let collapsible = pc.kind == CharKind::Space;
                    let hang = ws == WhiteSpace::PreWrap && !collapsible;
                    let bb = ws == WhiteSpace::BreakSpaces && matches!(self.last_content, Some(UnitKind::Space { .. }));
                    self.push(Unit { kind: UnitKind::Space { collapsible, hang }, owner, text: " ".into(), range: (pc.src, pc.src + 1), width: space_w, break_before: bb, face: 0 });
                    self.last_content = Some(UnitKind::Space { collapsible, hang });
                    self.last_char = Some(' ');
                    self.last_wraps = wraps;
                    self.pending_break = false;
                    i += 1;
                }
                CharKind::Tab => {
                    self.push(Unit { kind: UnitKind::Tab, owner, text: "\t".into(), range: (pc.src, pc.src + 1), width: tab_w, break_before: false, face: 0 });
                    self.last_content = Some(UnitKind::Tab);
                    self.last_char = Some('\t');
                    self.last_wraps = wraps;
                    i += 1;
                }
                CharKind::Newline => {
                    self.push(Unit { kind: UnitKind::Newline, owner, text: String::new(), range: (pc.src, pc.src + 1), width: Au::ZERO, break_before: false, face: 0 });
                    self.last_content = None;
                    self.last_char = None;
                    self.pending_break = false;
                    i += 1;
                }
                CharKind::ZeroWidthSpace | CharKind::SoftHyphen => {
                    self.pending_break = true;
                    i += 1;
                }
                CharKind::Other => {
                    // A word: consecutive non-space characters, split at font fallback
                    // changes and at break opportunities inside the word.
                    let mut bb = self.break_before_content(false)
                        || (matches!(self.last_content, Some(UnitKind::Word)) && self.last_wraps && wraps && self.last_char.is_some_and(|p| text::break_between(p, pc.ch, s.word_break)) && self.last_owner == Some(owner));
                    if !wraps {
                        bb = self.pending_break;
                    }
                    let face = text::face_key(font, pc.ch);
                    let start_src = pc.src;
                    let mut end_src = pc.src + pc.ch.len_utf8();
                    let mut word = String::new();
                    let mut width = Au::ZERO;
                    let mut prev = None;
                    while i < chars.len() {
                        let c = chars[i];
                        if c.kind != CharKind::Other {
                            break;
                        }
                        if !word.is_empty() {
                            if text::face_key(font, c.ch) != face {
                                break;
                            }
                            if let Some(p) = prev {
                                if wraps && text::break_between(p, c.ch, s.word_break) {
                                    break;
                                }
                            }
                        }
                        word.push(c.ch);
                        width += text::advance(font, c.ch) + ls;
                        end_src = c.src + c.ch.len_utf8();
                        prev = Some(c.ch);
                        i += 1;
                    }
                    // `end_src` may be smaller than the mapped source for transforms
                    // that expand; keep the range monotonic.
                    let end_src = end_src.max(start_src);
                    self.push(Unit { kind: UnitKind::Word, owner, text: word, range: (start_src, end_src), width, break_before: bb, face });
                    self.last_content = Some(UnitKind::Word);
                    self.last_char = prev;
                    self.last_owner = Some(owner);
                    self.last_wraps = wraps;
                    self.pending_break = false;
                }
            }
        }
    }

    /// Breaks before content that follows a run of opening edges move to the first
    /// edge, so an inline box's start edge stays with its content.
    fn fix_edge_breaks(&mut self) {
        let units = &mut self.content.units;
        let n = units.len();
        let mut i = 0;
        while i < n {
            if let UnitKind::Open(_) = units[i].kind {
                let mut j = i;
                while j < n && matches!(units[j].kind, UnitKind::Open(_)) {
                    j += 1;
                }
                if j < n && matches!(units[j].kind, UnitKind::Word | UnitKind::Atomic(_) | UnitKind::Tab) {
                    let bb = units[i].break_before || units[j].break_before;
                    units[i].break_before = bb;
                    for u in units.iter_mut().take(j + 1).skip(i + 1) {
                        u.break_before = false;
                    }
                }
                i = j.max(i + 1);
            } else {
                i += 1;
            }
        }
    }
}

/// Baseline of an atomic inline from its margin-box top (§10.8.1): the last line box
/// for inline-blocks with in-flow lines and visible overflow; else the bottom margin
/// edge (`None`). Replaced elements sit on their bottom margin edge.
fn atomic_baseline(s: &ComputedStyle, fragment: &Fragment, margin: &Edges, replaced: bool) -> Option<Au> {
    if replaced {
        if let FragmentKind::Box { baseline: Some(b), replaced: Some(crate::layout::fragment::Replaced::Control(_)), .. } = &fragment.kind {
            return Some(margin.top + *b);
        }
        return None;
    }
    if s.overflow_x != Overflow::Visible || s.overflow_y != Overflow::Visible {
        return None;
    }
    last_baseline(fragment).map(|b| margin.top + b)
}

/// The baseline of the last line box inside a fragment, from its border-box top.
fn last_baseline(f: &Fragment) -> Option<Au> {
    let mut best = None;
    for c in &f.children {
        if c.is_float || c.is_positioned && matches!(c.kind, FragmentKind::Box { .. }) && c.establishes_stacking_context {
            continue;
        }
        match &c.kind {
            FragmentKind::Line => {
                if let Some(b) = line_baseline(c) {
                    best = Some(c.rect.origin.y + b);
                }
            }
            FragmentKind::Box { .. } => {
                if let Some(b) = last_baseline(c) {
                    best = Some(c.rect.origin.y + b);
                }
            }
            _ => {}
        }
    }
    best
}

fn line_baseline(line: &Fragment) -> Option<Au> {
    for c in &line.children {
        match &c.kind {
            FragmentKind::Text { baseline, .. } => return Some(c.rect.origin.y + *baseline),
            FragmentKind::InlineBox { .. } => {
                if let Some(b) = line_baseline(c) {
                    return Some(c.rect.origin.y + b);
                }
            }
            FragmentKind::Box { baseline: Some(b), .. } => return Some(c.rect.origin.y + *b),
            _ => {}
        }
    }
    None
}

// Line building.

#[derive(Debug)]
enum NodeKind {
    Root,
    Inline { id: BoxId, first: bool, last: bool },
    Text { owner: BoxId, text: String, range: (usize, usize), node: Option<crate::dom::NodeId>, source: StyleSource },
    Atomic(usize),
}

#[derive(Debug)]
struct LineNode {
    kind: NodeKind,
    x: Au,
    width: Au,
    children: Vec<LineNode>,
    /// Filled by vertical alignment: baseline from the line top.
    baseline: Au,
    above: Au,
    below: Au,
}

impl LineNode {
    fn new(kind: NodeKind, x: Au) -> LineNode {
        LineNode { kind, x, width: Au::ZERO, children: Vec::new(), baseline: Au::ZERO, above: Au::ZERO, below: Au::ZERO }
    }
}

struct Metrics {
    fm: FontMetrics,
    /// Half-leading-adjusted extents of the inline box around its baseline.
    above: Au,
    below: Au,
}

fn metrics_of(s: &ComputedStyle) -> Metrics {
    let fm = text::font_metrics(&s.font);
    let lh = s.line_height_au(fm.normal_line_height());
    let content = fm.content_height();
    let leading = lh - content;
    let half = leading / 2;
    Metrics { fm, above: fm.ascent + half, below: fm.descent + (leading - half) }
}

/// Lays out the inline content of `container` into line boxes. `origin` is the BFC
/// coordinate of the content box.
pub fn layout_inline_content(ctx: &LayoutContext, container: BoxId, cb: &Cb, bfc: &mut Bfc, origin: Point) -> InlineResult {
    let content = collect(ctx, container, cb, true);
    let mut lb = LineBreaker::new(ctx, container, cb, bfc, origin, content);
    lb.run();
    lb.finish()
}

struct LineBreaker<'c, 'a, 'b> {
    ctx: &'c LayoutContext<'a>,
    container: BoxId,
    cb: Cb,
    bfc: &'b mut Bfc,
    origin: Point,
    units: Vec<Unit>,
    atomics: Vec<Option<AtomicLayout>>,
    cw: Au,
    indent: Au,
    y: Au,
    first_line: bool,
    open_stack: Vec<BoxId>,
    fragments: Vec<Fragment>,
    abs: Vec<AbsRequest>,
    first_baseline: Option<Au>,
    last_baseline: Option<Au>,
    any_line: bool,
    pending_floats: Vec<BoxId>,
    /// Float units already placed (index into units).
    placed_floats: Vec<bool>,
    rtl: bool,
}

/// One unit's placement on a line, before alignment.
#[derive(Clone, Copy)]
struct Placed {
    unit: usize,
    x: Au,
    width: Au,
}

impl<'c, 'a, 'b> LineBreaker<'c, 'a, 'b> {
    fn new(ctx: &'c LayoutContext<'a>, container: BoxId, cb: &Cb, bfc: &'b mut Bfc, origin: Point, content: InlineContent) -> Self {
        let s = ctx.style(container);
        let n = content.units.len();
        LineBreaker {
            ctx,
            container,
            cb: *cb,
            bfc,
            origin,
            units: content.units,
            atomics: content.atomics.into_iter().map(Some).collect(),
            cw: cb.width,
            indent: s.text_indent.resolve(cb.width),
            y: Au::ZERO,
            first_line: true,
            open_stack: Vec::new(),
            fragments: Vec::new(),
            abs: Vec::new(),
            first_baseline: None,
            last_baseline: None,
            any_line: false,
            pending_floats: Vec::new(),
            placed_floats: vec![false; n],
            rtl: s.direction == Direction::Rtl,
        }
    }

    fn available(&self) -> (Au, Au) {
        let (l, r) = self.bfc.available(self.origin.y + self.y, self.origin.x, self.origin.x + self.cw);
        (l - self.origin.x, r - self.origin.x)
    }

    fn place_float_now(&mut self, id: BoxId, ceiling: Au) {
        let pf = block::prepare_float(self.ctx, id, &self.cb);
        let (frag, mut abs) = block::place_float(self.ctx, id, pf, &self.cb, self.bfc, self.origin, ceiling);
        block::translate_requests(&mut abs, frag.rect.origin.x, frag.rect.origin.y);
        self.abs.extend(abs);
        self.fragments.push(frag);
    }

    fn run(&mut self) {
        let mut i = 0;
        let mut guard = 0usize;
        while i < self.units.len() {
            let n = self.units.len();
            guard += 1;
            if guard > n * 4 + 16 {
                break;
            }
            let (mut l, mut r) = self.available();
            let mut indent = if self.first_line { self.indent } else { Au::ZERO };
            let mut avail = r - l - indent;
            // Greedy fill.
            let mut j = i;
            let mut width = Au::ZERO; // committed (excluding trailing spaces)
            let mut trailing = Au::ZERO;
            let mut has_content = false;
            let mut last_break: Option<usize> = None;
            let mut forced = false;
            let mut moved_down = false;
            let mut placed: Vec<Placed> = Vec::new();
            let mut abs_here: Vec<(BoxId, Au)> = Vec::new();
            while j < n {
                let u = &self.units[j];
                match u.kind {
                    UnitKind::Newline | UnitKind::Br(_) => {
                        placed.push(Placed { unit: j, x: width + trailing, width: Au::ZERO });
                        j += 1;
                        forced = true;
                        break;
                    }
                    UnitKind::Float(fid) => {
                        if !self.placed_floats[j] {
                            self.placed_floats[j] = true;
                            let pf = block::prepare_float(self.ctx, fid, &self.cb);
                            let fw = pf.margin_size().width;
                            let room = avail - width - trailing;
                            if fw <= room || (!has_content && placed.is_empty()) {
                                let (frag, mut abs) = block::place_float(self.ctx, fid, pf, &self.cb, self.bfc, self.origin, self.y);
                                let placed_top = frag.rect.origin.y - self.ctx.style(fid).margin.top.resolve(self.cb.width).unwrap_or(Au::ZERO);
                                block::translate_requests(&mut abs, frag.rect.origin.x, frag.rect.origin.y);
                                self.abs.extend(abs);
                                self.fragments.push(frag);
                                if placed_top <= self.y {
                                    let (nl, nr) = self.available();
                                    l = nl;
                                    r = nr;
                                    avail = r - l - indent;
                                }
                            } else {
                                self.pending_floats.push(fid);
                            }
                        }
                        j += 1;
                        continue;
                    }
                    UnitKind::Abs(aid) => {
                        abs_here.push((aid, width + trailing));
                        j += 1;
                        continue;
                    }
                    UnitKind::Space { collapsible, .. } => {
                        if !has_content && collapsible && !placed.iter().any(|p| matches!(self.units[p.unit].kind, UnitKind::Open(_)) && self.units[p.unit].width > Au::ZERO) {
                            // Leading collapsible space is removed.
                            j += 1;
                            continue;
                        }
                        if u.break_before && width + trailing + u.width > avail && has_content {
                            // break-spaces: wrap before this space.
                            break;
                        }
                        placed.push(Placed { unit: j, x: width + trailing, width: u.width });
                        trailing += u.width;
                        j += 1;
                        continue;
                    }
                    UnitKind::Word | UnitKind::Atomic(_) | UnitKind::Open(_) | UnitKind::Close(_) | UnitKind::Tab => {
                        let needed = width + trailing + u.width;
                        let is_edge = matches!(u.kind, UnitKind::Open(_) | UnitKind::Close(_));
                        if needed <= avail || (!has_content && !is_edge && width.is_zero()) || is_edge && !u.break_before {
                            if needed > avail && !has_content && !is_edge {
                                // Nothing fits on this line: next to a float, move down;
                                // otherwise break inside the word if allowed.
                                if avail < self.cw - indent && self.bfc.next_change(self.origin.y + self.y).is_some() {
                                    let ny = self.bfc.next_change(self.origin.y + self.y).unwrap() - self.origin.y;
                                    if ny > self.y {
                                        self.y = ny;
                                        moved_down = true;
                                        break;
                                    }
                                }
                                if matches!(u.kind, UnitKind::Word) && self.can_break_inside(u.owner) {
                                    self.split_word(j, avail - width - trailing);
                                }
                            }
                            let u = &self.units[j];
                            let w = u.width;
                            if u.break_before && has_content {
                                last_break = Some(j);
                            }
                            placed.push(Placed { unit: j, x: width + trailing, width: w });
                            width += trailing + w;
                            trailing = Au::ZERO;
                            if !is_edge {
                                has_content = true;
                            }
                            j += 1;
                            continue;
                        }
                        // Does not fit.
                        if u.break_before && has_content {
                            break;
                        }
                        if let Some(lb) = last_break {
                            // Back up to the last opportunity.
                            let keep = placed.iter().position(|p| p.unit == lb).unwrap_or(placed.len());
                            placed.truncate(keep);
                            j = lb;
                            break;
                        }
                        if matches!(u.kind, UnitKind::Word) && self.can_break_inside(u.owner) && has_content {
                            let room = avail - width - trailing;
                            if room > Au::ZERO && self.split_word(j, room) {
                                continue;
                            }
                            break;
                        }
                        // Overflow: place it anyway.
                        placed.push(Placed { unit: j, x: width + trailing, width: u.width });
                        width += trailing + u.width;
                        trailing = Au::ZERO;
                        has_content = true;
                        j += 1;
                    }
                }
            }
            if moved_down {
                // Retry the same units lower down; floats placed meanwhile stay.
                continue;
            }
            // Trim trailing collapsible spaces; hanging spaces stay but do not count.
            let mut end_content = placed.len();
            while end_content > 0 {
                let p = placed[end_content - 1];
                match self.units[p.unit].kind {
                    UnitKind::Space { collapsible: true, .. } => end_content -= 1,
                    UnitKind::Space { hang: true, .. } => end_content -= 1,
                    UnitKind::Close(_) | UnitKind::Newline | UnitKind::Br(_) => end_content -= 1,
                    _ => break,
                }
            }
            let mut content_width = Au::ZERO;
            for (k, p) in placed.iter().enumerate() {
                let is_trailing_space = k >= end_content && matches!(self.units[p.unit].kind, UnitKind::Space { .. });
                if !is_trailing_space {
                    content_width = content_width.max(p.x + p.width);
                }
            }
            let ended_by_br = forced;
            let emit = has_content || ended_by_br || placed.iter().any(|p| matches!(self.units[p.unit].kind, UnitKind::Open(_) | UnitKind::Close(_)) && self.units[p.unit].width > Au::ZERO);
            let more_after = j < n;
            if emit {
                let br_clear = placed.iter().rev().find_map(|p| match self.units[p.unit].kind {
                    UnitKind::Br(c) => Some(c),
                    _ => None,
                });
                self.build_line(&placed, end_content, content_width, l, avail, indent, ended_by_br || !more_after, abs_here);
                if let Some(c) = br_clear {
                    if c != Clear::None {
                        if let Some(cy) = self.bfc.clear_y(c) {
                            let cy = cy - self.origin.y;
                            if cy > self.y {
                                self.y = cy;
                            }
                        }
                    }
                }
            } else if !more_after {
                // Nothing to show on the last line; static positions of absolutes.
                for (aid, x) in abs_here {
                    self.abs.push(AbsRequest { id: aid, static_pos: Point { x: l + indent + x, y: self.y }, fixed: self.ctx.style(aid).position == Position::Fixed });
                }
            } else {
                for (aid, x) in abs_here {
                    self.abs.push(AbsRequest { id: aid, static_pos: Point { x: l + indent + x, y: self.y }, fixed: self.ctx.style(aid).position == Position::Fixed });
                }
            }
            self.first_line = false;
            indent = Au::ZERO;
            let _ = indent;
            // Floats that did not fit next to this line go below it.
            let pend = std::mem::take(&mut self.pending_floats);
            for fid in pend {
                let y = self.y;
                self.place_float_now(fid, y);
            }
            if j == i {
                // No progress (an unbreakable unit that never fits): force it.
                j = i + 1;
            }
            i = j;
        }
    }

    fn can_break_inside(&self, owner: BoxId) -> bool {
        let s = self.ctx.style(owner);
        matches!(s.overflow_wrap, OverflowWrap::Anywhere | OverflowWrap::BreakWord) || s.word_break == WordBreak::BreakWord || s.word_break == WordBreak::BreakAll
    }

    /// Splits the word unit at `j` so the first part fits in `room` (at least one
    /// character). Returns false when the word has a single character.
    fn split_word(&mut self, j: usize, room: Au) -> bool {
        let u = &self.units[j];
        let s = self.ctx.style(u.owner).clone();
        let chars: Vec<char> = u.text.chars().collect();
        if chars.len() < 2 {
            return false;
        }
        let mut w = Au::ZERO;
        let mut cut = 0;
        let mut cut_bytes = 0;
        for (k, c) in chars.iter().enumerate() {
            let a = text::advance(&s.font, *c) + s.letter_spacing;
            if k > 0 && w + a > room {
                break;
            }
            w += a;
            cut = k + 1;
            cut_bytes += c.len_utf8();
        }
        if cut >= chars.len() {
            return false;
        }
        let rest_text: String = chars[cut..].iter().collect();
        let rest_w = u.width - w;
        let (rs, re) = u.range;
        let mid = (rs + cut_bytes).min(re);
        let rest = Unit { kind: UnitKind::Word, owner: u.owner, text: rest_text, range: (mid, re), width: rest_w, break_before: true, face: u.face };
        let first = &mut self.units[j];
        first.text = chars[..cut].iter().collect();
        first.width = w;
        first.range = (rs, mid);
        self.units.insert(j + 1, rest);
        self.placed_floats.insert(j + 1, false);
        true
    }

    /// Builds the fragments of one line from its placed units.
    #[allow(clippy::too_many_arguments)]
    fn build_line(&mut self, placed: &[Placed], end_content: usize, content_width: Au, line_left: Au, avail: Au, indent: Au, last_line: bool, abs_here: Vec<(BoxId, Au)>) {
        let ctx = self.ctx;
        let cs = ctx.style(self.container);
        // Justification: extra width per expansion opportunity (spaces before content).
        let mut extra_per_space = Au::ZERO;
        let mut extra_rem = Au::ZERO;
        let mut n_spaces = 0;
        let justify = cs.text_align == TextAlign::Justify && !last_line && cs.white_space.wraps();
        if justify {
            n_spaces = placed[..end_content].iter().filter(|p| matches!(self.units[p.unit].kind, UnitKind::Space { .. })).count() as i32;
            if n_spaces > 0 {
                let free = (avail - content_width).max(Au::ZERO);
                extra_per_space = Au(free.0 / n_spaces);
                extra_rem = free - extra_per_space * n_spaces;
            }
        }
        let free = (avail - content_width).max(Au::ZERO);
        let align_shift = match cs.text_align {
            TextAlign::Left => Au::ZERO,
            TextAlign::Right => free,
            TextAlign::Center => free / 2,
            TextAlign::Start => {
                if self.rtl {
                    free
                } else {
                    Au::ZERO
                }
            }
            TextAlign::End => {
                if self.rtl {
                    Au::ZERO
                } else {
                    free
                }
            }
            TextAlign::Justify => {
                if self.rtl && (last_line || n_spaces == 0) {
                    free
                } else {
                    Au::ZERO
                }
            }
        };
        let _ = n_spaces;

        // Build the node tree for the line.
        let mut root = LineNode::new(NodeKind::Root, Au::ZERO);
        let mut stack: Vec<LineNode> = Vec::new();
        for &b in &self.open_stack {
            stack.push(LineNode::new(NodeKind::Inline { id: b, first: false, last: false }, Au::ZERO));
        }
        let mut x = Au::ZERO;
        let mut delta = Au::ZERO; // justification shift accumulated
        let mut spaces_seen = 0;
        let mut closed_on_line: Vec<BoxId> = Vec::new();
        for (k, p) in placed.iter().enumerate() {
            let u = &self.units[p.unit];
            let ux = p.x + delta;
            match u.kind {
                UnitKind::Open(id) => {
                    let mut node = LineNode::new(NodeKind::Inline { id, first: ctx.tree[id].split_first, last: false }, ux);
                    node.width = u.width;
                    stack.push(node);
                }
                UnitKind::Close(id) => {
                    if let Some(mut node) = stack.pop() {
                        if let NodeKind::Inline { last, .. } = &mut node.kind {
                            *last = ctx.tree[id].split_last;
                        }
                        node.width = ux + u.width - node.x;
                        closed_on_line.push(id);
                        match stack.last_mut() {
                            Some(parent) => parent.children.push(node),
                            None => root.children.push(node),
                        }
                    }
                }
                UnitKind::Word | UnitKind::Space { .. } | UnitKind::Tab => {
                    let is_space = matches!(u.kind, UnitKind::Space { .. });
                    let trailing_space = k >= end_content && is_space;
                    if trailing_space && matches!(u.kind, UnitKind::Space { collapsible: true, .. }) {
                        continue;
                    }
                    let mut w = u.width;
                    if is_space && justify && k < end_content {
                        let e = extra_per_space + if spaces_seen < extra_rem.0 { Au(1) } else { Au::ZERO };
                        spaces_seen += 1;
                        w += e;
                        delta += e;
                    }
                    let ob = &ctx.tree[u.owner];
                    let (node_id, source) = match &ob.kind {
                        BoxKind::Text(t) => (t.node, ob.source),
                        _ => (None, ob.source),
                    };
                    let target = stack.last_mut().map(|n| &mut n.children).unwrap_or(&mut root.children);
                    // Merge with the previous text node of the same owner and face
                    // unless justifying (each word is positioned separately then).
                    let merged = if !justify && !trailing_space {
                        match target.last_mut() {
                            Some(LineNode { kind: NodeKind::Text { owner, text, range, .. }, width: pw, x: px, .. }) if *owner == u.owner && *px + *pw == ux && !is_space_only(text) || false => {
                                let _ = owner;
                                text.push_str(&u.text);
                                range.1 = range.1.max(u.range.1);
                                *pw += w;
                                true
                            }
                            _ => false,
                        }
                    } else {
                        false
                    };
                    if !merged {
                        let mut node = LineNode::new(NodeKind::Text { owner: u.owner, text: u.text.clone(), range: u.range, node: node_id, source }, ux);
                        node.width = w;
                        target.push(node);
                    }
                }
                UnitKind::Atomic(idx) => {
                    let mut node = LineNode::new(NodeKind::Atomic(idx), ux);
                    node.width = u.width;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => root.children.push(node),
                    }
                }
                UnitKind::Newline | UnitKind::Br(_) | UnitKind::Float(_) | UnitKind::Abs(_) => {}
            }
            x = ux + u.width;
        }
        let _ = x;
        // Boxes still open continue on the next line.
        let mut still_open = Vec::new();
        while let Some(mut node) = stack.pop() {
            if let NodeKind::Inline { id, .. } = node.kind {
                still_open.push(id);
            }
            node.width = (content_width + delta - node.x).max(Au::ZERO);
            match stack.last_mut() {
                Some(parent) => parent.children.push(node),
                None => root.children.push(node),
            }
        }
        still_open.reverse();
        self.open_stack = still_open;

        // Vertical alignment.
        let root_m = metrics_of(cs);
        root.above = root_m.above;
        root.below = root_m.below;
        let mut min_top = -root.above;
        let mut max_bottom = root.below;
        let mut aligned_heights: Vec<Au> = Vec::new();
        self.assign_metrics(&mut root, cs, &root_m);
        self.place_vertical(&mut root, Au::ZERO, cs, &root_m, &mut min_top, &mut max_bottom, &mut aligned_heights);
        let mut line_height = max_bottom - min_top;
        for h in &aligned_heights {
            line_height = line_height.max(*h);
        }
        if placed.is_empty() {
            line_height = root.above + root.below;
        }
        let root_baseline = -min_top;
        // Shift baselines to line coordinates and position top/bottom subtrees.
        shift_baselines(&mut root, root_baseline);
        self.finalize_aligned(&mut root, Au::ZERO, line_height);

        // Emit fragments. Line rect is in content-box coordinates.
        let line_x = line_left + indent + align_shift;
        let mut line = Fragment::new(FragmentKind::Line, Rect::new(line_left, self.y, avail + indent, line_height));
        let inner_x = indent + align_shift;
        let total = content_width + delta;
        for child in root.children.drain(..) {
            let f = self.emit(child, Au::ZERO, Au::ZERO, total, inner_x);
            line.children.push(f);
        }
        block::compute_overflow(&mut line, false);
        // text-overflow: ellipsis on overflow-clipping nowrap blocks.
        if cs.text_overflow == TextOverflow::Ellipsis && cs.overflow_x != Overflow::Visible && total > avail {
            let edge = self.cw - line_left;
            apply_ellipsis(ctx, &mut line, edge, cs);
        }
        let baseline_y = self.y + root_baseline;
        if self.first_baseline.is_none() {
            self.first_baseline = Some(baseline_y);
        }
        self.last_baseline = Some(baseline_y);
        for (aid, ax) in abs_here {
            let sx = if self.rtl { line_x + (total - ax) } else { line_x + ax };
            self.abs.push(AbsRequest { id: aid, static_pos: Point { x: sx, y: self.y }, fixed: ctx.style(aid).position == Position::Fixed });
        }
        self.y += line_height;
        self.any_line = true;
        self.fragments.push(line);
    }

    fn assign_metrics(&self, node: &mut LineNode, parent_style: &ComputedStyle, parent_m: &Metrics) {
        for c in &mut node.children {
            match &c.kind {
                NodeKind::Inline { id, .. } => {
                    let s = self.ctx.style(*id);
                    let m = metrics_of(s);
                    c.above = m.above;
                    c.below = m.below;
                    let id = *id;
                    let st = self.ctx.style(id);
                    self.assign_metrics(c, st, &m);
                }
                NodeKind::Text { .. } => {
                    c.above = parent_m.above;
                    c.below = parent_m.below;
                }
                NodeKind::Atomic(i) => {
                    if let Some(a) = &self.atomics[*i] {
                        c.above = a.above();
                        c.below = a.below();
                    }
                }
                NodeKind::Root => {}
            }
        }
        let _ = parent_style;
    }

    fn va_of(&self, c: &LineNode) -> VerticalAlign {
        match &c.kind {
            NodeKind::Inline { id, .. } => self.ctx.style(*id).vertical_align,
            NodeKind::Atomic(i) => match &self.atomics[*i] {
                Some(a) => self.ctx.style(a.id).vertical_align,
                None => VerticalAlign::Baseline,
            },
            _ => VerticalAlign::Baseline,
        }
    }

    /// Computes baselines relative to the root baseline and the line extent.
    /// `top`/`bottom` aligned children are measured around their own baseline
    /// (their extent stored in `above`/`below`) and positioned by `finalize_aligned`.
    #[allow(clippy::too_many_arguments)]
    fn place_vertical(&self, node: &mut LineNode, baseline: Au, pstyle: &ComputedStyle, pm: &Metrics, min_top: &mut Au, max_bottom: &mut Au, aligned: &mut Vec<Au>) {
        node.baseline = baseline;
        for c in &mut node.children {
            let (cstyle, cm): (&ComputedStyle, Metrics) = match &c.kind {
                NodeKind::Inline { id, .. } => {
                    let s = self.ctx.style(*id);
                    (s, metrics_of(s))
                }
                NodeKind::Atomic(i) => {
                    let id = self.atomics[*i].as_ref().map(|a| a.id).unwrap_or(self.container);
                    let s = self.ctx.style(id);
                    (s, metrics_of(s))
                }
                _ => {
                    // Text: on the parent's baseline.
                    c.baseline = baseline;
                    *min_top = (*min_top).min(baseline - c.above);
                    *max_bottom = (*max_bottom).max(baseline + c.below);
                    continue;
                }
            };
            let va = cstyle.vertical_align;
            match va {
                VerticalAlign::Top | VerticalAlign::Bottom => {
                    let mut sub_min = -c.above;
                    let mut sub_max = c.below;
                    let mut inner: Vec<Au> = Vec::new();
                    self.place_vertical(c, Au::ZERO, cstyle, &cm, &mut sub_min, &mut sub_max, &mut inner);
                    for h in inner {
                        sub_max = sub_max.max(sub_min + h);
                    }
                    // Nested aligned boxes align within this subtree's extent.
                    self.finalize_aligned(c, sub_min, sub_max - sub_min);
                    c.above = -sub_min;
                    c.below = sub_max;
                    aligned.push(sub_max - sub_min);
                }
                _ => {
                    let shift = baseline_shift(va, cstyle, &cm, pstyle, pm, c.above, c.below);
                    let cb = baseline + shift;
                    self.place_vertical(c, cb, cstyle, &cm, min_top, max_bottom, aligned);
                    *min_top = (*min_top).min(cb - c.above);
                    *max_bottom = (*max_bottom).max(cb + c.below);
                }
            }
        }
    }

    /// Positions `top`/`bottom` aligned children of `node` inside the extent
    /// `[top, top + height)`; other children are searched recursively.
    fn finalize_aligned(&self, node: &mut LineNode, top: Au, height: Au) {
        let vas: Vec<VerticalAlign> = node.children.iter().map(|c| self.va_of(c)).collect();
        for (c, va) in node.children.iter_mut().zip(vas) {
            match va {
                VerticalAlign::Top => {
                    let d = top + c.above - c.baseline;
                    shift_baselines(c, d);
                }
                VerticalAlign::Bottom => {
                    let d = top + height - c.below - c.baseline;
                    shift_baselines(c, d);
                }
                _ => self.finalize_aligned(c, top, height),
            }
        }
    }

    /// Emits the fragment of a line node; `px`/`py` are the parent's origin in line
    /// coordinates, `total` the line's content width (for rtl mirroring) and
    /// `inner_x` where the content starts within the line (indent and alignment).
    fn emit(&mut self, node: LineNode, px: Au, py: Au, total: Au, inner_x: Au) -> Fragment {
        let ctx = self.ctx;
        let x_of = |x: Au, w: Au| -> Au {
            if self.rtl {
                inner_x + total - (x + w)
            } else {
                inner_x + x
            }
        };
        match node.kind {
            NodeKind::Inline { id, first, last } => {
                let s = ctx.style(id);
                let p = block::padding_edges(s, self.cb.width);
                let bw = s.used_border_widths();
                let fm = text::font_metrics(&s.font);
                let ml = if first { s.margin.left.resolve(self.cb.width).unwrap_or(Au::ZERO) } else { Au::ZERO };
                let mr = if last { s.margin.right.resolve(self.cb.width).unwrap_or(Au::ZERO) } else { Au::ZERO };
                let (ml, mr) = if self.rtl { (mr, ml) } else { (ml, mr) };
                let pl = if first { p.left } else { Au::ZERO };
                let pr = if last { p.right } else { Au::ZERO };
                let bl = if first { bw.left } else { Au::ZERO };
                let br = if last { bw.right } else { Au::ZERO };
                let (pl, pr, bl, br) = if self.rtl { (pr, pl, br, bl) } else { (pl, pr, bl, br) };
                let x = x_of(node.x, node.width) + ml;
                let w = (node.width - ml - mr).max(Au::ZERO);
                let top = node.baseline - fm.ascent - p.top - bw.top;
                let h = fm.content_height() + p.vertical() + bw.vertical();
                let rect = Rect::new(x - px, top - py, w, h);
                let mut f = Fragment::new(FragmentKind::InlineBox { source: ctx.tree[id].source, padding: Edges { top: p.top, right: pr, bottom: p.bottom, left: pl }, border: Edges { top: bw.top, right: br, bottom: bw.bottom, left: bl }, first, last }, rect);
                let cx = x;
                let cy = top;
                let mut kids = Vec::new();
                let ch: Vec<LineNode> = node.children;
                for c in ch {
                    kids.push(self.emit(c, cx, cy, total, inner_x));
                }
                f.children = kids;
                let off = block::relative_offset(s, &self.cb);
                f.rect.origin.x += off.x;
                f.rect.origin.y += off.y;
                f.is_positioned = s.is_positioned();
                f.z_index = match s.z_index {
                    crate::style::ZIndex::Int(i) => i,
                    _ => 0,
                };
                f.establishes_stacking_context = s.establishes_stacking_context(false);
                block::compute_overflow(&mut f, false);
                f
            }
            NodeKind::Text { owner, text, range, node: tnode, source } => {
                let s = ctx.style(owner);
                let fm = text::font_metrics(&s.font);
                let x = x_of(node.x, node.width);
                let rect = Rect::new(x - px, node.baseline - fm.ascent - py, node.width, fm.content_height());
                Fragment::new(FragmentKind::Text { source, text, node: tnode, range, baseline: fm.ascent, ellipsis: false }, rect)
            }
            NodeKind::Atomic(i) => {
                let Some(a) = self.atomics[i].take() else {
                    return Fragment::new(FragmentKind::Line, Rect::default());
                };
                let above = a.above();
                let margin = a.margin;
                let aid = a.id;
                let mut f = a.fragment;
                let mut abs = a.abs;
                let ml = if self.rtl { margin.right } else { margin.left };
                let x = x_of(node.x, node.width) + ml;
                let top = node.baseline - above + margin.top;
                f.rect.origin = Point { x: x - px, y: top - py };
                let s = ctx.style(aid);
                let off = block::relative_offset(s, &self.cb);
                f.rect.origin.x += off.x;
                f.rect.origin.y += off.y;
                // Absolute requests inside: relative to this fragment; carry to the line
                // then to the block (positions get translated as fragments nest).
                block::translate_requests(&mut abs, f.rect.origin.x + px, f.rect.origin.y + py);
                for r in &mut abs {
                    r.static_pos.y += self.y;
                }
                self.abs.extend(abs);
                f
            }
            NodeKind::Root => Fragment::new(FragmentKind::Line, Rect::default()),
        }
    }

    fn finish(self) -> InlineResult {
        InlineResult { fragments: self.fragments, height: self.y, empty: !self.any_line, first_baseline: self.first_baseline, last_baseline: self.last_baseline, abs: self.abs }
    }
}

fn is_space_only(t: &str) -> bool {
    t.chars().all(|c| c == ' ')
}

fn shift_baselines(node: &mut LineNode, d: Au) {
    node.baseline += d;
    for c in &mut node.children {
        shift_baselines(c, d);
    }
}

/// The baseline shift of a child relative to its parent's baseline (§10.8.1),
/// positive downwards. `above`/`below` are the child's extents around its baseline.
#[allow(clippy::too_many_arguments)]
fn baseline_shift(va: VerticalAlign, cstyle: &ComputedStyle, cm: &Metrics, pstyle: &ComputedStyle, pm: &Metrics, above: Au, below: Au) -> Au {
    match va {
        VerticalAlign::Baseline => Au::ZERO,
        VerticalAlign::Sub => pstyle.font.size / 5,
        VerticalAlign::Super => -(pstyle.font.size / 3),
        VerticalAlign::TextTop => -pm.fm.ascent + above,
        VerticalAlign::TextBottom => pm.fm.descent - below,
        VerticalAlign::Middle => {
            let mid = (below - above) / 2;
            -(pm.fm.x_height / 2) - mid
        }
        VerticalAlign::Length(lp) => {
            let lh = cstyle.line_height_au(cm.fm.normal_line_height());
            -lp.resolve(lh)
        }
        VerticalAlign::Top | VerticalAlign::Bottom => Au::ZERO,
    }
}

/// Cuts the text fragments of a line at `edge` (content-box x) and appends `…`.
fn apply_ellipsis(ctx: &LayoutContext, line: &mut Fragment, edge: Au, cs: &ComputedStyle) {
    let line_x = line.rect.origin.x;
    let mut done = false;
    ellipsize_children(ctx, &mut line.children, edge - line_x, cs, &mut done);
}

fn ellipsize_children(ctx: &LayoutContext, kids: &mut Vec<Fragment>, edge: Au, cs: &ComputedStyle, done: &mut bool) {
    let mut keep = kids.len();
    for (i, k) in kids.iter_mut().enumerate() {
        if *done {
            keep = i;
            break;
        }
        let left = k.rect.origin.x;
        let right = k.rect.right();
        if right <= edge {
            continue;
        }
        match &mut k.kind {
            FragmentKind::Text { text, ellipsis, source, .. } => {
                let s = match source {
                    StyleSource::Before(n) => ctx.styles.before(*n),
                    StyleSource::After(n) => ctx.styles.after(*n),
                    StyleSource::Marker(n) => ctx.styles.marker(*n),
                    src => ctx.styles.get(src.node()),
                }
                .unwrap_or(cs);
                let ell = text::advance(&s.font, '\u{2026}');
                let room = edge - left - ell;
                let mut w = Au::ZERO;
                let mut out = String::new();
                for c in text.chars() {
                    let a = text::advance(&s.font, c) + s.letter_spacing;
                    if w + a > room {
                        break;
                    }
                    w += a;
                    out.push(c);
                }
                let out = out.trim_end().to_owned();
                let w = text::measure(&s.font, &out, s.letter_spacing, s.word_spacing);
                *text = format!("{out}\u{2026}");
                *ellipsis = true;
                k.rect.size.width = w + ell;
                *done = true;
                keep = i + 1;
                break;
            }
            FragmentKind::InlineBox { .. } => {
                let inner_edge = edge - left;
                ellipsize_children(ctx, &mut k.children, inner_edge, cs, done);
                if *done {
                    keep = i + 1;
                    break;
                }
            }
            _ => {
                if left >= edge {
                    keep = i;
                    *done = true;
                    break;
                }
            }
        }
    }
    kids.truncate(keep);
}

/// Intrinsic widths of an inline formatting context: the longest unbreakable run and
/// the longest line without soft wraps (§10.3.5 / css-sizing).
pub fn intrinsic_widths(ctx: &LayoutContext, container: BoxId) -> (Au, Au) {
    let cb = Cb { width: Au::ZERO, height: None };
    let content = collect(ctx, container, &cb, false);
    let s = ctx.style(container);
    let indent = match s.text_indent {
        crate::style::LengthPercentage::Length(l) => l,
        _ => Au::ZERO,
    };
    let mut min = Au::ZERO;
    let mut max = Au::ZERO;
    let mut run = Au::ZERO; // current unbreakable run
    let mut line = indent; // current max-content line
    let mut line_trailing = Au::ZERO;
    let mut float_sum = Au::ZERO;
    let mut first = true;
    let mut pending_spaces = Au::ZERO;
    for u in &content.units {
        match u.kind {
            UnitKind::Newline | UnitKind::Br(_) => {
                min = min.max(run);
                run = Au::ZERO;
                max = max.max(line - line_trailing);
                line = Au::ZERO;
                line_trailing = Au::ZERO;
                pending_spaces = Au::ZERO;
                first = false;
            }
            UnitKind::Space { .. } => {
                if line.is_zero() && first && indent.is_zero() || line == indent && first {
                    continue;
                }
                line += u.width;
                line_trailing += u.width;
                if u.break_before {
                    min = min.max(run);
                    run = Au::ZERO;
                }
                pending_spaces += u.width;
                let owner_ws = ctx.style(u.owner).white_space;
                if owner_ws.wraps() {
                    min = min.max(run);
                    run = Au::ZERO;
                    pending_spaces = Au::ZERO;
                } else {
                    run += u.width;
                }
            }
            UnitKind::Float(id) => {
                let (fmn, fmx) = crate::layout::intrinsic::min_max(ctx, id);
                let fs = ctx.style(id);
                let m = fs.margin.left.resolve(Au::ZERO).unwrap_or(Au::ZERO) + fs.margin.right.resolve(Au::ZERO).unwrap_or(Au::ZERO);
                min = min.max(fmn + m);
                float_sum += fmx + m;
            }
            UnitKind::Abs(_) => {}
            UnitKind::Word | UnitKind::Atomic(_) | UnitKind::Open(_) | UnitKind::Close(_) | UnitKind::Tab => {
                let w = match u.kind {
                    UnitKind::Atomic(i) => {
                        let a = &content.atomics[i];
                        let (mn, _) = crate::layout::intrinsic::min_max(ctx, a.id);
                        // Min-content uses the atomic's min-content width.
                        if u.break_before {
                            min = min.max(run);
                            run = Au::ZERO;
                        }
                        run += mn + a.margin.horizontal();
                        min = min.max(run);
                        run = Au::ZERO;
                        u.width
                    }
                    _ => {
                        if u.break_before {
                            min = min.max(run);
                            run = Au::ZERO;
                        }
                        run += u.width;
                        u.width
                    }
                };
                line += w;
                line_trailing = Au::ZERO;
                let _ = pending_spaces;
            }
        }
    }
    min = min.max(run);
    max = max.max(line - line_trailing);
    (min.max(Au::ZERO), (max + float_sum).max(min))
}

/// Whether this box's inline content is an inline formatting context (used by
/// intrinsic sizing to route).
pub fn is_inline_container(ctx: &LayoutContext, id: BoxId) -> bool {
    ctx.tree[id].inline_children && ctx.tree[id].level != Level::Inline || ctx.tree[id].inline_children
}

#[allow(dead_code)]
fn _size_unused(_: Size) {}
