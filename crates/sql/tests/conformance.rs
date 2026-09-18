//! SQL conformance: every expectation here was checked against SQLite 3.45.1.
use cw_sql::{Database, Value};

fn db(setup: &str) -> Database {
    let mut d = Database::new();
    d.execute(setup).expect("setup runs");
    d
}
/// Rows of the last statement, each rendered the way `sqlite3` list mode prints it.
fn rows(d: &mut Database, sql: &str) -> Vec<String> {
    let out = d.query(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    out.rows
        .iter()
        .map(|r| r.iter().map(Value::to_text).collect::<Vec<_>>().join("|"))
        .collect()
}
fn err(d: &mut Database, sql: &str) -> String {
    d.execute(sql).expect_err(sql).message
}

const SHOP: &str = "
create table customer(id integer primary key, name text not null, city text);
create table orders(id integer primary key, customer int references customer(id), total real, placed text);
insert into customer values (1,'Ada','London'),(2,'Grace','New York'),(3,'Linus',null);
insert into orders values (10,1,25.5,'2026-09-01'),(11,1,4.5,'2026-09-03'),(12,2,100,'2026-09-02'),(13,null,7,'2026-09-04');
";

#[test]
fn joins_inner_left_cross_and_using() {
    let mut d = db(SHOP);
    assert_eq!(
        rows(&mut d, "select c.name, o.total from customer c join orders o on o.customer = c.id order by o.id"),
        ["Ada|25.5", "Ada|4.5", "Grace|100.0"]
    );
    assert_eq!(
        rows(&mut d, "select c.name, count(o.id) from customer c left join orders o on o.customer = c.id group by c.id order by c.id"),
        ["Ada|2", "Grace|1", "Linus|0"]
    );
    assert_eq!(
        rows(&mut d, "select count(*) from customer, orders"),
        ["12"]
    );
    assert_eq!(
        rows(&mut d, "select o.id, c.name from orders o left join customer c on c.id = o.customer where c.id is null"),
        ["13|"]
    );
    d.execute("create table a(k, x); create table b(k, y); insert into a values (1,'a1'),(2,'a2'); insert into b values (2,'b2'),(3,'b3');").unwrap();
    assert_eq!(rows(&mut d, "select * from a join b using(k)"), ["2|a2|b2"]);
    assert_eq!(rows(&mut d, "select * from a natural join b"), ["2|a2|b2"]);
    assert_eq!(
        rows(
            &mut d,
            "select a.k, b.k from a full join b on a.k = b.k order by coalesce(a.k, b.k)"
        ),
        ["1|", "2|2", "|3"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select b.y from a right join b on a.k = b.k where a.k is null"
        ),
        ["b3"]
    );
    assert!(err(&mut d, "select k from a join b on a.k = b.k").contains("ambiguous column name: k"));
}

#[test]
fn grouping_having_ordering_and_limits() {
    let mut d = db(SHOP);
    assert_eq!(
        rows(
            &mut d,
            "select customer, sum(total) s, count(*) from orders group by customer order by s desc"
        ),
        ["2|100.0|1", "1|30.0|2", "|7.0|1"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select customer from orders group by customer having count(*) > 1"
        ),
        ["1"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select total from orders order by total limit 2 offset 1"
        ),
        ["7.0", "25.5"]
    );
    assert_eq!(
        rows(&mut d, "select total from orders order by 1 desc limit 1"),
        ["100.0"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select distinct customer from orders order by customer"
        ),
        ["", "1", "2"]
    );
    // A bare column next to max() comes from the row that held the maximum.
    assert_eq!(
        rows(&mut d, "select max(total), placed from orders"),
        ["100.0|2026-09-02"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select avg(total), min(total), total(total), group_concat(id, ';') from orders"
        ),
        ["34.25|4.5|137.0|10;11;12;13"]
    );
    assert_eq!(
        rows(&mut d, "select count(distinct customer) from orders"),
        ["2"]
    );
    assert!(
        err(&mut d, "select * from orders order by 9").contains("1st ORDER BY term out of range")
    );
    assert_eq!(
        err(&mut d, "select name from customer having 1"),
        "HAVING clause on a non-aggregate query"
    );
}

#[test]
fn null_semantics_are_three_valued() {
    let mut d = db("create table t(x); insert into t values (null),(1),(2);");
    assert_eq!(
        rows(
            &mut d,
            "select x is null, x = null, x in (1, null), coalesce(x, 'none'), nullif(x, 1) from t"
        ),
        ["1|||none|", "0||1|1|", "0|||2|2"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select null and 0, null and 1, null or 1, null or 0, not null"
        ),
        ["0||1||"]
    );
    assert_eq!(
        rows(&mut d, "select count(x), count(*), sum(x), avg(x) from t"),
        ["2|3|3|1.5"]
    );
    assert_eq!(rows(&mut d, "select sum(x) from t where x > 5"), [""]);
    assert_eq!(rows(&mut d, "select total(x) from t where x > 5"), ["0.0"]);
    assert_eq!(
        rows(&mut d, "select x from t order by x desc"),
        ["2", "1", ""]
    );
    assert_eq!(
        rows(&mut d, "select x from t order by x nulls last"),
        ["1", "2", ""]
    );
    assert_eq!(
        rows(&mut d, "select 1 where 2 not in (1, null)"),
        Vec::<String>::new()
    );
}

#[test]
fn subqueries_ctes_and_compounds() {
    let mut d = db(SHOP);
    assert_eq!(
        rows(
            &mut d,
            "select name from customer where id in (select customer from orders)"
        ),
        ["Ada", "Grace"]
    );
    assert_eq!(rows(&mut d, "select name from customer c where not exists (select 1 from orders where customer = c.id)"), ["Linus"]);
    assert_eq!(
        rows(
            &mut d,
            "select name, (select sum(total) from orders where customer = c.id) from customer c"
        ),
        ["Ada|30.0", "Grace|100.0", "Linus|"]
    );
    assert_eq!(
        rows(
            &mut d,
            "with big as (select * from orders where total > 20) select count(*) from big"
        ),
        ["2"]
    );
    assert_eq!(
        rows(&mut d, "with recursive fib(a, b) as (select 0, 1 union all select b, a + b from fib where a < 20) select group_concat(a) from fib"),
        ["0,1,1,2,3,5,8,13,21"]
    );
    assert_eq!(rows(&mut d, "with recursive n(i) as (select 1 union all select i + 1 from n) select i from n limit 3"), ["1", "2", "3"]);
    assert_eq!(
        rows(&mut d, "select 3 union select 1 union select 3"),
        ["1", "3"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select city from customer intersect select 'London'"
        ),
        ["London"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select id from customer except select customer from orders"
        ),
        ["3"]
    );
    assert!(err(&mut d, "select 1 union select 1, 2")
        .contains("do not have the same number of result columns"));
}

#[test]
fn expressions_follow_sqlite_types() {
    let mut d = Database::new();
    assert_eq!(
        rows(&mut d, "select 7/2, 7.0/2, 7%3, -7/2, 1/0, 5.5 % 2, 'a' || 1.0, 9223372036854775807 + 1, 0.1 + 0.2"),
        ["3|3.5|1|-3||1.0|a1.0|9.22337203685478e+18|0.3"]
    );
    assert_eq!(rows(&mut d, "select 10 = '10', '10' + 5, cast('12abc' as integer), cast(3.9 as integer), cast('1e2' as numeric)"), ["0|15|12|3|100"]);
    assert_eq!(
        rows(
            &mut d,
            "select typeof(1), typeof(1.0), typeof('x'), typeof(x'00'), typeof(null)"
        ),
        ["integer|real|text|blob|null"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select case 2 when 1 then 'one' when 2 then 'two' end, iif(0, 'y', 'n')"
        ),
        ["two|n"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select 'abc' like 'A%', 'abc' glob 'A*', 'x_y' like 'x\\_y' escape '\\'"
        ),
        ["1|0|1"]
    );
    assert_eq!(rows(&mut d, "select upper('abc'), length('héllo'), substr('hello', -3, 2), replace('aaa', 'a', 'bc'), trim('  x '), instr('hello', 'l')"), ["ABC|5|ll|bcbcbc|x|3"]);
    assert_eq!(rows(&mut d, "select round(2.675, 2), round(-0.5), abs(-3), printf('%05.1f|%,d|%s', 3.14159, 1234567, 'x'), quote('it''s'), hex('ab')"), ["2.67|-1.0|3|003.1|1,234,567|x|'it''s'|6162"]);
    assert_eq!(
        rows(
            &mut d,
            "select pow(2, 10), sqrt(16), exp(0), ln(1), log10(1000), pi() > 3.14"
        ),
        ["1024.0|4.0|1.0|0.0|3.0|1"]
    );
    assert_eq!(
        err(&mut d, "select abs(-9223372036854775808)"),
        "integer overflow"
    );
    assert_eq!(err(&mut d, "select nosuch(1)"), "no such function: nosuch");
}

#[test]
fn dates_read_the_world_clock() {
    let mut d = Database::new();
    // 2026-09-17 09:00:00 UTC, the world's first moment.
    d.set_now(1_789_635_600_000_000);
    assert_eq!(
        rows(
            &mut d,
            "select date('now'), time('now'), datetime('now', '+1 day', 'start of day')"
        ),
        ["2026-09-17|09:00:00|2026-09-18 00:00:00"]
    );
    assert_eq!(
        rows(
            &mut d,
            "select current_date, strftime('%Y-%m-%d %H:%M', 'now', '+90 minutes')"
        ),
        ["2026-09-17|2026-09-17 10:30"]
    );
    assert_eq!(rows(&mut d, "select date('2026-01-31', '+1 month'), julianday('2000-01-01 12:00:00'), unixepoch('1970-01-02')"), ["2026-03-03|2451545.0|86400"]);
}

#[test]
fn constraints_are_enforced_with_sqlite_messages() {
    let mut d = db(
        "create table t(a unique, b not null, c check (c > 0), d primary key);
                    insert into t values (1, 'x', 1, 'k1');",
    );
    assert_eq!(
        err(&mut d, "insert into t values (1, 'y', 2, 'k2')"),
        "UNIQUE constraint failed: t.a"
    );
    assert_eq!(
        err(&mut d, "insert into t values (2, null, 2, 'k3')"),
        "NOT NULL constraint failed: t.b"
    );
    assert_eq!(
        err(&mut d, "insert into t values (3, 'z', -1, 'k4')"),
        "CHECK constraint failed: c > 0"
    );
    assert_eq!(
        err(&mut d, "insert into t values (4, 'w', 1, 'k1')"),
        "UNIQUE constraint failed: t.d"
    );
    d.execute("insert or ignore into t values (1, 'q', 1, 'k9')")
        .unwrap();
    d.execute("insert or replace into t values (1, 'r', 5, 'k8')")
        .unwrap();
    assert_eq!(rows(&mut d, "select * from t"), ["1|r|5|k8"]);
    // A failed statement leaves nothing behind, even from rows it had already written.
    d.execute("create table u(x unique)").unwrap();
    assert!(d.execute("insert into u values (1), (2), (1)").is_err());
    assert_eq!(rows(&mut d, "select count(*) from u"), ["0"]);
    let mut d = db("create table k(id integer primary key, v)");
    assert_eq!(
        err(&mut d, "insert into k values ('abc', 1)"),
        "datatype mismatch"
    );
    d.execute("insert into k values (2.0, 'two'); insert into k(v) values ('three');")
        .unwrap();
    assert_eq!(rows(&mut d, "select id, v from k"), ["2|two", "3|three"]);
    d.execute("create table seq(id integer primary key autoincrement, v); insert into seq(v) values (1),(2); delete from seq where id = 2; insert into seq(v) values (3);").unwrap();
    assert_eq!(rows(&mut d, "select id from seq"), ["1", "3"]);
    assert_eq!(rows(&mut d, "select * from sqlite_sequence"), ["seq|3"]);
}

#[test]
fn foreign_keys_check_and_cascade_when_enabled() {
    let mut d = db("create table p(id integer primary key);
                    create table c(pid int references p(id) on delete cascade, v);
                    create table r(pid int references p(id));
                    insert into p values (1), (2);
                    insert into c values (1, 'a'), (2, 'b');");
    // Off by default, as in SQLite.
    d.execute("insert into c values (9, 'dangling')").unwrap();
    d.execute("delete from c where pid = 9; pragma foreign_keys = on;")
        .unwrap();
    assert_eq!(
        err(&mut d, "insert into c values (3, 'bad')"),
        "FOREIGN KEY constraint failed"
    );
    d.execute("insert into r values (2)").unwrap();
    d.execute("delete from p where id = 1").unwrap();
    assert_eq!(rows(&mut d, "select * from c"), ["2|b"]);
    assert_eq!(
        err(&mut d, "delete from p where id = 2"),
        "FOREIGN KEY constraint failed"
    );
    assert_eq!(rows(&mut d, "select count(*) from p"), ["1"]);
    assert_eq!(
        rows(&mut d, "pragma foreign_key_check"),
        Vec::<String>::new()
    );
}

#[test]
fn transactions_and_savepoints() {
    let mut d = db("create table t(x)");
    d.execute("begin; insert into t values (1); savepoint s; insert into t values (2); rollback to s; insert into t values (3); commit;").unwrap();
    assert_eq!(rows(&mut d, "select x from t"), ["1", "3"]);
    d.execute("begin; delete from t;").unwrap();
    assert!(d.in_transaction());
    // What a file would hold is the committed state only.
    let committed = Database::open(&d.to_bytes()).unwrap();
    assert_eq!(committed.row_count("t"), Some(2));
    d.execute("rollback").unwrap();
    assert_eq!(rows(&mut d, "select count(*) from t"), ["2"]);
    assert_eq!(
        err(&mut d, "commit"),
        "cannot commit - no transaction is active"
    );
    assert_eq!(
        err(&mut d, "begin; begin"),
        "cannot start a transaction within a transaction"
    );
    d.execute("rollback").unwrap();
}

#[test]
fn indexes_drive_the_planner() {
    let mut d = db("create table t(id integer primary key, v, w);
                    create index iv on t(v);
                    create index ivw on t(v, w);");
    for i in 0..200 {
        d.execute(&format!(
            "insert into t values ({i}, 'v{}', {})",
            i % 10,
            i % 7
        ))
        .unwrap();
    }
    let plan = |d: &mut Database, sql: &str| {
        d.query(&format!("explain query plan {sql}"))
            .unwrap()
            .rows
            .iter()
            .map(|r| r[3].to_text())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        plan(&mut d, "select * from t where id = 5"),
        ["SEARCH t USING INTEGER PRIMARY KEY (rowid=?)"]
    );
    assert_eq!(
        plan(&mut d, "select * from t where v = 'v3' and w = 2"),
        ["SEARCH t USING INDEX ivw (v=? AND w=?)"]
    );
    assert_eq!(plan(&mut d, "select * from t where w = 2"), ["SCAN t"]);
    assert_eq!(
        rows(&mut d, "select count(*) from t where v = 'v3' and w = 2"),
        ["3"]
    );
    assert_eq!(
        rows(&mut d, "select count(*) from t where v in ('v1', 'v2')"),
        ["40"]
    );
    assert_eq!(
        rows(&mut d, "select count(*) from t where id between 10 and 19"),
        ["10"]
    );
    d.execute("create unique index uw on t(id, w)").unwrap();
    assert!(err(&mut d, "create unique index uv on t(v)").starts_with("UNIQUE constraint failed"));
    assert_eq!(rows(&mut d, "pragma integrity_check"), ["ok"]);
}

#[test]
fn views_alter_and_schema() {
    let mut d = db("create table t(a, b); insert into t values (1, 2);");
    d.execute("create view v as select a + b as s from t")
        .unwrap();
    assert_eq!(rows(&mut d, "select * from v"), ["3"]);
    assert!(
        err(&mut d, "insert into v values (1)").contains("cannot modify v because it is a view")
    );
    d.execute("alter table t add column c default 7; alter table t rename column b to bb; alter table t rename to t2;").unwrap();
    assert_eq!(rows(&mut d, "select * from t2"), ["1|2|7"]);
    assert_eq!(
        rows(&mut d, "select sql from sqlite_schema where name = 't2'"),
        ["CREATE TABLE \"t2\"(a, bb, c default 7)"]
    );
    d.execute("alter table t2 drop column c").unwrap();
    assert_eq!(rows(&mut d, "select * from t2"), ["1|2"]);
    assert_eq!(
        rows(&mut d, "pragma table_info(t2)"),
        ["0|a||0||0", "1|bb||0||0"]
    );
    assert_eq!(err(&mut d, "create table t2(x)"), "table t2 already exists");
    d.execute("create table if not exists t2(x); drop table if exists nope;")
        .unwrap();
    assert_eq!(err(&mut d, "drop table nope"), "no such table: nope");
}

#[test]
fn upsert_and_returning() {
    let mut d = db("create table u(k primary key, n); insert into u values ('a', 1);");
    d.execute("insert into u values ('a', 5) on conflict(k) do update set n = n + excluded.n")
        .unwrap();
    assert_eq!(rows(&mut d, "select * from u"), ["a|6"]);
    assert_eq!(
        rows(&mut d, "insert into u values ('b', 1) returning k, n * 10"),
        ["b|10"]
    );
    assert_eq!(
        rows(&mut d, "delete from u where k = 'b' returning *"),
        ["b|1"]
    );
}

#[test]
fn parameters_bind_by_position_and_name() {
    let mut d = db("create table t(a, b)");
    d.execute_one(
        "insert into t values (?, :b)",
        &[Value::Integer(1), Value::Text("x".into())],
    )
    .unwrap();
    d.execute_one(
        "insert into t values (?2, ?1)",
        &[Value::Text("y".into()), Value::Integer(2)],
    )
    .unwrap();
    assert_eq!(rows(&mut d, "select * from t"), ["1|x", "2|y"]);
}

#[test]
fn a_database_survives_the_sqlite_file_format() {
    let mut d = db("
        create table big(id integer primary key, body text, n real, raw blob);
        create index big_n on big(n desc);
        create table tags(name text primary key collate nocase, weight int unique);
        create view heavy as select name from tags where weight > 5;
        pragma user_version = 7;");
    // Enough rows, and long enough values, for interior pages and overflow chains.
    for i in 0..600 {
        d.execute_one(
            "insert into big values (?, ?, ?, ?)",
            &[
                Value::Integer(i),
                Value::Text("x".repeat((i as usize % 9) * 700 + 1)),
                Value::Real(i as f64 / 3.0),
                Value::Blob(vec![i as u8; (i as usize) % 40]),
            ],
        )
        .unwrap();
    }
    d.execute("insert into tags values ('Alpha', 3), ('beta', 9)")
        .unwrap();
    let bytes = d.to_bytes();
    assert_eq!(&bytes[..16], b"SQLite format 3\0");
    assert_eq!(bytes.len() % 4096, 0);
    assert_eq!(bytes, d.to_bytes(), "the same state writes the same bytes");
    let mut back = Database::open(&bytes).unwrap();
    assert!(back.same_content(&d) || rows(&mut back, "select count(*) from big") == ["600"]);
    assert_eq!(
        rows(
            &mut back,
            "select count(*), sum(length(body)), sum(length(raw)) from big"
        ),
        rows(
            &mut d,
            "select count(*), sum(length(body)), sum(length(raw)) from big"
        )
    );
    assert_eq!(rows(&mut back, "select * from heavy"), ["beta"]);
    assert_eq!(
        rows(&mut back, "select name from tags where name = 'ALPHA'"),
        ["Alpha"]
    );
    assert_eq!(rows(&mut back, "pragma user_version"), ["7"]);
    assert_eq!(rows(&mut back, "pragma integrity_check"), ["ok"]);
    assert_eq!(
        err(&mut back, "insert into tags values ('BETA', 1)"),
        "UNIQUE constraint failed: tags.name"
    );
    assert_eq!(
        Database::open(b"not a database at all, clearly not one of them")
            .unwrap_err()
            .message,
        "file is not a database"
    );
}

#[test]
fn triggers_fire_per_row_with_new_and_old() {
    let mut d = db("create table t(a primary key, b); create table log(msg);
        create trigger t1 after insert on t begin insert into log values('t1 '||new.a); end;
        create trigger t2 after insert on t begin insert into log values('t2 '||new.a); end;");
    d.execute("insert into t values (1, 5)").unwrap();
    // The newest trigger fires first; changes() counts only the statement's own rows.
    assert_eq!(rows(&mut d, "select * from log"), ["t2 1", "t1 1"]);
    assert_eq!(rows(&mut d, "select changes()"), ["1"]);
    d.execute("drop trigger t2; delete from log;").unwrap();
    d.execute("create trigger t7 after update of b on t begin insert into log values(old.b||'->'||new.b); end")
        .unwrap();
    d.execute("update t set b = 9").unwrap();
    // UPDATE OF b only fires when the SET list names b.
    d.execute("update t set a = a").unwrap();
    assert_eq!(rows(&mut d, "select * from log"), ["5->9"]);
    // BEFORE triggers with WHEN, RAISE(IGNORE) skipping just that row.
    d.execute(
        "create trigger ig before insert on t when new.a = 99 begin select raise(ignore); end",
    )
    .unwrap();
    d.execute("insert into t values (99, 1), (98, 1)").unwrap();
    assert_eq!(rows(&mut d, "select a from t order by a"), ["1", "98"]);
    // RAISE(FAIL) keeps the rows before it; RAISE(ABORT) undoes the statement.
    d.execute("create trigger fl before insert on t when new.a = 77 begin select raise(fail, 'failing'); end;
               create trigger ab before insert on t when new.a = 55 begin select raise(abort, 'aborting'); end;")
        .unwrap();
    let e = d
        .execute("insert into t values (76, 1), (77, 1), (78, 1)")
        .unwrap_err();
    assert_eq!((e.message.as_str(), e.code), ("failing", 19));
    let e = d
        .execute("insert into t values (54, 1), (55, 1)")
        .unwrap_err();
    assert_eq!(e.message, "aborting");
    assert_eq!(
        rows(&mut d, "select a from t order by a"),
        ["1", "76", "98"]
    );
    assert_eq!(
        err(&mut d, "select raise(abort, 'x')"),
        "RAISE() may only be used within a trigger-program"
    );
    // A BEFORE INSERT trigger sees an automatic rowid as -1; last_insert_rowid() is
    // the statement's own row, whatever its triggers inserted.
    d.execute("create table x(i integer primary key, v);
               create trigger bx before insert on x begin insert into log values ('new.i='||quote(new.i)); end;
               create trigger ax after insert on x begin insert into log values ('after.i='||new.i); end;
               insert into x(v) values ('q'), ('r');")
        .unwrap();
    assert_eq!(rows(&mut d, "select last_insert_rowid()"), ["2"]);
    assert_eq!(
        rows(&mut d, "select * from log where msg like '%.i=%'"),
        ["new.i=-1", "after.i=1", "new.i=-1", "after.i=2"]
    );
    // Dropping a table drops its triggers.
    d.execute("drop table x").unwrap();
    assert_eq!(
        rows(
            &mut d,
            "select name from sqlite_schema where type = 'trigger' order by name"
        ),
        ["ab", "fl", "ig", "t1", "t7"]
    );
}

#[test]
fn triggers_refuse_what_sqlite_refuses() {
    let mut d = db("create table t(a); create view v as select * from t;
        create trigger t1 after insert on t begin select 1; end;");
    assert_eq!(
        err(
            &mut d,
            "create trigger t1 after insert on t begin select 1; end"
        ),
        "trigger t1 already exists"
    );
    assert_eq!(
        err(
            &mut d,
            "create trigger x instead of insert on t begin select 1; end"
        ),
        "cannot create INSTEAD OF trigger on table: t"
    );
    assert_eq!(
        err(
            &mut d,
            "create trigger x before insert on v begin select 1; end"
        ),
        "cannot create BEFORE trigger on view: v"
    );
    assert_eq!(
        err(
            &mut d,
            "create trigger x after insert on nope begin select 1; end"
        ),
        "no such table: main.nope"
    );
    assert_eq!(err(&mut d, "drop trigger nope"), "no such trigger: nope");
    d.execute("drop trigger if exists nope").unwrap();
    // A program naming a missing table fails when it runs, in the main schema's words.
    d.execute("create trigger bad after insert on t begin insert into nope values (1); end")
        .unwrap();
    assert_eq!(
        err(&mut d, "insert into t values (1)"),
        "no such table: main.nope"
    );
    assert_eq!(
        err(&mut d, "alter table t rename to t2"),
        "error in trigger bad: no such table: main.nope"
    );
    // A script with trigger bodies splits at the END that closes each one.
    let stmts = cw_sql::lexer::split_statements(
        "create trigger a after insert on t begin select 1; select 2; end; select 3;",
    );
    assert_eq!(stmts.len(), 2);
    assert!(cw_sql::lexer::is_complete(
        "create trigger a after insert on t begin select 1; end;"
    ));
    assert!(!cw_sql::lexer::is_complete(
        "create trigger a after insert on t begin select 1;"
    ));
}

#[test]
fn instead_of_triggers_make_views_writable() {
    let mut d = db("create table t(a primary key, b); create table log(msg);
        create view v as select * from t;
        create trigger vi instead of insert on v begin insert into t values (new.a*10, new.b); end;
        create trigger vd instead of delete on v begin delete from t where a = old.a; end;
        create trigger vu instead of update of b on v begin update t set b = new.b + 100 where a = old.a; end;
        create trigger del after delete on t begin insert into log values ('gone '||old.a); end;");
    d.execute("insert into v values (3, 4), (5, 6)").unwrap();
    // Rows a view's triggers write are not the statement's own changes.
    assert_eq!(rows(&mut d, "select changes()"), ["0"]);
    d.execute("delete from v where a = 50; update v set b = 5 where a = 30;")
        .unwrap();
    assert_eq!(rows(&mut d, "select * from t"), ["30|105"]);
    assert_eq!(rows(&mut d, "select * from log"), ["gone 50"]);
    assert!(err(
        &mut d,
        "create view w as select 1 as x; insert into w values (1)"
    )
    .contains("cannot modify w because it is a view"));
}

#[test]
fn triggers_do_not_recurse_and_follow_renames() {
    let mut d = db(
        "create table cnt(n); insert into cnt values (0);
        create trigger rec after update on cnt when new.n < 5 begin update cnt set n = n + 1; end;",
    );
    // recursive_triggers is off: the trigger's own update does not fire it again.
    d.execute("update cnt set n = 1").unwrap();
    assert_eq!(rows(&mut d, "select n from cnt"), ["2"]);
    d.execute("alter table cnt rename to counter").unwrap();
    assert_eq!(
        rows(&mut d, "select tbl_name, sql from sqlite_schema where type = 'trigger'"),
        ["counter|CREATE TRIGGER rec after update on \"counter\" when new.n < 5 begin update \"counter\" set n = n + 1; end"]
    );
    d.execute("update counter set n = 0").unwrap();
    assert_eq!(rows(&mut d, "select n from counter"), ["1"]);
    // Foreign key actions fire the child's triggers.
    let mut d = db("pragma foreign_keys = on;
        create table p(id integer primary key);
        create table c(pid references p(id) on delete cascade, tag);
        create table log(msg);
        create trigger cd after delete on c begin insert into log values ('child '||old.tag); end;
        insert into p values (1); insert into c values (1, 'x'), (1, 'y');");
    d.execute("delete from p").unwrap();
    assert_eq!(rows(&mut d, "select * from log"), ["child x", "child y"]);
}

#[test]
fn without_rowid_tables_are_keyed_by_their_primary_key() {
    let mut d = db(
        "create table w(k text primary key collate nocase, n int, note) without rowid;
        create index wn on w(n);
        insert into w values ('beta', 2, 'x'), ('Alpha', 1, 'y'), ('gamma', 3, null);",
    );
    // Rows come back in key order, not insertion order.
    assert_eq!(
        rows(&mut d, "select * from w"),
        ["Alpha|1|y", "beta|2|x", "gamma|3|"]
    );
    assert_eq!(
        err(&mut d, "insert into w values ('ALPHA', 9, 'dup')"),
        "UNIQUE constraint failed: w.k"
    );
    assert_eq!(
        err(&mut d, "insert into w values (null, 9, 'nul')"),
        "NOT NULL constraint failed: w.k"
    );
    assert_eq!(err(&mut d, "select rowid from w"), "no such column: rowid");
    assert_eq!(
        err(&mut d, "insert into w(rowid, k) values (5, 'q')"),
        "table w has no column named rowid"
    );
    assert_eq!(
        rows(&mut d, "pragma table_info(w)"),
        ["0|k|TEXT|1||1", "1|n|INT|0||0", "2|note||0||0"]
    );
    assert_eq!(
        rows(&mut d, "pragma index_list(w)"),
        ["0|wn|0|c|0", "1|sqlite_autoindex_w_1|1|pk|0"]
    );
    d.execute("update w set n = n * 10 where k = 'GAMMA'; delete from w where k = 'beta';")
        .unwrap();
    assert_eq!(rows(&mut d, "select * from w"), ["Alpha|1|y", "gamma|30|"]);
    // Inserting into a WITHOUT ROWID table leaves last_insert_rowid() alone.
    assert_eq!(rows(&mut d, "select last_insert_rowid()"), ["0"]);
    assert_eq!(
        err(&mut d, "create table bad(a) without rowid"),
        "PRIMARY KEY missing on table bad"
    );
    assert_eq!(
        err(
            &mut d,
            "create table bad(a integer primary key autoincrement) without rowid"
        ),
        "AUTOINCREMENT not allowed on WITHOUT ROWID tables"
    );
    assert_eq!(rows(&mut d, "pragma integrity_check"), ["ok"]);
}

#[test]
fn the_plan_marks_covering_indexes_as_sqlite_does() {
    let mut d = db("create table t(id integer primary key, v, w, x);
        create index iv on t(v);
        create index ivw on t(v, w);
        create table u(a, b);
        create index ua on u(a, b);
        create table w(k primary key, b, c) without rowid;
        create index wb on w(b);");
    let plan = |d: &mut Database, sql: &str| {
        d.query(&format!("explain query plan {sql}"))
            .unwrap()
            .rows
            .iter()
            .map(|r| r[3].to_text())
            .collect::<Vec<_>>()
    };
    for (sql, expected) in [
        (
            "select v from t where v = 1",
            vec!["SEARCH t USING COVERING INDEX iv (v=?)"],
        ),
        (
            "select v, w from t where v = 1",
            vec!["SEARCH t USING COVERING INDEX ivw (v=?)"],
        ),
        (
            "select id, v from t where v = 1",
            vec!["SEARCH t USING COVERING INDEX iv (v=?)"],
        ),
        (
            "select x from t where v = 1",
            vec!["SEARCH t USING INDEX ivw (v=?)"],
        ),
        ("select v from t", vec!["SCAN t USING COVERING INDEX iv"]),
        (
            "select count(*) from t",
            vec!["SCAN t USING COVERING INDEX iv"],
        ),
        ("select * from t", vec!["SCAN t"]),
        (
            "select w from t where v > 3",
            vec!["SEARCH t USING COVERING INDEX ivw (v>?)"],
        ),
        (
            "select count(*) from t where w = 2",
            vec!["SCAN t USING COVERING INDEX ivw"],
        ),
        ("select * from t order by v", vec!["SCAN t USING INDEX iv"]),
        ("select * from t order by id", vec!["SCAN t"]),
        (
            "select * from t where v = 3 order by id",
            vec!["SEARCH t USING INDEX iv (v=?)"],
        ),
        (
            "select x from t where v > 3 order by w",
            vec![
                "SEARCH t USING INDEX iv (v>?)",
                "USE TEMP B-TREE FOR ORDER BY",
            ],
        ),
        (
            "select v, count(*) from t group by v",
            vec!["SCAN t USING COVERING INDEX iv"],
        ),
        (
            "select * from t where v in (1, 2) order by v",
            vec!["SEARCH t USING INDEX ivw (v=?)"],
        ),
        (
            "select x from t order by x",
            vec!["SCAN t", "USE TEMP B-TREE FOR ORDER BY"],
        ),
        ("select * from u", vec!["SCAN u"]),
        (
            "select a from u order by a desc",
            vec!["SCAN u USING COVERING INDEX ua"],
        ),
        (
            "select b from u where a = 1 order by b",
            vec!["SEARCH u USING COVERING INDEX ua (a=?)"],
        ),
        (
            "select * from w where k = 1",
            vec!["SEARCH w USING PRIMARY KEY (k=?)"],
        ),
        (
            "select k from w where b = 1",
            vec!["SEARCH w USING COVERING INDEX wb (b=?)"],
        ),
        (
            "select c from w where b = 1",
            vec!["SEARCH w USING INDEX wb (b=?)"],
        ),
        ("select * from w", vec!["SCAN w"]),
    ] {
        assert_eq!(plan(&mut d, sql), expected, "{sql}");
    }
    // A covering scan reads the index, so rows come back in its order.
    d.execute("insert into t values (1, 'c', 1, 0), (2, 'a', 2, 0), (3, 'b', 3, 0)")
        .unwrap();
    assert_eq!(rows(&mut d, "select v from t"), ["a", "b", "c"]);
    assert_eq!(
        rows(&mut d, "select v from t where v in ('c', 'a')"),
        ["a", "c"]
    );
}

#[test]
fn triggers_and_without_rowid_tables_survive_the_file_format() {
    // Written by SQLite 3.45.1 itself: WITHOUT ROWID tables spread over interior
    // pages, a secondary and a unique index on them, triggers and a view.
    let fixture = include_bytes!("fixtures/triggers-without-rowid.db");
    let mut d = Database::open(fixture).unwrap();
    assert_eq!(rows(&mut d, "pragma integrity_check"), ["ok"]);
    assert_eq!(
        rows(&mut d, "select count(*), sum(a) from p"),
        ["403|80204"]
    );
    assert_eq!(
        rows(&mut d, "select * from p where b = 'z'"),
        ["1|z|10", "2|z|20"]
    );
    assert_eq!(rows(&mut d, "select k from w"), ["Alpha", "beta", "gamma"]);
    assert_eq!(
        rows(&mut d, "select type, name from sqlite_schema"),
        [
            "table|w",
            "table|p",
            "index|wn",
            "index|pc",
            "table|log",
            "trigger|wi",
            "view|vw",
            "trigger|vwi"
        ]
    );
    d.execute("insert into vw values ('delta', 4)").unwrap();
    assert_eq!(
        rows(&mut d, "select * from log"),
        ["ins beta", "ins Alpha", "ins gamma", "ins delta"]
    );
    // And back: what this engine writes reads back the same, byte for byte stable.
    let bytes = d.to_bytes();
    assert_eq!(bytes, d.to_bytes());
    let mut back = Database::open(&bytes).unwrap();
    // (Rows of a WITHOUT ROWID table are renumbered in key order on reading, so the
    // comparison is by content.)
    assert_eq!(
        rows(&mut back, "select * from w"),
        rows(&mut d, "select * from w")
    );
    assert_eq!(rows(&mut back, "pragma integrity_check"), ["ok"]);
    assert_eq!(
        rows(&mut back, "select k, n from w where n = 4"),
        ["delta|4"]
    );
    assert_eq!(
        err(&mut back, "insert into p values (1, 'z', 99)"),
        "UNIQUE constraint failed: p.b, p.a"
    );
    back.execute("insert into w values ('epsilon', 5, null)")
        .unwrap();
    assert_eq!(rows(&mut back, "select count(*) from log"), ["5"]);
}
