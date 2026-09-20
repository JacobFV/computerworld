//! End-to-end regression tests for what Acid2 and the Slack-style shell exercise: real
//! HTML and CSS through the parser, the cascade, layout and paint, with the document's
//! `data:` images decoded by [`ImageMap::from_document`]. Each test is the smallest
//! page that showed the bug.

use cw_scene::{Color, Primitive, Scene};

use super::{paint, ImageMap, PaintContext};
use crate::css::{parse_stylesheet, MatchContext, Media, Origin};
use crate::dom::{Document, NodeId};
use crate::geom::{Au, Point, Rect};
use crate::layout::fragment::{Fragment, FragmentKind, FragmentTree, Replaced};
use crate::layout::{layout_with, LayoutCache, LayoutOptions, ScrollState};
use crate::style::StyleSet;
use crate::{Strictness, Viewport};

/// A 1x1 yellow PNG and a 64x64 red one, as Acid2 spells them (percent-encoded base64).
const YELLOW_1X1: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR42mP4%2F58BAAT%2FAf9jgNErAAAAAElFTkSuQmCC";
const RED_64: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAFSDNYfAAAAaklEQVR42u3XQQrAIAwAQeP%2F%2F6wf8CJBJTK9lnQ7FpHGaOurt1I34nfH9pMMZAZ8BwMGEvvh%2BBsJCAgICLwIOA8EBAQEBAQEBAQEBK79H5RfIQAAAAAAAAAAAAAAAAAAAAAAAAAAAID%2FABMSqAfj%2FsLmvAAAAABJRU5ErkJggg%3D%3D";

const VP: Viewport = Viewport { width: 400, height: 300, scale: 1, zoom: 100 };

struct Page {
    doc: Document,
    styles: StyleSet,
    tree: FragmentTree,
    scene: Scene,
}

fn render(html: &str) -> Page {
    render_with(html, Au::ZERO, false)
}

/// Renders `html` with the document scrolled to `scroll_y`.
fn render_with(html: &str, scroll_y: Au, overlay_scrollbars: bool) -> Page {
    render_scrolled(html, scroll_y, &[], overlay_scrollbars)
}

/// Renders `html` with the document scrolled to `scroll_y` and each named element
/// scrolled to its own offset (measured from the start of its scrollable area).
fn render_scrolled(html: &str, scroll_y: Au, inner: &[(&str, Au)], overlay_scrollbars: bool) -> Page {
    let doc = crate::html::parse(html);
    let mut sheets = Vec::new();
    for n in doc.descendants(Document::ROOT) {
        if doc.is(n, "style") {
            sheets.push(parse_stylesheet(&doc.text_content(n), Origin::Author, Strictness::Lenient).expect("stylesheet"));
        }
    }
    let media = Media::with_size(VP.width as i32, VP.height as i32);
    let styles = crate::style::cascade(&doc, &sheets, &media, &MatchContext::new(), Strictness::Lenient).expect("cascade");
    let images = ImageMap::from_document(&doc, &styles);
    let mut scroll = ScrollState::new();
    scroll.insert(Document::ROOT, (Au::ZERO, scroll_y));
    for (id, y) in inner {
        scroll.insert(*doc.by_id(id).first().unwrap_or_else(|| panic!("no #{id}")), (Au::ZERO, *y));
    }
    let mut cache = LayoutCache { overlay_scrollbars, ..LayoutCache::default() };
    let tree = layout_with(&doc, &styles, VP, LayoutOptions { images: &images, scroll: &scroll }, &mut cache);
    let mut ctx = PaintContext::new(&images);
    if let FragmentKind::Box { scroll: Some(info), .. } = &tree.root.kind {
        ctx.scroll = Point { x: info.scroll_x, y: info.scroll_y };
    }
    let scene = paint(&doc, &styles, &tree, VP, &ctx);
    Page { doc, styles, tree, scene }
}

impl Page {
    fn by_id(&self, id: &str) -> NodeId {
        *self.doc.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"))
    }
    /// The absolute border box of the first box fragment of `#id`, and the fragment.
    fn frag(&self, id: &str) -> Option<(Rect, &Fragment)> {
        let node = self.by_id(id);
        let mut found = None;
        self.tree.root.walk(Point::default(), &mut |f, abs| {
            if found.is_none() && matches!(f.kind, FragmentKind::Box { .. } | FragmentKind::InlineBox { .. }) && f.source().is_some_and(|s| s.node() == node && !s.is_anonymous()) {
                found = Some((abs, f));
            }
        });
        found
    }
    fn rect(&self, id: &str) -> Rect {
        self.frag(id).unwrap_or_else(|| panic!("#{id} generates no box")).0
    }
    /// The colour of the topmost opaque rectangle covering the pixel: enough to read
    /// solid-colour test pages without rasterising.
    fn top_fill(&self, x: i32, y: i32) -> Option<Color> {
        self.scene.nodes.iter().rev().find_map(|n| {
            let inside = n.bounds.contains(x, y) && n.clip.is_none_or(|c| c.contains(x, y));
            match &n.primitive {
                Primitive::Box { fill, .. } | Primitive::RoundedBox { fill, .. } if inside && fill.3 == 255 => Some(*fill),
                _ => None,
            }
        })
    }
    fn images(&self) -> Vec<(cw_scene::Rect, u32, u32)> {
        self.scene
            .nodes
            .iter()
            .filter_map(|n| match &n.primitive {
                Primitive::Image { width, height, .. } => Some((n.bounds, *width, *height)),
                _ => None,
            })
            .collect()
    }
}

fn px(n: i32) -> Au {
    Au::from_px_i32(n)
}

const GREEN: Color = Color(0, 128, 0, 255);

#[test]
fn img_with_a_data_url_takes_its_intrinsic_size_and_paints() {
    let p = render(&format!("<!doctype html><body style='margin:0'><img id=i src=\"{RED_64}\" alt=''>"));
    let r = p.rect("i");
    assert_eq!((r.size.width, r.size.height), (px(64), px(64)));
    assert_eq!(p.images().len(), 1, "the decoded image is painted");
    assert_eq!((p.images()[0].1, p.images()[0].2), (64, 64));
}

#[test]
fn background_image_from_a_data_url_tiles_the_box() {
    let p = render(&format!("<!doctype html><style>body {{ margin: 0 }} #b {{ width: 20px; height: 10px; background: red url({YELLOW_1X1}) }}</style><div id=b></div>"));
    assert!(!p.images().is_empty(), "the 1x1 tile is painted over the fallback colour");
    assert!(p.images().iter().all(|(_, w, h)| (*w, *h) == (1, 1)));
}

#[test]
fn object_that_cannot_load_renders_its_fallback_and_a_loaded_image_object_renders_the_image() {
    // Acid2's chain: an unknown type, then a document the engine does not nest (a 404
    // on the web), then the image. Only the innermost is replaced content; the outer
    // two are ordinary inline boxes around it and the text `ERROR` is never rendered.
    let p = render(&format!(
        "<!doctype html><body style='margin:0'><div id=host><object id=a data=\"data:application/x-unknown,ERROR\"><object id=b data=\"404.html\" type=\"text/html\"><object id=c data=\"{RED_64}\">ERROR</object></object></object></div>"
    ));
    let (rc, fc) = p.frag("c").expect("the innermost object has a box");
    assert!(matches!(&fc.kind, FragmentKind::Box { replaced: Some(Replaced::Image { .. }), .. }), "the image object is replaced content");
    assert_eq!((rc.size.width, rc.size.height), (px(64), px(64)));
    let (_, fa) = p.frag("a").expect("the outer object has a box");
    assert!(matches!(&fa.kind, FragmentKind::InlineBox { .. }), "a fallen-back object is an inline box, not a 300x150 placeholder: {:?}", fa.kind);
    let mut texts = Vec::new();
    p.tree.root.walk(Point::default(), &mut |f, _| {
        if let FragmentKind::Text { text, .. } = &f.kind {
            texts.push(text.clone());
        }
    });
    assert!(texts.iter().all(|t| !t.contains("ERROR")), "the image's fallback text is not rendered: {texts:?}");
    assert_eq!(p.images().len(), 1);
}

#[test]
fn replaced_elements_clip_their_overflow_in_the_ua_sheet() {
    let p = render(&format!("<!doctype html><img id=i src=\"{RED_64}\"><button id=b>Go</button>"));
    let s = p.styles.get(p.by_id("i")).unwrap();
    assert_eq!((s.overflow_x, s.overflow_y), (crate::style::Overflow::Clip, crate::style::Overflow::Clip));
    // A `<button>`'s label wraps (`white-space: normal`); only input buttons are `pre`.
    assert_eq!(p.styles.get(p.by_id("b")).unwrap().white_space, crate::style::WhiteSpace::Normal);
}

#[test]
fn a_clipping_replaced_element_is_not_a_scroll_area() {
    // `overflow: clip` from the UA sheet clips an image to its content box; it must
    // not turn every `<img>` into something the shell offers to scroll.
    let with = render(&format!("<!doctype html><p><img src=\"{RED_64}\"></p>"));
    let without = render("<!doctype html><p>text</p>");
    assert_eq!(with.scene.scrolls.len(), without.scene.scrolls.len());
}

#[test]
fn fragment_navigation_scrolls_a_viewport_whose_overflow_is_hidden() {
    // `html { overflow: hidden }` hides the scrollbars and stops the user scrolling;
    // navigating to `#top` still scrolls, and the page is offset exactly once.
    let html = "<!doctype html><style>html { overflow: hidden } body { margin: 0 } #gap { height: 1000px } #top { height: 50px; background: green } #tail { height: 1000px }</style><div id=gap></div><div id=top></div><div id=tail></div>";
    let p = render_with(html, px(1000), false);
    match &p.tree.root.kind {
        FragmentKind::Box { scroll: Some(info), .. } => assert_eq!(info.scroll_y, px(1000)),
        k => panic!("root has no scroll info: {k:?}"),
    }
    assert_eq!(p.top_fill(10, 10), Some(GREEN), "#top is at the top of the viewport, not scrolled past it");
    assert_eq!(p.top_fill(10, 49), Some(GREEN));
    assert_ne!(p.top_fill(10, 51), Some(GREEN));
}

#[test]
fn fixed_boxes_stay_put_when_the_document_scrolls() {
    let html = "<!doctype html><style>body { margin: 0; height: 3000px } #f { position: fixed; top: 20px; left: 30px; width: 40px; height: 10px; background: green }</style><div id=f></div>";
    let p = render_with(html, px(500), false);
    assert_eq!(p.top_fill(31, 21), Some(GREEN));
    assert_ne!(p.top_fill(31, 35), Some(GREEN));
}

#[test]
fn positioned_descendants_of_a_z_auto_box_interleave_with_the_parent_context_in_document_order() {
    // Acid2's eyes. `#picture` is positioned with `z-index: auto`, so it is not a
    // stacking context: its absolutely positioned `#eyes` belongs to the root context,
    // and paints after the fixed `#bad` that precedes it in the document, although
    // the fixed box hangs off the viewport's fragment, after everything in `<html>`.
    let html = "<!doctype html><style>body { margin: 0 } #picture { position: relative; height: 100px } #bad { margin: 0; position: fixed; top: 10px; left: 10px; width: 50px; height: 20px; background: red } #eyes { position: absolute; top: 0; left: 0; width: 100px; height: 50px; background: green }</style><div id=picture><p id=bad></p><div id=eyes></div></div>";
    let p = render(html);
    assert_eq!(p.top_fill(20, 20), Some(GREEN), "the later absolute box covers the earlier fixed one");
    // And the other way round: a fixed box that follows paints on top.
    let html = "<!doctype html><style>body { margin: 0 } #picture { position: relative; height: 100px } #late { margin: 0; position: fixed; top: 10px; left: 10px; width: 50px; height: 20px; background: green } #eyes { position: absolute; top: 0; left: 0; width: 100px; height: 50px; background: red }</style><div id=picture><div id=eyes></div><p id=late></p></div>";
    assert_eq!(render(html).top_fill(20, 20), Some(GREEN));
}

#[test]
fn positioned_child_of_a_float_joins_the_enclosing_context() {
    // A float is painted atomically at step 4, but its positioned descendants are
    // layer-6 members of the enclosing context: they paint over later inline content
    // and in document order against other positioned boxes.
    let html = "<!doctype html><style>body { margin: 0 } #fl { float: left; width: 100px; height: 100px } #in { position: relative; width: 60px; height: 60px; background: red } #over { position: absolute; top: 0; left: 0; width: 60px; height: 60px; background: green }</style><div id=fl><div id=in></div></div><div id=over></div>";
    assert_eq!(render(html).top_fill(30, 30), Some(GREEN));
}

#[test]
fn margins_inside_an_empty_block_collapse_through_it_and_clearance_can_be_negative() {
    // Acid2 between the nose and the smile (CSS 2.1 §8.3.1, §9.5.2). `#empty`
    // collapses through, and so does its child, whose -72px bottom margin joins the
    // set: 48 (forehead), 75, 75, 0, -72 collapse to 3px, so `#smile` would sit at
    // 12 + 3 = 15px, above the float's bottom margin edge at 72px; clearance puts
    // its border edge exactly there, and being 72 - 15 - 60 = -3px it is negative.
    let html = "<!doctype html><style>body { margin: 0 } #forehead { height: 12px; margin-bottom: 48px } #nose { float: left; width: 50px; height: 48px; margin: -24px 0 -12px } #empty { margin: 75px } #empty div { margin: 0 24px -72px 48px } #smile { margin: 60px 36px; clear: both; height: 10px }</style><div id=forehead></div><div id=nose></div><div id=empty><div></div></div><div id=smile></div>";
    let p = render(html);
    assert_eq!(p.rect("nose").origin.y, px(12 + 48 - 24));
    assert_eq!(p.rect("smile").origin.y, px(36 + 48 - 12), "the smile's border edge is at the float's bottom margin edge");
}

#[test]
fn a_block_that_collapses_through_sits_before_its_own_bottom_margin() {
    // Positions measured in Chromium: the empty block is placed where the margins up
    // to and including its top margin collapse (its own bottom margin joins the set
    // only for what follows), and the next block after the whole set.
    let html = "<!doctype html><body style='margin:0'><div id=a style='height:10px;margin-bottom:10px'></div><div id=e style='margin-top:20px;margin-bottom:30px'></div><div id=b style='margin-top:15px;height:10px'></div>";
    let p = render(html);
    assert_eq!(p.rect("e").origin.y, px(30));
    assert_eq!(p.rect("b").origin.y, px(40));
}

#[test]
fn button_contents_are_centred_vertically_in_an_explicit_height() {
    let html = "<!doctype html><style>body { margin: 0 } button { height: 60px; padding: 0; border: 0; font: 16px/20px sans-serif } span { display: block; width: 10px; height: 20px }</style><button id=b><span id=s></span></button><button id=tall style='height: auto'><span id=t></span></button>";
    let p = render(html);
    let (b, s) = (p.rect("b"), p.rect("s"));
    assert_eq!(b.size.height, px(60));
    assert_eq!(s.origin.y - b.origin.y, px(20), "20px of content in 60px: 20px above it");
    let (tall, t) = (p.rect("tall"), p.rect("t"));
    assert_eq!(t.origin.y, tall.origin.y, "an auto-height button has nothing to distribute");
}

#[test]
fn overlay_scrollbars_take_no_space_from_a_scroll_container() {
    let html = "<!doctype html><style>body { margin: 0 } #s { width: 200px; height: 100px; overflow: auto } #c { height: 500px }</style><div id=s><div id=c></div></div>";
    assert_eq!(render_with(html, Au::ZERO, false).rect("c").size.width, px(185), "a classic bar reserves 15px");
    assert_eq!(render_with(html, Au::ZERO, true).rect("c").size.width, px(200), "an overlay bar reserves nothing");
}


// --- The gaps the service-migration pages hit ---------------------------------

/// The rasterised page, so a test can read what a reader would see rather than the
/// primitive that was meant to draw it.
struct Pixels(cw_render::Frame);

fn pixels(p: &Page) -> Pixels {
    Pixels(cw_render::Renderer::new().render(&p.scene))
}

impl Pixels {
    fn at(&self, x: u32, y: u32) -> Color {
        let i = ((y * self.0.width + x) * 4) as usize;
        Color(self.0.rgba[i], self.0.rgba[i + 1], self.0.rgba[i + 2], self.0.rgba[i + 3])
    }
    /// How many pixels of `rect` are `color` exactly.
    fn count(&self, rect: (u32, u32, u32, u32), color: Color) -> u32 {
        let (x0, y0, w, h) = rect;
        (y0..y0 + h).flat_map(|y| (x0..x0 + w).map(move |x| (x, y))).filter(|&(x, y)| self.at(x, y) == color).count() as u32
    }
}

/// The string a text node draws, whichever text primitive it is.
fn label(n: &cw_scene::Node) -> Option<&str> {
    match &n.primitive {
        Primitive::Text { text, .. } | Primitive::UiText { text, .. } | Primitive::UiTextBold { text, .. } => Some(text),
        _ => None,
    }
}

const WHITE: Color = Color(255, 255, 255, 255);
const INK: Color = Color(0, 136, 0, 255);

#[test]
fn a_background_with_unequal_radii_and_a_border_with_a_transparent_side_still_paint() {
    // Both are drawn as `Path` nodes, whose points are offsets from the node's
    // bounds; emitted in absolute coordinates they landed off the box and the boxes
    // appeared blank. A uniform radius takes the `RoundedBox` path and always worked.
    let html = "<!doctype html><body style='margin:0'><div id=a style='width:60px;height:40px;background:#080;border-radius:0 8px 8px 0'></div><div id=b style='width:60px;height:40px;border:3px solid #080;border-top-color:transparent;border-radius:0 0 12px 12px'></div>";
    let f = pixels(&render(html));
    assert_eq!(f.at(1, 1), INK, "the square top-left corner is filled");
    assert_eq!(f.at(30, 20), INK, "and so is the middle");
    assert_eq!(f.at(59, 0), WHITE, "the 8px top-right corner is cut away");
    // The bordered box starts at y = 40: left and bottom borders paint, the top does not.
    assert_eq!(f.at(1, 60), INK, "the left border");
    assert_eq!(f.at(30, 84), INK, "the bottom border, under the 12px corners");
    assert_eq!(f.at(30, 41), WHITE, "the transparent top border");
}

#[test]
fn three_borders_on_a_zero_sized_box_paint_a_triangle() {
    // The CSS caret: `width: 0` with a coloured border on one side and transparent
    // ones above and below. Each side used to be a rectangular strip, and the strip
    // between two full-height borders is empty, so nothing was drawn at all.
    let html = "<!doctype html><body style='margin:0'><div id=t style='width:0;height:0;border-top:12px solid transparent;border-bottom:12px solid transparent;border-right:18px solid #080'></div>";
    let p = render(html);
    assert_eq!(p.rect("t").size, crate::geom::Size { width: px(18), height: px(24) });
    let f = pixels(&p);
    assert_eq!(f.at(16, 12), INK, "the wide end of the triangle");
    assert_eq!(f.at(3, 12), INK, "and its tip, halfway up");
    assert_eq!(f.at(3, 2), WHITE, "the corner above the hypotenuse is clear");
    assert_eq!(f.at(3, 21), WHITE, "and the one below it");
}

#[test]
fn an_absolutely_positioned_flex_container_paints_once_and_keeps_its_text() {
    // Its text box borrows the container's style, so `position: absolute` made the
    // text look like an absolutely positioned child: it was dropped as an item and
    // laid out as the element all over again, inside itself and offset by the
    // element's own inset.
    let html = "<!doctype html><body style='margin:0'><div style='position:relative;width:60px;height:40px'><span id=x style='position:absolute;left:10px;top:5px;display:inline-flex;width:30px;height:30px;border-radius:50%;background:#080'>AB</span></div>";
    let p = render(html);
    assert_eq!(p.rect("x").origin, Point { x: px(10), y: px(5) });
    let fills: Vec<_> = p.scene.nodes.iter().filter(|n| matches!(&n.primitive, Primitive::RoundedBox { fill, .. } if *fill == INK)).map(|n| n.bounds).collect();
    assert_eq!(fills.len(), 1, "one background, not one per copy of the box: {fills:?}");
    let texts: Vec<&str> = p.scene.nodes.iter().filter_map(label).collect();
    assert_eq!(texts, ["AB"], "the flex container's text survives");
}

#[test]
fn a_reversed_column_scrolls_to_the_content_above_it() {
    // `column-reverse` packs from the bottom, so the earlier items sit above the
    // padding box. That part of the scrollable overflow was not counted, so the
    // offset clamped to zero and the content above could never be reached.
    let html = "<!doctype html><style>body { margin: 0 } #s { height: 100px; overflow-y: auto; display: flex; flex-direction: column-reverse } div div { flex: none; height: 60px }</style><div id=s><div id=a>a</div><div id=b>b</div><div id=c>c</div></div>";
    let info_of = |p: &Page| match &p.frag("s").unwrap().1.kind {
        FragmentKind::Box { scroll: Some(i), .. } => *i,
        _ => panic!("#s is not a scroll container"),
    };
    // Where the line of text `c` — the first item, at the top of the area — is drawn.
    let c_at = |p: &Page| p.scene.nodes.iter().find(|n| label(n) == Some("c")).expect("the first item's text").bounds.y;
    let p = render(html);
    let info = info_of(&p);
    assert_eq!(info.content_height, px(180), "three 60px items are the scrollable height");
    assert_eq!(info.origin_y, px(-80), "80px of them sit above the padding box");
    assert!(info.shows_y_bar, "and `auto` therefore shows a bar");
    assert_eq!(p.rect("a").origin.y, px(40), "at rest the last item is at the bottom");
    assert!(p.rect("c").origin.y < Au::ZERO, "and the first is out of sight above");
    assert!(c_at(&p) < 0, "nothing of it is painted inside the box");
    // An offset is measured from the start of the scrollable area, as `scrollTop`
    // is, so 0 is the top of the first item — not the resting place, which is the
    // end. The whole 80px above the box is reachable.
    let top = render_scrolled(html, Au::ZERO, &[("s", Au::ZERO)], false);
    assert_eq!(info_of(&top).scroll_y, px(-80), "the contents drop by the 80px above the box");
    assert_eq!(c_at(&top), c_at(&p) + 80, "and the first item comes into view");
    let end = render_scrolled(html, Au::ZERO, &[("s", px(80))], false);
    assert_eq!(info_of(&end).scroll_y, Au::ZERO, "the maximum offset is where layout put them");
    assert_eq!(c_at(&end), c_at(&p));
}

#[test]
fn an_emoji_measures_as_wide_as_it_is_painted() {
    // Layout gave any character the metrics tables do not cover 0.6 em, while the
    // renderer shaped it with the fallback face, which for an emoji is far wider.
    // A flex item sized from that measurement was too narrow and its text spilled.
    let html = "<!doctype html><body style='margin:0'><button id=b style='display:inline-flex;font:16px sans-serif'>\u{1f525} 2</button>";
    let p = render(html);
    let text = p.scene.nodes.iter().find(|n| label(n) == Some("\u{1f525} 2")).expect("the label");
    let button = p.rect("b");
    assert!(
        Au::from_px_i32(text.bounds.right()) <= button.right(),
        "the label ({:?}) fits the button it measured ({:?})",
        text.bounds,
        button
    );
}

#[test]
fn an_inset_shadow_on_a_round_box_is_a_ring() {
    // `inset 0 0 0 4px` is how a ring inside a round avatar is drawn. It used to be
    // four square shadow strips laid over the box, which read as an offset block.
    let html = "<!doctype html><body style='margin:0'><div id=a style='width:40px;height:40px;border-radius:50%;background:#2b2d31;box-shadow:inset 0 0 0 4px #080'></div>";
    let f = pixels(&render(html));
    assert_eq!(f.at(20, 2), INK, "the ring at the top of the circle");
    assert_eq!(f.at(2, 20), INK, "and at its left");
    assert_eq!(f.at(20, 20), Color(43, 45, 49, 255), "the fill inside the ring");
    assert_eq!(f.at(2, 2), WHITE, "the corner outside the circle is untouched");
}

#[test]
fn a_scaled_box_paints_scaled() {
    // The element's own background was painted in its parent's coordinate space, so
    // a `transform` moved only its children and looked like it had done nothing.
    let html = "<!doctype html><body style='margin:0'><div style='width:100px;height:100px'><div id=a style='width:40px;height:40px;background:#080;transform:scale(2)'></div></div>";
    let f = pixels(&render(html));
    assert_eq!(f.at(55, 55), INK, "40x40 scaled about its centre reaches 60,60");
    assert_eq!(f.at(65, 65), WHITE, "but no further");
}

#[test]
fn a_scroll_container_paints_a_bar_in_the_gutter_it_reserved() {
    // Reserving 15px and drawing nothing in it left a blank strip down every pane.
    let html = "<!doctype html><style>body { margin: 0 } #s { width: 200px; height: 100px; overflow-y: auto } #c { height: 500px }</style><div id=s><div id=c></div></div>";
    let p = render(html);
    let track = p.scene.nodes.iter().find(|n| matches!(&n.primitive, Primitive::Box { fill, .. } if *fill == super::SCROLLBAR_TRACK)).expect("a track");
    assert_eq!(track.bounds, cw_scene::Rect::new(185, 0, 15, 100));
    let thumb = p.scene.nodes.iter().find(|n| matches!(&n.primitive, Primitive::RoundedBox { fill, .. } if *fill == super::SCROLLBAR_THUMB)).expect("a thumb");
    assert_eq!((thumb.bounds.y, thumb.bounds.height), (0, 20), "at the top, a fifth of the track");
    let scrolled = render_scrolled(html, Au::ZERO, &[("s", px(400))], false);
    let thumb = scrolled.scene.nodes.iter().find(|n| matches!(&n.primitive, Primitive::RoundedBox { fill, .. } if *fill == super::SCROLLBAR_THUMB)).expect("a thumb");
    assert_eq!(thumb.bounds.y, 80, "scrolled to the end it sits at the bottom");
    // An overlay host reserves no gutter, so it draws no bar.
    let overlay = render_with(html, Au::ZERO, true);
    assert!(!overlay.scene.nodes.iter().any(|n| matches!(&n.primitive, Primitive::Box { fill, .. } if *fill == super::SCROLLBAR_TRACK)));
}

#[test]
fn a_fractional_font_size_does_not_weld_a_word_to_the_inline_box_after_it() {
    // A scene's text size is a whole number of pixels, so a 14.67px face was drawn
    // at 15px while layout placed the next box for 14.67px: over a few words the
    // extra third of a pixel per em ate the space and the page read "one twoSPAN".
    for size in ["13px", "14px", "14.67px", "15px", "15.5px", "16px", "18.25px"] {
        let html = format!("<!doctype html><body style='margin:0;font-family:Arial'><p id=p style='font-size:{size}'>one two three four five <span id=s>SPAN</span></p>");
        let p = render(&html);
        let run = p.scene.nodes.iter().find(|n| label(n).is_some_and(|t| t.starts_with("one two"))).expect("the prose");
        let span = p.scene.nodes.iter().find(|n| label(n) == Some("SPAN")).expect("the span");
        // The run's node is its advance plus two pixels of slack; the advance may
        // still round up by the one pixel `text_width` adds, but no more: the space
        // before the span has to survive.
        assert!(
            run.bounds.right() - 2 <= span.bounds.x + 1,
            "at {size} the prose is drawn to {} and the span starts at {}",
            run.bounds.right() - 2,
            span.bounds.x
        );
    }
}

#[test]
fn an_element_with_an_id_is_addressable_in_the_semantic_tree() {
    // A cell or a span with an id has no role of its own worth reporting, but the id
    // is exactly how an agent is told to find it, so it gets an entry either way.
    let html = "<!doctype html><body><table><tr><td id=sheet-A2>412 ms</td><td>no id</td></tr></table><span id=title>Documents</span><span>plain</span>";
    let p = render(html);
    let named: Vec<(&str, &str, &str)> = p
        .scene
        .nodes
        .iter()
        .filter_map(|n| Some((n.interaction.as_deref()?, n.semantic.as_ref()?.role.as_str(), n.semantic.as_ref()?.label.as_str())))
        .collect();
    assert!(named.contains(&("sheet-A2", "cell", "412 ms")), "{named:?}");
    assert!(named.contains(&("title", "generic", "Documents")), "{named:?}");
    assert!(!named.iter().any(|(_, _, label)| *label == "plain"), "an id-less span stays out: {named:?}");
}
