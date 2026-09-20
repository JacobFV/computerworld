//! Layout: from the styled DOM to a tree of positioned fragments in app units.
//! `fragment.rs` is the shared contract between `layout` and `paint`; see DESIGN.md.
//!
//! The pipeline inside this module:
//!
//! ```text
//! Document + StyleSet --boxes--> BoxTree (anonymous boxes, table fixup, generated content)
//! BoxTree --block/inline/table--> Fragment tree (block formatting contexts, floats,
//!                                positioned boxes, line boxes)
//! --scroll--> scroll containers sized, root overflow propagated
//! --sticky post-pass--> FragmentTree
//! ```
//!
//! Everything is fixed point (`Au`); text is measured through `cw_scene::metrics` and
//! quantised to 1/64 px exactly as the renderer paints it. Bidi: `direction: rtl`
//! reverses the line direction and the alignment start/end; full Unicode bidi
//! reordering of mixed-direction runs is out of scope for M1.

pub mod block;
pub mod boxes;
pub mod debug;
pub mod flex;
pub mod fragment;
pub mod grid;
pub mod inline;
pub mod intrinsic;
pub mod scroll;
pub mod table;
pub mod text;

#[cfg(test)]
mod tests;

pub use fragment::*;

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::dom::{Document, NodeId, QuirksMode};
use crate::geom::{Au, Size};
use crate::style::{ComputedStyle, StyleSet};
use crate::Viewport;

use boxes::{BoxId, BoxTree};

/// Intrinsic sizes of images the document references, supplied by the caller from its
/// image cache. `None` means the image is not (yet) available: the element falls back
/// to its attributes, then to a 16x16 placeholder.
pub trait ImageSizes {
    fn size(&self, src: &str) -> Option<(u32, u32)>;
}

/// No images are known.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoImages;

impl ImageSizes for NoImages {
    fn size(&self, _src: &str) -> Option<(u32, u32)> {
        None
    }
}

/// Image sizes from a map, for tests and simple hosts.
#[derive(Clone, Debug, Default)]
pub struct ImageSizeMap(pub BTreeMap<String, (u32, u32)>);

impl ImageSizes for ImageSizeMap {
    fn size(&self, src: &str) -> Option<(u32, u32)> {
        self.0.get(src).copied()
    }
}

/// Scroll offsets per scroll container, keyed by the element's node; the viewport is
/// keyed by `Document::ROOT`. Values are `(scroll_x, scroll_y)` in `Au`, clamped by
/// layout to the scrollable range.
pub type ScrollState = BTreeMap<NodeId, (Au, Au)>;

/// What the caller passes besides the document, styles and viewport.
#[derive(Clone, Copy)]
pub struct LayoutOptions<'a> {
    pub images: &'a dyn ImageSizes,
    pub scroll: &'a ScrollState,
}

/// Per-box cached results. Invalidated wholesale on every `layout` call for now; the
/// cache exists so that M3 can invalidate per node instead. Intrinsic sizes are keyed
/// by `BoxId` of the tree built for that call; `BoxTree::box_of` maps nodes to boxes.
#[derive(Clone, Debug, Default)]
pub struct LayoutCache {
    /// `(min-content, max-content)` widths of the box's margin-less border box.
    pub intrinsic: Vec<Option<(Au, Au)>>,
    /// Structural "collapses through" answers (see `block::is_empty_block`).
    pub empty_block: Vec<Option<bool>>,
    /// Content-box heights imposed on boxes by the formatting context they are items
    /// of (flex and grid stretch or flexing); `block::layout_block_box` uses one in
    /// place of the box's own `height` (`None` means "as if auto"). Set and removed
    /// around the item's layout.
    pub forced_height: BTreeMap<BoxId, Option<Au>>,
}

impl LayoutCache {
    pub fn invalidate_all(&mut self) {
        self.intrinsic.clear();
        self.empty_block.clear();
        self.forced_height.clear();
    }
    fn reserve(&mut self, n: usize) {
        self.intrinsic.resize(n, None);
        self.empty_block.resize(n, None);
    }
}

/// Everything a layout pass reads. Immutable except for the cache.
pub struct LayoutContext<'a> {
    pub doc: &'a Document,
    pub styles: &'a StyleSet,
    pub tree: BoxTree,
    pub images: &'a dyn ImageSizes,
    pub scroll: &'a ScrollState,
    /// The initial containing block, in CSS px units after zoom.
    pub viewport: Size,
    pub quirks: bool,
    /// The viewport's `overflow` (x, y), propagated from `<html>` or `<body>`.
    pub root_overflow: (crate::style::Overflow, crate::style::Overflow),
    pub cache: RefCell<LayoutCache>,
}

impl<'a> LayoutContext<'a> {
    pub fn style(&self, b: BoxId) -> &ComputedStyle {
        &self.tree[b].style
    }
}

/// The standard entry point: no image sizes known, no scroll offsets.
pub fn layout(doc: &Document, styles: &StyleSet, viewport: Viewport) -> FragmentTree {
    let scroll = ScrollState::new();
    let mut cache = LayoutCache::default();
    layout_with(doc, styles, viewport, LayoutOptions { images: &NoImages, scroll: &scroll }, &mut cache)
}

/// Layout with image sizes, scroll offsets and a caller-owned cache.
pub fn layout_with(doc: &Document, styles: &StyleSet, viewport: Viewport, opts: LayoutOptions<'_>, cache: &mut LayoutCache) -> FragmentTree {
    cache.invalidate_all();
    let zoom = (viewport.zoom.max(1)) as i32;
    let vw = Au::from_px_i32(viewport.width as i32).scale(100, zoom);
    let vh = Au::from_px_i32(viewport.height as i32).scale(100, zoom);
    let mut tree = boxes::build(doc, styles, opts.images);
    let root_overflow = scroll::propagate_root_overflow(doc, &mut tree);
    cache.reserve(tree.len());
    let ctx = LayoutContext {
        doc,
        styles,
        tree,
        images: opts.images,
        scroll: opts.scroll,
        viewport: Size { width: vw, height: vh },
        quirks: doc.quirks == QuirksMode::Quirks,
        root_overflow,
        cache: RefCell::new(std::mem::take(cache)),
    };
    let out = scroll::layout_root(&ctx);
    *cache = ctx.cache.into_inner();
    out
}

