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
        ["rowid", "oid", "_rowid_"]
            .iter()
            .any(|r| r.eq_ignore_ascii_case(name))
            && self.column(name).is_none()
    }
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct State {
    pub tables: BTreeMap<String, Arc<Table>>,
    pub indexes: BTreeMap<String, Arc<Index>>,
    pub views: BTreeMap<String, View>,
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
    if ct.without_rowid {
        return Err(SqlError::new(
            "WITHOUT ROWID tables are not supported by this engine",
        ));
    }
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
    if let Some((cols, autoinc)) = &pk {
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
