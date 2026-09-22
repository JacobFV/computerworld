//! Cell and range addresses in A1 notation.
use serde::{Deserialize, Serialize};

/// Grid limits of the XLSX format.
pub const MAX_ROWS: u32 = 1_048_576;
pub const MAX_COLS: u32 = 16_384;

/// A zero-based cell position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Cell {
    pub row: u32,
    pub col: u32,
}
impl Cell {
    pub const fn new(row: u32, col: u32) -> Self {
        Self { row, col }
    }
    /// `A1`-style name.
    pub fn a1(self) -> String {
        format!("{}{}", column_name(self.col), self.row + 1)
    }
    /// Parse `B7` or `$B$7` (the dollars are ignored).
    pub fn parse(text: &str) -> Option<Self> {
        let r = CellRef::parse(text)?;
        Some(r.cell())
    }
}
/// Column letters: 0 is `A`, 25 is `Z`, 26 is `AA`.
pub fn column_name(mut col: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}
/// Inverse of [`column_name`], case-insensitive.
pub fn column_index(name: &str) -> Option<u32> {
    if name.is_empty() || name.len() > 3 {
        return None;
    }
    let mut n: u32 = 0;
    for c in name.chars() {
        let c = c.to_ascii_uppercase();
        if !c.is_ascii_uppercase() {
            return None;
        }
        n = n * 26 + (c as u32 - 'A' as u32 + 1);
    }
    (1..=MAX_COLS).contains(&n).then(|| n - 1)
}

/// A reference as a formula spells it: the target and which parts are absolute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellRef {
    pub row: u32,
    pub col: u32,
    pub row_abs: bool,
    pub col_abs: bool,
}
impl CellRef {
    pub fn relative(cell: Cell) -> Self {
        Self {
            row: cell.row,
            col: cell.col,
            row_abs: false,
            col_abs: false,
        }
    }
    pub fn cell(self) -> Cell {
        Cell::new(self.row, self.col)
    }
    pub fn parse(text: &str) -> Option<Self> {
        let b = text.as_bytes();
        let mut i = 0;
        let col_abs = b.first() == Some(&b'$');
        if col_abs {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        let col = column_index(&text[start..i])?;
        let row_abs = b.get(i) == Some(&b'$');
        if row_abs {
            i += 1;
        }
        let digits = &text[i..];
        if digits.is_empty()
            || !digits.bytes().all(|c| c.is_ascii_digit())
            || digits.starts_with('0')
        {
            return None;
        }
        let row: u32 = digits.parse().ok()?;
        (1..=MAX_ROWS).contains(&row).then_some(Self {
            row: row - 1,
            col,
            row_abs,
            col_abs,
        })
    }
    pub fn a1(self) -> String {
        format!(
            "{}{}{}{}",
            if self.col_abs { "$" } else { "" },
            column_name(self.col),
            if self.row_abs { "$" } else { "" },
            self.row + 1
        )
    }
    /// The reference as it reads after being copied `dr` rows and `dc` columns away:
    /// relative parts move, absolute ones stay. `None` when it would leave the grid.
    pub fn shifted(self, dr: i64, dc: i64) -> Option<Self> {
        let row = if self.row_abs {
            i64::from(self.row)
        } else {
            i64::from(self.row) + dr
        };
        let col = if self.col_abs {
            i64::from(self.col)
        } else {
            i64::from(self.col) + dc
        };
        if row < 0 || col < 0 || row >= i64::from(MAX_ROWS) || col >= i64::from(MAX_COLS) {
            return None;
        }
        Some(Self {
            row: row as u32,
            col: col as u32,
            ..self
        })
    }
}

/// A rectangle of cells, `start` top-left and `end` bottom-right inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Range {
    pub start: Cell,
    pub end: Cell,
}
impl Range {
    /// The rectangle spanned by two corners in any order.
    pub fn new(a: Cell, b: Cell) -> Self {
        Self {
            start: Cell::new(a.row.min(b.row), a.col.min(b.col)),
            end: Cell::new(a.row.max(b.row), a.col.max(b.col)),
        }
    }
    pub fn single(c: Cell) -> Self {
        Self { start: c, end: c }
    }
    pub fn rows(&self) -> u32 {
        self.end.row - self.start.row + 1
    }
    pub fn cols(&self) -> u32 {
        self.end.col - self.start.col + 1
    }
    pub fn contains(&self, c: Cell) -> bool {
        (self.start.row..=self.end.row).contains(&c.row)
            && (self.start.col..=self.end.col).contains(&c.col)
    }
    pub fn is_single(&self) -> bool {
        self.start == self.end
    }
    /// Cells row by row.
    pub fn cells(&self) -> impl Iterator<Item = Cell> + '_ {
        (self.start.row..=self.end.row)
            .flat_map(move |r| (self.start.col..=self.end.col).map(move |c| Cell::new(r, c)))
    }
    pub fn a1(&self) -> String {
        if self.is_single() {
            self.start.a1()
        } else {
            format!("{}:{}", self.start.a1(), self.end.a1())
        }
    }
    /// Parse `A1`, `A1:C3` (dollars ignored), `A:C` or `2:5`.
    pub fn parse(text: &str) -> Option<Self> {
        let clean = text.replace('$', "");
        match clean.split_once(':') {
            None => Cell::parse(&clean).map(Self::single),
            Some((a, b)) => {
                if let (Some(x), Some(y)) = (Cell::parse(a), Cell::parse(b)) {
                    return Some(Self::new(x, y));
                }
                if let (Some(x), Some(y)) = (column_index(a), column_index(b)) {
                    return Some(Self::new(Cell::new(0, x), Cell::new(MAX_ROWS - 1, y)));
                }
                let (x, y): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
                if x == 0 || y == 0 || x > MAX_ROWS || y > MAX_ROWS {
                    return None;
                }
                Some(Self::new(
                    Cell::new(x - 1, 0),
                    Cell::new(y - 1, MAX_COLS - 1),
                ))
            }
        }
    }
    pub fn intersect(&self, other: &Range) -> Option<Range> {
        let top = self.start.row.max(other.start.row);
        let left = self.start.col.max(other.start.col);
        let bottom = self.end.row.min(other.end.row);
        let right = self.end.col.min(other.end.col);
        (top <= bottom && left <= right)
            .then(|| Range::new(Cell::new(top, left), Cell::new(bottom, right)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn columns_and_cells_name_themselves_like_excel() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(16_383), "XFD");
        assert_eq!(column_index("xfd"), Some(16_383));
        assert_eq!(column_index("XFE"), None);
        assert_eq!(Cell::parse("B7"), Some(Cell::new(6, 1)));
        assert_eq!(CellRef::parse("$B7").unwrap().a1(), "$B7");
        assert_eq!(CellRef::parse("A0"), None);
        assert_eq!(Range::parse("C3:A1").unwrap().a1(), "A1:C3");
        assert_eq!(Range::parse("B:B").unwrap().rows(), MAX_ROWS);
    }
    #[test]
    fn copying_moves_only_relative_parts() {
        let r = CellRef::parse("A$1").unwrap();
        assert_eq!(r.shifted(5, 2).unwrap().a1(), "C$1");
        assert_eq!(CellRef::parse("A1").unwrap().shifted(-1, 0), None);
    }
}
