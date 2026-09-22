//! Glyph legibility guarantees for OCR consumers of rendered terminal frames.
//! Each assertion is the measurable form of an ambiguity reported against the
//! unfitted rasterizer; see crates/graphics/render/assets/README.md for what is still ambiguous.
use cw_render::{Frame, Renderer};
use cw_scene::{text_cell, Color, Node, Rect, Scene};

/// Terminal text, black on white, laid out on the fixed-pitch cell grid.
fn render(text: &str, size: u16) -> Frame {
    let (cell, line) = text_cell(size);
    let (w, h) = (cell * (text.chars().count() as u32 + 2), line * 3);
    let mut scene = Scene::new(w, h);
    scene
        .nodes
        .push(Node::rectangle(1, Rect::new(0, 0, w, h), Color::WHITE));
    scene.nodes.push(Node::text(
        2,
        Rect::new(0, 0, w, h),
        text,
        size,
        Color::BLACK,
    ));
    Renderer::new().render(&scene)
}

/// Coverage of the glyph at this pixel: 0 is bare paper, 255 is solid ink.
fn ink(frame: &Frame, x: u32, y: u32) -> u8 {
    255 - frame.pixel(x, y).expect("inside frame")[0]
}

/// Peak ink per row over a column range.
fn rows(frame: &Frame, x0: u32, x1: u32) -> Vec<u8> {
    (0..frame.height)
        .map(|y| (x0..x1).map(|x| ink(frame, x, y)).max().unwrap_or(0))
        .collect()
}

/// One cell of terminal text as a 2:1 box-filtered grid, the way a consumer that
/// downscales frames before OCR sees it.
fn halve(frame: &Frame, cell: u32) -> Vec<u8> {
    let (w, h) = (cell.div_ceil(2), frame.height.div_ceil(2));
    let mut out = vec![0u8; (w * h) as usize];
    for y in 0..frame.height {
        for x in 0..cell {
            let i = ((y / 2) * w + x / 2) as usize;
            out[i] = out[i].saturating_add(ink(frame, x, y) / 4);
        }
    }
    out
}

const TERMINAL_SIZES: std::ops::RangeInclusive<u16> = 10..=20;

#[test]
fn hyphen_is_one_solid_pixel_row_at_every_terminal_size() {
    // `atlas-sync-31288` read as `atlas sync-31288`: the bar straddled two rows at
    // 45% coverage each and vanished. Fitted, it is a single opaque row.
    let text = "atlas-sync-31288";
    for size in TERMINAL_SIZES {
        let cell = text_cell(size).0;
        let frame = render(text, size);
        for column in text
            .char_indices()
            .filter(|(_, c)| *c == '-')
            .map(|(i, _)| i as u32)
        {
            let bar = rows(&frame, column * cell, (column + 1) * cell);
            let lit: Vec<u8> = bar.iter().copied().filter(|v| *v >= 64).collect();
            assert_eq!(
                lit.len(),
                1,
                "size {size}: hyphen at column {column} spans {} rows, profile {bar:?}",
                lit.len()
            );
            assert!(
                lit[0] >= 200,
                "size {size}: hyphen at column {column} peaks at {} of 255",
                lit[0]
            );
        }
    }
}

#[test]
fn equals_sign_keeps_two_separated_bars() {
    // The same snapping must not merge `=` into one bar or it becomes another `-`.
    for size in TERMINAL_SIZES {
        let frame = render("=", size);
        let bar = rows(&frame, 0, text_cell(size).0);
        let lit: Vec<usize> = (0..bar.len()).filter(|&y| bar[y] >= 64).collect();
        assert_eq!(lit.len(), 2, "size {size}: `=` profile {bar:?}");
        assert!(lit[1] - lit[0] >= 2, "size {size}: `=` bars touch, {bar:?}");
        assert!(
            bar[lit[0]] >= 200 && bar[lit[1]] >= 200,
            "size {size}: {bar:?}"
        );
    }
}

#[test]
fn tilde_wave_has_amplitude_that_survives_downsampling() {
    // A two-row tilde downsamples to a smear that OCR reads as `"` or `#`.
    for size in TERMINAL_SIZES {
        let cell = text_cell(size).0;
        let frame = render("~", size);
        let wave = rows(&frame, 0, cell);
        let strong = wave.iter().filter(|v| **v >= 64).count();
        let extent = wave.iter().filter(|v| **v >= 16).count();
        assert!(
            strong >= 3,
            "size {size}: tilde has {strong} strong rows, {wave:?}"
        );
        assert!(
            extent >= 4,
            "size {size}: tilde spans {extent} rows, {wave:?}"
        );
        // The advance is tabulated, so a taller tilde must still sit in one cell.
        let spill: u8 = (cell..frame.width)
            .flat_map(|x| (0..frame.height).map(move |y| (x, y)))
            .map(|(x, y)| ink(&frame, x, y))
            .max()
            .unwrap_or(0);
        assert_eq!(spill, 0, "size {size}: tilde painted past its cell");
    }
}

#[test]
fn tilde_stays_distinct_after_a_halving_downsample() {
    // Consumers OCR downscaled frames; a two-row wave averages into one flat bar and
    // reads as `"` or `#`. Compare the glyphs the way a 2:1 box filter sees them.
    for size in TERMINAL_SIZES {
        let cell = text_cell(size).0;
        let tilde = halve(&render("~", size), cell);
        for other in ["\"", "#", "-"] {
            let rival = halve(&render(other, size), cell);
            let differing = (0..tilde.len())
                .filter(|&i| tilde[i].abs_diff(rival[i]) > 16)
                .count();
            assert!(
                differing >= 4,
                "size {size}: `~` and `{other}` differ in {differing} cells once halved"
            );
        }
    }
}

#[test]
fn dotted_zero_carries_a_full_ink_mark_that_o_and_eight_lack() {
    // `0` is the letter bowl plus a centre dot; the dot is the whole signal and it
    // rasterized mid-grey. Fitted, it is the one place the zero out-inks a capital O.
    // At 10 px and below the dot merges into the bowl outright; see assets/README.md.
    for size in 11..=*TERMINAL_SIZES.end() {
        let cell = text_cell(size).0;
        let zero = render("0", size);
        let letter = render("O", size);
        let mark = (0..cell)
            .flat_map(|x| (0..zero.height).map(move |y| (x, y)))
            .map(|(x, y)| ink(&zero, x, y).saturating_sub(ink(&letter, x, y)))
            .max()
            .unwrap_or(0);
        assert!(
            mark >= 250,
            "size {size}: zero's dot peaks at {mark} over `O`"
        );
    }
    // And the pair a reader actually confuses stays far apart at the terminal size.
    let (zero, eight) = (render("0", 13), render("8", 13));
    let cell = text_cell(13).0;
    let differing = (0..cell)
        .flat_map(|x| (0..zero.height).map(move |y| (x, y)))
        .filter(|&(x, y)| ink(&zero, x, y).abs_diff(ink(&eight, x, y)) > 16)
        .count();
    assert!(
        differing >= 45,
        "`0` and `8` differ in only {differing} pixels at 13"
    );
}

#[test]
fn fitting_is_bit_reproducible() {
    // Fitting is integer-only; two independent renderers must agree byte for byte.
    let text = "atlas-sync-31288 ~/=_0O8";
    for size in TERMINAL_SIZES {
        assert_eq!(
            render(text, size).rgba,
            render(text, size).rgba,
            "size {size}"
        );
    }
    // A warm cache must not change what a cold one produced.
    let mut warm = Renderer::new();
    let (cell, line) = text_cell(13);
    let (w, h) = (cell * 26, line * 3);
    let mut scene = Scene::new(w, h);
    scene
        .nodes
        .push(Node::rectangle(1, Rect::new(0, 0, w, h), Color::WHITE));
    scene
        .nodes
        .push(Node::text(2, Rect::new(0, 0, w, h), text, 13, Color::BLACK));
    let first = warm.render(&scene).rgba;
    scene.nodes[1] = Node::text(2, Rect::new(0, 0, w, h), text, 13, Color::BLACK);
    assert_eq!(warm.render(&scene).rgba, first);
}
