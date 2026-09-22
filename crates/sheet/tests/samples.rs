//! The sample spreadsheet files seeded into users' Documents folders are made by this
//! engine, not by hand: `worlds/company-2026/samples/Budget.xlsx` and `Sales.csv` must be
//! exactly what these builders write. Regenerate them with
//! `CW_UPDATE_SAMPLES=1 cargo test -p cw-sheet --test samples`, then run
//! `scripts/build-content.sh` to seed them.
use cw_sheet::{Cell, ChartKind, Range, Workbook};
use std::path::PathBuf;

/// 2026-09-17 09:00:00 UTC, the world's first tick.
const EPOCH_UNIX_US: i64 = 1_789_635_600_000_000;

fn files() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../worlds/company-2026/samples")
}
fn range(text: &str) -> Range {
    Range::parse(text).expect("range")
}
fn put(wb: &mut Workbook, sheet: usize, rows: &[&[&str]]) {
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            if !text.is_empty() {
                wb.set_input(sheet, Cell::new(r as u32, c as u32), text)
                    .expect("input");
            }
        }
    }
}

/// A quarter's household budget: expenses by month with row totals and averages,
/// monthly totals, a summary sheet that reads across sheets, and a column chart.
pub fn budget() -> Workbook {
    let mut wb = Workbook::new();
    wb.set_now(EPOCH_UNIX_US);
    wb.rename_sheet(0, "Budget").unwrap();
    put(
        &mut wb,
        0,
        &[
            &[
                "Category",
                "July",
                "August",
                "September",
                "Total",
                "Average",
            ],
            &["Rent", "1850", "1850", "1850"],
            &["Utilities", "142.35", "156.80", "131.10"],
            &["Groceries", "486.20", "512.75", "470.40"],
            &["Transport", "96", "110.5", "88.25"],
            &["Insurance", "215", "215", "215"],
            &["Entertainment", "120", "85.6", "143.9"],
            &[
                "Total",
                "=SUM(B2:B7)",
                "=SUM(C2:C7)",
                "=SUM(D2:D7)",
                "=SUM(E2:E7)",
                "=AVERAGE(B8:D8)",
            ],
        ],
    );
    for r in 2..=7 {
        wb.set_input(0, Cell::new(r - 1, 4), &format!("=SUM(B{r}:D{r})"))
            .unwrap();
        wb.set_input(
            0,
            Cell::new(r - 1, 5),
            &format!("=ROUND(AVERAGE(B{r}:D{r}),2)"),
        )
        .unwrap();
    }
    wb.update_style(0, range("A1:F1"), |s| {
        s.bold = true;
        s.fill = Some([221, 235, 247]);
    })
    .unwrap();
    wb.update_style(0, range("A8:F8"), |s| s.bold = true)
        .unwrap();
    wb.update_style(0, range("B2:F8"), |s| s.format = "$#,##0.00".into())
        .unwrap();
    wb.set_col_width(0, 0, 110).unwrap();
    for c in 1..=5 {
        wb.set_col_width(0, c, 84).unwrap();
    }
    wb.set_freeze(0, 1, 0).unwrap();
    wb.add_chart(0, ChartKind::Column, range("A1:D7"), "Spending by month")
        .unwrap();

    let summary = wb.add_sheet(Some("Summary")).unwrap();
    put(
        &mut wb,
        summary,
        &[
            &["Quarter total", "=Budget!E8"],
            &["Monthly average", "=Budget!F8"],
            &[
                "Largest category",
                "=INDEX(Budget!A2:A7,MATCH(MAX(Budget!E2:E7),Budget!E2:E7,0))",
            ],
            &["Share of rent", "=Budget!E2/Budget!E8"],
            &["Months over $3,000", "=COUNTIF(Budget!B8:D8,\">3000\")"],
            &["Prepared", "=DATE(2026,9,17)"],
        ],
    );
    wb.update_style(summary, range("B1:B2"), |s| s.format = "$#,##0.00".into())
        .unwrap();
    wb.update_style(summary, range("B4"), |s| s.format = "0.0%".into())
        .unwrap();
    wb.update_style(summary, range("B6"), |s| s.format = "mmmm d, yyyy".into())
        .unwrap();
    wb.update_style(summary, range("A1:A6"), |s| s.bold = true)
        .unwrap();
    wb.set_col_width(summary, 0, 140).unwrap();
    wb.set_col_width(summary, 1, 120).unwrap();
    wb
}

/// A quarter of sales records, as a system would export them.
pub fn sales() -> String {
    let regions = ["North", "South", "East", "West"];
    let products = [
        ("Widget", 19.99),
        ("Gadget", 34.5),
        ("Gizmo", 12.75),
        ("Doohickey", 8.25),
    ];
    let mut out = String::from("Date,Region,Product,Units,Unit Price\n");
    // A fixed linear congruential sequence: the same rows every time, varied enough to
    // sort, filter and chart.
    let mut seed: u32 = 20260917;
    for i in 0..48u32 {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let month = 7 + i / 16;
        let day = 1 + (i % 16) * 2 - u32::from(month == 9 && i % 16 == 15);
        let region = regions[((seed >> 8) % 4) as usize];
        let (product, price) = products[((seed >> 12) % 4) as usize];
        let units = 5 + (seed >> 16) % 60;
        out.push_str(&format!(
            "2026-{month:02}-{day:02},{region},{product},{units},{price:.2}\n"
        ));
    }
    out
}

fn check(name: &str, bytes: &[u8]) {
    let path = files().join(name);
    if std::env::var_os("CW_UPDATE_SAMPLES").is_some() {
        std::fs::create_dir_all(files()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        return;
    }
    let current = std::fs::read(&path).unwrap_or_default();
    assert!(
        current == bytes,
        "{name} is not what the engine writes; regenerate with CW_UPDATE_SAMPLES=1"
    );
}

#[test]
fn the_seeded_budget_workbook_is_what_the_engine_writes() {
    let wb = budget();
    // The sample is worth opening: its formulas evaluate to real totals.
    assert_eq!(wb.display(0, Cell::new(7, 4)), "$8,738.85");
    assert_eq!(wb.display(1, Cell::new(2, 1)), "Rent");
    let bytes = cw_sheet::xlsx::write(&wb, "Microsoft Excel");
    // And it reads back as the same workbook.
    let back = cw_sheet::xlsx::read(&bytes).unwrap();
    assert_eq!(back.display(0, Cell::new(7, 4)), "$8,738.85");
    assert_eq!(back.sheets[0].charts.len(), 1);
    check("Budget.xlsx", &bytes);
}

#[test]
fn the_seeded_sales_csv_is_what_the_builder_writes() {
    let text = sales();
    let wb = cw_sheet::csv::read(&text, "Sales");
    assert_eq!(wb.sheets[0].cells.len(), 49 * 5);
    check("Sales.csv", text.as_bytes());
}
