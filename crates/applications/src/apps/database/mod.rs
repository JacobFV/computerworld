//! A SQLite database client on the `cw-sql` engine: DB Browser for SQLite on Ubuntu and
//! Windows, TablePlus on the Mac. Databases are real SQLite 3 files on the machine,
//! read and written byte for byte; edits stay pending in the open connection until
//! Write Changes (TablePlus: Commit) writes the whole file back, and Revert Changes
//! returns to what the file holds.
//!
//! Every control dispatches a `db:*` command handled by [`Client::command`] or is
//! painted disabled with the reason it cannot.
use super::push_bounded;
use crate::desktop_scene::DesktopTheme;
use crate::AppEffect;
use cw_sql::Value;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod design_view;
pub mod designer;
mod structure;
#[cfg(test)]
mod tests;
mod view;
pub use designer::{DesignFocus, Field, IndexDesign, TableDesign};

/// Clock origin: tick 0 of the world is 2026-09-17 09:00:00 UTC.
const EPOCH_UNIX_US: i64 = 1_789_635_600_000_000;
/// Rows a result or browse grid keeps from one query.
pub const RESULT_ROWS: usize = 1000;
/// Rows a page key moves.
const PAGE: usize = 20;
/// Longest SQL the editor holds.
const SQL_LIMIT: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    Structure,
    Browse,
    Pragmas,
    Execute,
}
impl Tab {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "structure" => Self::Structure,
            "browse" => Self::Browse,
            "pragmas" => Self::Pragmas,
            "execute" => Self::Execute,
            _ => return None,
        })
    }
}
/// Where typing goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    Grid,
    Sql,
    Filter(usize),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellEdit {
    pub row: usize,
    pub col: usize,
    pub text: String,
}
/// What the last Execute SQL run produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    /// Rows the statement returned, of which `rows` keeps the first [`RESULT_ROWS`].
    pub total: usize,
    pub message: String,
    pub error: bool,
}
/// The open-file dialog: what it is choosing a file for and the folder it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Open,
    Import,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub path: String,
    pub entries: Vec<String>,
    pub problem: Option<String>,
}
/// A question that has to be answered before anything else happens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialog {
    /// Closing a database with changes that were never written, and the command that
    /// asked (New, Open), which goes ahead once the question is answered.
    CloseChanges { then: Option<String> },
    /// Dropping a table.
    DropTable(String),
}
/// Rows of the Browse Data grid, as the current filters and sort select them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rows {
    pub columns: Vec<String>,
    /// Each row's rowid (none for a view) and values.
    pub rows: Vec<(Option<i64>, Vec<Value>)>,
    /// Rows that match the filters in all.
    pub total: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Client {
    pub db: Option<cw_sql::Database>,
    /// The database as the file holds it, for Revert Changes and the modified mark.
    pub saved: Option<cw_sql::Database>,
    pub path: Option<String>,
    pub name: String,
    /// A database file being read.
    pub loading: Option<String>,
    /// A CSV file being read for import.
    pub importing: Option<String>,
    pub tab: Tab,
    /// Structure tree nodes that are open: `tables`, `indexes`, `views`, `table:NAME`.
    pub expanded: BTreeSet<String>,
    pub tree_selected: Option<String>,
    /// The table or view Browse Data shows.
    pub table: Option<String>,
    /// First row shown in Browse Data.
    pub offset: usize,
    /// Column index and ascending.
    pub sort: Option<(usize, bool)>,
    pub filters: BTreeMap<usize, String>,
    /// Selected cell of Browse Data: row (in the filtered, sorted order) and column.
    pub cell: Option<(usize, usize)>,
    pub edit: Option<CellEdit>,
    pub focus: Focus,
    pub sql: String,
    /// Byte offset of the caret in `sql`.
    pub caret: usize,
    pub result: Option<ExecResult>,
    pub result_offset: usize,
    pub dialog_files: Option<Purpose>,
    pub folder: Folder,
    pub dialog: Option<Dialog>,
    pub message: Option<String>,
    pub menu: Option<String>,
    /// DB Browser's Edit Table Definition dialog, or TablePlus's staged structure.
    #[serde(default)]
    pub design: Option<TableDesign>,
    /// DB Browser's Edit Index Definition dialog (TablePlus's New Index).
    #[serde(default)]
    pub index_design: Option<IndexDesign>,
}

fn ext(path: &str) -> String {
    path.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}
/// Whether the client opens a file of this name as a database.
pub fn opens(name: &str) -> bool {
    matches!(ext(name).as_str(), "db" | "sqlite" | "sqlite3" | "db3")
}
fn importable(name: &str) -> bool {
    matches!(ext(name).as_str(), "csv" | "tsv" | "txt")
}
/// A name quoted as an SQL identifier.
pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
/// How a value reads in a grid cell.
pub fn shown(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Blob(b) => format!("BLOB ({} bytes)", b.len()),
        other => other.to_text().replace(['\n', '\r', '\t'], " "),
    }
}

impl Client {
    pub fn launch(argument: &str, window: u64) -> (Self, Vec<AppEffect>) {
        let is_file = opens(argument);
        let folder = if is_file {
            parent(argument).to_owned()
        } else {
            argument.trim_end_matches('/').to_owned()
        };
        let mut client = Self {
            db: None,
            saved: None,
            path: None,
            name: String::new(),
            loading: None,
            importing: None,
            tab: Tab::Structure,
            expanded: ["tables".to_string()].into_iter().collect(),
            tree_selected: None,
            table: None,
            offset: 0,
            sort: None,
            filters: BTreeMap::new(),
            cell: None,
            edit: None,
            focus: Focus::Grid,
            sql: String::new(),
            caret: 0,
            result: None,
            result_offset: 0,
            dialog_files: None,
            folder: Folder {
                path: if folder.is_empty() {
                    "Documents".into()
                } else {
                    folder
                },
                ..Folder::default()
            },
            dialog: None,
            message: None,
            menu: None,
            design: None,
            index_design: None,
        };
        let mut effects = vec![AppEffect::ListDirectory {
            window,
            tab: 0,
            path: client.folder.path.clone(),
        }];
        if is_file {
            client.loading = Some(argument.to_owned());
            client.name = file_name(argument).to_owned();
            effects.insert(
                0,
                AppEffect::ReadBytes {
                    window,
                    path: argument.to_owned(),
                },
            );
        }
        (client, effects)
    }
    /// Whether a text field has focus: the SQL editor, a filter box or a cell editor.
    pub fn accepts_text(&self) -> bool {
        if let Some(d) = &self.index_design {
            return matches!(d.focus, DesignFocus::Name | DesignFocus::Where);
        }
        if let Some(d) = &self.design {
            if !d.inline || self.tab == Tab::Structure {
                return matches!(d.focus, DesignFocus::Name | DesignFocus::Cell(..));
            }
        }
        self.dialog.is_none()
            && self.dialog_files.is_none()
            && (self.edit.is_some()
                || matches!(
                    (self.tab, self.focus),
                    (Tab::Execute, Focus::Sql) | (Tab::Browse, Focus::Filter(_))
                ))
    }
    pub fn modified(&self) -> bool {
        if self.staged_structure() {
            return true;
        }
        match (&self.db, &self.saved) {
            (Some(d), Some(s)) => !d.same_content(s),
            (Some(_), None) => true,
            _ => false,
        }
    }
    fn now(clock_us: u64) -> i64 {
        EPOCH_UNIX_US + clock_us as i64
    }
    fn db_mut(&mut self) -> Result<&mut cw_sql::Database, String> {
        self.db
            .as_mut()
            .ok_or_else(|| "no database is open".to_string())
    }
    /// Tables and views, in schema order.
    pub fn objects(&self) -> Vec<cw_sql::SchemaEntry> {
        self.db.as_ref().map(|d| d.schema()).unwrap_or_default()
    }
    pub fn tables(&self) -> Vec<String> {
        self.objects()
            .into_iter()
            .filter(|e| e.kind == "table" || e.kind == "view")
            .map(|e| e.name)
            .collect()
    }
    fn is_view(&self, name: &str) -> bool {
        self.db.as_ref().is_some_and(|d| !d.is_table(name))
    }

    // ----- files -----

    pub fn listed(&mut self, entries: Vec<String>) {
        self.folder.entries = entries;
        self.folder.problem = None;
    }
    pub fn listing_failed(&mut self, reason: &str) {
        self.folder.problem = Some(reason.to_owned());
    }
    /// Bytes this client asked for: a database to open or a CSV file to import.
    pub fn bytes(&mut self, path: &str, result: Result<Vec<u8>, String>, clock_us: u64) {
        if self.loading.as_deref() == Some(path) {
            self.loading = None;
            let opened = result.and_then(|b| cw_sql::Database::open(&b).map_err(|e| e.message));
            match opened {
                Ok(mut db) => {
                    db.set_now(Self::now(clock_us));
                    // DB Browser turns foreign key enforcement on for every database it
                    // opens; it is a connection setting, not part of the file.
                    let _ = db.execute("PRAGMA foreign_keys = ON");
                    self.saved = Some(db.clone());
                    self.db = Some(db);
                    self.path = Some(path.to_owned());
                    self.name = file_name(path).to_owned();
                    self.reset_views();
                    self.table = self.tables().into_iter().next();
                }
                Err(e) => {
                    self.message = Some(format!("Could not open database file.\nReason: {e}"));
                    if self.db.is_none() {
                        self.name.clear();
                    }
                }
            }
        } else if self.importing.as_deref() == Some(path) {
            self.importing = None;
            match result.and_then(|b| self.import_csv(path, &b)) {
                Ok(message) => self.message = Some(message),
                Err(e) => self.message = Some(format!("Import failed: {e}")),
            }
        }
    }
    fn reset_views(&mut self) {
        self.offset = 0;
        self.sort = None;
        self.filters.clear();
        self.cell = None;
        self.edit = None;
        self.result = None;
        self.result_offset = 0;
        self.tree_selected = None;
        self.focus = Focus::Grid;
    }
    pub fn saved(&mut self, path: &str, result: Result<(), String>) {
        match result {
            Ok(()) => {
                if Some(path) == self.path.as_deref() {
                    self.saved = self.db.clone();
                } else if ext(path) == "csv" {
                    self.message = Some(format!("Export completed.\n{path}"));
                }
                if parent(path) == self.folder.path
                    && !self.folder.entries.iter().any(|e| e == file_name(path))
                {
                    self.folder.entries.push(file_name(path).to_owned());
                    self.folder.entries.sort();
                }
            }
            Err(e) => self.message = Some(format!("{} could not be written: {e}", file_name(path))),
        }
    }
    fn write(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        self.commit_cell()?;
        // TablePlus: Commit runs the staged structure change first.
        if self.staged_structure() {
            // A refused change is reported and nothing is written.
            if self.apply_design().is_err() {
                return Ok(vec![]);
            }
        } else if self.design.as_ref().is_some_and(|d| d.inline) {
            self.design = None;
        }
        let db = self.db.as_ref().ok_or("no database is open")?;
        let path = self.path.clone().ok_or("the database has no file")?;
        let mut effects = Vec::new();
        if !parent(&path).is_empty() {
            effects.push(AppEffect::CreateDirectory {
                window,
                path: parent(&path).to_owned(),
            });
        }
        effects.push(AppEffect::WriteBytes {
            window,
            path,
            bytes: db.to_bytes(),
        });
        Ok(effects)
    }
    fn unique_name(&self, base: &str, extension: &str) -> String {
        let mut n = 1;
        loop {
            let name = if n == 1 {
                format!("{base}.{extension}")
            } else {
                format!("{base} {n}.{extension}")
            };
            if !self
                .folder
                .entries
                .iter()
                .any(|e| e.eq_ignore_ascii_case(&name))
            {
                return name;
            }
            n += 1;
        }
    }
    fn close_database(&mut self) {
        self.db = None;
        self.saved = None;
        self.path = None;
        self.name.clear();
        self.table = None;
        self.reset_views();
        self.dialog = None;
        self.design = None;
        self.index_design = None;
    }
    /// Create a table from a CSV file: named after the file, columns from its header
    /// row, typed INTEGER, REAL or TEXT by what the column holds. A table of that name
    /// with as many columns takes the rows instead.
    fn import_csv(&mut self, path: &str, bytes: &[u8]) -> Result<String, String> {
        let text = String::from_utf8_lossy(bytes);
        let sep = if ext(path) == "tsv" { '\t' } else { ',' };
        let mut records = cw_sql::csv::parse(&text, sep);
        if records.is_empty() {
            return Err("the file is empty".into());
        }
        let header = records.remove(0);
        let width = header.len();
        let table: String = stem(file_name(path))
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let db = self.db.as_mut().ok_or("open or create a database first")?;
        let existing = db.table_info(&table);
        let types: Vec<&str> = (0..width)
            .map(|c| {
                let cells = records
                    .iter()
                    .filter_map(|r| r.get(c))
                    .filter(|v| !v.is_empty());
                let mut kind = "INTEGER";
                let mut any = false;
                for v in cells {
                    any = true;
                    match cw_sql::value::exact_number(v.trim()) {
                        Some(Value::Integer(_)) => {}
                        Some(_) => {
                            if kind == "INTEGER" {
                                kind = "REAL";
                            }
                        }
                        None => return "TEXT",
                    }
                }
                if any {
                    kind
                } else {
                    "TEXT"
                }
            })
            .collect();
        let mut script = String::new();
        match &existing {
            Some(cols) if cols.len() == width => {}
            Some(_) => {
                return Err(format!(
                    "there is already a table named {table} with a different number of columns"
                ))
            }
            None => {
                let mut names: Vec<String> = Vec::new();
                for (i, h) in header.iter().enumerate() {
                    let mut name = if h.trim().is_empty() {
                        format!("field{}", i + 1)
                    } else {
                        h.trim().to_owned()
                    };
                    while names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                        name.push('_');
                    }
                    names.push(name);
                }
                let cols: Vec<String> = names
                    .iter()
                    .zip(&types)
                    .map(|(n, t)| format!("{} {t}", ident(n)))
                    .collect();
                script = format!("CREATE TABLE {} ({})", ident(&table), cols.join(", "));
            }
        }
        // All or nothing: a row the table refuses leaves the database as it was.
        let before = db.clone();
        let result = (|| -> Result<usize, cw_sql::SqlError> {
            if !script.is_empty() {
                db.execute_one(&script, &[])?;
            }
            let marks = vec!["?"; width].join(", ");
            let insert = format!("INSERT INTO {} VALUES ({marks})", ident(&table));
            for r in &records {
                let params: Vec<Value> = (0..width)
                    .map(|c| {
                        let v = r.get(c).map(String::as_str).unwrap_or("");
                        if v.is_empty() && types[c] != "TEXT" {
                            Value::Null
                        } else {
                            Value::Text(v.to_owned())
                        }
                    })
                    .collect();
                db.execute_one(&insert, &params)?;
            }
            Ok(records.len())
        })();
        match result {
            Ok(n) => {
                self.table = Some(table.clone());
                self.tab = Tab::Browse;
                self.offset = 0;
                self.cell = None;
                self.filters.clear();
                self.sort = None;
                Ok(format!("Imported {n} rows into table {table}."))
            }
            Err(e) => {
                *db = before;
                Err(e.message)
            }
        }
    }
    fn export_csv(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let table = self.table.clone().ok_or("choose a table to export")?;
        let db = self.db.as_ref().ok_or("no database is open")?;
        let mut scratch = db.clone();
        let out = scratch
            .execute_one(&format!("SELECT * FROM {}", ident(&table)), &[])
            .map_err(|e| e.message)?;
        let mut text = String::new();
        let line = |fields: Vec<String>| {
            fields
                .iter()
                .map(|f| cw_sql::csv::quote(f, ","))
                .collect::<Vec<_>>()
                .join(",")
        };
        text.push_str(&line(out.columns.clone()));
        text.push('\n');
        for r in &out.rows {
            text.push_str(&line(r.iter().map(Value::to_text).collect()));
            text.push('\n');
        }
        let name = self.unique_name(&table, "csv");
        Ok(vec![AppEffect::WriteBytes {
            window,
            path: format!("{}/{name}", self.folder.path),
            bytes: text.into_bytes(),
        }])
    }

    // ----- Browse Data -----

    fn where_clause(&self, columns: &[String]) -> (String, Vec<Value>) {
        let mut parts = Vec::new();
        let mut params = Vec::new();
        for (c, text) in &self.filters {
            let Some(name) = columns.get(*c) else {
                continue;
            };
            let t = text.trim();
            if t.is_empty() {
                continue;
            }
            let ops = [">=", "<=", "<>", "!=", "=", ">", "<"];
            if let Some(op) = ops.iter().find(|op| t.starts_with(**op)) {
                let v = t[op.len()..].trim();
                parts.push(format!("{} {op} ?", ident(name)));
                params.push(
                    cw_sql::value::exact_number(v).unwrap_or_else(|| Value::Text(v.to_owned())),
                );
            } else {
                let escaped = t
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                parts.push(format!("{} LIKE ? ESCAPE '\\'", ident(name)));
                params.push(Value::Text(format!("%{escaped}%")));
            }
        }
        if parts.is_empty() {
            (String::new(), params)
        } else {
            (format!(" WHERE {}", parts.join(" AND ")), params)
        }
    }
    /// The Browse Data rows from `from`, at most `limit` of them.
    pub fn rows(&self, from: usize, limit: usize) -> Result<Rows, String> {
        let db = self.db.as_ref().ok_or("no database is open")?;
        let table = self.table.as_deref().ok_or("no table is chosen")?;
        let columns: Vec<String> = db
            .table_info(table)
            .ok_or_else(|| format!("no such table: {table}"))?
            .into_iter()
            .map(|c| c.name)
            .collect();
        let view = !db.is_table(table);
        // A WITHOUT ROWID table's rows are found and ordered by their primary key.
        let keyed = !view && !db.has_rowid(table);
        let (filter, params) = self.where_clause(&columns);
        let order = match self.sort {
            Some((c, asc)) if c < columns.len() => {
                format!(
                    " ORDER BY {} {}",
                    ident(&columns[c]),
                    if asc { "ASC" } else { "DESC" }
                )
            }
            _ if keyed => format!(" ORDER BY {}", Self::key_columns(db, table).join(", ")),
            _ if !view => " ORDER BY rowid".to_string(),
            _ => String::new(),
        };
        let mut scratch = db.clone();
        let total = scratch
            .execute_one(
                &format!("SELECT count(*) FROM {}{filter}", ident(table)),
                &params,
            )
            .map_err(|e| e.message)?
            .rows
            .first()
            .and_then(|r| r.first())
            .and_then(Value::to_i64)
            .unwrap_or(0) as usize;
        let select = if view || keyed { "*" } else { "rowid, *" };
        let out = scratch
            .execute_one(
                &format!(
                    "SELECT {select} FROM {}{filter}{order} LIMIT {limit} OFFSET {from}",
                    ident(table)
                ),
                &params,
            )
            .map_err(|e| e.message)?;
        let rows = out
            .rows
            .into_iter()
            .map(|mut r| {
                if view || keyed {
                    (None, r)
                } else {
                    let id = r.remove(0).to_i64();
                    (id, r)
                }
            })
            .collect();
        Ok(Rows {
            columns,
            rows,
            total,
        })
    }
    /// A table's primary key columns, quoted, in key order.
    fn key_columns(db: &cw_sql::Database, table: &str) -> Vec<String> {
        let mut cols: Vec<(usize, String)> = db
            .table_info(table)
            .unwrap_or_default()
            .into_iter()
            .filter(|c| c.primary_key > 0)
            .map(|c| (c.primary_key, ident(&c.name)))
            .collect();
        cols.sort();
        cols.into_iter().map(|(_, c)| c).collect()
    }
    /// The WHERE clause that finds one Browse Data row: by rowid, or by primary key in
    /// a WITHOUT ROWID table.
    fn locate(&self, row: usize) -> Result<(String, Vec<Value>), String> {
        let rows = self.rows(row, 1)?;
        let (rowid, values) = rows
            .rows
            .first()
            .cloned()
            .ok_or("that row is no longer there")?;
        if let Some(id) = rowid {
            return Ok(("rowid = ?".into(), vec![Value::Integer(id)]));
        }
        let db = self.db.as_ref().ok_or("no database is open")?;
        let table = self.table.as_deref().unwrap_or_default();
        let info = db.table_info(table).unwrap_or_default();
        let mut parts = Vec::new();
        let mut params = Vec::new();
        let mut keys: Vec<(usize, usize)> = info
            .iter()
            .enumerate()
            .filter(|(_, c)| c.primary_key > 0)
            .map(|(i, c)| (c.primary_key, i))
            .collect();
        keys.sort();
        for (_, i) in keys {
            parts.push(format!("{} = ?", ident(&info[i].name)));
            params.push(values.get(i).cloned().unwrap_or(Value::Null));
        }
        if parts.is_empty() {
            return Err("that row has no rowid".into());
        }
        Ok((parts.join(" AND "), params))
    }
    fn row_at(&self, row: usize) -> Result<(Option<i64>, Vec<Value>), String> {
        self.rows(row, 1)?
            .rows
            .into_iter()
            .next()
            .ok_or_else(|| "that row is no longer there".to_string())
    }
    fn why_read_only(&self) -> Option<String> {
        let table = self.table.as_deref()?;
        self.is_view(table)
            .then(|| format!("{table} is a view; its rows cannot be edited"))
    }
    fn begin_edit(&mut self, text: Option<String>) -> Result<(), String> {
        let (row, col) = self.cell.ok_or("select a cell first")?;
        if let Some(why) = self.why_read_only() {
            return Err(why);
        }
        let text = match text {
            Some(t) => t,
            None => {
                let (_, values) = self.row_at(row)?;
                match values.get(col) {
                    Some(Value::Null) | None => String::new(),
                    Some(Value::Blob(_)) => {
                        return Err("binary values cannot be edited as text".into())
                    }
                    Some(v) => v.to_text(),
                }
            }
        };
        self.edit = Some(CellEdit { row, col, text });
        self.focus = Focus::Grid;
        Ok(())
    }
    /// Write the cell editor into the row: an UPDATE by rowid, so the database's own
    /// type affinity and constraints decide what is stored.
    fn commit_cell(&mut self) -> Result<(), String> {
        let Some(edit) = self.edit.take() else {
            return Ok(());
        };
        self.set_cell(edit.row, edit.col, Value::Text(edit.text))
    }
    fn set_cell(&mut self, row: usize, col: usize, value: Value) -> Result<(), String> {
        if let Some(why) = self.why_read_only() {
            return Err(why);
        }
        let rows = self.rows(row, 1)?;
        let (found, key) = self.locate(row)?;
        let column = rows.columns.get(col).ok_or("no such column")?.clone();
        let table = self.table.clone().unwrap_or_default();
        let db = self.db_mut()?;
        let sql = format!(
            "UPDATE {} SET {} = ? WHERE {found}",
            ident(&table),
            ident(&column)
        );
        let mut params = vec![value];
        params.extend(key);
        if let Err(e) = db.execute_one(&sql, &params) {
            self.message = Some(format!("Error changing data:\n{}", e.message));
        }
        Ok(())
    }
    fn move_cell(&mut self, dr: i64, dc: i64) -> Result<(), String> {
        let rows = self.rows(0, 0)?;
        if rows.total == 0 || rows.columns.is_empty() {
            return Ok(());
        }
        let (r, c) = self.cell.unwrap_or((self.offset, 0));
        let r = (r as i64 + dr).clamp(0, rows.total as i64 - 1) as usize;
        let c = (c as i64 + dc).clamp(0, rows.columns.len() as i64 - 1) as usize;
        self.cell = Some((r, c));
        if r < self.offset {
            self.offset = r;
        } else if r >= self.offset + PAGE {
            self.offset = r + 1 - PAGE;
        }
        Ok(())
    }
    fn choose_table(&mut self, name: &str) -> Result<(), String> {
        if !self.tables().iter().any(|t| t == name) {
            return Err(format!("no such table: {name}"));
        }
        if self.staged_structure()
            && self.design.as_ref().and_then(|d| d.original.as_deref()) != Some(name)
        {
            let staged = self
                .design
                .as_ref()
                .and_then(|d| d.original.clone())
                .unwrap_or_default();
            self.message = Some(format!(
                "The structure of {staged} has changes that are not committed.\nCommit (⌘S) or discard them first."
            ));
            return Ok(());
        }
        if self.design.as_ref().is_some_and(|d| d.inline) && !self.staged_structure() {
            self.design = None;
        }
        self.commit_cell()?;
        if self.table.as_deref() != Some(name) {
            self.table = Some(name.to_owned());
            self.offset = 0;
            self.sort = None;
            self.filters.clear();
            self.cell = None;
        }
        self.tab = Tab::Browse;
        self.focus = Focus::Grid;
        Ok(())
    }

    // ----- Execute SQL -----

    /// Run `sql` statement by statement, stopping at the first error. Earlier
    /// statements keep their effect, as they do in DB Browser; the grid shows the rows
    /// of the last statement that returned any.
    fn run(&mut self, sql: &str, first_line: usize) -> Result<(), String> {
        let db = self.db.as_mut().ok_or("open or create a database first")?;
        let statements: Vec<(usize, String)> = cw_sql::lexer::split_statements(sql)
            .into_iter()
            .filter(|(_, s)| !cw_sql::lexer::is_blank(s))
            .collect();
        if statements.is_empty() {
            return Err("there is no SQL to execute".into());
        }
        let mut result = ExecResult {
            columns: vec![],
            rows: vec![],
            total: 0,
            message: String::new(),
            error: false,
        };
        let mut summary = String::new();
        for (start, text) in &statements {
            // The line the statement itself starts on, past any blank lines before it.
            let leading = text.len() - text.trim_start().len();
            let line = first_line
                + sql[..*start].matches('\n').count()
                + text[..leading].matches('\n').count();
            match db.execute_one(text, &[]) {
                Ok(out) => {
                    if !out.columns.is_empty() {
                        result.total = out.rows.len();
                        result.columns = out.columns;
                        result.rows = out.rows.into_iter().take(RESULT_ROWS).collect();
                        summary = format!(
                            "Result: {} row{} returned\nAt line {line}:\n{}",
                            result.total,
                            if result.total == 1 { "" } else { "s" },
                            text.trim()
                        );
                    } else {
                        summary = format!(
                            "Result: query executed successfully. {} row{} affected\nAt line {line}:\n{}",
                            out.changes,
                            if out.changes == 1 { "" } else { "s" },
                            text.trim()
                        );
                    }
                }
                Err(e) => {
                    result.error = true;
                    result.message = format!(
                        "Execution finished with errors.\nResult: {}\nAt line {line}:\n{}",
                        e.message,
                        text.trim()
                    );
                    break;
                }
            }
        }
        // The client keeps its own pending changes; a transaction the SQL opened and
        // left open folds into them, and Write Changes or Revert decides.
        if db.in_transaction() {
            let _ = db.execute("COMMIT");
            if !result.error {
                summary.push_str("\n(The open transaction was committed to the pending changes.)");
            }
        }
        if !result.error {
            result.message = format!("Execution finished without errors.\n{summary}");
        }
        self.result = Some(result);
        self.result_offset = 0;
        // Tables may have come or gone.
        if let Some(t) = &self.table {
            if !self.tables().contains(t) {
                self.table = self.tables().into_iter().next();
                self.cell = None;
                self.offset = 0;
            }
        } else {
            self.table = self.tables().into_iter().next();
        }
        Ok(())
    }
    fn current_line(&self) -> (usize, String) {
        let start = self.sql[..self.caret].rfind('\n').map_or(0, |i| i + 1);
        let end = self.sql[self.caret..]
            .find('\n')
            .map_or(self.sql.len(), |i| self.caret + i);
        let line = self.sql[..start].matches('\n').count() + 1;
        (line, self.sql[start..end].to_owned())
    }
    fn sql_key(&mut self, key: &str) -> Result<bool, String> {
        let s = &mut self.sql;
        let prev = |s: &str, at: usize| s[..at].char_indices().next_back().map_or(0, |(i, _)| i);
        let next = |s: &str, at: usize| s[at..].chars().next().map_or(at, |c| at + c.len_utf8());
        match key {
            "Enter" => {
                if s.len() < SQL_LIMIT {
                    s.insert(self.caret, '\n');
                    self.caret += 1;
                }
            }
            "Tab" => {
                if s.len() + 4 <= SQL_LIMIT {
                    s.insert_str(self.caret, "    ");
                    self.caret += 4;
                }
            }
            "Backspace" => {
                if self.caret > 0 {
                    let at = prev(s, self.caret);
                    s.drain(at..self.caret);
                    self.caret = at;
                }
            }
            "Delete" => {
                let to = next(s, self.caret);
                s.drain(self.caret..to);
            }
            "ArrowLeft" => self.caret = prev(s, self.caret),
            "ArrowRight" => self.caret = next(s, self.caret),
            "Home" => self.caret = s[..self.caret].rfind('\n').map_or(0, |i| i + 1),
            "End" => {
                self.caret = s[self.caret..]
                    .find('\n')
                    .map_or(s.len(), |i| self.caret + i)
            }
            "Ctrl+Home" => self.caret = 0,
            "Ctrl+End" => self.caret = s.len(),
            "ArrowUp" | "ArrowDown" => {
                let start = s[..self.caret].rfind('\n').map_or(0, |i| i + 1);
                let column = s[start..self.caret].chars().count();
                let target_start = if key == "ArrowUp" {
                    if start == 0 {
                        return Ok(true);
                    }
                    s[..start - 1].rfind('\n').map_or(0, |i| i + 1)
                } else {
                    match s[self.caret..].find('\n') {
                        Some(i) => self.caret + i + 1,
                        None => return Ok(true),
                    }
                };
                let line_end = s[target_start..]
                    .find('\n')
                    .map_or(s.len(), |i| target_start + i);
                self.caret = s[target_start..line_end]
                    .char_indices()
                    .nth(column)
                    .map_or(line_end, |(i, _)| target_start + i);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    // ----- input -----

    pub fn text(&mut self, text: &str) -> Result<(), String> {
        if self.loading.is_some() {
            return Err("the database is still opening".into());
        }
        if self.dialog.is_some() || self.dialog_files.is_some() {
            return Err("answer the dialog first".into());
        }
        if self.message.is_some() {
            return Err("dismiss the message first".into());
        }
        if let Some(r) = self.design_text(text) {
            return r;
        }
        match self.focus {
            Focus::Sql if self.tab == Tab::Execute => {
                let mut t = String::new();
                push_bounded(&mut t, text, SQL_LIMIT.saturating_sub(self.sql.len()));
                self.sql.insert_str(self.caret, &t);
                self.caret += t.len();
                Ok(())
            }
            Focus::Filter(c) if self.tab == Tab::Browse => {
                let f = self.filters.entry(c).or_default();
                push_bounded(f, text, 256);
                self.offset = 0;
                self.cell = None;
                Ok(())
            }
            _ if self.tab == Tab::Browse => {
                if let Some(e) = &mut self.edit {
                    push_bounded(&mut e.text, text, 64 * 1024);
                    return Ok(());
                }
                // Typing over a selected cell replaces its value, as in a spreadsheet.
                self.begin_edit(Some(String::new()))?;
                if let Some(e) = &mut self.edit {
                    push_bounded(&mut e.text, text, 64 * 1024);
                }
                Ok(())
            }
            _ => Err("click where the text should go first".into()),
        }
    }
    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        match (self.tab, self.focus) {
            (Tab::Execute, Focus::Sql) => {
                // The editor keeps line breaks that the typing path would drop.
                let mut t = String::new();
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        t.push('\n');
                    }
                    push_bounded(&mut t, line.trim_end_matches('\r'), SQL_LIMIT);
                }
                let room = SQL_LIMIT.saturating_sub(self.sql.len());
                let mut cut = t.len().min(room);
                while !t.is_char_boundary(cut) {
                    cut -= 1;
                }
                self.sql.insert_str(self.caret, &t[..cut]);
                self.caret += cut;
                Ok(())
            }
            _ => self.text(text.lines().next().unwrap_or("")),
        }
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        if let Some(db) = &mut self.db {
            db.set_now(Self::now(clock_us));
        }
        if self.loading.is_some() {
            return Err("the database is still opening".into());
        }
        let key = key.replace("Meta+", "Ctrl+");
        if self.message.is_some() && matches!(key.as_str(), "Enter" | "Escape") {
            self.message = None;
            return Ok(vec![]);
        }
        if let Some(r) = self.design_key(&key) {
            return r;
        }
        if self.designing() {
            return Err("finish the designer dialog first".into());
        }
        if self.dialog.is_some() || self.dialog_files.is_some() {
            if key == "Escape" {
                self.dialog = None;
                self.dialog_files = None;
                return Ok(vec![]);
            }
            return Err("answer the dialog first".into());
        }
        if key == "Escape" && self.menu.is_some() {
            self.menu = None;
            return Ok(vec![]);
        }
        let global = match key.as_str() {
            "Ctrl+s" => Some("write"),
            "Ctrl+o" => Some("open"),
            "Ctrl+n" => Some("new"),
            "Ctrl+w" | "Ctrl+F4" => Some("close"),
            "F5" | "Ctrl+Enter" | "Ctrl+r" if self.tab == Tab::Execute => Some("run"),
            "Shift+F5" if self.tab == Tab::Execute => Some("runline"),
            _ => None,
        };
        if let Some(command) = global {
            return self.command(window, command, clock_us);
        }
        if self.tab == Tab::Execute && self.focus == Focus::Sql {
            if key == "Ctrl+v" {
                return Ok(vec![AppEffect::Paste { window }]);
            }
            if self.sql_key(&key)? {
                return Ok(vec![]);
            }
            return Err(format!("unsupported editor key {key}"));
        }
        if self.tab != Tab::Browse {
            return Err(format!("unsupported key {key}"));
        }
        if let Focus::Filter(c) = self.focus {
            match key.as_str() {
                "Backspace" => {
                    if let Some(f) = self.filters.get_mut(&c) {
                        f.pop();
                    }
                }
                "Enter" | "Escape" | "Tab" => self.focus = Focus::Grid,
                other => return Err(format!("unsupported filter key {other}")),
            }
            return Ok(vec![]);
        }
        if let Some(e) = &mut self.edit {
            match key.as_str() {
                "Enter" | "Tab" => {
                    self.commit_cell()?;
                    if key == "Tab" {
                        self.move_cell(0, 1)?;
                    } else {
                        self.move_cell(1, 0)?;
                    }
                }
                "Escape" => self.edit = None,
                "Backspace" => {
                    e.text.pop();
                }
                "Ctrl+v" => return Ok(vec![AppEffect::Paste { window }]),
                other => return Err(format!("unsupported editing key {other}")),
            }
            return Ok(vec![]);
        }
        match key.as_str() {
            "ArrowUp" => self.move_cell(-1, 0)?,
            "ArrowDown" => self.move_cell(1, 0)?,
            "ArrowLeft" | "Shift+Tab" => self.move_cell(0, -1)?,
            "ArrowRight" | "Tab" => self.move_cell(0, 1)?,
            "PageDown" => self.move_cell(PAGE as i64, 0)?,
            "PageUp" => self.move_cell(-(PAGE as i64), 0)?,
            "Ctrl+Home" => {
                self.cell = Some((0, 0));
                self.offset = 0;
            }
            "F2" | "Enter" => self.begin_edit(None)?,
            // Set as NULL, as DB Browser's Alt+Del.
            "Delete" | "Alt+Delete" => {
                let (r, c) = self.cell.ok_or("select a cell first")?;
                self.set_cell(r, c, Value::Null)?;
            }
            "Ctrl+c" => {
                let (r, c) = self.cell.ok_or("select a cell first")?;
                let (_, values) = self.row_at(r)?;
                let text = values.get(c).map(Value::to_text).unwrap_or_default();
                return Ok(vec![AppEffect::CopyText { window, text }]);
            }
            "Escape" => self.cell = None,
            other => return Err(format!("unsupported key {other}")),
        }
        Ok(vec![])
    }

    // ----- commands -----

    pub fn command(
        &mut self,
        window: u64,
        command: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        if let Some(db) = &mut self.db {
            db.set_now(Self::now(clock_us));
        }
        if self.loading.is_some() && command != "dismiss" {
            return Err("the database is still opening".into());
        }
        let (verb, arg) = command.split_once(':').unwrap_or((command, ""));
        match verb {
            "menu" => {
                self.menu = if self.menu.as_deref() == Some(arg) {
                    None
                } else {
                    Some(arg.to_owned())
                };
                return Ok(vec![]);
            }
            "dismiss" => {
                self.message = None;
                self.menu = None;
                return Ok(vec![]);
            }
            "noop" => return Ok(vec![]),
            _ => {}
        }
        if self.designing() && !matches!(verb, "design" | "index") {
            return Err("finish the designer dialog first".into());
        }
        self.menu = None;
        let needs_db = !matches!(
            verb,
            "new"
                | "open"
                | "openfile"
                | "folder"
                | "cancel"
                | "tab"
                | "sql"
                | "yes"
                | "no"
                | "discard"
                | "savechanges"
        );
        if needs_db && self.db.is_none() {
            return Err("open or create a database first".into());
        }
        if let Some(r) = self.structure_command(verb, arg) {
            return r;
        }
        match verb {
            "tab" => {
                self.commit_cell()?;
                self.tab = Tab::parse(arg).ok_or("no such tab")?;
                self.focus = if self.tab == Tab::Execute {
                    Focus::Sql
                } else {
                    Focus::Grid
                };
            }
            "new" => {
                self.commit_cell()?;
                if self.modified() {
                    self.dialog = Some(Dialog::CloseChanges {
                        then: Some("new".into()),
                    });
                    return Ok(vec![]);
                }
                let name = self.unique_name("Untitled", "db");
                let path = format!("{}/{name}", self.folder.path);
                let mut db = cw_sql::Database::new();
                db.set_now(Self::now(clock_us));
                let _ = db.execute("PRAGMA foreign_keys = ON");
                self.close_database();
                self.db = Some(db);
                self.path = Some(path.clone());
                self.name = name;
                // DB Browser creates the file at once; the empty database is a real
                // one-page SQLite file.
                self.tab = Tab::Execute;
                self.focus = Focus::Sql;
                return self.write(window);
            }
            "open" => {
                self.commit_cell()?;
                if self.modified() {
                    self.dialog = Some(Dialog::CloseChanges {
                        then: Some("open".into()),
                    });
                    return Ok(vec![]);
                }
                self.dialog_files = Some(Purpose::Open);
                return Ok(vec![AppEffect::ListDirectory {
                    window,
                    tab: 0,
                    path: self.folder.path.clone(),
                }]);
            }
            "import" => {
                self.commit_cell()?;
                self.dialog_files = Some(Purpose::Import);
                return Ok(vec![AppEffect::ListDirectory {
                    window,
                    tab: 0,
                    path: self.folder.path.clone(),
                }]);
            }
            "cancel" => {
                self.dialog_files = None;
                self.dialog = None;
            }
            "folder" => {
                let next = if arg == ".." {
                    parent(&self.folder.path).to_owned()
                } else {
                    if !self.folder.entries.contains(&format!("{arg}/")) {
                        return Err("that folder is not in the list".into());
                    }
                    format!("{}/{arg}", self.folder.path)
                };
                if next.is_empty() {
                    return Err("that is the top of the filesystem".into());
                }
                self.folder = Folder {
                    path: next.clone(),
                    ..Folder::default()
                };
                return Ok(vec![AppEffect::ListDirectory {
                    window,
                    tab: 0,
                    path: next,
                }]);
            }
            "openfile" => {
                let purpose = self.dialog_files.ok_or("no file dialog is open")?;
                if !self.folder.entries.iter().any(|e| e == arg) {
                    return Err("that file is not in the list".into());
                }
                let path = format!("{}/{arg}", self.folder.path);
                self.dialog_files = None;
                match purpose {
                    Purpose::Open => {
                        if self.modified() {
                            return Err(
                                "write or revert the changes to the open database first".into()
                            );
                        }
                        self.loading = Some(path.clone());
                    }
                    Purpose::Import => self.importing = Some(path.clone()),
                }
                return Ok(vec![AppEffect::ReadBytes { window, path }]);
            }
            "write" => return self.write(window),
            "revert" => {
                self.edit = None;
                if self.design.as_ref().is_some_and(|d| d.inline) {
                    self.design = None;
                }
                self.db = self.saved.clone();
                if let Some(t) = &self.table {
                    if !self.tables().contains(t) {
                        self.table = self.tables().into_iter().next();
                    }
                }
                self.cell = None;
            }
            "close" => {
                self.commit_cell()?;
                if self.modified() {
                    self.dialog = Some(Dialog::CloseChanges { then: None });
                } else {
                    self.close_database();
                }
            }
            "savechanges" | "discard" => {
                let then = match self.dialog.take() {
                    Some(Dialog::CloseChanges { then }) => then,
                    _ => None,
                };
                // Save, then close: the write goes out before the connection does.
                let mut effects = if verb == "savechanges" {
                    self.write(window)?
                } else {
                    vec![]
                };
                self.close_database();
                if let Some(next) = then {
                    effects.extend(self.command(window, &next, clock_us)?);
                }
                return Ok(effects);
            }
            "exportcsv" => return self.export_csv(window),
            "expand" => {
                if !self.expanded.remove(arg) {
                    self.expanded.insert(arg.to_owned());
                }
            }
            "tree" => self.tree_selected = Some(arg.to_owned()),
            "droptable" => {
                let name = match arg {
                    "" => self
                        .tree_selected
                        .as_deref()
                        .and_then(|s| s.strip_prefix("table:"))
                        .ok_or("select a table in the tree first")?
                        .to_owned(),
                    n => n.to_owned(),
                };
                if !self.tables().contains(&name) {
                    return Err(format!("no such table: {name}"));
                }
                self.dialog = Some(Dialog::DropTable(name));
            }
            "yes" => {
                if let Some(Dialog::DropTable(name)) = self.dialog.take() {
                    let kind = if self.is_view(&name) { "VIEW" } else { "TABLE" };
                    let db = self.db_mut()?;
                    if let Err(e) = db.execute_one(&format!("DROP {kind} {}", ident(&name)), &[]) {
                        self.message = Some(format!("Error deleting {name}:\n{}", e.message));
                    }
                    self.tree_selected = None;
                    if self.table.as_deref() == Some(name.as_str()) {
                        self.table = self.tables().into_iter().next();
                        self.cell = None;
                        self.offset = 0;
                    }
                }
            }
            "no" => self.dialog = None,
            "table" => self.choose_table(arg)?,
            "sort" => {
                let c: usize = arg.parse().map_err(|_| "no such column")?;
                self.commit_cell()?;
                // Ascending, then descending, then back to the table's own order.
                self.sort = match self.sort {
                    Some((s, true)) if s == c => Some((c, false)),
                    Some((s, false)) if s == c => None,
                    _ => Some((c, true)),
                };
                self.cell = None;
                self.offset = 0;
            }
            "filter" => {
                let c: usize = arg.parse().map_err(|_| "no such column")?;
                self.commit_cell()?;
                self.focus = Focus::Filter(c);
            }
            "clearfilters" => {
                self.filters.clear();
                self.sort = None;
                self.offset = 0;
                self.cell = None;
                self.focus = Focus::Grid;
            }
            "refresh" => {
                self.commit_cell()?;
            }
            "cell" => {
                let (r, c) = arg.split_once(':').ok_or("which cell?")?;
                let r: usize = r.parse().map_err(|_| "no such row")?;
                let c: usize = c.parse().map_err(|_| "no such column")?;
                self.commit_cell()?;
                self.cell = Some((r, c));
                self.focus = Focus::Grid;
            }
            "editcell" => {
                if let Some((r, c)) = arg.split_once(':') {
                    let r: usize = r.parse().map_err(|_| "no such row")?;
                    let c: usize = c.parse().map_err(|_| "no such column")?;
                    self.commit_cell()?;
                    self.cell = Some((r, c));
                }
                self.begin_edit(None)?;
            }
            "setnull" => {
                let (r, c) = self.cell.ok_or("select a cell first")?;
                self.edit = None;
                self.set_cell(r, c, Value::Null)?;
            }
            "newrow" => {
                self.commit_cell()?;
                if let Some(why) = self.why_read_only() {
                    return Err(why);
                }
                let table = self.table.clone().ok_or("choose a table first")?;
                let db = self.db_mut()?;
                let keyed = !db.has_rowid(&table);
                let mut scratch = db.clone();
                // As DB Browser does: columns that must not be NULL and have no default
                // start as 0 or an empty string, everything else takes its default. A
                // WITHOUT ROWID table's key has no rowid to fall back on: a number key
                // takes the next number.
                let required: Vec<(String, Value)> = db
                    .table_info(&table)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|c| {
                        // An INTEGER PRIMARY KEY is the rowid, which SQLite assigns.
                        let rowid = !keyed
                            && c.primary_key > 0
                            && c.decl_type.eq_ignore_ascii_case("INTEGER");
                        (c.not_null || (keyed && c.primary_key > 0))
                            && c.default.is_none()
                            && !rowid
                    })
                    .map(|c| {
                        let numeric = cw_sql::Affinity::from_type(&c.decl_type).numeric();
                        let blank = if keyed && c.primary_key > 0 && numeric {
                            scratch
                                .query(&format!(
                                    "SELECT coalesce(max({}), 0) + 1 FROM {}",
                                    ident(&c.name),
                                    ident(&table)
                                ))
                                .ok()
                                .and_then(|o| o.rows.into_iter().next())
                                .and_then(|r| r.into_iter().next())
                                .unwrap_or(Value::Integer(1))
                        } else if numeric {
                            Value::Integer(0)
                        } else {
                            Value::Text(String::new())
                        };
                        (c.name, blank)
                    })
                    .collect();
                let db = self.db_mut()?;
                let sql = if required.is_empty() {
                    format!("INSERT INTO {} DEFAULT VALUES", ident(&table))
                } else {
                    let names: Vec<String> = required.iter().map(|(n, _)| ident(n)).collect();
                    let marks = vec!["?"; required.len()].join(", ");
                    format!(
                        "INSERT INTO {} ({}) VALUES ({marks})",
                        ident(&table),
                        names.join(", ")
                    )
                };
                let params: Vec<Value> = required.into_iter().map(|(_, v)| v).collect();
                match db.execute_one(&sql, &params) {
                    Ok(_) => {
                        // The new row is last in rowid order; clear the view so it shows.
                        self.filters.clear();
                        self.sort = None;
                        let total = self.rows(0, 0)?.total;
                        self.cell = Some((total.saturating_sub(1), 0));
                        self.offset = total.saturating_sub(PAGE);
                    }
                    Err(e) => self.message = Some(format!("Adding record failed:\n{}", e.message)),
                }
            }
            "deleterow" => {
                self.commit_cell()?;
                if let Some(why) = self.why_read_only() {
                    return Err(why);
                }
                let (r, _) = self.cell.ok_or("select a record first")?;
                let (found, key) = self.locate(r)?;
                let table = self.table.clone().unwrap_or_default();
                let db = self.db_mut()?;
                if let Err(e) = db.execute_one(
                    &format!("DELETE FROM {} WHERE {found}", ident(&table)),
                    &key,
                ) {
                    self.message = Some(format!("Deletion failed:\n{}", e.message));
                } else {
                    let total = self.rows(0, 0)?.total;
                    self.cell = if total == 0 {
                        None
                    } else {
                        Some((r.min(total - 1), 0))
                    };
                }
            }
            "page" => {
                let total = self.rows(0, 0)?.total;
                let last = total.saturating_sub(1) / PAGE * PAGE;
                self.offset = match arg {
                    "first" => 0,
                    "prev" => self.offset.saturating_sub(PAGE),
                    "next" => (self.offset + PAGE).min(last),
                    "last" => last,
                    _ => return Err("unknown page".into()),
                };
            }
            "sql" => {
                self.tab = Tab::Execute;
                self.focus = Focus::Sql;
                if arg == "start" {
                    self.caret = 0;
                } else {
                    self.caret = self.sql.len();
                }
            }
            "run" => {
                let sql = self.sql.clone();
                self.run(&sql, 1)?;
            }
            "runline" => {
                let (line, text) = self.current_line();
                self.run(&text, line)?;
            }
            "clearsql" => {
                self.sql.clear();
                self.caret = 0;
                self.focus = Focus::Sql;
            }
            "results" => {
                let total = self.result.as_ref().map_or(0, |r| r.rows.len());
                self.result_offset = match arg {
                    "up" => self.result_offset.saturating_sub(PAGE),
                    "down" => (self.result_offset + PAGE).min(total.saturating_sub(1)),
                    _ => return Err("unknown scroll".into()),
                };
            }
            "pragma" => {
                let sql = match arg {
                    "foreign_keys" => {
                        let on = self.db.as_ref().is_some_and(|d| d.foreign_keys());
                        format!("PRAGMA foreign_keys = {}", if on { "OFF" } else { "ON" })
                    }
                    "user_version:up" | "user_version:down" => {
                        let mut scratch = self.db.clone().unwrap_or_default();
                        let v = scratch
                            .query("PRAGMA user_version")
                            .ok()
                            .and_then(|o| {
                                o.rows
                                    .first()
                                    .and_then(|r| r.first())
                                    .and_then(Value::to_i64)
                            })
                            .unwrap_or(0);
                        let next = if arg.ends_with("up") {
                            v.saturating_add(1)
                        } else {
                            v.saturating_sub(1)
                        };
                        format!("PRAGMA user_version = {next}")
                    }
                    _ => return Err(format!("the {arg} pragma cannot be changed here")),
                };
                let db = self.db_mut()?;
                db.execute(&sql).map_err(|e| e.message)?;
            }
            "integrity" => {
                let mut scratch = self.db.clone().unwrap_or_default();
                let out = scratch
                    .query("PRAGMA integrity_check")
                    .map_err(|e| e.message)?;
                let lines: Vec<String> = out
                    .rows
                    .iter()
                    .filter_map(|r| r.first())
                    .map(Value::to_text)
                    .collect();
                self.message = Some(format!("Integrity check:\n{}", lines.join("\n")));
            }
            other => return Err(format!("unknown database command {other}")),
        }
        Ok(vec![])
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("db:")
            .ok_or("interaction does not belong to the database client")?;
        self.command(window, command, clock_us)
    }
    /// Double-click: edit a cell, browse a table from the structure tree, open a file.
    pub fn activate(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        if let Some(r) = self.design_activate(target) {
            return r;
        }
        if let Some(cell) = target.strip_prefix("db:cell:") {
            return self.command(window, &format!("editcell:{cell}"), clock_us);
        }
        if let Some(name) = target.strip_prefix("db:tree:table:") {
            self.choose_table(name)?;
            return Ok(vec![]);
        }
        self.click(window, target, clock_us)
    }
}

/// Whether this platform's client is TablePlus (the Mac) rather than DB Browser.
pub fn tableplus(theme: DesktopTheme) -> bool {
    theme == DesktopTheme::Macos
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Database(pub Box<Client>);
impl Database {
    pub const KIND: &'static str = "database";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let (client, effects) = Client::launch(argument, window);
        (Self(Box::new(client)), effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        if tableplus(theme) {
            "TablePlus".into()
        } else {
            match &self.0.path {
                // DB Browser titles itself with the database's full path.
                Some(path) if self.0.db.is_some() => format!("DB Browser for SQLite - {path}"),
                _ => "DB Browser for SQLite".into(),
            }
        }
    }
    pub fn document(&self) -> String {
        self.0.path.clone().unwrap_or_default()
    }
    pub fn caption(&self) -> String {
        self.0.name.clone()
    }
    pub fn modified(&self) -> bool {
        self.0.modified()
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        self.0.text(text)
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        self.0.key(window, key, clock_us)
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        self.0.click(window, target, clock_us)
    }
    pub fn activate(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        self.0.activate(window, target, clock_us)
    }
    pub fn http(
        &mut self,
        _window: u64,
        _tag: &str,
        _status: u16,
        _body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        Err("the database client works on local files and makes no requests".into())
    }
    pub fn offline(&mut self, tag: &str, reason: &str) {
        if tag == "listing" {
            self.0.listing_failed(reason);
        } else {
            self.0.message = Some(reason.to_owned());
        }
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        view::page(&self.0, page)
    }
    pub fn render(&self, p: &mut crate::desktop_scene::Painter, env: &crate::AppEnv<'_>) {
        view::render(&self.0, p, env)
    }
}
