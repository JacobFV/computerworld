//! Grid layout tests with hand-built documents and style sets. Expected values are
//! computed by hand from css-grid-1; text widths come from `text::measure`.

use std::rc::Rc;

use crate::dom::{Attribute, Document, NodeId};
use crate::geom::{Au, Rect};
use crate::layout::fragment::{FragmentKind, FragmentTree};
use crate::layout::{debug, layout, text};
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
fn lp(n: i32) -> LengthPercentage {
    LengthPercentage::Length(px(n))
}
fn m(n: i32) -> LengthPercentageAuto {
    LengthPercentageAuto::Set(lp(n))
}
fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(px(x), px(y), px(w), px(h))
}
fn fx(n: i32) -> TrackSize {
    TrackSize::Fixed(lp(n))
}
fn fr(n: i32) -> TrackSize {
    TrackSize::Flex(n * 1000)
}
fn tracks(ts: &[TrackSize]) -> TrackList {
    TrackList {
        tracks: ts.to_vec(),
        line_names: vec![Vec::new(); ts.len() + 1],
        auto_repeat: None,
    }
}
fn names(ts: &[TrackSize], names: &[&[&str]]) -> TrackList {
    let mut tl = tracks(ts);
    for (i, n) in names.iter().enumerate() {
        tl.line_names[i] = n.iter().map(|s| s.to_string()).collect();
    }
    tl
}
fn auto_repeat(fill: bool, at: usize, outside: &[TrackSize], rep: &[TrackSize]) -> TrackList {
    let mut tl = tracks(outside);
    tl.auto_repeat = Some(AutoRepeat {
        fill,
        at,
        tracks: rep.to_vec(),
        line_names: vec![Vec::new(); rep.len() + 1],
    });
    tl
}
fn line(n: i32) -> GridLine {
    GridLine::Line(n, None)
}
fn span(n: u32) -> GridLine {
    GridLine::Span(n, None)
}
fn name(s: &str) -> GridLine {
    GridLine::Name(s.into())
}
fn areas(rows: &[&str]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| r.split_whitespace().map(str::to_owned).collect())
        .collect()
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

struct T {
    doc: Document,
    styles: StyleSet,
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
        T { doc, styles, body }
    }
    fn style_of(&self, n: NodeId) -> ComputedStyle {
        self.styles
            .get(n)
            .cloned()
            .unwrap_or_else(ComputedStyle::initial)
    }
    fn el(&mut self, parent: NodeId, tag: &str, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
        let n = self.doc.create_element(tag, Vec::<Attribute>::new());
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
    /// A grid container of the given width (auto height unless set in `f`).
    fn grid(&mut self, parent: NodeId, width: i32, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
        self.div(parent, |s| {
            s.display = Display::Grid;
            s.width = len(width);
            f(s);
        })
    }
    /// A grid item with a fixed height (and auto width).
    fn item(&mut self, parent: NodeId, height: i32, f: impl FnOnce(&mut ComputedStyle)) -> NodeId {
        self.div(parent, |s| {
            s.height = len(height);
            f(s);
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
    fn dump(&self, tree: &FragmentTree) -> String {
        debug::dump_doc(&self.doc, tree)
    }
}

// §7.2: explicit tracks.

#[test]
fn fr_columns_1fr_2fr() {
    let mut t = T::new();
    let g = t.grid(t.body, 900, |s| {
        s.grid_template_columns = tracks(&[fr(1), fr(2)])
    });
    let a = t.item(g, 50, |_| {});
    let b = t.item(g, 30, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 300, 50), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(300, 0, 600, 30));
    // The row is as tall as its tallest item; the container's auto height is the row.
    assert_eq!(t.rect(&tree, g), r(0, 0, 900, 50));
}

#[test]
fn repeat_three_fixed_columns_wraps_into_implicit_rows() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100), fx(100)])
    });
    let items: Vec<NodeId> = (0..4).map(|_| t.item(g, 20, |_| {})).collect();
    let tree = t.layout();
    assert_eq!(t.rect(&tree, items[0]), r(0, 0, 100, 20));
    assert_eq!(t.rect(&tree, items[1]), r(100, 0, 100, 20));
    assert_eq!(t.rect(&tree, items[2]), r(200, 0, 100, 20));
    assert_eq!(t.rect(&tree, items[3]), r(0, 20, 100, 20));
    assert_eq!(t.rect(&tree, g).size.height, px(40));
}

#[test]
fn auto_fill_minmax_at_several_widths() {
    // At 99px the single track keeps its 100px minimum and overflows.
    for (width, count, track) in [
        (800, 8, Au::from_px_i32(100)),
        (450, 4, Au(7200)),
        (150, 1, Au::from_px_i32(150)),
        (99, 1, Au::from_px_i32(100)),
    ] {
        let mut t = T::new();
        let g = t.grid(t.body, width, |s| {
            s.grid_template_columns = auto_repeat(
                true,
                0,
                &[],
                &[TrackSize::MinMax(
                    TrackBreadth::Fixed(lp(100)),
                    TrackBreadth::Flex(1000),
                )],
            )
        });
        let a = t.item(g, 10, |_| {});
        let b = t.item(g, 10, |_| {});
        let tree = t.layout();
        assert_eq!(t.rect(&tree, a).size.width, track, "width {width}");
        let b_x = if count > 1 { track } else { Au::ZERO };
        let b_y = if count > 1 { Au::ZERO } else { px(10) };
        assert_eq!(
            t.rect(&tree, b).origin,
            crate::geom::Point { x: b_x, y: b_y },
            "width {width}\n{}",
            t.dump(&tree)
        );
    }
}

#[test]
fn auto_fill_counts_gaps_and_outside_tracks() {
    // 100px repeat(auto-fill, 50px) 100px at 500px with 10px gaps:
    // A = 200 + 10 * (2 - 1) = 210; denominator = 50 + 10; n = floor(290 / 60) = 4.
    let mut t = T::new();
    let g = t.grid(t.body, 500, |s| {
        s.grid_template_columns = auto_repeat(true, 1, &[fx(100), fx(100)], &[fx(50)]);
        s.column_gap = lp(10);
    });
    let last = t.item(g, 10, |s| s.grid_column_start = line(-1 - 1));
    let tree = t.layout();
    // Six tracks: 100 | 50 50 50 50 | 100, five gaps. Line -2 is the sixth track's start.
    assert_eq!(
        t.rect(&tree, last),
        r(100 + 4 * 50 + 5 * 10, 0, 100, 10),
        "{}",
        t.dump(&tree)
    );
}

#[test]
fn auto_fill_rows_use_max_height_then_min_height() {
    // Indefinite height: the max-size rule gives floor(200 / 50) = 4 rows.
    let mut t = T::new();
    let g = t.grid(t.body, 300, |s| {
        s.grid_template_rows = auto_repeat(true, 0, &[], &[fx(50)]);
        s.max_height = len(200);
    });
    t.item(g, 10, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, g).size.height, px(200));
    // Only a min-height: the smallest count that fulfils it, ceil(120 / 50) = 3.
    let mut t = T::new();
    let g = t.grid(t.body, 300, |s| {
        s.grid_template_rows = auto_repeat(true, 0, &[], &[fx(50)]);
        s.min_height = len(120);
    });
    t.item(g, 10, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, g).size.height, px(150));
}

#[test]
fn auto_fit_collapses_empty_tracks_and_their_gutters() {
    // (800 + 10) / 110 = 7 tracks; two items keep two; the rest collapse to 0 with
    // no gaps: the free space is 800 - 10 = 790, split 395 each.
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = auto_repeat(
            false,
            0,
            &[],
            &[TrackSize::MinMax(
                TrackBreadth::Fixed(lp(100)),
                TrackBreadth::Flex(1000),
            )],
        );
        s.column_gap = lp(10);
    });
    let a = t.item(g, 10, |_| {});
    let b = t.item(g, 10, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 395, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(405, 0, 395, 10));
}

#[test]
fn grid_auto_rows_cycle_for_implicit_rows() {
    let mut t = T::new();
    let g = t.grid(t.body, 100, |s| s.grid_auto_rows = vec![fx(20), fx(40)]);
    let items: Vec<NodeId> = (0..4).map(|_| t.div(g, |_| {})).collect();
    let tree = t.layout();
    assert_eq!(t.rect(&tree, items[0]), r(0, 0, 100, 20));
    assert_eq!(t.rect(&tree, items[1]), r(0, 20, 100, 40));
    assert_eq!(t.rect(&tree, items[2]), r(0, 60, 100, 20));
    assert_eq!(t.rect(&tree, items[3]), r(0, 80, 100, 40));
    assert_eq!(t.rect(&tree, g).size.height, px(120));
}

#[test]
fn areas_wider_than_template_add_explicit_tracks_sized_by_auto_columns() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[fx(100)]);
        s.grid_template_areas = areas(&["a b c"]);
        s.grid_auto_columns = vec![fx(50)];
        s.justify_content = JustifyContent::Start;
    });
    let c = t.item(g, 10, |s| {
        s.grid_column_start = name("c");
        s.grid_column_end = name("c");
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, c), r(150, 0, 50, 10), "{}", t.dump(&tree));
}

// §7.3, §8.3: named lines, areas and line numbers.

#[test]
fn named_areas_place_a_page_layout() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.height = len(400);
        s.grid_template_columns = tracks(&[fx(200), fr(1)]);
        s.grid_template_rows = tracks(&[fx(50), fr(1), fx(30)]);
        s.grid_template_areas = areas(&["header header", "sidebar main", "footer footer"]);
    });
    let area = |t: &mut T, n: &str| {
        t.div(g, |s| {
            s.grid_row_start = name(n);
            s.grid_row_end = name(n);
            s.grid_column_start = name(n);
            s.grid_column_end = name(n);
        })
    };
    let main = area(&mut t, "main");
    let header = area(&mut t, "header");
    let footer = area(&mut t, "footer");
    let sidebar = area(&mut t, "sidebar");
    let tree = t.layout();
    assert_eq!(t.rect(&tree, header), r(0, 0, 800, 50), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, sidebar), r(0, 50, 200, 320));
    assert_eq!(t.rect(&tree, main), r(200, 50, 600, 320));
    assert_eq!(t.rect(&tree, footer), r(0, 370, 800, 30));
}

#[test]
fn line_names_at_both_ends_and_multiple_names_per_line() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = names(&[fx(100), fx(100)], &[&["a", "b"], &["c"], &["d", "e"]]);
        s.justify_content = JustifyContent::Start;
    });
    let x = t.item(g, 10, |s| {
        s.grid_column_start = name("b");
        s.grid_column_end = name("e");
    });
    let y = t.item(g, 10, |s| {
        s.grid_column_start = name("c");
        s.grid_column_end = name("d");
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, x), r(0, 0, 200, 10));
    assert_eq!(t.rect(&tree, y), r(100, 10, 100, 10));
}

#[test]
fn nth_named_line_and_named_span() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = names(
            &[fx(100), fx(100), fx(100), fx(100)],
            &[&["a"], &["a"], &["a"], &["a"], &[]],
        );
        s.justify_content = JustifyContent::Start;
    });
    // `a 2 / span 2 a`: the second `a` line (2), then two `a` lines after it (4).
    let x = t.item(g, 10, |s| {
        s.grid_column_start = GridLine::Line(2, Some("a".into()));
        s.grid_column_end = GridLine::Span(2, Some("a".into()));
    });
    // `span a / a 4`: backwards from line 4, the first `a` line before it is 3.
    let y = t.item(g, 10, |s| {
        s.grid_column_start = GridLine::Span(1, Some("a".into()));
        s.grid_column_end = GridLine::Line(4, Some("a".into()));
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, x), r(100, 0, 200, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, y), r(200, 10, 100, 10));
}

#[test]
fn negative_line_numbers_count_from_the_explicit_end() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100), fx(100), fx(100)]);
        s.justify_content = JustifyContent::Start;
    });
    let full = t.item(g, 10, |s| {
        s.grid_column_start = line(1);
        s.grid_column_end = line(-1);
    });
    let last = t.item(g, 10, |s| s.grid_column_start = line(-2));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, full), r(0, 0, 400, 10));
    assert_eq!(t.rect(&tree, last), r(300, 10, 100, 10));
}

#[test]
fn span_two_from_auto_and_from_a_line() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100), fx(100)]);
        s.justify_content = JustifyContent::Start;
    });
    let a = t.item(g, 10, |s| s.grid_column_end = span(2));
    let b = t.item(g, 10, |s| {
        s.grid_column_start = line(2);
        s.grid_column_end = span(2);
    });
    let c = t.item(g, 10, |s| {
        s.grid_column_start = span(2);
        s.grid_column_end = line(3);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(100, 10, 200, 10));
    assert_eq!(t.rect(&tree, c), r(0, 20, 200, 10));
}

#[test]
fn lines_beyond_the_explicit_grid_create_implicit_tracks() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100)]);
        s.grid_auto_columns = vec![fx(60)];
        s.justify_content = JustifyContent::Start;
    });
    let far = t.item(g, 10, |s| s.grid_column_start = line(4));
    let named = t.item(g, 10, |s| {
        s.grid_column_start = GridLine::Line(2, Some("nothing".into()))
    });
    let tree = t.layout();
    // Lines 3 and 4 are implicit; track 3 is 60px, so line 4 is at 260.
    assert_eq!(t.rect(&tree, far), r(260, 0, 60, 10), "{}", t.dump(&tree));
    // No line is named `nothing`: implicit lines after the grid all are. The first
    // implicit line is 4, so the second is line 5, in the same row.
    assert_eq!(t.rect(&tree, named), r(320, 0, 60, 10));
}

#[test]
fn negative_implicit_lines_grow_the_grid_before_the_explicit_one() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| {
        s.grid_template_rows = tracks(&[fx(50)]);
        s.grid_auto_rows = vec![fx(30)];
    });
    let a = t.div(g, |s| s.grid_row_start = line(1));
    // Explicit end line is 2; -3 is line 0, one implicit row before the grid.
    let b = t.div(g, |s| s.grid_row_start = line(-3));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, b), r(0, 0, 200, 30), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, a), r(0, 30, 200, 50));
    assert_eq!(t.rect(&tree, g).size.height, px(80));
}

// §8.5: auto-placement.

#[test]
fn auto_placement_sparse_with_a_locked_item() {
    let mut t = T::new();
    let g = t.grid(t.body, 300, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100), fx(100)]);
        s.grid_auto_rows = vec![fx(10)];
    });
    let locked = t.div(g, |s| {
        s.grid_column_start = line(2);
        s.grid_row_start = line(1);
    });
    let two = t.div(g, |s| s.grid_column_end = span(2));
    let one = t.div(g, |_| {});
    let three = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(
        t.rect(&tree, locked),
        r(100, 0, 100, 10),
        "{}",
        t.dump(&tree)
    );
    assert_eq!(t.rect(&tree, two), r(0, 10, 200, 10));
    assert_eq!(t.rect(&tree, one), r(200, 10, 100, 10));
    assert_eq!(t.rect(&tree, three), r(0, 20, 100, 10));
}

#[test]
fn auto_placement_dense_backfills_holes() {
    let mut t = T::new();
    let g = t.grid(t.body, 300, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100), fx(100)]);
        s.grid_auto_rows = vec![fx(10)];
        s.grid_auto_flow = GridAutoFlow::RowDense;
    });
    let locked = t.div(g, |s| {
        s.grid_column_start = line(2);
        s.grid_row_start = line(1);
    });
    let two = t.div(g, |s| s.grid_column_end = span(2));
    let one = t.div(g, |_| {});
    let three = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(
        t.rect(&tree, locked),
        r(100, 0, 100, 10),
        "{}",
        t.dump(&tree)
    );
    assert_eq!(t.rect(&tree, two), r(0, 10, 200, 10));
    assert_eq!(t.rect(&tree, one), r(0, 0, 100, 10));
    assert_eq!(t.rect(&tree, three), r(200, 0, 100, 10));
}

#[test]
fn items_locked_in_a_row_go_after_earlier_items_in_that_row() {
    let mut t = T::new();
    let g = t.grid(t.body, 300, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100), fx(100)]);
        s.grid_auto_rows = vec![fx(10)];
    });
    let a = t.div(g, |s| s.grid_row_start = line(2));
    let b = t.div(g, |s| s.grid_row_start = line(2));
    let c = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 10, 100, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(100, 10, 100, 10));
    assert_eq!(t.rect(&tree, c), r(0, 0, 100, 10));
}

#[test]
fn column_flow_fills_columns_and_adds_implicit_columns() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_rows = tracks(&[fx(50), fx(50)]);
        s.grid_auto_columns = vec![fx(100)];
        s.grid_auto_flow = GridAutoFlow::Column;
        s.justify_content = JustifyContent::Start;
    });
    let items: Vec<NodeId> = (0..5).map(|_| t.div(g, |_| {})).collect();
    let tree = t.layout();
    assert_eq!(
        t.rect(&tree, items[0]),
        r(0, 0, 100, 50),
        "{}",
        t.dump(&tree)
    );
    assert_eq!(t.rect(&tree, items[1]), r(0, 50, 100, 50));
    assert_eq!(t.rect(&tree, items[2]), r(100, 0, 100, 50));
    assert_eq!(t.rect(&tree, items[3]), r(100, 50, 100, 50));
    assert_eq!(t.rect(&tree, items[4]), r(200, 0, 100, 50));
}

#[test]
fn column_flow_dense_with_a_locked_row() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_rows = tracks(&[fx(50), fx(50)]);
        s.grid_auto_columns = vec![fx(100)];
        s.grid_auto_flow = GridAutoFlow::ColumnDense;
        s.justify_content = JustifyContent::Start;
    });
    let tall = t.div(g, |s| s.grid_row_end = span(2));
    let second_row = t.div(g, |s| s.grid_row_start = line(2));
    let small = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, tall), r(0, 0, 100, 100), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, second_row), r(100, 50, 100, 50));
    // Dense: the hole at column 2, row 1 is filled.
    assert_eq!(t.rect(&tree, small), r(100, 0, 100, 50));
}

#[test]
fn order_changes_placement_order() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100)])
    });
    let a = t.item(g, 10, |_| {});
    let b = t.item(g, 10, |s| s.order = -1);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, b), r(0, 0, 100, 10));
    assert_eq!(t.rect(&tree, a), r(100, 0, 100, 10));
}

// §6: item generation.

#[test]
fn text_becomes_an_anonymous_item_and_white_space_is_dropped() {
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[fx(200), fx(200)]);
        s.grid_auto_rows = vec![fx(30)];
    });
    t.text(g, "   ");
    let a = t.item(g, 10, |_| {});
    t.text(g, "hi");
    let b = t.item(g, 10, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 10), "{}", t.dump(&tree));
    // "hi" is the second item, so `b` lands in the second row.
    assert_eq!(t.rect(&tree, b), r(0, 30, 200, 10));
    let mut texts = Vec::new();
    tree.root.walk(Default::default(), &mut |f, rect| {
        if let FragmentKind::Text { text, .. } = &f.kind {
            texts.push((text.clone(), rect));
        }
    });
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].0, "hi");
    assert_eq!(texts[0].1.origin.x, px(200));
}

#[test]
fn display_contents_child_contributes_its_children_and_inline_children_are_blockified() {
    let mut t = T::new();
    let g = t.grid(t.body, 300, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100), fx(100)]);
        s.grid_auto_rows = vec![fx(10)];
    });
    let wrapper = t.div(g, |s| s.display = Display::Contents);
    let a = t.div(wrapper, |_| {});
    let b = t.el(wrapper, "span", |s| s.display = Display::Inline);
    let c = t.el(g, "span", |s| s.display = Display::InlineBlock);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(100, 0, 100, 10));
    assert_eq!(t.rect(&tree, c), r(200, 0, 100, 10));
}

// §11: track sizing.

#[test]
fn minmax_auto_max_content_takes_the_text_width() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[
            TrackSize::MinMax(TrackBreadth::Auto, TrackBreadth::MaxContent),
            fx(100),
        ]);
        s.justify_content = JustifyContent::Start;
    });
    let a = t.div(g, |_| {});
    t.text(a, "Hello world");
    let tree = t.layout();
    assert_eq!(
        t.rect(&tree, a),
        Rect::new(Au::ZERO, Au::ZERO, tw("Hello world"), lh()),
        "{}",
        t.dump(&tree)
    );
}

#[test]
fn min_content_and_max_content_tracks() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[TrackSize::MinContent, TrackSize::MaxContent]);
        s.justify_content = JustifyContent::Start;
    });
    let a = t.div(g, |_| {});
    t.text(a, "Hello world");
    let b = t.div(g, |_| {});
    t.text(b, "Hello world");
    let tree = t.layout();
    assert_eq!(
        t.rect(&tree, a).size.width,
        tw("Hello").max(tw("world")),
        "{}",
        t.dump(&tree)
    );
    assert_eq!(t.rect(&tree, b).size.width, tw("Hello world"));
    assert_eq!(t.rect(&tree, b).origin.x, tw("Hello").max(tw("world")));
}

#[test]
fn spanning_item_grows_intrinsic_tracks_equally() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[TrackSize::Auto, TrackSize::Auto]);
        s.justify_content = JustifyContent::Start;
    });
    let a = t.item(g, 10, |s| s.width = len(100));
    let b = t.item(g, 10, |s| s.width = len(100));
    let c = t.item(g, 10, |s| {
        s.width = len(300);
        s.grid_column_end = span(2);
    });
    let tree = t.layout();
    // 300 - 200 = 100 of extra space, 50 to each auto track.
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(150, 0, 100, 10));
    assert_eq!(t.rect(&tree, c), r(0, 10, 300, 10));
}

#[test]
fn spanning_item_prefers_tracks_with_intrinsic_maximums() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[
            TrackSize::MinMax(TrackBreadth::Auto, TrackBreadth::Fixed(lp(100))),
            TrackSize::Auto,
        ]);
        s.justify_content = JustifyContent::Start;
    });
    let c = t.item(g, 10, |s| {
        s.width = len(300);
        s.grid_column_end = span(2);
    });
    let b = t.item(g, 10, |s| s.grid_column_start = line(2));
    let tree = t.layout();
    // Up to limits: the first track stops at 100; beyond limits only the auto track
    // grows: 100 + 200.
    assert_eq!(t.rect(&tree, c), r(0, 0, 300, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(100, 10, 200, 10));
}

#[test]
fn fit_content_limits_max_content_but_not_min_content() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[
            TrackSize::FitContent(lp(200)),
            TrackSize::FitContent(lp(200)),
        ]);
        s.justify_content = JustifyContent::Start;
    });
    let a = t.div(g, |_| {});
    t.text(
        a,
        "A long line of words that measures far more than two hundred pixels wide",
    );
    let b = t.div(g, |_| {});
    t.text(b, "Short text");
    let tree = t.layout();
    assert!(
        tw("A long line of words that measures far more than two hundred pixels wide") > px(200)
    );
    assert_eq!(t.rect(&tree, a).size.width, px(200), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b).size.width, tw("Short text"));
    assert_eq!(t.rect(&tree, b).origin.x, px(200));
}

#[test]
fn flexible_tracks_with_content_larger_than_their_share_become_inflexible() {
    let mut t = T::new();
    let g = t.grid(t.body, 300, |s| {
        s.grid_template_columns = tracks(&[fr(1), fr(1)])
    });
    let a = t.item(g, 10, |s| s.width = len(200));
    let b = t.item(g, 10, |s| s.width = len(50));
    let tree = t.layout();
    // Hypothetical fr 150 < 200: the first track is inflexible; fr = 100.
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(200, 0, 50, 10));
}

#[test]
fn fractional_flex_factors_less_than_one_leave_free_space() {
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[TrackSize::Flex(500)])
    });
    let a = t.item(g, 10, |_| {});
    let tree = t.layout();
    // A flex sum under 1 is treated as 1: 0.5fr of 400 is 200.
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 10), "{}", t.dump(&tree));
}

#[test]
fn auto_tracks_stretch_with_normal_content_distribution() {
    let mut t = T::new();
    let g = t.grid(t.body, 600, |s| {
        s.grid_template_columns = tracks(&[TrackSize::Auto, fx(100), TrackSize::Auto])
    });
    let a = t.item(g, 10, |s| s.width = len(50));
    let b = t.item(g, 10, |_| {});
    let c = t.item(g, 10, |s| s.width = len(150));
    let tree = t.layout();
    // Bases 50, 100, 150; free 300 split between the two auto tracks: 200, 100, 300.
    assert_eq!(t.rect(&tree, a), r(0, 0, 50, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(200, 0, 100, 10));
    assert_eq!(t.rect(&tree, c), r(300, 0, 150, 10));
}

#[test]
fn percentage_tracks_resolve_against_the_container_or_behave_as_auto() {
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns =
            tracks(&[TrackSize::Fixed(LengthPercentage::Percent(2500)), fr(1)]);
        s.grid_template_rows = tracks(&[TrackSize::Fixed(LengthPercentage::Percent(5000))]);
    });
    let a = t.item(g, 20, |_| {});
    let b = t.item(g, 20, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 20), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(100, 0, 300, 20));
    // The container's height is auto, so the 50% row behaves as auto: 20px.
    assert_eq!(t.rect(&tree, g).size.height, px(20));
}

#[test]
fn definite_height_sizes_fr_rows_and_percent_rows() {
    let mut t = T::new();
    let g = t.grid(t.body, 100, |s| {
        s.height = len(200);
        s.grid_template_rows = tracks(&[
            TrackSize::Fixed(LengthPercentage::Percent(2500)),
            fr(1),
            fr(2),
        ]);
    });
    let a = t.div(g, |_| {});
    let b = t.div(g, |_| {});
    let c = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 50), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(0, 50, 100, 50));
    assert_eq!(t.rect(&tree, c), r(0, 100, 100, 100));
}

#[test]
fn implicit_rows_from_overflowing_items_and_container_auto_height() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100)]);
        s.grid_template_rows = tracks(&[fx(50)]);
        s.row_gap = lp(10);
    });
    let items: Vec<NodeId> = (0..5).map(|_| t.item(g, 20, |_| {})).collect();
    let tree = t.layout();
    assert_eq!(
        t.rect(&tree, items[2]),
        r(0, 60, 100, 20),
        "{}",
        t.dump(&tree)
    );
    assert_eq!(t.rect(&tree, items[4]), r(0, 90, 100, 20));
    // 50 + 10 + 20 + 10 + 20.
    assert_eq!(t.rect(&tree, g).size.height, px(110));
}

#[test]
fn implicit_tracks_beyond_a_definite_height_overflow_the_container() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| {
        s.height = len(100);
        s.grid_template_rows = tracks(&[fx(100)]);
        s.grid_auto_rows = vec![fx(50)];
    });
    let a = t.div(g, |_| {});
    let b = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 100));
    assert_eq!(t.rect(&tree, b), r(0, 100, 200, 50));
    let gf = tree.root.children[0].children[0].children[0].clone();
    assert_eq!(gf.rect, r(0, 0, 200, 100));
    assert_eq!(gf.overflow, r(0, 0, 200, 150), "{}", t.dump(&tree));
}

// Gaps and content distribution.

#[test]
fn percentage_gaps_resolve_against_the_container() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.height = len(200);
        s.grid_template_columns = tracks(&[fr(1), fr(1)]);
        s.grid_template_rows = tracks(&[fx(50), fx(50)]);
        s.column_gap = LengthPercentage::Percent(1000);
        s.row_gap = LengthPercentage::Percent(1000);
    });
    let items: Vec<NodeId> = (0..3).map(|_| t.div(g, |_| {})).collect();
    let tree = t.layout();
    assert_eq!(
        t.rect(&tree, items[0]),
        r(0, 0, 360, 50),
        "{}",
        t.dump(&tree)
    );
    assert_eq!(t.rect(&tree, items[1]), r(440, 0, 360, 50));
    assert_eq!(t.rect(&tree, items[2]), r(0, 70, 360, 50));
}

#[test]
fn justify_content_space_between_with_fixed_tracks() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100), fx(100)]);
        s.justify_content = JustifyContent::SpaceBetween;
    });
    let items: Vec<NodeId> = (0..3).map(|_| t.item(g, 10, |_| {})).collect();
    let tree = t.layout();
    assert_eq!(t.rect(&tree, items[0]).origin.x, px(0));
    assert_eq!(t.rect(&tree, items[1]).origin.x, px(350));
    assert_eq!(t.rect(&tree, items[2]).origin.x, px(700));
}

#[test]
fn justify_content_end_center_space_around_and_evenly() {
    for (jc, xs) in [
        (JustifyContent::End, [500, 600, 700]),
        (JustifyContent::Center, [250, 350, 450]),
        (
            JustifyContent::SpaceAround,
            [250 / 3, 250 / 3 + 100 + 500 / 3, 250 / 3 + 200 + 1000 / 3],
        ),
        (JustifyContent::SpaceEvenly, [125, 350, 575]),
    ] {
        let mut t = T::new();
        let g = t.grid(t.body, 800, |s| {
            s.grid_template_columns = tracks(&[fx(100), fx(100), fx(100)]);
            s.justify_content = jc;
        });
        let items: Vec<NodeId> = (0..3).map(|_| t.item(g, 10, |_| {})).collect();
        let tree = t.layout();
        for (i, x) in xs.iter().enumerate() {
            let got = t.rect(&tree, items[i]).origin.x;
            // Space-around thirds are not whole pixels; compare in Au with the same
            // rounding the layout uses (Au::scale, half away from zero).
            let want = match jc {
                JustifyContent::SpaceAround => {
                    let each = px(500).scale(1, 3);
                    each.scale(1, 2) + (each + px(100)) * i as i32
                }
                _ => px(*x),
            };
            assert_eq!(got, want, "{jc:?} item {i}\n{}", t.dump(&tree));
        }
    }
}

#[test]
fn align_content_center_and_space_between_rows() {
    let mut t = T::new();
    let g = t.grid(t.body, 100, |s| {
        s.height = len(200);
        s.grid_template_rows = tracks(&[fx(50), fx(50)]);
        s.align_content = AlignContent::Center;
    });
    let a = t.div(g, |_| {});
    let b = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 50, 100, 50), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(0, 100, 100, 50));
    let mut t = T::new();
    let g = t.grid(t.body, 100, |s| {
        s.height = len(200);
        s.grid_template_rows = tracks(&[fx(50), fx(50)]);
        s.align_content = AlignContent::SpaceBetween;
    });
    let a = t.div(g, |_| {});
    let b = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 50));
    assert_eq!(t.rect(&tree, b), r(0, 150, 100, 50));
}

// Item alignment and sizing.

#[test]
fn align_items_center_and_stretch() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| {
        s.grid_template_rows = tracks(&[fx(100)]);
        s.grid_template_columns = tracks(&[fx(100), fx(100)]);
        s.align_items = AlignItems::Center;
    });
    let a = t.item(g, 20, |_| {});
    let b = t.div(g, |s| s.align_self = AlignSelf::Stretch);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 40, 100, 20), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(100, 0, 100, 100));
}

#[test]
fn stretched_item_height_is_definite_for_its_children() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| s.grid_template_rows = tracks(&[fx(100)]));
    let a = t.div(g, |_| {});
    let inner = t.div(a, |s| s.height = pct(50));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 100));
    assert_eq!(t.rect(&tree, inner), r(0, 0, 200, 50), "{}", t.dump(&tree));
}

#[test]
fn align_self_end_and_justify_items_center_and_end() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| {
        s.grid_template_rows = tracks(&[fx(100)]);
        s.grid_template_columns = tracks(&[fx(100), fx(100)]);
        s.justify_items = AlignItems::Center;
    });
    let a = t.item(g, 20, |s| {
        s.width = len(50);
        s.align_self = AlignSelf::End;
    });
    let b = t.item(g, 20, |s| {
        s.width = len(50);
        s.justify_self = AlignSelf::End;
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(25, 80, 50, 20), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(150, 0, 50, 20));
}

#[test]
fn baseline_alignment_shares_a_row_context() {
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[fx(200), fx(200)]);
        s.align_items = AlignItems::Baseline;
    });
    let a = t.div(g, |s| s.padding.top = lp(10));
    t.text(a, "a");
    let b = t.div(g, |s| s.padding.top = lp(30));
    t.text(b, "b");
    let tree = t.layout();
    // Ascents differ by 20: the first item is shifted down to share the baseline.
    assert_eq!(t.rect(&tree, a).origin.y, px(20), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b).origin.y, px(0));
    // The row is as tall as the shimmed items: 30 + line height.
    assert_eq!(t.rect(&tree, g).size.height, px(30) + lh());
    // The container's baseline is the shared one.
    let gf = &tree.root.children[0].children[0].children[0];
    match &gf.kind {
        FragmentKind::Box { baseline, .. } => assert_eq!(
            *baseline,
            Some(
                px(30)
                    + text::font_metrics(&font()).ascent
                    + text::half_leading(lh(), text::font_metrics(&font()).content_height())
            )
        ),
        _ => panic!(),
    }
}

#[test]
fn auto_margins_center_and_push_items() {
    let mut t = T::new();
    let g = t.grid(t.body, 300, |s| {
        s.grid_template_rows = tracks(&[fx(100), fx(100)])
    });
    let a = t.item(g, 20, |s| {
        s.width = len(100);
        s.margin.left = LengthPercentageAuto::Auto;
    });
    let b = t.item(g, 20, |s| {
        s.width = len(100);
        s.margin.left = LengthPercentageAuto::Auto;
        s.margin.right = LengthPercentageAuto::Auto;
        s.margin.top = LengthPercentageAuto::Auto;
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(200, 0, 100, 20), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(100, 180, 100, 20));
}

#[test]
fn min_width_auto_grows_fr_tracks_but_not_minmax_zero_tracks() {
    let long = "Unbreakablewordthatisverylong";
    let w = tw(long);
    assert!(w > px(200) && w < px(400), "{w:?}");
    // `1fr 1fr`: the first track's auto minimum is the word; the second gets the rest.
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[fr(1), fr(1)])
    });
    let a = t.div(g, |_| {});
    t.text(a, long);
    let b = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, w, "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), Rect::new(w, Au::ZERO, px(400) - w, lh()));
    // `minmax(0, 1fr)`: tracks stay 200 and the item overflows its area.
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[
            TrackSize::MinMax(TrackBreadth::Fixed(lp(0)), TrackBreadth::Flex(1000)),
            TrackSize::MinMax(TrackBreadth::Fixed(lp(0)), TrackBreadth::Flex(1000)),
        ])
    });
    let a = t.div(g, |_| {});
    t.text(a, long);
    let b = t.div(g, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, w, "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b).origin.x, px(200));
    assert_eq!(t.rect(&tree, b).size.width, px(200));
}

#[test]
fn min_width_auto_is_clamped_by_fixed_tracks() {
    let long = "Unbreakablewordthatisverylongindeedandkeepsgoing";
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100)])
    });
    let a = t.div(g, |_| {});
    t.text(a, long);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, px(100), "{}", t.dump(&tree));
    // A scroll container has a zero automatic minimum in an fr track too.
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[fr(1), fr(1)])
    });
    let a = t.div(g, |s| s.overflow_x = Overflow::Hidden);
    t.text(a, long);
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, px(200), "{}", t.dump(&tree));
}

#[test]
fn percentage_item_sizes_resolve_against_the_grid_area() {
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[fx(200), fx(200)]);
        s.grid_template_rows = tracks(&[fx(100)]);
    });
    let a = t.div(g, |s| {
        s.width = pct(50);
        s.height = pct(50);
        s.padding.left = LengthPercentage::Percent(1000);
        s.margin.top = LengthPercentageAuto::Set(LengthPercentage::Percent(1000));
    });
    let tree = t.layout();
    // Width 100 + 20 padding; margin-top 10% of the area width (20).
    assert_eq!(t.rect(&tree, a), r(0, 20, 120, 50), "{}", t.dump(&tree));
}

#[test]
fn box_sizing_border_box_items_and_max_width_end_stretching() {
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[fx(200), fx(200)])
    });
    let a = t.item(g, 50, |s| {
        s.width = len(100);
        s.padding = Sides::uniform(lp(10));
        s.box_sizing = BoxSizing::BorderBox;
    });
    let b = t.item(g, 50, |s| s.max_width = len(120));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 50), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(200, 0, 120, 50));
}

#[test]
fn replaced_items_are_not_stretched() {
    let mut t = T::new();
    let g = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[fx(200)]);
        s.grid_template_rows = tracks(&[fx(100)]);
    });
    let img = t.el(g, "img", |s| {
        s.display = Display::Inline;
        s.width = len(40);
        s.height = len(30);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, img), r(0, 0, 40, 30), "{}", t.dump(&tree));
}

#[test]
fn relative_positioning_offsets_an_item() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100)])
    });
    let a = t.item(g, 10, |s| {
        s.position = Position::Relative;
        s.inset.left = m(5);
        s.inset.top = m(7);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(5, 7, 100, 10));
}

// §9: absolutely positioned children.

#[test]
fn absolutely_positioned_child_in_a_named_area() {
    let mut t = T::new();
    let g = t.grid(t.body, 800, |s| {
        s.position = Position::Relative;
        s.padding = Sides::uniform(lp(10));
        s.height = len(300);
        s.grid_template_columns = tracks(&[fx(200), fr(1)]);
        s.grid_template_rows = tracks(&[fx(50), fr(1)]);
        s.grid_template_areas = areas(&["head head", "side main"]);
    });
    let a = t.div(g, |s| {
        s.position = Position::Absolute;
        s.grid_row_start = name("main");
        s.grid_row_end = name("main");
        s.grid_column_start = name("main");
        s.grid_column_end = name("main");
        s.inset = Sides::uniform(m(0));
    });
    // Auto lines: the containing block is the padding box.
    let b = t.div(g, |s| {
        s.position = Position::Absolute;
        s.inset.right = m(0);
        s.inset.bottom = m(0);
        s.width = len(20);
        s.height = len(20);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(210, 60, 600, 250), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(800 + 20 - 20, 320 - 20, 20, 20));
}

#[test]
fn absolutely_positioned_child_static_position_when_not_the_containing_block() {
    let mut t = T::new();
    let wrapper = t.div(t.body, |s| s.position = Position::Relative);
    let g = t.grid(wrapper, 400, |s| {
        s.padding = Sides::uniform(lp(10));
        s.grid_template_columns = tracks(&[fx(100), fx(100)]);
    });
    t.item(g, 30, |_| {});
    let a = t.div(g, |s| {
        s.position = Position::Absolute;
        s.width = len(20);
        s.height = len(20);
    });
    let tree = t.layout();
    // The static position is the container's content-box origin.
    assert_eq!(t.rect(&tree, a), r(10, 10, 20, 20), "{}", t.dump(&tree));
}

// Nesting and intrinsic sizing.

#[test]
fn nested_grid_in_a_grid_item_with_fr_rows() {
    let mut t = T::new();
    let outer = t.grid(t.body, 400, |s| {
        s.grid_template_columns = tracks(&[fx(200), fx(200)]);
        s.grid_template_rows = tracks(&[fx(200)]);
    });
    let inner = t.div(outer, |s| {
        s.display = Display::Grid;
        s.grid_template_rows = tracks(&[fr(1), fr(1)]);
        s.grid_template_columns = tracks(&[fr(1)]);
    });
    let a = t.div(inner, |_| {});
    let b = t.div(inner, |_| {});
    let tree = t.layout();
    // The outer item is stretched to 200px, which is definite for the inner rows.
    assert_eq!(t.rect(&tree, inner), r(0, 0, 200, 200), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, a), r(0, 0, 200, 100));
    assert_eq!(t.rect(&tree, b), r(0, 100, 200, 100));
}

#[test]
fn grid_inside_a_float_is_sized_by_its_intrinsic_width() {
    let mut t = T::new();
    let f = t.div(t.body, |s| s.float = Float::Left);
    let g = t.div(f, |s| {
        s.display = Display::Grid;
        s.grid_template_columns = tracks(&[fx(100), fx(100)]);
        s.column_gap = lp(10);
    });
    t.item(g, 10, |_| {});
    let after = t.div(t.body, |s| s.height = len(10));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f), r(0, 0, 210, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, g).size.width, px(210));
    assert_eq!(t.rect(&tree, after), r(0, 0, 800, 10));
    // Auto columns: the float's max-content is the sum of the items' widths.
    let mut t = T::new();
    let f = t.div(t.body, |s| s.float = Float::Left);
    let g = t.div(f, |s| {
        s.display = Display::Grid;
        s.grid_template_columns = tracks(&[TrackSize::Auto, TrackSize::Auto]);
    });
    t.item(g, 10, |s| s.width = len(50));
    t.item(g, 10, |s| s.width = len(70));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f).size.width, px(120), "{}", t.dump(&tree));
}

#[test]
fn intrinsic_widths_of_fr_columns_and_min_content() {
    let mut t = T::new();
    let f = t.div(t.body, |s| s.float = Float::Left);
    let g = t.div(f, |s| {
        s.display = Display::Grid;
        s.grid_template_columns = tracks(&[fr(1), fr(2)]);
    });
    t.item(g, 10, |s| s.width = len(100));
    t.item(g, 10, |s| s.width = len(100));
    let tree = t.layout();
    // Max-content: the fr size is max(100 / 1, 100 / 2) = 100, so 100 + 200.
    assert_eq!(t.rect(&tree, f).size.width, px(300), "{}", t.dump(&tree));
    // Min-content (a narrow float): flexible tracks keep their base sizes.
    let mut t = T::new();
    let f = t.div(t.body, |s| {
        s.float = Float::Left;
        s.width = Sizing::MinContent;
    });
    let g = t.div(f, |s| {
        s.display = Display::Grid;
        s.grid_template_columns = tracks(&[fr(1), fr(2)]);
    });
    t.item(g, 10, |s| s.width = len(100));
    t.item(g, 10, |s| s.width = len(100));
    let tree = t.layout();
    assert_eq!(t.rect(&tree, f).size.width, px(200), "{}", t.dump(&tree));
}

#[test]
fn inline_grid_shrinks_to_fit_on_a_line() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "x");
    let g = t.el(p, "span", |s| {
        s.display = Display::InlineGrid;
        s.grid_template_columns = tracks(&[fx(100), fx(100)]);
    });
    t.item(g, 10, |_| {});
    let tree = t.layout();
    let gr = t.rect(&tree, g);
    assert_eq!(gr.size.width, px(200), "{}", t.dump(&tree));
    assert_eq!(gr.origin.x, tw("x"));
}

#[test]
fn grid_container_baseline_is_its_first_item_when_nothing_is_baseline_aligned() {
    let mut t = T::new();
    let p = t.div(t.body, |_| {});
    t.text(p, "x");
    let g = t.el(p, "span", |s| {
        s.display = Display::InlineGrid;
        s.grid_template_columns = tracks(&[fx(100)]);
    });
    let a = t.div(g, |s| s.padding.top = lp(20));
    t.text(a, "y");
    let tree = t.layout();
    // The inline-grid's baseline is the item's text baseline, 20px below where the
    // line's text would sit, so the line grows and the "x" run moves down 20px.
    assert_eq!(
        t.rect(&tree, g).origin.y,
        t.rect(&tree, p).origin.y,
        "{}",
        t.dump(&tree)
    );
    assert_eq!(t.rect(&tree, p).size.height, px(20) + lh());
    let mut x_top = None;
    tree.root.walk(Default::default(), &mut |f, rect| {
        if matches!(&f.kind, FragmentKind::Text { text, .. } if text == "x") {
            x_top = Some(rect.origin.y);
        }
    });
    assert_eq!(x_top, Some(px(20)));
}

#[test]
fn empty_grid_has_the_height_of_its_explicit_rows() {
    let mut t = T::new();
    let g = t.grid(t.body, 100, |s| {
        s.grid_template_rows = tracks(&[fx(30), fx(40)]);
        s.row_gap = lp(5);
    });
    let tree = t.layout();
    assert_eq!(t.rect(&tree, g), r(0, 0, 100, 75));
}

#[test]
fn scroll_container_grid_reserves_a_scrollbar() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| {
        s.height = len(50);
        s.overflow_y = Overflow::Scroll;
        s.grid_template_columns = tracks(&[fr(1)]);
    });
    let a = t.item(g, 100, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a).size.width, px(185), "{}", t.dump(&tree));
}

#[test]
fn floated_children_are_grid_items() {
    let mut t = T::new();
    let g = t.grid(t.body, 200, |s| {
        s.grid_template_columns = tracks(&[fx(100), fx(100)])
    });
    let a = t.item(g, 10, |s| s.float = Float::Right);
    let b = t.item(g, 10, |_| {});
    let tree = t.layout();
    assert_eq!(t.rect(&tree, a), r(0, 0, 100, 10), "{}", t.dump(&tree));
    assert_eq!(t.rect(&tree, b), r(100, 0, 100, 10));
}

#[test]
fn placement_unit_tests() {
    use super::{auto_place, resolve_axis, Names, Res};
    let mut names = Names {
        lines: Default::default(),
        explicit: 3,
    };
    names.lines.insert("a".into(), vec![1, 3]);
    assert_eq!(
        resolve_axis(&names, &line(2), &line(2)),
        Res::Definite(2, 3)
    );
    assert_eq!(
        resolve_axis(&names, &line(3), &line(1)),
        Res::Definite(1, 3)
    );
    assert_eq!(
        resolve_axis(&names, &GridLine::Auto, &line(3)),
        Res::Definite(2, 3)
    );
    assert_eq!(
        resolve_axis(&names, &span(3), &GridLine::Auto),
        Res::Span(3)
    );
    assert_eq!(resolve_axis(&names, &span(2), &span(3)), Res::Span(2));
    assert_eq!(
        resolve_axis(
            &names,
            &GridLine::Line(-1, Some("a".into())),
            &GridLine::Auto
        ),
        Res::Definite(3, 4)
    );
    assert_eq!(
        resolve_axis(
            &names,
            &GridLine::Line(3, Some("a".into())),
            &GridLine::Auto
        ),
        Res::Definite(5, 6)
    );
    assert_eq!(
        resolve_axis(
            &names,
            &GridLine::Line(-3, Some("a".into())),
            &GridLine::Auto
        ),
        Res::Definite(0, 1)
    );
    assert_eq!(
        resolve_axis(&names, &GridLine::Span(2, Some("a".into())), &line(4)),
        Res::Definite(1, 4)
    );
    // Two auto items in a two-column grid, then a span that wraps.
    let placed = auto_place(
        &[
            (Res::Span(1), Res::Span(1)),
            (Res::Span(1), Res::Span(1)),
            (Res::Span(1), Res::Span(2)),
        ],
        2,
        false,
    );
    assert_eq!(
        placed,
        vec![((1, 2), (1, 2)), ((1, 2), (2, 3)), ((2, 3), (1, 3))]
    );
}
