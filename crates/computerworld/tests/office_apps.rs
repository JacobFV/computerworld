//! The spreadsheets and the SQLite clients end to end, driven the way a person drives
//! them: pointer clicks and drags on the painted grid, typed formulas, ribbon and menu
//! commands, and files that land on the machine in formats other programs read.
use computerworld::{reference_world, World};
use cw_applications::apps::database::Client;
use cw_applications::apps::sheet::Book;
use cw_applications::{AppState, NativeApp};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use cw_scene::Rect;
use cw_sheet::Cell;
use serde_json::{json, Value};

const W: u32 = 1280;
const H: u32 = 800;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../worlds/company-2026/files/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

struct Session {
    world: World,
    actor: String,
    machine: &'static str,
}

fn session(theme: &str, apps: &[&str]) -> Session {
    let machine = "alice-mac";
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ machine: theme });
    let c = definition
        .computers
        .iter_mut()
        .find(|c| c.id == machine)
        .unwrap();
    c.installed_apps.extend(apps.iter().map(|a| a.to_string()));
    // The documents the reference desktops are seeded with.
    c.initial_binary_files.insert(
        "Documents/Budget.xlsx".into(),
        base64(&sample("Budget.xlsx")),
    );
    c.initial_binary_files.insert(
        "Documents/Inventory.db".into(),
        base64(&sample("Inventory.db")),
    );
    c.initial_files.insert(
        "Documents/Sales.csv".into(),
        String::from_utf8(sample("Sales.csv")).unwrap(),
    );
    let mut world = World::new(definition, 7).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", machine))
        .unwrap();
    Session {
        world,
        actor,
        machine,
    }
}

impl Session {
    fn try_act(&mut self, family: &str, op: &str, payload: Value) -> Result<Value, String> {
        let result = self
            .world
            .step(
                &self.actor,
                vec![ActionEnvelope::new(family, op, self.machine, payload)],
            )
            .unwrap();
        let outcome = &result.outcomes[0];
        if outcome.success {
            Ok(outcome.value.clone())
        } else {
            Err(format!("{:?}", outcome.error))
        }
    }
    fn act(&mut self, family: &str, op: &str, payload: Value) -> Value {
        self.try_act(family, op, payload.clone())
            .unwrap_or_else(|e| panic!("{family} {op} {payload}: {e}"))
    }
    fn launch(&mut self, kind: &str, argument: &str) -> u64 {
        let v = self.act(
            "application.v1",
            "launch",
            json!({"kind": kind, "argument": argument}),
        );
        v["window"].as_u64().unwrap()
    }
    fn home(&self) -> String {
        self.world
            .interfaces()
            .session(&self.actor)
            .unwrap()
            .machines[self.machine]
            .desktop
            .home_folder()
    }
    /// Screen bounds of the control whose interaction ends in `target`.
    fn find(&self, target: &str) -> Option<(Rect, String)> {
        let scene = self.world.scene(&self.actor, W, H).unwrap();
        let suffix = format!(":content:{target}");
        scene.nodes.iter().rev().find_map(|n| {
            let t = n.interaction.as_deref()?;
            (t == target
                || t.ends_with(&suffix)
                || (target.ends_with(':') && t.contains(&format!(":content:{target}"))))
            .then(|| (n.transform.bounds(n.bounds), t.to_owned()))
        })
    }
    fn bounds(&self, target: &str) -> Rect {
        self.find(target)
            .unwrap_or_else(|| panic!("nothing on screen does {target}"))
            .0
    }
    fn pointer(&mut self, op: &str, (x, y): (i32, i32)) {
        self.act(
            "pointer.v1",
            op,
            json!({"x": x, "y": y, "width": W, "height": H}),
        );
    }
    /// Click the middle of the control that does `target`, as a person would.
    fn click(&mut self, target: &str) {
        let r = self.bounds(target);
        self.pointer(
            "click",
            (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2),
        );
    }
    fn double_click(&mut self, target: &str) {
        let r = self.bounds(target);
        let at = (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
        self.pointer("click", at);
        self.pointer("double_click", at);
    }
    fn drag(&mut self, points: &[(i32, i32)]) {
        self.pointer("down", points[0]);
        for p in &points[1..] {
            self.pointer("move", *p);
        }
        self.pointer("up", *points.last().unwrap());
    }
    fn key(&mut self, key: &str) {
        self.act("keyboard.v1", "key", json!({ "key": key }));
    }
    fn type_text(&mut self, text: &str) {
        self.act("keyboard.v1", "type", json!({ "text": text }));
    }
    fn state(&self, window: u64) -> AppState {
        self.world
            .interfaces()
            .session(&self.actor)
            .unwrap()
            .machines[self.machine]
            .desktop
            .windows[&window]
            .state
            .clone()
    }
    fn book(&self, window: u64) -> Book {
        match self.state(window) {
            AppState::Native(NativeApp::Spreadsheet(a)) => *a.0,
            AppState::Native(NativeApp::Excel(a)) => *a.0,
            other => panic!("not a spreadsheet: {other:?}"),
        }
    }
    fn client(&self, window: u64) -> Client {
        match self.state(window) {
            AppState::Native(NativeApp::Database(a)) => *a.0,
            other => panic!("not a database client: {other:?}"),
        }
    }
    fn file(&self, path: &str) -> Vec<u8> {
        self.world.runtime().read_file(self.machine, path).unwrap()
    }
    /// Screen centre of cell `(row, col)` on the grid as painted (no scrolling, default
    /// widths), from the grid surface's own geometry.
    fn cell(&self, row: u32, col: u32) -> (i32, i32) {
        let (r, target) = self.find("sheet:grid:").expect("a grid on screen");
        let mut parts = target.rsplit(':');
        let scale: u32 = parts.next().unwrap().parse().unwrap();
        let row_h: i32 = parts.next().unwrap().parse().unwrap();
        let col_w = (64 * scale / 100) as i32;
        (
            r.x + col as i32 * col_w + col_w / 2,
            r.y + row as i32 * row_h + row_h / 2,
        )
    }
}

#[test]
fn excel_takes_formulas_fills_down_by_the_handle_charts_and_saves_xlsx() {
    let mut s = session("virtual-windows-11", &["spreadsheet"]);
    let home = s.home();
    let w = s.launch("spreadsheet", "");
    // Excel opens on its start page, listing the workbooks in Documents.
    assert!(s.find("sheet:openfile:Budget.xlsx").is_some());
    s.click("sheet:new:excel");
    assert_eq!(s.book(w).name, "Book1");
    // Type a column of numbers, pressing Enter after each, as a person does.
    s.pointer("click", s.cell(0, 0));
    for n in ["10", "20", "30"] {
        s.type_text(n);
        s.key("Enter");
    }
    s.pointer("click", s.cell(0, 1));
    s.type_text("=A1*2");
    s.key("Enter");
    assert_eq!(s.book(w).workbook.display(0, Cell::new(0, 1)), "20");
    // Drag B1's fill handle down to B3: the formula follows, relative references move.
    s.pointer("click", s.cell(0, 1));
    let handle = s.bounds("sheet:fill:");
    s.drag(&[
        (
            handle.x + handle.width as i32 / 2,
            handle.y + handle.height as i32 / 2,
        ),
        s.cell(1, 1),
        s.cell(2, 1),
    ]);
    let book = s.book(w);
    assert_eq!(book.workbook.input(0, Cell::new(2, 1)), "=A3*2");
    assert_eq!(book.workbook.display(0, Cell::new(2, 1)), "60");
    // A total by AutoSum, confirmed with Enter.
    s.pointer("click", s.cell(3, 0));
    s.click("sheet:autosum");
    s.key("Enter");
    assert_eq!(s.book(w).workbook.input(0, Cell::new(3, 0)), "=SUM(A1:A3)");
    assert_eq!(s.book(w).workbook.display(0, Cell::new(3, 0)), "60");
    // Drag across the block: the status bar sums what is selected.
    s.drag(&[s.cell(0, 0), s.cell(1, 1), s.cell(2, 1)]);
    let book = s.book(w);
    assert_eq!(book.selection().a1(), "A1:B3");
    assert_eq!(book.active, Cell::new(0, 0));
    // Insert a column chart of it from the ribbon.
    s.click("sheet:ribbon:insert");
    s.click("sheet:chart:column");
    assert_eq!(s.book(w).workbook.sheets[0].charts.len(), 1);
    assert!(
        s.find("sheet:chartsel:0").is_some(),
        "the chart is on the sheet"
    );
    // Ctrl+S writes a real workbook into Documents.
    s.key("Ctrl+s");
    let path = format!("{home}/Documents/Book1.xlsx");
    let back = cw_sheet::xlsx::read(&s.file(&path)).unwrap();
    assert_eq!(back.input(0, Cell::new(2, 1)), "=A3*2");
    assert_eq!(back.display(0, Cell::new(3, 0)), "60");
    assert_eq!(back.sheets[0].charts.len(), 1);
    assert!(!s.book(w).modified);
    // Undo takes the chart back off; the saved file keeps it.
    s.click("sheet:ribbon:home");
    s.click("sheet:undo");
    assert!(s.book(w).workbook.sheets[0].charts.is_empty());
}

#[test]
fn calc_and_numbers_open_the_seeded_documents_and_keep_their_formats() {
    let mut s = session("virtual-ubuntu-24", &["spreadsheet"]);
    let home = s.home();
    let w = s.launch("spreadsheet", "");
    s.click("sheet:openfile:Budget.xlsx");
    let book = s.book(w);
    assert_eq!(book.workbook.display(0, Cell::new(7, 4)), "$8,738.85");
    assert_eq!(book.workbook.display(1, Cell::new(2, 1)), "Rent");
    // Change a figure: every total that depends on it follows.
    s.click("sheet:namebox");
    s.type_text("B2");
    s.key("Enter");
    s.type_text("1900");
    s.key("Enter");
    let book = s.book(w);
    assert_eq!(book.workbook.display(0, Cell::new(7, 4)), "$8,788.85");
    assert_eq!(book.workbook.display(1, Cell::new(0, 1)), "$8,788.85");
    // Saving keeps the file's own format and place.
    s.key("Ctrl+s");
    let back = cw_sheet::xlsx::read(&s.file(&format!("{home}/Documents/Budget.xlsx"))).unwrap();
    assert_eq!(back.display(0, Cell::new(7, 4)), "$8,788.85");
    // A new Calc document is an OpenDocument spreadsheet.
    s.click("sheet:menu:file");
    s.click("sheet:new:calc");
    s.type_text("=2^10");
    s.key("Enter");
    s.key("Ctrl+s");
    let ods = cw_sheet::ods::read(&s.file(&format!("{home}/Documents/Untitled 1.ods"))).unwrap();
    assert_eq!(ods.display(0, Cell::new(0, 0)), "1024");

    // Numbers on the iPhone opens the CSV export as a table.
    let mut phone = session("virtual-ios-18", &["spreadsheet"]);
    let home = phone.home();
    let w = phone.launch("spreadsheet", &format!("{home}/Documents/Sales.csv"));
    let book = phone.book(w);
    assert_eq!(book.workbook.display(0, Cell::new(0, 1)), "Region");
    assert_eq!(book.workbook.used_range(0).unwrap().a1(), "A1:E49");
    // A tap selects; the keyboard only comes up to edit.
    phone.pointer("click", phone.cell(2, 3));
    let book = phone.book(w);
    assert_eq!(book.active, Cell::new(2, 3));
    assert!(book.editing.is_none());
}

#[test]
fn db_browser_runs_a_query_edits_a_row_and_writes_the_file() {
    let mut s = session("virtual-ubuntu-24", &["database"]);
    let home = s.home();
    let path = format!("{home}/Documents/Inventory.db");
    let w = s.launch("database", &path);
    assert!(s.client(w).db.is_some());
    // Execute SQL: type a query and run it with F5.
    s.click("db:tab:execute");
    s.click("db:sql");
    s.type_text("SELECT sku, stock FROM products WHERE stock < 20 ORDER BY stock;");
    s.key("F5");
    let result = s.client(w).result.unwrap();
    assert!(!result.error, "{}", result.message);
    assert_eq!(result.rows.len(), 4);
    assert_eq!(result.rows[0][0], cw_sql::Value::Text("BRK-700".into()));
    // Browse Data: pick the table, double-click a cell, type over it, Enter.
    s.click("db:tab:browse");
    s.click("db:menu:tables");
    s.click("db:table:products");
    s.double_click("db:cell:1:5");
    assert!(s.client(w).edit.is_some());
    s.key("Backspace");
    s.key("Backspace");
    s.type_text("99");
    s.key("Enter");
    assert!(s.client(w).modified());
    // Nothing reaches the file until Write Changes.
    let before = cw_sql::Database::open(&s.file(&path))
        .unwrap()
        .query("SELECT stock FROM products WHERE sku = 'GAD-200'")
        .unwrap();
    assert_eq!(before.rows[0][0], cw_sql::Value::Integer(12));
    s.click("db:write");
    let mut after = cw_sql::Database::open(&s.file(&path)).unwrap();
    let out = after
        .query("SELECT stock, typeof(stock) FROM products WHERE sku = 'GAD-200'")
        .unwrap();
    assert_eq!(
        out.rows[0],
        vec![
            cw_sql::Value::Integer(99),
            cw_sql::Value::Text("integer".into())
        ]
    );
    assert_eq!(
        after.query("PRAGMA integrity_check").unwrap().rows[0][0],
        cw_sql::Value::Text("ok".into())
    );
    assert!(!s.client(w).modified());
}

#[test]
fn tableplus_stages_a_new_row_and_commits_it() {
    let mut s = session("virtual-macos-golden-gate", &["database"]);
    let home = s.home();
    let path = format!("{home}/Documents/Inventory.db");
    let w = s.launch("database", &path);
    s.click("db:table:suppliers");
    s.click("db:newrow");
    let c = s.client(w);
    assert_eq!(c.cell, Some((4, 0)));
    s.double_click("db:cell:4:1");
    s.type_text("Lisbon Metals");
    s.key("Tab");
    s.double_click("db:cell:4:2");
    s.type_text("Portugal");
    s.key("Enter");
    // Staged: Commit and Discard appear, and Cmd+S commits.
    assert!(s.find("db:write").is_some());
    s.key("Meta+s");
    let mut db = cw_sql::Database::open(&s.file(&path)).unwrap();
    let out = db
        .query("SELECT name, country FROM suppliers WHERE id = 5")
        .unwrap();
    assert_eq!(
        out.rows[0],
        vec![
            cw_sql::Value::Text("Lisbon Metals".into()),
            cw_sql::Value::Text("Portugal".into())
        ]
    );
    assert!(s.find("db:write").is_none());
}

#[test]
fn the_file_manager_opens_workbooks_and_databases_in_their_applications() {
    let mut s = session("virtual-windows-11", &["spreadsheet", "database"]);
    let home = s.home();
    s.launch("files", &format!("{home}/Documents"));
    let focused = |s: &Session| {
        s.world.interfaces().session(&s.actor).unwrap().machines[s.machine]
            .desktop
            .focused
            .unwrap()
    };
    // The listing is sorted by name: Budget.xlsx, Inventory.db, Sales.csv.
    s.double_click("open:0");
    let book = s.book(focused(&s));
    assert_eq!(
        book.path.as_deref(),
        Some(format!("{home}/Documents/Budget.xlsx").as_str())
    );
    s.launch("files", &format!("{home}/Documents"));
    s.double_click("open:1");
    let client = s.client(focused(&s));
    assert_eq!(client.name, "Inventory.db");
    assert!(client.tables().contains(&"products".to_string()));
}
