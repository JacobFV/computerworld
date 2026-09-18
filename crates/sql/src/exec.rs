//! Query execution: FROM with an index-aware join planner, WHERE, grouping and
//! aggregates, ORDER BY / LIMIT, compound selects and CTEs.
use crate::ast::*;
use crate::eval::{
    affinity_of, coerce_pair, collation_of, eval, is_aggregate, key_of, Aggregates, ColMeta, Ctx,
    Relation, Scope,
};
use crate::schema::{Index, Table};
use crate::value::{compare, fold, same, Affinity, Collation, Key, Value};
use crate::SqlError;
use std::cmp::Ordering;
use std::rc::Rc;
use std::sync::Arc;

/// Rows a recursive CTE may produce before the engine calls it runaway.
const RECURSION_LIMIT: usize = 1_000_000;

pub fn table_cols(table: &Table, alias: &str) -> Vec<ColMeta> {
    let mut cols: Vec<ColMeta> = table
        .columns
        .iter()
        .map(|c| ColMeta {
            table: Some(alias.to_owned()),
            name: c.name.clone(),
            affinity: Some(c.affinity),
            collation: c.collation,
            hidden: false,
            merged: false,
        })
        .collect();
    cols.push(ColMeta {
        table: Some(alias.to_owned()),
        name: "rowid".into(),
        affinity: Some(Affinity::Integer),
        collation: Collation::Binary,
        hidden: true,
        merged: false,
    });
    cols
}
pub fn table_row(table: &Table, rowid: i64, stored: &[Value]) -> Vec<Value> {
    let mut row = table.full_row(rowid, stored);
    row.push(Value::Integer(rowid));
    row
}
fn limit_value(ctx: &Ctx, e: &Option<Expr>, what: &str) -> Result<Option<i64>, SqlError> {
    match e {
        None => Ok(None),
        Some(e) => {
            let v = eval(ctx, None, e)?;
            match v.to_number() {
                Value::Integer(i) if !matches!(v, Value::Text(ref t) if crate::value::exact_number(t).is_none()) => {
                    Ok(Some(i))
                }
                _ => Err(SqlError::new(format!("datatype mismatch in {what}")).with_code(20)),
            }
        }
    }
}

/// Run a SELECT, with `outer` as the scope correlated subqueries see.
pub fn select(ctx: &Ctx, s: &Select, outer: Option<&Scope>) -> Result<Relation, SqlError> {
    let owned;
    let ctx = match &s.with {
        Some(with) => {
            owned = with_ctes(ctx, with, outer, demand(s))?;
            &owned
        }
        None => ctx,
    };
    let limit = limit_value(ctx, &s.limit, "LIMIT")?;
    let offset = limit_value(ctx, &s.offset, "OFFSET")?.unwrap_or(0).max(0) as usize;
    let apply_limit = |rows: &mut Vec<Vec<Value>>| {
        let start = offset.min(rows.len());
        rows.drain(..start);
        if let Some(l) = limit {
            if l >= 0 {
                rows.truncate(l as usize);
            }
        }
    };
    if s.compounds.is_empty() {
        // The core sorts, because only it knows each ORDER BY term's collation.
        let (cols, rows) = core(ctx, &s.first, outer, &s.order_by)?;
        let mut out: Vec<Vec<Value>> = rows.into_iter().map(|(r, _)| r).collect();
        apply_limit(&mut out);
        return Ok(Relation { cols, rows: out });
    }
    let (cols, first) = core(ctx, &s.first, outer, &[])?;
    let mut rows: Vec<Vec<Value>> = first.into_iter().map(|(r, _)| r).collect();
    let mut distinct = false;
    for (op, c) in &s.compounds {
        let (ccols, more) = core(ctx, c, outer, &[])?;
        if ccols.len() != cols.len() {
            return Err(SqlError::new(format!(
                "SELECTs to the left and right of {} do not have the same number of result columns",
                match op {
                    CompoundOp::Union => "UNION",
                    CompoundOp::UnionAll => "UNION ALL",
                    CompoundOp::Intersect => "INTERSECT",
                    CompoundOp::Except => "EXCEPT",
                }
            )));
        }
        let more: Vec<Vec<Value>> = more.into_iter().map(|(r, _)| r).collect();
        match op {
            CompoundOp::UnionAll => rows.extend(more),
            CompoundOp::Union => {
                rows.extend(more);
                rows = dedupe(rows);
                distinct = true;
            }
            CompoundOp::Intersect => {
                let keep: Vec<Key> = more.into_iter().map(Key).collect();
                rows = dedupe(rows)
                    .into_iter()
                    .filter(|r| keep.iter().any(|k| row_same(&k.0, r)))
                    .collect();
                distinct = true;
            }
            CompoundOp::Except => {
                let drop: Vec<Key> = more.into_iter().map(Key).collect();
                rows = dedupe(rows)
                    .into_iter()
                    .filter(|r| !drop.iter().any(|k| row_same(&k.0, r)))
                    .collect();
                distinct = true;
            }
        }
    }
    if distinct {
        // SQLite produces distinct compound results through a sorted temporary b-tree.
        rows.sort_by_key(|r| Key(r.clone()));
        ctx.note("COMPOUND QUERY");
    }
    if !s.order_by.is_empty() {
        let mut keyed = Vec::new();
        let mut collations = Vec::new();
        for (n, term) in s.order_by.iter().enumerate() {
            collations.push(
                explicit_collation(&term.expr)
                    .unwrap_or(cols[compound_order_column(&term.expr, &cols, n)?].collation),
            );
        }
        for r in rows {
            let mut keys = Vec::new();
            for (n, term) in s.order_by.iter().enumerate() {
                let i = compound_order_column(&term.expr, &cols, n)?;
                keys.push(r[i].clone());
            }
            keyed.push((r, keys));
        }
        ctx.note("USE TEMP B-TREE FOR ORDER BY");
        sort_rows(&mut keyed, &s.order_by, &collations);
        rows = keyed.into_iter().map(|(r, _)| r).collect();
    }
    apply_limit(&mut rows);
    Ok(Relation { cols, rows })
}
fn ordinal_word(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}
fn compound_order_column(e: &Expr, cols: &[ColMeta], n: usize) -> Result<usize, SqlError> {
    let inner = match e {
        Expr::Collate { expr, .. } => expr.as_ref(),
        other => other,
    };
    match inner {
        Expr::Literal(Value::Integer(k)) => {
            if *k >= 1 && (*k as usize) <= cols.len() {
                Ok(*k as usize - 1)
            } else {
                Err(SqlError::new(format!(
                    "{} ORDER BY term out of range - should be between 1 and {}",
                    ordinal_word(n + 1),
                    cols.len()
                )))
            }
        }
        Expr::Column { name, .. } => cols
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                SqlError::new(format!(
                    "{} ORDER BY term does not match any column in the result set",
                    ordinal_word(n + 1)
                ))
            }),
        _ => Err(SqlError::new(format!(
            "{} ORDER BY term does not match any column in the result set",
            ordinal_word(n + 1)
        ))),
    }
}
fn row_same(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y))
}
fn dedupe(rows: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for r in rows {
        if seen.insert(Key(r.clone())) {
            out.push(r);
        }
    }
    out
}
fn explicit_collation(e: &Expr) -> Option<Collation> {
    match e {
        Expr::Collate { collation, .. } => Collation::parse(collation),
        _ => None,
    }
}
fn sort_rows(rows: &mut [(Vec<Value>, Vec<Value>)], terms: &[OrderTerm], collations: &[Collation]) {
    rows.sort_by(|(_, a), (_, b)| {
        for (i, t) in terms.iter().enumerate() {
            let (x, y) = (&a[i], &b[i]);
            let nulls_first = t.nulls_first.unwrap_or(!t.desc);
            let o = match (x.is_null(), y.is_null()) {
                (true, true) => Ordering::Equal,
                (true, false) => {
                    if nulls_first {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                }
                (false, true) => {
                    if nulls_first {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    }
                }
                _ => {
                    let o = compare(x, y, collations[i]);
                    if t.desc {
                        o.reverse()
                    } else {
                        o
                    }
                }
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
}

/// Materialise every CTE of a WITH clause, in order, into a context that sees them.
/// How many rows of which CTE a query can possibly read: a plain `SELECT ... FROM cte
/// LIMIT n` never needs more than `n` (plus its offset). SQLite runs recursive CTEs as
/// coroutines, so an unbounded recursion under such a LIMIT is legitimate there.
fn demand(s: &Select) -> Option<(String, usize)> {
    if !s.compounds.is_empty() || !s.order_by.is_empty() {
        return None;
    }
    let SelectCore::Select {
        distinct: false,
        from: Some(FromItem::Table { name, .. }),
        filter: None,
        group_by,
        having: None,
        columns,
    } = &s.first
    else {
        return None;
    };
    if !group_by.is_empty() {
        return None;
    }
    let mut aggregate = false;
    for c in columns {
        if let ResultCol::Expr { expr, .. } = c {
            expr.walk(&mut |e| {
                if let Expr::Function {
                    name, args, star, ..
                } = e
                {
                    aggregate |= is_aggregate(name, args.len(), *star);
                }
            });
        }
    }
    let literal = |e: &Option<Expr>| match e {
        Some(Expr::Literal(Value::Integer(i))) if *i >= 0 => Some(*i as usize),
        None => Some(0),
        _ => None,
    };
    let limit = match &s.limit {
        Some(Expr::Literal(Value::Integer(i))) if *i >= 0 => *i as usize,
        _ => return None,
    };
    (!aggregate).then(|| {
        (
            name.to_ascii_lowercase(),
            limit + literal(&s.offset).unwrap_or(0),
        )
    })
}
fn with_ctes<'a>(
    ctx: &Ctx<'a>,
    with: &With,
    outer: Option<&Scope>,
    demand: Option<(String, usize)>,
) -> Result<Ctx<'a>, SqlError> {
    let mut out = ctx.clone();
    for cte in &with.ctes {
        let rel = if references(&cte.select, &cte.name) {
            let cap = demand
                .as_ref()
                .filter(|(n, _)| cte.name.eq_ignore_ascii_case(n))
                .map(|(_, c)| *c);
            recursive_cte(&out, cte, outer, cap)?
        } else {
            let mut rel = select(&out, &cte.select, outer)?;
            rename(&mut rel, &cte.columns, &cte.name)?;
            rel
        };
        let mut list = (*out.ctes).clone();
        list.push((cte.name.to_ascii_lowercase(), Rc::new(rel)));
        out.ctes = Rc::new(list);
    }
    Ok(out)
}
fn rename(rel: &mut Relation, columns: &[String], name: &str) -> Result<(), SqlError> {
    if !columns.is_empty() {
        if columns.len() != rel.cols.len() {
            return Err(SqlError::new(format!(
                "table {name} has {} values for {} columns",
                rel.cols.len(),
                columns.len()
            )));
        }
        for (c, n) in rel.cols.iter_mut().zip(columns) {
            c.name = n.clone();
        }
    }
    Ok(())
}
fn references(s: &Select, name: &str) -> bool {
    let mut cores = vec![&s.first];
    cores.extend(s.compounds.iter().map(|(_, c)| c));
    cores.into_iter().any(|c| match c {
        SelectCore::Select { from: Some(f), .. } => from_references(f, name),
        _ => false,
    })
}
fn from_references(f: &FromItem, name: &str) -> bool {
    match f {
        FromItem::Table { name: n, .. } => n.eq_ignore_ascii_case(name),
        FromItem::Subquery { select, .. } => references(select, name),
        FromItem::Join { left, right, .. } => {
            from_references(left, name) || from_references(right, name)
        }
    }
}
fn recursive_cte(
    ctx: &Ctx,
    cte: &Cte,
    outer: Option<&Scope>,
    cap: Option<usize>,
) -> Result<Relation, SqlError> {
    let s = &cte.select;
    let key = cte.name.to_ascii_lowercase();
    let (initial, recursive): (Vec<&SelectCore>, Vec<(CompoundOp, &SelectCore)>) = {
        let mut all = vec![(CompoundOp::UnionAll, &s.first)];
        all.extend(s.compounds.iter().map(|(o, c)| (*o, c)));
        let split = all
            .iter()
            .position(|(_, c)| core_references(c, &cte.name))
            .ok_or_else(|| SqlError::new("recursive reference in a subquery"))?;
        if split == 0 {
            return Err(SqlError::new(format!("circular reference: {}", cte.name)));
        }
        (
            all[..split].iter().map(|(_, c)| *c).collect(),
            all[split..].to_vec(),
        )
    };
    let distinct = recursive.iter().any(|(o, _)| *o == CompoundOp::Union);
    let limit = limit_value(ctx, &s.limit, "LIMIT")?;
    let offset = limit_value(ctx, &s.offset, "OFFSET")?.unwrap_or(0).max(0) as usize;
    let mut cols = Vec::new();
    let mut result: Vec<Vec<Value>> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut queue = Vec::new();
    for c in initial {
        let (ccols, rows) = core(ctx, c, outer, &[])?;
        if cols.is_empty() {
            cols = ccols;
        }
        for (r, _) in rows {
            if !distinct || seen.insert(Key(r.clone())) {
                queue.push(r);
            }
        }
    }
    rename(
        &mut Relation {
            cols: cols.clone(),
            rows: vec![],
        },
        &cte.columns,
        &cte.name,
    )?;
    if !cte.columns.is_empty() {
        for (c, n) in cols.iter_mut().zip(&cte.columns) {
            c.name = n.clone();
        }
    }
    let wanted = match (limit.filter(|l| *l >= 0).map(|l| l as usize + offset), cap) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    while !queue.is_empty() {
        result.extend(queue.iter().cloned());
        if wanted.is_some_and(|w| result.len() >= w) {
            break;
        }
        if result.len() > RECURSION_LIMIT {
            return Err(SqlError::new(format!(
                "recursive query {} produced more than {RECURSION_LIMIT} rows",
                cte.name
            )));
        }
        let mut inner = ctx.clone();
        let mut list = (*inner.ctes).clone();
        list.push((
            key.clone(),
            Rc::new(Relation {
                cols: cols.clone(),
                rows: std::mem::take(&mut queue),
            }),
        ));
        inner.ctes = Rc::new(list);
        for (_, c) in &recursive {
            let (_, rows) = core(&inner, c, outer, &[])?;
            for (r, _) in rows {
                if !distinct || seen.insert(Key(r.clone())) {
                    queue.push(r);
                }
            }
        }
    }
    if !s.order_by.is_empty() {
        let mut keyed = Vec::new();
        for r in result {
            let mut keys = Vec::new();
            for (n, term) in s.order_by.iter().enumerate() {
                keys.push(r[compound_order_column(&term.expr, &cols, n)?].clone());
            }
            keyed.push((r, keys));
        }
        let collations: Vec<Collation> = s
            .order_by
            .iter()
            .enumerate()
            .map(|(n, t)| {
                explicit_collation(&t.expr).unwrap_or_else(|| {
                    compound_order_column(&t.expr, &cols, n)
                        .map_or(Collation::Binary, |i| cols[i].collation)
                })
            })
            .collect();
        sort_rows(&mut keyed, &s.order_by, &collations);
        result = keyed.into_iter().map(|(r, _)| r).collect();
    }
    let start = offset.min(result.len());
    result.drain(..start);
    if let Some(l) = limit.filter(|l| *l >= 0) {
        result.truncate(l as usize);
    }
    Ok(Relation { cols, rows: result })
}
fn core_references(c: &SelectCore, name: &str) -> bool {
    matches!(c, SelectCore::Select { from: Some(f), .. } if from_references(f, name))
}

/// Aggregate calls in an expression, not descending into aggregate arguments or
/// subqueries.
fn collect_aggregates<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) -> Result<(), SqlError> {
    if let Expr::Function {
        name, args, star, ..
    } = e
    {
        if is_aggregate(name, args.len(), *star) {
            for a in args {
                let mut nested = Vec::new();
                collect_aggregates(a, &mut nested)?;
                if !nested.is_empty() {
                    return Err(SqlError::new(format!(
                        "misuse of aggregate function {name}()"
                    )));
                }
            }
            out.push(e);
            return Ok(());
        }
    }
    let mut children: Vec<&'a Expr> = Vec::new();
    match e {
        Expr::Unary(_, a) | Expr::Cast { expr: a, .. } | Expr::Collate { expr: a, .. } => {
            children.push(a)
        }
        Expr::Binary(_, a, b)
        | Expr::Is {
            left: a, right: b, ..
        } => {
            children.push(a);
            children.push(b);
        }
        Expr::Like {
            expr,
            pattern,
            escape,
            ..
        } => {
            children.push(expr);
            children.push(pattern);
            if let Some(x) = escape {
                children.push(x);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            children.extend([expr.as_ref(), low.as_ref(), high.as_ref()]);
        }
        Expr::InList { expr, list, .. } => {
            children.push(expr);
            children.extend(list.iter());
        }
        Expr::InSelect { expr, .. } | Expr::InTable { expr, .. } => children.push(expr),
        Expr::Case {
            operand,
            whens,
            otherwise,
        } => {
            if let Some(o) = operand {
                children.push(o);
            }
            for (w, t) in whens {
                children.push(w);
                children.push(t);
            }
            if let Some(o) = otherwise {
                children.push(o);
            }
        }
        Expr::Function { args, .. } => children.extend(args.iter()),
        Expr::Row(items) => children.extend(items.iter()),
        _ => {}
    }
    for c in children {
        collect_aggregates(c, out)?;
    }
    Ok(())
}

type Keyed = Vec<(Vec<Value>, Vec<Value>)>;

/// Replace bare names that match no source column but do match a result alias with
/// the aliased expression.
fn with_aliases(e: &Expr, aliases: &[(String, &Expr)], cols: &[ColMeta]) -> Expr {
    if aliases.is_empty() {
        return e.clone();
    }
    let map = |x: &Expr| with_aliases(x, aliases, cols);
    let b = |x: &Expr| Box::new(with_aliases(x, aliases, cols));
    match e {
        Expr::Column { table: None, name } => {
            let known = cols.iter().any(|c| c.name.eq_ignore_ascii_case(name));
            match aliases.iter().find(|(a, _)| a.eq_ignore_ascii_case(name)) {
                Some((_, target)) if !known => (*target).clone(),
                _ => e.clone(),
            }
        }
        Expr::Unary(op, a) => Expr::Unary(*op, b(a)),
        Expr::Binary(op, x, y) => Expr::Binary(*op, b(x), b(y)),
        Expr::Is { left, right, not } => Expr::Is {
            left: b(left),
            right: b(right),
            not: *not,
        },
        Expr::Like {
            op,
            expr,
            pattern,
            escape,
            not,
        } => Expr::Like {
            op: *op,
            expr: b(expr),
            pattern: b(pattern),
            escape: escape.as_ref().map(|x| b(x)),
            not: *not,
        },
        Expr::Between {
            expr,
            low,
            high,
            not,
        } => Expr::Between {
            expr: b(expr),
            low: b(low),
            high: b(high),
            not: *not,
        },
        Expr::InList { expr, list, not } => Expr::InList {
            expr: b(expr),
            list: list.iter().map(map).collect(),
            not: *not,
        },
        Expr::InSelect { expr, select, not } => Expr::InSelect {
            expr: b(expr),
            select: select.clone(),
            not: *not,
        },
        Expr::Case {
            operand,
            whens,
            otherwise,
        } => Expr::Case {
            operand: operand.as_ref().map(|x| b(x)),
            whens: whens.iter().map(|(w, t)| (map(w), map(t))).collect(),
            otherwise: otherwise.as_ref().map(|x| b(x)),
        },
        Expr::Cast { expr, type_name } => Expr::Cast {
            expr: b(expr),
            type_name: type_name.clone(),
        },
        Expr::Collate { expr, collation } => Expr::Collate {
            expr: b(expr),
            collation: collation.clone(),
        },
        Expr::Function {
            name,
            args,
            distinct,
            star,
            filter,
            window,
        } => Expr::Function {
            name: name.clone(),
            args: args.iter().map(map).collect(),
            distinct: *distinct,
            star: *star,
            filter: filter.as_ref().map(|x| b(x)),
            window: *window,
        },
        Expr::Row(items) => Expr::Row(items.iter().map(map).collect()),
        other => other.clone(),
    }
}

/// One SELECT core: output columns and rows, each with its ORDER BY keys.
fn core(
    ctx: &Ctx,
    c: &SelectCore,
    outer: Option<&Scope>,
    order_by: &[OrderTerm],
) -> Result<(Vec<ColMeta>, Keyed), SqlError> {
    let (distinct, columns, from, filter, group_by, having) = match c {
        SelectCore::Values(rows) => {
            let n = rows.first().map_or(0, Vec::len);
            let cols: Vec<ColMeta> = (1..=n)
                .map(|i| ColMeta::plain(format!("column{i}")))
                .collect();
            let mut out = Vec::new();
            for r in rows {
                let vals: Vec<Value> = r
                    .iter()
                    .map(|e| eval(ctx, outer, e))
                    .collect::<Result<_, _>>()?;
                let mut keys = Vec::new();
                for (i, t) in order_by.iter().enumerate() {
                    keys.push(vals[compound_order_column(&t.expr, &cols, i)?].clone());
                }
                out.push((vals, keys));
            }
            return Ok((cols, out));
        }
        SelectCore::Select {
            distinct,
            columns,
            from,
            filter,
            group_by,
            having,
        } => (distinct, columns, from, filter, group_by, having),
    };
    let source = match from {
        Some(f) => from_relation(ctx, f, filter.as_ref(), outer)?,
        None => Relation {
            cols: vec![],
            rows: vec![vec![]],
        },
    };
    // Result columns, with * expanded against the source.
    let mut outs: Vec<(Option<&Expr>, Option<usize>, ColMeta)> = Vec::new();
    for rc in columns {
        match rc {
            ResultCol::Star => {
                if from.is_none() {
                    return Err(SqlError::new("no tables specified"));
                }
                for (i, col) in source.cols.iter().enumerate() {
                    if !col.hidden && !col.merged {
                        outs.push((None, Some(i), col.clone()));
                    }
                }
            }
            ResultCol::TableStar(t) => {
                let before = outs.len();
                for (i, col) in source.cols.iter().enumerate() {
                    if !col.hidden
                        && col
                            .table
                            .as_deref()
                            .is_some_and(|x| x.eq_ignore_ascii_case(t))
                    {
                        outs.push((None, Some(i), col.clone()));
                    }
                }
                if outs.len() == before {
                    return Err(SqlError::new(format!("no such table: {t}")));
                }
            }
            ResultCol::Expr { expr, alias, text } => {
                let scope = Scope {
                    cols: &source.cols,
                    row: &[],
                    parent: outer,
                    aggs: None,
                };
                let (name, affinity, collation) = match (alias, expr) {
                    (Some(a), _) => (
                        a.clone(),
                        affinity_of(Some(&scope), expr),
                        collation_of(Some(&scope), expr).map_or(Collation::Binary, |c| c.0),
                    ),
                    (None, Expr::Column { table, name }) => {
                        let found = source.cols.iter().find(|c| {
                            c.name.eq_ignore_ascii_case(name)
                                && table.as_ref().is_none_or(|t| {
                                    c.table
                                        .as_deref()
                                        .is_some_and(|ct| ct.eq_ignore_ascii_case(t))
                                })
                        });
                        let shown = match found {
                            Some(c) if !c.hidden => c.name.clone(),
                            _ => name.clone(),
                        };
                        (
                            shown,
                            affinity_of(Some(&scope), expr),
                            collation_of(Some(&scope), expr).map_or(Collation::Binary, |c| c.0),
                        )
                    }
                    (None, _) => (
                        text.clone(),
                        affinity_of(Some(&scope), expr),
                        collation_of(Some(&scope), expr).map_or(Collation::Binary, |c| c.0),
                    ),
                };
                outs.push((
                    Some(expr),
                    None,
                    ColMeta {
                        table: None,
                        name,
                        affinity,
                        collation,
                        hidden: false,
                        merged: false,
                    },
                ));
            }
        }
    }
    let out_cols: Vec<ColMeta> = outs.iter().map(|o| o.2.clone()).collect();
    // SQLite lets WHERE, GROUP BY, HAVING and ORDER BY name a result column by its
    // alias when no source column has that name.
    let aliases: Vec<(String, &Expr)> = columns
        .iter()
        .filter_map(|rc| match rc {
            ResultCol::Expr {
                expr,
                alias: Some(a),
                ..
            } => Some((a.clone(), expr)),
            _ => None,
        })
        .collect();
    let owned_filter = filter
        .as_ref()
        .map(|f| with_aliases(f, &aliases, &source.cols));
    let filter = &owned_filter;
    let owned_group: Vec<Expr> = group_by
        .iter()
        .map(|g| with_aliases(g, &aliases, &source.cols))
        .collect();
    let group_by = &owned_group;
    let owned_having = having
        .as_ref()
        .map(|h| with_aliases(h, &aliases, &source.cols));
    let having = &owned_having;
    let owned_order: Vec<OrderTerm> = order_by
        .iter()
        .map(|t| OrderTerm {
            expr: match &t.expr {
                // A bare alias is resolved to its output column below.
                Expr::Column { table: None, .. } => t.expr.clone(),
                e => with_aliases(e, &aliases, &source.cols),
            },
            ..t.clone()
        })
        .collect();
    let order_by = &owned_order[..];
    // WHERE
    let mut rows: Vec<&Vec<Value>> = Vec::new();
    for r in &source.rows {
        let keep = match filter {
            Some(f) => {
                let scope = Scope {
                    cols: &source.cols,
                    row: r,
                    parent: outer,
                    aggs: None,
                };
                eval(ctx, Some(&scope), f)?.truth() == Some(true)
            }
            None => true,
        };
        if keep {
            rows.push(r);
        }
    }
    // Which ORDER BY terms name an output column.
    let order_targets: Vec<Option<usize>> = order_by
        .iter()
        .enumerate()
        .map(|(n, t)| {
            let inner = match &t.expr {
                Expr::Collate { expr, .. } => expr.as_ref(),
                e => e,
            };
            match inner {
                Expr::Literal(Value::Integer(k)) => {
                    if *k >= 1 && (*k as usize) <= out_cols.len() {
                        Ok(Some(*k as usize - 1))
                    } else {
                        Err(SqlError::new(format!(
                            "{} ORDER BY term out of range - should be between 1 and {}",
                            ordinal_word(n + 1),
                            out_cols.len()
                        )))
                    }
                }
                Expr::Column { table: None, name } => Ok(columns
                    .iter()
                    .filter_map(|rc| match rc {
                        ResultCol::Expr { alias: Some(a), .. } => Some(a),
                        _ => None,
                    })
                    .position(|a| a.eq_ignore_ascii_case(name))
                    .and_then(|_| {
                        outs.iter()
                            .position(|o| o.0.is_some() && o.2.name.eq_ignore_ascii_case(name))
                    })),
                _ => Ok(None),
            }
        })
        .collect::<Result<_, _>>()?;
    let mut aggs: Vec<&Expr> = Vec::new();
    for (e, _, _) in &outs {
        if let Some(e) = e {
            collect_aggregates(e, &mut aggs)?;
        }
    }
    if let Some(h) = having {
        collect_aggregates(h, &mut aggs)?;
    }
    for (t, target) in order_by.iter().zip(&order_targets) {
        if target.is_none() {
            collect_aggregates(&t.expr, &mut aggs)?;
        }
    }
    for g in group_by {
        let mut nested = Vec::new();
        collect_aggregates(g, &mut nested)?;
        if !nested.is_empty() {
            return Err(SqlError::new(
                "aggregate functions are not allowed in the GROUP BY clause",
            ));
        }
    }
    if let Some(f) = filter {
        let mut nested = Vec::new();
        collect_aggregates(f, &mut nested)?;
        if !nested.is_empty() {
            return Err(SqlError::new("misuse of aggregate function"));
        }
    }
    let grouped = !group_by.is_empty() || !aggs.is_empty();
    if having.is_some() && !grouped {
        return Err(SqlError::new("HAVING clause on a non-aggregate query"));
    }
    let mut out: Keyed = Vec::new();
    let emit = |scope: &Scope, out: &mut Keyed| -> Result<(), SqlError> {
        let mut vals = Vec::with_capacity(outs.len());
        for (e, idx, _) in &outs {
            vals.push(match (e, idx) {
                (Some(e), _) => eval(ctx, Some(scope), e)?,
                (None, Some(i)) => scope.row.get(*i).cloned().unwrap_or(Value::Null),
                _ => Value::Null,
            });
        }
        let mut keys = Vec::with_capacity(order_by.len());
        for (t, target) in order_by.iter().zip(&order_targets) {
            keys.push(match target {
                Some(i) => vals[*i].clone(),
                None => eval(ctx, Some(scope), &t.expr)?,
            });
        }
        out.push((vals, keys));
        Ok(())
    };
    if grouped {
        if !group_by.is_empty() {
            ctx.note("USE TEMP B-TREE FOR GROUP BY");
        }
        // Group rows by their key, in key order as SQLite's sorter produces them.
        let mut groups: std::collections::BTreeMap<Key, Vec<&Vec<Value>>> = Default::default();
        if group_by.is_empty() {
            groups.insert(Key(vec![]), rows.clone());
        } else {
            for r in &rows {
                let scope = Scope {
                    cols: &source.cols,
                    row: r,
                    parent: outer,
                    aggs: None,
                };
                let mut key = Vec::new();
                for g in group_by {
                    // A bare integer in GROUP BY names an output column.
                    let v = match g {
                        Expr::Literal(Value::Integer(k))
                            if *k >= 1 && (*k as usize) <= outs.len() =>
                        {
                            match outs[*k as usize - 1] {
                                (Some(e), _, _) => eval(ctx, Some(&scope), e)?,
                                (None, Some(i), _) => r[i].clone(),
                                _ => Value::Null,
                            }
                        }
                        _ => eval(ctx, Some(&scope), g)?,
                    };
                    let coll = collation_of(Some(&scope), g).map_or(Collation::Binary, |c| c.0);
                    key.push(fold(v, coll));
                }
                groups.entry(Key(key)).or_default().push(r);
            }
        }
        let empty_row = vec![Value::Null; source.cols.len()];
        for members in groups.values() {
            let mut values = Aggregates::new();
            let mut chosen: Option<usize> = None;
            let single_minmax = aggs.len() == 1
                && matches!(aggs[0], Expr::Function { name, .. } if name == "min" || name == "max");
            for a in &aggs {
                let (v, at) = aggregate(ctx, a, members, &source.cols, outer)?;
                if single_minmax {
                    chosen = at;
                }
                values.insert(key_of(a), v);
            }
            let rep: &[Value] = match (chosen, members.last()) {
                (Some(i), _) => members[i],
                (None, Some(last)) => last,
                (None, None) => &empty_row,
            };
            let scope = Scope {
                cols: &source.cols,
                row: rep,
                parent: outer,
                aggs: Some(&values),
            };
            if let Some(h) = having {
                if eval(ctx, Some(&scope), h)?.truth() != Some(true) {
                    continue;
                }
            }
            emit(&scope, &mut out)?;
        }
    } else {
        for r in &rows {
            let scope = Scope {
                cols: &source.cols,
                row: r,
                parent: outer,
                aggs: None,
            };
            emit(&scope, &mut out)?;
        }
    }
    if *distinct {
        ctx.note("USE TEMP B-TREE FOR DISTINCT");
        let mut seen = std::collections::BTreeSet::new();
        out.retain(|(r, _)| seen.insert(Key(r.clone())));
    }
    if !order_by.is_empty() {
        let scope = Scope {
            cols: &source.cols,
            row: &[],
            parent: outer,
            aggs: None,
        };
        let collations: Vec<Collation> = order_by
            .iter()
            .zip(&order_targets)
            .map(|(t, target)| {
                explicit_collation(&t.expr).unwrap_or_else(|| match target {
                    Some(i) => out_cols[*i].collation,
                    None => collation_of(Some(&scope), &t.expr).map_or(Collation::Binary, |c| c.0),
                })
            })
            .collect();
        ctx.note("USE TEMP B-TREE FOR ORDER BY");
        sort_rows(&mut out, order_by, &collations);
    }
    Ok((out_cols, out))
}

/// Evaluate one aggregate over a group. Also returns which member row a min() or
/// max() came from, which bare columns then read (SQLite's documented behaviour).
fn aggregate(
    ctx: &Ctx,
    e: &Expr,
    members: &[&Vec<Value>],
    cols: &[ColMeta],
    outer: Option<&Scope>,
) -> Result<(Value, Option<usize>), SqlError> {
    let Expr::Function {
        name,
        args,
        distinct,
        star,
        filter,
        ..
    } = e
    else {
        return Ok((Value::Null, None));
    };
    let mut inputs: Vec<(usize, Vec<Value>)> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut collation = Collation::Binary;
    for (i, r) in members.iter().enumerate() {
        let scope = Scope {
            cols,
            row: r,
            parent: outer,
            aggs: None,
        };
        if let Some(f) = filter {
            if eval(ctx, Some(&scope), f)?.truth() != Some(true) {
                continue;
            }
        }
        let vals: Vec<Value> = args
            .iter()
            .map(|a| eval(ctx, Some(&scope), a))
            .collect::<Result<_, _>>()?;
        if let Some(a) = args.first() {
            collation = collation_of(Some(&scope), a).map_or(Collation::Binary, |c| c.0);
        }
        if *distinct {
            if vals.len() != 1 {
                return Err(SqlError::new(
                    "DISTINCT aggregates must have exactly one argument",
                ));
            }
            if vals[0].is_null() || !seen.insert(Key(vec![fold(vals[0].clone(), collation)])) {
                continue;
            }
        }
        inputs.push((i, vals));
    }
    let arg_count = |range: std::ops::RangeInclusive<usize>| {
        if range.contains(&args.len()) {
            Ok(())
        } else {
            Err(SqlError::new(format!(
                "wrong number of arguments to function {name}()"
            )))
        }
    };
    Ok(match name.as_str() {
        "count" => {
            if *star {
                (Value::Integer(inputs.len() as i64), None)
            } else {
                arg_count(1..=1)?;
                (
                    Value::Integer(inputs.iter().filter(|(_, v)| !v[0].is_null()).count() as i64),
                    None,
                )
            }
        }
        "sum" | "total" | "avg" => {
            arg_count(1..=1)?;
            let mut int_sum: Option<i64> = Some(0);
            let mut overflow = false;
            // Kahan–Babuška–Neumaier summation, as SQLite 3.43+ does.
            let (mut s, mut c) = (0.0f64, 0.0f64);
            let mut n = 0i64;
            for (_, v) in &inputs {
                let v = &v[0];
                if v.is_null() {
                    continue;
                }
                n += 1;
                let numeric = match v {
                    Value::Text(_) | Value::Blob(_) => Affinity::Numeric.apply(v.clone()),
                    other => other.clone(),
                };
                match numeric {
                    Value::Integer(i) => {
                        if let Some(acc) = int_sum {
                            match acc.checked_add(i) {
                                Some(x) => int_sum = Some(x),
                                None => overflow = true,
                            }
                        }
                        kbn(&mut s, &mut c, i as f64);
                    }
                    other => {
                        int_sum = None;
                        kbn(&mut s, &mut c, other.to_f64().unwrap_or(0.0));
                    }
                }
            }
            let real = s + c;
            let v = match name.as_str() {
                "total" => Value::Real(real),
                "avg" => {
                    if n == 0 {
                        Value::Null
                    } else {
                        Value::Real(real / n as f64)
                    }
                }
                _ => {
                    if n == 0 {
                        Value::Null
                    } else if let Some(i) = int_sum {
                        if overflow {
                            return Err(SqlError::new("integer overflow"));
                        }
                        Value::Integer(i)
                    } else {
                        Value::Real(real)
                    }
                }
            };
            (v, None)
        }
        "min" | "max" => {
            let want = if name == "min" {
                Ordering::Less
            } else {
                Ordering::Greater
            };
            let mut best: Option<(usize, Value)> = None;
            for (i, v) in &inputs {
                if v[0].is_null() {
                    continue;
                }
                match &best {
                    Some((_, b)) if compare(&v[0], b, collation) != want => {}
                    _ => best = Some((*i, v[0].clone())),
                }
            }
            match best {
                Some((i, v)) => (v, Some(i)),
                None => (Value::Null, None),
            }
        }
        "group_concat" | "string_agg" => {
            if name == "string_agg" {
                arg_count(2..=2)?;
            } else {
                arg_count(1..=2)?;
            }
            let mut out: Option<String> = None;
            for (_, v) in &inputs {
                if v[0].is_null() {
                    continue;
                }
                let sep = v.get(1).map_or(",".to_string(), |s| s.to_text());
                match &mut out {
                    Some(o) => {
                        o.push_str(&sep);
                        o.push_str(&v[0].to_text());
                    }
                    None => out = Some(v[0].to_text()),
                }
            }
            (out.map_or(Value::Null, Value::Text), None)
        }
        _ => (Value::Null, None),
    })
}
fn kbn(sum: &mut f64, comp: &mut f64, x: f64) {
    let t = *sum + x;
    if sum.abs() >= x.abs() {
        *comp += (*sum - t) + x;
    } else {
        *comp += (x - t) + *sum;
    }
    *sum = t;
}

// ----- FROM and the planner -----

enum SourceData {
    Table(Arc<Table>, String),
    Rows(Vec<Vec<Value>>),
}
struct Source {
    cols: Vec<ColMeta>,
    data: SourceData,
}
/// Resolve a table name in FROM: CTEs first, then tables, views and the schema table.
fn source_for(ctx: &Ctx, name: &str, alias: Option<&str>) -> Result<Source, SqlError> {
    let key = name.to_ascii_lowercase();
    let shown = alias.unwrap_or(name).to_owned();
    if let Some((_, rel)) = ctx.ctes.iter().rev().find(|(n, _)| *n == key) {
        ctx.note(format!("SCAN {shown}"));
        let cols = rel
            .cols
            .iter()
            .map(|c| ColMeta {
                table: Some(shown.clone()),
                merged: false,
                ..c.clone()
            })
            .collect();
        return Ok(Source {
            cols,
            data: SourceData::Rows(rel.rows.clone()),
        });
    }
    if let Some(t) = ctx.state.tables.get(&key) {
        return Ok(Source {
            cols: table_cols(t, &shown),
            data: SourceData::Table(t.clone(), shown),
        });
    }
    if let Some(v) = ctx.state.views.get(&key) {
        let parsed = crate::parser::Parser::new(&v.select)?.select()?;
        let inner = Ctx {
            ctes: Rc::new(Vec::new()),
            depth: ctx.depth + 1,
            ..ctx.clone()
        };
        let mut rel = select(&inner, &parsed, None)?;
        rename(&mut rel, &v.columns, &v.name)?;
        ctx.note(format!("SCAN {shown}"));
        return Ok(Source {
            cols: rel
                .cols
                .into_iter()
                .map(|c| ColMeta {
                    table: Some(shown.clone()),
                    ..c
                })
                .collect(),
            data: SourceData::Rows(rel.rows),
        });
    }
    if matches!(
        key.as_str(),
        "sqlite_schema" | "sqlite_master" | "sqlite_temp_schema" | "sqlite_temp_master"
    ) {
        let rows = if key.contains("temp") {
            vec![]
        } else {
            crate::schema_rows(ctx.state)
        };
        let cols = ["type", "name", "tbl_name", "rootpage", "sql"]
            .iter()
            .map(|n| ColMeta {
                table: Some(shown.clone()),
                ..ColMeta::plain(*n)
            })
            .collect();
        ctx.note(format!("SCAN {shown}"));
        return Ok(Source {
            cols,
            data: SourceData::Rows(rows),
        });
    }
    Err(SqlError::new(format!("no such table: {name}")))
}
fn flatten<'a>(
    f: &'a FromItem,
    out: &mut Vec<(JoinKind, &'a JoinConstraint, &'a FromItem)>,
    none: &'a JoinConstraint,
) {
    match f {
        FromItem::Join {
            left,
            right,
            kind,
            constraint,
        } => {
            flatten(left, out, none);
            out.push((*kind, constraint, right));
        }
        other => out.push((JoinKind::Cross, none, other)),
    }
}
fn materialise(ctx: &Ctx, item: &FromItem, outer: Option<&Scope>) -> Result<Source, SqlError> {
    match item {
        FromItem::Table { name, alias } => source_for(ctx, name, alias.as_deref()),
        FromItem::Subquery { select: s, alias } => {
            let inner = Ctx {
                depth: ctx.depth + 1,
                ..ctx.clone()
            };
            let rel = select(&inner, s, outer)?;
            let shown = alias.clone();
            ctx.note(format!(
                "SCAN {}",
                shown.clone().unwrap_or_else(|| "(subquery)".into())
            ));
            Ok(Source {
                cols: rel
                    .cols
                    .into_iter()
                    .map(|c| ColMeta {
                        table: shown.clone(),
                        ..c
                    })
                    .collect(),
                data: SourceData::Rows(rel.rows),
            })
        }
        join => {
            let rel = from_relation(ctx, join, None, outer)?;
            Ok(Source {
                cols: rel.cols,
                data: SourceData::Rows(rel.rows),
            })
        }
    }
}
fn conjuncts<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary(BinOp::And, a, b) = e {
        conjuncts(a, out);
        conjuncts(b, out);
    } else {
        out.push(e);
    }
}
/// Does `e` read any column of the source named `alias` with columns `cols`?
fn touches(e: &Expr, alias: &str, cols: &[ColMeta], left: &[ColMeta]) -> bool {
    let mut hit = false;
    e.walk(&mut |x| match x {
        Expr::Column { table: Some(t), .. } => {
            if t.eq_ignore_ascii_case(alias) {
                hit = true;
            }
        }
        Expr::Column { table: None, name } => {
            let here = cols.iter().any(|c| c.name.eq_ignore_ascii_case(name));
            let there = left.iter().any(|c| c.name.eq_ignore_ascii_case(name));
            if here && !there || here && there {
                hit = true;
            }
        }
        Expr::Subquery(_) | Expr::Exists(_) | Expr::InSelect { .. } => hit = true,
        Expr::Function {
            name, args, star, ..
        } if is_aggregate(name, args.len(), *star) => hit = true,
        Expr::Function { name, .. } if name == "random" || name == "randomblob" => hit = true,
        _ => {}
    });
    hit
}
/// The column of the source this expression names, if it is a plain reference to one.
fn own_column(e: &Expr, alias: &str, table: &Table, left: &[ColMeta]) -> Option<Option<usize>> {
    let Expr::Column { table: t, name } = e else {
        return None;
    };
    match t {
        Some(t) if !t.eq_ignore_ascii_case(alias) => return None,
        None if left.iter().any(|c| c.name.eq_ignore_ascii_case(name)) => return None,
        _ => {}
    }
    if let Some(i) = table.column(name) {
        return Some(if table.ipk == Some(i) { None } else { Some(i) });
    }
    if table.is_rowid_name(name) {
        return Some(None);
    }
    None
}
#[derive(Clone)]
enum Bound<'a> {
    Eq(&'a Expr),
    In(&'a [Expr]),
    Range(Option<(&'a Expr, bool)>, Option<(&'a Expr, bool)>),
}
struct Path<'a> {
    /// `None` targets the rowid.
    index: Option<Arc<Index>>,
    /// Equality (or IN) bounds on leading key columns, then an optional range.
    bounds: Vec<Bound<'a>>,
    detail: String,
}
fn plan<'a>(
    table: &Table,
    alias: &str,
    indexes: &[Arc<Index>],
    terms: &[&'a Expr],
    cols: &[ColMeta],
    left: &[ColMeta],
) -> Option<Path<'a>> {
    // (column or rowid, operator, other side)
    let mut facts: Vec<(Option<usize>, BinOp, &'a Expr)> = Vec::new();
    let mut ins: Vec<(Option<usize>, &'a [Expr])> = Vec::new();
    let usable_other = |other: &Expr, col: Option<usize>| -> bool {
        if touches(other, alias, cols, left) {
            return false;
        }
        // Only use the index when the comparison applies the column's own affinity
        // and collation, so the lookup finds exactly what the predicate accepts.
        match other {
            Expr::Collate { .. } => false,
            Expr::Column { .. } => {
                let aff = col.map_or(Some(Affinity::Integer), |c| Some(table.columns[c].affinity));
                let other_aff = left
                    .iter()
                    .find(|c| matches!(other, Expr::Column { name, .. } if c.name.eq_ignore_ascii_case(name)))
                    .and_then(|c| c.affinity);
                aff == other_aff || other_aff.is_none()
            }
            _ => true,
        }
    };
    for t in terms {
        match t {
            Expr::Binary(
                op @ (BinOp::Eq | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge),
                a,
                b,
            ) => {
                if let Some(c) = own_column(a, alias, table, left) {
                    if usable_other(b, c) {
                        facts.push((c, *op, b));
                        continue;
                    }
                }
                if let Some(c) = own_column(b, alias, table, left) {
                    if usable_other(a, c) {
                        let flipped = match op {
                            BinOp::Lt => BinOp::Gt,
                            BinOp::Le => BinOp::Ge,
                            BinOp::Gt => BinOp::Lt,
                            BinOp::Ge => BinOp::Le,
                            o => *o,
                        };
                        facts.push((c, flipped, a));
                    }
                }
            }
            Expr::Between {
                expr,
                low,
                high,
                not: false,
            } => {
                if let Some(c) = own_column(expr, alias, table, left) {
                    if usable_other(low, c) && usable_other(high, c) {
                        facts.push((c, BinOp::Ge, low));
                        facts.push((c, BinOp::Le, high));
                    }
                }
            }
            Expr::InList {
                expr,
                list,
                not: false,
            } => {
                if let Some(c) = own_column(expr, alias, table, left) {
                    if list.iter().all(|e| usable_other(e, c)) {
                        ins.push((c, list));
                    }
                }
            }
            _ => {}
        }
    }
    let range_for = |c: Option<usize>| -> Option<Bound<'a>> {
        let mut lo = None;
        let mut hi = None;
        for (fc, op, e) in &facts {
            if *fc != c {
                continue;
            }
            match op {
                BinOp::Gt => lo = Some((*e, false)),
                BinOp::Ge => lo = Some((*e, true)),
                BinOp::Lt => hi = Some((*e, false)),
                BinOp::Le => hi = Some((*e, true)),
                _ => {}
            }
        }
        (lo.is_some() || hi.is_some()).then_some(Bound::Range(lo, hi))
    };
    let eq_for = |c: Option<usize>| -> Option<Bound<'a>> {
        facts
            .iter()
            .find(|(fc, op, _)| *fc == c && *op == BinOp::Eq)
            .map(|(_, _, e)| Bound::Eq(e))
            .or_else(|| {
                ins.iter()
                    .find(|(ic, _)| *ic == c)
                    .map(|(_, l)| Bound::In(l))
            })
    };
    let describe = |cols: &[String], bounds: &[Bound]| -> String {
        let mut parts = Vec::new();
        for (name, b) in cols.iter().zip(bounds) {
            match b {
                Bound::Eq(_) => parts.push(format!("{name}=?")),
                Bound::In(_) => parts.push(format!("{name}=?")),
                Bound::Range(lo, hi) => {
                    if let Some((_, inc)) = lo {
                        parts.push(format!("{name}>{}?", if *inc { "=" } else { "" }));
                    }
                    if let Some((_, inc)) = hi {
                        parts.push(format!("{name}<{}?", if *inc { "=" } else { "" }));
                    }
                }
            }
        }
        parts.join(" AND ")
    };
    // The rowid is the best path there is.
    if let Some(b) = eq_for(None).or_else(|| range_for(None)) {
        let detail = format!(
            "SEARCH {alias} USING INTEGER PRIMARY KEY ({})",
            describe(&["rowid".into()], std::slice::from_ref(&b))
        );
        return Some(Path {
            index: None,
            bounds: vec![b],
            detail,
        });
    }
    // Otherwise the index with the longest usable prefix; ties go to the earliest.
    let mut best: Option<(usize, Path)> = None;
    for index in indexes {
        let mut bounds = Vec::new();
        let mut names = Vec::new();
        let mut score = 0;
        for ic in &index.columns {
            if table.columns[ic.column].collation != ic.collation {
                break;
            }
            names.push(table.columns[ic.column].name.clone());
            if let Some(b) = eq_for(Some(ic.column)) {
                bounds.push(b);
                score += 2;
                continue;
            }
            if let Some(b) = range_for(Some(ic.column)) {
                bounds.push(b);
                score += 1;
            }
            break;
        }
        if bounds.is_empty() {
            continue;
        }
        if index.unique
            && bounds.len() == index.columns.len()
            && bounds.iter().all(|b| matches!(b, Bound::Eq(_)))
        {
            score += 1;
        }
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            let detail = format!(
                "SEARCH {alias} USING INDEX {} ({})",
                index.name,
                describe(&names, &bounds)
            );
            best = Some((
                score,
                Path {
                    index: Some(index.clone()),
                    bounds,
                    detail,
                },
            ));
        }
    }
    best.map(|(_, p)| p)
}
/// Rowids a path selects, evaluating its bound expressions in `scope`.
fn run_path(
    ctx: &Ctx,
    table: &Table,
    path: &Path,
    scope: Option<&Scope>,
) -> Result<Vec<i64>, SqlError> {
    let key_value = |e: &Expr, col: Option<usize>| -> Result<Value, SqlError> {
        let v = eval(ctx, scope, e)?;
        let aff = col.map_or(Affinity::Integer, |c| table.columns[c].affinity);
        let (_, v) = coerce_pair(Value::Null, Some(aff), v, affinity_of(scope, e));
        let v = if aff == Affinity::Text {
            Affinity::Text.apply(v)
        } else {
            v
        };
        Ok(match col {
            Some(c) => fold(v, table.columns[c].collation),
            None => v,
        })
    };
    match &path.index {
        None => {
            let mut ids = Vec::new();
            match &path.bounds[0] {
                Bound::Eq(e) => {
                    if let Some(id) = rowid_of(&key_value(e, None)?) {
                        if table.rows.contains_key(&id) {
                            ids.push(id);
                        }
                    }
                }
                Bound::In(list) => {
                    for e in *list {
                        if let Some(id) = rowid_of(&key_value(e, None)?) {
                            if table.rows.contains_key(&id) && !ids.contains(&id) {
                                ids.push(id);
                            }
                        }
                    }
                    ids.sort_unstable();
                }
                Bound::Range(lo, hi) => {
                    let lo_v = match lo {
                        Some((e, inc)) => Some((key_value(e, None)?, *inc)),
                        None => None,
                    };
                    let hi_v = match hi {
                        Some((e, inc)) => Some((key_value(e, None)?, *inc)),
                        None => None,
                    };
                    if lo_v.as_ref().is_some_and(|(v, _)| v.is_null())
                        || hi_v.as_ref().is_some_and(|(v, _)| v.is_null())
                    {
                        return Ok(vec![]);
                    }
                    for id in table.rows.keys() {
                        let v = Value::Integer(*id);
                        let ok_lo = lo_v.as_ref().is_none_or(|(b, inc)| {
                            let o = compare(&v, b, Collation::Binary);
                            o == Ordering::Greater || (*inc && o == Ordering::Equal)
                        });
                        let ok_hi = hi_v.as_ref().is_none_or(|(b, inc)| {
                            let o = compare(&v, b, Collation::Binary);
                            o == Ordering::Less || (*inc && o == Ordering::Equal)
                        });
                        if ok_lo && ok_hi {
                            ids.push(*id);
                        }
                    }
                }
            }
            Ok(ids)
        }
        Some(index) => {
            // Expand IN lists into every combination of equality prefixes.
            let mut prefixes: Vec<Vec<Value>> = vec![vec![]];
            let mut range = None;
            for (i, b) in path.bounds.iter().enumerate() {
                let col = Some(index.columns[i].column);
                match b {
                    Bound::Eq(e) => {
                        let v = key_value(e, col)?;
                        if v.is_null() {
                            return Ok(vec![]);
                        }
                        for p in &mut prefixes {
                            p.push(v.clone());
                        }
                    }
                    Bound::In(list) => {
                        let mut vals = Vec::new();
                        for e in *list {
                            let v = key_value(e, col)?;
                            if !v.is_null() && !vals.iter().any(|x| same(x, &v)) {
                                vals.push(v);
                            }
                        }
                        prefixes = prefixes
                            .into_iter()
                            .flat_map(|p| {
                                vals.iter().map(move |v| {
                                    let mut q = p.clone();
                                    q.push(v.clone());
                                    q
                                })
                            })
                            .collect();
                    }
                    Bound::Range(lo, hi) => {
                        let lo_v = match lo {
                            Some((e, inc)) => Some((key_value(e, col)?, *inc)),
                            None => None,
                        };
                        let hi_v = match hi {
                            Some((e, inc)) => Some((key_value(e, col)?, *inc)),
                            None => None,
                        };
                        range = Some((lo_v, hi_v));
                    }
                }
            }
            let mut ids = Vec::new();
            for p in prefixes {
                match &range {
                    None => ids.extend(index.lookup_prefix(&p)),
                    Some((lo, hi)) => {
                        if lo.as_ref().is_some_and(|(v, _)| v.is_null())
                            || hi.as_ref().is_some_and(|(v, _)| v.is_null())
                        {
                            continue;
                        }
                        let n = p.len();
                        let start = Key(p.clone());
                        for k in index.entries.range(start..) {
                            if !k.0.iter().zip(&p).all(|(a, b)| same(a, b)) {
                                break;
                            }
                            let v = &k.0[n];
                            if v.is_null() {
                                continue;
                            }
                            if let Some((b, inc)) = lo {
                                let o = compare(v, b, Collation::Binary);
                                if !(o == Ordering::Greater || (*inc && o == Ordering::Equal)) {
                                    continue;
                                }
                            }
                            if let Some((b, inc)) = hi {
                                let o = compare(v, b, Collation::Binary);
                                if !(o == Ordering::Less || (*inc && o == Ordering::Equal)) {
                                    break;
                                }
                            }
                            if let Some(Value::Integer(id)) = k.0.last() {
                                ids.push(*id);
                            }
                        }
                    }
                }
            }
            Ok(ids)
        }
    }
}
fn rowid_of(v: &Value) -> Option<i64> {
    match v {
        Value::Integer(i) => Some(*i),
        Value::Real(r) if *r == r.trunc() && r.abs() < 9.2e18 => Some(*r as i64),
        _ => None,
    }
}
/// Rows of a base table the given terms select: through an index or rowid when the
/// planner finds one, else every row. Callers still apply the full predicate.
pub fn table_candidates(
    ctx: &Ctx,
    table: &Table,
    alias: &str,
    terms: &[&Expr],
    left: &[ColMeta],
    scope: Option<&Scope>,
    note: bool,
) -> Result<Vec<i64>, SqlError> {
    let cols = table_cols(table, alias);
    let indexes = ctx.state.indexes_of(&table.name);
    match plan(table, alias, &indexes, terms, &cols, left) {
        Some(path) => {
            if note {
                ctx.note(path.detail.clone());
            }
            // A key that cannot be evaluated here (it names a table joined later) makes
            // the path unusable, not the query wrong: fall back to scanning.
            Ok(run_path(ctx, table, &path, scope)
                .unwrap_or_else(|_| table.rows.keys().copied().collect()))
        }
        None => {
            if note {
                ctx.note(format!("SCAN {alias}"));
            }
            Ok(table.rows.keys().copied().collect())
        }
    }
}

/// Evaluate a FROM clause into one relation, using `filter`'s conjuncts to pick access
/// paths where that cannot change the result.
pub fn from_relation(
    ctx: &Ctx,
    f: &FromItem,
    filter: Option<&Expr>,
    outer: Option<&Scope>,
) -> Result<Relation, SqlError> {
    let none = JoinConstraint::None;
    let mut items = Vec::new();
    flatten(f, &mut items, &none);
    let any_right = items
        .iter()
        .any(|(k, _, _)| matches!(k, JoinKind::Right | JoinKind::Full));
    let mut where_terms = Vec::new();
    if let Some(w) = filter {
        conjuncts(w, &mut where_terms);
    }
    let mut cols: Vec<ColMeta> = Vec::new();
    let mut rows: Vec<Vec<Value>> = vec![vec![]];
    let empty_cols: Vec<ColMeta> = Vec::new();
    for (n, (kind, constraint, item)) in items.iter().enumerate() {
        let src = match item {
            FromItem::Table { name, alias } => source_for(ctx, name, alias.as_deref())?,
            other => materialise(ctx, other, outer)?,
        };
        // USING and NATURAL become equality terms on the shared columns.
        let mut using: Vec<String> = match constraint {
            JoinConstraint::Using(c) => c.clone(),
            JoinConstraint::Natural => src
                .cols
                .iter()
                .filter(|c| !c.hidden)
                .filter(|c| {
                    cols.iter()
                        .any(|l| !l.hidden && !l.merged && l.name.eq_ignore_ascii_case(&c.name))
                })
                .map(|c| c.name.clone())
                .collect(),
            _ => vec![],
        };
        using.dedup();
        let mut using_pairs: Vec<(usize, usize)> = Vec::new();
        for u in &using {
            let l = cols
                .iter()
                .position(|c| !c.hidden && !c.merged && c.name.eq_ignore_ascii_case(u));
            let r = src
                .cols
                .iter()
                .position(|c| !c.hidden && c.name.eq_ignore_ascii_case(u));
            match (l, r) {
                (Some(l), Some(r)) => using_pairs.push((l, r)),
                _ => {
                    return Err(SqlError::new(format!(
                        "cannot join using column {u} - column not present in both tables"
                    )))
                }
            }
        }
        let on = match constraint {
            JoinConstraint::On(e) => Some(e),
            _ => None,
        };
        if on.is_some() && n == 0 {
            return Err(SqlError::new("a JOIN clause is required before ON"));
        }
        let mut src_cols = src.cols.clone();
        for (_, r) in &using_pairs {
            src_cols[*r].merged = true;
        }
        let mut joined_cols = cols.clone();
        joined_cols.extend(src_cols.iter().cloned());
        // Terms that may drive this source's access path.
        let mut terms: Vec<&Expr> = Vec::new();
        if let Some(e) = on {
            conjuncts(e, &mut terms);
        }
        // WHERE terms cannot drive a LEFT join's right side: they would drop the
        // null-extended rows the WHERE clause is meant to see.
        let null_extended = any_right || matches!(kind, JoinKind::Left);
        if !null_extended {
            terms.extend(where_terms.iter().copied());
        }
        let width = src.cols.len();
        let mut next_rows = Vec::new();
        let mut matched_right = vec![
            false;
            match &src.data {
                SourceData::Rows(r) => r.len(),
                SourceData::Table(t, _) => t.rows.len(),
            }
        ];
        let right_rows: Vec<Vec<Value>>;
        let right_ids: Vec<i64>;
        let (table, alias) = match &src.data {
            SourceData::Table(t, a) => (Some(t.clone()), a.clone()),
            SourceData::Rows(_) => (None, String::new()),
        };
        match &src.data {
            // Derived tables (views, subqueries, CTEs) are scanned.
            SourceData::Rows(r) => {
                right_rows = r.clone();
                right_ids = vec![];
            }
            SourceData::Table(t, _) => {
                right_ids = t.rows.keys().copied().collect();
                right_rows = vec![];
            }
        }
        let mut noted = false;
        for left_row in &rows {
            let left_scope = Scope {
                cols: &cols,
                row: left_row,
                parent: outer,
                aggs: None,
            };
            // Candidate right rows, through the planner for base tables.
            let candidates: Vec<(usize, Vec<Value>)> = match &table {
                Some(t) if !any_right => {
                    let ids = table_candidates(
                        ctx,
                        t,
                        &alias,
                        &terms,
                        if n == 0 { &empty_cols } else { &cols },
                        Some(&left_scope),
                        !noted,
                    )?;
                    noted = true;
                    ids.into_iter()
                        .map(|id| {
                            let pos = right_ids.binary_search(&id).unwrap_or(0);
                            (pos, table_row(t, id, &t.rows[&id]))
                        })
                        .collect()
                }
                Some(t) => {
                    if !noted {
                        ctx.note(format!("SCAN {alias}"));
                        noted = true;
                    }
                    right_ids
                        .iter()
                        .enumerate()
                        .map(|(i, id)| (i, table_row(t, *id, &t.rows[id])))
                        .collect()
                }
                None => right_rows.iter().cloned().enumerate().collect(),
            };
            let mut any = false;
            for (pos, right) in candidates {
                let mut combined = left_row.clone();
                combined.extend(right);
                let scope = Scope {
                    cols: &joined_cols,
                    row: &combined,
                    parent: outer,
                    aggs: None,
                };
                let mut ok = using_pairs.iter().all(|(l, r)| {
                    let (a, b) = (&combined[*l], &combined[cols.len() + *r]);
                    !a.is_null() && !b.is_null() && {
                        let (x, y) = coerce_pair(
                            a.clone(),
                            cols[*l].affinity,
                            b.clone(),
                            src_cols[*r].affinity,
                        );
                        compare(&x, &y, cols[*l].collation) == Ordering::Equal
                    }
                });
                if ok {
                    if let Some(e) = on {
                        ok = eval(ctx, Some(&scope), e)?.truth() == Some(true);
                    }
                }
                if ok {
                    any = true;
                    if let Some(m) = matched_right.get_mut(pos) {
                        *m = true;
                    }
                    next_rows.push(combined);
                }
            }
            if !any && matches!(kind, JoinKind::Left | JoinKind::Full) {
                let mut combined = left_row.clone();
                combined.extend(std::iter::repeat_n(Value::Null, width));
                next_rows.push(combined);
            }
        }
        if matches!(kind, JoinKind::Right | JoinKind::Full) {
            let all: Vec<Vec<Value>> = match &table {
                Some(t) => right_ids
                    .iter()
                    .map(|id| table_row(t, *id, &t.rows[id]))
                    .collect(),
                None => right_rows.clone(),
            };
            for (i, r) in all.into_iter().enumerate() {
                if !matched_right[i] {
                    let mut combined = vec![Value::Null; cols.len()];
                    combined.extend(r);
                    next_rows.push(combined);
                }
            }
        }
        cols = joined_cols;
        rows = next_rows;
    }
    Ok(Relation { cols, rows })
}
