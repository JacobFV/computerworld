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

use crate::dom::{Document, NodeId, NodeKind, QuirksMode};
use crate::geom::{Au, Size};
use crate::style::{ComputedStyle, StyleSet};
use crate::Viewport;

use boxes::{BoxId, BoxKind, BoxTree};

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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    /// The host draws scrollbars over the content (or not at all) instead of taking
    /// 15 px from the scroll container: overlay scrollbars, or a headless Chromium
    /// (Playwright launches it with `--hide-scrollbars`, which is what the parity
    /// dumps were taken with). A host setting, so it survives `invalidate_all`.
    pub overlay_scrollbars: bool,
    /// Results of `block::layout_block_box` for formatting-context roots within one
    /// pass, by box and constraints (see there).
    pub block_memo: std::collections::HashMap<BlockMemoKey, block::MemoEntry>,
    /// Lay every box out every time it is asked for (for checks of the memo).
    pub no_memo: bool,
    /// Results of `block::layout_block_box` kept from earlier passes, by the digest
    /// of everything the box's layout reads (see `digests`) and its constraints:
    /// a formatting-context root whose subtree did not change is not laid out
    /// again. Entries not used by a pass are dropped after it.
    pub kept: std::collections::HashMap<KeptKey, block::MemoEntry>,
    /// The entries this pass used or made, which become `kept` after it.
    pub kept_next: std::collections::HashMap<KeptKey, block::MemoEntry>,
    /// Keep results across passes (a host that lays the same document out again
    /// and again sets this; a one-off layout has nothing to reuse them for).
    pub keep_across_passes: bool,
    /// Per box of this pass, the digest of its subtree's layout inputs.
    pub digests: Vec<u128>,
}

/// A subtree's digest and the constraints it was laid out under (as `BlockMemoKey`).
pub type KeptKey = (u128, Au, Option<Au>, Option<Au>, Option<Option<Au>>);

/// A box and the constraints it was laid out under: containing block width and
/// height, forced width, forced height.
pub type BlockMemoKey = (BoxId, Au, Option<Au>, Option<Au>, Option<Option<Au>>);

impl LayoutCache {
    pub fn invalidate_all(&mut self) {
        self.intrinsic.clear();
        self.empty_block.clear();
        self.forced_height.clear();
        self.block_memo.clear();
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

/// For each box, a 128-bit digest of everything `block::layout_block_box` reads
/// for it and its subtree, so that a later pass can reuse a result: each box's
/// kind (text, replaced content and sizes, cell spans, marker text), flags and
/// style (its node's style epoch: see `StyleSet::epoch`), its scroll offset, its children's and marker's digests, an
/// inline svg's elements and their styles, and what every box reads globally (the
/// viewport, quirks mode, scrollbar mode, the viewport's overflow and the root and
/// body styles).
fn digests(
    doc: &Document,
    styles: &StyleSet,
    tree: &BoxTree,
    scroll: &ScrollState,
    viewport: (Au, Au),
    root_overflow: (crate::style::Overflow, crate::style::Overflow),
    cache: &LayoutCache,
) -> Vec<u128> {
    use std::hash::{Hash, Hasher};
    type H = std::collections::hash_map::DefaultHasher;
    let mut global = H::new();
    viewport.hash(&mut global);
    (doc.quirks == QuirksMode::Quirks).hash(&mut global);
    cache.overlay_scrollbars.hash(&mut global);
    root_overflow.hash(&mut global);
    for n in [doc.document_element(), doc.body()].into_iter().flatten() {
        styles.epoch(n).hash(&mut global);
    }
    let global = global.finish();
    let n = tree.len();
    let mut out: Vec<Option<u128>> = vec![None; n];
    fn visit(
        id: BoxId,
        doc: &Document,
        styles: &StyleSet,
        tree: &BoxTree,
        scroll: &ScrollState,
        global: u64,
        out: &mut Vec<Option<u128>>,
    ) -> u128 {
        if let Some(d) = out[id.index()] {
            return d;
        }
        let b = &tree[id];
        let mut hs = [H::new(), H::new()];
        0x9e37_79b9u32.hash(&mut hs[1]);
        let kids: Vec<u128> = b
            .children
            .iter()
            .chain(b.marker.iter())
            .map(|c| visit(*c, doc, styles, tree, scroll, global, out))
            .collect();
        for h in hs.iter_mut() {
            global.hash(h);
            b.kind.hash(h);
            b.source.hash(h);
            b.node.hash(h);
            b.level.hash(h);
            b.inline_children.hash(h);
            b.control.hash(h);
            b.is_root.hash(h);
            b.split_first.hash(h);
            b.split_last.hash(h);
            b.is_item.hash(h);
            b.marker.is_some().hash(h);
            // Every box's style is its node's computed (or pseudo-element) style,
            // or derived from it by the box's kind and flags (blockified items,
            // anonymous boxes, table wrappers), all hashed here.
            styles.epoch(b.source.node()).hash(h);
            if let Some(node) = b.node {
                scroll.get(&node).hash(h);
            }
            kids.hash(h);
        }
        // An inline svg's shapes and text are laid out from its elements.
        if let (BoxKind::Replaced(_), Some(node)) = (&b.kind, b.node) {
            if doc.tag(node) == Some("svg") && crate::svg::is_svg(doc, node) {
                for d in doc.descendants(node) {
                    for h in hs.iter_mut() {
                        match doc.kind(d) {
                            NodeKind::Element { tag, attrs, .. } => {
                                tag.hash(h);
                                for a in attrs {
                                    a.name.hash(h);
                                    a.value.hash(h);
                                }
                            }
                            NodeKind::Text(t) => t.hash(h),
                            _ => {}
                        }
                        styles.epoch(d).hash(h);
                        doc.parent(d).hash(h);
                    }
                }
            }
        }
        let d = (u128::from(hs[0].finish()) << 64) | u128::from(hs[1].finish());
        out[id.index()] = Some(d);
        d
    }
    (0..n)
        .map(|i| visit(BoxId(i as u32), doc, styles, tree, scroll, global, &mut out))
        .collect()
}

/// The standard entry point: no image sizes known, no scroll offsets.
pub fn layout(doc: &Document, styles: &StyleSet, viewport: Viewport) -> FragmentTree {
    let scroll = ScrollState::new();
    let mut cache = LayoutCache::default();
    layout_with(
        doc,
        styles,
        viewport,
        LayoutOptions {
            images: &NoImages,
            scroll: &scroll,
        },
        &mut cache,
    )
}

/// Layout with image sizes, scroll offsets and a caller-owned cache.
pub fn layout_with(
    doc: &Document,
    styles: &StyleSet,
    viewport: Viewport,
    opts: LayoutOptions<'_>,
    cache: &mut LayoutCache,
) -> FragmentTree {
    cache.invalidate_all();
    let zoom = (viewport.zoom.max(1)) as i32;
    let vw = Au::from_px_i32(viewport.width as i32).scale(100, zoom);
    let vh = Au::from_px_i32(viewport.height as i32).scale(100, zoom);
    let bt = crate::style::profile::span(crate::style::profile::Phase::BoxTree);
    let mut tree = boxes::build(doc, styles, opts.images);
    let root_overflow = scroll::propagate_root_overflow(doc, &mut tree);
    cache.digests = if cache.no_memo || !cache.keep_across_passes {
        Vec::new()
    } else {
        digests(
            doc,
            styles,
            &tree,
            opts.scroll,
            (vw, vh),
            root_overflow,
            cache,
        )
    };
    drop(bt);
    let _t = crate::style::profile::span(crate::style::profile::Phase::Layout);
    cache.reserve(tree.len());
    let ctx = LayoutContext {
        doc,
        styles,
        tree,
        images: opts.images,
        scroll: opts.scroll,
        viewport: Size {
            width: vw,
            height: vh,
        },
        quirks: doc.quirks == QuirksMode::Quirks,
        root_overflow,
        cache: RefCell::new(std::mem::take(cache)),
    };
    let out = scroll::layout_root(&ctx);
    *cache = ctx.cache.into_inner();
    cache.block_memo.clear();
    cache.kept = std::mem::take(&mut cache.kept_next);
    cache.digests.clear();
    // Under the incremental check, the memo is checked against laying every box
    // out every time.
    if crate::style::profile::verifying() && !cache.no_memo {
        let mut plain = LayoutCache {
            overlay_scrollbars: cache.overlay_scrollbars,
            no_memo: true,
            ..LayoutCache::default()
        };
        let fresh = layout_with(doc, styles, viewport, opts, &mut plain);
        assert!(
            fresh == out,
            "a layout reusing results within the pass differs from one that does not"
        );
    }
    out
}
