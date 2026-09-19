//! The client's side of the designers: DB Browser's Create/Modify Table and Create
//! Index dialogs (`db:createtable`, `db:modifytable`, `db:createindex`, `db:design:*`,
//! `db:index:*`) and TablePlus's structure editor (`db:struct:*`), whose changes are
//! staged until Commit.
use super::designer::{DesignFocus, Field, IndexDesign, TableDesign, COLUMNS, TYPES};
use super::{ident, push_bounded, Client, Tab};
use crate::AppEffect;

/// The text columns of the Fields grid, in Tab order.
const TEXT_COLUMNS: [usize; 6] = [0, 1, 6, 7, 8, 9];
/// TablePlus's structure grid columns and the design field each edits.
pub const TABLEPLUS_COLUMNS: [(&str, usize); 6] = [
    ("column_name", 0),
    ("data_type", 1),
    ("is_nullable", 2),
    ("column_default", 6),
    ("primary_key", 3),
    ("foreign_key", 9),
];

fn parse_cell(arg: &str) -> Result<(usize, usize), String> {
    let (r, c) = arg.split_once(':').ok_or("which cell?")?;
    Ok((
        r.parse().map_err(|_| "no such field")?,
        c.parse().map_err(|_| "no such column")?,
    ))
}

impl Client {
    /// The table the structure tree or Browse Data has selected.
    fn selected_table(&self) -> Option<String> {
        self.tree_selected
            .as_deref()
            .and_then(|t| t.strip_prefix("table:"))
            .map(str::to_owned)
            .filter(|t| self.db.as_ref().is_some_and(|d| d.is_table(t)))
    }
    /// Whether a designer dialog is open (TablePlus's staged structure is not one).
    pub fn designing(&self) -> bool {
        self.design.as_ref().is_some_and(|d| !d.inline) || self.index_design.is_some()
    }
    /// A staged TablePlus structure change that differs from the table.
    pub fn staged_structure(&self) -> bool {
        match (&self.design, &self.db) {
            (Some(d), Some(db)) if d.inline => d.original.as_deref().is_none_or(|o| {
                TableDesign::of_table(db, o).map_or(true, |mut before| {
                    before.inline = true;
                    before.selected = d.selected;
                    before.focus = d.focus;
                    before.replace = d.replace;
                    before != *d
                })
            }),
            _ => false,
        }
    }
    /// Run a design's statements; on any failure the database is left as it was.
    fn run_design(&mut self, stmts: &[String]) -> Result<(), String> {
        let db = self.db.as_mut().ok_or("no database is open")?;
        let backup = db.clone();
        for s in stmts {
            match db.execute_one(s, &[]) {
                Ok(out) if s.starts_with("PRAGMA foreign_key_check") && !out.rows.is_empty() => {
                    *db = backup;
                    return Err("FOREIGN KEY constraint failed: rows in this table would refer to rows that do not exist".into());
                }
                Ok(_) => {}
                Err(e) => {
                    *db = backup;
                    return Err(e.message);
                }
            }
        }
        Ok(())
    }
    /// Make the open design real: DB Browser's OK, TablePlus's Commit.
    pub(super) fn apply_design(&mut self) -> Result<(), String> {
        let Some(design) = self.design.clone() else {
            return Ok(());
        };
        let db = self.db.as_ref().ok_or("no database is open")?;
        let stmts = design.statements(db);
        let result = stmts.and_then(|s| self.run_design(&s));
        match result {
            Ok(()) => {
                self.design = None;
                if let Some(o) = &design.original {
                    if self.table.as_deref() == Some(o.as_str()) {
                        self.table = Some(design.name.clone());
                    }
                }
                if design.original.is_none() {
                    self.table = Some(design.name.clone());
                }
                self.tree_selected = Some(format!("table:{}", design.name));
                self.cell = None;
                self.sort = None;
                self.filters.clear();
                Ok(())
            }
            Err(e) => {
                let what = if design.original.is_some() {
                    "Error altering table"
                } else {
                    "Error creating table"
                };
                self.message = Some(format!("{what}. Message from database engine:\n{e}"));
                Err(e)
            }
        }
    }
    fn design_mut(&mut self) -> Result<&mut TableDesign, String> {
        self.design
            .as_mut()
            .ok_or_else(|| "no table design is open".to_string())
    }
    /// TablePlus: the staged design of the table on show, made on first edit.
    fn staged_mut(&mut self) -> Result<&mut TableDesign, String> {
        let table = self.table.clone().ok_or("choose a table first")?;
        if !self.db.as_ref().is_some_and(|d| d.is_table(&table)) {
            return Err(format!("{table} is a view; its structure cannot be edited"));
        }
        let fresh = !matches!(&self.design, Some(d) if d.inline && d.original.as_deref() == Some(table.as_str()));
        if fresh {
            let db = self.db.as_ref().ok_or("no database is open")?;
            let mut d = TableDesign::of_table(db, &table)?;
            d.inline = true;
            self.design = Some(d);
        }
        self.design_mut()
    }
    fn toggle(d: &mut TableDesign, row: usize, col: usize) {
        let f = &mut d.fields[row];
        match col {
            2 => f.not_null = !f.not_null,
            3 => {
                f.pk = !f.pk;
                if !f.pk {
                    f.autoinc = false;
                }
            }
            4 => {
                // AUTOINCREMENT makes the field an INTEGER primary key, as in DB Browser.
                f.autoinc = !f.autoinc;
                if f.autoinc {
                    f.pk = true;
                    f.ty = "INTEGER".into();
                }
            }
            5 => f.unique = !f.unique,
            _ => {}
        }
    }

    /// `createtable`, `modifytable`, `createindex`, `design:*`, `index:*`, `struct:*`.
    pub(super) fn structure_command(
        &mut self,
        verb: &str,
        arg: &str,
    ) -> Option<Result<Vec<AppEffect>, String>> {
        let r = match verb {
            "createtable" => {
                self.edit = None;
                self.design = Some(TableDesign::new());
                Ok(())
            }
            "modifytable" => {
                let table = if arg.is_empty() {
                    self.selected_table()
                } else {
                    Some(arg.to_owned())
                };
                match table {
                    None => Err("select a table in Database Structure first".to_string()),
                    Some(t) => self
                        .db
                        .as_ref()
                        .ok_or_else(|| "no database is open".to_string())
                        .and_then(|db| TableDesign::of_table(db, &t))
                        .map(|d| self.design = Some(d)),
                }
            }
            "createindex" => {
                let table = if arg.is_empty() {
                    self.selected_table()
                        .or_else(|| self.table.clone().filter(|t| !self.is_view(t)))
                        .or_else(|| {
                            let db = self.db.as_ref()?;
                            self.tables().into_iter().find(|t| db.is_table(t))
                        })
                } else {
                    Some(arg.to_owned())
                };
                match table {
                    Some(t) => {
                        self.index_design = Some(IndexDesign::new(&t));
                        Ok(())
                    }
                    None => Err("the database has no tables to index".into()),
                }
            }
            "design" => self.design_step(arg),
            "index" => self.index_step(arg),
            "struct" => self.struct_step(arg),
            _ => return None,
        };
        Some(r.map(|()| vec![]))
    }
    fn design_step(&mut self, arg: &str) -> Result<(), String> {
        let (what, rest) = arg.split_once(':').unwrap_or((arg, ""));
        match what {
            "ok" => {
                // A design that cannot be made says why in a message box and stays open.
                let d = self.design_mut()?;
                d.focus = DesignFocus::None;
                if let Err(e) = d.validate() {
                    self.message = Some(e);
                    return Ok(());
                }
                let _ = self.apply_design();
                return Ok(());
            }
            "cancel" => {
                self.design = None;
                return Ok(());
            }
            _ => {}
        }
        let d = self.design_mut()?;
        match what {
            "name" => {
                d.focus = DesignFocus::Name;
                d.replace = false;
            }
            "add" => {
                let name = d.next_field_name();
                d.fields.push(Field::new(&name, "INTEGER"));
                let i = d.fields.len() - 1;
                d.selected = Some(i);
                d.focus = DesignFocus::Cell(i, 0);
                d.replace = true;
            }
            "remove" => {
                let i = d.selected.ok_or("select a field first")?;
                d.fields.remove(i);
                d.selected = if d.fields.is_empty() {
                    None
                } else {
                    Some(i.min(d.fields.len() - 1))
                };
                d.focus = DesignFocus::None;
            }
            "top" | "up" | "down" | "bottom" => {
                let i = d.selected.ok_or("select a field first")?;
                let n = d.fields.len();
                let to = match what {
                    "top" => 0,
                    "up" => i.checked_sub(1).ok_or("that field is already first")?,
                    "down" if i + 1 < n => i + 1,
                    "down" => return Err("that field is already last".into()),
                    _ => n - 1,
                };
                let f = d.fields.remove(i);
                d.fields.insert(to, f);
                d.selected = Some(to);
                d.focus = DesignFocus::None;
            }
            "cell" => {
                let (r, c) = parse_cell(rest)?;
                if r >= d.fields.len() || c >= COLUMNS.len() {
                    return Err("no such cell".into());
                }
                d.selected = Some(r);
                if COLUMNS[c].1 {
                    d.focus = DesignFocus::Cell(r, c);
                    d.replace = true;
                } else {
                    d.focus = DesignFocus::None;
                    Self::toggle(d, r, c);
                }
            }
            "type" => {
                let (r, t) = rest.split_once(':').ok_or("which field?")?;
                let r: usize = r.parse().map_err(|_| "no such field")?;
                if !TYPES.contains(&t) {
                    return Err(format!("{t} is not one of the listed types"));
                }
                let f = d.fields.get_mut(r).ok_or("no such field")?;
                f.ty = t.into();
                d.selected = Some(r);
                d.focus = DesignFocus::None;
                self.menu = None;
            }
            "withoutrowid" => d.without_rowid = !d.without_rowid,
            other => return Err(format!("unknown designer control {other}")),
        }
        Ok(())
    }
    fn index_step(&mut self, arg: &str) -> Result<(), String> {
        let (what, rest) = arg.split_once(':').unwrap_or((arg, ""));
        if what == "cancel" {
            self.index_design = None;
            return Ok(());
        }
        if what == "ok" {
            let d = self.index_design.clone().ok_or("no index design is open")?;
            if let Err(e) = d.validate() {
                self.message = Some(e);
                return Ok(());
            }
            match self.run_design(&[d.sql()]) {
                Ok(()) => {
                    self.index_design = None;
                    self.tree_selected = Some(format!("index:{}", d.name));
                }
                Err(e) => self.message = Some(format!("Creating the index failed:\n{e}")),
            }
            return Ok(());
        }
        let columns: Vec<String> = {
            let table = self
                .index_design
                .as_ref()
                .map(|d| d.table.clone())
                .unwrap_or_default();
            self.db
                .as_ref()
                .and_then(|db| db.table_info(&table))
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.name)
                .collect()
        };
        let tables: Vec<String> = self
            .tables()
            .into_iter()
            .filter(|t| !self.is_view(t))
            .collect();
        let d = self
            .index_design
            .as_mut()
            .ok_or("no index design is open")?;
        match what {
            "name" => d.focus = DesignFocus::Name,
            "where" => return Err(super::design_view::PARTIAL.into()),
            "unique" => d.unique = !d.unique,
            "table" => {
                if !tables.iter().any(|t| t == rest) {
                    return Err(format!("no such table: {rest}"));
                }
                if d.table != rest {
                    d.table = rest.into();
                    d.columns.clear();
                }
                self.menu = None;
            }
            "col" => {
                if !columns.iter().any(|c| c == rest) {
                    return Err(format!("{} has no column {rest}", d.table));
                }
                match d.columns.iter().position(|(c, _)| c == rest) {
                    Some(i) => {
                        d.columns.remove(i);
                    }
                    None => d.columns.push((rest.into(), false)),
                }
            }
            "order" => {
                let i: usize = rest.parse().map_err(|_| "no such column")?;
                let c = d.columns.get_mut(i).ok_or("no such column")?;
                c.1 = !c.1;
            }
            other => return Err(format!("unknown index control {other}")),
        }
        Ok(())
    }
    fn struct_step(&mut self, arg: &str) -> Result<(), String> {
        let (what, rest) = arg.split_once(':').unwrap_or((arg, ""));
        match what {
            "cell" => {
                let (r, c) = parse_cell(rest)?;
                let &(_, col) = TABLEPLUS_COLUMNS.get(c).ok_or("no such column")?;
                let d = self.staged_mut()?;
                if r >= d.fields.len() {
                    return Err("no such column".into());
                }
                d.selected = Some(r);
                d.focus = DesignFocus::None;
                if COLUMNS[col].1 {
                    // A click selects; a double click (or typing) edits.
                    d.focus = DesignFocus::Cell(r, col);
                    d.replace = true;
                } else {
                    Self::toggle(d, r, col);
                }
            }
            "addcol" => {
                let d = self.staged_mut()?;
                let name = (1..)
                    .map(|n| format!("column_{n}"))
                    .find(|n| !d.fields.iter().any(|f| f.name.eq_ignore_ascii_case(n)))
                    .unwrap_or_default();
                d.fields.push(Field::new(&name, "TEXT"));
                let i = d.fields.len() - 1;
                d.selected = Some(i);
                d.focus = DesignFocus::Cell(i, 0);
                d.replace = true;
            }
            "delcol" => {
                let d = self.staged_mut()?;
                let i = d.selected.ok_or("select a column first")?;
                d.fields.remove(i);
                d.selected = None;
                d.focus = DesignFocus::None;
            }
            "dropindex" => {
                let is_index = self.db.as_ref().is_some_and(|db| {
                    db.schema()
                        .iter()
                        .any(|e| e.kind == "index" && e.name == rest && e.sql.is_some())
                });
                if !is_index {
                    return Err(format!("no index {rest} that can be dropped"));
                }
                self.run_design(&[format!("DROP INDEX {};", ident(rest))])?;
            }
            other => return Err(format!("unknown structure control {other}")),
        }
        Ok(())
    }
    /// Double-click a designer cell: edit it with the caret at the end.
    pub(super) fn design_activate(
        &mut self,
        target: &str,
    ) -> Option<Result<Vec<AppEffect>, String>> {
        let (inline, rest) = if let Some(r) = target.strip_prefix("db:design:cell:") {
            (false, r)
        } else {
            (true, target.strip_prefix("db:struct:cell:")?)
        };
        let command = if inline {
            format!("struct:cell:{rest}")
        } else {
            format!("design:cell:{rest}")
        };
        let (verb, arg) = command.split_once(':').unwrap_or((&command, ""));
        let r = self.structure_command(verb, arg)?;
        if r.is_ok() {
            if let Some(d) = &mut self.design {
                d.replace = false;
            }
        }
        Some(r)
    }
    /// Typing into the designer that has focus.
    pub(super) fn design_text(&mut self, text: &str) -> Option<Result<(), String>> {
        if let Some(d) = &mut self.index_design {
            let target = match d.focus {
                DesignFocus::Name => &mut d.name,
                DesignFocus::Where => &mut d.filter,
                _ => return Some(Err("click the index name or its WHERE clause first".into())),
            };
            push_bounded(target, text, 1024);
            return Some(Ok(()));
        }
        let visible = self.tab == Tab::Structure || self.design.as_ref().is_some_and(|d| !d.inline);
        let d = self.design.as_mut().filter(|_| visible)?;
        let target = match d.focus {
            DesignFocus::Name => &mut d.name,
            DesignFocus::Cell(r, c) => match d.fields.get_mut(r).and_then(|f| f.text_mut(c)) {
                Some(t) => t,
                None => return Some(Err("no such cell".into())),
            },
            _ => {
                if d.inline {
                    return None;
                }
                return Some(Err("click a field or the table name first".into()));
            }
        };
        if d.replace {
            target.clear();
            d.replace = false;
        }
        push_bounded(target, text, 4096);
        Some(Ok(()))
    }
    /// Keys while a designer has focus; `None` when the key is not for it.
    pub(super) fn design_key(&mut self, key: &str) -> Option<Result<Vec<AppEffect>, String>> {
        if let Some(d) = &mut self.index_design {
            let r = match key {
                "Backspace" => {
                    match d.focus {
                        DesignFocus::Name => {
                            d.name.pop();
                        }
                        DesignFocus::Where => {
                            d.filter.pop();
                        }
                        _ => {}
                    }
                    Ok(vec![])
                }
                "Tab" => {
                    d.focus = DesignFocus::Name;
                    Ok(vec![])
                }
                "Enter" => self.structure_command("index", "ok")?,
                "Escape" => self.structure_command("index", "cancel")?,
                other => Err(format!("unsupported key {other} in the index dialog")),
            };
            return Some(r);
        }
        let inline = self.design.as_ref()?.inline;
        if inline && self.tab != Tab::Structure {
            return None;
        }
        let d = self.design.as_mut()?;
        let r = match (key, d.focus) {
            ("Backspace", DesignFocus::Name) => {
                d.name.pop();
                Ok(vec![])
            }
            ("Backspace", DesignFocus::Cell(r, c)) => {
                if let Some(t) = d.fields.get_mut(r).and_then(|f| f.text_mut(c)) {
                    if d.replace {
                        t.clear();
                    } else {
                        t.pop();
                    }
                }
                d.replace = false;
                Ok(vec![])
            }
            ("Tab", DesignFocus::Cell(r, c)) => {
                let cols: Vec<usize> = if inline {
                    TABLEPLUS_COLUMNS
                        .iter()
                        .map(|(_, c)| *c)
                        .filter(|c| COLUMNS[*c].1)
                        .collect()
                } else {
                    TEXT_COLUMNS.to_vec()
                };
                let k = cols.iter().position(|x| *x == c).unwrap_or(0);
                let (r, c) = if k + 1 < cols.len() {
                    (r, cols[k + 1])
                } else {
                    ((r + 1) % d.fields.len().max(1), cols[0])
                };
                d.focus = DesignFocus::Cell(r, c);
                d.selected = Some(r);
                d.replace = true;
                Ok(vec![])
            }
            ("Tab", DesignFocus::Name) if !d.fields.is_empty() => {
                d.focus = DesignFocus::Cell(0, 0);
                d.selected = Some(0);
                d.replace = true;
                Ok(vec![])
            }
            ("Enter" | "Escape", DesignFocus::Cell(..)) => {
                d.focus = DesignFocus::None;
                Ok(vec![])
            }
            ("Enter", _) if !inline => self.structure_command("design", "ok")?,
            ("Escape", _) if !inline => self.structure_command("design", "cancel")?,
            (_, _) if inline => return None,
            (other, _) => Err(format!("unsupported key {other} in the table designer")),
        };
        Some(r)
    }
}
