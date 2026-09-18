//! Workbooks: sheets of cells, the dependency graph that keeps formulas current, and
//! every edit a spreadsheet makes, each one undoable.
use crate::address::{Cell, CellRef, Range, MAX_COLS, MAX_ROWS};
use crate::eval::Eval;
use crate::parser::{self, Expr, Formula};
use crate::value::{compare, ErrorKind, Value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Undo steps kept; older ones are dropped.
const UNDO_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Align {
    #[default]
    General,
    Left,
    Center,
    Right,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    /// An Excel number format code; `General` by default.
    pub format: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub fill: Option<[u8; 3]>,
    #[serde(default)]
    pub color: Option<[u8; 3]>,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            format: "General".into(),
            bold: false,
            italic: false,
            underline: false,
            align: Align::General,
            fill: None,
            color: None,
        }
    }
}
impl Style {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Input {
    Value(Value),
    Formula(Formula),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellData {
    pub input: Input,
    /// The computed value (the input itself for a constant).
    pub value: Value,
    #[serde(default, skip_serializing_if = "Style::is_default")]
    pub style: Style,
}
impl CellData {
    fn is_blank(&self) -> bool {
        matches!(self.input, Input::Value(Value::Empty)) && self.style.is_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartKind {
    Column,
    Bar,
    Line,
    Pie,
}
impl ChartKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "column" => Self::Column,
            "bar" => Self::Bar,
            "line" => Self::Line,
            "pie" => Self::Pie,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Column => "column",
            Self::Bar => "bar",
            Self::Line => "line",
            Self::Pie => "pie",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chart {
    pub kind: ChartKind,
    /// The data it plots, on its own sheet.
    pub range: Range,
    pub title: String,
    /// The cell its top-left corner sits over.
    pub anchor: Cell,
    /// Size in columns and rows.
    pub cols: u32,
    pub rows: u32,
}
/// One series of a chart: its name and a value per category (`None` for a gap).
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub name: String,
    pub values: Vec<Option<f64>>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ChartData {
    pub categories: Vec<String>,
    pub series: Vec<Series>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoFilter {
    pub range: Range,
    /// Values (as displayed) each column hides.
    pub hidden: BTreeMap<u32, BTreeSet<String>>,
}

mod cells_serde {
    use super::{Cell, CellData};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;
    pub fn serialize<S: Serializer>(m: &BTreeMap<Cell, CellData>, s: S) -> Result<S::Ok, S::Error> {
        m.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<Cell, CellData>, D::Error> {
        Ok(Vec::<(Cell, CellData)>::deserialize(d)?
            .into_iter()
            .collect())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    #[serde(with = "cells_serde")]
    pub cells: BTreeMap<Cell, CellData>,
    /// Column widths in pixels at 100%, where they differ from the default.
    #[serde(default)]
    pub col_widths: BTreeMap<u32, u32>,
    /// Frozen rows and columns at the top-left.
    #[serde(default)]
    pub freeze: (u32, u32),
    #[serde(default)]
    pub filter: Option<AutoFilter>,
    #[serde(default)]
    pub charts: Vec<Chart>,
}
/// Default column width in pixels (Excel's 8.43 characters of Calibri 11).
pub const DEFAULT_COL_WIDTH: u32 = 64;
impl Sheet {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cells: BTreeMap::new(),
            col_widths: BTreeMap::new(),
            freeze: (0, 0),
            filter: None,
            charts: Vec::new(),
        }
    }
    pub fn col_width(&self, col: u32) -> u32 {
        self.col_widths
            .get(&col)
            .copied()
            .unwrap_or(DEFAULT_COL_WIDTH)
    }
    /// Rows the auto filter currently hides.
    pub fn hidden_rows(&self) -> BTreeSet<u32> {
        let mut out = BTreeSet::new();
        let Some(f) = &self.filter else {
            return out;
        };
        for row in f.range.start.row + 1..=f.range.end.row {
            for (col, hidden) in &f.hidden {
                let shown = self
                    .cells
                    .get(&Cell::new(row, *col))
                    .map(|c| display_value(&c.value, &c.style.format))
                    .unwrap_or_default();
                if hidden.contains(&shown) {
                    out.insert(row);
                }
            }
        }
        out
    }
}
fn display_value(v: &Value, format: &str) -> String {
    crate::format::format(v, format).text
}

/// One undo step: what changed, as it was before and after.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Change {
    Cells {
        sheet: usize,
        #[serde(with = "change_cells")]
        before: Vec<(Cell, Option<CellData>)>,
        #[serde(with = "change_cells")]
        after: Vec<(Cell, Option<CellData>)>,
    },
    Book {
        before: Box<Book>,
        after: Box<Book>,
    },
}
mod change_cells {
    use super::{Cell, CellData};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    type Cells = Vec<(Cell, Option<CellData>)>;
    pub fn serialize<S: Serializer>(v: &Cells, s: S) -> Result<S::Ok, S::Error> {
        v.serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Cells, D::Error> {
        Cells::deserialize(d)
    }
}
/// Everything a structural edit can change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Book {
    sheets: Vec<Sheet>,
    names: BTreeMap<String, (String, Range)>,
}

/// The dependency graph: who reads which cell. Derived from the formulas, so it is
/// rebuilt on load rather than stored.
#[derive(Clone, Debug, Default)]
struct Graph {
    /// A cell and the formulas that read it directly.
    readers: HashMap<(usize, Cell), BTreeSet<(usize, Cell)>>,
    /// Ranges (and names) formulas read, with the formula.
    ranges: Vec<((usize, Range), (usize, Cell))>,
    /// Formulas that must recalculate on every recalculation.
    volatile: BTreeSet<(usize, Cell)>,
    /// What each formula reads, so it can be removed when the formula changes.
    reads: HashMap<(usize, Cell), Vec<(usize, Range)>>,
}
#[derive(Clone, Debug, Default)]
struct GraphCache(Option<Graph>);
impl PartialEq for GraphCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for GraphCache {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
    /// Defined names: display name → (sheet name, range).
    #[serde(default)]
    pub names: BTreeMap<String, (String, Range)>,
    /// The world clock in microseconds since the Unix epoch, for TODAY() and NOW().
    #[serde(default)]
    now_us: i64,
    /// Recalculations so far, which RAND() draws from.
    #[serde(default)]
    generation: u64,
    #[serde(default)]
    undo: Vec<Change>,
    #[serde(default)]
    redo: Vec<Change>,
    #[serde(skip)]
    graph: GraphCache,
}
impl Default for Workbook {
    fn default() -> Self {
        Self::new()
    }
}

/// Selection statistics, as a status bar shows them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Non-empty cells.
    pub count: usize,
    /// Cells holding numbers.
    pub numbers: usize,
    pub sum: f64,
    pub average: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// A copied block: inputs and styles, and where it came from so formulas can be moved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clip {
    pub origin: Cell,
    pub rows: u32,
    pub cols: u32,
    /// Row-major, `None` for an empty cell.
    pub cells: Vec<Option<(Input, Style)>>,
}

impl Workbook {
    pub fn new() -> Self {
        Self::with_sheet("Sheet1")
    }
    pub fn with_sheet(name: &str) -> Self {
        Self {
            sheets: vec![Sheet::new(name)],
            names: BTreeMap::new(),
            now_us: 0,
            generation: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            graph: GraphCache(None),
        }
    }
    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheets
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
    }
    pub fn sheet(&self, i: usize) -> Option<&Sheet> {
        self.sheets.get(i)
    }
    /// The world clock, as microseconds since the Unix epoch. Volatile formulas
    /// recalculate when it moves.
    pub fn set_now(&mut self, unix_us: i64) {
        if unix_us != self.now_us {
            self.now_us = unix_us;
            self.recalc_volatile();
        }
    }
    /// The world clock as a serial date.
    pub fn now(&self) -> f64 {
        crate::date::from_unix_micros(self.now_us)
    }
    /// A deterministic draw in [0, 1) for RAND(): the same cell in the same
    /// recalculation always draws the same number.
    pub fn random(&self, sheet: usize, at: Cell, salt: u64) -> f64 {
        let mut x = self.generation.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (sheet as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
            ^ (u64::from(at.row) << 20 | u64::from(at.col)).wrapping_mul(0x94D0_49BB_1331_11EB)
            ^ salt;
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 31;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
    /// A defined name's target.
    pub fn name(&self, name: &str) -> Option<(usize, Range)> {
        let (_, (sheet, range)) = self
            .names
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))?;
        Some((self.sheet_index(sheet)?, *range))
    }
    pub fn value(&self, sheet: usize, c: Cell) -> Value {
        self.sheets
            .get(sheet)
            .and_then(|s| s.cells.get(&c))
            .map(|d| d.value.clone())
            .unwrap_or_default()
    }
    pub fn cell(&self, sheet: usize, c: Cell) -> Option<&CellData> {
        self.sheets.get(sheet)?.cells.get(&c)
    }
    pub fn style(&self, sheet: usize, c: Cell) -> Style {
        self.cell(sheet, c)
            .map(|d| d.style.clone())
            .unwrap_or_default()
    }
    /// What the formula bar shows: `=SUM(A1:A3)`, a number as typed, text.
    pub fn input(&self, sheet: usize, c: Cell) -> String {
        match self.cell(sheet, c).map(|d| &d.input) {
            Some(Input::Formula(f)) => format!("={}", f.text()),
            Some(Input::Value(Value::Number(n))) => {
                let style = self.style(sheet, c);
                if crate::format::is_date_format(&style.format) {
                    crate::format::format(
                        &Value::Number(*n),
                        if n.fract() == 0.0 {
                            "m/d/yyyy"
                        } else {
                            "m/d/yyyy h:mm:ss"
                        },
                    )
                    .text
                } else if style.format.contains('%') {
                    format!("{}%", crate::value::general(n * 100.0))
                } else {
                    crate::value::general(*n)
                }
            }
            Some(Input::Value(Value::Text(t))) => {
                // Text that would read as something else keeps its apostrophe.
                if t.starts_with('=')
                    || crate::value::parse_number_text(t).is_some()
                    || crate::date::parse_datetime(t).is_some()
                {
                    format!("'{t}")
                } else {
                    t.clone()
                }
            }
            Some(Input::Value(v)) => v.display(),
            None => String::new(),
        }
    }
    /// The cell as displayed, through its number format.
    pub fn display(&self, sheet: usize, c: Cell) -> String {
        match self.cell(sheet, c) {
            Some(d) => display_value(&d.value, &d.style.format),
            None => String::new(),
        }
    }
    pub fn used_range(&self, sheet: usize) -> Option<Range> {
        let s = self.sheets.get(sheet)?;
        let mut keys = s
            .cells
            .iter()
            .filter(|(_, d)| !d.is_blank())
            .map(|(k, _)| *k);
        let first = keys.next()?;
        let (mut r0, mut r1, mut c0, mut c1) = (first.row, first.row, first.col, first.col);
        for k in keys {
            r0 = r0.min(k.row);
            r1 = r1.max(k.row);
            c0 = c0.min(k.col);
            c1 = c1.max(k.col);
        }
        Some(Range::new(Cell::new(r0, c0), Cell::new(r1, c1)))
    }

    // ----- the dependency graph and recalculation -----

    fn reads_of(&self, sheet: usize, e: &Expr) -> (Vec<(usize, Range)>, bool) {
        let mut refs = Vec::new();
        parser::references(e, &mut refs);
        let mut out = Vec::new();
        for (name, a, b) in refs {
            let s = match &name {
                None => Some(sheet),
                Some(n) => self.sheet_index(n),
            };
            if let Some(s) = s {
                out.push((s, Range::new(a.cell(), b.cell())));
            }
        }
        let mut volatile = false;
        parser::walk(e, &mut |x| match x {
            Expr::Name(n) => {
                if let Some(t) = self.name(n) {
                    out.push(t);
                }
            }
            Expr::Call(f, _) if crate::functions::VOLATILE.contains(&f.as_str()) => {
                volatile = true;
            }
            _ => {}
        });
        (out, volatile)
    }
    fn build_graph(&self) -> Graph {
        let mut g = Graph::default();
        for (si, s) in self.sheets.iter().enumerate() {
            for (c, d) in &s.cells {
                if let Input::Formula(f) = &d.input {
                    self.add_edges(&mut g, (si, *c), &f.expr);
                }
            }
        }
        g
    }
    fn add_edges(&self, g: &mut Graph, at: (usize, Cell), e: &Expr) {
        let (reads, volatile) = self.reads_of(at.0, e);
        for (s, r) in &reads {
            if r.is_single() {
                g.readers.entry((*s, r.start)).or_default().insert(at);
            } else {
                g.ranges.push(((*s, *r), at));
            }
        }
        if volatile {
            g.volatile.insert(at);
        }
        g.reads.insert(at, reads);
    }
    fn remove_edges(g: &mut Graph, at: (usize, Cell)) {
        if let Some(reads) = g.reads.remove(&at) {
            for (s, r) in reads {
                if r.is_single() {
                    if let Some(set) = g.readers.get_mut(&(s, r.start)) {
                        set.remove(&at);
                    }
                }
            }
            g.ranges.retain(|(_, f)| *f != at);
        }
        g.volatile.remove(&at);
    }
    fn graph(&mut self) -> &mut Graph {
        if self.graph.0.is_none() {
            self.graph.0 = Some(self.build_graph());
        }
        self.graph.0.as_mut().expect("just built")
    }
    /// Formulas that read `cell`, directly or through a range.
    fn readers_of(g: &Graph, cell: (usize, Cell)) -> Vec<(usize, Cell)> {
        let mut out: Vec<(usize, Cell)> = g
            .readers
            .get(&cell)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        for ((s, r), f) in &g.ranges {
            if *s == cell.0 && r.contains(cell.1) {
                out.push(*f);
            }
        }
        out
    }
    /// Recalculate everything that depends on `changed`, in dependency order. Cells in
    /// a cycle get the circular-reference error; cells downstream of a cycle then see
    /// that error like any other.
    fn recalc(&mut self, changed: &[(usize, Cell)]) {
        self.generation = self.generation.wrapping_add(1);
        let g = self.graph().clone();
        // Everything downstream of the change.
        let mut dirty: BTreeSet<(usize, Cell)> = BTreeSet::new();
        let mut stack: Vec<(usize, Cell)> = Vec::new();
        for c in changed {
            if self.is_formula(*c) {
                stack.push(*c);
            }
            stack.extend(Self::readers_of(&g, *c));
        }
        stack.extend(g.volatile.iter().copied());
        while let Some(c) = stack.pop() {
            if dirty.insert(c) {
                stack.extend(Self::readers_of(&g, c));
            }
        }
        self.evaluate_in_order(&g, dirty);
    }
    fn is_formula(&self, c: (usize, Cell)) -> bool {
        matches!(
            self.cell(c.0, c.1).map(|d| &d.input),
            Some(Input::Formula(_))
        )
    }
    fn evaluate_in_order(&mut self, g: &Graph, dirty: BTreeSet<(usize, Cell)>) {
        // Kahn's algorithm over the dirty subgraph: an edge runs from a precedent to a
        // formula that reads it, when both are dirty.
        let mut indegree: BTreeMap<(usize, Cell), usize> = dirty.iter().map(|c| (*c, 0)).collect();
        let mut out_edges: BTreeMap<(usize, Cell), Vec<(usize, Cell)>> = BTreeMap::new();
        for f in &dirty {
            let reads = g.reads.get(f).cloned().unwrap_or_default();
            let mut precedents = BTreeSet::new();
            for (s, r) in reads {
                for d in dirty.range((s, r.start)..=(s, r.end)) {
                    if r.contains(d.1) && d.0 == s {
                        precedents.insert(*d);
                    }
                }
            }
            for p in precedents {
                *indegree.get_mut(f).expect("dirty") += 1;
                out_edges.entry(p).or_default().push(*f);
            }
        }
        let mut ready: BTreeSet<(usize, Cell)> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(c, _)| *c)
            .collect();
        let mut done: BTreeSet<(usize, Cell)> = BTreeSet::new();
        loop {
            while let Some(c) = ready.pop_first() {
                self.evaluate(c);
                done.insert(c);
                for next in out_edges.get(&c).cloned().unwrap_or_default() {
                    if done.contains(&next) {
                        continue;
                    }
                    let d = indegree.get_mut(&next).expect("dirty");
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        ready.insert(next);
                    }
                }
            }
            let left: Vec<(usize, Cell)> = indegree
                .keys()
                .filter(|c| !done.contains(c))
                .copied()
                .collect();
            if left.is_empty() {
                break;
            }
            // What remains sits on or behind a cycle. Mark the members of every cycle,
            // then let the cells behind them evaluate against the errors.
            let cyclic = cycle_members(&left, &out_edges);
            for c in &cyclic {
                if let Some(d) = self.sheets[c.0].cells.get_mut(&c.1) {
                    d.value = Value::Error(ErrorKind::Circular);
                }
                done.insert(*c);
                for next in out_edges.get(c).cloned().unwrap_or_default() {
                    if let Some(d) = indegree.get_mut(&next) {
                        *d = d.saturating_sub(1);
                        if *d == 0 && !done.contains(&next) {
                            ready.insert(next);
                        }
                    }
                }
            }
            if cyclic.is_empty() {
                break;
            }
        }
    }
    fn evaluate(&mut self, c: (usize, Cell)) {
        let expr = match self.cell(c.0, c.1).map(|d| &d.input) {
            Some(Input::Formula(f)) => f.expr.clone(),
            _ => return,
        };
        let v = Eval::new(self, c.0, c.1).cell_result(&expr);
        if let Some(d) = self.sheets[c.0].cells.get_mut(&c.1) {
            d.value = v;
        }
    }
    /// Recalculate every formula in the workbook.
    pub fn recalc_all(&mut self) {
        self.graph.0 = None;
        let all: Vec<(usize, Cell)> = self
            .sheets
            .iter()
            .enumerate()
            .flat_map(|(si, s)| {
                s.cells
                    .iter()
                    .filter(|(_, d)| matches!(d.input, Input::Formula(_)))
                    .map(move |(c, _)| (si, *c))
            })
            .collect();
        self.generation = self.generation.wrapping_add(1);
        let g = self.graph().clone();
        self.evaluate_in_order(&g, all.into_iter().collect());
    }
    fn recalc_volatile(&mut self) {
        let g = self.graph().clone();
        if g.volatile.is_empty() {
            return;
        }
        let v: Vec<(usize, Cell)> = g.volatile.iter().copied().collect();
        self.recalc(&v);
    }
    /// Cells whose value is a circular-reference error.
    pub fn circular(&self) -> Vec<(usize, Cell)> {
        let mut out = Vec::new();
        for (si, s) in self.sheets.iter().enumerate() {
            for (c, d) in &s.cells {
                if d.value == Value::Error(ErrorKind::Circular) {
                    out.push((si, *c));
                }
            }
        }
        out
    }

    // ----- undo -----

    fn snapshot(&self, sheet: usize, cells: &[Cell]) -> Vec<(Cell, Option<CellData>)> {
        cells
            .iter()
            .map(|c| (*c, self.cell(sheet, *c).cloned()))
            .collect()
    }
    /// Record a cell edit: `f` runs, and what it changed in `cells` becomes one step.
    fn cells_edit<T>(
        &mut self,
        sheet: usize,
        cells: Vec<Cell>,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let before = self.snapshot(sheet, &cells);
        let out = f(self)?;
        let after = self.snapshot(sheet, &cells);
        if before != after {
            self.push_undo(Change::Cells {
                sheet,
                before,
                after,
            });
        }
        Ok(out)
    }
    fn book(&self) -> Book {
        Book {
            sheets: self.sheets.clone(),
            names: self.names.clone(),
        }
    }
    fn book_edit<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let before = self.book();
        let out = f(self)?;
        self.graph.0 = None;
        self.recalc_all();
        let after = self.book();
        if before != after {
            self.push_undo(Change::Book {
                before: Box::new(before),
                after: Box::new(after),
            });
        }
        Ok(out)
    }
    fn push_undo(&mut self, c: Change) {
        self.undo.push(c);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    fn apply_change(&mut self, c: &Change, forward: bool) {
        match c {
            Change::Cells {
                sheet,
                before,
                after,
            } => {
                let cells = if forward { after } else { before };
                let mut touched = Vec::new();
                for (cell, data) in cells {
                    let key = (*sheet, *cell);
                    if let Some(g) = &mut self.graph.0 {
                        Self::remove_edges(g, key);
                    }
                    match data {
                        Some(d) => {
                            self.sheets[*sheet].cells.insert(*cell, d.clone());
                        }
                        None => {
                            self.sheets[*sheet].cells.remove(cell);
                        }
                    }
                    if let Some(Input::Formula(f)) = data.as_ref().map(|d| &d.input) {
                        let expr = f.expr.clone();
                        let mut g = self.graph.0.take().unwrap_or_else(|| self.build_graph());
                        self.add_edges(&mut g, key, &expr);
                        self.graph.0 = Some(g);
                    }
                    touched.push(key);
                }
                self.recalc(&touched);
            }
            Change::Book { before, after } => {
                let b = if forward { after } else { before };
                self.sheets = b.sheets.clone();
                self.names = b.names.clone();
                self.graph.0 = None;
                self.recalc_all();
            }
        }
    }
    pub fn undo(&mut self) -> bool {
        let Some(c) = self.undo.pop() else {
            return false;
        };
        self.apply_change(&c, false);
        self.redo.push(c);
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(c) = self.redo.pop() else {
            return false;
        };
        self.apply_change(&c, true);
        self.undo.push(c);
        true
    }

    // ----- cell edits -----

    fn check_sheet(&self, sheet: usize) -> Result<(), String> {
        if sheet < self.sheets.len() {
            Ok(())
        } else {
            Err("no such sheet".into())
        }
    }
    /// Store one cell's input without recording undo or recalculating.
    fn put(&mut self, sheet: usize, c: Cell, input: Input, style: Option<Style>) {
        let key = (sheet, c);
        let style = style.unwrap_or_else(|| self.style(sheet, c));
        let value = match &input {
            Input::Value(v) => v.clone(),
            Input::Formula(_) => Value::Empty,
        };
        if let Some(g) = &mut self.graph.0 {
            Self::remove_edges(g, key);
        }
        let data = CellData {
            input,
            value,
            style,
        };
        if data.is_blank() {
            self.sheets[sheet].cells.remove(&c);
        } else {
            if let Input::Formula(f) = &data.input {
                let expr = f.expr.clone();
                if self.graph.0.is_some() {
                    let mut g = self.graph.0.take().expect("checked");
                    self.add_edges(&mut g, key, &expr);
                    self.graph.0 = Some(g);
                }
            }
            self.sheets[sheet].cells.insert(c, data);
        }
    }
    /// Interpret what a person typed: `=` starts a formula, `'` forces text, and numbers,
    /// percentages, currency, dates, times and TRUE/FALSE are recognised (giving the
    /// cell a matching format if it had none). Empty text clears the contents.
    pub fn parse_entry(text: &str) -> Result<(Input, Option<&'static str>), String> {
        if let Some(body) = text.strip_prefix('=') {
            if body.trim().is_empty() {
                return Ok((Input::Value(Value::Text(text.into())), None));
            }
            return Formula::parse(body).map(|f| (Input::Formula(f), None));
        }
        if let Some(rest) = text.strip_prefix('\'') {
            return Ok((Input::Value(Value::Text(rest.into())), None));
        }
        let t = text.trim();
        if t.is_empty() {
            return Ok((Input::Value(Value::Empty), None));
        }
        if t.eq_ignore_ascii_case("TRUE") || t.eq_ignore_ascii_case("FALSE") {
            return Ok((
                Input::Value(Value::Bool(t.eq_ignore_ascii_case("TRUE"))),
                None,
            ));
        }
        if let Some(k) = ErrorKind::parse(t) {
            return Ok((Input::Value(Value::Error(k)), None));
        }
        if let Some(x) = crate::value::parse_number_text(t) {
            let format = if t.ends_with('%') {
                Some(if t.contains('.') { "0.00%" } else { "0%" })
            } else if t.trim_start_matches(['-', '(']).starts_with('$') {
                Some(if t.contains('.') {
                    "$#,##0.00"
                } else {
                    "$#,##0"
                })
            } else if t.contains(',') {
                Some(if t.contains('.') { "#,##0.00" } else { "#,##0" })
            } else {
                None
            };
            return Ok((Input::Value(Value::Number(x)), format));
        }
        if let Some((serial, format)) = crate::date::parse_datetime(t) {
            return Ok((Input::Value(Value::Number(serial)), Some(format)));
        }
        Ok((Input::Value(Value::Text(text.into())), None))
    }
    /// Type into a cell, as pressing Enter commits it.
    pub fn set_input(&mut self, sheet: usize, c: Cell, text: &str) -> Result<(), String> {
        self.check_sheet(sheet)?;
        let (input, format) = Self::parse_entry(text)?;
        self.cells_edit(sheet, vec![c], |wb| {
            let mut style = wb.style(sheet, c);
            if let Some(f) = format {
                if style.format == "General" {
                    style.format = f.into();
                }
            }
            wb.put(sheet, c, input, Some(style));
            wb.recalc(&[(sheet, c)]);
            Ok(())
        })
    }
    /// Delete contents (formats stay), as the Delete key does.
    pub fn clear(&mut self, sheet: usize, range: Range) -> Result<(), String> {
        self.check_sheet(sheet)?;
        let cells = self.stored_in(sheet, range);
        self.cells_edit(sheet, cells.clone(), |wb| {
            for c in &cells {
                wb.put(sheet, *c, Input::Value(Value::Empty), None);
            }
            let touched: Vec<_> = cells.iter().map(|c| (sheet, *c)).collect();
            wb.recalc(&touched);
            Ok(())
        })
    }
    /// Delete contents and formats.
    pub fn clear_all(&mut self, sheet: usize, range: Range) -> Result<(), String> {
        self.check_sheet(sheet)?;
        let cells = self.stored_in(sheet, range);
        self.cells_edit(sheet, cells.clone(), |wb| {
            for c in &cells {
                wb.put(
                    sheet,
                    *c,
                    Input::Value(Value::Empty),
                    Some(Style::default()),
                );
            }
            let touched: Vec<_> = cells.iter().map(|c| (sheet, *c)).collect();
            wb.recalc(&touched);
            Ok(())
        })
    }
    fn stored_in(&self, sheet: usize, range: Range) -> Vec<Cell> {
        let s = &self.sheets[sheet];
        let mut out = Vec::new();
        for row in range.start.row..=range.end.row.min(MAX_ROWS - 1) {
            for (c, _) in s
                .cells
                .range(Cell::new(row, range.start.col)..=Cell::new(row, range.end.col))
            {
                out.push(*c);
            }
            if out.len() > 1_000_000 {
                break;
            }
        }
        out
    }
    /// Change the style of every cell in a range.
    pub fn update_style(
        &mut self,
        sheet: usize,
        range: Range,
        f: impl Fn(&mut Style),
    ) -> Result<(), String> {
        self.check_sheet(sheet)?;
        if u64::from(range.rows()) * u64::from(range.cols()) > 1_000_000 {
            return Err("that range is too large to format cell by cell".into());
        }
        let cells: Vec<Cell> = range.cells().collect();
        self.cells_edit(sheet, cells.clone(), |wb| {
            for c in &cells {
                let mut style = wb.style(sheet, *c);
                f(&mut style);
                let input = wb
                    .cell(sheet, *c)
                    .map(|d| d.input.clone())
                    .unwrap_or(Input::Value(Value::Empty));
                let value = wb.value(sheet, *c);
                let data = CellData {
                    input,
                    value,
                    style,
                };
                if data.is_blank() {
                    wb.sheets[sheet].cells.remove(c);
                } else {
                    wb.sheets[sheet].cells.insert(*c, data);
                }
            }
            Ok(())
        })
    }

    // ----- clipboard and fill -----

    pub fn copy(&self, sheet: usize, range: Range) -> Clip {
        let mut cells = Vec::with_capacity((range.rows() * range.cols()) as usize);
        for c in range.cells() {
            cells.push(
                self.cell(sheet, c)
                    .map(|d| (d.input.clone(), d.style.clone())),
            );
        }
        Clip {
            origin: range.start,
            rows: range.rows(),
            cols: range.cols(),
            cells,
        }
    }
    /// A formula moved by (dr, dc): relative references follow, absolute ones stay,
    /// and a reference pushed off the grid becomes `#REF!`.
    pub fn shift_formula(f: &Formula, dr: i64, dc: i64) -> Formula {
        Formula {
            expr: parser::map_refs(&f.expr, &mut |_, r: CellRef| r.shifted(dr, dc)),
        }
    }
    fn shifted_input(input: &Input, dr: i64, dc: i64) -> Input {
        match input {
            Input::Formula(f) => Input::Formula(Self::shift_formula(f, dr, dc)),
            other => other.clone(),
        }
    }
    /// Paste a copied block with its top-left at `at`, tiling it across `target` when
    /// the target is a whole multiple of it. Returns the range written.
    pub fn paste(
        &mut self,
        clip: &Clip,
        sheet: usize,
        at: Cell,
        target: Option<Range>,
    ) -> Result<Range, String> {
        self.check_sheet(sheet)?;
        let (mut rows, mut cols) = (clip.rows, clip.cols);
        if let Some(t) = target {
            if t.rows() % clip.rows == 0 && t.cols() % clip.cols == 0 {
                rows = t.rows();
                cols = t.cols();
            }
        }
        if at.row + rows > MAX_ROWS || at.col + cols > MAX_COLS {
            return Err("the paste area extends past the edge of the sheet".into());
        }
        let dest = Range::new(at, Cell::new(at.row + rows - 1, at.col + cols - 1));
        let cells: Vec<Cell> = dest.cells().collect();
        self.cells_edit(sheet, cells.clone(), |wb| {
            for c in &cells {
                let (r, k) = ((c.row - at.row) % clip.rows, (c.col - at.col) % clip.cols);
                let src = Cell::new(clip.origin.row + r, clip.origin.col + k);
                let dr = i64::from(c.row) - i64::from(src.row);
                let dc = i64::from(c.col) - i64::from(src.col);
                match &clip.cells[(r * clip.cols + k) as usize] {
                    Some((input, style)) => wb.put(
                        sheet,
                        *c,
                        Self::shifted_input(input, dr, dc),
                        Some(style.clone()),
                    ),
                    None => wb.put(
                        sheet,
                        *c,
                        Input::Value(Value::Empty),
                        Some(Style::default()),
                    ),
                }
            }
            let touched: Vec<_> = cells.iter().map(|c| (sheet, *c)).collect();
            wb.recalc(&touched);
            Ok(dest)
        })
    }
    /// Move a block (cut and paste): formulas anywhere that pointed into it follow it.
    pub fn move_range(&mut self, sheet: usize, from: Range, at: Cell) -> Result<Range, String> {
        self.check_sheet(sheet)?;
        if at.row + from.rows() > MAX_ROWS || at.col + from.cols() > MAX_COLS {
            return Err("the paste area extends past the edge of the sheet".into());
        }
        let dr = i64::from(at.row) - i64::from(from.start.row);
        let dc = i64::from(at.col) - i64::from(from.start.col);
        let dest = Range::new(
            at,
            Cell::new(at.row + from.rows() - 1, at.col + from.cols() - 1),
        );
        let sheet_name = self.sheets[sheet].name.clone();
        self.book_edit(|wb| {
            let clip = wb.copy(sheet, from);
            for c in from.cells() {
                wb.sheets[sheet].cells.remove(&c);
            }
            for c in dest.cells() {
                wb.sheets[sheet].cells.remove(&c);
            }
            for (i, c) in dest.cells().enumerate() {
                if let Some((input, style)) = &clip.cells[i] {
                    // The moved formulas keep pointing where they pointed, except at
                    // cells that moved with them.
                    let input = match input {
                        Input::Formula(f) => Input::Formula(Formula {
                            expr: parser::map_refs(
                                &f.expr,
                                &mut |s: &Option<String>, r: CellRef| {
                                    let same = s
                                        .as_ref()
                                        .is_none_or(|n| n.eq_ignore_ascii_case(&sheet_name));
                                    if same && from.contains(r.cell()) {
                                        r.shifted_both(dr, dc)
                                    } else {
                                        Some(r)
                                    }
                                },
                            ),
                        }),
                        other => other.clone(),
                    };
                    let value = match &input {
                        Input::Value(v) => v.clone(),
                        Input::Formula(_) => Value::Empty,
                    };
                    wb.sheets[sheet].cells.insert(
                        c,
                        CellData {
                            input,
                            value,
                            style: style.clone(),
                        },
                    );
                }
            }
            // Formulas elsewhere that read the moved cells now read their new place.
            let names: Vec<String> = wb.sheets.iter().map(|s| s.name.clone()).collect();
            for (si, s) in wb.sheets.iter_mut().enumerate() {
                for (c, d) in s.cells.iter_mut() {
                    if si == sheet && dest.contains(*c) {
                        continue;
                    }
                    if let Input::Formula(f) = &d.input {
                        let expr =
                            parser::map_refs(&f.expr, &mut |name: &Option<String>, r: CellRef| {
                                let target = match name {
                                    None => si,
                                    Some(n) => names
                                        .iter()
                                        .position(|x| x.eq_ignore_ascii_case(n))
                                        .unwrap_or(usize::MAX),
                                };
                                if target == sheet && from.contains(r.cell()) {
                                    r.shifted_both(dr, dc)
                                } else {
                                    Some(r)
                                }
                            });
                        d.input = Input::Formula(Formula { expr });
                    }
                }
            }
            Ok(dest)
        })
    }
    /// Drag the fill handle from `source` across `target` (which contains it). Numbers
    /// and dates continue a series when the source has two or more of them in a line;
    /// text ending in a number counts on; formulas are copied with their references
    /// moved; anything else repeats.
    pub fn fill(&mut self, sheet: usize, source: Range, target: Range) -> Result<(), String> {
        self.check_sheet(sheet)?;
        if target.intersect(&source) != Some(source) {
            return Err("the fill area must include the source".into());
        }
        let vertical = target.cols() == source.cols() && target.rows() != source.rows();
        let horizontal = target.rows() == source.rows() && target.cols() != source.cols();
        if !vertical && !horizontal {
            return if target == source {
                Ok(())
            } else {
                Err("fill extends a selection in one direction".into())
            };
        }
        let cells: Vec<Cell> = target.cells().filter(|c| !source.contains(*c)).collect();
        self.cells_edit(sheet, cells.clone(), |wb| {
            // Each line (column when filling down, row when filling right) fills alone.
            let lines = if vertical {
                source.cols()
            } else {
                source.rows()
            };
            for line in 0..lines {
                let src: Vec<Cell> = if vertical {
                    (source.start.row..=source.end.row)
                        .map(|r| Cell::new(r, source.start.col + line))
                        .collect()
                } else {
                    (source.start.col..=source.end.col)
                        .map(|c| Cell::new(source.start.row + line, c))
                        .collect()
                };
                let dst: Vec<Cell> = if vertical {
                    (target.start.row..=target.end.row)
                        .filter(|r| !(source.start.row..=source.end.row).contains(r))
                        .map(|r| Cell::new(r, source.start.col + line))
                        .collect()
                } else {
                    (target.start.col..=target.end.col)
                        .filter(|c| !(source.start.col..=source.end.col).contains(c))
                        .map(|c| Cell::new(source.start.row + line, c))
                        .collect()
                };
                let inputs: Vec<Option<CellData>> =
                    src.iter().map(|c| wb.cell(sheet, *c).cloned()).collect();
                let numbers: Option<Vec<f64>> = inputs
                    .iter()
                    .map(|d| match d.as_ref().map(|d| &d.input) {
                        Some(Input::Value(Value::Number(x))) => Some(*x),
                        _ => None,
                    })
                    .collect();
                let series = numbers.as_ref().filter(|n| n.len() >= 2).map(|n| {
                    // A least-squares line through the source, as Excel's fill series.
                    let k = n.len() as f64;
                    let xs: Vec<f64> = (0..n.len()).map(|i| i as f64).collect();
                    let mx = xs.iter().sum::<f64>() / k;
                    let my = n.iter().sum::<f64>() / k;
                    let sxx: f64 = xs.iter().map(|x| (x - mx) * (x - mx)).sum();
                    let sxy: f64 = xs.iter().zip(n).map(|(x, y)| (x - mx) * (y - my)).sum();
                    let slope = sxy / sxx;
                    (my - slope * mx, slope)
                });
                for d in &dst {
                    // Position of this cell relative to the source, positive or negative.
                    let offset = if vertical {
                        i64::from(d.row) - i64::from(source.start.row)
                    } else {
                        i64::from(d.col) - i64::from(source.start.col)
                    };
                    let len = src.len() as i64;
                    let pick = offset.rem_euclid(len) as usize;
                    let from_cell = src[pick];
                    let dr = i64::from(d.row) - i64::from(from_cell.row);
                    let dc = i64::from(d.col) - i64::from(from_cell.col);
                    let (input, style) = match &inputs[pick] {
                        None => (Input::Value(Value::Empty), Style::default()),
                        Some(data) => {
                            let input = match (&series, &data.input) {
                                (Some((a, b)), _) => Input::Value(Value::Number(
                                    crate::format::round_decimal(a + b * offset as f64, 12),
                                )),
                                (None, Input::Value(Value::Number(x)))
                                    if src.len() == 1
                                        && crate::format::is_date_format(&data.style.format) =>
                                {
                                    // A single date counts on by days.
                                    Input::Value(Value::Number(x + offset as f64))
                                }
                                (None, Input::Value(Value::Text(t))) => Input::Value(Value::Text(
                                    count_on(t, offset / len, &inputs, pick),
                                )),
                                (None, other) => Self::shifted_input(other, dr, dc),
                            };
                            (input, data.style.clone())
                        }
                    };
                    wb.put(sheet, *d, input, Some(style));
                }
            }
            let touched: Vec<_> = cells.iter().map(|c| (sheet, *c)).collect();
            wb.recalc(&touched);
            Ok(())
        })
    }

    // ----- structure -----

    fn shift_cells(&mut self, sheet: usize, rows: bool, at: u32, delta: i64) {
        let old = std::mem::take(&mut self.sheets[sheet].cells);
        let mut moved = BTreeMap::new();
        for (c, d) in old {
            let k = if rows { c.row } else { c.col };
            if delta < 0 && k >= at && i64::from(k) < i64::from(at) - delta {
                continue;
            }
            let nk = if k >= at {
                (i64::from(k) + delta) as u32
            } else {
                k
            };
            let limit = if rows { MAX_ROWS } else { MAX_COLS };
            if nk >= limit {
                continue;
            }
            let nc = if rows {
                Cell::new(nk, c.col)
            } else {
                Cell::new(c.row, nk)
            };
            moved.insert(nc, d);
        }
        self.sheets[sheet].cells = moved;
    }
    fn restructure(&mut self, sheet: usize, rows: bool, at: u32, delta: i64) -> Result<(), String> {
        self.check_sheet(sheet)?;
        let limit = if rows { MAX_ROWS } else { MAX_COLS };
        if at >= limit || delta == 0 {
            return Err("that position is outside the sheet".into());
        }
        self.book_edit(|wb| {
            wb.shift_cells(sheet, rows, at, delta);
            let removed = if delta < 0 {
                Some((at, (i64::from(at) - delta - 1) as u32))
            } else {
                None
            };
            // References to a deleted row become #REF!; a range loses the deleted part.
            let adjust = |k: u32| -> Option<u32> {
                match removed {
                    Some((a, b)) if (a..=b).contains(&k) => None,
                    _ if k >= at => {
                        let n = i64::from(k) + delta;
                        (n >= 0 && n < i64::from(limit)).then_some(n as u32)
                    }
                    _ => Some(k),
                }
            };
            let names: Vec<String> = wb.sheets.iter().map(|s| s.name.clone()).collect();
            for (si, s) in wb.sheets.iter_mut().enumerate() {
                for d in s.cells.values_mut() {
                    if let Input::Formula(fm) = &d.input {
                        let expr = remap_structure(
                            &fm.expr, si, sheet, &names, rows, &adjust, removed, delta,
                        );
                        d.input = Input::Formula(Formula { expr });
                    }
                }
            }
            for (sname, range) in wb.names.values_mut() {
                if wb
                    .sheets
                    .get(sheet)
                    .is_some_and(|s| s.name.eq_ignore_ascii_case(sname))
                {
                    if let Some(r) = shrink_range(*range, rows, &adjust, removed) {
                        *range = r;
                    }
                }
            }
            let s = &mut wb.sheets[sheet];
            if !rows {
                let widths = std::mem::take(&mut s.col_widths);
                s.col_widths = widths
                    .into_iter()
                    .filter_map(|(c, w)| adjust(c).map(|n| (n, w)))
                    .collect();
            }
            for chart in &mut s.charts {
                if let Some(r) = shrink_range(chart.range, rows, &adjust, removed) {
                    chart.range = r;
                }
                let a = if rows {
                    chart.anchor.row
                } else {
                    chart.anchor.col
                };
                if let Some(n) = adjust(a) {
                    if rows {
                        chart.anchor.row = n;
                    } else {
                        chart.anchor.col = n;
                    }
                }
            }
            if let Some(f) = &mut s.filter {
                match shrink_range(f.range, rows, &adjust, removed) {
                    Some(r) => f.range = r,
                    None => s.filter = None,
                }
            }
            Ok(())
        })
    }
    pub fn insert_rows(&mut self, sheet: usize, at: u32, count: u32) -> Result<(), String> {
        self.restructure(sheet, true, at, i64::from(count))
    }
    pub fn delete_rows(&mut self, sheet: usize, at: u32, count: u32) -> Result<(), String> {
        self.restructure(sheet, true, at, -i64::from(count))
    }
    pub fn insert_cols(&mut self, sheet: usize, at: u32, count: u32) -> Result<(), String> {
        self.restructure(sheet, false, at, i64::from(count))
    }
    pub fn delete_cols(&mut self, sheet: usize, at: u32, count: u32) -> Result<(), String> {
        self.restructure(sheet, false, at, -i64::from(count))
    }
    /// Sort the rows of `range` by one column. A header row stays on top. Formulas in
    /// the sorted rows move with them, their relative references following the move.
    pub fn sort(
        &mut self,
        sheet: usize,
        range: Range,
        by_col: u32,
        ascending: bool,
        header: bool,
    ) -> Result<(), String> {
        self.check_sheet(sheet)?;
        if !(range.start.col..=range.end.col).contains(&by_col) {
            return Err("the sort column is outside the range".into());
        }
        let first = range.start.row + u32::from(header);
        if first > range.end.row {
            return Ok(());
        }
        let body = Range::new(Cell::new(first, range.start.col), range.end);
        let cells: Vec<Cell> = body.cells().collect();
        self.cells_edit(sheet, cells.clone(), |wb| {
            let mut rows: Vec<u32> = (first..=range.end.row).collect();
            rows.sort_by(|a, b| {
                let va = wb.value(sheet, Cell::new(*a, by_col));
                let vb = wb.value(sheet, Cell::new(*b, by_col));
                // Blanks go last either way, as Excel sorts them.
                match (va.is_empty(), vb.is_empty()) {
                    (true, true) => std::cmp::Ordering::Equal,
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    _ => {
                        let o = compare(&va, &vb);
                        if ascending {
                            o
                        } else {
                            o.reverse()
                        }
                    }
                }
            });
            let snapshot: Vec<Vec<Option<CellData>>> = rows
                .iter()
                .map(|r| {
                    (range.start.col..=range.end.col)
                        .map(|c| wb.cell(sheet, Cell::new(*r, c)).cloned())
                        .collect()
                })
                .collect();
            for (i, (src_row, row)) in rows.iter().zip(snapshot).enumerate() {
                let dst_row = first + i as u32;
                let dr = i64::from(dst_row) - i64::from(*src_row);
                for (k, data) in row.into_iter().enumerate() {
                    let c = Cell::new(dst_row, range.start.col + k as u32);
                    match data {
                        Some(d) => wb.put(
                            sheet,
                            c,
                            Self::shifted_input(&d.input, dr, 0),
                            Some(d.style),
                        ),
                        None => {
                            wb.put(sheet, c, Input::Value(Value::Empty), Some(Style::default()))
                        }
                    }
                }
            }
            let touched: Vec<_> = cells.iter().map(|c| (sheet, *c)).collect();
            wb.recalc(&touched);
            Ok(())
        })
    }
    /// Turn an auto filter on over `range` (header in its first row), or off.
    pub fn set_filter(&mut self, sheet: usize, range: Option<Range>) -> Result<(), String> {
        self.check_sheet(sheet)?;
        self.book_edit(|wb| {
            wb.sheets[sheet].filter = range.map(|range| AutoFilter {
                range,
                hidden: BTreeMap::new(),
            });
            Ok(())
        })
    }
    /// Values a filter column offers, as displayed, in order.
    pub fn filter_values(&self, sheet: usize, col: u32) -> Vec<String> {
        let Some(f) = self.sheets.get(sheet).and_then(|s| s.filter.as_ref()) else {
            return vec![];
        };
        let mut vals: Vec<(Value, String)> = Vec::new();
        for row in f.range.start.row + 1..=f.range.end.row {
            let c = Cell::new(row, col);
            let shown = self.display(sheet, c);
            if !vals.iter().any(|(_, s)| *s == shown) {
                vals.push((self.value(sheet, c), shown));
            }
        }
        vals.sort_by(|a, b| compare(&a.0, &b.0));
        vals.into_iter().map(|(_, s)| s).collect()
    }
    /// Show or hide one value in a filter column.
    pub fn filter_toggle(&mut self, sheet: usize, col: u32, value: &str) -> Result<(), String> {
        self.check_sheet(sheet)?;
        if self.sheets[sheet].filter.is_none() {
            return Err("there is no filter on this sheet".into());
        }
        self.book_edit(|wb| {
            let f = wb.sheets[sheet].filter.as_mut().expect("checked");
            if !(f.range.start.col..=f.range.end.col).contains(&col) {
                return Err("that column is outside the filter".into());
            }
            let set = f.hidden.entry(col).or_default();
            if !set.remove(value) {
                set.insert(value.to_owned());
            }
            if set.is_empty() {
                f.hidden.remove(&col);
            }
            Ok(())
        })
    }
    pub fn set_freeze(&mut self, sheet: usize, rows: u32, cols: u32) -> Result<(), String> {
        self.check_sheet(sheet)?;
        self.book_edit(|wb| {
            wb.sheets[sheet].freeze = (rows, cols);
            Ok(())
        })
    }
    pub fn set_col_width(&mut self, sheet: usize, col: u32, px: u32) -> Result<(), String> {
        self.check_sheet(sheet)?;
        self.book_edit(|wb| {
            if px == DEFAULT_COL_WIDTH {
                wb.sheets[sheet].col_widths.remove(&col);
            } else {
                wb.sheets[sheet].col_widths.insert(col, px.clamp(8, 1200));
            }
            Ok(())
        })
    }

    // ----- sheets and names -----

    /// A new sheet at the end, named `Sheet<n>` (or `name`).
    pub fn add_sheet(&mut self, name: Option<&str>) -> Result<usize, String> {
        let name = match name {
            Some(n) => {
                validate_sheet_name(n)?;
                if self.sheet_index(n).is_some() {
                    return Err(format!("a sheet named {n} already exists"));
                }
                n.to_owned()
            }
            None => (1..)
                .map(|i| format!("Sheet{i}"))
                .find(|n| self.sheet_index(n).is_none())
                .expect("unbounded"),
        };
        self.book_edit(|wb| {
            wb.sheets.push(Sheet::new(name));
            Ok(wb.sheets.len() - 1)
        })
    }
    pub fn rename_sheet(&mut self, sheet: usize, name: &str) -> Result<(), String> {
        self.check_sheet(sheet)?;
        validate_sheet_name(name)?;
        if self.sheet_index(name).is_some_and(|i| i != sheet) {
            return Err(format!("a sheet named {name} already exists"));
        }
        let old = self.sheets[sheet].name.clone();
        self.book_edit(|wb| {
            wb.sheets[sheet].name = name.to_owned();
            for s in wb.sheets.iter_mut() {
                for d in s.cells.values_mut() {
                    if let Input::Formula(f) = &d.input {
                        d.input = Input::Formula(Formula {
                            expr: rename_sheet_refs(&f.expr, &old, name),
                        });
                    }
                }
            }
            for (s, _) in wb.names.values_mut() {
                if s.eq_ignore_ascii_case(&old) {
                    *s = name.to_owned();
                }
            }
            Ok(())
        })
    }
    pub fn remove_sheet(&mut self, sheet: usize) -> Result<(), String> {
        self.check_sheet(sheet)?;
        if self.sheets.len() == 1 {
            return Err("a workbook must contain at least one visible worksheet".into());
        }
        let old = self.sheets[sheet].name.clone();
        self.book_edit(|wb| {
            wb.sheets.remove(sheet);
            for s in wb.sheets.iter_mut() {
                for d in s.cells.values_mut() {
                    if let Input::Formula(f) = &d.input {
                        d.input = Input::Formula(Formula {
                            expr: drop_sheet_refs(&f.expr, &old),
                        });
                    }
                }
            }
            wb.names.retain(|_, (s, _)| !s.eq_ignore_ascii_case(&old));
            Ok(())
        })
    }
    pub fn define_name(&mut self, name: &str, sheet: usize, range: Range) -> Result<(), String> {
        self.check_sheet(sheet)?;
        let ok = name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '\\')
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '\\')
            && crate::address::CellRef::parse(name).is_none()
            && !name.eq_ignore_ascii_case("TRUE")
            && !name.eq_ignore_ascii_case("FALSE");
        if !ok {
            return Err(format!("{name} is not a valid name"));
        }
        let sheet_name = self.sheets[sheet].name.clone();
        self.book_edit(|wb| {
            wb.names.retain(|k, _| !k.eq_ignore_ascii_case(name));
            wb.names.insert(name.to_owned(), (sheet_name, range));
            Ok(())
        })
    }
    pub fn remove_name(&mut self, name: &str) -> Result<(), String> {
        let key = self
            .names
            .keys()
            .find(|k| k.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or("no such name")?;
        self.book_edit(|wb| {
            wb.names.remove(&key);
            Ok(())
        })
    }

    // ----- charts -----

    pub fn add_chart(
        &mut self,
        sheet: usize,
        kind: ChartKind,
        range: Range,
        title: &str,
    ) -> Result<usize, String> {
        self.check_sheet(sheet)?;
        let right = range.end.col + 2;
        self.book_edit(|wb| {
            let s = &mut wb.sheets[sheet];
            s.charts.push(Chart {
                kind,
                range,
                title: title.to_owned(),
                anchor: Cell::new(range.start.row, right.min(MAX_COLS - 8)),
                cols: 7,
                rows: 15,
            });
            Ok(s.charts.len() - 1)
        })
    }
    pub fn remove_chart(&mut self, sheet: usize, index: usize) -> Result<(), String> {
        self.check_sheet(sheet)?;
        if index >= self.sheets[sheet].charts.len() {
            return Err("no such chart".into());
        }
        self.book_edit(|wb| {
            wb.sheets[sheet].charts.remove(index);
            Ok(())
        })
    }
    pub fn set_chart_kind(
        &mut self,
        sheet: usize,
        index: usize,
        kind: ChartKind,
    ) -> Result<(), String> {
        self.check_sheet(sheet)?;
        if index >= self.sheets[sheet].charts.len() {
            return Err("no such chart".into());
        }
        self.book_edit(|wb| {
            wb.sheets[sheet].charts[index].kind = kind;
            Ok(())
        })
    }
    /// What a chart plots. A first row of text names the series and a first column of
    /// text (or dates) names the categories; series run down columns when the data is
    /// taller than it is wide, as Excel lays them out.
    /// How a chart reads its range: (first row names series, first column labels
    /// categories, series run down columns).
    pub fn chart_layout(&self, sheet: usize, chart: &Chart) -> (bool, bool, bool) {
        let r = chart.range;
        let v = |row: u32, col: u32| self.value(sheet, Cell::new(row, col));
        let text_like = |x: &Value| matches!(x, Value::Text(_) | Value::Empty);
        let header_row = r.rows() > 1
            && (r.start.col..=r.end.col).any(|c| matches!(v(r.start.row, c), Value::Text(_)))
            && (r.start.col..=r.end.col).all(|c| text_like(&v(r.start.row, c)) || c == r.start.col);
        let body = r.start.row + u32::from(header_row)..=r.end.row;
        let label_col = r.cols() > 1
            && (body
                .clone()
                .all(|row| !matches!(v(row, r.start.col), Value::Number(_)))
                || body.clone().all(|row| {
                    crate::format::is_date_format(
                        &self.style(sheet, Cell::new(row, r.start.col)).format,
                    )
                }));
        let rows = r.rows() - u32::from(header_row);
        let cols = r.cols() - u32::from(label_col);
        (
            header_row,
            label_col,
            rows >= cols || chart.kind == ChartKind::Pie,
        )
    }
    pub fn chart_data(&self, sheet: usize, chart: &Chart) -> ChartData {
        let r = chart.range;
        let v = |row: u32, col: u32| self.value(sheet, Cell::new(row, col));
        let (header_row, label_col, columns_are_series) = self.chart_layout(sheet, chart);
        let data_rows: Vec<u32> = (r.start.row + u32::from(header_row)..=r.end.row).collect();
        let data_cols: Vec<u32> = (r.start.col + u32::from(label_col)..=r.end.col).collect();
        let num = |x: Value| match x {
            Value::Number(n) => Some(n),
            _ => None,
        };
        let label = |row: u32, col: u32| self.display(sheet, Cell::new(row, col));
        if columns_are_series {
            let categories = data_rows
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    if label_col {
                        label(*row, r.start.col)
                    } else {
                        (i + 1).to_string()
                    }
                })
                .collect();
            let series = data_cols
                .iter()
                .enumerate()
                .map(|(i, col)| Series {
                    name: if header_row {
                        label(r.start.row, *col)
                    } else {
                        format!("Series{}", i + 1)
                    },
                    values: data_rows.iter().map(|row| num(v(*row, *col))).collect(),
                })
                .collect();
            ChartData { categories, series }
        } else {
            let categories = data_cols
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    if header_row {
                        label(r.start.row, *col)
                    } else {
                        (i + 1).to_string()
                    }
                })
                .collect();
            let series = data_rows
                .iter()
                .enumerate()
                .map(|(i, row)| Series {
                    name: if label_col {
                        label(*row, r.start.col)
                    } else {
                        format!("Series{}", i + 1)
                    },
                    values: data_cols.iter().map(|col| num(v(*row, *col))).collect(),
                })
                .collect();
            ChartData { categories, series }
        }
    }

    // ----- statistics -----

    pub fn stats(&self, sheet: usize, range: Range) -> Stats {
        let mut st = Stats::default();
        let Some(s) = self.sheets.get(sheet) else {
            return st;
        };
        let hidden = s.hidden_rows();
        let mut nums = Vec::new();
        for row in range.start.row
            ..=range
                .end
                .row
                .min(self.used_range(sheet).map_or(0, |u| u.end.row))
        {
            if hidden.contains(&row) {
                continue;
            }
            for (_, d) in s
                .cells
                .range(Cell::new(row, range.start.col)..=Cell::new(row, range.end.col))
            {
                match &d.value {
                    Value::Empty => {}
                    Value::Number(x) => {
                        st.count += 1;
                        nums.push(*x);
                    }
                    _ => st.count += 1,
                }
            }
        }
        st.numbers = nums.len();
        if !nums.is_empty() {
            let mut sum = 0.0;
            let mut c = 0.0;
            for x in &nums {
                let t = sum + x;
                c += if f64::abs(sum) >= x.abs() {
                    (sum - t) + x
                } else {
                    (x - t) + sum
                };
                sum = t;
            }
            st.sum = sum + c;
            st.average = Some(st.sum / nums.len() as f64);
            st.min = nums.iter().copied().reduce(f64::min);
            st.max = nums.iter().copied().reduce(f64::max);
        }
        st
    }
    /// Load a finished workbook's state after reading a file: rebuild the graph and
    /// compute every formula.
    pub fn loaded(mut self) -> Self {
        self.undo.clear();
        self.redo.clear();
        self.graph.0 = None;
        self.recalc_all();
        self
    }
    /// Set a cell's input and value directly, as a file reader does before
    /// [`Workbook::loaded`] computes everything.
    pub fn load_cell(&mut self, sheet: usize, c: Cell, input: Input, value: Value, style: Style) {
        let data = CellData {
            input,
            value,
            style,
        };
        if !data.is_blank() {
            self.sheets[sheet].cells.insert(c, data);
        }
    }
}
impl CellRef {
    /// Moved by an offset whatever its anchoring, as when the cells it names move.
    pub fn shifted_both(self, dr: i64, dc: i64) -> Option<Self> {
        let row = i64::from(self.row) + dr;
        let col = i64::from(self.col) + dc;
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
fn validate_sheet_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name.chars().count() > 31
        || name.contains(['\\', '/', '?', '*', '[', ']', ':'])
        || name.starts_with('\'')
        || name.ends_with('\'')
    {
        return Err(format!("{name} is not a valid sheet name"));
    }
    Ok(())
}
/// Text ending in a number counts on (`Item 1` → `Item 2`); month and weekday names
/// follow the calendar; other text repeats.
fn count_on(text: &str, step: i64, _inputs: &[Option<CellData>], _pick: usize) -> String {
    let lower = text.to_ascii_lowercase();
    for (list, n) in [
        (&crate::date::MONTHS[..], 12i64),
        (&crate::date::WEEKDAYS[..], 7),
    ] {
        for short in [false, true] {
            if let Some(i) = list.iter().position(|m| {
                let m = if short { &m[..3] } else { m };
                m.eq_ignore_ascii_case(&lower)
            }) {
                let next = list[(i as i64 + step).rem_euclid(n) as usize];
                let next = if short { &next[..3] } else { next };
                return match_case(text, next);
            }
        }
    }
    let digits: String = text
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if !digits.is_empty() && digits.len() < text.len() {
        let stem = &text[..text.len() - digits.len()];
        if let Ok(k) = digits.parse::<i64>() {
            let next = (k + step).max(0);
            return format!(
                "{stem}{next:0width$}",
                width = if digits.starts_with('0') {
                    digits.len()
                } else {
                    0
                }
            );
        }
    }
    text.to_owned()
}
fn match_case(model: &str, word: &str) -> String {
    if model.chars().all(|c| !c.is_lowercase()) {
        word.to_uppercase()
    } else if model.chars().all(|c| !c.is_uppercase()) {
        word.to_lowercase()
    } else {
        word.to_owned()
    }
}
/// Members of every non-trivial strongly connected component among `nodes`
/// (iterative Tarjan), plus nodes that read themselves.
fn cycle_members(
    nodes: &[(usize, Cell)],
    edges: &BTreeMap<(usize, Cell), Vec<(usize, Cell)>>,
) -> BTreeSet<(usize, Cell)> {
    let set: BTreeSet<(usize, Cell)> = nodes.iter().copied().collect();
    let mut index: BTreeMap<(usize, Cell), usize> = BTreeMap::new();
    let mut low: BTreeMap<(usize, Cell), usize> = BTreeMap::new();
    let mut on_stack: BTreeSet<(usize, Cell)> = BTreeSet::new();
    let mut stack: Vec<(usize, Cell)> = Vec::new();
    let mut out = BTreeSet::new();
    let mut counter = 0;
    let succ = |n: &(usize, Cell)| -> Vec<(usize, Cell)> {
        edges
            .get(n)
            .map(|v| v.iter().filter(|x| set.contains(x)).copied().collect())
            .unwrap_or_default()
    };
    for &root in nodes {
        if index.contains_key(&root) {
            continue;
        }
        let mut work: Vec<((usize, Cell), usize)> = vec![(root, 0)];
        while let Some((node, child)) = work.pop() {
            if child == 0 {
                index.insert(node, counter);
                low.insert(node, counter);
                counter += 1;
                stack.push(node);
                on_stack.insert(node);
            }
            let next = succ(&node);
            if let Some(w) = next.get(child) {
                work.push((node, child + 1));
                if !index.contains_key(w) {
                    work.push((*w, 0));
                } else if on_stack.contains(w) {
                    let l = low[&node].min(index[w]);
                    low.insert(node, l);
                }
                continue;
            }
            if low[&node] == index[&node] {
                let mut comp = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack.remove(&w);
                    comp.push(w);
                    if w == node {
                        break;
                    }
                }
                let self_loop = next.contains(&node);
                if comp.len() > 1 || self_loop {
                    out.extend(comp);
                }
            }
            if let Some((parent, _)) = work.last() {
                let l = low[parent].min(low[&node]);
                low.insert(*parent, l);
            }
        }
    }
    out
}
fn shrink_range(
    r: Range,
    rows: bool,
    adjust: &dyn Fn(u32) -> Option<u32>,
    removed: Option<(u32, u32)>,
) -> Option<Range> {
    let (a, b) = if rows {
        (r.start.row, r.end.row)
    } else {
        (r.start.col, r.end.col)
    };
    let na = adjust(a)
        .or_else(|| removed.and_then(|(_, hi)| (hi < b).then(|| adjust(hi + 1)).flatten()));
    let nb = adjust(b)
        .or_else(|| removed.and_then(|(lo, _)| (lo > a).then(|| adjust(lo - 1)).flatten()));
    let (na, nb) = (na?, nb?);
    Some(if rows {
        Range::new(Cell::new(na, r.start.col), Cell::new(nb, r.end.col))
    } else {
        Range::new(Cell::new(r.start.row, na), Cell::new(r.end.row, nb))
    })
}
#[allow(clippy::too_many_arguments)]
fn remap_structure(
    e: &Expr,
    formula_sheet: usize,
    changed: usize,
    names: &[String],
    rows: bool,
    adjust: &dyn Fn(u32) -> Option<u32>,
    removed: Option<(u32, u32)>,
    _delta: i64,
) -> Expr {
    let target = |name: &Option<String>| match name {
        None => Some(formula_sheet),
        Some(n) => names.iter().position(|x| x.eq_ignore_ascii_case(n)),
    };
    let rec = |x: &Expr| {
        remap_structure(
            x,
            formula_sheet,
            changed,
            names,
            rows,
            adjust,
            removed,
            _delta,
        )
    };
    match e {
        Expr::Ref { sheet, cell } if target(sheet) == Some(changed) => {
            let k = if rows { cell.row } else { cell.col };
            match adjust(k) {
                Some(n) => {
                    let mut c = *cell;
                    if rows {
                        c.row = n;
                    } else {
                        c.col = n;
                    }
                    Expr::Ref {
                        sheet: sheet.clone(),
                        cell: c,
                    }
                }
                None => Expr::Error(ErrorKind::Ref),
            }
        }
        Expr::Range {
            sheet,
            start,
            end,
            kind,
        } if target(sheet) == Some(changed) => {
            // Whole columns survive row edits and whole rows survive column edits.
            if (rows && *kind == parser::RangeKind::Columns)
                || (!rows && *kind == parser::RangeKind::Rows)
            {
                return e.clone();
            }
            match shrink_range(Range::new(start.cell(), end.cell()), rows, adjust, removed) {
                Some(r) => {
                    let (mut s, mut t) = (*start, *end);
                    if rows {
                        s.row = r.start.row;
                        t.row = r.end.row;
                    } else {
                        s.col = r.start.col;
                        t.col = r.end.col;
                    }
                    Expr::Range {
                        sheet: sheet.clone(),
                        start: s,
                        end: t,
                        kind: *kind,
                    }
                }
                None => Expr::Error(ErrorKind::Ref),
            }
        }
        Expr::Neg(a) => Expr::Neg(Box::new(rec(a))),
        Expr::Plus(a) => Expr::Plus(Box::new(rec(a))),
        Expr::Percent(a) => Expr::Percent(Box::new(rec(a))),
        Expr::Group(a) => Expr::Group(Box::new(rec(a))),
        Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(rec(a)), Box::new(rec(b))),
        Expr::Call(n, args) => Expr::Call(n.clone(), args.iter().map(rec).collect()),
        other => other.clone(),
    }
}
fn rename_sheet_refs(e: &Expr, old: &str, new: &str) -> Expr {
    let fix = |s: &Option<String>| match s {
        Some(n) if n.eq_ignore_ascii_case(old) => Some(new.to_owned()),
        other => other.clone(),
    };
    let rec = |x: &Expr| rename_sheet_refs(x, old, new);
    match e {
        Expr::Ref { sheet, cell } => Expr::Ref {
            sheet: fix(sheet),
            cell: *cell,
        },
        Expr::Range {
            sheet,
            start,
            end,
            kind,
        } => Expr::Range {
            sheet: fix(sheet),
            start: *start,
            end: *end,
            kind: *kind,
        },
        Expr::Neg(a) => Expr::Neg(Box::new(rec(a))),
        Expr::Plus(a) => Expr::Plus(Box::new(rec(a))),
        Expr::Percent(a) => Expr::Percent(Box::new(rec(a))),
        Expr::Group(a) => Expr::Group(Box::new(rec(a))),
        Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(rec(a)), Box::new(rec(b))),
        Expr::Call(n, args) => Expr::Call(n.clone(), args.iter().map(rec).collect()),
        other => other.clone(),
    }
}
fn drop_sheet_refs(e: &Expr, old: &str) -> Expr {
    let gone = |s: &Option<String>| s.as_ref().is_some_and(|n| n.eq_ignore_ascii_case(old));
    let rec = |x: &Expr| drop_sheet_refs(x, old);
    match e {
        Expr::Ref { sheet, .. } | Expr::Range { sheet, .. } if gone(sheet) => {
            Expr::Error(ErrorKind::Ref)
        }
        Expr::Neg(a) => Expr::Neg(Box::new(rec(a))),
        Expr::Plus(a) => Expr::Plus(Box::new(rec(a))),
        Expr::Percent(a) => Expr::Percent(Box::new(rec(a))),
        Expr::Group(a) => Expr::Group(Box::new(rec(a))),
        Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(rec(a)), Box::new(rec(b))),
        Expr::Call(n, args) => Expr::Call(n.clone(), args.iter().map(rec).collect()),
        other => other.clone(),
    }
}
