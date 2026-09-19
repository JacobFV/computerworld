//! Tables, indexes and views as the engine stores them, and the database state that a
//! transaction snapshots. Tables and indexes sit behind `Arc` so a snapshot is a cheap
//! copy and a statement only clones what it actually changes.
use crate::ast::{FkAction, TableConstraint};
use crate::value::{fold, Affinity, Collation, Key, Value};
use crate::SqlError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub decl_type: String,
    pub affinity: Affinity,
    pub not_null: bool,
    /// Source text of the DEFAULT expression.
    pub default: Option<String>,
    pub collation: Collation,
    pub primary_key: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub columns: Vec<usize>,
    pub parent: String,
    /// Empty means the parent's primary key.
    pub parent_columns: Vec<String>,
    pub on_delete: FkAction,
    pub on_update: FkAction,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub sql: String,
    pub columns: Vec<Column>,
    /// The INTEGER PRIMARY KEY column, which is the rowid itself.
    pub ipk: Option<usize>,
    pub autoincrement: bool,
    pub primary_key: Vec<usize>,
    pub checks: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
    pub rows: BTreeMap<i64, Vec<Value>>,
    pub temp: bool,
    pub ordinal: u64,
    /// A `WITHOUT ROWID` table: its rows are keyed by the primary key. The engine
    /// still numbers them internally, but no rowid is visible and the file stores
    /// the table as an index B-tree in primary-key order.
    #[serde(default)]
    pub without_rowid: bool,
    /// Each primary key column's collation and direction, in key order.
    #[serde(default)]
    pub pk_order: Vec<(Collation, bool)>,
}
impl Table {
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }
    /// A stored row with the rowid alias filled in.
    pub fn full_row(&self, rowid: i64, stored: &[Value]) -> Vec<Value> {
        let mut row = stored.to_vec();
        row.resize(self.columns.len(), Value::Null);
        if let Some(i) = self.ipk {
            row[i] = Value::Integer(rowid);
        }
        row
    }
    pub fn is_rowid_name(&self, name: &str) -> bool {
        !self.without_rowid
            && ["rowid", "oid", "_rowid_"]
                .iter()
                .any(|r| r.eq_ignore_ascii_case(name))
            && self.column(name).is_none()
    }
    /// Stored rows in the order a full scan visits them: rowid order, or primary key
    /// order (with each key column's collation and direction) for a WITHOUT ROWID table.
    pub fn scan_ids(&self) -> Vec<i64> {
        let mut ids: Vec<i64> = self.rows.keys().copied().collect();
        if self.without_rowid {
            let key: Vec<(usize, Collation, bool)> = self
                .primary_key
                .iter()
                .zip(&self.pk_order)
                .map(|(c, (coll, desc))| (*c, *coll, *desc))
                .collect();
            ids.sort_by(|a, b| {
                let (ra, rb) = (&self.rows[a], &self.rows[b]);
                for (c, coll, desc) in &key {
                    let o = crate::value::compare(
                        ra.get(*c).unwrap_or(&Value::Null),
                        rb.get(*c).unwrap_or(&Value::Null),
                        *coll,
                    );
                    let o = if *desc { o.reverse() } else { o };
                    if o != std::cmp::Ordering::Equal {
                        return o;
                    }
                }
                a.cmp(b)
            });
        }
        ids
    }
    /// A WITHOUT ROWID table's record order in the file: the primary key columns,
    /// then every other column in table order.
    pub fn record_columns(&self) -> Vec<usize> {
        let mut out = self.primary_key.clone();
        out.extend((0..self.columns.len()).filter(|c| !self.primary_key.contains(c)));
        out
    }
}
/// SQLite's estimate of a column's width (`szEst`), scaled so an integer is 1: text and
/// blob columns count as about 20 bytes unless their type gives a length.
pub fn size_estimate(decl_type: &str) -> u32 {
    if decl_type.trim().is_empty() {
        return 1;
    }
    let lower = decl_type.to_ascii_lowercase();
    let aff = Affinity::from_type(decl_type);
    let v = if matches!(aff, Affinity::Text | Affinity::Blob) {
        // Only CHAR types (and BLOB(n)) read a length; others are assumed 16 bytes.
        let digits_after = |at: usize| -> u32 {
            let rest = &lower[at..];
            let start = rest.find(|c: char| c.is_ascii_digit());
            start.map_or(0, |s| {
                rest[s..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
        };
        if let Some(i) = lower.find("char") {
            digits_after(i + 4)
        } else if let Some(i) = lower.find("blob") {
            if lower[i + 4..].trim_start().starts_with('(') {
                digits_after(i + 4)
            } else {
                16
            }
        } else {
            16
        }
    } else {
        0
    };
    (v / 4 + 1).min(255)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexOrigin {
    Created,
    Unique,
    PrimaryKey,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexColumn {
    pub column: usize,
    pub collation: Collation,
    pub desc: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub name: String,
    /// Lower-case key of the table in `State::tables`.
    pub table: String,
    pub columns: Vec<IndexColumn>,
    pub unique: bool,
    pub origin: IndexOrigin,
    /// `None` for the automatic indexes behind UNIQUE and PRIMARY KEY constraints.
    pub sql: Option<String>,
    pub ordinal: u64,
    /// Collation-folded key values followed by the rowid.
    #[serde(with = "entry_set")]
    pub entries: BTreeSet<Key>,
}
mod entry_set {
    use super::Key;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeSet;
    pub fn serialize<S: Serializer>(set: &BTreeSet<Key>, s: S) -> Result<S::Ok, S::Error> {
        set.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeSet<Key>, D::Error> {
        Ok(Vec::<Key>::deserialize(d)?.into_iter().collect())
    }
}
impl Index {
    pub fn key_for(&self, row: &[Value], rowid: i64) -> Key {
        let mut k: Vec<Value> = self
            .columns
            .iter()
            .map(|c| fold(row[c.column].clone(), c.collation))
            .collect();
        k.push(Value::Integer(rowid));
        Key(k)
    }
    pub fn rebuild(&mut self, table: &Table) {
        self.entries = table
            .rows
            .iter()
            .map(|(id, stored)| self.key_for(&table.full_row(*id, stored), *id))
            .collect();
    }
    /// Rowids whose key columns equal `prefix` (already folded) on the leading columns.
    pub fn lookup_prefix(&self, prefix: &[Value]) -> Vec<i64> {
        let start = Key(prefix.to_vec());
        self.entries
            .range(start..)
            .take_while(|k| {
                k.0.iter()
                    .zip(prefix)
                    .all(|(a, b)| crate::value::same(a, b))
            })
            .filter_map(|k| match k.0.last() {
                Some(Value::Integer(id)) => Some(*id),
                _ => None,
            })
            .collect()
    }
    /// The rowid of another row that holds these unique key values, if any. NULLs are
    /// distinct from each other, so a key containing one never conflicts.
    pub fn conflict(&self, row: &[Value], rowid: i64) -> Option<i64> {
        if !self.unique {
            return None;
        }
        let prefix: Vec<Value> = self
            .columns
            .iter()
            .map(|c| fold(row[c.column].clone(), c.collation))
            .collect();
        if prefix.iter().any(Value::is_null) {
            return None;
        }
        self.lookup_prefix(&prefix)
            .into_iter()
            .find(|id| *id != rowid)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    pub sql: String,
    /// The SELECT text, reparsed when the view is used.
    pub select: String,
    pub columns: Vec<String>,
    pub ordinal: u64,
}

/// A trigger as the schema keeps it: its `CREATE TRIGGER` text, parsed again when it
/// fires, and the table or view it watches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub name: String,
    /// Lower-case key of the table (or view) in `State`.
    pub table: String,
    pub sql: String,
    pub ordinal: u64,
    pub temp: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct State {
    pub tables: BTreeMap<String, Arc<Table>>,
    pub indexes: BTreeMap<String, Arc<Index>>,
    pub views: BTreeMap<String, View>,
    #[serde(default)]
    pub triggers: BTreeMap<String, Trigger>,
    pub next_ordinal: u64,
    pub user_version: i64,
    pub foreign_keys: bool,
    pub schema_cookie: u32,
    pub change_counter: u32,
}
impl State {
    pub fn table(&self, name: &str) -> Result<&Arc<Table>, SqlError> {
        self.tables
            .get(&name.to_ascii_lowercase())
            .ok_or_else(|| SqlError::new(format!("no such table: {name}")))
    }
    pub fn table_mut(&mut self, name: &str) -> Result<&mut Table, SqlError> {
        let t = self
            .tables
            .get_mut(&name.to_ascii_lowercase())
            .ok_or_else(|| SqlError::new(format!("no such table: {name}")))?;
        Ok(Arc::make_mut(t))
    }
    pub fn indexes_of(&self, table: &str) -> Vec<Arc<Index>> {
        let key = table.to_ascii_lowercase();
        let mut v: Vec<_> = self
            .indexes
            .values()
            .filter(|i| i.table == key)
            .cloned()
            .collect();
        v.sort_by_key(|i| i.ordinal);
        v
    }
    pub fn name_taken(&self, name: &str) -> Option<&'static str> {
        let key = name.to_ascii_lowercase();
        if self.tables.contains_key(&key) {
            Some("table")
        } else if self.indexes.contains_key(&key) {
            Some("index")
        } else if self.views.contains_key(&key) {
            Some("view")
        } else {
            None
        }
    }
    /// Triggers on a table or view, most recently created first (the order they fire).
    pub fn triggers_on(&self, table: &str) -> Vec<&Trigger> {
        let key = table.to_ascii_lowercase();
        let mut v: Vec<&Trigger> = self.triggers.values().filter(|t| t.table == key).collect();
        v.sort_by_key(|t| std::cmp::Reverse(t.ordinal));
        v
    }
    pub fn ordinal(&mut self) -> u64 {
        self.next_ordinal += 1;
        self.next_ordinal
    }
    pub fn touch_schema(&mut self) {
        self.schema_cookie = self.schema_cookie.wrapping_add(1);
    }
}

/// A constrained column: its position, explicit collation and direction.
type KeyColumn = (usize, Option<String>, bool);

/// Build a table (and the automatic indexes its constraints need) from a definition.
pub fn build_table(
    state: &mut State,
    ct: &crate::ast::CreateTable,
    sql: String,
) -> Result<(Table, Vec<Index>), SqlError> {
    if ct.columns.is_empty() {
        return Err(SqlError::new("a table needs at least one column"));
    }
    let mut columns = Vec::new();
    let mut seen = BTreeSet::new();
    let mut checks = Vec::new();
    let mut fks = Vec::new();
    let mut pk: Option<(Vec<KeyColumn>, bool)> = None;
    let mut uniques: Vec<Vec<KeyColumn>> = Vec::new();
    // Automatic indexes are numbered in the order their constraints appear.
    let mut auto_order: Vec<(bool, usize)> = Vec::new();
    for (i, c) in ct.columns.iter().enumerate() {
        if !seen.insert(c.name.to_ascii_lowercase()) {
            return Err(SqlError::new(format!("duplicate column name: {}", c.name)));
        }
        if c.generated.is_some() {
            return Err(SqlError::new(
                "generated columns are not supported by this engine",
            ));
        }
        let collation = match &c.collation {
            Some(name) => Collation::parse(name)
                .ok_or_else(|| SqlError::new(format!("no such collation sequence: {name}")))?,
            None => Collation::Binary,
        };
        if let Some(spec) = &c.primary_key {
            if pk.is_some() {
                return Err(SqlError::new(format!(
                    "table \"{}\" has more than one primary key",
                    ct.name
                )));
            }
            pk = Some((vec![(i, None, spec.desc)], spec.autoincrement));
            auto_order.push((true, 0));
        }
        if c.unique {
            uniques.push(vec![(i, None, false)]);
            auto_order.push((false, uniques.len() - 1));
        }
        checks.extend(c.checks.iter().cloned());
        if let Some(r) = &c.references {
            fks.push(ForeignKey {
                columns: vec![i],
                parent: r.table.clone(),
                parent_columns: r.columns.clone(),
                on_delete: r.on_delete,
                on_update: r.on_update,
            });
        }
        columns.push(Column {
            name: c.name.clone(),
            decl_type: c.type_name.clone(),
            affinity: Affinity::from_type(&c.type_name),
            not_null: c.not_null,
            default: c.default.clone(),
            collation,
            primary_key: false,
        });
    }
    let find = |name: &str| -> Result<usize, SqlError> {
        ct.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| SqlError::new(format!("no such column: {name}")))
    };
    for constraint in &ct.constraints {
        match constraint {
            TableConstraint::PrimaryKey(cols, autoinc) => {
                if pk.is_some() {
                    return Err(SqlError::new(format!(
                        "table \"{}\" has more than one primary key",
                        ct.name
                    )));
                }
                let mut v = Vec::new();
                for c in cols {
                    v.push((find(&c.name)?, c.collation.clone(), c.desc));
                }
                pk = Some((v, *autoinc));
                auto_order.push((true, 0));
            }
            TableConstraint::Unique(cols) => {
                let mut v = Vec::new();
                for c in cols {
                    v.push((find(&c.name)?, c.collation.clone(), c.desc));
                }
                uniques.push(v);
                auto_order.push((false, uniques.len() - 1));
            }
            TableConstraint::Check(text) => checks.push(text.clone()),
            TableConstraint::ForeignKey(cols, spec) => {
                let mut v = Vec::new();
                for c in cols {
                    v.push(find(c)?);
                }
                if !spec.columns.is_empty() && spec.columns.len() != v.len() {
                    return Err(SqlError::new(
                        "number of columns in foreign key does not match the number of columns in the referenced table",
                    ));
                }
                fks.push(ForeignKey {
                    columns: v,
                    parent: spec.table.clone(),
                    parent_columns: spec.columns.clone(),
                    on_delete: spec.on_delete,
                    on_update: spec.on_update,
                });
            }
        }
    }
    let mut ipk = None;
    let mut autoincrement = false;
    let mut primary_key = Vec::new();
    let mut pk_order = Vec::new();
    if ct.without_rowid {
        match &pk {
            None => {
                return Err(SqlError::new(format!(
                    "PRIMARY KEY missing on table {}",
                    ct.name
                )))
            }
            Some((_, true)) => {
                return Err(SqlError::new(
                    "AUTOINCREMENT not allowed on WITHOUT ROWID tables",
                ))
            }
            Some(_) => {}
        }
    }
    if let Some((cols, _)) = &pk {
        for (c, coll, desc) in cols {
            let collation = match coll {
                Some(name) => Collation::parse(name)
                    .ok_or_else(|| SqlError::new(format!("no such collation sequence: {name}")))?,
                None => columns[*c].collation,
            };
            pk_order.push((collation, *desc));
        }
    }
    if let Some((cols, _)) = pk.as_ref().filter(|_| ct.without_rowid) {
        primary_key = cols.iter().map(|c| c.0).collect();
        for &(c, _, _) in cols {
            columns[c].primary_key = true;
            // Every primary key column of a WITHOUT ROWID table is NOT NULL.
            columns[c].not_null = true;
        }
    } else if let Some((cols, autoinc)) = &pk {
        primary_key = cols.iter().map(|c| c.0).collect();
        for &(c, _, _) in cols {
            columns[c].primary_key = true;
        }
        // Only a lone column declared exactly "INTEGER" (any case) aliases the rowid,
        // and a DESC column-constraint key does not (SQLite's documented quirk).
        let single_desc_column_key = ct
            .columns
            .iter()
            .any(|c| c.primary_key.as_ref().is_some_and(|p| p.desc));
        if cols.len() == 1
            && columns[cols[0].0].decl_type.eq_ignore_ascii_case("INTEGER")
            && !single_desc_column_key
        {
            ipk = Some(cols[0].0);
        }
        if *autoinc {
            if ipk.is_none() {
                return Err(SqlError::new(
                    "AUTOINCREMENT is only allowed on an INTEGER PRIMARY KEY",
                ));
            }
            autoincrement = true;
        }
    }
    let ordinal = state.ordinal();
    let table = Table {
        name: ct.name.clone(),
        sql,
        columns,
        ipk,
        autoincrement,
        primary_key,
        checks,
        foreign_keys: fks,
        rows: BTreeMap::new(),
        temp: ct.temporary,
        ordinal,
        without_rowid: ct.without_rowid,
        pk_order,
    };
    let mut indexes = Vec::new();
    let mut n = 0;
    for (is_pk, which) in auto_order {
        let (cols, origin) = if is_pk {
            if ipk.is_some() {
                continue;
            }
            (
                pk.as_ref().map(|p| p.0.clone()).unwrap_or_default(),
                IndexOrigin::PrimaryKey,
            )
        } else {
            (uniques[which].clone(), IndexOrigin::Unique)
        };
        n += 1;
        let mut icols = Vec::new();
        for (c, coll, desc) in cols {
            let collation = match coll {
                Some(name) => Collation::parse(&name)
                    .ok_or_else(|| SqlError::new(format!("no such collation sequence: {name}")))?,
                None => table.columns[c].collation,
            };
            icols.push(IndexColumn {
                column: c,
                collation,
                desc,
            });
        }
        let ordinal = state.ordinal();
        indexes.push(Index {
            name: format!("sqlite_autoindex_{}_{n}", table.name),
            table: table.name.to_ascii_lowercase(),
            columns: icols,
            unique: true,
            origin,
            sql: None,
            ordinal,
            entries: BTreeSet::new(),
        });
    }
    Ok((table, indexes))
}
