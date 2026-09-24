//! Box tree generation from the styled DOM (CSS 2.1 §9.2, §12, §17.2.1).
//!
//! What happens here: `display: none` subtrees are skipped, `display: contents`
//! elements are transparent, anonymous block boxes wrap runs of inline content next to
//! block-level siblings, inline boxes containing blocks are split, list items get
//! marker boxes, `::before`/`::after` generate content (text, `attr()`, counters and
//! quotes), tables get their anonymous wrappers and missing parents/children, and
//! replaced elements get their intrinsic sizes.

use std::collections::BTreeMap;
use std::ops::Index;
use std::rc::Rc;

use crate::dom::{Document, NodeId, NodeKind};
use crate::geom::{Au, Size};
use crate::layout::fragment::{ControlKind, Replaced, StyleSource};
use crate::layout::text;
use crate::layout::ImageSizes;
use crate::style::{
    Clear, ComputedStyle, Content, ContentItem, Display, Float, LengthPercentage,
    LengthPercentageAuto, ListStylePosition, ListStyleType, Position, Sizing, StyleSet, WhiteSpace,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoxId(pub u32);

impl BoxId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A length from an HTML attribute such as `width="50%"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dim {
    Px(Au),
    Percent(i32),
}

impl Dim {
    pub fn parse(s: &str) -> Option<Dim> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let (num, pct) = match s.strip_suffix('%') {
            Some(n) => (n.trim(), true),
            None => (s.trim_end_matches("px"), false),
        };
        let digits: String = num
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if digits.is_empty() {
            return None;
        }
        let mut parts = digits.splitn(2, '.');
        let whole: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let frac_s = parts.next().unwrap_or("");
        let frac: i64 = frac_s
            .chars()
            .take(2)
            .collect::<String>()
            .parse()
            .unwrap_or(0)
            * if frac_s.len() >= 2 { 1 } else { 10 };
        let centi = whole * 100 + frac;
        if pct {
            Some(Dim::Percent(centi.min(i32::MAX as i64) as i32))
        } else {
            Some(Dim::Px(Au(
                (centi * 64 / 100).clamp(0, Au::MAX.0 as i64) as i32
            )))
        }
    }
    pub fn to_sizing(self) -> Sizing {
        match self {
            Dim::Px(a) => Sizing::Set(LengthPercentage::Length(a)),
            Dim::Percent(p) => Sizing::Set(LengthPercentage::Percent(p)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBox {
    pub text: String,
    /// The DOM text node; `None` for generated content.
    pub node: Option<NodeId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacedBox {
    pub replaced: Replaced,
    /// Intrinsic content-box size, when known (image cache, control metrics).
    pub intrinsic: Option<Size>,
    pub attr_width: Option<Dim>,
    pub attr_height: Option<Dim>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellBox {
    pub colspan: u32,
    pub rowspan: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColBox {
    pub span: u32,
    pub width: Option<Dim>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoxKind {
    /// A block container box (block, list-item principal box, flow-root, anonymous
    /// block, body of a `<button>`).
    Block,
    /// An atomic inline block container (`inline-block`).
    InlineBlock,
    /// A non-replaced inline box.
    Inline,
    Text(TextBox),
    Replaced(ReplacedBox),
    /// `<br>`; `clear` from the style or the attribute.
    Br(Clear),
    /// `<wbr>`.
    Wbr,
    /// An outside list marker, positioned by its list item.
    Marker(String),
    /// The table wrapper box: margins, float and position; children are captions and
    /// the table grid box.
    TableWrapper,
    /// The table grid box: border, padding, background; children are column groups,
    /// columns and row groups.
    Table,
    RowGroup,
    Row,
    Cell(CellBox),
    ColGroup(ColBox),
    Col(ColBox),
    Caption,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Block,
    Inline,
}

#[derive(Clone, Debug)]
pub struct LayoutBox {
    pub kind: BoxKind,
    pub style: Rc<ComputedStyle>,
    pub source: StyleSource,
    /// The element this box was generated for (attributes); `None` for anonymous and
    /// generated boxes.
    pub node: Option<NodeId>,
    pub level: Level,
    pub children: Vec<BoxId>,
    /// For block containers: the children are inline-level (an inline formatting
    /// context) rather than block-level.
    pub inline_children: bool,
    /// The outside marker box of a list item.
    pub marker: Option<BoxId>,
    /// A block container that is a form control (`<button>`): paint draws the control
    /// and its children.
    pub control: Option<ControlKind>,
    /// The `<html>` element's box.
    pub is_root: bool,
    /// The box is a continuation piece of an inline split around a block; first and
    /// last pieces carry the start and end edges.
    pub split_first: bool,
    pub split_last: bool,
    /// The box is a flex or grid item: it establishes an independent formatting
    /// context and its `z-index` creates a stacking context.
    pub is_item: bool,
}

impl LayoutBox {
    pub fn is_block_container(&self) -> bool {
        matches!(
            self.kind,
            BoxKind::Block | BoxKind::InlineBlock | BoxKind::Cell(_) | BoxKind::Caption
        )
    }
    pub fn is_table_internal(&self) -> bool {
        matches!(
            self.kind,
            BoxKind::RowGroup
                | BoxKind::Row
                | BoxKind::Cell(_)
                | BoxKind::ColGroup(_)
                | BoxKind::Col(_)
                | BoxKind::Caption
        )
    }
    pub fn is_out_of_flow(&self) -> bool {
        self.style.is_out_of_flow()
    }
    pub fn is_float(&self) -> bool {
        !self.is_text() && self.style.is_floating()
    }
    /// A text box borrows its parent element's computed style, but the text itself
    /// is never out of flow: `position` and `float` belong to the element, whose own
    /// box already carries them. Without this an absolutely positioned flex
    /// container treated its text as an absolutely positioned child, laying the
    /// element's box out a second time inside itself.
    fn is_text(&self) -> bool {
        matches!(self.kind, BoxKind::Text(_))
    }
    pub fn is_abs(&self) -> bool {
        !self.is_text() && matches!(self.style.position, Position::Absolute | Position::Fixed)
    }
    pub fn is_atomic_inline(&self) -> bool {
        self.level == Level::Inline
            && matches!(
                self.kind,
                BoxKind::InlineBlock | BoxKind::Replaced(_) | BoxKind::TableWrapper
            )
    }
    /// Establishes a new block formatting context for its contents.
    pub fn establishes_bfc(&self) -> bool {
        let s = &self.style;
        self.is_root
            || self.is_item
            || s.is_out_of_flow()
            || matches!(
                self.kind,
                BoxKind::InlineBlock
                    | BoxKind::Cell(_)
                    | BoxKind::Caption
                    | BoxKind::TableWrapper
                    | BoxKind::Table
            )
            || matches!(
                s.display,
                Display::FlowRoot
                    | Display::InlineBlock
                    | Display::Flex
                    | Display::InlineFlex
                    | Display::Grid
                    | Display::InlineGrid
            )
            || !matches!(s.overflow_x, crate::style::Overflow::Visible)
            || !matches!(s.overflow_y, crate::style::Overflow::Visible)
            || self.control.is_some()
    }
    pub fn is_scroll_container(&self) -> bool {
        use crate::style::Overflow as O;
        matches!(
            self.style.overflow_x,
            O::Hidden | O::Scroll | O::Auto | O::Clip
        ) || matches!(
            self.style.overflow_y,
            O::Hidden | O::Scroll | O::Auto | O::Clip
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct BoxTree {
    pub boxes: Vec<LayoutBox>,
    /// The `<html>` element's box.
    pub root: Option<BoxId>,
    /// The principal box of each element that generated one.
    pub by_node: BTreeMap<NodeId, BoxId>,
}

impl Index<BoxId> for BoxTree {
    type Output = LayoutBox;
    fn index(&self, i: BoxId) -> &LayoutBox {
        &self.boxes[i.index()]
    }
}

impl BoxTree {
    pub fn len(&self) -> usize {
        self.boxes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }
    pub fn box_of(&self, n: NodeId) -> Option<BoxId> {
        self.by_node.get(&n).copied()
    }
    pub fn children(&self, b: BoxId) -> &[BoxId] {
        &self.boxes[b.index()].children
    }
}

struct Counter {
    name: String,
    value: i32,
    depth: u32,
}

struct Builder<'a> {
    doc: &'a Document,
    styles: &'a StyleSet,
    images: &'a dyn ImageSizes,
    boxes: Vec<LayoutBox>,
    by_node: BTreeMap<NodeId, BoxId>,
    counters: Vec<Counter>,
    quote_depth: i32,
}

/// Builds the box tree for the whole document.
pub fn build(doc: &Document, styles: &StyleSet, images: &dyn ImageSizes) -> BoxTree {
    let mut b = Builder {
        doc,
        styles,
        images,
        boxes: Vec::with_capacity(doc.len()),
        by_node: BTreeMap::new(),
        counters: Vec::new(),
        quote_depth: 0,
    };
    let root = doc.document_element().and_then(|html| {
        let items = b.build_element(html, 0);
        items
            .into_iter()
            .find(|&i| b.boxes[i.index()].level == Level::Block)
    });
    if let Some(r) = root {
        b.boxes[r.index()].is_root = true;
        let s = &b.boxes[r.index()].style;
        // The root is always block-level and in flow.
        if s.display.is_inline_level() || s.is_out_of_flow() {
            let mut st = (**s).clone();
            st.display = Display::Block;
            st.position = if st.position == Position::Relative {
                Position::Relative
            } else {
                Position::Static
            };
            st.float = Float::None;
            b.boxes[r.index()].style = Rc::new(st);
            b.boxes[r.index()].level = Level::Block;
        }
    }
    BoxTree {
        boxes: b.boxes,
        root,
        by_node: b.by_node,
    }
}

fn anon_style(parent: &ComputedStyle, display: Display) -> Rc<ComputedStyle> {
    let mut s = ComputedStyle::inherit_from(parent);
    s.display = display;
    Rc::new(s)
}

impl<'a> Builder<'a> {
    fn push(
        &mut self,
        kind: BoxKind,
        style: Rc<ComputedStyle>,
        source: StyleSource,
        node: Option<NodeId>,
        level: Level,
    ) -> BoxId {
        let id = BoxId(self.boxes.len() as u32);
        self.boxes.push(LayoutBox {
            kind,
            style,
            source,
            node,
            level,
            children: Vec::new(),
            inline_children: false,
            marker: None,
            control: None,
            is_root: false,
            split_first: true,
            split_last: true,
            is_item: false,
        });
        id
    }

    fn level_of(display: Display) -> Level {
        if display.is_inline_level() {
            Level::Inline
        } else {
            Level::Block
        }
    }

    // Counters (CSS 2.1 §12.4).

    fn counter_reset(&mut self, name: &str, value: i32, depth: u32) {
        self.counters.push(Counter {
            name: name.to_owned(),
            value,
            depth,
        });
    }
    fn counter_increment(&mut self, name: &str, by: i32, depth: u32) {
        match self.counters.iter_mut().rev().find(|c| c.name == name) {
            Some(c) => c.value = c.value.saturating_add(by),
            None => self.counters.push(Counter {
                name: name.to_owned(),
                value: by,
                depth,
            }),
        }
    }
    fn counter_set(&mut self, name: &str, value: i32, depth: u32) {
        match self.counters.iter_mut().rev().find(|c| c.name == name) {
            Some(c) => c.value = value,
            None => self.counters.push(Counter {
                name: name.to_owned(),
                value,
                depth,
            }),
        }
    }
    fn counter_value(&self, name: &str) -> i32 {
        self.counters
            .iter()
            .rev()
            .find(|c| c.name == name)
            .map(|c| c.value)
            .unwrap_or(0)
    }
    fn counters_leave(&mut self, depth: u32) {
        while self.counters.last().is_some_and(|c| c.depth > depth) {
            self.counters.pop();
        }
    }
    fn apply_counter_props(&mut self, style: &ComputedStyle, depth: u32) {
        let resets = style.counter_reset.clone();
        for (n, v) in resets {
            self.counter_reset(&n, v, depth);
        }
        let incs = style.counter_increment.clone();
        for (n, v) in incs {
            self.counter_increment(&n, v, depth);
        }
    }

    /// Builds the boxes for an element; returns the items it contributes to its
    /// parent (several for `display: contents` and split inlines).
    fn build_element(&mut self, node: NodeId, depth: u32) -> Vec<BoxId> {
        let tag = self.doc.tag(node).unwrap_or("");
        if matches!(
            tag,
            "head"
                | "script"
                | "style"
                | "template"
                | "meta"
                | "link"
                | "title"
                | "base"
                | "param"
                | "datalist"
                | "noscript"
                | "area"
        ) {
            return Vec::new();
        }
        if tag == "input"
            && self
                .doc
                .attr(node, "type")
                .is_some_and(|t| t.eq_ignore_ascii_case("hidden"))
        {
            return Vec::new();
        }
        let Some(style) = self.styles.get_rc(node).cloned() else {
            return Vec::new();
        };
        if style.display.is_none() {
            return Vec::new();
        }
        self.apply_counter_props(&style, depth);
        if style.display == Display::Contents {
            let mut items = Vec::new();
            if let Some(b) = self.build_pseudo(node, &style, true, depth) {
                items.push(b);
            }
            items.extend(self.build_children(node, &style, depth));
            if let Some(b) = self.build_pseudo(node, &style, false, depth) {
                items.push(b);
            }
            self.counters_leave(depth);
            return items;
        }
        // Blockify floats, absolutes, and flex and grid items (css-display §2.7).
        let is_item = crate::layout::flex::is_flex_or_grid_item(self.doc, self.styles, node);
        let style = if is_item
            && crate::layout::flex::blockify_item_display(style.display) != style.display
        {
            let mut s = (*style).clone();
            s.display = crate::layout::flex::blockify_item_display(s.display);
            Rc::new(s)
        } else if style.is_out_of_flow() && style.display.is_inline_level() {
            let mut s = (*style).clone();
            s.display = s.display.blockify();
            Rc::new(s)
        } else {
            style
        };
        let display = style.display;
        let level = Self::level_of(display);
        let source = StyleSource::Element(node);

        // Replaced elements and controls.
        if let Some(rb) = self.replaced_box(node, tag, &style) {
            let id = self.push(BoxKind::Replaced(rb), style, source, Some(node), level);
            self.by_node.insert(node, id);
            self.counters_leave(depth);
            return vec![id];
        }
        if tag == "br" {
            let clear = if style.clear != Clear::None {
                style.clear
            } else {
                match self
                    .doc
                    .attr(node, "clear")
                    .map(|s| s.to_ascii_lowercase())
                    .as_deref()
                {
                    Some("left") => Clear::Left,
                    Some("right") => Clear::Right,
                    Some("all") | Some("both") => Clear::Both,
                    _ => Clear::None,
                }
            };
            let id = self.push(BoxKind::Br(clear), style, source, Some(node), Level::Inline);
            self.by_node.insert(node, id);
            return vec![id];
        }
        if tag == "wbr" {
            let id = self.push(BoxKind::Wbr, style, source, Some(node), Level::Inline);
            self.by_node.insert(node, id);
            return vec![id];
        }

        match display {
            Display::Table | Display::InlineTable => {
                let items = self.build_table(node, &style, depth);
                self.counters_leave(depth);
                items
            }
            Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup => {
                let id = self.push(
                    BoxKind::RowGroup,
                    style.clone(),
                    source,
                    Some(node),
                    Level::Block,
                );
                self.by_node.insert(node, id);
                let kids = self.build_children(node, &style, depth);
                let kids = self.fixup_row_group_children(&style, kids);
                self.boxes[id.index()].children = kids;
                self.counters_leave(depth);
                vec![id]
            }
            Display::TableRow => {
                let id = self.push(
                    BoxKind::Row,
                    style.clone(),
                    source,
                    Some(node),
                    Level::Block,
                );
                self.by_node.insert(node, id);
                let kids = self.build_children(node, &style, depth);
                let kids = self.fixup_row_children(&style, kids);
                self.boxes[id.index()].children = kids;
                self.counters_leave(depth);
                vec![id]
            }
            Display::TableCell => {
                let cell = CellBox {
                    colspan: attr_u32(self.doc, node, "colspan", 1).clamp(1, 1000),
                    rowspan: attr_u32(self.doc, node, "rowspan", 1).clamp(1, 65534),
                };
                let id = self.push(
                    BoxKind::Cell(cell),
                    style.clone(),
                    source,
                    Some(node),
                    Level::Block,
                );
                self.by_node.insert(node, id);
                self.build_container_contents(id, node, &style, depth);
                self.counters_leave(depth);
                vec![id]
            }
            Display::TableCaption => {
                let id = self.push(
                    BoxKind::Caption,
                    style.clone(),
                    source,
                    Some(node),
                    Level::Block,
                );
                self.by_node.insert(node, id);
                self.build_container_contents(id, node, &style, depth);
                self.counters_leave(depth);
                vec![id]
            }
            Display::TableColumnGroup => {
                let span = attr_u32(self.doc, node, "span", 1).clamp(1, 1000);
                let width = self.doc.attr(node, "width").and_then(Dim::parse);
                let id = self.push(
                    BoxKind::ColGroup(ColBox { span, width }),
                    style.clone(),
                    source,
                    Some(node),
                    Level::Block,
                );
                self.by_node.insert(node, id);
                let mut cols = Vec::new();
                for c in self.doc.children(node).collect::<Vec<_>>() {
                    if self.doc.is(c, "col") {
                        let items = self.build_element(c, depth + 1);
                        cols.extend(
                            items
                                .into_iter()
                                .filter(|i| matches!(self.boxes[i.index()].kind, BoxKind::Col(_))),
                        );
                    }
                }
                self.boxes[id.index()].children = cols;
                vec![id]
            }
            Display::TableColumn => {
                let span = attr_u32(self.doc, node, "span", 1).clamp(1, 1000);
                let width = self.doc.attr(node, "width").and_then(Dim::parse);
                let id = self.push(
                    BoxKind::Col(ColBox { span, width }),
                    style,
                    source,
                    Some(node),
                    Level::Block,
                );
                self.by_node.insert(node, id);
                vec![id]
            }
            Display::Inline => {
                let id = self.push(
                    BoxKind::Inline,
                    style.clone(),
                    source,
                    Some(node),
                    Level::Inline,
                );
                self.by_node.insert(node, id);
                let mut kids = Vec::new();
                if let Some(b) = self.build_pseudo(node, &style, true, depth) {
                    kids.push(b);
                }
                kids.extend(self.build_children(node, &style, depth));
                if let Some(b) = self.build_pseudo(node, &style, false, depth) {
                    kids.push(b);
                }
                self.counters_leave(depth);
                // Block-in-inline splitting (§9.2.1.1).
                let has_block = kids.iter().any(|k| {
                    self.boxes[k.index()].level == Level::Block
                        && !self.boxes[k.index()].is_out_of_flow()
                });
                if !has_block {
                    self.boxes[id.index()].children = kids;
                    return vec![id];
                }
                let mut out = Vec::new();
                let mut piece = id;
                let mut piece_kids = Vec::new();
                let mut first = true;
                for k in kids {
                    let kb = &self.boxes[k.index()];
                    if kb.level == Level::Block && !kb.is_out_of_flow() {
                        self.boxes[piece.index()].children = std::mem::take(&mut piece_kids);
                        self.boxes[piece.index()].split_first = first;
                        self.boxes[piece.index()].split_last = false;
                        out.push(piece);
                        out.push(k);
                        first = false;
                        piece = self.push(
                            BoxKind::Inline,
                            style.clone(),
                            source,
                            Some(node),
                            Level::Inline,
                        );
                    } else {
                        piece_kids.push(k);
                    }
                }
                self.boxes[piece.index()].children = piece_kids;
                self.boxes[piece.index()].split_first = false;
                self.boxes[piece.index()].split_last = true;
                out.push(piece);
                out
            }
            _ => {
                // Block containers: block, inline-block, list-item, flow-root, flex and
                // grid (laid out as block containers in M1).
                let kind = if display == Display::InlineBlock || (display.is_inline_level()) {
                    BoxKind::InlineBlock
                } else {
                    BoxKind::Block
                };
                let id = self.push(kind, style.clone(), source, Some(node), level);
                self.by_node.insert(node, id);
                if tag == "button" {
                    self.boxes[id.index()].control = Some(ControlKind::Button);
                }
                if display == Display::ListItem {
                    self.list_item_marker(id, node, &style, depth);
                }
                self.build_container_contents(id, node, &style, depth);
                self.counters_leave(depth);
                vec![id]
            }
        }
    }

    /// Fills a block container with its pseudo-elements and children, then wraps
    /// inline runs in anonymous blocks where needed.
    fn build_container_contents(
        &mut self,
        id: BoxId,
        node: NodeId,
        style: &Rc<ComputedStyle>,
        depth: u32,
    ) {
        let mut kids = Vec::new();
        if let Some(m) = self.boxes[id.index()].marker {
            // Inside markers are the first inline child.
            if matches!(self.boxes[m.index()].kind, BoxKind::Inline) {
                kids.push(m);
                self.boxes[id.index()].marker = None;
            }
        }
        if let Some(b) = self.build_pseudo(node, style, true, depth) {
            kids.push(b);
        }
        kids.extend(self.build_children(node, style, depth));
        if let Some(b) = self.build_pseudo(node, style, false, depth) {
            kids.push(b);
        }
        if matches!(style.display, Display::Flex | Display::InlineFlex) {
            let items = crate::layout::flex::wrap_flex_items(&mut self.boxes, id, kids);
            self.boxes[id.index()].children = items;
            self.boxes[id.index()].inline_children = false;
            return;
        }
        if matches!(style.display, Display::Grid | Display::InlineGrid) {
            let items = crate::layout::grid::wrap_grid_items(&mut self.boxes, id, kids);
            self.boxes[id.index()].children = items;
            self.boxes[id.index()].inline_children = false;
            return;
        }
        self.make_container(id, kids);
    }

    fn build_children(
        &mut self,
        node: NodeId,
        style: &Rc<ComputedStyle>,
        depth: u32,
    ) -> Vec<BoxId> {
        let mut out = Vec::new();
        let kids: Vec<NodeId> = self.doc.children(node).collect();
        for c in kids {
            match self.doc.kind(c) {
                NodeKind::Element { .. } => out.extend(self.build_element(c, depth + 1)),
                NodeKind::Text(t) => {
                    if t.is_empty() {
                        continue;
                    }
                    let id = self.push(
                        BoxKind::Text(TextBox {
                            text: t.clone(),
                            node: Some(c),
                        }),
                        style.clone(),
                        StyleSource::Element(node),
                        None,
                        Level::Inline,
                    );
                    out.push(id);
                }
                _ => {}
            }
        }
        out
    }

    /// Anonymous block wrapping (§9.2.1.1) and anonymous table wrapping (§17.2.1 rule 3)
    /// for the children of a block container.
    fn make_container(&mut self, id: BoxId, kids: Vec<BoxId>) {
        let kids = self.wrap_stray_table_parts(id, kids);
        let parent_style = self.boxes[id.index()].style.clone();
        let has_block = kids.iter().any(|k| {
            let b = &self.boxes[k.index()];
            b.level == Level::Block && !b.is_out_of_flow()
        });
        if !has_block {
            self.boxes[id.index()].children = kids;
            self.boxes[id.index()].inline_children = true;
            return;
        }
        let mut out = Vec::new();
        let mut run: Vec<BoxId> = Vec::new();
        let flush = |this: &mut Self, run: &mut Vec<BoxId>, out: &mut Vec<BoxId>| {
            if run.is_empty() {
                return;
            }
            let has_content = run.iter().any(|k| {
                let b = &this.boxes[k.index()];
                match &b.kind {
                    BoxKind::Text(t) => {
                        !text::is_collapsible_whitespace(&t.text, b.style.white_space)
                    }
                    _ => !b.is_out_of_flow(),
                }
            });
            if has_content {
                let anon = this.push(
                    BoxKind::Block,
                    anon_style(&parent_style, Display::Block),
                    StyleSource::Anonymous(this.boxes[id.index()].source.node()),
                    None,
                    Level::Block,
                );
                this.boxes[anon.index()].children = std::mem::take(run);
                this.boxes[anon.index()].inline_children = true;
                out.push(anon);
            } else {
                // Only white space and out-of-flow boxes: the floats and absolutes
                // become direct children, the white space is dropped.
                for k in run.drain(..) {
                    if !matches!(this.boxes[k.index()].kind, BoxKind::Text(_)) {
                        out.push(k);
                    }
                }
            }
        };
        for k in kids {
            let b = &self.boxes[k.index()];
            if b.level == Level::Block && !b.is_out_of_flow() {
                flush(self, &mut run, &mut out);
                out.push(k);
            } else {
                run.push(k);
            }
        }
        flush(self, &mut run, &mut out);
        self.boxes[id.index()].children = out;
        self.boxes[id.index()].inline_children = false;
    }

    /// Rule 3 of §17.2.1: table-internal boxes outside a table get an anonymous table.
    fn wrap_stray_table_parts(&mut self, parent: BoxId, kids: Vec<BoxId>) -> Vec<BoxId> {
        if !kids
            .iter()
            .any(|k| self.boxes[k.index()].is_table_internal())
        {
            return kids;
        }
        let parent_style = self.boxes[parent.index()].style.clone();
        let mut out = Vec::new();
        let mut run: Vec<BoxId> = Vec::new();
        let node = self.boxes[parent.index()].source.node();
        let flush = |this: &mut Self, run: &mut Vec<BoxId>, out: &mut Vec<BoxId>| {
            if run.is_empty() {
                return;
            }
            let tstyle = anon_style(&parent_style, Display::Table);
            let items = this.make_table(
                node,
                StyleSource::Anonymous(node),
                None,
                tstyle,
                std::mem::take(run),
            );
            out.extend(items);
        };
        for k in kids {
            let b = &self.boxes[k.index()];
            if b.is_table_internal() {
                run.push(k);
            } else if !run.is_empty()
                && matches!(&b.kind, BoxKind::Text(t) if text::is_collapsible_whitespace(&t.text, b.style.white_space))
            {
                // White space between table parts is dropped.
            } else {
                flush(self, &mut run, &mut out);
                out.push(k);
            }
        }
        flush(self, &mut run, &mut out);
        out
    }

    fn build_table(&mut self, node: NodeId, style: &Rc<ComputedStyle>, depth: u32) -> Vec<BoxId> {
        let mut kids = Vec::new();
        if let Some(b) = self.build_pseudo(node, style, true, depth) {
            kids.push(b);
        }
        kids.extend(self.build_children(node, style, depth));
        if let Some(b) = self.build_pseudo(node, style, false, depth) {
            kids.push(b);
        }
        self.make_table(
            node,
            StyleSource::Element(node),
            Some(node),
            style.clone(),
            kids,
        )
    }

    /// Builds the wrapper and grid boxes for a table with these children, applying the
    /// missing-child rules (§17.2.1 rules 1 and 2).
    fn make_table(
        &mut self,
        node: NodeId,
        source: StyleSource,
        elem: Option<NodeId>,
        style: Rc<ComputedStyle>,
        kids: Vec<BoxId>,
    ) -> Vec<BoxId> {
        let level = Self::level_of(style.display);
        // The wrapper takes the margins, float, position and clear; the grid box the rest.
        let mut wrapper_style = ComputedStyle::inherit_from(&style);
        wrapper_style.display = if level == Level::Inline {
            Display::InlineBlock
        } else {
            Display::Block
        };
        wrapper_style.position = style.position;
        wrapper_style.float = style.float;
        wrapper_style.clear = style.clear;
        wrapper_style.margin = style.margin;
        wrapper_style.inset = style.inset;
        wrapper_style.z_index = style.z_index;
        wrapper_style.opacity = style.opacity;
        wrapper_style.transform = style.transform.clone();
        let mut grid_style = (*style).clone();
        grid_style.margin =
            crate::style::Sides::uniform(LengthPercentageAuto::Set(LengthPercentage::ZERO));
        grid_style.position = Position::Static;
        grid_style.float = Float::None;
        grid_style.clear = Clear::None;
        grid_style.z_index = crate::style::ZIndex::Auto;
        grid_style.opacity = 255;
        grid_style.transform = Vec::new();
        let anon = StyleSource::Anonymous(node);
        let wrapper = self.push(
            BoxKind::TableWrapper,
            Rc::new(wrapper_style),
            anon,
            elem,
            level,
        );
        let grid = self.push(
            BoxKind::Table,
            Rc::new(grid_style),
            source,
            elem,
            Level::Block,
        );
        if let Some(n) = elem {
            self.by_node.insert(n, grid);
        }
        let style_rc = self.boxes[grid.index()].style.clone();

        let mut captions_top = Vec::new();
        let mut captions_bottom = Vec::new();
        let mut cols = Vec::new();
        let mut groups: Vec<BoxId> = Vec::new();
        let mut header = Vec::new();
        let mut footer = Vec::new();
        let mut stray: Vec<BoxId> = Vec::new();
        let flush_stray = |this: &mut Self, stray: &mut Vec<BoxId>, groups: &mut Vec<BoxId>| {
            if stray.is_empty() {
                return;
            }
            let rows = this.fixup_row_group_children(&style_rc, std::mem::take(stray));
            let g = this.push(
                BoxKind::RowGroup,
                anon_style(&style_rc, Display::TableRowGroup),
                anon,
                None,
                Level::Block,
            );
            this.boxes[g.index()].children = rows;
            groups.push(g);
        };
        for k in kids {
            let b = &self.boxes[k.index()];
            let disp = b.style.display;
            match &b.kind {
                BoxKind::Caption => {
                    if b.style.caption_side == crate::style::CaptionSide::Bottom {
                        captions_bottom.push(k)
                    } else {
                        captions_top.push(k)
                    }
                }
                BoxKind::ColGroup(_) | BoxKind::Col(_) => cols.push(k),
                BoxKind::RowGroup => {
                    flush_stray(self, &mut stray, &mut groups);
                    match disp {
                        Display::TableHeaderGroup if header.is_empty() => header.push(k),
                        Display::TableFooterGroup if footer.is_empty() => footer.push(k),
                        _ => groups.push(k),
                    }
                }
                BoxKind::Text(t)
                    if text::is_collapsible_whitespace(&t.text, b.style.white_space) => {}
                _ => stray.push(k),
            }
        }
        flush_stray(self, &mut stray, &mut groups);
        let mut grid_kids = cols;
        grid_kids.extend(header);
        grid_kids.extend(groups);
        grid_kids.extend(footer);
        self.boxes[grid.index()].children = grid_kids;
        let mut wkids = captions_top;
        wkids.push(grid);
        wkids.extend(captions_bottom);
        self.boxes[wrapper.index()].children = wkids;
        vec![wrapper]
    }

    /// Children of a row group: rows; anything else is wrapped in anonymous rows.
    fn fixup_row_group_children(
        &mut self,
        style: &Rc<ComputedStyle>,
        kids: Vec<BoxId>,
    ) -> Vec<BoxId> {
        let mut out = Vec::new();
        let mut stray: Vec<BoxId> = Vec::new();
        let anon = StyleSource::Anonymous(match self.boxes.last() {
            Some(b) => b.source.node(),
            None => NodeId(0),
        });
        let flush = |this: &mut Self, stray: &mut Vec<BoxId>, out: &mut Vec<BoxId>| {
            if stray.is_empty() {
                return;
            }
            let cells = this.fixup_row_children(style, std::mem::take(stray));
            let r = this.push(
                BoxKind::Row,
                anon_style(style, Display::TableRow),
                anon,
                None,
                Level::Block,
            );
            this.boxes[r.index()].children = cells;
            out.push(r);
        };
        for k in kids {
            let b = &self.boxes[k.index()];
            match &b.kind {
                BoxKind::Row => {
                    flush(self, &mut stray, &mut out);
                    out.push(k);
                }
                BoxKind::Text(t)
                    if text::is_collapsible_whitespace(&t.text, b.style.white_space) => {}
                BoxKind::ColGroup(_) | BoxKind::Col(_) | BoxKind::Caption => {}
                _ => stray.push(k),
            }
        }
        flush(self, &mut stray, &mut out);
        out
    }

    /// Children of a row: cells; consecutive other boxes get an anonymous cell.
    fn fixup_row_children(&mut self, style: &Rc<ComputedStyle>, kids: Vec<BoxId>) -> Vec<BoxId> {
        let mut out = Vec::new();
        let mut stray: Vec<BoxId> = Vec::new();
        let anon = StyleSource::Anonymous(match self.boxes.last() {
            Some(b) => b.source.node(),
            None => NodeId(0),
        });
        let flush = |this: &mut Self, stray: &mut Vec<BoxId>, out: &mut Vec<BoxId>| {
            if stray.is_empty() {
                return;
            }
            let c = this.push(
                BoxKind::Cell(CellBox {
                    colspan: 1,
                    rowspan: 1,
                }),
                anon_style(style, Display::TableCell),
                anon,
                None,
                Level::Block,
            );
            let run = std::mem::take(stray);
            this.make_container(c, run);
            out.push(c);
        };
        for k in kids {
            let b = &self.boxes[k.index()];
            match &b.kind {
                BoxKind::Cell(_) => {
                    flush(self, &mut stray, &mut out);
                    out.push(k);
                }
                BoxKind::Text(t)
                    if text::is_collapsible_whitespace(&t.text, b.style.white_space) => {}
                BoxKind::ColGroup(_) | BoxKind::Col(_) | BoxKind::Caption => {}
                BoxKind::Row | BoxKind::RowGroup => {
                    // A row inside a row: its cells join this row.
                    let inner = self.boxes[k.index()].children.clone();
                    for c in inner {
                        if matches!(self.boxes[c.index()].kind, BoxKind::Cell(_)) {
                            flush(self, &mut stray, &mut out);
                            out.push(c);
                        } else {
                            stray.push(c);
                        }
                    }
                }
                _ => stray.push(k),
            }
        }
        flush(self, &mut stray, &mut out);
        out
    }

    /// `::before` (`before == true`) or `::after` content.
    fn build_pseudo(
        &mut self,
        node: NodeId,
        parent: &Rc<ComputedStyle>,
        before: bool,
        depth: u32,
    ) -> Option<BoxId> {
        let pstyle = if before {
            self.styles.before(node)
        } else {
            self.styles.after(node)
        }?;
        let items = match &pstyle.content {
            Content::Normal | Content::None => return None,
            Content::Items(items) => items.clone(),
        };
        if pstyle.display.is_none() {
            return None;
        }
        let pstyle = Rc::new(pstyle.clone());
        let source = if before {
            StyleSource::Before(node)
        } else {
            StyleSource::After(node)
        };
        self.apply_counter_props(&pstyle, depth + 1);
        let mut display = pstyle.display;
        if pstyle.is_out_of_flow() {
            display = display.blockify();
        }
        if matches!(
            parent.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
        ) {
            display = crate::layout::flex::blockify_item_display(display);
        }
        let level = Self::level_of(display);
        let kind = match display {
            Display::Inline => BoxKind::Inline,
            Display::InlineBlock => BoxKind::InlineBlock,
            _ => BoxKind::Block,
        };
        let is_inline = kind == BoxKind::Inline;
        let id = self.push(kind, pstyle.clone(), source, None, level);
        let mut kids = Vec::new();
        let mut buf = String::new();
        for it in items {
            match it {
                ContentItem::Text(t) => buf.push_str(&t),
                ContentItem::Attr(a) => buf.push_str(self.doc.attr(node, &a).unwrap_or("")),
                ContentItem::Counter(name, st) => {
                    buf.push_str(&text::counter_text(self.counter_value(&name), st))
                }
                ContentItem::OpenQuote => {
                    let q = &pstyle.quotes;
                    if !q.is_empty() {
                        let i = (self.quote_depth.max(0) as usize).min(q.len() - 1);
                        buf.push_str(&q[i].0);
                    }
                    self.quote_depth += 1;
                }
                ContentItem::CloseQuote => {
                    self.quote_depth = (self.quote_depth - 1).max(0);
                    let q = &pstyle.quotes;
                    if !q.is_empty() {
                        let i = (self.quote_depth.max(0) as usize).min(q.len() - 1);
                        buf.push_str(&q[i].1);
                    }
                }
                ContentItem::Url(u) => {
                    if !buf.is_empty() {
                        let t = self.push(
                            BoxKind::Text(TextBox {
                                text: std::mem::take(&mut buf),
                                node: None,
                            }),
                            pstyle.clone(),
                            source,
                            None,
                            Level::Inline,
                        );
                        kids.push(t);
                    }
                    let intrinsic = self.images.size(&u).map(|(w, h)| Size {
                        width: Au::from_px_i32(w as i32),
                        height: Au::from_px_i32(h as i32),
                    });
                    let rb = ReplacedBox {
                        replaced: Replaced::Image {
                            src: u,
                            alt: String::new(),
                        },
                        intrinsic,
                        attr_width: None,
                        attr_height: None,
                    };
                    let mut s = ComputedStyle::inherit_from(&pstyle);
                    s.display = Display::Inline;
                    let r = self.push(
                        BoxKind::Replaced(rb),
                        Rc::new(s),
                        source,
                        None,
                        Level::Inline,
                    );
                    kids.push(r);
                }
            }
        }
        if !buf.is_empty() {
            let t = self.push(
                BoxKind::Text(TextBox {
                    text: buf,
                    node: None,
                }),
                pstyle.clone(),
                source,
                None,
                Level::Inline,
            );
            kids.push(t);
        }
        if is_inline {
            self.boxes[id.index()].children = kids;
        } else {
            self.make_container(id, kids);
        }
        let _ = parent;
        Some(id)
    }

    fn list_item_marker(&mut self, id: BoxId, node: NodeId, style: &Rc<ComputedStyle>, depth: u32) {
        // The HTML list counter: <ol>/<ul> reset it, every list item increments it.
        let tag = self.doc.tag(node).unwrap_or("");
        if tag == "li" {
            if let Some(v) = self
                .doc
                .attr(node, "value")
                .and_then(|v| v.trim().parse::<i32>().ok())
            {
                self.counter_set("list-item", v, depth);
            } else {
                self.counter_increment("list-item", self.list_direction(node), depth);
            }
        } else {
            self.counter_increment("list-item", 1, depth);
        }
        let n = self.counter_value("list-item");
        if style.list_style_type == ListStyleType::None {
            return;
        }
        let mstyle = match self.styles.marker(node) {
            Some(m) => Rc::new(m.clone()),
            None => {
                let mut s = ComputedStyle::inherit_from(style);
                s.display = Display::Inline;
                s.white_space = WhiteSpace::Pre;
                Rc::new(s)
            }
        };
        let txt = text::marker_text(n, style.list_style_type);
        let outside = style.list_style_position == ListStylePosition::Outside;
        let m = if outside {
            self.push(
                BoxKind::Marker(txt),
                mstyle,
                StyleSource::Marker(node),
                None,
                Level::Block,
            )
        } else {
            let m = self.push(
                BoxKind::Inline,
                mstyle.clone(),
                StyleSource::Marker(node),
                None,
                Level::Inline,
            );
            let t = self.push(
                BoxKind::Text(TextBox {
                    text: txt,
                    node: None,
                }),
                mstyle,
                StyleSource::Marker(node),
                None,
                Level::Inline,
            );
            self.boxes[m.index()].children = vec![t];
            m
        };
        self.boxes[id.index()].marker = Some(m);
    }

    /// +1, or -1 inside a reversed `<ol>`.
    fn list_direction(&self, li: NodeId) -> i32 {
        match self.doc.parent(li) {
            Some(p) if self.doc.is(p, "ol") && self.doc.has_attr(p, "reversed") => -1,
            _ => 1,
        }
    }

    /// Replaced elements and form controls with intrinsic sizes (§10.3.2, HTML
    /// rendering section).
    fn replaced_box(
        &mut self,
        node: NodeId,
        tag: &str,
        style: &ComputedStyle,
    ) -> Option<ReplacedBox> {
        let doc = self.doc;
        let px = |w: u32, h: u32| {
            Some(Size {
                width: Au::from_px_i32(w as i32),
                height: Au::from_px_i32(h as i32),
            })
        };
        let attr_w = doc.attr(node, "width").and_then(Dim::parse);
        let attr_h = doc.attr(node, "height").and_then(Dim::parse);
        let font = &style.font;
        let lh = style.line_height_au(text::font_metrics(font).normal_line_height());
        let ch = text::ch_unit(font);
        match tag {
            "img" => {
                let src = doc.attr(node, "src").unwrap_or("").to_owned();
                let alt = doc.attr(node, "alt").unwrap_or("").to_owned();
                let intrinsic = self.images.size(&src).map(|(w, h)| Size {
                    width: Au::from_px_i32(w as i32),
                    height: Au::from_px_i32(h as i32),
                });
                Some(ReplacedBox {
                    replaced: Replaced::Image { src, alt },
                    intrinsic,
                    attr_width: attr_w,
                    attr_height: attr_h,
                })
            }
            "input" => {
                let ty = doc
                    .attr(node, "type")
                    .map(|t| t.to_ascii_lowercase())
                    .unwrap_or_default();
                let (kind, intrinsic) = match ty.as_str() {
                    "checkbox" => (ControlKind::Checkbox, px(13, 13)),
                    "radio" => (ControlKind::Radio, px(13, 13)),
                    "submit" | "button" | "reset" | "image" => {
                        let label =
                            doc.attr(node, "value")
                                .map(str::to_owned)
                                .unwrap_or_else(|| match ty.as_str() {
                                    "submit" => "Submit".into(),
                                    "reset" => "Reset".into(),
                                    _ => String::new(),
                                });
                        let w =
                            text::measure(font, &label, style.letter_spacing, style.word_spacing);
                        (
                            if ty == "submit" {
                                ControlKind::Submit
                            } else {
                                ControlKind::Button
                            },
                            Some(Size {
                                width: w,
                                height: lh,
                            }),
                        )
                    }
                    "range" => (ControlKind::Range, px(129, 20)),
                    "color" => (ControlKind::Color, px(50, 27)),
                    "file" => (
                        ControlKind::File,
                        Some(Size {
                            width: ch * 20 + Au::from_px_i32(80),
                            height: lh,
                        }),
                    ),
                    "password" => {
                        let size = attr_u32(doc, node, "size", 20).clamp(1, 1000) as i32;
                        (
                            ControlKind::Password,
                            Some(Size {
                                width: text_control_width(font, size),
                                height: lh,
                            }),
                        )
                    }
                    _ => {
                        let size = attr_u32(doc, node, "size", 20).clamp(1, 1000) as i32;
                        (
                            ControlKind::TextInput,
                            Some(Size {
                                width: text_control_width(font, size),
                                height: lh,
                            }),
                        )
                    }
                };
                Some(ReplacedBox {
                    replaced: Replaced::Control(kind),
                    intrinsic,
                    attr_width: None,
                    attr_height: None,
                })
            }
            "select" => Some(ReplacedBox {
                replaced: Replaced::Control(ControlKind::Select),
                intrinsic: Some(Size {
                    width: ch * 20,
                    height: lh,
                }),
                attr_width: None,
                attr_height: None,
            }),
            "textarea" => {
                let cols = attr_u32(doc, node, "cols", 20).clamp(1, 1000) as i32;
                let rows = attr_u32(doc, node, "rows", 2).clamp(1, 10_000) as i32;
                Some(ReplacedBox {
                    replaced: Replaced::Control(ControlKind::TextArea),
                    intrinsic: Some(Size {
                        width: ch * cols,
                        height: lh * rows,
                    }),
                    attr_width: None,
                    attr_height: None,
                })
            }
            "object" | "embed" => {
                // An `<object>` whose `data` is an image the host decoded renders it
                // (HTML §4.8.7, the image case); one whose data cannot be loaded (an
                // unknown type, a failed fetch, a document the engine does not nest)
                // renders its fallback content instead, as an ordinary non-replaced
                // element of its `display`. `<embed>` has no fallback content, so an
                // unloaded one stays a placeholder.
                let src = doc
                    .attr(node, if tag == "object" { "data" } else { "src" })
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                match self.images.size(&src) {
                    Some((w, h)) => Some(ReplacedBox {
                        replaced: Replaced::Image {
                            src,
                            alt: String::new(),
                        },
                        intrinsic: px(w, h),
                        attr_width: attr_w,
                        attr_height: attr_h,
                    }),
                    None if tag == "object" => None,
                    None => Some(ReplacedBox {
                        replaced: Replaced::Placeholder(tag.to_owned()),
                        intrinsic: px(300, 150),
                        attr_width: attr_w,
                        attr_height: attr_h,
                    }),
                }
            }
            "svg" if crate::svg::is_svg(doc, node) => {
                // `width` and `height` reach the box as presentational hints (CSS
                // `width`/`height`); the natural size is theirs in px, with the
                // `viewBox` ratio filling in a missing one, else 300 × 150.
                let (w, h, ratio) = crate::svg::natural_size(doc, node);
                let (w, h) = match (w, h, ratio) {
                    (Some(w), Some(h), _) => (w, h),
                    (Some(w), None, Some(r)) => (w, w / r),
                    (None, Some(h), Some(r)) => (h * r, h),
                    (None, None, Some(r)) => (300.0, 300.0 / r),
                    (w, h, _) => (w.unwrap_or(300.0), h.unwrap_or(150.0)),
                };
                Some(ReplacedBox {
                    replaced: Replaced::Placeholder(tag.to_owned()),
                    intrinsic: Some(Size {
                        width: Au((w * 64.0).round() as i32),
                        height: Au((h * 64.0).round() as i32),
                    }),
                    attr_width: None,
                    attr_height: None,
                })
            }
            "iframe" | "canvas" | "video" | "svg" | "frame" => Some(ReplacedBox {
                replaced: Replaced::Placeholder(tag.to_owned()),
                intrinsic: px(300, 150),
                attr_width: attr_w,
                attr_height: attr_h,
            }),
            "audio" => {
                if doc.has_attr(node, "controls") {
                    Some(ReplacedBox {
                        replaced: Replaced::Placeholder(tag.to_owned()),
                        intrinsic: px(300, 54),
                        attr_width: attr_w,
                        attr_height: attr_h,
                    })
                } else {
                    Some(ReplacedBox {
                        replaced: Replaced::Placeholder(tag.to_owned()),
                        intrinsic: px(0, 0),
                        attr_width: None,
                        attr_height: None,
                    })
                }
            }
            "progress" | "meter" => Some(ReplacedBox {
                replaced: Replaced::Placeholder(tag.to_owned()),
                intrinsic: Some(Size {
                    width: Au::from_px_i32(160),
                    height: lh,
                }),
                attr_width: None,
                attr_height: None,
            }),
            _ => None,
        }
    }
}

/// The intrinsic content width of a single-line text control with `size` columns,
/// the way Blink sizes one (`LayoutTextControlSingleLine::PreferredContentLogicalWidth`):
/// `ceil(ceil(size * avg) + max - avg)` in whole pixels, where `avg` is
/// the face's OS/2 `xAvgCharWidth` and `max` its `head` bounding-box width
/// (`xMax - xMin`). The table holds the values of the faces a Linux Chromium shapes
/// with (Liberation for the Croscore stand-ins, read with fontTools). Faces without
/// an entry fall back to `size` advances of `0`.
pub fn text_control_width(font: &crate::style::Font, size: i32) -> Au {
    use cw_scene::Typeface;
    // `(units per em, xAvgCharWidth, xMax - xMin)`.
    let units: Option<(i64, i64, i64)> = match font.typeface {
        Typeface::Arimo => Some((2048, 1187, 3780)),
        Typeface::Tinos => Some((2048, 1137, 4013)),
        Typeface::Cousine => Some((2048, 1229, 2508)),
        Typeface::DejaVu => Some((2048, 1038, 5532)),
        Typeface::Mono => Some((2048, 1233, 2614)),
        _ => None,
    };
    let Some((upem, avg_units, max_units)) = units else {
        return text::ch_unit(font) * size;
    };
    // Everything in Au (1/64 px) as i64, truncated like FreeType's 26.6 metrics
    // that Skia hands Blink (so 55 columns of 16px Arial are 510px, not 511).
    let fs = font.size.0 as i64;
    let avg = (fs * avg_units).div_euclid(upem);
    let max = (fs * max_units).div_euclid(upem);
    let columns = (avg * size as i64 + 63).div_euclid(64);
    let total = columns * 64 + max - avg;
    Au::from_px_i32(((total + 63).div_euclid(64)).clamp(0, 100_000) as i32)
}

fn attr_u32(doc: &Document, node: NodeId, name: &str, default: u32) -> u32 {
    doc.attr(node, name)
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dims_parse() {
        assert_eq!(Dim::parse("50%"), Some(Dim::Percent(5000)));
        assert_eq!(Dim::parse("120"), Some(Dim::Px(Au::from_px_i32(120))));
        assert_eq!(Dim::parse("12.5px"), Some(Dim::Px(Au(800))));
        assert_eq!(Dim::parse("abc"), None);
        assert_eq!(Dim::parse(" 3 "), Some(Dim::Px(Au::from_px_i32(3))));
    }
}
