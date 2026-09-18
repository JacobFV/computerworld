//! A pure, deterministic SQL engine for a SQLite-compatible dialect.
//!
//! The database lives in memory as tables keyed by rowid plus B-tree indexes, and is
//! persisted in the real SQLite 3 file format (`file`), so a database written here opens
//! in `sqlite3` and one written by `sqlite3` opens here. Nothing reads the host clock or
//! a host random source: `now` is whatever the caller says the world clock reads, and
//! `random()` is a seeded stream kept with the connection.
pub mod ast;
pub mod cli;
pub mod csv;
mod datetime;
mod dml;
mod eval;
mod exec;
pub mod file;
mod func;
pub mod lexer;
pub mod parser;
mod schema;
mod trigger;
pub mod value;

pub use func::SCALAR_FUNCTIONS;
pub use value::{Affinity, Collation, Value};

use ast::{PragmaArg, Stmt};
use eval::{Ctx, Env};
use schema::{IndexOrigin, State};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

/// The SQLite release whose behaviour this engine follows.
pub const SQLITE_VERSION: &str = "3.45.1";
pub const SQLITE_VERSION_NUMBER: u32 = 3_045_001;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlError {
    pub message: String,
    /// Byte offset of the offending token, for syntax errors.
    pub offset: Option<usize>,
    /// SQLite's primary result code: 1 error, 19 constraint, 20 mismatch, 26 not a database.
    pub code: i32,
    /// Set when a trigger program's `RAISE()` produced this error: how the statement
    /// that fired the trigger must end.
    pub raise: Option<ast::RaiseKind>,
}
impl SqlError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            offset: None,
            code: 1,
            raise: None,
        }
    }
    pub fn syntax_at(message: impl Into<String>, offset: usize) -> Self {
        Self {
            message: message.into(),
            offset: Some(offset),
            code: 1,
            raise: None,
        }
    }
    /// `RAISE(kind, message)`: a constraint error (19) carrying how to end the statement.
    pub fn raise(kind: ast::RaiseKind, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            offset: None,
            code: 19,
            raise: Some(kind),
        }
    }
    pub fn with_code(mut self, code: i32) -> Self {
        self.code = code;
        self
    }
    fn shifted(mut self, by: usize) -> Self {
        if let Some(o) = &mut self.offset {
            *o += by;
        }
        self
    }
}
impl std::fmt::Display for SqlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for SqlError {}

/// What one statement produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    /// Rows inserted, updated or deleted.
    pub changes: u64,
    /// Rows are an `EXPLAIN QUERY PLAN` tree: `id`, `parent`, `notused`, `detail`.
    pub plan: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub decl_type: String,
    pub not_null: bool,
    pub default: Option<String>,
    /// Position in the primary key, from 1; 0 when not part of it.
    pub primary_key: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaEntry {
    /// `table`, `index` or `view`.
    pub kind: String,
    pub name: String,
    pub table: String,
    pub sql: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Database {
    state: State,
    /// Open transaction: the state at BEGIN, then one snapshot per SAVEPOINT.
    savepoints: Vec<(Option<String>, State)>,
    now_us: i64,
    rng: u64,
    last_insert_rowid: i64,
    changes: i64,
    total_changes: i64,
}

impl Database {
    pub fn new() -> Self {
        Self::default()
    }
    /// Open a database from SQLite file bytes; empty bytes are a new database.
    pub fn open(bytes: &[u8]) -> Result<Self, SqlError> {
        if bytes.is_empty() {
            return Ok(Self::new());
        }
        Ok(Self {
            state: file::read(bytes)?,
            ..Self::default()
        })
    }
    /// The database in the SQLite 3 file format. Uncommitted work of an open
    /// transaction is not included: a file only ever holds committed state.
    pub fn to_bytes(&self) -> Vec<u8> {
        let committed = match self.savepoints.first() {
            Some((_, s)) => s,
            None => &self.state,
        };
        file::write(committed)
    }
    /// The world clock, in microseconds since the Unix epoch, that `'now'` reads.
    pub fn set_now(&mut self, unix_us: i64) {
        self.now_us = unix_us;
    }
    pub fn in_transaction(&self) -> bool {
        !self.savepoints.is_empty()
    }
    pub fn last_insert_rowid(&self) -> i64 {
        self.last_insert_rowid
    }
    pub fn changes(&self) -> i64 {
        self.changes
    }
    pub fn total_changes_count(&self) -> i64 {
        self.total_changes
    }
    /// Whether two connections hold the same tables, indexes, views and settings,
    /// ignoring connection counters.
    pub fn same_content(&self, other: &Database) -> bool {
        self.state == other.state
    }
    pub fn foreign_keys(&self) -> bool {
        self.state.foreign_keys
    }
    /// Discard an open transaction, as closing a connection does.
    pub fn rollback_all(&mut self) {
        if let Some((_, s)) = self.savepoints.drain(..).next() {
            self.state = s;
        }
    }

    /// Run every statement in `sql`, stopping at the first error. Statements before
    /// it keep their effect, as with `sqlite3_exec`.
    pub fn execute(&mut self, sql: &str) -> Result<Vec<Output>, SqlError> {
        let mut out = Vec::new();
        for (start, text) in lexer::split_statements(sql) {
            if lexer::is_blank(&text) {
                continue;
            }
            let o = self.execute_one(&text, &[]).map_err(|e| e.shifted(start))?;
            out.push(o);
        }
        Ok(out)
    }
    /// Run one statement with bound parameters (`?`, `?N`, `:name`).
    pub fn execute_one(&mut self, sql: &str, params: &[Value]) -> Result<Output, SqlError> {
        let mut p = parser::Parser::new(sql)?;
        let mut stmts = Vec::new();
        loop {
            while p.skip_semi() {}
            if p.at_end() {
                break;
            }
            stmts.push(p.statement()?);
            if !p.at_end() && !p.skip_semi() {
                return Err(p.unexpected());
            }
        }
        match stmts.len() {
            0 => Ok(Output::default()),
            1 => self.run(&stmts[0], sql, params),
            _ => Err(SqlError::new(
                "only one statement may be prepared at a time",
            )),
        }
    }
    /// Convenience: the rows of a single query.
    pub fn query(&mut self, sql: &str) -> Result<Output, SqlError> {
        self.execute(sql).map(|mut v| v.pop().unwrap_or_default())
    }

    fn env(&self) -> Env {
        let env = Env {
            now_us: self.now_us,
            ..Env::default()
        };
        env.rng.set(self.rng);
        env.last_insert_rowid.set(self.last_insert_rowid);
        env.changes.set(self.changes);
        env.total_changes.set(self.total_changes);
        env
    }
    fn settle(&mut self, env: &Env) {
        self.rng = env.rng.get();
        self.last_insert_rowid = env.last_insert_rowid.get();
        // Rows changed by trigger programs count in total_changes().
        self.total_changes = env.total_changes.get();
    }
    fn run(&mut self, stmt: &Stmt, text: &str, params: &[Value]) -> Result<Output, SqlError> {
        let env = self.env();
        let run = dml::Run {
            params,
            env: &env,
            ctes: Rc::new(Vec::new()),
        };
        let conflict = match stmt {
            Stmt::Insert(i) => Some(i.conflict),
            Stmt::Update(u) => Some(u.conflict),
            _ => None,
        };
        let before = self.state.clone();
        let result = match stmt {
            Stmt::Select(s) => {
                let ctx = Ctx {
                    state: &self.state,
                    params,
                    env: &env,
                    ctes: Rc::new(Vec::new()),
                    plan: None,
                    depth: 0,
                    shape: None,
                };
                exec::select(&ctx, s, None).map(|rel| Output {
                    columns: rel.cols.into_iter().map(|c| c.name).collect(),
                    rows: rel.rows,
                    ..Output::default()
                })
            }
            Stmt::Insert(i) => with_ctes(&self.state, &env, params, &i.with)
                .and_then(|ctes| dml::insert(&mut self.state, &dml::Run { ctes, ..run }, i)),
            Stmt::Update(u) => with_ctes(&self.state, &env, params, &u.with)
                .and_then(|ctes| dml::update(&mut self.state, &dml::Run { ctes, ..run }, u)),
            Stmt::Delete(d) => with_ctes(&self.state, &env, params, &d.with)
                .and_then(|ctes| dml::delete(&mut self.state, &dml::Run { ctes, ..run }, d)),
            Stmt::CreateTable(ct) => dml::create_table(&mut self.state, &run, ct),
            Stmt::CreateIndex(ci) => dml::create_index(&mut self.state, ci),
            Stmt::CreateTrigger(ct) => trigger::create(&mut self.state, ct),
            Stmt::CreateView(cv) => {
                let select_text = view_select_text(&cv.sql);
                dml::create_view(&mut self.state, &run, cv, select_text)
            }
            Stmt::Drop {
                kind,
                name,
                if_exists,
            } => dml::drop(&mut self.state, &run, *kind, name, *if_exists),
            Stmt::AlterTable(a) => dml::alter(&mut self.state, &run, a),
            Stmt::Begin => {
                if self.in_transaction() {
                    Err(SqlError::new(
                        "cannot start a transaction within a transaction",
                    ))
                } else {
                    self.savepoints.push((None, self.state.clone()));
                    Ok(Output::default())
                }
            }
            Stmt::Commit => {
                if self.in_transaction() {
                    self.savepoints.clear();
                    Ok(Output::default())
                } else {
                    Err(SqlError::new("cannot commit - no transaction is active"))
                }
            }
            Stmt::Rollback { savepoint: None } => {
                if self.in_transaction() {
                    self.rollback_all();
                    Ok(Output::default())
                } else {
                    Err(SqlError::new("cannot rollback - no transaction is active"))
                }
            }
            Stmt::Rollback {
                savepoint: Some(name),
            } => match self.find_savepoint(name) {
                Some(i) => {
                    self.state = self.savepoints[i].1.clone();
                    self.savepoints.truncate(i + 1);
                    Ok(Output::default())
                }
                None => Err(SqlError::new(format!("no such savepoint: {name}"))),
            },
            Stmt::Savepoint(name) => {
                self.savepoints
                    .push((Some(name.clone()), self.state.clone()));
                Ok(Output::default())
            }
            Stmt::Release(name) => match self.find_savepoint(name) {
                Some(i) => {
                    self.savepoints.truncate(i);
                    Ok(Output::default())
                }
                None => Err(SqlError::new(format!("no such savepoint: {name}"))),
            },
            Stmt::Pragma { name, arg } => self.pragma(name, arg.as_ref()),
            Stmt::Explain {
                query_plan: true,
                stmt,
            } => self.explain(stmt, params),
            Stmt::Explain { .. } => Err(SqlError::new(
                "EXPLAIN bytecode listings are not supported; use EXPLAIN QUERY PLAN",
            )),
            Stmt::Vacuum => {
                if self.in_transaction() {
                    Err(SqlError::new("cannot VACUUM from within a transaction"))
                } else {
                    // The file is always written compactly; there is nothing to reclaim.
                    Ok(Output::default())
                }
            }
            Stmt::Analyze => Ok(Output::default()),
            Stmt::Reindex => {
                let tables = self.state.tables.clone();
                for index in self.state.indexes.values_mut() {
                    if let Some(t) = tables.get(&index.table) {
                        std::sync::Arc::make_mut(index).rebuild(t);
                    }
                }
                Ok(Output::default())
            }
        };
        let _ = text;
        self.settle(&env);
        match result {
            Ok(o) => {
                if matches!(stmt, Stmt::Insert(_) | Stmt::Update(_) | Stmt::Delete(_)) {
                    self.changes = o.changes as i64;
                    self.total_changes += o.changes as i64;
                }
                if self.state != before && !self.in_transaction() {
                    self.state.change_counter = self.state.change_counter.wrapping_add(1);
                }
                Ok(o)
            }
            Err(e) => {
                // RAISE() in a trigger program decides how the statement ends: FAIL keeps
                // what it changed so far, ROLLBACK ends the whole transaction.
                let conflict = match e.raise {
                    Some(ast::RaiseKind::Fail) => Some(ast::Conflict::Fail),
                    Some(ast::RaiseKind::Rollback) => Some(ast::Conflict::Rollback),
                    Some(_) => Some(ast::Conflict::Abort),
                    None => conflict,
                };
                match conflict {
                    Some(ast::Conflict::Fail) => {
                        // The rows changed before the failure stay, and count.
                        let direct = env.direct.get() as i64;
                        self.changes = direct;
                        self.total_changes += direct;
                    }
                    Some(ast::Conflict::Rollback) if e.code == 19 => {
                        if self.in_transaction() {
                            self.rollback_all();
                        } else {
                            self.state = before;
                        }
                    }
                    _ => self.state = before,
                }
                Err(e)
            }
        }
    }
    fn find_savepoint(&self, name: &str) -> Option<usize> {
        self.savepoints
            .iter()
            .rposition(|(n, _)| n.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
    }
    fn explain(&mut self, stmt: &Stmt, params: &[Value]) -> Result<Output, SqlError> {
        let notes = RefCell::new(Vec::new());
        let env = self.env();
        match stmt {
            Stmt::Select(s) => {
                let ctx = Ctx {
                    state: &self.state,
                    params,
                    env: &env,
                    ctes: Rc::new(Vec::new()),
                    plan: Some(&notes),
                    depth: 0,
                    shape: None,
                };
                exec::select(&ctx, s, None)?;
            }
            other => {
                // Plan a write against a scratch copy: EXPLAIN never changes anything.
                let mut scratch = self.clone();
                let _ = scratch.run(other, "", params);
                notes.borrow_mut().push((0, "SCAN (write)".to_string()));
            }
        }
        let mut rows = Vec::new();
        let mut seen: Vec<(usize, String)> = Vec::new();
        let mut last_at_depth: Vec<i64> = Vec::new();
        for (depth, detail) in notes.into_inner() {
            if seen.contains(&(depth, detail.clone())) {
                continue;
            }
            seen.push((depth, detail.clone()));
            let id = rows.len() as i64 + 2;
            let parent = if depth == 0 {
                0
            } else {
                last_at_depth.get(depth - 1).copied().unwrap_or(0)
            };
            if last_at_depth.len() <= depth {
                last_at_depth.resize(depth + 1, 0);
            }
            last_at_depth[depth] = id;
            rows.push(vec![
                Value::Integer(id),
                Value::Integer(parent),
                Value::Integer(0),
                Value::Text(detail),
            ]);
        }
        Ok(Output {
            columns: vec![
                "id".into(),
                "parent".into(),
                "notused".into(),
                "detail".into(),
            ],
            rows,
            changes: 0,
            plan: true,
        })
    }
    fn pragma(&mut self, name: &str, arg: Option<&PragmaArg>) -> Result<Output, SqlError> {
        let one = |col: &str, v: Value| Output {
            columns: vec![col.into()],
            rows: vec![vec![v]],
            ..Output::default()
        };
        let table_arg = |arg: Option<&PragmaArg>| match arg {
            Some(PragmaArg::Call(t)) | Some(PragmaArg::Set(Value::Text(t))) => Some(t.clone()),
            _ => None,
        };
        let truthy = |v: &Value| match v {
            Value::Text(t) => {
                matches!(t.to_ascii_lowercase().as_str(), "on" | "yes" | "true" | "1")
            }
            other => other.to_i64().unwrap_or(0) != 0,
        };
        Ok(match name {
            "foreign_keys" => match arg {
                Some(a) => {
                    let v = match a {
                        PragmaArg::Set(v) => v.clone(),
                        PragmaArg::Call(t) => Value::Text(t.clone()),
                    };
                    // A no-op inside a transaction, as SQLite documents.
                    if !self.in_transaction() {
                        self.state.foreign_keys = truthy(&v);
                    }
                    Output::default()
                }
                None => one(
                    "foreign_keys",
                    Value::Integer(i64::from(self.state.foreign_keys)),
                ),
            },
            "user_version" => match arg {
                Some(PragmaArg::Set(v)) => {
                    self.state.user_version = v.to_i64().unwrap_or(0);
                    Output::default()
                }
                _ => one("user_version", Value::Integer(self.state.user_version)),
            },
            "schema_version" => one(
                "schema_version",
                Value::Integer(i64::from(self.state.schema_cookie)),
            ),
            "page_size" => one("page_size", Value::Integer(file::PAGE_SIZE as i64)),
            "page_count" => one(
                "page_count",
                Value::Integer((file::write(&self.state).len() / file::PAGE_SIZE) as i64),
            ),
            "freelist_count" => one("freelist_count", Value::Integer(0)),
            "encoding" => one("encoding", Value::Text("UTF-8".into())),
            "journal_mode" => one("journal_mode", Value::Text("delete".into())),
            "table_info" | "table_xinfo" => {
                let Some(t) = table_arg(arg) else {
                    return Ok(Output::default());
                };
                let mut out = Output {
                    columns: ["cid", "name", "type", "notnull", "dflt_value", "pk"]
                        .map(String::from)
                        .to_vec(),
                    ..Output::default()
                };
                if let Some(info) = self.table_info(&t) {
                    for (i, c) in info.iter().enumerate() {
                        out.rows.push(vec![
                            Value::Integer(i as i64),
                            Value::Text(c.name.clone()),
                            // SQLite keeps the six standard type names in its own
                            // spelling (the ones STRICT tables allow).
                            Value::Text(match c.decl_type.to_ascii_uppercase().as_str() {
                                t @ ("ANY" | "BLOB" | "INT" | "INTEGER" | "REAL" | "TEXT") => {
                                    t.to_owned()
                                }
                                _ => c.decl_type.clone(),
                            }),
                            Value::Integer(i64::from(c.not_null)),
                            c.default.clone().map_or(Value::Null, Value::Text),
                            Value::Integer(c.primary_key as i64),
                        ]);
                    }
                }
                out
            }
            "index_list" => {
                let Some(t) = table_arg(arg) else {
                    return Ok(Output::default());
                };
                let mut out = Output {
                    columns: ["seq", "name", "unique", "origin", "partial"]
                        .map(String::from)
                        .to_vec(),
                    ..Output::default()
                };
                let mut list = self.state.indexes_of(&t);
                list.reverse();
                for (i, idx) in list.iter().enumerate() {
                    out.rows.push(vec![
                        Value::Integer(i as i64),
                        Value::Text(idx.name.clone()),
                        Value::Integer(i64::from(idx.unique)),
                        Value::Text(
                            match idx.origin {
                                IndexOrigin::Created => "c",
                                IndexOrigin::Unique => "u",
                                IndexOrigin::PrimaryKey => "pk",
                            }
                            .into(),
                        ),
                        Value::Integer(0),
                    ]);
                }
                out
            }
            "index_info" => {
                let Some(i) = table_arg(arg) else {
                    return Ok(Output::default());
                };
                let mut out = Output {
                    columns: ["seqno", "cid", "name"].map(String::from).to_vec(),
                    ..Output::default()
                };
                if let Some(idx) = self.state.indexes.get(&i.to_ascii_lowercase()) {
                    let t = &self.state.tables[&idx.table];
                    for (n, c) in idx.columns.iter().enumerate() {
                        out.rows.push(vec![
                            Value::Integer(n as i64),
                            Value::Integer(c.column as i64),
                            Value::Text(t.columns[c.column].name.clone()),
                        ]);
                    }
                }
                out
            }
            "foreign_key_list" => {
                let Some(t) = table_arg(arg) else {
                    return Ok(Output::default());
                };
                let mut out = Output {
                    columns: [
                        "id",
                        "seq",
                        "table",
                        "from",
                        "to",
                        "on_update",
                        "on_delete",
                        "match",
                    ]
                    .map(String::from)
                    .to_vec(),
                    ..Output::default()
                };
                if let Ok(table) = self.state.table(&t) {
                    let action = |a: ast::FkAction| match a {
                        ast::FkAction::NoAction => "NO ACTION",
                        ast::FkAction::Restrict => "RESTRICT",
                        ast::FkAction::SetNull => "SET NULL",
                        ast::FkAction::SetDefault => "SET DEFAULT",
                        ast::FkAction::Cascade => "CASCADE",
                    };
                    let n = table.foreign_keys.len();
                    for (id, fk) in table.foreign_keys.iter().rev().enumerate() {
                        let _ = n;
                        for (seq, c) in fk.columns.iter().enumerate() {
                            out.rows.push(vec![
                                Value::Integer(id as i64),
                                Value::Integer(seq as i64),
                                Value::Text(fk.parent.clone()),
                                Value::Text(table.columns[*c].name.clone()),
                                fk.parent_columns
                                    .get(seq)
                                    .cloned()
                                    .map_or(Value::Null, Value::Text),
                                Value::Text(action(fk.on_update).into()),
                                Value::Text(action(fk.on_delete).into()),
                                Value::Text("NONE".into()),
                            ]);
                        }
                    }
                }
                out
            }
            "table_list" => {
                let mut out = Output {
                    columns: ["schema", "name", "type", "ncol", "wr", "strict"]
                        .map(String::from)
                        .to_vec(),
                    ..Output::default()
                };
                for e in self.schema() {
                    if e.kind == "index" || e.kind == "trigger" {
                        continue;
                    }
                    let without_rowid = e.kind == "table" && !self.has_rowid(&e.name);
                    let ncol = if e.kind == "table" {
                        self.state.tables[&e.name.to_ascii_lowercase()]
                            .columns
                            .len()
                    } else {
                        self.table_info(&e.name).map_or(0, |c| c.len())
                    };
                    out.rows.push(vec![
                        Value::Text("main".into()),
                        Value::Text(e.name.clone()),
                        Value::Text(e.kind.clone()),
                        Value::Integer(ncol as i64),
                        Value::Integer(i64::from(without_rowid)),
                        Value::Integer(0),
                    ]);
                }
                out.rows.push(vec![
                    Value::Text("main".into()),
                    Value::Text("sqlite_schema".into()),
                    Value::Text("table".into()),
                    Value::Integer(5),
                    Value::Integer(0),
                    Value::Integer(0),
                ]);
                out
            }
            "database_list" => Output {
                columns: ["seq", "name", "file"].map(String::from).to_vec(),
                rows: vec![vec![
                    Value::Integer(0),
                    Value::Text("main".into()),
                    Value::Text(String::new()),
                ]],
                ..Output::default()
            },
            "integrity_check" | "quick_check" => {
                let problems = self.integrity_problems();
                Output {
                    columns: vec![name.into()],
                    rows: if problems.is_empty() {
                        vec![vec![Value::Text("ok".into())]]
                    } else {
                        problems.into_iter().map(|p| vec![Value::Text(p)]).collect()
                    },
                    ..Output::default()
                }
            }
            "foreign_key_check" => self.foreign_key_check()?,
            // Settings with no observable effect in a single-connection, in-memory
            // engine are accepted and reported as SQLite would.
            "synchronous" => one("synchronous", Value::Integer(2)),
            "cache_size" => one("cache_size", Value::Integer(-2000)),
            "auto_vacuum" => one("auto_vacuum", Value::Integer(0)),
            "application_id" => one("application_id", Value::Integer(0)),
            _ => Output::default(),
        })
    }
    fn integrity_problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for index in self.state.indexes.values() {
            let Some(t) = self.state.tables.get(&index.table) else {
                continue;
            };
            let mut fresh = (**index).clone();
            fresh.rebuild(t);
            if fresh.entries != index.entries {
                out.push(format!("wrong # of entries in index {}", index.name));
            }
        }
        for t in self.state.tables.values() {
            for (i, c) in t.columns.iter().enumerate() {
                if c.not_null && Some(i) != t.ipk {
                    for (id, r) in &t.rows {
                        if r.get(i).is_none_or(Value::is_null) {
                            out.push(format!("NULL value in {}.{} (rowid {id})", t.name, c.name));
                        }
                    }
                }
            }
        }
        out
    }
    fn foreign_key_check(&self) -> Result<Output, SqlError> {
        let mut out = Output {
            columns: ["table", "rowid", "parent", "fkid"]
                .map(String::from)
                .to_vec(),
            ..Output::default()
        };
        for t in self.state.tables.values() {
            for (fkid, fk) in t.foreign_keys.iter().enumerate() {
                let Some(parent) = self.state.tables.get(&fk.parent.to_ascii_lowercase()) else {
                    continue;
                };
                let cols: Vec<Option<usize>> = if fk.parent_columns.is_empty() {
                    match parent.ipk {
                        Some(_) => vec![None],
                        None => parent.primary_key.iter().map(|c| Some(*c)).collect(),
                    }
                } else {
                    fk.parent_columns
                        .iter()
                        .map(|n| parent.column(n).filter(|i| Some(*i) != parent.ipk))
                        .collect()
                };
                for (id, s) in &t.rows {
                    let row = t.full_row(*id, s);
                    let vals: Vec<Value> = fk.columns.iter().map(|c| row[*c].clone()).collect();
                    if vals.iter().any(Value::is_null) {
                        continue;
                    }
                    let found = parent.rows.iter().any(|(pid, ps)| {
                        let prow = parent.full_row(*pid, ps);
                        cols.iter().zip(&vals).all(|(c, v)| {
                            let pv = c.map_or(Value::Integer(*pid), |i| prow[i].clone());
                            value::same(&pv, v)
                        })
                    });
                    if !found {
                        out.rows.push(vec![
                            Value::Text(t.name.clone()),
                            Value::Integer(*id),
                            Value::Text(fk.parent.clone()),
                            Value::Integer(fkid as i64),
                        ]);
                    }
                }
            }
        }
        Ok(out)
    }

    // ----- schema inspection for tools -----

    /// Every table, index and view, in creation order (as `sqlite_schema` lists them).
    pub fn schema(&self) -> Vec<SchemaEntry> {
        let mut out: Vec<(u64, SchemaEntry)> = Vec::new();
        for t in self.state.tables.values() {
            if t.temp {
                continue;
            }
            out.push((
                t.ordinal,
                SchemaEntry {
                    kind: "table".into(),
                    name: t.name.clone(),
                    table: t.name.clone(),
                    sql: Some(t.sql.clone()),
                },
            ));
        }
        for i in self.state.indexes.values() {
            let owner = self.state.tables.get(&i.table);
            // A WITHOUT ROWID table is its own primary key index: no row of its own.
            if owner.is_some_and(|t| t.without_rowid) && i.origin == IndexOrigin::PrimaryKey {
                continue;
            }
            if owner.is_some_and(|t| t.temp) {
                continue;
            }
            let table = owner.map_or(i.table.clone(), |t| t.name.clone());
            out.push((
                i.ordinal,
                SchemaEntry {
                    kind: "index".into(),
                    name: i.name.clone(),
                    table,
                    sql: i.sql.clone(),
                },
            ));
        }
        for v in self.state.views.values() {
            out.push((
                v.ordinal,
                SchemaEntry {
                    kind: "view".into(),
                    name: v.name.clone(),
                    table: v.name.clone(),
                    sql: Some(v.sql.clone()),
                },
            ));
        }
        for t in self.state.triggers.values() {
            if t.temp {
                continue;
            }
            let table = self
                .state
                .tables
                .get(&t.table)
                .map(|x| x.name.clone())
                .or_else(|| self.state.views.get(&t.table).map(|v| v.name.clone()))
                .unwrap_or_else(|| t.table.clone());
            out.push((
                t.ordinal,
                SchemaEntry {
                    kind: "trigger".into(),
                    name: t.name.clone(),
                    table,
                    sql: Some(t.sql.clone()),
                },
            ));
        }
        out.sort_by_key(|(o, _)| *o);
        out.into_iter().map(|(_, e)| e).collect()
    }
    /// Columns of a table or view.
    pub fn table_info(&self, name: &str) -> Option<Vec<ColumnInfo>> {
        let key = name.to_ascii_lowercase();
        if let Some(t) = self.state.tables.get(&key) {
            return Some(
                t.columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| ColumnInfo {
                        name: c.name.clone(),
                        decl_type: c.decl_type.clone(),
                        not_null: c.not_null,
                        default: c.default.clone(),
                        primary_key: t
                            .primary_key
                            .iter()
                            .position(|p| *p == i)
                            .map_or(0, |p| p + 1),
                    })
                    .collect(),
            );
        }
        if self.state.views.contains_key(&key) {
            let mut scratch = self.clone();
            let out = scratch
                .execute_one(
                    &format!(
                        "SELECT * FROM {} LIMIT 0",
                        value::Value::Text(name.into()).quoted().replace('\'', "\"")
                    ),
                    &[],
                )
                .ok()?;
            return Some(
                out.columns
                    .into_iter()
                    .map(|n| ColumnInfo {
                        name: n,
                        decl_type: String::new(),
                        not_null: false,
                        default: None,
                        primary_key: 0,
                    })
                    .collect(),
            );
        }
        None
    }
    /// Number of rows in a table, without running a query.
    pub fn row_count(&self, table: &str) -> Option<usize> {
        self.state
            .tables
            .get(&table.to_ascii_lowercase())
            .map(|t| t.rows.len())
    }
    /// Whether `name` is a table (rather than a view).
    pub fn is_table(&self, name: &str) -> bool {
        self.state.tables.contains_key(&name.to_ascii_lowercase())
    }
    /// Whether `name` is a table with a rowid (not a view or a WITHOUT ROWID table).
    pub fn has_rowid(&self, name: &str) -> bool {
        self.state
            .tables
            .get(&name.to_ascii_lowercase())
            .is_some_and(|t| !t.without_rowid)
    }
    /// Triggers on a table or view: (name, CREATE TRIGGER text), oldest first.
    pub fn triggers_of(&self, table: &str) -> Vec<(String, String)> {
        let mut v: Vec<(u64, String, String)> = self
            .state
            .triggers
            .values()
            .filter(|t| t.table.eq_ignore_ascii_case(table))
            .map(|t| (t.ordinal, t.name.clone(), t.sql.clone()))
            .collect();
        v.sort();
        v.into_iter().map(|(_, n, s)| (n, s)).collect()
    }
}
fn with_ctes(
    state: &State,
    env: &Env,
    params: &[Value],
    with: &Option<ast::With>,
) -> Result<eval::Ctes, SqlError> {
    let Some(with) = with else {
        return Ok(Rc::new(Vec::new()));
    };
    let ctx = Ctx {
        state,
        params,
        env,
        ctes: Rc::new(Vec::new()),
        plan: None,
        depth: 0,
        shape: None,
    };
    // Each CTE is materialised by selecting everything from it, in a context that
    // already holds the ones before it.
    let mut list = Vec::new();
    let mut ctx2 = ctx.clone();
    for cte in &with.ctes {
        let one = ast::Select {
            with: Some(ast::With {
                recursive: with.recursive,
                ctes: vec![cte.clone()],
            }),
            first: ast::SelectCore::Select {
                distinct: false,
                columns: vec![ast::ResultCol::Star],
                from: Some(ast::FromItem::Table {
                    name: cte.name.clone(),
                    alias: None,
                }),
                filter: None,
                group_by: vec![],
                having: None,
            },
            compounds: vec![],
            order_by: vec![],
            limit: None,
            offset: None,
        };
        let rel = exec::select(&ctx2, &one, None)?;
        list.push((cte.name.to_ascii_lowercase(), Rc::new(rel)));
        ctx2.ctes = Rc::new(list.clone());
    }
    Ok(Rc::new(list))
}
/// The SELECT part of a stored `CREATE VIEW` text.
pub(crate) fn view_select_text(sql: &str) -> String {
    let Ok(toks) = lexer::tokenize(sql) else {
        return String::new();
    };
    // CREATE VIEW name [(cols)] AS <select>
    let mut depth = 0;
    for t in &toks {
        match t.tok {
            lexer::Tok::Op("(") => depth += 1,
            lexer::Tok::Op(")") => depth -= 1,
            _ => {}
        }
        if depth == 0 && t.keyword().as_deref() == Some("AS") {
            return sql[t.end..].trim().to_owned();
        }
    }
    String::new()
}
/// Rows of `sqlite_schema`: type, name, tbl_name, rootpage, sql.
pub(crate) fn schema_rows(state: &State) -> Vec<Vec<Value>> {
    let db = Database {
        state: state.clone(),
        ..Database::default()
    };
    let roots = file::root_pages(state);
    db.schema()
        .into_iter()
        .map(|e| {
            let root = if e.kind == "table" || e.kind == "index" {
                roots
                    .get(&e.name.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0)
            } else {
                0
            };
            vec![
                Value::Text(e.kind),
                Value::Text(e.name),
                Value::Text(e.table),
                Value::Integer(root as i64),
                e.sql.map_or(Value::Null, Value::Text),
            ]
        })
        .collect()
}
