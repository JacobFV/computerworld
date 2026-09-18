//! Conditional formatting, as Excel defines it: highlight-cell rules (cell value,
//! text, dates, duplicates, blanks, errors), top/bottom and above/below average rules,
//! formula rules, data bars, colour scales and icon sets. Rules apply in priority order
//! (first in the list first); a format rule's properties win over those of rules below
//! it, and `stop_if_true` ends the list for a cell it matched.
use crate::address::{Cell, Range};
use crate::parser::Formula;
use crate::value::{formula_compare, ErrorKind, Value};
use crate::workbook::Workbook;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// The format a matching rule applies (Excel's differential format).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dxf {
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    #[serde(default)]
    pub fill: Option<[u8; 3]>,
}
impl Dxf {
    /// Excel's six preset formats in the highlight dialogs.
    pub fn preset(name: &str) -> Option<Self> {
        let (fill, color) = match name {
            "lightred" => (Some([255, 199, 206]), Some([156, 0, 6])),
            "yellow" => (Some([255, 235, 156]), Some([156, 87, 0])),
            "green" => (Some([198, 239, 206]), Some([0, 97, 0])),
            "redfill" => (Some([255, 199, 206]), None),
            "redtext" => (None, Some([156, 0, 6])),
            "bold" => {
                return Some(Self {
                    bold: true,
                    ..Self::default()
                })
            }
            _ => return None,
        };
        Some(Self {
            fill,
            color,
            ..Self::default()
        })
    }
    /// Lay `self` (higher priority) over `below`.
    fn over(self, below: Self) -> Self {
        Self {
            bold: self.bold || below.bold,
            italic: self.italic || below.italic,
            underline: self.underline || below.underline,
            color: self.color.or(below.color),
            fill: self.fill.or(below.fill),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellOp {
    Between,
    NotBetween,
    Equal,
    NotEqual,
    Greater,
    Less,
    GreaterEqual,
    LessEqual,
}
impl CellOp {
    /// SpreadsheetML's `operator` attribute.
    pub fn name(self) -> &'static str {
        match self {
            Self::Between => "between",
            Self::NotBetween => "notBetween",
            Self::Equal => "equal",
            Self::NotEqual => "notEqual",
            Self::Greater => "greaterThan",
            Self::Less => "lessThan",
            Self::GreaterEqual => "greaterThanOrEqual",
            Self::LessEqual => "lessThanOrEqual",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Between,
            Self::NotBetween,
            Self::Equal,
            Self::NotEqual,
            Self::Greater,
            Self::Less,
            Self::GreaterEqual,
            Self::LessEqual,
        ]
        .into_iter()
        .find(|o| o.name() == s)
    }
    pub fn operands(self) -> usize {
        if matches!(self, Self::Between | Self::NotBetween) {
            2
        } else {
            1
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextOp {
    Contains,
    NotContains,
    BeginsWith,
    EndsWith,
}
impl TextOp {
    pub fn name(self) -> &'static str {
        match self {
            Self::Contains => "containsText",
            Self::NotContains => "notContains",
            Self::BeginsWith => "beginsWith",
            Self::EndsWith => "endsWith",
        }
    }
}
/// `A Date Occurring…` periods (SpreadsheetML's `timePeriod`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    Yesterday,
    Today,
    Tomorrow,
    Last7Days,
    LastWeek,
    ThisWeek,
    NextWeek,
    LastMonth,
    ThisMonth,
    NextMonth,
}
impl Period {
    pub const ALL: [Period; 10] = [
        Period::Yesterday,
        Period::Today,
        Period::Tomorrow,
        Period::Last7Days,
        Period::LastWeek,
        Period::ThisWeek,
        Period::NextWeek,
        Period::LastMonth,
        Period::ThisMonth,
        Period::NextMonth,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Yesterday => "yesterday",
            Self::Today => "today",
            Self::Tomorrow => "tomorrow",
            Self::Last7Days => "last7Days",
            Self::LastWeek => "lastWeek",
            Self::ThisWeek => "thisWeek",
            Self::NextWeek => "nextWeek",
            Self::LastMonth => "lastMonth",
            Self::ThisMonth => "thisMonth",
            Self::NextMonth => "nextMonth",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(s))
    }
    /// The inclusive range of serial days the period covers, given today's serial.
    fn days(self, today: f64) -> (f64, f64) {
        let t = today.floor();
        // Serial 1 (1900-01-01) was a Sunday; weeks run Sunday to Saturday.
        let dow = (t - 1.0).rem_euclid(7.0);
        let sunday = t - dow;
        let (y, m, _) = crate::date::ymd(t).unwrap_or((1900, 1, 1));
        let m = i64::from(m);
        let month = |y: i64, m: i64| -> (f64, f64) {
            let (y, m) = if m == 0 {
                (y - 1, 12)
            } else if m == 13 {
                (y + 1, 1)
            } else {
                (y, m)
            };
            let first = crate::date::serial(y, m, 1).unwrap_or(t);
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            let next = crate::date::serial(ny, nm, 1).unwrap_or(first + 31.0);
            (first, next - 1.0)
        };
        match self {
            Self::Yesterday => (t - 1.0, t - 1.0),
            Self::Today => (t, t),
            Self::Tomorrow => (t + 1.0, t + 1.0),
            Self::Last7Days => (t - 6.0, t),
            Self::LastWeek => (sunday - 7.0, sunday - 1.0),
            Self::ThisWeek => (sunday, sunday + 6.0),
            Self::NextWeek => (sunday + 7.0, sunday + 13.0),
            Self::LastMonth => month(y, m - 1),
            Self::ThisMonth => month(y, m),
            Self::NextMonth => month(y, m + 1),
        }
    }
}
/// A data bar's or colour scale's reference point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Cfvo {
    Min,
    Max,
    Num(f64),
    Percent(f64),
    Percentile(f64),
    Formula(String),
}
impl Eq for Cfvo {}
impl Cfvo {
    /// SpreadsheetML's `type` and `val`.
    pub fn xml(&self) -> (&'static str, Option<String>) {
        match self {
            Self::Min => ("min", None),
            Self::Max => ("max", None),
            Self::Num(x) => ("num", Some(crate::value::general(*x))),
            Self::Percent(x) => ("percent", Some(crate::value::general(*x))),
            Self::Percentile(x) => ("percentile", Some(crate::value::general(*x))),
            Self::Formula(f) => ("formula", Some(f.clone())),
        }
    }
    pub fn from_xml(kind: &str, val: Option<&str>) -> Option<Self> {
        let num = || val.and_then(|v| v.trim().parse::<f64>().ok());
        Some(match kind {
            "min" => Self::Min,
            "max" => Self::Max,
            "num" => match num() {
                Some(x) => Self::Num(x),
                None => Self::Formula(val?.to_owned()),
            },
            "percent" => Self::Percent(num()?),
            "percentile" => Self::Percentile(num()?),
            "formula" => Self::Formula(val?.to_owned()),
            _ => return None,
        })
    }
}
/// Excel's icon sets that this engine draws.
pub const ICON_SETS: [(&str, usize); 8] = [
    ("3Arrows", 3),
    ("3ArrowsGray", 3),
    ("3TrafficLights1", 3),
    ("3Symbols", 3),
    ("3Flags", 3),
    ("4Arrows", 4),
    ("4TrafficLights", 4),
    ("5Arrows", 5),
];
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rule {
    /// `Cell Value` compared with one or two formulas (relative to the range's top-left).
    CellIs {
        op: CellOp,
        formulas: Vec<String>,
        style: Dxf,
    },
    Text {
        op: TextOp,
        text: String,
        style: Dxf,
    },
    Dates {
        period: Period,
        style: Dxf,
    },
    Duplicates {
        unique: bool,
        style: Dxf,
    },
    Top {
        bottom: bool,
        rank: u32,
        percent: bool,
        style: Dxf,
    },
    Average {
        below: bool,
        equal: bool,
        style: Dxf,
    },
    Blanks {
        blanks: bool,
        style: Dxf,
    },
    Errors {
        errors: bool,
        style: Dxf,
    },
    /// `Use a formula to determine which cells to format`.
    Expression {
        formula: String,
        style: Dxf,
    },
    DataBar {
        color: [u8; 3],
        min: Cfvo,
        max: Cfvo,
    },
    ColorScale {
        stops: Vec<(Cfvo, [u8; 3])>,
    },
    IconSet {
        set: String,
        points: Vec<Cfvo>,
        #[serde(default)]
        reverse: bool,
        #[serde(default = "yes")]
        show_value: bool,
    },
}
fn yes() -> bool {
    true
}
impl Rule {
    pub fn style(&self) -> Option<Dxf> {
        match self {
            Self::CellIs { style, .. }
            | Self::Text { style, .. }
            | Self::Dates { style, .. }
            | Self::Duplicates { style, .. }
            | Self::Top { style, .. }
            | Self::Average { style, .. }
            | Self::Blanks { style, .. }
            | Self::Errors { style, .. }
            | Self::Expression { style, .. } => Some(*style),
            _ => None,
        }
    }
    /// How Excel's Rules Manager describes the rule.
    pub fn describe(&self) -> String {
        match self {
            Self::CellIs { op, formulas, .. } => {
                let f = |i: usize| formulas.get(i).cloned().unwrap_or_default();
                match op {
                    CellOp::Between => format!("Cell Value between {} and {}", f(0), f(1)),
                    CellOp::NotBetween => {
                        format!("Cell Value not between {} and {}", f(0), f(1))
                    }
                    CellOp::Equal => format!("Cell Value = {}", f(0)),
                    CellOp::NotEqual => format!("Cell Value <> {}", f(0)),
                    CellOp::Greater => format!("Cell Value > {}", f(0)),
                    CellOp::Less => format!("Cell Value < {}", f(0)),
                    CellOp::GreaterEqual => format!("Cell Value >= {}", f(0)),
                    CellOp::LessEqual => format!("Cell Value <= {}", f(0)),
                }
            }
            Self::Text { op, text, .. } => match op {
                TextOp::Contains => format!("Cell Value contains \"{text}\""),
                TextOp::NotContains => format!("Cell Value does not contain \"{text}\""),
                TextOp::BeginsWith => format!("Cell Value begins with \"{text}\""),
                TextOp::EndsWith => format!("Cell Value ends with \"{text}\""),
            },
            Self::Dates { period, .. } => format!("A date occurring {}", period.name()),
            Self::Duplicates { unique: false, .. } => "Duplicate Values".into(),
            Self::Duplicates { unique: true, .. } => "Unique Values".into(),
            Self::Top {
                bottom,
                rank,
                percent,
                ..
            } => format!(
                "{} {rank}{}",
                if *bottom { "Bottom" } else { "Top" },
                if *percent { "%" } else { "" }
            ),
            Self::Average { below, equal, .. } => format!(
                "{}{} Average",
                if *below { "Below" } else { "Above" },
                if *equal { " or Equal to" } else { "" }
            ),
            Self::Blanks { blanks: true, .. } => "Blanks".into(),
            Self::Blanks { blanks: false, .. } => "No Blanks".into(),
            Self::Errors { errors: true, .. } => "Errors".into(),
            Self::Errors { errors: false, .. } => "No Errors".into(),
            Self::Expression { formula, .. } => format!("Formula: ={formula}"),
            Self::DataBar { .. } => "Data Bar".into(),
            Self::ColorScale { stops } => format!("Graded Color Scale ({} colors)", stops.len()),
            Self::IconSet { set, .. } => format!("Icon Set {set}"),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CondFormat {
    /// Where it applies; formulas are written for the first range's top-left cell.
    pub ranges: Vec<Range>,
    pub rule: Rule,
    #[serde(default)]
    pub stop_if_true: bool,
}
impl CondFormat {
    pub fn new(range: Range, rule: Rule) -> Self {
        Self {
            ranges: vec![range],
            rule,
            stop_if_true: false,
        }
    }
    pub fn applies_to(&self, c: Cell) -> bool {
        self.ranges.iter().any(|r| r.contains(c))
    }
    pub fn sqref(&self) -> String {
        self.ranges
            .iter()
            .map(|r| r.a1())
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn origin(&self) -> Cell {
        self.ranges.first().map_or(Cell::new(0, 0), |r| r.start)
    }
}
/// What conditional formatting does to one cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effect {
    pub style: Dxf,
    /// Data bar: its length as a fraction of the cell, and colour.
    pub bar: Option<(f64, [u8; 3])>,
    /// Colour scale fill.
    pub scale: Option<[u8; 3]>,
    /// Icon: its set, which icon (0 is the lowest), and how many the set has.
    pub icon: Option<(String, usize, usize)>,
    /// An icon set can hide the value itself.
    pub hide_value: bool,
}

/// Statistics of a rule's cells, computed once for every cell it formats.
struct Stats {
    numbers: Vec<f64>,
    /// Displayed values in lower case and how often each occurs.
    counts: BTreeMap<String, usize>,
}
fn cells_of(wb: &Workbook, sheet: usize, cf: &CondFormat) -> Vec<Cell> {
    let used = wb.used_range(sheet);
    let mut out = Vec::new();
    for r in &cf.ranges {
        let Some(r) = used.and_then(|u| u.intersect(r)) else {
            continue;
        };
        if u64::from(r.rows()) * u64::from(r.cols()) > 1_000_000 {
            continue;
        }
        out.extend(r.cells());
    }
    out
}
fn stats(wb: &Workbook, sheet: usize, cells: &[Cell]) -> Stats {
    let mut numbers = Vec::new();
    let mut counts = BTreeMap::new();
    for c in cells {
        let v = wb.value(sheet, *c);
        if let Value::Number(n) = v {
            numbers.push(n);
        }
        if !v.is_empty() {
            *counts
                .entry(wb.display(sheet, *c).to_lowercase())
                .or_insert(0) += 1;
        }
    }
    numbers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    Stats { numbers, counts }
}
/// Evaluate a rule formula as if it were written in `at`.
fn formula_value(wb: &Workbook, sheet: usize, origin: Cell, at: Cell, text: &str) -> Value {
    let text = text.trim_start_matches('=');
    let Ok(f) = Formula::parse(text) else {
        return Value::Error(ErrorKind::Name);
    };
    let moved = Workbook::shift_formula(
        &f,
        i64::from(at.row) - i64::from(origin.row),
        i64::from(at.col) - i64::from(origin.col),
    );
    crate::eval::Eval::new(wb, sheet, at).cell_result(&moved.expr)
}
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0).clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo as f64)
}
fn point(wb: &Workbook, sheet: usize, cf: &CondFormat, st: &Stats, v: &Cfvo) -> f64 {
    let (lo, hi) = (
        st.numbers.first().copied().unwrap_or(0.0),
        st.numbers.last().copied().unwrap_or(0.0),
    );
    match v {
        Cfvo::Min => lo,
        Cfvo::Max => hi,
        Cfvo::Num(x) => *x,
        Cfvo::Percent(p) => lo + (hi - lo) * p / 100.0,
        Cfvo::Percentile(p) => percentile(&st.numbers, *p),
        Cfvo::Formula(f) => match formula_value(wb, sheet, cf.origin(), cf.origin(), f) {
            Value::Number(n) => n,
            _ => lo,
        },
    }
}
fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let ch = |i: usize| (f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * t).round() as u8;
    [ch(0), ch(1), ch(2)]
}
/// Whether a format rule matches the value at `c`.
fn matches(wb: &Workbook, sheet: usize, cf: &CondFormat, st: &Stats, c: Cell) -> bool {
    let v = wb.value(sheet, c);
    let cmp = |a: &Value, b: &Value| formula_compare(a, b).unwrap_or(Ordering::Equal);
    let cell_value = || match &v {
        Value::Empty => Value::Number(0.0),
        other => other.clone(),
    };
    match &cf.rule {
        Rule::CellIs { op, formulas, .. } => {
            if v.error().is_some() {
                return false;
            }
            let arg = |i: usize| {
                formula_value(
                    wb,
                    sheet,
                    cf.origin(),
                    c,
                    formulas.get(i).map_or("", String::as_str),
                )
            };
            let x = cell_value();
            let a = arg(0);
            match op {
                CellOp::Between | CellOp::NotBetween => {
                    let b = arg(1);
                    let (lo, hi) = if cmp(&a, &b) == Ordering::Greater {
                        (b, a)
                    } else {
                        (a, b)
                    };
                    let inside =
                        cmp(&x, &lo) != Ordering::Less && cmp(&x, &hi) != Ordering::Greater;
                    inside == (*op == CellOp::Between)
                }
                CellOp::Equal => cmp(&x, &a) == Ordering::Equal,
                CellOp::NotEqual => cmp(&x, &a) != Ordering::Equal,
                CellOp::Greater => cmp(&x, &a) == Ordering::Greater,
                CellOp::Less => cmp(&x, &a) == Ordering::Less,
                CellOp::GreaterEqual => cmp(&x, &a) != Ordering::Less,
                CellOp::LessEqual => cmp(&x, &a) != Ordering::Greater,
            }
        }
        Rule::Text { op, text, .. } => {
            if v.error().is_some() {
                return false;
            }
            let shown = wb.display(sheet, c).to_lowercase();
            let t = text.to_lowercase();
            match op {
                TextOp::Contains => shown.contains(&t),
                TextOp::NotContains => !shown.contains(&t),
                TextOp::BeginsWith => shown.starts_with(&t),
                TextOp::EndsWith => shown.ends_with(&t),
            }
        }
        Rule::Dates { period, .. } => match v {
            Value::Number(n) => {
                let (a, b) = period.days(wb.now());
                (a..=b + 0.999_999_999).contains(&n.floor())
            }
            _ => false,
        },
        Rule::Duplicates { unique, .. } => {
            if v.is_empty() {
                return false;
            }
            let n = st
                .counts
                .get(&wb.display(sheet, c).to_lowercase())
                .copied()
                .unwrap_or(0);
            (n > 1) != *unique
        }
        Rule::Top {
            bottom,
            rank,
            percent,
            ..
        } => {
            let Value::Number(x) = v else {
                return false;
            };
            let n = st.numbers.len();
            if n == 0 {
                return false;
            }
            let k = if *percent {
                ((n as f64 * f64::from(*rank) / 100.0).floor() as usize).max(1)
            } else {
                *rank as usize
            }
            .clamp(1, n);
            if *bottom {
                x <= st.numbers[k - 1]
            } else {
                x >= st.numbers[n - k]
            }
        }
        Rule::Average { below, equal, .. } => {
            let Value::Number(x) = v else {
                return false;
            };
            if st.numbers.is_empty() {
                return false;
            }
            let avg = st.numbers.iter().sum::<f64>() / st.numbers.len() as f64;
            match (below, equal) {
                (false, false) => x > avg,
                (false, true) => x >= avg,
                (true, false) => x < avg,
                (true, true) => x <= avg,
            }
        }
        Rule::Blanks { blanks, .. } => {
            let blank = match &v {
                Value::Empty => true,
                Value::Text(t) => t.trim().is_empty(),
                _ => false,
            };
            blank == *blanks
        }
        Rule::Errors { errors, .. } => v.error().is_some() == *errors,
        Rule::Expression { formula, .. } => {
            match formula_value(wb, sheet, cf.origin(), c, formula) {
                Value::Bool(b) => b,
                Value::Number(n) => n != 0.0,
                _ => false,
            }
        }
        _ => false,
    }
}

impl Workbook {
    /// Conditional formatting of every cell in `window` that any rule touches.
    pub fn conditional_effects(&self, sheet: usize, window: Range) -> BTreeMap<Cell, Effect> {
        let mut out: BTreeMap<Cell, Effect> = BTreeMap::new();
        let Some(s) = self.sheets.get(sheet) else {
            return out;
        };
        let mut stopped: std::collections::BTreeSet<Cell> = Default::default();
        for cf in &s.conditional {
            let targets: Vec<Cell> = cf
                .ranges
                .iter()
                .filter_map(|r| r.intersect(&window))
                .flat_map(|r| r.cells().collect::<Vec<_>>())
                .collect();
            if targets.is_empty() {
                continue;
            }
            let st = stats(self, sheet, &cells_of(self, sheet, cf));
            for c in targets {
                if stopped.contains(&c) {
                    continue;
                }
                let e = out.entry(c).or_default();
                let hit = match &cf.rule {
                    Rule::DataBar { color, min, max } => match self.value(sheet, c) {
                        Value::Number(x) if e.bar.is_none() => {
                            let (lo, hi) = (
                                point(self, sheet, cf, &st, min),
                                point(self, sheet, cf, &st, max),
                            );
                            let f = if hi > lo { (x - lo) / (hi - lo) } else { 1.0 };
                            e.bar = Some((f.clamp(0.0, 1.0), *color));
                            true
                        }
                        _ => false,
                    },
                    Rule::ColorScale { stops } => match self.value(sheet, c) {
                        Value::Number(x) if e.scale.is_none() && stops.len() >= 2 => {
                            let pts: Vec<(f64, [u8; 3])> = stops
                                .iter()
                                .map(|(v, col)| (point(self, sheet, cf, &st, v), *col))
                                .collect();
                            let mut color = pts[0].1;
                            if x >= pts[pts.len() - 1].0 {
                                color = pts[pts.len() - 1].1;
                            } else {
                                for w in pts.windows(2) {
                                    if x >= w[0].0 && x <= w[1].0 {
                                        let t = if w[1].0 > w[0].0 {
                                            (x - w[0].0) / (w[1].0 - w[0].0)
                                        } else {
                                            0.0
                                        };
                                        color = mix(w[0].1, w[1].1, t);
                                        break;
                                    }
                                }
                            }
                            e.scale = Some(color);
                            true
                        }
                        _ => false,
                    },
                    Rule::IconSet {
                        set,
                        points,
                        reverse,
                        show_value,
                    } => match self.value(sheet, c) {
                        Value::Number(x) if e.icon.is_none() && !points.is_empty() => {
                            let mut idx = 0;
                            for (i, p) in points.iter().enumerate() {
                                if x >= point(self, sheet, cf, &st, p) {
                                    idx = i;
                                }
                            }
                            let n = points.len();
                            let idx = if *reverse { n - 1 - idx } else { idx };
                            e.icon = Some((set.clone(), idx, n));
                            e.hide_value = !show_value;
                            true
                        }
                        _ => false,
                    },
                    rule => {
                        let ok = matches(self, sheet, cf, &st, c);
                        if ok {
                            // Rules earlier in the list win: lay what is already there
                            // over this rule's format.
                            e.style = e.style.over(rule.style().unwrap_or_default());
                        }
                        ok
                    }
                };
                if hit && cf.stop_if_true {
                    stopped.insert(c);
                }
            }
        }
        out.retain(|_, e| *e != Effect::default());
        out
    }
    fn conditional_edit(
        &mut self,
        sheet: usize,
        f: impl FnOnce(&mut Vec<CondFormat>) -> Result<(), String>,
    ) -> Result<(), String> {
        if sheet >= self.sheets.len() {
            return Err("no such sheet".into());
        }
        self.book_edit(|wb| f(&mut wb.sheets[sheet].conditional))
    }
    /// A new rule, at the top of the list (as Excel adds them).
    pub fn add_conditional(&mut self, sheet: usize, cf: CondFormat) -> Result<(), String> {
        validate(&cf)?;
        self.conditional_edit(sheet, |list| {
            list.insert(0, cf);
            Ok(())
        })
    }
    pub fn remove_conditional(&mut self, sheet: usize, index: usize) -> Result<(), String> {
        self.conditional_edit(sheet, |list| {
            if index >= list.len() {
                return Err("no such rule".into());
            }
            list.remove(index);
            Ok(())
        })
    }
    /// Move a rule up (earlier, higher priority) or down the list.
    pub fn move_conditional(&mut self, sheet: usize, index: usize, up: bool) -> Result<(), String> {
        self.conditional_edit(sheet, |list| {
            let to = if up {
                index.checked_sub(1).ok_or("that rule is already first")?
            } else {
                index + 1
            };
            if to >= list.len() || index >= list.len() {
                return Err("that rule is already last".into());
            }
            list.swap(index, to);
            Ok(())
        })
    }
    pub fn set_stop_if_true(&mut self, sheet: usize, index: usize, on: bool) -> Result<(), String> {
        self.conditional_edit(sheet, |list| {
            list.get_mut(index).ok_or("no such rule")?.stop_if_true = on;
            Ok(())
        })
    }
    /// Clear Rules: from the cells in `range`, or (with `None`) from the whole sheet.
    /// A rule over a larger area keeps the part outside the range.
    pub fn clear_conditional(&mut self, sheet: usize, range: Option<Range>) -> Result<(), String> {
        self.conditional_edit(sheet, |list| {
            match range {
                None => list.clear(),
                Some(r) => {
                    for cf in list.iter_mut() {
                        cf.ranges = cf.ranges.iter().flat_map(|x| subtract(*x, r)).collect();
                    }
                    list.retain(|cf| !cf.ranges.is_empty());
                }
            }
            Ok(())
        })
    }
}
fn validate(cf: &CondFormat) -> Result<(), String> {
    if cf.ranges.is_empty() {
        return Err("a rule needs cells to apply to".into());
    }
    let check = |f: &str| {
        Formula::parse(f.trim_start_matches('='))
            .map(|_| ())
            .map_err(|e| format!("There's a problem with this formula: {e}"))
    };
    match &cf.rule {
        Rule::CellIs { op, formulas, .. } => {
            if formulas.len() != op.operands() {
                return Err("the rule needs a value to compare with".into());
            }
            for f in formulas {
                check(f)?;
            }
        }
        Rule::Expression { formula, .. } => check(formula)?,
        Rule::Top { rank, percent, .. } => {
            if *rank == 0 || (*percent && *rank > 100) || *rank > 1000 {
                return Err(
                    "enter a whole number between 1 and 1000 (1 and 100 for a percent)".into(),
                );
            }
        }
        Rule::ColorScale { stops } if !(2..=3).contains(&stops.len()) => {
            return Err("a colour scale has two or three colours".into())
        }
        Rule::IconSet { set, points, .. } => {
            let n = ICON_SETS
                .iter()
                .find(|(s, _)| s == set)
                .map(|(_, n)| *n)
                .ok_or("unknown icon set")?;
            if points.len() != n {
                return Err("an icon set needs a threshold per icon".into());
            }
        }
        _ => {}
    }
    Ok(())
}
/// `a` without the cells of `b`, as up to four rectangles.
pub fn subtract(a: Range, b: Range) -> Vec<Range> {
    let Some(i) = a.intersect(&b) else {
        return vec![a];
    };
    let mut out = Vec::new();
    if i.start.row > a.start.row {
        out.push(Range::new(a.start, Cell::new(i.start.row - 1, a.end.col)));
    }
    if i.end.row < a.end.row {
        out.push(Range::new(Cell::new(i.end.row + 1, a.start.col), a.end));
    }
    if i.start.col > a.start.col {
        out.push(Range::new(
            Cell::new(i.start.row, a.start.col),
            Cell::new(i.end.row, i.start.col - 1),
        ));
    }
    if i.end.col < a.end.col {
        out.push(Range::new(
            Cell::new(i.start.row, i.end.col + 1),
            Cell::new(i.end.row, a.end.col),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wb() -> Workbook {
        let mut wb = Workbook::new();
        wb.set_now(1_789_635_600_000_000);
        for (i, v) in ["5", "12", "7", "12", "30", "", "apple pie", "=1/0"]
            .iter()
            .enumerate()
        {
            wb.set_input(0, Cell::new(i as u32, 0), v).unwrap();
        }
        wb
    }
    fn hits(wb: &Workbook) -> Vec<u32> {
        wb.conditional_effects(0, Range::parse("A1:A8").unwrap())
            .into_iter()
            .filter(|(_, e)| e.style != Dxf::default())
            .map(|(c, _)| c.row + 1)
            .collect()
    }
    #[test]
    fn highlight_rules_follow_excel() {
        let red = Dxf::preset("lightred").unwrap();
        let r = Range::parse("A1:A8").unwrap();
        let mut w = wb();
        w.add_conditional(
            0,
            CondFormat::new(
                r,
                Rule::CellIs {
                    op: CellOp::Greater,
                    formulas: vec!["10".into()],
                    style: red,
                },
            ),
        )
        .unwrap();
        // Text is greater than any number in Excel's comparisons; errors never match.
        assert_eq!(hits(&w), [2, 4, 5, 7]);
        w.clear_conditional(0, None).unwrap();
        for (rule, expected) in [
            (
                Rule::Duplicates {
                    unique: false,
                    style: red,
                },
                vec![2, 4],
            ),
            (
                Rule::Top {
                    bottom: false,
                    rank: 2,
                    percent: false,
                    style: red,
                },
                vec![2, 4, 5],
            ),
            (
                Rule::Average {
                    below: true,
                    equal: false,
                    style: red,
                },
                vec![1, 2, 3, 4],
            ),
            (
                Rule::Text {
                    op: TextOp::Contains,
                    text: "PIE".into(),
                    style: red,
                },
                vec![7],
            ),
            (
                Rule::Blanks {
                    blanks: true,
                    style: red,
                },
                vec![6],
            ),
            (
                Rule::Errors {
                    errors: true,
                    style: red,
                },
                vec![8],
            ),
            (
                Rule::Expression {
                    formula: "MOD(A1,2)=1".into(),
                    style: red,
                },
                vec![1, 3],
            ),
        ] {
            let mut w = wb();
            w.add_conditional(0, CondFormat::new(r, rule.clone()))
                .unwrap();
            assert_eq!(hits(&w), expected, "{rule:?}");
        }
    }
    #[test]
    fn bars_scales_and_icons_scale_with_the_values() {
        let mut w = wb();
        let r = Range::parse("A1:A5").unwrap();
        w.add_conditional(
            0,
            CondFormat::new(
                r,
                Rule::DataBar {
                    color: [99, 142, 198],
                    min: Cfvo::Min,
                    max: Cfvo::Max,
                },
            ),
        )
        .unwrap();
        w.add_conditional(
            0,
            CondFormat::new(
                r,
                Rule::ColorScale {
                    stops: vec![(Cfvo::Min, [248, 105, 107]), (Cfvo::Max, [99, 190, 123])],
                },
            ),
        )
        .unwrap();
        w.add_conditional(
            0,
            CondFormat::new(
                r,
                Rule::IconSet {
                    set: "3Arrows".into(),
                    points: vec![Cfvo::Percent(0.0), Cfvo::Percent(33.0), Cfvo::Percent(67.0)],
                    reverse: false,
                    show_value: true,
                },
            ),
        )
        .unwrap();
        let e = w.conditional_effects(0, r);
        let first = &e[&Cell::new(0, 0)];
        assert_eq!(first.bar, Some((0.0, [99, 142, 198])));
        assert_eq!(first.scale, Some([248, 105, 107]));
        assert_eq!(first.icon, Some(("3Arrows".into(), 0, 3)));
        let last = &e[&Cell::new(4, 0)];
        assert_eq!(last.bar.unwrap().0, 1.0);
        assert_eq!(last.icon.as_ref().unwrap().1, 2);
        // 12 is (12-5)/25 = 28% of the way: the lowest arrow, a third of the red-green way.
        assert_eq!(e[&Cell::new(1, 0)].icon.as_ref().unwrap().1, 0);
        assert_eq!(
            e[&Cell::new(1, 0)].scale,
            Some(mix([248, 105, 107], [99, 190, 123], 0.28))
        );
        // Rules can be reordered and undone.
        w.move_conditional(0, 2, true).unwrap();
        assert!(matches!(
            w.sheets[0].conditional[1].rule,
            Rule::DataBar { .. }
        ));
        assert!(w.undo());
        assert!(matches!(
            w.sheets[0].conditional[2].rule,
            Rule::DataBar { .. }
        ));
    }
    #[test]
    fn dates_occurring_read_the_world_clock() {
        let mut w = Workbook::new();
        w.set_now(1_789_635_600_000_000); // Thursday 2026-09-17
        for (i, d) in [
            "2026-09-16",
            "2026-09-17",
            "2026-09-13",
            "2026-08-31",
            "2026-10-01",
        ]
        .iter()
        .enumerate()
        {
            w.set_input(0, Cell::new(i as u32, 0), d).unwrap();
        }
        let r = Range::parse("A1:A5").unwrap();
        let rows = |w: &Workbook| -> Vec<u32> {
            w.conditional_effects(0, r)
                .keys()
                .map(|c| c.row + 1)
                .collect()
        };
        for (p, expected) in [
            (Period::Yesterday, vec![1]),
            (Period::ThisWeek, vec![1, 2, 3]),
            (Period::LastMonth, vec![4]),
            (Period::NextMonth, vec![5]),
        ] {
            let mut x = w.clone();
            x.add_conditional(
                0,
                CondFormat::new(
                    r,
                    Rule::Dates {
                        period: p,
                        style: Dxf::preset("green").unwrap(),
                    },
                ),
            )
            .unwrap();
            assert_eq!(rows(&x), expected, "{p:?}");
        }
    }
}
