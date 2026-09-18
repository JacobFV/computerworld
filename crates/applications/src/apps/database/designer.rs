//! The table and index designers: DB Browser's Edit Table Definition and Edit Index
//! Definition dialogs and TablePlus's structure editor, over one model. A design is
//! turned into the SQL SQLite itself needs: `CREATE TABLE`, the `ALTER TABLE` forms
//! SQLite supports (rename a table or a column, add or drop a column), and for every
//! other change the twelve-step rebuild of <https://www.sqlite.org/lang_altertable.html>
//! (new table, copy, drop, rename, then indexes, triggers and views put back).
use super::ident;
use cw_sql::ast::{FkAction, ForeignKeySpec, Stmt, TableConstraint};
use serde::{Deserialize, Serialize};

/// Where a column of the old table goes in the new one (none: it is dropped).
type ColumnMap<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// The types DB Browser's Type drop-down offers.
pub const TYPES: [&str; 5] = ["INTEGER", "TEXT", "BLOB", "REAL", "NUMERIC"];

/// One column of a design, as the Fields grid shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub not_null: bool,
    pub pk: bool,
    pub autoinc: bool,
    pub unique: bool,
    /// DEFAULT, as typed: a literal or expression is kept, other text becomes a string.
    pub default: String,
    pub check: String,
    pub collate: String,
    /// What follows REFERENCES: `"table"("column") ON DELETE …`.
    pub fk: String,
    /// `GENERATED ALWAYS AS (…)` text, kept as the table had it.
    #[serde(default)]
    pub generated: Option<String>,
    /// The column of the existing table this field is, for copying its data.
    pub origin: Option<String>,
}
impl Field {
    pub fn new(name: &str, ty: &str) -> Self {
        Self {
            name: name.into(),
            ty: ty.into(),
            ..Self::default()
        }
    }
    /// The text of one of the grid's text columns.
    pub fn text(&self, col: usize) -> &str {
        match col {
            0 => &self.name,
            1 => &self.ty,
            6 => &self.default,
            7 => &self.check,
            8 => &self.collate,
            9 => &self.fk,
            _ => "",
        }
    }
    pub fn text_mut(&mut self, col: usize) -> Option<&mut String> {
        Some(match col {
            0 => &mut self.name,
            1 => &mut self.ty,
            6 => &mut self.default,
            7 => &mut self.check,
            8 => &mut self.collate,
            9 => &mut self.fk,
            _ => return None,
        })
    }
}
/// The grid's columns: text (editable by typing) or a check box.
pub const COLUMNS: [(&str, bool); 10] = [
    ("Name", true),
    ("Type", true),
    ("NN", false),
    ("PK", false),
    ("AI", false),
    ("U", false),
    ("Default", true),
    ("Check", true),
    ("Collation", true),
    ("Foreign Key", true),
];

/// A table constraint the Fields grid has no column for: a primary key, unique or
/// foreign key over several columns, or a table CHECK.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraint {
    /// `UNIQUE`, `CHECK`, `FOREIGN KEY`.
    pub kind: String,
    pub columns: Vec<String>,
    /// CHECK's expression, or what follows REFERENCES.
    pub text: String,
}

/// Where typing goes in a designer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignFocus {
    Name,
    Cell(usize, usize),
    Where,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDesign {
    /// The table being modified; none for Create Table.
    pub original: Option<String>,
    pub name: String,
    pub fields: Vec<Field>,
    pub constraints: Vec<Constraint>,
    pub without_rowid: bool,
    pub selected: Option<usize>,
    pub focus: DesignFocus,
    /// Typing replaces the focused cell (it was selected, not opened for editing).
    #[serde(default)]
    pub replace: bool,
    /// TablePlus's structure view edits in place, staged until Commit; DB Browser's
    /// designer is a dialog.
    #[serde(default)]
    pub inline: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDesign {
    pub name: String,
    pub table: String,
    pub unique: bool,
    /// Indexed columns and whether each is descending.
    pub columns: Vec<(String, bool)>,
    /// A partial index's WHERE expression.
    pub filter: String,
    pub focus: DesignFocus,
}

fn action(a: FkAction) -> &'static str {
    match a {
        FkAction::NoAction => "NO ACTION",
        FkAction::Restrict => "RESTRICT",
        FkAction::SetNull => "SET NULL",
        FkAction::SetDefault => "SET DEFAULT",
        FkAction::Cascade => "CASCADE",
    }
}
fn references(fk: &ForeignKeySpec) -> String {
    let mut s = ident(&fk.table);
    if !fk.columns.is_empty() {
        let cols: Vec<String> = fk.columns.iter().map(|c| ident(c)).collect();
        s.push_str(&format!("({})", cols.join(",")));
    }
    if fk.on_delete != FkAction::NoAction {
        s.push_str(&format!(" ON DELETE {}", action(fk.on_delete)));
    }
    if fk.on_update != FkAction::NoAction {
        s.push_str(&format!(" ON UPDATE {}", action(fk.on_update)));
    }
    s
}
fn quoted_list(cols: &[String]) -> String {
    cols.iter().map(|c| ident(c)).collect::<Vec<_>>().join(",")
}
/// A DEFAULT as typed, made into SQL: literals and expressions stay, other text is a
/// string, as DB Browser does.
fn default_sql(text: &str) -> String {
    let t = text.trim();
    let upper = t.to_ascii_uppercase();
    let literal = t.parse::<f64>().is_ok()
        || t.starts_with('\'')
        || t.starts_with('(')
        || t.starts_with("x'")
        || t.starts_with("X'")
        || matches!(
            upper.as_str(),
            "NULL" | "TRUE" | "FALSE" | "CURRENT_TIME" | "CURRENT_DATE" | "CURRENT_TIMESTAMP"
        );
    if literal {
        t.to_owned()
    } else {
        format!("'{}'", t.replace('\'', "''"))
    }
}

impl TableDesign {
    pub fn new() -> Self {
        Self {
            original: None,
            name: String::new(),
            fields: Vec::new(),
            constraints: Vec::new(),
            without_rowid: false,
            selected: None,
            focus: DesignFocus::Name,
            replace: false,
            inline: false,
        }
    }
    /// The design of an existing table, read from its CREATE TABLE statement.
    pub fn of_table(db: &cw_sql::Database, table: &str) -> Result<Self, String> {
        let entry = db
            .schema()
            .into_iter()
            .find(|e| e.kind == "table" && e.name.eq_ignore_ascii_case(table))
            .ok_or_else(|| format!("no such table: {table}"))?;
        let sql = entry.sql.unwrap_or_default();
        let stmts = cw_sql::parser::parse(&sql).map_err(|e| e.message)?;
        let Some(Stmt::CreateTable(ct)) = stmts.into_iter().next() else {
            return Err(format!("{table} was not made by CREATE TABLE"));
        };
        let mut fields: Vec<Field> = ct
            .columns
            .iter()
            .map(|c| Field {
                name: c.name.clone(),
                ty: c.type_name.clone(),
                not_null: c.not_null,
                pk: c.primary_key.is_some(),
                autoinc: c.primary_key.as_ref().is_some_and(|p| p.autoincrement),
                unique: c.unique,
                default: c.default.clone().unwrap_or_default(),
                check: c.checks.join(" AND "),
                collate: c.collation.clone().unwrap_or_default(),
                fk: c.references.as_ref().map(references).unwrap_or_default(),
                generated: c.generated.clone(),
                origin: Some(c.name.clone()),
            })
            .collect();
        let find = |fields: &mut Vec<Field>, name: &str| -> Option<usize> {
            fields
                .iter()
                .position(|f| f.name.eq_ignore_ascii_case(name))
        };
        let mut constraints = Vec::new();
        for c in &ct.constraints {
            match c {
                TableConstraint::PrimaryKey(cols, autoinc) => {
                    for col in cols {
                        if let Some(i) = find(&mut fields, &col.name) {
                            fields[i].pk = true;
                            fields[i].autoinc |= *autoinc;
                        }
                    }
                }
                TableConstraint::Unique(cols) if cols.len() == 1 => {
                    if let Some(i) = find(&mut fields, &cols[0].name) {
                        fields[i].unique = true;
                    }
                }
                TableConstraint::Unique(cols) => constraints.push(Constraint {
                    kind: "UNIQUE".into(),
                    columns: cols.iter().map(|c| c.name.clone()).collect(),
                    text: String::new(),
                }),
                TableConstraint::Check(text) => constraints.push(Constraint {
                    kind: "CHECK".into(),
                    columns: vec![],
                    text: text.clone(),
                }),
                TableConstraint::ForeignKey(cols, fk) => {
                    let single = (cols.len() == 1)
                        .then(|| find(&mut fields, &cols[0]))
                        .flatten()
                        .filter(|i| fields[*i].fk.is_empty());
                    match single {
                        Some(i) => fields[i].fk = references(fk),
                        None => constraints.push(Constraint {
                            kind: "FOREIGN KEY".into(),
                            columns: cols.clone(),
                            text: references(fk),
                        }),
                    }
                }
            }
        }
        Ok(Self {
            original: Some(entry.name.clone()),
            name: entry.name,
            fields,
            constraints,
            without_rowid: ct.without_rowid,
            selected: None,
            focus: DesignFocus::None,
            replace: false,
            inline: false,
        })
    }
    /// A field name not yet used: `Field1`, `Field2`, …
    pub fn next_field_name(&self) -> String {
        (1..)
            .map(|n| format!("Field{n}"))
            .find(|n| !self.fields.iter().any(|f| f.name.eq_ignore_ascii_case(n)))
            .unwrap_or_default()
    }
    /// Why this design cannot be made, if it cannot.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Please enter a name for the table.".into());
        }
        if self.fields.is_empty() {
            return Err("A table needs at least one field.".into());
        }
        for (i, f) in self.fields.iter().enumerate() {
            if f.name.trim().is_empty() {
                return Err(format!("Field {} has no name.", i + 1));
            }
            if self.fields[..i]
                .iter()
                .any(|g| g.name.eq_ignore_ascii_case(&f.name))
            {
                return Err(format!(
                    "There already is a field with the name '{}'. Please rename it first or choose a different name for this field.",
                    f.name
                ));
            }
        }
        let pks: Vec<&Field> = self.fields.iter().filter(|f| f.pk).collect();
        if let Some(f) = self.fields.iter().find(|f| f.autoinc) {
            if pks.len() != 1 || !f.pk || !f.ty.eq_ignore_ascii_case("INTEGER") {
                return Err(format!(
                    "Column '{}' cannot be AUTOINCREMENT: only a single INTEGER PRIMARY KEY can be.",
                    f.name
                ));
            }
        }
        if self.without_rowid {
            if pks.is_empty() {
                return Err("Please add a field which meets the following criteria before setting the without rowid flag:\n - Primary key flag set".into());
            }
            if self.fields.iter().any(|f| f.autoinc) {
                return Err("A WITHOUT ROWID table cannot have an AUTOINCREMENT field.".into());
            }
        }
        Ok(())
    }
    fn column_sql(f: &Field) -> String {
        let mut s = ident(&f.name);
        if !f.ty.trim().is_empty() {
            s.push('\t');
            s.push_str(f.ty.trim());
        }
        if f.not_null {
            s.push_str(" NOT NULL");
        }
        if !f.default.trim().is_empty() {
            s.push_str(&format!(" DEFAULT {}", default_sql(&f.default)));
        }
        if f.unique {
            s.push_str(" UNIQUE");
        }
        if !f.check.trim().is_empty() {
            s.push_str(&format!(" CHECK({})", f.check.trim()));
        }
        if !f.collate.trim().is_empty() {
            s.push_str(&format!(" COLLATE {}", f.collate.trim()));
        }
        if let Some(g) = &f.generated {
            s.push_str(&format!(" GENERATED ALWAYS AS ({g})"));
        }
        s
    }
    /// The CREATE TABLE statement for this design under `name`, as DB Browser writes it.
    pub fn create_sql(&self, name: &str) -> String {
        let mut lines: Vec<String> = self.fields.iter().map(Self::column_sql).collect();
        let pks: Vec<&Field> = self.fields.iter().filter(|f| f.pk).collect();
        if !pks.is_empty() {
            let cols: Vec<String> = pks
                .iter()
                .map(|f| {
                    if f.autoinc {
                        format!("{} AUTOINCREMENT", ident(&f.name))
                    } else {
                        ident(&f.name)
                    }
                })
                .collect();
            lines.push(format!("PRIMARY KEY({})", cols.join(",")));
        }
        for f in &self.fields {
            if !f.fk.trim().is_empty() {
                lines.push(format!(
                    "FOREIGN KEY({}) REFERENCES {}",
                    ident(&f.name),
                    f.fk.trim()
                ));
            }
        }
        for c in &self.constraints {
            lines.push(match c.kind.as_str() {
                "CHECK" => format!("CHECK({})", c.text),
                "FOREIGN KEY" => format!(
                    "FOREIGN KEY({}) REFERENCES {}",
                    quoted_list(&c.columns),
                    c.text
                ),
                kind => format!("{kind}({})", quoted_list(&c.columns)),
            });
        }
        let body = lines
            .iter()
            .map(|l| format!("\t{l}"))
            .collect::<Vec<_>>()
            .join(",\n");
        format!(
            "CREATE TABLE {} (\n{body}\n){}",
            ident(name),
            if self.without_rowid {
                " WITHOUT ROWID"
            } else {
                ""
            }
        )
    }
    /// What the columns of the original table are called in this design.
    fn renames(&self) -> Vec<(String, String)> {
        self.fields
            .iter()
            .filter_map(|f| f.origin.clone().map(|o| (o, f.name.clone())))
            .collect()
    }
    /// The SQL that makes the database match this design, in order. For a new table,
    /// its CREATE TABLE; for an existing one, ALTER TABLE where SQLite can do the change
    /// that way and otherwise the twelve-step rebuild.
    pub fn statements(&self, db: &cw_sql::Database) -> Result<Vec<String>, String> {
        self.validate()?;
        let Some(original) = &self.original else {
            return Ok(vec![format!("{};", self.create_sql(&self.name))]);
        };
        let before = Self::of_table(db, original)?;
        if let Some(simple) = self.simple_alter(&before) {
            // SQLite refuses some of these (a column in an index cannot be dropped);
            // try them on a copy, and rebuild when it says no.
            let mut scratch = db.clone();
            if simple.iter().all(|s| scratch.execute_one(s, &[]).is_ok()) {
                return Ok(simple);
            }
        }
        Ok(self.rebuild(db, original))
    }
    /// The change as ALTER TABLE statements, when it is only renames, columns added at
    /// the end, or columns dropped.
    fn simple_alter(&self, before: &TableDesign) -> Option<Vec<String>> {
        let original = before.name.clone();
        let same = |a: &Field, b: &Field| {
            let mut a = a.clone();
            a.name = b.name.clone();
            a == *b
        };
        if self.constraints != before.constraints || self.without_rowid != before.without_rowid {
            return None;
        }
        let mut out = Vec::new();
        let kept: Vec<&Field> = self.fields.iter().filter(|f| f.origin.is_some()).collect();
        let added: Vec<&Field> = self.fields.iter().filter(|f| f.origin.is_none()).collect();
        // Added fields must all come after the kept ones.
        if self
            .fields
            .iter()
            .take(kept.len())
            .any(|f| f.origin.is_none())
        {
            return None;
        }
        // The kept fields, in the original order, unchanged but for their names.
        let mut prev = None;
        for f in &kept {
            let at = before
                .fields
                .iter()
                .position(|b| Some(&b.name) == f.origin.as_ref())?;
            if prev.is_some_and(|p| at <= p) || !same(&before.fields[at], f) {
                return None;
            }
            prev = Some(at);
        }
        let dropped: Vec<&Field> = before
            .fields
            .iter()
            .filter(|b| !kept.iter().any(|f| f.origin.as_ref() == Some(&b.name)))
            .collect();
        if !dropped.is_empty() && !added.is_empty() {
            return None;
        }
        let mut table = original.clone();
        if !self.name.eq(&original) {
            out.push(format!(
                "ALTER TABLE {} RENAME TO {};",
                ident(&original),
                ident(&self.name)
            ));
            table = self.name.clone();
        }
        for f in &kept {
            let from = f.origin.as_ref()?;
            if from != &f.name {
                out.push(format!(
                    "ALTER TABLE {} RENAME COLUMN {} TO {};",
                    ident(&table),
                    ident(from),
                    ident(&f.name)
                ));
            }
        }
        for d in &dropped {
            out.push(format!(
                "ALTER TABLE {} DROP COLUMN {};",
                ident(&table),
                ident(&d.name)
            ));
        }
        for a in &added {
            // ADD COLUMN takes no PRIMARY KEY or UNIQUE, and NOT NULL needs a default.
            if a.pk || a.unique || (a.not_null && a.default.trim().is_empty()) {
                return None;
            }
            let mut col = Self::column_sql(a).replace('\t', " ");
            if !a.fk.trim().is_empty() {
                col.push_str(&format!(" REFERENCES {}", a.fk.trim()));
            }
            out.push(format!("ALTER TABLE {} ADD COLUMN {col};", ident(&table)));
        }
        Some(out)
    }
    /// SQLite's twelve steps for any other schema change.
    fn rebuild(&self, db: &cw_sql::Database, original: &str) -> Vec<String> {
        let fk_on = db.foreign_keys();
        let renames = self.renames();
        let renamed = |c: &str| -> Option<String> {
            renames
                .iter()
                .find(|(o, _)| o.eq_ignore_ascii_case(c))
                .map(|(_, n)| n.clone())
        };
        let temp = (1..)
            .map(|n| format!("sqlb_temp_table_{n}"))
            .find(|n| !db.is_table(n))
            .unwrap_or_default();
        let mut out = Vec::new();
        // 1. Foreign keys off (it cannot change inside a transaction).
        if fk_on {
            out.push("PRAGMA foreign_keys = 0;".to_string());
        }
        // 2. A transaction.
        out.push("BEGIN TRANSACTION;".into());
        // 3. The indexes, triggers and views that belong to the table.
        let schema = db.schema();
        let indexes: Vec<String> = schema
            .iter()
            .filter(|e| e.kind == "index" && e.table.eq_ignore_ascii_case(original))
            .filter_map(|e| e.sql.clone())
            .collect();
        let triggers: Vec<String> = db
            .triggers_of(original)
            .into_iter()
            .map(|(_, s)| s)
            .collect();
        let lower = original.to_ascii_lowercase();
        let views: Vec<(String, String)> = schema
            .iter()
            .filter(|e| e.kind == "view")
            .filter(|e| {
                e.sql
                    .as_deref()
                    .is_some_and(|s| s.to_ascii_lowercase().contains(&lower))
            })
            .map(|e| (e.name.clone(), e.sql.clone().unwrap_or_default()))
            .collect();
        for (v, _) in &views {
            out.push(format!("DROP VIEW {};", ident(v)));
        }
        // 4. The new table, under a temporary name. Renamed columns keep their old
        // names through the rebuild and are renamed at the end with RENAME COLUMN, so
        // the views, triggers and indexes that name them follow as SQLite rewrites
        // them. When the names cross (a takes b's name) that cannot work, and the new
        // table is made under the new names directly.
        let changed: Vec<(String, String)> =
            renames.iter().filter(|(o, n)| o != n).cloned().collect();
        let back: Vec<(String, String)> = changed
            .iter()
            .map(|(o, n)| (n.clone(), o.clone()))
            .collect();
        let before = Self::of_table(db, original).ok();
        let original_check = |f: &Field| -> Option<String> {
            let b = before.as_ref()?;
            let o = f.origin.as_deref()?;
            b.fields
                .iter()
                .find(|x| x.name == o)
                .map(|x| x.check.clone())
        };
        let mut inter = self.clone();
        for f in &mut inter.fields {
            if original_check(f).as_deref() != Some(f.check.as_str()) {
                f.check = renamed_text(&f.check, &back);
            }
            if let Some(o) = &f.origin {
                f.name = o.clone();
            }
        }
        let unchanged_constraints = before
            .as_ref()
            .is_some_and(|b| b.constraints == self.constraints);
        for c in &mut inter.constraints {
            if !unchanged_constraints {
                if c.kind == "CHECK" {
                    c.text = renamed_text(&c.text, &back);
                }
                for col in &mut c.columns {
                    if let Some((_, o)) = back.iter().find(|(n, _)| n.eq_ignore_ascii_case(col)) {
                        *col = o.clone();
                    }
                }
            }
        }
        let crossing = changed.iter().any(|(_, n)| {
            self.fields.iter().any(|f| {
                f.origin
                    .as_deref()
                    .is_some_and(|o| o.eq_ignore_ascii_case(n))
            })
        });
        let duplicate = inter.fields.iter().enumerate().any(|(i, f)| {
            inter.fields[..i]
                .iter()
                .any(|g| g.name.eq_ignore_ascii_case(&f.name))
        });
        let two_phase = !changed.is_empty() && !crossing && !duplicate;
        let target = if two_phase || changed.is_empty() {
            inter
        } else {
            let mut target = self.clone();
            for f in &mut target.fields {
                f.check = renamed_text(&f.check, &renames);
                f.generated = f.generated.as_deref().map(|g| renamed_text(g, &renames));
            }
            for c in &mut target.constraints {
                if c.kind == "CHECK" {
                    c.text = renamed_text(&c.text, &renames);
                }
                for col in &mut c.columns {
                    if let Some(n) = renamed(col) {
                        *col = n;
                    }
                }
            }
            target
        };
        let kept = |c: &str| -> Option<String> {
            renames
                .iter()
                .find(|(o, _)| o.eq_ignore_ascii_case(c))
                .map(|(o, _)| o.clone())
        };
        let direct = !(two_phase || changed.is_empty());
        let index_names: &ColumnMap<'_> = if direct { &renamed } else { &kept };
        let index_renames: &[(String, String)] = if direct { &renames } else { &[] };
        out.push(format!("{};", target.create_sql(&temp)));
        // 5. Its rows.
        let copied: Vec<&Field> = target
            .fields
            .iter()
            .zip(&self.fields)
            .filter(|(_, f)| f.origin.is_some() && f.generated.is_none())
            .map(|(t, _)| t)
            .collect();
        let origins: Vec<&Field> = self
            .fields
            .iter()
            .filter(|f| f.origin.is_some() && f.generated.is_none())
            .collect();
        if !copied.is_empty() {
            let to: Vec<String> = copied.iter().map(|f| ident(&f.name)).collect();
            let copied = origins;
            let from: Vec<String> = copied
                .iter()
                .map(|f| ident(f.origin.as_deref().unwrap_or_default()))
                .collect();
            out.push(format!(
                "INSERT INTO \"main\".{} ({}) SELECT {} FROM \"main\".{};",
                ident(&temp),
                to.join(","),
                from.join(","),
                ident(original)
            ));
        }
        // 6. The old table goes, 7. the new one takes the name.
        out.push(format!("DROP TABLE {};", ident(original)));
        out.push(format!(
            "ALTER TABLE {} RENAME TO {};",
            ident(&temp),
            ident(&self.name)
        ));
        // 8. Indexes and triggers back, following renamed columns; an index on a
        // column that is gone goes with it.
        for sql in indexes {
            if let Some(s) = self.index_again(&sql, index_names, index_renames) {
                out.push(format!("{s};"));
            }
        }
        for sql in triggers {
            out.push(format!("{};", retarget_trigger(&sql, original, &self.name)));
        }
        // 9. Views back, then the renamed columns take their new names.
        for (_, sql) in views {
            out.push(format!("{sql};"));
        }
        if two_phase {
            for (o, n) in &changed {
                out.push(format!(
                    "ALTER TABLE {} RENAME COLUMN {} TO {};",
                    ident(&self.name),
                    ident(o),
                    ident(n)
                ));
            }
        }
        // 10. The foreign keys still hold.
        if fk_on {
            out.push("PRAGMA foreign_key_check;".into());
        }
        // 11. Commit, 12. foreign keys on again.
        out.push("COMMIT;".into());
        if fk_on {
            out.push("PRAGMA foreign_keys = 1;".into());
        }
        out
    }
    /// An index of the old table made again on the new one.
    fn index_again(
        &self,
        sql: &str,
        renamed: &ColumnMap<'_>,
        renames: &[(String, String)],
    ) -> Option<String> {
        let Ok(stmts) = cw_sql::parser::parse(sql) else {
            return None;
        };
        let Some(Stmt::CreateIndex(ci)) = stmts.into_iter().next() else {
            return None;
        };
        let mut cols = Vec::new();
        for c in &ci.columns {
            let name = renamed(&c.name)?;
            let mut s = ident(&name);
            if let Some(coll) = &c.collation {
                s.push_str(&format!(" COLLATE {coll}"));
            }
            if c.desc {
                s.push_str(" DESC");
            }
            cols.push(s);
        }
        let filter = where_clause(sql).map(|w| renamed_text(&w, renames));
        Some(format!(
            "CREATE {}INDEX {} ON {} ({}){}",
            if ci.unique { "UNIQUE " } else { "" },
            ident(&ci.name),
            ident(&self.name),
            cols.join(", "),
            filter.map(|w| format!(" WHERE {w}")).unwrap_or_default()
        ))
    }
}
impl Default for TableDesign {
    fn default() -> Self {
        Self::new()
    }
}
/// Expression text with renamed columns under their new names, as SQLite's own
/// RENAME COLUMN rewrites CHECK constraints, generated columns and partial indexes.
fn renamed_text(text: &str, renames: &[(String, String)]) -> String {
    let Ok(tokens) = cw_sql::lexer::tokenize(text) else {
        return text.to_owned();
    };
    let mut out = String::new();
    let mut at = 0;
    for t in tokens {
        if let cw_sql::lexer::Tok::Ident { name, .. } = &t.tok {
            if let Some((_, new)) = renames
                .iter()
                .find(|(o, n)| o.eq_ignore_ascii_case(name) && o != n)
            {
                out.push_str(&text[at..t.start]);
                out.push_str(&ident(new));
                at = t.end;
            }
        }
    }
    out.push_str(&text[at..]);
    out
}
/// A partial index's WHERE text: what follows the column list.
fn where_clause(sql: &str) -> Option<String> {
    let open = sql.find('(')?;
    let mut depth = 0;
    let mut close = None;
    for (i, ch) in sql[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let rest = sql[close? + 1..].trim();
    let upper = rest.to_ascii_uppercase();
    upper
        .starts_with("WHERE")
        .then(|| rest[5..].trim().to_owned())
}
/// A trigger's CREATE text pointed at the table's (possibly new) name.
fn retarget_trigger(sql: &str, from: &str, to: &str) -> String {
    if from == to {
        return sql.to_owned();
    }
    // The table follows ON; replace that one occurrence.
    let upper = sql.to_ascii_uppercase();
    let mut at = 0;
    while let Some(i) = upper[at..].find(" ON ") {
        let start = at + i + 4;
        let rest = &sql[start..];
        let trimmed = rest.trim_start();
        let skip = rest.len() - trimmed.len();
        for candidate in [
            ident(from),
            from.to_owned(),
            format!("`{from}`"),
            format!("[{from}]"),
        ] {
            if trimmed.len() >= candidate.len()
                && trimmed[..candidate.len()].eq_ignore_ascii_case(&candidate)
            {
                let end = start + skip + candidate.len();
                return format!("{}{}{}", &sql[..start + skip], ident(to), &sql[end..]);
            }
        }
        at = start;
    }
    sql.to_owned()
}

impl IndexDesign {
    pub fn new(table: &str) -> Self {
        Self {
            name: String::new(),
            table: table.into(),
            unique: false,
            columns: Vec::new(),
            filter: String::new(),
            focus: DesignFocus::Name,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Please enter a name for the index.".into());
        }
        if self.table.is_empty() {
            return Err("Please choose a table for the index.".into());
        }
        if self.columns.is_empty() {
            return Err("Please add at least one column to the index.".into());
        }
        Ok(())
    }
    /// The CREATE INDEX statement, as DB Browser writes it.
    pub fn sql(&self) -> String {
        let cols: Vec<String> = self
            .columns
            .iter()
            .map(|(c, desc)| format!("\t{}\t{}", ident(c), if *desc { "DESC" } else { "ASC" }))
            .collect();
        format!(
            "CREATE {}INDEX {} ON {} (\n{}\n){};",
            if self.unique { "UNIQUE " } else { "" },
            ident(&self.name),
            ident(&self.table),
            cols.join(",\n"),
            if self.filter.trim().is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", self.filter.trim())
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cw_sql::{Database, Value};

    fn db(sql: &str) -> Database {
        let mut d = Database::new();
        d.execute(sql).unwrap();
        d
    }
    fn apply(d: &mut Database, stmts: &[String]) {
        for s in stmts {
            let out = d
                .execute_one(s, &[])
                .unwrap_or_else(|e| panic!("{s}: {}", e.message));
            assert!(
                !s.starts_with("PRAGMA foreign_key_check") || out.rows.is_empty(),
                "{:?}",
                out.rows
            );
        }
    }
    fn rows(d: &mut Database, sql: &str) -> Vec<Vec<Value>> {
        d.query(sql).unwrap().rows
    }

    #[test]
    fn a_new_table_is_written_the_way_db_browser_writes_it() {
        let mut t = TableDesign::new();
        t.name = "people".into();
        let mut id = Field::new("id", "INTEGER");
        id.not_null = true;
        id.pk = true;
        id.autoinc = true;
        let mut name = Field::new("name", "TEXT");
        name.unique = true;
        name.default = "nobody".into();
        let mut age = Field::new("age", "INTEGER");
        age.check = "age >= 0".into();
        t.fields = vec![id, name, age];
        let d = Database::new();
        let sql = t.statements(&d).unwrap();
        assert_eq!(
            sql,
            vec!["CREATE TABLE \"people\" (\n\t\"id\"\tINTEGER NOT NULL,\n\t\"name\"\tTEXT DEFAULT 'nobody' UNIQUE,\n\t\"age\"\tINTEGER CHECK(age >= 0),\n\tPRIMARY KEY(\"id\" AUTOINCREMENT)\n);".to_string()]
        );
        let mut d = d;
        apply(&mut d, &sql);
        d.execute("INSERT INTO people (age) VALUES (3)").unwrap();
        assert_eq!(
            rows(&mut d, "SELECT id, name, age FROM people"),
            vec![vec![
                Value::Integer(1),
                Value::Text("nobody".into()),
                Value::Integer(3)
            ]]
        );
        // The design reads back from the table it made.
        let back = TableDesign::of_table(&d, "people").unwrap();
        assert_eq!(back.fields.len(), 3);
        assert!(back.fields[0].pk && back.fields[0].autoinc && back.fields[0].not_null);
        assert!(back.fields[1].unique);
        assert_eq!(back.fields[1].default, "'nobody'");
        assert_eq!(back.fields[2].check, "age >= 0");
    }

    #[test]
    fn designs_that_cannot_be_made_say_why() {
        let mut t = TableDesign::new();
        assert!(t.validate().unwrap_err().contains("name for the table"));
        t.name = "t".into();
        assert!(t.validate().unwrap_err().contains("at least one field"));
        t.fields = vec![Field::new("a", "TEXT"), Field::new("A", "TEXT")];
        assert!(t.validate().unwrap_err().contains("already is a field"));
        t.fields[1].name = "b".into();
        t.fields[1].autoinc = true;
        t.fields[1].pk = true;
        assert!(t.validate().unwrap_err().contains("AUTOINCREMENT"));
        t.fields[1].autoinc = false;
        t.fields[1].pk = false;
        t.without_rowid = true;
        assert!(t.validate().unwrap_err().contains("Primary key"));
        t.fields[0].pk = true;
        t.validate().unwrap();
        let mut d = Database::new();
        let sql = t.statements(&d).unwrap();
        apply(&mut d, &sql);
        assert!(!d.has_rowid("t"));
    }

    #[test]
    fn renames_and_added_columns_use_alter_table() {
        let d = db("CREATE TABLE t (a INTEGER, b TEXT); INSERT INTO t VALUES (1, 'x');");
        let mut t = TableDesign::of_table(&d, "t").unwrap();
        t.name = "u".into();
        t.fields[1].name = "bee".into();
        let mut c = Field::new("c", "REAL");
        c.default = "2.5".into();
        c.not_null = true;
        // Renames and a column added at the end: three ALTER TABLE statements.
        t.fields.push(c.clone());
        let s = t.statements(&d).unwrap();
        assert_eq!(s.len(), 3, "{s:?}");
        assert_eq!(
            s[2],
            "ALTER TABLE \"u\" ADD COLUMN \"c\" REAL NOT NULL DEFAULT 2.5;"
        );
        // A column added anywhere else is a rebuild.
        let added = t.fields.pop().unwrap();
        t.fields.insert(0, added);
        let s = t.statements(&d).unwrap();
        assert!(s
            .iter()
            .any(|s| s.starts_with("CREATE TABLE \"sqlb_temp_table_1\"")));
        t.fields.remove(0);
        let s = t.statements(&d).unwrap();
        assert_eq!(
            s,
            vec![
                "ALTER TABLE \"t\" RENAME TO \"u\";".to_string(),
                "ALTER TABLE \"u\" RENAME COLUMN \"b\" TO \"bee\";".to_string()
            ]
        );
        let mut d2 = d.clone();
        apply(&mut d2, &s);
        assert_eq!(
            rows(&mut d2, "SELECT a, bee FROM u"),
            vec![vec![Value::Integer(1), Value::Text("x".into())]]
        );
        // Adding a column at the end alone is ADD COLUMN.
        let mut t = TableDesign::of_table(&d, "t").unwrap();
        t.fields.push(c);
        let s = t.statements(&d).unwrap();
        assert_eq!(
            s,
            vec!["ALTER TABLE \"t\" ADD COLUMN \"c\" REAL NOT NULL DEFAULT 2.5;".to_string()]
        );
    }

    #[test]
    fn other_changes_rebuild_the_table_keeping_rows_indexes_triggers_and_views() {
        let mut d = db("PRAGMA foreign_keys = ON;
            CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT);
            CREATE TABLE child (id INTEGER PRIMARY KEY, pid INTEGER REFERENCES parent(id), note TEXT, extra BLOB);
            CREATE INDEX child_pid ON child (pid);
            CREATE INDEX child_extra ON child (extra);
            CREATE UNIQUE INDEX child_note ON child (note COLLATE NOCASE DESC);
            CREATE TABLE log (msg TEXT);
            CREATE TRIGGER child_ins AFTER INSERT ON child BEGIN INSERT INTO log VALUES ('added ' || new.note); END;
            CREATE VIEW notes AS SELECT note FROM child;
            INSERT INTO parent VALUES (1, 'p');
            INSERT INTO child VALUES (1, 1, 'first', NULL), (2, 1, 'second', x'00');");
        let mut t = TableDesign::of_table(&d, "child").unwrap();
        // Change a type, make a field NOT NULL, drop one, rename one and move it first.
        t.fields[2].not_null = true;
        t.fields[2].name = "text".into();
        t.fields[1].ty = "INT".into();
        t.fields.remove(3);
        let note = t.fields.remove(2);
        t.fields.insert(0, note);
        let s = t.statements(&d).unwrap();
        assert_eq!(s[0], "PRAGMA foreign_keys = 0;");
        assert_eq!(s[1], "BEGIN TRANSACTION;");
        assert!(s.contains(&"DROP TABLE \"child\";".to_string()));
        assert!(s.contains(&"ALTER TABLE \"sqlb_temp_table_1\" RENAME TO \"child\";".to_string()));
        assert!(s.contains(&"PRAGMA foreign_key_check;".to_string()));
        assert_eq!(s.last().unwrap(), "PRAGMA foreign_keys = 1;");
        apply(&mut d, &s);
        assert_eq!(
            rows(&mut d, "SELECT text, id, pid FROM child ORDER BY id"),
            vec![
                vec![
                    Value::Text("first".into()),
                    Value::Integer(1),
                    Value::Integer(1)
                ],
                vec![
                    Value::Text("second".into()),
                    Value::Integer(2),
                    Value::Integer(1)
                ],
            ]
        );
        let idx: Vec<Vec<Value>> = rows(
            &mut d,
            "SELECT name, sql FROM sqlite_schema WHERE type = 'index' AND tbl_name = 'child' ORDER BY name",
        );
        assert_eq!(idx.len(), 2, "{idx:?}");
        assert_eq!(
            idx[0][1],
            Value::Text(
                "CREATE UNIQUE INDEX \"child_note\" ON \"child\" (\"text\" COLLATE NOCASE DESC)"
                    .into()
            )
        );
        // The trigger is back; the view was dropped and made again.
        d.execute("INSERT INTO child (id, text, pid) VALUES (3, 'third', 1)")
            .unwrap();
        assert_eq!(
            rows(&mut d, "SELECT msg FROM log"),
            ["added first", "added second", "added third"]
                .map(|m| vec![Value::Text(m.into())])
                .to_vec()
        );
        assert!(d.foreign_keys());
        assert_eq!(
            rows(&mut d, "PRAGMA integrity_check"),
            vec![vec![Value::Text("ok".into())]]
        );
        let v = TableDesign::of_table(&d, "child").unwrap();
        assert_eq!(v.fields[0].name, "text");
        assert!(v.fields[0].not_null);
        assert_eq!(v.fields[2].fk, "\"parent\"(\"id\")");
    }

    #[test]
    fn a_rebuild_that_breaks_a_foreign_key_is_refused() {
        let mut d = db("PRAGMA foreign_keys = ON;
            CREATE TABLE parent (id INTEGER PRIMARY KEY);
            CREATE TABLE child (pid INTEGER);
            INSERT INTO child VALUES (7);");
        let mut t = TableDesign::of_table(&d, "child").unwrap();
        t.fields[0].fk = "\"parent\"(\"id\")".into();
        let s = t.statements(&d).unwrap();
        let check = s
            .iter()
            .position(|s| s == "PRAGMA foreign_key_check;")
            .unwrap();
        for st in &s[..check] {
            d.execute_one(st, &[]).unwrap();
        }
        let out = d.execute_one(&s[check], &[]).unwrap();
        assert_eq!(out.rows.len(), 1, "the orphan row is reported");
    }

    #[test]
    fn an_index_design_writes_create_index() {
        let mut i = IndexDesign::new("t");
        assert!(i.validate().is_err());
        i.name = "t_ab".into();
        i.unique = true;
        i.columns = vec![("a".into(), false), ("b".into(), true)];
        i.validate().unwrap();
        assert_eq!(
            i.sql(),
            "CREATE UNIQUE INDEX \"t_ab\" ON \"t\" (\n\t\"a\"\tASC,\n\t\"b\"\tDESC\n);"
        );
        let mut d = db("CREATE TABLE t (a INTEGER, b TEXT)");
        d.execute(&i.sql()).unwrap();
        let plan = d
            .query("EXPLAIN QUERY PLAN SELECT b FROM t WHERE a = 1")
            .unwrap();
        assert!(format!("{:?}", plan.rows).contains("t_ab"));
    }

    #[test]
    fn triggers_follow_a_renamed_table() {
        assert_eq!(
            retarget_trigger(
                "CREATE TRIGGER x AFTER INSERT ON \"old\" BEGIN SELECT 1; END",
                "old",
                "new"
            ),
            "CREATE TRIGGER x AFTER INSERT ON \"new\" BEGIN SELECT 1; END"
        );
        assert_eq!(
            retarget_trigger(
                "CREATE TRIGGER x BEFORE DELETE ON old BEGIN SELECT 1; END",
                "old",
                "new"
            ),
            "CREATE TRIGGER x BEFORE DELETE ON \"new\" BEGIN SELECT 1; END"
        );
    }
}
