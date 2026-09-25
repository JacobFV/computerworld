//! The fragment tree: what layout produces and paint consumes. Positions are relative
//! to the parent fragment's border box origin, in `Au`. Every fragment that came from
//! an element carries its `NodeId`, so paint can look up its `ComputedStyle` and the
//! DOM's semantics, and hit testing can map a point back to an element.

use crate::dom::NodeId;
use crate::geom::{Au, Edges, Rect};
use crate::style::BorderSide;

/// Borders resolved by the collapsing border model (§17.6.2) for a table cell or the
/// table grid box. Paint draws these full widths centred on the fragment's border-box
/// edges instead of the style's own borders; the fragment's `border` edges hold the
/// half widths that layout reserved inside the box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollapsedBorders {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
}

/// Which style a fragment paints with: the element's own, or one of its pseudo-elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StyleSource {
    Element(NodeId),
    Before(NodeId),
    After(NodeId),
    Marker(NodeId),
    /// Anonymous boxes (table wrappers, anonymous block/inline boxes) inherit from
    /// this element and paint no background or border.
    Anonymous(NodeId),
}

impl StyleSource {
    pub fn node(self) -> NodeId {
        match self {
            StyleSource::Element(n)
            | StyleSource::Before(n)
            | StyleSource::After(n)
            | StyleSource::Marker(n)
            | StyleSource::Anonymous(n) => n,
        }
    }
    pub fn is_anonymous(self) -> bool {
        matches!(self, StyleSource::Anonymous(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FragmentKind {
    /// A box: block, inline-block, table part, flex/grid container or item,
    /// list marker box, replaced element's box. `rect` is the border box.
    Box {
        source: StyleSource,
        /// Padding edges, so paint can find the padding and content boxes.
        padding: Edges,
        border: Edges,
        /// Set for replaced content: what to draw inside the content box.
        replaced: Option<Replaced>,
        /// The box establishes a scroll container; content size for scrolling.
        scroll: Option<ScrollInfo>,
        /// Baseline of the first line, from the top of the border box, if any.
        baseline: Option<Au>,
    },
    /// An inline box's piece on one line: paints its background, borders (with the
    /// open/closed ends) and contains the text and nested inline fragments.
    InlineBox {
        source: StyleSource,
        padding: Edges,
        border: Edges,
        /// Whether this piece is the first/last on its line (for border ends).
        first: bool,
        last: bool,
    },
    /// A run of shaped text on one line, one font, one style. `rect` is the run's
    /// box (advance width by line height contribution); baseline offset from its top.
    Text {
        source: StyleSource,
        text: String,
        /// The text node it came from, for selection and hit testing; None for
        /// generated content.
        node: Option<NodeId>,
        /// Byte range in the text node's data that this run covers.
        range: (usize, usize),
        baseline: Au,
        /// Set when the run was cut for `text-overflow: ellipsis`.
        ellipsis: bool,
    },
    /// A line box, for debugging and hit testing between runs. `rect` spans the line.
    Line,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Replaced {
    /// An `<img>`; `src` resolved; intrinsic size when known.
    Image { src: String, alt: String },
    /// A form control, drawn by paint from its DOM state.
    Control(ControlKind),
    /// `<canvas>`, `<video>`, `<iframe>`, `<svg>`, `<object>`: a placeholder box.
    Placeholder(String),
    /// A list-item marker string (`•`, `1.`) drawn in the marker's font.
    Marker(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlKind {
    TextInput,
    Password,
    Checkbox,
    Radio,
    Button,
    Submit,
    Select,
    TextArea,
    Range,
    File,
    Color,
    Hidden,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollInfo {
    pub content_width: Au,
    pub content_height: Au,
    /// The offset the painter subtracts from the contents. It is zero when the box
    /// sits as laid out, which is not the start of the scrollable area for a box
    /// whose content runs off the start edge (see `origin_x`/`origin_y`).
    pub scroll_x: Au,
    pub scroll_y: Au,
    /// Where the scrollable area starts, relative to the padding box: zero unless
    /// the content overflows the start edge, as a reversed flex container's does.
    /// `scroll_* - origin_*` is the offset from the start, which is what `scrollTop`
    /// and `scrollLeft` measure.
    pub origin_x: Au,
    pub origin_y: Au,
    pub shows_x_bar: bool,
    pub shows_y_bar: bool,
}

/// A fragment's children, shared between copies of the fragment until one of them
/// changes its list (copy on write): the layout memo hands out copies of whole
/// laid-out subtrees, which would otherwise be copied fragment by fragment.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct FragmentList(std::rc::Rc<Vec<Fragment>>);

impl FragmentList {
    /// The children as a vector of their own (copied only when shared).
    pub fn into_vec(self) -> Vec<Fragment> {
        std::rc::Rc::try_unwrap(self.0).unwrap_or_else(|rc| (*rc).clone())
    }
}

impl std::ops::Deref for FragmentList {
    type Target = Vec<Fragment>;
    fn deref(&self) -> &Vec<Fragment> {
        &self.0
    }
}

impl std::ops::DerefMut for FragmentList {
    fn deref_mut(&mut self) -> &mut Vec<Fragment> {
        std::rc::Rc::make_mut(&mut self.0)
    }
}

impl From<Vec<Fragment>> for FragmentList {
    fn from(v: Vec<Fragment>) -> FragmentList {
        FragmentList(std::rc::Rc::new(v))
    }
}

impl IntoIterator for FragmentList {
    type Item = Fragment;
    type IntoIter = std::vec::IntoIter<Fragment>;
    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a> IntoIterator for &'a FragmentList {
    type Item = &'a Fragment;
    type IntoIter = std::slice::Iter<'a, Fragment>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut FragmentList {
    type Item = &'a mut Fragment;
    type IntoIter = std::slice::IterMut<'a, Fragment>;
    fn into_iter(self) -> Self::IntoIter {
        std::rc::Rc::make_mut(&mut self.0).iter_mut()
    }
}

impl std::fmt::Debug for FragmentList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    pub kind: FragmentKind,
    /// Border-box rect relative to the parent fragment's border-box origin. For
    /// positioned fragments this already includes the positioning offset.
    pub rect: Rect,
    pub children: FragmentList,
    /// Painting order hints: fragments are painted in tree order except that paint
    /// sorts stacking contexts by `z_index` and paints floats and positioned boxes in
    /// the CSS 2.1 Appendix E order. Layout marks what it knows.
    pub establishes_stacking_context: bool,
    pub z_index: i32,
    pub is_float: bool,
    pub is_positioned: bool,
    /// The fragment's overflow rect (union of descendants), relative to its own
    /// origin, for `overflow: visible` painting and scroll extents.
    pub overflow: Rect,
    /// Set on table cells and the table grid box under `border-collapse: collapse`.
    pub collapsed_borders: Option<Box<CollapsedBorders>>,
    /// The box's used margins, `auto` resolved, when the layout mode that placed it
    /// records them (block boxes, flex items, grid items): what `getComputedStyle`
    /// reports for `margin-*` on a rendered box.
    pub used_margin: Option<crate::geom::Edges>,
    /// Laid out but not rendered: the lines after a `-webkit-line-clamp`. The
    /// fragment keeps its geometry (a `Range` still reports its rects, as in Blink)
    /// but takes no part in painting, hit testing or scrollable overflow.
    pub hidden_for_paint: bool,
    /// For a fixed box whose containing block is the viewport but which sits inside
    /// an element that establishes a stacking context (MUI's sidebar inside its
    /// `position: relative; z-index: 1` layout): that element, whose context it is
    /// painted and hit-tested in, though its fragment is the root's.
    pub stacking_parent: Option<crate::dom::NodeId>,
}

impl Fragment {
    pub fn new(kind: FragmentKind, rect: Rect) -> Fragment {
        Fragment {
            kind,
            rect,
            children: FragmentList::default(),
            establishes_stacking_context: false,
            z_index: 0,
            is_float: false,
            is_positioned: false,
            overflow: Rect::new(Au::ZERO, Au::ZERO, rect.size.width, rect.size.height),
            collapsed_borders: None,
            used_margin: None,
            hidden_for_paint: false,
            stacking_parent: None,
        }
    }
    pub fn source(&self) -> Option<StyleSource> {
        match &self.kind {
            FragmentKind::Box { source, .. }
            | FragmentKind::InlineBox { source, .. }
            | FragmentKind::Text { source, .. } => Some(*source),
            FragmentKind::Line => None,
        }
    }
    /// Walks the tree with absolute rects (relative to the root).
    pub fn walk<'a>(&'a self, origin: crate::geom::Point, f: &mut dyn FnMut(&'a Fragment, Rect)) {
        let abs = self.rect.translate(origin.x, origin.y);
        f(self, abs);
        for c in &self.children {
            c.walk(abs.origin, f);
        }
    }
}

/// The result of layout: the root fragment (the initial containing block, sized to
/// the viewport) and the document's scrollable size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentTree {
    pub root: Fragment,
    pub content_width: Au,
    pub content_height: Au,
    pub viewport_width: Au,
    pub viewport_height: Au,
    /// Where each node of the laid-out document sits in tree order, so painting
    /// order (and hit testing, which has no document) can sort a stacking layer.
    pub doc_order: DocOrder,
}

/// Each node's preorder index and the index of its last descendant, by `NodeId`,
/// for the document a tree was laid out from; empty for a hand-built tree.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct DocOrder(Vec<(u32, u32)>);

impl DocOrder {
    const NONE: (u32, u32) = (u32::MAX, u32::MAX);

    pub fn of(doc: &crate::dom::Document) -> DocOrder {
        let mut v = vec![Self::NONE; doc.len()];
        let order: Vec<NodeId> = doc.descendants(crate::dom::Document::ROOT).collect();
        for (i, n) in order.iter().enumerate() {
            if let Some(e) = v.get_mut(n.0 as usize) {
                *e = (i as u32, i as u32);
            }
        }
        // Reverse preorder meets every descendant before its ancestor.
        for n in order.iter().rev() {
            let end = v[n.0 as usize].1;
            if let Some(p) = doc.parent(*n) {
                let e = &mut v[p.0 as usize];
                e.1 = e.1.max(end);
            }
        }
        DocOrder(v)
    }

    /// `(start, end)` of a node, or `None` when it was not in the document.
    pub fn get(&self, n: NodeId) -> Option<(u32, u32)> {
        self.0
            .get(n.0 as usize)
            .copied()
            .filter(|e| *e != Self::NONE)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for DocOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DocOrder({} nodes)", self.0.len())
    }
}

impl FragmentTree {
    /// Every fragment whose absolute rect contains the point, innermost last.
    pub fn hit(&self, x: Au, y: Au) -> Vec<(&Fragment, Rect)> {
        fn visit<'a>(
            f: &'a Fragment,
            origin: crate::geom::Point,
            x: Au,
            y: Au,
            out: &mut Vec<(&'a Fragment, Rect)>,
        ) {
            if f.hidden_for_paint {
                return;
            }
            let abs = f.rect.translate(origin.x, origin.y);
            if abs.contains(x, y) {
                out.push((f, abs));
            }
            for c in &f.children {
                visit(c, abs.origin, x, y, out);
            }
        }
        let mut out = Vec::new();
        visit(&self.root, crate::geom::Point::default(), x, y, &mut out);
        out
    }
    /// Absolute border-box rects of every fragment of the element, in order.
    pub fn rects_of(&self, node: NodeId) -> Vec<Rect> {
        let mut out = Vec::new();
        self.root.walk(crate::geom::Point::default(), &mut |f, r| {
            if let Some(s) = f.source() {
                if !s.is_anonymous()
                    && s.node() == node
                    && matches!(
                        f.kind,
                        FragmentKind::Box { .. } | FragmentKind::InlineBox { .. }
                    )
                {
                    out.push(r);
                }
            }
        });
        out
    }
}
