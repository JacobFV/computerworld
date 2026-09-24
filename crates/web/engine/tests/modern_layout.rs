//! Regression tests for the engine bugs the modern parity fixtures (stripe-marketing,
//! amazon-grid, github-repo) exposed. Each is a small document run through the whole
//! pipeline (parse, cascade, layout) and checked against the geometry Chromium gives,
//! by element id, in CSS px.

mod support;

#[cfg(feature = "pipeline")]
mod cases {
    use super::support::*;

    struct Page(Dump);

    fn page(html: &str) -> Page {
        let vp = viewport();
        let rendered = run(html, vp);
        if std::env::var_os("CW_DUMP_TREE").is_some() {
            eprintln!("{}", cw_web::layout::debug::dump(&rendered.tree));
        }
        Page(engine_dump("case.html", &rendered, vp))
    }

    impl Page {
        fn rect(&self, id: &str) -> DumpRect {
            self.0
                .nodes
                .iter()
                .find_map(|n| match n {
                    DumpNode::Element { id: i, rect, .. } if i == id => Some(*rect),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no element #{id}"))
        }
        /// The text lines of the element's text children, as (text path order) rects.
        fn text_lines(&self, id: &str) -> Vec<DumpRect> {
            let path = self
                .0
                .nodes
                .iter()
                .find_map(|n| match n {
                    DumpNode::Element { id: i, path, .. } if i == id => Some(path.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no element #{id}"));
            self.0
                .nodes
                .iter()
                .filter_map(|n| match n {
                    DumpNode::Text { parent, rects, .. } if *parent == path => Some(rects.clone()),
                    _ => None,
                })
                .flatten()
                .collect()
        }
        fn computed(&self, id: &str, prop: &str) -> String {
            self.0
                .nodes
                .iter()
                .find_map(|n| match n {
                    DumpNode::Element {
                        id: i, computed, ..
                    } if i == id => computed.get(prop).cloned(),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no #{id} or no `{prop}`"))
        }
    }

    #[track_caller]
    fn close(got: f64, want: f64, what: &str) {
        assert!(
            (got - want).abs() <= 0.51,
            "{what}: expected {want}, got {got}"
        );
    }

    #[track_caller]
    fn rect_is(p: &Page, id: &str, x: f64, y: f64, w: f64, h: f64) {
        let r = p.rect(id);
        close(r.x, x, &format!("#{id} x"));
        close(r.y, y, &format!("#{id} y"));
        close(r.width, w, &format!("#{id} width"));
        close(r.height, h, &format!("#{id} height"));
    }

    const RESET: &str = "<!doctype html><style>html, body { margin: 0; font: 14px/20px Arial } * { box-sizing: border-box }</style>";

    /// An inline-flex container whose items have no baseline synthesises one from its
    /// first item's bottom edge, so a row of icon boxes sits on the text baseline
    /// instead of hanging below it (amazon-grid's star rows).
    #[test]
    fn inline_flex_of_empty_boxes_sits_on_the_baseline() {
        let p = page(&format!(
            "{RESET}<style>li {{ line-height: 22px; list-style: none }} .stars {{ display: inline-flex; gap: 1px; vertical-align: -2px }} .stars i {{ display: block; width: 14px; height: 14px }}</style>
             <ul style='margin:0;padding:0'><li id=li><span class=stars id=stars><i></i><i></i></span> &amp; Up</li></ul>"
        ));
        rect_is(&p, "li", 0.0, 0.0, 1280.0, 22.0);
        rect_is(&p, "stars", 0.0, 4.0, 29.0, 14.0);
    }

    /// `aspect-ratio` gives an auto height from the width (of the `box-sizing` box),
    /// that height is definite for percentage children, a definite height gives an
    /// auto width, and content taller than the ratio grows the box unless it clips.
    #[test]
    fn aspect_ratio_sizes_non_replaced_boxes() {
        let p = page(&format!(
            "{RESET}<div style='width: 400px'>
               <div id=sq style='aspect-ratio: 1; border: 10px solid'><div id=kid style='width: 50%; height: 50%'></div></div>
               <div id=wide style='aspect-ratio: 16 / 9; box-sizing: content-box; padding: 5px'></div>
               <div id=fromh style='height: 50px; aspect-ratio: 2'></div>
               <div id=grows style='width: 100px; aspect-ratio: 4 / 1'><div style='height: 60px'></div></div>
               <div id=clips style='width: 100px; aspect-ratio: 4 / 1; overflow: hidden'><div style='height: 60px'></div></div>
             </div>"
        ));
        rect_is(&p, "sq", 0.0, 0.0, 400.0, 400.0);
        rect_is(&p, "kid", 10.0, 10.0, 190.0, 190.0);
        // content box 390 wide: 390 * 9 / 16 = 219.375, plus padding.
        rect_is(&p, "wide", 0.0, 400.0, 400.0, 229.375);
        rect_is(&p, "fromh", 0.0, 629.375, 100.0, 50.0);
        rect_is(&p, "grows", 0.0, 679.375, 100.0, 60.0);
        rect_is(&p, "clips", 0.0, 739.375, 100.0, 25.0);
        assert_eq!(p.computed("sq", "display"), "block");
    }

    /// amazon-grid's thumbnails: a column flex item with `width: 100%` and
    /// `aspect-ratio: 1` that is itself a centring flex container around a
    /// percentage-sized box.
    #[test]
    fn aspect_ratio_flex_item_in_a_column_card() {
        let p = page(&format!(
            "{RESET}<div id=card style='width: 239px; border: 1px solid; display: flex; flex-direction: column'>
               <div id=thumb style='width: 100%; aspect-ratio: 1; display: flex; align-items: center; justify-content: center'>
                 <div id=art style='width: 60%; height: 60%'></div>
               </div>
               <div id=body style='flex: 1'><div style='height: 50px'></div></div>
             </div>"
        ));
        rect_is(&p, "thumb", 1.0, 1.0, 237.0, 237.0);
        rect_is(&p, "art", 48.4, 48.4, 142.2, 142.2);
        rect_is(&p, "body", 1.0, 238.0, 237.0, 50.0);
        rect_is(&p, "card", 0.0, 0.0, 239.0, 289.0);
    }

    /// A collapsible space that ends a `nowrap` run hangs: it is not part of the
    /// run's min-content width, so a button whose label is `"+ "` before an icon
    /// (github-repo's header) is not one space too wide.
    #[test]
    fn trailing_nowrap_space_is_not_intrinsic_width() {
        let p = page(&format!(
            "{RESET}<style>.btn {{ display: inline-flex; align-items: center; gap: 6px; padding: 0 12px; border: 1px solid; white-space: nowrap }} .caret {{ display: inline-block; width: 8px; height: 4px }}</style>
             <div style='display: flex'><a class=btn id=spaced>+ <span class=caret></span></a><a class=btn id=tight>+<span class=caret></span></a></div>"
        ));
        let (spaced, tight) = (p.rect("spaced"), p.rect("tight"));
        close(spaced.width, tight.width, "a trailing space adds no width");
        // 26px of padding and border, the `+` (8.17px in Arimo at 14px), the gap, the caret.
        close(tight.width, 26.0 + 8.17 + 6.0 + 8.0, "#tight width");
    }

    /// `getComputedStyle` reports what an `auto` margin resolved to. On a flex item
    /// that is the free space it absorbed (github-repo's gear pushed right past the
    /// heading text and the gap; amazon-grid's button pushed to the card's foot),
    /// not the distance to the container's edge.
    #[test]
    fn auto_margins_of_flex_items_report_the_space_they_absorbed() {
        let p = page(&format!(
            "{RESET}<h2 id=h style='margin: 0; width: 296px; display: flex; align-items: center; gap: 8px; font: bold 16px/24px Arial'>About <span id=gear style='display: inline-block; margin-left: auto; width: 16px; height: 16px'></span></h2>
             <div style='display: flex; flex-direction: column; width: 100px; height: 200px'><div style='height: 30px'></div><a id=add style='margin-top: auto; align-self: flex-start; height: 29px'>Add</a></div>"
        ));
        rect_is(&p, "gear", 280.0, 4.0, 16.0, 16.0);
        // 296 - "About" (46.2px) - the 8px gap - 16px.
        let ml: f64 = p
            .computed("gear", "margin-left")
            .trim_end_matches("px")
            .parse()
            .unwrap();
        close(ml, 225.8, "#gear margin-left");
        assert_eq!(p.computed("add", "margin-top"), "141px");
        rect_is(&p, "add", 0.0, 24.0 + 171.0, p.rect("add").width, 29.0);
    }

    /// stripe-marketing's container: `width: min(1080px, calc(100% - 2 * 32px))` mixes
    /// a length with a percentage, so the comparison waits for the containing block;
    /// with `margin: 0 auto` it centres at 1280px and tracks a narrow parent.
    #[test]
    fn min_of_a_length_and_a_percentage_resolves_at_layout() {
        let p = page(&format!(
            "{RESET}<style>:root {{ --container: 1080px; --gutter: 32px }} .container {{ width: min(var(--container), calc(100% - 2 * var(--gutter))); margin: 0 auto; height: 10px }}</style>
             <div class=container id=wide></div>
             <div style='width: 500px'><div class=container id=narrow></div></div>
             <div style='width: 500px'><div id=clamped style='width: clamp(100px, 50%, 200px); height: 10px'></div><div id=floor style='width: max(300px, 50%); height: 10px'></div></div>"
        ));
        rect_is(&p, "wide", 100.0, 0.0, 1080.0, 10.0);
        assert_eq!(p.computed("wide", "margin-left"), "100px");
        rect_is(&p, "narrow", 32.0, 10.0, 436.0, 10.0);
        rect_is(&p, "clamped", 0.0, 20.0, 200.0, 10.0);
        rect_is(&p, "floor", 0.0, 30.0, 300.0, 10.0);
    }

    /// `ch` is the advance of `0` in the element's own font (0.556em in Arimo), not
    /// half an em: stripe's `max-width: 50ch` lead is 500.5px wide at 18px.
    #[test]
    fn ch_measures_the_zero_of_the_font() {
        let p = page(&format!(
            "{RESET}<p id=lead style='margin: 0; font-size: 18px; max-width: 50ch'>x</p>"
        ));
        close(p.rect("lead").width, 500.53, "#lead width");
    }

    /// `<sup>` and `<sub>` inherit `line-height` as in Blink's sheet (no
    /// `line-height: normal`), so a superscript in `line-height: 1.5` text reports
    /// and uses 1.5 times its own smaller font size.
    #[test]
    fn sup_inherits_line_height() {
        let p = page(&format!("{RESET}<p style='margin: 0; font-size: 16px; line-height: 1.5'>x<sup id=s style='font-size: 18px'>2</sup></p>"));
        assert_eq!(p.computed("s", "line-height"), "27px");
    }

    /// The dump measures like `getBoundingClientRect`: a transformed element and its
    /// descendants report the bounds of their transformed boxes, while the computed
    /// `width` stays the untransformed used value.
    #[test]
    fn client_rects_follow_transforms() {
        let p = page(&format!(
            "{RESET}<div style='position: relative; width: 400px; height: 100px'>
               <span id=badge style='position: absolute; top: 0; left: 50%; width: 100px; height: 20px; transform: translateX(-50%)'></span>
               <div id=lifted style='width: 100px; height: 50px; transform: translateY(-12px)'><div id=inner style='width: 10px; height: 10px'></div></div>
             </div>
             <div id=turned style='width: 200px; height: 100px; transform: rotate(90deg)'></div>
             <div id=scaled style='width: 100px; height: 100px; transform: scale(0.5); transform-origin: 0 0'></div>"
        ));
        rect_is(&p, "badge", 150.0, 0.0, 100.0, 20.0);
        rect_is(&p, "lifted", 0.0, -12.0, 100.0, 50.0);
        rect_is(&p, "inner", 0.0, -12.0, 10.0, 10.0);
        // A quarter turn about the centre (100, 150) swaps the sides.
        rect_is(&p, "turned", 50.0, 50.0, 100.0, 200.0);
        assert_eq!(p.computed("turned", "width"), "200px");
        rect_is(&p, "scaled", 0.0, 200.0, 50.0, 50.0);
    }

    /// A grid item's `auto` margins take the free space of its area, and the used
    /// value is what the computed style reports.
    #[test]
    fn auto_margins_of_grid_items_report_the_free_space_of_the_area() {
        let p = page(&format!(
            "{RESET}<div style='display: grid; grid-template-columns: 300px 300px; grid-template-rows: 100px'>
               <div id=pushed style='width: 100px; height: 40px; margin-left: auto; margin-top: auto'></div>
               <div id=centred style='width: 100px; height: 40px; margin: auto'></div>
             </div>"
        ));
        rect_is(&p, "pushed", 200.0, 60.0, 100.0, 40.0);
        assert_eq!(p.computed("pushed", "margin-left"), "200px");
        assert_eq!(p.computed("pushed", "margin-top"), "60px");
        rect_is(&p, "centred", 400.0, 30.0, 100.0, 40.0);
        assert_eq!(p.computed("centred", "margin-right"), "100px");
        assert_eq!(p.computed("centred", "margin-bottom"), "30px");
    }

    /// amazon-grid's titles: `display: -webkit-box; -webkit-box-orient: vertical;
    /// -webkit-line-clamp: 2` is a block (`flow-root`) that shows two lines, the
    /// second ending in an ellipsis inside the line, and is two lines tall on its own
    /// (no `max-height` needed); a text that fits is left alone.
    #[test]
    fn webkit_line_clamp_keeps_two_lines() {
        let p = page(&format!(
            "{RESET}<style>.t {{ width: 209px; font-size: 16px; line-height: 22px; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2 }}</style>
             <a class=t id=long>Orbita Pocket Bone Conduction Headphones for Running and Cycling, Yellow</a>
             <a class=t id=short>Orbita Pocket</a>
             <div id=after style='height: 5px'></div>"
        ));
        assert_eq!(p.computed("long", "display"), "flow-root");
        rect_is(&p, "long", 0.0, 0.0, 209.0, 44.0);
        // Chromium gives four rects for the text: the two full lines, the cut line as
        // if it were not cut, and what stays of it; the third line stays laid out
        // below the box (a `Range` reports it, as in Blink) but is hidden for paint.
        let lines = p.text_lines("long");
        assert_eq!(lines.len(), 4, "a cut line is reported twice: {lines:?}");
        close(lines[0].width, 138.75, "the first line");
        close(lines[1].y, 24.0, "the cut line's y");
        close(lines[1].width, 199.25, "the cut line before the cut");
        close(
            lines[2].y,
            24.0,
            "what stays of the cut line is on the same line",
        );
        close(lines[3].y, 46.0, "the third line sits below the box");
        close(lines[3].width, 201.92, "the third line keeps its own width");
        // Blink cuts at the content edge less the ellipsis, not at the line's own
        // end: 185.03px stay of a 199.25px line, and the ellipsis (16px at this size)
        // goes in the 209px box after them.
        close(lines[2].width, 185.03, "what stays of the cut line");
        assert!(
            lines[2].width + 16.0 <= 209.0,
            "the ellipsis fits the box: {lines:?}"
        );
        rect_is(&p, "short", 0.0, 44.0, 209.0, 22.0);
        assert_eq!(p.text_lines("short").len(), 1);
        rect_is(&p, "after", 0.0, 66.0, 1280.0, 5.0);
    }

    /// `letter-spacing` does not turn pair kerning off: Blink adds the spacing on top
    /// of the kerned advance, so stripe-marketing's uppercase `0.1em` label is exactly
    /// one spacing per character wider than the same text unspaced.
    #[test]
    fn letter_spacing_keeps_pair_kerning() {
        let p = page(&format!(
            "{RESET}<style>p {{ margin: 0; font-family: Inter, -apple-system, 'Segoe UI', Roboto, sans-serif; font-size: 13px; text-transform: uppercase }} .ls {{ letter-spacing: 0.1em }}</style>
             <p class=ls id=spaced>Trusted by ambitious companies everywhere</p>
             <p id=plain>Trusted by ambitious companies everywhere</p>
             <p class=ls id=kerny style='font: 20px/24px Arial; letter-spacing: 5px; text-transform: none'>AVATAR WAVY Ty.</p>
             <p id=kernyplain style='font: 20px/24px Arial; text-transform: none'>AVATAR WAVY Ty.</p>"
        ));
        let (spaced, plain) = (
            p.text_lines("spaced")[0].width,
            p.text_lines("plain")[0].width,
        );
        // Chromium: 386.3125 spaced, 333.0156 plain, 41 characters at 1.3px.
        close(plain, 333.02, "the unspaced label");
        close(spaced, 386.31, "the spaced label");
        close(
            spaced - plain,
            41.0 * 1.3,
            "one spacing per character, kerning kept",
        );
        let (kerny, kernyplain) = (
            p.text_lines("kerny")[0].width,
            p.text_lines("kernyplain")[0].width,
        );
        // Chromium: 166.328 unspaced and 241.328 at `letter-spacing: 5px`.
        close(kernyplain, 166.33, "a kerned run");
        close(kerny, 241.33, "the same run spaced");
    }

    /// When the clamped line's text plus the ellipsis fits the box, Blink appends the
    /// ellipsis and cuts nothing: the line keeps its own rect, one per line, and the
    /// text after the clamp still lays out below.
    #[test]
    fn webkit_line_clamp_appends_an_ellipsis_that_fits() {
        let p = page(&format!(
            "{RESET}<style>.t {{ width: 209px; font-size: 16px; line-height: 22px; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 1 }}</style>
             <a class=t id=one>Short Supercalifragilisticexpialidociousness</a>"
        ));
        rect_is(&p, "one", 0.0, 0.0, 209.0, 22.0);
        let lines = p.text_lines("one");
        assert_eq!(
            lines.len(),
            2,
            "nothing is cut, so no line is reported twice: {lines:?}"
        );
        // The rects are the glyph boxes inside the 22px lines. "Short" is 38.27px in
        // Arimo at 16px; the ellipsis is a run of its own, outside the text node.
        close(lines[0].width, 38.27, "the kept line is whole");
        close(lines[1].y, 24.0, "the long word lays out below the clamp");
    }

    /// A fixed box with all four insets and `height: auto` has a definite height, so
    /// its flex items centre in the viewport and its absolutely positioned children
    /// fill it (a Tailwind `fixed inset-0 flex items-center` modal and its backdrop).
    #[test]
    fn fixed_inset_box_fills_the_viewport_and_centres_its_flex_items() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0 }</style>\
             <div id=o style='position: fixed; top: 0; right: 0; bottom: 0; left: 0; display: flex; align-items: center; justify-content: center'>\
             <div id=bg style='position: absolute; top: 0; right: 0; bottom: 0; left: 0'></div>\
             <div id=m style='position: relative; width: 200px; height: 100px'></div></div>",
        );
        rect_is(&p, "o", 0.0, 0.0, 1280.0, 800.0);
        rect_is(&p, "bg", 0.0, 0.0, 1280.0, 800.0);
        rect_is(&p, "m", 540.0, 350.0, 200.0, 100.0);
    }

    /// A text input has a natural size but no natural aspect ratio: at `width: 100%`
    /// its height stays one line (Tailwind's `w-full` search fields).
    #[test]
    fn a_full_width_text_input_keeps_its_one_line_height() {
        let p = page(
            "<!DOCTYPE html><style>html { line-height: 1.5; font-family: Inter } body { margin: 0 }\
             input { font-family: inherit; font-size: 100%; line-height: inherit; margin: 0; padding: 0; border-width: 0 }</style>\
             <div id=d><input id=i placeholder='Search' style='font-size: 14px; line-height: 20px; padding: 8px 16px; width: 100%; box-sizing: border-box'></div>",
        );
        rect_is(&p, "i", 0.0, 0.0, 1280.0, 36.0);
        rect_is(&p, "d", 0.0, 0.0, 1280.0, 36.0);
    }

    /// Inline SVG lays out as a replaced box sized by its classes, and each shape
    /// inside reports its fill geometry mapped through the `viewBox`, as
    /// `getBoundingClientRect` does: a Lucide icon at 16 px, a chart's text label.
    #[test]
    fn inline_svg_children_report_their_geometry() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0; font: 16px/1.5 Arimo } svg { display: block }</style>\
             <svg id=icon width=24 height=24 viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 style='width: 16px; height: 16px'>\
             <circle id=c cx=11 cy=11 r=8 /><path id=p d='m21 21-4.3-4.3' /><rect id=r x=3 y=3 width=7 height=9 rx=1 /></svg>\
             <svg id=chart viewBox='0 0 200 100' style='width: 400px; height: 200px'>\
             <g id=g><line id=l x1=0 x2=200 y1=50 y2=50 stroke=black /><text id=t x=100 y=90 text-anchor=middle font-size=10>Mon</text></g></svg>",
        );
        rect_is(&p, "icon", 0.0, 0.0, 16.0, 16.0);
        let s = 16.0 / 24.0;
        rect_is(&p, "c", 3.0 * s, 3.0 * s, 16.0 * s, 16.0 * s);
        rect_is(&p, "p", 16.7 * s, 16.7 * s, 4.3 * s, 4.3 * s);
        rect_is(&p, "r", 3.0 * s, 3.0 * s, 7.0 * s, 9.0 * s);
        // A presentation attribute is a computed value; a shape's box is not.
        assert_eq!(p.computed("r", "width"), "7px");
        assert_eq!(p.computed("c", "width"), "auto");
        rect_is(&p, "chart", 0.0, 16.0, 400.0, 200.0);
        rect_is(&p, "l", 0.0, 116.0, 400.0, 0.0);
        // `font-size=10` in a 2x viewBox is 20 px text, centred on x = 200.
        let t = p.rect("t");
        close(t.x + t.width / 2.0, 200.0, "text centred on its anchor");
        close(
            t.y + t.height,
            16.0 + 180.0 + 4.0,
            "text box ends at the descent",
        );
        assert_eq!(p.computed("t", "font-size"), "10px");
        assert_eq!(p.computed("t", "display"), "block");
        let g = p.rect("g");
        close(g.width, 400.0, "the group spans its children");
        assert_eq!(p.computed("chart", "overflow-x"), "hidden");
    }

    /// A text input as a flex item keeps its one-line height when the row gives it
    /// the width (a chat composer's `flex-1` field).
    #[test]
    fn a_flexed_text_input_keeps_its_one_line_height() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0; font: 14px/20px Arimo }\
             input { font: inherit; margin: 0; padding: 4px 0; border: 0 }</style>\
             <div id=row style='display: flex; align-items: center; width: 600px'>\
             <input id=i style='flex: 1'><button style='width: 32px; height: 32px; border: 0; padding: 0'></button></div>",
        );
        rect_is(&p, "i", 0.0, 2.0, 568.0, 28.0);
        rect_is(&p, "row", 0.0, 0.0, 600.0, 32.0);
    }
}
