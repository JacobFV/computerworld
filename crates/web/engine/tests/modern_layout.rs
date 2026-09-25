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

    /// An absolutely positioned box centred by `auto` margins between its insets
    /// reports the margins it used (an icon placed with `inset-y-0 my-auto`).
    #[test]
    fn auto_margins_of_an_absolute_box_report_their_used_values() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0 }</style>\
             <div style='position: relative; height: 36px'>\
             <div id=a style='position: absolute; top: 0; bottom: 0; left: 12px; height: 16px; width: 16px; margin: auto 0'></div></div>",
        );
        rect_is(&p, "a", 12.0, 10.0, 16.0, 16.0);
        assert_eq!(p.computed("a", "margin-top"), "10px");
        assert_eq!(p.computed("a", "margin-bottom"), "10px");
    }

    /// `min-width: 100%` stretches a table whose columns need less (Tailwind's
    /// `min-w-full`), and the extra width goes to its columns.
    #[test]
    fn a_table_is_at_least_its_min_width() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0; font: 16px Arimo } td { padding: 0 }</style>\
             <div style='width: 500px'><table id=t style='min-width: 100%; border-spacing: 0'>\
             <tr><td id=a>a</td><td id=b>b</td></tr></table></div>",
        );
        let t = p.rect("t");
        close(t.width, 500.0, "table width");
        let (a, b) = (p.rect("a"), p.rect("b"));
        close(a.width + b.width, 500.0, "columns fill it");
    }

    /// A block-level box made absolute after inline content takes its static
    /// position below the line, where it would have started as a block; an inline
    /// one stays beside its line's content (a dropdown menu under its button).
    #[test]
    fn a_blocklike_absolute_box_after_a_line_starts_below_it() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0; font: 16px/20px Arimo }</style>\
             <div style='position: relative; width: 300px'>\
             <button id=b style='display: inline-block; width: 100px; height: 30px; border: 0; padding: 0; vertical-align: top'></button>\
             <div id=menu style='position: absolute; right: 0; margin-top: 8px; width: 50px; height: 10px'></div>\
             <span id=tip style='position: absolute; width: 20px; height: 10px'></span></div>",
        );
        rect_is(&p, "b", 0.0, 0.0, 100.0, 30.0);
        rect_is(&p, "menu", 250.0, 38.0, 50.0, 10.0);
        let tip = p.rect("tip");
        close(tip.y, 0.0, "inline absolute stays on the line");
        close(tip.x, 100.0, "after the button");
    }

    /// A `bottom: 0` sticky box below the fold is pulled up to the viewport's bottom
    /// edge (a form's sticky action bar), and a `right: 0` one to its right edge;
    /// neither leaves its containing block.
    #[test]
    fn sticky_bottom_and_right_pull_the_box_into_the_port() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0 }</style>\
             <form id=f style='display: block; margin-top: 100px'><div style='height: 700px'></div>\
             <div id=foot style='position: sticky; bottom: 0; height: 69px'></div></form>\
             <div style='display: flex; width: 2000px'><div style='width: 1400px'></div>\
             <div id=side style='position: sticky; right: 0; width: 100px; height: 10px'></div></div>\
             <div id=box style='height: 300px; margin-top: 600px'><div style='height: 280px'></div>\
             <div id=kept style='position: sticky; bottom: 0; height: 10px'></div></div>",
        );
        // Chromium's dump of this page.
        rect_is(&p, "foot", 0.0, 731.0, 1280.0, 69.0);
        rect_is(&p, "side", 1180.0, 869.0, 100.0, 10.0);
        rect_is(&p, "kept", 0.0, 1479.0, 1280.0, 10.0);
    }

    /// A textarea's rows size its content box whatever its `box-sizing`, and a menu
    /// list select is one line of rounded ascent and descent plus Blink's internal
    /// pixel above and below, at `line-height: normal` whatever the page says; the
    /// computed values are Chromium's (its dump of this page).
    #[test]
    fn textarea_rows_and_menu_list_heights_follow_blink() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0 } * { box-sizing: border-box }</style>\
             <textarea id=t rows=3 style='display: block; width: 300px; padding: 8px 12px; border: 0; font: 14px/20px Arimo'></textarea>\
             <select id=s style='font: 14px Arimo; padding: 8px 12px; border: 0; line-height: 20px; display: block'><option id=o>One</option></select>",
        );
        rect_is(&p, "t", 0.0, 0.0, 300.0, 76.0);
        assert_eq!(p.computed("t", "vertical-align"), "baseline");
        let s = p.rect("s");
        close(s.y, 76.0, "select top");
        close(s.height, 34.0, "select height");
        assert_eq!(p.computed("s", "line-height"), "normal");
        assert_eq!(p.computed("s", "background-color"), "rgb(239, 239, 239)");
        assert_eq!(p.computed("s", "overflow-x"), "clip");
        assert_eq!(p.computed("o", "display"), "block");
        assert_eq!(p.computed("o", "white-space"), "nowrap");
        assert_eq!(p.computed("o", "padding-left"), "2px");
    }

    /// On the Linux baseline the dumps were made on, a family is found only if the
    /// machine has it or the page downloads it: `@font-face` serving Inter makes
    /// Inter the face (the bundled file is the one the page serves), and without the
    /// rule the list falls through to Liberation Sans's stand-in.
    #[test]
    fn a_font_face_rule_makes_its_family_available() {
        let face = |css: &str| {
            let p = page(&format!(
                "<!DOCTYPE html><style>{css} p {{ font: 16px Inter, sans-serif }}</style><p id=p>Hello</p>"
            ));
            p.0.fonts
                .iter()
                .find(|f| f.family.starts_with("Inter"))
                .map(|f| f.engine.clone())
                .unwrap()
        };
        assert_eq!(
            face("@font-face { font-family: 'Inter'; src: url(/vendor/inter-regular.ttf) format('truetype') }"),
            "Inter"
        );
        assert_eq!(face(""), "Arimo");
        assert_eq!(
            face("@font-face { font-family: Inter; src: local(Inter) }"),
            "Arimo"
        );
    }

    /// A menu list's baseline is its border and padding, Blink's internal pixel and
    /// the font's rounded ascent: on a 40px Arimo line a 13.33px select's top is 22
    /// px below the line's and a 20px one's 16 (Chromium's dump of this page).
    #[test]
    fn a_menu_list_sits_on_blinks_baseline() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0 }</style>\
             <div id=b style='font: 40px Arimo'>x<select id=s2 style='font: 13.3333px Arimo'><option>Hi</option></select></div>\
             <div id=c style='font: 40px Arimo'>x<select id=s3 style='font: 20px Arimo'><option>Hi</option></select></div>",
        );
        let (b, c) = (p.rect("b"), p.rect("c"));
        close(
            p.rect("s2").y - b.y,
            22.0,
            "13.33px select below the line top",
        );
        close(p.rect("s3").y - c.y, 16.0, "20px select below the line top");
        close(p.rect("s3").height, 26.0, "20px select height");
    }
    /// A block-level `<button>` with `width: auto` shrinks to its content instead of
    /// filling its container (Blink's `AutoWidthShouldFitContent`): app-calendar's
    /// month view has `display: flex` day buttons, and a `div` beside them stretches.
    #[test]
    fn a_block_level_button_shrinks_to_fit() {
        let p = page(&format!(
            "{RESET}<div style='width: 300px'>
               <button id=flex style='display: flex; height: 24px; min-width: 24px; padding: 0 4px; border: 0; font: 12px/16px Arimo; justify-content: center'></button>
               <button id=block style='display: block; width: auto; padding: 0; border: 0; height: 10px'><span style='display: inline-block; width: 50px'></span></button>
               <button id=wide style='display: block; padding: 0; border: 0; height: 10px; width: 100%'></button>
               <div id=div style='display: flex; height: 10px'></div>
             </div>"
        ));
        rect_is(&p, "flex", 0.0, 0.0, 24.0, 24.0);
        rect_is(&p, "block", 0.0, 24.0, 50.0, 10.0);
        rect_is(&p, "wide", 0.0, 34.0, 300.0, 10.0);
        rect_is(&p, "div", 0.0, 44.0, 300.0, 10.0);
    }
    /// `text-overflow: ellipsis` keeps each prefix whose width, measured as the run
    /// is, plus the ellipsis's own width fits the line, as Blink's truncator does:
    /// "heads-" + "…" is 186.75 + 12.109375, a sixty-fourth over 198.84375, so the
    /// hyphen goes; and trailing spaces stay before the ellipsis. Widths are
    /// Chromium's for these pages.
    #[test]
    fn ellipsis_cuts_where_chromium_does() {
        let p = page(
            "<!DOCTYPE html><style>@font-face { font-family: Inter; src: url(/vendor/inter-regular.ttf) format('truetype') }
             body { margin: 0; font: 14px Inter } p { margin: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }</style>
             <p id=a style='width: 198.84375px'>Great, thanks for the heads-up!</p>
             <p id=b style='width: 199px'>Great, thanks for the heads-up!</p>
             <p id=c style='width: 34px'>ab  cd</p>
             <p id=d style='width: 28px'>ab  cd</p>",
        );
        let w = |id: &str| p.text_lines(id).last().unwrap().width;
        close(w("a"), 180.3125, "#a kept text");
        close(w("b"), 186.75, "#b kept text");
        close(w("c"), 20.375, "#c keeps the space");
        close(w("d"), 7.875, "#d");
    }
    /// The UA's centring of `<th>` is Blink's `-internal-center`: it applies only
    /// while the parent's `text-align` is the initial `start`, so header cells in a
    /// `text-left` table (app-music's track list) start at the left; an author's
    /// `center` still wins.
    #[test]
    fn a_header_cell_inherits_a_set_text_align() {
        let p = page(&format!(
            "{RESET}<table style='text-align: left; width: 300px'><tr><th id=a>A</th><th id=b style='text-align: center'>B</th></tr></table>
             <table style='width: 300px'><tr><th id=c>C</th></tr><tr style='text-align: right'><th id=d>D</th></tr></table>"
        ));
        assert_eq!(p.computed("a", "text-align"), "left");
        assert_eq!(p.computed("b", "text-align"), "center");
        assert_eq!(p.computed("c", "text-align"), "center");
        assert_eq!(p.computed("d", "text-align"), "right");
    }
    /// Spaces before a `<br>` do not widen an aligned line, even in a larger font
    /// than the content (hn-front's centred footer links), and an inline box that
    /// closes after its own trailing space ends at its content plus its padding.
    /// Positions are Chromium's.
    #[test]
    fn spaces_before_a_br_do_not_shift_an_aligned_line() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0 }</style>
             <div style='text-align: center; width: 200px; font: 20px/20px Arimo'><span id=s style='font-size: 10px'>x</span>
             <br>y</div>
             <div style='text-align: right; width: 200px; font: 20px/20px Arimo'><span id=t style='font-size: 10px; padding-right: 3px'>x </span> <br>y</div>",
        );
        close(p.rect("s").x, 97.5, "#s x");
        close(p.rect("s").width, 5.0, "#s width");
        close(p.rect("t").x, 192.0, "#t x");
        close(p.rect("t").width, 8.0, "#t width");
    }
    /// A text input's `size` columns are Blink's `PreferredContentLogicalWidth` on
    /// Linux Chromium's metrics: the average width goes up to a whole pixel when
    /// its fraction is a half or more, the bounding-box width rounds, and plain
    /// `monospace` is DejaVu Sans Mono. Widths are Chromium's for these inputs.
    #[test]
    fn text_input_columns_match_chromium() {
        let cases: &[(&str, &str, u32, f64)] = &[
            ("a", "9px Arial", 1, 17.0),
            ("b", "13px Arial", 20, 176.0),
            ("c", "14px Arial", 20, 181.0),
            ("d", "16px Arial", 20, 207.0),
            ("e", "20px Arial", 20, 265.0),
            ("f", "13px 'Times New Roman'", 20, 160.0),
            ("g", "11px 'DejaVu Sans'", 20, 145.0),
            ("h", "16px 'Courier New'", 20, 210.0),
            ("i", "16px monospace", 20, 210.0),
        ];
        let mut html = String::from(
            "<!DOCTYPE html><style>body { margin: 0 } input { display: block; padding: 0; border: 0 }</style>",
        );
        for (id, font, size, _) in cases {
            html.push_str(&format!(
                "<input id={id} size={size} style=\"font: {font}\">"
            ));
        }
        let p = page(&html);
        for (id, font, size, want) in cases {
            close(p.rect(id).width, *want, &format!("{font} size={size}"));
        }
    }
    /// What TodoMVC's stylesheet needed from the UA sheet and media queries, each
    /// value Chromium's: WebKit's prefixed `device-pixel-ratio` media features match
    /// (plain and in range syntax), form controls reset `font-weight` as the `font`
    /// shorthand in Chromium's UA sheet does, a checkbox's margins are `3px 3px 3px
    /// 4px`, an `h1` in a `section` keeps the plain `h1` margins (Chromium dropped the
    /// section rules), and disabled controls take Chromium's colours.
    #[test]
    fn todomvc_ua_and_media_details_match_chromium() {
        let p = page(
            "<!DOCTYPE html><style>body { margin: 0; font-weight: 300 }
             @media screen and (-webkit-min-device-pixel-ratio: 0) { #a { height: 40px } }
             @media screen and (-webkit-device-pixel-ratio>=0) { #b { height: 30px } }</style>
             <input type=checkbox id=a><input type=checkbox id=b>
             <section><h1 id=h style='font-size: 80px'>todos</h1></section>
             <input id=d disabled><button id=e disabled>x</button><select id=f disabled><option>o</option></select>",
        );
        close(p.rect("a").height, 40.0, "#a height");
        close(p.rect("b").height, 30.0, "#b height");
        assert_eq!(p.computed("a", "font-weight"), "400");
        assert_eq!(p.computed("a", "margin-bottom"), "3px");
        assert_eq!(p.computed("a", "margin-left"), "4px");
        let h1_margin: f64 = p
            .computed("h", "margin-top")
            .trim_end_matches("px")
            .parse()
            .unwrap();
        close(h1_margin, 53.6, "#h margin-top");
        assert_eq!(p.computed("d", "color"), "rgb(84, 84, 84)");
        assert_eq!(p.computed("e", "color"), "rgba(16, 16, 16, 0.3)");
        assert_eq!(p.computed("f", "color"), "rgb(128, 128, 128)");
    }
    /// Flex items whose content ends in a space inside an inline box and an empty
    /// inline box with a margin (JSON Server's header nav) share free space as in
    /// Chromium: trailing spaces are not part of an item's max-content width.
    /// Widths are Chromium's.
    #[test]
    fn trailing_spaces_in_inline_boxes_do_not_widen_flex_items() {
        let p = page("<!DOCTYPE html><style>body{margin:0;font-family:Arimo} ul{display:flex;justify-content:space-between;margin:0;padding:0;width:960px} li{flex-grow:1;text-align:right} li.t{flex-grow:5;font-weight:bold;font-size:22.4px;text-align:left} i{margin-right:.5rem} a{color:inherit;text-decoration:none}</style>\n<ul>\n            <li class=\"t\" id=a>\n              JSON Server\n            </li>\n            <li id=b>\n              <a href=\"x\">\n                <i class=\"fas fa-heart\"></i>GitHub Sponsors\n              </a>\n            </li>\n            <li id=c><a href=\"x\">GitHub Sponsors</a></li>\n            <li id=d><a href=\"x\"><i></i>GitHub Sponsors</a></li>\n</ul>\n");
        close(p.rect("a").width, 412.890625, "#a width");
        close(p.rect("b").width, 185.046875, "#b width");
        close(p.rect("c").width, 177.03125, "#c width");
        close(p.rect("d").width, 185.03125, "#d width");
    }
    /// `vertical-align: super` and `sub` shift by a third and a fifth of the parent's
    /// font size plus one pixel, as Blink does (JSON Server's resource counts).
    /// Offsets are Chromium's.
    #[test]
    fn super_and_sub_shift_as_in_blink() {
        for (fs, up, down) in [
            (16.0, 6.328125, 4.1875),
            (13.0, 5.328125, 3.59375),
            (20.0, 7.65625, 5.0),
        ] {
            let p = page(&format!(
                "<!DOCTYPE html><div style='font: {fs}px Arimo; line-height: 80px'><span id=r>x</span><span id=s style='vertical-align: super'>x</span><span id=b style='vertical-align: sub'>x</span></div>"
            ));
            let r = p.rect("r").y;
            close(r - p.rect("s").y, up, &format!("super at {fs}px"));
            close(p.rect("b").y - r, down, &format!("sub at {fs}px"));
        }
    }
    /// Conduit's sidebar and sign-in form: `white-space: nowrap` inline-blocks still
    /// wrap between each other (a break around an atomic inline follows its parent's
    /// `white-space`), and a `<fieldset>` contains its floats. Rects are Chromium's.
    #[test]
    fn nowrap_pills_wrap_and_a_fieldset_contains_floats() {
        let p = page("<!DOCTYPE html><style>body{margin:0;font:16px/20px Arimo} .p{display:inline-block;white-space:nowrap;width:60px;height:20px;margin-right:10px}</style><div style=\"width:150px\"><span class=p id=a></span><span class=p id=b></span><span class=p id=c></span></div><fieldset id=f style=\"margin:0;padding:0;border:0\"><div style=\"float:right;width:50px;height:40px\"></div><div id=g style=\"height:10px\"></div></fieldset>");
        rect_is(&p, "a", 0.0, 0.0, 60.0, 20.0);
        rect_is(&p, "b", 70.0, 0.0, 60.0, 20.0);
        rect_is(&p, "c", 0.0, 25.0, 60.0, 20.0);
        rect_is(&p, "f", 0.0, 50.0, 1280.0, 40.0);
    }
    /// A collapsible space before the line's first content is removed even after an
    /// empty inline box with a margin or padding, in layout and in max-content width
    /// (Conduit Vue's favourite button: an icon `<i>` with a margin, then text).
    /// Positions and widths are Chromium's.
    #[test]
    fn a_space_after_an_empty_inline_box_at_line_start_is_removed() {
        let p = page(
            "<!DOCTYPE html><body style='margin: 0; font: 16px Arimo'>
             <div><i style='margin-right: 8px'></i> <b id=a>X</b></div>
             <div><i style='padding-right: 8px'></i> <b id=b>X</b></div>
             <div><i></i> <b id=d>X</b></div>
             <div><i style='margin-left: 8px'></i> <b id=e>X</b></div>
             <div><i style='margin-right: 8px'>i</i> <b id=f>X</b></div>
             <button id=g style='font: 16px Arimo; padding: 0; border: 0'><i style='margin-right: 8px'></i> X</button>",
        );
        close(p.rect("a").x, 8.0, "#a x");
        close(p.rect("b").x, 8.0, "#b x");
        close(p.rect("d").x, 0.0, "#d x");
        close(p.rect("e").x, 8.0, "#e x");
        close(p.rect("f").x, 16.015625, "#f x");
        close(p.rect("g").width, 18.671875, "#g width");
    }
}
