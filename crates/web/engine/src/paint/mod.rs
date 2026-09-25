//! Paint: the fragment tree to a `cw_scene::Scene`.
//!
//! The entry point is [`paint`]. It walks the fragment tree in the CSS 2.1 Appendix E
//! painting order ([`display_list`]), and for every fragment draws its backgrounds
//! ([`background`]), borders, outline and shadows ([`border`]), text runs and
//! decorations ([`text`]) and replaced content ([`replaced`]); the DOM supplies the
//! accessibility layer ([`semantics`]) and [`hit`] answers `elementFromPoint` over the
//! same ordering.
//!
//! Scene node ids are a pure function of the DOM node, the ordinal of the fragment
//! among that node's fragments in tree order, and a part number, so two paints of the
//! same tree produce the same ids and a paint of a slightly changed tree keeps the ids
//! of everything that did not move: `Scene::diff` then reports real changes only.
//!
//! Everything is integer arithmetic: `Au` in, whole device pixels out, `1/1024`
//! transform coefficients, and a table-free integer sine ([`trig`]) for rotations,
//! gradients and rounded corners.

pub mod background;
pub mod border;
pub mod data_url;
pub mod display_list;
pub mod hit;
pub mod images;
pub mod png;
pub mod replaced;
pub mod semantics;
pub mod text;
pub mod trig;

#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use cw_scene::{Color, Node, Primitive, Rect as SRect, RoundedClip, Scene, ScrollArea, Transform};

use crate::dom::{Document, NodeId};
use crate::geom::{Au, Point, Rect};
use crate::layout::fragment::{Fragment, FragmentKind, FragmentTree, StyleSource};
use crate::style::computed::{BackgroundImage, ComputedStyle, StyleSet};
use crate::Viewport;

/// A decoded raster, RGBA row-major, `width * height * 4` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl RgbaImage {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> RgbaImage {
        debug_assert_eq!(rgba.len() as u64, width as u64 * height as u64 * 4);
        RgbaImage {
            width,
            height,
            rgba,
        }
    }
    /// A solid colour image, for tests and placeholders.
    pub fn solid(width: u32, height: u32, c: Color) -> RgbaImage {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            rgba.extend_from_slice(&[c.0, c.1, c.2, c.3]);
        }
        RgbaImage {
            width,
            height,
            rgba,
        }
    }
}

/// Where paint gets pixels for a resolved image URL. The session owns fetching and
/// decoding; paint never does I/O.
pub trait ImageCache {
    fn image(&self, url: &str) -> Option<&RgbaImage>;
}

/// An image cache with nothing in it: every image is missing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoImages;

impl ImageCache for NoImages {
    fn image(&self, _url: &str) -> Option<&RgbaImage> {
        None
    }
}

/// An in-memory image cache keyed by URL.
#[derive(Clone, Debug, Default)]
pub struct ImageMap(pub BTreeMap<String, RgbaImage>);

impl ImageCache for ImageMap {
    fn image(&self, url: &str) -> Option<&RgbaImage> {
        self.0.get(url)
    }
}

static NO_IMAGES: NoImages = NoImages;

/// A text selection: `(text node, byte offset)` endpoints in document order,
/// `start` before `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: (NodeId, usize),
    pub end: (NodeId, usize),
}

/// Everything paint needs beyond the styled fragment tree: the interaction state the
/// browser session holds and the resources it fetched.
pub struct PaintContext<'a> {
    pub images: &'a dyn ImageCache,
    /// How far the document (the root scroll container) is scrolled.
    pub scroll: Point,
    /// Scroll offsets of inner scroll containers, overriding what layout recorded.
    pub scroll_offsets: BTreeMap<NodeId, Point>,
    /// The element holding keyboard focus; its caret is painted and `Scene::focus`
    /// filled.
    pub focused: Option<NodeId>,
    /// The element under the pointer (`:hover` is the cascade's business; paint uses
    /// this only for the cursor of the focus record).
    pub hovered: Option<NodeId>,
    pub selection: Option<Selection>,
    /// Caret position in the focused control's value, in characters; `None` is the
    /// end.
    pub caret: Option<usize>,
    /// Live values of form controls (what the user typed or chose), overriding the
    /// DOM's `value` attribute and text content.
    pub values: BTreeMap<NodeId, String>,
    /// Added to every scene node id, so a shell can keep several documents apart.
    pub id_base: u64,
}

impl<'a> PaintContext<'a> {
    pub fn new(images: &'a dyn ImageCache) -> PaintContext<'a> {
        PaintContext {
            images,
            scroll: Point::default(),
            scroll_offsets: BTreeMap::new(),
            focused: None,
            hovered: None,
            selection: None,
            caret: None,
            values: BTreeMap::new(),
            id_base: 0,
        }
    }
}

impl Default for PaintContext<'static> {
    fn default() -> Self {
        PaintContext::new(&NO_IMAGES)
    }
}

/// Drops what painting caches across paints (rasterised inline svgs), so the next
/// paint computes everything afresh. Output never depends on the caches; this is
/// for tests that check so.
pub fn clear_caches() {
    replaced::clear_svg_cache();
}

/// Paints the document. See the module documentation.
pub fn paint(
    doc: &Document,
    styles: &StyleSet,
    tree: &FragmentTree,
    viewport: Viewport,
    ctx: &PaintContext,
) -> Scene {
    let _t = crate::style::profile::span(crate::style::profile::Phase::Paint);
    let mut p = Painter::new(Some(doc), styles, tree, viewport, ctx);
    p.run();
    p.finish()
}

/// Paints without a document: no semantics, no interactions, no form state. Used by
/// hit testing and by tests that build fragment trees by hand.
pub fn paint_fragments(
    styles: &StyleSet,
    tree: &FragmentTree,
    viewport: Viewport,
    ctx: &PaintContext,
) -> Scene {
    let mut p = Painter::new(None, styles, tree, viewport, ctx);
    p.run();
    p.finish()
}

/// The classic scrollbar an inner scroll container draws in the gutter it reserved:
/// Chromium's own light track and thumb, so a pane that reserves 15 px shows a bar
/// there instead of a blank strip.
pub(crate) const SCROLLBAR_TRACK: Color = Color(241, 241, 241, 255);
pub(crate) const SCROLLBAR_THUMB: Color = Color(193, 193, 193, 255);

// Scene node ids: `base + node << 28 | ordinal << 16 | part`.
const ORDINAL_BITS: u32 = 12;
const PART_BITS: u32 = 16;

/// The scene node id of `part` of the `ordinal`-th fragment of DOM node `node`.
pub fn scene_id(base: u64, node: NodeId, ordinal: u32, part: u32) -> u64 {
    let ordinal = (ordinal as u64) & ((1 << ORDINAL_BITS) - 1);
    let part = (part as u64) & ((1 << PART_BITS) - 1);
    base.wrapping_add(
        ((node.0 as u64) << (ORDINAL_BITS + PART_BITS)) | (ordinal << PART_BITS) | part,
    )
}

/// Whole-pixel conversion, half away from zero.
pub(crate) fn px(a: Au) -> i32 {
    a.to_px_round()
}

pub(crate) fn upx(a: Au) -> u32 {
    a.to_px_round().max(0) as u32
}

pub(crate) fn mul_opacity(a: u8, b: u8) -> u8 {
    ((a as u32 * b as u32 + 127) / 255) as u8
}

/// State inherited down the fragment tree while painting: where the parent's border
/// box is, what clips apply, the group opacity, the accumulated transform.
#[derive(Clone, Debug)]
pub(crate) struct State {
    /// Absolute origin (scene coordinates, `Au`) of the parent fragment's border box,
    /// including scroll offsets.
    pub origin: Point,
    /// The scroll offsets accumulated on the way down (`origin` already subtracts
    /// them); a `position: fixed` box adds them back.
    pub scrolled: Point,
    pub clip: Option<SRect>,
    pub rounded_clip: Option<RoundedClip>,
    pub opacity: u8,
    pub transform: Transform,
    pub transformed: bool,
}

impl State {
    fn root(viewport: Viewport) -> State {
        State {
            origin: Point::default(),
            scrolled: Point::default(),
            clip: Some(SRect::new(0, 0, viewport.width, viewport.height)),
            rounded_clip: None,
            opacity: 255,
            transform: Transform::default(),
            transformed: false,
        }
    }
    /// Intersects the inherited clip with `r` (scene coordinates).
    pub fn clipped(&self, r: SRect) -> State {
        let mut s = self.clone();
        s.clip = Some(match s.clip {
            Some(c) => c.intersection(r).unwrap_or(SRect::new(r.x, r.y, 0, 0)),
            None => r,
        });
        s
    }
}

/// What hit testing needs about one painted box, recorded in paint order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HitItem {
    pub node: NodeId,
    pub bounds: SRect,
    pub radius: u32,
    pub clip: Option<SRect>,
    pub rounded_clip: Option<RoundedClip>,
    pub transform: Transform,
    pub pointer_none: bool,
}

impl HitItem {
    pub fn covers(&self, x: i32, y: i32) -> bool {
        self.clip.is_none_or(|c| c.contains(x, y))
            && self
                .rounded_clip
                .is_none_or(|c| cw_scene::rounded_contains(c.rect, c.radius, x, y))
            && self
                .transform
                .inverse_point(x, y)
                .is_some_and(|(x, y)| cw_scene::rounded_contains(self.bounds, self.radius, x, y))
    }
}

type GradientKey = (u32, u32, BackgroundImage);

/// The paint pass: accumulates scene nodes, scroll areas, hit items and the focus.
pub(crate) struct Painter<'a> {
    /// Record only hit-test items: a hit list needs the traversal, clips and
    /// transforms, not the scene's primitives.
    pub hits_only: bool,
    pub doc: Option<&'a Document>,
    pub styles: &'a StyleSet,
    pub tree: &'a FragmentTree,
    pub viewport: Viewport,
    pub ctx: &'a PaintContext<'a>,
    pub nodes: Vec<Node>,
    pub scrolls: Vec<ScrollArea>,
    pub focus: Option<cw_scene::Focus>,
    pub background: Color,
    pub hits: Vec<HitItem>,
    /// `(node, ordinal)` of every fragment, keyed by its address, assigned in tree
    /// order before painting starts.
    ordinals: HashMap<usize, (NodeId, u32), PtrHash>,
    pub gradients: HashMap<GradientKey, Rc<Vec<u8>>>,
    initial: ComputedStyle,
    pub semantics: semantics::Tables,
    /// The element whose background was propagated to the canvas, which then paints
    /// none of its own.
    pub canvas_source: Option<NodeId>,
    /// Sequential part numbers handed out per fragment (`ordinal key`) for pieces
    /// whose count depends on content (dashes, tiles, glyphs).
    parts: HashMap<(NodeId, u32), u32>,
    /// Document (pre-order) index of every DOM node, and of the last node of its
    /// subtree: stacking layers paint in tree order of the *elements*, which the
    /// fragment tree loses for boxes hoisted to their containing block.
    doc_order: HashMap<NodeId, (u32, u32)>,
}

/// Part numbers of the fixed pieces of a fragment; content-dependent pieces are
/// allocated from `DYNAMIC_PARTS` upwards.
pub(crate) mod parts {
    pub const REGION: u32 = 0;
    pub const BACKGROUND: u32 = 1;
    pub const BORDER_TOP: u32 = 2;
    pub const BORDER_RIGHT: u32 = 3;
    pub const BORDER_BOTTOM: u32 = 4;
    pub const BORDER_LEFT: u32 = 5;
    pub const OUTLINE: u32 = 6;
    pub const TEXT: u32 = 7;
    pub const UNDERLINE: u32 = 8;
    pub const OVERLINE: u32 = 9;
    pub const LINE_THROUGH: u32 = 10;
    pub const SELECTION: u32 = 11;
    pub const CARET: u32 = 12;
    pub const CONTENT: u32 = 13;
    pub const CONTENT_TEXT: u32 = 14;
    pub const CONTENT_GLYPH: u32 = 15;
    pub const BACKDROP: u32 = 16;
    pub const DYNAMIC: u32 = 64;
}

impl<'a> Painter<'a> {
    pub fn new(
        doc: Option<&'a Document>,
        styles: &'a StyleSet,
        tree: &'a FragmentTree,
        viewport: Viewport,
        ctx: &'a PaintContext<'a>,
    ) -> Painter<'a> {
        let mut p = Painter {
            doc,
            styles,
            tree,
            viewport,
            ctx,
            nodes: Vec::new(),
            scrolls: Vec::new(),
            focus: None,
            background: Color::WHITE,
            hits: Vec::new(),
            hits_only: false,
            ordinals: HashMap::default(),
            gradients: HashMap::new(),
            initial: ComputedStyle::initial(),
            semantics: semantics::Tables::default(),
            canvas_source: None,
            parts: HashMap::new(),
            doc_order: HashMap::new(),
        };
        if let Some(doc) = doc {
            p.semantics = semantics::Tables::build(doc);
        }
        // Layout recorded the order of the document it laid out; only a hand-built
        // tree painted with a document needs it worked out here.
        if let (Some(doc), true) = (doc, tree.doc_order.is_empty()) {
            for (i, n) in doc.descendants(Document::ROOT).enumerate() {
                p.doc_order.insert(n, (i as u32, i as u32));
                for a in doc.ancestors(n) {
                    if let Some(e) = p.doc_order.get_mut(&a) {
                        e.1 = i as u32;
                    }
                }
            }
        }
        p
    }

    /// Where a fragment's element sits in document order: `(index, rank)`, with
    /// `::before` just inside the element's start and `::after` after its last
    /// descendant. `None` without a laid-out or given document, or for anonymous
    /// fragments' lack of one.
    pub fn tree_order(&self, f: &Fragment) -> Option<(u32, u8)> {
        let src = f.source()?;
        let (start, end) = match self.tree.doc_order.get(src.node()) {
            Some(e) => e,
            None => *self.doc_order.get(&src.node())?,
        };
        Some(match src {
            StyleSource::Before(_) | StyleSource::Marker(_) => (start, 1),
            StyleSource::After(_) => (end, 2),
            _ => (start, 0),
        })
    }

    fn assign_ordinals(&mut self) {
        let mut counts: Vec<u32> = Vec::new();
        fn walk(
            f: &Fragment,
            counts: &mut Vec<u32>,
            out: &mut HashMap<usize, (NodeId, u32), PtrHash>,
        ) {
            if let Some(node) = id_node(f) {
                if counts.len() <= node.index() {
                    counts.resize(node.index() + 1, 0);
                }
                let n = &mut counts[node.index()];
                out.insert(f as *const Fragment as usize, (node, *n));
                *n += 1;
            }
            for c in &f.children {
                walk(c, counts, out);
            }
        }
        walk(&self.tree.root, &mut counts, &mut self.ordinals);
    }

    /// `(node, ordinal)` of a fragment; line boxes have none. A hits-only pass
    /// names no scene nodes, so it needs only whether there is one.
    pub fn key(&self, f: &Fragment) -> Option<(NodeId, u32)> {
        if self.hits_only {
            return id_node(f).map(|n| (n, 0));
        }
        self.ordinals.get(&(f as *const Fragment as usize)).copied()
    }

    pub fn id(&self, key: (NodeId, u32), part: u32) -> u64 {
        scene_id(self.ctx.id_base, key.0, key.1, part)
    }

    /// The next content-dependent part number of a fragment.
    pub fn next_part(&mut self, key: (NodeId, u32)) -> u32 {
        let p = self.parts.entry(key).or_insert(parts::DYNAMIC);
        let v = *p;
        *p += 1;
        v
    }

    pub fn style(&self, src: StyleSource) -> &ComputedStyle {
        let s = match src {
            StyleSource::Element(n) | StyleSource::Anonymous(n) => self.styles.get(n),
            StyleSource::Before(n) => self.styles.before(n).or_else(|| self.styles.get(n)),
            StyleSource::After(n) => self.styles.after(n).or_else(|| self.styles.get(n)),
            StyleSource::Marker(n) => self.styles.marker(n).or_else(|| self.styles.get(n)),
        };
        s.unwrap_or(&self.initial)
    }

    /// [`Painter::style`], shared: for a caller that goes on to borrow the
    /// painter mutably while it reads the style (a clone of the style would cost a
    /// deep copy per fragment).
    pub fn style_rc(&self, src: StyleSource) -> std::rc::Rc<ComputedStyle> {
        let s = match src {
            StyleSource::Element(n) | StyleSource::Anonymous(n) => self.styles.get_rc(n),
            StyleSource::Before(n) => self.styles.before.get(&n).or(self.styles.get_rc(n)),
            StyleSource::After(n) => self.styles.after.get(&n).or(self.styles.get_rc(n)),
            StyleSource::Marker(n) => self.styles.marker.get(&n).or(self.styles.get_rc(n)),
        };
        match s {
            Some(s) => s.clone(),
            None => ComputedStyle::initial_rc(),
        }
    }

    pub fn style_of(&self, f: &Fragment) -> &ComputedStyle {
        match f.source() {
            Some(s) => self.style(s),
            None => &self.initial,
        }
    }

    /// Adds a node with the inherited clip, opacity and transform applied. Returns its
    /// index in `nodes`.
    pub fn emit(&mut self, state: &State, id: u64, bounds: SRect, primitive: Primitive) -> usize {
        let mut n = Node::new(id, bounds, primitive);
        n.z = 0;
        n.clip = state.clip;
        n.rounded_clip = state.rounded_clip;
        n.opacity = state.opacity;
        if state.transformed {
            n.transform = state.transform;
        }
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    /// Adds a `Path` node from points in scene coordinates. The scene's path points
    /// are relative to the node's bounds, which is easy to get wrong: emitting
    /// absolute points draws the path displaced by the box's own origin.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_path(
        &mut self,
        state: &State,
        id: u64,
        bounds: SRect,
        points: Vec<(i32, i32)>,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: u16,
        closed: bool,
    ) -> usize {
        let points = points
            .into_iter()
            .map(|(x, y)| (x - bounds.x, y - bounds.y))
            .collect();
        self.emit(
            state,
            id,
            bounds,
            Primitive::Path {
                points,
                fill,
                stroke,
                stroke_width,
                closed,
            },
        )
    }

    pub fn record_hit(
        &mut self,
        state: &State,
        node: NodeId,
        bounds: SRect,
        radius: u32,
        pointer_none: bool,
    ) {
        self.hits.push(HitItem {
            node,
            bounds,
            radius,
            clip: state.clip,
            rounded_clip: state.rounded_clip,
            transform: state.transform,
            pointer_none,
        });
    }

    pub fn run(&mut self) {
        if !self.hits_only && self.ordinals.is_empty() {
            self.assign_ordinals();
        }
        let state = State::root(self.viewport);
        display_list::paint_root(self, &state);
    }

    pub fn finish(self) -> Scene {
        let mut scene = Scene::new(self.viewport.width, self.viewport.height);
        scene.background = self.background;
        scene.typeface = self
            .doc
            .and_then(|d| d.document_element())
            .and_then(|h| self.styles.get(h))
            .map(|s| s.font.typeface)
            .unwrap_or_default();
        scene.nodes = self.nodes;
        // Later nodes paint over earlier ones; publish that as `z` so consumers that
        // sort by `z` alone see the painting order.
        for (i, n) in scene.nodes.iter_mut().enumerate() {
            n.z = i as i32;
        }
        scene.scrolls = self.scrolls;
        scene.focus = self.focus;
        scene
    }

    /// The scroll offset of a scroll container, from the context or from layout.
    pub fn scroll_of(&self, node: NodeId, info: &crate::layout::fragment::ScrollInfo) -> Point {
        self.ctx
            .scroll_offsets
            .get(&node)
            .copied()
            .unwrap_or(Point {
                x: info.scroll_x,
                y: info.scroll_y,
            })
    }
}

/// The DOM node a fragment's scene ids are derived from: the text node for a text
/// run, else the element the fragment paints for.
fn id_node(f: &Fragment) -> Option<NodeId> {
    match &f.kind {
        FragmentKind::Text { node: Some(n), .. } => Some(*n),
        _ => f.source().map(StyleSource::node),
    }
}

/// A hasher for keys that are addresses: one multiply, where SipHash would do
/// dozens of rounds per fragment.
#[derive(Clone, Copy, Default)]
pub(crate) struct PtrHasher(u64);

impl std::hash::Hasher for PtrHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(*b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_usize(&mut self, n: usize) {
        self.0 = (self.0.rotate_left(5) ^ n as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

pub(crate) type PtrHash = std::hash::BuildHasherDefault<PtrHasher>;

/// Absolute border-box rect of a fragment in scene pixels.
pub(crate) fn abs_rect(state: &State, f: &Fragment) -> Rect {
    f.rect.translate(state.origin.x, state.origin.y)
}

/// Snaps to scene pixels.
pub(crate) fn snap(r: Rect) -> SRect {
    r.to_scene()
}
