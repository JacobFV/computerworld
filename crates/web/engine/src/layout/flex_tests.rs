//! Flexbox tests with hand-built documents and style sets; expected values are
//! computed by hand from css-flexbox-1 and asserted in exact `Au`.

use std::rc::Rc;

use super::mul_div;
use crate::dom::{Document, NodeId, QuirksMode};
use crate::geom::{Au, Rect};
use crate::layout::debug;
use crate::layout::fragment::{Fragment, FragmentKind, FragmentTree, StyleSource};
use crate::layout::text;
use crate::layout::{layout, layout_with, ImageSizeMap, LayoutCache, LayoutOptions, ScrollState};
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
    BorderSide {
        width: px(w),
        style: BorderStyle::Solid,
        color: cw_scene::Color(0, 0, 0, 255),
    }
}
fn font() -> Font {
    ComputedStyle::initial().font
}
fn tw(s: &str) -> Au {
    text::measure(&font(), s, Au::ZERO, Au::ZERO)
}
fn lh() -> Au {
    text::font_metrics(&font()).normal_line_height()
}
fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(px(x), px(y), px(w), px(h))
}

struct T {
    doc: Document,
    styles: StyleSet,
    body: NodeId,
}

impl T {
    fn new() -> T {
        let mut doc = Document::new();
        doc.quirks = QuirksMode::NoQuirks;
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
        T { doc, styles, body }
    }
    fn style_of(&self, n: NodeId) -> ComputedStyle {
        self.styles
            .get(n)
            .cloned()
            .unwrap_or_else(ComputedStyle::initial)
    }
    fn el(&mut self, parent: NodeId, tag: &str, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
        let n = self.doc.create_element(tag, vec![]);
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
    /// A `display: flex` container with this width and height (`None` = auto).
    fn flex(
        &mut self,
        parent: NodeId,
        w: i32,
        h: Option<i32>,
        f: impl FnOnce(&mut ComputedStyle),
    ) -> NodeId {
        self.div(parent, |s| {
            s.display = Display::Flex;
            s.width = len(w);
            if let Some(h) = h {
                s.height = len(h);
            }
            f(s)
        })
    }
    /// An item with a fixed size.
    fn item(
        &mut self,
        parent: NodeId,
        w: i32,
        h: i32,
        f: impl FnOnce(&mut ComputedStyle),
    ) -> NodeId {
        self.div(parent, |s| {
            s.width = len(w);
            s.height = len(h);
            f(s)
        })
    }
    fn text(&mut self, parent: NodeId, t: &str) -> NodeId {
        let n = self.doc.create_text(t);
        self.doc.append(parent, n);
        n
    }
    fn layout(&self) -> FragmentTree {
        layout(
            &self.doc,
            &self.styles,
            Viewport {
                width: 800,
                height: 600,
                scale: 1,
                zoom: 100,
            },
        )
    }
    fn rect(&self, tree: &FragmentTree, n: NodeId) -> Rect {
        let rs = tree.rects_of(n);
        assert!(
            !rs.is_empty(),
            "no fragment for node {n:?}\n{}",
            debug::dump_doc(&self.doc, tree)
        );
        rs[0]
    }
}

fn find<'a>(f: &'a Fragment, pred: &dyn Fn(&Fragment) -> bool) -> Option<&'a Fragment> {
    if pred(f) {
        return Some(f);
    }
    f.children.iter().find_map(|c| find(c, pred))
}

/// `(text, absolute rect, absolute baseline y)` of every text run.
fn texts(tree: &FragmentTree) -> Vec<(String, Rect, Au)> {
    let mut out = Vec::new();
    tree.root.walk(Default::default(), &mut |f, r| {
        if let FragmentKind::Text { text, baseline, .. } = &f.kind {
            out.push((text.clone(), r, r.origin.y + *baseline));
        }
    });
    out
}

fn box_of(tree: &FragmentTree, n: NodeId) -> Fragment {
    find(
        &tree.root,
        &|f| matches!(f.kind, FragmentKind::Box { source: StyleSource::Element(x), .. } if x == n),
    )
    .cloned()
    .expect("box fragment")
}

fn box_baseline(f: &Fragment) -> Option<Au> {
    match &f.kind {
        FragmentKind::Box { baseline, .. } => *baseline,
        _ => None,
    }
}

// §9.7: resolving flexible lengths.

#[test]
fn three_growing_items_share_free_space_equally() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(100), |_| {});
    let a = t.item(c, 50, 20, |s| s.flex_grow = 1000);
    let b = t.item(c, 50, 20, |s| s.flex_grow = 1000);
    let d = t.item(c, 50, 20, |s| s.flex_grow = 1000);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 20));
    assert_eq!(t.rect(&tree, b), r(100, 0, 100, 20));
    assert_eq!(t.rect(&tree, d), r(200, 0, 100, 20));
    assert_eq!(t.rect(&tree, c), r(0, 0, 300, 100));
}

#[test]
fn grow_factors_split_proportionally_from_zero_basis() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(50), |_| {});
    let a = t.div(c, |s| {
        s.flex_basis = len(0);
        s.flex_grow = 1000;
    });
    let b = t.div(c, |s| {
        s.flex_basis = len(0);
        s.flex_grow = 2000;
    });
    let d = t.div(c, |s| {
        s.flex_basis = len(0);
        s.flex_grow = 1000;
    });
    let tree = t.layout();
    // Stretched to the container's definite cross size.
    assert_eq!(t.rect(&tree, a), r(0, 0, 75, 50));
    assert_eq!(t.rect(&tree, b), r(75, 0, 150, 50));
    assert_eq!(t.rect(&tree, d), r(225, 0, 75, 50));
}

#[test]
fn shrink_uses_scaled_flex_shrink_factors() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |_| {});
    let a = t.item(c, 300, 20, |_| {});
    let b = t.item(c, 100, 20, |_| {});
    let tree = t.layout();
    // Negative free space 100, scaled factors 300:100 → 75 and 25 removed.
    assert_eq!(t.rect(&tree, a), r(0, 0, 225, 20));
    assert_eq!(t.rect(&tree, b), r(225, 0, 75, 20));
}

#[test]
fn shrink_with_different_factors() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |_| {});
    let a = t.item(c, 200, 20, |s| s.flex_shrink = 3000);
    let b = t.item(c, 200, 20, |s| s.flex_shrink = 1000);
    let tree = t.layout();
    // Scaled: 600 vs 200 → a loses 75, b loses 25.
    assert_eq!(t.rect(&tree, a).size.width, px(125));
    assert_eq!(t.rect(&tree, b).size.width, px(175));
}

#[test]
fn max_violation_freezes_item_and_redistributes() {
    let mut t = T::new();
    let c = t.flex(t.body, 600, Some(20), |_| {});
    let a = t.item(c, 50, 20, |s| {
        s.flex_grow = 1000;
        s.max_width = len(120);
    });
    let b = t.item(c, 50, 20, |s| s.flex_grow = 1000);
    let d = t.item(c, 50, 20, |s| s.flex_grow = 1000);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 120, 20));
    assert_eq!(t.rect(&tree, b), r(120, 0, 240, 20));
    assert_eq!(t.rect(&tree, d), r(360, 0, 240, 20));
}

#[test]
fn min_violation_freezes_item_and_reshrinks_the_rest() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |_| {});
    let a = t.item(c, 200, 20, |s| s.min_width = len(180));
    let b = t.item(c, 200, 20, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 180, 20));
    assert_eq!(t.rect(&tree, b), r(180, 0, 120, 20));
}

#[test]
fn flex_1_1_0_equalises_but_flex_auto_keeps_widths() {
    let mut t = T::new();
    let c1 = t.flex(t.body, 300, Some(20), |_| {});
    let a = t.item(c1, 100, 20, |s| {
        s.flex_grow = 1000;
        s.flex_basis = len(0);
    });
    let b = t.item(c1, 200, 20, |s| {
        s.flex_grow = 1000;
        s.flex_basis = len(0);
    });
    let c2 = t.flex(t.body, 400, Some(20), |_| {});
    let d = t.item(c2, 100, 20, |s| s.flex_grow = 1000);
    let e = t.item(c2, 200, 20, |s| s.flex_grow = 1000);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, px(150));
    assert_eq!(t.rect(&tree, b).size.width, px(150));
    assert_eq!(t.rect(&tree, d), r(0, 20, 150, 20));
    assert_eq!(t.rect(&tree, e), r(150, 20, 250, 20));
}

#[test]
fn flex_basis_content_ignores_width() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |_| {});
    let a = t.div(c, |s| {
        s.width = len(10);
        s.flex_basis = Sizing::MaxContent;
    });
    t.text(a, "Hello");
    let b = t.div(c, |s| s.width = len(10));
    t.text(b, "Hello");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, tw("Hello"));
    // `min-width: auto` is min(specified 10, content): the specified size wins.
    assert_eq!(
        t.rect(&tree, b),
        Rect::new(tw("Hello"), Au::ZERO, px(10), px(20))
    );
}

#[test]
fn percentage_flex_basis_and_border_box_basis() {
    let mut t = T::new();
    let c = t.flex(t.body, 400, Some(20), |_| {});
    let a = t.div(c, |s| s.flex_basis = pct(25));
    let b = t.div(c, |s| {
        s.flex_basis = len(100);
        s.box_sizing = BoxSizing::BorderBox;
        s.padding = Sides::uniform(lp(10));
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 20));
    assert_eq!(t.rect(&tree, b), r(100, 0, 100, 20));
}

#[test]
fn gaps_count_in_free_space_with_exact_rounding() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |s| s.column_gap = lp(10));
    let a = t.item(c, 50, 20, |s| s.flex_grow = 1000);
    let b = t.item(c, 50, 20, |s| s.flex_grow = 1000);
    let d = t.item(c, 50, 20, |s| s.flex_grow = 1000);
    let tree = t.layout();
    // Free space 130 px = 8320 Au shared as 2773 + 2774 + 2773.
    let ra = t.rect(&tree, a);
    let rb = t.rect(&tree, b);
    let rd = t.rect(&tree, d);
    assert_eq!(ra.size.width, Au(50 * 64 + 2773));
    assert_eq!(rb.size.width, Au(50 * 64 + 2774));
    assert_eq!(rd.size.width, Au(50 * 64 + 2773));
    assert_eq!(rb.origin.x, ra.right() + px(10));
    assert_eq!(rd.origin.x, rb.right() + px(10));
    assert_eq!(rd.right(), px(300));
}

#[test]
fn zero_shrink_items_overflow_the_container() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |_| {});
    let a = t.item(c, 200, 20, |s| s.flex_shrink = 0);
    let b = t.item(c, 200, 20, |s| s.flex_shrink = 0);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 20));
    assert_eq!(t.rect(&tree, b), r(200, 0, 200, 20));
    let cf = box_of(&tree, c);
    assert_eq!(cf.overflow.size.width, px(400));
}

// §9.3: lines.

#[test]
fn wrap_into_three_lines_with_gaps() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |s| {
        s.flex_wrap = FlexWrap::Wrap;
        s.column_gap = lp(10);
        s.row_gap = lp(5);
    });
    let items: Vec<NodeId> = (0..6).map(|_| t.item(c, 100, 50, |_| {})).collect();
    let tree = t.layout();
    for (i, &n) in items.iter().enumerate() {
        let (col, row) = (i % 2, i / 2);
        assert_eq!(
            t.rect(&tree, n),
            r(col as i32 * 110, row as i32 * 55, 100, 50),
            "item {i}"
        );
    }
    assert_eq!(t.rect(&tree, c), r(0, 0, 300, 160));
}

#[test]
fn align_content_values_on_two_lines() {
    let cases: [(AlignContent, i32, i32); 6] = [
        (AlignContent::FlexStart, 0, 50),
        (AlignContent::FlexEnd, 200, 250),
        (AlignContent::Center, 100, 150),
        (AlignContent::SpaceBetween, 0, 250),
        (AlignContent::SpaceAround, 50, 200),
        (AlignContent::Stretch, 0, 150),
    ];
    for (ac, y0, y1) in cases {
        let mut t = T::new();
        let c = t.flex(t.body, 300, Some(300), |s| {
            s.flex_wrap = FlexWrap::Wrap;
            s.align_content = ac;
        });
        let items: Vec<NodeId> = (0..4).map(|_| t.item(c, 150, 50, |_| {})).collect();
        let tree = t.layout();
        assert_eq!(t.rect(&tree, items[0]).origin.y, px(y0), "{ac:?}");
        assert_eq!(
            t.rect(&tree, items[3]).origin,
            crate::geom::Point {
                x: px(150),
                y: px(y1)
            },
            "{ac:?}"
        );
    }
    // space-evenly: 200/3 between the edges and the lines.
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(300), |s| {
        s.flex_wrap = FlexWrap::Wrap;
        s.align_content = AlignContent::SpaceEvenly;
    });
    let items: Vec<NodeId> = (0..4).map(|_| t.item(c, 150, 50, |_| {})).collect();
    let tree = t.layout();
    assert_eq!(t.rect(&tree, items[0]).origin.y, Au(4267));
    assert_eq!(t.rect(&tree, items[2]).origin.y, Au(11733));
}

#[test]
fn align_content_stretch_grows_lines_and_stretched_items() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(300), |s| s.flex_wrap = FlexWrap::Wrap);
    let a = t.div(c, |s| s.width = len(300));
    let b = t.div(c, |s| s.width = len(300));
    let tree = t.layout();
    // Two empty lines of 0 stretched by 150 each; the items stretch to the lines.
    assert_eq!(t.rect(&tree, a), r(0, 0, 300, 150));
    assert_eq!(t.rect(&tree, b), r(0, 150, 300, 150));
}

#[test]
fn wrap_reverse_stacks_lines_from_the_bottom() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(200), |s| {
        s.flex_wrap = FlexWrap::WrapReverse
    });
    let items: Vec<NodeId> = (0..4).map(|_| t.item(c, 150, 50, |_| {})).collect();
    let tree = t.layout();
    // Lines stretched to 100 each; the first line sits at the cross-start (bottom)
    // and its items flush with the line's cross-start edge.
    assert_eq!(t.rect(&tree, items[0]), r(0, 150, 150, 50));
    assert_eq!(t.rect(&tree, items[1]), r(150, 150, 150, 50));
    assert_eq!(t.rect(&tree, items[2]), r(0, 50, 150, 50));
    assert_eq!(t.rect(&tree, items[3]), r(150, 50, 150, 50));
}

// §9.5: justify-content.

#[test]
fn justify_content_values_with_positive_free_space() {
    let cases: [(JustifyContent, [i32; 3]); 6] = [
        (JustifyContent::FlexStart, [0, 50, 100]),
        (JustifyContent::FlexEnd, [150, 200, 250]),
        (JustifyContent::Center, [75, 125, 175]),
        (JustifyContent::SpaceBetween, [0, 125, 250]),
        (JustifyContent::SpaceAround, [25, 125, 225]),
        (JustifyContent::Stretch, [0, 50, 100]),
    ];
    for (jc, xs) in cases {
        let mut t = T::new();
        let c = t.flex(t.body, 300, Some(20), |s| s.justify_content = jc);
        let items: Vec<NodeId> = (0..3).map(|_| t.item(c, 50, 20, |_| {})).collect();
        let tree = t.layout();
        for (i, &n) in items.iter().enumerate() {
            assert_eq!(t.rect(&tree, n).origin.x, px(xs[i]), "{jc:?} item {i}");
        }
    }
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |s| {
        s.justify_content = JustifyContent::SpaceEvenly
    });
    let items: Vec<NodeId> = (0..3).map(|_| t.item(c, 50, 20, |_| {})).collect();
    let tree = t.layout();
    assert_eq!(t.rect(&tree, items[0]).origin.x, Au(2400)); // 37.5 px
    assert_eq!(t.rect(&tree, items[1]).origin.x, px(125));
    assert_eq!(t.rect(&tree, items[2]).origin.x, Au(212 * 64 + 32));
}

#[test]
fn justify_content_with_negative_free_space_uses_fallbacks() {
    // Three inflexible 150 px items in 300 px: free space -150.
    let cases: [(JustifyContent, [i32; 3]); 6] = [
        (JustifyContent::FlexStart, [0, 150, 300]),
        (JustifyContent::FlexEnd, [-150, 0, 150]),
        (JustifyContent::Center, [-75, 75, 225]),
        (JustifyContent::SpaceBetween, [0, 150, 300]),
        (JustifyContent::SpaceAround, [-75, 75, 225]),
        (JustifyContent::SpaceEvenly, [-75, 75, 225]),
    ];
    for (jc, xs) in cases {
        let mut t = T::new();
        let c = t.flex(t.body, 300, Some(20), |s| s.justify_content = jc);
        let items: Vec<NodeId> = (0..3)
            .map(|_| t.item(c, 150, 20, |s| s.flex_shrink = 0))
            .collect();
        let tree = t.layout();
        for (i, &n) in items.iter().enumerate() {
            assert_eq!(t.rect(&tree, n).origin.x, px(xs[i]), "{jc:?} item {i}");
        }
    }
}

#[test]
fn justify_start_end_left_right_in_row_reverse() {
    let cases: [(JustifyContent, i32); 4] = [
        (JustifyContent::Start, 0),
        (JustifyContent::End, 250),
        (JustifyContent::Left, 0),
        (JustifyContent::Right, 250),
    ];
    for (jc, x) in cases {
        let mut t = T::new();
        let c = t.flex(t.body, 300, Some(20), |s| {
            s.justify_content = jc;
            s.flex_direction = FlexDirection::RowReverse;
        });
        let a = t.item(c, 50, 20, |_| {});
        let tree = t.layout();
        assert_eq!(t.rect(&tree, a).origin.x, px(x), "{jc:?}");
    }
}

#[test]
fn row_reverse_lays_items_from_the_right() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |s| {
        s.flex_direction = FlexDirection::RowReverse
    });
    let a = t.item(c, 50, 20, |_| {});
    let b = t.item(c, 50, 20, |_| {});
    let d = t.item(c, 50, 20, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).origin.x, px(250));
    assert_eq!(t.rect(&tree, b).origin.x, px(200));
    assert_eq!(t.rect(&tree, d).origin.x, px(150));
}

#[test]
fn auto_margins_absorb_main_free_space_before_justify() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |s| {
        s.justify_content = JustifyContent::Center
    });
    let a = t.item(c, 50, 20, |_| {});
    let b = t.item(c, 50, 20, |s| s.margin.left = LengthPercentageAuto::Auto);
    let d = t.item(c, 50, 20, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).origin.x, px(0));
    assert_eq!(t.rect(&tree, b).origin.x, px(200));
    assert_eq!(t.rect(&tree, d).origin.x, px(250));
    // Two auto margins split the space.
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |_| {});
    let a = t.item(c, 50, 20, |s| {
        s.margin.left = LengthPercentageAuto::Auto;
        s.margin.right = LengthPercentageAuto::Auto;
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).origin.x, px(125));
}

#[test]
fn auto_margins_in_the_cross_axis_center_or_push() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(100), |_| {});
    let a = t.item(c, 50, 50, |s| {
        s.margin.top = LengthPercentageAuto::Auto;
        s.margin.bottom = LengthPercentageAuto::Auto;
    });
    let b = t.item(c, 50, 50, |s| s.margin.top = LengthPercentageAuto::Auto);
    let d = t.item(c, 50, 150, |s| s.margin.top = LengthPercentageAuto::Auto);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).origin.y, px(25));
    assert_eq!(t.rect(&tree, b).origin.y, px(50));
    // Negative free space: the auto margin is zero and the item starts at the top.
    assert_eq!(t.rect(&tree, d).origin.y, px(0));
}

// §9.4 and §8.3: cross sizes and align-items / align-self.

#[test]
fn align_items_and_align_self_positions() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(100), |s| {
        s.align_items = AlignItems::FlexEnd
    });
    let a = t.item(c, 50, 50, |_| {});
    let b = t.item(c, 50, 50, |s| s.align_self = AlignSelf::Center);
    let d = t.item(c, 50, 50, |s| s.align_self = AlignSelf::FlexStart);
    let e = t.div(c, |s| {
        s.width = len(50);
        s.align_self = AlignSelf::Stretch;
    });
    let f = t.div(c, |s| {
        s.width = len(50);
        s.max_height = len(60);
        s.align_self = AlignSelf::Stretch;
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).origin.y, px(50));
    assert_eq!(t.rect(&tree, b).origin.y, px(25));
    assert_eq!(t.rect(&tree, d).origin.y, px(0));
    assert_eq!(t.rect(&tree, e), r(150, 0, 50, 100));
    // Stretch is clamped by max-height and the item stays at the cross-start.
    assert_eq!(t.rect(&tree, f), r(200, 0, 50, 60));
}

#[test]
fn stretched_item_percent_children_and_definite_cross_size() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(200), |_| {});
    let a = t.div(c, |s| s.width = len(100));
    let inner = t.div(a, |s| s.height = pct(50));
    let b = t.div(c, |s| {
        s.width = len(100);
        s.height = pct(25);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 200));
    assert_eq!(t.rect(&tree, inner), r(0, 0, 100, 100));
    assert_eq!(t.rect(&tree, b), r(100, 0, 100, 50));
}

#[test]
fn baseline_alignment_with_different_font_sizes() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |s| s.align_items = AlignItems::Baseline);
    let a = t.div(c, |_| {});
    t.text(a, "small");
    let b = t.div(c, |s| s.font.size = px(32));
    t.text(b, "big");
    let tree = t.layout();
    let ts = texts(&tree);
    let small = ts.iter().find(|(s, _, _)| s == "small").unwrap();
    let big = ts.iter().find(|(s, _, _)| s == "big").unwrap();
    assert_eq!(
        small.2,
        big.2,
        "baselines line up\n{}",
        debug::dump_doc(&t.doc, &tree)
    );
    assert_eq!(t.rect(&tree, b).origin.y, px(0));
    assert!(t.rect(&tree, a).origin.y > px(0));
    // The line is as tall as the tallest baseline-aligned extent; the container's
    // baseline is the first item's.
    let cf = box_of(&tree, c);
    assert_eq!(box_baseline(&cf), Some(small.2));
}

#[test]
fn baseline_of_item_without_text_is_its_bottom_edge() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |s| s.align_items = AlignItems::Baseline);
    let a = t.div(c, |_| {});
    t.text(a, "x");
    let b = t.item(c, 50, 40, |_| {});
    let tree = t.layout();
    let ts = texts(&tree);
    let x = ts.iter().find(|(s, _, _)| s == "x").unwrap();
    // The empty box's synthesized baseline (its bottom, 40) is the lowest.
    assert_eq!(t.rect(&tree, b).origin.y, px(0));
    assert_eq!(x.2, px(40));
    assert_eq!(
        t.rect(&tree, c).size.height,
        px(40).max(t.rect(&tree, a).bottom())
    );
}

#[test]
fn single_line_cross_size_is_clamped_by_container_min_height() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |s| s.min_height = len(200));
    let a = t.div(c, |s| s.width = len(50));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 50, 200));
    assert_eq!(t.rect(&tree, c).size.height, px(200));
}

// Order, direction, column.

#[test]
fn order_sorts_items_stably_for_layout_and_painting() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |_| {});
    let a = t.item(c, 50, 20, |s| s.order = 2);
    let b = t.item(c, 50, 20, |_| {});
    let d = t.item(c, 50, 20, |s| s.order = 1);
    let e = t.item(c, 50, 20, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, b).origin.x, px(0));
    assert_eq!(t.rect(&tree, e).origin.x, px(50));
    assert_eq!(t.rect(&tree, d).origin.x, px(100));
    assert_eq!(t.rect(&tree, a).origin.x, px(150));
    let cf = box_of(&tree, c);
    let order: Vec<NodeId> = cf
        .children
        .iter()
        .map(|f| f.source().unwrap().node())
        .collect();
    assert_eq!(order, vec![b, e, d, a]);
}

#[test]
fn column_with_definite_height_grows_and_stretches_widths() {
    let mut t = T::new();
    let c = t.flex(t.body, 200, Some(300), |s| {
        s.flex_direction = FlexDirection::Column
    });
    let a = t.div(c, |s| {
        s.height = len(50);
        s.flex_grow = 1000;
    });
    let b = t.div(c, |s| {
        s.height = len(50);
        s.flex_grow = 1000;
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 150));
    assert_eq!(t.rect(&tree, b), r(0, 150, 200, 150));
}

#[test]
fn column_with_indefinite_height_sizes_to_content() {
    let mut t = T::new();
    let c = t.flex(t.body, 200, None, |s| {
        s.flex_direction = FlexDirection::Column;
        s.row_gap = lp(10);
    });
    let a = t.div(c, |s| {
        s.height = len(50);
        s.flex_grow = 1000;
    });
    let b = t.div(c, |s| s.width = len(80));
    let inner = t.div(b, |s| s.height = len(30));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 50));
    assert_eq!(t.rect(&tree, b), r(0, 60, 80, 30));
    assert_eq!(t.rect(&tree, inner), r(0, 60, 80, 30));
    assert_eq!(t.rect(&tree, c), r(0, 0, 200, 90));
}

#[test]
fn column_max_height_shrinks_items_but_content_minimum_holds() {
    let mut t = T::new();
    let c = t.flex(t.body, 200, None, |s| {
        s.flex_direction = FlexDirection::Column;
        s.max_height = len(100);
    });
    let a = t.div(c, |s| s.height = len(80));
    let b = t.div(c, |s| s.height = len(80));
    let tree = t.layout();
    // Shrinks 160 → 100 (empty items have zero content minimum).
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 50));
    assert_eq!(t.rect(&tree, b), r(0, 50, 200, 50));
    assert_eq!(t.rect(&tree, c).size.height, px(100));
    // With content, `min-height: auto` stops the shrink at the content height.
    let mut t = T::new();
    let c = t.flex(t.body, 200, Some(100), |s| {
        s.flex_direction = FlexDirection::Column
    });
    let a = t.div(c, |_| {});
    t.div(a, |s| s.height = len(80));
    let b = t.div(c, |s| s.overflow_y = Overflow::Hidden);
    t.div(b, |s| s.height = len(80));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 80));
    // The scroll container has a zero minimum and takes the rest.
    assert_eq!(t.rect(&tree, b), r(0, 80, 200, 20));
}

#[test]
fn column_reverse_and_percent_basis_without_definite_height() {
    let mut t = T::new();
    let c = t.flex(t.body, 200, Some(300), |s| {
        s.flex_direction = FlexDirection::ColumnReverse
    });
    let a = t.div(c, |s| s.height = len(50));
    let b = t.div(c, |s| s.height = len(50));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).origin.y, px(250));
    assert_eq!(t.rect(&tree, b).origin.y, px(200));
    let mut t = T::new();
    let c = t.flex(t.body, 200, None, |s| {
        s.flex_direction = FlexDirection::Column
    });
    let a = t.div(c, |s| s.flex_basis = pct(50));
    t.div(a, |s| s.height = len(30));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.height, px(30));
}

#[test]
fn column_baseline_alignment_falls_back_to_flex_start() {
    let mut t = T::new();
    let c = t.flex(t.body, 200, None, |s| {
        s.flex_direction = FlexDirection::Column;
        s.align_items = AlignItems::Baseline;
    });
    let a = t.item(c, 50, 20, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 50, 20));
}

// §4: box generation.

#[test]
fn text_runs_become_anonymous_items_and_white_space_is_dropped() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |_| {});
    t.text(c, "  ");
    t.text(c, "hello");
    let a = t.item(c, 50, 10, |_| {});
    t.text(c, " ");
    let tree = t.layout();
    let cf = box_of(&tree, c);
    assert_eq!(cf.children.len(), 2, "{}", debug::dump_doc(&t.doc, &tree));
    assert!(matches!(
        cf.children[0].kind,
        FragmentKind::Box {
            source: StyleSource::Anonymous(_),
            ..
        }
    ));
    assert_eq!(cf.children[0].rect.size.width, tw("hello"));
    assert_eq!(t.rect(&tree, a).origin.x, tw("hello"));
    assert_eq!(t.rect(&tree, c).size.height, lh());
}

#[test]
fn inline_children_are_blockified_and_display_contents_is_transparent() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |_| {});
    let a = t.el(c, "span", |s| {
        s.display = Display::Inline;
        s.width = len(50);
    });
    let wrapper = t.div(c, |s| s.display = Display::Contents);
    let b = t.item(wrapper, 60, 20, |_| {});
    let d = t.el(c, "span", |s| {
        s.display = Display::InlineBlock;
        s.width = len(70);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 50, 20));
    assert_eq!(t.rect(&tree, b), r(50, 0, 60, 20));
    assert_eq!(t.rect(&tree, d), r(110, 0, 70, 20));
}

#[test]
fn absolute_child_takes_static_position_as_sole_item() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(100), |s| {
        s.position = Position::Relative;
        s.justify_content = JustifyContent::Center;
        s.align_items = AlignItems::Center;
    });
    let a = t.item(c, 50, 20, |_| {});
    let abs = t.item(c, 50, 20, |s| s.position = Position::Absolute);
    let b = t.item(c, 50, 20, |_| {});
    let tree = t.layout();
    // The absolute does not participate: a and b are centred as a pair.
    assert_eq!(t.rect(&tree, a).origin.x, px(100));
    assert_eq!(t.rect(&tree, b).origin.x, px(150));
    assert_eq!(t.rect(&tree, abs), r(125, 40, 50, 20));
    // flex-end in row-reverse: the static position is the left edge.
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(100), |s| {
        s.position = Position::Relative;
        s.flex_direction = FlexDirection::RowReverse;
        s.justify_content = JustifyContent::FlexEnd;
    });
    let abs = t.item(c, 50, 20, |s| s.position = Position::Absolute);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, abs), r(0, 0, 50, 20));
}

#[test]
fn nested_flex_item_with_percentage_children() {
    let mut t = T::new();
    let outer = t.flex(t.body, 400, Some(100), |_| {});
    let inner = t.div(outer, |s| {
        s.display = Display::Flex;
        s.flex_grow = 1000;
    });
    let a = t.div(inner, |s| s.width = pct(50));
    let b = t.div(inner, |s| {
        s.width = pct(50);
        s.height = pct(50);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, inner), r(0, 0, 400, 100));
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 100));
    assert_eq!(t.rect(&tree, b), r(200, 0, 200, 50));
}

#[test]
fn min_width_auto_keeps_a_long_word_unless_scroll_container() {
    let word = "Supercalifragilisticexpialidocious";
    let w = tw(word);
    assert!(w < px(300));
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |_| {});
    let a = t.div(c, |_| {});
    t.text(a, word);
    let b = t.item(c, 300, 10, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, w);
    assert_eq!(t.rect(&tree, b).size.width, px(300) - w);
    // As a scroll container the minimum is zero and it shrinks by its scaled factor.
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |_| {});
    let a = t.div(c, |s| s.overflow_x = Overflow::Hidden);
    t.text(a, word);
    let b = t.item(c, 300, 10, |_| {});
    let tree = t.layout();
    let free = -w;
    let sum = w.0 as i128 * 1000 + 300 * 64 * 1000;
    let upto = mul_div(free, w.0 as i128 * 1000, sum);
    assert_eq!(t.rect(&tree, a).size.width, w + upto);
    assert_eq!(t.rect(&tree, b).size.width, px(300) + (free - upto));
}

#[test]
fn inline_flex_sits_on_the_line_with_its_first_item_baseline() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "x");
    let c = t.div(p, |s| {
        s.display = Display::InlineFlex;
        s.padding = Sides::uniform(lp(4));
        s.border = Sides::uniform(side(1));
    });
    let a = t.div(c, |_| {});
    t.text(a, "one");
    let b = t.div(c, |_| {});
    t.text(b, "two");
    t.text(b, " ");
    t.el(b, "br", |s| s.display = Display::Inline);
    t.text(b, "lines");
    let tree = t.layout();
    let ts = texts(&tree);
    let x = ts.iter().find(|(s, _, _)| s == "x").unwrap();
    let one = ts.iter().find(|(s, _, _)| s == "one").unwrap();
    assert_eq!(x.2, one.2, "{}", debug::dump_doc(&t.doc, &tree));
    let cr = t.rect(&tree, c);
    assert_eq!(
        cr.size.width,
        tw("one") + tw("two").max(tw("lines")) + px(10)
    );
    assert_eq!(cr.size.height, lh() * 2 + px(10));
    assert_eq!(cr.origin.x, tw("x"));
}

#[test]
fn intrinsic_sizes_of_a_flex_container_inside_a_float() {
    let mut t = T::new();
    let fl = t.div(t.body, |s| s.float = Float::Left);
    let c = t.div(fl, |s| {
        s.display = Display::Flex;
        s.column_gap = lp(10);
    });
    t.item(c, 50, 10, |_| {});
    t.item(c, 70, 10, |s| s.margin.left = m(5));
    let fl2 = t.div(t.body, |s| {
        s.float = Float::Left;
        s.clear = Clear::Left;
    });
    let c2 = t.div(fl2, |s| {
        s.display = Display::Flex;
        s.flex_direction = FlexDirection::Column;
    });
    t.item(c2, 50, 10, |_| {});
    t.item(c2, 70, 10, |_| {});
    let fl3 = t.div(t.body, |s| {
        s.float = Float::Left;
        s.clear = Clear::Left;
        s.width = Sizing::MinContent;
    });
    let c3 = t.div(fl3, |s| {
        s.display = Display::Flex;
        s.flex_wrap = FlexWrap::Wrap;
    });
    let w1 = t.div(c3, |_| {});
    t.text(w1, "aa bbbb");
    let w2 = t.div(c3, |_| {});
    t.text(w2, "cc");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, fl).size.width, px(135));
    assert_eq!(t.rect(&tree, fl2).size.width, px(70));
    // Multi-line min-content: the largest item's min-content ("bbbb").
    assert_eq!(t.rect(&tree, fl3).size.width, tw("bbbb"));
    assert_eq!(t.rect(&tree, w2).origin.y, t.rect(&tree, w1).bottom());
    let _ = c;
}

#[test]
fn flex_container_contains_floats_and_items_do_not_collapse_margins() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |_| {});
    let a = t.div(c, |s| s.width = len(100));
    let f = t.item(a, 30, 50, |s| s.float = Float::Left);
    let b = t.div(c, |s| s.width = len(100));
    let child = t.item(b, 30, 30, |s| s.margin.top = m(20));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 50));
    assert_eq!(t.rect(&tree, f), r(0, 0, 30, 50));
    assert_eq!(t.rect(&tree, b), r(100, 0, 100, 50));
    assert_eq!(t.rect(&tree, child), r(100, 20, 30, 30));
    // The container's own margins still collapse with its siblings.
    let mut t = T::new();
    let before = t.item(t.body, 50, 10, |s| s.margin.bottom = m(30));
    let c = t.flex(t.body, 300, Some(10), |s| s.margin.top = m(20));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, before).bottom(), px(10));
    assert_eq!(t.rect(&tree, c).origin.y, px(40));
}

#[test]
fn overflow_makes_the_container_a_scroll_container() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(50), |s| s.overflow_x = Overflow::Auto);
    let a = t.item(c, 200, 20, |s| s.flex_shrink = 0);
    let b = t.item(c, 200, 20, |s| s.flex_shrink = 0);
    let tree = t.layout();
    let cf = box_of(&tree, c);
    let info = match &cf.kind {
        FragmentKind::Box {
            scroll: Some(i), ..
        } => *i,
        _ => panic!("no scroll info\n{}", debug::dump_doc(&t.doc, &tree)),
    };
    assert_eq!(info.content_width, px(400));
    assert!(info.shows_x_bar);
    assert_eq!(t.rect(&tree, a).size.width, px(200));
    assert_eq!(t.rect(&tree, b).origin.x, px(200));
    assert_eq!(cf.overflow.size.width, px(300));
}

#[test]
fn relative_offsets_apply_after_alignment_and_items_stack_with_z_index() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(100), |s| {
        s.align_items = AlignItems::Center
    });
    let a = t.item(c, 50, 50, |s| {
        s.position = Position::Relative;
        s.inset.left = m(10);
        s.inset.top = m(-5);
    });
    let b = t.item(c, 50, 50, |s| s.z_index = ZIndex::Int(3));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(10, 20, 50, 50));
    let bf = box_of(&tree, b);
    assert!(bf.establishes_stacking_context);
    assert_eq!(bf.z_index, 3);
}

#[test]
fn replaced_item_grows_and_keeps_its_height() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |_| {});
    let img = t.doc.create_element(
        "img",
        vec![crate::dom::Attribute {
            name: "src".into(),
            value: "a.png".into(),
        }],
    );
    t.doc.append(c, img);
    let mut s = ComputedStyle::inherit_from(&t.style_of(c));
    s.display = Display::Inline;
    s.flex_grow = 1000;
    t.styles.set(img, Rc::new(s));
    let mut images = ImageSizeMap::default();
    images.0.insert("a.png".into(), (100, 50));
    let scroll = ScrollState::new();
    let mut cache = LayoutCache::default();
    let tree = layout_with(
        &t.doc,
        &t.styles,
        Viewport {
            width: 800,
            height: 600,
            scale: 1,
            zoom: 100,
        },
        LayoutOptions {
            images: &images,
            scroll: &scroll,
        },
        &mut cache,
    );
    // Grown to the line; the hypothetical cross size follows the aspect ratio from
    // the used main size (§9.4 step 7): 300 * 50 / 100.
    assert_eq!(t.rect(&tree, img), r(0, 0, 300, 150));
    assert_eq!(t.rect(&tree, c).size.height, px(150));
}

#[test]
fn empty_container_has_no_height_or_baseline() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |_| {});
    t.text(c, "   ");
    let after = t.item(t.body, 10, 10, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, c), r(0, 0, 300, 0));
    assert_eq!(box_baseline(&box_of(&tree, c)), None);
    assert_eq!(t.rect(&tree, after).origin.y, px(0));
}

#[test]
fn item_padding_border_and_margins_count_in_the_outer_size() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(100), |_| {});
    let a = t.div(c, |s| {
        s.flex_grow = 1000;
        s.padding = Sides::uniform(lp(5));
        s.border = Sides::uniform(side(2));
        s.margin = Sides::uniform(m(10));
    });
    let b = t.item(c, 100, 20, |s| s.margin.left = m(20));
    let tree = t.layout();
    // 300 - (20 + 100) - 20 (margins of a) = 160 border box for a.
    assert_eq!(t.rect(&tree, a), r(10, 10, 160, 80));
    assert_eq!(t.rect(&tree, b), r(200, 0, 100, 20));
}

#[test]
fn line_breaking_counts_margins_and_never_splits_a_single_item() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, None, |s| s.flex_wrap = FlexWrap::Wrap);
    let a = t.item(c, 140, 10, |s| s.margin.right = m(30));
    let b = t.item(c, 140, 10, |_| {});
    let d = t.item(c, 400, 10, |_| {});
    let tree = t.layout();
    assert_eq!(
        t.rect(&tree, a).origin,
        crate::geom::Point { x: px(0), y: px(0) }
    );
    assert_eq!(
        t.rect(&tree, b).origin,
        crate::geom::Point {
            x: px(0),
            y: px(10)
        }
    );
    // Too wide for any line: alone on its own line, shrunk to the container.
    assert_eq!(t.rect(&tree, d), r(0, 20, 300, 10));
}

#[test]
fn rtl_row_starts_at_the_right() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(20), |s| s.direction = Direction::Rtl);
    let a = t.item(c, 50, 20, |_| {});
    let b = t.item(c, 50, 20, |_| {});
    let tree = t.layout();
    // The rtl container itself sits at the right of the body.
    let cx = t.rect(&tree, c).origin.x;
    assert_eq!(cx, px(500));
    assert_eq!(t.rect(&tree, a).origin.x, cx + px(250));
    assert_eq!(t.rect(&tree, b).origin.x, cx + px(200));
}

#[test]
fn align_self_start_and_end_follow_the_writing_mode_under_wrap_reverse() {
    let mut t = T::new();
    let c = t.flex(t.body, 300, Some(100), |s| {
        s.flex_wrap = FlexWrap::WrapReverse
    });
    let a = t.item(c, 50, 20, |s| s.align_self = AlignSelf::Start);
    let b = t.item(c, 50, 20, |s| s.align_self = AlignSelf::FlexStart);
    let d = t.item(c, 50, 20, |s| s.align_self = AlignSelf::End);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).origin.y, px(0));
    assert_eq!(t.rect(&tree, b).origin.y, px(80));
    assert_eq!(t.rect(&tree, d).origin.y, px(80));
}
