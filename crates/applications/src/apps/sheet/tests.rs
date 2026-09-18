use super::*;
use crate::desktop_scene::Painter;
use crate::{AppEnv, PointerPhase, SystemSettings};

fn env(theme: DesktopTheme, width: u32, height: u32) -> AppEnv<'static> {
    AppEnv {
        theme,
        width,
        height,
        clock_us: 0,
        settings: &SystemSettings::DEFAULT,
        clipboard: None,
        share_to: None,
        files: Default::default(),
        editor: None,
        pointer: None,
    }
}
const PRODUCTS: [(DesktopTheme, bool, u32, u32); 6] = [
    (DesktopTheme::Windows, false, 1100, 700),
    (DesktopTheme::Macos, false, 1100, 700),
    (DesktopTheme::Macos, true, 1100, 700),
    (DesktopTheme::Ubuntu, false, 1100, 700),
    (DesktopTheme::Ios, false, 820, 1100),
    (DesktopTheme::Android, false, 800, 1200),
];

/// A workbook with a little data, a formula and a chart, B2:C4 selected.
fn book(excel: bool, flavor: Flavor) -> Book {
    let (mut b, _) = Book::launch("/home/u/Documents", 1, 0, excel);
    b.listed(vec![
        "Budget.xlsx".into(),
        "notes.txt".into(),
        "Archive/".into(),
    ]);
    b.command(1, &format!("new:{}", flavor.name()), 0, flavor)
        .unwrap();
    for (cell, text) in [
        ("A1", "Item"),
        ("B1", "Q1"),
        ("C1", "Q2"),
        ("A2", "Pens"),
        ("B2", "3"),
        ("C2", "4"),
        ("A3", "Ink"),
        ("B3", "5"),
        ("C3", "6"),
        ("A4", "Pads"),
        ("B4", "=B2+B3"),
        ("C4", "=SUM(C2:C3)"),
    ] {
        let r = Range::parse(cell).unwrap();
        b.workbook.set_input(0, r.start, text).unwrap();
    }
    b.command(1, "select:A1:C4", 0, flavor).unwrap();
    b.command(1, "chart:column", 0, flavor).unwrap();
    b.command(1, "select:B2:C4", 0, flavor).unwrap();
    b
}
fn targets(b: &Book, theme: DesktopTheme, w: u32, h: u32) -> Vec<String> {
    let mut p = Painter::new(w, h);
    chrome::render(b, &mut p, &env(theme, w, h));
    p.scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .filter(|t| t.starts_with("sheet:") && !Book::drags(t))
        .collect()
}
fn states(excel: bool, flavor: Flavor) -> Vec<Book> {
    let (mut start, _) = Book::launch("/home/u/Documents", 1, 0, excel);
    start.listed(vec!["Budget.xlsx".into(), "Archive/".into()]);
    let base = book(excel, flavor);
    let mut empty = start.clone();
    empty
        .command(1, &format!("new:{}", flavor.name()), 0, flavor)
        .unwrap();
    let mut out = vec![start, empty, base.clone()];
    let menus = [
        "insert",
        "delete",
        "format",
        "fill",
        "clear",
        "sort",
        "numfmt",
        "fillcolor",
        "fontcolor",
        "chart",
        "freeze",
        "zoom",
        "functions",
        "file",
        "edit",
        "view",
        "calcinsert",
        "calcformat",
        "calcsheet",
        "calcdata",
        "more",
        "plus",
        "filter:B",
        "sheetsformat",
    ];
    for m in menus {
        let mut s = base.clone();
        if m == "filter:B" {
            s.command(1, "filter", 0, flavor).unwrap();
        }
        s.menu = Some(m.into());
        out.push(s);
    }
    for tab in ["insert", "formulas", "data", "view", "file"] {
        let mut s = base.clone();
        s.ribbon = tab.into();
        out.push(s);
    }
    for pane in ["format", "organize"] {
        let mut s = base.clone();
        s.command(1, &format!("inspector:{pane}"), 0, flavor)
            .unwrap();
        out.push(s);
    }
    let mut chart = base.clone();
    chart.command(1, "chartsel:0", 0, flavor).unwrap();
    out.push(chart.clone());
    chart.menu = Some("charttype".into());
    out.push(chart);
    let mut editing = base.clone();
    editing.command(1, "select:E1", 0, flavor).unwrap();
    editing.text("=SUM(").unwrap();
    out.push(editing);
    let mut message = base;
    message.message = Some("Something to say".into());
    out.push(message);
    out
}

#[test]
fn every_painted_control_is_one_the_book_handles() {
    for (theme, excel, w, h) in PRODUCTS {
        let flavor = Flavor::of(theme, excel);
        for state in states(excel, flavor) {
            for target in targets(&state, theme, w, h) {
                let command = target.strip_prefix("sheet:").unwrap();
                let mut s = state.clone();
                let result = s.command(1, command, 0, flavor);
                assert!(
                    result.is_ok(),
                    "{flavor:?} on {theme:?}: painted control {target} is refused: {result:?}"
                );
            }
        }
    }
}

#[test]
fn a_pointer_drag_selects_a_range_and_the_fill_handle_fills_it() {
    let mut b = book(false, Flavor::Excel);
    let geom = Geom {
        row_h: 20,
        scale: 100,
    };
    // Cells are 64 px wide and 20 px tall at 100%.
    b.pointer("sheet:grid:20:100", PointerPhase::Down, 70, 25)
        .unwrap();
    b.pointer("sheet:grid:20:100", PointerPhase::Move, 140, 65)
        .unwrap();
    b.pointer("sheet:grid:20:100", PointerPhase::Up, 140, 65)
        .unwrap();
    assert_eq!(b.selection(), Range::parse("B2:C4").unwrap());
    // The press point stays the active cell, as Excel keeps it.
    assert_eq!(b.active, Cell::new(1, 1));
    // Fill B4 down to B6: the handle sits at the corner of the selection.
    b.command(1, "select:B4", 0, Flavor::Excel).unwrap();
    let (hx, hy) = b.cell_origin(geom, Cell::new(4, 2)).unwrap();
    let handle = "sheet:fill:20:100";
    b.pointer(handle, PointerPhase::Down, 3, 3).unwrap();
    b.pointer(handle, PointerPhase::Move, 3, 3 + 40).unwrap();
    b.pointer(handle, PointerPhase::Up, 3, 3 + 40).unwrap();
    let _ = (hx, hy);
    assert_eq!(b.workbook.input(0, Cell::new(4, 1)), "=B3+B4");
    assert_eq!(b.workbook.display(0, Cell::new(4, 1)), "13");
    assert_eq!(b.workbook.input(0, Cell::new(6, 1)), "=B5+B6");
}

#[test]
fn typing_a_formula_and_pointing_at_cells_builds_the_reference() {
    let mut b = book(false, Flavor::Excel);
    b.command(1, "select:E1", 0, Flavor::Excel).unwrap();
    b.text("=SUM(").unwrap();
    b.pointer("sheet:grid:20:100", PointerPhase::Down, 70, 25)
        .unwrap();
    b.pointer("sheet:grid:20:100", PointerPhase::Move, 70, 65)
        .unwrap();
    b.pointer("sheet:grid:20:100", PointerPhase::Up, 70, 65)
        .unwrap();
    assert_eq!(b.editing.as_ref().unwrap().text, "=SUM(B2:B4");
    // Enter closes the parenthesis Excel's way and commits.
    b.key(1, "Enter", 0, None).unwrap();
    assert_eq!(b.workbook.input(0, Cell::new(0, 4)), "=SUM(B2:B4)");
    assert_eq!(b.workbook.display(0, Cell::new(0, 4)), "16");
}

#[test]
fn saving_writes_the_products_own_format() {
    for (flavor, ext) in [
        (Flavor::Calc, "ods"),
        (Flavor::Excel, "xlsx"),
        (Flavor::Numbers, "xlsx"),
    ] {
        let mut b = book(false, flavor);
        let effects = b
            .command(1, &format!("save:{}", flavor.name()), 0, flavor)
            .unwrap();
        let written = effects
            .iter()
            .find_map(|e| match e {
                AppEffect::WriteBytes { path, bytes, .. } => Some((path.clone(), bytes.clone())),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            written.0,
            format!("/home/u/Documents/{}.{ext}", flavor.untitled())
        );
        let back = if ext == "ods" {
            cw_sheet::ods::read(&written.1)
        } else {
            cw_sheet::xlsx::read(&written.1)
        }
        .unwrap();
        assert_eq!(back.input(0, Cell::new(3, 2)), "=SUM(C2:C3)");
        b.saved(&written.0, Ok(()));
        assert!(!b.modified);
    }
}

#[test]
fn the_status_bar_sums_the_selection_in_the_active_cells_format() {
    let mut b = book(false, Flavor::Excel);
    b.command(1, "select:B2:C3", 0, Flavor::Excel).unwrap();
    assert_eq!(
        chrome::stats_text(&b, Flavor::Excel),
        "Average: 4.5    Count: 4    Sum: 18"
    );
    b.command(1, "fmt:currency", 0, Flavor::Excel).unwrap();
    assert_eq!(
        chrome::stats_text(&b, Flavor::Excel),
        "Average: $4.50    Count: 4    Sum: $18.00"
    );
}
