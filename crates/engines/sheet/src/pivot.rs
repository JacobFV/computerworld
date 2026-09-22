//! Pivot tables: a summary of a source range (a list with a header row) laid out on a
//! sheet by row fields, a column field, value fields with an aggregate each, and
//! report filters. Like Excel's, the report is written into ordinary cells, which
//! formulas can read; it is rebuilt by a refresh (automatically after every change
//! for the products that do that, Google Sheets and Numbers).
use crate::address::{Cell, Range};
use crate::value::{compare, Value};
use crate::workbook::{Input, Style, Workbook};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Agg {
    Sum,
    Count,
    Average,
    Max,
    Min,
}
impl Agg {
    pub const ALL: [Agg; 5] = [Agg::Sum, Agg::Count, Agg::Average, Agg::Max, Agg::Min];
    /// SpreadsheetML's `subtotal` and OpenDocument's `table:function`.
    pub fn name(self) -> &'static str {
        match self {
            Agg::Sum => "sum",
            Agg::Count => "count",
            Agg::Average => "average",
            Agg::Max => "max",
            Agg::Min => "min",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Agg::ALL
            .into_iter()
            .find(|a| a.name().eq_ignore_ascii_case(s))
    }
}
/// Whose captions the report uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PivotStyle {
    #[default]
    Excel,
    Calc,
    Sheets,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pivot {
    pub name: String,
    /// The list it summarises: a header row naming the fields, then records.
    pub source_sheet: String,
    pub source: Range,
    /// Top-left of the report body (report filters sit above it).
    pub at: Cell,
    /// Fields by their column in the source (0 is its first column).
    pub rows: Vec<usize>,
    /// At most one column field.
    pub cols: Vec<usize>,
    pub values: Vec<(usize, Agg)>,
    pub filters: Vec<usize>,
    /// Items each field hides, as they are shown.
    #[serde(default)]
    pub hidden: BTreeMap<usize, BTreeSet<String>>,
    #[serde(default)]
    pub style: PivotStyle,
    /// Rebuilt after every change to the workbook, as Sheets and Numbers do; Excel and
    /// Calc wait for Refresh.
    #[serde(default)]
    pub auto: bool,
    /// The cells the last refresh wrote.
    #[serde(default)]
    pub extent: Option<Range>,
}
/// Where an accumulator sits: a row key prefix and a column item (none for totals).
type Slot = (Vec<Item>, Option<Vec<Item>>);
/// A summarised value's key: the value, compared as a spreadsheet sorts.
#[derive(Clone, Debug, PartialEq)]
struct Item(Value);
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        // Blanks sort last, as "(blank)" does in every product.
        match (self.0.is_empty(), o.0.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => compare(&self.0, &o.0),
        }
    }
}
/// How an item is shown, and the name a filter hides it by.
pub fn item_label(v: &Value) -> String {
    match v {
        Value::Empty => "(blank)".into(),
        other => other.display(),
    }
}
#[derive(Default, Clone)]
struct Acc {
    sum: f64,
    numbers: usize,
    count: usize,
    max: Option<f64>,
    min: Option<f64>,
}
impl Acc {
    fn add(&mut self, v: &Value) {
        if !v.is_empty() {
            self.count += 1;
        }
        if let Value::Number(n) = v {
            self.sum += n;
            self.numbers += 1;
            self.max = Some(self.max.map_or(*n, |m| m.max(*n)));
            self.min = Some(self.min.map_or(*n, |m| m.min(*n)));
        }
    }
    fn result(&self, agg: Agg) -> Value {
        match agg {
            Agg::Sum => Value::number(self.sum),
            Agg::Count => Value::Number(self.count as f64),
            Agg::Average if self.numbers == 0 => Value::Error(crate::value::ErrorKind::Div0),
            Agg::Average => Value::number(self.sum / self.numbers as f64),
            Agg::Max => Value::number(self.max.unwrap_or(0.0)),
            Agg::Min => Value::number(self.min.unwrap_or(0.0)),
        }
    }
}
/// The finished report: rows of values from the top-left of its extent, and which of
/// them are headings (drawn bold).
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub cells: Vec<Vec<Value>>,
    pub bold: BTreeSet<(usize, usize)>,
    /// The row the body starts on, below the report filters.
    pub body_row: usize,
}
impl Pivot {
    pub fn caption(&self, field: &str, agg: Agg) -> String {
        match self.style {
            PivotStyle::Excel => {
                let a = match agg {
                    Agg::Sum => "Sum",
                    Agg::Count => "Count",
                    Agg::Average => "Average",
                    Agg::Max => "Max",
                    Agg::Min => "Min",
                };
                format!("{a} of {field}")
            }
            PivotStyle::Calc => {
                let a = match agg {
                    Agg::Sum => "Sum",
                    Agg::Count => "Count",
                    Agg::Average => "Average",
                    Agg::Max => "Max",
                    Agg::Min => "Min",
                };
                format!("{a} - {field}")
            }
            PivotStyle::Sheets => {
                let a = match agg {
                    Agg::Sum => "SUM",
                    Agg::Count => "COUNTA",
                    Agg::Average => "AVERAGE",
                    Agg::Max => "MAX",
                    Agg::Min => "MIN",
                };
                format!("{a} of {field}")
            }
        }
    }
    fn total_label(&self) -> &'static str {
        match self.style {
            PivotStyle::Calc => "Total Result",
            _ => "Grand Total",
        }
    }
    /// The cells of the whole report, from the top-left of its filters.
    pub fn build(&self, wb: &Workbook) -> Result<Report, String> {
        let si = wb
            .sheet_index(&self.source_sheet)
            .ok_or_else(|| format!("the source sheet {} is gone", self.source_sheet))?;
        let names = fields(wb, si, self.source)?;
        let field = |f: usize| names.get(f).cloned().unwrap_or_default();
        let width = self.source.cols() as usize;
        // Records, after the report filters and hidden items.
        let mut records: Vec<Vec<Value>> = Vec::new();
        'rows: for r in self.source.start.row + 1..=self.source.end.row {
            let rec: Vec<Value> = (0..width)
                .map(|k| wb.value(si, Cell::new(r, self.source.start.col + k as u32)))
                .collect();
            for (f, hidden) in &self.hidden {
                if rec.get(*f).is_some_and(|v| hidden.contains(&item_label(v))) {
                    continue 'rows;
                }
            }
            records.push(rec);
        }
        let key = |rec: &Vec<Value>, fs: &[usize]| -> Vec<Item> {
            fs.iter()
                .map(|f| Item(rec.get(*f).cloned().unwrap_or_default()))
                .collect()
        };
        let col_items: BTreeSet<Vec<Item>> = records.iter().map(|r| key(r, &self.cols)).collect();
        let col_items: Vec<Vec<Item>> = if self.cols.is_empty() {
            vec![]
        } else {
            col_items.into_iter().collect()
        };
        // Accumulate by (row prefix, column item or none for the total).
        let nv = self.values.len();
        let mut acc: BTreeMap<Slot, Vec<Acc>> = BTreeMap::new();
        let mut row_keys: BTreeSet<Vec<Item>> = BTreeSet::new();
        for rec in &records {
            let rk = key(rec, &self.rows);
            let ck = (!self.cols.is_empty()).then(|| key(rec, &self.cols));
            row_keys.insert(rk.clone());
            for depth in 0..=rk.len() {
                let prefix = rk[..depth].to_vec();
                for c in [ck.clone(), None] {
                    let slot = acc
                        .entry((prefix.clone(), c.clone()))
                        .or_insert_with(|| vec![Acc::default(); nv]);
                    for (i, (f, _)) in self.values.iter().enumerate() {
                        slot[i].add(rec.get(*f).unwrap_or(&Value::Empty));
                    }
                    if ck.is_none() {
                        break;
                    }
                }
            }
        }
        let value_at = |prefix: &[Item], c: Option<&Vec<Item>>, i: usize| -> Value {
            match acc.get(&(prefix.to_vec(), c.cloned())) {
                Some(a) => a[i].result(self.values[i].1),
                None => Value::Empty,
            }
        };
        let mut rep = Report::default();
        // Report filters.
        if self.style != PivotStyle::Sheets {
            for f in &self.filters {
                let items: BTreeSet<Item> = (self.source.start.row + 1..=self.source.end.row)
                    .map(|r| Item(wb.value(si, Cell::new(r, self.source.start.col + *f as u32))))
                    .collect();
                let hidden = self.hidden.get(f).cloned().unwrap_or_default();
                let shown: Vec<&Item> = items
                    .iter()
                    .filter(|i| !hidden.contains(&item_label(&i.0)))
                    .collect();
                let all = if self.style == PivotStyle::Calc {
                    "- all -"
                } else {
                    "(All)"
                };
                let sel = if hidden.is_empty() {
                    Value::Text(all.into())
                } else if shown.len() == 1 {
                    shown[0].0.clone()
                } else {
                    Value::Text("(Multiple Items)".into())
                };
                rep.bold.insert((rep.cells.len(), 0));
                rep.cells.push(vec![Value::Text(field(*f)), sel]);
            }
            if !self.filters.is_empty() {
                rep.cells.push(vec![]);
            }
        }
        rep.body_row = rep.cells.len();
        let tabular = self.rows.len() > 1 || self.style != PivotStyle::Excel;
        let label_cols = if self.rows.is_empty() {
            0
        } else if tabular {
            self.rows.len()
        } else {
            1
        };
        // Data columns: each column item with each value, then the totals.
        let mut data_cols: Vec<(Option<Vec<Item>>, usize)> = Vec::new();
        for ci in &col_items {
            for i in 0..nv {
                data_cols.push((Some(ci.clone()), i));
            }
        }
        for i in 0..nv {
            data_cols.push((None, i));
        }
        let caption = |i: usize| self.caption(&field(self.values[i].0), self.values[i].1);
        let row_header = |k: usize| -> Value {
            if tabular {
                Value::Text(field(self.rows[k]))
            } else {
                Value::Text("Row Labels".into())
            }
        };
        let base = rep.cells.len();
        let push = |rep: &mut Report, row: Vec<Value>, bold_all: bool| {
            let r = rep.cells.len();
            if bold_all {
                for c in 0..row.len() {
                    rep.bold.insert((r, c));
                }
            }
            rep.cells.push(row);
        };
        if !self.cols.is_empty() {
            // First header: the data caption and the column field's heading.
            let mut h1 = vec![Value::Empty; label_cols.max(1)];
            if nv == 1 {
                h1[0] = Value::Text(caption(0));
            }
            if label_cols == 0 {
                h1.clear();
            }
            h1.push(Value::Text(match self.style {
                PivotStyle::Excel => "Column Labels".into(),
                _ => field(self.cols[0]),
            }));
            push(&mut rep, h1, true);
            let mut h2: Vec<Value> = (0..label_cols).map(row_header).collect();
            for (c, i) in &data_cols {
                h2.push(match c {
                    Some(ci) if *i == 0 => Value::Text(item_label(&ci[0].0)),
                    Some(_) => Value::Empty,
                    None if nv == 1 => Value::Text(self.total_label().into()),
                    None => Value::Text(format!("Total {}", caption(*i))),
                });
            }
            push(&mut rep, h2, true);
            if nv > 1 {
                let mut h3 = vec![Value::Empty; label_cols];
                for (c, i) in &data_cols {
                    h3.push(if c.is_some() {
                        Value::Text(caption(*i))
                    } else {
                        Value::Empty
                    });
                }
                push(&mut rep, h3, true);
            }
        } else {
            let mut h: Vec<Value> = (0..label_cols).map(row_header).collect();
            for i in 0..nv {
                h.push(Value::Text(caption(i)));
            }
            push(&mut rep, h, true);
        }
        let values_row = |prefix: &[Item]| -> Vec<Value> {
            data_cols
                .iter()
                .map(|(c, i)| value_at(prefix, c.as_ref(), *i))
                .collect()
        };
        if self.rows.is_empty() {
            push(&mut rep, values_row(&[]), false);
        } else if !tabular {
            for rk in &row_keys {
                let mut row = vec![Value::Text(item_label(&rk[0].0))];
                if matches!(rk[0].0, Value::Number(_) | Value::Bool(_)) {
                    row[0] = rk[0].0.clone();
                }
                row.extend(values_row(rk));
                push(&mut rep, row, false);
            }
        } else {
            // Tabular: each row field in its own column, an outer label on the first
            // row of its group, and a subtotal under every outer group.
            let keys: Vec<&Vec<Item>> = row_keys.iter().collect();
            let n = self.rows.len();
            for (idx, rk) in keys.iter().enumerate() {
                let prev = idx.checked_sub(1).map(|p| keys[p]);
                let mut row = vec![Value::Empty; n];
                for (lvl, item) in rk.iter().enumerate() {
                    let same = prev.is_some_and(|p| p[..=lvl] == rk[..=lvl]);
                    if !same || lvl == n - 1 {
                        row[lvl] = match &item.0 {
                            Value::Empty => Value::Text("(blank)".into()),
                            v => v.clone(),
                        };
                    }
                }
                row.extend(values_row(rk));
                push(&mut rep, row, false);
                // Close the groups this record ends, innermost first.
                let next = keys.get(idx + 1);
                for lvl in (0..n - 1).rev() {
                    if next.is_none_or(|nk| nk[..=lvl] != rk[..=lvl]) {
                        let mut sub = vec![Value::Empty; n];
                        sub[lvl] = Value::Text(format!("{} Total", item_label(&rk[lvl].0)));
                        sub.extend(values_row(&rk[..=lvl]));
                        push(&mut rep, sub, true);
                    }
                }
            }
        }
        if !self.rows.is_empty() {
            let mut total = vec![Value::Text(self.total_label().into())];
            total.extend(std::iter::repeat_n(Value::Empty, label_cols - 1));
            total.extend(values_row(&[]));
            push(&mut rep, total, true);
        }
        let _ = base;
        Ok(rep)
    }
    /// Where the report's filters and body start, given how many filter rows it has.
    pub fn origin(&self) -> Cell {
        let above = if self.filters.is_empty() || self.style == PivotStyle::Sheets {
            0
        } else {
            self.filters.len() as u32 + 1
        };
        Cell::new(self.at.row.saturating_sub(above), self.at.col)
    }
}
/// The field names of a source range: its header row.
pub fn fields(wb: &Workbook, sheet: usize, source: Range) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for c in source.start.col..=source.end.col {
        let name = wb.display(sheet, Cell::new(source.start.row, c));
        if name.trim().is_empty() {
            return Err("The PivotTable field name is not valid. To create a PivotTable report, you must use data that is organized as a list with labeled columns.".into());
        }
        out.push(name);
    }
    Ok(out)
}

impl Workbook {
    /// Write a pivot table's report into its sheet, clearing what the last refresh
    /// wrote. No undo step of its own: it belongs to the edit that asked for it.
    fn write_pivot(&mut self, sheet: usize, index: usize) -> Result<(), String> {
        let mut p = self.sheets[sheet].pivots[index].clone();
        // A report whose filters need rows above it moves down to make room.
        let need = if p.filters.is_empty() || p.style == PivotStyle::Sheets {
            0
        } else {
            p.filters.len() as u32 + 1
        };
        if p.at.row < need {
            p.at.row = need;
        }
        let rep = p.build(self)?;
        if let Some(old) = p.extent.take() {
            for c in old.cells().collect::<Vec<_>>() {
                self.sheets[sheet].cells.remove(&c);
            }
        }
        let origin = p.origin();
        let width = rep.cells.iter().map(Vec::len).max().unwrap_or(1).max(1) as u32;
        let height = rep.cells.len().max(1) as u32;
        if origin.row + height > crate::MAX_ROWS || origin.col + width > crate::MAX_COLS {
            return Err("the PivotTable report does not fit on the sheet there".into());
        }
        for (r, row) in rep.cells.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                if v.is_empty() {
                    continue;
                }
                let at = Cell::new(origin.row + r as u32, origin.col + c as u32);
                let style = Style {
                    bold: rep.bold.contains(&(r, c)),
                    ..Style::default()
                };
                self.load_cell(sheet, at, Input::Value(v.clone()), v.clone(), style);
            }
        }
        p.extent = Some(Range::new(
            origin,
            Cell::new(origin.row + height - 1, origin.col + width - 1),
        ));
        self.sheets[sheet].pivots[index] = p;
        Ok(())
    }
    /// A pivot table over `source` (on `source_sheet`), placed at `at` on `sheet`.
    pub fn add_pivot(
        &mut self,
        source_sheet: usize,
        source: Range,
        sheet: usize,
        at: Cell,
        style: PivotStyle,
        auto: bool,
    ) -> Result<usize, String> {
        if source_sheet >= self.sheets.len() || sheet >= self.sheets.len() {
            return Err("no such sheet".into());
        }
        if source.rows() < 2 {
            return Err("Select a range with a header row and at least one row of data.".into());
        }
        fields(self, source_sheet, source)?;
        let n = self.sheets.iter().map(|s| s.pivots.len()).sum::<usize>() + 1;
        let name = match style {
            PivotStyle::Calc => format!("DataPilot{n}"),
            _ => format!("PivotTable{n}"),
        };
        let src_name = self.sheets[source_sheet].name.clone();
        self.book_edit(|wb| {
            wb.sheets[sheet].pivots.push(Pivot {
                name,
                source_sheet: src_name,
                source,
                at,
                rows: vec![],
                cols: vec![],
                values: vec![],
                filters: vec![],
                hidden: BTreeMap::new(),
                style,
                auto,
                extent: None,
            });
            let i = wb.sheets[sheet].pivots.len() - 1;
            wb.write_pivot(sheet, i)?;
            Ok(i)
        })
    }
    /// Change a pivot table's layout, then rebuild it.
    pub fn edit_pivot(
        &mut self,
        sheet: usize,
        index: usize,
        f: impl FnOnce(&mut Pivot) -> Result<(), String>,
    ) -> Result<(), String> {
        if self
            .sheets
            .get(sheet)
            .and_then(|s| s.pivots.get(index))
            .is_none()
        {
            return Err("no such PivotTable".into());
        }
        self.book_edit(|wb| {
            f(&mut wb.sheets[sheet].pivots[index])?;
            let p = &wb.sheets[sheet].pivots[index];
            if p.cols.len() > 1 {
                return Err("this PivotTable takes one column field".into());
            }
            wb.write_pivot(sheet, index)
        })
    }
    /// Refresh: rebuild one pivot table from its source as it is now.
    pub fn refresh_pivot(&mut self, sheet: usize, index: usize) -> Result<(), String> {
        self.edit_pivot(sheet, index, |_| Ok(()))
    }
    pub fn remove_pivot(&mut self, sheet: usize, index: usize) -> Result<(), String> {
        if self
            .sheets
            .get(sheet)
            .and_then(|s| s.pivots.get(index))
            .is_none()
        {
            return Err("no such PivotTable".into());
        }
        self.book_edit(|wb| {
            let p = wb.sheets[sheet].pivots.remove(index);
            if let Some(r) = p.extent {
                for c in r.cells().collect::<Vec<_>>() {
                    wb.sheets[sheet].cells.remove(&c);
                }
            }
            Ok(())
        })
    }
    /// Rebuild every automatically refreshing pivot table whose report is out of date,
    /// without an undo step (the edit that changed its source has one).
    pub fn refresh_auto_pivots(&mut self) {
        let mut changed = false;
        for s in 0..self.sheets.len() {
            for i in 0..self.sheets[s].pivots.len() {
                if !self.sheets[s].pivots[i].auto {
                    continue;
                }
                let before = self.sheets[s].cells.clone();
                if self.write_pivot(s, i).is_ok() && self.sheets[s].cells != before {
                    changed = true;
                }
            }
        }
        if changed {
            self.recalc_all();
        }
    }
    /// The pivot table whose report covers `c`, if any.
    pub fn pivot_at(&self, sheet: usize, c: Cell) -> Option<usize> {
        self.sheets
            .get(sheet)?
            .pivots
            .iter()
            .position(|p| p.extent.is_some_and(|e| e.contains(c)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sales() -> Workbook {
        let mut wb = Workbook::new();
        let data = [
            ["Region", "Product", "Sales", "Units"],
            ["East", "Pens", "10", "1"],
            ["West", "Pens", "20", "2"],
            ["East", "Ink", "5", "3"],
            ["West", "Pads", "7", "4"],
            ["East", "Pens", "1", "5"],
        ];
        for (r, row) in data.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                wb.set_input(0, Cell::new(r as u32, c as u32), v).unwrap();
            }
        }
        wb
    }
    fn texts(wb: &Workbook, sheet: usize, r: Range) -> Vec<String> {
        (r.start.row..=r.end.row)
            .map(|row| {
                (r.start.col..=r.end.col)
                    .map(|c| wb.display(sheet, Cell::new(row, c)))
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect()
    }
    #[test]
    fn a_pivot_summarises_rows_by_field_as_excel_lays_it_out() {
        let mut wb = sales();
        let src = Range::parse("A1:D6").unwrap();
        let s = wb.add_sheet(Some("Report")).unwrap();
        let p = wb
            .add_pivot(0, src, s, Cell::new(2, 0), PivotStyle::Excel, false)
            .unwrap();
        wb.edit_pivot(s, p, |p| {
            p.rows = vec![0];
            p.values = vec![(2, Agg::Sum)];
            Ok(())
        })
        .unwrap();
        assert_eq!(
            texts(&wb, s, Range::parse("A3:B6").unwrap()),
            [
                "Row Labels|Sum of Sales",
                "East|16",
                "West|27",
                "Grand Total|43"
            ]
        );
        // A column field, a report filter hiding one product, and a count.
        wb.edit_pivot(s, p, |p| {
            p.cols = vec![1];
            p.filters = vec![1];
            p.values = vec![(2, Agg::Count)];
            p.hidden
                .insert(1, ["Pads".to_string()].into_iter().collect());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            texts(&wb, s, Range::parse("A1:D7").unwrap()),
            [
                "Product|(Multiple Items)||",
                "|||",
                "Count of Sales|Column Labels||",
                "Row Labels|Ink|Pens|Grand Total",
                "East|1|2|3",
                "West||1|1",
                "Grand Total|1|3|4"
            ]
        );
        // Formulas can read the report; a refresh picks up a changed source.
        wb.set_input(s, Cell::new(10, 0), "=D7*10").unwrap();
        wb.set_input(0, Cell::new(5, 1), "Ink").unwrap();
        assert_eq!(
            wb.display(s, Cell::new(4, 1)),
            "1",
            "Excel waits for Refresh"
        );
        wb.refresh_pivot(s, p).unwrap();
        assert_eq!(wb.display(s, Cell::new(4, 1)), "2");
        assert_eq!(wb.display(s, Cell::new(10, 0)), "40");
        assert!(wb.undo());
        assert_eq!(wb.display(s, Cell::new(4, 1)), "1");
    }
    #[test]
    fn nested_rows_get_subtotals_and_sheets_refreshes_by_itself() {
        let mut wb = sales();
        let src = Range::parse("A1:D6").unwrap();
        let p = wb
            .add_pivot(0, src, 0, Cell::new(0, 6), PivotStyle::Sheets, true)
            .unwrap();
        wb.edit_pivot(0, p, |p| {
            p.rows = vec![0, 1];
            p.values = vec![(2, Agg::Sum), (3, Agg::Max)];
            Ok(())
        })
        .unwrap();
        assert_eq!(
            texts(&wb, 0, Range::parse("G1:J8").unwrap()),
            [
                "Region|Product|SUM of Sales|MAX of Units",
                "East|Ink|5|3",
                "|Pens|11|5",
                "East Total||16|5",
                "West|Pads|7|4",
                "|Pens|20|2",
                "West Total||27|4",
                "Grand Total||43|5"
            ]
        );
        wb.set_input(0, Cell::new(1, 2), "100").unwrap();
        wb.refresh_auto_pivots();
        assert_eq!(wb.display(0, Cell::new(7, 8)), "133");
    }
}
