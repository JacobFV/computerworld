//! Deterministic UI text metrics for the bundled font families. Advances come from a
//! table generated from the exact font files the renderer embeds, so layout code can
//! measure, centre, wrap and truncate text without rasterizing or consulting a host.
use crate::kerning_data as kerning;
use crate::kerning_dejavu;
use crate::metrics_data as data;
use crate::metrics_dejavu as dejavu_web;
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

/// How a run of UI text is set: weight, slant and language, and whether it is web
/// content. `From<bool>` reads the bool as bold, so every metrics function still
/// takes the plain `bold` flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub lang: Lang,
    /// Text laid out by the web engine, which measures the way Chromium does: every
    /// family kerns by its own `kern` feature (DejaVu Sans and the platform faces
    /// too, not only the web faces), and [`Typeface::Mono`] advances by the font's
    /// own advance, `1233 / 2048` em, instead of the terminal grid. Off for the
    /// native UI (desktop scenes, the terminal, legacy pages), whose layout and
    /// golden frames are unkerned and on the grid. [`Style::for_web`] turns it on.
    pub web: bool,
}
impl Style {
    pub const fn new(bold: bool, italic: bool, lang: Lang) -> Self {
        Self {
            bold,
            italic,
            lang,
            web: false,
        }
    }
    /// The same style, measured as web content (see [`Style::web`]).
    pub const fn for_web(self) -> Self {
        Self { web: true, ..self }
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
    /// The family's kern pairs by `bold + 2 * italic`, and whether they are a web
    /// family's (indexed through [`Typeface::web_face_index`]). DejaVu Sans kerns
    /// only as web content ([`Style::web`]); the native UI lays it out unkerned.
    /// The platform faces never kern: the files they are drawn from keep no layout
    /// tables, and Chromium measures a page serving those files unkerned. Nor do
    /// the monospace faces.
    fn kerning(self, style: Style) -> Option<(&'static [kerning::Pairs; 4], bool)> {
        if let Some(pairs) = self.web_kerning() {
            return Some((pairs, true));
        }
        (style.web && self == Self::DejaVu).then_some((&kerning_dejavu::DEJAVU, false))
    }
    /// A web family's kern pairs by `bold + 2 * italic`; `None` for the platform and
    /// DejaVu families (see [`Typeface::kerning`]).
    fn web_kerning(self) -> Option<&'static [kerning::Pairs; 4]> {
        Some(match self {
            Self::Arimo => &kerning::ARIMO,
            Self::Tinos => &kerning::TINOS,
            Self::Cousine => &kerning::COUSINE,
            Self::Gelasio => &kerning::GELASIO,
            Self::Carlito => &kerning::CARLITO,
            Self::Caladea => &kerning::CALADEA,
            Self::Lato => &kerning::LATO,
            Self::SourceSans => &kerning::SOURCESANS,
            Self::SourceSerif => &kerning::SOURCESERIF,
            Self::Poppins => &kerning::POPPINS,
            Self::Montserrat => &kerning::MONTSERRAT,
            Self::Playfair => &kerning::PLAYFAIR,
            Self::JetBrainsMono => &kerning::JETBRAINSMONO,
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
/// The advance of `c` in font units in `family`'s face (`slanted` or upright), with
/// the face's units per em. The DejaVu tables tabulate the `WIDE` ranges; for web
/// content the upright DejaVu Sans and Mono faces also answer for the rest of what
/// their files draw (symbols, arrows, maths, box drawing, dingbats), which native
/// text measures at 0.6 em but Chromium, falling back to DejaVu Sans, by the font.
fn face_units(family: Typeface, style: Style, slanted: bool, c: char) -> Option<(u16, u32)> {
    let (table, upem) = family.table(style.bold, slanted);
    lookup(table, c).map(|units| (units, upem)).or_else(|| {
        if !style.web || slanted {
            return None;
        }
        let (table, upem) = match family {
            Typeface::DejaVu if style.bold => (
                dejavu_web::DEJAVU_BOLD_WEB,
                dejavu_web::DEJAVU_BOLD_WEB_UPEM,
            ),
            Typeface::DejaVu => (
                dejavu_web::DEJAVU_REGULAR_WEB,
                dejavu_web::DEJAVU_REGULAR_WEB_UPEM,
            ),
            Typeface::Mono => (
                dejavu_web::DEJAVU_MONO_WEB,
                dejavu_web::DEJAVU_MONO_WEB_UPEM,
            ),
            _ => return None,
        };
        lookup(table, c).map(|units| (units, upem))
    })
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
            if face_units(family, style, slanted, c).is_some() {
                return Some((family, slanted));
            }
        }
    }
    None
}
/// The tabulated advance of `c` in font units, with the face's units per em: the
/// face's own advance, even for [`Typeface::Mono`] outside web content (which
/// [`tabulated_advance`] puts on the terminal grid instead). `None` when only the
/// rasterizer knows the glyph. Tabs read as spaces.
pub fn advance_units(typeface: Typeface, style: impl Into<Style>, c: char) -> Option<(u16, u32)> {
    let style = style.into();
    let c = if c == '\t' { ' ' } else { c };
    let (family, slanted) = table_face(typeface, style, c)?;
    face_units(family, style, slanted, c)
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
    // Web content advances by the face's own advance, as Chromium does.
    if typeface == Typeface::Mono && !style.web {
        return Some(i64::from(crate::text_cell(size).0) * 64);
    }
    face_units(family, style, slanted, c).map(|(units, upem)| {
        (i64::from(units) * i64::from(size) * 64 + i64::from(upem) / 2) / i64::from(upem)
    })
}
/// Advance in 1/64 pixel. Tabs advance four spaces; unknown glyphs use 0.6 em.
pub fn advance(typeface: Typeface, style: impl Into<Style>, c: char, size: u16) -> i64 {
    let style = style.into();
    let one = tabulated_advance(typeface, style, c, size).unwrap_or(
        if typeface == Typeface::Mono && !style.web {
            i64::from(crate::text_cell(size).0) * 64
        } else {
            i64::from(size) * 64 * 3 / 5
        },
    );
    if c == '\t' {
        one * 4
    } else {
        one
    }
}
/// The face's `kern` adjustment between `left` and `right` in font units, with the
/// face's units per em; `None` when the pair is not kerned. The web faces
/// ([`Typeface::WEB`]) kern always, DejaVu Sans only as web content
/// ([`Style::web`]), the platform and monospace faces never; and only between two
/// characters the family's own face draws (a DejaVu fallback glyph never kerns
/// against its neighbour). The table covers ASCII, Latin-1 and common punctuation.
pub fn kern_units(
    typeface: Typeface,
    style: impl Into<Style>,
    left: char,
    right: char,
) -> Option<(i16, u32)> {
    let style = style.into();
    let (faces, web_family) = typeface.kerning(style)?;
    let (l, r) = (
        u16::try_from(left as u32).ok()?,
        u16::try_from(right as u32).ok()?,
    );
    let (family, slanted) = table_face(typeface, style, left)?;
    if family != typeface || table_face(typeface, style, right) != Some((family, slanted)) {
        return None;
    }
    let i = if web_family {
        typeface.web_face_index(style.bold, slanted)?
    } else {
        usize::from(style.bold) + 2 * usize::from(slanted)
    };
    let (lefts, rights, values) = faces[i]?;
    let at = lefts.binary_search_by_key(&l, |e| e.0).ok()?;
    let start = lefts[at].1 as usize;
    let end = lefts.get(at + 1).map_or(rights.len(), |e| e.1 as usize);
    let k = rights[start..end].binary_search(&r).ok()?;
    Some((values[start + k], typeface.table(style.bold, slanted).1))
}
/// Pair kerning between `left` and `right` in 1/64 pixel (usually negative), rounded
/// to nearest like [`advance`]; 0 for an unkerned pair, for every platform face, and
/// for DejaVu outside web content. Measurement, wrapping, truncation and the renderer's glyph placement
/// all add it between adjacent characters.
pub fn kern(
    typeface: Typeface,
    style: impl Into<Style>,
    left: char,
    right: char,
    size: u16,
) -> i64 {
    kern_units(typeface, style, left, right).map_or(0, |(units, upem)| {
        (i64::from(units) * i64::from(size) * 64 + i64::from(upem) / 2).div_euclid(i64::from(upem))
    })
}
/// Kerning between an optional previous character and `c`.
pub(crate) fn kern_after(
    typeface: Typeface,
    style: Style,
    prev: Option<char>,
    c: char,
    size: u16,
) -> i64 {
    prev.map_or(0, |p| kern(typeface, style, p, c, size))
}
fn width_64(typeface: Typeface, style: Style, text: &str, size: u16) -> i64 {
    let mut prev = None;
    let mut pen = 0;
    for c in text.chars().filter(|c| *c != '\r') {
        pen += kern_after(typeface, style, prev, c, size) + advance(typeface, style, c, size);
        prev = Some(c);
    }
    pen
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
    // The last character on the line, which the next one kerns against.
    let mut last = None;
    for word in paragraph.split_inclusive(' ') {
        let first = word.chars().find(|c| *c != '\r');
        let joint = first.map_or(0, |c| kern_after(typeface, style, last, c, size));
        let visible = width_64(typeface, style, word.trim_end_matches(' '), size);
        if pen > 0 && pen + joint + visible > limit {
            lines.push(std::mem::take(&mut line));
            pen = 0;
            last = None;
        }
        if visible > limit {
            for c in word.chars().filter(|c| *c != '\r') {
                let mut a =
                    kern_after(typeface, style, last, c, size) + advance(typeface, style, c, size);
                if pen > 0 && pen + a > limit {
                    lines.push(std::mem::take(&mut line));
                    pen = 0;
                    a = advance(typeface, style, c, size);
                }
                line.push(c);
                pen += a;
                last = Some(c);
            }
        } else {
            let joint = first.map_or(0, |c| kern_after(typeface, style, last, c, size));
            line.extend(word.chars().filter(|c| *c != '\r'));
            pen += joint + width_64(typeface, style, word, size);
            last = word.chars().rfind(|c| *c != '\r').or(last);
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
    let mut prev = None;
    for c in text.chars() {
        let a = kern_after(typeface, style, prev, c, size) + advance(typeface, style, c, size);
        if pen + a + kern(typeface, style, c, '…', size).max(0) + ellipsis > limit {
            break;
        }
        out.push(c);
        pen += a;
        prev = Some(c);
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
    fn web_faces_kern_by_their_gpos_pairs() {
        // Font units, read from the instanced masters with fontTools.
        let t = Typeface::Arimo;
        assert_eq!(kern_units(t, false, 'T', 'a'), Some((-227, 2048)));
        assert_eq!(kern_units(t, false, 'A', 'V'), Some((-152, 2048)));
        assert_eq!(kern_units(t, false, 'P', '.'), Some((-264, 2048)));
        assert_eq!(kern_units(t, false, 'a', 'l'), None);
        assert_eq!(kern(t, false, 'T', 'a', 13), -92); // -227 * 13 * 64 / 2048 = -92.2
        assert_eq!(kern(t, false, 'a', 'l', 13), 0);
        // Chromium measures "Talk" in 13 px Liberation Sans at 23.118 px (24.559
        // unkerned): (1251 + 1139 + 455 + 1024 - 227) * 13 / 2048.
        let talk: i64 = width_64(t, Style::default(), "Talk", 13);
        assert!(
            (talk - (23.118f64 * 64.0).round() as i64).abs() <= 1,
            "{talk}"
        );
        let plain: i64 = "Talk".chars().map(|c| advance(t, false, c, 13)).sum();
        assert_eq!(talk, plain + kern(t, false, 'T', 'a', 13));
        assert_eq!(text_width(t, false, "Talk", 13), 24);
        // The monospace and unkerned families, and every platform face, stay at the
        // sum of their advances: desktop scenes are laid out without kerning.
        for t in [
            Typeface::DejaVu,
            Typeface::Inter,
            Typeface::OpenSans,
            Typeface::Ubuntu,
            Typeface::Roboto,
            Typeface::Mono,
            Typeface::Cousine,
            Typeface::JetBrainsMono,
        ] {
            for (l, r) in [('T', 'a'), ('A', 'V'), ('P', '.'), ('W', 'o')] {
                assert_eq!(kern(t, false, l, r, 16), 0, "{t:?} {l}{r}");
            }
        }
        // A fallback glyph does not kern against the family's own.
        assert_eq!(kern(Typeface::Caladea, false, 'T', 'λ', 16), 0);
        // Each face has its own pairs.
        assert!(kern(Typeface::Tinos, true, 'A', 'V', 16) < 0);
        assert!(
            kern(
                Typeface::Lato,
                Style::new(false, true, Lang::Auto),
                'T',
                'o',
                16
            ) < 0
        );
    }
    #[test]
    fn web_content_kerns_every_family_and_sets_mono_on_its_advance() {
        let web = |bold: bool| Style::from(bold).for_web();
        // Chromium's widths (LayoutUnit, 1/64 px) of DejaVu Sans, which it kerns by
        // the face's GPOS `kern` feature: "Tracker" bold 18 px, "Total" 12 px and
        // "To do" bold 13 px. Unkerned they are 77.03, 30.06 and 40.56 px.
        for (text, bold, size, chromium) in [
            ("Tracker", true, 18, 74.578125),
            ("Total", false, 12, 28.03125),
            ("To do", true, 13, 38.84375),
        ] {
            // Within 2/64 px: each advance here is rounded to 1/64 px, where
            // Chromium sums float advances and rounds the total up.
            let kerned = width_64(Typeface::DejaVu, web(bold), text, size);
            assert!(
                (kerned - (chromium * 64.0) as i64).abs() <= 2,
                "{text}: {kerned}"
            );
            assert!(width_64(Typeface::DejaVu, Style::from(bold), text, size) > kerned + 64);
        }
        assert_eq!(
            kern_units(Typeface::DejaVu, web(false), 'T', 'o'),
            Some((-348, 2048))
        );
        assert_eq!(kern_units(Typeface::DejaVu, false, 'T', 'o'), None);
        // Each face has its own pairs, the obliques included. The platform faces
        // stay unkerned: their files have no pairs for Chromium to apply either.
        let italic = Style::new(false, true, Lang::Auto).for_web();
        assert!(kern(Typeface::DejaVu, italic, 'T', 'o', 16) < 0);
        assert_ne!(
            kern(Typeface::DejaVu, italic, 'T', 'o', 16),
            kern(Typeface::DejaVu, web(false), 'T', 'o', 16)
        );
        for t in [
            Typeface::Inter,
            Typeface::OpenSans,
            Typeface::Ubuntu,
            Typeface::Roboto,
        ] {
            assert_eq!(kern(t, web(false), 'T', 'o', 16), 0, "{t:?}");
        }
        // DejaVu Sans Mono advances by 1233/2048 em (7.22 px at 12 px), where the
        // terminal grid gives it a whole 8 px cell; it has no pairs.
        assert_eq!(advance(Typeface::Mono, web(false), 'i', 12), 462);
        assert_eq!(advance(Typeface::Mono, false, 'i', 12), 8 * 64);
        assert_eq!(text_width(Typeface::Mono, web(false), "npm run", 12), 51);
        assert_eq!(kern(Typeface::Mono, web(false), 'T', 'o', 12), 0);
        // A symbol DejaVu draws beyond the tabulated ranges measures by the font
        // as web content (Chromium falls back to DejaVu Sans for it: 13 px "☎" is
        // 16.19 px, "⋮" 13 px) and at 0.6 em natively; in the monospace face, by
        // the mono advance.
        assert_eq!(advance(Typeface::DejaVu, web(false), '☎', 13), 1036);
        assert_eq!(advance(Typeface::DejaVu, web(false), '⋮', 13), 13 * 64);
        assert_eq!(advance(Typeface::Arimo, web(false), '⋮', 13), 13 * 64);
        assert_eq!(advance(Typeface::DejaVu, false, '☎', 13), 13 * 64 * 3 / 5);
        assert_eq!(advance(Typeface::Mono, web(false), '─', 12), 462);
        assert_eq!(
            table_face(Typeface::Mono, web(false), '─'),
            Some((Typeface::Mono, false))
        );
        assert_eq!(table_face(Typeface::DejaVu, Style::default(), '☎'), None);
        // Placement follows the same pen, so drawing and measuring agree.
        let laid = &crate::text::layout(Typeface::DejaVu, web(true), "Tracker", 18, 400)[0];
        assert_eq!(
            laid.width,
            width_64(Typeface::DejaVu, web(true), "Tracker", 18)
        );
        assert_eq!(
            laid.glyphs[1].x,
            advance(Typeface::DejaVu, true, 'T', 18)
                + kern(Typeface::DejaVu, web(true), 'T', 'r', 18)
        );
    }
    #[test]
    fn wrapping_and_truncation_respect_kerning() {
        let t = Typeface::Arimo;
        let text = "AVAVAVAV ToToToTo";
        let kerned = text_width(t, false, "AVAVAVAV", 16);
        let plain: i64 = "AVAVAVAV".chars().map(|c| advance(t, false, c, 16)).sum();
        assert!(i64::from(kerned) * 64 < plain - 64 * 4, "{kerned} {plain}");
        // Wide enough for the kerned words but not the unkerned ones: one word a line,
        // no mid-word break.
        let lines = wrap(
            t,
            false,
            text,
            16,
            kerned.max(text_width(t, false, "ToToToTo", 16)),
        );
        assert_eq!(lines, ["AVAVAVAV ", "ToToToTo"]);
        for line in wrap(t, false, "Talk To AV. Talk To AV. Talk To AV.", 16, 90) {
            assert!(text_width(t, false, line.trim_end(), 16) <= 90, "{line}");
        }
        // A word wider than the line breaks between characters by kerned widths.
        for line in wrap(t, false, "AVAVAVAVAVAVAVAV", 16, 50) {
            assert!(text_width(t, false, &line, 16) <= 50, "{line}");
        }
        let cut = ellipsize(t, false, "AVAVAVAVAVAVAVAVAVAV", 16, 100);
        assert!(
            cut.ends_with('…') && text_width(t, false, &cut, 16) <= 100,
            "{cut}"
        );
        // Layout places glyphs by the same kerned pen, so its width is the measure.
        let laid = &crate::text::layout(t, false, "Talk", 13, 200)[0];
        assert_eq!(laid.width, width_64(t, Style::default(), "Talk", 13));
        assert_eq!(
            laid.glyphs[1].x,
            advance(t, false, 'T', 13) + kern(t, false, 'T', 'a', 13)
        );
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
