//! Merged cells, borders, conditional formatting, pivot tables and chart placement:
//! the edits, and their survival through XLSX and ODS, including files LibreOffice
//! 24.2 wrote.
use cw_sheet::conditional::{CellOp, Cfvo, CondFormat, Dxf, Period, Rule, TextOp};
use cw_sheet::pivot::{Agg, PivotStyle};
use cw_sheet::*;

fn cell(a1: &str) -> Cell {
    Cell::parse(a1).unwrap()
}
fn range(a1: &str) -> Range {
    Range::parse(a1).unwrap()
}
fn sales() -> Workbook {
    let mut wb = Workbook::new();
    wb.set_now(1_789_635_600_000_000);
    let data = [
        ["Region", "Product", "Sales", "Units"],
        ["East", "Pens", "10", "1"],
        ["West", "Pens", "20", "2"],
        ["East", "Ink", "5", "3"],
        ["West", "Pads", "7", "4"],
        ["East", "Pens", "1", "5"],
    ];
    for (r, row) in data.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            wb.set_input(0, Cell::new(r as u32, c as u32), v).unwrap();
        }
    }
    wb
}
fn every_rule() -> Vec<Rule> {
    let red = Dxf::preset("lightred").unwrap();
    let green = Dxf::preset("green").unwrap();
    vec![
        Rule::CellIs {
            op: CellOp::Between,
            formulas: vec!["2".into(), "$D$6".into()],
            style: red,
        },
        Rule::Text {
            op: TextOp::BeginsWith,
            text: "Pe\"n".into(),
            style: green,
        },
        Rule::Dates {
            period: Period::LastMonth,
            style: red,
        },
        Rule::Duplicates {
            unique: true,
            style: green,
        },
        Rule::Top {
            bottom: true,
            rank: 20,
            percent: true,
            style: red,
        },
        Rule::Average {
            below: true,
            equal: true,
            style: green,
        },
        Rule::Blanks {
            blanks: false,
            style: red,
        },
        Rule::Errors {
            errors: true,
            style: green,
        },
        Rule::Expression {
            formula: "AND(C2>5,$D2<4)".into(),
            style: Dxf {
                bold: true,
                italic: true,
                underline: true,
                color: Some([0, 0, 255]),
                fill: None,
            },
        },
        Rule::DataBar {
            color: [99, 190, 123],
            min: Cfvo::Num(0.0),
            max: Cfvo::Percentile(90.0),
        },
        Rule::ColorScale {
            stops: vec![
                (Cfvo::Min, [248, 105, 107]),
                (Cfvo::Percentile(50.0), [255, 235, 132]),
                (Cfvo::Max, [99, 190, 123]),
            ],
        },
        Rule::IconSet {
            set: "4Arrows".into(),
            points: vec![
                Cfvo::Percent(0.0),
                Cfvo::Percent(25.0),
                Cfvo::Percent(50.0),
                Cfvo::Percent(75.0),
            ],
            reverse: true,
            show_value: false,
        },
    ]
}
/// Everything this suite checks, on one workbook.
fn featured() -> Workbook {
    let mut wb = sales();
    wb.set_input(0, cell("A9"), "Quarterly sales").unwrap();
    wb.merge(0, range("A9:D10"), MergeMode::All, true).unwrap();
    wb.merge(0, range("F1:G3"), MergeMode::Across, false)
        .unwrap();
    let black = Edge::new(Line::Thin, [0, 0, 0]);
    wb.apply_border(0, range("A1:D6"), BorderPreset::All, black)
        .unwrap();
    wb.apply_border(0, range("A1:D6"), BorderPreset::ThickOutside, black)
        .unwrap();
    wb.apply_border(
        0,
        range("C6"),
        BorderPreset::TopDoubleBottom,
        Edge::new(Line::Dashed, [192, 0, 0]),
    )
    .unwrap();
    for rule in every_rule().into_iter().rev() {
        wb.add_conditional(0, CondFormat::new(range("C2:D6"), rule))
            .unwrap();
    }
    wb.add_chart(0, ChartKind::Line, range("B1:C6"), "Sales")
        .unwrap();
    wb.place_chart(0, 0, (cell("I2"), 13, 7), (cell("O16"), 30, 11))
        .unwrap();
    let s = wb.add_sheet(Some("Report")).unwrap();
    let p = wb
        .add_pivot(0, range("A1:D6"), s, cell("A3"), PivotStyle::Excel, false)
        .unwrap();
    wb.edit_pivot(s, p, |p| {
        p.rows = vec![0];
        p.cols = vec![1];
        p.filters = vec![3];
        p.values = vec![(2, Agg::Sum)];
        p.hidden.insert(3, ["5".to_string()].into_iter().collect());
        Ok(())
    })
    .unwrap();
    wb
}

#[test]
fn merging_keeps_the_upper_left_value_and_follows_structure_edits() {
    let mut wb = sales();
    assert!(wb.merge_loses_data(0, range("A1:B2"), MergeMode::All));
    wb.merge(0, range("A1:B2"), MergeMode::All, true).unwrap();
    assert_eq!(wb.sheets[0].merges, [range("A1:B2")]);
    assert_eq!(wb.display(0, cell("A1")), "Region");
    assert_eq!(wb.display(0, cell("B1")), "", "the other values are gone");
    assert_eq!(wb.style(0, cell("A1")).align, Align::Center);
    // A selection that cuts a merged area grows to hold it, as Excel's does.
    assert_eq!(wb.sheets[0].expand_merges(range("B2:C3")), range("A1:C3"));
    // Merge Across merges each row on its own; merging over merges absorbs them.
    wb.merge(0, range("A4:C5"), MergeMode::Across, false)
        .unwrap();
    assert_eq!(
        wb.sheets[0].merges,
        [range("A1:B2"), range("A4:C4"), range("A5:C5")]
    );
    wb.insert_rows(0, 0, 2).unwrap();
    assert_eq!(wb.sheets[0].merges[0], range("A3:B4"));
    wb.delete_cols(0, 1, 1).unwrap();
    assert_eq!(wb.sheets[0].merges[0], range("A3:A4"));
    assert!(wb.sort(0, range("A3:C8"), 0, true, false).is_err());
    wb.unmerge(0, range("A1:Z99")).unwrap();
    assert!(wb.sheets[0].merges.is_empty());
    assert!(wb.undo());
    assert_eq!(wb.sheets[0].merges.len(), 3);
}

#[test]
fn border_presets_draw_edges_the_way_excel_does() {
    let mut wb = sales();
    let pen = Edge::new(Line::Thin, [0, 0, 0]);
    wb.apply_border(0, range("B2:C3"), BorderPreset::Outside, pen)
        .unwrap();
    let b = |wb: &Workbook, a: &str| wb.style(0, cell(a)).borders;
    assert_eq!(b(&wb, "B2").top, Some(pen));
    assert_eq!(b(&wb, "B2").left, Some(pen));
    assert_eq!(b(&wb, "B2").right, None, "inside edges stay clear");
    assert_eq!(b(&wb, "C3").bottom, Some(pen));
    wb.apply_border(0, range("B2:C3"), BorderPreset::InsideVertical, pen)
        .unwrap();
    assert_eq!(b(&wb, "B2").right, Some(pen));
    assert_eq!(b(&wb, "C2").left, Some(pen));
    wb.apply_border(0, range("B2:C3"), BorderPreset::ThickBottom, pen)
        .unwrap();
    assert_eq!(b(&wb, "C3").bottom.unwrap().line, Line::Thick);
    // A shared edge shows the heavier of the two cells' own edges.
    assert_eq!(
        wb.shared_edge(0, cell("C2"), false),
        None,
        "C2 and C3 share no edge yet"
    );
    assert_eq!(
        wb.shared_edge(0, cell("B3"), false).unwrap().line,
        Line::Thick
    );
    wb.apply_border(0, range("B2:C3"), BorderPreset::None, pen)
        .unwrap();
    assert!(b(&wb, "B2").is_empty());
    // Borders on empty cells keep them stored, and undo takes them away again.
    wb.apply_border(0, range("K20"), BorderPreset::All, pen)
        .unwrap();
    assert!(wb.cell(0, cell("K20")).is_some());
    assert!(wb.undo());
    assert!(wb.cell(0, cell("K20")).is_none());
}

#[test]
fn everything_survives_xlsx() {
    let wb = featured();
    let bytes = xlsx::write(&wb, "Microsoft Excel");
    assert_eq!(bytes, xlsx::write(&wb, "Microsoft Excel"));
    let back = xlsx::read(&bytes).unwrap();
    let (a, b) = (&wb.sheets[0], &back.sheets[0]);
    assert_eq!(b.merges, a.merges);
    assert_eq!(b.conditional, a.conditional, "every rule, format and order");
    for c in ["A1", "D6", "C6", "B3"] {
        assert_eq!(
            back.style(0, cell(c)).borders,
            wb.style(0, cell(c)).borders,
            "{c}"
        );
    }
    assert_eq!(b.charts[0].offsets, [13, 7, 30, 11]);
    assert_eq!(b.charts[0].corners(), a.charts[0].corners());
    let (pa, pb) = (&wb.sheets[1].pivots[0], &back.sheets[1].pivots[0]);
    assert_eq!(
        (
            &pb.source_sheet,
            pb.source,
            pb.at,
            &pb.rows,
            &pb.cols,
            &pb.values,
            &pb.filters,
            &pb.hidden
        ),
        (
            &pa.source_sheet,
            pa.source,
            pa.at,
            &pa.rows,
            &pa.cols,
            &pa.values,
            &pa.filters,
            &pa.hidden
        )
    );
    assert_eq!(pb.extent, pa.extent);
    // Center Across Selection is an alignment of its own in SpreadsheetML.
    let mut wb = sales();
    wb.update_style(0, range("A1:C1"), |s| s.align = Align::CenterAcross)
        .unwrap();
    let back = xlsx::read(&xlsx::write(&wb, "Microsoft Excel")).unwrap();
    assert_eq!(back.style(0, cell("B1")).align, Align::CenterAcross);
}

#[test]
fn everything_survives_ods() {
    let wb = featured();
    let back = ods::read(&ods::write(&wb, "LibreOffice/24.2")).unwrap();
    let (a, b) = (&wb.sheets[0], &back.sheets[0]);
    assert_eq!(b.merges, a.merges);
    // ODF has no Excel-only rule flags (stop if true, reversed icons); the rules are
    // the same otherwise.
    let plain = |v: &[CondFormat]| -> Vec<CondFormat> {
        v.iter()
            .map(|cf| {
                let mut cf = cf.clone();
                if let Rule::IconSet { reverse, .. } = &mut cf.rule {
                    *reverse = false;
                }
                cf
            })
            .collect()
    };
    let expected = plain(&a.conditional)
        .into_iter()
        .map(|mut cf| {
            // Blank tests travel as the formula LibreOffice evaluates.
            if let Rule::Blanks { blanks, style } = cf.rule {
                cf.rule = Rule::Expression {
                    formula: format!("LEN(TRIM(C2)){}0", if blanks { "=" } else { ">" }),
                    style,
                };
            }
            cf
        })
        .collect::<Vec<_>>();
    assert_eq!(b.conditional, expected);
    for c in ["A1", "D6", "B3"] {
        assert_eq!(
            back.style(0, cell(c)).borders,
            wb.style(0, cell(c)).borders,
            "{c}"
        );
    }
    let (pa, pb) = (&wb.sheets[1].pivots[0], &back.sheets[1].pivots[0]);
    assert_eq!(
        (
            &pb.rows,
            &pb.cols,
            &pb.values,
            &pb.filters,
            &pb.hidden,
            pb.at,
            pb.extent
        ),
        (
            &pa.rows,
            &pa.cols,
            &pa.values,
            &pa.filters,
            &pa.hidden,
            pa.at,
            pa.extent
        )
    );
}

#[test]
fn files_libreoffice_wrote_read_back_with_their_features() {
    for (name, bytes) in [
        (
            "xlsx",
            &include_bytes!("fixtures/libreoffice-features.xlsx")[..],
        ),
        (
            "ods",
            &include_bytes!("fixtures/libreoffice-features.ods")[..],
        ),
    ] {
        let wb = if name == "xlsx" {
            xlsx::read(bytes)
        } else {
            ods::read(bytes)
        }
        .unwrap();
        let s = &wb.sheets[0];
        assert_eq!(s.merges, [range("A9:C10")], "{name}");
        let a1 = wb.style(0, cell("A1")).borders;
        assert_eq!(a1.top, Some(Edge::new(Line::Thick, [192, 0, 0])), "{name}");
        assert_eq!(a1.bottom, Some(Edge::new(Line::Thin, [0, 0, 0])), "{name}");
        assert!(
            s.conditional.iter().any(|cf| cf.rule
                == Rule::CellIs {
                    op: CellOp::Greater,
                    formulas: vec!["6".into()],
                    style: Dxf::preset("lightred").unwrap()
                }),
            "{name}"
        );
        assert!(s
            .conditional
            .iter()
            .any(|cf| matches!(cf.rule, Rule::DataBar { .. })));
        let p = &wb.sheets[1].pivots[0];
        assert_eq!(
            (&p.rows, &p.cols, &p.values, &p.filters),
            (&vec![0], &vec![1], &vec![(2, Agg::Sum)], &vec![3]),
            "{name}"
        );
        // Rebuilding the report from the source gives LibreOffice's figures.
        let mut wb = wb.clone();
        wb.refresh_pivot(1, 0).unwrap();
        let body = wb.sheets[1].pivots[0].at;
        let east = Cell::new(body.row + 2, body.col + 1);
        assert_eq!(wb.display(1, east), "5", "{name}: East, Ink");
    }
}
