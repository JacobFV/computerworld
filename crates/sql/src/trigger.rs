//! Triggers: `CREATE TRIGGER` and `DROP TRIGGER`, and firing trigger programs for each
//! row an INSERT, UPDATE or DELETE touches, as SQLite does (`FOR EACH ROW` only).
//!
//! A trigger is kept as its `CREATE TRIGGER` text, as `sqlite_schema` holds it, and
//! parsed when it fires. `NEW.x` and `OLD.x` in the WHEN clause and the program are
//! bound to the row's values, keeping each column's affinity and collation. Triggers
//! fire most recently created first; a trigger that is already running does not fire
//! again (`PRAGMA recursive_triggers` is off, as it is by default); and `RAISE()`
//! ends the row (`IGNORE`) or the statement (`ABORT`, `FAIL`, `ROLLBACK`).
use crate::ast::*;
use crate::dml::Run;
use crate::eval::eval;
use crate::schema::{State, Table, Trigger};
use crate::value::{Affinity, Collation, Value};
use crate::{Output, SqlError};
use std::rc::Rc;

/// SQLite's `SQLITE_MAX_TRIGGER_DEPTH`.
const DEPTH_LIMIT: usize = 1000;

/// A row as a trigger sees it through `NEW` or `OLD`.
pub struct Image {
    cols: Vec<(String, Option<Affinity>, Collation)>,
    values: Vec<Value>,
    rowid: Option<i64>,
}
impl Image {
    /// A table row: `values` in column order; `rowid` is what `NEW.rowid` reads.
    pub fn of_table(table: &Table, values: &[Value], rowid: Option<i64>) -> Self {
        Self {
            cols: table
                .columns
                .iter()
                .map(|c| (c.name.clone(), Some(c.affinity), c.collation))
                .collect(),
            values: values[..table.columns.len().min(values.len())].to_vec(),
            rowid: if table.without_rowid { None } else { rowid },
        }
    }
    fn bind(&self, name: &str) -> Option<Expr> {
        if let Some(i) = self
            .cols
            .iter()
            .position(|c| c.0.eq_ignore_ascii_case(name))
        {
            return Some(Expr::Bound {
                value: self.values.get(i).cloned().unwrap_or(Value::Null),
                affinity: self.cols[i].1,
                collation: self.cols[i].2,
            });
        }
        let rowid = self.rowid?;
        ["rowid", "oid", "_rowid_"]
            .iter()
            .any(|r| r.eq_ignore_ascii_case(name))
            .then_some(Expr::Bound {
                value: Value::Integer(rowid),
                affinity: Some(Affinity::Integer),
                collation: Collation::Binary,
            })
    }
}

/// The change being made, and for an UPDATE the columns its SET list names (which an
/// `UPDATE OF` trigger watches).
pub enum Event<'a> {
    Insert,
    Update(&'a [String]),
    Delete,
}

// ----- binding NEW and OLD -----

fn bind(e: &mut Expr, old: Option<&Image>, new: Option<&Image>) {
    if let Expr::Column {
        table: Some(t),
        name,
    } = e
    {
        let image = if t.eq_ignore_ascii_case("new") {
            new
        } else if t.eq_ignore_ascii_case("old") {
            old
        } else {
            None
        };
        if let Some(b) = image.and_then(|i| i.bind(name)) {
            *e = b;
        }
    }
}
pub fn expr_mut(e: &mut Expr, f: &mut dyn FnMut(&mut Expr)) {
    f(e);
    match e {
        Expr::Unary(_, a) | Expr::Cast { expr: a, .. } | Expr::Collate { expr: a, .. } => {
            expr_mut(a, f)
        }
        Expr::Binary(_, a, b)
        | Expr::Is {
            left: a, right: b, ..
        } => {
            expr_mut(a, f);
            expr_mut(b, f);
        }
        Expr::Like {
            expr,
            pattern,
            escape,
            ..
        } => {
            expr_mut(expr, f);
            expr_mut(pattern, f);
            if let Some(x) = escape {
                expr_mut(x, f);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            expr_mut(expr, f);
            expr_mut(low, f);
            expr_mut(high, f);
        }
        Expr::InList { expr, list, .. } => {
            expr_mut(expr, f);
            for x in list {
                expr_mut(x, f);
            }
        }
        Expr::InSelect { expr, select, .. } => {
            expr_mut(expr, f);
            select_mut(select, f);
        }
        Expr::InTable { expr, .. } => expr_mut(expr, f),
        Expr::Exists(s) | Expr::Subquery(s) => select_mut(s, f),
        Expr::Case {
            operand,
            whens,
            otherwise,
        } => {
            if let Some(o) = operand {
                expr_mut(o, f);
            }
            for (w, t) in whens {
                expr_mut(w, f);
                expr_mut(t, f);
            }
            if let Some(o) = otherwise {
                expr_mut(o, f);
            }
        }
        Expr::Function { args, filter, .. } => {
            for a in args {
                expr_mut(a, f);
            }
            if let Some(x) = filter {
                expr_mut(x, f);
            }
        }
        Expr::Row(items) => {
            for x in items {
                expr_mut(x, f);
            }
        }
        Expr::Literal(_)
        | Expr::Column { .. }
        | Expr::Param(_)
        | Expr::Raise(..)
        | Expr::Bound { .. } => {}
    }
}
fn result_mut(cols: &mut [ResultCol], f: &mut dyn FnMut(&mut Expr)) {
    for rc in cols {
        if let ResultCol::Expr { expr, .. } = rc {
            expr_mut(expr, f);
        }
    }
}
fn from_mut(item: &mut FromItem, f: &mut dyn FnMut(&mut Expr)) {
    match item {
        FromItem::Table { .. } => {}
        FromItem::Subquery { select, .. } => select_mut(select, f),
        FromItem::Join {
            left,
            right,
            constraint,
            ..
        } => {
            from_mut(left, f);
            from_mut(right, f);
            if let JoinConstraint::On(e) = constraint {
                expr_mut(e, f);
            }
        }
    }
}
fn core_mut(c: &mut SelectCore, f: &mut dyn FnMut(&mut Expr)) {
    match c {
        SelectCore::Values(rows) => {
            for e in rows.iter_mut().flatten() {
                expr_mut(e, f);
            }
        }
        SelectCore::Select {
            columns,
            from,
            filter,
            group_by,
            having,
            ..
        } => {
            result_mut(columns, f);
            if let Some(fr) = from {
                from_mut(fr, f);
            }
            for e in filter.iter_mut().chain(group_by.iter_mut()).chain(having) {
                expr_mut(e, f);
            }
        }
    }
}
fn with_mut(w: &mut Option<With>, f: &mut dyn FnMut(&mut Expr)) {
    if let Some(w) = w {
        for c in &mut w.ctes {
            select_mut(&mut c.select, f);
        }
    }
}
pub fn select_mut(s: &mut Select, f: &mut dyn FnMut(&mut Expr)) {
    with_mut(&mut s.with, f);
    core_mut(&mut s.first, f);
    for (_, c) in &mut s.compounds {
        core_mut(c, f);
    }
    for t in &mut s.order_by {
        expr_mut(&mut t.expr, f);
    }
    for e in s.limit.iter_mut().chain(s.offset.iter_mut()) {
        expr_mut(e, f);
    }
}
fn stmt_mut(stmt: &mut Stmt, f: &mut dyn FnMut(&mut Expr)) {
    match stmt {
        Stmt::Select(s) => select_mut(s, f),
        Stmt::Insert(i) => {
            with_mut(&mut i.with, f);
            match &mut i.source {
                InsertSource::Values(rows) => {
                    for e in rows.iter_mut().flatten() {
                        expr_mut(e, f);
                    }
                }
                InsertSource::Select(s) => select_mut(s, f),
                InsertSource::Default => {}
            }
            if let Some(up) = &mut i.upsert {
                if let Some((sets, filter)) = &mut up.update {
                    for (_, e) in sets {
                        expr_mut(e, f);
                    }
                    if let Some(x) = filter {
                        expr_mut(x, f);
                    }
                }
            }
            result_mut(&mut i.returning, f);
        }
        Stmt::Update(u) => {
            with_mut(&mut u.with, f);
            for (_, e) in &mut u.sets {
                expr_mut(e, f);
            }
            if let Some(fr) = &mut u.from {
                from_mut(fr, f);
            }
            if let Some(x) = &mut u.filter {
                expr_mut(x, f);
            }
            result_mut(&mut u.returning, f);
        }
        Stmt::Delete(d) => {
            with_mut(&mut d.with, f);
            if let Some(x) = &mut d.filter {
                expr_mut(x, f);
            }
            result_mut(&mut d.returning, f);
        }
        _ => {}
    }
}

// ----- firing -----

fn parse(sql: &str) -> Result<CreateTrigger, SqlError> {
    match crate::parser::parse(sql)?.into_iter().next() {
        Some(Stmt::CreateTrigger(t)) => Ok(*t),
        _ => Err(SqlError::new(
            "malformed database schema: a trigger is not a trigger",
        )),
    }
}
/// Whether any trigger watches this table or view at all.
pub fn any(state: &State, table: &str) -> bool {
    let key = table.to_ascii_lowercase();
    state.triggers.values().any(|t| t.table == key)
}
/// Run one statement of a trigger program.
fn step(state: &mut State, run: &Run, stmt: &Stmt) -> Result<(), SqlError> {
    let inner = Run {
        params: &[],
        env: run.env,
        ctes: Rc::new(Vec::new()),
    };
    let out: Result<Output, SqlError> = match stmt {
        Stmt::Insert(i) => crate::dml::insert(state, &inner, i),
        Stmt::Update(u) => crate::dml::update(state, &inner, u),
        Stmt::Delete(d) => crate::dml::delete(state, &inner, d),
        Stmt::Select(s) => {
            let ctx = inner.ctx(state);
            crate::exec::select(&ctx, s, None).map(|_| Output::default())
        }
        _ => Err(SqlError::new(
            "a trigger program may only hold INSERT, UPDATE, DELETE and SELECT",
        )),
    };
    match out {
        Ok(o) => {
            // Rows a trigger changes count towards total_changes(), not changes().
            let env = run.env;
            env.total_changes
                .set(env.total_changes.get() + o.changes as i64);
            Ok(())
        }
        // Names in a trigger program resolve in the schema it belongs to.
        Err(mut e) => {
            if let Some(t) = e.message.strip_prefix("no such table: ") {
                if !t.starts_with("main.") {
                    e.message = format!("no such table: main.{t}");
                }
            }
            Err(e)
        }
    }
}
/// Fire the triggers on `table` with this timing for this change of one row. Returns
/// `false` when a trigger program ran `RAISE(IGNORE)`: the change to this row is
/// abandoned (and no further triggers run for it), but the statement goes on.
pub fn fire(
    state: &mut State,
    run: &Run,
    table: &str,
    timing: TriggerTiming,
    event: &Event,
    old: Option<&Image>,
    new: Option<&Image>,
) -> Result<bool, SqlError> {
    let list: Vec<Trigger> = state.triggers_on(table).into_iter().cloned().collect();
    for t in list {
        let running = run
            .env
            .triggers
            .borrow()
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&t.name));
        if running {
            continue;
        }
        let def = parse(&t.sql)?;
        if def.timing != timing {
            continue;
        }
        let watches = match (&def.event, event) {
            (TriggerEvent::Insert, Event::Insert) | (TriggerEvent::Delete, Event::Delete) => true,
            (TriggerEvent::Update(cols), Event::Update(set)) => {
                cols.is_empty()
                    || cols
                        .iter()
                        .any(|c| set.iter().any(|s| s.eq_ignore_ascii_case(c)))
            }
            _ => false,
        };
        if !watches {
            continue;
        }
        if run.env.triggers.borrow().len() >= DEPTH_LIMIT {
            return Err(SqlError::new("too many levels of trigger recursion"));
        }
        let mut binder = |e: &mut Expr| bind(e, old, new);
        if let Some(when) = &def.when {
            let mut w = when.clone();
            expr_mut(&mut w, &mut binder);
            let ctx = run.ctx(state);
            if eval(&ctx, None, &w)?.truth() != Some(true) {
                continue;
            }
        }
        run.env.triggers.borrow_mut().push(t.name.clone());
        // A trigger's inserts do not change what last_insert_rowid() reports.
        let rowid = run.env.last_insert_rowid.get();
        let mut result = Ok(());
        for stmt in &def.body {
            let mut s = stmt.clone();
            stmt_mut(&mut s, &mut binder);
            result = step(state, run, &s);
            if result.is_err() {
                break;
            }
        }
        run.env.triggers.borrow_mut().pop();
        run.env.last_insert_rowid.set(rowid);
        match result {
            Ok(()) => {}
            Err(e) if e.raise == Some(RaiseKind::Ignore) => return Ok(false),
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

// ----- INSTEAD OF triggers on views -----

fn view_rows(
    state: &State,
    run: &Run,
    view: &str,
    alias: Option<&str>,
    filter: Option<&Expr>,
) -> Result<(Vec<crate::eval::ColMeta>, Vec<Vec<Value>>), SqlError> {
    let ctx = run.ctx(state);
    let rel = crate::exec::from_relation(
        &ctx,
        &FromItem::Table {
            name: view.to_owned(),
            alias: alias.map(str::to_owned),
        },
        None,
        None,
    )?;
    let mut rows = Vec::new();
    for r in &rel.rows {
        let keep = match filter {
            Some(f) => {
                let scope = crate::eval::Scope {
                    cols: &rel.cols,
                    row: r,
                    parent: None,
                    aggs: None,
                };
                eval(&ctx, Some(&scope), f)?.truth() == Some(true)
            }
            None => true,
        };
        if keep {
            rows.push(r.clone());
        }
    }
    Ok((rel.cols, rows))
}
fn view_image(cols: &[crate::eval::ColMeta], values: Vec<Value>) -> Image {
    Image {
        cols: cols
            .iter()
            .map(|c| (c.name.clone(), c.affinity, c.collation))
            .collect(),
        values,
        rowid: None,
    }
}
fn refuse_view(name: &str) -> SqlError {
    SqlError::new(format!("cannot modify {name} because it is a view"))
}
/// Whether an INSTEAD OF trigger handles this change to a view.
fn instead(state: &State, view: &str, event: &Event) -> Result<bool, SqlError> {
    for t in state.triggers_on(view) {
        let def = parse(&t.sql)?;
        let same = matches!(
            (&def.event, event),
            (TriggerEvent::Insert, Event::Insert)
                | (TriggerEvent::Delete, Event::Delete)
                | (TriggerEvent::Update(_), Event::Update(_))
        );
        if same && def.timing == TriggerTiming::InsteadOf {
            return Ok(true);
        }
    }
    Ok(false)
}
/// INSERT into a view: each row goes to its INSTEAD OF INSERT triggers as `NEW`.
/// Changes a view's triggers make are not the statement's own, so none are counted.
pub fn insert_view(state: &mut State, run: &Run, ins: &Insert) -> Result<Output, SqlError> {
    if !instead(state, &ins.table, &Event::Insert)? {
        return Err(refuse_view(&ins.table));
    }
    if !ins.returning.is_empty() {
        return Err(SqlError::new("cannot use RETURNING with a view"));
    }
    let (cols, _) = view_rows(
        state,
        run,
        &ins.table,
        None,
        Some(&Expr::lit(Value::Integer(0))),
    )?;
    let slots: Vec<usize> = if ins.columns.is_empty() {
        (0..cols.len()).collect()
    } else {
        ins.columns
            .iter()
            .map(|c| {
                cols.iter()
                    .position(|m| m.name.eq_ignore_ascii_case(c))
                    .ok_or_else(|| {
                        SqlError::new(format!("table {} has no column named {c}", ins.table))
                    })
            })
            .collect::<Result<_, _>>()?
    };
    let source: Vec<Vec<Value>> = {
        let ctx = run.ctx(state);
        match &ins.source {
            InsertSource::Default => vec![vec![]],
            InsertSource::Values(rows) => rows
                .iter()
                .map(|r| r.iter().map(|e| eval(&ctx, None, e)).collect())
                .collect::<Result<_, _>>()?,
            InsertSource::Select(s) => crate::exec::select(&ctx, s, None)?.rows,
        }
    };
    for src in source {
        if !matches!(ins.source, InsertSource::Default) && src.len() != slots.len() {
            return Err(SqlError::new(format!(
                "{} values for {} columns",
                src.len(),
                slots.len()
            )));
        }
        let mut row = vec![Value::Null; cols.len()];
        for (s, v) in slots.iter().zip(src) {
            row[*s] = v;
        }
        let new = view_image(&cols, row);
        fire(
            state,
            run,
            &ins.table,
            TriggerTiming::InsteadOf,
            &Event::Insert,
            None,
            Some(&new),
        )?;
    }
    Ok(Output::default())
}
/// UPDATE of a view: each matching row, before and after the SET list, goes to its
/// INSTEAD OF UPDATE triggers as `OLD` and `NEW`.
pub fn update_view(state: &mut State, run: &Run, up: &Update) -> Result<Output, SqlError> {
    let targets: Vec<String> = up.sets.iter().flat_map(|(t, _)| t.clone()).collect();
    if !instead(state, &up.table, &Event::Update(&targets))? {
        return Err(refuse_view(&up.table));
    }
    let (cols, rows) = view_rows(
        state,
        run,
        &up.table,
        up.alias.as_deref(),
        up.filter.as_ref(),
    )?;
    let mut updates = Vec::new();
    {
        let ctx = run.ctx(state);
        for old in &rows {
            let scope = crate::eval::Scope {
                cols: &cols,
                row: old,
                parent: None,
                aggs: None,
            };
            let mut new = old.clone();
            for (names, e) in &up.sets {
                let values = if names.len() == 1 {
                    vec![eval(&ctx, Some(&scope), e)?]
                } else {
                    match e {
                        Expr::Row(items) if items.len() == names.len() => items
                            .iter()
                            .map(|i| eval(&ctx, Some(&scope), i))
                            .collect::<Result<_, _>>()?,
                        _ => {
                            return Err(SqlError::new(format!(
                                "{} columns assigned 1 values",
                                names.len()
                            )))
                        }
                    }
                };
                for (n, v) in names.iter().zip(values) {
                    let i = cols
                        .iter()
                        .position(|c| c.name.eq_ignore_ascii_case(n))
                        .ok_or_else(|| SqlError::new(format!("no such column: {n}")))?;
                    new[i] = v;
                }
            }
            updates.push((old.clone(), new));
        }
    }
    for (old, new) in updates {
        let (o, n) = (view_image(&cols, old), view_image(&cols, new));
        fire(
            state,
            run,
            &up.table,
            TriggerTiming::InsteadOf,
            &Event::Update(&targets),
            Some(&o),
            Some(&n),
        )?;
    }
    Ok(Output::default())
}
/// DELETE from a view: each matching row goes to its INSTEAD OF DELETE triggers.
pub fn delete_view(state: &mut State, run: &Run, del: &Delete) -> Result<Output, SqlError> {
    if !instead(state, &del.table, &Event::Delete)? {
        return Err(refuse_view(&del.table));
    }
    let (cols, rows) = view_rows(
        state,
        run,
        &del.table,
        del.alias.as_deref(),
        del.filter.as_ref(),
    )?;
    for old in rows {
        let o = view_image(&cols, old);
        fire(
            state,
            run,
            &del.table,
            TriggerTiming::InsteadOf,
            &Event::Delete,
            Some(&o),
            None,
        )?;
    }
    Ok(Output::default())
}

// ----- schema statements -----

fn from_tables(item: &FromItem, out: &mut Vec<String>) {
    match item {
        FromItem::Table { name, .. } => out.push(name.clone()),
        FromItem::Subquery { select, .. } => select_tables(select, out),
        FromItem::Join {
            left,
            right,
            constraint,
            ..
        } => {
            from_tables(left, out);
            from_tables(right, out);
            if let JoinConstraint::On(e) = constraint {
                expr_tables(e, out);
            }
        }
    }
}
fn expr_tables(e: &Expr, out: &mut Vec<String>) {
    e.walk(&mut |x| match x {
        Expr::Exists(s) | Expr::Subquery(s) | Expr::InSelect { select: s, .. } => {
            select_tables(s, out)
        }
        Expr::InTable { table, .. } => out.push(table.clone()),
        _ => {}
    });
}
fn select_tables(s: &Select, out: &mut Vec<String>) {
    let mut found = Vec::new();
    let mut ctes = Vec::new();
    if let Some(w) = &s.with {
        for c in &w.ctes {
            ctes.push(c.name.to_ascii_lowercase());
            select_tables(&c.select, &mut found);
        }
    }
    for c in std::iter::once(&s.first).chain(s.compounds.iter().map(|(_, c)| c)) {
        if let SelectCore::Select {
            columns,
            from,
            filter,
            group_by,
            having,
            ..
        } = c
        {
            if let Some(f) = from {
                from_tables(f, &mut found);
            }
            for rc in columns {
                if let ResultCol::Expr { expr, .. } = rc {
                    expr_tables(expr, &mut found);
                }
            }
            for e in filter.iter().chain(group_by).chain(having) {
                expr_tables(e, &mut found);
            }
        }
    }
    out.extend(
        found
            .into_iter()
            .filter(|n| !ctes.contains(&n.to_ascii_lowercase())),
    );
}
/// Tables a trigger program names.
fn program_tables(def: &CreateTrigger) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in &def.body {
        match stmt {
            Stmt::Insert(i) => {
                out.push(i.table.clone());
                if let InsertSource::Select(s) = &i.source {
                    select_tables(s, &mut out);
                }
            }
            Stmt::Update(u) => {
                out.push(u.table.clone());
                if let Some(f) = &u.from {
                    from_tables(f, &mut out);
                }
                if let Some(e) = &u.filter {
                    expr_tables(e, &mut out);
                }
            }
            Stmt::Delete(d) => {
                out.push(d.table.clone());
                if let Some(e) = &d.filter {
                    expr_tables(e, &mut out);
                }
            }
            Stmt::Select(s) => select_tables(s, &mut out),
            _ => {}
        }
    }
    out
}
/// Before ALTER TABLE rewrites the schema, SQLite re-reads every trigger and refuses
/// the change when one names a table that does not exist.
pub fn check_programs(state: &State) -> Result<(), SqlError> {
    let mut list: Vec<&Trigger> = state.triggers.values().collect();
    list.sort_by_key(|t| t.ordinal);
    for t in list {
        let def = parse(&t.sql)?;
        for name in program_tables(&def) {
            let key = name.to_ascii_lowercase();
            let known = state.tables.contains_key(&key)
                || state.views.contains_key(&key)
                || matches!(key.as_str(), "sqlite_schema" | "sqlite_master");
            if !known {
                return Err(SqlError::new(format!(
                    "error in trigger {}: no such table: main.{name}",
                    t.name
                )));
            }
        }
    }
    Ok(())
}

pub fn create(state: &mut State, ct: &CreateTrigger) -> Result<Output, SqlError> {
    let key = ct.name.to_ascii_lowercase();
    if state.triggers.contains_key(&key) {
        if ct.if_not_exists {
            return Ok(Output::default());
        }
        return Err(SqlError::syntax_at(
            format!("trigger {} already exists", ct.name),
            ct.name_at,
        ));
    }
    if key.starts_with("sqlite_") {
        return Err(SqlError::new(format!(
            "object name reserved for internal use: {}",
            ct.name
        )));
    }
    let table = ct.table.to_ascii_lowercase();
    if matches!(table.as_str(), "sqlite_schema" | "sqlite_master") || table.starts_with("sqlite_") {
        return Err(SqlError::new("cannot create trigger on system table"));
    }
    let view = state.views.get(&table).map(|v| v.name.clone());
    let base = state.tables.get(&table).map(|t| (t.name.clone(), t.temp));
    let timing_word = match ct.timing {
        TriggerTiming::Before => "BEFORE",
        TriggerTiming::After => "AFTER",
        TriggerTiming::InsteadOf => "INSTEAD OF",
    };
    let temp = match (&view, &base) {
        (None, None) => return Err(SqlError::new(format!("no such table: main.{}", ct.table))),
        (Some(v), _) if ct.timing != TriggerTiming::InsteadOf => {
            return Err(SqlError::new(format!(
                "cannot create {timing_word} trigger on view: {v}"
            )))
        }
        (None, Some((t, _))) if ct.timing == TriggerTiming::InsteadOf => {
            return Err(SqlError::new(format!(
                "cannot create INSTEAD OF trigger on table: {t}"
            )))
        }
        (_, Some((_, temp))) => ct.temporary || *temp,
        _ => ct.temporary,
    };
    let ordinal = state.ordinal();
    state.triggers.insert(
        key,
        Trigger {
            name: ct.name.clone(),
            table,
            sql: ct.sql.clone(),
            ordinal,
            temp,
        },
    );
    state.touch_schema();
    Ok(Output::default())
}
