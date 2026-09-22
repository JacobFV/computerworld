//! Worksheet functions. Argument rules follow Excel: numbers passed directly are
//! coerced (numeric text, booleans), numbers inside references and arrays are taken
//! as they are and text there is skipped, and the first error wins.
use crate::address::{Cell, Range};
use crate::date;
use crate::eval::{power, Eval, Grid, Operand};
use crate::format::{fixed, format, round_decimal};
use crate::parser::Expr;
use crate::value::{compare, general, parse_number_text, text_cmp, ErrorKind, Value};
use cw_determinism::math;
use std::cmp::Ordering;

type R<T> = Result<T, ErrorKind>;

/// Every function this engine implements.
pub const FUNCTIONS: &[&str] = &[
    "ABS",
    "ACOS",
    "ADDRESS",
    "AND",
    "ASIN",
    "ATAN",
    "ATAN2",
    "AVERAGE",
    "AVERAGEA",
    "AVERAGEIF",
    "AVERAGEIFS",
    "CEILING",
    "CEILING.MATH",
    "CHAR",
    "CHOOSE",
    "CLEAN",
    "CODE",
    "COLUMN",
    "COLUMNS",
    "CONCAT",
    "CONCATENATE",
    "CORREL",
    "COS",
    "COUNT",
    "COUNTA",
    "COUNTBLANK",
    "COUNTIF",
    "COUNTIFS",
    "DATE",
    "DATEDIF",
    "DATEVALUE",
    "DAY",
    "DAYS",
    "DEGREES",
    "DOLLAR",
    "EDATE",
    "EOMONTH",
    "ERROR.TYPE",
    "EVEN",
    "EXACT",
    "EXP",
    "FACT",
    "FALSE",
    "FIND",
    "FIXED",
    "FLOOR",
    "FLOOR.MATH",
    "FV",
    "GCD",
    "HLOOKUP",
    "HOUR",
    "IF",
    "IFERROR",
    "IFNA",
    "IFS",
    "INDEX",
    "INDIRECT",
    "INT",
    "IPMT",
    "IRR",
    "ISBLANK",
    "ISERR",
    "ISERROR",
    "ISEVEN",
    "ISLOGICAL",
    "ISNA",
    "ISNONTEXT",
    "ISNUMBER",
    "ISODD",
    "ISTEXT",
    "LARGE",
    "LCM",
    "LEFT",
    "LEN",
    "LN",
    "LOG",
    "LOG10",
    "LOOKUP",
    "LOWER",
    "MATCH",
    "MAX",
    "MAXA",
    "MAXIFS",
    "MEDIAN",
    "MID",
    "MIN",
    "MINA",
    "MINIFS",
    "MINUTE",
    "MOD",
    "MODE",
    "MODE.SNGL",
    "MONTH",
    "MROUND",
    "N",
    "NA",
    "NETWORKDAYS",
    "NOT",
    "NOW",
    "NPER",
    "NPV",
    "ODD",
    "OFFSET",
    "OR",
    "PERCENTILE",
    "PERCENTILE.INC",
    "PI",
    "PMT",
    "POWER",
    "PPMT",
    "PRODUCT",
    "PROPER",
    "PV",
    "QUARTILE",
    "QUARTILE.INC",
    "QUOTIENT",
    "RADIANS",
    "RAND",
    "RANDBETWEEN",
    "RANK",
    "RANK.EQ",
    "RATE",
    "REPLACE",
    "REPT",
    "RIGHT",
    "ROUND",
    "ROUNDDOWN",
    "ROUNDUP",
    "ROW",
    "ROWS",
    "SEARCH",
    "SECOND",
    "SIGN",
    "SIN",
    "SMALL",
    "SQRT",
    "STDEV",
    "STDEV.P",
    "STDEV.S",
    "STDEVP",
    "SUBSTITUTE",
    "SUBTOTAL",
    "SUM",
    "SUMIF",
    "SUMIFS",
    "SUMPRODUCT",
    "SUMSQ",
    "SWITCH",
    "T",
    "TAN",
    "TEXT",
    "TEXTJOIN",
    "TIME",
    "TIMEVALUE",
    "TODAY",
    "TRIM",
    "TRUE",
    "TRUNC",
    "TYPE",
    "UNICHAR",
    "UNICODE",
    "UPPER",
    "VALUE",
    "VAR",
    "VAR.P",
    "VAR.S",
    "VARP",
    "VLOOKUP",
    "WEEKDAY",
    "WEEKNUM",
    "WORKDAY",
    "XLOOKUP",
    "XMATCH",
    "XOR",
    "YEAR",
];
/// Functions whose result changes without any cell changing.
pub const VOLATILE: &[&str] = &["NOW", "TODAY", "RAND", "RANDBETWEEN", "OFFSET", "INDIRECT"];

fn v(x: Value) -> Operand {
    Operand::V(x)
}
fn e(k: ErrorKind) -> Operand {
    Operand::V(Value::Error(k))
}
fn n(x: f64) -> Operand {
    Operand::V(Value::number(x))
}
fn from<T: Into<Operand>>(r: R<T>) -> Operand {
    match r {
        Ok(x) => x.into(),
        Err(k) => e(k),
    }
}
impl From<Value> for Operand {
    fn from(x: Value) -> Self {
        Operand::V(x)
    }
}
impl From<f64> for Operand {
    fn from(x: f64) -> Self {
        Operand::V(Value::number(x))
    }
}
impl From<bool> for Operand {
    fn from(x: bool) -> Self {
        Operand::V(Value::Bool(x))
    }
}
impl From<String> for Operand {
    fn from(x: String) -> Self {
        Operand::V(Value::Text(x))
    }
}

struct Args<'e, 'a> {
    ev: &'e Eval<'a>,
    args: &'e [Expr],
}
impl Args<'_, '_> {
    fn len(&self) -> usize {
        self.args.len()
    }
    fn given(&self, i: usize) -> bool {
        self.args
            .get(i)
            .is_some_and(|a| !matches!(a, Expr::Missing))
    }
    fn op(&self, i: usize) -> Operand {
        match self.args.get(i) {
            Some(a) => self.ev.eval(a),
            None => v(Value::Empty),
        }
    }
    fn value(&self, i: usize) -> Value {
        let op = self.op(i);
        self.ev.to_scalar(op)
    }
    fn num(&self, i: usize) -> R<f64> {
        self.value(i).to_number()
    }
    fn num_or(&self, i: usize, d: f64) -> R<f64> {
        if self.given(i) {
            self.num(i)
        } else {
            Ok(d)
        }
    }
    fn int(&self, i: usize) -> R<i64> {
        Ok(self.num(i)?.trunc() as i64)
    }
    fn text(&self, i: usize) -> R<String> {
        self.value(i).to_text()
    }
    fn boolean(&self, i: usize) -> R<bool> {
        self.value(i).to_bool()
    }
    fn bool_or(&self, i: usize, d: bool) -> R<bool> {
        if self.given(i) {
            self.boolean(i)
        } else {
            Ok(d)
        }
    }
    fn grid(&self, i: usize) -> Grid {
        let op = self.op(i);
        self.ev.grid(&op)
    }
    /// Numbers from every argument, by Excel's argument rules.
    fn numbers(&self, from: usize) -> R<Vec<f64>> {
        let mut out = Vec::new();
        for i in from..self.len() {
            match self.op(i) {
                Operand::V(val) => match val {
                    Value::Empty => {
                        if self.given(i) {
                            out.push(0.0)
                        }
                    }
                    Value::Error(k) => return Err(k),
                    other => out.push(other.to_number()?),
                },
                op => {
                    for val in &self.ev.grid(&op).values {
                        match val {
                            Value::Number(x) => out.push(*x),
                            Value::Error(k) => return Err(*k),
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(out)
    }
    /// Like `numbers`, but text in references counts as 0 and booleans as 1/0 (the
    /// `…A` functions).
    fn numbers_a(&self) -> R<Vec<f64>> {
        let mut out = Vec::new();
        for i in 0..self.len() {
            match self.op(i) {
                Operand::V(val) => match val {
                    Value::Error(k) => return Err(k),
                    Value::Empty => {}
                    other => out.push(other.to_number()?),
                },
                op => {
                    for val in &self.ev.grid(&op).values {
                        match val {
                            Value::Number(x) => out.push(*x),
                            Value::Bool(b) => out.push(if *b { 1.0 } else { 0.0 }),
                            Value::Text(_) => out.push(0.0),
                            Value::Error(k) => return Err(*k),
                            Value::Empty => {}
                        }
                    }
                }
            }
        }
        Ok(out)
    }
    /// Every value of every argument, flattened.
    fn values(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for i in 0..self.len() {
            match self.op(i) {
                Operand::V(val) => out.push(val),
                op => out.extend(self.ev.grid(&op).values),
            }
        }
        out
    }
}

// ----- criteria (COUNTIF and friends) -----

#[derive(Clone, Copy, PartialEq)]
enum Rel {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}
struct Criterion {
    rel: Rel,
    value: Value,
}
fn criterion(v: &Value) -> Criterion {
    let Value::Text(t) = v else {
        return Criterion {
            rel: Rel::Eq,
            value: v.clone(),
        };
    };
    let (rel, rest) = if let Some(r) = t.strip_prefix("<=") {
        (Rel::Le, r)
    } else if let Some(r) = t.strip_prefix(">=") {
        (Rel::Ge, r)
    } else if let Some(r) = t.strip_prefix("<>") {
        (Rel::Ne, r)
    } else if let Some(r) = t.strip_prefix('<') {
        (Rel::Lt, r)
    } else if let Some(r) = t.strip_prefix('>') {
        (Rel::Gt, r)
    } else if let Some(r) = t.strip_prefix('=') {
        (Rel::Eq, r)
    } else {
        (Rel::Eq, t.as_str())
    };
    let value = if rest.is_empty() {
        Value::Empty
    } else if let Some(x) = parse_number_text(rest) {
        Value::Number(x)
    } else if let Some((s, _)) = date::parse_datetime(rest) {
        Value::Number(s)
    } else if rest.eq_ignore_ascii_case("TRUE") {
        Value::Bool(true)
    } else if rest.eq_ignore_ascii_case("FALSE") {
        Value::Bool(false)
    } else {
        Value::Text(rest.to_owned())
    };
    Criterion { rel, value }
}
/// `*`, `?` and `~` wildcards, case-insensitive.
pub fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi, ti));
                    pi += 1;
                    continue;
                }
                '~' if pi + 1 < p.len() && ti < t.len() && t[ti] == p[pi + 1] => {
                    pi += 2;
                    ti += 1;
                    continue;
                }
                '?' if ti < t.len() => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                c if c != '~' && c != '?' && ti < t.len() && t[ti] == c => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                _ => {}
            }
        } else if ti == t.len() {
            return true;
        }
        match star {
            Some((sp, st)) if st < t.len() => {
                star = Some((sp, st + 1));
                pi = sp + 1;
                ti = st + 1;
            }
            _ => return false,
        }
    }
}
fn matches(c: &Criterion, cell: &Value) -> bool {
    match (&c.value, c.rel) {
        (Value::Empty, Rel::Eq) => {
            matches!(cell, Value::Empty) || matches!(cell, Value::Text(t) if t.is_empty())
        }
        (Value::Empty, Rel::Ne) => !matches!(cell, Value::Empty),
        (Value::Number(x), rel) => {
            let Value::Number(y) = cell else {
                return rel == Rel::Ne;
            };
            let o = y.partial_cmp(x).unwrap_or(Ordering::Equal);
            rel_holds(rel, o)
        }
        (Value::Text(p), rel) => match cell {
            Value::Text(t) => match rel {
                Rel::Eq => wildcard(p, t),
                Rel::Ne => !wildcard(p, t),
                _ => rel_holds(rel, text_cmp(t, p)),
            },
            _ => rel == Rel::Ne,
        },
        (Value::Bool(b), rel) => match cell {
            Value::Bool(x) => rel_holds(rel, x.cmp(b)),
            _ => rel == Rel::Ne,
        },
        (Value::Error(k), rel) => match cell {
            Value::Error(x) => (x == k) == (rel == Rel::Eq),
            _ => rel == Rel::Ne,
        },
        (Value::Empty, _) => false,
    }
}
fn rel_holds(rel: Rel, o: Ordering) -> bool {
    match rel {
        Rel::Eq => o == Ordering::Equal,
        Rel::Ne => o != Ordering::Equal,
        Rel::Lt => o == Ordering::Less,
        Rel::Le => o != Ordering::Greater,
        Rel::Gt => o == Ordering::Greater,
        Rel::Ge => o != Ordering::Less,
    }
}
/// Indexes of cells in `ranges` meeting every `(range, criterion)` pair, which must
/// all be the same shape.
fn criteria_hits(a: &Args, pairs: &[(usize, usize)]) -> R<(Vec<usize>, Vec<Grid>)> {
    let mut grids = Vec::new();
    let mut crits = Vec::new();
    for (ri, ci) in pairs {
        grids.push(a.grid(*ri));
        let c = a.value(*ci);
        if let Value::Error(k) = c {
            return Err(k);
        }
        crits.push(criterion(&c));
    }
    let (rows, cols) = (grids[0].rows, grids[0].cols);
    if grids.iter().any(|g| g.rows != rows || g.cols != cols) {
        return Err(ErrorKind::Value);
    }
    let hits = (0..rows * cols)
        .filter(|i| {
            grids
                .iter()
                .zip(&crits)
                .all(|(g, c)| matches(c, &g.values[*i]))
        })
        .collect();
    Ok((hits, grids))
}
/// The cells of `range` (argument `ri`) laid over `shape`, for SUMIF's sum range.
fn aligned(a: &Args, ri: usize, rows: usize, cols: usize) -> Vec<Value> {
    match a.op(ri) {
        Operand::R(s, r) => {
            let mut out = Vec::with_capacity(rows * cols);
            for dr in 0..rows as u32 {
                for dc in 0..cols as u32 {
                    out.push(a.ev.cell_value(s, Cell::new(r.start.row + dr, r.start.col + dc)));
                }
            }
            out
        }
        op => {
            let g = a.ev.grid(&op);
            (0..rows * cols)
                .map(|i| g.values.get(i).cloned().unwrap_or(Value::Empty))
                .collect()
        }
    }
}

// ----- statistics helpers -----

fn mean(xs: &[f64]) -> R<f64> {
    if xs.is_empty() {
        return Err(ErrorKind::Div0);
    }
    Ok(sum(xs) / xs.len() as f64)
}
/// Neumaier-compensated sum, so long columns add up the same way every time.
fn sum(xs: &[f64]) -> f64 {
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for &x in xs {
        let t = s + x;
        if s.abs() >= x.abs() {
            c += (s - t) + x;
        } else {
            c += (x - t) + s;
        }
        s = t;
    }
    s + c
}
fn variance(xs: &[f64], sample: bool) -> R<f64> {
    let n = xs.len();
    if n < if sample { 2 } else { 1 } {
        return Err(ErrorKind::Div0);
    }
    let m = mean(xs)?;
    let ss: Vec<f64> = xs.iter().map(|x| (x - m) * (x - m)).collect();
    Ok(sum(&ss) / if sample { n - 1 } else { n } as f64)
}
fn sorted(mut xs: Vec<f64>) -> Vec<f64> {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    xs
}
fn percentile(xs: Vec<f64>, k: f64) -> R<f64> {
    if xs.is_empty() || !(0.0..=1.0).contains(&k) {
        return Err(ErrorKind::Num);
    }
    let s = sorted(xs);
    let pos = k * (s.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    Ok(s[lo] + (s[hi] - s[lo]) * (pos - lo as f64))
}

// ----- lookup helpers -----

fn lookup_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Text(p), Value::Text(t)) => wildcard(p, t),
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => false,
    }
}
fn same_type(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}
/// Approximate match over data sorted ascending: the last position whose value is
/// `<=` the target (Excel's binary search, which assumes the order).
fn approx_position(values: &[Value], target: &Value) -> Option<usize> {
    let (mut lo, mut hi) = (0usize, values.len());
    let mut best = None;
    while lo < hi {
        let mid = (lo + hi) / 2;
        let v = &values[mid];
        // Mismatched types (and blanks) are skipped towards the start, as Excel does.
        if !same_type(v, target) {
            let mut j = mid;
            let mut found = None;
            while j > lo {
                j -= 1;
                if same_type(&values[j], target) {
                    found = Some(j);
                    break;
                }
            }
            match found {
                Some(j) if compare(&values[j], target) != Ordering::Greater => {
                    best = Some(j);
                    lo = mid + 1;
                }
                _ => hi = mid,
            }
            continue;
        }
        if compare(v, target) != Ordering::Greater {
            best = Some(mid);
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    best
}
fn vector(g: &Grid) -> Vec<Value> {
    g.values.clone()
}
fn nth_row(g: &Grid, r: usize) -> Vec<Value> {
    (0..g.cols).map(|c| g.get(r, c).clone()).collect()
}
fn nth_col(g: &Grid, c: usize) -> Vec<Value> {
    (0..g.rows).map(|r| g.get(r, c).clone()).collect()
}

// ----- dates -----

fn serial_of(v: &Value) -> R<f64> {
    match v {
        Value::Text(t) => date::parse_datetime(t)
            .map(|x| x.0)
            .or_else(|| parse_number_text(t))
            .ok_or(ErrorKind::Value),
        other => other.to_number(),
    }
}
fn ymd_of(x: f64) -> R<(i64, u32, u32)> {
    if x < 0.0 {
        return Err(ErrorKind::Num);
    }
    date::ymd(x).ok_or(ErrorKind::Num)
}
fn is_workday(serial: f64, holidays: &[f64]) -> bool {
    let wd = date::weekday(serial);
    wd != 0 && wd != 6 && !holidays.contains(&serial.floor())
}

// ----- finance -----

fn fv_of(rate: f64, nper: f64, pmt: f64, pv: f64, due: bool) -> f64 {
    if rate == 0.0 {
        return -(pv + pmt * nper);
    }
    let g = math::pow(1.0 + rate, nper);
    let t = if due { 1.0 + rate } else { 1.0 };
    -(pv * g + pmt * t * (g - 1.0) / rate)
}
fn pmt_of(rate: f64, nper: f64, pv: f64, fv: f64, due: bool) -> R<f64> {
    if nper == 0.0 {
        return Err(ErrorKind::Num);
    }
    if rate == 0.0 {
        return Ok(-(pv + fv) / nper);
    }
    let g = math::pow(1.0 + rate, nper);
    let t = if due { 1.0 + rate } else { 1.0 };
    Ok(-(rate * (fv + pv * g)) / (t * (g - 1.0)))
}
fn ipmt_of(rate: f64, per: f64, nper: f64, pv: f64, fv: f64, due: bool) -> R<f64> {
    if per < 1.0 || per > nper {
        return Err(ErrorKind::Num);
    }
    let pmt = pmt_of(rate, nper, pv, fv, due)?;
    if due && per == 1.0 {
        return Ok(0.0);
    }
    let balance = fv_of(rate, per - 1.0, pmt, pv, due);
    let interest = balance * rate;
    Ok(if due {
        interest / (1.0 + rate)
    } else {
        interest
    })
}
/// Newton's method from `guess`, as Excel's RATE and IRR iterate.
fn newton(guess: f64, f: impl Fn(f64) -> f64) -> R<f64> {
    let mut x = guess;
    for _ in 0..100 {
        let y = f(x);
        let h = 1e-7 * x.abs().max(1e-3);
        let d = (f(x + h) - y) / h;
        if d == 0.0 || !d.is_finite() {
            return Err(ErrorKind::Num);
        }
        let next = x - y / d;
        if !next.is_finite() {
            return Err(ErrorKind::Num);
        }
        if (next - x).abs() < 1e-10 {
            return Ok(next);
        }
        x = next;
    }
    Err(ErrorKind::Num)
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}
fn trig(a: &Args, f: fn(f64) -> f64) -> Operand {
    from(a.num(0).and_then(|x| {
        let r = f(x);
        if r.is_nan() {
            Err(ErrorKind::Num)
        } else {
            Ok(r)
        }
    }))
}
fn round_toward(x: f64, digits: i64, up: bool) -> f64 {
    let scale = math::pow(10.0, digits as f64);
    let y = round_decimal(x * scale, 9);
    let r = if up { y.abs().ceil() } else { y.abs().floor() };
    let r = if x < 0.0 { -r } else { r } / scale;
    round_decimal(r, digits.clamp(-15, 15) as i32)
}
fn text_of(v: &Value) -> R<String> {
    v.to_text()
}

/// Evaluate a function call.
pub fn call(ev: &Eval, name: &str, args: &[Expr]) -> Operand {
    let a = Args { ev, args };
    let count = |min: usize, max: usize| (min..=max).contains(&args.len());
    let name = name.strip_prefix("_XLFN.").unwrap_or(name);
    macro_rules! arity {
        ($min:expr, $max:expr) => {
            if !count($min, $max) {
                return e(ErrorKind::Value);
            }
        };
    }
    match name {
        // ----- logic -----
        "TRUE" => v(Value::Bool(true)),
        "FALSE" => v(Value::Bool(false)),
        "IF" => {
            arity!(1, 3);
            match a.boolean(0) {
                Err(k) => e(k),
                Ok(true) => {
                    if a.given(1) {
                        a.op(1)
                    } else if args.len() >= 2 {
                        n(0.0)
                    } else {
                        v(Value::Bool(true))
                    }
                }
                Ok(false) => {
                    if a.given(2) {
                        a.op(2)
                    } else if args.len() == 3 {
                        n(0.0)
                    } else {
                        v(Value::Bool(false))
                    }
                }
            }
        }
        "IFS" => {
            if args.len() < 2 || !args.len().is_multiple_of(2) {
                return e(ErrorKind::Value);
            }
            for i in (0..args.len()).step_by(2) {
                match a.boolean(i) {
                    Err(k) => return e(k),
                    Ok(true) => return a.op(i + 1),
                    Ok(false) => {}
                }
            }
            e(ErrorKind::NA)
        }
        "SWITCH" => {
            arity!(3, 254);
            let target = a.value(0);
            if let Value::Error(k) = target {
                return e(k);
            }
            let mut i = 1;
            while i + 1 < args.len() {
                let candidate = a.value(i);
                if crate::value::formula_compare(&target, &candidate) == Ok(Ordering::Equal) {
                    return a.op(i + 1);
                }
                i += 2;
            }
            if i < args.len() {
                a.op(i)
            } else {
                e(ErrorKind::NA)
            }
        }
        "AND" | "OR" | "XOR" => {
            let mut seen = false;
            let mut acc = name == "AND";
            let mut ones = 0;
            for val in a.values() {
                let b = match val {
                    Value::Error(k) => return e(k),
                    Value::Bool(b) => b,
                    Value::Number(x) => x != 0.0,
                    _ => continue,
                };
                seen = true;
                match name {
                    "AND" => acc &= b,
                    "OR" => acc |= b,
                    _ => ones += usize::from(b),
                }
            }
            if !seen {
                return e(ErrorKind::Value);
            }
            v(Value::Bool(if name == "XOR" { ones % 2 == 1 } else { acc }))
        }
        "NOT" => {
            arity!(1, 1);
            from(a.boolean(0).map(|b| !b))
        }
        "IFERROR" => {
            arity!(2, 2);
            let op = a.op(0);
            match &op {
                Operand::V(Value::Error(_)) => a.op(1),
                Operand::R(..) => match ev.to_scalar(op.clone()) {
                    Value::Error(_) => a.op(1),
                    _ => op,
                },
                _ => op,
            }
        }
        "IFNA" => {
            arity!(2, 2);
            match a.value(0) {
                Value::Error(ErrorKind::NA) => a.op(1),
                _ => a.op(0),
            }
        }
        "CHOOSE" => {
            arity!(2, 255);
            match a.int(0) {
                Err(k) => e(k),
                Ok(i) if i >= 1 && (i as usize) < args.len() => a.op(i as usize),
                Ok(_) => e(ErrorKind::Value),
            }
        }
        "NA" => e(ErrorKind::NA),
        // ----- information -----
        "ISBLANK" => v(Value::Bool(a.value(0).is_empty())),
        "ISNUMBER" => v(Value::Bool(matches!(a.value(0), Value::Number(_)))),
        "ISTEXT" => v(Value::Bool(matches!(a.value(0), Value::Text(_)))),
        "ISNONTEXT" => v(Value::Bool(!matches!(a.value(0), Value::Text(_)))),
        "ISLOGICAL" => v(Value::Bool(matches!(a.value(0), Value::Bool(_)))),
        "ISERROR" => v(Value::Bool(matches!(a.value(0), Value::Error(_)))),
        "ISERR" => v(Value::Bool(
            matches!(a.value(0), Value::Error(k) if k != ErrorKind::NA),
        )),
        "ISNA" => v(Value::Bool(matches!(
            a.value(0),
            Value::Error(ErrorKind::NA)
        ))),
        "ISEVEN" | "ISODD" => from(a.num(0).map(|x| {
            let even = (x.trunc() as i64) % 2 == 0;
            Value::Bool(even == (name == "ISEVEN"))
        })),
        "ERROR.TYPE" => match a.value(0) {
            Value::Error(k) => n(k.number()),
            _ => e(ErrorKind::NA),
        },
        "TYPE" => n(match a.op(0) {
            Operand::A(_) => 64.0,
            op => match ev.to_scalar(op) {
                Value::Number(_) | Value::Empty => 1.0,
                Value::Text(_) => 2.0,
                Value::Bool(_) => 4.0,
                Value::Error(_) => 16.0,
            },
        }),
        "N" => n(match a.value(0) {
            Value::Number(x) => x,
            Value::Bool(b) => f64::from(u8::from(b)),
            Value::Error(k) => return e(k),
            _ => 0.0,
        }),
        "T" => match a.value(0) {
            Value::Text(t) => v(Value::Text(t)),
            Value::Error(k) => e(k),
            _ => v(Value::Text(String::new())),
        },
        // ----- sums and counts -----
        "SUM" => from(a.numbers(0).map(|xs| sum(&xs))),
        "SUMSQ" => from(
            a.numbers(0)
                .map(|xs| sum(&xs.iter().map(|x| x * x).collect::<Vec<_>>())),
        ),
        "PRODUCT" => from(a.numbers(0).map(|xs| {
            if xs.is_empty() {
                0.0
            } else {
                xs.iter().product()
            }
        })),
        "AVERAGE" => from(a.numbers(0).and_then(|xs| mean(&xs))),
        "AVERAGEA" => from(a.numbers_a().and_then(|xs| mean(&xs))),
        "COUNT" => {
            let mut k = 0;
            for i in 0..args.len() {
                match a.op(i) {
                    Operand::V(val) => {
                        if val.to_number().is_ok() && !matches!(val, Value::Empty | Value::Text(_))
                            || matches!(&val, Value::Text(t) if parse_number_text(t).is_some())
                        {
                            k += 1
                        }
                    }
                    op => {
                        k += ev
                            .grid(&op)
                            .values
                            .iter()
                            .filter(|x| matches!(x, Value::Number(_)))
                            .count()
                    }
                }
            }
            n(k as f64)
        }
        "COUNTA" => n(a.values().iter().filter(|x| !x.is_empty()).count() as f64),
        "COUNTBLANK" => {
            arity!(1, 1);
            match a.op(0) {
                Operand::R(s, r) => {
                    let total = u64::from(r.rows()) * u64::from(r.cols());
                    let filled = ev.used(s, r).map_or(0, |u| {
                        u.cells().filter(|c| !matches!(ev.cell_value(s, *c), Value::Empty) && !matches!(ev.cell_value(s, *c), Value::Text(ref t) if t.is_empty())).count() as u64
                    });
                    n((total - filled) as f64)
                }
                _ => e(ErrorKind::Value),
            }
        }
        "MAX" | "MIN" | "MAXA" | "MINA" => {
            let xs = if name.ends_with('A') {
                a.numbers_a()
            } else {
                a.numbers(0)
            };
            from(xs.map(|xs| {
                if xs.is_empty() {
                    0.0
                } else if name.starts_with("MAX") {
                    xs.iter().copied().fold(f64::MIN, f64::max)
                } else {
                    xs.iter().copied().fold(f64::MAX, f64::min)
                }
            }))
        }
        "COUNTIF" => {
            arity!(2, 2);
            from(criteria_hits(&a, &[(0, 1)]).map(|(h, _)| h.len() as f64))
        }
        "COUNTIFS" => {
            if args.len() < 2 || !args.len().is_multiple_of(2) {
                return e(ErrorKind::Value);
            }
            let pairs: Vec<(usize, usize)> =
                (0..args.len()).step_by(2).map(|i| (i, i + 1)).collect();
            from(criteria_hits(&a, &pairs).map(|(h, _)| h.len() as f64))
        }
        "SUMIF" | "AVERAGEIF" => {
            arity!(2, 3);
            let (hits, grids) = match criteria_hits(&a, &[(0, 1)]) {
                Ok(x) => x,
                Err(k) => return e(k),
            };
            let values = if a.given(2) {
                aligned(&a, 2, grids[0].rows, grids[0].cols)
            } else {
                grids[0].values.clone()
            };
            let mut xs = Vec::new();
            for i in hits {
                match &values[i] {
                    Value::Number(x) => xs.push(*x),
                    Value::Error(k) => return e(*k),
                    _ => {}
                }
            }
            if name == "SUMIF" {
                n(sum(&xs))
            } else {
                from(mean(&xs))
            }
        }
        "SUMIFS" | "AVERAGEIFS" | "MAXIFS" | "MINIFS" => {
            if args.len() < 3 || args.len().is_multiple_of(2) {
                return e(ErrorKind::Value);
            }
            let pairs: Vec<(usize, usize)> =
                (1..args.len()).step_by(2).map(|i| (i, i + 1)).collect();
            let (hits, grids) = match criteria_hits(&a, &pairs) {
                Ok(x) => x,
                Err(k) => return e(k),
            };
            let target = a.grid(0);
            if target.rows != grids[0].rows || target.cols != grids[0].cols {
                return e(ErrorKind::Value);
            }
            let mut xs = Vec::new();
            for i in hits {
                match &target.values[i] {
                    Value::Number(x) => xs.push(*x),
                    Value::Error(k) => return e(*k),
                    _ => {}
                }
            }
            match name {
                "SUMIFS" => n(sum(&xs)),
                "AVERAGEIFS" => from(mean(&xs)),
                "MAXIFS" => n(xs
                    .iter()
                    .copied()
                    .fold(f64::MIN, f64::max)
                    .max(if xs.is_empty() { 0.0 } else { f64::MIN })),
                _ => n(if xs.is_empty() {
                    0.0
                } else {
                    xs.iter().copied().fold(f64::MAX, f64::min)
                }),
            }
        }
        "SUMPRODUCT" => {
            arity!(1, 255);
            let grids: Vec<Grid> = (0..args.len()).map(|i| a.grid(i)).collect();
            let (rows, cols) = (grids[0].rows, grids[0].cols);
            if grids.iter().any(|g| g.rows != rows || g.cols != cols) {
                return e(ErrorKind::Value);
            }
            let mut terms = Vec::with_capacity(rows * cols);
            for i in 0..rows * cols {
                let mut p = 1.0;
                for g in &grids {
                    p *= match &g.values[i] {
                        Value::Number(x) => *x,
                        Value::Bool(b) if args.len() == 1 => f64::from(u8::from(*b)),
                        Value::Error(k) => return e(*k),
                        _ => 0.0,
                    };
                }
                terms.push(p);
            }
            n(sum(&terms))
        }
        "SUBTOTAL" => {
            arity!(2, 255);
            let code = match a.int(0) {
                Ok(c) => c % 100,
                Err(k) => return e(k),
            };
            let sub = Args {
                ev,
                args: &args[1..],
            };
            match code {
                1 => from(sub.numbers(0).and_then(|xs| mean(&xs))),
                2 => n(sub
                    .values()
                    .iter()
                    .filter(|x| matches!(x, Value::Number(_)))
                    .count() as f64),
                3 => n(sub.values().iter().filter(|x| !x.is_empty()).count() as f64),
                4 => from(sub.numbers(0).map(|xs| {
                    if xs.is_empty() {
                        0.0
                    } else {
                        xs.iter().copied().fold(f64::MIN, f64::max)
                    }
                })),
                5 => from(sub.numbers(0).map(|xs| {
                    if xs.is_empty() {
                        0.0
                    } else {
                        xs.iter().copied().fold(f64::MAX, f64::min)
                    }
                })),
                6 => from(sub.numbers(0).map(|xs| xs.iter().product::<f64>())),
                7 => from(
                    sub.numbers(0)
                        .and_then(|xs| variance(&xs, true))
                        .map(f64::sqrt),
                ),
                8 => from(
                    sub.numbers(0)
                        .and_then(|xs| variance(&xs, false))
                        .map(f64::sqrt),
                ),
                9 => from(sub.numbers(0).map(|xs| sum(&xs))),
                10 => from(sub.numbers(0).and_then(|xs| variance(&xs, true))),
                11 => from(sub.numbers(0).and_then(|xs| variance(&xs, false))),
                _ => e(ErrorKind::Value),
            }
        }
        // ----- statistics -----
        "MEDIAN" => from(a.numbers(0).and_then(|xs| percentile(xs, 0.5))),
        "MODE" | "MODE.SNGL" => from(a.numbers(0).and_then(|xs| {
            let mut best: Option<(f64, usize)> = None;
            for (i, x) in xs.iter().enumerate() {
                let c = xs.iter().filter(|y| *y == x).count();
                if c > 1 && best.is_none_or(|(_, bc)| c > bc) && !xs[..i].contains(x) {
                    best = Some((*x, c));
                }
            }
            best.map(|b| b.0).ok_or(ErrorKind::NA)
        })),
        "STDEV" | "STDEV.S" => from(
            a.numbers(0)
                .and_then(|xs| variance(&xs, true))
                .map(f64::sqrt),
        ),
        "STDEVP" | "STDEV.P" => from(
            a.numbers(0)
                .and_then(|xs| variance(&xs, false))
                .map(f64::sqrt),
        ),
        "VAR" | "VAR.S" => from(a.numbers(0).and_then(|xs| variance(&xs, true))),
        "VARP" | "VAR.P" => from(a.numbers(0).and_then(|xs| variance(&xs, false))),
        "LARGE" | "SMALL" => {
            arity!(2, 2);
            let xs = match (Args {
                ev,
                args: &args[..1],
            })
            .numbers(0)
            {
                Ok(x) => sorted(x),
                Err(k) => return e(k),
            };
            match a.num(1) {
                Err(k) => e(k),
                Ok(k) => {
                    let k = k.ceil() as usize;
                    if k == 0 || k > xs.len() {
                        return e(ErrorKind::Num);
                    }
                    n(if name == "SMALL" {
                        xs[k - 1]
                    } else {
                        xs[xs.len() - k]
                    })
                }
            }
        }
        "PERCENTILE" | "PERCENTILE.INC" | "QUARTILE" | "QUARTILE.INC" => {
            arity!(2, 2);
            let xs = match (Args {
                ev,
                args: &args[..1],
            })
            .numbers(0)
            {
                Ok(x) => x,
                Err(k) => return e(k),
            };
            let k = match a.num(1) {
                Ok(k) => k,
                Err(err) => return e(err),
            };
            let k = if name.starts_with("QUARTILE") {
                let q = k.trunc();
                if !(0.0..=4.0).contains(&q) {
                    return e(ErrorKind::Num);
                }
                q / 4.0
            } else {
                k
            };
            from(percentile(xs, k))
        }
        "RANK" | "RANK.EQ" => {
            arity!(2, 3);
            let x = match a.num(0) {
                Ok(x) => x,
                Err(k) => return e(k),
            };
            let xs: Vec<f64> = a
                .grid(1)
                .values
                .iter()
                .filter_map(|v| {
                    if let Value::Number(x) = v {
                        Some(*x)
                    } else {
                        None
                    }
                })
                .collect();
            if !xs.contains(&x) {
                return e(ErrorKind::NA);
            }
            let ascending = a.num_or(2, 0.0).unwrap_or(0.0) != 0.0;
            n(1.0
                + xs.iter()
                    .filter(|y| if ascending { **y < x } else { **y > x })
                    .count() as f64)
        }
        "CORREL" => {
            arity!(2, 2);
            let (g1, g2) = (a.grid(0), a.grid(1));
            if g1.values.len() != g2.values.len() {
                return e(ErrorKind::NA);
            }
            let pairs: Vec<(f64, f64)> = g1
                .values
                .iter()
                .zip(&g2.values)
                .filter_map(|p| match p {
                    (Value::Number(x), Value::Number(y)) => Some((*x, *y)),
                    _ => None,
                })
                .collect();
            let xs: Vec<f64> = pairs.iter().map(|p| p.0).collect();
            let ys: Vec<f64> = pairs.iter().map(|p| p.1).collect();
            let (mx, my) = match (mean(&xs), mean(&ys)) {
                (Ok(x), Ok(y)) => (x, y),
                _ => return e(ErrorKind::Div0),
            };
            let cov = sum(&pairs
                .iter()
                .map(|(x, y)| (x - mx) * (y - my))
                .collect::<Vec<_>>());
            let sx = sum(&xs.iter().map(|x| (x - mx) * (x - mx)).collect::<Vec<_>>());
            let sy = sum(&ys.iter().map(|y| (y - my) * (y - my)).collect::<Vec<_>>());
            if sx == 0.0 || sy == 0.0 {
                return e(ErrorKind::Div0);
            }
            n(cov / (sx * sy).sqrt())
        }
        // ----- arithmetic -----
        "ABS" => from(a.num(0).map(f64::abs)),
        "SIGN" => from(a.num(0).map(|x| {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        })),
        "INT" => from(a.num(0).map(f64::floor)),
        "TRUNC" => {
            arity!(1, 2);
            match (a.num(0), a.num_or(1, 0.0)) {
                (Ok(x), Ok(d)) => n(round_toward(x, d.trunc() as i64, false)),
                (Err(k), _) | (_, Err(k)) => e(k),
            }
        }
        "ROUND" | "ROUNDUP" | "ROUNDDOWN" => {
            arity!(2, 2);
            match (a.num(0), a.num(1)) {
                (Ok(x), Ok(d)) => {
                    let d = d.trunc() as i64;
                    n(match name {
                        "ROUND" => round_decimal(x, d.clamp(-308, 308) as i32),
                        "ROUNDUP" => round_toward(x, d, true),
                        _ => round_toward(x, d, false),
                    })
                }
                (Err(k), _) | (_, Err(k)) => e(k),
            }
        }
        "MROUND" => match (a.num(0), a.num(1)) {
            (Ok(x), Ok(m)) => {
                if m == 0.0 {
                    return n(0.0);
                }
                if x * m < 0.0 {
                    return e(ErrorKind::Num);
                }
                n(round_decimal((x / m).abs(), 0) * m.abs() * x.signum())
            }
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "CEILING" | "CEILING.MATH" | "FLOOR" | "FLOOR.MATH" => {
            let x = match a.num(0) {
                Ok(x) => x,
                Err(k) => return e(k),
            };
            let default = if x < 0.0 && name.ends_with("MATH") {
                -1.0
            } else {
                1.0
            };
            let s = match a.num_or(
                1,
                if x < 0.0 && !name.ends_with("MATH") {
                    -1.0
                } else {
                    default
                },
            ) {
                Ok(s) => s.abs(),
                Err(k) => return e(k),
            };
            if s == 0.0 {
                return n(0.0);
            }
            let q = round_decimal(x / s, 9);
            let up = name.starts_with("CEILING");
            n(if up { q.ceil() } else { q.floor() } * s)
        }
        "EVEN" | "ODD" => from(a.num(0).map(|x| {
            let m = x.abs().ceil();
            let m = if name == "EVEN" {
                if m % 2.0 == 0.0 {
                    m
                } else {
                    m + 1.0
                }
            } else if m % 2.0 == 1.0 {
                m
            } else {
                m + 1.0
            };
            if x < 0.0 {
                -m
            } else {
                m
            }
        })),
        "MOD" => {
            arity!(2, 2);
            match (a.num(0), a.num(1)) {
                (Ok(_), Ok(0.0)) => e(ErrorKind::Div0),
                (Ok(x), Ok(d)) => n(x - d * (x / d).floor()),
                (Err(k), _) | (_, Err(k)) => e(k),
            }
        }
        "QUOTIENT" => match (a.num(0), a.num(1)) {
            (Ok(_), Ok(0.0)) => e(ErrorKind::Div0),
            (Ok(x), Ok(d)) => n((x / d).trunc()),
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "POWER" => match (a.num(0), a.num(1)) {
            (Ok(x), Ok(y)) => v(power(x, y)),
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "SQRT" => from(a.num(0).and_then(|x| {
            if x < 0.0 {
                Err(ErrorKind::Num)
            } else {
                Ok(x.sqrt())
            }
        })),
        "EXP" => from(a.num(0).map(math::exp)),
        "LN" => from(a.num(0).and_then(|x| {
            if x <= 0.0 {
                Err(ErrorKind::Num)
            } else {
                Ok(math::ln(x))
            }
        })),
        "LOG10" => from(a.num(0).and_then(|x| {
            if x <= 0.0 {
                Err(ErrorKind::Num)
            } else {
                Ok(math::log10(x))
            }
        })),
        "LOG" => match (a.num(0), a.num_or(1, 10.0)) {
            (Ok(x), Ok(b)) => {
                if x <= 0.0 || b <= 0.0 || b == 1.0 {
                    e(ErrorKind::Num)
                } else if b == 10.0 {
                    n(math::log10(x))
                } else if b == 2.0 {
                    n(math::log2(x))
                } else {
                    n(math::ln(x) / math::ln(b))
                }
            }
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "PI" => n(std::f64::consts::PI),
        "FACT" => from(a.num(0).and_then(|x| {
            if x < 0.0 {
                return Err(ErrorKind::Num);
            }
            let k = x.trunc() as u32;
            if k > 170 {
                return Err(ErrorKind::Num);
            }
            Ok((1..=k).map(f64::from).product::<f64>())
        })),
        "GCD" | "LCM" => from(a.numbers(0).and_then(|xs| {
            if xs.iter().any(|x| *x < 0.0) {
                return Err(ErrorKind::Num);
            }
            let ints: Vec<u64> = xs.iter().map(|x| x.trunc() as u64).collect();
            Ok(if name == "GCD" {
                ints.iter().fold(0, |g, x| gcd(g, *x)) as f64
            } else {
                ints.iter().fold(1u64, |l, x| {
                    if *x == 0 || l == 0 {
                        0
                    } else {
                        l / gcd(l, *x) * x
                    }
                }) as f64
            })
        })),
        "RAND" => n(ev.wb.random(ev.sheet, ev.at, 0)),
        "RANDBETWEEN" => match (a.num(0), a.num(1)) {
            (Ok(lo), Ok(hi)) => {
                let (lo, hi) = (lo.ceil(), hi.floor());
                if lo > hi {
                    return e(ErrorKind::Num);
                }
                n(lo + (ev.wb.random(ev.sheet, ev.at, 0) * (hi - lo + 1.0)).floor())
            }
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "SIN" => trig(&a, math::sin),
        "COS" => trig(&a, math::cos),
        "TAN" => trig(&a, math::tan),
        "ASIN" => trig(&a, math::asin),
        "ACOS" => trig(&a, math::acos),
        "ATAN" => trig(&a, math::atan),
        "ATAN2" => match (a.num(0), a.num(1)) {
            (Ok(x), Ok(y)) if x == 0.0 && y == 0.0 => e(ErrorKind::Div0),
            (Ok(x), Ok(y)) => n(math::atan2(y, x)),
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "RADIANS" => from(a.num(0).map(|x| x * std::f64::consts::PI / 180.0)),
        "DEGREES" => from(a.num(0).map(|x| x * 180.0 / std::f64::consts::PI)),
        // ----- text -----
        "CONCAT" | "CONCATENATE" => {
            let mut s = String::new();
            for val in a.values() {
                match text_of(&val) {
                    Ok(t) => s.push_str(&t),
                    Err(k) => return e(k),
                }
            }
            v(Value::Text(s))
        }
        "TEXTJOIN" => {
            arity!(3, 255);
            let (sep, skip) = match (a.text(0), a.boolean(1)) {
                (Ok(s), Ok(b)) => (s, b),
                (Err(k), _) | (_, Err(k)) => return e(k),
            };
            let rest = Args {
                ev,
                args: &args[2..],
            };
            let mut parts = Vec::new();
            for val in rest.values() {
                let t = match text_of(&val) {
                    Ok(t) => t,
                    Err(k) => return e(k),
                };
                if skip && t.is_empty() {
                    continue;
                }
                parts.push(t);
            }
            v(Value::Text(parts.join(&sep)))
        }
        "LEFT" | "RIGHT" => {
            arity!(1, 2);
            match (a.text(0), a.num_or(1, 1.0)) {
                (Ok(t), Ok(k)) => {
                    if k < 0.0 {
                        return e(ErrorKind::Value);
                    }
                    let chars: Vec<char> = t.chars().collect();
                    let k = (k.trunc() as usize).min(chars.len());
                    v(Value::Text(if name == "LEFT" {
                        chars[..k].iter().collect()
                    } else {
                        chars[chars.len() - k..].iter().collect()
                    }))
                }
                (Err(k), _) | (_, Err(k)) => e(k),
            }
        }
        "MID" => {
            arity!(3, 3);
            match (a.text(0), a.num(1), a.num(2)) {
                (Ok(t), Ok(s), Ok(k)) => {
                    if s < 1.0 || k < 0.0 {
                        return e(ErrorKind::Value);
                    }
                    let chars: Vec<char> = t.chars().collect();
                    let start = (s.trunc() as usize - 1).min(chars.len());
                    let end = (start + k.trunc() as usize).min(chars.len());
                    v(Value::Text(chars[start..end].iter().collect()))
                }
                (Err(k), _, _) | (_, Err(k), _) | (_, _, Err(k)) => e(k),
            }
        }
        "LEN" => from(a.text(0).map(|t| t.chars().count() as f64)),
        "UPPER" => from(a.text(0).map(|t| t.to_uppercase())),
        "LOWER" => from(a.text(0).map(|t| t.to_lowercase())),
        "PROPER" => from(a.text(0).map(|t| {
            let mut out = String::new();
            let mut prev_letter = false;
            for c in t.chars() {
                if c.is_alphabetic() {
                    if prev_letter {
                        out.extend(c.to_lowercase());
                    } else {
                        out.extend(c.to_uppercase());
                    }
                    prev_letter = true;
                } else {
                    out.push(c);
                    prev_letter = false;
                }
            }
            out
        })),
        "TRIM" => from(a.text(0).map(|t| {
            t.split(' ')
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })),
        "CLEAN" => from(
            a.text(0)
                .map(|t| t.chars().filter(|c| (*c as u32) >= 32).collect::<String>()),
        ),
        "SUBSTITUTE" => {
            arity!(3, 4);
            match (a.text(0), a.text(1), a.text(2)) {
                (Ok(t), Ok(old), Ok(new)) => {
                    if old.is_empty() {
                        return v(Value::Text(t));
                    }
                    if a.given(3) {
                        let k = match a.num(3) {
                            Ok(k) if k >= 1.0 => k.trunc() as usize,
                            Ok(_) => return e(ErrorKind::Value),
                            Err(err) => return e(err),
                        };
                        match t.match_indices(&old).nth(k - 1) {
                            Some((i, _)) => v(Value::Text(format!(
                                "{}{new}{}",
                                &t[..i],
                                &t[i + old.len()..]
                            ))),
                            None => v(Value::Text(t)),
                        }
                    } else {
                        v(Value::Text(t.replace(&old, &new)))
                    }
                }
                (Err(k), _, _) | (_, Err(k), _) | (_, _, Err(k)) => e(k),
            }
        }
        "REPLACE" => {
            arity!(4, 4);
            match (a.text(0), a.num(1), a.num(2), a.text(3)) {
                (Ok(t), Ok(s), Ok(k), Ok(new)) => {
                    if s < 1.0 || k < 0.0 {
                        return e(ErrorKind::Value);
                    }
                    let chars: Vec<char> = t.chars().collect();
                    let start = (s as usize - 1).min(chars.len());
                    let end = (start + k as usize).min(chars.len());
                    let mut out: String = chars[..start].iter().collect();
                    out.push_str(&new);
                    out.extend(&chars[end..]);
                    v(Value::Text(out))
                }
                _ => e(ErrorKind::Value),
            }
        }
        "FIND" | "SEARCH" => {
            arity!(2, 3);
            match (a.text(0), a.text(1), a.num_or(2, 1.0)) {
                (Ok(needle), Ok(hay), Ok(start)) => {
                    let chars: Vec<char> = hay.chars().collect();
                    if start < 1.0 || start as usize > chars.len() + 1 {
                        return e(ErrorKind::Value);
                    }
                    let from_idx = start as usize - 1;
                    let n_chars: Vec<char> = needle.chars().collect();
                    for i in from_idx..=chars.len() {
                        let hit = if name == "FIND" {
                            chars[i..].starts_with(&n_chars)
                        } else {
                            (i..=chars.len()).any(|j| {
                                let seg: String = chars[i..j].iter().collect();
                                wildcard(&needle, &seg)
                            })
                        };
                        if hit {
                            return n((i + 1) as f64);
                        }
                    }
                    e(ErrorKind::Value)
                }
                (Err(k), _, _) | (_, Err(k), _) | (_, _, Err(k)) => e(k),
            }
        }
        "EXACT" => match (a.text(0), a.text(1)) {
            (Ok(x), Ok(y)) => v(Value::Bool(x == y)),
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "REPT" => match (a.text(0), a.num(1)) {
            (Ok(t), Ok(k)) if k >= 0.0 && t.len() * (k as usize) <= 32_767 => {
                v(Value::Text(t.repeat(k as usize)))
            }
            (Ok(_), Ok(_)) => e(ErrorKind::Value),
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "CHAR" | "UNICHAR" => from(a.num(0).and_then(|x| {
            let c = x.trunc() as u32;
            let limit = if name == "CHAR" { 255 } else { 0x10_FFFF };
            if c < 1 || c > limit {
                return Err(ErrorKind::Value);
            }
            char::from_u32(c).map(String::from).ok_or(ErrorKind::Value)
        })),
        "CODE" | "UNICODE" => from(a.text(0).and_then(|t| {
            t.chars()
                .next()
                .map(|c| c as u32 as f64)
                .ok_or(ErrorKind::Value)
        })),
        "VALUE" => match a.value(0) {
            Value::Number(x) => n(x),
            Value::Empty => n(0.0),
            Value::Text(t) => {
                match parse_number_text(&t).or_else(|| date::parse_datetime(&t).map(|x| x.0)) {
                    Some(x) => n(x),
                    None => e(ErrorKind::Value),
                }
            }
            Value::Bool(_) => e(ErrorKind::Value),
            Value::Error(k) => e(k),
        },
        "TEXT" => {
            arity!(2, 2);
            match (a.value(0), a.text(1)) {
                (Value::Error(k), _) | (_, Err(k)) => e(k),
                (val, Ok(code)) => {
                    let val = match val {
                        Value::Text(t) => {
                            parse_number_text(&t).map_or(Value::Text(t), Value::Number)
                        }
                        Value::Empty => Value::Number(0.0),
                        other => other,
                    };
                    v(Value::Text(format(&val, &code).text))
                }
            }
        }
        "FIXED" | "DOLLAR" => {
            arity!(1, 3);
            match (a.num(0), a.num_or(1, 2.0)) {
                (Ok(x), Ok(d)) => {
                    let d = d.trunc().clamp(-127.0, 127.0) as i32;
                    let rounded = round_decimal(x, d);
                    let decimals = d.max(0) as usize;
                    let mut code = String::from(if name == "DOLLAR" { "$#,##0" } else { "#,##0" });
                    if name == "FIXED" && a.bool_or(2, false).unwrap_or(false) {
                        code = "0".into();
                    }
                    if decimals > 0 {
                        code.push('.');
                        code.push_str(&"0".repeat(decimals));
                    }
                    if name == "DOLLAR" {
                        code = format!("{code};({code})");
                    }
                    v(Value::Text(format(&Value::Number(rounded), &code).text))
                }
                (Err(k), _) | (_, Err(k)) => e(k),
            }
        }
        // ----- dates -----
        "TODAY" => n(ev.wb.now().floor()),
        "NOW" => n(ev.wb.now()),
        "DATE" => {
            arity!(3, 3);
            match (a.num(0), a.num(1), a.num(2)) {
                (Ok(y), Ok(m), Ok(d)) => {
                    let mut y = y.trunc() as i64;
                    if (0..1900).contains(&y) {
                        y += 1900;
                    }
                    match date::serial(y, m.trunc() as i64, d.trunc() as i64) {
                        Some(s) => n(s),
                        None => e(ErrorKind::Num),
                    }
                }
                (Err(k), _, _) | (_, Err(k), _) | (_, _, Err(k)) => e(k),
            }
        }
        "TIME" => match (a.num(0), a.num(1), a.num(2)) {
            (Ok(h), Ok(m), Ok(s)) => {
                let secs = h.trunc() * 3600.0 + m.trunc() * 60.0 + s.trunc();
                if secs < 0.0 {
                    return e(ErrorKind::Num);
                }
                n((secs % 86_400.0) / 86_400.0)
            }
            (Err(k), _, _) | (_, Err(k), _) | (_, _, Err(k)) => e(k),
        },
        "DATEVALUE" => from(a.text(0).and_then(|t| {
            date::parse_datetime(&t)
                .map(|x| x.0.floor())
                .ok_or(ErrorKind::Value)
        })),
        "TIMEVALUE" => from(a.text(0).and_then(|t| {
            date::parse_datetime(&t)
                .map(|x| x.0 - x.0.floor())
                .ok_or(ErrorKind::Value)
        })),
        "YEAR" | "MONTH" | "DAY" => from(serial_of(&a.value(0)).and_then(ymd_of).map(
            |(y, m, d)| match name {
                "YEAR" => y as f64,
                "MONTH" => f64::from(m),
                _ => f64::from(d),
            },
        )),
        "HOUR" | "MINUTE" | "SECOND" => from(serial_of(&a.value(0)).map(|s| {
            let (h, m, sec) = date::hms(s);
            f64::from(match name {
                "HOUR" => h,
                "MINUTE" => m,
                _ => sec,
            })
        })),
        "WEEKDAY" => {
            arity!(1, 2);
            match (serial_of(&a.value(0)), a.num_or(1, 1.0)) {
                (Ok(s), Ok(t)) => {
                    let wd = date::weekday(s) as i64; // 0 = Sunday
                    let r = match t as i64 {
                        1 | 17 => wd + 1,
                        2 | 11 => (wd + 6) % 7 + 1,
                        3 => (wd + 6) % 7,
                        12 => (wd + 5) % 7 + 1,
                        13 => (wd + 4) % 7 + 1,
                        14 => (wd + 3) % 7 + 1,
                        15 => (wd + 2) % 7 + 1,
                        16 => (wd + 1) % 7 + 1,
                        _ => return e(ErrorKind::Num),
                    };
                    n(r as f64)
                }
                (Err(k), _) | (_, Err(k)) => e(k),
            }
        }
        "WEEKNUM" => match (serial_of(&a.value(0)), a.num_or(1, 1.0)) {
            (Ok(s), Ok(t)) => {
                let (y, _, _) = match ymd_of(s) {
                    Ok(x) => x,
                    Err(k) => return e(k),
                };
                let jan1 = date::serial(y, 1, 1).unwrap_or(1.0);
                let start = if t as i64 == 2 { 1 } else { 0 };
                let offset = (date::weekday(jan1) as i64 - start).rem_euclid(7);
                n(((s.floor() - jan1) as i64 + offset) as f64 / 7.0 + 1.0).clone_floor()
            }
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "EDATE" | "EOMONTH" => match (serial_of(&a.value(0)), a.num(1)) {
            (Ok(s), Ok(m)) => {
                let (y, mo, d) = match ymd_of(s) {
                    Ok(x) => x,
                    Err(k) => return e(k),
                };
                let total = y * 12 + i64::from(mo) - 1 + m.trunc() as i64;
                let (ny, nm) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
                let last = date::days_in_month(ny, nm);
                let day = if name == "EOMONTH" { last } else { d.min(last) };
                match date::serial(ny, i64::from(nm), i64::from(day)) {
                    Some(s) => n(s),
                    None => e(ErrorKind::Num),
                }
            }
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "DAYS" => match (serial_of(&a.value(0)), serial_of(&a.value(1))) {
            (Ok(end), Ok(start)) => n(end.floor() - start.floor()),
            (Err(k), _) | (_, Err(k)) => e(k),
        },
        "DATEDIF" => {
            arity!(3, 3);
            match (serial_of(&a.value(0)), serial_of(&a.value(1)), a.text(2)) {
                (Ok(s), Ok(t), Ok(unit)) => {
                    if s > t {
                        return e(ErrorKind::Num);
                    }
                    let (Ok((y1, m1, d1)), Ok((y2, m2, d2))) = (ymd_of(s), ymd_of(t)) else {
                        return e(ErrorKind::Num);
                    };
                    let months =
                        (y2 - y1) * 12 + i64::from(m2) - i64::from(m1) - i64::from(d2 < d1);
                    n(match unit.to_ascii_uppercase().as_str() {
                        "D" => t.floor() - s.floor(),
                        "M" => months as f64,
                        "Y" => (months / 12) as f64,
                        "YM" => (months % 12) as f64,
                        "MD" => {
                            if d2 >= d1 {
                                f64::from(d2 - d1)
                            } else {
                                let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                                f64::from(date::days_in_month(py, pm) - d1 + d2)
                            }
                        }
                        "YD" => {
                            let anniversary = date::serial(
                                y2 - i64::from((m2, d2) < (m1, d1)),
                                i64::from(m1),
                                i64::from(d1),
                            )
                            .unwrap_or(s);
                            t.floor() - anniversary
                        }
                        _ => return e(ErrorKind::Num),
                    })
                }
                (Err(k), _, _) | (_, Err(k), _) | (_, _, Err(k)) => e(k),
            }
        }
        "NETWORKDAYS" | "WORKDAY" => {
            arity!(2, 3);
            let holidays: Vec<f64> = if a.given(2) {
                a.grid(2)
                    .values
                    .iter()
                    .filter_map(|v| serial_of(v).ok())
                    .map(f64::floor)
                    .collect()
            } else {
                vec![]
            };
            let start = match serial_of(&a.value(0)) {
                Ok(s) => s.floor(),
                Err(k) => return e(k),
            };
            if name == "NETWORKDAYS" {
                let end = match serial_of(&a.value(1)) {
                    Ok(s) => s.floor(),
                    Err(k) => return e(k),
                };
                let (lo, hi, sign) = if start <= end {
                    (start, end, 1.0)
                } else {
                    (end, start, -1.0)
                };
                let mut k = 0.0;
                let mut d = lo;
                while d <= hi {
                    if is_workday(d, &holidays) {
                        k += 1.0;
                    }
                    d += 1.0;
                }
                n(k * sign)
            } else {
                let days = match a.num(1) {
                    Ok(x) => x.trunc() as i64,
                    Err(k) => return e(k),
                };
                let step = if days < 0 { -1.0 } else { 1.0 };
                let mut left = days.abs();
                let mut d = start;
                while left > 0 {
                    d += step;
                    if is_workday(d, &holidays) {
                        left -= 1;
                    }
                }
                n(d)
            }
        }
        // ----- finance -----
        "PMT" => {
            arity!(3, 5);
            match (
                a.num(0),
                a.num(1),
                a.num(2),
                a.num_or(3, 0.0),
                a.num_or(4, 0.0),
            ) {
                (Ok(r), Ok(np), Ok(pv), Ok(fv), Ok(t)) => from(pmt_of(r, np, pv, fv, t != 0.0)),
                _ => e(ErrorKind::Value),
            }
        }
        "IPMT" | "PPMT" => {
            arity!(4, 6);
            match (
                a.num(0),
                a.num(1),
                a.num(2),
                a.num(3),
                a.num_or(4, 0.0),
                a.num_or(5, 0.0),
            ) {
                (Ok(r), Ok(per), Ok(np), Ok(pv), Ok(fv), Ok(t)) => {
                    let due = t != 0.0;
                    match (ipmt_of(r, per, np, pv, fv, due), pmt_of(r, np, pv, fv, due)) {
                        (Ok(i), Ok(p)) => n(if name == "IPMT" { i } else { p - i }),
                        (Err(k), _) | (_, Err(k)) => e(k),
                    }
                }
                _ => e(ErrorKind::Value),
            }
        }
        "FV" => {
            arity!(3, 5);
            match (
                a.num(0),
                a.num(1),
                a.num(2),
                a.num_or(3, 0.0),
                a.num_or(4, 0.0),
            ) {
                (Ok(r), Ok(np), Ok(pmt), Ok(pv), Ok(t)) => n(fv_of(r, np, pmt, pv, t != 0.0)),
                _ => e(ErrorKind::Value),
            }
        }
        "PV" => {
            arity!(3, 5);
            match (
                a.num(0),
                a.num(1),
                a.num(2),
                a.num_or(3, 0.0),
                a.num_or(4, 0.0),
            ) {
                (Ok(r), Ok(np), Ok(pmt), Ok(fv), Ok(t)) => {
                    if r == 0.0 {
                        return n(-(fv + pmt * np));
                    }
                    let g = math::pow(1.0 + r, np);
                    let due = if t != 0.0 { 1.0 + r } else { 1.0 };
                    n(-(fv + pmt * due * (g - 1.0) / r) / g)
                }
                _ => e(ErrorKind::Value),
            }
        }
        "NPER" => {
            arity!(3, 5);
            match (
                a.num(0),
                a.num(1),
                a.num(2),
                a.num_or(3, 0.0),
                a.num_or(4, 0.0),
            ) {
                (Ok(r), Ok(pmt), Ok(pv), Ok(fv), Ok(t)) => {
                    if r == 0.0 {
                        if pmt == 0.0 {
                            return e(ErrorKind::Num);
                        }
                        return n(-(pv + fv) / pmt);
                    }
                    let due = if t != 0.0 { 1.0 + r } else { 1.0 };
                    let num = pmt * due - fv * r;
                    let den = pv * r + pmt * due;
                    if num / den <= 0.0 {
                        return e(ErrorKind::Num);
                    }
                    n(math::ln(num / den) / math::ln(1.0 + r))
                }
                _ => e(ErrorKind::Value),
            }
        }
        "NPV" => {
            arity!(2, 255);
            let r = match a.num(0) {
                Ok(r) => r,
                Err(k) => return e(k),
            };
            match a.numbers(1) {
                Ok(xs) => n(sum(&xs
                    .iter()
                    .enumerate()
                    .map(|(i, x)| x / math::pow(1.0 + r, (i + 1) as f64))
                    .collect::<Vec<_>>())),
                Err(k) => e(k),
            }
        }
        "IRR" => {
            arity!(1, 2);
            let xs: Vec<f64> = a
                .grid(0)
                .values
                .iter()
                .filter_map(|v| {
                    if let Value::Number(x) = v {
                        Some(*x)
                    } else {
                        None
                    }
                })
                .collect();
            if !xs.iter().any(|x| *x > 0.0) || !xs.iter().any(|x| *x < 0.0) {
                return e(ErrorKind::Num);
            }
            let guess = a.num_or(1, 0.1).unwrap_or(0.1);
            from(newton(guess, |r| {
                sum(&xs
                    .iter()
                    .enumerate()
                    .map(|(i, x)| x / math::pow(1.0 + r, i as f64))
                    .collect::<Vec<_>>())
            }))
        }
        "RATE" => {
            arity!(3, 6);
            match (
                a.num(0),
                a.num(1),
                a.num(2),
                a.num_or(3, 0.0),
                a.num_or(4, 0.0),
                a.num_or(5, 0.1),
            ) {
                (Ok(np), Ok(pmt), Ok(pv), Ok(fv), Ok(t), Ok(g)) => {
                    let due = t != 0.0;
                    from(newton(g, |r| {
                        if r == 0.0 {
                            pv + pmt * np + fv
                        } else {
                            let gr = math::pow(1.0 + r, np);
                            pv * gr + pmt * (1.0 + if due { r } else { 0.0 }) * (gr - 1.0) / r + fv
                        }
                    }))
                }
                _ => e(ErrorKind::Value),
            }
        }
        // ----- lookup and reference -----
        "VLOOKUP" | "HLOOKUP" => {
            arity!(3, 4);
            let target = a.value(0);
            if let Value::Error(k) = target {
                return e(k);
            }
            let table = a.grid(1);
            let index = match a.num(2) {
                Ok(i) if i >= 1.0 => i as usize - 1,
                Ok(_) => return e(ErrorKind::Value),
                Err(k) => return e(k),
            };
            let approximate = match a.bool_or(3, true) {
                Ok(b) => b,
                Err(k) => return e(k),
            };
            let vertical = name == "VLOOKUP";
            let (keys, width) = if vertical {
                (nth_col(&table, 0), table.cols)
            } else {
                (nth_row(&table, 0), table.rows)
            };
            // The table may extend past the used area; the column still exists.
            let limit = if vertical {
                a_range_cols(&a, 1)
            } else {
                a_range_rows(&a, 1)
            }
            .unwrap_or(width);
            if index >= limit {
                return e(ErrorKind::Ref);
            }
            let pos = if approximate {
                approx_position(&keys, &target)
            } else {
                keys.iter().position(|k| lookup_eq(&target, k))
            };
            match pos {
                None => e(ErrorKind::NA),
                Some(p) => {
                    if index >= width {
                        return n(0.0).into_empty_zero();
                    }
                    let val = if vertical {
                        table.get(p, index).clone()
                    } else {
                        table.get(index, p).clone()
                    };
                    v(if val.is_empty() {
                        Value::Number(0.0)
                    } else {
                        val
                    })
                }
            }
        }
        "MATCH" | "XMATCH" => {
            arity!(2, 4);
            let target = a.value(0);
            if let Value::Error(k) = target {
                return e(k);
            }
            let values = vector(&a.grid(1));
            let mode = if name == "MATCH" {
                match a.num_or(2, 1.0) {
                    // `f64::signum` calls zero positive; MATCH type 0 is an exact match.
                    Ok(m) => i64::from(m > 0.0) - i64::from(m < 0.0),
                    Err(k) => return e(k),
                }
            } else {
                match a.num_or(2, 0.0) {
                    Ok(m) => m as i64,
                    Err(k) => return e(k),
                }
            };
            let pos = match (name, mode) {
                (_, 0) | ("XMATCH", 2) => values.iter().position(|k| lookup_eq(&target, k)),
                ("MATCH", 1) => approx_position(&values, &target),
                ("MATCH", _) => {
                    // Descending data: the last position whose value is >= the target.
                    let mut best = None;
                    for (i, k) in values.iter().enumerate() {
                        if same_type(k, &target) && compare(k, &target) != Ordering::Less {
                            best = Some(i);
                        } else if same_type(k, &target) {
                            break;
                        }
                    }
                    best
                }
                (_, m) => nearest(&values, &target, m),
            };
            match pos {
                Some(p) => n((p + 1) as f64),
                None => e(ErrorKind::NA),
            }
        }
        "XLOOKUP" => {
            arity!(3, 6);
            let target = a.value(0);
            if let Value::Error(k) = target {
                return e(k);
            }
            let keys_grid = a.grid(1);
            let keys = vector(&keys_grid);
            let mode = a.num_or(4, 0.0).unwrap_or(0.0) as i64;
            let reverse = a.num_or(5, 1.0).unwrap_or(1.0) < 0.0;
            let pos = match mode {
                0 | 2 => {
                    if reverse {
                        keys.iter().rposition(|k| lookup_eq(&target, k))
                    } else {
                        keys.iter().position(|k| lookup_eq(&target, k))
                    }
                }
                m => nearest(&keys, &target, m),
            };
            let Some(p) = pos else {
                return if a.given(3) {
                    a.op(3)
                } else {
                    e(ErrorKind::NA)
                };
            };
            match a.op(2) {
                Operand::R(s, r) => {
                    if keys_grid.cols == 1 || keys_grid.rows > 1 {
                        // Vertical lookup: return that row of the result range.
                        let row = r.start.row + p as u32;
                        Operand::R(
                            s,
                            Range::new(Cell::new(row, r.start.col), Cell::new(row, r.end.col)),
                        )
                    } else {
                        let col = r.start.col + p as u32;
                        Operand::R(
                            s,
                            Range::new(Cell::new(r.start.row, col), Cell::new(r.end.row, col)),
                        )
                    }
                }
                op => {
                    let g = ev.grid(&op);
                    v(g.values
                        .get(p)
                        .cloned()
                        .unwrap_or(Value::Error(ErrorKind::NA)))
                }
            }
        }
        "LOOKUP" => {
            arity!(2, 3);
            let target = a.value(0);
            let keys_grid = a.grid(1);
            let (keys, results) = if a.given(2) {
                (vector(&keys_grid), vector(&a.grid(2)))
            } else if keys_grid.rows >= keys_grid.cols {
                (
                    nth_col(&keys_grid, 0),
                    nth_col(&keys_grid, keys_grid.cols - 1),
                )
            } else {
                (
                    nth_row(&keys_grid, 0),
                    nth_row(&keys_grid, keys_grid.rows - 1),
                )
            };
            match approx_position(&keys, &target) {
                Some(p) => v(results
                    .get(p)
                    .cloned()
                    .unwrap_or(Value::Error(ErrorKind::NA))),
                None => e(ErrorKind::NA),
            }
        }
        "INDEX" => {
            arity!(2, 3);
            let row = match a.num(1) {
                Ok(r) => r.trunc() as i64,
                Err(k) => return e(k),
            };
            let col = match a.num_or(2, 0.0) {
                Ok(c) => c.trunc() as i64,
                Err(k) => return e(k),
            };
            if row < 0 || col < 0 {
                return e(ErrorKind::Value);
            }
            match a.op(0) {
                Operand::R(s, r) => {
                    let (row, col) = if r.rows() == 1 && !a.given(2) {
                        (1, row)
                    } else {
                        (row, col)
                    };
                    let (rows, cols) = (i64::from(r.rows()), i64::from(r.cols()));
                    if row > rows || col > cols {
                        return e(ErrorKind::Ref);
                    }
                    let (r0, r1) = if row == 0 {
                        (r.start.row, r.end.row)
                    } else {
                        let x = r.start.row + row as u32 - 1;
                        (x, x)
                    };
                    let (c0, c1) = if col == 0 {
                        (r.start.col, r.end.col)
                    } else {
                        let x = r.start.col + col as u32 - 1;
                        (x, x)
                    };
                    Operand::R(s, Range::new(Cell::new(r0, c0), Cell::new(r1, c1)))
                }
                op => {
                    let g = ev.grid(&op);
                    let (row, col) = if g.rows == 1 && !a.given(2) {
                        (1, row.max(1))
                    } else {
                        (row.max(1), col.max(1))
                    };
                    if row as usize > g.rows || col as usize > g.cols {
                        return e(ErrorKind::Ref);
                    }
                    v(g.get(row as usize - 1, col as usize - 1).clone())
                }
            }
        }
        "OFFSET" => {
            arity!(3, 5);
            let Operand::R(s, r) = a.op(0) else {
                return e(ErrorKind::Value);
            };
            match (a.num(1), a.num(2)) {
                (Ok(dr), Ok(dc)) => {
                    let h = a.num_or(3, f64::from(r.rows())).unwrap_or(1.0).trunc() as i64;
                    let w = a.num_or(4, f64::from(r.cols())).unwrap_or(1.0).trunc() as i64;
                    let top = i64::from(r.start.row) + dr.trunc() as i64;
                    let left = i64::from(r.start.col) + dc.trunc() as i64;
                    if top < 0
                        || left < 0
                        || h < 1
                        || w < 1
                        || top + h > i64::from(crate::address::MAX_ROWS)
                        || left + w > i64::from(crate::address::MAX_COLS)
                    {
                        return e(ErrorKind::Ref);
                    }
                    Operand::R(
                        s,
                        Range::new(
                            Cell::new(top as u32, left as u32),
                            Cell::new((top + h - 1) as u32, (left + w - 1) as u32),
                        ),
                    )
                }
                (Err(k), _) | (_, Err(k)) => e(k),
            }
        }
        "INDIRECT" => {
            arity!(1, 2);
            let text = match a.text(0) {
                Ok(t) => t,
                Err(k) => return e(k),
            };
            let (sheet, body) = match text.rsplit_once('!') {
                Some((s, b)) => (Some(s.trim_matches('\'').replace("''", "'")), b.to_owned()),
                None => (None, text.clone()),
            };
            let s = match ev.sheet_index(&sheet) {
                Ok(s) => s,
                Err(k) => return e(k),
            };
            if let Some(r) = Range::parse(&body) {
                return Operand::R(s, r);
            }
            match ev.wb.name(&body) {
                Some((s, r)) => Operand::R(s, r),
                None => e(ErrorKind::Ref),
            }
        }
        "ROW" | "COLUMN" => {
            if args.is_empty() {
                return n(f64::from(if name == "ROW" { ev.at.row } else { ev.at.col }) + 1.0);
            }
            match a.op(0) {
                Operand::R(_, r) => n(f64::from(if name == "ROW" {
                    r.start.row
                } else {
                    r.start.col
                }) + 1.0),
                _ => e(ErrorKind::Value),
            }
        }
        "ROWS" | "COLUMNS" => match a.op(0) {
            Operand::R(_, r) => n(f64::from(if name == "ROWS" { r.rows() } else { r.cols() })),
            Operand::A(g) => n((if name == "ROWS" { g.rows } else { g.cols }) as f64),
            Operand::V(Value::Error(k)) => e(k),
            Operand::V(_) => n(1.0),
        },
        "ADDRESS" => {
            arity!(2, 5);
            match (a.num(0), a.num(1), a.num_or(2, 1.0)) {
                (Ok(r), Ok(c), Ok(abs)) => {
                    if r < 1.0 || c < 1.0 {
                        return e(ErrorKind::Value);
                    }
                    let abs = abs as i64;
                    let cell = crate::address::CellRef {
                        row: r as u32 - 1,
                        col: c as u32 - 1,
                        row_abs: abs == 1 || abs == 2,
                        col_abs: abs == 1 || abs == 3,
                    };
                    let mut s = cell.a1();
                    if a.given(4) {
                        if let Ok(sheet) = a.text(4) {
                            s = format!("{}!{s}", crate::parser::quote_sheet(&sheet));
                        }
                    }
                    v(Value::Text(s))
                }
                (Err(k), _, _) | (_, Err(k), _) | (_, _, Err(k)) => e(k),
            }
        }
        _ => e(ErrorKind::Name),
    }
}
/// XLOOKUP/XMATCH match modes -1 (exact or next smaller) and 1 (exact or next larger).
fn nearest(values: &[Value], target: &Value, mode: i64) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, k) in values.iter().enumerate() {
        if !same_type(k, target) {
            continue;
        }
        let o = compare(k, target);
        if o == Ordering::Equal {
            return Some(i);
        }
        let better = |b: usize| {
            let ob = compare(k, &values[b]);
            if mode < 0 {
                ob == Ordering::Greater
            } else {
                ob == Ordering::Less
            }
        };
        if (mode < 0 && o == Ordering::Less || mode > 0 && o == Ordering::Greater)
            && best.is_none_or(better)
        {
            best = Some(i);
        }
    }
    best
}
fn a_range_cols(a: &Args, i: usize) -> Option<usize> {
    match a.op(i) {
        Operand::R(_, r) => Some(r.cols() as usize),
        _ => None,
    }
}
fn a_range_rows(a: &Args, i: usize) -> Option<usize> {
    match a.op(i) {
        Operand::R(_, r) => Some(r.rows() as usize),
        _ => None,
    }
}
trait OperandExt {
    fn into_empty_zero(self) -> Operand;
    fn clone_floor(self) -> Operand;
}
impl OperandExt for Operand {
    fn into_empty_zero(self) -> Operand {
        self
    }
    fn clone_floor(self) -> Operand {
        match self {
            Operand::V(Value::Number(x)) => Operand::V(Value::Number(x.floor())),
            other => other,
        }
    }
}
/// A number in General format, for callers that print values as Excel would.
pub fn general_text(x: f64) -> String {
    general(x)
}
#[allow(dead_code)]
fn fixed_text(x: f64, d: usize) -> String {
    fixed(x, d)
}
