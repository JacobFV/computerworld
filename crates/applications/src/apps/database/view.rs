//! DB Browser for SQLite's window (menu bar, toolbar, the four tabs and status bar) and
//! TablePlus's (toolbar, sidebar of tables, data and structure views, staged changes
//! with Commit), both over one [`Client`].
use super::{ident, shown, tableplus, Client, Dialog, Focus, Purpose, Tab};
use crate::desktop_scene::shared::Align;
use crate::desktop_scene::Painter;
use crate::AppEnv;
use cw_scene::{Color, Rect};
use cw_sql::Value;

const INK: Color = Color::rgb(33, 33, 33);
const MUTED: Color = Color::rgb(110, 110, 110);
const FAINT: Color = Color::rgb(170, 170, 170);
const LINE: Color = Color::rgb(214, 214, 214);
const NULL_BG: Color = Color::rgb(236, 236, 236);

struct Style {
    accent: Color,
    chrome: Color,
    font: u16,
    row_h: u32,
    header_bg: Color,
    selection: Color,
    stripe: Option<Color>,
}

/// A command button: live, or painted disabled with the reason.
fn control(
    p: &mut Painter,
    r: Rect,
    label: &str,
    symbol: Option<&str>,
    target: Result<&str, &str>,
    st: &Style,
) {
    let live = target.is_ok();
    match target {
        Ok(t) => p.region(r, &format!("db:{t}"), label),
        Err(why) => {
            p.region(r, "db:noop", label);
            p.disabled(why);
        }
    }
    let ink = if live { INK } else { FAINT };
    let mut x = r.x + 6;
    if let Some(s) = symbol {
        p.symbol(
            s,
            x,
            r.y + (r.height as i32 - 16) / 2,
            16,
            if live { st.accent } else { FAINT },
        );
        x += 20;
    }
    p.label(
        x,
        r.y + (r.height as i32 - 16) / 2,
        (r.x + r.width as i32 - x).max(0) as u32,
        label,
        12,
        ink,
        false,
        Align::Left,
    );
}
fn control_width(p: &Painter, label: &str, symbol: bool) -> u32 {
    p.measure(label, 12, false) + if symbol { 34 } else { 14 }
}
fn menu(p: &mut Painter, x: i32, y: i32, items: &[(String, Result<String, String>)], max_x: i32) {
    let width = items
        .iter()
        .map(|(l, _)| p.measure(l, 12, false))
        .max()
        .unwrap_or(100)
        + 40;
    let x = x.min(max_x - width as i32 - 4).max(0);
    let r = Rect::new(x, y, width, items.len() as u32 * 24 + 8);
    p.drop_shadow(r, 4, 8, 50, 2);
    p.box_(r, Color::WHITE, 4);
    p.border(r, Color::TRANSPARENT, 4, LINE);
    p.region(r, "db:noop", "Menu");
    for (i, (label, target)) in items.iter().enumerate() {
        let row = Rect::new(x + 4, y + 4 + i as i32 * 24, width - 8, 24);
        if label.is_empty() {
            p.hline(row.x + 4, row.y + 12, row.width - 8, LINE);
            continue;
        }
        match target {
            Ok(t) => {
                p.region(row, &format!("db:{t}"), label);
                p.label(
                    row.x + 12,
                    row.y + 4,
                    row.width - 16,
                    label,
                    12,
                    INK,
                    false,
                    Align::Left,
                );
            }
            Err(why) => {
                p.region(row, "db:noop", label);
                p.disabled(why);
                p.label(
                    row.x + 12,
                    row.y + 4,
                    row.width - 16,
                    label,
                    12,
                    FAINT,
                    false,
                    Align::Left,
                );
            }
        }
    }
}
fn ok(t: &str) -> Result<String, String> {
    Ok(t.into())
}
fn need_db(c: &Client, t: &str) -> Result<String, String> {
    if c.db.is_some() {
        Ok(t.into())
    } else {
        Err("no database is open".into())
    }
}
fn changes(c: &Client, t: &str) -> Result<String, String> {
    if c.db.is_none() {
        Err("no database is open".into())
    } else if !c.modified() {
        Err("there are no changes to write or revert".into())
    } else {
        Ok(t.into())
    }
}
fn table_target(c: &Client, t: &str) -> Result<String, String> {
    match &c.table {
        Some(_) => Ok(t.into()),
        None => Err("choose a table first".into()),
    }
}
fn record_target(c: &Client, t: &str, needs_cell: bool) -> Result<String, String> {
    if c.db.is_none() {
        return Err("no database is open".into());
    }
    if c.table.is_none() {
        return Err("choose a table first".into());
    }
    if let Some(why) = c.why_read_only() {
        return Err(why);
    }
    if needs_cell && c.cell.is_none() {
        return Err("select a record first".into());
    }
    Ok(t.into())
}

pub fn render(c: &Client, p: &mut Painter, env: &AppEnv<'_>) {
    p.scene.background = Color::WHITE;
    if tableplus(env.theme) {
        tableplus_window(c, p, env);
    } else {
        dbbrowser(c, p, env);
    }
    overlays(c, p, env);
}

// ----- the grid both products draw -----

struct GridSpec<'a> {
    columns: &'a [String],
    rows: Vec<Vec<Value>>,
    /// Index of the first row, for row numbers and cell targets.
    first: usize,
    /// Cells carry `db:cell:R:C` targets (the Browse grid) or none (query results).
    cells: bool,
    selected: Option<(usize, usize)>,
    edit: Option<&'a super::CellEdit>,
    sort: Option<(usize, bool)>,
    /// Filter boxes under the headers, and the focused one.
    filters: Option<(&'a std::collections::BTreeMap<usize, String>, Option<usize>)>,
    row_numbers: bool,
}
fn column_widths(p: &Painter, spec: &GridSpec, font: u16) -> Vec<u32> {
    spec.columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let mut w = p.measure(name, font, true) + 26;
            for r in spec.rows.iter().take(40) {
                if let Some(v) = r.get(i) {
                    let t: String = shown(v).chars().take(40).collect();
                    w = w.max(p.measure(&t, font, false) + 14);
                }
            }
            w.clamp(60, 260)
        })
        .collect()
}
fn grid(p: &mut Painter, r: Rect, spec: &GridSpec, st: &Style) {
    p.box_(r, Color::WHITE, 0);
    let widths = column_widths(p, spec, st.font);
    let num_w = if spec.row_numbers {
        p.measure(&(spec.first + spec.rows.len()).to_string(), st.font, false) + 16
    } else {
        0
    };
    let head_h = st.row_h + 2;
    let filter_h = if spec.filters.is_some() { st.row_h } else { 0 };
    // Headers.
    p.box_(
        Rect::new(r.x, r.y, r.width, head_h + filter_h),
        st.header_bg,
        0,
    );
    let mut x = r.x + num_w as i32;
    let right = r.x + r.width as i32;
    for (i, name) in spec.columns.iter().enumerate() {
        if x >= right {
            break;
        }
        let w = widths[i].min((right - x) as u32);
        let h = Rect::new(x, r.y, w, head_h);
        if spec.cells {
            p.region(h, &format!("db:sort:{i}"), &format!("Sort by {name}"));
        }
        p.label(
            h.x + 6,
            h.y + (head_h as i32 - 16) / 2,
            w.saturating_sub(22),
            name,
            st.font,
            INK,
            true,
            Align::Left,
        );
        if let Some((s, asc)) = spec.sort {
            if s == i {
                p.symbol(
                    if asc { "chevron-up" } else { "chevron-down" },
                    h.x + w as i32 - 16,
                    h.y + (head_h as i32 - 12) / 2,
                    12,
                    MUTED,
                );
            }
        }
        p.vline(x + w as i32 - 1, r.y, head_h + filter_h, LINE);
        if let Some((filters, focused)) = spec.filters {
            let f = Rect::new(
                x + 2,
                r.y + head_h as i32 + 2,
                w.saturating_sub(4),
                filter_h.saturating_sub(4),
            );
            let focus = focused == Some(i);
            p.border(f, Color::WHITE, 2, if focus { st.accent } else { LINE });
            p.region(f, &format!("db:filter:{i}"), &format!("Filter {name}"));
            let text = filters.get(&i).map(String::as_str).unwrap_or("");
            if text.is_empty() && !focus {
                p.label(
                    f.x + 4,
                    f.y + (f.height as i32 - 15) / 2,
                    f.width.saturating_sub(8),
                    "Filter",
                    st.font - 1,
                    FAINT,
                    false,
                    Align::Left,
                );
            } else {
                let tw = p.label(
                    f.x + 4,
                    f.y + (f.height as i32 - 15) / 2,
                    f.width.saturating_sub(8),
                    text,
                    st.font - 1,
                    INK,
                    false,
                    Align::Left,
                );
                if focus {
                    p.vline(
                        f.x + 5 + tw as i32,
                        f.y + 3,
                        f.height.saturating_sub(6),
                        INK,
                    );
                }
            }
        }
        x += w as i32;
    }
    p.hline(r.x, r.y + (head_h + filter_h) as i32 - 1, r.width, LINE);
    // Rows.
    let top = r.y + (head_h + filter_h) as i32;
    for (k, row) in spec.rows.iter().enumerate() {
        let y = top + k as i32 * st.row_h as i32;
        if y + st.row_h as i32 > r.y + r.height as i32 {
            break;
        }
        let index = spec.first + k;
        if let Some(stripe) = st.stripe {
            if index % 2 == 1 {
                p.box_(
                    Rect::new(r.x + num_w as i32, y, r.width - num_w, st.row_h),
                    stripe,
                    0,
                );
            }
        }
        if spec.row_numbers {
            let n = Rect::new(r.x, y, num_w, st.row_h);
            p.box_(n, st.header_bg, 0);
            p.label(
                n.x,
                n.y + (st.row_h as i32 - 16) / 2,
                num_w - 6,
                &(index + 1).to_string(),
                st.font,
                MUTED,
                false,
                Align::Right,
            );
            p.vline(r.x + num_w as i32 - 1, y, st.row_h, LINE);
        }
        let mut x = r.x + num_w as i32;
        for (i, v) in row.iter().enumerate() {
            if x >= right || i >= widths.len() {
                break;
            }
            let w = widths[i].min((right - x) as u32);
            let cell = Rect::new(x, y, w, st.row_h);
            let selected = spec.selected == Some((index, i));
            let editing = spec.edit.filter(|e| e.row == index && e.col == i);
            if v.is_null() && editing.is_none() {
                p.box_(
                    Rect::new(cell.x, cell.y, cell.width - 1, cell.height - 1),
                    NULL_BG,
                    0,
                );
            }
            if selected {
                p.box_(
                    Rect::new(cell.x, cell.y, cell.width - 1, cell.height - 1),
                    st.selection,
                    0,
                );
            }
            if spec.cells {
                p.region(
                    cell,
                    &format!("db:cell:{index}:{i}"),
                    &format!(
                        "Row {} {}",
                        index + 1,
                        spec.columns.get(i).map(String::as_str).unwrap_or("")
                    ),
                );
            }
            match editing {
                Some(e) => {
                    p.border(
                        Rect::new(cell.x, cell.y, cell.width - 1, cell.height - 1),
                        Color::WHITE,
                        0,
                        st.accent,
                    );
                    let tw = p.label(
                        cell.x + 5,
                        cell.y + (st.row_h as i32 - 16) / 2,
                        w.saturating_sub(10),
                        &e.text,
                        st.font,
                        INK,
                        false,
                        Align::Left,
                    );
                    p.vline(cell.x + 6 + tw as i32, cell.y + 3, st.row_h - 6, INK);
                }
                None => {
                    let text = shown(v);
                    let numeric = matches!(v, Value::Integer(_) | Value::Real(_));
                    let color = if v.is_null() { MUTED } else { INK };
                    p.label(
                        cell.x + 5,
                        cell.y + (st.row_h as i32 - 16) / 2,
                        w.saturating_sub(10),
                        &text,
                        st.font,
                        color,
                        false,
                        if numeric { Align::Right } else { Align::Left },
                    );
                }
            }
            if selected {
                p.border(
                    Rect::new(cell.x, cell.y, cell.width - 1, cell.height - 1),
                    Color::TRANSPARENT,
                    0,
                    st.accent,
                );
            }
            p.vline(x + w as i32 - 1, y, st.row_h, Color::rgb(232, 232, 232));
            x += w as i32;
        }
        p.hline(
            r.x,
            y + st.row_h as i32 - 1,
            r.width,
            Color::rgb(232, 232, 232),
        );
    }
}
/// Rows that fit in a grid of this height.
fn fits(height: u32, st: &Style, filters: bool) -> usize {
    let used = st.row_h + 2 + if filters { st.row_h } else { 0 };
    (height.saturating_sub(used) / st.row_h) as usize
}

// ----- DB Browser for SQLite -----

fn dbbrowser(c: &Client, p: &mut Painter, env: &AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let st = Style {
        accent: if env.theme == crate::desktop_scene::DesktopTheme::Windows {
            Color::rgb(0, 120, 215)
        } else {
            Color::rgb(48, 140, 198)
        },
        chrome: Color::rgb(239, 239, 239),
        font: 12,
        row_h: 22,
        header_bg: Color::rgb(245, 245, 245),
        selection: Color(48, 140, 198, 60),
        stripe: Some(Color::rgb(250, 250, 250)),
    };
    p.box_(Rect::new(0, 0, w, h), st.chrome, 0);
    // Menu bar.
    let menus: [(&str, Result<&str, &str>); 5] = [
        ("File", Ok("file")),
        ("Edit", Ok("edit")),
        ("View", Ok("view")),
        ("Tools", Ok("tools")),
        ("Help", Err("help content is not available offline")),
    ];
    let mut x = 4;
    let mut anchors = Vec::new();
    for (label, m) in menus {
        let mw = p.measure(label, 12, false) + 16;
        let r = Rect::new(x, 0, mw, 22);
        match m {
            Ok(m) => {
                if c.menu.as_deref() == Some(m) {
                    p.box_(r, Color(st.accent.0, st.accent.1, st.accent.2, 50), 0);
                }
                p.region(r, &format!("db:menu:{m}"), label);
                p.label(r.x, r.y + 3, r.width, label, 12, INK, false, Align::Center);
                anchors.push((m, x));
            }
            Err(why) => {
                p.region(r, "db:noop", label);
                p.disabled(why);
                p.label(
                    r.x,
                    r.y + 3,
                    r.width,
                    label,
                    12,
                    FAINT,
                    false,
                    Align::Center,
                );
            }
        }
        x += mw as i32;
    }
    // Main toolbar.
    let ty = 22;
    let tools: [(&str, &str, Result<String, String>); 7] = [
        ("New Database", "new-tab", ok("new")),
        ("Open Database", "folder", ok("open")),
        ("Write Changes", "download", changes(c, "write")),
        ("Revert Changes", "undo", changes(c, "revert")),
        (
            "Open Project",
            "archive",
            Err("projects are not modeled".into()),
        ),
        (
            "Attach Database",
            "link",
            Err("attached databases are not modeled".into()),
        ),
        ("Close Database", "close", need_db(c, "close")),
    ];
    let mut x = 4;
    for (i, (label, sym, target)) in tools.iter().enumerate() {
        if i == 4 || i == 6 {
            p.vline(x + 2, ty + 6, 22, LINE);
            x += 6;
        }
        let tw = control_width(p, label, true);
        if x + tw as i32 > w as i32 {
            break;
        }
        control(
            p,
            Rect::new(x, ty + 3, tw, 28),
            label,
            Some(sym),
            target.as_deref().map_err(String::as_str),
            &st,
        );
        x += tw as i32 + 2;
    }
    // Tabs.
    let tab_y = ty + 36;
    let tabs = [
        ("Database Structure", Tab::Structure, "structure"),
        ("Browse Data", Tab::Browse, "browse"),
        ("Edit Pragmas", Tab::Pragmas, "pragmas"),
        ("Execute SQL", Tab::Execute, "execute"),
    ];
    let mut x = 6;
    for (label, tab, id) in tabs {
        let tw = p.measure(label, 12, false) + 24;
        let r = Rect::new(x, tab_y, tw, 26);
        let on = c.tab == tab;
        p.box_(
            r,
            if on {
                Color::WHITE
            } else {
                Color::rgb(228, 228, 228)
            },
            3,
        );
        p.border(r, Color::TRANSPARENT, 3, LINE);
        p.region(r, &format!("db:tab:{id}"), label);
        p.label(r.x, r.y + 5, r.width, label, 12, INK, on, Align::Center);
        x += tw as i32 + 2;
    }
    let body = Rect::new(
        4,
        tab_y + 26,
        w.saturating_sub(8),
        h.saturating_sub(tab_y as u32 + 26 + 24),
    );
    p.box_(body, Color::WHITE, 0);
    p.border(body, Color::TRANSPARENT, 0, LINE);
    match c.tab {
        Tab::Structure => structure(c, p, body, &st),
        Tab::Browse => browse(c, p, body, &st),
        Tab::Pragmas => pragmas(c, p, body, &st),
        Tab::Execute => execute(c, p, body, &st),
    }
    // Status bar.
    let sy = h as i32 - 22;
    p.box_(Rect::new(0, sy, w, 22), st.chrome, 0);
    p.hline(0, sy, w, LINE);
    let status = match (&c.path, &c.db) {
        (Some(path), Some(_)) => format!(
            "{path}{}",
            if c.modified() {
                "  (unsaved changes)"
            } else {
                ""
            }
        ),
        _ => "No database open".into(),
    };
    p.left(8, sy + 4, w.saturating_sub(100), &status, 11, MUTED);
    p.right(0, sy + 4, w.saturating_sub(10), "UTF-8", 11, MUTED);
    if let Some(m) = &c.menu {
        let x = anchors.iter().find(|(n, _)| n == m).map_or(4, |(_, x)| *x);
        let items = menu_items(c, m);
        if !items.is_empty() {
            menu(p, x, 22, &items, w as i32);
        }
    }
}
fn menu_items(c: &Client, name: &str) -> Vec<(String, Result<String, String>)> {
    let s = |l: &str, t: Result<String, String>| (l.to_string(), t);
    match name {
        "file" => vec![
            s("New Database\tCtrl+N", ok("new")),
            s("Open Database...\tCtrl+O", ok("open")),
            s("Write Changes\tCtrl+S", changes(c, "write")),
            s("Revert Changes", changes(c, "revert")),
            s("", Err(String::new())),
            s("Import Table from CSV file...", need_db(c, "import")),
            s("Export Table as CSV file...", table_target(c, "exportcsv")),
            s("", Err(String::new())),
            s("Close Database\tCtrl+W", need_db(c, "close")),
        ],
        "edit" => vec![
            s("Create Table...", need_db(c, "createtable")),
            s(
                "Modify Table...",
                match c
                    .tree_selected
                    .as_deref()
                    .and_then(|t| t.strip_prefix("table:"))
                    .filter(|t| c.db.as_ref().is_some_and(|d| d.is_table(t)))
                {
                    Some(t) => Ok(format!("modifytable:{t}")),
                    None => Err("select a table in Database Structure first".into()),
                },
            ),
            s("Create Index...", need_db(c, "createindex")),
            s(
                "Delete Table...",
                match c
                    .tree_selected
                    .as_deref()
                    .and_then(|t| t.strip_prefix("table:"))
                {
                    Some(t) => Ok(format!("droptable:{t}")),
                    None => Err("select a table in Database Structure first".into()),
                },
            ),
            s("", Err(String::new())),
            s("Insert New Record", record_target(c, "newrow", false)),
            s("Delete Record", record_target(c, "deleterow", true)),
            s("Set as NULL\tAlt+Del", record_target(c, "setnull", true)),
        ],
        "view" => vec![
            s("Database Structure", ok("tab:structure")),
            s("Browse Data", ok("tab:browse")),
            s("Edit Pragmas", ok("tab:pragmas")),
            s("Execute SQL", ok("tab:execute")),
        ],
        "tools" => vec![s("Integrity Check", need_db(c, "integrity"))],
        "tables" => c
            .tables()
            .into_iter()
            .map(|t| (t.clone(), Ok(format!("table:{t}"))))
            .collect(),
        _ => vec![],
    }
}
fn sub_toolbar(
    p: &mut Painter,
    r: Rect,
    items: &[(&str, &str, Result<String, String>)],
    st: &Style,
) -> i32 {
    let mut x = r.x + 4;
    for (label, sym, target) in items {
        let tw = control_width(p, label, true);
        if x + tw as i32 > r.x + r.width as i32 {
            break;
        }
        control(
            p,
            Rect::new(x, r.y + 2, tw, 26),
            label,
            Some(sym),
            target.as_deref().map_err(String::as_str),
            st,
        );
        x += tw as i32 + 2;
    }
    x
}
fn no_database(p: &mut Painter, r: Rect) {
    p.center(
        r.x,
        r.y + r.height as i32 / 2 - 20,
        r.width,
        "No database is open",
        14,
        MUTED,
    );
    p.center(
        r.x,
        r.y + r.height as i32 / 2 + 4,
        r.width,
        "Use New Database or Open Database to begin.",
        12,
        FAINT,
    );
}
fn structure(c: &Client, p: &mut Painter, r: Rect, st: &Style) {
    let selected_table = c
        .tree_selected
        .as_deref()
        .and_then(|t| t.strip_prefix("table:"));
    let modifiable = selected_table.filter(|t| c.db.as_ref().is_some_and(|d| d.is_table(t)));
    sub_toolbar(
        p,
        Rect::new(r.x, r.y, r.width, 30),
        &[
            ("Create Table", "plus", need_db(c, "createtable")),
            ("Create Index", "list-view", need_db(c, "createindex")),
            (
                "Modify Table",
                "edit",
                match modifiable {
                    Some(t) => Ok(format!("modifytable:{t}")),
                    None => Err("select a table in the tree first".into()),
                },
            ),
            (
                "Delete Table",
                "trash",
                match selected_table {
                    Some(t) if c.db.is_some() => Ok(format!("droptable:{t}")),
                    _ => Err("select a table in the tree first".into()),
                },
            ),
            ("Print", "document", Err("printing is not modeled".into())),
        ],
        st,
    );
    let t = Rect::new(r.x, r.y + 30, r.width, r.height.saturating_sub(30));
    let Some(db) = &c.db else {
        no_database(p, t);
        return;
    };
    let name_w = (t.width * 3 / 10).max(160);
    let type_w = 90;
    p.box_(Rect::new(t.x, t.y, t.width, 22), st.header_bg, 0);
    p.left(t.x + 8, t.y + 3, name_w, "Name", 12, INK);
    p.left(t.x + name_w as i32, t.y + 3, type_w, "Type", 12, INK);
    p.left(
        t.x + (name_w + type_w) as i32,
        t.y + 3,
        t.width.saturating_sub(name_w + type_w),
        "Schema",
        12,
        INK,
    );
    p.hline(t.x, t.y + 22, t.width, LINE);
    let schema = db.schema();
    let mut y = t.y + 24;
    let row_h = 22;
    let bottom = t.y + t.height as i32;
    let mut row = |p: &mut Painter,
                   depth: i32,
                   node: Option<&str>,
                   target: Option<String>,
                   name: &str,
                   kind: &str,
                   sql: &str,
                   bold: bool| {
        if y + row_h > bottom {
            return;
        }
        let rr = Rect::new(t.x, y, t.width, row_h as u32);
        if target.is_some() && c.tree_selected.as_deref() == target.as_deref() {
            p.box_(rr, st.selection, 0);
        }
        let x0 = t.x + 6 + depth * 18;
        if let Some(n) = node {
            let open = c.expanded.contains(n);
            let arrow = Rect::new(x0, y + 3, 16, 16);
            p.region(
                arrow,
                &format!("db:expand:{n}"),
                if open { "Collapse" } else { "Expand" },
            );
            p.symbol(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                arrow.x + 2,
                arrow.y + 2,
                12,
                MUTED,
            );
        }
        if let Some(tg) = &target {
            p.region(
                Rect::new(
                    x0 + 18,
                    y,
                    t.width.saturating_sub((x0 + 18 - t.x) as u32),
                    row_h as u32,
                ),
                &format!("db:tree:{tg}"),
                name,
            );
        }
        p.label(
            x0 + 18,
            y + 3,
            name_w.saturating_sub((x0 + 18 - t.x) as u32),
            name,
            12,
            INK,
            bold,
            Align::Left,
        );
        p.left(t.x + name_w as i32, y + 3, type_w, kind, 12, MUTED);
        let one_line: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
        p.left(
            t.x + (name_w + type_w) as i32,
            y + 3,
            t.width.saturating_sub(name_w + type_w + 8),
            &one_line,
            12,
            MUTED,
        );
        y += row_h;
    };
    let groups: [(&str, &str, &str); 4] = [
        ("tables", "Tables", "table"),
        ("indexes", "Indices", "index"),
        ("views", "Views", "view"),
        ("triggers", "Triggers", "trigger"),
    ];
    for (node, title, kind) in groups {
        let items: Vec<&cw_sql::SchemaEntry> = schema.iter().filter(|e| e.kind == kind).collect();
        row(
            p,
            0,
            Some(node),
            None,
            &format!("{title} ({})", items.len()),
            "",
            "",
            true,
        );
        if !c.expanded.contains(node) {
            continue;
        }
        for e in items {
            let sql = e.sql.clone().unwrap_or_default();
            if kind == "table" {
                let key = format!("table:{}", e.name);
                row(
                    p,
                    1,
                    Some(&key),
                    Some(key.clone()),
                    &e.name,
                    "",
                    &sql,
                    false,
                );
                if c.expanded.contains(&key) {
                    for col in db.table_info(&e.name).unwrap_or_default() {
                        let mut def = format!("{} {}", ident(&col.name), col.decl_type);
                        if col.primary_key > 0 {
                            def.push_str(" PRIMARY KEY");
                        }
                        if col.not_null {
                            def.push_str(" NOT NULL");
                        }
                        if let Some(d) = &col.default {
                            def.push_str(&format!(" DEFAULT {d}"));
                        }
                        row(
                            p,
                            2,
                            None,
                            None,
                            &col.name,
                            &col.decl_type,
                            def.trim(),
                            false,
                        );
                    }
                }
            } else {
                let key = format!("{kind}:{}", e.name);
                let target = (kind == "view").then(|| format!("table:{}", e.name));
                row(p, 1, None, target.or(Some(key)), &e.name, "", &sql, false);
            }
        }
    }
}
fn browse(c: &Client, p: &mut Painter, r: Rect, st: &Style) {
    // Table chooser and record commands.
    let bar = Rect::new(r.x, r.y, r.width, 32);
    p.left(bar.x + 8, bar.y + 8, 40, "Table:", 12, INK);
    let combo = Rect::new(bar.x + 52, bar.y + 4, 180, 24);
    p.border(combo, Color::WHITE, 3, LINE);
    match &c.db {
        Some(_) => p.region(combo, "db:menu:tables", "Table"),
        None => {
            p.region(combo, "db:noop", "Table");
            p.disabled("no database is open");
        }
    }
    p.left(
        combo.x + 6,
        combo.y + 4,
        150,
        c.table.as_deref().unwrap_or(""),
        12,
        INK,
    );
    p.symbol("chevron-down", combo.x + 162, combo.y + 6, 12, MUTED);
    let mut x = combo.x + combo.width as i32 + 8;
    let filtered = !c.filters.values().all(|f| f.trim().is_empty()) || c.sort.is_some();
    let items = [
        ("Refresh", "reload", table_target(c, "refresh")),
        (
            "Clear Filters",
            "eraser",
            if filtered {
                Ok("clearfilters".to_string())
            } else {
                Err("no filter or sort is set".to_string())
            },
        ),
        ("New Record", "plus", record_target(c, "newrow", false)),
        (
            "Delete Record",
            "minus",
            record_target(c, "deleterow", true),
        ),
        ("Export CSV", "download", table_target(c, "exportcsv")),
    ];
    for (label, sym, target) in &items {
        let tw = control_width(p, label, true);
        if x + tw as i32 > r.x + r.width as i32 {
            break;
        }
        control(
            p,
            Rect::new(x, bar.y + 3, tw, 26),
            label,
            Some(sym),
            target.as_deref().map_err(String::as_str),
            st,
        );
        x += tw as i32 + 2;
    }
    let area = Rect::new(r.x, r.y + 32, r.width, r.height.saturating_sub(32 + 30));
    if c.db.is_none() {
        no_database(p, area);
        return;
    }
    let n = fits(area.height, st, true);
    match c.rows(c.offset, n) {
        Ok(rows) => {
            let values: Vec<Vec<Value>> = rows.rows.iter().map(|(_, v)| v.clone()).collect();
            let spec = GridSpec {
                columns: &rows.columns,
                rows: values,
                first: c.offset,
                cells: true,
                selected: c.cell,
                edit: c.edit.as_ref(),
                sort: c.sort,
                filters: Some((
                    &c.filters,
                    match c.focus {
                        Focus::Filter(i) => Some(i),
                        _ => None,
                    },
                )),
                row_numbers: true,
            };
            grid(p, area, &spec, st);
            pager(
                p,
                Rect::new(r.x, area.y + area.height as i32, r.width, 30),
                c.offset,
                rows.rows.len(),
                rows.total,
                st,
            );
        }
        Err(e) => {
            p.center(area.x, area.y + 40, area.width, &e, 12, MUTED);
        }
    }
}
fn pager(p: &mut Painter, r: Rect, offset: usize, shown_rows: usize, total: usize, st: &Style) {
    p.hline(r.x, r.y, r.width, LINE);
    let at_start = offset == 0;
    let at_end = offset + super::PAGE >= total;
    let buttons: [(&str, &str, bool, &str); 4] = [
        ("First", "skip-previous", at_start, "first"),
        ("Previous", "chevron-left", at_start, "prev"),
        ("Next", "chevron-right", at_end, "next"),
        ("Last", "skip-next", at_end, "last"),
    ];
    let label = if total == 0 {
        "0 - 0 of 0".to_string()
    } else {
        format!("{} - {} of {total}", offset + 1, offset + shown_rows)
    };
    let lw = p.measure(&label, 12, false) + 20;
    let mut x = r.x + 6;
    for (i, (name, sym, disabled, arg)) in buttons.iter().enumerate() {
        if i == 2 {
            p.label(x, r.y + 7, lw, &label, 12, INK, false, Align::Center);
            x += lw as i32;
        }
        let b = Rect::new(x, r.y + 3, 24, 24);
        let target = if *disabled {
            Err(if i < 2 {
                "this is the first page"
            } else {
                "this is the last page"
            })
        } else {
            Ok(format!("page:{arg}"))
        };
        match &target {
            Ok(t) => p.region(b, &format!("db:{t}"), name),
            Err(why) => {
                p.region(b, "db:noop", name);
                p.disabled(why);
            }
        }
        p.symbol(
            sym,
            b.x + 4,
            b.y + 4,
            16,
            if target.is_ok() { st.accent } else { FAINT },
        );
        x += 26;
    }
}
fn pragma_value(c: &Client, name: &str) -> String {
    let Some(db) = &c.db else {
        return String::new();
    };
    let mut scratch = db.clone();
    scratch
        .query(&format!("PRAGMA {name}"))
        .ok()
        .and_then(|o| o.rows.first().and_then(|r| r.first()).map(Value::to_text))
        .unwrap_or_default()
}
fn pragmas(c: &Client, p: &mut Painter, r: Rect, st: &Style) {
    if c.db.is_none() {
        no_database(p, r);
        return;
    }
    let fixed = |why: &'static str| Err::<String, &str>(why);
    let rows: Vec<(&str, String, Result<String, &str>)> = vec![
        (
            "Encoding",
            "UTF-8".into(),
            fixed("databases are always UTF-8 here"),
        ),
        (
            "Foreign Keys",
            if c.db.as_ref().is_some_and(|d| d.foreign_keys()) {
                "On".into()
            } else {
                "Off".into()
            },
            Ok("pragma:foreign_keys".into()),
        ),
        (
            "Page Size",
            "4096".into(),
            fixed("pages are always 4096 bytes here"),
        ),
        (
            "Journal Mode",
            "delete".into(),
            fixed("the whole file is written at once; there is no journal to choose"),
        ),
        (
            "User Version",
            pragma_value(c, "user_version"),
            Ok(String::new()),
        ),
    ];
    let mut y = r.y + 16;
    for (label, value, target) in rows {
        p.right(r.x, y + 4, 160, label, 12, INK);
        let field = Rect::new(r.x + 176, y, 200, 26);
        match (&target, label) {
            (Ok(t), "Foreign Keys") => {
                let on = value == "On";
                let b = Rect::new(field.x, field.y + 4, 18, 18);
                p.border(
                    b,
                    if on { st.accent } else { Color::WHITE },
                    3,
                    if on { st.accent } else { LINE },
                );
                if on {
                    p.symbol("check", b.x + 2, b.y + 2, 14, Color::WHITE);
                }
                p.region(
                    Rect::new(field.x, field.y, 120, 26),
                    &format!("db:{t}"),
                    "Foreign Keys",
                );
                p.left(field.x + 26, field.y + 4, 100, &value, 12, INK);
            }
            (Ok(_), _) => {
                p.border(Rect::new(field.x, field.y, 120, 26), Color::WHITE, 3, LINE);
                p.left(field.x + 8, field.y + 5, 80, &value, 12, INK);
                let down = Rect::new(field.x + 124, field.y, 26, 26);
                let up = Rect::new(field.x + 152, field.y, 26, 26);
                p.border(down, Color::WHITE, 3, LINE);
                p.region(down, "db:pragma:user_version:down", "Decrease User Version");
                p.symbol("minus", down.x + 5, down.y + 5, 16, st.accent);
                p.border(up, Color::WHITE, 3, LINE);
                p.region(up, "db:pragma:user_version:up", "Increase User Version");
                p.symbol("plus", up.x + 5, up.y + 5, 16, st.accent);
            }
            (Err(why), _) => {
                p.border(
                    Rect::new(field.x, field.y, 120, 26),
                    Color::rgb(248, 248, 248),
                    3,
                    LINE,
                );
                p.region(Rect::new(field.x, field.y, 120, 26), "db:noop", label);
                p.disabled(why);
                p.left(field.x + 8, field.y + 5, 110, &value, 12, MUTED);
            }
        }
        y += 34;
    }
    let check = Rect::new(r.x + 176, y + 8, 140, 28);
    control(
        p,
        check,
        "Integrity Check",
        Some("check"),
        Ok("integrity"),
        st,
    );
    p.border(check, Color::TRANSPARENT, 3, LINE);
}
fn sql_editor(c: &Client, p: &mut Painter, r: Rect, st: &Style, mono: u16) {
    p.box_(r, Color::WHITE, 0);
    p.border(
        r,
        Color::TRANSPARENT,
        0,
        if c.focus == Focus::Sql {
            st.accent
        } else {
            LINE
        },
    );
    p.region(r, "db:sql", "SQL editor");
    let gutter = 36;
    p.box_(
        Rect::new(r.x + 1, r.y + 1, gutter, r.height.saturating_sub(2)),
        Color::rgb(245, 245, 245),
        0,
    );
    let line_h = mono as i32 + 6;
    let caret_line = c.sql[..c.caret].matches('\n').count();
    let visible = ((r.height as i32 - 8) / line_h).max(1) as usize;
    let first = caret_line.saturating_sub(visible - 1);
    for (i, line) in c.sql.split('\n').enumerate().skip(first).take(visible) {
        let y = r.y + 4 + (i - first) as i32 * line_h;
        p.label(
            r.x + 2,
            y,
            gutter - 8,
            &(i + 1).to_string(),
            mono - 1,
            FAINT,
            false,
            Align::Right,
        );
        p.text(
            r.x + gutter as i32 + 6,
            y,
            r.width.saturating_sub(gutter + 10),
            line,
            mono,
            INK,
        );
        if c.focus == Focus::Sql && i == caret_line {
            let start = c.sql[..c.caret].rfind('\n').map_or(0, |k| k + 1);
            let cx =
                r.x + gutter as i32 + 6 + p.measure(&c.sql[start..c.caret], mono, false) as i32;
            p.vline(cx, y, line_h as u32 - 2, INK);
        }
    }
    if c.sql.is_empty() && c.focus != Focus::Sql {
        p.left(
            r.x + gutter as i32 + 6,
            r.y + 4,
            r.width.saturating_sub(gutter + 10),
            "Type SQL here",
            mono,
            FAINT,
        );
    }
}
fn results(c: &Client, p: &mut Painter, grid_r: Rect, msg_r: Rect, st: &Style) {
    match &c.result {
        Some(res) => {
            if !res.columns.is_empty() {
                let n = fits(grid_r.height, st, false);
                let rows: Vec<Vec<Value>> = res
                    .rows
                    .iter()
                    .skip(c.result_offset)
                    .take(n)
                    .cloned()
                    .collect();
                let spec = GridSpec {
                    columns: &res.columns,
                    rows,
                    first: c.result_offset,
                    cells: false,
                    selected: None,
                    edit: None,
                    sort: None,
                    filters: None,
                    row_numbers: true,
                };
                grid(p, grid_r, &spec, st);
                let more_up = c.result_offset > 0;
                let more_down = c.result_offset + n < res.rows.len();
                for (i, (label, sym, live, t)) in [
                    ("Scroll results up", "chevron-up", more_up, "results:up"),
                    (
                        "Scroll results down",
                        "chevron-down",
                        more_down,
                        "results:down",
                    ),
                ]
                .iter()
                .enumerate()
                {
                    let b = Rect::new(
                        grid_r.x + grid_r.width as i32 - 24,
                        grid_r.y + 26 + i as i32 * 24,
                        22,
                        22,
                    );
                    if *live {
                        p.region(b, &format!("db:{t}"), label);
                        p.symbol(sym, b.x + 3, b.y + 3, 16, st.accent);
                    } else {
                        p.region(b, "db:noop", label);
                        p.disabled(if i == 0 {
                            "the first rows are shown"
                        } else {
                            "the last rows are shown"
                        });
                        p.symbol(sym, b.x + 3, b.y + 3, 16, FAINT);
                    }
                }
            } else {
                p.box_(grid_r, Color::WHITE, 0);
            }
            p.box_(msg_r, Color::WHITE, 0);
            p.border(msg_r, Color::TRANSPARENT, 0, LINE);
            let color = if res.error {
                Color::rgb(190, 30, 30)
            } else {
                INK
            };
            let mut y = msg_r.y + 6;
            for line in res.message.lines() {
                if y + 16 > msg_r.y + msg_r.height as i32 {
                    break;
                }
                p.left(
                    msg_r.x + 8,
                    y,
                    msg_r.width.saturating_sub(16),
                    line,
                    12,
                    color,
                );
                y += 17;
            }
        }
        None => {
            p.box_(grid_r, Color::WHITE, 0);
            p.box_(msg_r, Color::WHITE, 0);
            p.border(msg_r, Color::TRANSPARENT, 0, LINE);
        }
    }
}
fn execute(c: &Client, p: &mut Painter, r: Rect, st: &Style) {
    let run = |t: &str| {
        need_db(c, t).and_then(|t| {
            if c.sql.trim().is_empty() {
                Err("there is no SQL to execute".into())
            } else {
                Ok(t)
            }
        })
    };
    sub_toolbar(
        p,
        Rect::new(r.x, r.y, r.width, 30),
        &[
            ("Execute all (F5)", "play", run("run")),
            (
                "Execute current line (Shift+F5)",
                "skip-next",
                run("runline"),
            ),
            (
                "Clear",
                "eraser",
                if c.sql.is_empty() {
                    Err("the editor is empty".into())
                } else {
                    Ok("clearsql".into())
                },
            ),
            ("Stop", "close", Err("nothing is running".into())),
        ],
        st,
    );
    let body = Rect::new(
        r.x + 4,
        r.y + 32,
        r.width.saturating_sub(8),
        r.height.saturating_sub(36),
    );
    let editor_h = body.height * 2 / 5;
    let msg_h = 90.min(body.height / 4);
    sql_editor(
        c,
        p,
        Rect::new(body.x, body.y, body.width, editor_h),
        st,
        12,
    );
    let grid_r = Rect::new(
        body.x,
        body.y + editor_h as i32 + 4,
        body.width,
        body.height.saturating_sub(editor_h + msg_h + 8),
    );
    let msg_r = Rect::new(
        body.x,
        grid_r.y + grid_r.height as i32 + 4,
        body.width,
        msg_h,
    );
    results(c, p, grid_r, msg_r, st);
}

// ----- TablePlus -----

fn tableplus_window(c: &Client, p: &mut Painter, env: &AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let st = Style {
        accent: Color::rgb(10, 132, 255),
        chrome: Color::rgb(236, 236, 236),
        font: 12,
        row_h: 24,
        header_bg: Color::rgb(246, 246, 246),
        selection: Color(10, 132, 255, 45),
        stripe: Some(Color::rgb(248, 248, 250)),
    };
    let Some(db) = &c.db else {
        welcome(c, p, env, &st);
        return;
    };
    // Toolbar.
    p.box_(Rect::new(0, 0, w, 44), st.chrome, 0);
    p.hline(0, 44, w, LINE);
    let left: [(&str, &str, Result<String, String>); 3] = [
        ("Open", "folder", ok("open")),
        ("SQL", "terminal", ok("sql")),
        ("Refresh", "reload", table_target(c, "refresh")),
    ];
    let mut x = 10;
    for (label, sym, target) in &left {
        let tw = control_width(p, label, true);
        control(
            p,
            Rect::new(x, 8, tw, 28),
            label,
            Some(sym),
            target.as_deref().map_err(String::as_str),
            &st,
        );
        x += tw as i32 + 4;
    }
    let pill_w = 280.min(w.saturating_sub(460));
    if pill_w > 100 {
        let pill = Rect::new((w as i32 - pill_w as i32) / 2, 8, pill_w, 28);
        p.box_(pill, Color::WHITE, 6);
        p.border(pill, Color::TRANSPARENT, 6, LINE);
        p.circle(pill.x + 14, pill.y + 14, 4, Color::rgb(52, 199, 89));
        p.label(
            pill.x + 24,
            pill.y + 6,
            pill_w - 30,
            &format!("SQLite : {}", c.name),
            12,
            INK,
            false,
            Align::Center,
        );
    }
    let right: [(&str, &str, Result<String, String>); 2] = [
        ("Import", "download", ok("import")),
        ("Export", "share", table_target(c, "exportcsv")),
    ];
    let mut x = w as i32 - 10;
    for (label, sym, target) in right.iter().rev() {
        let tw = control_width(p, label, true);
        x -= tw as i32;
        control(
            p,
            Rect::new(x, 8, tw, 28),
            label,
            Some(sym),
            target.as_deref().map_err(String::as_str),
            &st,
        );
        x -= 4;
    }
    // Sidebar of tables and views.
    let side_w = 210.min(w / 3);
    let side = Rect::new(0, 45, side_w, h.saturating_sub(45));
    p.box_(side, Color::rgb(242, 242, 245), 0);
    p.vline(side_w as i32, 45, side.height, LINE);
    let mut y = side.y + 10;
    let close = Rect::new(side.x + side_w as i32 - 30, side.y + 6, 22, 22);
    p.region(close, "db:close", "Close connection");
    p.symbol("close", close.x + 4, close.y + 4, 14, MUTED);
    for (title, kind) in [("Tables", "table"), ("Views", "view")] {
        let items: Vec<cw_sql::SchemaEntry> =
            db.schema().into_iter().filter(|e| e.kind == kind).collect();
        p.label(
            side.x + 12,
            y,
            side_w - 40,
            title,
            11,
            MUTED,
            true,
            Align::Left,
        );
        y += 22;
        for e in items {
            if y + 24 > side.y + side.height as i32 {
                break;
            }
            let r = Rect::new(side.x + 6, y, side_w - 12, 24);
            if c.table.as_deref() == Some(e.name.as_str()) && c.tab != Tab::Execute {
                p.box_(r, st.selection, 5);
            }
            p.region(r, &format!("db:table:{}", e.name), &e.name);
            p.symbol(
                if kind == "table" { "grid" } else { "eye" },
                r.x + 6,
                r.y + 5,
                14,
                st.accent,
            );
            p.left(r.x + 26, r.y + 4, r.width - 30, &e.name, 12, INK);
            y += 26;
        }
        y += 8;
    }
    // Main area: a tab for what is open, then its content and the bottom bar.
    let main = Rect::new(
        side_w as i32 + 1,
        45,
        w.saturating_sub(side_w + 1),
        h.saturating_sub(45),
    );
    let title = match c.tab {
        Tab::Execute => "SQL Query".to_string(),
        _ => c.table.clone().unwrap_or_else(|| "No table".into()),
    };
    p.box_(
        Rect::new(main.x, main.y, main.width, 28),
        Color::rgb(246, 246, 246),
        0,
    );
    let tw = p.measure(&title, 12, false) + 40;
    p.box_(Rect::new(main.x, main.y, tw, 28), Color::WHITE, 0);
    p.label(
        main.x + 12,
        main.y + 6,
        tw - 16,
        &title,
        12,
        INK,
        false,
        Align::Left,
    );
    p.hline(main.x, main.y + 28, main.width, LINE);
    let bottom_h = 34;
    let content = Rect::new(
        main.x,
        main.y + 29,
        main.width,
        main.height.saturating_sub(29 + bottom_h),
    );
    match c.tab {
        Tab::Execute => {
            let editor_h = content.height * 2 / 5;
            let bar = Rect::new(content.x, content.y + editor_h as i32, content.width, 32);
            sql_editor(
                c,
                p,
                Rect::new(content.x, content.y, content.width, editor_h),
                &st,
                13,
            );
            p.box_(bar, st.header_bg, 0);
            let can_run = !c.sql.trim().is_empty();
            let run = |t: &str| {
                if can_run {
                    Ok(t.to_string())
                } else {
                    Err("there is no SQL to run".to_string())
                }
            };
            let mut x = bar.x + 8;
            for (label, t) in [("Run Current", "runline"), ("Run All", "run")] {
                let tw = control_width(p, label, true);
                let r = Rect::new(x, bar.y + 3, tw, 26);
                control(
                    p,
                    r,
                    label,
                    Some("play"),
                    run(t).as_deref().map_err(String::as_str),
                    &st,
                );
                p.border(r, Color::TRANSPARENT, 5, LINE);
                x += tw as i32 + 6;
            }
            let msg_h = 70;
            let grid_r = Rect::new(
                content.x,
                bar.y + 32,
                content.width,
                content.height.saturating_sub(editor_h + 32 + msg_h),
            );
            let msg_r = Rect::new(
                content.x,
                grid_r.y + grid_r.height as i32,
                content.width,
                msg_h,
            );
            results(c, p, grid_r, msg_r, &st);
        }
        Tab::Structure => super::design_view::tableplus_structure(c, db, p, content, st.accent),
        _ => {
            let n = fits(content.height, &st, false);
            match c.rows(c.offset, n) {
                Ok(rows) => {
                    let values: Vec<Vec<Value>> =
                        rows.rows.iter().map(|(_, v)| v.clone()).collect();
                    let spec = GridSpec {
                        columns: &rows.columns,
                        rows: values,
                        first: c.offset,
                        cells: true,
                        selected: c.cell,
                        edit: c.edit.as_ref(),
                        sort: c.sort,
                        filters: None,
                        row_numbers: false,
                    };
                    grid(p, content, &spec, &st);
                }
                Err(e) => p.center(content.x, content.y + 40, content.width, &e, 12, MUTED),
            }
        }
    }
    // Bottom bar: Data / Structure, row buttons, staged changes, pages.
    let by = main.y + main.height as i32 - bottom_h as i32;
    let bar = Rect::new(main.x, by, main.width, bottom_h);
    p.box_(bar, Color::rgb(246, 246, 246), 0);
    p.hline(bar.x, by, bar.width, LINE);
    let seg = Rect::new(bar.x + 10, by + 6, 150, 22);
    p.border(seg, Color::WHITE, 5, LINE);
    for (i, (label, t, tab)) in [
        ("Data", "tab:browse", Tab::Browse),
        ("Structure", "tab:structure", Tab::Structure),
    ]
    .iter()
    .enumerate()
    {
        let r = Rect::new(seg.x + i as i32 * 75, seg.y, 75, 22);
        if c.tab == *tab {
            p.box_(
                Rect::new(r.x + 2, r.y + 2, r.width - 4, r.height - 4),
                st.accent,
                4,
            );
        }
        match table_target(c, t) {
            Ok(t) => p.region(r, &format!("db:{t}"), label),
            Err(why) => {
                p.region(r, "db:noop", label);
                p.disabled(&why);
            }
        }
        p.label(
            r.x,
            r.y + 3,
            r.width,
            label,
            12,
            if c.tab == *tab { Color::WHITE } else { INK },
            false,
            Align::Center,
        );
    }
    let mut x = seg.x + seg.width as i32 + 10;
    for (label, sym, target) in [
        ("Row", "plus", record_target(c, "newrow", false)),
        ("Delete", "minus", record_target(c, "deleterow", true)),
    ] {
        let tw = control_width(p, label, true);
        control(
            p,
            Rect::new(x, by + 5, tw, 24),
            label,
            Some(sym),
            target.as_deref().map_err(String::as_str),
            &st,
        );
        x += tw as i32 + 4;
    }
    if c.modified() {
        let commit = Rect::new(bar.x + bar.width as i32 - 100, by + 5, 90, 24);
        p.button(commit, st.accent, 5, "db:write", "Commit");
        p.label(
            commit.x,
            commit.y + 4,
            commit.width,
            "Commit ⌘S",
            12,
            Color::WHITE,
            true,
            Align::Center,
        );
        let discard = Rect::new(commit.x - 80, by + 5, 72, 24);
        p.border(discard, Color::WHITE, 5, LINE);
        p.region(discard, "db:revert", "Discard");
        p.label(
            discard.x,
            discard.y + 4,
            discard.width,
            "Discard",
            12,
            INK,
            false,
            Align::Center,
        );
    } else if c.tab == Tab::Browse && c.table.is_some() {
        if let Ok(rows) = c.rows(0, 0) {
            let label = format!("{} rows", rows.total);
            p.right(
                bar.x,
                by + 9,
                bar.width.saturating_sub(70),
                &label,
                12,
                MUTED,
            );
            let prev = Rect::new(bar.x + bar.width as i32 - 60, by + 6, 24, 22);
            let next = Rect::new(bar.x + bar.width as i32 - 32, by + 6, 24, 22);
            for (r, sym, live, t, why) in [
                (
                    prev,
                    "chevron-left",
                    c.offset > 0,
                    "page:prev",
                    "this is the first page",
                ),
                (
                    next,
                    "chevron-right",
                    c.offset + super::PAGE < rows.total,
                    "page:next",
                    "this is the last page",
                ),
            ] {
                if live {
                    p.region(
                        r,
                        &format!("db:{t}"),
                        if t == "page:prev" {
                            "Previous page"
                        } else {
                            "Next page"
                        },
                    );
                } else {
                    p.region(
                        r,
                        "db:noop",
                        if t == "page:prev" {
                            "Previous page"
                        } else {
                            "Next page"
                        },
                    );
                    p.disabled(why);
                }
                p.symbol(
                    sym,
                    r.x + 4,
                    r.y + 3,
                    16,
                    if live { st.accent } else { FAINT },
                );
            }
        }
    }
}
fn welcome(c: &Client, p: &mut Painter, env: &AppEnv<'_>, st: &Style) {
    let (w, h) = (env.width, env.height);
    p.box_(Rect::new(0, 0, w, h), Color::rgb(246, 246, 248), 0);
    p.strong_center(0, h as i32 / 2 - 110, w, "TablePlus", 26, INK);
    p.center(
        0,
        h as i32 / 2 - 72,
        w,
        "Open a SQLite database file, or create a new one.",
        13,
        MUTED,
    );
    let bw = 150;
    let open = Rect::new(w as i32 / 2 - bw as i32 - 6, h as i32 / 2 - 30, bw, 32);
    let new = Rect::new(w as i32 / 2 + 6, h as i32 / 2 - 30, bw, 32);
    p.button(open, st.accent, 6, "db:open", "Open…");
    p.label(
        open.x,
        open.y + 8,
        open.width,
        "Open…",
        13,
        Color::WHITE,
        true,
        Align::Center,
    );
    p.border(new, Color::WHITE, 6, LINE);
    p.region(new, "db:new", "New Database");
    p.label(
        new.x,
        new.y + 8,
        new.width,
        "New Database",
        13,
        INK,
        false,
        Align::Center,
    );
    let recent: Vec<&String> = c
        .folder
        .entries
        .iter()
        .filter(|e| super::opens(e))
        .collect();
    let mut y = h as i32 / 2 + 24;
    if !recent.is_empty() {
        p.center(0, y, w, &format!("In {}", c.folder.path), 11, MUTED);
        y += 22;
        for name in recent.iter().take(6) {
            p.center(0, y, w, name, 12, INK);
            y += 18;
        }
        p.center(0, y + 4, w, "Choose Open… to pick one.", 11, FAINT);
    }
}

// ----- dialogs -----

fn overlays(c: &Client, p: &mut Painter, env: &AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let accent = if tableplus(env.theme) {
        Color::rgb(10, 132, 255)
    } else {
        Color::rgb(48, 140, 198)
    };
    if c.menu.as_deref() == Some("tables") && !tableplus(env.theme) {
        let items = menu_items(c, "tables");
        if !items.is_empty() {
            menu(p, 60, 22 + 36 + 26 + 30, &items, w as i32);
        }
    }
    let scrim = |p: &mut Painter| {
        p.box_(Rect::new(0, 0, w, h), Color(0, 0, 0, 50), 0);
        p.region(Rect::new(0, 0, w, h), "db:noop", "Dialog");
    };
    let button = |p: &mut Painter, r: Rect, label: &str, target: &str, primary: bool| {
        if primary {
            p.button(r, accent, 4, target, label);
            p.label(
                r.x,
                r.y + 6,
                r.width,
                label,
                12,
                Color::WHITE,
                true,
                Align::Center,
            );
        } else {
            p.border(r, Color::WHITE, 4, LINE);
            p.region(r, target, label);
            p.label(r.x, r.y + 6, r.width, label, 12, INK, false, Align::Center);
        }
    };
    if let Some(purpose) = c.dialog_files {
        scrim(p);
        let dw = 480.min(w.saturating_sub(20));
        let dh = 360.min(h.saturating_sub(20));
        let r = Rect::new(
            (w as i32 - dw as i32) / 2,
            (h as i32 - dh as i32) / 2,
            dw,
            dh,
        );
        p.drop_shadow(r, 8, 16, 70, 4);
        p.box_(r, Color::WHITE, 8);
        let title = match purpose {
            Purpose::Open => "Choose a database file",
            Purpose::Import => "Choose a text file to import",
        };
        p.strong(r.x + 16, r.y + 14, dw - 32, title, 14, INK);
        p.left(r.x + 16, r.y + 38, dw - 32, &c.folder.path, 11, MUTED);
        let list = Rect::new(r.x + 12, r.y + 58, dw - 24, dh.saturating_sub(58 + 50));
        p.border(list, Color::WHITE, 4, LINE);
        let mut y = list.y + 2;
        let mut rows: Vec<(String, String, &str)> =
            vec![("..".into(), "db:folder:..".into(), "folder")];
        for e in &c.folder.entries {
            if let Some(dir) = e.strip_suffix('/') {
                rows.push((dir.into(), format!("db:folder:{dir}"), "folder"));
            } else if match purpose {
                Purpose::Open => super::opens(e),
                Purpose::Import => super::importable(e),
            } {
                rows.push((e.clone(), format!("db:openfile:{e}"), "document"));
            }
        }
        if let Some(problem) = &c.folder.problem {
            p.left(list.x + 8, y + 6, list.width - 16, problem, 12, MUTED);
        } else {
            for (label, target, sym) in rows {
                if y + 26 > list.y + list.height as i32 {
                    break;
                }
                let row = Rect::new(list.x + 2, y, list.width - 4, 26);
                p.region(row, &target, &label);
                p.symbol(sym, row.x + 6, row.y + 5, 16, accent);
                p.left(row.x + 28, row.y + 5, row.width - 32, &label, 12, INK);
                y += 26;
            }
        }
        button(
            p,
            Rect::new(r.x + dw as i32 - 96, r.y + dh as i32 - 40, 80, 28),
            "Cancel",
            "db:cancel",
            false,
        );
        return;
    }
    if c.designing() {
        scrim(p);
        super::design_view::table_dialog(c, p, w, h, accent);
        super::design_view::index_dialog(c, p, w, h, accent);
        if let Some(m) = &c.message {
            message_box(p, m, w, h, accent);
        }
        return;
    }
    if let Some(d) = &c.dialog {
        scrim(p);
        let dw = 420.min(w.saturating_sub(20));
        let r = Rect::new((w as i32 - dw as i32) / 2, h as i32 / 3 - 60, dw, 130);
        p.drop_shadow(r, 8, 16, 70, 4);
        p.box_(r, Color::WHITE, 8);
        match d {
            Dialog::CloseChanges { .. } => {
                p.paragraph(
                    r.x + 16,
                    r.y + 16,
                    dw - 32,
                    "Do you want to save the changes made to the database file?",
                    13,
                    INK,
                );
                button(
                    p,
                    Rect::new(r.x + dw as i32 - 260, r.y + 88, 76, 28),
                    "Cancel",
                    "db:cancel",
                    false,
                );
                button(
                    p,
                    Rect::new(r.x + dw as i32 - 176, r.y + 88, 76, 28),
                    "Discard",
                    "db:discard",
                    false,
                );
                button(
                    p,
                    Rect::new(r.x + dw as i32 - 92, r.y + 88, 76, 28),
                    "Save",
                    "db:savechanges",
                    true,
                );
            }
            Dialog::DropTable(name) => {
                p.paragraph(r.x + 16, r.y + 16, dw - 32, &format!("Are you sure you want to delete the table '{name}'?\nAll data associated with the table will be lost."), 13, INK);
                button(
                    p,
                    Rect::new(r.x + dw as i32 - 176, r.y + 88, 76, 28),
                    "No",
                    "db:no",
                    false,
                );
                button(
                    p,
                    Rect::new(r.x + dw as i32 - 92, r.y + 88, 76, 28),
                    "Yes",
                    "db:yes",
                    true,
                );
            }
        }
        return;
    }
    if let Some(m) = &c.message {
        scrim(p);
        message_box(p, m, w, h, accent);
    }
    if c.loading.is_some() || c.importing.is_some() {
        p.center(0, h as i32 - 60, w, "Reading…", 12, MUTED);
    }
}

fn message_box(p: &mut Painter, m: &str, w: u32, h: u32, accent: Color) {
    p.box_(Rect::new(0, 0, w, h), Color(0, 0, 0, 30), 0);
    p.region(Rect::new(0, 0, w, h), "db:noop", "Message");
    let dw = 420.min(w.saturating_sub(20));
    let lines = m.lines().count().min(12) as u32;
    let dh = 90 + lines * 18;
    let r = Rect::new((w as i32 - dw as i32) / 2, h as i32 / 3 - 60, dw, dh);
    p.drop_shadow(r, 8, 16, 70, 4);
    p.box_(r, Color::WHITE, 8);
    let mut y = r.y + 16;
    for line in m.lines().take(12) {
        p.left(r.x + 16, y, dw - 32, line, 12, INK);
        y += 18;
    }
    let ok = Rect::new(r.x + dw as i32 - 92, r.y + dh as i32 - 40, 76, 28);
    p.button(ok, accent, 4, "db:dismiss", "OK");
    p.label(
        ok.x,
        ok.y + 6,
        ok.width,
        "OK",
        12,
        Color::WHITE,
        true,
        Align::Center,
    );
}
/// Semantic projection: what is open, its tables, the browse rows and the last result.
pub fn page(c: &Client, page: &mut cw_protocol::Page) {
    use cw_protocol::PageElement as E;
    let act = |url: &str| cw_protocol::PageAction {
        method: "APP".into(),
        url: url.into(),
        fields: Default::default(),
    };
    page.elements.push(E::Heading {
        id: "db-title".into(),
        text: if c.name.is_empty() {
            "No database".into()
        } else {
            c.name.clone()
        },
        level: 2,
    });
    if let Some(m) = &c.message {
        page.elements.push(E::Text {
            id: "db-message".into(),
            text: m.clone(),
        });
    }
    for t in c.tables() {
        let id = format!("db:table:{t}");
        page.elements.push(E::Button {
            id: id.clone(),
            text: t,
            action: act(&id),
            style: None,
        });
    }
    if c.tab == Tab::Browse {
        if let Ok(rows) = c.rows(c.offset, super::PAGE) {
            let mut lines = vec![rows.columns.join("\t")];
            for (_, r) in &rows.rows {
                lines.push(r.iter().map(shown).collect::<Vec<_>>().join("\t"));
            }
            page.elements.push(E::Text {
                id: "db-rows".into(),
                text: lines.join("\n"),
            });
        }
    }
    if c.tab == Tab::Execute {
        page.elements.push(E::Input {
            id: "db-sql".into(),
            label: "SQL".into(),
            value: c.sql.clone(),
            placeholder: String::new(),
        });
        if let Some(r) = &c.result {
            let mut lines = vec![r.columns.join("\t")];
            for row in r.rows.iter().take(50) {
                lines.push(row.iter().map(shown).collect::<Vec<_>>().join("\t"));
            }
            page.elements.push(E::Text {
                id: "db-result".into(),
                text: lines.join("\n"),
            });
            page.elements.push(E::Text {
                id: "db-result-message".into(),
                text: r.message.clone(),
            });
        }
    }
    for (id, label) in [
        ("db:new", "New Database"),
        ("db:open", "Open Database"),
        ("db:write", "Write Changes"),
        ("db:revert", "Revert Changes"),
        ("db:tab:browse", "Browse Data"),
        ("db:tab:execute", "Execute SQL"),
        ("db:run", "Execute all"),
    ] {
        page.elements.push(E::Button {
            id: id.into(),
            text: label.into(),
            action: act(id),
            style: None,
        });
    }
}
