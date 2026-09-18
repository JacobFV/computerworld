//! The formula engine against Excel's documented results.
use cw_sheet::{Cell, ChartKind, ErrorKind, Range, Value, Workbook};

fn cell(a1: &str) -> Cell {
    Cell::parse(a1).unwrap()
}
fn range(a1: &str) -> Range {
    Range::parse(a1).unwrap()
}
/// A workbook with `entries` typed into the first sheet.
fn book(entries: &[(&str, &str)]) -> Workbook {
    let mut wb = Workbook::new();
    wb.set_now(1_789_635_600_000_000); // 2026-09-17 09:00 UTC
    for (a1, text) in entries {
        wb.set_input(0, cell(a1), text)
            .unwrap_or_else(|e| panic!("{a1} {text}: {e}"));
    }
    wb
}
fn shown(wb: &Workbook, a1: &str) -> String {
    wb.display(0, cell(a1))
}
/// Evaluate one formula in a scratch cell of `wb`.
fn eval(wb: &mut Workbook, formula: &str) -> String {
    let at = cell("Z100");
    wb.set_input(0, at, formula)
        .unwrap_or_else(|e| panic!("{formula}: {e}"));
    let v = wb.value(0, at);
    wb.clear(0, Range::single(at)).unwrap();
    v.display()
}

const SALES: &[(&str, &str)] = &[
    ("A1", "Region"),
    ("B1", "Rep"),
    ("C1", "Units"),
    ("D1", "Price"),
    ("A2", "North"),
    ("B2", "Ada"),
    ("C2", "10"),
    ("D2", "2.5"),
    ("A3", "South"),
    ("B3", "Bob"),
    ("C3", "4"),
    ("D3", "10"),
    ("A4", "North"),
    ("B4", "Cy"),
    ("C4", "7"),
    ("D4", "3"),
    ("A5", "East"),
    ("B5", "Di"),
    ("C5", ""),
    ("D5", "8"),
    ("A6", "South"),
    ("B6", "Ed"),
    ("C6", "12"),
    ("D6", "1.25"),
];

#[test]
fn arithmetic_follows_excel_precedence_and_coercion() {
    let mut wb = book(&[("A1", "3"), ("A2", "'5"), ("A3", "abc"), ("A4", "TRUE")]);
    for (f, want) in [
        ("=1+2*3", "7"),
        ("=(1+2)*3", "9"),
        ("=-2^2", "4"),
        ("=2^3^2", "64"),
        ("=10%", "0.1"),
        ("=1/3", "0.333333333333333"),
        ("=0.1+0.2", "0.3"),
        ("=A1+A2", "8"),
        ("=A1+A4", "4"),
        ("=A1+A3", "#VALUE!"),
        ("=A1/0", "#DIV/0!"),
        ("=A1&\"-\"&A4", "3-TRUE"),
        ("=\"a\"=\"A\"", "TRUE"),
        ("=1<\"a\"", "TRUE"),
        ("=A9+1", "1"),
        ("=A9=0", "TRUE"),
        ("=0^0", "#NUM!"),
        ("=nosuch(1)", "#NAME?"),
        ("=undefined_name", "#NAME?"),
    ] {
        assert_eq!(eval(&mut wb, f), want, "{f}");
    }
}

#[test]
fn aggregation_and_conditional_functions() {
    let mut wb = book(SALES);
    for (f, want) in [
        ("=SUM(C2:C6)", "33"),
        ("=SUM(C2:C6,5,\"2\")", "40"),
        ("=AVERAGE(C2:C6)", "8.25"),
        ("=COUNT(C2:C6)", "4"),
        ("=COUNTA(A1:A6)", "6"),
        ("=COUNTBLANK(C2:C6)", "1"),
        ("=MIN(D2:D6)", "1.25"),
        ("=MAX(C:C)", "12"),
        ("=COUNTIF(A2:A6,\"North\")", "2"),
        ("=COUNTIF(C2:C6,\">5\")", "3"),
        ("=COUNTIF(B2:B6,\"?d*\")", "2"),
        ("=COUNTIFS(A2:A6,\"South\",C2:C6,\">5\")", "1"),
        ("=SUMIF(A2:A6,\"North\",C2:C6)", "17"),
        ("=SUMIFS(C2:C6,A2:A6,\"<>North\",D2:D6,\">=1\")", "16"),
        ("=AVERAGEIF(A2:A6,\"South\",D2:D6)", "5.625"),
        ("=MAXIFS(C2:C6,A2:A6,\"South\")", "12"),
        ("=MINIFS(D2:D6,A2:A6,\"North\")", "2.5"),
        ("=SUMPRODUCT(C2:C6,D2:D6)", "101"),
        ("=SUMPRODUCT((A2:A6=\"North\")*C2:C6)", "17"),
        ("=PRODUCT(2,3,4)", "24"),
        ("=SUBTOTAL(9,C2:C6)", "33"),
    ] {
        assert_eq!(eval(&mut wb, f), want, "{f}");
    }
}

#[test]
fn logic_lookup_and_reference_functions() {
    let mut wb = book(SALES);
    for (f, want) in [
        ("=IF(C2>5,\"big\",\"small\")", "big"),
        ("=IF(C3>5,\"big\")", "FALSE"),
        ("=IFS(C3>10,\"a\",C3>3,\"b\")", "b"),
        ("=AND(C2>1,D2>1)", "TRUE"),
        ("=OR(C2>100,D2>100)", "FALSE"),
        ("=NOT(TRUE)", "FALSE"),
        ("=XOR(TRUE,TRUE)", "FALSE"),
        ("=IFERROR(1/0,\"none\")", "none"),
        ("=IFNA(NA(),\"missing\")", "missing"),
        ("=SWITCH(A3,\"North\",1,\"South\",2,0)", "2"),
        ("=CHOOSE(2,\"x\",\"y\",\"z\")", "y"),
        ("=VLOOKUP(\"Cy\",B2:D6,2,FALSE)", "7"),
        ("=VLOOKUP(\"Zed\",B2:D6,2,FALSE)", "#N/A"),
        ("=HLOOKUP(\"Units\",A1:D6,3,FALSE)", "4"),
        ("=INDEX(A1:D6,3,2)", "Bob"),
        ("=MATCH(\"Di\",B1:B6,0)", "5"),
        ("=INDEX(C2:C6,MATCH(\"Ed\",B2:B6,0))", "12"),
        ("=XLOOKUP(\"Bob\",B2:B6,D2:D6)", "10"),
        ("=XLOOKUP(\"Zed\",B2:B6,D2:D6,\"none\")", "none"),
        ("=ROWS(A1:D6)*COLUMNS(A1:D6)", "24"),
        ("=ROW(C4)+COLUMN(C4)", "7"),
        ("=SUM(OFFSET(C2,1,0,3,1))", "11"),
        ("=INDIRECT(\"B\"&3)", "Bob"),
        ("=ADDRESS(2,3)", "$C$2"),
        ("=ISBLANK(C5)", "TRUE"),
        ("=ISNUMBER(C2)", "TRUE"),
        ("=ISTEXT(A2)", "TRUE"),
        ("=ISERROR(1/0)", "TRUE"),
    ] {
        assert_eq!(eval(&mut wb, f), want, "{f}");
    }
    // Approximate VLOOKUP and MATCH over a sorted table.
    let mut wb = book(&[
        ("A1", "0"),
        ("B1", "F"),
        ("A2", "60"),
        ("B2", "D"),
        ("A3", "70"),
        ("B3", "C"),
        ("A4", "80"),
        ("B4", "B"),
        ("A5", "90"),
        ("B5", "A"),
    ]);
    assert_eq!(eval(&mut wb, "=VLOOKUP(85,A1:B5,2)"), "B");
    assert_eq!(eval(&mut wb, "=VLOOKUP(59,A1:B5,2,TRUE)"), "F");
    assert_eq!(eval(&mut wb, "=MATCH(72,A1:A5)"), "3");
    assert_eq!(eval(&mut wb, "=VLOOKUP(-1,A1:B5,2)"), "#N/A");
    // Type 0 is exact whatever the order: the largest value first, not a binary search.
    let mut wb = book(&[("A1", "5550"), ("A2", "430"), ("A3", "1469"), ("A4", "95")]);
    assert_eq!(eval(&mut wb, "=MATCH(5550,A1:A4,0)"), "1");
    assert_eq!(eval(&mut wb, "=MATCH(96,A1:A4,0)"), "#N/A");
}

#[test]
fn math_statistics_and_rounding() {
    let mut wb = book(&[
        ("A1", "2"),
        ("A2", "4"),
        ("A3", "4"),
        ("A4", "4"),
        ("A5", "5"),
        ("A6", "5"),
        ("A7", "7"),
        ("A8", "9"),
    ]);
    for (f, want) in [
        ("=ROUND(2.675,2)", "2.68"),
        ("=ROUND(-2.5,0)", "-3"),
        ("=ROUND(1234.567,-2)", "1200"),
        ("=ROUNDUP(3.141,2)", "3.15"),
        ("=ROUNDDOWN(-3.149,2)", "-3.14"),
        ("=MROUND(10,3)", "9"),
        ("=CEILING(4.2,0.5)", "4.5"),
        ("=FLOOR(4.7,2)", "4"),
        ("=INT(-3.5)", "-4"),
        ("=TRUNC(-3.5)", "-3"),
        ("=ABS(-7)", "7"),
        ("=MOD(-7,3)", "2"),
        ("=POWER(2,10)", "1024"),
        ("=SQRT(16)", "4"),
        ("=SQRT(-1)", "#NUM!"),
        ("=EXP(1)", "2.71828182845905"),
        ("=LN(EXP(2))", "2"),
        ("=LOG10(1000)", "3"),
        ("=LOG(8,2)", "3"),
        ("=FACT(5)", "120"),
        ("=GCD(12,18)", "6"),
        ("=LCM(4,6)", "12"),
        ("=EVEN(3)", "4"),
        ("=ODD(4)", "5"),
        ("=SIGN(-2)", "-1"),
        ("=PI()", "3.14159265358979"),
        ("=DEGREES(PI())", "180"),
        ("=SIN(PI()/2)", "1"),
        ("=AVERAGE(A1:A8)", "5"),
        ("=MEDIAN(A1:A8)", "4.5"),
        ("=MODE(A1:A8)", "4"),
        ("=ROUND(STDEV(A1:A8),10)", "2.1380899353"),
        ("=STDEVP(A1:A8)", "2"),
        ("=VAR(A1:A8)", "4.57142857142857"),
        ("=VARP(A1:A8)", "4"),
        ("=LARGE(A1:A8,2)", "7"),
        ("=SMALL(A1:A8,3)", "4"),
        ("=RANK(7,A1:A8)", "2"),
        ("=RANK(2,A1:A8,1)", "1"),
        ("=PERCENTILE(A1:A8,0.25)", "4"),
        ("=QUARTILE(A1:A8,3)", "5.5"),
    ] {
        assert_eq!(eval(&mut wb, f), want, "{f}");
    }
}

#[test]
fn text_functions() {
    let mut wb = book(&[
        ("A1", "  Hello   World  "),
        ("A2", "spreadsheet"),
        ("A3", "1234.5"),
    ]);
    for (f, want) in [
        ("=TRIM(A1)", "Hello World"),
        ("=LEFT(A2,6)", "spread"),
        ("=RIGHT(A2,5)", "sheet"),
        ("=MID(A2,7,5)", "sheet"),
        ("=LEN(A2)", "11"),
        ("=UPPER(\"abc\")", "ABC"),
        ("=LOWER(\"ABC\")", "abc"),
        ("=PROPER(\"hello wORLD\")", "Hello World"),
        ("=CONCAT(\"a\",1,TRUE)", "a1TRUE"),
        ("=CONCATENATE(\"x\",\"y\")", "xy"),
        ("=TEXTJOIN(\", \",TRUE,\"a\",\"\",\"b\")", "a, b"),
        ("=SUBSTITUTE(\"a-b-c\",\"-\",\"+\")", "a+b+c"),
        ("=SUBSTITUTE(\"a-b-c\",\"-\",\"+\",2)", "a-b+c"),
        ("=REPLACE(\"abcdef\",2,3,\"X\")", "aXef"),
        ("=FIND(\"s\",A2)", "1"),
        ("=FIND(\"S\",A2)", "#VALUE!"),
        ("=SEARCH(\"S\",A2)", "1"),
        ("=SEARCH(\"e?d\",A2)", "4"),
        ("=SEARCH(\"x\",A2)", "#VALUE!"),
        ("=SEARCH(\"e*t\",A2)", "4"),
        ("=REPT(\"ab\",3)", "ababab"),
        ("=EXACT(\"a\",\"A\")", "FALSE"),
        ("=VALUE(\"$1,234.50\")", "1234.5"),
        ("=TEXT(A3,\"#,##0.00\")", "1,234.50"),
        ("=TEXT(0.256,\"0.0%\")", "25.6%"),
        ("=TEXT(46283,\"dddd mmm d\")", "Friday Sep 18"),
        ("=CHAR(65)&CODE(\"a\")", "A97"),
        ("=FIXED(1234.567,1)", "1,234.6"),
        ("=DOLLAR(-5)", "($5.00)"),
    ] {
        assert_eq!(eval(&mut wb, f), want, "{f}");
    }
}

#[test]
fn dates_read_the_world_clock() {
    let mut wb = book(&[("A1", "2026-01-31"), ("A2", "9/18/2026"), ("A3", "13:30")]);
    assert_eq!(shown(&wb, "A1"), "1/31/2026");
    for (f, want) in [
        ("=TODAY()", "46282"),
        ("=TEXT(NOW(),\"yyyy-mm-dd hh:mm\")", "2026-09-17 09:00"),
        ("=DATE(2026,9,18)", "46283"),
        ("=DATE(2026,14,1)=DATE(2027,2,1)", "TRUE"),
        ("=YEAR(A2)&\"-\"&MONTH(A2)&\"-\"&DAY(A2)", "2026-9-18"),
        ("=TEXT(EDATE(A1,1),\"yyyy-mm-dd\")", "2026-02-28"),
        ("=TEXT(EOMONTH(A2,0),\"yyyy-mm-dd\")", "2026-09-30"),
        ("=WEEKDAY(A2)", "6"),
        ("=WEEKDAY(A2,2)", "5"),
        ("=HOUR(A3)&\":\"&MINUTE(A3)", "13:30"),
        ("=DATEDIF(A1,A2,\"m\")", "7"),
        ("=DATEDIF(A1,A2,\"d\")", "230"),
        ("=DAYS(A2,A1)", "230"),
        ("=NETWORKDAYS(DATE(2026,9,14),DATE(2026,9,25))", "10"),
        (
            "=TEXT(WORKDAY(DATE(2026,9,18),1),\"yyyy-mm-dd\")",
            "2026-09-21",
        ),
        ("=TEXT(TIME(14,5,0),\"h:mm AM/PM\")", "2:05 PM"),
    ] {
        assert_eq!(eval(&mut wb, f), want, "{f}");
    }
    // Moving the clock recalculates what reads it.
    wb.set_input(0, cell("B1"), "=TODAY()").unwrap();
    wb.set_now(1_789_635_600_000_000 + 86_400_000_000);
    assert_eq!(wb.value(0, cell("B1")), Value::Number(46283.0));
}

#[test]
fn financial_functions() {
    let mut wb = Workbook::new();
    for (f, want) in [
        ("=ROUND(PMT(0.05/12,360,200000),2)", "-1073.64"),
        ("=ROUND(FV(0.06/12,10*12,-100),2)", "16387.93"),
        ("=ROUND(PV(0.08/12,20*12,500),2)", "-59777.15"),
        ("=ROUND(NPV(0.1,-10000,3000,4200,6800),2)", "1188.44"),
        ("=ROUND(NPER(0.01,-100,1000),4)", "10.5886"),
        ("=ROUND(RATE(48,-200,8000),6)", "0.007701"),
        (
            "=ROUND(IRR({-70000,12000,15000,18000,21000,26000}),6)",
            "0.086631",
        ),
        ("=ROUND(IPMT(0.1/12,1,36,8000),2)", "-66.67"),
        ("=ROUND(PPMT(0.1/12,1,36,8000),2)", "-191.47"),
    ] {
        assert_eq!(eval(&mut wb, f), want, "{f}");
    }
}

#[test]
fn recalculation_follows_dependencies_across_sheets_and_names() {
    let mut wb = book(&[
        ("A1", "1"),
        ("A2", "=A1*2"),
        ("A3", "=A2+A1"),
        ("A4", "=SUM(A1:A3)"),
    ]);
    assert_eq!(shown(&wb, "A4"), "6");
    wb.set_input(0, cell("A1"), "10").unwrap();
    assert_eq!(
        (shown(&wb, "A2"), shown(&wb, "A3"), shown(&wb, "A4")),
        ("20".into(), "30".into(), "60".into())
    );
    let s2 = wb.add_sheet(Some("Totals")).unwrap();
    wb.set_input(s2, cell("A1"), "=Sheet1!A4/2").unwrap();
    wb.define_name("Base", 0, range("A1")).unwrap();
    wb.set_input(s2, cell("A2"), "=Base+1").unwrap();
    wb.set_input(0, cell("A1"), "2").unwrap();
    assert_eq!(wb.value(s2, cell("A1")), Value::Number(6.0));
    assert_eq!(wb.value(s2, cell("A2")), Value::Number(3.0));
    // A formula entered before its inputs picks them up when they arrive.
    wb.set_input(0, cell("C1"), "=C2+C3").unwrap();
    wb.set_input(0, cell("C3"), "5").unwrap();
    assert_eq!(shown(&wb, "C1"), "5");
    // Renaming a sheet rewrites the formulas that name it.
    wb.rename_sheet(0, "Data").unwrap();
    assert_eq!(wb.input(s2, cell("A1")), "=Data!A4/2");
    assert_eq!(wb.value(s2, cell("A1")), Value::Number(6.0));
}

#[test]
fn circular_references_are_detected_and_contained() {
    let mut wb = book(&[
        ("A1", "=B1+1"),
        ("B1", "=A1+1"),
        ("C1", "=A1*2"),
        ("D1", "=D1"),
        ("E1", "7"),
    ]);
    assert_eq!(wb.value(0, cell("A1")), Value::Error(ErrorKind::Circular));
    assert_eq!(wb.value(0, cell("B1")), Value::Error(ErrorKind::Circular));
    assert_eq!(wb.value(0, cell("D1")), Value::Error(ErrorKind::Circular));
    // Downstream of a cycle sees its error; unrelated cells are untouched.
    assert_eq!(wb.value(0, cell("C1")), Value::Error(ErrorKind::Circular));
    assert_eq!(shown(&wb, "E1"), "7");
    assert_eq!(wb.circular().len(), 4);
    // Breaking the cycle heals everything behind it.
    wb.set_input(0, cell("B1"), "5").unwrap();
    assert_eq!(
        (shown(&wb, "A1"), shown(&wb, "C1")),
        ("6".into(), "12".into())
    );
}

#[test]
fn copy_paste_and_fill_adjust_relative_references() {
    let mut wb = book(&[("A1", "1"), ("A2", "2"), ("B1", "=A1*$C$1"), ("C1", "10")]);
    let clip = wb.copy(0, range("B1"));
    wb.paste(&clip, 0, cell("B2"), None).unwrap();
    assert_eq!(wb.input(0, cell("B2")), "=A2*$C$1");
    assert_eq!(shown(&wb, "B2"), "20");
    // Pasting across a larger target tiles the block.
    wb.paste(&clip, 0, cell("D1"), Some(range("D1:D3")))
        .unwrap();
    assert_eq!(wb.input(0, cell("D3")), "=C3*$C$1");
    // A reference pushed off the grid becomes #REF!.
    let clip = wb.copy(0, range("B2"));
    wb.paste(&clip, 0, cell("A5"), None).unwrap();
    assert_eq!(wb.input(0, cell("A5")), "=#REF!*$C$1");
    // Fill down: formulas move, number pairs extend as series, text counts on.
    let mut wb = book(&[
        ("A1", "1"),
        ("A2", "3"),
        ("B1", "=A1*2"),
        ("C1", "Item 1"),
        ("D1", "Mon"),
        ("E1", "2026-09-18"),
    ]);
    wb.fill(0, range("A1:A2"), range("A1:A5")).unwrap();
    assert_eq!(
        (shown(&wb, "A3"), shown(&wb, "A5")),
        ("5".into(), "9".into())
    );
    wb.fill(0, range("B1:E1"), range("B1:E4")).unwrap();
    assert_eq!(wb.input(0, cell("B4")), "=A4*2");
    assert_eq!(shown(&wb, "B4"), "14");
    assert_eq!(shown(&wb, "C3"), "Item 3");
    assert_eq!(shown(&wb, "D4"), "Thu");
    assert_eq!(shown(&wb, "E2"), "9/19/2026");
    // Fill right works the same way.
    wb.fill(0, range("B1"), range("B1:D1")).unwrap();
    assert_eq!(wb.input(0, cell("D1")), "=C1*2");
}

#[test]
fn inserting_and_deleting_rows_and_columns_fixes_references() {
    let mut wb = book(&[
        ("A1", "1"),
        ("A2", "2"),
        ("A3", "3"),
        ("A4", "=SUM(A1:A3)"),
        ("B1", "=A3*10"),
        ("C1", "=A2"),
    ]);
    wb.insert_rows(0, 1, 2).unwrap();
    assert_eq!(wb.input(0, cell("A6")), "=SUM(A1:A5)");
    assert_eq!(wb.input(0, cell("B1")), "=A5*10");
    assert_eq!(shown(&wb, "A6"), "6");
    wb.delete_rows(0, 3, 1).unwrap(); // the row holding 2
    assert_eq!(wb.input(0, cell("A5")), "=SUM(A1:A4)");
    assert_eq!(wb.input(0, cell("C1")), "=#REF!");
    assert_eq!(shown(&wb, "A5"), "4");
    wb.insert_cols(0, 0, 1).unwrap();
    assert_eq!(wb.input(0, cell("B5")), "=SUM(B1:B4)");
    assert_eq!(wb.input(0, cell("C1")), "=B4*10");
    wb.delete_cols(0, 0, 1).unwrap();
    assert_eq!(wb.input(0, cell("A5")), "=SUM(A1:A4)");
    // Undo reverses a structural edit completely.
    assert!(wb.undo());
    assert_eq!(wb.input(0, cell("B5")), "=SUM(B1:B4)");
}

#[test]
fn moving_cells_carries_the_references_to_them() {
    let mut wb = book(&[("A1", "5"), ("B1", "=A1*2"), ("C1", "=SUM(A1:B1)")]);
    wb.move_range(0, range("A1"), cell("A3")).unwrap();
    assert_eq!(wb.input(0, cell("B1")), "=A3*2");
    assert_eq!(shown(&wb, "B1"), "10");
    assert!(wb.value(0, cell("A1")).is_empty());
}

#[test]
fn entries_are_typed_like_excel() {
    let wb = book(&[
        ("A1", "1,234.50"),
        ("A2", "15%"),
        ("A3", "$9.99"),
        ("A4", "2026-09-18"),
        ("A5", "'00123"),
        ("A6", "true"),
        ("A7", "#N/A"),
        ("A8", "(42)"),
        ("A9", "1:30 PM"),
    ]);
    assert_eq!(wb.value(0, cell("A1")), Value::Number(1234.5));
    assert_eq!(shown(&wb, "A1"), "1,234.50");
    assert_eq!(
        (wb.value(0, cell("A2")), shown(&wb, "A2")),
        (Value::Number(0.15), "15%".into())
    );
    assert_eq!(shown(&wb, "A3"), "$9.99");
    assert_eq!(shown(&wb, "A4"), "9/18/2026");
    assert_eq!(wb.value(0, cell("A5")), Value::Text("00123".into()));
    assert_eq!(wb.input(0, cell("A5")), "'00123");
    assert_eq!(wb.value(0, cell("A6")), Value::Bool(true));
    assert_eq!(wb.value(0, cell("A7")), Value::Error(ErrorKind::NA));
    assert_eq!(wb.value(0, cell("A8")), Value::Number(-42.0));
    assert_eq!(shown(&wb, "A9"), "1:30 PM");
    let mut wb = Workbook::new();
    assert!(
        wb.set_input(0, cell("A1"), "=SUM(1,").is_err(),
        "a broken formula is refused"
    );
    assert!(wb.value(0, cell("A1")).is_empty());
}

#[test]
fn sort_filter_undo_redo_and_statistics() {
    let mut wb = book(SALES);
    wb.set_input(0, cell("E2"), "=C2*D2").unwrap();
    wb.fill(0, range("E2"), range("E2:E6")).unwrap();
    wb.sort(0, range("A1:E6"), 1, false, true).unwrap();
    let reps: Vec<String> = (2..=6).map(|r| shown(&wb, &format!("B{r}"))).collect();
    assert_eq!(reps, ["Ed", "Di", "Cy", "Bob", "Ada"]);
    // Formulas moved with their rows and still read their own row.
    assert_eq!(wb.input(0, cell("E2")), "=C2*D2");
    assert_eq!(shown(&wb, "E2"), "15");
    assert!(wb.undo());
    assert_eq!(shown(&wb, "B2"), "Ada");
    assert!(wb.redo());
    assert_eq!(shown(&wb, "B2"), "Ed");
    wb.set_filter(0, Some(range("A1:E6"))).unwrap();
    assert_eq!(wb.filter_values(0, 0), ["East", "North", "South"]);
    wb.filter_toggle(0, 0, "North").unwrap();
    assert_eq!(wb.sheets[0].hidden_rows().len(), 2);
    // Status-bar statistics skip rows the filter hides.
    let st = wb.stats(0, range("C2:C6"));
    assert_eq!((st.count, st.sum), (2, 16.0));
    wb.filter_toggle(0, 0, "North").unwrap();
    let st = wb.stats(0, range("A1:D6"));
    assert_eq!((st.count, st.numbers, st.sum), (23, 9, 57.75));
    assert_eq!(st.average, Some(57.75 / 9.0));
    // Typing is undoable too, one step per edit.
    let mut wb = book(&[("A1", "1")]);
    wb.set_input(0, cell("A1"), "2").unwrap();
    wb.set_input(0, cell("B1"), "=A1+1").unwrap();
    assert!(wb.undo());
    assert!(wb.value(0, cell("B1")).is_empty());
    assert!(wb.undo());
    assert_eq!(shown(&wb, "A1"), "1");
    assert!(wb.redo() && wb.redo());
    assert_eq!(shown(&wb, "B1"), "3");
    assert!(!wb.redo());
}

#[test]
fn charts_read_their_ranges() {
    let mut wb = book(&[
        ("A1", "Month"),
        ("B1", "Sales"),
        ("C1", "Costs"),
        ("A2", "Jan"),
        ("B2", "10"),
        ("C2", "7"),
        ("A3", "Feb"),
        ("B3", "12"),
        ("C3", "8"),
        ("A4", "Mar"),
        ("B4", "9"),
        ("C4", ""),
    ]);
    let i = wb
        .add_chart(0, ChartKind::Line, range("A1:C4"), "Q1")
        .unwrap();
    let data = wb.chart_data(0, &wb.sheets[0].charts[i].clone());
    assert_eq!(data.categories, ["Jan", "Feb", "Mar"]);
    assert_eq!(data.series.len(), 2);
    assert_eq!(data.series[0].name, "Sales");
    assert_eq!(data.series[0].values, [Some(10.0), Some(12.0), Some(9.0)]);
    assert_eq!(data.series[1].values, [Some(7.0), Some(8.0), None]);
    // Inserting a row inside the data grows the chart's range with it.
    wb.insert_rows(0, 2, 1).unwrap();
    assert_eq!(wb.sheets[0].charts[0].range, range("A1:C5"));
    wb.set_chart_kind(0, 0, ChartKind::Pie).unwrap();
    assert_eq!(wb.sheets[0].charts[0].kind, ChartKind::Pie);
}

#[test]
fn a_workbook_survives_its_own_serialisation() {
    let mut wb = book(SALES);
    wb.set_input(0, cell("E2"), "=SUM(C2:C6)").unwrap();
    wb.set_input(0, cell("B1"), "Rep name").unwrap();
    let json = serde_json::to_string(&wb).unwrap();
    let back: Workbook = serde_json::from_str(&json).unwrap();
    assert_eq!(back, wb);
    // The dependency graph is rebuilt from the formulas after loading.
    let mut back = back;
    back.set_input(0, cell("C2"), "100").unwrap();
    assert_eq!(back.display(0, cell("E2")), "123");
}
