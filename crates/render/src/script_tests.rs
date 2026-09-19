//! Rendering of the scripts beyond DejaVu: each draws real glyphs, pixels follow the
//! same layout the scene metrics measure, and one multi-script frame is pinned so the
//! Node smoke test can check Wasm draws the identical pixels once the pack is loaded.
use super::*;
use cw_scene::metrics::{text_width, wrap};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const SAMPLES: &[(&str, &str)] = &[
    ("hebrew", "שלום עולם"),
    ("arabic", "مرحبا بالعالم"),
    ("thai", "สวัสดีชาวโลก"),
    ("devanagari", "नमस्ते दुनिया"),
    ("simplified chinese", "简体中文测试"),
    ("traditional chinese", "繁體中文測試"),
    ("japanese", "ひらがなカタカナ漢字"),
    ("korean", "안녕하세요 세계"),
    ("emoji", "😀👍🏽🇯🇵👨\u{200D}👩\u{200D}👧"),
    ("georgian", "გამარჯობა მსოფლიო"),
    ("armenian", "Բարեւ աշխարհ"),
    ("bengali", "ওহে বিশ্ব ক্ষমা"),
    ("tamil", "வணக்கம் உலகம்"),
    ("gurmukhi", "ਸਤਿ ਸ੍ਰੀ ਅਕਾਲ"),
    ("lao", "ສະບາຍດີ ໂລກ"),
    ("khmer", "សួស្តី ពិភពលោក"),
    ("gujarati", "નમસ્તે દુનિયા"),
    ("ethiopic", "ሰላም ልዑል ዓለም"),
    ("myanmar", "မင်္ဂလာပါ ကမ္ဘာ"),
    ("sinhala", "ආයුබෝවන් ලෝකය"),
];
/// The multi-script golden scene, shared with `scripts/smoke-node.cjs`.
const SCENE: &str = include_str!("../tests/scripts-scene.json");
const SCENE_SHA256: &str = "a5c6132a7f8f967f99cb79df655841f8e57bf9393f370894fe509840c91e1f1f";

fn ink(alpha: &[u8]) -> u32 {
    alpha.iter().map(|a| u32::from(*a)).sum()
}
fn ink_columns(frame: &Frame) -> Option<(u32, u32)> {
    let inked: Vec<u32> = (0..frame.width)
        .filter(|&x| (0..frame.height).any(|y| frame.pixel(x, y).unwrap()[0] < 200))
        .collect();
    Some((*inked.first()?, *inked.last()?))
}
fn label(text: &str, width: u32, height: u32, size: u16) -> Frame {
    let mut scene = Scene::new(width, height);
    scene.nodes.push(Node::ui_text(
        1,
        Rect::new(0, 0, width, height),
        text,
        size,
        Color::BLACK,
    ));
    Renderer::new().render(&scene)
}

#[test]
fn every_script_draws_its_own_glyphs_not_boxes() {
    let mut renderer = Renderer::new();
    for bold in [false, true] {
        let ui = 1 + u8::from(bold);
        let notdef = renderer
            .glyph('\u{FFFF}', 24, ui, Typeface::DejaVu)
            .alpha
            .clone();
        assert!(ink(&notdef) > 0);
        for (name, text) in SAMPLES {
            let lines = shaping::layout(Typeface::DejaVu, bold, text, 24, 10_000);
            assert_eq!(lines.len(), 1);
            let mut distinct = BTreeSet::new();
            for placed in &lines[0].glyphs {
                let alpha = match (placed.face, placed.glyph) {
                    (Some(face), GlyphRef::Index(i)) => {
                        renderer.shaped_glyph(face, i, 24).unwrap().alpha.clone()
                    }
                    (None, GlyphRef::Char(' ')) => continue,
                    other => panic!("{name}: {other:?} fell back to the table face"),
                };
                assert_ne!(alpha, notdef, "{name} (bold {bold}) drew a .notdef box");
                // Some shapers emit invisible glyphs (Myanmar's kinzi and stacking
                // placeholders); every visible one must be real.
                if ink(&alpha) > 0 {
                    distinct.insert(alpha);
                }
            }
            // The colour face replaces the monochrome emoji glyphs when drawing,
            // but these are the glyphs layout placed, all real.
            assert!(
                distinct.len() >= 4,
                "{name}: only {} glyph shapes",
                distinct.len()
            );
        }
    }
    // Terminal text has no shaping, but still reaches the fallback faces.
    let mono_notdef = renderer
        .glyph('\u{FFFF}', 16, 0, Typeface::DejaVu)
        .alpha
        .clone();
    for c in ['中', 'ש', 'ب', 'ก', 'क', '한', '😀', 'ა', 'ক', 'ሀ'] {
        let glyph = renderer.glyph(c, 16, 0, Typeface::DejaVu);
        assert_ne!(glyph.alpha, mono_notdef, "terminal {c} is a box");
        assert!(ink(&glyph.alpha) > 0);
    }
}

#[test]
fn right_to_left_text_is_drawn_right_to_left() {
    // "אב" draws bet on the left: the left part of the word is pixel-identical to
    // bet drawn alone.
    let word = label("אב", 60, 30, 24);
    let bet = label("ב", 60, 30, 24);
    let (left, right) = ink_columns(&bet).unwrap();
    for x in left..=right {
        for y in 0..30 {
            assert_eq!(word.pixel(x, y), bet.pixel(x, y), "({x}, {y})");
        }
    }
    // ... and it is not alef that is drawn there.
    let alef = label("א", 60, 30, 24);
    assert!((left..=right).any(|x| (0..30).any(|y| word.pixel(x, y) != alef.pixel(x, y))));
}

#[test]
fn raster_stays_inside_the_measured_width_and_wraps_where_metrics_do() {
    for (name, text) in SAMPLES {
        let width = text_width(Typeface::DejaVu, false, text, 20);
        assert_eq!(
            wrap(Typeface::DejaVu, false, text, 20, width).len(),
            1,
            "{name} wraps at its own measured width"
        );
        let frame = label(text, width + 40, 40, 20);
        let (_, right) = ink_columns(&frame).unwrap();
        assert!(
            right <= width + 2,
            "{name}: ink reaches {right}, measured {width}"
        );
    }
    // A narrow CJK paragraph: the inked rows are exactly the wrapped lines.
    let text = "日本語の文章は単語の間に空白を置かないので文字の間で折り返します";
    let lines = wrap(Typeface::DejaVu, false, text, 16, 100);
    assert!(lines.len() >= 4, "{lines:?}");
    let frame = label(text, 100, 200, 16);
    let line_height = 16 + 16u32.div_ceil(4);
    for row in 0..8u32 {
        let inked = (row * line_height..(row + 1) * line_height)
            .any(|y| (0..100).any(|x| frame.pixel(x, y).unwrap()[0] < 128));
        assert_eq!(
            inked,
            (row as usize) < lines.len(),
            "row {row} of {lines:?}"
        );
    }
}

#[test]
fn a_missing_pack_draws_boxes_without_moving_layout() {
    let text = "中文 😀 한국어";
    let with_pack = label(text, 200, 30, 18);
    let width = text_width(Typeface::DejaVu, false, text, 18);
    font_pack::HIDE_PACK.with(|h| h.set(true));
    let without = label(text, 200, 30, 18);
    let status = font_pack_status();
    let layout_without = text_width(Typeface::DejaVu, false, text, 18);
    font_pack::HIDE_PACK.with(|h| h.set(false));
    assert_eq!(width, layout_without, "layout must not depend on the pack");
    assert_ne!(with_pack, without);
    assert_eq!(
        ink_columns(&with_pack).unwrap().0,
        ink_columns(&without).unwrap().0
    );
    assert!(status.missing.contains(&"noto-sans-sc.ttf"), "{status:?}");
    assert!(status.missing.contains(&"noto-emoji.ttf"), "{status:?}");
    // The embedded scripts never wait for the pack.
    font_pack::HIDE_PACK.with(|h| h.set(true));
    let hebrew = label("שלום", 100, 30, 18);
    font_pack::HIDE_PACK.with(|h| h.set(false));
    assert_eq!(hebrew, label("שלום", 100, 30, 18));
}

#[test]
fn multi_script_scene_is_pinned() {
    let scene: Scene = serde_json::from_str(SCENE).unwrap();
    let frame = Renderer::new().render(&scene);
    assert_eq!(frame, Renderer::new().render(&scene));
    let digest = format!("{:x}", Sha256::digest(&frame.rgba));
    assert_eq!(digest, SCENE_SHA256);
}
