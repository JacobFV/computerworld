//! The `sqlite3` shell, checked against the output of SQLite 3.45.1's own shell.
use cw_sql::cli::{run, CliResult, Host};
use std::collections::BTreeMap;

#[derive(Default)]
struct Files(BTreeMap<String, Vec<u8>>);
impl Host for Files {
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.0.get(path).cloned())
    }
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.0.insert(path.into(), bytes.to_vec());
        Ok(())
    }
}
fn sh(files: &mut Files, args: &[&str], stdin: &str) -> CliResult {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    run(&args, stdin, files, 1_789_635_600_000_000)
}

#[test]
fn arguments_run_sql_and_persist_to_the_file() {
    let mut f = Files::default();
    let r = sh(
        &mut f,
        &[
            "shop.db",
            "create table t(a, b); insert into t values (1, 'x'), (2, 'y');",
        ],
        "",
    );
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
    assert!(f.0["shop.db"].starts_with(b"SQLite format 3\0"));
    let r = sh(&mut f, &["shop.db", "select * from t order by a desc"], "");
    assert_eq!(r.stdout, "2|y\n1|x\n");
    // Reading never creates a file that did not exist.
    let r = sh(&mut f, &["new.db", "select 1"], "");
    assert_eq!(r.stdout, "1\n");
    assert!(!f.0.contains_key("new.db"));
    let r = sh(
        &mut f,
        &[
            "-header",
            "-csv",
            "shop.db",
            "select a, b as \"b b\" from t",
        ],
        "",
    );
    assert_eq!(r.stdout, "a,\"b b\"\r\n1,x\r\n2,y\r\n");
}

#[test]
fn piped_scripts_mix_sql_and_dot_commands() {
    let mut f = Files::default();
    let script = "create table emp(id integer primary key, name text, dept text);\n\
                  insert into emp(name, dept) values ('Ada', 'eng'),\n  ('Bob', 'ops');\n\
                  create view eng as select name from emp where dept = 'eng';\n\
                  .tables\n\
                  .schema emp\n\
                  .headers on\n\
                  .mode column\n\
                  select * from emp;\n\
                  .mode box\n\
                  select name from eng;\n\
                  .quit\n\
                  select 'never reached';\n";
    let r = sh(&mut f, &["team.db"], script);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(
        r.stdout,
        "emp  eng\n\
         CREATE TABLE emp(id integer primary key, name text, dept text);\n\
         id  name  dept\n\
         --  ----  ----\n\
         1   Ada   eng \n\
         2   Bob   ops \n\
         ┌──────┐\n\
         │ name │\n\
         ├──────┤\n\
         │ Ada  │\n\
         └──────┘\n"
    );
}

#[test]
fn errors_are_reported_like_the_real_shell() {
    let mut f = Files::default();
    let r = sh(
        &mut f,
        &["x.db"],
        "select 1;\nselect * from nope;\nselec 2;\nselect 3;\n",
    );
    assert_eq!(r.stdout, "1\n3\n");
    assert_eq!(
        r.stderr,
        "Parse error near line 2: no such table: nope\n\
         Parse error near line 3: near \"selec\": syntax error\n  selec 2;\n  ^--- error here\n"
    );
    assert_eq!(r.code, 1);
    let r = sh(
        &mut f,
        &[
            ":memory:",
            "create table t(a unique)",
            "insert into t values (1),(1)",
            "select 'skipped'",
        ],
        "",
    );
    assert_eq!(
        r.stderr,
        "Error: stepping, UNIQUE constraint failed: t.a (19)\n"
    );
    assert_eq!((r.stdout.as_str(), r.code), ("", 1));
    let r = sh(&mut f, &["-bogus", "x.db"], "");
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("unknown option: -bogus"));
    let r = sh(&mut f, &[":memory:", ".frobnicate"], "");
    assert!(r
        .stderr
        .contains("unknown command or invalid arguments:  \"frobnicate\""));
}

#[test]
fn import_creates_a_table_from_a_csv_header() {
    let mut f = Files::default();
    f.0.insert(
        "items.csv".into(),
        b"name,qty,\"note, with comma\"\nwidget,3,\"say \"\"hi\"\"\"\ngadget,4\n".to_vec(),
    );
    let r = sh(&mut f, &["inv.db"], ".import --csv items.csv items\n.schema\nselect name, qty * 2, \"note, with comma\" from items;\n");
    assert_eq!(
        r.stderr,
        "items.csv:3: expected 3 columns but found 2 - filling the rest with NULL\n"
    );
    // Warnings are not errors: the import succeeded.
    assert_eq!(r.code, 0);
    assert_eq!(
        r.stdout,
        "CREATE TABLE IF NOT EXISTS \"items\"(\n\"name\" TEXT, \"qty\" TEXT, \"note, with comma\" TEXT);\n\
         widget|6|say \"hi\"\n\
         gadget|8|\n"
    );
}

#[test]
fn output_modes_match_sqlite() {
    let mut f = Files::default();
    let setup = "create table t(a integer, b real, c text); insert into t values (1, 2.5, 'x'), (12345, null, 'yy');";
    let r = sh(&mut f, &["-json", ":memory:", setup, "select * from t"], "");
    assert_eq!(
        r.stdout,
        "[{\"a\":1,\"b\":2.5,\"c\":\"x\"},\n{\"a\":12345,\"b\":null,\"c\":\"yy\"}]\n"
    );
    let r = sh(
        &mut f,
        &["-line", ":memory:", setup, "select a, c from t limit 1"],
        "",
    );
    assert_eq!(r.stdout, "    a = 1\n    c = x\n");
    let r = sh(
        &mut f,
        &["-table", ":memory:", setup, "select * from t"],
        "",
    );
    assert_eq!(
        r.stdout,
        "+-------+-----+----+\n|   a   |  b  | c  |\n+-------+-----+----+\n| 1     | 2.5 | x  |\n| 12345 |     | yy |\n+-------+-----+----+\n"
    );
    let r = sh(
        &mut f,
        &["-markdown", ":memory:", setup, "select a from t"],
        "",
    );
    assert_eq!(r.stdout, "|   a   |\n|-------|\n| 1     |\n| 12345 |\n");
    let r = sh(
        &mut f,
        &[
            ":memory:",
            setup,
            ".mode insert t",
            "select * from t limit 1",
            ".mode quote",
            "select c, b from t",
        ],
        "",
    );
    assert_eq!(
        r.stdout,
        "INSERT INTO t VALUES(1,2.5,'x');\n'x',2.5\n'yy',NULL\n"
    );
    let r = sh(
        &mut f,
        &[
            ":memory:",
            setup,
            "explain query plan select * from t where rowid = 1",
        ],
        "",
    );
    assert_eq!(
        r.stdout,
        "QUERY PLAN\n`--SEARCH t USING INTEGER PRIMARY KEY (rowid=?)\n"
    );
}

#[test]
fn dump_and_read_round_trip_a_database() {
    let mut f = Files::default();
    sh(&mut f, &["a.db", "create table t(id integer primary key, v text); insert into t values (1, 'it''s'); create index tv on t(v);"], "");
    let r = sh(&mut f, &["a.db", ".dump"], "");
    assert_eq!(
        r.stdout,
        "PRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;\nCREATE TABLE t(id integer primary key, v text);\nINSERT INTO t VALUES(1,'it''s');\nCREATE INDEX tv on t(v);\nCOMMIT;\n"
    );
    f.0.insert("dump.sql".into(), r.stdout.into_bytes());
    let r = sh(&mut f, &["b.db", ".read dump.sql", "select v from t"], "");
    assert_eq!(r.stdout, "it's\n");
    // An open transaction is rolled back when the shell exits, as closing does.
    sh(&mut f, &["b.db"], "begin;\ndelete from t;\n");
    assert_eq!(
        sh(&mut f, &["b.db", "select count(*) from t"], "").stdout,
        "1\n"
    );
}
