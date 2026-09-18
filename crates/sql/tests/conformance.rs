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
