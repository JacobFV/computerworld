//! The sample database seeded into users' Documents folders is written by this engine:
//! `worlds/company-2026/files/Inventory.db` must be exactly these bytes. Regenerate it
//! with `CW_UPDATE_SAMPLES=1 cargo test -p cw-sql --test samples`, then run
//! `scripts/build-content.sh` to seed it.
use cw_sql::{Database, Value};
use std::path::PathBuf;

/// 2026-09-17 09:00:00 UTC, the world's first tick.
const EPOCH_UNIX_US: i64 = 1_789_635_600_000_000;

const SCHEMA: &str = "
CREATE TABLE suppliers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    country TEXT NOT NULL,
    email TEXT
);
CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    sku TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    supplier_id INTEGER REFERENCES suppliers(id),
    price REAL NOT NULL CHECK (price >= 0),
    stock INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX products_supplier ON products(supplier_id);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    product_id INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    ordered_at TEXT NOT NULL
);
CREATE INDEX orders_product ON orders(product_id);
CREATE VIEW low_stock AS
    SELECT sku, name, stock FROM products WHERE stock < 20 ORDER BY stock;
CREATE VIEW sales_by_product AS
    SELECT p.name AS product, sum(o.quantity) AS units, round(sum(o.quantity * p.price), 2) AS revenue
    FROM orders o JOIN products p ON p.id = o.product_id
    GROUP BY p.id ORDER BY revenue DESC;
INSERT INTO suppliers (name, country, email) VALUES
    ('Acme Components', 'United States', 'orders@acme.example'),
    ('Nordic Parts AB', 'Sweden', 'sales@nordicparts.example'),
    ('Kyoto Precision', 'Japan', NULL),
    ('Bavaria Werkzeug', 'Germany', 'info@bavaria.example');
INSERT INTO products (sku, name, supplier_id, price, stock) VALUES
    ('WID-100', 'Widget', 1, 19.99, 140),
    ('GAD-200', 'Gadget', 1, 34.50, 12),
    ('GIZ-300', 'Gizmo', 2, 12.75, 66),
    ('DOO-400', 'Doohickey', 3, 8.25, 5),
    ('SPR-500', 'Sprocket', 4, 3.10, 410),
    ('FLG-600', 'Flange', 4, 7.80, 18),
    ('BRK-700', 'Bracket', 2, 5.45, 0),
    ('HNG-800', 'Hinge', 3, 2.99, 250);
";

pub fn inventory() -> Database {
    let mut db = Database::new();
    db.set_now(EPOCH_UNIX_US);
    db.execute("PRAGMA foreign_keys = ON").unwrap();
    db.execute(SCHEMA).unwrap();
    // A month of orders, a fixed sequence so the file is the same every time.
    let mut seed: u32 = 17;
    for day in 1..=30u32 {
        for _ in 0..(1 + day % 3) {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let product = 1 + i64::from((seed >> 10) % 8);
            let quantity = 1 + i64::from((seed >> 16) % 24);
            db.execute_one(
                "INSERT INTO orders (product_id, quantity, ordered_at) VALUES (?, ?, ?)",
                &[
                    Value::Integer(product),
                    Value::Integer(quantity),
                    Value::Text(format!(
                        "2026-08-{day:02} {:02}:{:02}:00",
                        8 + (seed >> 4) % 10,
                        (seed >> 20) % 60
                    )),
                ],
            )
            .unwrap();
        }
    }
    db.execute("PRAGMA foreign_keys = OFF").unwrap();
    db
}

#[test]
fn the_seeded_inventory_database_is_what_the_engine_writes() {
    let db = inventory();
    let bytes = db.to_bytes();
    // A real SQLite file that reads back whole.
    assert_eq!(&bytes[..16], b"SQLite format 3\0");
    let mut back = Database::open(&bytes).unwrap();
    let check = back.query("PRAGMA integrity_check").unwrap();
    assert_eq!(check.rows, vec![vec![Value::Text("ok".into())]]);
    let low = back.query("SELECT sku FROM low_stock").unwrap();
    assert_eq!(low.rows.len(), 4);
    let orders = back.query("SELECT count(*) FROM orders").unwrap();
    assert_eq!(orders.rows, vec![vec![Value::Integer(60)]]);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../worlds/company-2026/files/Inventory.db");
    if std::env::var_os("CW_UPDATE_SAMPLES").is_some() {
        std::fs::write(&path, &bytes).unwrap();
        return;
    }
    assert!(
        std::fs::read(&path).unwrap_or_default() == bytes,
        "Inventory.db is not what the engine writes; regenerate with CW_UPDATE_SAMPLES=1"
    );
}
