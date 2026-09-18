//! Text beyond DejaVu's coverage: the font fallback chain, bidirectional reordering,
//! OpenType shaping and script-aware line breaking. Layout code (through
//! [`crate::metrics`]) and the renderer both place glyphs with [`layout`], so what is
//! measured is exactly what is drawn.
//!
//! **Fallback chain.** For each character, in order: the scene's platform typeface,
//! DejaVu (regular or bold; italic text tries the italic faces of both first), then
//! the Noto script faces (matching weight): Hebrew, Arabic, Thai, Lao, Devanagari,
//! Bengali, Gurmukhi, Tamil, Khmer, Georgian and Armenian embedded, and Gujarati,
//! Ethiopic, Myanmar and Sinhala from the font pack; Noto Sans SC (Han, kana, CJK
//! punctuation) with its Traditional Chinese, Japanese and Korean locale faces;
//! Noto Sans KR (Hangul) and Noto Emoji. A character none of them maps draws
//! DejaVu's `.notdef`. Emoji sequences (VS16, ZWJ, skin-tone modifiers, keycaps,
//! flags and tag sequences) go to Noto Emoji as one cluster even when their base
//! character is one DejaVu draws as text; VS15 keeps the text glyph.
//!
//! **Language.** [`Style::lang`] picks the regional forms of Han: Simplified Chinese
//! (Noto Sans SC), Traditional Chinese, Japanese or Korean. `Lang::Auto` infers it per
//! paragraph from the text ([`infer_lang`]).
//!
//! **Determinism.** Everything here is integer arithmetic over embedded bytes:
//! `rustybuzz` positions in font units, advances are scaled to 1/64 pixel with the
//! same rounding the advance table uses, and no host font, locale or clock is read.
//! The CJK and emoji faces are an on-demand pack for the Wasm build; layout shapes
//! their outline-free *stubs* (same cmap, advances and GSUB, no outlines), which are
//! always embedded, so layout never depends on whether the pack has been fetched.
//!
//! **Fast path.** A paragraph with nothing outside the platform/DejaVu tables and no
//! right-to-left, joiner or emoji-sequence characters is measured and placed exactly as
//! before this module existed: per-character tabulated advances, breaks after spaces.
//! Latin output is therefore unchanged by construction.
#[rustfmt::skip]
mod coverage;
#[rustfmt::skip]
mod han;
pub mod terminal;
#[rustfmt::skip]
mod wide;

use crate::metrics::{self, Typeface};
pub use crate::metrics::{Lang, Style};
use std::ops::Range;
use std::sync::OnceLock;
use unicode_bidi::{Level, ParagraphBidiInfo};
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

/// A face beyond the table-driven platform/DejaVu faces. The embedded faces come
/// first, in `EMBEDDED` order; the rest are the font pack (see [`FaceId::PACK`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FaceId {
    Hebrew,
    HebrewBold,
    Arabic,
    ArabicBold,
    Thai,
    ThaiBold,
    Devanagari,
    DevanagariBold,
    Bengali,
    BengaliBold,
    Georgian,
    GeorgianBold,
    Armenian,
    ArmenianBold,
    Tamil,
    TamilBold,
    Gurmukhi,
    GurmukhiBold,
    Lao,
    LaoBold,
    Khmer,
    KhmerBold,
    /// Noto Sans SC: common Han of GB 2312, Big5 level 1 and JIS X 0208, plus kana,
    /// CJK punctuation and fullwidth forms. Font pack.
    Han,
    /// Noto Sans KR: all 11,172 Hangul syllables and compatibility jamo. Font pack.
    Hangul,
    /// Noto Emoji (monochrome), with its ZWJ/flag/keycap/modifier ligatures. Font
    /// pack. Layout always shapes emoji with this face's stub; see `ColorEmoji`.
    Emoji,
    /// `Han` at weight 700: same glyph order, cmap and advances. Font pack.
    HanBold,
    /// `Hangul` at weight 700. Font pack.
    HangulBold,
    /// Noto Sans TC: the Big5 level 1 Han whose Traditional Chinese forms differ from
    /// `Han`'s. A character it lacks is the same drawing in `Han`. Font pack.
    HanTc,
    HanTcBold,
    /// Noto Sans JP: the JIS X 0208 level 1 kanji whose Japanese forms differ. Pack.
    HanJp,
    HanJpBold,
    /// Noto Sans KR: the KS X 1001 hanja whose Korean forms differ. Font pack.
    HanKr,
    HanKrBold,
    /// Font-pack scripts, regular weight only.
    Gujarati,
    Ethiopic,
    Myanmar,
    Sinhala,
    /// Noto Color Emoji (COLRv1). Never chosen by layout: the renderer draws it in
    /// place of each `Emoji` cluster when it is installed.
    ColorEmoji,
}

macro_rules! font {
    ($name:literal) => {
        include_bytes!(concat!("../../../render/assets/fonts/", $name)) as &[u8]
    };
}
/// Font bytes live in statics, not constants: a constant is materialized at each
/// use site that survives inlining, which embedded every face twice in the Wasm.
static EMBEDDED: [&[u8]; 22] = [
    font!("noto-hebrew-regular.ttf"),
    font!("noto-hebrew-bold.ttf"),
    font!("noto-arabic-regular.ttf"),
    font!("noto-arabic-bold.ttf"),
    font!("noto-thai-regular.ttf"),
    font!("noto-thai-bold.ttf"),
    font!("noto-devanagari-regular.ttf"),
    font!("noto-devanagari-bold.ttf"),
    font!("noto-bengali-regular.ttf"),
    font!("noto-bengali-bold.ttf"),
    font!("noto-georgian-regular.ttf"),
    font!("noto-georgian-bold.ttf"),
    font!("noto-armenian-regular.ttf"),
    font!("noto-armenian-bold.ttf"),
    font!("noto-tamil-regular.ttf"),
    font!("noto-tamil-bold.ttf"),
    font!("noto-gurmukhi-regular.ttf"),
    font!("noto-gurmukhi-bold.ttf"),
    font!("noto-lao-regular.ttf"),
    font!("noto-lao-bold.ttf"),
    font!("noto-khmer-regular.ttf"),
    font!("noto-khmer-bold.ttf"),
];
/// Outline-free twins of the pack faces, which layout shapes. Bold pack faces share
/// their regular twin's stub; `ColorEmoji` is never shaped by layout.
static STUBS: [(FaceId, &[u8]); 10] = [
    (FaceId::Han, font!("stubs/noto-sans-sc.ttf")),
    (FaceId::Hangul, font!("stubs/noto-sans-kr.ttf")),
    (FaceId::Emoji, font!("stubs/noto-emoji.ttf")),
    (FaceId::HanTc, font!("stubs/noto-sans-tc.ttf")),
    (FaceId::HanJp, font!("stubs/noto-sans-jp.ttf")),
    (FaceId::HanKr, font!("stubs/noto-sans-kr-han.ttf")),
    (FaceId::Gujarati, font!("stubs/noto-gujarati.ttf")),
    (FaceId::Ethiopic, font!("stubs/noto-ethiopic.ttf")),
    (FaceId::Myanmar, font!("stubs/noto-myanmar.ttf")),
    (FaceId::Sinhala, font!("stubs/noto-sinhala.ttf")),
];

impl FaceId {
    pub const COUNT: usize = 38;
    /// Every face, in declaration order (`ALL[i] as usize == i`).
    pub const ALL: [FaceId; Self::COUNT] = [
        Self::Hebrew,
        Self::HebrewBold,
        Self::Arabic,
        Self::ArabicBold,
        Self::Thai,
        Self::ThaiBold,
        Self::Devanagari,
        Self::DevanagariBold,
        Self::Bengali,
        Self::BengaliBold,
        Self::Georgian,
        Self::GeorgianBold,
        Self::Armenian,
        Self::ArmenianBold,
        Self::Tamil,
        Self::TamilBold,
        Self::Gurmukhi,
        Self::GurmukhiBold,
        Self::Lao,
        Self::LaoBold,
        Self::Khmer,
        Self::KhmerBold,
        Self::Han,
        Self::Hangul,
        Self::Emoji,
        Self::HanBold,
        Self::HangulBold,
        Self::HanTc,
        Self::HanTcBold,
        Self::HanJp,
        Self::HanJpBold,
        Self::HanKr,
        Self::HanKrBold,
        Self::Gujarati,
        Self::Ethiopic,
        Self::Myanmar,
        Self::Sinhala,
        Self::ColorEmoji,
    ];
    /// Faces whose outlines ship in the separately fetched font pack.
    pub const PACK: [FaceId; 16] = [
        Self::Han,
        Self::Hangul,
        Self::Emoji,
        Self::HanBold,
        Self::HangulBold,
        Self::HanTc,
        Self::HanTcBold,
        Self::HanJp,
        Self::HanJpBold,
        Self::HanKr,
        Self::HanKrBold,
        Self::Gujarati,
        Self::Ethiopic,
        Self::Myanmar,
        Self::Sinhala,
        Self::ColorEmoji,
    ];
    pub fn in_pack(self) -> bool {
        self as usize >= EMBEDDED.len()
    }
    /// File name under `crates/render/assets/fonts/` (and `pack/` for pack faces).
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Hebrew => "noto-hebrew-regular.ttf",
            Self::HebrewBold => "noto-hebrew-bold.ttf",
            Self::Arabic => "noto-arabic-regular.ttf",
            Self::ArabicBold => "noto-arabic-bold.ttf",
            Self::Thai => "noto-thai-regular.ttf",
            Self::ThaiBold => "noto-thai-bold.ttf",
            Self::Devanagari => "noto-devanagari-regular.ttf",
            Self::DevanagariBold => "noto-devanagari-bold.ttf",
            Self::Bengali => "noto-bengali-regular.ttf",
            Self::BengaliBold => "noto-bengali-bold.ttf",
            Self::Georgian => "noto-georgian-regular.ttf",
            Self::GeorgianBold => "noto-georgian-bold.ttf",
            Self::Armenian => "noto-armenian-regular.ttf",
            Self::ArmenianBold => "noto-armenian-bold.ttf",
            Self::Tamil => "noto-tamil-regular.ttf",
            Self::TamilBold => "noto-tamil-bold.ttf",
            Self::Gurmukhi => "noto-gurmukhi-regular.ttf",
            Self::GurmukhiBold => "noto-gurmukhi-bold.ttf",
            Self::Lao => "noto-lao-regular.ttf",
            Self::LaoBold => "noto-lao-bold.ttf",
            Self::Khmer => "noto-khmer-regular.ttf",
            Self::KhmerBold => "noto-khmer-bold.ttf",
            Self::Han => "noto-sans-sc.ttf",
            Self::Hangul => "noto-sans-kr.ttf",
            Self::Emoji => "noto-emoji.ttf",
            Self::HanBold => "noto-sans-sc-bold.ttf",
            Self::HangulBold => "noto-sans-kr-bold.ttf",
            Self::HanTc => "noto-sans-tc.ttf",
            Self::HanTcBold => "noto-sans-tc-bold.ttf",
            Self::HanJp => "noto-sans-jp.ttf",
            Self::HanJpBold => "noto-sans-jp-bold.ttf",
            Self::HanKr => "noto-sans-kr-han.ttf",
            Self::HanKrBold => "noto-sans-kr-han-bold.ttf",
            Self::Gujarati => "noto-gujarati.ttf",
            Self::Ethiopic => "noto-ethiopic.ttf",
            Self::Myanmar => "noto-myanmar.ttf",
            Self::Sinhala => "noto-sinhala.ttf",
            Self::ColorEmoji => "noto-color-emoji.ttf",
        }
    }
    /// Complete font bytes for the embedded faces; `None` for pack faces, whose
    /// outlines the renderer obtains separately.
    pub fn embedded_bytes(self) -> Option<&'static [u8]> {
        EMBEDDED.get(self as usize).copied()
    }
    /// The face whose layout (glyph order, cmap, advances) this one shares: a bold
    /// pack face lays out with its regular twin's stub.
    pub fn layout_twin(self) -> Self {
        match self {
            Self::HanBold => Self::Han,
            Self::HangulBold => Self::Hangul,
            Self::HanTcBold => Self::HanTc,
            Self::HanJpBold => Self::HanJp,
            Self::HanKrBold => Self::HanKr,
            Self::ColorEmoji => Self::Emoji,
            face => face,
        }
    }
    /// Bytes layout shapes with: the face itself, or a pack face's outline-free stub.
    pub fn layout_bytes(self) -> &'static [u8] {
        if let Some(bytes) = self.embedded_bytes() {
            return bytes;
        }
        let twin = self.layout_twin();
        STUBS
            .iter()
            .find(|(face, _)| *face == twin)
            .map(|(_, bytes)| *bytes)
            .expect("every pack face has a stub")
    }
    pub(crate) fn shaper(self) -> &'static rustybuzz::Face<'static> {
        static SHAPERS: [OnceLock<rustybuzz::Face<'static>>; FaceId::COUNT] =
            [const { OnceLock::new() }; FaceId::COUNT];
        let twin = self.layout_twin();
        SHAPERS[twin as usize].get_or_init(|| {
            rustybuzz::Face::from_slice(twin.layout_bytes(), 0).expect("bundled face parses")
        })
    }
    /// Whether this face's cmap maps `c`.
    pub fn covers(self, c: char) -> bool {
        self.shaper().glyph_index(c).is_some()
    }
    /// The bold twin, for faces that have one (the pack scripts do not).
    pub fn weight(self, bold: bool) -> Self {
        if !bold {
            return self;
        }
        match self {
            Self::Hebrew => Self::HebrewBold,
            Self::Arabic => Self::ArabicBold,
            Self::Thai => Self::ThaiBold,
            Self::Devanagari => Self::DevanagariBold,
            Self::Bengali => Self::BengaliBold,
            Self::Georgian => Self::GeorgianBold,
            Self::Armenian => Self::ArmenianBold,
            Self::Tamil => Self::TamilBold,
            Self::Gurmukhi => Self::GurmukhiBold,
            Self::Lao => Self::LaoBold,
            Self::Khmer => Self::KhmerBold,
            Self::Han => Self::HanBold,
            Self::Hangul => Self::HangulBold,
            Self::HanTc => Self::HanTcBold,
            Self::HanJp => Self::HanJpBold,
            Self::HanKr => Self::HanKrBold,
            face => face,
        }
    }
    /// Whether this is a Han face (Simplified or a locale face, either weight).
    pub fn is_han(self) -> bool {
        matches!(
            self.layout_twin(),
            Self::Han | Self::HanTc | Self::HanJp | Self::HanKr
        )
    }
}

/// What to rasterize: a character of the table-driven face (the renderer resolves
/// platform face versus DejaVu exactly as for plain Latin text), or a shaped glyph id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GlyphRef {
    Char(char),
    Index(u16),
}

/// One glyph of a laid-out line. `face` is `None` for the table-driven face.
/// `x` is the pen position from the line's left edge and `y` a baseline offset (up
/// is positive), both in 1/64 pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedGlyph {
    pub face: Option<FaceId>,
    pub glyph: GlyphRef,
    pub x: i64,
    pub y: i64,
}

/// One emoji presentation sequence of a laid-out line: its text (a byte range of
/// [`LaidLine::text`]) and the pen span, in 1/64 pixel, that its monochrome glyphs
/// occupy. A renderer with a colour emoji face draws the cluster there instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmojiSpan {
    pub text: Range<usize>,
    pub x0: i64,
    pub x1: i64,
}

/// A wrapped line: its logical text, its glyphs left to right, and its advance width
/// in 1/64 pixel (as `metrics::text_width` measures it, before rounding up).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaidLine {
    pub text: String,
    pub glyphs: Vec<PlacedGlyph>,
    pub width: i64,
    /// Emoji clusters of this line, in visual order.
    pub emoji: Vec<EmojiSpan>,
}

/// Wrap `text` to `width` pixels and place every glyph. Paragraphs split on `\n`; a
/// right-to-left paragraph (first strong character Hebrew or Arabic) is reordered
/// with that base direction on every one of its lines. Lines are left-aligned.
pub fn layout(
    typeface: Typeface,
    style: impl Into<Style>,
    text: &str,
    size: u16,
    width: u32,
) -> Vec<LaidLine> {
    block(typeface, style.into(), text, size, width, true)
}

pub(crate) fn block(
    typeface: Typeface,
    style: Style,
    text: &str,
    size: u16,
    width: u32,
    glyphs: bool,
) -> Vec<LaidLine> {
    let limit = i64::from(width) * 64;
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if is_simple(typeface, style, paragraph) {
            let mut lines = Vec::new();
            metrics::wrap_simple_paragraph(typeface, style, paragraph, size, limit, &mut lines);
            for line in lines {
                // `metrics::wrap` only needs the line text; skip placement for it.
                let mut placed = Vec::new();
                let mut pen = 0;
                for c in line.chars().filter(|_| glyphs) {
                    placed.push(PlacedGlyph {
                        face: None,
                        glyph: GlyphRef::Char(c),
                        x: pen,
                        y: 0,
                    });
                    pen += metrics::advance(typeface, style, c, size);
                }
                out.push(LaidLine {
                    text: line,
                    glyphs: placed,
                    width: pen,
                    emoji: Vec::new(),
                });
            }
            continue;
        }
        let paragraph: String = paragraph.chars().filter(|c| *c != '\r').collect();
        let ctx = Ctx::new(typeface, style, size, &paragraph);
        for range in ctx.wrap(&paragraph, limit) {
            let line = &paragraph[range];
            let (width, placed, emoji) = ctx.line(line, glyphs);
            out.push(LaidLine {
                text: line.to_owned(),
                glyphs: placed,
                width,
                emoji,
            });
        }
    }
    out
}

/// Advance width of one line (no wrapping) in 1/64 pixel.
pub(crate) fn line_width(typeface: Typeface, style: Style, line: &str, size: u16) -> i64 {
    let line: String = line.chars().filter(|c| *c != '\r').collect();
    Ctx::new(typeface, style, size, &line).line(&line, false).0
}

/// Truncate one line at a cluster boundary so that it and a trailing ellipsis fit.
pub(crate) fn ellipsize(
    typeface: Typeface,
    style: Style,
    line: &str,
    size: u16,
    width: u32,
) -> String {
    let line: String = line.chars().filter(|c| *c != '\r').collect();
    let limit = i64::from(width) * 64;
    let ctx = Ctx::new(typeface, style, size, &line);
    if ctx.line(&line, false).0 <= limit {
        return line;
    }
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut best = String::from("…");
    for i in 1..chars.len() {
        if !atom_boundary(&chars, i) {
            continue;
        }
        let mut candidate = line[..chars[i].0].trim_end().to_owned();
        candidate.push('…');
        if ctx.line(&candidate, false).0 > limit {
            break;
        }
        best = candidate;
    }
    best
}

/// Whether a paragraph takes the original table-only path (see the module docs).
pub fn is_simple(typeface: Typeface, style: impl Into<Style>, text: &str) -> bool {
    let style = style.into();
    text.chars().all(|c| !needs_complex(typeface, style, c))
}

fn needs_complex(typeface: Typeface, style: Style, c: char) -> bool {
    // Everything below Armenian is Latin, Greek or Cyrillic: table faces only.
    if (c as u32) < 0x0530 {
        return false;
    }
    is_rtl(c)
        || is_ignorable(c)
        || is_emoji_component(c)
        || is_regional_indicator(c)
        || (!table_covers(typeface, style, c) && script_face(style, c).is_some())
}

/// Whether the table-driven faces draw `c`: the platform family or DejaVu, in the
/// style's slant, or upright DejaVu (which italic text falls back to).
fn table_covers(typeface: Typeface, style: Style, c: char) -> bool {
    metrics::table_face(typeface, style, c).is_some() || dejavu_covers(style.bold, c)
}

/// Whether the embedded DejaVu sans face (regular or bold) maps `c`.
pub fn dejavu_covers(bold: bool, c: char) -> bool {
    in_ranges(
        if bold {
            coverage::DEJAVU_SANS_BOLD
        } else {
            coverage::DEJAVU_SANS
        },
        c,
    )
}

/// Whether the embedded DejaVu Sans Oblique face (regular or bold) maps `c`.
pub fn dejavu_oblique_covers(bold: bool, c: char) -> bool {
    in_ranges(
        if bold {
            coverage::DEJAVU_SANS_BOLD_OBLIQUE
        } else {
            coverage::DEJAVU_SANS_OBLIQUE
        },
        c,
    )
}

/// East Asian Wide or Fullwidth (UAX #11): two cells in a terminal.
pub fn is_wide(c: char) -> bool {
    in_ranges(wide::WIDE, c)
}

/// Whether the embedded DejaVu Sans Mono face maps `c`.
pub fn dejavu_mono_covers(c: char) -> bool {
    in_ranges(coverage::DEJAVU_MONO, c)
}

fn in_ranges(ranges: &[(u32, u32)], c: char) -> bool {
    let u = c as u32;
    ranges
        .binary_search_by(|&(a, b)| {
            if b < u {
                std::cmp::Ordering::Less
            } else if a > u {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// The script face whose block contains `c`, before weight and coverage checks.
fn block_face(c: char) -> Option<FaceId> {
    Some(match c as u32 {
        0x0530..=0x058F | 0xFB13..=0xFB17 => FaceId::Armenian,
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => FaceId::Hebrew,
        0x0600..=0x06FF | 0x0750..=0x077F | 0xFE70..=0xFEFF => FaceId::Arabic,
        0x0900..=0x097F | 0xA8E0..=0xA8FF => FaceId::Devanagari,
        0x0980..=0x09FF => FaceId::Bengali,
        0x0A00..=0x0A7F => FaceId::Gurmukhi,
        0x0A80..=0x0AFF => FaceId::Gujarati,
        0x0B80..=0x0BFF => FaceId::Tamil,
        0x0D80..=0x0DFF => FaceId::Sinhala,
        0x0E00..=0x0E7F => FaceId::Thai,
        0x0E80..=0x0EFF => FaceId::Lao,
        0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F => FaceId::Myanmar,
        0x10A0..=0x10FF | 0x1C90..=0x1CBF | 0x2D00..=0x2D2F => FaceId::Georgian,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => FaceId::Hangul,
        0x1200..=0x139F | 0x2D80..=0x2DDF => FaceId::Ethiopic,
        0x1780..=0x17FF | 0x19E0..=0x19FF => FaceId::Khmer,
        _ => return None,
    })
}

/// The first non-table face in the fallback chain that maps `c`, with Han in the
/// forms of `style.lang` (Simplified Chinese when `Auto`).
pub fn script_face(style: impl Into<Style>, c: char) -> Option<FaceId> {
    let style = style.into();
    face_for(style.bold, style.lang, c)
}

fn face_for(bold: bool, lang: Lang, c: char) -> Option<FaceId> {
    if let Some(face) = block_face(c) {
        if face.covers(c) {
            return Some(face.weight(bold));
        }
    }
    let face = [FaceId::Han, FaceId::Hangul, FaceId::Emoji]
        .into_iter()
        .find(|f| f.covers(c))?;
    if face != FaceId::Han {
        return Some(face.weight(bold));
    }
    let local = match lang {
        Lang::ZhHant => Some(FaceId::HanTc),
        Lang::Ja => Some(FaceId::HanJp),
        Lang::Ko => Some(FaceId::HanKr),
        Lang::Auto | Lang::ZhHans => None,
    };
    Some(
        local
            .filter(|f| f.covers(c))
            .unwrap_or(FaceId::Han)
            .weight(bold),
    )
}

/// The language of text with no tag, as far as Han forms go: kana makes it
/// Japanese, Hangul Korean, a Traditional-only character (in Big5 level 1 but not
/// GB 2312) Traditional Chinese; anything else is Simplified Chinese.
pub fn infer_lang(text: &str) -> Lang {
    let mut traditional = false;
    for c in text.chars() {
        match c as u32 {
            0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9D => return Lang::Ja,
            0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => return Lang::Ko,
            u @ 0x4E00..=0x9FFF => {
                let i = (u - 0x4E00) as usize;
                traditional |= han::TRADITIONAL_ONLY[i / 64] >> (i % 64) & 1 == 1;
            }
            _ => {}
        }
    }
    if traditional {
        Lang::ZhHant
    } else {
        Lang::ZhHans
    }
}

pub(crate) fn is_rtl(c: char) -> bool {
    matches!(
        c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF
    )
}

/// Default-ignorable format characters: joiners, directional marks and controls,
/// variation selectors and tags. Zero width and never drawn on their own.
pub(crate) fn is_ignorable(c: char) -> bool {
    matches!(
        c as u32,
        0x061C
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x2069
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xE0000..=0xE0FFF
    )
}

const ZWJ: char = '\u{200D}';
const VS15: char = '\u{FE0E}';
const VS16: char = '\u{FE0F}';
const KEYCAP: char = '\u{20E3}';

fn is_emoji_modifier(c: char) -> bool {
    matches!(c as u32, 0x1F3FB..=0x1F3FF)
}
fn is_tag(c: char) -> bool {
    matches!(c as u32, 0xE0020..=0xE007F)
}
fn is_regional_indicator(c: char) -> bool {
    matches!(c as u32, 0x1F1E6..=0x1F1FF)
}
/// Characters that only ever extend an emoji sequence.
pub(crate) fn is_emoji_component(c: char) -> bool {
    c == ZWJ || c == VS16 || c == KEYCAP || is_emoji_modifier(c) || is_tag(c)
}
pub(crate) fn is_mark(c: char) -> bool {
    c.general_category_group() == GeneralCategoryGroup::Mark
}
/// Characters that belong to the script around them rather than their own block:
/// the Indic dandas are encoded once, in Devanagari, and used by every Indic script.
fn is_shared_punctuation(c: char) -> bool {
    matches!(c as u32, 0x0964 | 0x0965)
}

/// End (exclusive char index) of an emoji presentation sequence starting at `i`.
/// `text_face` says whether the text face (the table faces, or the terminal's
/// monospace face) draws a character, which keeps a bare symbol it has as text.
pub(crate) fn emoji_cluster(
    text_face: &dyn Fn(char) -> bool,
    chars: &[(usize, char)],
    i: usize,
) -> Option<usize> {
    let c = chars[i].1;
    if c == ZWJ || c == VS16 || c == KEYCAP || is_tag(c) || !FaceId::Emoji.covers(c) {
        return None;
    }
    let next = |k: usize| chars.get(k).map(|x| x.1);
    if next(i + 1) == Some(VS15) {
        return None;
    }
    if is_regional_indicator(c) {
        // Flags are pairs; an unpaired indicator is drawn alone.
        return Some(if next(i + 1).is_some_and(is_regional_indicator) {
            i + 2
        } else {
            i + 1
        });
    }
    let mut j = i + 1;
    let mut keycap = false;
    loop {
        match next(j) {
            Some(d) if d == VS16 || is_emoji_modifier(d) || is_tag(d) || d == KEYCAP => {
                keycap |= d == KEYCAP;
                j += 1;
            }
            Some(ZWJ)
                if next(j + 1).is_some_and(|e| {
                    FaceId::Emoji.covers(e) && !is_emoji_component(e) && !e.is_ascii()
                }) =>
            {
                j += 2
            }
            _ => break,
        }
    }
    let forced = j > i + 1;
    if (c.is_ascii() && !keycap) || (text_face(c) && !forced) {
        return None;
    }
    Some(j)
}

/// Whether a cluster boundary falls before char `i`: never inside a base-plus-marks
/// cluster, an emoji sequence, a flag pair or a virama conjunct.
pub(crate) fn atom_boundary(chars: &[(usize, char)], i: usize) -> bool {
    if i == 0 || i >= chars.len() {
        return true;
    }
    let (p, c) = (chars[i - 1].1, chars[i].1);
    if is_mark(c) || is_emoji_component(c) || is_ignorable(c) || p == ZWJ {
        return false;
    }
    // Viramas (Devanagari, Bengali, Gurmukhi, Gujarati, Tamil, Sinhala, Myanmar,
    // Khmer coeng), Thai phinthu; Thai and Lao preposed vowels attach forward.
    if matches!(
        p as u32,
        0x094D
            | 0x09CD
            | 0x0A4D
            | 0x0ACD
            | 0x0BCD
            | 0x0DCA
            | 0x1039
            | 0x103A
            | 0x17D2
            | 0x0E3A
            | 0x0E40..=0x0E44
            | 0x0EC0..=0x0EC4
    ) {
        return false;
    }
    if is_regional_indicator(p) && is_regional_indicator(c) {
        let run = chars[..i]
            .iter()
            .rev()
            .take_while(|x| is_regional_indicator(x.1))
            .count();
        return run % 2 == 0;
    }
    true
}

/// Characters around which lines may break without a space (UAX #14 class ID and
/// friends): Han, kana, Hangul, CJK punctuation and fullwidth forms.
fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x1100..=0x11FF
            | 0x2E80..=0x2FFF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3100..=0x31FF
            | 0x3200..=0x9FFF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7FF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFFEF
            | 0x20000..=0x3FFFF
    )
}
/// Kinsoku: characters a line must not start with (closing brackets and punctuation,
/// small kana, iteration and prolonged-sound marks).
fn no_break_before(c: char) -> bool {
    ")]},.:;!?%’”…‥、。，．：；！？）］｝」』〕】〉》〙〗〟ー々〻ゝゞヽヾ・〜～％\
     ぁぃぅぇぉっゃゅょゎゕゖァィゥェォッャュョヮヵヶ"
        .contains(c)
}
/// Kinsoku: characters a line must not end with (opening brackets, currency signs).
fn no_break_after(c: char) -> bool {
    "([{‘“（［｛「『〔【〈《〘〖〝$£¥￥＄".contains(c)
}
/// A line may break before char `i`: after a space (as for Latin text), or between
/// CJK characters subject to kinsoku, and never inside a cluster.
fn break_before(chars: &[(usize, char)], i: usize) -> bool {
    let (p, c) = (chars[i - 1].1, chars[i].1);
    if p == ' ' {
        return true;
    }
    if c == ' ' || !atom_boundary(chars, i) {
        return false;
    }
    (is_cjk(p) || is_cjk(c)) && !no_break_before(c) && !no_break_after(p)
}

fn base_rtl(paragraph: &str) -> bool {
    matches!(
        unicode_bidi::get_base_direction(paragraph),
        unicode_bidi::Direction::Rtl
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Table,
    Face(FaceId),
    Hidden,
}

struct Ctx {
    typeface: Typeface,
    style: Style,
    size: u16,
    rtl: bool,
    /// `style.lang`, or the language inferred from the paragraph when that is `Auto`.
    lang: Lang,
}

impl Ctx {
    fn new(typeface: Typeface, style: Style, size: u16, paragraph: &str) -> Self {
        Self {
            typeface,
            style,
            size,
            rtl: base_rtl(paragraph),
            lang: if style.lang.is_auto() {
                infer_lang(paragraph)
            } else {
                style.lang
            },
        }
    }

    fn scale(&self, units: i32, upem: i32) -> i64 {
        scale(units, upem, self.size)
    }

    /// Logical runs of one face. Emoji clusters stay separate runs so each shapes
    /// left to right even inside right-to-left text.
    fn itemize(&self, line: &str) -> Vec<(Range<usize>, Kind)> {
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let end = |k: usize| chars.get(k).map_or(line.len(), |x| x.0);
        let text_face = |c: char| table_covers(self.typeface, self.style, c);
        let mut out: Vec<(Range<usize>, Kind)> = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let (b, c) = chars[i];
            if let Some(j) = emoji_cluster(&text_face, &chars, i) {
                out.push((b..end(j), Kind::Face(FaceId::Emoji)));
                i = j;
                continue;
            }
            let prev = out.last().map(|x| x.1);
            let joins = |f: FaceId| f != FaceId::Emoji && f.covers(c);
            let kind = match prev {
                Some(Kind::Face(f))
                    if (is_ignorable(c) || is_mark(c) || is_shared_punctuation(c)) && joins(f) =>
                {
                    Kind::Face(f)
                }
                _ if is_ignorable(c) => Kind::Hidden,
                _ if text_face(c) => Kind::Table,
                _ => face_for(self.style.bold, self.lang, c).map_or(Kind::Table, Kind::Face),
            };
            match out.last_mut() {
                Some((range, k)) if *k == kind && kind != Kind::Face(FaceId::Emoji) => {
                    range.end = end(i + 1)
                }
                _ => out.push((b..end(i + 1), kind)),
            }
            i += 1;
        }
        out
    }

    /// Width, glyphs (when `glyphs`) and emoji spans of one line in visual order.
    fn line(&self, line: &str, glyphs: bool) -> (i64, Vec<PlacedGlyph>, Vec<EmojiSpan>) {
        let mut out = Vec::new();
        let mut emoji = Vec::new();
        let mut pen = 0i64;
        if line.is_empty() {
            return (0, out, emoji);
        }
        // UAX #9 rule L1 sends a line's trailing spaces to the paragraph's visual
        // end, which for a right-to-left paragraph is the left edge. They are
        // invisible, so keep them counted in the width but out of the way on the
        // right, where the left-aligned line would otherwise start with a gap.
        let trimmed = line.trim_end_matches(' ');
        if self.rtl && trimmed.len() < line.len() {
            let (width, placed, emoji) = self.line(trimmed, glyphs);
            let spaces = (line.len() - trimmed.len()) as i64;
            return (
                width + spaces * metrics::advance(self.typeface, self.style, ' ', self.size),
                placed,
                emoji,
            );
        }
        let items = self.itemize(line);
        let level = if self.rtl { Level::rtl() } else { Level::ltr() };
        let info = ParagraphBidiInfo::new(line, Some(level));
        let (levels, runs) = info.visual_runs(0..line.len());
        for run in runs {
            let rtl = levels[run.start].is_rtl();
            let mut pieces: Vec<(Range<usize>, Kind)> = items
                .iter()
                .filter_map(|(r, k)| {
                    let (a, b) = (r.start.max(run.start), r.end.min(run.end));
                    (a < b).then_some((a..b, *k))
                })
                .collect();
            if rtl {
                pieces.reverse();
            }
            for (range, kind) in pieces {
                let text = &line[range.clone()];
                match kind {
                    Kind::Hidden => {}
                    Kind::Table => {
                        let mut place = |c: char| {
                            let drawn = if rtl {
                                unicode_bidi_mirroring::get_mirrored(c).unwrap_or(c)
                            } else {
                                c
                            };
                            if glyphs {
                                out.push(PlacedGlyph {
                                    face: None,
                                    glyph: GlyphRef::Char(drawn),
                                    x: pen,
                                    y: 0,
                                });
                            }
                            pen += metrics::advance(self.typeface, self.style, c, self.size);
                        };
                        if rtl {
                            text.chars().rev().for_each(&mut place);
                        } else {
                            text.chars().for_each(&mut place);
                        }
                    }
                    Kind::Face(face) => {
                        let start = pen;
                        for g in shape(face, text, rtl && face != FaceId::Emoji) {
                            if glyphs {
                                out.push(PlacedGlyph {
                                    face: Some(face),
                                    glyph: GlyphRef::Index(g.glyph),
                                    x: pen + self.scale(g.x_offset, g.upem),
                                    y: self.scale(g.y_offset, g.upem),
                                });
                            }
                            pen += self.scale(g.x_advance, g.upem);
                        }
                        if glyphs && face == FaceId::Emoji {
                            emoji.push(EmojiSpan {
                                text: range,
                                x0: start,
                                x1: pen,
                            });
                        }
                    }
                }
            }
        }
        (pen, out, emoji)
    }

    fn fits(&self, text: &str, limit: i64) -> bool {
        self.line(text.trim_end_matches(' '), false).0 <= limit
    }

    /// Greedy wrapping over break opportunities, measuring each candidate line exactly
    /// as it will be laid out. A unit wider than the line breaks between clusters.
    fn wrap(&self, paragraph: &str, limit: i64) -> Vec<Range<usize>> {
        let chars: Vec<(usize, char)> = paragraph.char_indices().collect();
        let mut stops: Vec<usize> = (1..chars.len())
            .filter(|&i| break_before(&chars, i))
            .map(|i| chars[i].0)
            .collect();
        stops.push(paragraph.len());
        let mut lines = Vec::new();
        let mut start = 0;
        let mut fit = 0;
        for stop in stops {
            if self.fits(&paragraph[start..stop], limit) {
                fit = stop;
                continue;
            }
            if fit > start {
                lines.push(start..fit);
                start = fit;
                if self.fits(&paragraph[start..stop], limit) {
                    fit = stop;
                    continue;
                }
            }
            // A single unit wider than the line: break between clusters, keeping at
            // least one cluster on every line.
            let from = start;
            let bounds: Vec<usize> = (0..chars.len())
                .filter(|&i| chars[i].0 > from && chars[i].0 < stop && atom_boundary(&chars, i))
                .map(|i| chars[i].0)
                .chain([stop])
                .collect();
            let mut last = start;
            for b in bounds {
                if self.fits(&paragraph[start..b], limit) {
                    last = b;
                    continue;
                }
                if last > start {
                    lines.push(start..last);
                    start = last;
                }
                if !self.fits(&paragraph[start..b], limit) {
                    lines.push(start..b);
                    start = b;
                }
                last = b;
            }
            fit = stop;
        }
        if start < paragraph.len() || lines.is_empty() {
            lines.push(start..paragraph.len());
        }
        lines
    }
}

/// The mirrored form of a bracket-like character in right-to-left text (UAX #9
/// rule L4), or the character itself.
pub fn mirrored(c: char) -> char {
    unicode_bidi_mirroring::get_mirrored(c).unwrap_or(c)
}

/// Font units scaled to 1/64 pixel at `size`, rounding half away from zero (the
/// advance table's rounding).
pub fn scale(units: i32, upem: i32, size: u16) -> i64 {
    let n = i64::from(units) * i64::from(size) * 64;
    let d = i64::from(upem.max(1));
    if n >= 0 {
        (n + d / 2) / d
    } else {
        -((-n + d / 2) / d)
    }
}

/// One shaped glyph in font units, with the byte offset of its cluster in the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shaped {
    pub glyph: u16,
    pub cluster: u32,
    pub x_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub upem: i32,
}

/// Shape `text` with `face` (its layout stub for pack faces), in visual order.
pub fn shape(face: FaceId, text: &str, rtl: bool) -> Vec<Shaped> {
    shape_with(face.shaper(), text, rtl)
}

/// Shape `text` with any parsed face, in visual order. Joiners and selectors do
/// their work during shaping and then leave no glyph behind.
pub fn shape_with(shaper: &rustybuzz::Face<'_>, text: &str, rtl: bool) -> Vec<Shaped> {
    let upem = shaper.units_per_em();
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(if rtl {
        rustybuzz::Direction::RightToLeft
    } else {
        rustybuzz::Direction::LeftToRight
    });
    buffer.guess_segment_properties();
    buffer.set_flags(rustybuzz::BufferFlags::REMOVE_DEFAULT_IGNORABLES);
    let shaped = rustybuzz::shape(shaper, &[], buffer);
    shaped
        .glyph_infos()
        .iter()
        .zip(shaped.glyph_positions())
        .map(|(info, pos)| Shaped {
            glyph: info.glyph_id as u16,
            cluster: info.cluster,
            x_advance: pos.x_advance,
            x_offset: pos.x_offset,
            y_offset: pos.y_offset,
            upem,
        })
        .collect()
}

#[cfg(test)]
mod tests;
