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
        ("中文字符", FaceId::Han),
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
        ('中', FaceId::HanBold),
        ('한', FaceId::HangulBold),
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

macro_rules! pack {
    ($name:literal) => {
        include_bytes!(concat!("../../../render/assets/fonts/pack/", $name)) as &[u8]
    };
}
/// The complete pack files, for comparing against the stubs layout uses.
fn pack_bytes(face: FaceId) -> &'static [u8] {
    match face {
        FaceId::Han => pack!("noto-sans-sc.ttf"),
        FaceId::HanBold => pack!("noto-sans-sc-bold.ttf"),
        FaceId::Hangul => pack!("noto-sans-kr.ttf"),
        FaceId::HangulBold => pack!("noto-sans-kr-bold.ttf"),
        FaceId::Emoji => pack!("noto-emoji.ttf"),
        FaceId::HanTc => pack!("noto-sans-tc.ttf"),
        FaceId::HanTcBold => pack!("noto-sans-tc-bold.ttf"),
        FaceId::HanJp => pack!("noto-sans-jp.ttf"),
        FaceId::HanJpBold => pack!("noto-sans-jp-bold.ttf"),
        FaceId::HanKr => pack!("noto-sans-kr-han.ttf"),
        FaceId::HanKrBold => pack!("noto-sans-kr-han-bold.ttf"),
        FaceId::Gujarati => pack!("noto-gujarati.ttf"),
        FaceId::Ethiopic => pack!("noto-ethiopic.ttf"),
        FaceId::Myanmar => pack!("noto-myanmar.ttf"),
        FaceId::Sinhala => pack!("noto-sinhala.ttf"),
        FaceId::ColorEmoji => pack!("noto-color-emoji.ttf"),
        other => panic!("{other:?} is not a pack face"),
    }
}

#[test]
fn stubs_match_their_pack_faces() {
    // The stubs are what layout shapes; the pack is what the renderer draws. They
    // must agree on every glyph id and advance or layout and pixels would drift.
    // Bold pack faces are laid out with their regular twin's stub.
    for (i, face) in FaceId::ALL.into_iter().enumerate() {
        assert_eq!(face as usize, i);
        assert_eq!(face.in_pack(), FaceId::PACK.contains(&face), "{face:?}");
    }
    for face in FaceId::PACK {
        if face == FaceId::ColorEmoji {
            continue;
        }
        let full = rustybuzz::Face::from_slice(pack_bytes(face), 0).unwrap();
        let stub = face.shaper();
        assert_eq!(full.number_of_glyphs(), stub.number_of_glyphs(), "{face:?}");
        assert_eq!(full.units_per_em(), stub.units_per_em());
        for g in 0..full.number_of_glyphs() {
            let g = rustybuzz::ttf_parser::GlyphId(g);
            assert_eq!(
                full.glyph_hor_advance(g),
                stub.glyph_hor_advance(g),
                "{face:?}"
            );
        }
        for c in ['中', '한', '😀', 'あ', '骨', 'ક', 'ሀ', 'က', 'ක'] {
            assert_eq!(full.glyph_index(c), stub.glyph_index(c), "{face:?} {c}");
        }
    }
}

#[test]
fn italic_uses_the_italic_tables_and_symbols_stay_upright() {
    let italic = Style::new(false, true, Lang::Auto);
    for t in [
        Typeface::DejaVu,
        Typeface::Inter,
        Typeface::OpenSans,
        Typeface::Ubuntu,
        Typeface::Roboto,
    ] {
        // DejaVu Sans Oblique is its upright design slanted, advances and all; the
        // platform italics are drawn (and spaced) as italics.
        let (slanted, upright) = (
            text_width(t, italic, "Italic headline, affiliated", 16),
            text_width(t, false, "Italic headline, affiliated", 16),
        );
        assert_eq!(slanted == upright, t == Typeface::DejaVu, "{t:?}");
        assert_eq!(metrics::table_face(t, italic, 'a'), Some((t, true)));
        // Greek is outside the platform subsets: DejaVu's oblique draws it.
        assert_eq!(
            metrics::table_face(t, italic, 'λ'),
            Some((Typeface::DejaVu, true))
        );
        let bold_italic = Style::new(true, true, Lang::Auto);
        assert_eq!(metrics::table_face(t, bold_italic, 'Q'), Some((t, true)));
    }
    // Symbols the italic faces lack keep the upright face and its advance.
    let arrow = '⇒';
    assert!(!dejavu_oblique_covers(false, arrow));
    assert_eq!(
        metrics::advance(T, italic, arrow, 14),
        metrics::advance(T, false, arrow, 14)
    );
    // Italic text is still the table fast path, and wraps where it measures.
    assert!(is_simple(T, italic, "Italic text"));
    let lines = wrap(
        Typeface::Inter,
        italic,
        "an italic sentence that wraps",
        14,
        90,
    );
    assert!(lines.len() > 1);
    for line in &lines {
        assert!(text_width(Typeface::Inter, italic, line.trim_end(), 14) <= 90);
    }
    // Scripts without italic faces draw upright in italic runs.
    assert_eq!(script_face(italic, 'ש'), Some(FaceId::Hebrew));
    assert_eq!(Style::from(true), Style::new(true, false, Lang::Auto));
}

#[test]
fn han_takes_the_forms_of_its_language() {
    // 骨 (bone) and 次 (next) are drawn differently in each region.
    for c in ['骨', '次'] {
        let face = |lang| script_face(Style::new(false, false, lang), c).unwrap();
        assert_eq!(face(Lang::ZhHans), FaceId::Han, "{c}");
        assert_eq!(face(Lang::Auto), FaceId::Han, "{c}");
        assert_eq!(face(Lang::ZhHant), FaceId::HanTc, "{c}");
        assert_eq!(face(Lang::Ja), FaceId::HanJp, "{c}");
        assert_eq!(face(Lang::Ko), FaceId::HanKr, "{c}");
        let bold = |lang| script_face(Style::new(true, false, lang), c).unwrap();
        assert_eq!(bold(Lang::ZhHans), FaceId::HanBold);
        assert_eq!(bold(Lang::Ja), FaceId::HanJpBold);
    }
    // A character whose drawing is the same everywhere stays in the SC face.
    assert!(!FaceId::HanJp.covers('一'));
    assert_eq!(
        script_face(Style::new(false, false, Lang::Ja), '一'),
        Some(FaceId::Han)
    );
    // Language tags and the inference used for untagged text.
    assert_eq!(Lang::from_tag("zh-TW"), Lang::ZhHant);
    assert_eq!(Lang::from_tag("zh_Hant_HK"), Lang::ZhHant);
    assert_eq!(Lang::from_tag("zh-CN"), Lang::ZhHans);
    assert_eq!(Lang::from_tag("zh"), Lang::ZhHans);
    assert_eq!(Lang::from_tag("ja-JP"), Lang::Ja);
    assert_eq!(Lang::from_tag("ko"), Lang::Ko);
    assert_eq!(Lang::from_tag("en-US"), Lang::Auto);
    assert_eq!(infer_lang("骨の髄まで"), Lang::Ja);
    assert_eq!(infer_lang("骨頭 한국"), Lang::Ko);
    assert_eq!(infer_lang("繁體中文的骨"), Lang::ZhHant);
    assert_eq!(infer_lang("简体中文的骨"), Lang::ZhHans);
    // Untagged layout follows the inference: the same 骨 in a Japanese sentence and
    // in a Simplified one shapes to different faces; the width does not change.
    let face_of = |text: &str| {
        one_line(text)
            .glyphs
            .iter()
            .find(|g| matches!(g.face, Some(f) if f.is_han()))
            .and_then(|g| g.face)
    };
    assert_eq!(face_of("骨です"), Some(FaceId::HanJp));
    assert_eq!(face_of("骨头"), Some(FaceId::Han));
    let tagged = |lang| layout(T, Style::new(false, false, lang), "骨", 20, 1000);
    assert_eq!(tagged(Lang::Ja)[0].width, tagged(Lang::ZhHans)[0].width);
    assert_ne!(tagged(Lang::Ja)[0].glyphs, tagged(Lang::ZhHans)[0].glyphs);
}

#[test]
fn new_scripts_resolve_and_shape() {
    for (text, face) in [
        ("ქართული", FaceId::Georgian),
        ("Հայերեն", FaceId::Armenian),
        ("বাংলা", FaceId::Bengali),
        ("தமிழ்", FaceId::Tamil),
        ("ਪੰਜਾਬੀ", FaceId::Gurmukhi),
        ("ພາສາລາວ", FaceId::Lao),
        ("ភាសាខ្មែរ", FaceId::Khmer),
        ("ગુજરાતી", FaceId::Gujarati),
        ("ግዕዝ", FaceId::Ethiopic),
        ("မြန်မာ", FaceId::Myanmar),
        ("සිංහල", FaceId::Sinhala),
    ] {
        let line = one_line(text);
        assert!(!line.glyphs.is_empty(), "{text}");
        for g in &line.glyphs {
            assert_eq!(g.face, Some(face), "{text}");
            assert_ne!(g.glyph, GlyphRef::Index(0), "{text} shaped to .notdef");
        }
        let bold = layout(T, true, text, 20, 10_000);
        assert_eq!(bold[0].glyphs[0].face, Some(face.weight(true)), "{text}");
    }
    // Bengali: the i-sign is written before its consonant, a virama conjunct forms,
    // and the danda stays in the Bengali run.
    let ids = shaped(FaceId::Bengali, "কি");
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[1], gid(FaceId::Bengali, 'ক'));
    let conjunct = one_line("ক্ষ");
    assert!(conjunct.width < one_line("ক").width + one_line("ষ").width);
    assert!(one_line("বাংলা।")
        .glyphs
        .iter()
        .all(|g| g.face == Some(FaceId::Bengali)));
    // Tamil: the e-sign is reordered before its consonant.
    let ids = shaped(FaceId::Tamil, "கெ");
    assert_eq!(ids[1], gid(FaceId::Tamil, 'க'));
    // A Bengali conjunct is never split by an emergency break.
    let lines = wrap(T, false, "ক্ষক্ষক্ষ", 20, 12);
    assert!(
        lines.iter().all(|l| l.chars().count() % 3 == 0),
        "{lines:?}"
    );
}

#[test]
fn emoji_clusters_are_reported_with_their_span() {
    let line = one_line("a😀b👍🏽");
    assert_eq!(line.emoji.len(), 2);
    let a = metrics::advance(T, false, 'a', 20);
    assert_eq!(line.emoji[0].x0, a);
    assert_eq!(&line.text[line.emoji[0].text.clone()], "😀");
    assert_eq!(&line.text[line.emoji[1].text.clone()], "👍🏽");
    assert!(line.emoji[1].x0 > line.emoji[0].x1);
    assert_eq!(line.emoji[1].x1, line.width);
}

#[test]
fn terminal_cells_wrap_by_width_and_reorder_right_to_left_runs() {
    use terminal::{columns, layout_line, wrap as rows};
    // Wide characters take two cells, marks none; ASCII is one per char.
    assert_eq!(columns("abc"), 3);
    assert_eq!(columns("中文ab"), 6);
    assert_eq!(columns("e\u{0301}x"), 2);
    assert_eq!(columns("😀!"), 3);
    assert_eq!(columns("👨\u{200D}👩\u{200D}👧"), 2);
    let text = "日本語のテキスト";
    let wrapped: Vec<&str> = rows(text, 5).into_iter().map(|r| &text[r]).collect();
    assert_eq!(wrapped, ["日本", "語の", "テキ", "スト"]);
    // A wide character never straddles the edge; a lone one gets its own row.
    let wrapped: Vec<&str> = rows("a中b", 2).into_iter().map(|r| &"a中b"[r]).collect();
    assert_eq!(wrapped, ["a", "中", "b"]);
    // An LTR row with a Hebrew word: the word's letters run right to left in cells.
    let line = "ab אבג cd";
    let laid = layout_line(line, 13);
    let visual: String = laid.iter().map(|c| &line[c.text.clone()]).collect();
    assert_eq!(visual, "ab גבא cd");
    assert!(laid.iter().enumerate().all(|(i, c)| c.col == i as u32));
    // Mirroring is the renderer's: the clusters report their direction.
    let laid = layout_line("א(ב)", 13);
    assert!(laid.iter().all(|c| c.rtl));
    // Arabic is shaped in the row: "بب" takes initial and final forms, neither of
    // them the isolated letter, each in its own cell.
    let laid = layout_line("بب", 13);
    assert_eq!(laid.len(), 2);
    let isolated = GlyphRef::Index(gid(FaceId::Arabic, 'ب'));
    for c in &laid {
        assert_eq!(c.width, 1);
        assert_eq!(c.face, Some(FaceId::Arabic));
        assert!(c.glyphs.iter().all(|g| g.glyph != isolated), "{laid:?}");
    }
    // Lam-alef takes its mandatory contextual pair, one glyph per cell.
    let laid = layout_line("لا", 13);
    let plain = [gid(FaceId::Arabic, 'ل'), gid(FaceId::Arabic, 'ا')].map(GlyphRef::Index);
    assert_eq!(laid.iter().map(|c| c.width).sum::<u32>(), 2);
    assert!(laid
        .iter()
        .flat_map(|c| &c.glyphs)
        .all(|g| !plain.contains(&g.glyph)));
    // A combining mark stays in its base's cell; wide clusters take two.
    let laid = layout_line("e\u{0301}中😀", 13);
    let cols: Vec<(u32, u32)> = laid.iter().map(|c| (c.col, c.width)).collect();
    assert_eq!(cols, [(0, 1), (1, 2), (3, 2)]);
    assert_eq!(laid[2].face, Some(FaceId::Emoji));
    // Plain ASCII keeps the original grid and wrapping.
    assert!(terminal::is_simple("ls -la ~/src"));
    assert!(!terminal::is_simple("שלום"));
    assert_eq!(crate::wrap_text("abcde\n", 3), vec!["abc", "de", ""]);
}
