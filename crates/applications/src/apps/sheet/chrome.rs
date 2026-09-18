//! Each product's window around the shared grid: Excel's ribbon, formula bar, sheet
//! tabs and status bar; Numbers' toolbar, sheet tabs, canvas and Format inspector;
//! LibreOffice Calc's menus, toolbars and formula bar; and Google Sheets' app bar,
//! formula bar and format sheet. Controls either dispatch `sheet:*` commands or are
//! painted disabled with the reason.
use super::grid::{self, Geom, Palette};
use super::{Book, Flavor};
use crate::desktop_scene::shared::Align;
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::AppEnv;
use cw_scene::{Color, Rect};

const INK: Color = Color::rgb(36, 36, 36);
const MUTED: Color = Color::rgb(110, 110, 110);
const FAINT: Color = Color::rgb(165, 165, 165);
const LINE: Color = Color::rgb(218, 218, 218);

const EXCEL_GREEN: Color = Color::rgb(33, 115, 70);
const EXCEL_CHARTS: [Color; 6] = [
    Color::rgb(68, 114, 196),
    Color::rgb(237, 125, 49),
    Color::rgb(165, 165, 165),
    Color::rgb(255, 192, 0),
    Color::rgb(91, 155, 213),
    Color::rgb(112, 173, 71),
];
const CALC_CHARTS: [Color; 6] = [
    Color::rgb(0, 69, 134),
    Color::rgb(255, 66, 14),
    Color::rgb(255, 211, 32),
    Color::rgb(87, 157, 28),
    Color::rgb(126, 0, 33),
    Color::rgb(131, 202, 255),
];
const NUMBERS_CHARTS: [Color; 6] = [
    Color::rgb(0, 162, 255),
    Color::rgb(97, 216, 54),
    Color::rgb(248, 186, 0),
    Color::rgb(238, 34, 12),
    Color::rgb(152, 80, 232),
    Color::rgb(0, 199, 190),
];
const SHEETS_CHARTS: [Color; 6] = [
    Color::rgb(66, 133, 244),
    Color::rgb(234, 67, 53),
    Color::rgb(251, 188, 4),
    Color::rgb(52, 168, 83),
    Color::rgb(255, 109, 1),
    Color::rgb(70, 189, 198),
];
/// Swatches the fill and font colour pickers offer (Excel's standard colours).
pub(super) const SWATCHES: [(&str, [u8; 3]); 8] = [
    ("Yellow", [255, 255, 0]),
    ("Light Green", [146, 208, 80]),
    ("Light Blue", [189, 215, 238]),
    ("Orange", [255, 192, 0]),
    ("Red", [255, 0, 0]),
    ("Blue", [0, 112, 192]),
    ("Green", [0, 176, 80]),
    ("Gray", [217, 217, 217]),
];

fn hex(c: [u8; 3]) -> String {
    format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// A control: live when `target` is some, otherwise disabled with `why`.
struct Tool<'a> {
    label: &'a str,
    symbol: Option<&'a str>,
    target: Result<String, &'a str>,
    on: bool,
}
fn tool<'a>(label: &'a str, symbol: Option<&'a str>, target: &str) -> Tool<'a> {
    Tool {
        label,
        symbol,
        target: Ok(target.into()),
        on: false,
    }
}
fn off<'a>(label: &'a str, symbol: Option<&'a str>, why: &'a str) -> Tool<'a> {
    Tool {
        label,
        symbol,
        target: Err(why),
        on: false,
    }
}
/// A small square or wide text button.
fn button(p: &mut Painter, r: Rect, t: &Tool, accent: Color) {
    let live = t.target.is_ok();
    if t.on {
        p.box_(r, Color(accent.0, accent.1, accent.2, 40), 3);
    }
    match &t.target {
        Ok(target) => p.region(r, &format!("sheet:{target}"), t.label),
        Err(why) => {
            p.region(r, "sheet:noop", t.label);
            p.disabled(why);
        }
    }
    let ink = if live { INK } else { FAINT };
    match t.symbol {
        Some(sym) => p.symbol(
            sym,
            r.x + (r.width as i32 - 16) / 2,
            r.y + (r.height as i32 - 16) / 2,
            16,
            ink,
        ),
        None => {
            let bold = matches!(t.label, "B");
            p.label(
                r.x,
                r.y + (r.height as i32 - 17) / 2,
                r.width,
                t.label,
                13,
                ink,
                bold,
                Align::Center,
            );
            if t.label == "U" {
                p.hline(
                    r.x + r.width as i32 / 2 - 4,
                    r.y + r.height as i32 / 2 + 7,
                    9,
                    ink,
                );
            }
        }
    }
}
/// An icon above a caption, as ribbons and the Numbers toolbar lay them out.
fn tall(p: &mut Painter, r: Rect, t: &Tool, accent: Color, icon: Color) {
    let live = t.target.is_ok();
    match &t.target {
        Ok(target) => p.region(r, &format!("sheet:{target}"), t.label),
        Err(why) => {
            p.region(r, "sheet:noop", t.label);
            p.disabled(why);
        }
    }
    if t.on {
        p.box_(r, Color(accent.0, accent.1, accent.2, 36), 4);
    }
    let ink = if live { icon } else { FAINT };
    if let Some(sym) = t.symbol {
        p.symbol(sym, r.x + (r.width as i32 - 22) / 2, r.y + 4, 22, ink);
    }
    p.label(
        r.x - 4,
        r.y + r.height as i32 - 17,
        r.width + 8,
        t.label,
        11,
        if live { INK } else { FAINT },
        false,
        Align::Center,
    );
}
/// A drop-down list of commands, painted above everything else.
fn menu(
    p: &mut Painter,
    x: i32,
    y: i32,
    width: u32,
    items: &[(&str, Result<String, &str>)],
    max_x: i32,
) {
    let h = items.len() as u32 * 26 + 8;
    let x = x.min(max_x - width as i32 - 4).max(0);
    let r = Rect::new(x, y, width, h);
    p.drop_shadow(r, 6, 10, 50, 3);
    p.box_(r, Color::WHITE, 6);
    p.border(r, Color::TRANSPARENT, 6, LINE);
    // Clicks on the menu's own body do nothing rather than falling through.
    p.region(r, "sheet:noop", "Menu");
    for (i, (label, target)) in items.iter().enumerate() {
        let row = Rect::new(x + 4, y + 4 + i as i32 * 26, width - 8, 26);
        if label.is_empty() {
            p.hline(row.x + 4, row.y + 13, row.width - 8, LINE);
            continue;
        }
        match target {
            Ok(t) => {
                p.region(row, &format!("sheet:{t}"), label);
                p.label(
                    row.x + 10,
                    row.y + 5,
                    row.width - 16,
                    label,
                    13,
                    INK,
                    false,
                    Align::Left,
                );
            }
            Err(why) => {
                p.region(row, "sheet:noop", label);
                p.disabled(why);
                p.label(
                    row.x + 10,
                    row.y + 5,
                    row.width - 16,
                    label,
                    13,
                    FAINT,
                    false,
                    Align::Left,
                );
            }
        }
    }
}
fn live(t: &str) -> Result<String, &'static str> {
    Ok(t.into())
}

/// Menus shared by every product, keyed by the book's open menu.
fn menu_items(
    book: &Book,
    name: &str,
    flavor: Flavor,
) -> Vec<(String, Result<String, &'static str>)> {
    if let Some(items) = super::panels::menu_items(book, name, flavor) {
        return items;
    }
    let f = flavor.name();
    let s = |label: &str, t: &str| (label.to_owned(), live(t));
    let editing = book.editing.is_some();
    let can_undo = book.workbook.can_undo();
    let can_redo = book.workbook.can_redo();
    match name {
        "insert" => vec![
            s("Insert Sheet Rows", "insert:rows"),
            s("Insert Sheet Columns", "insert:cols"),
            s("Insert Sheet", "insert:sheet"),
        ],
        "delete" => vec![
            s("Delete Sheet Rows", "delete:rows"),
            s("Delete Sheet Columns", "delete:cols"),
            (
                "Delete Sheet".into(),
                if book.workbook.sheets.len() > 1 {
                    live("delete:sheet")
                } else {
                    Err("a workbook keeps at least one sheet")
                },
            ),
        ],
        "format" => vec![
            s("AutoFit Column Width", "autofit"),
            s("Rename Sheet", "rename"),
            s("Clear Formats", "clearformats"),
        ],
        "fill" => vec![s("Down", "filldown"), s("Right", "fillright")],
        "clear" => vec![
            s("Clear All", "clearall"),
            s("Clear Formats", "clearformats"),
            s("Clear Contents", "clear"),
        ],
        "sort" => vec![
            s("Sort A to Z", "sort:asc"),
            s("Sort Z to A", "sort:desc"),
            ("".into(), Err("")),
            s(
                if book.sheet_ref().filter.is_some() {
                    "Clear Filter"
                } else {
                    "Filter"
                },
                "filter",
            ),
        ],
        "numfmt" => vec![
            s("General", "fmt:general"),
            s("Number", "fmt:number"),
            s("Currency", "fmt:currency"),
            s("Accounting", "fmt:accounting"),
            s("Short Date", "fmt:date"),
            s("Long Date", "fmt:longdate"),
            s("Time", "fmt:time"),
            s("Percentage", "fmt:percent"),
            s("Scientific", "fmt:scientific"),
            s("Text", "fmt:text"),
        ],
        "fillcolor" | "fontcolor" => {
            let verb = if name == "fillcolor" { "fill" } else { "color" };
            let mut v = vec![(
                if verb == "fill" {
                    "No Fill".to_string()
                } else {
                    "Automatic".to_string()
                },
                live(&format!("{verb}:none")),
            )];
            for (label, c) in SWATCHES {
                v.push((label.to_string(), live(&format!("{verb}:{}", hex(c)))));
            }
            v
        }
        "chart" => vec![
            s("Clustered Column", "chart:column"),
            s("Clustered Bar", "chart:bar"),
            s("Line with Markers", "chart:line"),
            s("Pie", "chart:pie"),
        ],
        "charttype" => vec![
            s("Column", "charttype:column"),
            s("Bar", "charttype:bar"),
            s("Line", "charttype:line"),
            s("Pie", "charttype:pie"),
            ("".into(), Err("")),
            s("Delete Chart", "delete:chart"),
        ],
        "freeze" => vec![
            s("Freeze Panes", "freeze:panes"),
            s("Freeze Top Row", "freeze:row"),
            s("Freeze First Column", "freeze:col"),
            (
                "Unfreeze Panes".into(),
                if book.sheet_ref().freeze != (0, 0) {
                    live("freeze:none")
                } else {
                    Err("nothing is frozen")
                },
            ),
        ],
        "zoom" => ["50", "75", "100", "125", "150", "200"]
            .iter()
            .map(|z| (format!("{z}%"), live(&format!("zoom:{z}"))))
            .collect(),
        "functions" => [
            "SUM", "AVERAGE", "COUNT", "MAX", "MIN", "IF", "VLOOKUP", "XLOOKUP", "ROUND", "TODAY",
            "CONCAT", "PMT",
        ]
        .iter()
        .map(|n| (n.to_string(), live(&format!("insertfn:{n}"))))
        .collect(),
        "file" => vec![
            s("New", &format!("new:{f}")),
            s("Open…", "open"),
            s("Save", &format!("save:{f}")),
            s("Export as CSV", "savecsv"),
            s("Export as Excel Workbook", "savexlsx"),
        ],
        "edit" => vec![
            (
                "Undo".into(),
                if can_undo {
                    live("undo")
                } else {
                    Err("there is nothing to undo")
                },
            ),
            (
                "Redo".into(),
                if can_redo {
                    live("redo")
                } else {
                    Err("there is nothing to redo")
                },
            ),
            ("".into(), Err("")),
            s("Cut", "cut"),
            s("Copy", "copy"),
            s("Paste", "paste"),
            s("Select All", "all"),
            s("Delete Contents", "clear"),
        ],
        "view" => vec![
            s("Freeze First Row", "freeze:row"),
            s("Freeze First Column", "freeze:col"),
            s("Freeze Rows and Columns", "freeze:panes"),
            s("Unfreeze", "freeze:none"),
            ("".into(), Err("")),
            s("Zoom In", "zoom:in"),
            s("Zoom Out", "zoom:out"),
            s(
                if book.gridlines {
                    "Hide Grid Lines"
                } else {
                    "Show Grid Lines"
                },
                "gridlines",
            ),
        ],
        "calcinsert" => vec![
            s("Rows Above", "insert:rows"),
            s("Columns Before", "insert:cols"),
            s("Sheet at End", "insert:sheet"),
            ("".into(), Err("")),
            s("Chart…", "chart:column"),
            s("Pivot Table…", "pivot:new"),
            s("Function…", "menu:functions"),
        ],
        "calcformat" => vec![
            s("Bold", "bold"),
            s("Italic", "italic"),
            s("Underline", "underline"),
            ("".into(), Err("")),
            s("Currency", "fmt:currency"),
            s("Percent", "fmt:percent"),
            s("Date", "fmt:date"),
            s("General", "fmt:general"),
            ("".into(), Err("")),
            s("Merge and Unmerge Cells ›", "menu:merge"),
            s("Borders ›", "menu:borders"),
            s("Conditional ›", "menu:cf"),
            ("".into(), Err("")),
            s("Clear Direct Formatting", "clearformats"),
        ],
        "calcsheet" => vec![
            s("Insert Rows Above", "insert:rows"),
            s("Insert Columns Before", "insert:cols"),
            s("Delete Rows", "delete:rows"),
            s("Delete Columns", "delete:cols"),
            ("".into(), Err("")),
            s("Insert Sheet at End", "insert:sheet"),
            (
                "Delete Sheet".into(),
                if book.workbook.sheets.len() > 1 {
                    live("delete:sheet")
                } else {
                    Err("a document keeps at least one sheet")
                },
            ),
            s("Rename Sheet…", "rename"),
        ],
        "calcdata" => vec![
            s("Sort Ascending", "sort:asc"),
            s("Sort Descending", "sort:desc"),
            s("AutoFilter", "filter"),
            ("".into(), Err("")),
            s("Pivot Table ›", "menu:pivotmenu"),
            s("Text to Columns…", "ttc"),
            s("Recalculate", "calcnow"),
            ("".into(), Err("")),
            s("Fill Down", "filldown"),
            s("Fill Right", "fillright"),
        ],
        "more" => vec![
            s("Sort A → Z", "sort:asc"),
            s("Sort Z → A", "sort:desc"),
            s(
                if book.sheet_ref().filter.is_some() {
                    "Turn off filter"
                } else {
                    "Create a filter"
                },
                "filter",
            ),
            s("Freeze 1 row", "freeze:row"),
            s("Freeze 1 column", "freeze:col"),
            s("Unfreeze", "freeze:none"),
            ("".into(), Err("")),
            s("Merge cells ›", "menu:merge"),
            s("Borders ›", "menu:borders"),
            s("Conditional formatting ›", "menu:cf"),
            ("".into(), Err("")),
            s("Save", &format!("save:{f}")),
            s("Export as CSV", "savecsv"),
        ],
        "plus" => vec![
            s("Row above", "insert:rows"),
            s("Column left", "insert:cols"),
            s("Sheet", "insert:sheet"),
            s("Chart", "chart:column"),
            s("Pivot table", "pivot:new"),
            s("Function", "menu:functions"),
        ],
        "numbersformat" => vec![
            s("Merge Cells ›", "menu:merge"),
            s("Cell Borders ›", "menu:borders"),
            s("Conditional Highlighting ›", "menu:cf"),
            ("".into(), Err("")),
            s("Rename Sheet", "rename"),
            s("Clear Formats", "clearformats"),
        ],
        other => {
            if let Some(col) = other.strip_prefix("filter:") {
                if let Some(c) = cw_sheet::column_index(col) {
                    let hidden = book
                        .sheet_ref()
                        .filter
                        .as_ref()
                        .and_then(|f| f.hidden.get(&c).cloned())
                        .unwrap_or_default();
                    return book
                        .workbook
                        .filter_values(book.sheet.min(book.workbook.sheets.len() - 1), c)
                        .into_iter()
                        .map(|v| {
                            let mark = if hidden.contains(&v) { "☐" } else { "☑" };
                            let shown = if v.is_empty() {
                                "(Blanks)".to_string()
                            } else {
                                v.clone()
                            };
                            (
                                format!("{mark} {shown}"),
                                live(&format!("filtertoggle:{col}:{v}")),
                            )
                        })
                        .collect();
                }
            }
            let _ = editing;
            vec![]
        }
    }
}
fn paint_menu(p: &mut Painter, book: &Book, flavor: Flavor, x: i32, y: i32, width: u32) {
    let Some(name) = &book.menu else {
        return;
    };
    let items = menu_items(book, name, flavor);
    if items.is_empty() {
        return;
    }
    let refs: Vec<(&str, Result<String, &str>)> =
        items.iter().map(|(l, t)| (l.as_str(), t.clone())).collect();
    menu(p, x, y, width, &refs, p.scene.width as i32);
}
/// Where the open menu drops from, by name (so each menu hangs under its button).
fn menu_anchor(book: &Book, anchors: &[(&str, i32, i32)]) -> Option<(i32, i32)> {
    let name = book.menu.as_deref()?;
    // A submenu drops from where its parent menu did.
    let key = match name {
        n if n.starts_with("filter:") => "filter",
        "cfhighlight" | "cftop" | "cfbars" | "cfscales" | "cficons" | "cfclear" => "cf",
        "bordercolor" | "borderline" => "borders",
        n => n,
    };
    anchors
        .iter()
        .find(|(n, _, _)| *n == key)
        .map(|(_, x, y)| (*x, *y))
}

pub fn render(book: &Book, p: &mut Painter, env: &AppEnv<'_>) {
    let flavor = book.flavor(env.theme);
    let (w, h) = (env.width, env.height);
    p.scene.background = Color::WHITE;
    if let Some(path) = &book.loading {
        p.center(
            0,
            h as i32 / 2 - 10,
            w,
            &format!("Opening {}…", super::file_name(path)),
            14,
            MUTED,
        );
        return;
    }
    if book.browsing {
        start_screen(book, p, env, flavor);
    } else {
        match flavor {
            Flavor::Excel => excel(book, p, env),
            Flavor::Numbers if env.theme == DesktopTheme::Ios => numbers_ios(book, p, env),
            Flavor::Numbers => numbers(book, p, env),
            Flavor::Calc => calc(book, p, env),
            Flavor::Sheets => sheets(book, p, env),
        }
    }
    if let Some(m) = &book.message {
        let dw = 380.min(w.saturating_sub(24));
        let r = Rect::new((w as i32 - dw as i32) / 2, (h as i32 - 150) / 3, dw, 140);
        p.box_(Rect::new(0, 0, w, h), Color(0, 0, 0, 40), 0);
        p.region(Rect::new(0, 0, w, h), "sheet:noop", "Dialog");
        p.drop_shadow(r, 8, 16, 70, 4);
        p.box_(r, Color::WHITE, 8);
        p.strong(
            r.x + 18,
            r.y + 16,
            r.width - 36,
            flavor.product(env.theme),
            14,
            INK,
        );
        p.paragraph(r.x + 18, r.y + 42, r.width - 36, m, 12, INK);
        let ok = Rect::new(
            r.x + r.width as i32 - 96,
            r.y + r.height as i32 - 40,
            80,
            28,
        );
        p.button(
            ok,
            match flavor {
                Flavor::Excel => EXCEL_GREEN,
                Flavor::Calc => Color::rgb(233, 84, 32),
                Flavor::Sheets => Color::rgb(26, 115, 232),
                Flavor::Numbers => Color::rgb(0, 122, 255),
            },
            4,
            "sheet:dismiss",
            "OK",
        );
        p.label(
            ok.x,
            ok.y + 6,
            ok.width,
            "OK",
            13,
            Color::WHITE,
            true,
            Align::Center,
        );
    } else {
        super::panels::paint_dialog(book, p, w, h, accent_of(flavor));
    }
}
fn accent_of(flavor: Flavor) -> Color {
    match flavor {
        Flavor::Excel => EXCEL_GREEN,
        Flavor::Calc => Color::rgb(233, 84, 32),
        Flavor::Sheets => Color::rgb(26, 115, 232),
        Flavor::Numbers => Color::rgb(0, 122, 255),
    }
}
const EXCEL_REFS: [Color; 7] = [
    Color::rgb(47, 117, 181),
    Color::rgb(192, 0, 0),
    Color::rgb(112, 48, 160),
    Color::rgb(0, 176, 80),
    Color::rgb(191, 143, 0),
    Color::rgb(197, 90, 17),
    Color::rgb(0, 112, 192),
];
const CALC_REFS: [Color; 8] = [
    Color::rgb(30, 144, 255),
    Color::rgb(199, 21, 133),
    Color::rgb(50, 205, 50),
    Color::rgb(218, 165, 32),
    Color::rgb(100, 149, 237),
    Color::rgb(255, 69, 0),
    Color::rgb(0, 128, 128),
    Color::rgb(218, 112, 214),
];
const NUMBERS_REFS: [Color; 6] = [
    Color::rgb(0, 122, 255),
    Color::rgb(52, 199, 89),
    Color::rgb(255, 149, 0),
    Color::rgb(175, 82, 222),
    Color::rgb(255, 45, 85),
    Color::rgb(90, 200, 250),
];
const SHEETS_REFS: [Color; 6] = [
    Color::rgb(255, 153, 0),
    Color::rgb(66, 133, 244),
    Color::rgb(155, 81, 224),
    Color::rgb(15, 157, 88),
    Color::rgb(219, 68, 55),
    Color::rgb(0, 172, 193),
];

/// The document list: Excel's Open page, Numbers' document browser, Calc's Start
/// Center, Sheets' home. It lists the spreadsheets in the folder, which are real
/// files, and offers a blank workbook.
fn start_screen(book: &Book, p: &mut Painter, env: &AppEnv<'_>, flavor: Flavor) {
    let (w, h) = (env.width, env.height);
    let (accent, side) = match flavor {
        Flavor::Excel => (EXCEL_GREEN, Some(EXCEL_GREEN)),
        Flavor::Calc => (Color::rgb(0, 164, 90), Some(Color::rgb(246, 246, 246))),
        Flavor::Numbers => (Color::rgb(0, 122, 255), None),
        Flavor::Sheets => (Color::rgb(15, 157, 88), None),
    };
    let mobile = env.theme.mobile();
    let left = if side.is_some() && !mobile && w > 600 {
        200
    } else {
        0
    };
    if let Some(bg) = side.filter(|_| left > 0) {
        p.box_(Rect::new(0, 0, left, h), bg, 0);
        let fg = if flavor == Flavor::Excel {
            Color::WHITE
        } else {
            INK
        };
        p.strong(
            20,
            20,
            left - 30,
            if flavor == Flavor::Calc {
                "LibreOffice"
            } else {
                "Excel"
            },
            20,
            fg,
        );
        let items = [
            ("New", format!("new:{}", flavor.name())),
            ("Open", "open".to_string()),
        ];
        for (i, (label, target)) in items.iter().enumerate() {
            let r = Rect::new(8, 70 + i as i32 * 40, left - 16, 34);
            p.button(
                r,
                if i == 1 {
                    Color(255, 255, 255, 40)
                } else {
                    Color::TRANSPARENT
                },
                4,
                &format!("sheet:{target}"),
                label,
            );
            p.label(
                r.x + 14,
                r.y + 8,
                r.width - 20,
                label,
                14,
                fg,
                false,
                Align::Left,
            );
        }
        if !book.name.is_empty() {
            let r = Rect::new(8, 150, left - 16, 34);
            p.button(
                r,
                Color::TRANSPARENT,
                4,
                "sheet:closelist",
                "Back to the workbook",
            );
            p.label(
                r.x + 14,
                r.y + 8,
                r.width - 20,
                "← Back",
                14,
                fg,
                false,
                Align::Left,
            );
        }
    }
    let x = left as i32 + 24;
    let content_w = w.saturating_sub(left + 48);
    let mut y = 20;
    let title = match flavor {
        Flavor::Excel => "Open",
        Flavor::Numbers => "Choose a Template",
        Flavor::Calc => "Start Center",
        Flavor::Sheets => "Sheets",
    };
    p.strong(x, y, content_w, title, if mobile { 22 } else { 20 }, INK);
    y += 40;
    // Blank workbook card.
    let card = Rect::new(x, y, 150.min(content_w), 110);
    p.border(card, Color::WHITE, 6, LINE);
    p.button(
        card,
        Color::TRANSPARENT,
        6,
        &format!("sheet:new:{}", flavor.name()),
        "Blank workbook",
    );
    for i in 0..4 {
        p.hline(card.x + 16, card.y + 20 + i * 16, card.width - 32, LINE);
    }
    p.box_(
        Rect::new(card.x + 16, card.y + 20, 30, 64),
        Color(accent.0, accent.1, accent.2, 30),
        0,
    );
    p.label(
        card.x,
        card.y + card.height as i32 + 6,
        card.width,
        match flavor {
            Flavor::Numbers => "Blank",
            Flavor::Calc => "Calc Spreadsheet",
            Flavor::Sheets => "Blank spreadsheet",
            Flavor::Excel => "Blank workbook",
        },
        12,
        INK,
        false,
        Align::Center,
    );
    y += 150;
    p.strong(
        x,
        y,
        content_w,
        &format!("Recent in {}", super::file_name(&book.folder.path)),
        14,
        INK,
    );
    y += 28;
    if let Some(problem) = &book.folder.problem {
        p.left(x, y, content_w, problem, 13, MUTED);
        return;
    }
    let files: Vec<&String> = book.folder.entries.iter().collect();
    if files.is_empty() {
        p.left(
            x,
            y,
            content_w,
            "No spreadsheets in this folder yet",
            13,
            MUTED,
        );
    }
    for (i, name) in files.iter().enumerate() {
        let r = Rect::new(x, y + i as i32 * 36, content_w, 34);
        if r.y as u32 + 34 > h {
            break;
        }
        let (target, label) = match name.strip_suffix('/') {
            Some(dir) => (format!("sheet:folder:{dir}"), dir.to_string()),
            None => (format!("sheet:openfile:{name}"), name.to_string()),
        };
        p.button(r, Color::TRANSPARENT, 4, &target, &label);
        p.symbol(
            if name.ends_with('/') {
                "folder"
            } else {
                "document"
            },
            r.x + 6,
            r.y + 8,
            18,
            accent,
        );
        p.left(r.x + 34, r.y + 8, r.width - 40, &label, 13, INK);
        p.hline(r.x, r.y + 34, r.width, LINE);
    }
}

/// The selection's statistics, as status bars show them.
pub(super) fn stats_text(book: &Book, style: Flavor) -> String {
    let sel = book.selection();
    if sel.is_single() {
        return String::new();
    }
    let sheet = book.sheet.min(book.workbook.sheets.len() - 1);
    let st = book.workbook.stats(sheet, sel);
    // The figures take the active cell's number format, as the status bars show them
    // ($486.68 over currency); over General cells, ten significant digits at most.
    let code = book.workbook.style(sheet, book.active).format;
    let n = |x: f64| {
        if code == "General" || cw_sheet::format::is_date_format(&code) {
            cw_determinism::math::format_significant(x, 10)
        } else {
            cw_sheet::format::format(&cw_sheet::Value::Number(x), &code).text
        }
    };
    match style {
        Flavor::Calc => match st.average {
            Some(a) => format!("Average: {}; Sum: {}", n(a), n(st.sum)),
            None => "Average: ; Sum: 0".to_string(),
        },
        Flavor::Numbers => match st.average {
            Some(a) => format!(
                "SUM {}   AVG {}   MIN {}   MAX {}   COUNT {}",
                n(st.sum),
                n(a),
                n(st.min.unwrap_or(0.0)),
                n(st.max.unwrap_or(0.0)),
                st.numbers
            ),
            None => format!("COUNTA {}", st.count),
        },
        Flavor::Sheets => match st.average {
            Some(_) => format!("Sum: {}", n(st.sum)),
            None => format!("Count: {}", st.count),
        },
        Flavor::Excel => {
            if st.count < 2 {
                return String::new();
            }
            match st.average {
                Some(a) if st.numbers > 0 => format!(
                    "Average: {}    Count: {}    Sum: {}",
                    n(a),
                    st.count,
                    n(st.sum)
                ),
                _ => format!("Count: {}", st.count),
            }
        }
    }
}
/// What the formula bar shows: the editor's text, or the active cell's input.
fn bar_text(book: &Book) -> String {
    match &book.editing {
        Some(e) => e.text.clone(),
        None => book
            .workbook
            .input(book.sheet.min(book.workbook.sheets.len() - 1), book.active),
    }
}
fn name_box_text(book: &Book) -> String {
    if let Some(n) = &book.name_box {
        return n.clone();
    }
    let sel = book.selection();
    if sel.is_single() || book.drag.is_none() {
        book.active.a1()
    } else {
        format!("{}R x {}C", sel.rows(), sel.cols())
    }
}
fn formula_bar(p: &mut Painter, book: &Book, r: Rect, name_w: u32, accent: Color, calc: bool) {
    p.box_(r, Color::WHITE, 0);
    p.hline(r.x, r.y + r.height as i32 - 1, r.width, LINE);
    let nb = Rect::new(r.x + 4, r.y + 3, name_w, r.height - 6);
    p.border(
        nb,
        Color::WHITE,
        2,
        if book.name_box.is_some() {
            accent
        } else {
            LINE
        },
    );
    p.region(nb, "sheet:namebox", "Name Box");
    p.left(
        nb.x + 6,
        nb.y + (nb.height as i32 - 17) / 2,
        nb.width - 10,
        &name_box_text(book),
        12,
        INK,
    );
    let mut x = nb.x + nb.width as i32 + 6;
    let editing = book.editing.is_some();
    let icons: Vec<Tool> = if calc {
        vec![
            tool("Function Wizard", None, "menu:functions"),
            tool("Select Function", None, "autosum"),
            if editing {
                tool("Formula", None, "cancel")
            } else {
                tool("Formula", None, "edit:bar")
            },
        ]
    } else {
        vec![
            if editing {
                tool("Cancel", Some("close"), "cancel")
            } else {
                off("Cancel", Some("close"), "there is no entry to cancel")
            },
            if editing {
                tool("Enter", Some("check"), "enter")
            } else {
                off("Enter", Some("check"), "there is no entry to confirm")
            },
            tool("Insert Function", None, "menu:functions"),
        ]
    };
    for (i, t) in icons.iter().enumerate() {
        let b = Rect::new(x, r.y + 3, 26, r.height - 6);
        let label = match (calc, i) {
            (true, 0) => "fx",
            (true, 1) => "Σ",
            (true, _) => {
                if editing {
                    "×"
                } else {
                    "="
                }
            }
            (false, 2) => "fx",
            _ => t.label,
        };
        let shown = Tool {
            label,
            symbol: if calc || i == 2 { None } else { t.symbol },
            target: t.target.clone(),
            on: false,
        };
        button(p, b, &shown, accent);
        x += 28;
    }
    let input = Rect::new(
        x + 4,
        r.y + 3,
        (r.x + r.width as i32 - x - 8).max(20) as u32,
        r.height - 6,
    );
    p.border(
        input,
        Color::WHITE,
        2,
        if book.editing.as_ref().is_some_and(|e| e.in_bar) {
            accent
        } else {
            LINE
        },
    );
    p.region(input, "sheet:edit:bar", "Formula bar");
    let text = bar_text(book);
    // While a formula is typed its references show in their colours.
    let refs: &[Color] = if book.editing.is_none() {
        &[]
    } else if calc {
        &CALC_REFS
    } else {
        &EXCEL_REFS
    };
    grid::formula_text(
        p,
        input.x + 6,
        input.y + (input.height as i32 - 17) / 2,
        input.width - 10,
        &text,
        12,
        INK,
        refs,
    );
    if let Some(e) = &book.editing {
        if e.in_bar {
            let cx = input.x + 6 + p.measure(&e.text[..e.caret], 12, false) as i32;
            p.vline(cx, input.y + 4, input.height - 8, INK);
        }
    }
}
fn sheet_tabs(
    p: &mut Painter,
    book: &Book,
    r: Rect,
    accent: Color,
    flavor: Flavor,
    plus_first: bool,
) -> i32 {
    p.box_(r, Color::rgb(243, 243, 243), 0);
    p.hline(r.x, r.y, r.width, LINE);
    let mut x = r.x + 6;
    let plus = |p: &mut Painter, x: i32| {
        let b = Rect::new(x, r.y + 3, 24, r.height - 6);
        button(
            p,
            b,
            &tool("New sheet", Some("plus"), "insert:sheet"),
            accent,
        );
    };
    if plus_first {
        plus(p, x);
        x += 30;
    }
    let active = book.sheet.min(book.workbook.sheets.len() - 1);
    for (i, s) in book.workbook.sheets.iter().enumerate() {
        let name = match &book.renaming {
            Some((ri, n)) if *ri == i => format!("{n}|"),
            _ => s.name.clone(),
        };
        let tw = p.measure(&name, 12, i == active) + 24;
        let t = Rect::new(x, r.y, tw, r.height - 1);
        if x > r.x + r.width as i32 - 40 {
            break;
        }
        if i == active {
            p.box_(t, Color::WHITE, 0);
            match flavor {
                Flavor::Excel => p.box_(
                    Rect::new(t.x + 4, t.y + t.height as i32 - 3, t.width - 8, 3),
                    accent,
                    1,
                ),
                _ => p.box_(Rect::new(t.x, t.y, t.width, 2), accent, 0),
            }
        }
        p.region(t, &format!("sheet:tab:{i}"), &s.name);
        p.label(
            t.x,
            t.y + (t.height as i32 - 17) / 2,
            t.width,
            &name,
            12,
            if i == active {
                if flavor == Flavor::Excel {
                    accent
                } else {
                    INK
                }
            } else {
                MUTED
            },
            i == active,
            Align::Center,
        );
        x += tw as i32 + 2;
    }
    if !plus_first {
        plus(p, x + 4);
        x += 34;
    }
    x
}
fn scrollbars(p: &mut Painter, book: &Book, cells: Rect) {
    // Vertical scroll buttons on the right edge of the grid, horizontal at the bottom.
    let v = Rect::new(cells.x + cells.width as i32 - 16, cells.y, 16, cells.height);
    p.box_(v, Color::rgb(248, 248, 248), 0);
    button(
        p,
        Rect::new(v.x, v.y, 16, 18),
        &tool("Scroll up", Some("chevron-up"), "scroll:up"),
        INK,
    );
    let track = v.height.saturating_sub(36);
    p.region(
        Rect::new(v.x, v.y + 18, 16, track / 2),
        "sheet:scroll:pageup",
        "Page up",
    );
    p.region(
        Rect::new(v.x, v.y + 18 + (track / 2) as i32, 16, track / 2),
        "sheet:scroll:pagedown",
        "Page down",
    );
    // The thumb shows where the first row shown sits in the rows in use.
    let used = book
        .workbook
        .used_range(book.sheet.min(book.workbook.sheets.len() - 1))
        .map_or(0, |u| u.end.row + 1);
    let extent = (used.max(book.scroll.0 + 40)) as i64;
    let thumb_h = ((i64::from(track) * 40 / extent.max(1)) as u32).clamp(20, track.max(20));
    let thumb_y = v.y
        + 18
        + (i64::from(track.saturating_sub(thumb_h)) * i64::from(book.scroll.0)
            / (extent - 40).max(1))
        .min(i64::from(track.saturating_sub(thumb_h))) as i32;
    p.box_(
        Rect::new(v.x + 4, thumb_y, 8, thumb_h),
        Color::rgb(200, 200, 200),
        4,
    );
    button(
        p,
        Rect::new(v.x, v.y + v.height as i32 - 18, 16, 18),
        &tool("Scroll down", Some("chevron-down"), "scroll:down"),
        INK,
    );
}
fn palette(flavor: Flavor, geom_head: (u32, u32)) -> Palette {
    match flavor {
        Flavor::Excel => Palette {
            header_bg: Color::rgb(245, 245, 245),
            header_text: Color::rgb(68, 68, 68),
            header_sel_bg: Color::rgb(210, 210, 210),
            header_sel_text: Color::rgb(16, 124, 65),
            header_line: Color::rgb(213, 213, 213),
            grid_line: Color::rgb(225, 225, 225),
            sel_fill: Color(0, 0, 0, 22),
            sel_border: Color::rgb(16, 124, 65),
            text: Color::BLACK,
            font: 13,
            header_font: 12,
            head_w: geom_head.0,
            head_h: geom_head.1,
            headers: true,
            chart_colors: &EXCEL_CHARTS,
            ref_colors: &EXCEL_REFS,
        },
        Flavor::Calc => Palette {
            header_bg: Color::rgb(240, 240, 240),
            header_text: Color::rgb(50, 50, 50),
            header_sel_bg: Color::rgb(254, 216, 196),
            header_sel_text: INK,
            header_line: Color::rgb(200, 200, 200),
            grid_line: Color::rgb(210, 210, 210),
            sel_fill: Color(233, 84, 32, 30),
            sel_border: Color::rgb(0, 0, 0),
            text: Color::BLACK,
            font: 12,
            header_font: 11,
            head_w: geom_head.0,
            head_h: geom_head.1,
            headers: true,
            chart_colors: &CALC_CHARTS,
            ref_colors: &CALC_REFS,
        },
        Flavor::Numbers => Palette {
            header_bg: Color::rgb(241, 241, 241),
            header_text: Color::rgb(120, 120, 120),
            header_sel_bg: Color::rgb(218, 234, 252),
            header_sel_text: Color::rgb(0, 122, 255),
            header_line: Color::rgb(226, 226, 226),
            grid_line: Color::rgb(222, 222, 222),
            sel_fill: Color(0, 122, 255, 30),
            sel_border: Color::rgb(0, 122, 255),
            text: INK,
            font: 13,
            header_font: 11,
            head_w: geom_head.0,
            head_h: geom_head.1,
            headers: true,
            chart_colors: &NUMBERS_CHARTS,
            ref_colors: &NUMBERS_REFS,
        },
        Flavor::Sheets => Palette {
            header_bg: Color::rgb(248, 249, 250),
            header_text: Color::rgb(95, 99, 104),
            header_sel_bg: Color::rgb(211, 227, 253),
            header_sel_text: Color::rgb(11, 87, 208),
            header_line: Color::rgb(218, 220, 224),
            grid_line: Color::rgb(226, 227, 227),
            sel_fill: Color(26, 115, 232, 30),
            sel_border: Color::rgb(26, 115, 232),
            text: Color::rgb(32, 33, 36),
            font: 13,
            header_font: 11,
            head_w: geom_head.0,
            head_h: geom_head.1,
            headers: true,
            chart_colors: &SHEETS_CHARTS,
            ref_colors: &SHEETS_REFS,
        },
    }
}
/// Whether the pivot table field list shows: the active cell is in a pivot table and
/// the list has not been closed.
fn show_pane(book: &Book) -> bool {
    !book.pivot_pane_closed && book.current_pivot().is_some()
}
/// The contextual command strip Excel shows over the ribbon for a selected chart or
/// pivot table.
fn contextual(p: &mut Painter, r: Rect, label: &str, target: &str, accent: Color) {
    p.button(
        r,
        Color(accent.0, accent.1, accent.2, 30),
        3,
        &format!("sheet:{target}"),
        label,
    );
    p.label(
        r.x,
        r.y + 2,
        r.width,
        label,
        11,
        accent,
        true,
        Align::Center,
    );
}
fn mode_text(book: &Book) -> &'static str {
    match book.draw {
        Some(super::DrawMode::Border) => return "Draw Border",
        Some(super::DrawMode::Grid) => return "Draw Border Grid",
        Some(super::DrawMode::Erase) => return "Erase Border",
        None => {}
    }
    match (&book.editing, &book.drag) {
        (Some(_), Some(super::Drag::Point { .. })) => "Point",
        (Some(e), _) if e.fresh => "Enter",
        (Some(_), _) => "Edit",
        _ => "Ready",
    }
}

// ----- Microsoft Excel -----

fn excel(book: &Book, p: &mut Painter, env: &AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let accent = EXCEL_GREEN;
    let f = Flavor::Excel;
    if book.ribbon == "file" {
        excel_backstage(book, p, env);
        return;
    }
    // Ribbon tabs.
    p.box_(Rect::new(0, 0, w, 30), Color::rgb(243, 243, 243), 0);
    let tabs: [(&str, &str, Option<&str>); 9] = [
        ("File", "file", None),
        ("Home", "home", None),
        ("Insert", "insert", None),
        (
            "Page Layout",
            "pagelayout",
            Some("page layout and printing are not modeled"),
        ),
        ("Formulas", "formulas", None),
        ("Data", "data", None),
        (
            "Review",
            "review",
            Some("comments and review are not modeled"),
        ),
        ("View", "view", None),
        (
            "Help",
            "help",
            Some("help content is not available offline"),
        ),
    ];
    let mut x = 8;
    let mut anchors: Vec<(&str, i32, i32)> = Vec::new();
    for (label, id, why) in tabs {
        let tw = p.measure(label, 12, false) + 20;
        let r = Rect::new(x, 2, tw, 28);
        let on = book.ribbon == id;
        match why {
            None => p.region(r, &format!("sheet:ribbon:{id}"), label),
            Some(why) => {
                p.region(r, "sheet:noop", label);
                p.disabled(why);
            }
        }
        p.label(
            r.x,
            r.y + 6,
            r.width,
            label,
            12,
            if why.is_some() {
                FAINT
            } else if on {
                accent
            } else {
                INK
            },
            on,
            Align::Center,
        );
        if on {
            p.box_(Rect::new(r.x + 8, r.y + 25, r.width - 16, 3), accent, 1);
        }
        x += tw as i32;
    }
    // Ribbon body.
    let rb = Rect::new(0, 30, w, 88);
    p.box_(rb, Color::rgb(249, 249, 249), 0);
    p.hline(0, rb.y + rb.height as i32 - 1, w, LINE);
    let mut gx = 6;
    let group = |p: &mut Painter, gx: &mut i32, label: &str, tools: Vec<(Tool, bool)>| {
        // (tool, tall?)
        let start = *gx;
        let mut col_x = *gx;
        let mut small_row = 0;
        for (t, is_tall) in &tools {
            if *is_tall {
                if small_row > 0 {
                    col_x += 28;
                    small_row = 0;
                }
                // Wide enough for its caption, as the ribbon sizes its big buttons.
                let tw = (p.measure(t.label, 11, false) + 10).max(52);
                let r = Rect::new(col_x, rb.y + 4, tw, 60);
                tall(p, r, t, accent, accent);
                col_x += tw as i32 + 2;
            } else {
                let r = Rect::new(col_x, rb.y + 6 + small_row * 20, 26, 20);
                button(p, r, t, accent);
                small_row += 1;
                if small_row == 3 {
                    small_row = 0;
                    col_x += 28;
                }
            }
        }
        if small_row > 0 {
            col_x += 28;
        }
        // A group is never narrower than its name.
        col_x = col_x.max(start + p.measure(label, 10, false) as i32 + 8);
        let gw = (col_x - start).max(40) as u32;
        p.label(start, rb.y + 68, gw, label, 10, MUTED, false, Align::Center);
        p.vline(col_x + 3, rb.y + 6, 74, LINE);
        *gx = col_x + 8;
    };
    let style = book
        .workbook
        .style(book.sheet.min(book.workbook.sheets.len() - 1), book.active);
    let can_undo = book.workbook.can_undo();
    let can_redo = book.workbook.can_redo();
    let undo = if can_undo {
        tool("Undo", Some("undo"), "undo")
    } else {
        off("Undo", Some("undo"), "there is nothing to undo")
    };
    let redo = if can_redo {
        tool("Redo", Some("redo"), "redo")
    } else {
        off("Redo", Some("redo"), "there is nothing to redo")
    };
    match book.ribbon.as_str() {
        "insert" => {
            let charts = [
                ("Column", "chart:column"),
                ("Bar", "chart:bar"),
                ("Line", "chart:line"),
                ("Pie", "chart:pie"),
            ];
            let start = gx;
            for (i, (label, t)) in charts.iter().enumerate() {
                let r = Rect::new(gx, rb.y + 4, 52, 60);
                p.region(r, &format!("sheet:{t}"), &format!("Insert {label} Chart"));
                mini_chart(p, Rect::new(r.x + 14, r.y + 6, 24, 22), i, accent);
                p.label(
                    r.x - 4,
                    r.y + 43,
                    r.width + 8,
                    label,
                    11,
                    INK,
                    false,
                    Align::Center,
                );
                gx += 54;
            }
            p.label(
                start,
                rb.y + 68,
                (gx - start) as u32,
                "Charts",
                10,
                MUTED,
                false,
                Align::Center,
            );
            p.vline(gx + 3, rb.y + 6, 74, LINE);
            gx += 8;
            group(
                p,
                &mut gx,
                "Tables",
                vec![
                    (tool("PivotTable", Some("grid"), "pivot:new"), true),
                    (
                        off("Table", Some("list-view"), "Excel tables are not modeled"),
                        true,
                    ),
                ],
            );
            group(
                p,
                &mut gx,
                "Illustrations",
                vec![
                    (
                        off(
                            "Pictures",
                            Some("image"),
                            "pictures in cells are not modeled",
                        ),
                        true,
                    ),
                    (
                        off("Shapes", Some("shapes"), "shapes are not modeled"),
                        true,
                    ),
                ],
            );
            anchors.push(("charttype", 6, rb.y + rb.height as i32));
        }
        "formulas" => {
            group(
                p,
                &mut gx,
                "Function Library",
                vec![
                    (
                        tool("Insert Function", Some("search"), "menu:functions"),
                        true,
                    ),
                    (tool("AutoSum", None, "autosum"), true),
                ],
            );
            anchors.push(("functions", 6, rb.y + rb.height as i32));
            group(
                p,
                &mut gx,
                "Defined Names",
                vec![(tool("Define Name", Some("tag"), "namebox"), true)],
            );
            group(
                p,
                &mut gx,
                "Calculation",
                vec![(tool("Calculate Now", Some("reload"), "calcnow"), true)],
            );
        }
        "data" => {
            let pivots = book.workbook.sheets.iter().any(|s| !s.pivots.is_empty());
            group(
                p,
                &mut gx,
                "Queries & Connections",
                vec![(
                    if pivots {
                        tool("Refresh All", Some("reload"), "pivot:refreshall")
                    } else {
                        off(
                            "Refresh All",
                            Some("reload"),
                            "there is nothing to refresh: the workbook has no PivotTables",
                        )
                    },
                    true,
                )],
            );
            group(
                p,
                &mut gx,
                "Sort & Filter",
                vec![
                    (tool("Sort A to Z", Some("sort"), "sort:asc"), true),
                    (tool("Sort Z to A", Some("sort"), "sort:desc"), true),
                    (
                        Tool {
                            on: book.sheet_ref().filter.is_some(),
                            ..tool("Filter", Some("filters"), "filter")
                        },
                        true,
                    ),
                ],
            );
            group(
                p,
                &mut gx,
                "Data Tools",
                vec![
                    (tool("Text to Columns", Some("split"), "ttc"), true),
                    (
                        off(
                            "Data Validation",
                            Some("check"),
                            "data validation is not modeled",
                        ),
                        true,
                    ),
                ],
            );
        }
        "view" => {
            group(
                p,
                &mut gx,
                "Window",
                vec![(tool("Freeze Panes", Some("grid"), "menu:freeze"), true)],
            );
            anchors.push(("freeze", 6, rb.y + rb.height as i32));
            group(
                p,
                &mut gx,
                "Zoom",
                vec![
                    (tool("Zoom In", Some("zoom-in"), "zoom:in"), true),
                    (tool("Zoom Out", Some("zoom-out"), "zoom:out"), true),
                    (tool("100%", None, "zoom:reset"), true),
                ],
            );
            group(
                p,
                &mut gx,
                "Show",
                vec![(
                    Tool {
                        on: book.gridlines,
                        ..tool("Gridlines", Some("grid-view"), "gridlines")
                    },
                    true,
                )],
            );
        }
        _ => {
            group(p, &mut gx, "Undo", vec![(undo, false), (redo, false)]);
            group(
                p,
                &mut gx,
                "Clipboard",
                vec![
                    (tool("Paste", Some("paste"), "paste"), true),
                    (tool("Cut", Some("scissors"), "cut"), false),
                    (tool("Copy", Some("copy"), "copy"), false),
                ],
            );
            let font_x = gx;
            let font_box = Rect::new(gx, rb.y + 6, 110, 20);
            p.border(font_box, Color::WHITE, 2, LINE);
            p.region(font_box, "sheet:noop", "Font");
            p.disabled("only the default font, Aptos Narrow, is available");
            p.left(
                font_box.x + 5,
                font_box.y + 2,
                100,
                "Aptos Narrow",
                11,
                FAINT,
            );
            let size_box = Rect::new(gx + 114, rb.y + 6, 36, 20);
            p.border(size_box, Color::WHITE, 2, LINE);
            p.region(size_box, "sheet:noop", "Font Size");
            p.disabled("font sizes are not modeled");
            p.left(size_box.x + 5, size_box.y + 2, 30, "11", 11, FAINT);
            let row2 = [
                Tool {
                    on: style.bold,
                    ..tool("B", None, "bold")
                },
                Tool {
                    on: style.italic,
                    ..tool("I", None, "italic")
                },
                Tool {
                    on: style.underline,
                    ..tool("U", None, "underline")
                },
                Tool {
                    on: book.draw.is_some(),
                    ..tool("Borders", Some("grid"), "menu:borders")
                },
                tool("Fill Color", Some("bucket"), "menu:fillcolor"),
                tool("Font Color", Some("text-tool"), "menu:fontcolor"),
            ];
            for (i, t) in row2.iter().enumerate() {
                button(
                    p,
                    Rect::new(gx + i as i32 * 28, rb.y + 32, 26, 22),
                    t,
                    accent,
                );
            }
            anchors.push(("borders", gx + 84, rb.y + 56));
            anchors.push(("fillcolor", gx + 112, rb.y + 56));
            anchors.push(("fontcolor", gx + 140, rb.y + 56));
            gx += 184;
            p.label(
                font_x,
                rb.y + 68,
                (gx - font_x) as u32,
                "Font",
                10,
                MUTED,
                false,
                Align::Center,
            );
            p.vline(gx - 2, rb.y + 6, 74, LINE);
            gx += 6;
            let align = |a: cw_sheet::Align| style.align == a;
            group(
                p,
                &mut gx,
                "Alignment",
                vec![
                    (
                        Tool {
                            on: align(cw_sheet::Align::Left),
                            ..tool("Align Left", Some("list-view"), "align:left")
                        },
                        false,
                    ),
                    (
                        Tool {
                            on: align(cw_sheet::Align::Center),
                            ..tool("Center", Some("menu"), "align:center")
                        },
                        false,
                    ),
                    (
                        Tool {
                            on: align(cw_sheet::Align::Right),
                            ..tool("Align Right", Some("list-view"), "align:right")
                        },
                        false,
                    ),
                    (
                        Tool {
                            on: book.sheet_ref().merges.contains(&book.selection()),
                            ..tool("Merge & Center", Some("split"), "merge:center")
                        },
                        false,
                    ),
                    (
                        tool("Merge options", Some("chevron-down"), "menu:merge"),
                        false,
                    ),
                ],
            );
            anchors.push(("merge", gx - 70, rb.y + 50));
            // Number group: format box, then $ % , and decimals.
            let num_x = gx;
            let fmt_box = Rect::new(gx, rb.y + 6, 110, 20);
            p.border(fmt_box, Color::WHITE, 2, LINE);
            p.region(fmt_box, "sheet:menu:numfmt", "Number Format");
            p.left(
                fmt_box.x + 5,
                fmt_box.y + 2,
                90,
                &format_name(&style.format),
                11,
                INK,
            );
            p.symbol("chevron-down", fmt_box.x + 94, fmt_box.y + 4, 12, INK);
            anchors.push(("numfmt", gx, rb.y + 28));
            let nums = [
                tool("$", None, "fmt:currency"),
                tool("%", None, "fmt:percent"),
                tool(",", None, "fmt:comma"),
                tool(".0+", None, "dec:more"),
                tool(".0-", None, "dec:less"),
            ];
            for (i, t) in nums.iter().enumerate() {
                button(
                    p,
                    Rect::new(gx + i as i32 * 23, rb.y + 32, 22, 22),
                    t,
                    accent,
                );
            }
            gx += 118;
            p.label(
                num_x,
                rb.y + 68,
                (gx - num_x) as u32,
                "Number",
                10,
                MUTED,
                false,
                Align::Center,
            );
            p.vline(gx - 2, rb.y + 6, 74, LINE);
            gx += 6;
            let styles_x = gx;
            group(
                p,
                &mut gx,
                "Styles",
                vec![(
                    tool("Conditional Formatting", Some("palette"), "menu:cf"),
                    true,
                )],
            );
            anchors.push(("cf", styles_x, rb.y + rb.height as i32));
            let cells_x = gx;
            group(
                p,
                &mut gx,
                "Cells",
                vec![
                    (tool("Insert", Some("plus"), "menu:insert"), false),
                    (tool("Delete", Some("minus"), "menu:delete"), false),
                    (tool("Format", Some("sliders"), "menu:format"), false),
                ],
            );
            anchors.push(("insert", cells_x, rb.y + 26));
            anchors.push(("delete", cells_x, rb.y + 46));
            anchors.push(("format", cells_x, rb.y + 66));
            let edit_x = gx;
            group(
                p,
                &mut gx,
                "Editing",
                vec![
                    (tool("Σ", None, "autosum"), false),
                    (tool("Fill", Some("arrow-up"), "menu:fill"), false),
                    (tool("Clear", Some("eraser"), "menu:clear"), false),
                    (tool("Sort & Filter", Some("sort"), "menu:sort"), true),
                ],
            );
            anchors.push(("fill", edit_x, rb.y + 46));
            anchors.push(("clear", edit_x, rb.y + 66));
            anchors.push(("sort", edit_x + 28, rb.y + 66));
        }
    }
    let _ = gx;
    // Formula bar.
    let fb = Rect::new(0, 118, w, 28);
    formula_bar(p, book, fb, 90, accent, false);
    anchors.push(("functions", 100, fb.y + fb.height as i32));
    // Grid, sheet tabs and status bar.
    let status_h = 22;
    let tabs_h = 28;
    let grid_rect = Rect::new(0, 146, w, h.saturating_sub(146 + tabs_h + status_h));
    let geom = Geom {
        row_h: 20 * book.zoom / 100,
        scale: book.zoom,
    };
    let pal = palette(Flavor::Excel, (40, 20));
    let pane_w = if show_pane(book) { 290.min(w / 3) } else { 0 };
    let cells = grid::paint(
        p,
        book,
        Rect::new(
            0,
            grid_rect.y,
            grid_rect.width.saturating_sub(16 + pane_w),
            grid_rect.height,
        ),
        geom,
        &pal,
    );
    scrollbars(p, book, Rect::new(0, cells.y, w - pane_w, cells.height));
    let pane_anchors = if pane_w > 0 {
        super::panels::pivot_pane(
            book,
            p,
            Rect::new((w - pane_w) as i32, grid_rect.y, pane_w, grid_rect.height),
            f,
            accent,
        )
    } else {
        vec![]
    };
    anchors.extend(pane_anchors.iter().map(|(n, x, y)| (n.as_str(), *x, *y)));
    let tabs_y = grid_rect.y + grid_rect.height as i32;
    sheet_tabs(p, book, Rect::new(0, tabs_y, w, tabs_h), accent, f, false);
    // Horizontal scroll arrows at the right of the tab strip.
    button(
        p,
        Rect::new(w as i32 - 40, tabs_y + 4, 18, 20),
        &tool("Scroll left", Some("chevron-left"), "scroll:left"),
        INK,
    );
    button(
        p,
        Rect::new(w as i32 - 20, tabs_y + 4, 18, 20),
        &tool("Scroll right", Some("chevron-right"), "scroll:right"),
        INK,
    );
    let sy = tabs_y + tabs_h as i32;
    p.box_(Rect::new(0, sy, w, status_h), Color::rgb(243, 243, 243), 0);
    p.left(8, sy + 3, 100, mode_text(book), 11, MUTED);
    let stats = stats_text(book, f);
    let zoom_w = 150;
    p.right(0, sy + 3, w.saturating_sub(zoom_w + 16), &stats, 11, INK);
    zoom_control(
        p,
        Rect::new(w as i32 - zoom_w as i32 - 6, sy, zoom_w, status_h),
        book,
        accent,
    );
    if book.chart.is_some() && book.menu.is_none() {
        // A selected chart gets its own contextual commands, as the Chart Design tab.
        let r = Rect::new(w as i32 - 250, 124, 240, 20);
        let t = tool("Change Chart Type", None, "menu:charttype");
        p.button(
            r,
            Color(accent.0, accent.1, accent.2, 30),
            3,
            "sheet:menu:charttype",
            t.label,
        );
        p.label(
            r.x,
            r.y + 2,
            r.width,
            "Chart Design: Change Chart Type ▾",
            11,
            accent,
            true,
            Align::Center,
        );
    }
    if book.chart.is_none() && book.menu.is_none() && book.current_pivot().is_some() {
        let r = Rect::new(w as i32 - 250, 124, 240, 20);
        if book.pivot_pane_closed {
            contextual(p, r, "PivotTable Analyze: Field List", "pivot:pane", accent);
        } else {
            contextual(p, r, "PivotTable Analyze: Refresh", "pivot:refresh", accent);
        }
    }
    anchors.push(("charttype", w as i32 - 250, 146));
    anchors.push(("filter", 60, 170));
    if let Some((mx, my)) = menu_anchor(book, &anchors) {
        paint_menu(p, book, f, mx, my, 210);
    } else if book.menu.is_some() {
        paint_menu(p, book, f, 60, 150, 210);
    }
}
fn zoom_control(p: &mut Painter, r: Rect, book: &Book, accent: Color) {
    button(
        p,
        Rect::new(r.x, r.y + 2, 18, 18),
        &tool("Zoom Out", Some("minus"), "zoom:out"),
        INK,
    );
    let track = Rect::new(r.x + 22, r.y + r.height as i32 / 2, 70, 2);
    p.box_(track, Color::rgb(150, 150, 150), 1);
    let pos = ((book.zoom.clamp(10, 400) as i32 - 10) * 70 / 390).clamp(0, 70);
    p.box_(Rect::new(track.x + pos - 2, track.y - 5, 4, 12), INK, 1);
    button(
        p,
        Rect::new(r.x + 96, r.y + 2, 18, 18),
        &tool("Zoom In", Some("plus"), "zoom:in"),
        INK,
    );
    p.region(
        Rect::new(r.x + 116, r.y, 36, r.height),
        "sheet:zoom:reset",
        "Zoom 100%",
    );
    p.left(
        r.x + 118,
        r.y + 3,
        34,
        &format!("{}%", book.zoom),
        11,
        if book.zoom == 100 { MUTED } else { accent },
    );
}
fn mini_chart(p: &mut Painter, r: Rect, kind: usize, color: Color) {
    match kind {
        0 => {
            for (i, hgt) in [10, 18, 14].iter().enumerate() {
                p.box_(
                    Rect::new(
                        r.x + i as i32 * 8,
                        r.y + r.height as i32 - hgt,
                        6,
                        *hgt as u32,
                    ),
                    color,
                    0,
                );
            }
        }
        1 => {
            for (i, len) in [12, 22, 16].iter().enumerate() {
                p.box_(Rect::new(r.x, r.y + i as i32 * 8, *len, 6), color, 0);
            }
        }
        2 => p.line(
            vec![
                (r.x, r.y + 18),
                (r.x + 8, r.y + 8),
                (r.x + 16, r.y + 13),
                (r.x + 24, r.y + 2),
            ],
            color,
            2,
        ),
        _ => {
            p.circle(r.x + 12, r.y + 11, 11, color);
            p.path(
                vec![(r.x + 12, r.y + 11), (r.x + 12, r.y), (r.x + 23, r.y + 11)],
                Color::WHITE,
            );
        }
    }
}
/// The Number Format box's name for a format code.
fn format_name(code: &str) -> String {
    match code {
        "General" => "General".into(),
        "#,##0.00" | "0.00" | "#,##0" | "0" => "Number".into(),
        c if c.starts_with('$') || c.contains("\"$\"") || c.starts_with("_($") => {
            if c.starts_with('_') {
                "Accounting".into()
            } else {
                "Currency".into()
            }
        }
        c if c.ends_with('%') => "Percentage".into(),
        "@" => "Text".into(),
        c if c.contains("E+") => "Scientific".into(),
        c if cw_sheet::format::is_date_format(c) => {
            if c.contains('h') {
                "Time".into()
            } else if c.contains("mmmm") || c.contains("dddd") {
                "Long Date".into()
            } else {
                "Short Date".into()
            }
        }
        _ => "Custom".into(),
    }
}
fn excel_backstage(book: &Book, p: &mut Painter, env: &AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    p.box_(Rect::new(0, 0, 180, h), EXCEL_GREEN, 0);
    button(
        p,
        Rect::new(8, 8, 32, 32),
        &tool("Back", Some("arrow-left"), "ribbon:home"),
        Color::WHITE,
    );
    p.symbol("arrow-left", 16, 16, 16, Color::WHITE);
    let items: [(&str, String); 5] = [
        ("New", "new:excel".into()),
        ("Open", "open".into()),
        ("Save", "save:excel".into()),
        ("Save a Copy (CSV)", "savecsv".into()),
        ("Close", "ribbon:home".into()),
    ];
    for (i, (label, target)) in items.iter().enumerate() {
        let r = Rect::new(0, 60 + i as i32 * 40, 180, 38);
        p.button(r, Color::TRANSPARENT, 0, &format!("sheet:{target}"), label);
        p.label(
            24,
            r.y + 10,
            150,
            label,
            14,
            Color::WHITE,
            false,
            Align::Left,
        );
    }
    p.strong(210, 30, w.saturating_sub(230), "Info", 24, INK);
    let path = book.path.clone().unwrap_or_else(|| "Not saved yet".into());
    p.left(210, 80, w.saturating_sub(230), &book.name, 16, INK);
    p.left(210, 104, w.saturating_sub(230), &path, 12, MUTED);
    p.left(
        210,
        140,
        w.saturating_sub(230),
        &format!(
            "{} sheet(s) · {}",
            book.workbook.sheets.len(),
            if book.modified {
                "unsaved changes"
            } else {
                "all changes saved"
            }
        ),
        12,
        MUTED,
    );
}

// ----- Numbers (macOS) -----

fn numbers(book: &Book, p: &mut Painter, env: &AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let accent = Color::rgb(0, 122, 255);
    let f = Flavor::Numbers;
    p.scene.background = Color::WHITE;
    // Toolbar: labelled icons, as Numbers draws them.
    let tb = Rect::new(0, 0, w, 54);
    p.box_(tb, Color::rgb(246, 246, 246), 0);
    p.hline(0, 53, w, LINE);
    let tools: Vec<Tool> = vec![
        off(
            "View",
            Some("sidebar"),
            "Numbers' view options are not modeled",
        ),
        tool("Zoom", Some("zoom-in"), "menu:zoom"),
        off(
            "Add Category",
            Some("list-view"),
            "categories are not modeled",
        ),
        tool("Pivot Table", Some("grid"), "pivot:new"),
        tool("Insert", Some("plus"), "menu:functions"),
        tool("Table", Some("grid-view"), "insert:sheet"),
        tool("Chart", Some("layers"), "menu:chart"),
        off("Text", Some("text-tool"), "text boxes are not modeled"),
        off("Shape", Some("shapes"), "shapes are not modeled"),
        off("Media", Some("image"), "media is not modeled"),
        off("Comment", Some("chat"), "comments are not modeled"),
    ];
    let pane_anchors: Vec<(String, i32, i32)>;
    let mut anchors: Vec<(&str, i32, i32)> = Vec::new();
    let mut x = 10;
    for t in &tools {
        let tw = (p.measure(t.label, 11, false) + 12).max(58);
        let r = Rect::new(x, 4, tw, 46);
        tall(p, r, t, accent, MUTED);
        match t.label {
            "Zoom" => anchors.push(("zoom", x, 54)),
            "Insert" => anchors.push(("functions", x, 54)),
            "Chart" => anchors.push(("chart", x, 54)),
            _ => {}
        }
        x += tw as i32 + 2;
        if x as u32 + 200 > w {
            break;
        }
    }
    let right = [
        tool("Format", Some("brush"), "inspector:format"),
        tool("Organize", Some("sort"), "inspector:organize"),
    ];
    for (i, t) in right.iter().enumerate() {
        let r = Rect::new(w as i32 - 128 + i as i32 * 62, 4, 58, 46);
        let on = book.inspector && book.ribbon == if i == 0 { "format" } else { "organize" };
        tall(
            p,
            r,
            &Tool {
                on,
                ..Tool {
                    label: t.label,
                    symbol: t.symbol,
                    target: t.target.clone(),
                    on: false,
                }
            },
            accent,
            MUTED,
        );
    }
    // Sheet tabs under the toolbar, with + first.
    sheet_tabs(p, book, Rect::new(0, 54, w, 30), accent, f, true);
    let side = if show_pane(book) {
        290.min(w / 3)
    } else if book.inspector {
        250.min(w / 3)
    } else {
        0
    };
    let canvas = Rect::new(0, 84, w.saturating_sub(side), h.saturating_sub(84 + 24));
    p.box_(canvas, Color::WHITE, 0);
    // The table sits on the canvas with its title above it.
    let title_y = canvas.y + 14;
    p.strong(canvas.x + 40, title_y, 300, "Table 1", 17, INK);
    let geom = Geom {
        row_h: 22 * book.zoom / 100,
        scale: book.zoom * 5 / 4,
    };
    let pal = palette(f, (34, 20));
    let area = Rect::new(
        canvas.x + 20,
        title_y + 30,
        canvas.width.saturating_sub(40),
        canvas.height.saturating_sub(60),
    );
    grid::paint(p, book, area, geom, &pal);
    anchors.push(("filter", area.x + 40, area.y + 40));
    // Format inspector (the pivot table options take its place in a pivot table).
    if side > 0 && !show_pane(book) {
        let s = Rect::new(w as i32 - side as i32, 84, side, h.saturating_sub(84 + 24));
        p.box_(s, Color::rgb(246, 246, 246), 0);
        p.vline(s.x, s.y, s.height, LINE);
        let style = book
            .workbook
            .style(book.sheet.min(book.workbook.sheets.len() - 1), book.active);
        let mut y = s.y + 12;
        if book.ribbon == "organize" {
            p.strong(s.x + 14, y, side - 28, "Sort", 13, INK);
            y += 26;
            for (label, t) in [("Ascending", "sort:asc"), ("Descending", "sort:desc")] {
                let r = Rect::new(s.x + 14, y, side - 28, 26);
                crate::apps::look::action(
                    p,
                    &crate::apps::look::look(DesktopTheme::Macos),
                    r,
                    label,
                    &format!("sheet:{t}"),
                    false,
                );
                y += 32;
            }
            y += 8;
            p.strong(s.x + 14, y, side - 28, "Filter", 13, INK);
            y += 26;
            let r = Rect::new(s.x + 14, y, side - 28, 26);
            crate::apps::look::action(
                p,
                &crate::apps::look::look(DesktopTheme::Macos),
                r,
                if book.sheet_ref().filter.is_some() {
                    "Remove Filter"
                } else {
                    "Add a Filter"
                },
                "sheet:filter",
                false,
            );
        } else {
            p.strong(s.x + 14, y, side - 28, "Data Format", 13, INK);
            y += 24;
            for (label, t) in [
                ("Automatic", "fmt:general"),
                ("Number", "fmt:number"),
                ("Currency", "fmt:currency"),
                ("Percentage", "fmt:percent"),
                ("Date & Time", "fmt:date"),
                ("Text", "fmt:text"),
            ] {
                let r = Rect::new(s.x + 14, y, side - 28, 22);
                let on = format_name(&style.format) == label
                    || (label == "Automatic" && style.format == "General");
                p.button(
                    r,
                    if on {
                        Color(0, 122, 255, 30)
                    } else {
                        Color::TRANSPARENT
                    },
                    4,
                    &format!("sheet:{t}"),
                    label,
                );
                p.left(
                    r.x + 8,
                    r.y + 3,
                    r.width - 16,
                    label,
                    12,
                    if on { accent } else { INK },
                );
                y += 24;
            }
            y += 4;
            let dec = [
                tool("Decimals −", None, "dec:less"),
                tool("Decimals +", None, "dec:more"),
            ];
            for (i, t) in dec.iter().enumerate() {
                let r = Rect::new(
                    s.x + 14 + i as i32 * ((side as i32 - 28) / 2),
                    y,
                    (side - 32) / 2,
                    24,
                );
                p.border(r, Color::WHITE, 4, LINE);
                button(p, r, t, accent);
            }
            y += 36;
            p.strong(s.x + 14, y, side - 28, "Text", 13, INK);
            y += 24;
            let text_tools = [
                Tool {
                    on: style.bold,
                    ..tool("B", None, "bold")
                },
                Tool {
                    on: style.italic,
                    ..tool("I", None, "italic")
                },
                Tool {
                    on: style.underline,
                    ..tool("U", None, "underline")
                },
                tool("Left", Some("list-view"), "align:left"),
                tool("Center", Some("menu"), "align:center"),
                tool("Right", Some("list-view"), "align:right"),
            ];
            for (i, t) in text_tools.iter().enumerate() {
                let r = Rect::new(s.x + 14 + i as i32 * 34, y, 30, 26);
                p.border(r, Color::WHITE, 4, LINE);
                button(p, r, t, accent);
            }
            y += 38;
            p.strong(s.x + 14, y, side - 28, "Fill", 13, INK);
            y += 24;
            for (i, (label, c)) in SWATCHES.iter().enumerate() {
                let r = Rect::new(s.x + 14 + (i as i32 % 8) * 26, y, 22, 22);
                p.box_(r, Color::rgb(c[0], c[1], c[2]), 11);
                p.region(r, &format!("sheet:fill:{}", hex(*c)), label);
            }
            let none = Rect::new(s.x + 14, y + 28, 80, 22);
            p.button(none, Color::TRANSPARENT, 4, "sheet:fill:none", "No Fill");
            p.left(none.x + 4, none.y + 3, 76, "No Fill", 12, accent);
            y += 62;
            // Cell: merging, borders and conditional highlighting.
            p.strong(s.x + 14, y, side - 28, "Cell", 13, INK);
            y += 24;
            for (i, (label, t, menu)) in [
                ("Merge", "menu:merge", "merge"),
                ("Borders", "menu:borders", "borders"),
                ("Conditional Highlighting…", "menu:cf", "cf"),
            ]
            .iter()
            .enumerate()
            {
                let r = Rect::new(s.x + 14, y + i as i32 * 30, side - 28, 26);
                crate::apps::look::action(
                    p,
                    &crate::apps::look::look(DesktopTheme::Macos),
                    r,
                    label,
                    &format!("sheet:{t}"),
                    false,
                );
                anchors.push((menu, r.x, r.y + 26));
            }
        }
    }
    if show_pane(book) {
        let pw = 290.min(w / 3);
        pane_anchors = super::panels::pivot_pane(
            book,
            p,
            Rect::new((w - pw) as i32, 84, pw, h.saturating_sub(84 + 24)),
            f,
            accent,
        );
        anchors.extend(pane_anchors.iter().map(|(n, x, y)| (n.as_str(), *x, *y)));
    }
    // The selection summary bar at the bottom.
    let by = h as i32 - 24;
    p.box_(Rect::new(0, by, w, 24), Color::rgb(246, 246, 246), 0);
    p.hline(0, by, w, LINE);
    p.left(
        12,
        by + 4,
        w.saturating_sub(24),
        &stats_text(book, f),
        11,
        MUTED,
    );
    if let Some((mx, my)) = menu_anchor(book, &anchors) {
        paint_menu(p, book, f, mx, my, 200);
    } else if book.menu.is_some() {
        paint_menu(p, book, f, 40, 90, 200);
    }
}

// ----- Numbers (iOS) -----

fn numbers_ios(book: &Book, p: &mut Painter, env: &AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let accent = Color::rgb(0, 122, 255);
    let f = Flavor::Numbers;
    p.scene.background = Color::rgb(242, 242, 247);
    let top: i32 = 50;
    p.box_(Rect::new(0, 0, w, top as u32), Color::rgb(249, 249, 249), 0);
    p.hline(0, top - 1, w, LINE);
    let back = Rect::new(4, 8, 120, 34);
    p.button(back, Color::TRANSPARENT, 6, "sheet:open", "Spreadsheets");
    p.symbol("chevron-left", back.x + 2, back.y + 8, 18, accent);
    p.left(back.x + 22, back.y + 8, 100, "Spreadsheets", 15, accent);
    p.label(0, 14, w, &book.name, 15, INK, true, Align::Center);
    let icons = [
        if book.workbook.can_undo() {
            tool("Undo", Some("undo"), "undo")
        } else {
            off("Undo", Some("undo"), "there is nothing to undo")
        },
        tool("Format", Some("brush"), "menu:numbersformat"),
        tool("Insert", Some("plus"), "menu:plus"),
        tool("More", Some("more"), "menu:more"),
    ];
    let mut anchors: Vec<(&str, i32, i32)> = Vec::new();
    for (i, t) in icons.iter().enumerate() {
        let r = Rect::new(w as i32 - 172 + i as i32 * 42, 8, 38, 34);
        let t2 = Tool {
            label: t.label,
            symbol: t.symbol,
            target: t.target.clone(),
            on: false,
        };
        button(p, r, &t2, accent);
        if let Some(s) = t.symbol {
            p.symbol(
                s,
                r.x + 10,
                r.y + 8,
                18,
                if t.target.is_ok() { accent } else { FAINT },
            );
        }
    }
    anchors.push(("format", w as i32 - 210, top));
    anchors.push(("numbersformat", w as i32 - 210, top));
    anchors.push(("merge", w as i32 - 210, top));
    anchors.push(("borders", w as i32 - 210, top));
    anchors.push(("cf", w as i32 - 210, top));
    anchors.push(("plus", w as i32 - 210, top));
    anchors.push(("more", w as i32 - 210, top));
    sheet_tabs(p, book, Rect::new(0, top, w, 36), accent, f, true);
    let bar_h = 46;
    let area = Rect::new(
        8,
        top + 44,
        w.saturating_sub(16),
        h.saturating_sub(top as u32 + 44 + bar_h + 8),
    );
    let geom = Geom {
        row_h: 30 * book.zoom / 100,
        scale: book.zoom * 5 / 4,
    };
    let pal = palette(f, (30, 24));
    let pane_w = if show_pane(book) { 290.min(w / 2) } else { 0 };
    let area = Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(pane_w),
        area.height,
    );
    grid::paint(p, book, area, geom, &pal);
    anchors.push(("filter", 20, area.y + 30));
    let pane_anchors = if pane_w > 0 {
        super::panels::pivot_pane(
            book,
            p,
            Rect::new((w - pane_w) as i32, area.y, pane_w, area.height),
            f,
            accent,
        )
    } else {
        vec![]
    };
    anchors.extend(pane_anchors.iter().map(|(n, x, y)| (n.as_str(), *x, *y)));
    mobile_formula_bar(
        p,
        book,
        Rect::new(0, h as i32 - bar_h as i32, w, bar_h),
        accent,
    );
    if let Some((mx, my)) = menu_anchor(book, &anchors) {
        paint_menu(p, book, f, mx, my, 200);
    }
}
/// The phones' input bar: what the cell holds, tapped to edit.
fn mobile_formula_bar(p: &mut Painter, book: &Book, r: Rect, accent: Color) {
    p.box_(r, Color::WHITE, 0);
    p.hline(r.x, r.y, r.width, LINE);
    p.label(
        r.x + 8,
        r.y + 12,
        26,
        "fx",
        14,
        if book.editing.is_some() {
            accent
        } else {
            MUTED
        },
        true,
        Align::Center,
    );
    let input = Rect::new(r.x + 40, r.y + 6, r.width.saturating_sub(96), r.height - 12);
    p.border(
        input,
        Color::rgb(248, 249, 250),
        6,
        if book.editing.is_some() { accent } else { LINE },
    );
    p.region(input, "sheet:edit:bar", "Enter text or formula");
    let text = bar_text(book);
    if text.is_empty() && book.editing.is_none() {
        p.left(
            input.x + 10,
            input.y + 8,
            input.width - 16,
            "Enter text or formula",
            13,
            FAINT,
        );
    } else {
        let refs: &[Color] = match (&book.editing, accent == Color::rgb(0, 122, 255)) {
            (None, _) => &[],
            (Some(_), true) => &NUMBERS_REFS,
            (Some(_), false) => &SHEETS_REFS,
        };
        grid::formula_text(
            p,
            input.x + 10,
            input.y + 8,
            input.width - 16,
            &text,
            13,
            INK,
            refs,
        );
    }
    let done = Rect::new(r.x + r.width as i32 - 52, r.y + 6, 46, r.height - 12);
    if book.editing.is_some() {
        p.button(done, accent, 17, "sheet:enter", "Done");
        p.symbol("check", done.x + 14, done.y + 8, 18, Color::WHITE);
    } else {
        p.region(done, "sheet:noop", "Done");
        p.disabled("there is no entry to confirm");
        p.symbol("check", done.x + 14, done.y + 8, 18, FAINT);
    }
}

// ----- LibreOffice Calc -----

fn calc(book: &Book, p: &mut Painter, env: &AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let accent = Color::rgb(233, 84, 32);
    let f = Flavor::Calc;
    p.scene.background = Color::rgb(246, 246, 246);
    // Menu bar.
    p.box_(Rect::new(0, 0, w, 26), Color::rgb(246, 246, 246), 0);
    let menus: [(&str, Result<&str, &str>); 11] = [
        ("File", Ok("file")),
        ("Edit", Ok("edit")),
        ("View", Ok("view")),
        ("Insert", Ok("calcinsert")),
        ("Format", Ok("calcformat")),
        ("Styles", Err("cell styles are not modeled")),
        ("Sheet", Ok("calcsheet")),
        ("Data", Ok("calcdata")),
        ("Tools", Err("the tools dialogs are not modeled")),
        ("Window", Err("one document window is shown at a time")),
        ("Help", Err("help content is not available offline")),
    ];
    let mut anchors: Vec<(&str, i32, i32)> = Vec::new();
    let mut x = 6;
    for (label, target) in menus {
        let mw = p.measure(label, 12, false) + 16;
        let r = Rect::new(x, 2, mw, 22);
        match target {
            Ok(m) => {
                let open = book.menu.as_deref() == Some(m);
                if open {
                    p.box_(r, Color(233, 84, 32, 40), 4);
                }
                p.region(r, &format!("sheet:menu:{m}"), label);
                p.label(r.x, r.y + 3, r.width, label, 12, INK, false, Align::Center);
                anchors.push((m, x, 26));
            }
            Err(why) => {
                p.region(r, "sheet:noop", label);
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
    // Standard toolbar.
    let tb = Rect::new(0, 26, w, 32);
    p.box_(tb, Color::rgb(246, 246, 246), 0);
    let std_tools = [
        tool("New", Some("new-tab"), "new:calc"),
        tool("Open", Some("folder"), "open"),
        tool("Save", Some("download"), "save:calc"),
        if book.workbook.can_undo() {
            tool("Undo", Some("undo"), "undo")
        } else {
            off("Undo", Some("undo"), "there is nothing to undo")
        },
        if book.workbook.can_redo() {
            tool("Redo", Some("redo"), "redo")
        } else {
            off("Redo", Some("redo"), "there is nothing to redo")
        },
        tool("Cut", Some("scissors"), "cut"),
        tool("Copy", Some("copy"), "copy"),
        tool("Paste", Some("paste"), "paste"),
        tool("Sort Ascending", Some("sort"), "sort:asc"),
        tool("Sort Descending", Some("sort"), "sort:desc"),
        Tool {
            on: book.sheet_ref().filter.is_some(),
            ..tool("AutoFilter", Some("filters"), "filter")
        },
        tool("Insert Chart", Some("layers"), "chart:column"),
        tool("Freeze Rows and Columns", Some("grid"), "freeze:panes"),
    ];
    let mut x = 6;
    for (i, t) in std_tools.iter().enumerate() {
        if [3, 5, 8, 11].contains(&i) {
            p.vline(x + 1, tb.y + 6, 20, LINE);
            x += 6;
        }
        button(p, Rect::new(x, tb.y + 3, 28, 26), t, accent);
        x += 30;
    }
    // Formatting toolbar.
    let ft = Rect::new(0, 58, w, 32);
    p.box_(ft, Color::rgb(246, 246, 246), 0);
    let font = Rect::new(6, ft.y + 4, 130, 24);
    p.border(font, Color::WHITE, 4, LINE);
    p.region(font, "sheet:noop", "Font Name");
    p.disabled("only Liberation Sans is available");
    p.left(font.x + 6, font.y + 4, 120, "Liberation Sans", 11, FAINT);
    let size = Rect::new(140, ft.y + 4, 50, 24);
    p.border(size, Color::WHITE, 4, LINE);
    p.region(size, "sheet:noop", "Font Size");
    p.disabled("font sizes are not modeled");
    p.left(size.x + 6, size.y + 4, 40, "10 pt", 11, FAINT);
    let style = book
        .workbook
        .style(book.sheet.min(book.workbook.sheets.len() - 1), book.active);
    let fmt_tools = [
        Tool {
            on: style.bold,
            ..tool("B", None, "bold")
        },
        Tool {
            on: style.italic,
            ..tool("I", None, "italic")
        },
        Tool {
            on: style.underline,
            ..tool("U", None, "underline")
        },
        tool("Font Color", Some("text-tool"), "menu:fontcolor"),
        tool("Background Color", Some("bucket"), "menu:fillcolor"),
        tool("Borders", Some("grid"), "menu:borders"),
        Tool {
            on: style.align == cw_sheet::Align::Left,
            ..tool("Align Left", Some("list-view"), "align:left")
        },
        Tool {
            on: style.align == cw_sheet::Align::Center,
            ..tool("Align Center", Some("menu"), "align:center")
        },
        Tool {
            on: style.align == cw_sheet::Align::Right,
            ..tool("Align Right", Some("list-view"), "align:right")
        },
        Tool {
            on: book.sheet_ref().merges.contains(&book.selection()),
            ..tool("Merge and Center Cells", Some("split"), "merge:center")
        },
        tool("$", None, "fmt:currency"),
        tool("%", None, "fmt:percent"),
        tool("0.0", None, "fmt:number"),
        tool("Date", Some("calendar"), "fmt:date"),
        tool(".0+", None, "dec:more"),
        tool(".0-", None, "dec:less"),
    ];
    let mut x = 196;
    for (i, t) in fmt_tools.iter().enumerate() {
        if [3, 6, 10, 14].contains(&i) {
            p.vline(x + 1, ft.y + 6, 20, LINE);
            x += 6;
        }
        button(p, Rect::new(x, ft.y + 3, 28, 26), t, accent);
        match t.label {
            "Font Color" => anchors.push(("fontcolor", x, ft.y + 30)),
            "Background Color" => anchors.push(("fillcolor", x, ft.y + 30)),
            "Borders" => anchors.push(("borders", x, ft.y + 30)),
            _ => {}
        }
        x += 30;
    }
    // Formula bar: Name Box, Function Wizard, Sum, =, input line.
    let fb = Rect::new(0, 90, w, 28);
    formula_bar(p, book, fb, 90, accent, true);
    anchors.push(("functions", 98, fb.y + 28));
    let status_h = 22;
    let tabs_h = 28;
    let grid_rect = Rect::new(0, 118, w, h.saturating_sub(118 + tabs_h + status_h));
    let geom = Geom {
        row_h: 18 * book.zoom / 100,
        scale: book.zoom * 5 / 4,
    };
    let pal = palette(f, (38, 18));
    let pane_w = if show_pane(book) { 290.min(w / 3) } else { 0 };
    let cells = grid::paint(
        p,
        book,
        Rect::new(
            0,
            grid_rect.y,
            grid_rect.width.saturating_sub(16 + pane_w),
            grid_rect.height,
        ),
        geom,
        &pal,
    );
    scrollbars(p, book, Rect::new(0, cells.y, w - pane_w, cells.height));
    anchors.push(("filter", 60, cells.y + 20));
    let pane_anchors = if pane_w > 0 {
        super::panels::pivot_pane(
            book,
            p,
            Rect::new((w - pane_w) as i32, grid_rect.y, pane_w, grid_rect.height),
            f,
            accent,
        )
    } else {
        vec![]
    };
    anchors.extend(pane_anchors.iter().map(|(n, x, y)| (n.as_str(), *x, *y)));
    let ty = grid_rect.y + grid_rect.height as i32;
    // Calc puts sheet navigation arrows and + before the tabs.
    p.box_(Rect::new(0, ty, w, tabs_h), Color::rgb(243, 243, 243), 0);
    let n = book.workbook.sheets.len();
    let nav = [
        ("First Sheet", Some("skip-previous"), 0usize),
        (
            "Previous Sheet",
            Some("chevron-left"),
            book.sheet.saturating_sub(1),
        ),
        (
            "Next Sheet",
            Some("chevron-right"),
            (book.sheet + 1).min(n - 1),
        ),
        ("Last Sheet", Some("skip-next"), n - 1),
    ];
    for (i, (label, sym, target)) in nav.iter().enumerate() {
        button(
            p,
            Rect::new(4 + i as i32 * 22, ty + 4, 20, 20),
            &tool(label, *sym, &format!("tab:{target}")),
            INK,
        );
    }
    sheet_tabs(
        p,
        book,
        Rect::new(92, ty, w.saturating_sub(92), tabs_h),
        accent,
        f,
        true,
    );
    let sy = ty + tabs_h as i32;
    p.box_(Rect::new(0, sy, w, status_h), Color::rgb(243, 243, 243), 0);
    p.hline(0, sy, w, LINE);
    p.left(
        8,
        sy + 3,
        140,
        &format!("Sheet {} of {}", book.sheet + 1, n),
        11,
        INK,
    );
    p.left(150, sy + 3, 80, "Default", 11, MUTED);
    p.left(230, sy + 3, 110, "English (USA)", 11, MUTED);
    let stats = stats_text(book, f);
    p.right(0, sy + 3, w.saturating_sub(170), &stats, 11, INK);
    zoom_control(
        p,
        Rect::new(w as i32 - 156, sy, 150, status_h),
        book,
        accent,
    );
    if let Some((mx, my)) = menu_anchor(book, &anchors) {
        paint_menu(p, book, f, mx, my, 230);
    } else if book.menu.is_some() {
        paint_menu(p, book, f, 60, 120, 230);
    }
}

// ----- Google Sheets (Android) -----

fn sheets(book: &Book, p: &mut Painter, env: &AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let accent = Color::rgb(26, 115, 232);
    let green = Color::rgb(15, 157, 88);
    let f = Flavor::Sheets;
    p.scene.background = Color::WHITE;
    let top: i32 = 56;
    p.box_(Rect::new(0, 0, w, top as u32), Color::WHITE, 0);
    button(
        p,
        Rect::new(4, 8, 40, 40),
        &tool("Back to spreadsheets", Some("arrow-left"), "open"),
        INK,
    );
    p.left(52, 17, w.saturating_sub(250), &book.name, 17, INK);
    let icons = [
        if book.workbook.can_undo() {
            tool("Undo", Some("undo"), "undo")
        } else {
            off("Undo", Some("undo"), "there is nothing to undo")
        },
        if book.workbook.can_redo() {
            tool("Redo", Some("redo"), "redo")
        } else {
            off("Redo", Some("redo"), "there is nothing to redo")
        },
        Tool {
            on: book.menu.as_deref() == Some("sheetsformat"),
            ..tool("Format", Some("text-tool"), "menu:sheetsformat")
        },
        tool("Insert", Some("plus"), "menu:plus"),
        tool("More options", Some("more-vertical"), "menu:more"),
    ];
    for (i, t) in icons.iter().enumerate() {
        button(
            p,
            Rect::new(w as i32 - 200 + i as i32 * 40, 8, 38, 40),
            t,
            green,
        );
    }
    let mut anchors = vec![
        ("plus", w as i32 - 210, top),
        ("more", w as i32 - 210, top),
        ("filter", 20, top + 40),
        ("merge", 16, top + 40),
        ("borders", 16, top + 40),
        ("cf", 16, top + 40),
    ];
    let format_panel = book.menu.as_deref() == Some("sheetsformat");
    let panel_h: u32 = if format_panel { 250 } else { 0 };
    let bar_h = 50;
    let tabs_h = 48;
    let area = Rect::new(
        0,
        top,
        w,
        h.saturating_sub(top as u32 + bar_h + tabs_h + panel_h),
    );
    let geom = Geom {
        row_h: 30 * book.zoom / 100,
        scale: book.zoom * 3 / 2,
    };
    let pal = palette(f, (36, 26));
    let pane_w = if show_pane(book) { 290.min(w / 2) } else { 0 };
    grid::paint(
        p,
        book,
        Rect::new(
            area.x,
            area.y,
            area.width.saturating_sub(pane_w),
            area.height,
        ),
        geom,
        &pal,
    );
    let pane_anchors = if pane_w > 0 {
        super::panels::pivot_pane(
            book,
            p,
            Rect::new((w - pane_w) as i32, area.y, pane_w, area.height),
            f,
            accent,
        )
    } else {
        vec![]
    };
    anchors.extend(pane_anchors.iter().map(|(n, x, y)| (n.as_str(), *x, *y)));
    let bar_y = area.y + area.height as i32;
    mobile_formula_bar(p, book, Rect::new(0, bar_y, w, bar_h), accent);
    if format_panel {
        let r = Rect::new(0, bar_y + bar_h as i32, w, panel_h);
        p.box_(r, Color::rgb(248, 249, 250), 0);
        p.hline(0, r.y, w, LINE);
        let style = book
            .workbook
            .style(book.sheet.min(book.workbook.sheets.len() - 1), book.active);
        p.strong(16, r.y + 10, 200, "Text", 13, INK);
        let text_tools = [
            Tool {
                on: style.bold,
                ..tool("B", None, "bold")
            },
            Tool {
                on: style.italic,
                ..tool("I", None, "italic")
            },
            Tool {
                on: style.underline,
                ..tool("U", None, "underline")
            },
            tool("Align left", Some("list-view"), "align:left"),
            tool("Align center", Some("menu"), "align:center"),
            tool("Align right", Some("list-view"), "align:right"),
        ];
        for (i, t) in text_tools.iter().enumerate() {
            let b = Rect::new(16 + i as i32 * 48, r.y + 34, 44, 40);
            p.border(b, Color::WHITE, 8, LINE);
            button(p, b, t, accent);
        }
        p.strong(16, r.y + 86, 200, "Cell", 13, INK);
        let chips = [
            ("123", "fmt:general"),
            ("0.00", "fmt:number"),
            ("%", "fmt:percent"),
            ("$", "fmt:currency"),
            ("Date", "fmt:date"),
        ];
        for (i, (label, t)) in chips.iter().enumerate() {
            let b = Rect::new(16 + i as i32 * 64, r.y + 110, 58, 34);
            p.border(b, Color::WHITE, 17, LINE);
            button(p, b, &tool(label, None, t), accent);
        }
        for (i, (label, c)) in SWATCHES.iter().enumerate() {
            let b = Rect::new(16 + i as i32 * 36, r.y + 156, 30, 30);
            p.box_(b, Color::rgb(c[0], c[1], c[2]), 15);
            p.region(b, &format!("sheet:fill:{}", hex(*c)), label);
        }
        for (i, (label, t)) in [
            ("Merge cells", "menu:merge"),
            ("Borders", "menu:borders"),
            ("Conditional formatting", "menu:cf"),
        ]
        .iter()
        .enumerate()
        {
            let bw = (w.saturating_sub(48)) / 3;
            let b = Rect::new(16 + i as i32 * (bw as i32 + 8), r.y + 200, bw, 36);
            p.border(b, Color::WHITE, 18, LINE);
            button(p, b, &tool(label, None, t), accent);
        }
    }
    let ty = h as i32 - tabs_h as i32;
    // Sheet tabs along the bottom, with the add button first.
    p.box_(Rect::new(0, ty, w, tabs_h), Color::rgb(248, 249, 250), 0);
    p.hline(0, ty, w, LINE);
    button(
        p,
        Rect::new(8, ty + 6, 36, 36),
        &tool("Add sheet", Some("plus"), "insert:sheet"),
        green,
    );
    let mut x = 52;
    let active = book.sheet.min(book.workbook.sheets.len() - 1);
    for (i, s) in book.workbook.sheets.iter().enumerate() {
        let tw = p.measure(&s.name, 13, i == active) + 32;
        let r = Rect::new(x, ty + 4, tw, tabs_h - 8);
        if i == active {
            p.box_(r, Color::rgb(232, 240, 254), 8);
        }
        p.region(r, &format!("sheet:tab:{i}"), &s.name);
        p.label(
            r.x,
            r.y + 11,
            r.width,
            &s.name,
            13,
            if i == active {
                Color::rgb(11, 87, 208)
            } else {
                MUTED
            },
            i == active,
            Align::Center,
        );
        x += tw as i32 + 4;
    }
    let stats = stats_text(book, f);
    if !stats.is_empty() {
        let sw = p.measure(&stats, 12, false) + 20;
        let r = Rect::new(w as i32 - sw as i32 - 8, ty + 8, sw, 32);
        p.box_(r, Color::rgb(232, 240, 254), 16);
        p.label(
            r.x,
            r.y + 8,
            r.width,
            &stats,
            12,
            Color::rgb(11, 87, 208),
            false,
            Align::Center,
        );
    }
    if let Some((mx, my)) = menu_anchor(book, &anchors) {
        paint_menu(p, book, f, mx, my, 200);
    }
}
