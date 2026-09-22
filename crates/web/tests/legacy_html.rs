//! Regression tests for the legacy-HTML and quirks-mode rules the parity fixtures
//! (google-1998, hn-front, tables, acid1) depend on: each test runs a small document
//! through the whole pipeline and checks the geometry Chromium produces for it. The
//! expected numbers were read from Chromium's dumps of those fixtures.

mod support;

use support::*;

fn dump(html: &str) -> Dump {
    let vp = viewport();
    let rendered = run(html, vp);
    engine_dump("test.html", &rendered, vp)
}

fn rect_of<'a>(d: &'a Dump, path: &str) -> &'a DumpRect {
    for n in &d.nodes {
        if let DumpNode::Element { path: p, rect, .. } = n {
            if p == path {
                return rect;
            }
        }
    }
    panic!(
        "no element at {path}; have {:?}",
        d.nodes
            .iter()
            .map(|n| n.path().to_owned())
            .collect::<Vec<_>>()
    );
}

fn computed<'a>(d: &'a Dump, path: &str, prop: &str) -> &'a str {
    for n in &d.nodes {
        if let DumpNode::Element {
            path: p, computed, ..
        } = n
        {
            if p == path {
                return computed.get(prop).map(String::as_str).unwrap_or("");
            }
        }
    }
    panic!("no element at {path}")
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

const QUIRKS: &str = "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\">";

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn quirks_mode_html_and_body_stretch_to_the_viewport() {
    let d = dump(&format!("{QUIRKS}<html><body>short</body></html>"));
    assert_eq!(rect_of(&d, "html").height, 800.0);
    assert_eq!(rect_of(&d, "html>body").height, 784.0);
    // Standards mode: the body is as tall as its content.
    let d = dump("<!DOCTYPE html><html><body>short</body></html>");
    assert!(rect_of(&d, "html>body").height < 100.0);
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn center_element_centres_tables_and_sized_blocks() {
    let d = dump("<!DOCTYPE html><html><body><center><table width=\"500\"><tr><td>x</td></tr></table><div style=\"width:100px\">b</div></center></body></html>");
    let t = rect_of(&d, "html>body>center:nth-child(1)>table:nth-child(1)");
    assert_eq!(t.width, 500.0);
    assert_eq!(t.x, 8.0 + (1264.0 - 500.0) / 2.0);
    let b = rect_of(&d, "html>body>center:nth-child(1)>div:nth-child(2)");
    assert_eq!(b.x, 8.0 + (1264.0 - 100.0) / 2.0);
    assert_eq!(
        computed(&d, "html>body>center:nth-child(1)", "text-align"),
        "-webkit-center"
    );
    // `align=center` on a cell is the same thing; `align=middle` is plain `center`.
    let d = dump("<!DOCTYPE html><html><body><table><tr><td align=center><div style=\"width:50px\">b</div></td><td align=middle>y</td></tr></table></body></html>");
    let cell = "html>body>table:nth-child(1)>tbody:nth-child(1)>tr:nth-child(1)>td:nth-child(1)";
    assert_eq!(computed(&d, cell, "text-align"), "-webkit-center");
    assert_eq!(
        computed(
            &d,
            "html>body>table:nth-child(1)>tbody:nth-child(1)>tr:nth-child(1)>td:nth-child(2)",
            "text-align"
        ),
        "center"
    );
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn quirks_mode_tables_reset_text_align_and_line_height_and_inputs_are_border_box() {
    let d = dump(&format!("{QUIRKS}<html><body style=\"line-height: 30px\"><center><table><tr><td>x</td></tr></table></center><input type=text size=20></body></html>"));
    assert_eq!(
        computed(
            &d,
            "html>body>center:nth-child(1)>table:nth-child(1)",
            "text-align"
        ),
        "start"
    );
    assert_eq!(
        computed(
            &d,
            "html>body>center:nth-child(1)>table:nth-child(1)",
            "line-height"
        ),
        "normal"
    );
    assert_eq!(
        computed(&d, "html>body>input:nth-child(2)", "box-sizing"),
        "border-box"
    );
    let d = dump("<!DOCTYPE html><html><body><input type=text size=20></body></html>");
    assert_eq!(
        computed(&d, "html>body>input:nth-child(1)", "box-sizing"),
        "content-box"
    );
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn submit_buttons_take_the_button_rules() {
    // The rule was dropped whole when its list held an unsupported pseudo-element.
    let d = dump("<!DOCTYPE html><html><body><input type=submit value=\"Go\"><button>b</button></body></html>");
    let p = "html>body>input:nth-child(1)";
    assert_eq!(computed(&d, p, "padding-left"), "6px");
    assert_eq!(computed(&d, p, "background-color"), "rgb(239, 239, 239)");
    assert_eq!(computed(&d, p, "white-space"), "pre");
    assert_eq!(computed(&d, p, "text-align"), "center");
    // 13.33px Arial: 15px line, 1px padding, 2px border each side.
    assert_eq!(rect_of(&d, p).height, 21.0);
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn text_input_width_follows_blink_formula() {
    // `ceil(ceil(size * avg) + max - avg)`. google-1998's 16px Arial `size=55` input
    // is 531px of content in Chromium (539 border box); hn-front's 13.33px monospace
    // `size=17` is 146px there with DejaVu Sans Mono and 145px here with Cousine.
    let d = dump("<!DOCTYPE html><html><body><input type=text size=55 style=\"font-family: Arial; font-size: 12pt\"><input type=text size=17 style=\"font-family: monospace; font-size: 10pt\"></body></html>");
    let p = "html>body>input:nth-child(1)";
    assert_eq!(computed(&d, p, "width"), "531px");
    assert_eq!(rect_of(&d, p).width, 539.0);
    assert_eq!(
        rect_of(&d, p).height,
        24.0,
        "18px line (14 + 3 + the 0.52px gap rounded to 1) + 2px padding + 4px border"
    );
    let p = "html>body>input:nth-child(2)";
    assert!(
        close(rect_of(&d, p).width, 154.0, 1.0),
        "{}",
        rect_of(&d, p).width
    );
    assert_eq!(rect_of(&d, p).height, 21.0);
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn rows_and_row_groups_exclude_the_outer_border_spacing() {
    let d = dump("<!DOCTYPE html><html><body style=\"margin:16px\"><table style=\"border:1px solid; border-spacing:4px\"><tr><td style=\"width:100px; padding:0\">a</td><td style=\"width:50px; padding:0\">b</td></tr></table></body></html>");
    let table = rect_of(&d, "html>body>table:nth-child(1)");
    let tbody = rect_of(&d, "html>body>table:nth-child(1)>tbody:nth-child(1)");
    let tr = rect_of(
        &d,
        "html>body>table:nth-child(1)>tbody:nth-child(1)>tr:nth-child(1)",
    );
    assert_eq!(table.x, 16.0);
    assert_eq!(tbody.x, 16.0 + 1.0 + 4.0);
    assert_eq!(tr.x, tbody.x);
    assert_eq!(tbody.width, 100.0 + 4.0 + 50.0);
    assert_eq!(tr.width, tbody.width);
    assert_eq!(table.width, 2.0 + 3.0 * 4.0 + 150.0);
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn table_rect_includes_its_caption() {
    let d = dump("<!DOCTYPE html><html><body style=\"margin:0\"><table style=\"border-spacing:0\"><caption style=\"padding:4px; font-size:13px; font-family: Arial\">cap</caption><tr><td style=\"padding:0; height:30px\">a</td></tr></table></body></html>");
    let t = rect_of(&d, "html>body>table:nth-child(1)");
    let cap = rect_of(&d, "html>body>table:nth-child(1)>caption:nth-child(1)");
    assert_eq!(cap.height, 23.0, "15px line plus 8px padding");
    assert_eq!(t.y, 0.0);
    assert_eq!(t.height, 23.0 + 30.0);
    assert_eq!(
        computed(&d, "html>body>table:nth-child(1)", "height"),
        "53px"
    );
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn fixed_layout_uses_column_widths_as_border_boxes() {
    let d = dump("<!DOCTYPE html><html><body style=\"margin:16px\"><table style=\"table-layout:fixed; width:600px; border-collapse:collapse\"><col style=\"width:80px\"><col style=\"width:220px\"><col><col><tr><th style=\"border:1px solid; padding:2px 4px\">a</th><th style=\"border:1px solid; padding:2px 4px\">b</th><th style=\"border:1px solid; padding:2px 4px\">c</th><th style=\"border:1px solid; padding:2px 4px\">d</th></tr></table></body></html>");
    let row = "html>body>table:nth-child(1)>tbody:nth-child(2)>tr:nth-child(1)";
    let w: Vec<f64> = (1..=4)
        .map(|i| rect_of(&d, &format!("{row}>th:nth-child({i})")).width)
        .collect();
    assert_eq!(w, vec![80.0, 220.0, 149.5, 149.5]);
    assert_eq!(rect_of(&d, "html>body>table:nth-child(1)").width, 600.0);
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn percentages_truncate_like_layout_units() {
    // Acid1: a 10.638% float next to a 34em float must fit in a 47em line.
    let d = dump("<!DOCTYPE html><html><body style=\"margin:0; font-size:10px\"><div style=\"width:470px\"><div style=\"float:left; width:10.638%; padding:10px; border:5px solid; height:20px\">a</div><div style=\"float:right; width:340px; margin-left:10px; padding:10px; border:10px solid; height:20px\">b</div></div></body></html>");
    let a = rect_of(&d, "html>body>div:nth-child(1)>div:nth-child(1)");
    let b = rect_of(&d, "html>body>div:nth-child(1)>div:nth-child(2)");
    assert!(close(a.width, 80.0, 0.02), "{}", a.width);
    assert_eq!(a.y, b.y, "both floats on the first line");
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn inline_element_rect_covers_its_block_children() {
    // Acid1's `form { display: inline }` holding two `<p>`s.
    let d = dump("<!DOCTYPE html><html><body style=\"margin:0\"><form style=\"display:inline\"><p style=\"margin:0; height:20px\">a</p><p style=\"margin:0; height:24px\">b</p></form></body></html>");
    let f = rect_of(&d, "html>body>form:nth-child(1)");
    assert_eq!(f.height, 44.0);
    assert_eq!(f.width, 1280.0);
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn border_zero_draws_no_cell_borders_and_border_one_does() {
    let d = dump("<!DOCTYPE html><html><body><table border=\"0\" cellspacing=\"0\"><tr><td>a</td></tr></table><table border=\"1\"><tr><td>b</td></tr></table></body></html>");
    let cell0 = "html>body>table:nth-child(1)>tbody:nth-child(1)>tr:nth-child(1)>td:nth-child(1)";
    let cell1 = "html>body>table:nth-child(2)>tbody:nth-child(1)>tr:nth-child(1)>td:nth-child(1)";
    assert_eq!(computed(&d, cell0, "border-left-width"), "0px");
    assert_eq!(
        computed(&d, "html>body>table:nth-child(1)", "border-left-width"),
        "0px"
    );
    assert_eq!(computed(&d, cell1, "border-left-width"), "1px");
    assert_eq!(
        computed(&d, "html>body>table:nth-child(2)", "border-left-width"),
        "1px"
    );
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn tables_do_not_inherit_webkit_center() {
    let d = dump("<!DOCTYPE html><html><body><center><table><tr><td>a</td></tr></table></center></body></html>");
    assert_eq!(
        computed(
            &d,
            "html>body>center:nth-child(1)>table:nth-child(1)",
            "text-align"
        ),
        "start"
    );
    assert_eq!(computed(&d, "html>body>center:nth-child(1)>table:nth-child(1)>tbody:nth-child(1)>tr:nth-child(1)>td:nth-child(1)", "text-align"), "start");
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn quirks_mode_line_height_quirk() {
    // A 16px cell holding only a 13px `<font>`: Chromium makes the line 15px in
    // quirks mode (the strut and the empty inline box do not count).
    let doc = "<html><body style=\"font-family: Arial\"><table cellpadding=0 cellspacing=0><tr><td><font size=-1><a href=x>Help!</a></font></td></tr></table><center><br>x</center>\n<div>\n<font size=-1>y</font>\n<br>\n</div></body></html>";
    let cell = "html>body>table:nth-child(1)>tbody:nth-child(1)>tr:nth-child(1)>td:nth-child(1)";
    let d = dump(&format!("{QUIRKS}{doc}"));
    assert_eq!(rect_of(&d, cell).height, 15.0);
    // A `<br>` alone on a line still makes a full 18px line (17px content area
    // plus the rounded line gap, as google-1998's three leading `<br>`s show).
    assert_eq!(rect_of(&d, "html>body>center:nth-child(2)").height, 36.0);
    // White space around the `<font>` is not text: the line stays 15px.
    assert_eq!(rect_of(&d, "html>body>div:nth-child(3)").height, 15.0);
    // Standards mode: the cell's own 16px strut (18px with the gap) applies.
    let d = dump(&format!("<!DOCTYPE html>{doc}"));
    assert_eq!(rect_of(&d, cell).height, 18.0);
}

#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the pipeline")]
fn body_link_colours_anchors() {
    let d = dump("<!DOCTYPE html><html><body link=\"#0000cc\"><a href=x>a</a><a>b</a><a href=y style=\"color: red\">c</a></body></html>");
    assert_eq!(
        computed(&d, "html>body>a:nth-child(1)", "color"),
        "rgb(0, 0, 204)"
    );
    assert_eq!(
        computed(&d, "html>body>a:nth-child(2)", "color"),
        "rgb(0, 0, 0)"
    );
    assert_eq!(
        computed(&d, "html>body>a:nth-child(3)", "color"),
        "rgb(255, 0, 0)"
    );
}
