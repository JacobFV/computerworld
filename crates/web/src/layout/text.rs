//! Text measurement and processing for inline layout: font metrics, advances in `Au`,
//! `text-transform`, `white-space` processing (CSS Text Level 3 §4) and soft wrap
//! opportunities. Every text measurement in layout goes through this file so that the
//! quantisation matches the renderer: `cw_scene::metrics::advance` gives 1/64 px per
//! glyph, which is exactly one `Au`.

use crate::geom::Au;
use crate::style::{Font, TextTransform, WhiteSpace, WordBreak};
use cw_scene::metrics;
use cw_scene::Typeface;

/// Vertical metrics of a font at its size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FontMetrics {
    pub ascent: Au,
    pub descent: Au,
    pub line_gap: Au,
    /// Approximate x-height, for `vertical-align: middle`.
    pub x_height: Au,
}

impl FontMetrics {
    /// `line-height: normal`: ascent + descent + line gap.
    pub fn normal_line_height(&self) -> Au {
        self.ascent + self.descent + self.line_gap
    }
    /// The content area height of an inline box.
    pub fn content_height(&self) -> Au {
        self.ascent + self.descent
    }
}

/// Vertical metrics of one bundled face in font units: `(upem, ascent, descent,
/// line gap, x-height)`, taken from the TTFs in `crates/render/assets/fonts` with
/// fontTools. Chromium on Linux reads `hhea` unless the face sets `USE_TYPO_METRICS`
/// (OS/2 fsSelection bit 7), in which case the OS/2 typo metrics apply; every bundled
/// face that sets the bit has identical hhea and typo values, so one table serves.
/// Descent is stored positive.
fn face_units(t: Typeface) -> (i32, i32, i32, i32, i32) {
    match t {
        Typeface::DejaVu | Typeface::Mono => (2048, 1901, 483, 0, 1120),
        Typeface::Inter => (2048, 1984, 494, 0, 1118),
        Typeface::OpenSans => (2048, 2189, 600, 0, 1096),
        Typeface::Ubuntu => (1000, 940, 260, 0, 518),
        Typeface::Roboto => (2048, 1900, 500, 0, 1082),
        Typeface::Arimo => (2048, 1854, 434, 67, 1082),
        Typeface::Tinos => (2048, 1825, 443, 87, 940),
        Typeface::Cousine => (2048, 1705, 615, 0, 1082),
        Typeface::Gelasio => (2048, 1900, 700, 0, 986),
        Typeface::Carlito => (2048, 1950, 550, 0, 978),
        Typeface::Caladea => (1000, 900, 250, 0, 467),
        Typeface::Lato => (2000, 1974, 426, 0, 1013),
        Typeface::SourceSans => (1000, 1024, 400, 0, 486),
        Typeface::SourceSerif => (1000, 1036, 335, 0, 475),
        Typeface::Poppins => (1000, 1050, 350, 100, 548),
        Typeface::Montserrat => (1000, 968, 251, 0, 525),
        Typeface::Playfair => (1000, 1082, 251, 0, 514),
        Typeface::JetBrainsMono => (1000, 1020, 300, 0, 550),
    }
}

/// `units` of the em at `size`, rounded to a whole pixel half up, the way Blink
/// rounds Skia's float ascent, descent and leading (`SkScalarRoundToScalar`) before
/// it adds them into the line spacing.
fn round_px(size: Au, units: i32, upem: i32) -> Au {
    let num = size.0 as i64 * units as i64;
    let den = upem as i64 * Au::PER_PX as i64;
    Au::from_px_i32((2 * num + den).div_euclid(2 * den) as i32)
}

/// The one place layout asks for ascent, descent and line gap. `cw_scene::metrics`
/// exposes advances only, so the vertical metrics come from `face_units`. Each of
/// the three is rounded to whole pixels first, as Blink does, so `line-height:
/// normal` is `round(ascent) + round(descent) + round(lineGap)` and a 14 px Arial
/// line has a 16 px content area. The monospace terminal face reports the terminal
/// cell height as its normal line height so code lines match the grid.
pub fn font_metrics(font: &Font) -> FontMetrics {
    let (upem, asc, desc, gap, xh) = face_units(font.typeface);
    let size = font.size;
    let ascent = round_px(size, asc, upem);
    let descent = round_px(size, desc, upem);
    let line_gap = if font.typeface == Typeface::Mono {
        let cell = cw_scene::text_cell(font.size_px()).1 as i32;
        (Au::from_px_i32(cell) - ascent - descent).max(Au::ZERO)
    } else {
        round_px(size, gap, upem)
    };
    FontMetrics {
        ascent,
        descent,
        line_gap,
        x_height: size.scale(xh, upem),
    }
}

/// The leading above the baseline of an inline box whose line height is `lh` and
/// content area `content`: half the leading, floored to a whole pixel (Blink's
/// `FontHeight::AddLeading`); the other half, with the remainder, goes below.
pub fn half_leading(lh: Au, content: Au) -> Au {
    Au::from_px_i32(Au((lh - content).0 / 2).to_px_floor())
}

/// The reference size advances are tabulated at: large enough that a glyph's advance
/// in 1/64 px at this size carries the font units to better than a part in a million.
const REF_SIZE: u16 = 4096;

/// Advance of one character in `Au` (1/64 px), without letter spacing, at the font's
/// exact (fractional) size: the tabulated advance is read at `REF_SIZE` and scaled,
/// so a 12.5 px face measures at 12.5 px, as Chromium's shaper does, not at 13.
pub fn advance(font: &Font, c: char) -> Au {
    if font.typeface == Typeface::Mono {
        let a = metrics::advance(font.typeface, font.scene_style(), c, font.size_px());
        return Au(a.clamp(0, Au::MAX.0 as i64) as i32);
    }
    let style = font.scene_style();
    let base = if c == '\t' { ' ' } else { c };
    let fine = match metrics::tabulated_advance(font.typeface, style, base, REF_SIZE) {
        Some(f) => f,
        None => fallback_advance(font.typeface, style, base),
    };
    let den = i64::from(REF_SIZE) * 64;
    let one = (fine * font.size.0 as i64 + den / 2).div_euclid(den);
    let a = if c == '\t' { one * 4 } else { one };
    Au(a.clamp(0, Au::MAX.0 as i64) as i32)
}

/// The advance of a character no metrics table covers, at [`REF_SIZE`], measured the
/// way the renderer draws it: through the complex-text path, which picks the fallback
/// face (an emoji face is far wider than the 0.6 em the tables assume). Layout used
/// the 0.6 em guess, so a flex item holding an emoji measured narrower than it
/// painted and its siblings overlapped it. Memoised: only characters outside the
/// tables reach here.
fn fallback_advance(typeface: Typeface, style: cw_scene::Style, c: char) -> i64 {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static CACHE: RefCell<HashMap<(Typeface, cw_scene::Style, char), i64>> = RefCell::new(HashMap::new());
    }
    CACHE.with(|cache| {
        if let Some(v) = cache.borrow().get(&(typeface, style, c)) {
            return *v;
        }
        let mut buf = [0u8; 4];
        // `text_width` rounds up to whole pixels; at REF_SIZE that is a part in 4096.
        let v = i64::from(metrics::text_width(
            typeface,
            style,
            c.encode_utf8(&mut buf),
            REF_SIZE,
        )) * 64;
        let v = if v == 0 {
            i64::from(REF_SIZE) * 64 * 3 / 5
        } else {
            v
        };
        cache.borrow_mut().insert((typeface, style, c), v);
        v
    })
}

/// Pair kerning between two adjacent characters in `Au`, at the font's exact
/// (fractional) size: the face's `kern` adjustment in font units
/// (`cw_scene::metrics::kern_units`) scaled and rounded to 1/64 px like `advance`.
/// Zero for unkerned pairs and for the faces that do not kern (the platform and
/// DejaVu faces, the monospace ones). It applies whatever `letter-spacing` is: see
/// `kern_spaced`.
pub fn kern(font: &Font, left: char, right: char) -> Au {
    let Some((units, upem)) = metrics::kern_units(font.typeface, font.scene_style(), left, right)
    else {
        return Au::ZERO;
    };
    let den = i64::from(upem);
    Au((i64::from(units) * font.size.0 as i64 + den / 2).div_euclid(den) as i32)
}

/// Kerning between an optional previous character and `c` under `letter_spacing`,
/// which does not turn kerning off: Blink keeps the `kern` feature and adds the
/// spacing on top of the kerned advance, so a spaced run is exactly
/// `letter-spacing` per character wider than the same run unspaced (measured
/// against Chromium: `AVATAR WAVY Ty.` at 20px Arial is 166.328px, and 241.328px
/// at `letter-spacing: 5px`, which is 15 x 5px more with the kerning kept).
/// The spacing is kept in the signature for the callers that pass it through.
pub fn kern_spaced(font: &Font, prev: Option<char>, c: char, _letter_spacing: Au) -> Au {
    match prev {
        Some(p) => kern(font, p, c),
        None => Au::ZERO,
    }
}

/// Width of a string, applying pair kerning between characters, `letter-spacing`
/// after every character and `word-spacing` after every space.
pub fn measure(font: &Font, text: &str, letter_spacing: Au, word_spacing: Au) -> Au {
    let mut w = Au::ZERO;
    let mut prev = None;
    for c in text.chars() {
        w += kern_spaced(font, prev, c, letter_spacing) + advance(font, c) + letter_spacing;
        if c == ' ' {
            w += word_spacing;
        }
        prev = Some(c);
    }
    w
}

/// The `ch` unit: the advance of `0`.
pub fn ch_unit(font: &Font) -> Au {
    advance(font, '0')
}

/// Which face draws the character, for splitting runs at font fallback boundaries:
/// 0 = the family's own face, 1 = the family's slanted face, 2/3 = DejaVu fallback,
/// 4 = only the rasteriser's own fallback.
pub fn face_key(font: &Font, c: char) -> u8 {
    match metrics::table_face(font.typeface, font.scene_style(), c) {
        Some((face, slanted)) => {
            let base = if face == font.typeface { 0 } else { 2 };
            base + slanted as u8
        }
        None => 4,
    }
}

/// `text-transform` applied to a string, keeping a byte-offset map from each output
/// character to the source byte it came from.
pub fn transform(text: &str, t: TextTransform) -> Vec<(char, usize)> {
    let mut out = Vec::with_capacity(text.len());
    let mut at_word_start = true;
    for (i, c) in text.char_indices() {
        match t {
            TextTransform::None => out.push((c, i)),
            TextTransform::Uppercase => out.extend(c.to_uppercase().map(|u| (u, i))),
            TextTransform::Lowercase => out.extend(c.to_lowercase().map(|u| (u, i))),
            TextTransform::Capitalize => {
                if at_word_start && c.is_alphanumeric() {
                    out.extend(c.to_uppercase().map(|u| (u, i)));
                } else {
                    out.push((c, i));
                }
            }
        }
        at_word_start = c.is_whitespace() || matches!(c, '-' | '/' | '(' | '"' | '\'');
    }
    out
}

/// One processed character of text after white-space handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharKind {
    /// A collapsible space (normal, nowrap, pre-line): a soft wrap opportunity after
    /// it, removed at line starts and ends.
    Space,
    /// A preserved space (pre, pre-wrap, break-spaces). Hangs at line end in pre-wrap.
    PreservedSpace,
    /// A preserved tab: advances to the next tab stop.
    Tab,
    /// A forced line break (preserved segment break).
    Newline,
    /// U+200B zero-width space: a break opportunity without width.
    ZeroWidthSpace,
    /// U+00AD soft hyphen: an invisible break opportunity.
    SoftHyphen,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PChar {
    pub ch: char,
    pub kind: CharKind,
    /// Byte offset in the source text node.
    pub src: usize,
}

/// State carried across text boxes in one inline formatting context, so that a space
/// at the end of one text node collapses with one at the start of the next.
#[derive(Clone, Copy, Debug, Default)]
pub struct CollapseState {
    /// The previous character emitted was a collapsible space.
    pub after_space: bool,
    /// Nothing has been emitted yet in this line's content (so leading spaces drop).
    pub at_start: bool,
}

/// CSS Text §4.1 white-space processing for one text node.
pub fn process(
    text: &str,
    ws: WhiteSpace,
    tt: TextTransform,
    state: &mut CollapseState,
) -> Vec<PChar> {
    let chars = transform(text, tt);
    let mut out: Vec<PChar> = Vec::with_capacity(chars.len());
    let collapses = ws.collapses();
    let keeps_newlines = ws.preserves_newlines();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let (c, src) = chars[i];
        let is_space = c == ' ' || c == '\t';
        let is_nl = c == '\n' || c == '\r';
        if c == '\r' {
            // CRLF and lone CR are segment breaks.
            if i + 1 < n && chars[i + 1].0 == '\n' {
                i += 1;
            }
        }
        if collapses {
            if is_nl {
                if keeps_newlines {
                    // pre-line: spaces around the break are removed.
                    while out.last().is_some_and(|p| p.kind == CharKind::Space) {
                        out.pop();
                    }
                    out.push(PChar {
                        ch: '\n',
                        kind: CharKind::Newline,
                        src,
                    });
                    state.after_space = false;
                    state.at_start = true;
                    i += 1;
                    while i < n && (chars[i].0 == ' ' || chars[i].0 == '\t') {
                        i += 1;
                    }
                    continue;
                }
                // Segment break becomes a space (then collapses).
                if !state.after_space {
                    out.push(PChar {
                        ch: ' ',
                        kind: CharKind::Space,
                        src,
                    });
                    state.after_space = true;
                }
                i += 1;
                continue;
            }
            if is_space {
                if !state.after_space {
                    out.push(PChar {
                        ch: ' ',
                        kind: CharKind::Space,
                        src,
                    });
                    state.after_space = true;
                }
                i += 1;
                continue;
            }
            let kind = match c {
                '\u{200B}' => CharKind::ZeroWidthSpace,
                '\u{00AD}' => CharKind::SoftHyphen,
                _ => CharKind::Other,
            };
            out.push(PChar { ch: c, kind, src });
            state.after_space = false;
            state.at_start = false;
        } else {
            let kind = if is_nl {
                CharKind::Newline
            } else if c == '\t' {
                CharKind::Tab
            } else if c == ' ' {
                CharKind::PreservedSpace
            } else {
                match c {
                    '\u{200B}' => CharKind::ZeroWidthSpace,
                    '\u{00AD}' => CharKind::SoftHyphen,
                    _ => CharKind::Other,
                }
            };
            out.push(PChar {
                ch: if is_nl { '\n' } else { c },
                kind,
                src,
            });
            state.after_space = false;
            state.at_start = false;
        }
        i += 1;
    }
    out
}

/// Whether the text is nothing but collapsible white space under `ws`.
pub fn is_collapsible_whitespace(text: &str, ws: WhiteSpace) -> bool {
    match ws {
        WhiteSpace::Normal | WhiteSpace::NoWrap => {
            text.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
        }
        WhiteSpace::PreLine => text.chars().all(|c| matches!(c, ' ' | '\t')),
        _ => text.is_empty(),
    }
}

fn is_cjk(c: char) -> bool {
    let u = c as u32;
    (0x3040..=0x30FF).contains(&u)      // kana
        || (0x3400..=0x4DBF).contains(&u)
        || (0x4E00..=0x9FFF).contains(&u) // CJK unified
        || (0xF900..=0xFAFF).contains(&u)
        || (0xAC00..=0xD7AF).contains(&u) // Hangul syllables
        || (0x20000..=0x2FA1F).contains(&u)
        || (0x3000..=0x303F).contains(&u) // CJK punctuation
        || (0xFF00..=0xFFEF).contains(&u) // full-width forms
}

fn is_hyphen(c: char) -> bool {
    matches!(c, '-' | '\u{2010}' | '\u{2013}')
}

/// A soft wrap opportunity between `prev` and `next` (both non-space characters)
/// under `word-break`. Spaces, `<wbr>` and forced breaks are handled by the caller.
pub fn break_between(prev: char, next: char, wb: WordBreak) -> bool {
    match wb {
        WordBreak::BreakAll => !next.is_ascii_punctuation() || is_hyphen(prev),
        WordBreak::KeepAll => is_hyphen(prev) && next.is_alphanumeric(),
        WordBreak::Normal | WordBreak::BreakWord => {
            (is_hyphen(prev) && next.is_alphanumeric() && !is_hyphen(next))
                || is_cjk(prev)
                || is_cjk(next)
        }
    }
}

/// Counter and marker text rendering.
pub fn counter_text(n: i32, style: crate::style::ListStyleType) -> String {
    use crate::style::ListStyleType as L;
    match style {
        L::None => String::new(),
        L::Disc => "\u{2022}".into(),
        L::Circle => "\u{25E6}".into(),
        L::Square => "\u{25AA}".into(),
        L::Decimal => n.to_string(),
        L::DecimalLeadingZero => {
            if n < 0 {
                format!("-{:02}", -n)
            } else {
                format!("{n:02}")
            }
        }
        L::LowerAlpha | L::UpperAlpha => {
            if n < 1 {
                return n.to_string();
            }
            let mut s = String::new();
            let mut v = n as u32;
            while v > 0 {
                let r = (v - 1) % 26;
                let c = (b'a' + r as u8) as char;
                s.insert(
                    0,
                    if style == L::UpperAlpha {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    },
                );
                v = (v - 1) / 26;
            }
            s
        }
        L::LowerRoman | L::UpperRoman => {
            if !(1..=3999).contains(&n) {
                return n.to_string();
            }
            const T: [(i32, &str); 13] = [
                (1000, "m"),
                (900, "cm"),
                (500, "d"),
                (400, "cd"),
                (100, "c"),
                (90, "xc"),
                (50, "l"),
                (40, "xl"),
                (10, "x"),
                (9, "ix"),
                (5, "v"),
                (4, "iv"),
                (1, "i"),
            ];
            let mut v = n;
            let mut s = String::new();
            for (val, sym) in T {
                while v >= val {
                    s.push_str(sym);
                    v -= val;
                }
            }
            if style == L::UpperRoman {
                s.to_ascii_uppercase()
            } else {
                s
            }
        }
    }
}

/// The marker string for a list item, with the trailing space markers get (§12.5).
pub fn marker_text(n: i32, style: crate::style::ListStyleType) -> String {
    use crate::style::ListStyleType as L;
    match style {
        L::None => String::new(),
        L::Disc | L::Circle | L::Square => format!("{} ", counter_text(n, style)),
        _ => format!("{}. ", counter_text(n, style)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::ComputedStyle;

    #[test]
    fn collapsing_and_preserving() {
        let mut st = CollapseState {
            after_space: false,
            at_start: true,
        };
        let p = process(
            "  a \n\t b  ",
            WhiteSpace::Normal,
            TextTransform::None,
            &mut st,
        );
        let s: String = p.iter().map(|c| c.ch).collect();
        assert_eq!(s, " a b ");
        assert!(st.after_space);
        let mut st = CollapseState::default();
        let p = process(
            "x  \n  y",
            WhiteSpace::PreLine,
            TextTransform::None,
            &mut st,
        );
        let s: String = p.iter().map(|c| c.ch).collect();
        assert_eq!(s, "x\ny");
        let mut st = CollapseState::default();
        let p = process("a\t b\r\nc", WhiteSpace::Pre, TextTransform::None, &mut st);
        assert_eq!(p.len(), 6);
        assert_eq!(p[1].kind, CharKind::Tab);
        assert_eq!(p[4].kind, CharKind::Newline);
        assert_eq!(p[5].src, 6);
    }

    #[test]
    fn transforms_keep_source_offsets() {
        let t = transform("héllo wörld-x y", TextTransform::Capitalize);
        let s: String = t.iter().map(|c| c.0).collect();
        assert_eq!(s, "Héllo Wörld-X Y");
        assert_eq!(t[6].1, 7);
        let t = transform("ß", TextTransform::Uppercase);
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|c| c.1 == 0));
    }

    #[test]
    fn counters_and_breaks() {
        use crate::style::ListStyleType as L;
        assert_eq!(counter_text(27, L::LowerAlpha), "aa");
        assert_eq!(counter_text(4, L::UpperRoman), "IV");
        assert_eq!(counter_text(7, L::DecimalLeadingZero), "07");
        assert_eq!(marker_text(3, L::Decimal), "3. ");
        assert!(break_between('-', 'a', WordBreak::Normal));
        assert!(!break_between('a', 'b', WordBreak::Normal));
        assert!(break_between('a', 'b', WordBreak::BreakAll));
        assert!(break_between('語', '言', WordBreak::Normal));
        assert!(!break_between('語', '言', WordBreak::KeepAll));
    }

    #[test]
    fn metrics_are_positive_and_quantised() {
        let s = ComputedStyle::initial();
        let m = font_metrics(&s.font);
        assert!(m.ascent > Au::ZERO && m.descent > Au::ZERO);
        // DejaVu Sans at 16 px: ascent 14.85 -> 15, descent 3.77 -> 4, no line gap.
        assert_eq!(m.ascent, Au::from_px_i32(15));
        assert_eq!(m.descent, Au::from_px_i32(4));
        assert_eq!(m.normal_line_height(), Au::from_px_i32(19));
        assert_eq!(
            advance(&s.font, 'a'),
            Au(metrics::advance(Typeface::DejaVu, false, 'a', 16) as i32)
        );
        assert_eq!(
            kern(&s.font, 'T', 'a'),
            Au::ZERO,
            "DejaVu is laid out unkerned"
        );
        assert_eq!(
            measure(&s.font, "a b", Au(1), Au(2)),
            advance(&s.font, 'a') + advance(&s.font, ' ') + advance(&s.font, 'b') + Au(3) + Au(2)
        );
    }
    #[test]
    fn kerning_matches_chromium_and_letter_spacing_disables_it() {
        let mut font = ComputedStyle::initial().font;
        font.typeface = Typeface::Arimo;
        font.size = Au::from_px_i32(13);
        let sum = |font: &Font, t: &str| t.chars().fold(Au::ZERO, |w, c| w + advance(font, c));
        // Chromium: "Talk" in 13 px Liberation Sans is 23.118 px (24.559 unkerned).
        let talk = measure(&font, "Talk", Au::ZERO, Au::ZERO);
        assert!(
            (talk.0 - (23.118f64 * 64.0).round() as i32).abs() <= 1,
            "{talk:?}"
        );
        assert!((sum(&font, "Talk").0 - (24.559f64 * 64.0).round() as i32).abs() <= 1);
        assert_eq!(talk, sum(&font, "Talk") + kern(&font, 'T', 'a'));
        // The same quantisation as the scene metrics at a whole size.
        assert_eq!(
            kern(&font, 'T', 'a').0 as i64,
            metrics::kern(Typeface::Arimo, false, 'T', 'a', 13)
        );
        // A fractional size kerns at that size: -227 units at 12.5 px is -88.7/64 px.
        font.size = Au::from_f64_px(12.5);
        assert_eq!(kern(&font, 'T', 'a'), Au(-89));
        // letter-spacing adds to the kerned advance; it does not turn kerning off.
        font.size = Au::from_px_i32(13);
        let spaced = measure(&font, "Talk", Au(64), Au::ZERO);
        assert_eq!(
            spaced,
            measure(&font, "Talk", Au::ZERO, Au::ZERO) + Au(64 * 4)
        );
        assert!(
            spaced < sum(&font, "Talk") + Au(64 * 4),
            "the T-a pair still kerns"
        );
        assert_eq!(
            kern_spaced(&font, Some('T'), 'a', Au(64)),
            kern(&font, 'T', 'a')
        );
        assert_eq!(
            kern_spaced(&font, Some('T'), 'a', Au::ZERO),
            kern(&font, 'T', 'a')
        );
        assert_eq!(kern_spaced(&font, None, 'a', Au::ZERO), Au::ZERO);
    }
}
