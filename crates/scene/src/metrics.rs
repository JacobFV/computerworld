//! Deterministic UI text metrics for the bundled font families. Advances come from a
//! table generated from the exact font files the renderer embeds, so layout code can
//! measure, centre, wrap and truncate text without rasterizing or consulting a host.
use crate::metrics_data as data;
use serde::{Deserialize, Serialize};

/// Bundled proportional UI font family. Glyphs missing from a family fall back to DejaVu.
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
}
impl Typeface {
    pub fn is_default(&self) -> bool {
        *self == Self::DejaVu
    }
    fn table(self, bold: bool) -> (&'static [(u32, u16)], u32) {
        match (self, bold) {
            (Self::DejaVu, false) => (data::DEJAVU_REGULAR, data::DEJAVU_REGULAR_UPEM),
            (Self::DejaVu, true) => (data::DEJAVU_BOLD, data::DEJAVU_BOLD_UPEM),
            (Self::Inter, false) => (data::INTER_REGULAR, data::INTER_REGULAR_UPEM),
            (Self::Inter, true) => (data::INTER_BOLD, data::INTER_BOLD_UPEM),
            (Self::OpenSans, false) => (data::OPENSANS_REGULAR, data::OPENSANS_REGULAR_UPEM),
            (Self::OpenSans, true) => (data::OPENSANS_BOLD, data::OPENSANS_BOLD_UPEM),
            (Self::Ubuntu, false) => (data::UBUNTU_REGULAR, data::UBUNTU_REGULAR_UPEM),
            (Self::Ubuntu, true) => (data::UBUNTU_BOLD, data::UBUNTU_BOLD_UPEM),
            (Self::Roboto, false) => (data::ROBOTO_REGULAR, data::ROBOTO_REGULAR_UPEM),
            (Self::Roboto, true) => (data::ROBOTO_BOLD, data::ROBOTO_BOLD_UPEM),
        }
    }
    /// Whether the family itself (not the DejaVu fallback) supplies this character.
    pub fn covers(self, bold: bool, c: char) -> bool {
        lookup(self.table(bold).0, c).is_some()
    }
}
fn lookup(table: &[(u32, u16)], c: char) -> Option<u16> {
    table
        .binary_search_by_key(&(c as u32), |e| e.0)
        .ok()
        .map(|i| table[i].1)
}
/// Tabulated advance in 1/64 pixel, or `None` when only the rasterizer knows the glyph.
pub fn tabulated_advance(typeface: Typeface, bold: bool, c: char, size: u16) -> Option<i64> {
    let c = if c == '\t' { ' ' } else { c };
    let scale = |units: u16, upem: u32| {
        (i64::from(units) * i64::from(size) * 64 + i64::from(upem) / 2) / i64::from(upem)
    };
    let (table, upem) = typeface.table(bold);
    if let Some(units) = lookup(table, c) {
        return Some(scale(units, upem));
    }
    let (table, upem) = Typeface::DejaVu.table(bold);
    lookup(table, c).map(|units| scale(units, upem))
}
/// Advance in 1/64 pixel. Tabs advance four spaces; unknown glyphs use 0.6 em.
pub fn advance(typeface: Typeface, bold: bool, c: char, size: u16) -> i64 {
    let one = tabulated_advance(typeface, bold, c, size).unwrap_or(i64::from(size) * 64 * 3 / 5);
    if c == '\t' {
        one * 4
    } else {
        one
    }
}
fn width_64(typeface: Typeface, bold: bool, text: &str, size: u16) -> i64 {
    text.chars()
        .filter(|c| *c != '\r')
        .map(|c| advance(typeface, bold, c, size))
        .sum()
}
/// Pixel width of the widest line, rounded up.
pub fn text_width(typeface: Typeface, bold: bool, text: &str, size: u16) -> u32 {
    text.split('\n')
        .map(|line| (width_64(typeface, bold, line, size) + 63) / 64)
        .max()
        .unwrap_or(0) as u32
}
/// Greedy word wrapping shared by layout and rasterization. Newlines are preserved,
/// breaks fall after spaces, and words wider than a line break between characters.
pub fn wrap(typeface: Typeface, bold: bool, text: &str, size: u16, width: u32) -> Vec<String> {
    let limit = i64::from(width) * 64;
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut pen = 0i64;
        for word in paragraph.split_inclusive(' ') {
            let visible = width_64(typeface, bold, word.trim_end_matches(' '), size);
            if pen > 0 && pen + visible > limit {
                lines.push(std::mem::take(&mut line));
                pen = 0;
            }
            if visible > limit {
                for c in word.chars().filter(|c| *c != '\r') {
                    let a = advance(typeface, bold, c, size);
                    if pen > 0 && pen + a > limit {
                        lines.push(std::mem::take(&mut line));
                        pen = 0;
                    }
                    line.push(c);
                    pen += a;
                }
            } else {
                line.extend(word.chars().filter(|c| *c != '\r'));
                pen += width_64(typeface, bold, word, size);
            }
        }
        lines.push(line);
    }
    lines
}
/// Single-line truncation with a trailing ellipsis when `text` exceeds `width` pixels.
pub fn ellipsize(typeface: Typeface, bold: bool, text: &str, size: u16, width: u32) -> String {
    let text = text.split('\n').next().unwrap_or("");
    let limit = i64::from(width) * 64;
    if width_64(typeface, bold, text, size) <= limit {
        return text.into();
    }
    let ellipsis = advance(typeface, bold, '…', size);
    let mut out = String::new();
    let mut pen = 0;
    for c in text.chars() {
        let a = advance(typeface, bold, c, size);
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
}
