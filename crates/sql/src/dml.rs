//! INSERT, UPDATE, DELETE with constraints and foreign keys, and the schema statements.
use crate::ast::*;
use crate::eval::{eval, ColMeta, Ctx, Env, Relation, Scope};
use crate::exec::{from_relation, table_candidates, table_cols, table_row};
use crate::parser::parse_expr;
use crate::schema::{build_table, Index, IndexColumn, IndexOrigin, State, Table, View};
use crate::value::{compare, Affinity, Collation, Value};
use crate::{Output, SqlError};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

/// Cascades deeper than this are a cycle, not a schema.
const CASCADE_LIMIT: usize = 1000;

pub struct Run<'a> {
    pub params: &'a [Value],
    pub env: &'a Env,
    pub ctes: Rc<Vec<(String, Rc<Relation>)>>,
}
impl Run<'_> {
    pub fn ctx<'s>(&'s self, state: &'s State) -> Ctx<'s> {
        Ctx {
            state,
            params: self.params,
            env: self.env,
            ctes: self.ctes.clone(),
            plan: None,
            depth: 0,
        }
    }
}
fn constraint(msg: impl Into<String>) -> SqlError {
    SqlError::new(msg).with_code(19)
}
fn guard_writable(state: &State, name: &str) -> Result<String, SqlError> {
    let key = name.to_ascii_lowercase();
    if state.views.contains_key(&key) {
        return Err(SqlError::new(format!(
            "cannot modify {name} because it is a view"
        )));
    }
    if matches!(key.as_str(), "sqlite_schema" | "sqlite_master") {
        return Err(SqlError::new(format!("table {name} may not be modified")));
    }
    state.table(name)?;
    Ok(key)
}
fn parsed_defaults(table: &Table) -> Result<Vec<Option<Expr>>, SqlError> {
    table
        .columns
        .iter()
        .map(|c| c.default.as_deref().map(parse_expr).transpose())
        .collect()
}
fn parsed_checks(table: &Table) -> Result<Vec<(String, Expr)>, SqlError> {
    table
        .checks
        .iter()
        .map(|t| Ok((t.clone(), parse_expr(t)?)))
        .collect()
}
fn eval_default(ctx: &Ctx, e: &Option<Expr>) -> Result<Value, SqlError> {
    match e {
        Some(e) => eval(ctx, None, e),
        None => Ok(Value::Null),
    }
}

/// Target of one value in an INSERT column list or an UPDATE assignment.
#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Column(usize),
    Rowid,
}
fn slot(table: &Table, name: &str) -> Result<Slot, SqlError> {
    if let Some(i) = table.column(name) {
        return Ok(Slot::Column(i));
    }
    if table.is_rowid_name(name) {
        return Ok(Slot::Rowid);
    }
    Err(SqlError::new(format!(
        "table {} has no column named {name}",
        table.name
    )))
}
fn as_rowid(v: &Value) -> Result<i64, SqlError> {
    match Affinity::Integer.apply(v.clone()) {
        Value::Integer(i) => Ok(i),
        _ => Err(SqlError::new("datatype mismatch").with_code(20)),
    }
}
fn next_rowid(state: &State, table: &Table) -> Result<i64, SqlError> {
    let max = table.rows.keys().next_back().copied().unwrap_or(0);
    let seq = if table.autoincrement {
        sequence(state, &table.name)
    } else {
        0
    };
    max.max(seq)
        .checked_add(1)
        .ok_or_else(|| SqlError::new("database or disk is full").with_code(13))
}
fn sequence(state: &State, table: &str) -> i64 {
    state
        .tables
        .get("sqlite_sequence")
        .and_then(|s| {
            s.rows
                .values()
                .find(|r| {
                    r.first()
                        .is_some_and(|n| n.to_text().eq_ignore_ascii_case(table))
                })
                .and_then(|r| r.get(1).and_then(Value::to_i64))
        })
        .unwrap_or(0)
}
fn bump_sequence(state: &mut State, table: &str, rowid: i64) {
    let Some(seq) = state.tables.get_mut("sqlite_sequence") else {
        return;
    };
    let seq = Arc::make_mut(seq);
    let found = seq
        .rows
        .iter()
        .find(|(_, r)| {
            r.first()
                .is_some_and(|n| n.to_text().eq_ignore_ascii_case(table))
        })
        .map(|(id, r)| (*id, r.get(1).and_then(Value::to_i64).unwrap_or(0)));
    match found {
        Some((id, cur)) => {
            if rowid > cur {
                seq.rows
                    .insert(id, vec![Value::Text(table.into()), Value::Integer(rowid)]);
            }
        }
        None => {
            let id = seq.rows.keys().next_back().copied().unwrap_or(0) + 1;
            seq.rows
                .insert(id, vec![Value::Text(table.into()), Value::Integer(rowid)]);
        }
    }
}

// ----- low-level row mutation with index maintenance -----

fn stored(table: &Table, row: &[Value]) -> Vec<Value> {
    let mut s = row[..table.columns.len()].to_vec();
    if let Some(i) = table.ipk {
        s[i] = Value::Null;
    }
    s
}
fn raw_insert(state: &mut State, key: &str, rowid: i64, row: &[Value]) {
    let table = Arc::make_mut(state.tables.get_mut(key).expect("table exists"));
    let s = stored(table, row);
    table.rows.insert(rowid, s);
    let full = table.full_row(rowid, &table.rows[&rowid]);
    for index in state.indexes.values_mut().filter(|i| i.table == key) {
        let k = index.key_for(&full, rowid);
        Arc::make_mut(index).entries.insert(k);
    }
}
fn raw_remove(state: &mut State, key: &str, rowid: i64) -> Option<Vec<Value>> {
    let table = Arc::make_mut(state.tables.get_mut(key)?);
    let old = table.rows.remove(&rowid)?;
    let full = table.full_row(rowid, &old);
    for index in state.indexes.values_mut().filter(|i| i.table == key) {
        let k = index.key_for(&full, rowid);
        Arc::make_mut(index).entries.remove(&k);
    }
    Some(full)
}

// ----- foreign keys -----

/// The parent table and its key columns (`None` is the rowid) for a foreign key.
fn parent_key(
    state: &State,
    child: &Table,
    fk: &crate::schema::ForeignKey,
) -> Result<(Arc<Table>, Vec<Option<usize>>), SqlError> {
    let mismatch = || {
        SqlError::new(format!(
            "foreign key mismatch - \"{}\" referencing \"{}\"",
            child.name, fk.parent
        ))
    };
    let parent = state
        .tables
        .get(&fk.parent.to_ascii_lowercase())
        .cloned()
        .ok_or_else(|| SqlError::new(format!("no such table: main.{}", fk.parent)))?;
    let cols: Vec<Option<usize>> = if fk.parent_columns.is_empty() {
        if let Some(i) = parent.ipk {
            vec![Some(i)]
        } else if parent.primary_key.is_empty() {
            return Err(mismatch());
        } else {
            parent.primary_key.iter().map(|c| Some(*c)).collect()
        }
    } else {
        fk.parent_columns
            .iter()
            .map(|n| parent.column(n).ok_or_else(mismatch).map(Some))
            .collect::<Result<_, _>>()?
    };
    if cols.len() != fk.columns.len() {
        return Err(mismatch());
    }
    // The parent key must be the primary key or carry a unique index.
    let set: BTreeSet<usize> = cols.iter().flatten().copied().collect();
    let is_ipk = cols.len() == 1 && cols[0] == parent.ipk && parent.ipk.is_some();
    let is_pk = parent.primary_key.iter().copied().collect::<BTreeSet<_>>() == set;
    let has_unique = state
        .indexes_of(&parent.name)
        .iter()
        .any(|i| i.unique && i.columns.iter().map(|c| c.column).collect::<BTreeSet<_>>() == set);
    if !(is_ipk || is_pk || has_unique) {
        return Err(mismatch());
    }
    let cols = cols
        .into_iter()
        .map(|c| if c == parent.ipk { None } else { c })
        .collect();
    Ok((parent, cols))
}
fn key_values(rowid: i64, row: &[Value], cols: &[Option<usize>]) -> Vec<Value> {
    cols.iter()
        .map(|c| match c {
            Some(i) => row[*i].clone(),
            None => Value::Integer(rowid),
        })
        .collect()
}
fn key_matches(
    parent: &Table,
    cols: &[Option<usize>],
    prow: &[Value],
    pid: i64,
    vals: &[Value],
) -> bool {
    cols.iter().zip(vals).all(|(c, v)| {
        let (pv, aff, coll) = match c {
            Some(i) => (
                prow[*i].clone(),
                parent.columns[*i].affinity,
                parent.columns[*i].collation,
            ),
            None => (Value::Integer(pid), Affinity::Integer, Collation::Binary),
        };
        compare(&pv, &aff.apply(v.clone()), coll) == Ordering::Equal
    })
}
fn parent_exists(parent: &Table, cols: &[Option<usize>], vals: &[Value]) -> bool {
    if cols.len() == 1 && cols[0].is_none() {
        return as_rowid(&vals[0])
            .ok()
            .is_some_and(|id| parent.rows.contains_key(&id));
    }
    parent
        .rows
        .iter()
        .any(|(id, s)| key_matches(parent, cols, &parent.full_row(*id, s), *id, vals))
}
/// Check a child row's foreign keys; `changed` limits the check to keys it touched.
fn check_child(
    state: &State,
    table: &Table,
    row: &[Value],
    changed: Option<&[bool]>,
) -> Result<(), SqlError> {
    if !state.foreign_keys {
        return Ok(());
    }
    for fk in &table.foreign_keys {
        if let Some(ch) = changed {
            if !fk.columns.iter().any(|c| ch[*c]) {
                continue;
            }
        }
        let vals: Vec<Value> = fk.columns.iter().map(|c| row[*c].clone()).collect();
        if vals.iter().any(Value::is_null) {
            continue;
        }
        let (parent, cols) = parent_key(state, table, fk)?;
        // A row may reference itself, and the new row is not stored yet.
        let self_ref = parent.name.eq_ignore_ascii_case(&table.name)
            && cols.iter().zip(&vals).all(|(c, v)| match c {
                Some(i) => compare(&row[*i], v, Collation::Binary) == Ordering::Equal,
                None => false,
            });
        if !self_ref && !parent_exists(&parent, &cols, &vals) {
            return Err(constraint("FOREIGN KEY constraint failed"));
        }
    }
    Ok(())
}
/// Rows of other tables that reference the given parent key values.
fn children_of(
    state: &State,
    parent_key_name: &str,
    old_rowid: i64,
    old_row: &[Value],
) -> Result<Vec<(String, usize, Vec<i64>)>, SqlError> {
    let mut out = Vec::new();
    for (ckey, child) in &state.tables {
        for (fi, fk) in child.foreign_keys.iter().enumerate() {
            if !fk.parent.eq_ignore_ascii_case(parent_key_name) {
                continue;
            }
            let (parent, cols) = parent_key(state, child, fk)?;
            let vals = key_values(old_rowid, old_row, &cols);
            if vals.iter().any(Value::is_null) {
                continue;
            }
            let ids: Vec<i64> = child
                .rows
                .iter()
                .filter(|(id, s)| {
                    let full = child.full_row(**id, s);
                    fk.columns.iter().zip(&vals).zip(&cols).all(|((c, v), pc)| {
                        let cv = &full[*c];
                        // The child value is compared under the parent column's rules.
                        let (aff, coll) = match pc {
                            Some(i) => (parent.columns[*i].affinity, parent.columns[*i].collation),
                            None => (Affinity::Integer, Collation::Binary),
                        };
                        !cv.is_null() && compare(&aff.apply(cv.clone()), v, coll) == Ordering::Equal
                    })
                })
                .map(|(id, _)| *id)
                .collect();
            if !ids.is_empty() {
                out.push((ckey.clone(), fi, ids));
            }
        }
    }
    Ok(out)
}
fn set_columns(
    state: &mut State,
    key: &str,
    rowid: i64,
    cols: &[usize],
    vals: &[Value],
) -> Result<(), SqlError> {
    let table = state.tables[key].clone();
    let Some(s) = table.rows.get(&rowid) else {
        return Ok(());
    };
    let mut row = table.full_row(rowid, s);
    for (c, v) in cols.iter().zip(vals) {
        let v = table.columns[*c].affinity.apply(v.clone());
        if v.is_null() && table.columns[*c].not_null {
            return Err(constraint(format!(
                "NOT NULL constraint failed: {}.{}",
                table.name, table.columns[*c].name
            )));
        }
        row[*c] = v;
    }
    raw_remove(state, key, rowid);
    let new_id = match table.ipk {
        Some(i) => as_rowid(&row[i])?,
        None => rowid,
    };
    raw_insert(state, key, new_id, &row);
    Ok(())
}
/// Apply ON DELETE / ON UPDATE actions for a parent row that is going away or changing.
fn parent_actions(
    state: &mut State,
    env: &Env,
    key: &str,
    rowid: i64,
    old: &[Value],
    new: Option<(i64, &[Value])>,
    depth: usize,
) -> Result<(), SqlError> {
    if !state.foreign_keys {
        return Ok(());
    }
    if depth > CASCADE_LIMIT {
        return Err(SqlError::new("too many levels of foreign key cascade"));
    }
    let name = state.tables[key].name.clone();
    for (ckey, fi, ids) in children_of(state, &name, rowid, old)? {
        let child = state.tables[&ckey].clone();
        let fk = child.foreign_keys[fi].clone();
        let (_, cols) = parent_key(state, &child, &fk)?;
        let old_vals = key_values(rowid, old, &cols);
        let action = match new {
            None => fk.on_delete,
            Some((nid, nrow)) => {
                let new_vals = key_values(nid, nrow, &cols);
                if old_vals
                    .iter()
                    .zip(&new_vals)
                    .all(|(a, b)| compare(a, b, Collation::Binary) == Ordering::Equal)
                {
                    continue;
                }
                fk.on_update
            }
        };
        for id in ids {
            // A row that references itself is handled by the operation on it.
            if ckey == key && id == rowid {
                continue;
            }
            match action {
                FkAction::NoAction | FkAction::Restrict => {
                    return Err(constraint("FOREIGN KEY constraint failed"))
                }
                FkAction::Cascade => match new {
                    None => delete_row(state, env, &ckey, id, depth + 1)?,
                    Some((nid, nrow)) => {
                        let vals = key_values(nid, nrow, &cols);
                        let before = state.tables[&ckey].clone();
                        let old_child = before.full_row(id, &before.rows[&id]);
                        set_columns(state, &ckey, id, &fk.columns, &vals)?;
                        let after = state.tables[&ckey].clone();
                        let nid2 = match after.ipk {
                            Some(i) => as_rowid(
                                &after.full_row(id, after.rows.get(&id).unwrap_or(&vec![]))[i],
                            )
                            .unwrap_or(id),
                            None => id,
                        };
                        if let Some(s) = after.rows.get(&nid2) {
                            let new_child = after.full_row(nid2, s);
                            parent_actions(
                                state,
                                env,
                                &ckey,
                                id,
                                &old_child,
                                Some((nid2, &new_child)),
                                depth + 1,
                            )?;
                        }
                    }
                },
                FkAction::SetNull => {
                    let nulls = vec![Value::Null; fk.columns.len()];
                    set_columns(state, &ckey, id, &fk.columns, &nulls)?;
                }
                FkAction::SetDefault => {
                    let ctx = Ctx {
                        state,
                        params: &[],
                        env,
                        ctes: Rc::new(vec![]),
                        plan: None,
                        depth: 0,
                    };
                    let defaults = parsed_defaults(&child)?;
                    let vals: Vec<Value> = fk
                        .columns
                        .iter()
                        .map(|c| eval_default(&ctx, &defaults[*c]))
                        .collect::<Result<_, _>>()?;
                    set_columns(state, &ckey, id, &fk.columns, &vals)?;
                    let after = state.tables[&ckey].clone();
                    if let Some(s) = after.rows.get(&id) {
                        check_child(state, &after, &after.full_row(id, s), None)?;
                    }
                }
            }
        }
    }
    Ok(())
}
pub fn delete_row(
    state: &mut State,
    env: &Env,
    key: &str,
    rowid: i64,
    depth: usize,
) -> Result<(), SqlError> {
    let table = state.tables[key].clone();
    let Some(s) = table.rows.get(&rowid) else {
        return Ok(());
    };
    let old = table.full_row(rowid, s);
    parent_actions(state, env, key, rowid, &old, None, depth)?;
    raw_remove(state, key, rowid);
    Ok(())
}

enum Written {
    Yes,
    Skipped,
}
/// Store a row, enforcing NOT NULL, CHECK, UNIQUE and foreign keys under a conflict
/// policy. `old` is the rowid being replaced by an UPDATE.
#[allow(clippy::too_many_arguments)]
fn write_row(
    state: &mut State,
    run: &Run,
    key: &str,
    old: Option<i64>,
    mut rowid: i64,
    mut row: Vec<Value>,
    conflict: Conflict,
    checks: &[(String, Expr)],
    defaults: &[Option<Expr>],
) -> Result<Written, SqlError> {
    let table = state.tables[key].clone();
    for (i, col) in table.columns.iter().enumerate() {
        if Some(i) == table.ipk || !col.not_null || !row[i].is_null() {
            continue;
        }
        match conflict {
            Conflict::Ignore => return Ok(Written::Skipped),
            Conflict::Replace if defaults[i].is_some() => {
                let v = eval_default(&run.ctx(state), &defaults[i])?;
                row[i] = col.affinity.apply(v);
                if !row[i].is_null() {
                    continue;
                }
                return Err(constraint(format!(
                    "NOT NULL constraint failed: {}.{}",
                    table.name, col.name
                )));
            }
            _ => {
                return Err(constraint(format!(
                    "NOT NULL constraint failed: {}.{}",
                    table.name, col.name
                )))
            }
        }
    }
    if let Some(i) = table.ipk {
        row[i] = Value::Integer(rowid);
    }
    {
        let ctx = run.ctx(state);
        let cols = table_cols(&table, &table.name);
        let mut full = row.clone();
        full.push(Value::Integer(rowid));
        let scope = Scope {
            cols: &cols,
            row: &full,
            parent: None,
            aggs: None,
        };
        for (text, e) in checks {
            if eval(&ctx, Some(&scope), e)?.truth() == Some(false) {
                if conflict == Conflict::Ignore {
                    return Ok(Written::Skipped);
                }
                return Err(constraint(format!("CHECK constraint failed: {text}")));
            }
        }
    }
    // Uniqueness: the rowid, then every unique index.
    let mut conflicts: Vec<(i64, String)> = Vec::new();
    if old != Some(rowid) && table.rows.contains_key(&rowid) {
        let col = table
            .ipk
            .map_or("rowid".to_string(), |i| table.columns[i].name.clone());
        conflicts.push((rowid, format!("{}.{col}", table.name)));
    }
    for index in state.indexes_of(&table.name) {
        let mut probe = row.clone();
        probe.push(Value::Integer(rowid));
        if let Some(other) = index.conflict(&probe, old.unwrap_or(rowid)) {
            if Some(other) == old {
                continue;
            }
            let names: Vec<String> = index
                .columns
                .iter()
                .map(|c| format!("{}.{}", table.name, table.columns[c.column].name))
                .collect();
            let what = names.join(", ");
            conflicts.push((other, what));
        }
    }
    if let Some((_, what)) = conflicts.first() {
        match conflict {
            Conflict::Ignore => return Ok(Written::Skipped),
            Conflict::Replace => {
                let ids: BTreeSet<i64> = conflicts.iter().map(|(id, _)| *id).collect();
                for id in ids {
                    if Some(id) != old {
                        delete_row(state, run.env, key, id, 0)?;
                    }
                }
            }
            _ => return Err(constraint(format!("UNIQUE constraint failed: {what}"))),
        }
    }
    let changed: Option<Vec<bool>> = old.map(|o| {
        let before = table.full_row(o, &table.rows[&o]);
        before
            .iter()
            .zip(&row)
            .map(|(a, b)| {
                compare(a, b, Collation::Binary) != Ordering::Equal
                    || a.type_name() != b.type_name()
            })
            .collect()
    });
    check_child(state, &table, &row, changed.as_deref())?;
    if let Some(o) = old {
        let before = table.full_row(o, &table.rows[&o]);
        parent_actions(state, run.env, key, o, &before, Some((rowid, &row)), 0)?;
        raw_remove(state, key, o);
    }
    if old.is_none() && state.tables[key].rows.contains_key(&rowid) {
        rowid = next_rowid(state, &state.tables[key])?;
    }
    raw_insert(state, key, rowid, &row);
    if table.autoincrement {
        bump_sequence(state, &table.name, rowid);
    }
    run.env.last_insert_rowid.set(if old.is_none() {
        rowid
    } else {
        run.env.last_insert_rowid.get()
    });
    Ok(Written::Yes)
}
fn returning(
    ctx: &Ctx,
    table: &Table,
    cols_spec: &[ResultCol],
    rowid: i64,
    row: &[Value],
    out: &mut Output,
) -> Result<(), SqlError> {
    if cols_spec.is_empty() {
        return Ok(());
    }
    let cols = table_cols(table, &table.name);
    let mut full = row.to_vec();
    full.push(Value::Integer(rowid));
    let scope = Scope {
        cols: &cols,
        row: &full,
        parent: None,
        aggs: None,
    };
    let mut vals = Vec::new();
    let mut names = Vec::new();
    for rc in cols_spec {
        match rc {
            ResultCol::Star => {
                for (i, c) in table.columns.iter().enumerate() {
                    names.push(c.name.clone());
                    vals.push(full[i].clone());
                }
            }
            ResultCol::TableStar(_) => {
                return Err(SqlError::new("RETURNING may not use \"TABLE.*\" wildcards"))
            }
            ResultCol::Expr { expr, alias, text } => {
                names.push(alias.clone().unwrap_or_else(|| match expr {
                    Expr::Column { name, .. } => name.clone(),
                    _ => text.clone(),
                }));
                vals.push(eval(ctx, Some(&scope), expr)?);
            }
        }
    }
    out.columns = names;
    out.rows.push(vals);
    Ok(())
}

pub fn insert(state: &mut State, run: &Run, ins: &Insert) -> Result<Output, SqlError> {
    let key = guard_writable(state, &ins.table)?;
    let table = state.tables[&key].clone();
    let slots: Vec<Slot> = if ins.columns.is_empty() {
        (0..table.columns.len()).map(Slot::Column).collect()
    } else {
        ins.columns
            .iter()
            .map(|c| slot(&table, c))
            .collect::<Result<_, _>>()?
    };
    let source: Vec<Vec<Value>> = {
        let ctx = run.ctx(state);
        match &ins.source {
            InsertSource::Default => vec![vec![]],
            InsertSource::Values(rows) => {
                let mut out = Vec::new();
                for r in rows {
                    out.push(
                        r.iter()
                            .map(|e| eval(&ctx, None, e))
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                }
                out
            }
            InsertSource::Select(s) => crate::exec::select(&ctx, s, None)?.rows,
        }
    };
    if !matches!(ins.source, InsertSource::Default) {
        if let Some(first) = source.first() {
            if first.len() != slots.len() {
                return Err(if ins.columns.is_empty() {
                    SqlError::new(format!(
                        "table {} has {} columns but {} values were supplied",
                        table.name,
                        slots.len(),
                        first.len()
                    ))
                } else {
                    SqlError::new(format!(
                        "{} values for {} columns",
                        first.len(),
                        slots.len()
                    ))
                });
            }
        }
    }
    let defaults = parsed_defaults(&table)?;
    let checks = parsed_checks(&table)?;
    let mut out = Output::default();
    for src in source {
        let mut row = vec![Value::Null; table.columns.len()];
        let mut given = vec![false; table.columns.len()];
        let mut explicit_rowid = None;
        for (s, v) in slots.iter().zip(src) {
            match s {
                Slot::Column(i) => {
                    row[*i] = v;
                    given[*i] = true;
                }
                Slot::Rowid => explicit_rowid = Some(v),
            }
        }
        {
            let ctx = run.ctx(state);
            for i in 0..row.len() {
                if !given[i] {
                    row[i] = eval_default(&ctx, &defaults[i])?;
                }
            }
        }
        for (i, c) in table.columns.iter().enumerate() {
            row[i] = c.affinity.apply(std::mem::take(&mut row[i]));
        }
        let rowid_value = match table.ipk {
            Some(i) if !row[i].is_null() => Some(row[i].clone()),
            _ => explicit_rowid.filter(|v| !v.is_null()),
        };
        let current = state.tables[&key].clone();
        let rowid = match rowid_value {
            Some(v) => as_rowid(&v)?,
            None => next_rowid(state, &current)?,
        };
        // Upsert: a conflict on the target becomes an update of the existing row.
        if let Some(up) = &ins.upsert {
            let mut probe = row.clone();
            if let Some(i) = current.ipk {
                probe[i] = Value::Integer(rowid);
            }
            probe.push(Value::Integer(rowid));
            let mut hit = None;
            if current.rows.contains_key(&rowid)
                && (up.target.is_empty()
                    || up.target.len() == 1
                        && current.ipk.is_some_and(|i| {
                            current.columns[i].name.eq_ignore_ascii_case(&up.target[0])
                        }))
            {
                hit = Some(rowid);
            }
            if hit.is_none() {
                for index in state.indexes_of(&current.name) {
                    let names: BTreeSet<String> = index
                        .columns
                        .iter()
                        .map(|c| current.columns[c.column].name.to_ascii_lowercase())
                        .collect();
                    let target: BTreeSet<String> =
                        up.target.iter().map(|t| t.to_ascii_lowercase()).collect();
                    if !up.target.is_empty() && names != target {
                        continue;
                    }
                    if let Some(other) = index.conflict(&probe, i64::MIN) {
                        hit = Some(other);
                        break;
                    }
                }
            }
            if let Some(existing) = hit {
                match &up.update {
                    None => continue,
                    Some((sets, filter)) => {
                        upsert_update(
                            state,
                            run,
                            &key,
                            existing,
                            &row,
                            sets,
                            filter.as_ref(),
                            &checks,
                            &defaults,
                            &ins.returning,
                            &mut out,
                        )?;
                        continue;
                    }
                }
            }
        }
        match write_row(
            state,
            run,
            &key,
            None,
            rowid,
            row.clone(),
            ins.conflict,
            &checks,
            &defaults,
        )? {
            Written::Skipped => {}
            Written::Yes => {
                out.changes += 1;
                let last = run.env.last_insert_rowid.get();
                let t = state.tables[&key].clone();
                let ctx = run.ctx(state);
                returning(
                    &ctx,
                    &t,
                    &ins.returning,
                    last,
                    &t.full_row(last, &t.rows[&last]),
                    &mut out,
                )?;
            }
        }
    }
    Ok(out)
}
#[allow(clippy::too_many_arguments)]
fn upsert_update(
    state: &mut State,
    run: &Run,
    key: &str,
    rowid: i64,
    proposed: &[Value],
    sets: &[(Vec<String>, Expr)],
    filter: Option<&Expr>,
    checks: &[(String, Expr)],
    defaults: &[Option<Expr>],
    ret: &[ResultCol],
    out: &mut Output,
) -> Result<(), SqlError> {
    let table = state.tables[key].clone();
    let old = table_row(&table, rowid, &table.rows[&rowid]);
    let mut cols = table_cols(&table, &table.name);
    // `excluded.x` is the proposed row; a bare `x` is the row already stored.
    let mut excluded = table_cols(&table, "excluded");
    excluded.pop();
    for c in &mut excluded {
        c.merged = true;
    }
    cols.extend(excluded);
    let mut scope_row = old.clone();
    scope_row.extend(proposed.iter().cloned());
    let (new_row, new_id) = {
        let ctx = run.ctx(state);
        let scope = Scope {
            cols: &cols,
            row: &scope_row,
            parent: None,
            aggs: None,
        };
        if let Some(f) = filter {
            if eval(&ctx, Some(&scope), f)?.truth() != Some(true) {
                return Ok(());
            }
        }
        assign(&ctx, &table, &scope, sets, &old, rowid)?
    };
    if let Written::Yes = write_row(
        state,
        run,
        key,
        Some(rowid),
        new_id,
        new_row,
        Conflict::Abort,
        checks,
        defaults,
    )? {
        out.changes += 1;
        let t = state.tables[key].clone();
        let ctx = run.ctx(state);
        returning(
            &ctx,
            &t,
            ret,
            new_id,
            &t.full_row(new_id, &t.rows[&new_id]),
            out,
        )?;
    }
    Ok(())
}
/// Evaluate SET assignments against the old row; returns the new row and rowid.
fn assign(
    ctx: &Ctx,
    table: &Table,
    scope: &Scope,
    sets: &[(Vec<String>, Expr)],
    old: &[Value],
    rowid: i64,
) -> Result<(Vec<Value>, i64), SqlError> {
    let mut row = old[..table.columns.len()].to_vec();
    let mut new_id = rowid;
    let mut rowid_set = None;
    for (targets, e) in sets {
        let values: Vec<Value> = if targets.len() == 1 {
            vec![eval(ctx, Some(scope), e)?]
        } else {
            match e {
                Expr::Row(items) if items.len() == targets.len() => items
                    .iter()
                    .map(|i| eval(ctx, Some(scope), i))
                    .collect::<Result<_, _>>()?,
                Expr::Subquery(s) => {
                    let rel = crate::exec::select(ctx, s, Some(scope))?;
                    if rel.cols.len() != targets.len() {
                        return Err(SqlError::new(format!(
                            "{} columns assigned {} values",
                            targets.len(),
                            rel.cols.len()
                        )));
                    }
                    rel.rows
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| vec![Value::Null; targets.len()])
                }
                _ => {
                    return Err(SqlError::new(format!(
                        "{} columns assigned 1 values",
                        targets.len()
                    )))
                }
            }
        };
        for (t, v) in targets.iter().zip(values) {
            match slot(table, t).map_err(|_| SqlError::new(format!("no such column: {t}")))? {
                Slot::Column(i) => {
                    row[i] = table.columns[i].affinity.apply(v);
                    if Some(i) == table.ipk {
                        rowid_set = Some(row[i].clone());
                    }
                }
                Slot::Rowid => rowid_set = Some(v),
            }
        }
    }
    if let Some(v) = rowid_set {
        new_id = as_rowid(&v)?;
    }
    Ok((row, new_id))
}

pub fn update(state: &mut State, run: &Run, up: &Update) -> Result<Output, SqlError> {
    let key = guard_writable(state, &up.table)?;
    let table = state.tables[&key].clone();
    let alias = up.alias.clone().unwrap_or_else(|| table.name.clone());
    for (targets, _) in &up.sets {
        for t in targets {
            slot(&table, t).map_err(|_| SqlError::new(format!("no such column: {t}")))?;
        }
    }
    let checks = parsed_checks(&table)?;
    let defaults = parsed_defaults(&table)?;
    let cols = table_cols(&table, &alias);
    // Collect the targets first, against the table as it was.
    let targets: Vec<(i64, Option<Vec<Value>>)> = {
        let ctx = run.ctx(state);
        let from = match &up.from {
            Some(f) => Some(from_relation(&ctx, f, None, None)?),
            None => None,
        };
        let mut terms = Vec::new();
        if let (Some(w), None) = (&up.filter, &from) {
            let mut stack = vec![w];
            while let Some(e) = stack.pop() {
                if let Expr::Binary(BinOp::And, a, b) = e {
                    stack.push(a);
                    stack.push(b);
                } else {
                    terms.push(e);
                }
            }
        }
        let ids = table_candidates(&ctx, &table, &alias, &terms, &[], None, false)?;
        let mut out = Vec::new();
        for id in ids {
            let row = table_row(&table, id, &table.rows[&id]);
            match &from {
                None => {
                    let scope = Scope {
                        cols: &cols,
                        row: &row,
                        parent: None,
                        aggs: None,
                    };
                    let ok = match &up.filter {
                        Some(f) => eval(&ctx, Some(&scope), f)?.truth() == Some(true),
                        None => true,
                    };
                    if ok {
                        out.push((id, None));
                    }
                }
                Some(rel) => {
                    let mut jcols = cols.clone();
                    jcols.extend(rel.cols.iter().cloned());
                    for fr in &rel.rows {
                        let mut combined = row.clone();
                        combined.extend(fr.iter().cloned());
                        let scope = Scope {
                            cols: &jcols,
                            row: &combined,
                            parent: None,
                            aggs: None,
                        };
                        let ok = match &up.filter {
                            Some(f) => eval(&ctx, Some(&scope), f)?.truth() == Some(true),
                            None => true,
                        };
                        if ok {
                            out.push((id, Some(fr.clone())));
                            break;
                        }
                    }
                }
            }
        }
        out
    };
    let from_cols: Vec<ColMeta> = match &up.from {
        Some(f) => {
            let ctx = run.ctx(state);
            from_relation(&ctx, f, None, None)?.cols
        }
        None => vec![],
    };
    let mut out = Output::default();
    for (id, from_row) in targets {
        let current = state.tables[&key].clone();
        let Some(s) = current.rows.get(&id) else {
            continue;
        };
        let old = table_row(&current, id, s);
        let (row, new_id) = {
            let ctx = run.ctx(state);
            let mut jcols = cols.clone();
            let mut jrow = old.clone();
            if let Some(fr) = &from_row {
                jcols.extend(from_cols.iter().cloned());
                jrow.extend(fr.iter().cloned());
            }
            let scope = Scope {
                cols: &jcols,
                row: &jrow,
                parent: None,
                aggs: None,
            };
            assign(&ctx, &current, &scope, &up.sets, &old, id)?
        };
        match write_row(
            state,
            run,
            &key,
            Some(id),
            new_id,
            row,
            up.conflict,
            &checks,
            &defaults,
        )? {
            Written::Skipped => {}
            Written::Yes => {
                out.changes += 1;
                let t = state.tables[&key].clone();
                if let Some(s) = t.rows.get(&new_id) {
                    let ctx = run.ctx(state);
                    returning(
                        &ctx,
                        &t,
                        &up.returning,
                        new_id,
                        &t.full_row(new_id, s),
                        &mut out,
                    )?;
                }
            }
        }
    }
    Ok(out)
}

pub fn delete(state: &mut State, run: &Run, del: &Delete) -> Result<Output, SqlError> {
    let key = guard_writable(state, &del.table)?;
    let table = state.tables[&key].clone();
    let alias = del.alias.clone().unwrap_or_else(|| table.name.clone());
    let cols = table_cols(&table, &alias);
    let mut out = Output::default();
    let targets: Vec<(i64, Vec<Value>)> = {
        let ctx = run.ctx(state);
        let mut terms = Vec::new();
        if let Some(w) = &del.filter {
            let mut stack = vec![w];
            while let Some(e) = stack.pop() {
                if let Expr::Binary(BinOp::And, a, b) = e {
                    stack.push(a);
                    stack.push(b);
                } else {
                    terms.push(e);
                }
            }
        }
        let ids = table_candidates(&ctx, &table, &alias, &terms, &[], None, false)?;
        let mut v = Vec::new();
        for id in ids {
            let row = table_row(&table, id, &table.rows[&id]);
            let ok = match &del.filter {
                Some(f) => {
                    let scope = Scope {
                        cols: &cols,
                        row: &row,
                        parent: None,
                        aggs: None,
                    };
                    eval(&ctx, Some(&scope), f)?.truth() == Some(true)
                }
                None => true,
            };
            if ok {
                v.push((id, row));
            }
        }
        v
    };
    for (id, row) in targets {
        if !state.tables[&key].rows.contains_key(&id) {
            continue;
        }
        if !del.returning.is_empty() {
            let ctx = run.ctx(state);
            returning(
                &ctx,
                &table,
                &del.returning,
                id,
                &row[..table.columns.len()],
                &mut out,
            )?;
        }
        delete_row(state, run.env, &key, id, 0)?;
        out.changes += 1;
    }
    Ok(out)
}

// ----- schema statements -----

fn quote_ident(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain {
        name.into()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}
pub fn create_table(state: &mut State, run: &Run, ct: &CreateTable) -> Result<Output, SqlError> {
    if let Some(kind) = state.name_taken(&ct.name) {
        if ct.if_not_exists {
            return Ok(Output::default());
        }
        return Err(SqlError::new(format!("{kind} {} already exists", ct.name)));
    }
    if ct.name.to_ascii_lowercase().starts_with("sqlite_") {
        return Err(SqlError::new(format!(
            "object name reserved for internal use: {}",
            ct.name
        )));
    }
    if let Some(s) = &ct.as_select {
        let rel = {
            let ctx = run.ctx(state);
            crate::exec::select(&ctx, s, None)?
        };
        let mut defs = Vec::new();
        let mut used = BTreeSet::new();
        for c in &rel.cols {
            let mut name = c.name.clone();
            let mut n = 1;
            while !used.insert(name.to_ascii_lowercase()) {
                name = format!("{}:{n}", c.name);
                n += 1;
            }
            let t = match c.affinity {
                Some(Affinity::Integer) => "INT",
                Some(Affinity::Real) => "REAL",
                Some(Affinity::Text) => "TEXT",
                Some(Affinity::Numeric) => "NUM",
                _ => "",
            };
            defs.push((name, t));
        }
        let sql = format!(
            "CREATE TABLE {}({})",
            quote_ident(&ct.name),
            defs.iter()
                .map(|(n, t)| if t.is_empty() {
                    quote_ident(n)
                } else {
                    format!("{} {t}", quote_ident(n))
                })
                .collect::<Vec<_>>()
                .join(",")
        );
        let def = CreateTable {
            name: ct.name.clone(),
            if_not_exists: false,
            temporary: ct.temporary,
            columns: defs
                .iter()
                .map(|(n, t)| ColumnDef {
                    name: n.clone(),
                    type_name: (*t).into(),
                    ..ColumnDef::default()
                })
                .collect(),
            constraints: vec![],
            as_select: None,
            without_rowid: false,
            strict: false,
            sql: sql.clone(),
        };
        let (table, _) = build_table(state, &def, sql)?;
        let key = table.name.to_ascii_lowercase();
        state.tables.insert(key.clone(), Arc::new(table));
        for (i, r) in rel.rows.into_iter().enumerate() {
            let t = state.tables[&key].clone();
            let row: Vec<Value> = r
                .into_iter()
                .zip(&t.columns)
                .map(|(v, c)| c.affinity.apply(v))
                .collect();
            raw_insert(state, &key, i as i64 + 1, &row);
        }
        state.touch_schema();
        return Ok(Output::default());
    }
    let (table, indexes) = build_table(state, ct, ct.sql.clone())?;
    let key = table.name.to_ascii_lowercase();
    let needs_sequence = table.autoincrement && !state.tables.contains_key("sqlite_sequence");
    state.tables.insert(key, Arc::new(table));
    for i in indexes {
        state
            .indexes
            .insert(i.name.to_ascii_lowercase(), Arc::new(i));
    }
    if needs_sequence {
        let def = crate::parser::parse("CREATE TABLE sqlite_sequence(name,seq)")?;
        if let Some(Stmt::CreateTable(ct)) = def.into_iter().next() {
            let (t, _) = build_table(state, &ct, "CREATE TABLE sqlite_sequence(name,seq)".into())?;
            state.tables.insert("sqlite_sequence".into(), Arc::new(t));
        }
    }
    state.touch_schema();
    Ok(Output::default())
}
pub fn create_index(state: &mut State, ci: &CreateIndex) -> Result<Output, SqlError> {
    if let Some(kind) = state.name_taken(&ci.name) {
        if ci.if_not_exists {
            return Ok(Output::default());
        }
        return Err(SqlError::new(format!("{kind} {} already exists", ci.name)));
    }
    if ci.filter.is_some() {
        return Err(SqlError::new(
            "partial indexes are not supported by this engine",
        ));
    }
    let key = ci.table.to_ascii_lowercase();
    if state.views.contains_key(&key) {
        return Err(SqlError::new("views may not be indexed"));
    }
    let table = state
        .tables
        .get(&key)
        .cloned()
        .ok_or_else(|| SqlError::new(format!("no such table: main.{}", ci.table)))?;
    let mut columns = Vec::new();
    for c in &ci.columns {
        let i = table
            .column(&c.name)
            .ok_or_else(|| SqlError::new(format!("no such column: {}", c.name)))?;
        let collation = match &c.collation {
            Some(n) => Collation::parse(n)
                .ok_or_else(|| SqlError::new(format!("no such collation sequence: {n}")))?,
            None => table.columns[i].collation,
        };
        columns.push(IndexColumn {
            column: i,
            collation,
            desc: c.desc,
        });
    }
    let ordinal = state.ordinal();
    let mut index = Index {
        name: ci.name.clone(),
        table: key,
        columns,
        unique: ci.unique,
        origin: IndexOrigin::Created,
        sql: Some(ci.sql.clone()),
        ordinal,
        entries: BTreeSet::new(),
    };
    index.rebuild(&table);
    if index.unique {
        for (id, s) in &table.rows {
            let mut full = table.full_row(*id, s);
            full.push(Value::Integer(*id));
            if index.conflict(&full, *id).is_some() {
                let names: Vec<String> = index
                    .columns
                    .iter()
                    .map(|c| format!("{}.{}", table.name, table.columns[c.column].name))
                    .collect();
                return Err(constraint(format!(
                    "UNIQUE constraint failed: {}",
                    names.join(", ")
                )));
            }
        }
    }
    state
        .indexes
        .insert(ci.name.to_ascii_lowercase(), Arc::new(index));
    state.touch_schema();
    Ok(Output::default())
}
pub fn create_view(
    state: &mut State,
    run: &Run,
    cv: &CreateView,
    select_text: String,
) -> Result<Output, SqlError> {
    if let Some(kind) = state.name_taken(&cv.name) {
        if cv.if_not_exists {
            return Ok(Output::default());
        }
        return Err(SqlError::new(format!("{kind} {} already exists", cv.name)));
    }
    // Resolve it once so a view over a missing table fails when it is created.
    {
        let ctx = run.ctx(state);
        let rel = crate::exec::select(&ctx, &cv.select, None)?;
        if !cv.columns.is_empty() && cv.columns.len() != rel.cols.len() {
            return Err(SqlError::new(format!(
                "expected {} columns for '{}' but got {}",
                cv.columns.len(),
                cv.name,
                rel.cols.len()
            )));
        }
    }
    let ordinal = state.ordinal();
    state.views.insert(
        cv.name.to_ascii_lowercase(),
        View {
            name: cv.name.clone(),
            sql: cv.sql.clone(),
            select: select_text,
            columns: cv.columns.clone(),
            ordinal,
        },
    );
    state.touch_schema();
    Ok(Output::default())
}
pub fn drop(
    state: &mut State,
    run: &Run,
    kind: ObjectKind,
    name: &str,
    if_exists: bool,
) -> Result<Output, SqlError> {
    let key = name.to_ascii_lowercase();
    let missing = |what: &str| {
        if if_exists {
            Ok(Output::default())
        } else {
            Err(SqlError::new(format!("no such {what}: {name}")))
        }
    };
    match kind {
        ObjectKind::Table => {
            if state.views.contains_key(&key) {
                return Err(SqlError::new(format!(
                    "use DROP VIEW to delete view {name}"
                )));
            }
            let Some(table) = state.tables.get(&key).cloned() else {
                return missing("table");
            };
            if key == "sqlite_sequence"
                || key.starts_with("sqlite_schema")
                || key == "sqlite_master"
            {
                return Err(SqlError::new(format!("table {name} may not be dropped")));
            }
            // Dropping a parent deletes its rows first, which foreign keys may refuse.
            if state.foreign_keys {
                let ids: Vec<i64> = table.rows.keys().copied().collect();
                for id in ids {
                    delete_row(state, run.env, &key, id, 0)?;
                }
            }
            state.tables.remove(&key);
            state.indexes.retain(|_, i| i.table != key);
            if let Some(seq) = state.tables.get_mut("sqlite_sequence") {
                Arc::make_mut(seq).rows.retain(|_, r| {
                    !r.first()
                        .is_some_and(|n| n.to_text().eq_ignore_ascii_case(&table.name))
                });
            }
        }
        ObjectKind::Index => {
            let Some(index) = state.indexes.get(&key) else {
                return missing("index");
            };
            if index.origin != IndexOrigin::Created {
                return Err(SqlError::new(
                    "index associated with UNIQUE or PRIMARY KEY constraint cannot be dropped",
                ));
            }
            state.indexes.remove(&key);
        }
        ObjectKind::View => {
            if state.tables.contains_key(&key) {
                return Err(SqlError::new(format!(
                    "use DROP TABLE to delete table {name}"
                )));
            }
            if state.views.remove(&key).is_none() {
                return missing("view");
            }
        }
        ObjectKind::Trigger => return missing("trigger"),
    }
    state.touch_schema();
    Ok(Output::default())
}

/// Replace identifier tokens equal to `from` in schema text with `to`.
fn rename_identifier(sql: &str, from: &str, to: &str, only_after: Option<&str>) -> String {
    let Ok(toks) = crate::lexer::tokenize(sql) else {
        return sql.to_owned();
    };
    let mut out = String::new();
    let mut last = 0;
    let mut prev_kw: Option<String> = None;
    for t in &toks {
        if let crate::lexer::Tok::Ident { name, .. } = &t.tok {
            let allowed = only_after.is_none_or(|kw| prev_kw.as_deref() == Some(kw));
            if name.eq_ignore_ascii_case(from) && allowed {
                out.push_str(&sql[last..t.start]);
                out.push_str(&quote_ident(to));
                last = t.end;
            }
        }
        prev_kw = t.keyword();
    }
    out.push_str(&sql[last..]);
    out
}
pub fn alter(state: &mut State, run: &Run, at: &AlterTable) -> Result<Output, SqlError> {
    match at {
        AlterTable::Rename { table, to } => {
            let key = guard_writable(state, table)?;
            if let Some(kind) = state.name_taken(to) {
                return Err(SqlError::new(format!(
                    "there is already another {kind} with this name: {to}"
                )));
            }
            let mut t = (*state.tables.remove(&key).expect("checked")).clone();
            let old_name = t.name.clone();
            t.sql = rename_first_name(&t.sql, to);
            t.name = to.clone();
            let new_key = to.to_ascii_lowercase();
            state.tables.insert(new_key.clone(), Arc::new(t));
            let names: Vec<String> = state.indexes.keys().cloned().collect();
            for n in names {
                if state.indexes[&n].table != key {
                    continue;
                }
                let mut i = (*state.indexes.remove(&n).expect("listed")).clone();
                i.table = new_key.clone();
                if i.origin != IndexOrigin::Created {
                    let suffix = i.name.rsplit('_').next().unwrap_or("1").to_owned();
                    i.name = format!("sqlite_autoindex_{to}_{suffix}");
                } else if let Some(sql) = &i.sql {
                    i.sql = Some(rename_identifier(sql, &old_name, to, Some("ON")));
                }
                state
                    .indexes
                    .insert(i.name.to_ascii_lowercase(), Arc::new(i));
            }
            for other in state.tables.values_mut() {
                if other
                    .foreign_keys
                    .iter()
                    .any(|f| f.parent.eq_ignore_ascii_case(&old_name))
                {
                    let o = Arc::make_mut(other);
                    for f in &mut o.foreign_keys {
                        if f.parent.eq_ignore_ascii_case(&old_name) {
                            f.parent = to.clone();
                        }
                    }
                    o.sql = rename_identifier(&o.sql, &old_name, to, Some("REFERENCES"));
                }
            }
            for v in state.views.values_mut() {
                v.sql = rename_identifier(&v.sql, &old_name, to, None);
                v.select = rename_identifier(&v.select, &old_name, to, None);
            }
            if let Some(seq) = state.tables.get_mut("sqlite_sequence") {
                for r in Arc::make_mut(seq).rows.values_mut() {
                    if r.first()
                        .is_some_and(|n| n.to_text().eq_ignore_ascii_case(&old_name))
                    {
                        r[0] = Value::Text(to.clone());
                    }
                }
            }
        }
        AlterTable::RenameColumn { table, from, to } => {
            let key = guard_writable(state, table)?;
            let t = state.tables[&key].clone();
            let i = t
                .column(from)
                .ok_or_else(|| SqlError::new(format!("no such column: \"{from}\"")))?;
            if t.column(to).is_some() {
                return Err(SqlError::new(format!("duplicate column name: {to}")));
            }
            let t = Arc::make_mut(state.tables.get_mut(&key).expect("checked"));
            t.columns[i].name = to.clone();
            t.sql = rename_identifier(&t.sql, from, to, None);
            t.checks = t
                .checks
                .iter()
                .map(|c| rename_identifier(c, from, to, None))
                .collect();
            let tname = t.name.clone();
            for idx in state.indexes.values_mut() {
                if idx.table == key {
                    if let Some(sql) = &idx.sql {
                        let s = rename_identifier(sql, from, to, None);
                        Arc::make_mut(idx).sql = Some(s);
                    }
                }
            }
            for other in state.tables.values_mut() {
                if other.foreign_keys.iter().any(|f| {
                    f.parent.eq_ignore_ascii_case(&tname)
                        && f.parent_columns
                            .iter()
                            .any(|c| c.eq_ignore_ascii_case(from))
                }) {
                    let o = Arc::make_mut(other);
                    for f in &mut o.foreign_keys {
                        if f.parent.eq_ignore_ascii_case(&tname) {
                            for c in &mut f.parent_columns {
                                if c.eq_ignore_ascii_case(from) {
                                    *c = to.clone();
                                }
                            }
                        }
                    }
                }
            }
        }
        AlterTable::AddColumn {
            table,
            column,
            text,
        } => {
            let key = guard_writable(state, table)?;
            let t = state.tables[&key].clone();
            if t.column(&column.name).is_some() {
                return Err(SqlError::new(format!(
                    "duplicate column name: {}",
                    column.name
                )));
            }
            if column.primary_key.is_some() {
                return Err(SqlError::new("Cannot add a PRIMARY KEY column"));
            }
            if column.unique {
                return Err(SqlError::new("Cannot add a UNIQUE column"));
            }
            let default = match &column.default {
                Some(d) => {
                    let e = parse_expr(d)?;
                    let mut constant = true;
                    e.walk(&mut |x| {
                        if matches!(x, Expr::Column { .. } | Expr::Subquery(_) | Expr::Exists(_)) {
                            constant = false;
                        }
                        if matches!(x, Expr::Function { name, .. } if name.starts_with("current_") || name == "random" || name == "datetime" || name == "date" || name == "time") {
                            constant = false;
                        }
                    });
                    if !constant {
                        return Err(SqlError::new(
                            "Cannot add a column with non-constant default",
                        ));
                    }
                    let ctx = run.ctx(state);
                    eval(&ctx, None, &e)?
                }
                None => Value::Null,
            };
            if column.not_null && default.is_null() {
                return Err(SqlError::new(
                    "Cannot add a NOT NULL column with default value NULL",
                ));
            }
            let mut def = crate::ast::CreateTable {
                name: t.name.clone(),
                if_not_exists: false,
                temporary: false,
                columns: vec![column.clone()],
                constraints: vec![],
                as_select: None,
                without_rowid: false,
                strict: false,
                sql: String::new(),
            };
            def.columns[0].primary_key = None;
            let mut scratch = State::default();
            let (built, _) = build_table(&mut scratch, &def, String::new())?;
            let mut col = built.columns[0].clone();
            col.primary_key = false;
            let affinity = col.affinity;
            let tm = Arc::make_mut(state.tables.get_mut(&key).expect("checked"));
            if let Some(fk) = built.foreign_keys.first() {
                let mut fk = fk.clone();
                fk.columns = vec![tm.columns.len()];
                tm.foreign_keys.push(fk);
            }
            tm.checks.extend(built.checks);
            tm.columns.push(col);
            let v = affinity.apply(default);
            for r in tm.rows.values_mut() {
                r.push(v.clone());
            }
            let close = tm.sql.rfind(')').unwrap_or(tm.sql.len());
            tm.sql = format!(
                "{}, {}{}",
                tm.sql[..close].trim_end(),
                text,
                &tm.sql[close..]
            );
        }
        AlterTable::DropColumn { table, column } => {
            let key = guard_writable(state, table)?;
            let t = state.tables[&key].clone();
            let i = t
                .column(column)
                .ok_or_else(|| SqlError::new(format!("no such column: \"{column}\"")))?;
            let refuse = |why: &str| {
                Err(SqlError::new(format!(
                    "error in table {} after drop column: cannot drop {why} column: \"{column}\"",
                    t.name
                )))
            };
            if t.primary_key.contains(&i) {
                return refuse("PRIMARY KEY");
            }
            if state
                .indexes_of(&t.name)
                .iter()
                .any(|x| x.columns.iter().any(|c| c.column == i))
            {
                return if state.indexes_of(&t.name).iter().any(|x| {
                    x.origin == IndexOrigin::Unique && x.columns.iter().any(|c| c.column == i)
                }) {
                    refuse("UNIQUE")
                } else {
                    Err(SqlError::new(format!(
                        "error in index after drop column: no such column: {column}"
                    )))
                };
            }
            if t.foreign_keys.iter().any(|f| f.columns.contains(&i)) {
                return refuse("foreign key");
            }
            if t.columns.len() == 1 {
                return Err(SqlError::new(format!(
                    "cannot drop column \"{column}\": no other columns exist"
                )));
            }
            let new_sql = drop_column_text(&t.sql, column)
                .ok_or_else(|| SqlError::new(format!("cannot drop column \"{column}\"")))?;
            let tm = Arc::make_mut(state.tables.get_mut(&key).expect("checked"));
            tm.columns.remove(i);
            for r in tm.rows.values_mut() {
                if i < r.len() {
                    r.remove(i);
                }
            }
            let shift = |c: &mut usize| {
                if *c > i {
                    *c -= 1
                }
            };
            if let Some(p) = &mut tm.ipk {
                shift(p);
            }
            tm.primary_key.iter_mut().for_each(shift);
            for f in &mut tm.foreign_keys {
                f.columns.iter_mut().for_each(shift);
            }
            tm.sql = new_sql;
            let rebuilt = tm.clone();
            for idx in state.indexes.values_mut() {
                if idx.table == key {
                    let im = Arc::make_mut(idx);
                    for c in &mut im.columns {
                        shift(&mut c.column);
                    }
                    im.rebuild(&rebuilt);
                }
            }
        }
    }
    state.touch_schema();
    Ok(Output::default())
}
/// CREATE TABLE text with the object name replaced.
fn rename_first_name(sql: &str, to: &str) -> String {
    let Ok(toks) = crate::lexer::tokenize(sql) else {
        return sql.into();
    };
    // CREATE TABLE <name>; SQLite always writes the new name quoted.
    if let Some(t) = toks.get(2) {
        return format!(
            "{}\"{}\"{}",
            &sql[..t.start],
            to.replace('"', "\"\""),
            &sql[t.end..]
        );
    }
    sql.into()
}
/// CREATE TABLE text with one column definition removed.
fn drop_column_text(sql: &str, column: &str) -> Option<String> {
    let toks = crate::lexer::tokenize(sql).ok()?;
    let mut depth = 0;
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut seg_start = None;
    for (i, t) in toks.iter().enumerate() {
        match t.tok {
            crate::lexer::Tok::Op("(") => {
                depth += 1;
                if depth == 1 {
                    seg_start = Some(i + 1);
                }
            }
            crate::lexer::Tok::Op(")") => {
                if depth == 1 {
                    segments.push((seg_start?, i));
                }
                depth -= 1;
            }
            crate::lexer::Tok::Op(",") if depth == 1 => {
                segments.push((seg_start?, i));
                seg_start = Some(i + 1);
            }
            _ => {}
        }
    }
    let pos = segments.iter().position(|(s, _)| {
        matches!(&toks[*s].tok, crate::lexer::Tok::Ident { name, .. } if name.eq_ignore_ascii_case(column))
    })?;
    let (s, e) = segments[pos];
    // Remove the segment and the comma before it (or after, for the first one).
    let (cut_start, cut_end) = if pos > 0 {
        (toks[s - 1].start, toks[e - 1].end)
    } else {
        (toks[s].start, toks[e].end)
    };
    let mut out = String::new();
    out.push_str(&sql[..cut_start]);
    let rest = &sql[cut_end..];
    out.push_str(if pos == 0 { rest.trim_start() } else { rest });
    Some(out)
}
