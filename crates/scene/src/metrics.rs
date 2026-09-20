//! Deterministic UI text metrics for the bundled font families. Advances come from a
//! table generated from the exact font files the renderer embeds, so layout code can
//! measure, centre, wrap and truncate text without rasterizing or consulting a host.
use crate::metrics_data as data;
use crate::metrics_italic as italic;
use crate::metrics_web as web;
use crate::text;
use serde::{Deserialize, Serialize};

/// Bundled UI font family. Glyphs missing from a family fall back to DejaVu. `Mono` is
/// the terminal's DejaVu Sans Mono, which has one weight and no slant: it measures on
/// the fixed grid `Primitive::Text` paints on ([`crate::text_cell`]), so a code span
/// laid out with it is exactly as wide as it is drawn.
///
/// `Inter`, `OpenSans`, `Ubuntu` and `Roboto` are the platform UI faces (Latin
/// subsets). The rest are the web faces: families pages ask for by name, and
/// metric-compatible stand-ins for the ones that cannot be bundled — Arimo for
/// Arial and Helvetica, Tinos for Times New Roman, Cousine for Courier New, Gelasio
/// for Georgia, Carlito for Calibri, Caladea for Cambria. Each has regular, bold,
/// italic and bold italic faces covering Latin, Greek and Cyrillic (whatever the
/// family designs of the DejaVu coverage set); [`crate::fonts::resolve_family`] maps
/// a CSS `font-family` list onto one of them.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Typeface {
    #[default]
    DejaVu,
    Inter,
    OpenSans,
    Ubuntu,
    Roboto,
    Mono,
    Arimo,
    Tinos,
    Cousine,
    Gelasio,
    Carlito,
    Caladea,
    Lato,
    SourceSans,
    SourceSerif,
    Poppins,
    Montserrat,
    Playfair,
    #[serde(rename = "jetbrains_mono")]
    JetBrainsMono,
}

/// The language a run of text is written in, as far as glyph selection cares: it
/// picks the regional forms of Han characters (and CJK punctuation). `Auto` infers
/// it from the text itself (kana means Japanese, Hangul Korean, Traditional-only
/// characters Traditional Chinese, otherwise Simplified Chinese).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Lang {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "zh-Hans")]
    ZhHans,
    #[serde(rename = "zh-Hant")]
    ZhHant,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "ko")]
    Ko,
}
impl Lang {
    pub fn is_auto(&self) -> bool {
        *self == Self::Auto
    }
    /// From a BCP 47 tag such as an HTML `lang` attribute or a locale name
    /// (`zh-TW`, `zh_Hant_HK`, `ja-JP`, `ko`, `en-US`). Languages without regional
    /// Han forms, and malformed tags, are `Auto`.
    pub fn from_tag(tag: &str) -> Self {
        let lower = tag.trim().to_ascii_lowercase().replace('_', "-");
        let mut parts = lower.split('-');
        match parts.next().unwrap_or("") {
            "ja" | "jpn" => Self::Ja,
            "ko" | "kor" => Self::Ko,
            "zh" | "zho" | "chi" | "cmn" | "yue" | "lzh" => {
                let rest: Vec<&str> = parts.collect();
                if rest.contains(&"hant") {
                    Self::ZhHant
                } else if rest.contains(&"hans") {
                    Self::ZhHans
                } else if rest.iter().any(|r| matches!(*r, "tw" | "hk" | "mo"))
                    || lower.starts_with("yue")
                {
                    Self::ZhHant
                } else {
                    Self::ZhHans
                }
            }
            _ => Self::Auto,
        }
    }
}

/// How a run of UI text is set: weight, slant and language. `From<bool>` reads the
/// bool as bold, so every metrics function still takes the plain `bold` flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub lang: Lang,
}
impl Style {
    pub const fn new(bold: bool, italic: bool, lang: Lang) -> Self {
        Self { bold, italic, lang }
    }
    /// The same weight and language, upright.
    pub const fn upright(self) -> Self {
        Self {
            italic: false,
            ..self
        }
    }
}
impl From<bool> for Style {
    fn from(bold: bool) -> Self {
        Self {
            bold,
            ..Self::default()
        }
    }
}

impl Typeface {
    /// Every bundled family, in declaration order.
    pub const ALL: [Typeface; 19] = [
        Self::DejaVu,
        Self::Inter,
        Self::OpenSans,
        Self::Ubuntu,
        Self::Roboto,
        Self::Mono,
        Self::Arimo,
        Self::Tinos,
        Self::Cousine,
        Self::Gelasio,
        Self::Carlito,
        Self::Caladea,
        Self::Lato,
        Self::SourceSans,
        Self::SourceSerif,
        Self::Poppins,
        Self::Montserrat,
        Self::Playfair,
        Self::JetBrainsMono,
    ];
    /// The web faces: every family with four faces (regular, bold, italic, bold
    /// italic) and Latin, Greek and Cyrillic coverage, in declaration order.
    pub const WEB: [Typeface; 13] = [
        Self::Arimo,
        Self::Tinos,
        Self::Cousine,
        Self::Gelasio,
        Self::Carlito,
        Self::Caladea,
        Self::Lato,
        Self::SourceSans,
        Self::SourceSerif,
        Self::Poppins,
        Self::Montserrat,
        Self::Playfair,
        Self::JetBrainsMono,
    ];
    pub fn is_default(&self) -> bool {
        *self == Self::DejaVu
    }
    /// The family's own name, as a stylesheet would write it in `font-family`.
    pub fn family_name(self) -> &'static str {
        match self {
            Self::DejaVu => "DejaVu Sans",
            Self::Inter => "Inter",
            Self::OpenSans => "Open Sans",
            Self::Ubuntu => "Ubuntu",
            Self::Roboto => "Roboto",
            Self::Mono => "DejaVu Sans Mono",
            Self::Arimo => "Arimo",
            Self::Tinos => "Tinos",
            Self::Cousine => "Cousine",
            Self::Gelasio => "Gelasio",
            Self::Carlito => "Carlito",
            Self::Caladea => "Caladea",
            Self::Lato => "Lato",
            Self::SourceSans => "Source Sans 3",
            Self::SourceSerif => "Source Serif 4",
            Self::Poppins => "Poppins",
            Self::Montserrat => "Montserrat",
            Self::Playfair => "Playfair Display",
            Self::JetBrainsMono => "JetBrains Mono",
        }
    }
    /// Whether the family is fixed pitch.
    pub fn is_monospace(self) -> bool {
        matches!(self, Self::Mono | Self::Cousine | Self::JetBrainsMono)
    }
    /// A web family's four faces by `bold + 2 * italic`, `None` where the family has
    /// no such file; `None` for the platform and DejaVu families.
    fn web_faces(self) -> Option<&'static [web::Face; 4]> {
        Some(match self {
            Self::Arimo => &web::ARIMO,
            Self::Tinos => &web::TINOS,
            Self::Cousine => &web::COUSINE,
            Self::Gelasio => &web::GELASIO,
            Self::Carlito => &web::CARLITO,
            Self::Caladea => &web::CALADEA,
            Self::Lato => &web::LATO,
            Self::SourceSans => &web::SOURCESANS,
            Self::SourceSerif => &web::SOURCESERIF,
            Self::Poppins => &web::POPPINS,
            Self::Montserrat => &web::MONTSERRAT,
            Self::Playfair => &web::PLAYFAIR,
            Self::JetBrainsMono => &web::JETBRAINSMONO,
            _ => return None,
        })
    }
    /// Which of a web family's faces serves `bold`/`italic`: the face itself when the
    /// family has it, else the nearest it does have, in the order the renderer
    /// synthesises from — the upright of the same weight (synthetic oblique), the
    /// regular of the same slant (synthetic bold), then the regular. Returns the
    /// index `bold + 2 * italic` of the face used, so measurement and drawing agree.
    pub fn web_face_index(self, bold: bool, italic: bool) -> Option<usize> {
        let faces = self.web_faces()?;
        let wanted = usize::from(bold) + 2 * usize::from(italic);
        [wanted, wanted & !2, wanted & !1, 0]
            .into_iter()
            .find(|&i| faces[i].is_some())
    }
    fn table(self, bold: bool, slanted: bool) -> (&'static [(u32, u16)], u32) {
        if let Some(faces) = self.web_faces() {
            let i = self
                .web_face_index(bold, slanted)
                .expect("every web family has a regular face");
            return faces[i].expect("web_face_index returns a present face");
        }
        match (self, bold, slanted) {
            // One face serves every weight and slant of the monospace family.
            (Self::Mono, _, _) => (data::DEJAVU_MONO, data::DEJAVU_MONO_UPEM),
            (Self::DejaVu, false, false) => (data::DEJAVU_REGULAR, data::DEJAVU_REGULAR_UPEM),
            (Self::DejaVu, true, false) => (data::DEJAVU_BOLD, data::DEJAVU_BOLD_UPEM),
            (Self::Inter, false, false) => (data::INTER_REGULAR, data::INTER_REGULAR_UPEM),
            (Self::Inter, true, false) => (data::INTER_BOLD, data::INTER_BOLD_UPEM),
            (Self::OpenSans, false, false) => (data::OPENSANS_REGULAR, data::OPENSANS_REGULAR_UPEM),
            (Self::OpenSans, true, false) => (data::OPENSANS_BOLD, data::OPENSANS_BOLD_UPEM),
            (Self::Ubuntu, false, false) => (data::UBUNTU_REGULAR, data::UBUNTU_REGULAR_UPEM),
            (Self::Ubuntu, true, false) => (data::UBUNTU_BOLD, data::UBUNTU_BOLD_UPEM),
            (Self::Roboto, false, false) => (data::ROBOTO_REGULAR, data::ROBOTO_REGULAR_UPEM),
            (Self::Roboto, true, false) => (data::ROBOTO_BOLD, data::ROBOTO_BOLD_UPEM),
            (Self::DejaVu, false, true) => (italic::DEJAVU_ITALIC, italic::DEJAVU_ITALIC_UPEM),
            (Self::DejaVu, true, true) => {
                (italic::DEJAVU_BOLD_ITALIC, italic::DEJAVU_BOLD_ITALIC_UPEM)
            }
            (Self::Inter, false, true) => (italic::INTER_ITALIC, italic::INTER_ITALIC_UPEM),
            (Self::Inter, true, true) => {
                (italic::INTER_BOLD_ITALIC, italic::INTER_BOLD_ITALIC_UPEM)
            }
            (Self::OpenSans, false, true) => {
                (italic::OPENSANS_ITALIC, italic::OPENSANS_ITALIC_UPEM)
            }
            (Self::OpenSans, true, true) => (
                italic::OPENSANS_BOLD_ITALIC,
                italic::OPENSANS_BOLD_ITALIC_UPEM,
            ),
            (Self::Ubuntu, false, true) => (italic::UBUNTU_ITALIC, italic::UBUNTU_ITALIC_UPEM),
            (Self::Ubuntu, true, true) => {
                (italic::UBUNTU_BOLD_ITALIC, italic::UBUNTU_BOLD_ITALIC_UPEM)
            }
            (Self::Roboto, false, true) => (italic::ROBOTO_ITALIC, italic::ROBOTO_ITALIC_UPEM),
            (Self::Roboto, true, true) => {
                (italic::ROBOTO_BOLD_ITALIC, italic::ROBOTO_BOLD_ITALIC_UPEM)
            }
            // Served by `web_faces` above.
            (
                Self::Arimo
                | Self::Tinos
                | Self::Cousine
                | Self::Gelasio
                | Self::Carlito
                | Self::Caladea
                | Self::Lato
                | Self::SourceSans
                | Self::SourceSerif
                | Self::Poppins
                | Self::Montserrat
                | Self::Playfair
                | Self::JetBrainsMono,
                _,
                _,
            ) => unreachable!("web families are table-driven through web_faces"),
        }
    }
    /// Whether the family itself (not the DejaVu fallback) supplies this character.
    pub fn covers(self, bold: bool, c: char) -> bool {
        lookup(self.table(bold, false).0, c).is_some()
    }
    /// Whether the family's face for `style` (italic or upright) supplies `c`.
    pub fn covers_style(self, style: Style, c: char) -> bool {
        lookup(self.table(style.bold, style.italic).0, c).is_some()
    }
}
fn lookup(table: &[(u32, u16)], c: char) -> Option<u16> {
    table
        .binary_search_by_key(&(c as u32), |e| e.0)
        .ok()
        .map(|i| table[i].1)
}
/// Which table-driven face draws `c` in `style`: the platform family's face, else
/// DejaVu's, and for italic text a character neither italic face has is drawn by the
/// upright faces in the same order (symbols, arrows and box drawing never slant).
/// Returns the family and whether the italic face is used; `None` when only the
/// renderer's own fallback (the complex-text chain or `.notdef`) can draw it.
pub fn table_face(typeface: Typeface, style: Style, c: char) -> Option<(Typeface, bool)> {
    let slants: &[bool] = if style.italic {
        &[true, false]
    } else {
        &[false]
    };
    for &slanted in slants {
        for family in [typeface, Typeface::DejaVu] {
            if lookup(family.table(style.bold, slanted).0, c).is_some() {
                return Some((family, slanted));
            }
        }
    }
    None
}
/// Tabulated advance in 1/64 pixel, or `None` when only the rasterizer knows the glyph.
pub fn tabulated_advance(
    typeface: Typeface,
    style: impl Into<Style>,
    c: char,
    size: u16,
) -> Option<i64> {
    let style = style.into();
    let c = if c == '\t' { ' ' } else { c };
    let (family, slanted) = table_face(typeface, style, c)?;
    // The monospace face is painted on the terminal grid, one cell per character,
    // whichever face supplies the glyph: measure that cell, not the font's advance.
    if typeface == Typeface::Mono {
        return Some(i64::from(crate::text_cell(size).0) * 64);
    }
    let (table, upem) = family.table(style.bold, slanted);
    lookup(table, c).map(|units| {
        (i64::from(units) * i64::from(size) * 64 + i64::from(upem) / 2) / i64::from(upem)
    })
}
/// Advance in 1/64 pixel. Tabs advance four spaces; unknown glyphs use 0.6 em.
pub fn advance(typeface: Typeface, style: impl Into<Style>, c: char, size: u16) -> i64 {
    let one =
        tabulated_advance(typeface, style, c, size).unwrap_or(if typeface == Typeface::Mono {
            i64::from(crate::text_cell(size).0) * 64
        } else {
            i64::from(size) * 64 * 3 / 5
        });
    if c == '\t' {
        one * 4
    } else {
        one
    }
}
fn width_64(typeface: Typeface, style: Style, text: &str, size: u16) -> i64 {
    text.chars()
        .filter(|c| *c != '\r')
        .map(|c| advance(typeface, style, c, size))
        .sum()
}
/// Advance width of one line in 1/64 pixel, through the complex-text path when the
/// line needs fallback faces, bidi or shaping (see [`crate::text`]).
fn line_width_64(typeface: Typeface, style: Style, line: &str, size: u16) -> i64 {
    if text::is_simple(typeface, style, line) {
        width_64(typeface, style, line, size)
    } else {
        text::line_width(typeface, style, line, size)
    }
}
/// Pixel width of the widest line, rounded up.
pub fn text_width(typeface: Typeface, style: impl Into<Style>, text: &str, size: u16) -> u32 {
    let style = style.into();
    text.split('\n')
        .map(|line| (line_width_64(typeface, style, line, size) + 63) / 64)
        .max()
        .unwrap_or(0) as u32
}
/// Greedy word wrapping shared by layout and rasterization. Newlines are preserved,
/// breaks fall after spaces (and between CJK characters), and words wider than a
/// line break between characters (grapheme clusters, for complex text).
pub fn wrap(
    typeface: Typeface,
    style: impl Into<Style>,
    text: &str,
    size: u16,
    width: u32,
) -> Vec<String> {
    text::block(typeface, style.into(), text, size, width, false)
        .into_iter()
        .map(|line| line.text)
        .collect()
}
/// The original table-only wrapping of one paragraph, used verbatim for text the
/// complex path is not needed for.
pub(crate) fn wrap_simple_paragraph(
    typeface: Typeface,
    style: Style,
    paragraph: &str,
    size: u16,
    limit: i64,
    lines: &mut Vec<String>,
) {
    let mut line = String::new();
    let mut pen = 0i64;
    for word in paragraph.split_inclusive(' ') {
        let visible = width_64(typeface, style, word.trim_end_matches(' '), size);
        if pen > 0 && pen + visible > limit {
            lines.push(std::mem::take(&mut line));
            pen = 0;
        }
        if visible > limit {
            for c in word.chars().filter(|c| *c != '\r') {
                let a = advance(typeface, style, c, size);
                if pen > 0 && pen + a > limit {
                    lines.push(std::mem::take(&mut line));
                    pen = 0;
                }
                line.push(c);
                pen += a;
            }
        } else {
            line.extend(word.chars().filter(|c| *c != '\r'));
            pen += width_64(typeface, style, word, size);
        }
    }
    lines.push(line);
}
/// Single-line truncation with a trailing ellipsis when `text` exceeds `width` pixels.
pub fn ellipsize(
    typeface: Typeface,
    style: impl Into<Style>,
    text: &str,
    size: u16,
    width: u32,
) -> String {
    let style = style.into();
    let text = text.split('\n').next().unwrap_or("");
    if !text::is_simple(typeface, style, text) {
        return text::ellipsize(typeface, style, text, size, width);
    }
    let limit = i64::from(width) * 64;
    if width_64(typeface, style, text, size) <= limit {
        return text.into();
    }
    let ellipsis = advance(typeface, style, '…', size);
    let mut out = String::new();
    let mut pen = 0;
    for c in text.chars() {
        let a = advance(typeface, style, c, size);
        if pen + a + ellipsis > limit {
            break;
        }
        out.push(c);
        pen += a;
    }
    let mut out = out.trim_end().to_owned();
    out.push('…');
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapping_prefers_word_boundaries_and_preserves_newlines() {
        let t = Typeface::Inter;
        let lines = wrap(
            t,
            false,
            "Publish a reviewed checklist after\nsign-off",
            13,
            120,
        );
        assert!(lines.len() >= 3);
        assert_eq!(lines.last().unwrap(), "sign-off");
        for line in &lines {
            assert!(text_width(t, false, line.trim_end(), 13) <= 120, "{line}");
            assert!(!line.starts_with(' '));
        }
        assert_eq!(
            lines.concat().replace(' ', ""),
            "Publishareviewedchecklistaftersign-off"
        );
        assert_eq!(wrap(t, false, "abcdefghij", 13, 20).concat(), "abcdefghij");
        assert_eq!(wrap(t, false, "a\n\nb", 13, 100), ["a", "", "b"]);
    }
    #[test]
    fn ellipsis_fits_and_families_differ() {
        for t in [
            Typeface::DejaVu,
            Typeface::Inter,
            Typeface::OpenSans,
            Typeface::Ubuntu,
            Typeface::Roboto,
        ] {
            let s = ellipsize(t, true, "Northstar Workshop release checklist", 14, 110);
            assert!(s.ends_with('…'));
            assert!(text_width(t, true, &s, 14) <= 110);
            assert_eq!(ellipsize(t, false, "Mail", 14, 110), "Mail");
            assert!(t.covers(false, 'é') && !t.covers(false, '語'));
        }
        assert_ne!(
            text_width(Typeface::DejaVu, false, "Settings", 13),
            text_width(Typeface::Roboto, false, "Settings", 13)
        );
        assert!(tabulated_advance(Typeface::Inter, false, 'λ', 13).is_some());
    }
    #[test]
    fn the_monospace_face_measures_on_the_terminal_grid() {
        let cell = crate::text_cell(13).0;
        for c in ['i', 'W', 'λ', '─'] {
            assert_eq!(
                advance(Typeface::Mono, false, c, 13),
                i64::from(cell) * 64,
                "{c}"
            );
            assert_eq!(
                advance(Typeface::Mono, true, c, 13),
                i64::from(cell) * 64,
                "{c}"
            );
        }
        assert!(Typeface::Mono.covers(false, 'é') && !Typeface::Mono.covers(false, '語'));
        assert_eq!(text_width(Typeface::Mono, false, "a1b2c3d4", 13), cell * 8);
        assert_eq!(
            table_face(Typeface::Mono, Style::new(true, true, Lang::Auto), 'x'),
            Some((Typeface::Mono, true))
        );
        let lines = wrap(
            Typeface::Mono,
            false,
            "fn main() { println!() }",
            13,
            cell * 10,
        );
        assert!(
            lines.iter().all(|l| l.trim_end().chars().count() <= 10),
            "{lines:?}"
        );
    }
    #[test]
    fn web_families_have_four_faces_and_measure_in_their_own_advances() {
        for t in Typeface::WEB {
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                assert_eq!(
                    t.web_face_index(bold, italic),
                    Some(usize::from(bold) + 2 * usize::from(italic)),
                    "{t:?} bold {bold} italic {italic}"
                );
                let style = Style::new(bold, italic, Lang::Auto);
                assert_eq!(table_face(t, style, 'a'), Some((t, italic)), "{t:?}");
                assert!(t.covers_style(style, 'é'), "{t:?}");
                assert!(!t.covers_style(style, '語'), "{t:?}");
            }
            // Bold is a real face, not the regular re-measured (italics may share the
            // upright's advances by design, as Arimo's does, like Arial's; a monospace
            // family's bold is fixed pitch at the same width).
            let sample = "Hamburgefonstiv 0123";
            if !t.is_monospace() {
                assert_ne!(
                    text_width(t, false, sample, 16),
                    text_width(t, true, sample, 16),
                    "{t:?} bold"
                );
            }
            // Greek and Cyrillic come from the family where it designs them; Gelasio,
            // Caladea and Poppins design neither and Playfair Display no Greek, and
            // those fall back to DejaVu like any other gap.
            let designs_greek = !matches!(
                t,
                Typeface::Gelasio | Typeface::Caladea | Typeface::Poppins | Typeface::Playfair
            );
            let designs_cyrillic =
                !matches!(t, Typeface::Gelasio | Typeface::Caladea | Typeface::Poppins);
            assert_eq!(t.covers(false, 'λ'), designs_greek, "{t:?} Greek");
            assert_eq!(t.covers(false, 'ж'), designs_cyrillic, "{t:?} Cyrillic");
            assert_eq!(
                table_face(t, Style::default(), 'λ'),
                Some((if designs_greek { t } else { Typeface::DejaVu }, false))
            );
        }
        // Distinct families measure distinctly; the monospace ones are fixed pitch.
        assert_ne!(
            text_width(Typeface::Arimo, false, "Settings", 13),
            text_width(Typeface::Tinos, false, "Settings", 13)
        );
        for t in [Typeface::Cousine, Typeface::JetBrainsMono] {
            assert_eq!(
                advance(t, false, 'i', 14),
                advance(t, false, 'W', 14),
                "{t:?}"
            );
        }
        assert_eq!(Typeface::ALL.len(), 6 + Typeface::WEB.len());
        for t in Typeface::ALL {
            assert!(!t.family_name().is_empty());
        }
    }
}
