//! Layout tests with hand-built documents and style sets: no dependency on the css or
//! style modules beyond `ComputedStyle::initial()` and `StyleSet`. Expected values
//! are computed by hand from the spec; text widths come from `text::measure` so they
//! track the bundled font tables.

use std::rc::Rc;

use super::block::{Bfc, MarginSet};
use super::debug;
use super::fragment::{Fragment, FragmentKind, FragmentTree, Replaced, StyleSource};
use super::text;
use super::{layout, layout_with, ImageSizeMap, LayoutCache, LayoutOptions, ScrollState};
use crate::dom::{Attribute, Document, NodeId, QuirksMode};
use crate::geom::{Au, Rect, Size};
use crate::style::*;
use crate::Viewport;

fn px(n: i32) -> Au {
    Au::from_px_i32(n)
}
fn len(n: i32) -> Sizing {
    Sizing::Set(LengthPercentage::Length(px(n)))
}
fn pct(p: i32) -> Sizing {
    Sizing::Set(LengthPercentage::Percent(p * 100))
}
fn m(n: i32) -> LengthPercentageAuto {
    LengthPercentageAuto::Set(LengthPercentage::Length(px(n)))
}
fn lp(n: i32) -> LengthPercentage {
    LengthPercentage::Length(px(n))
}
fn side(w: i32) -> BorderSide {
    BorderSide { width: px(w), style: BorderStyle::Solid, color: cw_scene::Color(0, 0, 0, 255) }
}
fn font() -> Font {
    ComputedStyle::initial().font
}
/// Width of a string in the default font.
fn tw(s: &str) -> Au {
    text::measure(&font(), s, Au::ZERO, Au::ZERO)
}
/// Normal line height of the default font.
fn lh() -> Au {
    text::font_metrics(&font()).normal_line_height()
}
fn ascent() -> Au {
    text::font_metrics(&font()).ascent
}

struct T {
    doc: Document,
    styles: StyleSet,
    html: NodeId,
    body: NodeId,
}

impl T {
    fn new() -> T {
        let mut doc = Document::new();
        let html = doc.create_element("html", vec![]);
        doc.append(Document::ROOT, html);
        let body = doc.create_element("body", vec![]);
        doc.append(html, body);
        let mut styles = StyleSet::new();
        let mut hs = ComputedStyle::initial();
        hs.display = Display::Block;
        styles.set(html, Rc::new(hs.clone()));
        let mut bs = ComputedStyle::inherit_from(&hs);
        bs.display = Display::Block;
        styles.set(body, Rc::new(bs));
        T { doc, styles, html, body }
    }
    fn style_of(&self, n: NodeId) -> ComputedStyle {
        self.styles.get(n).cloned().unwrap_or_else(ComputedStyle::initial)
    }
    /// A block element inheriting from its parent.
    fn el(&mut self, parent: NodeId, tag: &str, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
        self.el_attrs(parent, tag, vec![], f)
    }
    fn el_attrs(&mut self, parent: NodeId, tag: &str, attrs: Vec<(&str, &str)>, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
        let attrs = attrs.into_iter().map(|(n, v)| Attribute { name: n.into(), value: v.into() }).collect();
        let n = self.doc.create_element(tag, attrs);
        self.doc.append(parent, n);
        let mut s = ComputedStyle::inherit_from(&self.style_of(parent));
        s.display = Display::Block;
        f(&mut s);
        self.styles.set(n, Rc::new(s));
        n
    }
    fn div(&mut self, parent: NodeId, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
        self.el(parent, "div", f)
    }
    fn span(&mut self, parent: NodeId, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
        self.el(parent, "span", |s| {
            s.display = Display::Inline;
            f(s)
        })
    }
    fn text(&mut self, parent: NodeId, t: &str) -> NodeId {
        let n = self.doc.create_text(t);
        self.doc.append(parent, n);
        n
    }
    fn before(&mut self, n: NodeId, f: impl FnOnce(&mut ComputedStyle)) {
        let mut s = ComputedStyle::inherit_from(&self.style_of(n));
        s.display = Display::Inline;
        f(&mut s);
        self.styles.set_before(n, Rc::new(s));
    }
    fn layout(&self) -> FragmentTree {
        layout(&self.doc, &self.styles, Viewport { width: 800, height: 600, scale: 1, zoom: 100 })
    }
    fn layout_sized(&self, w: u32, h: u32) -> FragmentTree {
        layout(&self.doc, &self.styles, Viewport { width: w, height: h, scale: 1, zoom: 100 })
    }
    fn rect(&self, tree: &FragmentTree, n: NodeId) -> Rect {
        let r = tree.rects_of(n);
        assert!(!r.is_empty(), "no fragment for node {n:?}\n{}", debug::dump_doc(&self.doc, tree));
        r[0]
    }
    fn rects(&self, tree: &FragmentTree, n: NodeId) -> Vec<Rect> {
        tree.rects_of(n)
    }
}

fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(px(x), px(y), px(w), px(h))
}

fn find<'a>(f: &'a Fragment, pred: &dyn Fn(&Fragment) -> bool) -> Option<&'a Fragment> {
    if pred(f) {
        return Some(f);
    }
    f.children.iter().find_map(|c| find(c, pred))
}

fn lines(tree: &FragmentTree) -> Vec<Rect> {
    let mut out = Vec::new();
    tree.root.walk(Default::default(), &mut |f, r| {
        if matches!(f.kind, FragmentKind::Line) {
            out.push(r);
        }
    });
    out
}

fn texts(tree: &FragmentTree) -> Vec<(String, Rect)> {
    let mut out = Vec::new();
    tree.root.walk(Default::default(), &mut |f, r| {
        if let FragmentKind::Text { text, .. } = &f.kind {
            out.push((text.clone(), r));
        }
    });
    out
}

// Widths (§10.3.3), box-sizing, min/max, percentages.

#[test]
fn auto_width_fills_containing_block() {
    let mut t = T::new();
    let d = t.div(t.body, |s| {
        s.padding = Sides::uniform(lp(10));
        s.border = Sides::uniform(side(2));
        s.height = len(50);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, d), r(0, 0, 800, 74));
    assert_eq!(t.rect(&tree, t.body), r(0, 0, 800, 74));
}

#[test]
fn box_sizing_border_box_includes_edges() {
    let mut t = T::new();
    let d = t.div(t.body, |s| {
        s.width = len(200);
        s.height = len(100);
        s.padding = Sides::uniform(lp(10));
        s.border = Sides::uniform(side(5));
        s.box_sizing = BoxSizing::BorderBox;
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, d), r(0, 0, 200, 100));
    let d2 = t.div(t.body, |s| {
        s.width = len(200);
        s.height = len(100);
        s.padding = Sides::uniform(lp(10));
        s.border = Sides::uniform(side(5));
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, d2), r(0, 100, 230, 130));
}

#[test]
fn auto_margins_center_and_overconstrained_ignores_right() {
    let mut t = T::new();
    let a = t.div(t.body, |s| {
        s.width = len(300);
        s.height = len(10);
        s.margin.left = LengthPercentageAuto::Auto;
        s.margin.right = LengthPercentageAuto::Auto;
    });
    let b = t.div(t.body, |s| {
        s.width = len(300);
        s.height = len(10);
        s.margin.left = m(20);
        s.margin.right = m(999);
    });
    let c = t.div(t.body, |s| {
        s.width = len(300);
        s.height = len(10);
        s.margin.left = LengthPercentageAuto::Auto;
        s.margin.right = m(100);
    });
    let d = t.div(t.body, |s| {
        s.width = len(1000);
        s.height = len(10);
        s.margin.left = LengthPercentageAuto::Auto;
        s.margin.right = LengthPercentageAuto::Auto;
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(250, 0, 300, 10));
    assert_eq!(t.rect(&tree, b), r(20, 10, 300, 10));
    assert_eq!(t.rect(&tree, c), r(400, 20, 300, 10));
    // Wider than the containing block: auto margins are zero.
    assert_eq!(t.rect(&tree, d), r(0, 30, 1000, 10));
}

#[test]
fn rtl_overconstrained_ignores_left() {
    let mut t = T::new();
    let b = t.div(t.body, |s| {
        s.direction = Direction::Rtl;
        s.width = len(300);
        s.height = len(10);
        s.margin.left = m(999);
        s.margin.right = m(20);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, b), r(480, 0, 300, 10));
}

#[test]
fn min_max_width_clamp_order_and_recomputed_margins() {
    let mut t = T::new();
    let a = t.div(t.body, |s| {
        s.max_width = len(400);
        s.height = len(10);
        s.margin.left = LengthPercentageAuto::Auto;
        s.margin.right = LengthPercentageAuto::Auto;
    });
    let b = t.div(t.body, |s| {
        s.width = len(100);
        s.min_width = len(300);
        s.max_width = len(200);
        s.height = len(10);
    });
    let c = t.div(t.body, |s| {
        s.height = len(100);
        s.min_height = len(150);
        s.max_height = len(120);
    });
    let d = t.div(t.body, |s| {
        s.height = len(100);
        s.max_height = len(40);
    });
    let tree = t.layout();
    // max-width with auto margins re-solves the margins: centered.
    assert_eq!(t.rect(&tree, a), r(200, 0, 400, 10));
    // min wins over max.
    assert_eq!(t.rect(&tree, b), r(0, 10, 300, 10));
    assert_eq!(t.rect(&tree, c).size.height, px(150));
    assert_eq!(t.rect(&tree, d).size.height, px(40));
}

#[test]
fn percentage_widths_and_heights() {
    let mut t = T::new();
    let outer = t.div(t.body, |s| {
        s.width = len(400);
        s.height = len(200);
    });
    let a = t.div(outer, |s| {
        s.width = pct(50);
        s.height = pct(25);
        s.padding.left = LengthPercentage::Percent(1000);
    });
    let auto_h = t.div(t.body, |_| {});
    let b = t.div(auto_h, |s| {
        s.height = pct(50);
        s.width = pct(10);
    });
    let tree = t.layout();
    // 50% of 400 = 200 content + 10% padding (40).
    assert_eq!(t.rect(&tree, a), r(0, 0, 240, 50));
    // Percentage height against an auto-height parent is auto: no content, zero.
    assert_eq!(t.rect(&tree, b), r(0, 200, 80, 0));
}

#[test]
fn quirks_mode_body_percentage_height_uses_viewport() {
    let mut t = T::new();
    t.doc.quirks = QuirksMode::Quirks;
    let d = t.div(t.body, |s| s.height = pct(50));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, d).size.height, px(300));
    let mut t2 = T::new();
    let d2 = t2.div(t2.body, |s| s.height = pct(50));
    let tree2 = t2.layout();
    assert_eq!(t2.rect(&tree2, d2).size.height, px(0));
}

// Margin collapsing (§8.3.1).

#[test]
fn adjacent_sibling_margins_collapse_to_the_larger() {
    let mut t = T::new();
    let a = t.div(t.body, |s| {
        s.height = len(10);
        s.margin.bottom = m(20);
    });
    let b = t.div(t.body, |s| {
        s.height = len(10);
        s.margin.top = m(30);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 800, 10));
    assert_eq!(t.rect(&tree, b), r(0, 40, 800, 10));
}

#[test]
fn negative_margins_collapse_by_sum_of_extremes() {
    let mut t = T::new();
    let _a = t.div(t.body, |s| {
        s.height = len(10);
        s.margin.bottom = m(-10);
    });
    let b = t.div(t.body, |s| {
        s.height = len(10);
        s.margin.top = m(30);
    });
    let c = t.div(t.body, |s| {
        s.height = len(10);
        s.margin.top = m(-25);
    });
    let d = t.div(t.body, |s| {
        s.height = len(10);
        s.margin.top = m(-5);
    });
    let tree = t.layout();
    // 30 + (-10) = 20.
    assert_eq!(t.rect(&tree, b).origin.y, px(30));
    // Only negatives: the most negative.
    assert_eq!(t.rect(&tree, c).origin.y, px(15));
    assert_eq!(t.rect(&tree, d).origin.y, px(20));
}

#[test]
fn parent_and_first_child_margins_collapse_through() {
    let mut t = T::new();
    let outer = t.div(t.body, |s| s.margin.top = m(10));
    let inner = t.div(outer, |s| {
        s.margin.top = m(25);
        s.height = len(10);
    });
    let tree = t.layout();
    // body has no top border/padding either: the collapsed 25 sits above body too.
    assert_eq!(t.rect(&tree, t.body).origin.y, px(25));
    assert_eq!(t.rect(&tree, outer).origin.y, px(25));
    assert_eq!(t.rect(&tree, inner).origin.y, px(25));
    assert_eq!(t.rect(&tree, outer).size.height, px(10));
}

#[test]
fn padding_border_and_bfc_stop_collapsing() {
    let mut t = T::new();
    let p = t.div(t.body, |s| s.padding.top = lp(1));
    let pc = t.div(p, |s| {
        s.margin.top = m(20);
        s.height = len(10);
    });
    let b = t.div(t.body, |s| s.border.top = side(1));
    let bc = t.div(b, |s| {
        s.margin.top = m(20);
        s.height = len(10);
    });
    let o = t.div(t.body, |s| s.overflow_y = Overflow::Hidden);
    let oc = t.div(o, |s| {
        s.margin.top = m(20);
        s.margin.bottom = m(30);
        s.height = len(10);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, p), r(0, 0, 800, 31));
    assert_eq!(t.rect(&tree, pc).origin.y, px(21));
    assert_eq!(t.rect(&tree, b), r(0, 31, 800, 31));
    assert_eq!(t.rect(&tree, bc).origin.y, px(52));
    // The BFC root contains both margins of its child: 20 + 10 + 30.
    assert_eq!(t.rect(&tree, o), r(0, 62, 800, 60));
    assert_eq!(t.rect(&tree, oc).origin.y, px(82));
}

#[test]
fn last_child_bottom_margin_collapses_unless_height_or_min_height() {
    let mut t = T::new();
    let a = t.div(t.body, |_| {});
    let _ac = t.div(a, |s| {
        s.height = len(10);
        s.margin.bottom = m(20);
    });
    let next = t.div(t.body, |s| {
        s.height = len(10);
        s.margin.top = m(5);
    });
    let b = t.div(t.body, |s| s.min_height = len(5));
    let _bc = t.div(b, |s| {
        s.height = len(10);
        s.margin.bottom = m(20);
    });
    let last = t.div(t.body, |s| s.height = len(10));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.height, px(10));
    assert_eq!(t.rect(&tree, next).origin.y, px(30));
    assert_eq!(t.rect(&tree, b), r(0, 40, 800, 30));
    assert_eq!(t.rect(&tree, last).origin.y, px(70));
}

#[test]
fn empty_block_margins_collapse_through_each_other() {
    let mut t = T::new();
    let a = t.div(t.body, |s| {
        s.height = len(10);
        s.margin.bottom = m(10);
    });
    let e = t.div(t.body, |s| {
        s.margin.top = m(20);
        s.margin.bottom = m(30);
    });
    let b = t.div(t.body, |s| {
        s.margin.top = m(15);
        s.height = len(10);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).origin.y, px(0));
    // All four margins collapse: max(10, 20, 30, 15) = 30.
    assert_eq!(t.rect(&tree, b).origin.y, px(40));
    assert_eq!(t.rect(&tree, e).size.height, px(0));
    assert_eq!(t.rect(&tree, e).origin.y, px(30));
}

#[test]
fn margin_set_arithmetic() {
    let mut s = MarginSet::default();
    s.add(px(10));
    s.add(px(-4));
    s.add(px(7));
    s.add(px(-9));
    assert_eq!(s.collapse(), px(1));
    assert_eq!(MarginSet::of(px(3)).union(MarginSet::of(px(5))).collapse(), px(5));
}

#[test]
fn relative_positioning_offsets_without_affecting_flow() {
    let mut t = T::new();
    let a = t.div(t.body, |s| {
        s.position = Position::Relative;
        s.inset.left = m(30);
        s.inset.top = m(-5);
        s.height = len(10);
    });
    let b = t.div(t.body, |s| s.height = len(10));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(30, -5, 800, 10));
    assert_eq!(t.rect(&tree, b), r(0, 10, 800, 10));
}

// Floats (§9.5) and clearance.

#[test]
fn floats_shorten_lines_and_following_blocks_flow_under() {
    let mut t = T::new();
    let f = t.div(t.body, |s| {
        s.float = Float::Left;
        s.width = len(100);
        s.height = len(50);
    });
    let p = t.div(t.body, |_| {});
    t.text(p, "aaa");
    let after = t.div(t.body, |s| s.height = len(10));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f), r(0, 0, 100, 50));
    // The block itself spans the full width; its line box starts after the float.
    assert_eq!(t.rect(&tree, p), Rect::new(Au::ZERO, Au::ZERO, px(800), lh()));
    let ls = lines(&tree);
    assert_eq!(ls[0].origin.x, px(100));
    assert_eq!(ls[0].size.width, px(700));
    assert_eq!(t.rect(&tree, after).origin.y, lh());
}

#[test]
fn right_float_and_two_floats_side_by_side_then_wrap() {
    let mut t = T::new();
    let a = t.div(t.body, |s| {
        s.float = Float::Left;
        s.width = len(500);
        s.height = len(20);
    });
    let b = t.div(t.body, |s| {
        s.float = Float::Right;
        s.width = len(200);
        s.height = len(30);
    });
    let c = t.div(t.body, |s| {
        s.float = Float::Left;
        s.width = len(200);
        s.height = len(10);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 500, 20));
    assert_eq!(t.rect(&tree, b), r(600, 0, 200, 30));
    // 500 + 200 + 200 > 800: the third float moves below the first.
    assert_eq!(t.rect(&tree, c), r(0, 20, 200, 10));
}

#[test]
fn float_margins_and_clear_both() {
    let mut t = T::new();
    let f = t.div(t.body, |s| {
        s.float = Float::Left;
        s.width = len(100);
        s.height = len(50);
        s.margin = Sides::uniform(m(10));
    });
    let g = t.div(t.body, |s| {
        s.float = Float::Right;
        s.width = len(100);
        s.height = len(80);
    });
    let cl = t.div(t.body, |s| {
        s.clear = Clear::Both;
        s.height = len(10);
        s.margin.top = m(100);
    });
    let l = t.div(t.body, |s| {
        s.clear = Clear::Left;
        s.height = len(10);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f), r(10, 10, 100, 50));
    assert_eq!(t.rect(&tree, g), r(700, 0, 100, 80));
    // Its margin alone would put it at 100, below both floats: no clearance.
    assert_eq!(t.rect(&tree, cl).origin.y, px(100));
    assert_eq!(t.rect(&tree, l).origin.y, px(110));
}

#[test]
fn clearance_moves_block_below_float() {
    let mut t = T::new();
    let f = t.div(t.body, |s| {
        s.float = Float::Left;
        s.width = len(100);
        s.height = len(50);
    });
    let a = t.div(t.body, |s| s.height = len(10));
    let cl = t.div(t.body, |s| {
        s.clear = Clear::Left;
        s.height = len(10);
        s.margin.top = m(5);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f).size.height, px(50));
    assert_eq!(t.rect(&tree, a).origin.y, px(0));
    assert_eq!(t.rect(&tree, cl).origin.y, px(50));
    assert_eq!(t.rect(&tree, t.body).size.height, px(60));
}

#[test]
fn bfc_root_contains_floats_and_sits_next_to_them() {
    let mut t = T::new();
    let root = t.div(t.body, |s| s.overflow_x = Overflow::Hidden);
    let f = t.div(root, |s| {
        s.float = Float::Left;
        s.width = len(100);
        s.height = len(50);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, root), r(0, 0, 800, 50));
    assert_eq!(t.rect(&tree, f), r(0, 0, 100, 50));
    // A sibling BFC root next to an outer float narrows to the space beside it.
    let mut t = T::new();
    let f = t.div(t.body, |s| {
        s.float = Float::Left;
        s.width = len(100);
        s.height = len(50);
    });
    let b = t.div(t.body, |s| {
        s.display = Display::FlowRoot;
        s.height = len(10);
    });
    let c = t.div(t.body, |s| {
        s.overflow_y = Overflow::Auto;
        s.width = len(750);
        s.height = len(10);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f).size.width, px(100));
    assert_eq!(t.rect(&tree, b), r(100, 0, 700, 10));
    // Too wide to fit beside the float: moved below it.
    assert_eq!(t.rect(&tree, c), r(0, 50, 750, 10));
}

#[test]
fn float_in_inline_content_is_placed_at_the_line() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "before ");
    let f = t.el(p, "img", |s| {
        s.float = Float::Right;
        s.width = len(50);
        s.height = len(20);
    });
    t.text(p, "after");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f), r(750, 0, 50, 20));
    let ls = lines(&tree);
    assert_eq!(ls.len(), 1);
    assert_eq!(ls[0].size.width, px(750));
}

// Inline formatting.

#[test]
fn text_wraps_at_soft_wrap_opportunities() {
    let mut t = T::new();
    let p = t.div(t.body, |s| s.width = len(120));
    t.text(p, "one two three four");
    let tree = t.layout();
    let ls = lines(&tree);
    assert!(ls.len() >= 2, "{}", debug::dump_doc(&t.doc, &tree));
    for (txt, rect) in texts(&tree) {
        assert!(rect.size.width <= px(120), "{txt}");
        assert!(!txt.starts_with(' ') && !txt.ends_with(' '), "{txt:?}");
    }
    assert_eq!(t.rect(&tree, p).size.height, lh() * ls.len() as i32);
    let all: String = texts(&tree).iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join(" ");
    assert_eq!(all, "one two three four");
}

#[test]
fn whitespace_collapses_and_trims_line_edges() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "  hello \n  world  ");
    let tree = t.layout();
    let ts = texts(&tree);
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].0, "hello world");
    assert_eq!(ts[0].1.size.width, tw("hello world"));
    assert_eq!(ts[0].1.origin.x, px(0));
}

#[test]
fn pre_preserves_newlines_and_spaces_nowrap_does_not_wrap() {
    let mut t = T::new();
    let p = t.div(t.body, |s| {
        s.white_space = WhiteSpace::Pre;
        s.width = len(10);
    });
    t.text(p, "a  b\ncd\n");
    let tree = t.layout();
    let ls = lines(&tree);
    assert_eq!(ls.len(), 2);
    let ts = texts(&tree);
    assert_eq!(ts[0].0, "a  b");
    assert_eq!(ts[1].0, "cd");
    let mut t = T::new();
    let p = t.div(t.body, |s| {
        s.white_space = WhiteSpace::NoWrap;
        s.width = len(10);
    });
    t.text(p, "one two three");
    let tree = t.layout();
    assert_eq!(lines(&tree).len(), 1);
    assert_eq!(texts(&tree)[0].0, "one two three");
}

#[test]
fn pre_line_breaks_at_newlines_and_collapses_spaces() {
    let mut t = T::new();
    let p = t.div(t.body, |s| s.white_space = WhiteSpace::PreLine);
    t.text(p, "a   b\n  c");
    let tree = t.layout();
    let ts = texts(&tree);
    assert_eq!(ts.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), vec!["a b", "c"]);
}

#[test]
fn br_forces_lines_and_trailing_br_adds_none() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "a");
    t.el(p, "br", |s| s.display = Display::Inline);
    t.text(p, "b");
    t.el(p, "br", |s| s.display = Display::Inline);
    let q = t.div(t.body, |_| {});
    t.el(q, "br", |s| s.display = Display::Inline);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, p).size.height, lh() * 2);
    assert_eq!(t.rect(&tree, q).size.height, lh());
}

#[test]
fn text_align_right_center_and_indent() {
    let mut t = T::new();
    let a = t.div(t.body, |s| s.text_align = TextAlign::Right);
    t.text(a, "abc");
    let b = t.div(t.body, |s| s.text_align = TextAlign::Center);
    t.text(b, "abc");
    let c = t.div(t.body, |s| s.text_indent = lp(40));
    t.text(c, "abc");
    let d = t.div(t.body, |s| {
        s.text_align = TextAlign::End;
        s.direction = Direction::Rtl;
    });
    t.text(d, "abc");
    let tree = t.layout();
    let w = tw("abc");
    let ts = texts(&tree);
    assert_eq!(ts[0].1.origin.x, px(800) - w);
    assert_eq!(ts[1].1.origin.x, (px(800) - w) / 2);
    assert_eq!(ts[2].1.origin.x, px(40));
    // rtl + end = left.
    assert_eq!(ts[3].1.origin.x, px(0));
}

#[test]
fn justify_spreads_words_over_the_line_except_the_last() {
    let mut t = T::new();
    let p = t.div(t.body, |s| {
        s.text_align = TextAlign::Justify;
        s.width = len(100);
    });
    t.text(p, "aa bb cc dd ee ff gg hh ii jj kk ll mm");
    let tree = t.layout();
    let ls = lines(&tree);
    assert!(ls.len() >= 2);
    let ts = texts(&tree);
    // Words on the first line: the last one ends exactly at the right edge.
    let first_line_bottom = ls[0].bottom();
    let on_first: Vec<_> = ts.iter().filter(|(_, r)| r.origin.y < first_line_bottom).collect();
    let last = on_first.iter().rev().find(|(s, _)| s != " ").unwrap();
    assert_eq!(last.1.right(), px(100), "{}", debug::dump_doc(&t.doc, &tree));
    // The last line is not justified.
    let last_line_top = ls.last().unwrap().origin.y;
    let on_last: Vec<_> = ts.iter().filter(|(_, r)| r.origin.y >= last_line_top).collect();
    assert!(on_last.last().unwrap().1.right() < px(100));
}

#[test]
fn line_height_and_vertical_align_of_inline_boxes() {
    let mut t = T::new();
    let p = t.div(t.body, |s| s.line_height = LineHeight::Length(px(40)));
    t.text(p, "x");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, p).size.height, px(40));
    let ts = texts(&tree);
    // The text sits centred in the 40 px line: half-leading above the ascent.
    let fm = text::font_metrics(&font());
    let half = (px(40) - fm.content_height()) / 2;
    assert_eq!(ts[0].1.origin.y, half);

    // A taller inline child with vertical-align: baseline raises the line.
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "x");
    let big = t.span(p, |s| s.font.size = px(32));
    t.text(big, "Y");
    let tree = t.layout();
    let big_fm = text::font_metrics(&t.style_of(big).font);
    let small = text::font_metrics(&font());
    let expect = big_fm.ascent.max(small.ascent) + big_fm.descent.max(small.descent);
    assert_eq!(t.rect(&tree, p).size.height, expect);

    // vertical-align: top puts the box at the line top; super raises it.
    let mut t = T::new();
    let p = t.div(t.body, |s| s.line_height = LineHeight::Length(px(60)));
    t.text(p, "x");
    let top = t.span(p, |s| {
        s.vertical_align = VerticalAlign::Top;
        s.line_height = LineHeight::Length(px(20));
    });
    t.text(top, "t");
    let sup = t.span(p, |s| s.vertical_align = VerticalAlign::Super);
    t.text(sup, "s");
    let tree = t.layout();
    let fm = text::font_metrics(&font());
    let top_rect = t.rect(&tree, top);
    // The inline box's top (content area) is at the line top plus its half-leading.
    assert_eq!(top_rect.origin.y, (px(20) - fm.content_height()) / 2);
    let ts = texts(&tree);
    let x_y = ts[0].1.origin.y;
    let s_y = ts.iter().find(|(s, _)| s == "s").unwrap().1.origin.y;
    assert_eq!(x_y - s_y, px(16) / 3);
}

#[test]
fn inline_box_edges_take_width_but_not_height() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    let sp = t.span(p, |s| {
        s.padding = Sides::uniform(lp(10));
        s.border = Sides::uniform(side(2));
        s.margin.left = m(5);
        s.margin.right = m(7);
    });
    t.text(sp, "ab");
    t.text(p, "c");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, p).size.height, lh());
    let sr = t.rect(&tree, sp);
    assert_eq!(sr.origin.x, px(5));
    assert_eq!(sr.size.width, tw("ab") + px(24));
    let ts = texts(&tree);
    assert_eq!(ts[0].1.origin.x, px(17));
    assert_eq!(ts[1].1.origin.x, px(5) + tw("ab") + px(24) + px(7));
    // The inline box is the content area plus padding and border.
    let fm = text::font_metrics(&font());
    assert_eq!(sr.size.height, fm.content_height() + px(24));
}

#[test]
fn empty_inline_with_border_still_makes_a_line() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    let _sp = t.span(p, |s| s.border = Sides::uniform(side(1)));
    let q = t.div(t.body, |_| {});
    let _empty = t.span(q, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, p).size.height, lh());
    assert_eq!(t.rect(&tree, q).size.height, px(0));
}

#[test]
fn inline_block_baseline_and_shrink_to_fit() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "x");
    let ib = t.div(p, |s| {
        s.display = Display::InlineBlock;
        s.padding = Sides::uniform(lp(4));
        s.border = Sides::uniform(side(1));
    });
    t.text(ib, "hello");
    let empty_ib = t.div(p, |s| {
        s.display = Display::InlineBlock;
        s.width = len(20);
        s.height = len(30);
    });
    let tree = t.layout();
    let ibr = t.rect(&tree, ib);
    assert_eq!(ibr.size.width, tw("hello") + px(10));
    assert_eq!(ibr.size.height, lh() + px(10));
    // Baseline aligned: the inline-block's text baseline equals the outer baseline.
    let ts = texts(&tree);
    let bx = ts.iter().find(|(s, _)| s == "x").unwrap().1;
    let bh = ts.iter().find(|(s, _)| s == "hello").unwrap().1;
    assert_eq!(bx.origin.y + ascent(), bh.origin.y + ascent());
    assert_eq!(bh.origin.x, ibr.origin.x + px(5));
    // An empty inline-block sits on its bottom margin edge: its bottom is the baseline.
    let er = t.rect(&tree, empty_ib);
    assert_eq!(er.bottom(), bx.origin.y + ascent());
    assert_eq!(er.origin.x, ibr.right());
}

#[test]
fn ellipsis_truncates_overflowing_nowrap_text() {
    let mut t = T::new();
    let p = t.div(t.body, |s| {
        s.width = len(60);
        s.white_space = WhiteSpace::NoWrap;
        s.overflow_x = Overflow::Hidden;
        s.text_overflow = TextOverflow::Ellipsis;
    });
    t.text(p, "a very long piece of text");
    let tree = t.layout();
    let ts = texts(&tree);
    assert_eq!(ts.len(), 1);
    assert!(ts[0].0.ends_with('\u{2026}'), "{}", ts[0].0);
    assert!(ts[0].1.right() <= px(60));
    let f = find(&tree.root, &|f| matches!(f.kind, FragmentKind::Text { ellipsis: true, .. })).unwrap();
    assert!(matches!(f.kind, FragmentKind::Text { .. }));
}

#[test]
fn letter_and_word_spacing_and_transform() {
    let mut t = T::new();
    let p = t.div(t.body, |s| {
        s.letter_spacing = px(2);
        s.word_spacing = px(5);
        s.text_transform = TextTransform::Uppercase;
    });
    t.text(p, "ab cd");
    let tree = t.layout();
    let ts = texts(&tree);
    assert_eq!(ts[0].0, "AB CD");
    let expect = text::measure(&font(), "AB CD", px(2), px(5));
    assert_eq!(ts[0].1.size.width, expect);
    if let FragmentKind::Text { range, .. } = &find(&tree.root, &|f| matches!(f.kind, FragmentKind::Text { .. })).unwrap().kind {
        assert_eq!(*range, (0, 5));
    }
}

#[test]
fn long_word_overflows_unless_overflow_wrap_breaks_it() {
    let mut t = T::new();
    let a = t.div(t.body, |s| s.width = len(30));
    t.text(a, "unbreakableword");
    let b = t.div(t.body, |s| {
        s.width = len(30);
        s.overflow_wrap = OverflowWrap::Anywhere;
    });
    t.text(b, "unbreakableword");
    let c = t.div(t.body, |s| {
        s.width = len(30);
        s.word_break = WordBreak::BreakAll;
    });
    t.text(c, "unbreakableword");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.height, lh());
    assert!(t.rect(&tree, b).size.height > lh() * 2, "{}", debug::dump_doc(&t.doc, &tree));
    assert!(t.rect(&tree, c).size.height > lh() * 2);
    for (s, rect) in texts(&tree) {
        if rect.origin.y >= t.rect(&tree, b).origin.y {
            assert!(rect.size.width <= px(30), "{s}");
        }
    }
}

#[test]
fn wbr_and_hyphens_are_break_opportunities() {
    let mut t = T::new();
    let a = t.div(t.body, |s| s.width = len(60));
    t.text(a, "abcdefgh");
    t.el(a, "wbr", |s| s.display = Display::Inline);
    t.text(a, "ijklmnop");
    let b = t.div(t.body, |s| s.width = len(60));
    t.text(b, "abcdefgh-ijklmnop");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.height, lh() * 2);
    assert_eq!(t.rect(&tree, b).size.height, lh() * 2);
}

#[test]
fn rtl_lines_start_at_the_right() {
    let mut t = T::new();
    let p = t.div(t.body, |s| s.direction = Direction::Rtl);
    t.text(p, "ab ");
    let sp = t.span(p, |_| {});
    t.text(sp, "cd");
    let tree = t.layout();
    let ts = texts(&tree);
    // Logical order "ab", "cd" is laid out from the right: "ab" is rightmost.
    let ab = ts.iter().find(|(s, _)| s.trim() == "ab").unwrap().1;
    let cd = ts.iter().find(|(s, _)| s == "cd").unwrap().1;
    assert_eq!(ab.right(), px(800));
    assert!(cd.right() <= ab.origin.x);
}

#[test]
fn anonymous_blocks_wrap_inline_runs_and_blocks_split_inlines() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "a");
    let b = t.div(p, |s| s.height = len(10));
    t.text(p, "c");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, b).origin.y, lh());
    assert_eq!(t.rect(&tree, p).size.height, lh() * 2 + px(10));

    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    let sp = t.span(p, |s| s.border.left = side(3));
    t.text(sp, "x");
    let blk = t.div(sp, |s| s.height = len(10));
    t.text(sp, "y");
    let tree = t.layout();
    let pieces = t.rects(&tree, sp);
    assert_eq!(pieces.len(), 2, "{}", debug::dump_doc(&t.doc, &tree));
    assert_eq!(t.rect(&tree, blk), Rect::new(Au::ZERO, lh(), px(800), px(10)));
    // Only the first piece carries the left border.
    assert_eq!(pieces[0].size.width, tw("x") + px(3));
    assert_eq!(pieces[1].size.width, tw("y"));
}

#[test]
fn display_none_and_contents() {
    let mut t = T::new();
    let hidden = t.div(t.body, |s| {
        s.display = Display::None;
        s.height = len(100);
    });
    let _inner = t.div(hidden, |s| s.height = len(100));
    let c = t.div(t.body, |s| s.display = Display::Contents);
    let d = t.div(c, |s| s.height = len(10));
    let tree = t.layout();
    assert!(t.rects(&tree, hidden).is_empty());
    assert!(t.rects(&tree, c).is_empty());
    assert_eq!(t.rect(&tree, d), r(0, 0, 800, 10));
}

// Lists and generated content.

#[test]
fn list_item_markers_outside_and_inside() {
    let mut t = T::new();
    let ol = t.el(t.body, "ol", |s| s.padding.left = lp(40));
    let li1 = t.el(ol, "li", |s| {
        s.display = Display::ListItem;
        s.list_style_type = ListStyleType::Decimal;
    });
    t.text(li1, "one");
    let li2 = t.el(ol, "li", |s| {
        s.display = Display::ListItem;
        s.list_style_type = ListStyleType::LowerAlpha;
        s.list_style_position = ListStylePosition::Inside;
    });
    t.text(li2, "two");
    let tree = t.layout();
    let m1 = find(&tree.root, &|f| matches!(&f.kind, FragmentKind::Box { source: StyleSource::Marker(n), .. } if *n == li1)).unwrap();
    assert_eq!(m1.kind.clone(), m1.kind.clone());
    if let FragmentKind::Box { replaced: Some(Replaced::Marker(txt)), .. } = &m1.kind {
        assert_eq!(txt, "1. ");
    } else {
        panic!("no marker");
    }
    assert_eq!(m1.rect.right(), px(0));
    let ts = texts(&tree);
    let two = ts.iter().find(|(s, _)| s == "two").unwrap().1;
    assert_eq!(two.origin.x, px(40) + tw("b. "));
    assert!(ts.iter().any(|(s, _)| s == "b. "));
}

#[test]
fn ol_start_and_li_value_drive_the_counter() {
    let mut t = T::new();
    let ol = t.el_attrs(t.body, "ol", vec![("start", "5")], |_| {});
    // The UA sheet would reset the counter; emulate it through counter-reset.
    let mut s = t.style_of(ol);
    s.counter_reset = vec![("list-item".into(), 4)];
    t.styles.set(ol, Rc::new(s));
    let a = t.el(ol, "li", |s| {
        s.display = Display::ListItem;
        s.list_style_type = ListStyleType::Decimal;
    });
    let b = t.el_attrs(ol, "li", vec![("value", "10")], |s| {
        s.display = Display::ListItem;
        s.list_style_type = ListStyleType::Decimal;
    });
    let c = t.el(ol, "li", |s| {
        s.display = Display::ListItem;
        s.list_style_type = ListStyleType::UpperRoman;
    });
    let tree = t.layout();
    let marker = |n: NodeId| -> String {
        let f = find(&tree.root, &|f| matches!(&f.kind, FragmentKind::Box { source: StyleSource::Marker(m), .. } if *m == n)).unwrap();
        match &f.kind {
            FragmentKind::Box { replaced: Some(Replaced::Marker(t)), .. } => t.clone(),
            _ => String::new(),
        }
    };
    assert_eq!(marker(a), "5. ");
    assert_eq!(marker(b), "10. ");
    assert_eq!(marker(c), "XI. ");
}

#[test]
fn before_content_with_attr_counter_and_quotes() {
    let mut t = T::new();
    let p = t.el_attrs(t.body, "div", vec![("data-x", "Z")], |s| s.counter_increment = vec![("n".into(), 3)]);
    t.before(p, |s| {
        s.content = Content::Items(vec![ContentItem::OpenQuote, ContentItem::Text("v".into()), ContentItem::Attr("data-x".into()), ContentItem::Counter("n".into(), ListStyleType::Decimal), ContentItem::CloseQuote]);
    });
    t.text(p, "body");
    let tree = t.layout();
    let ts = texts(&tree);
    assert_eq!(ts[0].0, "\u{201C}vZ3\u{201D}");
    let f = find(&tree.root, &|f| matches!(f.kind, FragmentKind::Text { source: StyleSource::Before(_), .. })).unwrap();
    assert!(matches!(f.kind, FragmentKind::Text { node: None, .. }));
    assert_eq!(ts[1].1.origin.x, ts[0].1.right());
}

#[test]
fn block_before_pseudo_is_a_block_child() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.before(p, |s| {
        s.display = Display::Block;
        s.height = len(7);
        s.content = Content::Items(vec![ContentItem::Text(String::new())]);
    });
    t.text(p, "x");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, p).size.height, px(7) + lh());
}

// Replaced elements and controls.

#[test]
fn images_use_attributes_intrinsic_sizes_and_placeholders() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    let a = t.el_attrs(p, "img", vec![("src", "a.png"), ("width", "40")], |s| s.display = Display::Inline);
    let b = t.el_attrs(p, "img", vec![("src", "b.png")], |s| s.display = Display::Inline);
    let c = t.el_attrs(p, "img", vec![("src", "none.png")], |s| s.display = Display::Inline);
    let d = t.el_attrs(p, "img", vec![("src", "a.png")], |s| {
        s.display = Display::Block;
        s.height = len(10);
    });
    let mut images = ImageSizeMap::default();
    images.0.insert("a.png".into(), (100, 50));
    images.0.insert("b.png".into(), (30, 60));
    let scroll = ScrollState::new();
    let mut cache = LayoutCache::default();
    let tree = layout_with(&t.doc, &t.styles, Viewport { width: 800, height: 600, scale: 1, zoom: 100 }, LayoutOptions { images: &images, scroll: &scroll }, &mut cache);
    // width attribute keeps the aspect ratio.
    assert_eq!(t.rect(&tree, a).size, Size { width: px(40), height: px(20) });
    assert_eq!(t.rect(&tree, b).size, Size { width: px(30), height: px(60) });
    assert_eq!(t.rect(&tree, c).size, Size { width: px(16), height: px(16) });
    assert_eq!(t.rect(&tree, d).size, Size { width: px(20), height: px(10) });
    // Inline images sit on the baseline: the tallest one sets the line height.
    let line = lines(&tree)[0];
    assert!(line.size.height >= px(60));
}

#[test]
fn form_controls_have_intrinsic_sizes() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    let ti = t.el_attrs(p, "input", vec![("type", "text")], |s| {
        s.display = Display::InlineBlock;
        s.padding = Sides::uniform(lp(2));
        s.border = Sides::uniform(side(1));
    });
    let cb = t.el_attrs(p, "input", vec![("type", "checkbox")], |s| s.display = Display::InlineBlock);
    let ta = t.el_attrs(p, "textarea", vec![("cols", "10"), ("rows", "3")], |s| s.display = Display::InlineBlock);
    let sel = t.el(p, "select", |s| s.display = Display::InlineBlock);
    let _opt = t.el(sel, "option", |s| s.display = Display::Block);
    let btn = t.el(p, "button", |s| {
        s.display = Display::InlineBlock;
        s.padding = Sides::uniform(lp(3));
    });
    t.text(btn, "Go");
    let ifr = t.el(p, "iframe", |s| s.display = Display::Inline);
    let cv = t.el_attrs(p, "canvas", vec![("width", "10"), ("height", "20")], |s| s.display = Display::Inline);
    let tree = t.layout();
    let ch = text::ch_unit(&font());
    assert_eq!(t.rect(&tree, ti).size, Size { width: ch * 20 + px(6), height: lh() + px(6) });
    assert_eq!(t.rect(&tree, cb).size, Size { width: px(13), height: px(13) });
    assert_eq!(t.rect(&tree, ta).size, Size { width: ch * 10, height: lh() * 3 });
    assert_eq!(t.rect(&tree, sel).size, Size { width: ch * 20, height: lh() });
    assert_eq!(t.rect(&tree, btn).size, Size { width: tw("Go") + px(6), height: lh() + px(6) });
    assert_eq!(t.rect(&tree, ifr).size, Size { width: px(300), height: px(150) });
    assert_eq!(t.rect(&tree, cv).size, Size { width: px(10), height: px(20) });
    let bf = find(&tree.root, &|f| matches!(&f.kind, FragmentKind::Box { source: StyleSource::Element(n), .. } if *n == btn)).unwrap();
    assert!(matches!(&bf.kind, FragmentKind::Box { replaced: Some(Replaced::Control(super::fragment::ControlKind::Button)), .. }));
}

// Absolute and fixed positioning (§10.3.7, §10.6.4).

#[test]
fn absolute_insets_and_auto_sizes() {
    let mut t = T::new();
    let cb = t.div(t.body, |s| {
        s.position = Position::Relative;
        s.width = len(400);
        s.height = len(300);
        s.padding = Sides::uniform(lp(10));
        s.border = Sides::uniform(side(5));
    });
    let a = t.div(cb, |s| {
        s.position = Position::Absolute;
        s.inset.left = m(20);
        s.inset.top = m(30);
        s.width = len(50);
        s.height = len(40);
    });
    let b = t.div(cb, |s| {
        s.position = Position::Absolute;
        s.inset.right = m(0);
        s.inset.bottom = m(0);
        s.width = len(50);
        s.height = len(40);
    });
    let c = t.div(cb, |s| {
        s.position = Position::Absolute;
        s.inset.left = m(0);
        s.inset.right = m(0);
        s.inset.top = m(0);
        s.inset.bottom = m(0);
    });
    let d = t.div(cb, |s| {
        s.position = Position::Absolute;
        s.inset.left = m(0);
        s.inset.right = m(0);
        s.inset.top = m(0);
        s.inset.bottom = m(0);
        s.width = len(100);
        s.height = len(100);
        s.margin = Sides::uniform(LengthPercentageAuto::Auto);
    });
    let tree = t.layout();
    // Relative to the padding box: border 5 + inset.
    assert_eq!(t.rect(&tree, a), r(25, 35, 50, 40));
    // Padding box is 420x320 inside the border.
    assert_eq!(t.rect(&tree, b), r(5 + 420 - 50, 5 + 320 - 40, 50, 40));
    assert_eq!(t.rect(&tree, c), r(5, 5, 420, 320));
    // Auto margins centre the fully constrained box.
    assert_eq!(t.rect(&tree, d), r(5 + 160, 5 + 110, 100, 100));
}

#[test]
fn absolute_static_position_and_shrink_to_fit() {
    let mut t = T::new();
    let cb = t.div(t.body, |s| {
        s.position = Position::Relative;
        s.height = len(300);
    });
    let _first = t.div(cb, |s| s.height = len(20));
    let a = t.div(cb, |s| {
        s.position = Position::Absolute;
        s.padding = Sides::uniform(lp(2));
    });
    t.text(a, "hi");
    let _second = t.div(cb, |s| s.height = len(20));
    let b = t.div(cb, |s| {
        s.position = Position::Absolute;
        s.inset.right = m(10);
    });
    t.text(b, "hi");
    let tree = t.layout();
    // Static position: where it would have been (y = 20); width shrink-to-fit.
    assert_eq!(t.rect(&tree, a), Rect::new(px(0), px(20), tw("hi") + px(4), lh() + px(4)));
    assert_eq!(t.rect(&tree, b), Rect::new(px(790) - tw("hi"), px(40), tw("hi"), lh()));
}

#[test]
fn fixed_positions_against_the_viewport() {
    let mut t = T::new();
    let cb = t.div(t.body, |s| {
        s.position = Position::Relative;
        s.margin.left = m(100);
        s.height = len(300);
    });
    let f = t.div(cb, |s| {
        s.position = Position::Fixed;
        s.inset.right = m(10);
        s.inset.bottom = m(10);
        s.width = len(50);
        s.height = len(50);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f), r(740, 540, 50, 50));
    // Attached to the root, painted with a stacking context.
    assert!(tree.root.children.iter().any(|c| c.is_positioned && c.establishes_stacking_context && matches!(&c.kind, FragmentKind::Box { source: StyleSource::Element(n), .. } if *n == f)));
}

#[test]
fn absolute_inside_inline_content_uses_line_position() {
    let mut t = T::new();
    let cb = t.div(t.body, |s| s.position = Position::Relative);
    t.text(cb, "ab ");
    let a = t.span(cb, |s| {
        s.position = Position::Absolute;
        s.width = len(10);
        s.height = len(10);
    });
    t.text(cb, "cd");
    let tree = t.layout();
    let ar = t.rect(&tree, a);
    assert_eq!(ar.origin.y, px(0));
    assert_eq!(ar.origin.x, tw("ab "));
}

// Scroll containers and the root.

#[test]
fn overflow_auto_reserves_a_scrollbar_when_content_overflows() {
    let mut t = T::new();
    let sc = t.el_attrs(t.body, "div", vec![("id", "s")], |s| {
        s.overflow_y = Overflow::Auto;
        s.width = len(200);
        s.height = len(100);
    });
    let inner = t.div(sc, |s| s.height = len(500));
    let plain = t.div(t.body, |s| {
        s.overflow_y = Overflow::Scroll;
        s.width = len(200);
        s.height = len(100);
    });
    let pin = t.div(plain, |s| s.height = len(10));
    let mut scroll = ScrollState::new();
    scroll.insert(sc, (Au::ZERO, px(1000)));
    let mut cache = LayoutCache::default();
    let tree = layout_with(&t.doc, &t.styles, Viewport { width: 800, height: 600, scale: 1, zoom: 100 }, LayoutOptions { images: &super::NoImages, scroll: &scroll }, &mut cache);
    // The vertical bar takes 15 px from the content width.
    assert_eq!(t.rect(&tree, inner).size.width, px(185));
    assert_eq!(t.rect(&tree, pin).size.width, px(185));
    let f = find(&tree.root, &|f| matches!(&f.kind, FragmentKind::Box { source: StyleSource::Element(n), .. } if *n == sc)).unwrap();
    match &f.kind {
        FragmentKind::Box { scroll: Some(info), .. } => {
            assert_eq!(info.content_height, px(500));
            assert!(info.shows_y_bar && !info.shows_x_bar);
            // Clamped to the scrollable range.
            assert_eq!(info.scroll_y, px(400));
        }
        _ => panic!("no scroll info"),
    }
    // The scroll container's own overflow is clipped to itself.
    assert_eq!(f.overflow, Rect::new(Au::ZERO, Au::ZERO, px(200), px(100)));
}

#[test]
fn root_scrollable_size_and_body_overflow_propagation() {
    let mut t = T::new();
    let _tall = t.div(t.body, |s| s.height = len(2000));
    let wide = t.div(t.body, |s| {
        s.position = Position::Absolute;
        s.inset.left = m(900);
        s.inset.top = m(0);
        s.width = len(50);
        s.height = len(10);
    });
    let tree = t.layout();
    assert_eq!(tree.content_height, px(2000));
    assert_eq!(tree.content_width, px(950));
    assert_eq!(t.rect(&tree, wide).origin.x, px(900));
    // The viewport reserved a vertical scrollbar: the body is 785 wide.
    assert_eq!(t.rect(&tree, t.body).size.width, px(785));
    match &tree.root.kind {
        FragmentKind::Box { scroll: Some(info), .. } => assert!(info.shows_y_bar),
        _ => panic!(),
    }
    // overflow: hidden on body propagates to the viewport: no bar.
    let mut s = t.style_of(t.body);
    s.overflow_y = Overflow::Hidden;
    t.styles.set(t.body, Rc::new(s));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, t.body).size.width, px(800));
    match &tree.root.kind {
        FragmentKind::Box { scroll: Some(info), .. } => assert!(!info.shows_y_bar && info.scroll_y.is_zero()),
        _ => panic!(),
    }
}

#[test]
fn sticky_sticks_within_its_containing_block() {
    let mut t = T::new();
    let cb = t.div(t.body, |s| s.height = len(500));
    let sticky = t.div(cb, |s| {
        s.position = Position::Sticky;
        s.inset.top = m(10);
        s.height = len(20);
    });
    let _after = t.div(t.body, |s| s.height = len(2000));
    let mut scroll = ScrollState::new();
    scroll.insert(Document::ROOT, (Au::ZERO, px(100)));
    let mut cache = LayoutCache::default();
    let vp = Viewport { width: 800, height: 600, scale: 1, zoom: 100 };
    let tree = layout_with(&t.doc, &t.styles, vp, LayoutOptions { images: &super::NoImages, scroll: &scroll }, &mut cache);
    // Scrolled by 100: the box moves to 110 to keep 10 px from the top.
    assert_eq!(t.rect(&tree, sticky).origin.y, px(110));
    scroll.insert(Document::ROOT, (Au::ZERO, px(1000)));
    let tree = layout_with(&t.doc, &t.styles, vp, LayoutOptions { images: &super::NoImages, scroll: &scroll }, &mut cache);
    // Clamped by the containing block's bottom (500 - 20).
    assert_eq!(t.rect(&tree, sticky).origin.y, px(480));
}

// Tables (§17).

fn table(t: &mut T, parent: NodeId, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
    t.el(parent, "table", |s| {
        s.display = Display::Table;
        s.border_spacing = (px(2), px(2));
        f(s)
    })
}
fn row(t: &mut T, parent: NodeId) -> NodeId {
    t.el(parent, "tr", |s| s.display = Display::TableRow)
}
fn cell(t: &mut T, parent: NodeId, txt: &str, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
    let c = t.el(parent, "td", |s| {
        s.display = Display::TableCell;
        s.padding = Sides::uniform(lp(1));
        f(s)
    });
    if !txt.is_empty() {
        t.text(c, txt);
    }
    c
}

#[test]
fn auto_table_columns_from_content_with_anonymous_row_group() {
    let mut t = T::new();
    let body = t.body;
    let tb = table(&mut t, body, |_| {});
    let r1 = row(&mut t, tb);
    let a = cell(&mut t, r1, "aaaa", |_| {});
    let b = cell(&mut t, r1, "b", |_| {});
    let r2 = row(&mut t, tb);
    let c = cell(&mut t, r2, "c", |_| {});
    let d = cell(&mut t, r2, "dddddd", |_| {});
    let tree = t.layout();
    let wa = tw("aaaa") + px(2);
    let wd = tw("dddddd") + px(2);
    assert_eq!(t.rect(&tree, a), Rect::new(px(2), px(2), wa, lh() + px(2)));
    assert_eq!(t.rect(&tree, b), Rect::new(px(4) + wa, px(2), wd, lh() + px(2)));
    assert_eq!(t.rect(&tree, c).size.width, wa);
    assert_eq!(t.rect(&tree, d).origin.y, px(4) + lh() + px(2));
    // Table width shrinks to fit: spacing 3 x 2 + columns.
    assert_eq!(t.rect(&tree, tb).size.width, wa + wd + px(6));
    assert_eq!(t.rect(&tree, tb).size.height, (lh() + px(2)) * 2 + px(6));
    let anon = find(&tree.root, &|f| matches!(f.kind, FragmentKind::Box { source: StyleSource::Anonymous(_), .. }) && f.rect.size.height > Au::ZERO && f.children.len() == 2);
    assert!(anon.is_some(), "anonymous row group\n{}", debug::dump_doc(&t.doc, &tree));
}

#[test]
fn specified_table_width_distributes_extra_to_columns() {
    let mut t = T::new();
    let body = t.body;
    let tb = table(&mut t, body, |s| {
        s.width = len(406);
        s.border_spacing = (px(0), px(0));
    });
    let r1 = row(&mut t, tb);
    let a = cell(&mut t, r1, "aaaa", |_| {});
    let b = cell(&mut t, r1, "bb", |_| {});
    let tree = t.layout();
    let ra = t.rect(&tree, a);
    let rb = t.rect(&tree, b);
    assert_eq!(t.rect(&tree, tb).size.width, px(406));
    assert_eq!(ra.size.width + rb.size.width, px(406));
    // Proportional to max-content widths: the wider cell gets more.
    assert!(ra.size.width > rb.size.width);
    let wa = tw("aaaa") + px(2);
    let wb = tw("bb") + px(2);
    assert_eq!(ra.size.width, px(406).scale(wa.0, (wa + wb).0));
}

#[test]
fn percent_columns_and_fixed_layout() {
    let mut t = T::new();
    let body = t.body;
    let tb = table(&mut t, body, |s| {
        s.width = len(400);
        s.border_spacing = (px(0), px(0));
    });
    let r1 = row(&mut t, tb);
    let a = cell(&mut t, r1, "a", |s| s.width = pct(25));
    let b = cell(&mut t, r1, "b", |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, px(100), "{}", debug::dump_doc(&t.doc, &tree));
    assert_eq!(t.rect(&tree, b).size.width, px(300), "{}", debug::dump_doc(&t.doc, &tree));

    let mut t = T::new();
    let tb = table(&mut t, body, |s| {
        s.width = len(400);
        s.border_spacing = (px(0), px(0));
        s.table_layout = TableLayout::Fixed;
    });
    let r1 = row(&mut t, tb);
    let a = cell(&mut t, r1, "aaaaaaaaaaaaaaaaaaaaaaa", |s| s.width = len(50));
    let b = cell(&mut t, r1, "b", |_| {});
    let c = cell(&mut t, r1, "c", |_| {});
    let tree = t.layout();
    // The cell's content width plus its padding fixes the column; the rest share.
    assert_eq!(t.rect(&tree, a).size.width, px(52));
    assert_eq!(t.rect(&tree, b).size.width, px(174));
    assert_eq!(t.rect(&tree, c).size.width, px(174));
    assert_eq!(t.rect(&tree, tb).size.width, px(400));
}

#[test]
fn colspan_and_rowspan_distribute_sizes() {
    let mut t = T::new();
    let body = t.body;
    let tb = table(&mut t, body, |s| s.border_spacing = (px(0), px(0)));
    let r1 = row(&mut t, tb);
    let wide = t.el_attrs(r1, "td", vec![("colspan", "2")], |s| {
        s.display = Display::TableCell;
        s.width = len(200);
    });
    let tall = t.el_attrs(r1, "td", vec![("rowspan", "2")], |s| {
        s.display = Display::TableCell;
        s.height = len(100);
    });
    let r2 = row(&mut t, tb);
    let a = cell(&mut t, r2, "", |s| {
        s.width = len(30);
        s.padding = Sides::uniform(LengthPercentage::ZERO);
        s.height = len(10);
    });
    let b = cell(&mut t, r2, "", |s| {
        s.width = len(30);
        s.padding = Sides::uniform(LengthPercentage::ZERO);
    });
    let tree = t.layout();
    // The 200 px span is split over two 30 px columns: each grows to 100.
    assert_eq!(t.rect(&tree, wide).size.width, px(200));
    assert_eq!(t.rect(&tree, a).size.width, px(100));
    assert_eq!(t.rect(&tree, b).origin.x, px(100));
    // The rowspan cell's 100 px is spread over both rows.
    assert_eq!(t.rect(&tree, tall).size.height, px(100));
    assert_eq!(t.rect(&tree, r1).size.height + t.rect(&tree, r2).size.height, px(100));
    assert_eq!(t.rect(&tree, tb).size.height, px(100));
}

#[test]
fn cell_vertical_align_and_row_baseline() {
    let mut t = T::new();
    let body = t.body;
    let tb = table(&mut t, body, |s| s.border_spacing = (px(0), px(0)));
    let r1 = row(&mut t, tb);
    let tall = cell(&mut t, r1, "", |s| {
        s.height = len(100);
        s.padding = Sides::uniform(LengthPercentage::ZERO);
    });
    let top = cell(&mut t, r1, "t", |s| s.vertical_align = VerticalAlign::Top);
    let mid = cell(&mut t, r1, "m", |s| s.vertical_align = VerticalAlign::Middle);
    let bot = cell(&mut t, r1, "b", |s| s.vertical_align = VerticalAlign::Bottom);
    let big = cell(&mut t, r1, "B", |s| s.font.size = px(32));
    let base = cell(&mut t, r1, "s", |_| {});
    let tree = t.layout();
    let _ = tall;
    let ts = texts(&tree);
    let y_of = |s: &str| ts.iter().find(|(x, _)| x == s).unwrap().1.origin.y;
    let row_h = t.rect(&tree, r1).size.height;
    assert_eq!(row_h, px(100));
    assert_eq!(t.rect(&tree, top).size.height, px(100));
    assert_eq!(y_of("t"), px(1));
    assert_eq!(y_of("m"), (px(100) - lh() - px(2)) / 2 + px(1));
    assert_eq!(y_of("b"), px(100) - px(1) - lh());
    // Baseline cells share the row baseline set by the big font.
    let big_fm = text::font_metrics(&t.style_of(big).font);
    let big_lh = big_fm.normal_line_height();
    let big_half = (big_lh - big_fm.content_height()) / 2;
    let small = text::font_metrics(&font());
    let row_baseline = px(1) + big_half + big_fm.ascent;
    assert_eq!(y_of("B") + big_fm.ascent, row_baseline);
    assert_eq!(y_of("s") + small.ascent, row_baseline);
    let _ = (mid, bot, base);
}

#[test]
fn collapsed_borders_resolve_conflicts_and_halve_geometry() {
    let mut t = T::new();
    let body = t.body;
    let tb = table(&mut t, body, |s| {
        s.border_collapse = BorderCollapse::Collapse;
        s.border = Sides::uniform(side(10));
        s.padding = Sides::uniform(lp(5));
    });
    let r1 = row(&mut t, tb);
    let a = cell(&mut t, r1, "a", |s| {
        s.border = Sides::uniform(side(2));
        s.padding = Sides::uniform(LengthPercentage::ZERO);
        s.width = len(50);
        s.height = len(20);
    });
    let b = cell(&mut t, r1, "b", |s| {
        s.border = Sides::uniform(side(4));
        s.border.top = BorderSide { width: px(6), style: BorderStyle::Hidden, color: cw_scene::Color(0, 0, 0, 255) };
        s.padding = Sides::uniform(LengthPercentage::ZERO);
        s.width = len(50);
        s.height = len(20);
    });
    let tree = t.layout();
    let fa = find(&tree.root, &|f| matches!(&f.kind, FragmentKind::Box { source: StyleSource::Element(n), .. } if *n == a)).unwrap();
    let fb = find(&tree.root, &|f| matches!(&f.kind, FragmentKind::Box { source: StyleSource::Element(n), .. } if *n == b)).unwrap();
    let ft = find(&tree.root, &|f| matches!(&f.kind, FragmentKind::Box { source: StyleSource::Element(n), .. } if *n == tb)).unwrap();
    let ca = fa.collapsed_borders.as_ref().unwrap();
    let cbb = fb.collapsed_borders.as_ref().unwrap();
    // Table's 10 px wins on the outer edges; between the cells the wider 4 px wins.
    assert_eq!(ca.left.width, px(10));
    assert_eq!(ca.right.width, px(4));
    assert_eq!(cbb.left.width, px(4));
    // hidden wins over everything.
    assert_eq!(cbb.top.style, BorderStyle::Hidden);
    assert_eq!(ca.top.width, px(10));
    // Half widths inside the cells; padding and spacing are gone.
    if let FragmentKind::Box { border, padding, .. } = &fa.kind {
        assert_eq!(border.left, px(5));
        assert_eq!(border.right, px(2));
        assert_eq!(padding.left, Au::ZERO);
    }
    if let FragmentKind::Box { border, padding, .. } = &ft.kind {
        assert_eq!(border.left, px(5));
        assert_eq!(padding.left, Au::ZERO);
    }
    // Cells abut: b starts where a ends.
    assert_eq!(t.rect(&tree, b).origin.x, t.rect(&tree, a).right());
    assert_eq!(t.rect(&tree, a).origin.x, px(5));
}

#[test]
fn captions_and_table_margins_go_on_the_wrapper() {
    let mut t = T::new();
    let body = t.body;
    let tb = table(&mut t, body, |s| {
        s.margin.left = LengthPercentageAuto::Auto;
        s.margin.right = LengthPercentageAuto::Auto;
        s.margin.top = m(10);
        s.width = len(200);
        s.border_spacing = (px(0), px(0));
    });
    let cap = t.el(tb, "caption", |s| {
        s.display = Display::TableCaption;
        s.height = len(30);
    });
    t.text(cap, "cap");
    let r1 = row(&mut t, tb);
    let c = cell(&mut t, r1, "x", |s| s.height = len(20));
    let tree = t.layout();
    // Wrapper centred: (800 - 200) / 2; grid below the caption.
    assert_eq!(t.rect(&tree, cap), r(300, 10, 200, 30));
    assert_eq!(t.rect(&tree, tb), r(300, 40, 200, 22));
    assert_eq!(t.rect(&tree, c).origin.y, px(40));
    // The wrapper's top margin collapsed through the body's.
    assert_eq!(t.rect(&tree, t.body), r(0, 10, 800, 52));
    assert_eq!(t.rect(&tree, t.html).size.height, px(62));
}

#[test]
fn stray_cells_get_anonymous_table_and_row() {
    let mut t = T::new();
    let d = t.div(t.body, |s| s.border_spacing = (px(0), px(0)));
    let c = cell(&mut t, d, "x", |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, c).size.height, lh() + px(2));
    assert_eq!(t.rect(&tree, d).size.height, lh() + px(2));
}

#[test]
fn inline_table_is_atomic_in_a_line() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "x ");
    let tb = t.el(p, "table", |s| {
        s.display = Display::InlineTable;
        s.border_spacing = (px(0), px(0));
    });
    let r1 = row(&mut t, tb);
    let c = cell(&mut t, r1, "cell", |_| {});
    t.text(p, " y");
    let tree = t.layout();
    let ts = texts(&tree);
    let y_of = |s: &str| ts.iter().find(|(x, _)| x.trim() == s).unwrap_or_else(|| panic!("{s}: {}", debug::dump_doc(&t.doc, &tree))).1.origin.y;
    // "x" and "y" share the line the table sits on; the table's baseline is its
    // first row's, so the line grows by the cell padding.
    assert_eq!(y_of("x"), y_of("y"));
    assert_eq!(y_of("x"), px(1));
    assert_eq!(t.rect(&tree, p).size.height, lh() + px(2));
    assert_eq!(t.rect(&tree, tb).origin.x, tw("x "));
    assert_eq!(t.rect(&tree, c).size.width, tw("cell") + px(2));
}

// Intrinsic sizes, debug dump, cache and performance.

#[test]
fn shrink_to_fit_uses_min_and_max_content() {
    let mut t = T::new();
    let f = t.div(t.body, |s| s.float = Float::Left);
    t.text(f, "one two");
    let g = t.div(t.body, |s| {
        s.float = Float::Left;
        s.width = pct(50);
    });
    let narrow = t.div(t.body, |s| {
        s.width = len(40);
        s.clear = Clear::Both;
    });
    let h = t.div(narrow, |s| s.float = Float::Left);
    t.text(h, "one two");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f).size.width, tw("one two"));
    assert_eq!(t.rect(&tree, g).size.width, px(400));
    // Available width 40 < max-content: the float wraps to the widest word.
    assert_eq!(t.rect(&tree, h).size.width, tw("one").max(tw("two")).max(px(40).min(tw("one two"))));
    assert_eq!(t.rect(&tree, h).size.height, lh() * 2);
}

#[test]
fn min_max_content_keywords_for_width() {
    let mut t = T::new();
    let a = t.div(t.body, |s| s.width = Sizing::MaxContent);
    t.text(a, "one two");
    let b = t.div(t.body, |s| s.width = Sizing::MinContent);
    t.text(b, "one two");
    let c = t.div(t.body, |s| s.width = Sizing::FitContent);
    t.text(c, "one two");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, tw("one two"));
    assert_eq!(t.rect(&tree, b).size.width, tw("one").max(tw("two")));
    assert_eq!(t.rect(&tree, c).size.width, tw("one two"));
}

#[test]
fn debug_dump_lists_fragments() {
    let mut t = T::new();
    let p = t.div(t.body, |s| s.padding = Sides::uniform(lp(4)));
    t.text(p, "hi");
    let tree = t.layout();
    let d = debug::dump_doc(&t.doc, &tree);
    assert!(d.starts_with("viewport 800x600"), "{d}");
    assert!(d.contains("Box elem#1(html)"), "{d}");
    assert!(d.contains("pad[4 4 4 4]"), "{d}");
    assert!(d.contains("Line "), "{d}");
    assert!(d.contains("Text elem#3(div)"), "{d}");
    assert!(d.contains("\"hi\""), "{d}");
    let plain = debug::dump(&tree);
    assert!(plain.contains("Box elem#1 "), "{plain}");
}

#[test]
fn hit_testing_and_rects_of() {
    let mut t = T::new();
    let a = t.div(t.body, |s| s.height = len(50));
    let b = t.div(t.body, |s| s.height = len(50));
    let tree = t.layout();
    let hits = tree.hit(px(10), px(60));
    let last = hits.last().unwrap();
    assert!(matches!(&last.0.kind, FragmentKind::Box { source: StyleSource::Element(n), .. } if *n == b));
    assert_eq!(tree.rects_of(a).len(), 1);
}

#[test]
fn float_context_placement_rules() {
    let mut bfc = Bfc::new();
    let s = |w, h| Size { width: px(w), height: px(h) };
    let p1 = bfc.place(Float::Left, s(100, 50), Au::ZERO, Au::ZERO, px(300));
    let p2 = bfc.place(Float::Right, s(100, 30), Au::ZERO, Au::ZERO, px(300));
    // 100 + 100 + 150 > 300: goes below the shorter right float.
    let p3 = bfc.place(Float::Left, s(150, 10), Au::ZERO, Au::ZERO, px(300));
    assert_eq!((p1.x, p1.y), (px(0), px(0)));
    assert_eq!((p2.x, p2.y), (px(200), px(0)));
    assert_eq!((p3.x, p3.y), (px(100), px(30)));
    assert_eq!(bfc.available(px(5), Au::ZERO, px(300)), (px(100), px(200)));
    assert_eq!(bfc.available(px(35), Au::ZERO, px(300)), (px(250), px(300)));
    assert_eq!(bfc.clear_y(Clear::Right), Some(px(30)));
    assert_eq!(bfc.clear_y(Clear::Both), Some(px(50)));
    assert_eq!(bfc.next_change(px(35)), Some(px(40)));
}

#[test]
fn large_document_lays_out_quickly() {
    let mut t = T::new();
    let mut parent = t.body;
    for i in 0..5000 {
        let d = t.div(parent, |s| {
            s.padding = Sides::uniform(lp(1));
            s.margin.top = m(2);
        });
        t.text(d, "some text in a block with several words to wrap around");
        if i % 50 == 0 {
            let sp = t.span(d, |s| s.font.weight = 700);
            t.text(sp, "bold");
            let f = t.div(d, |s| {
                s.float = Float::Right;
                s.width = len(20);
                s.height = len(20);
            });
            let _ = f;
        }
        if i % 500 == 0 {
            parent = t.div(t.body, |s| s.width = len(700));
        }
    }
    let start = std::time::Instant::now();
    let tree = t.layout();
    let elapsed = start.elapsed();
    eprintln!("layout of 5000 elements: {elapsed:?}");
    assert!(tree.content_height > px(5000));
    // Debug builds are slower; release lays this out well under 100 ms.
    let limit = if cfg!(debug_assertions) { 4000 } else { 100 };
    assert!(elapsed.as_millis() < limit, "layout took {elapsed:?}");
}

#[test]
fn zoom_scales_the_viewport() {
    let mut t = T::new();
    let d = t.div(t.body, |s| s.height = len(10));
    let tree = layout(&t.doc, &t.styles, Viewport { width: 800, height: 600, scale: 1, zoom: 200 });
    assert_eq!(t.rect(&tree, d).size.width, px(400));
    assert_eq!(tree.viewport_width, px(400));
    let _ = t.layout_sized(100, 100);
}
