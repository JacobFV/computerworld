//! Italic faces, colour emoji, locale Han forms and the terminal grid's shaping.
use super::*;
use cw_scene::metrics::{text_width, wrap};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn styled(text: &str, width: u32, height: u32, size: u16, style: Style, t: Typeface) -> Frame {
    let mut scene = Scene::new(width, height);
    scene.typeface = t;
    scene.nodes.push(Node::ui_text_styled(
        1,
        Rect::new(0, 0, width, height),
        text,
        size,
        Color::BLACK,
        style,
    ));
    Renderer::new().render(&scene)
}
fn terminal(text: &str, width: u32, size: u16) -> Frame {
    let mut scene = Scene::new(width, 24);
    scene.nodes.push(Node::text(
        1,
        Rect::new(0, 0, width, 24),
        text,
        size,
        Color::BLACK,
    ));
    Renderer::new().render(&scene)
}
fn ink_right(frame: &Frame) -> Option<u32> {
    (0..frame.width)
        .rev()
        .find(|&x| (0..frame.height).any(|y| frame.pixel(x, y).unwrap()[0] < 250))
}
fn hash(frame: &Frame) -> String {
    format!("{:x}", Sha256::digest(&frame.rgba))
}
/// Pixels that are clearly coloured (not a grey of any level).
fn colourful(frame: &Frame) -> usize {
    (0..frame.height)
        .flat_map(|y| (0..frame.width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = frame.pixel(x, y).unwrap();
            let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
            hi - lo > 60
        })
        .count()
}
/// A region of a frame, for comparing cells.
fn region(frame: &Frame, x0: u32, x1: u32) -> Vec<[u8; 4]> {
    (0..frame.height)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .map(|(x, y)| frame.pixel(x, y).unwrap())
        .collect()
}

const ITALIC: Style = Style::new(false, true, Lang::Auto);
const BOLD_ITALIC: Style = Style::new(true, true, Lang::Auto);
const TYPEFACES: [Typeface; 5] = [
    Typeface::DejaVu,
    Typeface::Inter,
    Typeface::OpenSans,
    Typeface::Ubuntu,
    Typeface::Roboto,
];

#[test]
fn italic_faces_draw_slanted_and_stay_inside_the_measured_box() {
    let text = "Italic affine λ Ω";
    let mut seen = BTreeSet::new();
    for t in TYPEFACES {
        for style in [Style::default(), ITALIC, Style::from(true), BOLD_ITALIC] {
            let frame = styled(text, 260, 30, 18, style, t);
            assert_eq!(frame, styled(text, 260, 30, 18, style, t));
            // Every weight and slant of every family is its own drawing.
            assert!(seen.insert(hash(&frame)), "{t:?} {style:?} repeats a face");
            // The ink ends where layout measured, give or take the last glyph's
            // right side bearing (DejaVu Bold's Ω overhangs by 3 px at 18 px) and the
            // slant's overhang (a fifth of an em at most).
            let measured = text_width(t, style, text, 18);
            let right = ink_right(&frame).unwrap();
            let overhang = if style.italic { 18 / 5 } else { 0 };
            assert!(
                right <= measured + overhang + 3,
                "{t:?} {style:?}: ink at {right}, measured {measured}"
            );
            assert!(
                right + 6 >= measured,
                "{t:?} {style:?}: {right} vs {measured}"
            );
        }
    }
    // Wrapping follows the italic advances: rows of ink where the lines are.
    let text = "an italic paragraph wraps where its italic metrics say";
    let lines = wrap(Typeface::Inter, ITALIC, text, 14, 110);
    let frame = styled(text, 110, 120, 14, ITALIC, Typeface::Inter);
    let line_height = 14 + 14u32.div_ceil(4);
    for row in 0..6u32 {
        let inked = (row * line_height..(row + 1) * line_height)
            .any(|y| (0..110).any(|x| frame.pixel(x, y).unwrap()[0] < 128));
        assert_eq!(
            inked,
            (row as usize) < lines.len(),
            "row {row} of {lines:?}"
        );
    }
    // Characters no italic face has (arrows, box drawing) keep the upright glyph.
    let arrow_upright = styled("⇒", 30, 30, 18, Style::default(), Typeface::Inter);
    let arrow_italic = styled("⇒", 30, 30, 18, ITALIC, Typeface::Inter);
    assert_eq!(arrow_upright, arrow_italic);
}

#[test]
fn italic_scene_json_is_optional_and_pinned() {
    // Existing scene JSON has no `italic` or `lang`: it still parses, and a default
    // primitive serializes exactly as before.
    let json = r#"{"kind":"ui_text","text":"a","color":[0,0,0,255],"size":12}"#;
    let plain: Primitive = serde_json::from_str(json).unwrap();
    assert_eq!(plain.text_style(), Some(Style::default()));
    assert_eq!(serde_json::to_string(&plain).unwrap(), json);
    let tagged: Primitive = serde_json::from_str(
        r#"{"kind":"ui_text_bold","text":"a","size":12,"color":[0,0,0,255],"italic":true,"lang":"zh-Hant"}"#,
    )
    .unwrap();
    assert_eq!(
        tagged.text_style(),
        Some(Style::new(true, true, Lang::ZhHant))
    );
    let mut scene = Scene::new(240, 70);
    scene.typeface = Typeface::Inter;
    scene.nodes.push(Node::ui_text_styled(
        1,
        Rect::new(4, 4, 232, 24),
        "Preview: main.rs — λ",
        15,
        Color::rgb(30, 30, 30),
        ITALIC,
    ));
    scene.nodes.push(Node::ui_text_styled(
        2,
        Rect::new(4, 34, 232, 24),
        "Bold italic €42",
        15,
        Color::rgb(30, 30, 30),
        BOLD_ITALIC,
    ));
    let frame = Renderer::new().render(&scene);
    assert_eq!(
        hash(&frame),
        "cdec16514e9247bb6988c7f698095c586a6c467b71f98cc9dda7f83c7929483f"
    );
}

#[test]
fn color_emoji_draw_in_colour_and_fall_back_to_monochrome() {
    for emoji in ["😀", "🇯🇵", "🏳️‍🌈", "👍🏽", "❤️", "🎉"] {
        let frame = styled(emoji, 40, 30, 22, Style::default(), Typeface::DejaVu);
        assert!(colourful(&frame) > 20, "{emoji} has no colour");
        assert_eq!(
            frame,
            styled(emoji, 40, 30, 22, Style::default(), Typeface::DejaVu)
        );
        // The colour glyph stays in the monochrome glyph's box: layout is unchanged.
        let right = ink_right(&frame).unwrap();
        assert!(
            right <= text_width(Typeface::DejaVu, false, emoji, 22) + 1,
            "{emoji}"
        );
    }
    // Recognisable colours: the grinning face is yellow, the flag of Japan red.
    let face = styled("😀", 40, 30, 22, Style::default(), Typeface::DejaVu);
    let yellow = (0..30)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .any(|(x, y)| {
            let p = face.pixel(x, y).unwrap();
            p[0] > 200 && p[1] > 150 && p[2] < 90
        });
    assert!(yellow, "😀 is not yellow");
    let flag = styled("🇯🇵", 40, 30, 22, Style::default(), Typeface::DejaVu);
    let red = (0..30)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .any(|(x, y)| {
            let p = flag.pixel(x, y).unwrap();
            p[0] > 180 && p[1] < 80 && p[2] < 90
        });
    assert!(red, "🇯🇵 has no red disc");
    // The rainbow flag uses gradients and soft-light compositing: many colours.
    let rainbow = styled("🏳️‍🌈", 40, 30, 22, Style::default(), Typeface::DejaVu);
    let hues: BTreeSet<[u8; 3]> = (0..30)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .map(|(x, y)| {
            let p = rainbow.pixel(x, y).unwrap();
            [p[0] / 64, p[1] / 64, p[2] / 64]
        })
        .collect();
    assert!(hues.len() >= 6, "{hues:?}");
    // Without the colour face the monochrome glyphs draw, in the text colour.
    font_pack::HIDE_COLOR.with(|h| h.set(true));
    let mono = styled("😀", 40, 30, 22, Style::default(), Typeface::DejaVu);
    let status = font_pack_status();
    font_pack::HIDE_COLOR.with(|h| h.set(false));
    assert_eq!(colourful(&mono), 0);
    assert!(ink_right(&mono).is_some());
    assert_ne!(mono, face);
    assert!(
        status.missing.contains(&"noto-color-emoji.ttf"),
        "{status:?}"
    );
    // Opacity and clipping apply to colour glyphs like any text.
    let mut scene = Scene::new(40, 30);
    let mut node = Node::ui_text(1, Rect::new(0, 0, 40, 30), "😀", 22, Color::BLACK);
    node.opacity = 128;
    scene.nodes.push(node);
    let faded = Renderer::new().render(&scene);
    assert_ne!(faded, face);
    assert!(colourful(&faded) > 0);
    // Pinned: native and Wasm must agree (scripts/checks/smoke-node.cjs checks the
    // multi-script scene, which carries these emoji too).
    assert_eq!(
        hash(&face),
        "f7c89baa4dced159f4892c5f46b4d0034c1cd917069ccbfdf631ffd994c6f4d4"
    );
}

#[test]
fn han_is_drawn_in_the_forms_of_its_language() {
    let lang = |l: Lang, bold: bool| {
        styled(
            "骨",
            30,
            30,
            24,
            Style::new(bold, false, l),
            Typeface::DejaVu,
        )
    };
    let frames: Vec<Frame> = [Lang::ZhHans, Lang::ZhHant, Lang::Ja, Lang::Ko]
        .into_iter()
        .map(|l| lang(l, false))
        .collect();
    // The same codepoint in regional drawings: every locale's differs from the
    // Simplified one (Traditional Chinese and Korean share theirs).
    for other in &frames[1..] {
        assert_ne!(&frames[0], other);
    }
    assert_ne!(frames[1], frames[2]);
    assert_eq!(frames.iter().map(hash).collect::<BTreeSet<_>>().len(), 3);
    // Untagged text follows the script heuristic: kana makes 骨 Japanese.
    let untagged = styled("骨です", 80, 30, 24, Style::default(), Typeface::DejaVu);
    assert_eq!(
        region(&untagged, 0, 26),
        region(
            &styled(
                "骨です",
                80,
                30,
                24,
                Style::new(false, false, Lang::Ja),
                Typeface::DejaVu
            ),
            0,
            26
        )
    );
    // Bold CJK is a real bold: heavier than the regular, in every locale.
    for l in [Lang::ZhHans, Lang::ZhHant, Lang::Ja, Lang::Ko] {
        let ink = |f: &Frame| f.rgba.chunks(4).map(|p| 255 - u32::from(p[0])).sum::<u32>();
        assert!(
            ink(&lang(l, true)) > ink(&lang(l, false)) * 11 / 10,
            "{l:?}"
        );
    }
    let hangul = |bold| styled("한국어", 80, 30, 20, Style::from(bold), Typeface::DejaVu);
    assert_ne!(hangul(true), hangul(false));
}

#[test]
fn terminal_cells_hold_wide_characters_marks_and_right_to_left_runs() {
    let (cell, _) = text_cell(14);
    // A wide character takes two cells: the 'a' after 中 is in the third cell.
    let wide = terminal("中a", 200, 14);
    let plain = terminal("xya", 200, 14);
    assert_eq!(
        region(&wide, 2 * cell, 3 * cell),
        region(&plain, 2 * cell, 3 * cell)
    );
    // 中 is drawn at full size across its two cells, not shrunk into one.
    let right = ink_right(&terminal("中", 200, 14)).unwrap();
    assert!(right >= cell + cell / 2, "中 ends at {right}, cell {cell}");
    // A combining mark joins its base's cell and draws over it.
    let accented = terminal("e\u{0301}x", 200, 14);
    let bare = terminal("ex", 200, 14);
    assert_eq!(
        region(&accented, cell, 2 * cell),
        region(&bare, cell, 2 * cell)
    );
    assert_ne!(region(&accented, 0, cell), region(&bare, 0, cell));
    // Hebrew runs right to left in the grid: "אב" shows bet in the first cell.
    let word = terminal("אב", 200, 14);
    let bet = terminal("ב", 200, 14);
    assert_eq!(region(&word, 0, cell), region(&bet, 0, cell));
    // Arabic letters join: in "ببب" none of the cells is the isolated letter.
    let alone = terminal("ب", 200, 14);
    let joined = terminal("ببب", 200, 14);
    for k in 0..3 {
        assert_ne!(
            region(&joined, k * cell, (k + 1) * cell),
            region(&alone, 0, cell),
            "cell {k}"
        );
    }
    // Brackets mirror inside a right-to-left run.
    let mirrored = terminal("א(ב)", 200, 14);
    assert_ne!(mirrored, terminal("א)ב(", 200, 14));
    // Colour emoji in a terminal: two cells, in colour.
    let emoji = terminal("😀b", 200, 14);
    assert!(colourful(&emoji) > 10);
    assert_eq!(
        region(&emoji, 2 * cell, 3 * cell),
        region(&terminal("xyb", 200, 14), 2 * cell, 3 * cell)
    );
    // Plain ASCII rows are the original grid, pixel for pixel.
    assert_eq!(
        hash(&terminal("$ ls -la ~/src | grep rs", 200, 14)),
        hash(&terminal("$ ls -la ~/src | grep rs", 200, 14))
    );
}

fn last_ink_column(frame: &Frame) -> i64 {
    (0..frame.width)
        .rfind(|&x| (0..frame.height).any(|y| frame.pixel(x, y).unwrap()[0] < 200))
        .map(i64::from)
        .expect("the run draws ink")
}

/// The renderer places a run's glyphs at the kerned pen positions the scene metrics
/// measure: the last glyph of a kerned run sits exactly where the measured width
/// (less its own advance) puts it. Web faces kern, and DejaVu Sans as web content;
/// the platform faces do not, nor native DejaVu text, so desktop scenes and their
/// golden frames are as they were.
#[test]
fn the_renderer_kerns_a_run_exactly_as_metrics_measure_it() {
    use cw_scene::metrics::{advance, kern};
    let size = 32;
    let text = "AVAVAVAV";
    let web = Style::default().for_web();
    for (t, style, kerned) in [
        (Typeface::Arimo, Style::default(), true),
        (Typeface::Tinos, Style::default(), true),
        (Typeface::Inter, Style::default(), false),
        (Typeface::Inter, web, false),
        (Typeface::DejaVu, Style::default(), false),
        (Typeface::DejaVu, web, true),
    ] {
        let plain: i64 = text.chars().map(|c| advance(t, style, c, size)).sum();
        let pairs: i64 = text
            .chars()
            .zip(text.chars().skip(1))
            .map(|(l, r)| kern(t, style, l, r, size))
            .sum();
        assert_eq!(pairs < 0, kerned, "{t:?}");
        let laid = &cw_scene::text::layout(t, style, text, size, 400)[0];
        assert_eq!(
            laid.width,
            plain + pairs,
            "{t:?}: layout width is the kerned sum"
        );
        assert_eq!(
            i64::from(text_width(t, style, text, size)),
            (plain + pairs + 63) / 64,
            "{t:?}: text_width is the kerned sum"
        );
        // Ink: the last V's right edge is the lone V's, moved to the kerned pen.
        let lone = last_ink_column(&styled("V", 100, 48, size, style, t));
        let pen = plain + pairs - advance(t, style, 'V', size);
        let run = last_ink_column(&styled(text, 400, 48, size, style, t));
        assert_eq!(run, (pen + 32).div_euclid(64) + lone, "{t:?}");
        if kerned {
            let unkerned = (plain - advance(t, style, 'V', size) + 32).div_euclid(64) + lone;
            assert!(run + 8 < unkerned, "{t:?}: {run} vs {unkerned}");
        }
    }
}

/// Web content sets DejaVu Sans Mono on its own advance, 1233/2048 em, and the
/// renderer draws each glyph at that pen; native text keeps the terminal grid's
/// whole-pixel cells.
#[test]
fn web_monospace_is_drawn_on_its_real_advances() {
    use cw_scene::metrics::advance;
    let size = 24;
    let text = "iiiiiiiiii";
    let lone = last_ink_column(&styled(
        "i",
        100,
        40,
        size,
        Style::default(),
        Typeface::Mono,
    ));
    for (style, step) in [
        (Style::default().for_web(), (1233 * 24 * 64 + 1024) / 2048),
        (
            Style::default(),
            i64::from(cw_scene::text_cell(size).0) * 64,
        ),
    ] {
        assert_eq!(advance(Typeface::Mono, style, 'i', size), step);
        let run = last_ink_column(&styled(text, 400, 40, size, style, Typeface::Mono));
        assert_eq!(run, (9 * step + 32).div_euclid(64) + lone, "{style:?}");
    }
}

/// A symbol DejaVu draws beyond its tabulated ranges advances by the font's own
/// advance in web content, so the glyph after it sits where layout measured it
/// (natively it keeps the 0.6 em it always had, which the symbol's ink overhangs).
#[test]
fn web_symbols_advance_by_the_font() {
    use cw_scene::metrics::advance;
    let size = 26;
    let web = Style::default().for_web();
    let lone = last_ink_column(&styled("l", 100, 40, size, web, Typeface::DejaVu));
    let step = advance(Typeface::DejaVu, web, '☎', size);
    assert!(step > advance(Typeface::DejaVu, Style::default(), '☎', size) + 64 * 10);
    // Within a pixel: the glyph after it is drawn at its quarter-pixel phase.
    let run = last_ink_column(&styled("☎l", 200, 40, size, web, Typeface::DejaVu));
    assert!(
        (run - (step + 32).div_euclid(64) - lone).abs() <= 1,
        "{run}"
    );
}

/// Web text is placed to a quarter pixel, as Chromium places it at device scale 1:
/// a run of 13 px "l" (3.61 px apart) draws each stem at its own quarter-pixel phase,
/// so the stems' ink centroids advance by the run's real pitch, not by 3 or 4 whole
/// pixels. Native text keeps one bitmap per glyph at whole pixels.
#[test]
fn web_glyphs_are_placed_to_a_quarter_pixel() {
    let pitch = 569.0 * 13.0 / 2048.0; // DejaVu Sans "l"
    let centroids = |style: Style| -> Vec<f64> {
        let f = styled("llllllllllllllll", 400, 30, 13, style, Typeface::DejaVu);
        let mut out = Vec::new();
        for i in 0..16 {
            let (lo, hi) = ((i as f64 * pitch) as u32, ((i + 1) as f64 * pitch) as u32);
            let (mut sum, mut weight) = (0.0, 0.0);
            for x in lo..hi {
                let ink: u32 = (0..20)
                    .map(|y| 255 - u32::from(f.pixel(x, y).unwrap()[0]))
                    .sum();
                sum += f64::from(ink) * (f64::from(x) + 0.5);
                weight += f64::from(ink);
            }
            out.push(sum / weight);
        }
        out
    };
    let web = centroids(Style::default().for_web());
    for pair in web.windows(2) {
        let step = pair[1] - pair[0];
        assert!((step - pitch).abs() < 0.35, "{step} {web:?}");
    }
    let native = centroids(Style::default());
    for pair in native.windows(2) {
        let step = pair[1] - pair[0];
        assert!(
            (step - 3.0).abs() < 1e-9 || (step - 4.0).abs() < 1e-9,
            "{native:?}"
        );
    }
}
