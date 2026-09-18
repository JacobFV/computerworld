//! Fixed-pitch terminal text (`Primitive::Text`): character cells, wrapping, bidi and
//! shaping inside a cell grid, the way modern terminals (VTE/GNOME Terminal, with
//! its default implicit bidi mode) lay text out.
//!
//! * **Cells.** Text is split into clusters: a base character with its combining
//!   marks, joiners and selectors attached (never a cell of its own), or a whole
//!   emoji sequence. East Asian Wide and Fullwidth characters and emoji take two
//!   cells (UAX #11, as `wcwidth` reports); everything else takes one.
//! * **Bidi.** Each row is a left-to-right paragraph (VTE's default: implicit mode,
//!   no direction autodetection); right-to-left runs are reversed cluster by cluster
//!   within it, and brackets in them mirror.
//! * **Shaping.** Runs of a Noto script face are shaped whole, so Arabic letters
//!   take their joining forms and marks attach; each cluster's glyphs are then drawn
//!   inside its own cells.
//!
//! A row of monospace-face characters that are one cell wide, with no marks or
//! right-to-left text, is exactly the original one-character-per-cell grid.
use super::{
    atom_boundary, emoji_cluster, face_for, infer_lang, is_emoji_component, is_ignorable, is_mark,
    is_rtl, is_wide, scale, shape, FaceId, GlyphRef, Lang, PlacedGlyph,
};
use std::ops::Range;
use unicode_bidi::{Level, ParagraphBidiInfo};

/// Whether `c` joins the cluster before it instead of starting a cell.
fn attaches(c: char) -> bool {
    is_mark(c)
        || is_ignorable(c)
        || is_emoji_component(c)
        // Conjoining Hangul medial vowels and final consonants.
        || matches!(c as u32, 0x1160..=0x11FF | 0xD7B0..=0xD7FF)
}

/// Cells a character starts: 2 for East Asian Wide/Fullwidth, 0 for one that
/// attaches to the character before it, otherwise 1.
pub fn char_width(c: char) -> u32 {
    if attaches(c) {
        0
    } else if is_wide(c) {
        2
    } else {
        1
    }
}

/// Whether a row is the plain grid: every character one cell of the monospace face.
pub fn is_simple(line: &str) -> bool {
    line.chars().all(|c| {
        (c as u32) < 0x0300
            || (!is_rtl(c)
                && !attaches(c)
                && !is_wide(c)
                && !matches!(c as u32, 0x1F1E6..=0x1F1FF)
                && (super::dejavu_mono_covers(c) || face_for(false, Lang::Auto, c).is_none()))
    })
}

/// A cluster in logical order.
#[derive(Clone, Debug)]
struct Unit {
    text: Range<usize>,
    width: u32,
    face: Option<FaceId>,
}

fn units(line: &str, lang: Lang) -> Vec<Unit> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let end = |k: usize| chars.get(k).map_or(line.len(), |x| x.0);
    let mut out: Vec<Unit> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (b, c) = chars[i];
        if let Some(j) = emoji_cluster(&super::dejavu_mono_covers, &chars, i) {
            out.push(Unit {
                text: b..end(j),
                width: 2,
                face: Some(FaceId::Emoji),
            });
            i = j;
            continue;
        }
        match out.last_mut() {
            Some(unit) if attaches(c) || !atom_boundary(&chars, i) => unit.text.end = end(i + 1),
            _ => out.push(Unit {
                text: b..end(i + 1),
                width: if is_wide(c) { 2 } else { 1 },
                face: if super::dejavu_mono_covers(c) {
                    None
                } else {
                    face_for(false, lang, c)
                },
            }),
        }
        i += 1;
    }
    out
}

/// Cells one line occupies.
pub fn columns(line: &str) -> usize {
    if is_simple(line) {
        return line.chars().count();
    }
    units(line, Lang::Auto)
        .iter()
        .map(|u| u.width as usize)
        .sum()
}

/// Byte offset and starting cell of every cluster of a line, in logical order: the
/// places a caret can stand, and the cells in front of each.
pub fn cluster_columns(line: &str) -> Vec<(usize, usize)> {
    let mut col = 0;
    let mut out = Vec::new();
    if is_simple(line) {
        for (i, _) in line.char_indices() {
            out.push((i, col));
            col += 1;
        }
        return out;
    }
    for unit in units(line, Lang::Auto) {
        out.push((unit.text.start, col));
        col += unit.width as usize;
    }
    out
}

/// The byte offset of the first cluster at or past `column` cells, if any.
pub fn byte_at_column(line: &str, column: usize) -> Option<usize> {
    cluster_columns(line)
        .into_iter()
        .find(|&(_, col)| col >= column)
        .map(|(byte, _)| byte)
}

/// Wrap one line (no `\n`) into rows of at most `max_columns` cells, as byte
/// ranges. A cluster is never split; one wider than a row still gets a row.
pub fn wrap(line: &str, max_columns: usize) -> Vec<Range<usize>> {
    let max = max_columns.max(1);
    // An empty line is one empty row: the final push below.
    let mut rows = Vec::new();
    let (mut start, mut used) = (0usize, 0usize);
    let mut push = |range: Range<usize>, width: usize| {
        if used > 0 && used + width > max {
            rows.push(start..range.start);
            start = range.start;
            used = 0;
        }
        used += width;
    };
    if is_simple(line) {
        for (b, c) in line.char_indices() {
            push(b..b + c.len_utf8(), 1);
        }
    } else {
        for unit in units(line, infer_lang(line)) {
            push(unit.text.clone(), unit.width as usize);
        }
    }
    rows.push(start..line.len());
    rows
}

/// One cluster of a laid-out terminal row, in visual order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermCluster {
    /// Byte range of the cluster in the row's text.
    pub text: Range<usize>,
    /// Visual column of its first cell.
    pub col: u32,
    /// Cells it covers (a ligature that swallowed its neighbour covers both).
    pub width: u32,
    /// Whether it sits in a right-to-left run (monospace characters mirror).
    pub rtl: bool,
    /// `None`: each character is drawn by the monospace face at the cluster's cell.
    pub face: Option<FaceId>,
    /// Shaped glyphs of a `face` cluster: `x` from the cluster's own pen origin and
    /// `y` up from the baseline, in 1/64 pixel at the row's size.
    pub glyphs: Vec<PlacedGlyph>,
    /// Total advance of `glyphs`, 1/64 pixel.
    pub advance: i64,
}

/// Lay out one terminal row at `size` pixels: clusters in visual order with their
/// cells and, for script-face clusters, their shaped glyphs.
pub fn layout_line(line: &str, size: u16) -> Vec<TermCluster> {
    let units = units(line, infer_lang(line));
    let info = ParagraphBidiInfo::new(line, Some(Level::ltr()));
    let levels = info.reordered_levels(0..line.len());
    let level_at = |byte: usize| levels.get(byte).copied().unwrap_or(Level::ltr());
    let mut clusters: Vec<(TermCluster, Level)> = units
        .iter()
        .map(|u| {
            let level = level_at(u.text.start);
            (
                TermCluster {
                    text: u.text.clone(),
                    col: 0,
                    width: u.width,
                    rtl: level.is_rtl(),
                    face: u.face,
                    glyphs: Vec::new(),
                    advance: 0,
                },
                level,
            )
        })
        .collect();
    // Shape maximal runs of one face and one level, then hand each glyph to the
    // cluster its shaping cluster starts in.
    let mut i = 0;
    while i < clusters.len() {
        let Some(face) = clusters[i].0.face else {
            i += 1;
            continue;
        };
        let level = clusters[i].1;
        let mut j = i + 1;
        while face != FaceId::Emoji
            && j < clusters.len()
            && clusters[j].0.face == Some(face)
            && clusters[j].1 == level
        {
            j += 1;
        }
        let start = clusters[i].0.text.start;
        let text = &line[start..clusters[j - 1].0.text.end];
        let rtl = level.is_rtl() && face != FaceId::Emoji;
        for g in shape(face, text, rtl) {
            let at = start + g.cluster as usize;
            let k = (i..j)
                .rev()
                .find(|&k| clusters[k].0.text.start <= at)
                .unwrap_or(i);
            let cluster = &mut clusters[k].0;
            cluster.glyphs.push(PlacedGlyph {
                face: Some(face),
                glyph: GlyphRef::Index(g.glyph),
                x: cluster.advance + scale(g.x_offset, g.upem, size),
                y: scale(g.y_offset, g.upem, size),
            });
            cluster.advance += scale(g.x_advance, g.upem, size);
        }
        // A cluster whose glyphs were all taken by a ligature with the one before
        // it (Arabic lam-alef) lends that ligature its cells.
        let mut k = j;
        while k > i + 1 {
            k -= 1;
            if clusters[k].0.glyphs.is_empty() {
                let (merged, _) = clusters.remove(k);
                let owner = &mut clusters[k - 1].0;
                owner.width += merged.width;
                owner.text.end = owner.text.end.max(merged.text.end);
                j -= 1;
            }
        }
        i = j;
    }
    // UAX #9 L2 over clusters: from the highest level down to the lowest odd one,
    // reverse every maximal run at or above it.
    let max = clusters.iter().map(|c| c.1.number()).max().unwrap_or(0);
    let min_odd = clusters
        .iter()
        .map(|c| c.1.number())
        .filter(|l| l % 2 == 1)
        .min();
    if let Some(min_odd) = min_odd {
        for level in (min_odd..=max).rev() {
            let mut k = 0;
            while k < clusters.len() {
                if clusters[k].1.number() >= level {
                    let run = k;
                    while k < clusters.len() && clusters[k].1.number() >= level {
                        k += 1;
                    }
                    clusters[run..k].reverse();
                } else {
                    k += 1;
                }
            }
        }
    }
    let mut col = 0;
    clusters
        .into_iter()
        .map(|(mut c, _)| {
            c.col = col;
            col += c.width;
            c
        })
        .collect()
}
