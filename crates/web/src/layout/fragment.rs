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
            StyleSource::Element(n) | StyleSource::Before(n) | StyleSource::After(n) | StyleSource::Marker(n) | StyleSource::Anonymous(n) => n,
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

#[derive(Clone, Debug, PartialEq, Eq)]
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
    pub scroll_x: Au,
    pub scroll_y: Au,
    pub shows_x_bar: bool,
    pub shows_y_bar: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    pub kind: FragmentKind,
    /// Border-box rect relative to the parent fragment's border-box origin. For
    /// positioned fragments this already includes the positioning offset.
    pub rect: Rect,
    pub children: Vec<Fragment>,
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
}

impl Fragment {
    pub fn new(kind: FragmentKind, rect: Rect) -> Fragment {
        Fragment {
            kind,
            rect,
            children: Vec::new(),
            establishes_stacking_context: false,
            z_index: 0,
            is_float: false,
            is_positioned: false,
            overflow: Rect::new(Au::ZERO, Au::ZERO, rect.size.width, rect.size.height),
            collapsed_borders: None,
        }
    }
    pub fn source(&self) -> Option<StyleSource> {
        match &self.kind {
            FragmentKind::Box { source, .. } | FragmentKind::InlineBox { source, .. } | FragmentKind::Text { source, .. } => Some(*source),
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
}

impl FragmentTree {
    /// Every fragment whose absolute rect contains the point, innermost last.
    pub fn hit(&self, x: Au, y: Au) -> Vec<(&Fragment, Rect)> {
        let mut out = Vec::new();
        self.root.walk(crate::geom::Point::default(), &mut |f, r| {
            if r.contains(x, y) {
                out.push((f, r));
            }
        });
        out
    }
    /// Absolute border-box rects of every fragment of the element, in order.
    pub fn rects_of(&self, node: NodeId) -> Vec<Rect> {
        let mut out = Vec::new();
        self.root.walk(crate::geom::Point::default(), &mut |f, r| {
            if let Some(s) = f.source() {
                if !s.is_anonymous() && s.node() == node && matches!(f.kind, FragmentKind::Box { .. } | FragmentKind::InlineBox { .. }) {
                    out.push(r);
                }
            }
        });
        out
    }
}
