//! Expression evaluation with SQLite's NULL, affinity and collation semantics.
use crate::ast::*;
use crate::schema::State;
use crate::value::{compare, same, Affinity, Collation, Value};
use crate::SqlError;
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

/// One column of an intermediate result.
#[derive(Clone, Debug, PartialEq)]
pub struct ColMeta {
    /// The name (or alias) of the table it came from, for `t.col` references.
    pub table: Option<String>,
    pub name: String,
    pub affinity: Option<Affinity>,
    pub collation: Collation,
    /// The rowid pseudo-column: reachable by name, never part of `*`.
    pub hidden: bool,
    /// The right-hand copy of a `USING`/`NATURAL` join column: only reachable qualified.
    pub merged: bool,
}
impl ColMeta {
    pub fn plain(name: impl Into<String>) -> Self {
        Self {
            table: None,
            name: name.into(),
            affinity: None,
            collation: Collation::Binary,
            hidden: false,
            merged: false,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Relation {
    pub cols: Vec<ColMeta>,
    pub rows: Vec<Vec<Value>>,
}

/// Connection state an expression may read or advance.
#[derive(Debug, Default)]
pub struct Env {
    pub now_us: i64,
    pub rng: Cell<u64>,
    pub last_insert_rowid: Cell<i64>,
    pub changes: Cell<i64>,
    pub total_changes: Cell<i64>,
    /// Triggers running now, outermost first: a trigger never fires itself again
    /// while it runs (`PRAGMA recursive_triggers` is off), and RAISE needs one.
    pub triggers: RefCell<Vec<String>>,
    /// Rows the statement itself (not its triggers) has changed so far, which a
    /// statement that fails under FAIL still reports.
    pub direct: Cell<u64>,
}
impl Env {
    /// One row changed: by the statement itself when no trigger program is running.
    pub fn count_direct(&self) {
        if self.triggers.borrow().is_empty() {
            self.direct.set(self.direct.get() + 1);
        }
    }
}
/// Common table expressions in scope, innermost last.
pub type Ctes = Rc<Vec<(String, Rc<Relation>)>>;
#[derive(Clone)]
pub struct Ctx<'a> {
    pub state: &'a State,
    pub params: &'a [Value],
    pub env: &'a Env,
    pub ctes: Rc<Vec<(String, Rc<Relation>)>>,
    /// Collects `EXPLAIN QUERY PLAN` lines when set.
    pub plan: Option<&'a RefCell<Vec<(usize, String)>>>,
    pub depth: usize,
    /// What the SELECT core being planned reads, for covering indexes and sort
    /// avoidance; `None` outside a SELECT (an UPDATE or DELETE reads whole rows).
    pub shape: Option<Rc<Shape>>,
}
/// What one SELECT core reads from each of its tables, and the orders it asks for.
#[derive(Debug, Default)]
pub struct Shape {
    /// Per source alias (lower case): the column names read, or `None` for every one.
    pub needed: std::collections::BTreeMap<String, Option<std::collections::BTreeSet<String>>>,
    /// ORDER BY as (qualifier, column, descending), when every term is a plain column.
    pub order: Option<Vec<(Option<String>, String, bool)>>,
    /// GROUP BY as (qualifier, column), when every term is a plain column.
    pub group: Option<Vec<(Option<String>, String)>>,
    /// The FROM clause is one base table, the only case where its order is kept.
    pub single: bool,
    /// Set by the planner: the chosen path already delivers the ORDER BY order.
    pub ordered: Cell<bool>,
    /// Set by the planner: the chosen path already delivers rows grouped.
    pub grouped: Cell<bool>,
}
impl Ctx<'_> {
    pub fn note(&self, detail: impl Into<String>) {
        if let Some(plan) = self.plan {
            plan.borrow_mut().push((self.depth, detail.into()));
        }
    }
}

pub type Aggregates = HashMap<usize, Value>;
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    pub cols: &'a [ColMeta],
    pub row: &'a [Value],
    pub parent: Option<&'a Scope<'a>>,
    pub aggs: Option<&'a Aggregates>,
}
pub fn key_of(e: &Expr) -> usize {
    e as *const Expr as usize
}

pub fn is_aggregate(name: &str, argc: usize, star: bool) -> bool {
    match name {
        "count" | "sum" | "total" | "avg" | "group_concat" | "string_agg" => true,
        "min" | "max" => argc == 1 && !star,
        _ => false,
    }
}

/// Where a column reference lands: which scope level and which column.
fn resolve<'a>(
    scope: Option<&'a Scope<'a>>,
    table: Option<&str>,
    name: &str,
) -> Result<Option<(&'a Scope<'a>, usize)>, SqlError> {
    let mut level = scope;
    while let Some(s) = level {
        let mut found = None;
        for (i, c) in s.cols.iter().enumerate() {
            let name_ok = c.name.eq_ignore_ascii_case(name);
            let table_ok = match table {
                Some(t) => c
                    .table
                    .as_deref()
                    .is_some_and(|ct| ct.eq_ignore_ascii_case(t)),
                None => !c.merged,
            };
            if !(name_ok && table_ok) {
                continue;
            }
            if c.hidden {
                // The rowid is only a fallback when no real column has that name.
                if found.is_none() {
                    found = Some((i, true));
                }
                continue;
            }
            match found {
                Some((_, false)) => {
                    let shown = match table {
                        Some(t) => format!("{t}.{name}"),
                        None => name.to_owned(),
                    };
                    return Err(SqlError::new(format!("ambiguous column name: {shown}")));
                }
                _ => found = Some((i, false)),
            }
        }
        if let Some((i, _)) = found {
            return Ok(Some((s, i)));
        }
        level = s.parent;
    }
    Ok(None)
}
fn no_such_column(table: Option<&str>, name: &str) -> SqlError {
    match table {
        Some(t) => SqlError::new(format!("no such column: {t}.{name}")),
        None => SqlError::new(format!("no such column: {name}")),
    }
}

/// Affinity an expression carries into a comparison: only column references and CASTs
/// have one.
pub fn affinity_of(scope: Option<&Scope>, e: &Expr) -> Option<Affinity> {
    match e {
        Expr::Column { table, name } => resolve(scope, table.as_deref(), name)
            .ok()
            .flatten()
            .and_then(|(s, i)| s.cols[i].affinity),
        Expr::Bound { affinity, .. } => *affinity,
        Expr::Cast { type_name, .. } => Some(Affinity::from_type(type_name)),
        Expr::Collate { expr, .. } => affinity_of(scope, expr),
        _ => None,
    }
}
/// Collation of an expression: an explicit COLLATE wins, then a column's declared one.
pub fn collation_of(scope: Option<&Scope>, e: &Expr) -> Option<(Collation, bool)> {
    match e {
        Expr::Collate { collation, .. } => {
            Some((Collation::parse(collation).unwrap_or_default(), true))
        }
        Expr::Column { table, name } => resolve(scope, table.as_deref(), name)
            .ok()
            .flatten()
            .map(|(s, i)| (s.cols[i].collation, false)),
        Expr::Bound { collation, .. } => Some((*collation, false)),
        _ => None,
    }
}
pub fn comparison_collation(scope: Option<&Scope>, a: &Expr, b: &Expr) -> Collation {
    let ca = collation_of(scope, a);
    let cb = collation_of(scope, b);
    match (ca, cb) {
        (Some((c, true)), _) => c,
        (_, Some((c, true))) => c,
        (Some((c, false)), _) => c,
        (_, Some((c, false))) => c,
        _ => Collation::Binary,
    }
}
/// Apply SQLite's comparison affinity rules to a pair of operands.
pub fn coerce_pair(
    a: Value,
    aff_a: Option<Affinity>,
    b: Value,
    aff_b: Option<Affinity>,
) -> (Value, Value) {
    let numeric = |x: Option<Affinity>| x.is_some_and(Affinity::numeric);
    let textish = |x: Option<Affinity>| matches!(x, None | Some(Affinity::Text | Affinity::Blob));
    if numeric(aff_a) && textish(aff_b) {
        return (a, Affinity::Numeric.apply(b));
    }
    if numeric(aff_b) && textish(aff_a) {
        return (Affinity::Numeric.apply(a), b);
    }
    if aff_a == Some(Affinity::Text) && aff_b.is_none() {
        return (a, Affinity::Text.apply(b));
    }
    if aff_b == Some(Affinity::Text) && aff_a.is_none() {
        return (Affinity::Text.apply(a), b);
    }
    (a, b)
}
fn bool_value(b: bool) -> Value {
    Value::Integer(i64::from(b))
}
fn truth(v: &Value) -> Option<bool> {
    v.truth()
}

pub fn eval(ctx: &Ctx, scope: Option<&Scope>, e: &Expr) -> Result<Value, SqlError> {
    match e {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Bound { value, .. } => Ok(value.clone()),
        Expr::Raise(kind, message) => {
            if ctx.env.triggers.borrow().is_empty() {
                return Err(SqlError::new(
                    "RAISE() may only be used within a trigger-program",
                ));
            }
            Err(SqlError::raise(*kind, message.clone().unwrap_or_default()))
        }
        Expr::Param(p) => {
            let n: usize = p[1..].parse().unwrap_or(0);
            Ok(ctx
                .params
                .get(n.wrapping_sub(1))
                .cloned()
                .unwrap_or(Value::Null))
        }
        Expr::Column { table, name } => match resolve(scope, table.as_deref(), name)? {
            Some((s, i)) => Ok(s.row.get(i).cloned().unwrap_or(Value::Null)),
            None => Err(no_such_column(table.as_deref(), name)),
        },
        Expr::Unary(op, inner) => {
            let v = eval(ctx, scope, inner)?;
            Ok(match op {
                UnOp::Plus => v,
                UnOp::Neg => match v.to_number() {
                    Value::Null => Value::Null,
                    Value::Integer(i) => match i.checked_neg() {
                        Some(n) => Value::Integer(n),
                        None => Value::Real(-(i as f64)),
                    },
                    Value::Real(r) => Value::Real(-r),
                    other => other,
                },
                UnOp::Not => match truth(&v) {
                    None => Value::Null,
                    Some(b) => bool_value(!b),
                },
                UnOp::BitNot => match v.to_number() {
                    Value::Null => Value::Null,
                    n => Value::Integer(!n.to_i64().unwrap_or(0)),
                },
            })
        }
        Expr::Binary(op, a, b) => binary(ctx, scope, *op, a, b),
        Expr::Is { left, right, not } => {
            let (av, bv) = (eval(ctx, scope, left)?, eval(ctx, scope, right)?);
            let eq = match (&av, &bv) {
                (Value::Null, Value::Null) => true,
                (Value::Null, _) | (_, Value::Null) => false,
                _ => {
                    let (x, y) = coerce_pair(
                        av.clone(),
                        affinity_of(scope, left),
                        bv.clone(),
                        affinity_of(scope, right),
                    );
                    compare(&x, &y, comparison_collation(scope, left, right)) == Ordering::Equal
                }
            };
            Ok(bool_value(eq != *not))
        }
        Expr::Like {
            op,
            expr,
            pattern,
            escape,
            not,
        } => {
            let v = eval(ctx, scope, expr)?;
            let p = eval(ctx, scope, pattern)?;
            let esc = match escape {
                Some(x) => Some(eval(ctx, scope, x)?),
                None => None,
            };
            if v.is_null() || p.is_null() || esc.as_ref().is_some_and(Value::is_null) {
                return Ok(Value::Null);
            }
            let esc_char = match &esc {
                Some(x) => {
                    let t = x.to_text();
                    let mut chars = t.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) => Some(c),
                        _ => {
                            return Err(SqlError::new(
                                "ESCAPE expression must be a single character",
                            ))
                        }
                    }
                }
                None => None,
            };
            let matched = match op {
                LikeOp::Like => like(&p.to_text(), &v.to_text(), esc_char),
                LikeOp::Glob => glob(&p.to_text(), &v.to_text()),
            };
            Ok(bool_value(matched != *not))
        }
        Expr::Between {
            expr,
            low,
            high,
            not,
        } => {
            let v = eval(ctx, scope, expr)?;
            let lo = eval(ctx, scope, low)?;
            let hi = eval(ctx, scope, high)?;
            let ge = compare_op(scope, v.clone(), expr, lo, low, BinOp::Ge);
            let le = compare_op(scope, v, expr, hi, high, BinOp::Le);
            let r = and3(ge, le);
            Ok(match r {
                None => Value::Null,
                Some(b) => bool_value(b != *not),
            })
        }
        Expr::InList { expr, list, not } => {
            let v = eval(ctx, scope, expr)?;
            if list.is_empty() {
                return Ok(bool_value(*not));
            }
            if v.is_null() {
                return Ok(Value::Null);
            }
            let mut saw_null = false;
            for item in list {
                let iv = eval(ctx, scope, item)?;
                match compare_op(scope, v.clone(), expr, iv, item, BinOp::Eq) {
                    Some(true) => return Ok(bool_value(!*not)),
                    None => saw_null = true,
                    Some(false) => {}
                }
            }
            Ok(if saw_null {
                Value::Null
            } else {
                bool_value(*not)
            })
        }
        Expr::InSelect { expr, select, not } => {
            let rel = crate::exec::select(ctx, select, scope)?;
            if let Expr::Row(items) = expr.as_ref() {
                if rel.cols.len() != items.len() {
                    return Err(SqlError::new(format!(
                        "sub-select returns {} columns - expected {}",
                        rel.cols.len(),
                        items.len()
                    )));
                }
                let vals: Vec<Value> = items
                    .iter()
                    .map(|i| eval(ctx, scope, i))
                    .collect::<Result<_, _>>()?;
                let hit = rel
                    .rows
                    .iter()
                    .any(|r| r.iter().zip(&vals).all(|(a, b)| !a.is_null() && same(a, b)));
                return Ok(bool_value(hit != *not));
            }
            if rel.cols.len() != 1 {
                return Err(SqlError::new(format!(
                    "sub-select returns {} columns - expected 1",
                    rel.cols.len()
                )));
            }
            let v = eval(ctx, scope, expr)?;
            if rel.rows.is_empty() {
                return Ok(bool_value(*not));
            }
            if v.is_null() {
                return Ok(Value::Null);
            }
            let aff = affinity_of(scope, expr).or(rel.cols[0].affinity);
            let coll = collation_of(scope, expr).map_or(rel.cols[0].collation, |c| c.0);
            let mut saw_null = false;
            for row in &rel.rows {
                if row[0].is_null() {
                    saw_null = true;
                    continue;
                }
                let (x, y) = coerce_pair(v.clone(), aff, row[0].clone(), rel.cols[0].affinity);
                if compare(&x, &y, coll) == Ordering::Equal {
                    return Ok(bool_value(!*not));
                }
            }
            Ok(if saw_null {
                Value::Null
            } else {
                bool_value(*not)
            })
        }
        Expr::InTable { expr, table, not } => {
            let select = crate::parser::Parser::new(&format!(
                "SELECT * FROM \"{}\"",
                table.replace('"', "\"\"")
            ))?
            .select()?;
            eval(
                ctx,
                scope,
                &Expr::InSelect {
                    expr: expr.clone(),
                    select: Box::new(select),
                    not: *not,
                },
            )
        }
        Expr::Exists(select) => {
            let rel = crate::exec::select(ctx, select, scope)?;
            Ok(bool_value(!rel.rows.is_empty()))
        }
        Expr::Subquery(select) => {
            let rel = crate::exec::select(ctx, select, scope)?;
            if rel.cols.len() != 1 {
                return Err(SqlError::new(format!(
                    "sub-select returns {} columns - expected 1",
                    rel.cols.len()
                )));
            }
            Ok(rel
                .rows
                .into_iter()
                .next()
                .and_then(|r| r.into_iter().next())
                .unwrap_or(Value::Null))
        }
        Expr::Case {
            operand,
            whens,
            otherwise,
        } => {
            let base = match operand {
                Some(o) => Some(eval(ctx, scope, o)?),
                None => None,
            };
            for (w, t) in whens {
                let hit = match (&base, operand) {
                    (Some(b), Some(o)) => {
                        let wv = eval(ctx, scope, w)?;
                        compare_op(scope, b.clone(), o, wv, w, BinOp::Eq) == Some(true)
                    }
                    _ => truth(&eval(ctx, scope, w)?) == Some(true),
                };
                if hit {
                    return eval(ctx, scope, t);
                }
            }
            match otherwise {
                Some(o) => eval(ctx, scope, o),
                None => Ok(Value::Null),
            }
        }
        Expr::Cast { expr, type_name } => Ok(cast(eval(ctx, scope, expr)?, type_name)),
        Expr::Collate { expr, collation } => {
            if Collation::parse(collation).is_none() {
                return Err(SqlError::new(format!(
                    "no such collation sequence: {collation}"
                )));
            }
            eval(ctx, scope, expr)
        }
        Expr::Row(_) => Err(SqlError::new("row value misused")),
        Expr::Function {
            name,
            args,
            distinct,
            star,
            filter,
            window,
        } => {
            if *window {
                return Err(SqlError::new(format!(
                    "window functions are not supported: {name}()"
                )));
            }
            if is_aggregate(name, args.len(), *star) {
                let mut level = scope;
                while let Some(s) = level {
                    if let Some(v) = s.aggs.and_then(|m| m.get(&key_of(e))) {
                        return Ok(v.clone());
                    }
                    level = s.parent;
                }
                return Err(SqlError::new(format!(
                    "misuse of aggregate function {name}()"
                )));
            }
            if *distinct || filter.is_some() {
                return Err(SqlError::new(format!(
                    "DISTINCT or FILTER used with non-aggregate function {name}()"
                )));
            }
            match name.as_str() {
                "coalesce" | "ifnull" => {
                    if name == "ifnull" && args.len() != 2 || args.len() < 2 {
                        return Err(SqlError::new(format!(
                            "wrong number of arguments to function {name}()"
                        )));
                    }
                    for a in args {
                        let v = eval(ctx, scope, a)?;
                        if !v.is_null() {
                            return Ok(v);
                        }
                    }
                    Ok(Value::Null)
                }
                "iif" => {
                    if args.len() != 3 {
                        return Err(SqlError::new("wrong number of arguments to function iif()"));
                    }
                    if truth(&eval(ctx, scope, &args[0])?) == Some(true) {
                        eval(ctx, scope, &args[1])
                    } else {
                        eval(ctx, scope, &args[2])
                    }
                }
                _ => {
                    if *star {
                        return Err(SqlError::new(format!(
                            "wrong number of arguments to function {name}()"
                        )));
                    }
                    let values: Vec<Value> = args
                        .iter()
                        .map(|a| eval(ctx, scope, a))
                        .collect::<Result<_, _>>()?;
                    let collation = args
                        .first()
                        .and_then(|a| collation_of(scope, a))
                        .map_or(Collation::Binary, |c| c.0);
                    crate::func::call(ctx, name, &values, collation)
                }
            }
        }
    }
}
fn and3(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}
/// A comparison between two evaluated operands, honouring their expressions' affinity
/// and collation. `None` is SQL's unknown.
pub fn compare_op(
    scope: Option<&Scope>,
    a: Value,
    ae: &Expr,
    b: Value,
    be: &Expr,
    op: BinOp,
) -> Option<bool> {
    if a.is_null() || b.is_null() {
        return None;
    }
    let (x, y) = coerce_pair(a, affinity_of(scope, ae), b, affinity_of(scope, be));
    let o = compare(&x, &y, comparison_collation(scope, ae, be));
    Some(match op {
        BinOp::Eq => o == Ordering::Equal,
        BinOp::Ne => o != Ordering::Equal,
        BinOp::Lt => o == Ordering::Less,
        BinOp::Le => o != Ordering::Greater,
        BinOp::Gt => o == Ordering::Greater,
        BinOp::Ge => o != Ordering::Less,
        _ => false,
    })
}
fn binary(
    ctx: &Ctx,
    scope: Option<&Scope>,
    op: BinOp,
    a: &Expr,
    b: &Expr,
) -> Result<Value, SqlError> {
    match op {
        BinOp::And => {
            let av = truth(&eval(ctx, scope, a)?);
            if av == Some(false) {
                return Ok(bool_value(false));
            }
            let bv = truth(&eval(ctx, scope, b)?);
            Ok(match and3(av, bv) {
                None => Value::Null,
                Some(x) => bool_value(x),
            })
        }
        BinOp::Or => {
            let av = truth(&eval(ctx, scope, a)?);
            if av == Some(true) {
                return Ok(bool_value(true));
            }
            let bv = truth(&eval(ctx, scope, b)?);
            Ok(match (av, bv) {
                (_, Some(true)) => bool_value(true),
                (Some(false), Some(false)) => bool_value(false),
                _ => Value::Null,
            })
        }
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            if let (Expr::Row(xs), Expr::Row(ys)) = (a, b) {
                return row_compare(ctx, scope, xs, ys, op);
            }
            let av = eval(ctx, scope, a)?;
            let bv = eval(ctx, scope, b)?;
            Ok(match compare_op(scope, av, a, bv, b, op) {
                None => Value::Null,
                Some(x) => bool_value(x),
            })
        }
        _ => {
            let av = eval(ctx, scope, a)?;
            let bv = eval(ctx, scope, b)?;
            arithmetic(op, &av, &bv)
        }
    }
}
fn row_compare(
    ctx: &Ctx,
    scope: Option<&Scope>,
    xs: &[Expr],
    ys: &[Expr],
    op: BinOp,
) -> Result<Value, SqlError> {
    if xs.len() != ys.len() {
        return Err(SqlError::new("row value misused"));
    }
    let mut unknown = false;
    for (x, y) in xs.iter().zip(ys) {
        let (xv, yv) = (eval(ctx, scope, x)?, eval(ctx, scope, y)?);
        match compare_op(scope, xv, x, yv, y, BinOp::Eq) {
            None => {
                unknown = true;
                if matches!(op, BinOp::Eq | BinOp::Ne) {
                    continue;
                }
                return Ok(Value::Null);
            }
            Some(true) => continue,
            Some(false) => {
                if matches!(op, BinOp::Eq) {
                    return Ok(bool_value(false));
                }
                if matches!(op, BinOp::Ne) {
                    return Ok(bool_value(true));
                }
                let (xv, yv) = (eval(ctx, scope, x)?, eval(ctx, scope, y)?);
                return Ok(match compare_op(scope, xv, x, yv, y, op) {
                    None => Value::Null,
                    Some(b) => bool_value(b),
                });
            }
        }
    }
    if unknown {
        return Ok(Value::Null);
    }
    Ok(bool_value(matches!(op, BinOp::Eq | BinOp::Le | BinOp::Ge)))
}
pub fn arithmetic(op: BinOp, a: &Value, b: &Value) -> Result<Value, SqlError> {
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    if op == BinOp::Concat {
        return Ok(Value::Text(a.to_text() + &b.to_text()));
    }
    let (x, y) = (a.to_number(), b.to_number());
    if matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::Shl | BinOp::Shr) {
        let (i, j) = (x.to_i64().unwrap_or(0), y.to_i64().unwrap_or(0));
        return Ok(Value::Integer(match op {
            BinOp::BitAnd => i & j,
            BinOp::BitOr => i | j,
            BinOp::Shl => shift(i, j),
            _ => shift(i, j.checked_neg().unwrap_or(i64::MAX)),
        }));
    }
    if let (Value::Integer(i), Value::Integer(j)) = (&x, &y) {
        let (i, j) = (*i, *j);
        let r = match op {
            BinOp::Add => i.checked_add(j),
            BinOp::Sub => i.checked_sub(j),
            BinOp::Mul => i.checked_mul(j),
            BinOp::Div => {
                if j == 0 {
                    return Ok(Value::Null);
                }
                i.checked_div(j)
            }
            BinOp::Rem => {
                if j == 0 {
                    return Ok(Value::Null);
                }
                Some(i.checked_rem(j).unwrap_or(0))
            }
            _ => None,
        };
        if let Some(r) = r {
            return Ok(Value::Integer(r));
        }
    }
    let (f, g) = (x.to_f64().unwrap_or(0.0), y.to_f64().unwrap_or(0.0));
    Ok(match op {
        BinOp::Add => Value::real(f + g),
        BinOp::Sub => Value::real(f - g),
        BinOp::Mul => Value::real(f * g),
        BinOp::Div => {
            if g == 0.0 {
                Value::Null
            } else {
                Value::real(f / g)
            }
        }
        BinOp::Rem => {
            let (i, j) = (crate::value::real_to_int(f), crate::value::real_to_int(g));
            if j == 0 {
                Value::Null
            } else {
                Value::Real(i.checked_rem(j).unwrap_or(0) as f64)
            }
        }
        _ => Value::Null,
    })
}
fn shift(i: i64, by: i64) -> i64 {
    if by >= 64 {
        0
    } else if by >= 0 {
        ((i as u64) << by) as i64
    } else if by <= -64 {
        if i < 0 {
            -1
        } else {
            0
        }
    } else {
        i >> (-by)
    }
}
/// CAST semantics by the target type's affinity.
pub fn cast(v: Value, type_name: &str) -> Value {
    if v.is_null() {
        return v;
    }
    match Affinity::from_type(type_name) {
        Affinity::Integer => match v.to_number() {
            Value::Real(r) => Value::Integer(crate::value::real_to_int(r)),
            n => {
                // Text is read by its integer prefix only: '1e2' is 1.
                if let Value::Text(t) = &v {
                    let t = t.trim_start();
                    let end = t
                        .char_indices()
                        .take_while(|(i, c)| {
                            c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))
                        })
                        .map(|(i, c)| i + c.len_utf8())
                        .last()
                        .unwrap_or(0);
                    return Value::Integer(
                        t[..end].parse().unwrap_or_else(|_| n.to_i64().unwrap_or(0)),
                    );
                }
                n
            }
        },
        Affinity::Real => match v.to_number() {
            Value::Integer(i) => Value::Real(i as f64),
            n => n,
        },
        Affinity::Numeric => match v {
            Value::Text(_) | Value::Blob(_) => match v.to_number() {
                Value::Real(r) if r == r.trunc() && r.abs() < 9.2e18 => Value::Integer(r as i64),
                n => n,
            },
            other => other,
        },
        Affinity::Text => Value::Text(v.to_text()),
        Affinity::Blob => match v {
            Value::Blob(_) => v,
            other => Value::Blob(other.to_text().into_bytes()),
        },
    }
}

/// SQL LIKE: `%` and `_`, case-insensitive for ASCII letters.
pub fn like(pattern: &str, text: &str, escape: Option<char>) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    fn go(p: &[char], t: &[char], esc: Option<char>) -> bool {
        let mut pi = 0;
        let mut ti = 0;
        let mut star: Option<(usize, usize)> = None;
        loop {
            if pi < p.len() {
                let c = p[pi];
                if Some(c) == esc && pi + 1 < p.len() {
                    if ti < t.len() && t[ti].eq_ignore_ascii_case(&p[pi + 1]) {
                        pi += 2;
                        ti += 1;
                        continue;
                    }
                } else if c == '%' {
                    star = Some((pi, ti));
                    pi += 1;
                    continue;
                } else if ti < t.len() && (c == '_' || c.eq_ignore_ascii_case(&t[ti])) {
                    pi += 1;
                    ti += 1;
                    continue;
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
    go(&p, &t, escape)
}
/// SQL GLOB: `*`, `?` and `[...]` classes, case-sensitive.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    fn class(p: &[char], mut i: usize, c: char) -> Option<(bool, usize)> {
        // p[i] is just past '['.
        let negate = p.get(i) == Some(&'^');
        if negate {
            i += 1;
        }
        let mut hit = false;
        let mut first = true;
        while i < p.len() && (p[i] != ']' || first) {
            if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
                if p[i] <= c && c <= p[i + 2] {
                    hit = true;
                }
                i += 3;
            } else {
                if p[i] == c {
                    hit = true;
                }
                i += 1;
            }
            first = false;
        }
        if i >= p.len() {
            return None;
        }
        Some((hit != negate, i + 1))
    }
    fn go(p: &[char], t: &[char]) -> bool {
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
                    '?' if ti < t.len() => {
                        pi += 1;
                        ti += 1;
                        continue;
                    }
                    '[' if ti < t.len() => {
                        if let Some((ok, next)) = class(p, pi + 1, t[ti]) {
                            if ok {
                                pi = next;
                                ti += 1;
                                continue;
                            }
                        }
                    }
                    c if ti < t.len() && c == t[ti] && c != '?' && c != '[' => {
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
    go(&p, &t)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn like_and_glob_match_like_sqlite() {
        assert!(like("a%", "ABC", None));
        assert!(like("_b_", "abc", None));
        assert!(!like("a_", "abc", None));
        assert!(like("100\\%", "100%", Some('\\')));
        assert!(!like("100\\%", "1000", Some('\\')));
        assert!(glob("a*c", "abbbc"));
        assert!(!glob("a*c", "Abc"));
        assert!(glob("[a-c]?", "bz"));
        assert!(!glob("[^a-c]?", "bz"));
    }
    #[test]
    fn integer_arithmetic_overflows_into_reals() {
        let big = Value::Integer(i64::MAX);
        assert_eq!(
            arithmetic(BinOp::Add, &big, &Value::Integer(1)).unwrap(),
            Value::Real(9.223_372_036_854_776e18)
        );
        assert_eq!(
            arithmetic(BinOp::Div, &Value::Integer(7), &Value::Integer(2)).unwrap(),
            Value::Integer(3)
        );
        assert_eq!(
            arithmetic(BinOp::Div, &Value::Integer(7), &Value::Integer(0)).unwrap(),
            Value::Null
        );
        assert_eq!(
            arithmetic(BinOp::Rem, &Value::Real(5.5), &Value::Integer(2)).unwrap(),
            Value::Real(1.0)
        );
    }
}
