use super::*;
use crate::metrics::{ellipsize, text_width, wrap};

const T: Typeface = Typeface::DejaVu;

fn one_line(text: &str) -> LaidLine {
    let mut lines = layout(T, false, text, 20, 10_000);
    assert_eq!(lines.len(), 1, "{text:?}");
    lines.remove(0)
}
/// Glyph ids of `text` shaped alone in `face`, in visual order.
fn shaped(face: FaceId, text: &str) -> Vec<u16> {
    one_line(text)
        .glyphs
        .iter()
        .filter(|g| g.face == Some(face))
        .map(|g| match g.glyph {
            GlyphRef::Index(i) => i,
            GlyphRef::Char(_) => panic!("table glyph"),
        })
        .collect()
}
fn gid(face: FaceId, c: char) -> u16 {
    face.shaper().glyph_index(c).unwrap().0
}

#[test]
fn latin_takes_the_original_table_path() {
    for text in [
        "Settings λ 09:41",
        "Привет, мир",
        "a\u{0301} ⌘ ✓ ▒",
        "tab\there",
    ] {
        assert!(is_simple(T, false, text), "{text}");
        let line = one_line(text);
        let mut pen = 0;
        for (g, c) in line.glyphs.iter().zip(text.chars()) {
            assert_eq!(
                (g.face, g.glyph, g.x, g.y),
                (None, GlyphRef::Char(c), pen, 0)
            );
            pen += metrics::advance(T, false, c, 20);
        }
        assert_eq!(line.width, pen);
    }
    for text in [
        "שלום",
        "مرحبا",
        "สวัสดี",
        "नमस्ते",
        "中文",
        "한국어",
        "😀",
        "a\u{200D}b",
        "❤\u{FE0F}",
    ] {
        assert!(!is_simple(T, false, text), "{text}");
    }
}

#[test]
fn every_script_resolves_to_a_real_face() {
    for (text, face) in [
        ("שלום", FaceId::Hebrew),
        ("مرحبا", FaceId::Arabic),
        ("สวัสดี", FaceId::Thai),
        ("नमस्ते", FaceId::Devanagari),
        ("中文漢字日本語", FaceId::Han),
        ("ひらがなカタカナ", FaceId::Han),
        ("한국어", FaceId::Hangul),
        ("😀🎉", FaceId::Emoji),
    ] {
        let line = one_line(text);
        assert!(!line.glyphs.is_empty());
        for g in &line.glyphs {
            assert_eq!(g.face, Some(face), "{text}");
            assert_ne!(g.glyph, GlyphRef::Index(0), "{text} shaped to .notdef");
        }
    }
    for (c, face) in [
        ('ש', FaceId::HebrewBold),
        ('ب', FaceId::ArabicBold),
        ('中', FaceId::Han),
    ] {
        assert_eq!(script_face(true, c), Some(face));
    }
}

#[test]
fn hebrew_runs_right_to_left_and_mixed_runs_reorder() {
    // "שלום": the first logical letter is the rightmost glyph.
    let line = one_line("שלום");
    let ids: Vec<_> = line.glyphs.iter().map(|g| g.glyph).collect();
    let expected: Vec<_> = "שלום"
        .chars()
        .rev()
        .map(|c| GlyphRef::Index(gid(FaceId::Hebrew, c)))
        .collect();
    assert_eq!(ids, expected);
    assert!(line.glyphs.windows(2).all(|w| w[0].x < w[1].x));

    // LTR paragraph with an embedded RTL word: "ab אבג cd" draws "ab גבא cd".
    let line = one_line("ab אבג cd");
    let order: Vec<String> = line
        .glyphs
        .iter()
        .map(|g| match g.glyph {
            GlyphRef::Char(c) => c.to_string(),
            GlyphRef::Index(i) => {
                let c = "אבג"
                    .chars()
                    .find(|&c| gid(FaceId::Hebrew, c) == i)
                    .unwrap();
                c.to_string()
            }
        })
        .collect();
    assert_eq!(order.concat(), "ab גבא cd");

    // RTL paragraph (first strong character Hebrew): "אב cd!" draws "!cd בא",
    // with the neutral '!' taking the paragraph direction.
    let line = one_line("אב cd!");
    let order: String = line
        .glyphs
        .iter()
        .map(|g| match g.glyph {
            GlyphRef::Char(c) => c,
            GlyphRef::Index(i) => "אב".chars().find(|&c| gid(FaceId::Hebrew, c) == i).unwrap(),
        })
        .collect();
    assert_eq!(order, "!cd בא");

    // Brackets mirror in right-to-left runs.
    let line = one_line("א(ב)");
    let parens: String = line
        .glyphs
        .iter()
        .filter_map(|g| match g.glyph {
            GlyphRef::Char(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(parens, "()");
}

#[test]
fn arabic_letters_take_contextual_forms_and_lam_alef_ligates() {
    let beh = 'ب';
    let isolated = gid(FaceId::Arabic, beh);
    // Noto draws beh as a dotless body plus a dot mark. Three behs take final,
    // medial and initial bodies; none is the isolated one.
    let alone = shaped(FaceId::Arabic, "ب");
    let forms = shaped(FaceId::Arabic, "ببب");
    assert!(!forms.contains(&isolated), "{forms:?}");
    let bodies: std::collections::BTreeSet<_> =
        forms.iter().filter(|g| !alone.contains(g)).collect();
    assert_eq!(
        bodies.len(),
        3,
        "initial, medial and final forms are distinct: {forms:?} (alone {alone:?})"
    );
    // Lam + alef takes the mandatory lam-alef form (Noto draws it as a contextual
    // pair): neither the isolated letters nor the ordinary initial lam / final alef.
    let lam_alef = shaped(FaceId::Arabic, "لا");
    let ordinary_lam = shaped(FaceId::Arabic, "لب")[1];
    let ordinary_alef = shaped(FaceId::Arabic, "با")[0];
    for plain in [
        gid(FaceId::Arabic, 'ل'),
        gid(FaceId::Arabic, 'ا'),
        ordinary_lam,
        ordinary_alef,
    ] {
        assert!(!lam_alef.contains(&plain), "{lam_alef:?} contains {plain}");
    }
    // Joining is contextual, so the word is narrower than its isolated letters.
    let word = one_line("سلام").width;
    let apart: i64 = "سلام".chars().map(|c| one_line(&c.to_string()).width).sum();
    assert!(word < apart, "{word} >= {apart}");
}

#[test]
fn thai_and_devanagari_marks_attach_without_advancing() {
    // ก + sara ii + mai ek: both marks sit over the consonant, stacked.
    let base = one_line("ก");
    let line = one_line("กี่");
    assert_eq!(line.width, base.width, "marks must not advance");
    assert_eq!(line.glyphs.len(), 3);
    let (ii, ek) = (line.glyphs[1], line.glyphs[2]);
    assert!(
        ek.y > ii.y || ek.glyph != GlyphRef::Index(gid(FaceId::Thai, '\u{0E48}')),
        "mai ek is lifted above sara ii or replaced by its high form"
    );
    // Sara am decomposes: nikhahit over the consonant, sara aa after it.
    assert_eq!(shaped(FaceId::Thai, "กำ").len(), 3);

    // Devanagari short i is written before the consonant it follows.
    let ids = shaped(FaceId::Devanagari, "कि");
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[1], gid(FaceId::Devanagari, 'क'));
    assert_ne!(ids[0], ids[1]);
    // A virama conjunct forms: क्ष is not three separate glyphs side by side.
    let conjunct = one_line("क्ष");
    let apart = one_line("क").width + one_line("ष").width;
    assert!(conjunct.width < apart);
    // An anusvara is a zero-advance mark.
    assert_eq!(one_line("कं").width, one_line("क").width);
}

#[test]
fn cjk_wraps_between_ideographs_and_respects_kinsoku() {
    let text = "中文字符串测试，日本語のテキスト。한국어 문장입니다";
    for width in [60, 90, 130] {
        let lines = wrap(T, false, text, 16, width);
        assert!(lines.len() > 1, "{lines:?}");
        assert_eq!(lines.concat(), text);
        for line in &lines {
            assert!(
                text_width(T, false, line.trim_end(), 16) <= width,
                "{line} > {width}"
            );
            let first = line.chars().next().unwrap();
            assert!(
                !matches!(first, '，' | '。'),
                "line starts with closing punctuation: {lines:?}"
            );
        }
    }
    // Korean still prefers its spaces when a whole word fits.
    let lines = wrap(T, false, "한국어 문장", 16, 60);
    assert_eq!(lines, ["한국어 ", "문장"]);
    // Mixed Latin words are not split while CJK around them is.
    let lines = wrap(T, false, "中文English中文", 16, 80);
    assert!(lines.iter().any(|l| l.contains("English")), "{lines:?}");
}

#[test]
fn emoji_sequences_render_as_one_glyph() {
    for sequence in [
        "👍🏽",
        "👨\u{200D}👩\u{200D}👧",
        "🇯🇵",
        "1\u{FE0F}\u{20E3}",
        "❤\u{FE0F}",
        "🏳\u{FE0F}\u{200D}🌈",
        "🏴\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
    ] {
        let line = one_line(sequence);
        assert_eq!(line.glyphs.len(), 1, "{sequence:?}: {:?}", line.glyphs);
        assert_eq!(line.glyphs[0].face, Some(FaceId::Emoji));
    }
    // Without VS16, a heart DejaVu draws stays a text glyph; VS15 forces text.
    assert_eq!(one_line("❤").glyphs[0].face, None);
    assert_eq!(one_line("😀\u{FE0E}").glyphs[0].face, Some(FaceId::Emoji));
    // Emergency breaks never split a sequence.
    let family = "👨\u{200D}👩\u{200D}👧👨\u{200D}👩\u{200D}👧👨\u{200D}👩\u{200D}👧";
    let lines = wrap(T, false, family, 20, 30);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l == "👨\u{200D}👩\u{200D}👧"));
}

#[test]
fn measurement_agrees_with_placement() {
    for text in [
        "שלום עולם, hello",
        "السلام عليكم ورحمة الله",
        "ภาษาไทย ง่ายนิดเดียว",
        "नमस्ते दुनिया क्षत्रिय",
        "東京タワーと서울",
        "Party 🎉👨\u{200D}👩\u{200D}👧 time",
    ] {
        for width in [40, 100, 400] {
            let laid = layout(T, false, text, 15, width);
            let wrapped = wrap(T, false, text, 15, width);
            assert_eq!(
                laid.iter().map(|l| l.text.clone()).collect::<Vec<_>>(),
                wrapped
            );
            for line in &laid {
                let px = u32::try_from((line.width + 63) / 64).unwrap();
                assert_eq!(text_width(T, false, &line.text, 15), px, "{:?}", line.text);
                let end = line.glyphs.iter().map(|g| g.x).max().unwrap_or(0);
                assert!(end <= line.width, "glyph placed past the measured width");
            }
        }
        let short = ellipsize(T, false, text, 15, 70);
        assert!(short.ends_with('…'));
        assert!(text_width(T, false, &short, 15) <= 70, "{short}");
        assert_eq!(ellipsize(T, false, text, 15, 2000), text);
    }
}

#[test]
fn stubs_match_their_pack_faces() {
    // The stubs are what layout shapes; the pack is what the renderer draws. They
    // must agree on every glyph id and advance or layout and pixels would drift.
    let pack: [(FaceId, &[u8]); 3] = [
        (
            FaceId::Han,
            include_bytes!("../../../render/assets/fonts/pack/noto-sans-sc.ttf"),
        ),
        (
            FaceId::Hangul,
            include_bytes!("../../../render/assets/fonts/pack/noto-sans-kr.ttf"),
        ),
        (
            FaceId::Emoji,
            include_bytes!("../../../render/assets/fonts/pack/noto-emoji.ttf"),
        ),
    ];
    assert_eq!(pack.map(|p| p.0), FaceId::PACK);
    for (face, full) in pack {
        let full = rustybuzz::Face::from_slice(full, 0).unwrap();
        let stub = face.shaper();
        assert_eq!(full.number_of_glyphs(), stub.number_of_glyphs());
        assert_eq!(full.units_per_em(), stub.units_per_em());
        for g in 0..full.number_of_glyphs() {
            let g = rustybuzz::ttf_parser::GlyphId(g);
            assert_eq!(full.glyph_hor_advance(g), stub.glyph_hor_advance(g));
        }
        for c in ['中', '한', '😀', 'あ'] {
            assert_eq!(full.glyph_index(c), stub.glyph_index(c));
        }
    }
}
