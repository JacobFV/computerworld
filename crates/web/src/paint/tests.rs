//! Paint tests over hand-built fragment trees and computed styles: no parser, no
//! cascade, no layout, so they hold whatever those modules do.

use std::rc::Rc;

use cw_scene::{Color, Primitive, Scene};

use super::*;
use crate::dom::{Attribute, Document, NodeId};
use crate::geom::{Au, Edges, Rect};
use crate::layout::fragment::{ControlKind, Fragment, FragmentKind, FragmentTree, Replaced, ScrollInfo, StyleSource};
use crate::style::computed::*;

fn au(px: i32) -> Au {
    Au::from_px_i32(px)
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(au(x), au(y), au(w), au(h))
}

fn block() -> ComputedStyle {
    let mut s = ComputedStyle::initial();
    s.display = Display::Block;
    s
}

fn set(styles: &mut StyleSet, n: u32, f: impl FnOnce(&mut ComputedStyle)) {
    let mut s = block();
    f(&mut s);
    styles.set(NodeId(n), Rc::new(s));
}

fn boxf(n: u32, r: Rect) -> Fragment {
    Fragment::new(FragmentKind::Box { source: StyleSource::Element(NodeId(n)), padding: Edges::ZERO, border: Edges::ZERO, replaced: None, scroll: None, baseline: None }, r)
}

fn bordered(n: u32, r: Rect, b: Edges) -> Fragment {
    Fragment::new(FragmentKind::Box { source: StyleSource::Element(NodeId(n)), padding: Edges::ZERO, border: b, replaced: None, scroll: None, baseline: None }, r)
}

fn line(r: Rect, children: Vec<Fragment>) -> Fragment {
    let mut f = Fragment::new(FragmentKind::Line, r);
    f.children = children;
    f
}

fn text_run(elem: u32, text_node: Option<u32>, text: &str, r: Rect, baseline: i32) -> Fragment {
    Fragment::new(
        FragmentKind::Text { source: StyleSource::Element(NodeId(elem)), text: text.into(), node: text_node.map(NodeId), range: (0, text.len()), baseline: au(baseline), ellipsis: false },
        r,
    )
}

fn inline_box(n: u32, r: Rect, first: bool, last: bool) -> Fragment {
    Fragment::new(FragmentKind::InlineBox { source: StyleSource::Element(NodeId(n)), padding: Edges::ZERO, border: Edges::ZERO, first, last }, r)
}

fn tree(root: Fragment) -> FragmentTree {
    let (w, h) = (root.rect.size.width, root.rect.size.height);
    FragmentTree { root, content_width: w, content_height: h, viewport_width: w, viewport_height: h }
}

fn viewport(w: u32, h: u32) -> Viewport {
    Viewport { width: w, height: h, scale: 1, zoom: 100 }
}

/// `(node, ordinal, part)` of a scene node id.
fn decode(id: u64) -> (u32, u32, u32) {
    ((id >> 28) as u32, ((id >> 16) & 0xFFF) as u32, (id & 0xFFFF) as u32)
}

fn painted(scene: &Scene) -> Vec<(u32, u32)> {
    scene.nodes.iter().map(|n| decode(n.id)).map(|(n, _, p)| (n, p)).collect()
}

fn backgrounds(scene: &Scene) -> Vec<u32> {
    scene.nodes.iter().map(|n| decode(n.id)).filter(|(_, _, p)| *p == parts::BACKGROUND).map(|(n, _, _)| n).collect()
}

fn paint_no_doc(styles: &StyleSet, t: &FragmentTree) -> Scene {
    paint_fragments(styles, t, viewport(upx(t.viewport_width), upx(t.viewport_height)), &PaintContext::default())
}

const RED: Color = Color(255, 0, 0, 255);
const BLUE: Color = Color(0, 0, 255, 255);

#[test]
fn appendix_e_order_within_one_stacking_context() {
    // root(1): block A(2), float F(3), positioned P(4, z auto), negative N(5),
    // positive Z(6), a line with inline box I(7) around text of element 2.
    let mut styles = StyleSet::new();
    for n in 1..=7 {
        set(&mut styles, n, |s| s.background_color = RED);
    }
    let mut root = boxf(1, rect(0, 0, 200, 200));
    let mut n = boxf(5, rect(0, 0, 10, 10));
    n.establishes_stacking_context = true;
    n.z_index = -1;
    n.is_positioned = true;
    let mut z = boxf(6, rect(0, 0, 10, 10));
    z.establishes_stacking_context = true;
    z.z_index = 1;
    z.is_positioned = true;
    let mut p = boxf(4, rect(0, 0, 10, 10));
    p.is_positioned = true;
    let mut f = boxf(3, rect(0, 0, 10, 10));
    f.is_float = true;
    let a = boxf(2, rect(0, 20, 100, 20));
    let mut i = inline_box(7, rect(0, 0, 50, 20), true, true);
    i.children = vec![text_run(7, None, "hi", rect(0, 0, 20, 20), 16)];
    let l = line(rect(0, 40, 200, 20), vec![i]);
    // Tree order deliberately puts the later-painted things first.
    root.children = vec![z, p, l, f, n, a];
    let t = tree(root);
    let scene = paint_no_doc(&styles, &t);
    assert_eq!(backgrounds(&scene), vec![1, 5, 2, 3, 7, 4, 6], "root, negative, block, float, inline, positioned, positive");
    // Text paints after the inline box background and before the positioned box.
    let order = painted(&scene);
    let text_at = order.iter().position(|(n, p)| *n == 7 && *p == parts::TEXT).unwrap();
    let inline_bg = order.iter().position(|(n, p)| *n == 7 && *p == parts::BACKGROUND).unwrap();
    let pos_bg = order.iter().position(|(n, p)| *n == 4 && *p == parts::BACKGROUND).unwrap();
    assert!(inline_bg < text_at && text_at < pos_bg);
}

#[test]
fn z_index_sorts_contexts_and_keeps_tree_order_for_ties() {
    let mut styles = StyleSet::new();
    for n in 1..=6 {
        set(&mut styles, n, |s| s.background_color = RED);
    }
    let mut root = boxf(1, rect(0, 0, 100, 100));
    let mk = |n: u32, z: i32| {
        let mut b = boxf(n, rect(0, 0, 10, 10));
        b.establishes_stacking_context = true;
        b.is_positioned = true;
        b.z_index = z;
        b
    };
    root.children = vec![mk(2, 3), mk(3, -1), mk(4, 1), mk(5, -2), mk(6, 1)];
    let scene = paint_no_doc(&styles, &tree(root));
    assert_eq!(backgrounds(&scene), vec![1, 5, 3, 4, 6, 2]);
}

#[test]
fn floats_paint_before_inline_content_even_when_later_in_tree() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| s.background_color = BLUE);
    set(&mut styles, 3, |s| s.background_color = RED);
    let mut root = boxf(1, rect(0, 0, 100, 100));
    let l = line(rect(0, 0, 100, 20), vec![text_run(1, None, "text", rect(0, 0, 40, 20), 16)]);
    let mut f = boxf(2, rect(60, 0, 40, 40));
    f.is_float = true;
    let mut nested = boxf(3, rect(0, 0, 10, 10));
    nested.is_float = true;
    f.children = vec![nested];
    root.children = vec![l, f];
    let scene = paint_no_doc(&styles, &tree(root));
    let order = painted(&scene);
    let float_bg = order.iter().position(|(n, p)| *n == 2 && *p == parts::BACKGROUND).unwrap();
    let inner = order.iter().position(|(n, p)| *n == 3 && *p == parts::BACKGROUND).unwrap();
    let text_at = order.iter().position(|(n, p)| *n == 1 && *p == parts::TEXT).unwrap();
    assert!(float_bg < inner && inner < text_at);
}

fn simple_doc() -> (Document, NodeId, NodeId) {
    let mut d = Document::new();
    let html = d.create_element("html", vec![]);
    d.append(Document::ROOT, html);
    let body = d.create_element("body", vec![]);
    d.append(html, body);
    (d, html, body)
}

#[test]
fn body_background_propagates_to_the_canvas() {
    let (doc, html, body) = simple_doc();
    let mut styles = StyleSet::new();
    set(&mut styles, html.0, |_| {});
    set(&mut styles, body.0, |s| {
        s.background_color = RED;
        s.border = Sides::uniform(BorderSide { width: au(2), style: BorderStyle::Solid, color: BLUE });
    });
    let mut root = boxf(html.0, rect(0, 0, 100, 100));
    root.children = vec![bordered(body.0, rect(8, 8, 84, 84), Edges::uniform(au(2)))];
    let t = tree(root);
    let scene = paint(&doc, &styles, &t, viewport(100, 100), &PaintContext::default());
    assert_eq!(scene.background, RED);
    assert!(!backgrounds(&scene).contains(&body.0), "body paints no background of its own");
    assert!(scene.nodes.iter().any(|n| decode(n.id) == (body.0, 0, parts::BORDER_TOP)), "borders still paint");
    // With html carrying a background, html wins.
    set(&mut styles, html.0, |s| s.background_color = BLUE);
    let scene = paint(&doc, &styles, &t, viewport(100, 100), &PaintContext::default());
    assert_eq!(scene.background, BLUE);
    assert!(backgrounds(&scene).contains(&body.0));
}

fn side(style: BorderStyle, w: i32, color: Color) -> BorderSide {
    BorderSide { width: au(w), style, color }
}

fn border_scene(f: impl FnOnce(&mut ComputedStyle)) -> Scene {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, f);
    let mut root = boxf(1, rect(0, 0, 200, 200));
    root.children = vec![bordered(2, rect(10, 10, 100, 50), Edges::uniform(au(2)))];
    paint_no_doc(&styles, &tree(root))
}

fn border_nodes(scene: &Scene) -> Vec<&cw_scene::Node> {
    scene.nodes.iter().filter(|n| decode(n.id).0 == 2 && decode(n.id).2 >= parts::BORDER_TOP).collect()
}

#[test]
fn border_styles_produce_the_expected_node_counts() {
    let solid = border_scene(|s| s.border = Sides::uniform(side(BorderStyle::Solid, 2, RED)));
    assert_eq!(border_nodes(&solid).len(), 4);
    let top = border_nodes(&solid)[0];
    assert_eq!(top.bounds, cw_scene::Rect::new(10, 10, 100, 2));
    let double = border_scene(|s| s.border = Sides::uniform(side(BorderStyle::Double, 3, RED)));
    assert_eq!(border_nodes(&double).len(), 8);
    // Dashed top only: 100 px, dash 6 gap 2 -> 13 dashes.
    let dashed = border_scene(|s| s.border.top = side(BorderStyle::Dashed, 2, RED));
    assert_eq!(border_nodes(&dashed).len(), 13);
    let dotted = border_scene(|s| s.border.top = side(BorderStyle::Dotted, 2, RED));
    assert_eq!(border_nodes(&dotted).len(), 25);
    assert!(border_nodes(&dotted).iter().all(|n| matches!(n.primitive, Primitive::RoundedBox { .. })));
    let groove = border_scene(|s| s.border = Sides::uniform(side(BorderStyle::Groove, 4, Color(128, 128, 128, 255))));
    assert_eq!(border_nodes(&groove).len(), 8);
    let inset = border_scene(|s| s.border = Sides::uniform(side(BorderStyle::Inset, 4, Color(128, 128, 128, 255))));
    let nodes = border_nodes(&inset);
    assert_eq!(nodes.len(), 4);
    let fill = |n: &cw_scene::Node| match n.primitive {
        Primitive::Box { fill, .. } => fill,
        _ => unreachable!(),
    };
    assert!(fill(nodes[0]).0 < 128, "inset top is dark");
    assert!(fill(nodes[2]).0 > 128, "inset bottom is light");
    let none = border_scene(|s| s.border = Sides::uniform(side(BorderStyle::None, 2, RED)));
    assert_eq!(border_nodes(&none).len(), 0);
}

#[test]
fn rounded_borders_use_one_rounded_box_or_per_side_paths() {
    let r = (LengthPercentage::Length(au(8)), LengthPercentage::Length(au(8)));
    let uniform = border_scene(|s| {
        s.border = Sides::uniform(side(BorderStyle::Solid, 2, RED));
        s.border_radius = Corners { top_left: r, top_right: r, bottom_right: r, bottom_left: r };
        s.background_color = BLUE;
    });
    let nodes = border_nodes(&uniform);
    assert_eq!(nodes.len(), 1);
    assert!(matches!(nodes[0].primitive, Primitive::RoundedBox { radius: 8, border: Some(RED), border_width: 2, .. }));
    let bg = uniform.nodes.iter().find(|n| decode(n.id) == (2, 0, parts::BACKGROUND)).unwrap();
    assert!(matches!(bg.primitive, Primitive::RoundedBox { radius: 8, fill: BLUE, .. }));
    let mixed = border_scene(|s| {
        s.border = Sides::uniform(side(BorderStyle::Solid, 2, RED));
        s.border.bottom = side(BorderStyle::Solid, 4, BLUE);
        s.border_radius = Corners { top_left: r, top_right: r, bottom_right: r, bottom_left: (LengthPercentage::ZERO, LengthPercentage::ZERO) };
        s.background_color = BLUE;
    });
    let nodes = border_nodes(&mixed);
    assert_eq!(nodes.len(), 4);
    assert!(nodes.iter().all(|n| matches!(n.primitive, Primitive::Path { stroke: Some(_), closed: false, .. })));
    let bg = mixed.nodes.iter().find(|n| decode(n.id) == (2, 0, parts::BACKGROUND)).unwrap();
    assert!(matches!(bg.primitive, Primitive::Path { fill: Some(BLUE), closed: true, .. }));
}

#[test]
fn outline_and_shadows_are_drawn_outside_the_box() {
    let scene = border_scene(|s| {
        s.outline = side(BorderStyle::Solid, 2, RED);
        s.outline_offset = au(3);
        s.box_shadow = vec![BoxShadow { offset_x: au(2), offset_y: au(4), blur: au(6), spread: au(1), color: Color(0, 0, 0, 128), inset: false }];
    });
    let outline: Vec<_> = scene.nodes.iter().filter(|n| decode(n.id).2 == parts::OUTLINE).collect();
    assert_eq!(outline.len(), 1, "the first strip carries the outline part; the rest are dynamic parts");
    assert_eq!(outline[0].bounds, cw_scene::Rect::new(5, 5, 110, 2), "top strip of the offset outline");
    let strips = scene.nodes.iter().filter(|n| decode(n.id).0 == 2 && matches!(n.primitive, Primitive::Box { fill: RED, .. })).count();
    assert_eq!(strips, 4);
    let shadow = scene.nodes.iter().find(|n| matches!(n.primitive, Primitive::Shadow { .. })).unwrap();
    // border box 10,10,100,50 offset (2,4) spread 1 -> 11,13,102,52; blur 6 padding.
    assert_eq!(shadow.bounds, cw_scene::Rect::new(5, 7, 114, 64));
    assert!(matches!(shadow.primitive, Primitive::Shadow { blur: 6, .. }));
    // The shadow paints before the background, the outline after everything.
    let order: Vec<u32> = scene.nodes.iter().map(|n| decode(n.id).2).collect();
    let shadow_at = scene.nodes.iter().position(|n| matches!(n.primitive, Primitive::Shadow { .. })).unwrap();
    let outline_at = order.iter().position(|p| *p == parts::OUTLINE).unwrap();
    assert!(shadow_at < outline_at, "outlines paint last");
    assert!(order.last().is_some_and(|p| *p >= parts::DYNAMIC));
}

#[test]
fn text_is_placed_by_its_baseline_and_font_metrics() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| {
        s.font.size = au(16);
        s.color = BLUE;
        s.font.weight = 700;
        s.font.style = FontStyle::Italic;
        s.font.typeface = cw_scene::Typeface::Inter;
    });
    let mut root = boxf(1, rect(0, 0, 200, 100));
    root.children = vec![line(rect(0, 10, 200, 20), vec![text_run(2, None, "Hello", rect(5, 0, 40, 20), 14)])];
    let scene = paint_no_doc(&styles, &tree(root));
    let t = scene.nodes.iter().find(|n| decode(n.id).2 == parts::TEXT).unwrap();
    let font = styles.get(NodeId(2)).unwrap().font.clone();
    let w = cw_scene::metrics::text_width(cw_scene::Typeface::Inter, font.scene_style(), "Hello", 16);
    // Baseline at 10 + 14 = 24; the renderer draws the baseline `size` below the top.
    assert_eq!(t.bounds, cw_scene::Rect::new(5, 24 - 16, w + 2, 20));
    match &t.primitive {
        Primitive::UiTextBold { text, color, size, italic, typeface, .. } => {
            assert_eq!(text, "Hello");
            assert_eq!(*color, BLUE);
            assert_eq!(*size, 16);
            assert!(*italic);
            assert_eq!(*typeface, Some(cw_scene::Typeface::Inter));
        }
        p => panic!("{p:?}"),
    }
    assert_eq!(t.semantic.as_ref().map(|s| s.role.as_str()), Some("text"));
}

#[test]
fn decorations_sit_at_the_baseline_and_x_height() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| {
        s.font.size = au(20);
        s.text_decoration = TextDecoration { underline: true, overline: true, line_through: true, color: Some(RED), style: TextDecorationStyle::Solid };
    });
    let mut root = boxf(1, rect(0, 0, 200, 100));
    root.children = vec![line(rect(0, 0, 200, 30), vec![text_run(2, None, "abc", rect(0, 0, 40, 30), 22)])];
    let scene = paint_no_doc(&styles, &tree(root));
    let find = |part: u32| scene.nodes.iter().find(|n| decode(n.id).2 == part).unwrap();
    let w = cw_scene::metrics::text_width(cw_scene::Typeface::default(), false, "abc", 20);
    assert_eq!(find(parts::UNDERLINE).bounds, cw_scene::Rect::new(0, 23, w, 1));
    assert_eq!(find(parts::LINE_THROUGH).bounds, cw_scene::Rect::new(0, 22 - 7, w, 1));
    assert_eq!(find(parts::OVERLINE).bounds, cw_scene::Rect::new(0, 2, w, 1));
    assert!(matches!(find(parts::UNDERLINE).primitive, Primitive::Box { fill: RED, .. }));
    // Decorations follow the text in the display list.
    let order: Vec<u32> = scene.nodes.iter().map(|n| decode(n.id).2).collect();
    let text_at = order.iter().position(|p| *p == parts::TEXT).unwrap();
    let ul_at = order.iter().position(|p| *p == parts::UNDERLINE).unwrap();
    assert!(text_at < ul_at);
}

#[test]
fn letter_spacing_paints_one_node_per_character() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| s.letter_spacing = au(4));
    let mut root = boxf(1, rect(0, 0, 200, 100));
    root.children = vec![line(rect(0, 0, 200, 30), vec![text_run(2, None, "abc", rect(0, 0, 60, 30), 16)])];
    let scene = paint_no_doc(&styles, &tree(root));
    let glyphs: Vec<_> = scene.nodes.iter().filter(|n| n.painted_text().is_some()).collect();
    assert_eq!(glyphs.len(), 3);
    let a = cw_scene::metrics::advance(cw_scene::Typeface::default(), false, 'a', 16);
    assert_eq!(glyphs[1].bounds.x, ((a + 32) / 64) as i32 + 4);
}

#[test]
fn selection_highlights_the_selected_bytes() {
    let mut doc = Document::new();
    let html = doc.create_element("html", vec![]);
    doc.append(Document::ROOT, html);
    let p = doc.create_element("p", vec![]);
    doc.append(html, p);
    let t = doc.create_text("hello world");
    doc.append(p, t);
    let mut styles = StyleSet::new();
    set(&mut styles, html.0, |_| {});
    set(&mut styles, p.0, |_| {});
    let mut root = boxf(html.0, rect(0, 0, 200, 100));
    root.children = vec![line(rect(0, 0, 200, 20), vec![text_run(p.0, Some(t.0), "hello world", rect(0, 0, 100, 20), 16)])];
    let ctx = PaintContext { selection: Some(Selection { start: (t, 0), end: (t, 5) }), ..Default::default() };
    let scene = paint(&doc, &styles, &tree(root), viewport(200, 100), &ctx);
    let sel = scene.nodes.iter().find(|n| decode(n.id).2 == parts::SELECTION).expect("selection box");
    let w = cw_scene::metrics::text_width(cw_scene::Typeface::default(), false, "hello", 16);
    assert_eq!(sel.bounds.x, 0);
    assert!((sel.bounds.width as i32 - w as i32).abs() <= 1);
    assert!(matches!(sel.primitive, Primitive::Box { fill: text::SELECTION, .. }));
    let order: Vec<u32> = scene.nodes.iter().map(|n| decode(n.id).2).collect();
    assert!(order.iter().position(|p| *p == parts::SELECTION) < order.iter().position(|p| *p == parts::TEXT));
}

fn form_doc() -> (Document, Vec<NodeId>) {
    let mut d = Document::new();
    let html = d.create_element("html", vec![]);
    d.append(Document::ROOT, html);
    let body = d.create_element("body", vec![]);
    d.append(html, body);
    let a = d.create_element("a", vec![Attribute { name: "href".into(), value: "/x".into() }]);
    d.append(body, a);
    let at = d.create_text("  Go  home ");
    d.append(a, at);
    let h2 = d.create_element("h2", vec![]);
    d.append(body, h2);
    let ht = d.create_text("Title");
    d.append(h2, ht);
    let label = d.create_element("label", vec![Attribute { name: "for".into(), value: "q".into() }]);
    d.append(body, label);
    let lt = d.create_text("Search");
    d.append(label, lt);
    let input = d.create_element("input", vec![Attribute { name: "id".into(), value: "q".into() }, Attribute { name: "value".into(), value: "cats".into() }]);
    d.append(body, input);
    let button = d.create_element("button", vec![Attribute { name: "disabled".into(), value: "".into() }, Attribute { name: "aria-label".into(), value: "Send".into() }]);
    d.append(body, button);
    let img = d.create_element("img", vec![Attribute { name: "alt".into(), value: "A cat".into() }, Attribute { name: "src".into(), value: "cat.png".into() }]);
    d.append(body, img);
    let cb = d.create_element("input", vec![Attribute { name: "type".into(), value: "checkbox".into() }, Attribute { name: "checked".into(), value: "".into() }]);
    d.append(body, cb);
    let nav = d.create_element("nav", vec![Attribute { name: "role".into(), value: "menubar".into() }]);
    d.append(body, nav);
    let div = d.create_element("div", vec![]);
    d.append(body, div);
    (d, vec![html, body, a, h2, input, button, img, cb, nav, div])
}

#[test]
fn semantics_roles_labels_and_interactions_follow_the_dom() {
    let (doc, n) = form_doc();
    let [html, body, a, h2, input, button, img, cb, nav, div] = n[..] else { unreachable!() };
    assert_eq!(semantics::role_of(&doc, a).as_deref(), Some("link"));
    assert_eq!(semantics::label_of(&doc, &semantics::Tables::build(&doc), a), "Go home");
    assert_eq!(semantics::role_of(&doc, h2).as_deref(), Some("heading"));
    assert_eq!(semantics::label_of(&doc, &semantics::Tables::build(&doc), h2), "h2: Title");
    assert_eq!(semantics::role_of(&doc, input).as_deref(), Some("textbox"));
    assert_eq!(semantics::label_of(&doc, &semantics::Tables::build(&doc), input), "Search");
    assert_eq!(semantics::role_of(&doc, button).as_deref(), Some("button"));
    assert_eq!(semantics::label_of(&doc, &semantics::Tables::build(&doc), button), "Send");
    assert!(semantics::is_disabled(&doc, button));
    assert_eq!(semantics::role_of(&doc, img).as_deref(), Some("img"));
    assert_eq!(semantics::label_of(&doc, &semantics::Tables::build(&doc), img), "A cat");
    assert_eq!(semantics::role_of(&doc, cb).as_deref(), Some("checkbox"));
    assert_eq!(semantics::role_of(&doc, nav).as_deref(), Some("menubar"), "role attribute wins");
    assert_eq!(semantics::role_of(&doc, div), None);
    assert!(semantics::is_focusable(&doc, a));
    assert!(!semantics::is_focusable(&doc, div));
    // Interaction ids: the id attribute, else a path.
    assert_eq!(semantics::interaction_id(&doc, input), "q");
    assert_eq!(semantics::interaction_id(&doc, a), "/html[1]/body[1]/a[1]");
    assert_eq!(semantics::interaction_id(&doc, cb), "/html[1]/body[1]/input[2]");
    let _ = (html, body);
}

#[test]
fn regions_carry_semantics_focus_and_control_state() {
    let (doc, n) = form_doc();
    let [html, body, a, _h2, input, button, _img, cb, _nav, _div] = n[..] else { unreachable!() };
    let mut styles = StyleSet::new();
    for id in &n {
        set(&mut styles, id.0, |_| {});
    }
    let mut root = boxf(html.0, rect(0, 0, 300, 300));
    let mut b = boxf(body.0, rect(0, 0, 300, 300));
    let link = inline_box(a.0, rect(0, 0, 60, 20), true, true);
    let mut field = Fragment::new(FragmentKind::Box { source: StyleSource::Element(input), padding: Edges::uniform(au(2)), border: Edges::uniform(au(1)), replaced: Some(Replaced::Control(ControlKind::TextInput)), scroll: None, baseline: Some(au(16)) }, rect(0, 30, 150, 24));
    field.children = vec![];
    let btn = Fragment::new(FragmentKind::Box { source: StyleSource::Element(button), padding: Edges::ZERO, border: Edges::ZERO, replaced: Some(Replaced::Control(ControlKind::Button)), scroll: None, baseline: None }, rect(0, 60, 80, 24));
    let check = Fragment::new(FragmentKind::Box { source: StyleSource::Element(cb), padding: Edges::ZERO, border: Edges::ZERO, replaced: Some(Replaced::Control(ControlKind::Checkbox)), scroll: None, baseline: None }, rect(0, 90, 13, 13));
    b.children = vec![line(rect(0, 0, 300, 20), vec![link]), field, btn, check];
    root.children = vec![b];
    let mut ctx = PaintContext { focused: Some(input), ..Default::default() };
    ctx.values.insert(input, "dogs".into());
    let scene = paint(&doc, &styles, &tree(root), viewport(300, 300), &ctx);
    let by_action = |id: &str| scene.nodes.iter().find(|n| n.interaction.as_deref() == Some(id)).unwrap();
    let link_node = by_action("/html[1]/body[1]/a[1]");
    assert_eq!(link_node.semantic.as_ref().unwrap().role, "link");
    assert!(link_node.semantic.as_ref().unwrap().focusable);
    assert!(matches!(link_node.primitive, Primitive::Region));
    let field_node = by_action("q");
    let sem = field_node.semantic.as_ref().unwrap();
    assert_eq!(sem.role, "textbox");
    assert_eq!(sem.value.as_deref(), Some("dogs"), "live value overrides the attribute");
    assert!(field_node.state.unwrap().focused);
    let btn_node = by_action("/html[1]/body[1]/button[1]");
    assert!(btn_node.semantic.as_ref().unwrap().disabled);
    assert!(!btn_node.accepts_input());
    let cb_node = by_action("/html[1]/body[1]/input[2]");
    assert_eq!(cb_node.state.unwrap().checked, Some(true));
    // The check mark is a path; the field shows its value and a caret.
    assert!(scene.nodes.iter().any(|n| decode(n.id).0 == cb.0 && matches!(n.primitive, Primitive::Path { .. })));
    assert!(scene.nodes.iter().any(|n| n.painted_text() == Some("dogs")));
    let caret = scene.nodes.iter().find(|n| decode(n.id).2 == parts::CARET).expect("caret");
    let w = cw_scene::metrics::text_width(cw_scene::Typeface::default(), false, "dogs", 16);
    assert_eq!(caret.bounds.x, 3 + w as i32);
    let focus = scene.focus.as_ref().expect("focus");
    assert_eq!(focus.interaction.as_deref(), Some("q"));
    assert_eq!(focus.keyboard.route, "page");
    assert!(focus.keyboard.text_entry);
    assert_eq!(focus.caret.unwrap().bounds, caret.bounds);
    // The accessibility view merges by interaction.
    let ax = scene.accessibility();
    assert!(ax.iter().any(|a| a.id == "q" && a.focused && a.role == "textbox"));
}

#[test]
fn hit_testing_honours_stacking_clips_and_pointer_events() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| {
        s.overflow_x = Overflow::Hidden;
        s.overflow_y = Overflow::Hidden;
    });
    set(&mut styles, 3, |_| {});
    set(&mut styles, 4, |s| s.pointer_events = PointerEvents::None);
    set(&mut styles, 5, |_| {});
    let mut root = boxf(1, rect(0, 0, 200, 200));
    let mut clipper = boxf(2, rect(0, 0, 50, 50));
    clipper.children = vec![boxf(3, rect(0, 0, 100, 100))];
    let inert = boxf(4, rect(100, 100, 50, 50));
    let mut top = boxf(5, rect(0, 0, 30, 30));
    top.establishes_stacking_context = true;
    top.is_positioned = true;
    top.z_index = 5;
    root.children = vec![clipper, inert, top];
    let t = tree(root);
    assert_eq!(hit::hit_test(&t, &styles, 40, 40), Some(NodeId(3)));
    assert_eq!(hit::hit_test(&t, &styles, 75, 25), Some(NodeId(1)), "clipped child is not hit beyond its parent");
    assert_eq!(hit::hit_test(&t, &styles, 120, 120), Some(NodeId(1)), "pointer-events: none falls through");
    assert_eq!(hit::hit_test(&t, &styles, 10, 10), Some(NodeId(5)), "topmost stacking context wins");
    assert_eq!(hit::hit_test(&t, &styles, 500, 500), None);
}

#[test]
fn text_runs_hit_their_element() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |_| {});
    let mut root = boxf(1, rect(0, 0, 200, 200));
    root.children = vec![line(rect(0, 0, 200, 20), vec![text_run(2, Some(9), "hello", rect(10, 0, 50, 20), 16)])];
    let t = tree(root);
    assert_eq!(hit::hit_test(&t, &styles, 20, 10), Some(NodeId(2)));
    assert_eq!(hit::hit_test(&t, &styles, 100, 10), Some(NodeId(1)));
}

#[test]
fn ids_are_stable_across_paints_and_unrelated_edits() {
    let mut styles = StyleSet::new();
    for n in 1..=4 {
        set(&mut styles, n, |s| s.background_color = RED);
    }
    let build = |extra: bool| {
        let mut root = boxf(1, rect(0, 0, 200, 200));
        let mut kids = vec![boxf(2, rect(0, 0, 50, 50)), boxf(3, rect(0, 50, 50, 50))];
        if extra {
            kids.insert(0, boxf(4, rect(0, 100, 50, 50)));
        }
        root.children = kids;
        tree(root)
    };
    let a = paint_no_doc(&styles, &build(false));
    let b = paint_no_doc(&styles, &build(false));
    assert_eq!(a.nodes.iter().map(|n| n.id).collect::<Vec<_>>(), b.nodes.iter().map(|n| n.id).collect::<Vec<_>>());
    let c = paint_no_doc(&styles, &build(true));
    let ids_a: std::collections::BTreeSet<u64> = a.nodes.iter().map(|n| n.id).collect();
    let ids_c: std::collections::BTreeSet<u64> = c.nodes.iter().map(|n| n.id).collect();
    assert!(ids_a.is_subset(&ids_c), "inserting a sibling first keeps every existing id");
    assert_eq!(ids_c.len(), ids_a.len() + 1);
    // Ids decode to their node and part.
    assert_eq!(decode(scene_id(0, NodeId(3), 0, parts::BACKGROUND)), (3, 0, parts::BACKGROUND));
    assert_eq!(scene_id(1000, NodeId(3), 0, 1), 1000 + scene_id(0, NodeId(3), 0, 1));
    // Two fragments of one element get distinct ordinals.
    let mut root = boxf(1, rect(0, 0, 200, 200));
    root.children = vec![line(rect(0, 0, 200, 20), vec![inline_box(2, rect(0, 0, 50, 20), true, false)]), line(rect(0, 20, 200, 20), vec![inline_box(2, rect(0, 0, 50, 20), false, true)])];
    let s = paint_no_doc(&styles, &tree(root));
    let ords: Vec<u32> = s.nodes.iter().filter(|n| decode(n.id).0 == 2 && decode(n.id).2 == parts::BACKGROUND).map(|n| decode(n.id).1).collect();
    assert_eq!(ords, vec![0, 1]);
    s.validate().unwrap();
}

#[test]
fn opacity_groups_clips_and_visibility() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| {
        s.opacity = 128;
        s.background_color = RED;
        s.overflow_y = Overflow::Hidden;
    });
    set(&mut styles, 3, |s| {
        s.background_color = BLUE;
        s.visibility = Visibility::Hidden;
    });
    set(&mut styles, 4, |s| {
        s.background_color = BLUE;
        s.visibility = Visibility::Visible;
    });
    let mut root = boxf(1, rect(0, 0, 200, 200));
    let mut g = boxf(2, rect(10, 10, 100, 100));
    let mut hidden = boxf(3, rect(0, 0, 150, 150));
    hidden.children = vec![boxf(4, rect(5, 5, 20, 20))];
    g.children = vec![hidden];
    root.children = vec![g];
    let scene = paint_no_doc(&styles, &tree(root));
    let bg = backgrounds(&scene);
    assert!(bg.contains(&2) && bg.contains(&4) && !bg.contains(&3), "hidden box paints nothing but its visible child does");
    let group_bg = scene.nodes.iter().find(|n| decode(n.id) == (2, 0, parts::BACKGROUND)).unwrap();
    assert_eq!(group_bg.opacity, 128);
    let child = scene.nodes.iter().find(|n| decode(n.id) == (4, 0, parts::BACKGROUND)).unwrap();
    assert_eq!(child.opacity, 128, "the group opacity reaches every node of the group");
    assert_eq!(child.clip, Some(cw_scene::Rect::new(10, 10, 100, 100)), "overflow clip is the padding box");
    assert_eq!(group_bg.clip, Some(cw_scene::Rect::new(0, 0, 200, 200)), "a box is not clipped by its own overflow");
}

#[test]
fn transforms_compose_about_the_origin() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| {
        s.background_color = RED;
        s.transform = vec![TransformOp::Translate(LengthPercentage::Length(au(10)), LengthPercentage::Length(au(-5)))];
    });
    set(&mut styles, 3, |s| {
        s.background_color = RED;
        s.transform = vec![TransformOp::Rotate(9000)];
        s.transform_origin = (LengthPercentage::ZERO, LengthPercentage::ZERO);
    });
    set(&mut styles, 4, |s| {
        s.background_color = RED;
        s.transform = vec![TransformOp::Scale(2000, 2000)];
    });
    let mut root = boxf(1, rect(0, 0, 200, 200));
    let mut moved = boxf(2, rect(20, 20, 40, 40));
    moved.children = vec![boxf(4, rect(0, 0, 10, 10))];
    root.children = vec![moved, boxf(3, rect(100, 100, 40, 40))];
    let scene = paint_no_doc(&styles, &tree(root));
    // A transform moves the element's own background and its children alike.
    let moved_bg = scene.nodes.iter().find(|n| decode(n.id) == (2, 0, parts::BACKGROUND)).unwrap();
    assert_eq!((moved_bg.transform.tx, moved_bg.transform.ty), (10, -5));
    assert_eq!((moved_bg.transform.a, moved_bg.transform.d), (1024, 1024));
    assert_eq!(moved_bg.transform.point(20, 20), (30, 15));
    // The child composes its own scale (about its centre, (25, 25)) inside that.
    let child = scene.nodes.iter().find(|n| decode(n.id) == (4, 0, parts::BACKGROUND)).unwrap();
    assert_eq!((child.transform.a, child.transform.d), (2048, 2048));
    assert_eq!(child.transform.point(25, 25), (35, 20), "the centre only translates");
    assert_eq!(child.transform.point(20, 20), (25, 10), "a corner moves out from the centre, then translates");
    // The rotated box's own background rotates about its origin (its top-left here).
    let turned = scene.nodes.iter().find(|n| decode(n.id) == (3, 0, parts::BACKGROUND)).unwrap();
    assert_eq!(turned.transform.point(140, 100), (100, 140));
    // Rotation about (100, 100): the point (140, 100) maps to (100, 140).
    let rot = display_list::transform_matrix(&[TransformOp::Rotate(9000)], crate::geom::Size { width: au(40), height: au(40) }, (100, 100));
    assert_eq!(rot.point(140, 100), (100, 140));
    // A nested scale doubles about the child's centre.
    let sc = display_list::transform_matrix(&[TransformOp::Scale(2000, 2000)], crate::geom::Size { width: au(10), height: au(10) }, (25, 25));
    assert_eq!(sc.point(20, 20), (15, 15));
    assert_eq!(sc.point(30, 30), (35, 35));
}

#[test]
fn scroll_areas_are_registered_for_the_root_and_scroll_containers() {
    let (mut doc, html, body) = simple_doc();
    let div = doc.create_element("div", vec![Attribute { name: "id".into(), value: "list".into() }]);
    doc.append(body, div);
    let item = doc.create_element("p", vec![]);
    doc.append(div, item);
    let mut styles = StyleSet::new();
    set(&mut styles, html.0, |_| {});
    set(&mut styles, body.0, |_| {});
    set(&mut styles, div.0, |s| {
        s.overflow_y = Overflow::Auto;
        s.background_color = RED;
    });
    let mut root = boxf(html.0, rect(0, 0, 200, 200));
    let mut b = boxf(body.0, rect(0, 0, 200, 600));
    let mut list = Fragment::new(
        FragmentKind::Box { source: StyleSource::Element(div), padding: Edges::ZERO, border: Edges::uniform(au(1)), replaced: None, scroll: Some(ScrollInfo { content_width: au(90), content_height: au(400), scroll_x: Au::ZERO, scroll_y: au(30), origin_x: Au::ZERO, origin_y: Au::ZERO, shows_x_bar: false, shows_y_bar: true }), baseline: None },
        rect(10, 10, 100, 100),
    );
    list.children = vec![boxf(item.0, rect(0, 0, 90, 400))];
    set(&mut styles, item.0, |s| s.background_color = BLUE);
    b.children = vec![list];
    root.children = vec![b];
    let mut t = tree(root);
    t.content_height = au(600);
    let mut ctx = PaintContext { scroll: crate::geom::Point { x: Au::ZERO, y: au(50) }, ..Default::default() };
    let scene = paint(&doc, &styles, &t, viewport(200, 200), &ctx);
    assert_eq!(scene.scrolls.len(), 2);
    let page = &scene.scrolls[0];
    assert_eq!(page.target, "pane:page");
    assert_eq!((page.offset, page.extent), (50, 600));
    assert_eq!(page.bounds, cw_scene::Rect::new(0, 0, 200, 200));
    let inner = &scene.scrolls[1];
    assert_eq!(inner.target, "pane:list");
    assert_eq!(inner.bounds, cw_scene::Rect::new(11, 11 - 50, 98, 98));
    assert_eq!((inner.offset, inner.extent), (30, 400));
    // The list's child is shifted by both scrolls and clipped to the padding box.
    let child = scene.nodes.iter().find(|n| decode(n.id) == (item.0, 0, parts::BACKGROUND)).unwrap();
    assert_eq!(child.bounds, cw_scene::Rect::new(10, 10 - 50 - 30, 90, 400), "shifted by both scrolls");
    assert_eq!(child.clip, Some(cw_scene::Rect::new(11, 0, 98, 98 - 39)), "clipped to the padding box and the viewport");
    let list_bg = scene.nodes.iter().find(|n| decode(n.id) == (div.0, 0, parts::BACKGROUND)).unwrap();
    assert_eq!(list_bg.bounds, cw_scene::Rect::new(10, -40, 100, 100));
    // A context override wins over layout's recorded offset.
    ctx.scroll_offsets.insert(div, crate::geom::Point { x: Au::ZERO, y: au(70) });
    let scene = paint(&doc, &styles, &t, viewport(200, 200), &ctx);
    assert_eq!(scene.scrolls[1].offset, 70);
}

#[test]
fn gradients_rasterise_deterministically_and_cache() {
    let g = BackgroundImage::LinearGradient { angle_centi_deg: 18_000, stops: vec![GradientStop { color: RED, position: None }, GradientStop { color: BLUE, position: None }] };
    let img = background::rasterize_gradient_uncached(50, 100, &g);
    assert_eq!((img.width, img.height), (1, 100), "an axis-aligned gradient is a strip");
    assert!(img.rgba[0] > 250 && img.rgba[2] < 5, "top is the start colour: {:?}", &img.rgba[0..4]);
    assert!(img.rgba[396] < 5 && img.rgba[398] > 250, "bottom is the end colour: {:?}", &img.rgba[396..400]);
    let mid = &img.rgba[200..204];
    assert!(mid[0] > 100 && mid[2] > 100);
    let diag = BackgroundImage::LinearGradient { angle_centi_deg: 4500, stops: vec![GradientStop { color: RED, position: Some(LengthPercentage::Percent(2000)) }, GradientStop { color: BLUE, position: Some(LengthPercentage::Percent(8000)) }] };
    let d = background::rasterize_gradient_uncached(20, 20, &diag);
    assert_eq!((d.width, d.height), (20, 20));
    assert_eq!(&d.rgba[(19 * 20) * 4..(19 * 20) * 4 + 4], &[255, 0, 0, 255], "bottom-left corner is the start colour");
    assert_eq!(&d.rgba[19 * 4..19 * 4 + 4], &[0, 0, 255, 255], "top-right corner is the end colour");
    let radial = BackgroundImage::RadialGradient { circle: true, stops: vec![GradientStop { color: RED, position: None }, GradientStop { color: BLUE, position: None }] };
    let r = background::rasterize_gradient_uncached(21, 21, &radial);
    assert_eq!(&r.rgba[(10 * 21 + 10) * 4..(10 * 21 + 10) * 4 + 4], &[255, 0, 0, 255]);
    assert!(r.rgba[2] > 200, "the corner is nearly the end colour");
    // Painted through a box: an Image node the size of the box, cached on reuse.
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    let layer = BackgroundLayer { image: g.clone(), repeat: BackgroundRepeat::NoRepeat, size: BackgroundSize::Auto, position: (LengthPercentage::ZERO, LengthPercentage::ZERO), origin: BackgroundBox::PaddingBox, clip: BackgroundBox::BorderBox, attachment_fixed: false };
    set(&mut styles, 2, |s| s.background = vec![layer.clone()]);
    set(&mut styles, 3, |s| s.background = vec![layer]);
    let mut root = boxf(1, rect(0, 0, 200, 200));
    root.children = vec![boxf(2, rect(0, 0, 50, 100)), boxf(3, rect(100, 0, 50, 100))];
    let t = tree(root);
    let ctx = PaintContext::default();
    let mut p = Painter::new(None, &styles, &t, viewport(200, 200), &ctx);
    p.run();
    assert_eq!(p.gradients.len(), 1, "one raster serves both boxes");
    let scene = p.finish();
    let imgs: Vec<_> = scene.nodes.iter().filter(|n| matches!(n.primitive, Primitive::Image { .. })).collect();
    assert_eq!(imgs.len(), 2);
    assert_eq!(imgs[0].bounds, cw_scene::Rect::new(0, 0, 50, 100));
    assert_eq!(imgs[1].bounds, cw_scene::Rect::new(100, 0, 50, 100));
    scene.validate().unwrap();
}

#[test]
fn background_images_tile_scale_and_fix() {
    let mut images = ImageMap::default();
    images.0.insert("tile.png".into(), RgbaImage::solid(10, 10, RED));
    let ctx = PaintContext::new(&images);
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    let base = BackgroundLayer { image: BackgroundImage::Url("tile.png".into()), repeat: BackgroundRepeat::Repeat, size: BackgroundSize::Auto, position: (LengthPercentage::ZERO, LengthPercentage::ZERO), origin: BackgroundBox::PaddingBox, clip: BackgroundBox::PaddingBox, attachment_fixed: false };
    set(&mut styles, 2, |s| s.background = vec![base.clone()]);
    set(&mut styles, 3, |s| s.background = vec![BackgroundLayer { repeat: BackgroundRepeat::NoRepeat, size: BackgroundSize::Cover, ..base.clone() }]);
    set(&mut styles, 4, |s| s.background = vec![BackgroundLayer { attachment_fixed: true, ..base.clone() }]);
    set(&mut styles, 5, |s| s.background = vec![BackgroundLayer { repeat: BackgroundRepeat::RepeatX, position: (LengthPercentage::Percent(5000), LengthPercentage::Percent(10_000)), ..base.clone() }]);
    let mut root = boxf(1, rect(0, 0, 200, 200));
    root.children = vec![boxf(2, rect(0, 0, 25, 25)), boxf(3, rect(50, 0, 40, 20)), boxf(4, rect(0, 100, 30, 30)), boxf(5, rect(100, 100, 45, 30))];
    let scene = paint_fragments(&styles, &tree(root), viewport(200, 200), &ctx);
    let of = |n: u32| scene.nodes.iter().filter(move |x| decode(x.id).0 == n && matches!(x.primitive, Primitive::Image { .. })).collect::<Vec<_>>();
    assert_eq!(of(2).len(), 9, "3 x 3 tiles cover a 25 px box");
    assert!(of(2).iter().all(|n| n.clip == Some(cw_scene::Rect::new(0, 0, 25, 25))));
    let cover = of(3);
    assert_eq!(cover.len(), 1);
    assert_eq!(cover[0].bounds, cw_scene::Rect::new(50, 0, 40, 40), "cover scales to the larger ratio");
    assert!(matches!(cover[0].primitive, Primitive::Image { width: 40, height: 40, .. }), "resampled here, not by the renderer");
    let fixed = of(4);
    assert_eq!(fixed.len(), 9);
    assert!(fixed.iter().all(|n| n.bounds.x % 10 == 0 && n.bounds.y % 10 == 0), "fixed layers tile from the viewport origin");
    assert!(fixed.iter().all(|n| n.clip == Some(cw_scene::Rect::new(0, 100, 30, 30))));
    let row = of(5);
    assert_eq!(row.len(), 4, "tiles at 108, 118, 128, 138 cross the 100..145 box");
    assert!(row.iter().all(|n| n.bounds.y == 120), "bottom-aligned by the 100% position");
    let resampled = background::resample(&RgbaImage::solid(2, 2, BLUE), 3, 5);
    assert_eq!((resampled.width, resampled.height), (3, 5));
    assert_eq!(resampled.rgba.len(), 60);
}

#[test]
fn replaced_images_and_placeholders() {
    let mut images = ImageMap::default();
    images.0.insert("a.png".into(), RgbaImage::solid(20, 10, RED));
    let ctx = PaintContext::new(&images);
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| s.object_fit = ObjectFit::Contain);
    set(&mut styles, 3, |_| {});
    set(&mut styles, 4, |_| {});
    set(&mut styles, 5, |s| s.object_fit = ObjectFit::Cover);
    let img = |n: u32, src: &str, r: Rect| Fragment::new(FragmentKind::Box { source: StyleSource::Element(NodeId(n)), padding: Edges::ZERO, border: Edges::ZERO, replaced: Some(Replaced::Image { src: src.into(), alt: "photo".into() }), scroll: None, baseline: None }, r);
    let mut root = boxf(1, rect(0, 0, 300, 300));
    root.children = vec![
        img(2, "a.png", rect(0, 0, 100, 100)),
        img(3, "missing.png", rect(0, 100, 100, 40)),
        Fragment::new(FragmentKind::Box { source: StyleSource::Element(NodeId(4)), padding: Edges::ZERO, border: Edges::ZERO, replaced: Some(Replaced::Placeholder("iframe".into())), scroll: None, baseline: None }, rect(100, 100, 100, 60)),
        img(5, "a.png", rect(200, 0, 50, 50)),
    ];
    let scene = paint_fragments(&styles, &tree(root), viewport(300, 300), &ctx);
    let contain = scene.nodes.iter().find(|n| decode(n.id).0 == 2 && matches!(n.primitive, Primitive::Image { .. })).unwrap();
    assert_eq!(contain.bounds, cw_scene::Rect::new(0, 25, 100, 50));
    let missing = scene.nodes.iter().find(|n| decode(n.id) == (3, 0, parts::CONTENT)).unwrap();
    assert!(matches!(missing.primitive, Primitive::Box { border: Some(_), border_width: 1, .. }));
    assert!(scene.nodes.iter().any(|n| decode(n.id).0 == 3 && n.painted_text() == Some("photo")));
    assert!(scene.nodes.iter().any(|n| decode(n.id).0 == 4 && n.painted_text() == Some("iframe")));
    let cover = scene.nodes.iter().find(|n| decode(n.id).0 == 5 && matches!(n.primitive, Primitive::Image { .. })).unwrap();
    assert_eq!(cover.bounds, cw_scene::Rect::new(175, 0, 100, 50));
    assert_eq!(cover.clip, Some(cw_scene::Rect::new(200, 0, 50, 50)));
}

#[test]
fn controls_draw_from_dom_state() {
    let mut d = Document::new();
    let html = d.create_element("html", vec![]);
    d.append(Document::ROOT, html);
    let select = d.create_element("select", vec![]);
    d.append(html, select);
    let o1 = d.create_element("option", vec![]);
    d.append(select, o1);
    let t1 = d.create_text("One");
    d.append(o1, t1);
    let o2 = d.create_element("option", vec![Attribute { name: "selected".into(), value: "".into() }]);
    d.append(select, o2);
    let t2 = d.create_text("Two");
    d.append(o2, t2);
    let range = d.create_element("input", vec![Attribute { name: "type".into(), value: "range".into() }, Attribute { name: "value".into(), value: "25".into() }]);
    d.append(html, range);
    let radio = d.create_element("input", vec![Attribute { name: "type".into(), value: "radio".into() }, Attribute { name: "checked".into(), value: "".into() }, Attribute { name: "disabled".into(), value: "".into() }]);
    d.append(html, radio);
    let ta = d.create_element("textarea", vec![]);
    d.append(html, ta);
    let tt = d.create_text("line one\nline two");
    d.append(ta, tt);
    let pw = d.create_element("input", vec![Attribute { name: "type".into(), value: "password".into() }, Attribute { name: "value".into(), value: "abc".into() }]);
    d.append(html, pw);
    let empty = d.create_element("input", vec![Attribute { name: "placeholder".into(), value: "Type here".into() }]);
    d.append(html, empty);
    let submit = d.create_element("input", vec![Attribute { name: "type".into(), value: "submit".into() }]);
    d.append(html, submit);
    let mut styles = StyleSet::new();
    for n in [html, select, range, radio, ta, pw, empty, submit] {
        set(&mut styles, n.0, |_| {});
    }
    let ctl = |n: NodeId, k: ControlKind, r: Rect| Fragment::new(FragmentKind::Box { source: StyleSource::Element(n), padding: Edges::ZERO, border: Edges::ZERO, replaced: Some(Replaced::Control(k)), scroll: None, baseline: None }, r);
    let mut root = boxf(html.0, rect(0, 0, 300, 300));
    root.children = vec![
        ctl(select, ControlKind::Select, rect(0, 0, 100, 24)),
        ctl(range, ControlKind::Range, rect(0, 30, 112, 20)),
        ctl(radio, ControlKind::Radio, rect(0, 60, 13, 13)),
        ctl(ta, ControlKind::TextArea, rect(0, 80, 200, 60)),
        ctl(pw, ControlKind::Password, rect(0, 150, 100, 24)),
        ctl(empty, ControlKind::TextInput, rect(0, 180, 100, 24)),
        ctl(submit, ControlKind::Submit, rect(0, 210, 100, 24)),
    ];
    let scene = paint(&d, &styles, &tree(root), viewport(300, 300), &PaintContext::default());
    let texts: Vec<&str> = scene.nodes.iter().filter_map(|n| n.painted_text()).collect();
    assert!(texts.contains(&"Two"), "the selected option: {texts:?}");
    assert!(texts.contains(&"line one") && texts.contains(&"line two"));
    assert!(texts.contains(&"\u{2022}\u{2022}\u{2022}"));
    assert!(texts.contains(&"Type here"));
    assert!(texts.contains(&"Submit"));
    let chevron = scene.nodes.iter().find(|n| decode(n.id) == (select.0, 0, parts::CONTENT_GLYPH)).unwrap();
    assert!(matches!(chevron.primitive, Primitive::Path { .. }));
    let thumb = scene.nodes.iter().find(|n| decode(n.id) == (range.0, 0, parts::CONTENT_GLYPH)).unwrap();
    assert_eq!(thumb.bounds, cw_scene::Rect::new(25, 34, 12, 12), "25% along a 100 px span");
    let dot = scene.nodes.iter().find(|n| decode(n.id) == (radio.0, 0, parts::CONTENT_GLYPH)).unwrap();
    assert!(matches!(dot.primitive, Primitive::RoundedBox { fill: Color(109, 109, 109, 255), .. }), "disabled radio dot is grey");
    let placeholder = scene.nodes.iter().find(|n| n.painted_text() == Some("Type here")).unwrap();
    assert!(matches!(&placeholder.primitive, Primitive::UiText { color: Color(117, 117, 117, 255), .. }));
    assert!(scene.nodes.iter().all(|n| decode(n.id).2 != parts::CARET), "no caret without focus");
    scene.validate().unwrap();
}

#[test]
fn inline_fragments_open_their_cut_ends() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| {
        s.border = Sides::uniform(side(BorderStyle::Solid, 1, RED));
        s.background_color = BLUE;
    });
    let mut root = boxf(1, rect(0, 0, 200, 100));
    let mut a = inline_box(2, rect(0, 0, 50, 20), true, false);
    a.kind = FragmentKind::InlineBox { source: StyleSource::Element(NodeId(2)), padding: Edges::ZERO, border: Edges { top: au(1), right: Au::ZERO, bottom: au(1), left: au(1) }, first: true, last: false };
    let mut b = inline_box(2, rect(0, 0, 50, 20), false, true);
    b.kind = FragmentKind::InlineBox { source: StyleSource::Element(NodeId(2)), padding: Edges::ZERO, border: Edges { top: au(1), right: au(1), bottom: au(1), left: Au::ZERO }, first: false, last: true };
    root.children = vec![line(rect(0, 0, 200, 20), vec![a]), line(rect(0, 20, 200, 20), vec![b])];
    let scene = paint_no_doc(&styles, &tree(root));
    let parts_of = |ord: u32| scene.nodes.iter().filter(|n| decode(n.id).0 == 2 && decode(n.id).1 == ord).map(|n| decode(n.id).2).collect::<Vec<_>>();
    assert_eq!(parts_of(0), vec![parts::BACKGROUND, parts::BORDER_TOP, parts::BORDER_BOTTOM, parts::BORDER_LEFT]);
    assert_eq!(parts_of(1), vec![parts::BACKGROUND, parts::BORDER_TOP, parts::BORDER_RIGHT, parts::BORDER_BOTTOM]);
}

#[test]
fn fixed_boxes_ignore_the_scroll_offset() {
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| {
        s.position = Position::Fixed;
        s.background_color = RED;
    });
    set(&mut styles, 3, |s| s.background_color = BLUE);
    let mut root = boxf(1, rect(0, 0, 200, 1000));
    let mut fixed = boxf(2, rect(0, 0, 50, 50));
    fixed.is_positioned = true;
    fixed.establishes_stacking_context = true;
    root.children = vec![boxf(3, rect(0, 100, 50, 50)), fixed];
    let ctx = PaintContext { scroll: crate::geom::Point { x: Au::ZERO, y: au(300) }, ..Default::default() };
    let scene = paint_fragments(&styles, &tree(root), viewport(200, 200), &ctx);
    let f = scene.nodes.iter().find(|n| decode(n.id) == (2, 0, parts::BACKGROUND)).unwrap();
    assert_eq!(f.bounds, cw_scene::Rect::new(0, 0, 50, 50));
    let s = scene.nodes.iter().find(|n| decode(n.id) == (3, 0, parts::BACKGROUND)).unwrap();
    assert_eq!(s.bounds, cw_scene::Rect::new(0, -200, 50, 50));
}

/// A composite fixture through every module, digested. The digest pins the exact
/// scene: ids, order, bounds and primitives. Update it when a deliberate change to
/// paint changes the output, and only then.
#[test]
fn golden_composite_digest() {
    let (doc, n) = form_doc();
    let [html, body, a, h2, input, button, img, cb, nav, div] = n[..] else { unreachable!() };
    let mut styles = StyleSet::new();
    set(&mut styles, html.0, |_| {});
    set(&mut styles, body.0, |s| {
        s.background_color = Color(250, 250, 250, 255);
        s.font.size = au(14);
    });
    set(&mut styles, a.0, |s| {
        s.color = Color(0, 0, 238, 255);
        s.text_decoration.underline = true;
        s.font.size = au(14);
    });
    set(&mut styles, h2.0, |s| {
        s.font.size = au(24);
        s.font.weight = 700;
        s.text_shadow = vec![TextShadow { offset_x: au(1), offset_y: au(1), blur: Au::ZERO, color: Color(0, 0, 0, 80) }];
    });
    set(&mut styles, input.0, |s| {
        s.border = Sides::uniform(side(BorderStyle::Solid, 1, Color(118, 118, 118, 255)));
        s.background_color = Color::WHITE;
        s.border_radius = Corners { top_left: (LengthPercentage::Length(au(3)), LengthPercentage::Length(au(3))), top_right: (LengthPercentage::Length(au(3)), LengthPercentage::Length(au(3))), bottom_right: (LengthPercentage::Length(au(3)), LengthPercentage::Length(au(3))), bottom_left: (LengthPercentage::Length(au(3)), LengthPercentage::Length(au(3))) };
    });
    set(&mut styles, button.0, |s| {
        s.background_color = Color(239, 239, 239, 255);
        s.border = Sides::uniform(side(BorderStyle::Outset, 2, Color(200, 200, 200, 255)));
    });
    set(&mut styles, img.0, |_| {});
    set(&mut styles, cb.0, |_| {});
    set(&mut styles, nav.0, |s| {
        s.background = vec![BackgroundLayer { image: BackgroundImage::LinearGradient { angle_centi_deg: 9000, stops: vec![GradientStop { color: RED, position: None }, GradientStop { color: BLUE, position: None }] }, repeat: BackgroundRepeat::NoRepeat, size: BackgroundSize::Auto, position: (LengthPercentage::ZERO, LengthPercentage::ZERO), origin: BackgroundBox::PaddingBox, clip: BackgroundBox::BorderBox, attachment_fixed: false }];
        s.opacity = 200;
    });
    set(&mut styles, div.0, |s| {
        s.position = Position::Absolute;
        s.z_index = ZIndex::Int(2);
        s.background_color = Color(0, 128, 0, 255);
        s.box_shadow = vec![BoxShadow { offset_x: Au::ZERO, offset_y: au(2), blur: au(4), spread: Au::ZERO, color: Color(0, 0, 0, 60), inset: false }];
        s.transform = vec![TransformOp::Rotate(1500)];
    });
    let ctl = |n: NodeId, k: ControlKind, r: Rect, b: Edges| Fragment::new(FragmentKind::Box { source: StyleSource::Element(n), padding: Edges::uniform(au(2)), border: b, replaced: Some(Replaced::Control(k)), scroll: None, baseline: Some(au(15)) }, r);
    let mut root = boxf(html.0, rect(0, 0, 320, 240));
    let mut b = boxf(body.0, rect(8, 8, 304, 224));
    let mut link = inline_box(a.0, rect(0, 0, 60, 18), true, true);
    link.children = vec![text_run(a.0, None, "Go home", rect(0, 0, 60, 18), 14)];
    let mut heading = boxf(h2.0, rect(0, 20, 304, 30));
    heading.children = vec![line(rect(0, 0, 304, 30), vec![text_run(h2.0, None, "Title", rect(0, 0, 70, 30), 24)])];
    let field = ctl(input, ControlKind::TextInput, rect(0, 60, 150, 24), Edges::uniform(au(1)));
    let btn = ctl(button, ControlKind::Button, rect(160, 60, 80, 24), Edges::uniform(au(2)));
    let picture = Fragment::new(FragmentKind::Box { source: StyleSource::Element(img), padding: Edges::ZERO, border: Edges::ZERO, replaced: Some(Replaced::Image { src: "cat.png".into(), alt: "A cat".into() }), scroll: None, baseline: None }, rect(0, 90, 60, 40));
    let check = ctl(cb, ControlKind::Checkbox, rect(70, 100, 13, 13), Edges::ZERO);
    let menu = boxf(nav.0, rect(0, 140, 304, 30));
    let mut popup = boxf(div.0, rect(200, 100, 80, 60));
    popup.is_positioned = true;
    popup.establishes_stacking_context = true;
    popup.z_index = 2;
    b.children = vec![line(rect(0, 0, 304, 18), vec![link]), heading, field, btn, picture, check, menu, popup];
    root.children = vec![b];
    let ctx = PaintContext { focused: Some(input), ..Default::default() };
    let mut scene = paint(&doc, &styles, &tree(root), viewport(320, 240), &ctx);
    scene.validate().unwrap();
    scene.stamp();
    let empty = {
        let mut s = paint(&doc, &styles, &tree(boxf(html.0, rect(0, 0, 320, 240))), viewport(320, 240), &ctx);
        s.stamp();
        s.digest
    };
    assert_ne!(scene.digest, empty);
    // Painting order summary, then the digest.
    let summary: Vec<(u32, u32, u32)> = scene.nodes.iter().map(|n| decode(n.id)).collect();
    let expected_order = [
        (input.0, 0, parts::REGION),
        (input.0, 0, parts::BACKGROUND),
        (input.0, 0, parts::BORDER_TOP),
        (button.0, 0, parts::REGION),
        (button.0, 0, parts::BACKGROUND),
    ];
    for e in expected_order {
        assert!(summary.contains(&e), "{e:?} missing from {summary:?}");
    }
    assert_eq!(scene.background, Color(250, 250, 250, 255));
    assert_eq!(scene.digest, GOLDEN_DIGEST, "scene digest changed: {}", serde_json::to_string(&scene.nodes.iter().map(|n| (n.id, n.bounds, n.z)).collect::<Vec<_>>()).unwrap());
}

// Changed when the rotated box's own background started to carry its transform and
// the checkbox tick's path points became relative to its bounds (both were bugs).
const GOLDEN_DIGEST: u64 = 11_624_232_690_043_090_657;

#[test]
fn the_root_scroll_offset_is_applied_once() {
    // Layout records the document's scroll offset on the root fragment and the host
    // passes the same offset in `PaintContext::scroll`; the page must move by it
    // once, not twice (it used to: a page scrolled 100px was painted 200px up).
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |s| s.background_color = RED);
    let mut root = Fragment::new(
        FragmentKind::Box {
            source: StyleSource::Anonymous(Document::ROOT),
            padding: Edges::ZERO,
            border: Edges::ZERO,
            replaced: None,
            scroll: Some(ScrollInfo { content_width: au(200), content_height: au(1000), scroll_x: Au::ZERO, scroll_y: au(100), origin_x: Au::ZERO, origin_y: Au::ZERO, shows_x_bar: false, shows_y_bar: false }),
            baseline: None,
        },
        rect(0, 0, 200, 200),
    );
    root.establishes_stacking_context = true;
    root.children = vec![boxf(1, rect(0, 150, 50, 20))];
    let t = FragmentTree { root, content_width: au(200), content_height: au(1000), viewport_width: au(200), viewport_height: au(200) };
    let ctx = PaintContext { scroll: crate::geom::Point { x: Au::ZERO, y: au(100) }, ..Default::default() };
    let scene = paint_fragments(&styles, &t, viewport(200, 200), &ctx);
    let bg = scene.nodes.iter().find(|n| decode(n.id).0 == 1 && decode(n.id).2 == parts::BACKGROUND).expect("background");
    assert_eq!(bg.bounds.y, 50, "150px down the page, scrolled by 100px");
}

#[test]
fn a_positioned_box_with_z_auto_hands_its_positioned_children_to_the_enclosing_context() {
    // root(1) > P(2, positioned, z auto) > A(3, positioned) and N(4, z -1); then
    // Q(5, positioned) after P. Without a document the layers keep fragment order:
    // N paints first (layer 2 of the root context, not of P), then P, A, Q.
    let mut styles = StyleSet::new();
    for n in 1..=5 {
        set(&mut styles, n, |s| s.background_color = RED);
    }
    let mut root = boxf(1, rect(0, 0, 100, 100));
    let mut p = boxf(2, rect(0, 0, 50, 50));
    p.is_positioned = true;
    let mut a = boxf(3, rect(0, 0, 10, 10));
    a.is_positioned = true;
    let mut n = boxf(4, rect(0, 0, 10, 10));
    n.is_positioned = true;
    n.establishes_stacking_context = true;
    n.z_index = -1;
    p.children = vec![a, n];
    let mut q = boxf(5, rect(0, 0, 10, 10));
    q.is_positioned = true;
    root.children = vec![p, q];
    let scene = paint_no_doc(&styles, &tree(root));
    assert_eq!(backgrounds(&scene), vec![1, 4, 2, 3, 5]);
}

#[test]
fn path_points_are_relative_to_the_node_bounds() {
    // The scene's `Path` points are offsets from the node's bounds. A ring with four
    // differently coloured sides (one stroke per side) and a rounded background with
    // unequal radii used to be emitted in absolute coordinates, which drew them
    // displaced by the box's own origin: off the box entirely for any box not at 0,0.
    let mut styles = StyleSet::new();
    set(&mut styles, 1, |_| {});
    set(&mut styles, 2, |s| {
        let c = |r, g, b| BorderSide { width: au(3), style: BorderStyle::Solid, color: Color(r, g, b, 255) };
        s.border = Sides { top: c(66, 133, 244), right: c(234, 67, 53), bottom: c(251, 188, 5), left: c(52, 168, 83) };
        let half = (LengthPercentage::Percent(5000), LengthPercentage::Percent(5000));
        s.border_radius = Corners { top_left: half, top_right: half, bottom_right: half, bottom_left: half };
    });
    set(&mut styles, 3, |s| {
        s.background_color = BLUE;
        let r = |n| (LengthPercentage::Length(au(n)), LengthPercentage::Length(au(n)));
        s.border_radius = Corners { top_left: r(0), top_right: r(0), bottom_right: r(8), bottom_left: r(8) };
    });
    let mut root = boxf(1, rect(0, 0, 400, 300));
    root.children = vec![bordered(2, rect(100, 50, 22, 22), Edges::uniform(au(3))), boxf(3, rect(200, 150, 40, 20))];
    let scene = paint_no_doc(&styles, &tree(root));
    let paths: Vec<_> = scene.nodes.iter().filter(|n| matches!(n.primitive, Primitive::Path { .. })).collect();
    assert_eq!(paths.len(), 5, "four border sides and one background");
    for n in paths {
        let Primitive::Path { points, .. } = &n.primitive else { unreachable!() };
        let (w, h) = (n.bounds.width as i32, n.bounds.height as i32);
        assert!(points.iter().all(|&(x, y)| (0..=w).contains(&x) && (0..=h).contains(&y)), "points lie inside the {w}x{h} bounds at {:?}: {points:?}", n.bounds);
    }
}
