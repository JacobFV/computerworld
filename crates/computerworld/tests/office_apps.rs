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
        "{}/../../worlds/company-2026/samples/{name}",
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
        s.find("sheet:chartmove:0:").is_some(),
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

impl Session {
    /// Type `rows` into the grid from A1, clicking each cell first.
    fn fill(&mut self, rows: &[&[&str]]) {
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                self.pointer("click", self.cell(r as u32, c as u32));
                self.type_text(v);
                self.key("Enter");
            }
        }
    }
    /// The grid's column width and row height in screen pixels.
    fn cell_size(&self) -> (i32, i32) {
        let (_, target) = self.find("sheet:grid:").expect("a grid on screen");
        let mut parts = target.rsplit(':');
        let scale: u32 = parts.next().unwrap().parse().unwrap();
        let row_h: i32 = parts.next().unwrap().parse().unwrap();
        ((64 * scale / 100) as i32, row_h)
    }
    fn centre(&self, target: &str) -> (i32, i32) {
        let r = self.bounds(target);
        (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2)
    }
}

const SALES: &[&[&str]] = &[
    &["Region", "Sales", "Units"],
    &["East", "10", "3"],
    &["West", "25", "7"],
    &["East", "7", "1"],
    &["North", "31", "9"],
];

#[test]
fn excel_merges_borders_highlights_and_moves_charts_and_keeps_them_in_xlsx() {
    let mut s = session("virtual-windows-11", &["spreadsheet"]);
    let home = s.home();
    let w = s.launch("spreadsheet", "");
    s.click("sheet:new:excel");
    s.fill(SALES);
    s.pointer("click", s.cell(6, 0));
    s.type_text("Sales by region");
    s.key("Enter");

    // Merge & Center A7:C7 from the ribbon.
    s.drag(&[s.cell(6, 0), s.cell(6, 2)]);
    s.click("sheet:merge:center");
    let book = s.book(w);
    assert_eq!(book.workbook.sheets[0].merges[0].a1(), "A7:C7");
    // Clicking any part of the merge selects all of it; arrows step over it.
    s.pointer("click", s.cell(6, 1));
    let book = s.book(w);
    assert_eq!(book.selection().a1(), "A7:C7");
    assert_eq!(book.active, Cell::new(6, 0));
    s.key("ArrowRight");
    assert_eq!(s.book(w).active, Cell::new(6, 3));
    // Unmerge from the drop-down, then merge again.
    s.pointer("click", s.cell(6, 1));
    s.click("sheet:menu:merge");
    s.click("sheet:unmerge");
    assert!(s.book(w).workbook.sheets[0].merges.is_empty());
    s.drag(&[s.cell(6, 0), s.cell(6, 2)]);
    s.click("sheet:menu:merge");
    s.click("sheet:merge:center");

    // All Borders, then Thick Outside Borders, over the table.
    s.drag(&[s.cell(0, 0), s.cell(4, 2)]);
    s.click("sheet:menu:borders");
    s.click("sheet:border:all");
    s.click("sheet:menu:borders");
    s.click("sheet:border:thickoutside");
    let book = s.book(w);
    let inner = book.workbook.style(0, Cell::new(2, 1)).borders;
    assert_eq!(inner.left.unwrap().line, cw_sheet::Line::Thin);
    let corner = book.workbook.style(0, Cell::new(0, 0)).borders;
    assert_eq!(corner.top.unwrap().line, cw_sheet::Line::Thick);
    assert_eq!(corner.right.unwrap().line, cw_sheet::Line::Thin);

    // Conditional Formatting › Highlight Cells Rules › Greater Than… 20.
    s.drag(&[s.cell(1, 1), s.cell(4, 1)]);
    s.click("sheet:menu:cf");
    s.click("sheet:menu:cfhighlight");
    s.click("sheet:cf:greater");
    for _ in 0.."18.25".len() {
        s.key("Backspace");
    }
    s.type_text("20");
    s.click("sheet:dialog:preset:yellow");
    s.click("sheet:dialog:ok");
    // Data bars on the same cells.
    s.click("sheet:menu:cf");
    s.click("sheet:menu:cfbars");
    let bar = s.find("sheet:cf:bar:").unwrap().1;
    let bar = bar[bar.find("sheet:cf:bar:").unwrap()..].to_owned();
    s.click(&bar);
    let book = s.book(w);
    assert_eq!(book.workbook.sheets[0].conditional.len(), 2);
    let fx = book
        .workbook
        .conditional_effects(0, cw_sheet::Range::parse("A1:C5").unwrap());
    assert!(fx[&Cell::new(2, 1)].style.fill.is_some(), "25 > 20");
    assert!(fx
        .get(&Cell::new(1, 1))
        .is_none_or(|e| e.style.fill.is_none()));
    assert!(fx[&Cell::new(4, 1)].bar.is_some());
    // The Rules Manager deletes the data bar again.
    s.click("sheet:menu:cf");
    s.click("sheet:cfmanage");
    s.click("sheet:cfrule:1");
    s.click("sheet:cfdelete");
    s.click("sheet:dialog:ok");
    assert_eq!(s.book(w).workbook.sheets[0].conditional.len(), 1);

    // A chart, dragged two columns right and two rows down, then made bigger by its
    // bottom-right handle.
    s.drag(&[s.cell(0, 0), s.cell(4, 2)]);
    s.click("sheet:ribbon:insert");
    s.click("sheet:chart:column");
    let (cw, rh) = s.cell_size();
    let before = s.book(w).workbook.sheets[0].charts[0].clone();
    let from = s.centre("sheet:chartmove:0:");
    s.drag(&[
        from,
        (from.0 + cw, from.1 + rh),
        (from.0 + 2 * cw, from.1 + 2 * rh),
    ]);
    let moved = s.book(w).workbook.sheets[0].charts[0].clone();
    assert_eq!(
        moved.anchor,
        Cell::new(before.anchor.row + 2, before.anchor.col + 2)
    );
    assert_eq!((moved.cols, moved.rows), (before.cols, before.rows));
    assert_eq!(moved.offsets, before.offsets);
    let corner = s.centre("sheet:chartsize:0:4:");
    s.drag(&[corner, (corner.0 + cw, corner.1 + 3 * rh)]);
    let sized = s.book(w).workbook.sheets[0].charts[0].clone();
    assert_eq!(sized.anchor, moved.anchor);
    assert_eq!((sized.cols, sized.rows), (moved.cols + 1, moved.rows + 3));

    // Saved and read back: merges, borders, rules and the chart's place survive.
    s.key("Ctrl+s");
    let back = cw_sheet::xlsx::read(&s.file(&format!("{home}/Documents/Book1.xlsx"))).unwrap();
    let book = s.book(w);
    let sheet = &back.sheets[0];
    assert_eq!(sheet.merges[0].a1(), "A7:C7");
    assert_eq!(
        back.style(0, Cell::new(6, 0)).align,
        cw_sheet::Align::Center
    );
    for c in [Cell::new(0, 0), Cell::new(2, 1), Cell::new(4, 2)] {
        assert_eq!(back.style(0, c).borders, book.workbook.style(0, c).borders);
    }
    assert_eq!(sheet.conditional, book.workbook.sheets[0].conditional);
    assert_eq!(sheet.charts[0].anchor, sized.anchor);
    assert_eq!(
        (sheet.charts[0].cols, sheet.charts[0].rows),
        (sized.cols, sized.rows)
    );
}

#[test]
fn calc_builds_a_pivot_table_refreshes_it_and_keeps_it_in_ods() {
    let mut s = session("virtual-ubuntu-24", &["spreadsheet"]);
    let home = s.home();
    let w = s.launch("spreadsheet", "");
    s.click("sheet:new:calc");
    s.fill(SALES);
    // Insert › Pivot Table… over the data, onto a new sheet.
    s.drag(&[s.cell(0, 0), s.cell(4, 2)]);
    s.click("sheet:menu:calcinsert");
    s.click("sheet:pivot:new");
    s.click("sheet:dialog:ok");
    let book = s.book(w);
    assert_eq!(book.workbook.sheets.len(), 2);
    let ps = book.sheet;
    assert_eq!(book.workbook.sheets[ps].pivots.len(), 1);
    // Tick Region (to Rows) and Sales (to Values) in the field list.
    s.click("sheet:pivotfield:0");
    s.click("sheet:pivotfield:1");
    let book = s.book(w);
    let pivot = &book.workbook.sheets[ps].pivots[0];
    assert_eq!(pivot.rows, vec![0]);
    assert_eq!(pivot.values.len(), 1);
    let at = pivot.at;
    let row = |book: &Book, label: &str| -> String {
        let r = (0..8)
            .find(|r| book.workbook.display(ps, Cell::new(at.row + r, at.col)) == label)
            .unwrap_or_else(|| panic!("no {label} row"));
        book.workbook.display(ps, Cell::new(at.row + r, at.col + 1))
    };
    assert_eq!(row(&book, "East"), "17");
    // Average instead of Sum, from the value field's menu.
    s.click("sheet:menu:pivotvalue:0");
    s.click("sheet:pivotagg:0:average");
    let book = s.book(w);
    assert_eq!(
        book.workbook.sheets[ps].pivots[0].values[0].1,
        cw_sheet::pivot::Agg::Average
    );
    assert_eq!(row(&book, "East"), "8.5");
    // Change the source; the report follows on Refresh.
    let source = 1 - ps;
    s.click(&format!("sheet:tab:{source}"));
    s.pointer("click", s.cell(1, 1));
    s.type_text("30");
    s.key("Enter");
    s.click(&format!("sheet:tab:{ps}"));
    assert_eq!(row(&s.book(w), "East"), "8.5");
    s.pointer("click", s.cell(at.row + 1, at.col));
    s.click("sheet:pivot:refresh");
    assert_eq!(row(&s.book(w), "East"), "18.5");
    // Saved as OpenDocument and read back, the data pilot is still there.
    s.key("Ctrl+s");
    let back = cw_sheet::ods::read(&s.file(&format!("{home}/Documents/Untitled 1.ods"))).unwrap();
    let pivot = &back.sheets[ps].pivots[0];
    assert_eq!(pivot.rows, vec![0]);
    assert_eq!(pivot.values[0].1, cw_sheet::pivot::Agg::Average);
}

#[test]
fn db_browser_designs_tables_and_indexes_and_writes_them_to_the_file() {
    let mut s = session("virtual-ubuntu-24", &["database"]);
    let home = s.home();
    let path = format!("{home}/Documents/Inventory.db");
    let w = s.launch("database", &path);
    s.click("db:tab:structure");
    // Create Table: a name, two fields, an AUTOINCREMENT key and a NOT NULL column.
    s.click("db:createtable");
    s.type_text("warehouses");
    s.click("db:design:add");
    s.type_text("id");
    s.key("Enter");
    s.click("db:design:cell:0:4");
    s.click("db:design:add");
    s.type_text("city");
    s.key("Tab");
    s.type_text("TEXT");
    s.key("Enter");
    s.click("db:design:cell:1:2");
    s.click("db:design:ok");
    assert!(s.client(w).design.is_none(), "{:?}", s.client(w).message);
    // Modify Table on suppliers: email moves above country, which SQLite can only do
    // by rebuilding the table under the foreign key from products.
    s.click("db:tree:table:suppliers");
    s.click("db:modifytable:suppliers");
    s.click("db:design:cell:3:0");
    s.click("db:design:up");
    s.click("db:design:ok");
    assert!(s.client(w).design.is_none(), "{:?}", s.client(w).message);
    // Create Index on products(stock), descending.
    s.click("db:tree:table:products");
    s.click("db:createindex");
    s.type_text("products_stock");
    s.click("db:index:col:stock");
    s.click("db:index:order:0");
    s.click("db:index:ok");
    assert!(
        s.client(w).index_design.is_none(),
        "{:?}",
        s.client(w).message
    );
    // Nothing is in the file until Write Changes.
    assert!(!cw_sql::Database::open(&s.file(&path))
        .unwrap()
        .is_table("warehouses"));
    s.click("db:write");
    let mut db = cw_sql::Database::open(&s.file(&path)).unwrap();
    let text = |db: &mut cw_sql::Database, sql: &str| -> Vec<String> {
        db.query(sql)
            .unwrap()
            .rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(cw_sql::Value::to_text)
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect()
    };
    assert_eq!(
        text(&mut db, "SELECT sql FROM sqlite_schema WHERE name = 'warehouses'"),
        ["CREATE TABLE \"warehouses\" (\n\t\"id\"\tINTEGER,\n\t\"city\"\tTEXT NOT NULL,\n\tPRIMARY KEY(\"id\" AUTOINCREMENT)\n)"]
    );
    let cols: Vec<String> = db
        .table_info("suppliers")
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(cols, ["id", "name", "email", "country"]);
    assert_eq!(
        text(
            &mut db,
            "SELECT id, name, email, country FROM suppliers WHERE id = 3"
        ),
        ["3|Kyoto Precision||Japan"]
    );
    assert_eq!(
        text(
            &mut db,
            "SELECT sql FROM sqlite_schema WHERE name = 'products_stock'"
        ),
        ["CREATE INDEX \"products_stock\" ON \"products\" (\n\t\"stock\"\tDESC\n)"]
    );
    assert_eq!(text(&mut db, "PRAGMA integrity_check"), ["ok"]);
    assert!(text(&mut db, "PRAGMA foreign_key_check").is_empty());
    db.execute("PRAGMA foreign_keys = ON; INSERT INTO warehouses (city) VALUES ('Oslo')")
        .unwrap();
    assert_eq!(text(&mut db, "SELECT id, city FROM warehouses"), ["1|Oslo"]);
}

#[test]
fn tableplus_edits_a_tables_structure_in_place_and_commits_it() {
    let mut s = session("virtual-macos-golden-gate", &["database"]);
    let home = s.home();
    let path = format!("{home}/Documents/Inventory.db");
    let w = s.launch("database", &path);
    s.click("db:table:suppliers");
    s.click("db:tab:structure");
    // Double-click the email column's name and type over the end of it.
    s.double_click("db:struct:cell:3:0");
    s.type_text("_address");
    s.key("Enter");
    // A new column, typed in place.
    s.click("db:struct:addcol");
    s.type_text("phone");
    s.key("Enter");
    assert!(s.client(w).modified());
    assert!(s.find("db:write").is_some(), "Commit is offered");
    s.key("Meta+s");
    let mut db = cw_sql::Database::open(&s.file(&path)).unwrap();
    let cols: Vec<String> = db
        .table_info("suppliers")
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(cols, ["id", "name", "country", "email_address", "phone"]);
    let out = db
        .query("SELECT email_address FROM suppliers WHERE id = 1")
        .unwrap();
    assert_eq!(
        out.rows[0][0],
        cw_sql::Value::Text("orders@acme.example".into())
    );
    assert!(!s.client(w).modified());
}
