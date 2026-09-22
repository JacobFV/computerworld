//! `elementFromPoint` over the painting order.
//!
//! Hit testing replays the display-list traversal, recording every painted box and
//! text run with the clip, rounded clip and transform in force, then walks the record
//! from the top (last painted) down. A box with `pointer-events: none`, or one that
//! is `visibility: hidden`, is skipped so the hit falls through to whatever is
//! beneath it, which may be its own parent. A text run maps to the element it was
//! laid out for. Points outside every ancestor clip miss, as they do in a browser
//! where an overflow-clipped child is not clickable beyond its parent's edge.

use super::{PaintContext, Painter};
use crate::dom::NodeId;
use crate::layout::fragment::FragmentTree;
use crate::style::computed::StyleSet;
use crate::Viewport;

/// The topmost element at `(x, y)` in scene pixels, with no scroll and no context.
pub fn hit_test(tree: &FragmentTree, styles: &StyleSet, x_px: i32, y_px: i32) -> Option<NodeId> {
    let viewport = Viewport {
        width: super::upx(tree.viewport_width).max(1),
        height: super::upx(tree.viewport_height).max(1),
        scale: 1,
        zoom: 100,
    };
    hit_test_with(tree, styles, viewport, &PaintContext::default(), x_px, y_px)
}

/// Hit test with the scroll offsets and state of a paint context.
pub fn hit_test_with(
    tree: &FragmentTree,
    styles: &StyleSet,
    viewport: Viewport,
    ctx: &PaintContext,
    x_px: i32,
    y_px: i32,
) -> Option<NodeId> {
    let mut p = Painter::new(None, styles, tree, viewport, ctx);
    p.run();
    p.hits
        .iter()
        .rev()
        .find(|h| !h.pointer_none && h.covers(x_px, y_px))
        .map(|h| h.node)
}
