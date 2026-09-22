//! Formula evaluation: operators with Excel's coercions, references, ranges and arrays.
use crate::address::{Cell, Range, MAX_COLS, MAX_ROWS};
use crate::parser::{Expr, Op, RangeKind};
use crate::value::{formula_compare, ErrorKind, Value};
use crate::workbook::Workbook;
use std::cmp::Ordering;

/// What an expression produces before it lands in a cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    V(Value),
    /// A reference to a rectangle on a sheet.
    R(usize, Range),
    /// An array, row-major.
    A(Grid),
}
/// A rectangle of values.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<Value>,
}
impl Grid {
    pub fn scalar(v: Value) -> Self {
        Self {
            rows: 1,
            cols: 1,
            values: vec![v],
        }
    }
    pub fn get(&self, r: usize, c: usize) -> &Value {
        &self.values[r * self.cols + c]
    }
    /// Element at (r, c), broadcasting a single row or column.
    fn broadcast(&self, r: usize, c: usize) -> Value {
        let rr = if self.rows == 1 { 0 } else { r };
        let cc = if self.cols == 1 { 0 } else { c };
        if rr >= self.rows || cc >= self.cols {
            return Value::Error(ErrorKind::NA);
        }
        self.get(rr, cc).clone()
    }
}

pub struct Eval<'a> {
    pub wb: &'a Workbook,
    pub sheet: usize,
    pub at: Cell,
}
fn err(e: ErrorKind) -> Operand {
    Operand::V(Value::Error(e))
}
impl<'a> Eval<'a> {
    pub fn new(wb: &'a Workbook, sheet: usize, at: Cell) -> Self {
        Self { wb, sheet, at }
    }
    /// The value a formula leaves in its cell: a reference collapses to the cell in the
    /// formula's row or column (implicit intersection), an array to its first element.
    pub fn cell_result(&self, e: &Expr) -> Value {
        let v = match self.eval(e) {
            Operand::V(v) => v,
            op => self.to_scalar(op),
        };
        match v {
            // A formula that points at an empty cell shows 0.
            Value::Empty => Value::Number(0.0),
            Value::Number(n) => Value::number(n),
            other => other,
        }
    }
    pub fn sheet_index(&self, name: &Option<String>) -> Result<usize, ErrorKind> {
        match name {
            None => Ok(self.sheet),
            Some(n) => self.wb.sheet_index(n).ok_or(ErrorKind::Ref),
        }
    }
    pub fn eval(&self, e: &Expr) -> Operand {
        match e {
            Expr::Number(n) => Operand::V(Value::number(*n)),
            Expr::Text(s) => Operand::V(Value::Text(s.clone())),
            Expr::Bool(b) => Operand::V(Value::Bool(*b)),
            Expr::Error(k) => err(*k),
            Expr::Missing => Operand::V(Value::Empty),
            Expr::Group(a) => self.eval(a),
            Expr::Ref { sheet, cell } => match self.sheet_index(sheet) {
                Ok(s) => Operand::R(s, Range::single(cell.cell())),
                Err(k) => err(k),
            },
            Expr::Range {
                sheet,
                start,
                end,
                kind,
            } => match self.sheet_index(sheet) {
                Ok(s) => {
                    let (a, b) = match kind {
                        RangeKind::Cells => (start.cell(), end.cell()),
                        RangeKind::Columns => {
                            (Cell::new(0, start.col), Cell::new(MAX_ROWS - 1, end.col))
                        }
                        RangeKind::Rows => {
                            (Cell::new(start.row, 0), Cell::new(end.row, MAX_COLS - 1))
                        }
                    };
                    Operand::R(s, Range::new(a, b))
                }
                Err(k) => err(k),
            },
            Expr::Name(n) => match self.wb.name(n) {
                Some((s, r)) => Operand::R(s, r),
                None => err(ErrorKind::Name),
            },
            Expr::Array(rows) => {
                let cols = rows.first().map_or(0, Vec::len);
                let mut values = Vec::new();
                for r in rows {
                    for x in r {
                        values.push(match self.eval(x) {
                            Operand::V(v) => v,
                            _ => Value::Error(ErrorKind::Value),
                        });
                    }
                }
                Operand::A(Grid {
                    rows: rows.len(),
                    cols,
                    values,
                })
            }
            Expr::Neg(a) => self.unary(a, |x| Ok(Value::number(-x))),
            Expr::Plus(a) => self.eval(a),
            Expr::Percent(a) => self.unary(a, |x| Ok(Value::number(x / 100.0))),
            Expr::Bin(op, a, b) => self.binary(*op, a, b),
            Expr::Call(name, args) => crate::functions::call(self, name, args),
        }
    }
    fn unary(&self, a: &Expr, f: impl Fn(f64) -> Result<Value, ErrorKind>) -> Operand {
        let op = self.eval(a);
        self.map(op, &|v| match v.to_number() {
            Ok(x) => f(x).unwrap_or_else(Value::Error),
            Err(e) => Value::Error(e),
        })
    }
    /// Apply a scalar function to every element of an operand.
    pub fn map(&self, op: Operand, f: &dyn Fn(&Value) -> Value) -> Operand {
        match op {
            Operand::V(v) => Operand::V(f(&v)),
            other => {
                let g = self.grid(&other);
                if g.rows == 1 && g.cols == 1 {
                    return Operand::V(f(&g.values[0]));
                }
                Operand::A(Grid {
                    rows: g.rows,
                    cols: g.cols,
                    values: g.values.iter().map(f).collect(),
                })
            }
        }
    }
    fn binary(&self, op: Op, a: &Expr, b: &Expr) -> Operand {
        let x = self.eval(a);
        let y = self.eval(b);
        let scalar = |o: &Operand| match o {
            Operand::V(_) => true,
            Operand::R(_, r) => r.is_single(),
            Operand::A(g) => g.rows == 1 && g.cols == 1,
        };
        if scalar(&x) && scalar(&y) {
            let (u, v) = (self.to_scalar(x), self.to_scalar(y));
            return Operand::V(apply(op, &u, &v));
        }
        let (gx, gy) = (self.grid(&x), self.grid(&y));
        let rows = gx.rows.max(gy.rows);
        let cols = gx.cols.max(gy.cols);
        let mut values = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            for c in 0..cols {
                values.push(apply(op, &gx.broadcast(r, c), &gy.broadcast(r, c)));
            }
        }
        Operand::A(Grid { rows, cols, values })
    }
    pub fn cell_value(&self, sheet: usize, c: Cell) -> Value {
        self.wb.value(sheet, c)
    }
    /// A reference narrowed to where the sheet has anything, so whole-column
    /// references do not walk a million empty rows.
    pub fn used(&self, sheet: usize, r: Range) -> Option<Range> {
        let used = self.wb.used_range(sheet)?;
        r.intersect(&used)
    }
    /// Operand as a single value: implicit intersection for references.
    pub fn to_scalar(&self, op: Operand) -> Value {
        match op {
            Operand::V(v) => v,
            Operand::A(g) => g.values.into_iter().next().unwrap_or(Value::Empty),
            Operand::R(s, r) => {
                if r.is_single() {
                    return self.cell_value(s, r.start);
                }
                if r.cols() == 1 && (r.start.row..=r.end.row).contains(&self.at.row) {
                    return self.cell_value(s, Cell::new(self.at.row, r.start.col));
                }
                if r.rows() == 1 && (r.start.col..=r.end.col).contains(&self.at.col) {
                    return self.cell_value(s, Cell::new(r.start.row, self.at.col));
                }
                Value::Error(ErrorKind::Value)
            }
        }
    }
    /// Operand as a rectangle of values. References are clipped to the used area of
    /// their sheet (keeping the top-left corner), which changes nothing about their
    /// contents: everything beyond is empty.
    pub fn grid(&self, op: &Operand) -> Grid {
        match op {
            Operand::V(v) => Grid::scalar(v.clone()),
            Operand::A(g) => g.clone(),
            Operand::R(s, r) => {
                let end = match self.wb.used_range(*s) {
                    Some(u) => Cell::new(
                        r.end.row.min(u.end.row.max(r.start.row)),
                        r.end.col.min(u.end.col.max(r.start.col)),
                    ),
                    None => r.start,
                };
                let clipped = Range::new(r.start, end);
                let mut values = Vec::with_capacity((clipped.rows() * clipped.cols()) as usize);
                for c in clipped.cells() {
                    values.push(self.cell_value(*s, c));
                }
                Grid {
                    rows: clipped.rows() as usize,
                    cols: clipped.cols() as usize,
                    values,
                }
            }
        }
    }
}
/// A binary operator over two values.
pub fn apply(op: Op, a: &Value, b: &Value) -> Value {
    match op {
        Op::Concat => match (a.to_text(), b.to_text()) {
            (Ok(x), Ok(y)) => Value::Text(x + &y),
            (Err(e), _) | (_, Err(e)) => Value::Error(e),
        },
        Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => match formula_compare(a, b) {
            Err(e) => Value::Error(e),
            Ok(o) => Value::Bool(match op {
                Op::Eq => o == Ordering::Equal,
                Op::Ne => o != Ordering::Equal,
                Op::Lt => o == Ordering::Less,
                Op::Le => o != Ordering::Greater,
                Op::Gt => o == Ordering::Greater,
                _ => o != Ordering::Less,
            }),
        },
        _ => {
            let (x, y) = match (a.to_number(), b.to_number()) {
                (Ok(x), Ok(y)) => (x, y),
                (Err(e), _) | (_, Err(e)) => return Value::Error(e),
            };
            match op {
                Op::Add => Value::number(x + y),
                Op::Sub => Value::number(x - y),
                Op::Mul => Value::number(x * y),
                Op::Div => {
                    if y == 0.0 {
                        Value::Error(ErrorKind::Div0)
                    } else {
                        Value::number(x / y)
                    }
                }
                _ => power(x, y),
            }
        }
    }
}
pub fn power(x: f64, y: f64) -> Value {
    if x == 0.0 && y == 0.0 {
        return Value::Error(ErrorKind::Num);
    }
    if x == 0.0 && y < 0.0 {
        return Value::Error(ErrorKind::Div0);
    }
    let r = cw_determinism::math::pow(x, y);
    if r.is_nan() {
        Value::Error(ErrorKind::Num)
    } else {
        Value::number(r)
    }
}
