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
    HitList::build(tree, styles, viewport, ctx).at(x_px, y_px)
}

/// Every painted box and text run in painting order, with its clips: what hit
/// testing walks. Built once per layout (and style change), it answers any number
/// of points, so a host that hit-tests a pointer move and then a click on the same
/// layout pays for one traversal.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HitList {
    hits: Vec<super::HitItem>,
}

impl HitList {
    pub fn build(
        tree: &FragmentTree,
        styles: &StyleSet,
        viewport: Viewport,
        ctx: &PaintContext,
    ) -> HitList {
        let _t = crate::style::profile::span(crate::style::profile::Phase::HitTest);
        let mut p = Painter::new(None, styles, tree, viewport, ctx);
        p.hits_only = true;
        p.run();
        HitList { hits: p.hits }
    }

    /// The same list recorded by a full paint (primitives and all), which the
    /// hits-only traversal must reproduce; for checks.
    pub fn build_by_painting(
        tree: &FragmentTree,
        styles: &StyleSet,
        viewport: Viewport,
        ctx: &PaintContext,
    ) -> HitList {
        let mut p = Painter::new(None, styles, tree, viewport, ctx);
        p.run();
        HitList { hits: p.hits }
    }

    /// The topmost node at `(x, y)` in scene pixels.
    pub fn at(&self, x_px: i32, y_px: i32) -> Option<NodeId> {
        self.hits
            .iter()
            .rev()
            .find(|h| !h.pointer_none && h.covers(x_px, y_px))
            .map(|h| h.node)
    }
}
