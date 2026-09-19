use super::*;
use crate::desktop_scene::Painter;
use crate::{AppEnv, SystemSettings};

fn env(theme: DesktopTheme) -> AppEnv<'static> {
    AppEnv {
        theme,
        width: 1100,
        height: 700,
        clock_us: 0,
        settings: &SystemSettings::DEFAULT,
        clipboard: None,
        share_to: None,
        files: Default::default(),
        editor: None,
        pointer: None,
    }
}
const THEMES: [DesktopTheme; 3] = [
    DesktopTheme::Ubuntu,
    DesktopTheme::Windows,
    DesktopTheme::Macos,
];

fn file() -> Vec<u8> {
    let mut db = cw_sql::Database::new();
    db.execute(
        "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
         INSERT INTO people (name, age) VALUES ('Ada', 36), ('Grace', 45), ('Linus', NULL);
         CREATE INDEX people_name ON people(name);
         CREATE VIEW adults AS SELECT name FROM people WHERE age >= 18;",
    )
    .unwrap();
    db.to_bytes()
}
/// A client with `/home/u/Documents/people.db` open.
fn client() -> Client {
    let (mut c, effects) = Client::launch("/home/u/Documents/people.db", 1);
    assert!(
        matches!(&effects[0], AppEffect::ReadBytes { path, .. } if path == "/home/u/Documents/people.db")
    );
    c.listed(vec!["people.db".into(), "people.csv".into(), "Old/".into()]);
    c.bytes("/home/u/Documents/people.db", Ok(file()), 0);
    assert!(c.db.is_some(), "{:?}", c.message);
    c
}
fn targets(c: &Client, theme: DesktopTheme) -> Vec<String> {
    let mut p = Painter::new(1100, 700);
    view::render(c, &mut p, &env(theme));
    // Under a modal dialog's scrim nothing can be reached: only what is above it.
    let above = p
        .scene
        .nodes
        .iter()
        .rposition(|n| {
            n.interaction.as_deref() == Some("db:noop")
                && n.bounds.width == 1100
                && n.bounds.height == 700
        })
        .map_or(0, |i| i + 1);
    p.scene.nodes[above..]
        .iter()
        .filter_map(|n| n.interaction.clone())
        .filter(|t| t.starts_with("db:"))
        .collect()
}
fn states() -> Vec<Client> {
    let (mut empty, _) = Client::launch("/home/u/Documents", 1);
    empty.listed(vec!["people.db".into()]);
    let base = client();
    let mut out = vec![empty.clone(), base.clone()];
    let mut dialog = empty;
    dialog.command(1, "open", 0).unwrap();
    out.push(dialog);
    for tab in ["structure", "browse", "pragmas", "execute"] {
        let mut s = base.clone();
        s.command(1, &format!("tab:{tab}"), 0).unwrap();
        out.push(s.clone());
        if tab == "browse" {
            s.command(1, "cell:1:1", 0).unwrap();
            out.push(s.clone());
            s.command(1, "editcell", 0).unwrap();
            out.push(s.clone());
            s.edit = None;
            s.command(1, "table:adults", 0).unwrap();
            out.push(s.clone());
        }
        if tab == "execute" {
            s.text("SELECT * FROM people").unwrap();
            s.command(1, "run", 0).unwrap();
            out.push(s);
        }
    }
    for m in ["file", "edit", "view", "tools", "tables"] {
        let mut s = base.clone();
        s.command(1, "tab:browse", 0).unwrap();
        s.command(1, "cell:0:0", 0).unwrap();
        s.menu = Some(m.into());
        out.push(s);
    }
    let mut modified = base.clone();
    modified.command(1, "tab:browse", 0).unwrap();
    modified.command(1, "newrow", 0).unwrap();
    out.push(modified.clone());
    modified.command(1, "close", 0).unwrap();
    out.push(modified);
    let mut drop = base.clone();
    drop.command(1, "tree:table:people", 0).unwrap();
    out.push(drop.clone());
    drop.command(1, "droptable", 0).unwrap();
    out.push(drop);
    let mut message = base.clone();
    message.message = Some("Something\nto say".into());
    out.push(message);
    // The designers.
    let mut create = base.clone();
    create.command(1, "createtable", 0).unwrap();
    out.push(create.clone());
    create.command(1, "design:add", 0).unwrap();
    create.command(1, "design:add", 0).unwrap();
    out.push(create.clone());
    create.command(1, "menu:designtype:1", 0).unwrap();
    out.push(create);
    let mut modify = base.clone();
    modify.command(1, "tree:table:people", 0).unwrap();
    modify.command(1, "modifytable", 0).unwrap();
    modify.command(1, "design:cell:1:0", 0).unwrap();
    out.push(modify.clone());
    modify.command(1, "design:ok", 0).unwrap();
    out.push(modify);
    let mut index = base.clone();
    index.command(1, "createindex", 0).unwrap();
    index.command(1, "index:col:name", 0).unwrap();
    out.push(index.clone());
    index.command(1, "menu:indextables", 0).unwrap();
    out.push(index);
    let mut staged = base;
    staged.command(1, "table:people", 0).unwrap();
    staged.command(1, "tab:structure", 0).unwrap();
    out.push(staged.clone());
    staged.command(1, "struct:cell:2:0", 0).unwrap();
    staged.text("years").unwrap();
    out.push(staged);
    out
}

#[test]
fn every_painted_control_is_one_the_client_handles() {
    for theme in THEMES {
        for state in states() {
            for target in targets(&state, theme) {
                let mut s = state.clone();
                let result = s.click(1, &target, 0);
                assert!(
                    result.is_ok(),
                    "{theme:?}: painted control {target} is refused: {result:?}"
                );
            }
        }
    }
}

#[test]
fn editing_a_cell_updates_the_row_by_rowid_with_the_columns_affinity() {
    let mut c = client();
    c.command(1, "table:people", 0).unwrap();
    c.activate(1, "db:cell:1:2", 0).unwrap();
    assert_eq!(c.edit.as_ref().unwrap().text, "45");
    c.key(1, "Backspace", 0).unwrap();
    c.key(1, "Backspace", 0).unwrap();
    c.text("46").unwrap();
    c.key(1, "Enter", 0).unwrap();
    assert!(c.modified());
    let rows = c.rows(0, 10).unwrap();
    // Typed text lands as an integer in an INTEGER column.
    assert_eq!(rows.rows[1].1[2], Value::Integer(46));
    // The file is untouched until Write Changes.
    let effects = c.command(1, "write", 0).unwrap();
    let bytes = effects
        .iter()
        .find_map(|e| match e {
            AppEffect::WriteBytes { bytes, .. } => Some(bytes.clone()),
            _ => None,
        })
        .unwrap();
    c.saved("/home/u/Documents/people.db", Ok(()));
    assert!(!c.modified());
    let mut back = cw_sql::Database::open(&bytes).unwrap();
    let out = back
        .query("SELECT age FROM people WHERE name = 'Grace'")
        .unwrap();
    assert_eq!(out.rows, vec![vec![Value::Integer(46)]]);
    // A constraint the table has is reported, not bypassed.
    c.activate(1, "db:cell:0:1", 0).unwrap();
    c.command(1, "setnull", 0).unwrap();
    assert!(c
        .message
        .as_deref()
        .unwrap()
        .contains("NOT NULL constraint failed"));
}

#[test]
fn executing_sql_reports_rows_changes_and_errors_by_line() {
    let mut c = client();
    c.command(1, "tab:execute", 0).unwrap();
    c.text("UPDATE people SET age = age + 1;").unwrap();
    c.key(1, "Enter", 0).unwrap();
    c.text("SELECT name, age FROM people ORDER BY age DESC;")
        .unwrap();
    c.key(1, "F5", 0).unwrap();
    let r = c.result.clone().unwrap();
    assert_eq!(r.columns, ["name", "age"]);
    assert_eq!(
        r.rows[0],
        vec![Value::Text("Grace".into()), Value::Integer(46)]
    );
    assert!(
        r.message.contains("Result: 3 rows returned\nAt line 2:"),
        "{}",
        r.message
    );
    c.key(1, "Enter", 0).unwrap();
    c.text("SELEC 1;").unwrap();
    c.key(1, "Shift+F5", 0).unwrap();
    let r = c.result.clone().unwrap();
    assert!(r.error);
    assert!(r.message.contains("At line 3:"), "{}", r.message);
    // Revert Changes goes back to what the file holds.
    assert!(c.modified());
    c.command(1, "revert", 0).unwrap();
    assert!(!c.modified());
}

#[test]
fn importing_csv_makes_a_typed_table_and_exporting_writes_it_back() {
    let mut c = client();
    c.command(1, "import", 0).unwrap();
    c.listed(vec!["cities.csv".into()]);
    let effects = c.command(1, "openfile:cities.csv", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::ReadBytes { path, .. } if path.ends_with("cities.csv"))
    );
    c.bytes(
        "/home/u/Documents/cities.csv",
        Ok(b"city,population,area\nOslo,709037,454.0\n\"Bergen, NO\",291940,465\n".to_vec()),
        0,
    );
    assert_eq!(c.table.as_deref(), Some("cities"));
    let _ = c.message.take();
    let info = c.db.as_ref().unwrap().table_info("cities").unwrap();
    let types: Vec<&str> = info.iter().map(|i| i.decl_type.as_str()).collect();
    assert_eq!(types, ["TEXT", "INTEGER", "REAL"]);
    let effects = c.command(1, "exportcsv", 0).unwrap();
    let text = effects
        .iter()
        .find_map(|e| match e {
            AppEffect::WriteBytes { bytes, .. } => Some(String::from_utf8(bytes.clone()).unwrap()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        text,
        "city,population,area\nOslo,709037,454.0\n\"Bergen, NO\",291940,465.0\n"
    );
}

#[test]
fn db_browser_creates_a_table_from_the_designer() {
    let mut c = client();
    c.command(1, "tab:structure", 0).unwrap();
    c.command(1, "createtable", 0).unwrap();
    c.text("pets").unwrap();
    // An empty design says what is missing and stays open.
    c.key(1, "Enter", 0).unwrap();
    assert!(c.message.take().unwrap().contains("at least one field"));
    c.command(1, "design:add", 0).unwrap();
    // The new field's name is selected: typing replaces it.
    c.text("id").unwrap();
    c.key(1, "Enter", 0).unwrap();
    c.command(1, "design:cell:0:4", 0).unwrap(); // AI: an INTEGER primary key
    c.command(1, "design:add", 0).unwrap();
    c.text("name").unwrap();
    c.key(1, "Tab", 0).unwrap();
    c.text("TEXT").unwrap();
    c.key(1, "Enter", 0).unwrap();
    c.command(1, "design:cell:1:2", 0).unwrap(); // NN
    c.command(1, "design:add", 0).unwrap();
    c.text("owner").unwrap();
    c.key(1, "Enter", 0).unwrap();
    c.command(1, "menu:designtype:2", 0).unwrap();
    c.command(1, "design:type:2:INTEGER", 0).unwrap();
    c.command(1, "design:cell:2:9", 0).unwrap();
    c.text("\"people\"(\"id\")").unwrap();
    c.key(1, "Enter", 0).unwrap();
    // Move owner up above name.
    c.command(1, "design:up", 0).unwrap();
    let sql = c.design.as_ref().unwrap().create_sql("pets");
    assert_eq!(
        sql,
        "CREATE TABLE \"pets\" (\n\t\"id\"\tINTEGER,\n\t\"owner\"\tINTEGER,\n\t\"name\"\tTEXT NOT NULL,\n\tPRIMARY KEY(\"id\" AUTOINCREMENT),\n\tFOREIGN KEY(\"owner\") REFERENCES \"people\"(\"id\")\n)"
    );
    c.command(1, "design:ok", 0).unwrap();
    assert!(c.design.is_none(), "{:?}", c.message);
    assert!(c.modified());
    let db = c.db.as_mut().unwrap();
    db.execute("INSERT INTO pets (owner, name) VALUES (1, 'Rex')")
        .unwrap();
    assert_eq!(
        db.query("SELECT id, owner, name FROM pets").unwrap().rows,
        vec![vec![
            Value::Integer(1),
            Value::Integer(1),
            Value::Text("Rex".into())
        ]]
    );
    // The foreign key holds.
    assert!(db
        .execute("INSERT INTO pets (owner, name) VALUES (9, 'Stray')")
        .unwrap_err()
        .message
        .contains("FOREIGN KEY constraint failed"));
    // The tree lists it, selected.
    assert_eq!(c.tree_selected.as_deref(), Some("table:pets"));
}

#[test]
fn modify_table_rebuilds_when_alter_table_cannot_and_refuses_what_the_rows_break() {
    let mut c = client();
    c.command(1, "tree:table:people", 0).unwrap();
    c.command(1, "modifytable", 0).unwrap();
    // Linus has no age: NOT NULL on age cannot hold, and nothing changes.
    c.command(1, "design:cell:2:2", 0).unwrap();
    c.command(1, "design:ok", 0).unwrap();
    let m = c.message.take().unwrap();
    assert!(m.contains("Error altering table"), "{m}");
    assert!(m.contains("NOT NULL constraint failed"), "{m}");
    assert!(c.design.is_some(), "the dialog stays open");
    assert!(!c.modified());
    // Instead: age becomes REAL, and name moves last.
    c.command(1, "design:cell:2:2", 0).unwrap();
    c.activate(1, "db:design:cell:2:1", 0).unwrap();
    for _ in 0.."INTEGER".len() {
        c.key(1, "Backspace", 0).unwrap();
    }
    c.text("REAL").unwrap();
    c.key(1, "Enter", 0).unwrap();
    c.command(1, "design:cell:1:0", 0).unwrap();
    c.key(1, "Enter", 0).unwrap();
    c.command(1, "design:bottom", 0).unwrap();
    c.command(1, "design:ok", 0).unwrap();
    assert!(c.design.is_none(), "{:?}", c.message);
    let db = c.db.as_mut().unwrap();
    let info: Vec<(String, String)> = db
        .table_info("people")
        .unwrap()
        .into_iter()
        .map(|i| (i.name, i.decl_type))
        .collect();
    assert_eq!(
        info,
        vec![
            ("id".to_string(), "INTEGER".to_string()),
            ("age".into(), "REAL".into()),
            ("name".into(), "TEXT".into())
        ]
    );
    assert_eq!(
        db.query("SELECT id, name, age, typeof(age) FROM people ORDER BY id")
            .unwrap()
            .rows[1],
        vec![
            Value::Integer(2),
            Value::Text("Grace".into()),
            Value::Real(45.0),
            Value::Text("real".into())
        ]
    );
    // The index and the view came through.
    let names: Vec<String> = db.schema().into_iter().map(|e| e.name).collect();
    assert!(names.contains(&"people_name".to_string()), "{names:?}");
    assert_eq!(
        db.query("SELECT name FROM adults ORDER BY name")
            .unwrap()
            .rows
            .len(),
        2
    );
    assert_eq!(
        db.query("PRAGMA integrity_check").unwrap().rows[0][0],
        Value::Text("ok".into())
    );
}

#[test]
fn create_index_writes_the_index_the_dialog_describes() {
    let mut c = client();
    c.command(1, "tree:table:people", 0).unwrap();
    c.command(1, "createindex", 0).unwrap();
    c.text("people_age").unwrap();
    c.command(1, "index:col:age", 0).unwrap();
    c.command(1, "index:col:name", 0).unwrap();
    c.command(1, "index:order:0", 0).unwrap();
    c.command(1, "index:unique", 0).unwrap();
    // The engine has no partial indexes; the clause says so.
    assert!(c.command(1, "index:where", 0).is_err());
    assert_eq!(
        c.index_design.as_ref().unwrap().sql(),
        "CREATE UNIQUE INDEX \"people_age\" ON \"people\" (\n\t\"age\"\tDESC,\n\t\"name\"\tASC\n);"
    );
    c.key(1, "Enter", 0).unwrap();
    assert!(c.index_design.is_none(), "{:?}", c.message);
    let db = c.db.as_mut().unwrap();
    let plan = db
        .query("EXPLAIN QUERY PLAN SELECT name FROM people WHERE age = 36")
        .unwrap();
    assert!(
        format!("{:?}", plan.rows).contains("COVERING INDEX people_age"),
        "{:?}",
        plan.rows
    );
}

#[test]
fn tableplus_stages_structure_changes_until_commit() {
    let mut c = client();
    c.command(1, "table:people", 0).unwrap();
    c.command(1, "tab:structure", 0).unwrap();
    c.activate(1, "db:struct:cell:2:0", 0).unwrap();
    c.text("_years").unwrap();
    c.key(1, "Enter", 0).unwrap();
    c.command(1, "struct:addcol", 0).unwrap();
    c.text("email").unwrap();
    c.key(1, "Enter", 0).unwrap();
    assert!(c.modified());
    // Staged only: the database still has age.
    assert_eq!(
        c.db.as_ref().unwrap().table_info("people").unwrap()[2].name,
        "age"
    );
    let effects = c.key(1, "Meta+s", 0).unwrap();
    let bytes = effects
        .iter()
        .find_map(|e| match e {
            AppEffect::WriteBytes { bytes, .. } => Some(bytes.clone()),
            _ => None,
        })
        .unwrap();
    let back = cw_sql::Database::open(&bytes).unwrap();
    let cols: Vec<String> = back
        .table_info("people")
        .unwrap()
        .into_iter()
        .map(|i| i.name)
        .collect();
    assert_eq!(cols, ["id", "name", "age_years", "email"]);
    c.saved("/home/u/Documents/people.db", Ok(()));
    assert!(!c.modified());
    // Discard drops what is staged.
    c.command(1, "struct:cell:3:0", 0).unwrap();
    c.command(1, "struct:delcol", 0).unwrap();
    assert!(c.modified());
    c.command(1, "revert", 0).unwrap();
    assert!(!c.modified());
    assert_eq!(
        c.db.as_ref().unwrap().table_info("people").unwrap().len(),
        4
    );
}

#[test]
fn without_rowid_tables_are_browsed_and_edited_by_primary_key() {
    let mut c = client();
    c.db.as_mut()
        .unwrap()
        .execute(
            "CREATE TABLE codes (code TEXT, n INTEGER, label TEXT, PRIMARY KEY (code, n)) WITHOUT ROWID;
             INSERT INTO codes VALUES ('b', 1, 'bee'), ('a', 2, 'ay'), ('a', 1, 'aa');",
        )
        .unwrap();
    c.command(1, "table:codes", 0).unwrap();
    let rows = c.rows(0, 10).unwrap();
    let labels: Vec<Value> = rows.rows.iter().map(|(_, r)| r[2].clone()).collect();
    assert_eq!(
        labels,
        vec![
            Value::Text("aa".into()),
            Value::Text("ay".into()),
            Value::Text("bee".into())
        ]
    );
    c.activate(1, "db:cell:1:2", 0).unwrap();
    c.text("!").unwrap();
    c.key(1, "Enter", 0).unwrap();
    assert!(c.message.is_none(), "{:?}", c.message);
    let db = c.db.as_mut().unwrap();
    assert_eq!(
        db.query("SELECT label FROM codes WHERE code = 'a' AND n = 2")
            .unwrap()
            .rows,
        vec![vec![Value::Text("ay!".into())]]
    );
    c.command(1, "cell:0:0", 0).unwrap();
    c.command(1, "deleterow", 0).unwrap();
    assert_eq!(c.rows(0, 10).unwrap().total, 2);
    c.command(1, "newrow", 0).unwrap();
    assert!(c.message.is_none(), "{:?}", c.message);
    assert_eq!(c.rows(0, 10).unwrap().total, 3);
}
