//! Text beyond DejaVu's coverage: the font fallback chain, bidirectional reordering,
//! OpenType shaping and script-aware line breaking. Layout code (through
//! [`crate::metrics`]) and the renderer both place glyphs with [`layout`], so what is
//! measured is exactly what is drawn.
//!
//! **Fallback chain.** For each character, in order: the scene's platform typeface,
//! DejaVu (regular or bold), then Noto Sans Hebrew, Arabic, Thai and Devanagari
//! (matching weight), Noto Sans SC (Han, kana, CJK punctuation), Noto Sans KR
//! (Hangul) and Noto Emoji. A character none of them maps draws DejaVu's `.notdef`.
//! Emoji sequences (VS16, ZWJ, skin-tone modifiers, keycaps, flags and tag sequences)
//! go to Noto Emoji as one cluster even when their base character is one DejaVu
//! draws as text; VS15 keeps the text glyph.
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
mod coverage;

use crate::metrics::{self, Typeface};
use std::ops::Range;
use std::sync::OnceLock;
use unicode_bidi::{Level, ParagraphBidiInfo};
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

/// A face beyond the table-driven platform/DejaVu faces.
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
    /// Noto Sans SC: common Han of GB 2312, Big5 level 1 and JIS X 0208, plus kana,
    /// CJK punctuation and fullwidth forms. Font pack.
    Han,
    /// Noto Sans KR: all 11,172 Hangul syllables and compatibility jamo. Font pack.
    Hangul,
    /// Noto Emoji (monochrome), with its ZWJ/flag/keycap/modifier ligatures. Font pack.
    Emoji,
}

macro_rules! font {
    ($name:literal) => {
        include_bytes!(concat!("../../../render/assets/fonts/", $name)) as &[u8]
    };
}

impl FaceId {
    pub const ALL: [FaceId; 11] = [
        Self::Hebrew,
        Self::HebrewBold,
        Self::Arabic,
        Self::ArabicBold,
        Self::Thai,
        Self::ThaiBold,
        Self::Devanagari,
        Self::DevanagariBold,
        Self::Han,
        Self::Hangul,
        Self::Emoji,
    ];
    /// Faces whose outlines ship in the separately fetched font pack.
    pub const PACK: [FaceId; 3] = [Self::Han, Self::Hangul, Self::Emoji];
    pub fn in_pack(self) -> bool {
        Self::PACK.contains(&self)
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
            Self::Han => "noto-sans-sc.ttf",
            Self::Hangul => "noto-sans-kr.ttf",
            Self::Emoji => "noto-emoji.ttf",
        }
    }
    /// Complete font bytes for the embedded faces; `None` for pack faces, whose
    /// outlines the renderer obtains separately.
    pub fn embedded_bytes(self) -> Option<&'static [u8]> {
        Some(match self {
            Self::Hebrew => font!("noto-hebrew-regular.ttf"),
            Self::HebrewBold => font!("noto-hebrew-bold.ttf"),
            Self::Arabic => font!("noto-arabic-regular.ttf"),
            Self::ArabicBold => font!("noto-arabic-bold.ttf"),
            Self::Thai => font!("noto-thai-regular.ttf"),
            Self::ThaiBold => font!("noto-thai-bold.ttf"),
            Self::Devanagari => font!("noto-devanagari-regular.ttf"),
            Self::DevanagariBold => font!("noto-devanagari-bold.ttf"),
            Self::Han | Self::Hangul | Self::Emoji => return None,
        })
    }
    /// Bytes layout shapes with: the face itself, or a pack face's outline-free stub.
    pub fn layout_bytes(self) -> &'static [u8] {
        match self {
            Self::Han => font!("stubs/noto-sans-sc.ttf"),
            Self::Hangul => font!("stubs/noto-sans-kr.ttf"),
            Self::Emoji => font!("stubs/noto-emoji.ttf"),
            _ => self.embedded_bytes().expect("embedded face"),
        }
    }
    fn shaper(self) -> &'static rustybuzz::Face<'static> {
        static SHAPERS: [OnceLock<rustybuzz::Face<'static>>; 11] = [const { OnceLock::new() }; 11];
        SHAPERS[self as usize].get_or_init(|| {
            rustybuzz::Face::from_slice(self.layout_bytes(), 0).expect("bundled face parses")
        })
    }
    /// Whether this face's cmap maps `c`.
    pub fn covers(self, c: char) -> bool {
        self.shaper().glyph_index(c).is_some()
    }
    fn weight(self, bold: bool) -> Self {
        match (self, bold) {
            (Self::Hebrew, true) => Self::HebrewBold,
            (Self::Arabic, true) => Self::ArabicBold,
            (Self::Thai, true) => Self::ThaiBold,
            (Self::Devanagari, true) => Self::DevanagariBold,
            (face, _) => face,
        }
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

/// A wrapped line: its logical text, its glyphs left to right, and its advance width
/// in 1/64 pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaidLine {
    pub text: String,
    pub glyphs: Vec<PlacedGlyph>,
    pub width: i64,
}

/// Wrap `text` to `width` pixels and place every glyph. Paragraphs split on `\n`; a
/// right-to-left paragraph (first strong character Hebrew or Arabic) is reordered
/// with that base direction on every one of its lines. Lines are left-aligned.
pub fn layout(typeface: Typeface, bold: bool, text: &str, size: u16, width: u32) -> Vec<LaidLine> {
    block(typeface, bold, text, size, width, true)
}

pub(crate) fn block(
    typeface: Typeface,
    bold: bool,
    text: &str,
    size: u16,
    width: u32,
    glyphs: bool,
) -> Vec<LaidLine> {
    let limit = i64::from(width) * 64;
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if is_simple(typeface, bold, paragraph) {
            let mut lines = Vec::new();
            metrics::wrap_simple_paragraph(typeface, bold, paragraph, size, limit, &mut lines);
            for line in lines {
                let mut placed = Vec::new();
                let mut pen = 0;
                for c in line.chars() {
                    if glyphs {
                        placed.push(PlacedGlyph {
                            face: None,
                            glyph: GlyphRef::Char(c),
                            x: pen,
                            y: 0,
                        });
                    }
                    pen += metrics::advance(typeface, bold, c, size);
                }
                out.push(LaidLine {
                    text: line,
                    glyphs: placed,
                    width: pen,
                });
            }
            continue;
        }
        let paragraph: String = paragraph.chars().filter(|c| *c != '\r').collect();
        let ctx = Ctx {
            typeface,
            bold,
            size,
            rtl: base_rtl(&paragraph),
        };
        for range in ctx.wrap(&paragraph, limit) {
            let line = &paragraph[range];
            let (width, placed) = ctx.line(line, glyphs);
            out.push(LaidLine {
                text: line.to_owned(),
                glyphs: placed,
                width,
            });
        }
    }
    out
}

/// Advance width of one line (no wrapping) in 1/64 pixel.
pub(crate) fn line_width(typeface: Typeface, bold: bool, line: &str, size: u16) -> i64 {
    let line: String = line.chars().filter(|c| *c != '\r').collect();
    let ctx = Ctx {
        typeface,
        bold,
        size,
        rtl: base_rtl(&line),
    };
    ctx.line(&line, false).0
}

/// Truncate one line at a cluster boundary so that it and a trailing ellipsis fit.
pub(crate) fn ellipsize(
    typeface: Typeface,
    bold: bool,
    line: &str,
    size: u16,
    width: u32,
) -> String {
    let line: String = line.chars().filter(|c| *c != '\r').collect();
    let limit = i64::from(width) * 64;
    let ctx = Ctx {
        typeface,
        bold,
        size,
        rtl: base_rtl(&line),
    };
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
pub fn is_simple(typeface: Typeface, bold: bool, text: &str) -> bool {
    text.chars().all(|c| !needs_complex(typeface, bold, c))
}

fn needs_complex(typeface: Typeface, bold: bool, c: char) -> bool {
    let u = c as u32;
    if u < 0x0590 {
        return false;
    }
    is_rtl(c)
        || is_ignorable(c)
        || is_emoji_component(c)
        || is_regional_indicator(c)
        || (!table_covers(typeface, bold, c) && script_face(bold, c).is_some())
}

fn table_covers(typeface: Typeface, bold: bool, c: char) -> bool {
    typeface.covers(bold, c) || dejavu_covers(bold, c)
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

/// The first non-table face in the fallback chain that maps `c`.
pub fn script_face(bold: bool, c: char) -> Option<FaceId> {
    let u = c as u32;
    let candidate = match u {
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => Some(FaceId::Hebrew),
        0x0600..=0x06FF | 0x0750..=0x077F | 0xFE70..=0xFEFF => Some(FaceId::Arabic),
        0x0E00..=0x0E7F => Some(FaceId::Thai),
        0x0900..=0x097F | 0xA8E0..=0xA8FF => Some(FaceId::Devanagari),
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => Some(FaceId::Hangul),
        _ => None,
    };
    if let Some(face) = candidate {
        let face = face.weight(bold);
        if face.covers(c) {
            return Some(face);
        }
    }
    [FaceId::Han, FaceId::Hangul, FaceId::Emoji]
        .into_iter()
        .find(|f| f.covers(c))
}

fn is_rtl(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}

/// Default-ignorable format characters: joiners, directional marks and controls,
/// variation selectors and tags. Zero width and never drawn on their own.
fn is_ignorable(c: char) -> bool {
    matches!(c as u32, 0x061C | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x2069 | 0xFE00..=0xFE0F | 0xFEFF | 0xE0000..=0xE0FFF)
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
fn is_emoji_component(c: char) -> bool {
    c == ZWJ || c == VS16 || c == KEYCAP || is_emoji_modifier(c) || is_tag(c)
}
fn is_mark(c: char) -> bool {
    c.general_category_group() == GeneralCategoryGroup::Mark
}

/// End (exclusive char index) of an emoji presentation sequence starting at `i`.
fn emoji_cluster(
    typeface: Typeface,
    bold: bool,
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
    if (c.is_ascii() && !keycap) || (table_covers(typeface, bold, c) && !forced) {
        return None;
    }
    Some(j)
}

/// Whether a cluster boundary falls before char `i`: never inside a base-plus-marks
/// cluster, an emoji sequence, a flag pair or a virama conjunct.
fn atom_boundary(chars: &[(usize, char)], i: usize) -> bool {
    if i == 0 || i >= chars.len() {
        return true;
    }
    let (p, c) = (chars[i - 1].1, chars[i].1);
    if is_mark(c) || is_emoji_component(c) || is_ignorable(c) || p == ZWJ {
        return false;
    }
    // Devanagari virama, Thai phinthu; Thai preposed vowels attach forward.
    if matches!(p as u32, 0x094D | 0x0E3A | 0x0E40..=0x0E44) {
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
    matches!(c as u32, 0x1100..=0x11FF | 0x2E80..=0x2FFF | 0x3000..=0x303F | 0x3040..=0x30FF | 0x3100..=0x31FF | 0x3200..=0x9FFF | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}
/// Kinsoku: characters a line must not start with.
fn no_break_before(c: char) -> bool {
    matches!(
        c,
        ')' | ']'
            | '}'
            | ','
            | '.'
            | ':'
            | ';'
            | '!'
            | '?'
            | '%'
            | '’'
            | '”'
            | '…'
            | '‥'
            | '、'
            | '。'
            | '，'
            | '．'
            | '：'
            | '；'
            | '！'
            | '？'
            | '）'
            | '］'
            | '｝'
            | '」'
            | '』'
            | '〕'
            | '】'
            | '〉'
            | '》'
            | '〙'
            | '〗'
            | '〟'
            | 'ー'
            | '々'
            | '〻'
            | 'ゝ'
            | 'ゞ'
            | 'ヽ'
            | 'ヾ'
            | '・'
            | '〜'
            | '～'
            | 'ぁ'
            | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'っ'
            | 'ゃ'
            | 'ゅ'
            | 'ょ'
            | 'ゎ'
            | 'ゕ'
            | 'ゖ'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ッ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ヮ'
            | 'ヵ'
            | 'ヶ'
            | '％'
    )
}
/// Kinsoku: characters a line must not end with.
fn no_break_after(c: char) -> bool {
    matches!(
        c,
        '(' | '['
            | '{'
            | '‘'
            | '“'
            | '（'
            | '［'
            | '｛'
            | '「'
            | '『'
            | '〔'
            | '【'
            | '〈'
            | '《'
            | '〘'
            | '〖'
            | '〝'
            | '$'
            | '£'
            | '¥'
            | '￥'
            | '＄'
    )
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
    bold: bool,
    size: u16,
    rtl: bool,
}

impl Ctx {
    fn scale(&self, units: i32, upem: i32) -> i64 {
        let n = i64::from(units) * i64::from(self.size) * 64;
        let d = i64::from(upem.max(1));
        if n >= 0 {
            (n + d / 2) / d
        } else {
            -((-n + d / 2) / d)
        }
    }

    /// Logical runs of one face. Emoji clusters stay separate runs so each shapes
    /// left to right even inside right-to-left text.
    fn itemize(&self, line: &str) -> Vec<(Range<usize>, Kind)> {
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let end = |k: usize| chars.get(k).map_or(line.len(), |x| x.0);
        let mut out: Vec<(Range<usize>, Kind)> = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let (b, c) = chars[i];
            if let Some(j) = emoji_cluster(self.typeface, self.bold, &chars, i) {
                out.push((b..end(j), Kind::Face(FaceId::Emoji)));
                i = j;
                continue;
            }
            let prev = out.last().map(|x| x.1);
            let joins = |f: FaceId| f != FaceId::Emoji && f.covers(c);
            let kind = match prev {
                Some(Kind::Face(f)) if (is_ignorable(c) || is_mark(c)) && joins(f) => Kind::Face(f),
                _ if is_ignorable(c) => Kind::Hidden,
                _ if table_covers(self.typeface, self.bold, c) => Kind::Table,
                _ => script_face(self.bold, c).map_or(Kind::Table, Kind::Face),
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

    /// Width and (optionally) glyphs of one line in visual order.
    fn line(&self, line: &str, glyphs: bool) -> (i64, Vec<PlacedGlyph>) {
        let mut out = Vec::new();
        let mut pen = 0i64;
        if line.is_empty() {
            return (0, out);
        }
        // UAX #9 rule L1 sends a line's trailing spaces to the paragraph's visual
        // end, which for a right-to-left paragraph is the left edge. They are
        // invisible, so keep them counted in the width but out of the way on the
        // right, where the left-aligned line would otherwise start with a gap.
        let trimmed = line.trim_end_matches(' ');
        if self.rtl && trimmed.len() < line.len() {
            let (width, placed) = self.line(trimmed, glyphs);
            let spaces = (line.len() - trimmed.len()) as i64;
            return (
                width + spaces * metrics::advance(self.typeface, self.bold, ' ', self.size),
                placed,
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
                let text = &line[range];
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
                            pen += metrics::advance(self.typeface, self.bold, c, self.size);
                        };
                        if rtl {
                            text.chars().rev().for_each(&mut place);
                        } else {
                            text.chars().for_each(&mut place);
                        }
                    }
                    Kind::Face(face) => {
                        let shaper = face.shaper();
                        let upem = shaper.units_per_em();
                        let mut buffer = rustybuzz::UnicodeBuffer::new();
                        buffer.push_str(text);
                        buffer.set_direction(if rtl && face != FaceId::Emoji {
                            rustybuzz::Direction::RightToLeft
                        } else {
                            rustybuzz::Direction::LeftToRight
                        });
                        buffer.guess_segment_properties();
                        // Joiners and selectors do their work during shaping and
                        // then leave no glyph behind.
                        buffer.set_flags(rustybuzz::BufferFlags::REMOVE_DEFAULT_IGNORABLES);
                        let shaped = rustybuzz::shape(shaper, &[], buffer);
                        for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions())
                        {
                            if glyphs {
                                out.push(PlacedGlyph {
                                    face: Some(face),
                                    glyph: GlyphRef::Index(info.glyph_id as u16),
                                    x: pen + self.scale(pos.x_offset, upem),
                                    y: self.scale(pos.y_offset, upem),
                                });
                            }
                            pen += self.scale(pos.x_advance, upem);
                        }
                    }
                }
            }
        }
        (pen, out)
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

#[cfg(test)]
mod tests;
