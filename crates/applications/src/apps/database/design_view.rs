//! Painting the designers: DB Browser's Edit Table Definition and Edit Index Definition
//! dialogs, and TablePlus's editable structure view.
use super::designer::{DesignFocus, COLUMNS, TYPES};
use super::structure::TABLEPLUS_COLUMNS;
use super::Client;
use crate::desktop_scene::shared::Align;
use crate::desktop_scene::Painter;
use cw_scene::{Color, Rect};

const INK: Color = Color::rgb(33, 33, 33);
const MUTED: Color = Color::rgb(110, 110, 110);
const FAINT: Color = Color::rgb(170, 170, 170);
const LINE: Color = Color::rgb(214, 214, 214);
/// Why the partial index clause is not offered.
pub const PARTIAL: &str =
    "partial indexes (CREATE INDEX … WHERE) are not supported by the cw-sql engine";

fn text_box(
    p: &mut Painter,
    r: Rect,
    text: &str,
    focused: bool,
    target: &str,
    label: &str,
    accent: Color,
) {
    p.border(r, Color::WHITE, 3, if focused { accent } else { LINE });
    p.region(r, target, label);
    let tw = p.label(
        r.x + 5,
        r.y + (r.height as i32 - 16) / 2,
        r.width.saturating_sub(10),
        text,
        12,
        INK,
        false,
        Align::Left,
    );
    if focused {
        p.vline(
            r.x + 6 + tw as i32,
            r.y + 4,
            r.height.saturating_sub(8),
            INK,
        );
    }
}
fn check_box(p: &mut Painter, x: i32, y: i32, on: bool, accent: Color) {
    let b = Rect::new(x, y, 14, 14);
    p.border(b, Color::WHITE, 2, if on { accent } else { MUTED });
    if on {
        p.symbol("check", b.x + 1, b.y + 1, 12, accent);
    }
}
/// A push button, live or disabled with the reason.
fn push(
    p: &mut Painter,
    r: Rect,
    label: &str,
    target: Result<&str, &str>,
    primary: bool,
    accent: Color,
) {
    match target {
        Ok(t) if primary => p.button(r, accent, 4, t, label),
        Ok(t) => {
            p.border(r, Color::WHITE, 4, LINE);
            p.region(r, t, label);
        }
        Err(why) => {
            p.border(r, Color::rgb(245, 245, 245), 4, LINE);
            p.region(r, "db:noop", label);
            p.disabled(why);
        }
    }
    let ink = match (target, primary) {
        (Err(_), _) => FAINT,
        (Ok(_), true) => Color::WHITE,
        _ => INK,
    };
    p.label(
        r.x,
        r.y + (r.height as i32 - 16) / 2,
        r.width,
        label,
        12,
        ink,
        primary,
        Align::Center,
    );
}
/// SQL shown read-only under a designer, line by line.
fn sql_preview(p: &mut Painter, r: Rect, sql: &str) {
    p.border(r, Color::rgb(250, 250, 250), 3, LINE);
    let mut y = r.y + 4;
    for line in sql.lines() {
        if y + 16 > r.y + r.height as i32 {
            break;
        }
        p.left(
            r.x + 6,
            y,
            r.width.saturating_sub(12),
            &line.replace('\t', "    "),
            11,
            INK,
        );
        y += 16;
    }
}
fn dropdown(p: &mut Painter, x: i32, y: i32, items: &[(String, String)]) {
    let w = items
        .iter()
        .map(|(l, _)| p.measure(l, 12, false))
        .max()
        .unwrap_or(80)
        + 32;
    let r = Rect::new(x, y, w, items.len() as u32 * 22 + 6);
    p.drop_shadow(r, 4, 8, 50, 2);
    p.box_(r, Color::WHITE, 3);
    p.border(r, Color::TRANSPARENT, 3, LINE);
    for (i, (label, t)) in items.iter().enumerate() {
        let row = Rect::new(r.x + 3, r.y + 3 + i as i32 * 22, w - 6, 22);
        p.region(row, &format!("db:{t}"), label);
        p.left(row.x + 8, row.y + 3, row.width - 12, label, 12, INK);
    }
}

/// DB Browser's Edit Table Definition dialog.
pub fn table_dialog(c: &Client, p: &mut Painter, w: u32, h: u32, accent: Color) {
    let Some(d) = c.design.as_ref().filter(|d| !d.inline) else {
        return;
    };
    let dw = 820.min(w.saturating_sub(16));
    let dh = 540.min(h.saturating_sub(16));
    let r = Rect::new(
        (w as i32 - dw as i32) / 2,
        (h as i32 - dh as i32) / 2,
        dw,
        dh,
    );
    p.drop_shadow(r, 8, 16, 70, 4);
    p.box_(r, Color::rgb(240, 240, 240), 6);
    let x = r.x + 14;
    let inner = dw.saturating_sub(28);
    p.strong(x, r.y + 10, inner, "Edit table definition", 14, INK);
    // Table name.
    p.left(x, r.y + 42, 60, "Table", 12, INK);
    text_box(
        p,
        Rect::new(x + 60, r.y + 38, inner.saturating_sub(60), 24),
        &d.name,
        d.focus == DesignFocus::Name,
        "db:design:name",
        "Table name",
        accent,
    );
    // Advanced: Without Rowid.
    let adv = Rect::new(x, r.y + 70, 160, 22);
    p.region(adv, "db:design:withoutrowid", "Without Rowid");
    check_box(p, adv.x, adv.y + 4, d.without_rowid, accent);
    p.left(adv.x + 20, adv.y + 3, 140, "Without Rowid", 12, INK);
    // Fields toolbar.
    let ty = r.y + 100;
    p.strong(x, ty, 60, "Fields", 12, INK);
    let n = d.fields.len();
    let sel = d.selected;
    let tools: [(&str, Result<&str, &str>); 6] = [
        ("Add", Ok("db:design:add")),
        (
            "Remove",
            if sel.is_some() {
                Ok("db:design:remove")
            } else {
                Err("select a field first")
            },
        ),
        (
            "Move to top",
            match sel {
                Some(0) => Err("that field is already first"),
                Some(_) => Ok("db:design:top"),
                None => Err("select a field first"),
            },
        ),
        (
            "Move up",
            match sel {
                Some(0) => Err("that field is already first"),
                Some(_) => Ok("db:design:up"),
                None => Err("select a field first"),
            },
        ),
        (
            "Move down",
            match sel {
                Some(i) if i + 1 >= n => Err("that field is already last"),
                Some(_) => Ok("db:design:down"),
                None => Err("select a field first"),
            },
        ),
        (
            "Move to bottom",
            match sel {
                Some(i) if i + 1 >= n => Err("that field is already last"),
                Some(_) => Ok("db:design:bottom"),
                None => Err("select a field first"),
            },
        ),
    ];
    let mut bx = x + 60;
    for (label, t) in tools {
        let bw = p.measure(label, 12, false) + 20;
        push(p, Rect::new(bx, ty - 4, bw, 24), label, t, false, accent);
        bx += bw as i32 + 4;
    }
    // The Fields grid.
    let grid = Rect::new(x, ty + 26, inner, dh.saturating_sub(100 + 26 + 200));
    p.box_(grid, Color::WHITE, 0);
    p.border(grid, Color::TRANSPARENT, 0, LINE);
    let fixed: [u32; 9] = [130, 96, 34, 34, 34, 34, 96, 110, 80];
    let last = inner.saturating_sub(fixed.iter().sum::<u32>()).max(60);
    let widths: Vec<u32> = fixed.iter().copied().chain([last]).collect();
    let row_h: i32 = 24;
    p.box_(
        Rect::new(grid.x, grid.y, grid.width, row_h as u32),
        Color::rgb(245, 245, 245),
        0,
    );
    let mut cx = grid.x;
    let mut xs = Vec::new();
    for (i, (label, _)) in COLUMNS.iter().enumerate() {
        xs.push(cx);
        p.label(
            cx + 4,
            grid.y + 4,
            widths[i].saturating_sub(8),
            label,
            12,
            INK,
            true,
            Align::Left,
        );
        p.vline(cx + widths[i] as i32 - 1, grid.y, grid.height, LINE);
        cx += widths[i] as i32;
    }
    p.hline(grid.x, grid.y + row_h, grid.width, LINE);
    let mut type_menu = None;
    for (ri, f) in d.fields.iter().enumerate() {
        let y = grid.y + row_h + ri as i32 * row_h;
        if y + row_h > grid.y + grid.height as i32 {
            break;
        }
        if d.selected == Some(ri) {
            p.box_(
                Rect::new(grid.x + 1, y, grid.width - 2, row_h as u32),
                Color(accent.0, accent.1, accent.2, 40),
                0,
            );
        }
        for (ci, (label, text)) in COLUMNS.iter().enumerate() {
            let cell = Rect::new(xs[ci], y, widths[ci], row_h as u32);
            let target = format!("db:design:cell:{ri}:{ci}");
            if *text {
                let focused = d.focus == DesignFocus::Cell(ri, ci);
                if focused {
                    text_box(
                        p,
                        Rect::new(cell.x + 1, cell.y + 1, cell.width - 2, cell.height - 2),
                        f.text(ci),
                        true,
                        &target,
                        label,
                        accent,
                    );
                } else {
                    p.region(cell, &target, &format!("{label} of field {}", ri + 1));
                    p.left(
                        cell.x + 4,
                        cell.y + 4,
                        cell.width.saturating_sub(8),
                        f.text(ci),
                        12,
                        INK,
                    );
                }
                if ci == 1 {
                    // The Type drop-down.
                    let arrow = Rect::new(cell.x + cell.width as i32 - 18, cell.y + 3, 16, 18);
                    p.region(arrow, &format!("db:menu:designtype:{ri}"), "Choose a type");
                    p.symbol("chevron-down", arrow.x + 2, arrow.y + 3, 12, MUTED);
                    if c.menu.as_deref() == Some(&format!("designtype:{ri}")) {
                        type_menu = Some((ri, cell.x, cell.y + row_h));
                    }
                }
            } else {
                p.region(cell, &target, &format!("{label} of field {}", ri + 1));
                let on = match ci {
                    2 => f.not_null,
                    3 => f.pk,
                    4 => f.autoinc,
                    _ => f.unique,
                };
                check_box(p, cell.x + 10, cell.y + 5, on, accent);
            }
        }
        p.hline(grid.x, y + row_h - 1, grid.width, Color::rgb(236, 236, 236));
    }
    if d.fields.is_empty() {
        p.center(
            grid.x,
            grid.y + 60,
            grid.width,
            "Use Add to add a field.",
            12,
            FAINT,
        );
    }
    if !d.constraints.is_empty() {
        let kept: Vec<String> = d
            .constraints
            .iter()
            .map(|c| {
                if c.kind == "CHECK" {
                    format!("CHECK({})", c.text)
                } else {
                    format!("{} ({})", c.kind, c.columns.join(", "))
                }
            })
            .collect();
        p.left(
            grid.x,
            grid.y + grid.height as i32 + 4,
            grid.width,
            &format!("Constraints kept: {}", kept.join("; ")),
            11,
            MUTED,
        );
    }
    // The statement the design makes.
    let sy = grid.y + grid.height as i32 + 22;
    let sql = d.create_sql(&d.name);
    sql_preview(p, Rect::new(x, sy, inner, 140), &format!("{sql};"));
    let by = r.y + dh as i32 - 38;
    push(
        p,
        Rect::new(r.x + dw as i32 - 186, by, 80, 28),
        "OK",
        Ok("db:design:ok"),
        true,
        accent,
    );
    push(
        p,
        Rect::new(r.x + dw as i32 - 96, by, 80, 28),
        "Cancel",
        Ok("db:design:cancel"),
        false,
        accent,
    );
    if let Some((ri, mx, my)) = type_menu {
        let items: Vec<(String, String)> = TYPES
            .iter()
            .map(|t| (t.to_string(), format!("design:type:{ri}:{t}")))
            .collect();
        dropdown(p, mx, my, &items);
    }
}

/// DB Browser's Edit Index Definition dialog (TablePlus's New Index sheet).
pub fn index_dialog(c: &Client, p: &mut Painter, w: u32, h: u32, accent: Color) {
    let Some(d) = &c.index_design else {
        return;
    };
    let dw = 600.min(w.saturating_sub(16));
    let dh = 460.min(h.saturating_sub(16));
    let r = Rect::new(
        (w as i32 - dw as i32) / 2,
        (h as i32 - dh as i32) / 2,
        dw,
        dh,
    );
    p.drop_shadow(r, 8, 16, 70, 4);
    p.box_(r, Color::rgb(240, 240, 240), 6);
    let x = r.x + 14;
    let inner = dw.saturating_sub(28);
    p.strong(x, r.y + 10, inner, "Edit Index Definition", 14, INK);
    p.left(x, r.y + 42, 80, "Index name", 12, INK);
    text_box(
        p,
        Rect::new(x + 90, r.y + 38, inner.saturating_sub(90), 24),
        &d.name,
        d.focus == DesignFocus::Name,
        "db:index:name",
        "Index name",
        accent,
    );
    p.left(x, r.y + 72, 80, "Table", 12, INK);
    let combo = Rect::new(x + 90, r.y + 68, 220, 24);
    p.border(combo, Color::WHITE, 3, LINE);
    p.region(combo, "db:menu:indextables", "Table");
    p.left(
        combo.x + 6,
        combo.y + 4,
        combo.width - 30,
        &d.table,
        12,
        INK,
    );
    p.symbol(
        "chevron-down",
        combo.x + combo.width as i32 - 20,
        combo.y + 6,
        12,
        MUTED,
    );
    let uq = Rect::new(combo.x + combo.width as i32 + 20, combo.y, 100, 24);
    p.region(uq, "db:index:unique", "Unique");
    check_box(p, uq.x, uq.y + 5, d.unique, accent);
    p.left(uq.x + 20, uq.y + 4, 80, "Unique", 12, INK);
    // Table columns | Index columns.
    let ly = r.y + 104;
    let half = (inner - 12) / 2;
    let left = Rect::new(x, ly + 20, half, 170);
    let right = Rect::new(x + half as i32 + 12, ly + 20, half, 170);
    p.strong(left.x, ly, half, "Table columns", 12, INK);
    p.strong(right.x, ly, half, "Index columns", 12, INK);
    for b in [left, right] {
        p.box_(b, Color::WHITE, 0);
        p.border(b, Color::TRANSPARENT, 0, LINE);
    }
    let columns =
        c.db.as_ref()
            .and_then(|db| db.table_info(&d.table))
            .unwrap_or_default();
    for (i, col) in columns.iter().enumerate() {
        let row = Rect::new(left.x + 1, left.y + 1 + i as i32 * 22, left.width - 2, 22);
        if row.y + 22 > left.y + left.height as i32 {
            break;
        }
        let used = d.columns.iter().any(|(n, _)| *n == col.name);
        p.region(row, &format!("db:index:col:{}", col.name), &col.name);
        check_box(p, row.x + 4, row.y + 4, used, accent);
        p.left(row.x + 24, row.y + 3, row.width - 90, &col.name, 12, INK);
        p.left(
            row.x + row.width as i32 - 70,
            row.y + 3,
            66,
            &col.decl_type,
            11,
            MUTED,
        );
    }
    for (i, (name, desc)) in d.columns.iter().enumerate() {
        let row = Rect::new(
            right.x + 1,
            right.y + 1 + i as i32 * 22,
            right.width - 2,
            22,
        );
        if row.y + 22 > right.y + right.height as i32 {
            break;
        }
        p.left(row.x + 6, row.y + 3, row.width - 90, name, 12, INK);
        let order = Rect::new(row.x + row.width as i32 - 60, row.y + 1, 54, 20);
        p.border(order, Color::WHITE, 3, LINE);
        p.region(order, &format!("db:index:order:{i}"), "Sort order");
        p.center(
            order.x,
            order.y + 2,
            order.width,
            if *desc { "DESC" } else { "ASC" },
            11,
            INK,
        );
    }
    // Partial index clause.
    let wy = left.y + left.height as i32 + 10;
    p.left(x, wy + 4, 130, "Partial index clause", 12, INK);
    // The engine keeps every row in every index, so a partial index cannot be made.
    let clause = Rect::new(x + 136, wy, inner.saturating_sub(136), 24);
    p.border(clause, Color::rgb(245, 245, 245), 3, LINE);
    p.region(clause, "db:noop", "Partial index clause");
    p.disabled(PARTIAL);
    sql_preview(p, Rect::new(x, wy + 34, inner, 80), &d.sql());
    let by = r.y + dh as i32 - 38;
    push(
        p,
        Rect::new(r.x + dw as i32 - 186, by, 80, 28),
        "OK",
        Ok("db:index:ok"),
        true,
        accent,
    );
    push(
        p,
        Rect::new(r.x + dw as i32 - 96, by, 80, 28),
        "Cancel",
        Ok("db:index:cancel"),
        false,
        accent,
    );
    if c.menu.as_deref() == Some("indextables") {
        let items: Vec<(String, String)> = c
            .tables()
            .into_iter()
            .filter(|t| c.db.as_ref().is_some_and(|db| db.is_table(t)))
            .map(|t| (t.clone(), format!("index:table:{t}")))
            .collect();
        dropdown(p, combo.x, combo.y + 24, &items);
    }
}

/// TablePlus's structure view: the columns (editable in place, staged until Commit)
/// and the indexes under them.
pub fn tableplus_structure(
    c: &Client,
    db: &cw_sql::Database,
    p: &mut Painter,
    r: Rect,
    accent: Color,
) {
    let Some(table) = &c.table else {
        p.center(r.x, r.y + 40, r.width, "Choose a table", 12, MUTED);
        return;
    };
    let staged = c
        .design
        .as_ref()
        .filter(|d| d.inline && d.original.as_deref() == Some(table.as_str()))
        .cloned();
    let view = !db.is_table(table);
    let design = match staged {
        Some(d) => Some(d),
        None if !view => super::designer::TableDesign::of_table(db, table).ok(),
        None => None,
    };
    let row_h: i32 = 24;
    let widths: [u32; 6] = [180, 120, 90, 150, 90, 200];
    // Header.
    p.box_(
        Rect::new(r.x, r.y, r.width, row_h as u32),
        Color::rgb(246, 246, 246),
        0,
    );
    let mut xs = Vec::new();
    let mut x = r.x + 30;
    p.label(r.x + 6, r.y + 4, 24, "#", 12, INK, true, Align::Left);
    for (i, (label, _)) in TABLEPLUS_COLUMNS.iter().enumerate() {
        xs.push(x);
        p.label(
            x + 6,
            r.y + 4,
            widths[i] - 8,
            label,
            12,
            INK,
            true,
            Align::Left,
        );
        x += widths[i] as i32;
    }
    p.hline(r.x, r.y + row_h, r.width, LINE);
    let mut y = r.y + row_h + 1;
    match &design {
        Some(d) => {
            for (ri, f) in d.fields.iter().enumerate() {
                if y + row_h > r.y + r.height as i32 - 120 {
                    break;
                }
                if d.selected == Some(ri) {
                    p.box_(
                        Rect::new(r.x, y, r.width, row_h as u32),
                        Color(accent.0, accent.1, accent.2, 40),
                        0,
                    );
                }
                p.left(r.x + 6, y + 4, 24, &(ri + 1).to_string(), 12, MUTED);
                for (ci, (label, field_col)) in TABLEPLUS_COLUMNS.iter().enumerate() {
                    let cell = Rect::new(xs[ci], y, widths[ci], row_h as u32);
                    let target = format!("db:struct:cell:{ri}:{ci}");
                    let text = match field_col {
                        2 => (if f.not_null { "NO" } else { "YES" }).to_string(),
                        3 => (if f.pk { "YES" } else { "" }).to_string(),
                        k => f.text(*k).to_string(),
                    };
                    if d.focus == DesignFocus::Cell(ri, *field_col) {
                        text_box(
                            p,
                            Rect::new(cell.x + 1, cell.y + 1, cell.width - 2, cell.height - 2),
                            &text,
                            true,
                            &target,
                            label,
                            accent,
                        );
                    } else {
                        p.region(cell, &target, &format!("{label} of {}", f.name));
                        let ink = if text.is_empty() { FAINT } else { INK };
                        p.left(
                            cell.x + 6,
                            cell.y + 4,
                            cell.width - 10,
                            if text.is_empty() { "NULL" } else { &text },
                            12,
                            ink,
                        );
                    }
                }
                p.hline(r.x, y + row_h - 1, r.width, Color::rgb(236, 236, 236));
                y += row_h;
            }
        }
        None => {
            for (ri, col) in db.table_info(table).unwrap_or_default().iter().enumerate() {
                p.left(r.x + 6, y + 4, 24, &(ri + 1).to_string(), 12, MUTED);
                p.left(xs[0] + 6, y + 4, widths[0] - 10, &col.name, 12, INK);
                y += row_h;
            }
        }
    }
    // Column buttons.
    let by = y + 6;
    let why_not = if view {
        Some("a view's columns come from its query")
    } else {
        None
    };
    let sel = design.as_ref().and_then(|d| d.selected);
    push(
        p,
        Rect::new(r.x + 8, by, 90, 24),
        "+ Column",
        why_not.map_or(Ok("db:struct:addcol"), Err),
        false,
        accent,
    );
    push(
        p,
        Rect::new(r.x + 104, by, 110, 24),
        "Delete Column",
        match (why_not, sel) {
            (Some(w), _) => Err(w),
            (None, None) => Err("select a column first"),
            (None, Some(_)) => Ok("db:struct:delcol"),
        },
        false,
        accent,
    );
    // Indexes.
    let iy = by + 36;
    p.box_(
        Rect::new(r.x, iy, r.width, row_h as u32),
        Color::rgb(246, 246, 246),
        0,
    );
    for (i, label) in ["index_name", "is_unique", "columns", "condition"]
        .iter()
        .enumerate()
    {
        p.label(
            r.x + 30 + i as i32 * 170,
            iy + 4,
            160,
            label,
            12,
            INK,
            true,
            Align::Left,
        );
    }
    let mut y = iy + row_h + 1;
    for e in db
        .schema()
        .into_iter()
        .filter(|e| e.kind == "index" && e.table.eq_ignore_ascii_case(table))
    {
        if y + row_h > r.y + r.height as i32 - 30 {
            break;
        }
        let sql = e.sql.clone().unwrap_or_default();
        let parsed = cw_sql::parser::parse(&sql)
            .ok()
            .and_then(|s| s.into_iter().next());
        let (unique, cols) = match parsed {
            Some(cw_sql::ast::Stmt::CreateIndex(ci)) => (
                ci.unique,
                ci.columns
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            _ => (true, "(from a constraint)".to_string()),
        };
        let cond = sql
            .to_ascii_uppercase()
            .rfind(" WHERE ")
            .map(|i| sql[i + 7..].to_string())
            .unwrap_or_default();
        p.left(r.x + 30, y + 4, 160, &e.name, 12, INK);
        p.left(
            r.x + 200,
            y + 4,
            160,
            if unique { "TRUE" } else { "FALSE" },
            12,
            INK,
        );
        p.left(r.x + 370, y + 4, 160, &cols, 12, INK);
        p.left(r.x + 540, y + 4, 160, &cond, 12, MUTED);
        let del = Rect::new(r.x + 6, y + 4, 16, 16);
        if e.sql.is_some() {
            p.region(
                del,
                &format!("db:struct:dropindex:{}", e.name),
                "Delete index",
            );
            p.symbol("minus", del.x, del.y, 14, MUTED);
        }
        y += row_h;
    }
    push(
        p,
        Rect::new(r.x + 8, y + 6, 90, 24),
        "+ Index",
        why_not.map_or(Ok("db:createindex"), Err),
        false,
        accent,
    );
}
