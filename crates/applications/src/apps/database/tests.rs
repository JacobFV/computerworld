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
    p.scene
        .nodes
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
    let mut message = base;
    message.message = Some("Something\nto say".into());
    out.push(message);
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
