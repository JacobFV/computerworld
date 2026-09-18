//! Office Open XML workbooks (`.xlsx`): a ZIP of SpreadsheetML parts. Writing produces
//! what Excel needs to open a file without repair (content types, relationships, a
//! styles part with number formats, fonts and fills, shared strings, sheets with
//! formulas and their cached values, frozen panes, column widths, auto filters,
//! defined names, and DrawingML charts). Reading takes the same parts from files
//! Excel, Numbers and LibreOffice write, including shared formulas and rich text.
use crate::address::{Cell, CellRef, Range};
use crate::parser::{self, Expr, Formula};
use crate::value::{ErrorKind, Value};
use crate::workbook::{
    Align, AutoFilter, Chart, ChartKind, Input, Sheet, Style, Workbook, DEFAULT_COL_WIDTH,
};
use crate::xml::{escape, parse, Element};
use std::collections::BTreeMap;

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
/// Functions Excel stores with the `_xlfn.` prefix because they postdate the format.
const FUTURE: &[&str] = &[
    "XLOOKUP",
    "XMATCH",
    "IFS",
    "SWITCH",
    "CONCAT",
    "TEXTJOIN",
    "MAXIFS",
    "MINIFS",
    "STDEV.S",
    "STDEV.P",
    "VAR.S",
    "VAR.P",
    "MODE.SNGL",
    "PERCENTILE.INC",
    "QUARTILE.INC",
    "RANK.EQ",
    "CEILING.MATH",
    "FLOOR.MATH",
    "IFNA",
    "DAYS",
    "UNICHAR",
    "UNICODE",
    "XOR",
];

fn future_names(e: &Expr, add: bool) -> Expr {
    let rec = |x: &Expr| future_names(x, add);
    match e {
        Expr::Call(name, args) => {
            let base = name
                .trim_start_matches("_XLFN.")
                .trim_start_matches("_xlfn.")
                .trim_start_matches("_XLWS.")
                .to_owned();
            let base = base.to_ascii_uppercase();
            let n = if add && FUTURE.contains(&base.as_str()) {
                format!("_xlfn.{base}")
            } else {
                base
            };
            Expr::Call(n, args.iter().map(rec).collect())
        }
        Expr::Neg(a) => Expr::Neg(Box::new(rec(a))),
        Expr::Plus(a) => Expr::Plus(Box::new(rec(a))),
        Expr::Percent(a) => Expr::Percent(Box::new(rec(a))),
        Expr::Group(a) => Expr::Group(Box::new(rec(a))),
        Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(rec(a)), Box::new(rec(b))),
        other => other.clone(),
    }
}
fn width_chars(px: u32) -> f64 {
    ((f64::from(px) - 5.0) / 7.0 * 100.0).round() / 100.0
}
fn width_px(chars: f64) -> u32 {
    (chars * 7.0 + 5.0).round().max(0.0) as u32
}
fn rgb_hex(c: [u8; 3]) -> String {
    format!("FF{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}
fn parse_rgb(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    let s = if s.len() == 8 { &s[2..] } else { s };
    if s.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}
fn abs_range(sheet: &str, r: Range) -> String {
    let abs = |c: Cell| {
        CellRef {
            row: c.row,
            col: c.col,
            row_abs: true,
            col_abs: true,
        }
        .a1()
    };
    if r.is_single() {
        format!("{}!{}", parser::quote_sheet(sheet), abs(r.start))
    } else {
        format!(
            "{}!{}:{}",
            parser::quote_sheet(sheet),
            abs(r.start),
            abs(r.end)
        )
    }
}
/// `Sheet1!$A$1:$B$2` → (sheet, range).
fn parse_ref(text: &str) -> Option<(String, Range)> {
    let (sheet, body) = text.rsplit_once('!')?;
    let sheet = sheet.trim_matches('\'').replace("''", "'");
    Some((sheet, Range::parse(body)?))
}

struct Styles {
    xfs: Vec<Style>,
}
impl Styles {
    fn index(&mut self, s: &Style) -> usize {
        match self.xfs.iter().position(|x| x == s) {
            Some(i) => i,
            None => {
                self.xfs.push(s.clone());
                self.xfs.len() - 1
            }
        }
    }
    fn xml(&self) -> String {
        let mut numfmts: Vec<String> = Vec::new();
        let mut fonts: Vec<(bool, bool, bool, Option<[u8; 3]>)> = vec![(false, false, false, None)];
        let mut fills: Vec<Option<[u8; 3]>> = vec![None, None];
        let mut xf = String::new();
        for s in &self.xfs {
            let fmt_id = match crate::format::builtin_id(&s.format) {
                Some(id) => id,
                None => {
                    let i = match numfmts.iter().position(|f| *f == s.format) {
                        Some(i) => i,
                        None => {
                            numfmts.push(s.format.clone());
                            numfmts.len() - 1
                        }
                    };
                    164 + i as u32
                }
            };
            let font = (s.bold, s.italic, s.underline, s.color);
            let font_id = match fonts.iter().position(|f| *f == font) {
                Some(i) => i,
                None => {
                    fonts.push(font);
                    fonts.len() - 1
                }
            };
            let fill_id = match s.fill {
                None => 0,
                Some(c) => match fills.iter().skip(2).position(|f| *f == Some(c)) {
                    Some(i) => i + 2,
                    None => {
                        fills.push(Some(c));
                        fills.len() - 1
                    }
                },
            };
            let align = match s.align {
                Align::General => None,
                Align::Left => Some("left"),
                Align::Center => Some("center"),
                Align::Right => Some("right"),
            };
            xf.push_str(&format!(
                "<xf numFmtId=\"{fmt_id}\" fontId=\"{font_id}\" fillId=\"{fill_id}\" borderId=\"0\" xfId=\"0\"{}{}{}{}",
                if fmt_id != 0 { " applyNumberFormat=\"1\"" } else { "" },
                if font_id != 0 { " applyFont=\"1\"" } else { "" },
                if fill_id != 0 { " applyFill=\"1\"" } else { "" },
                match align {
                    Some(a) => format!(" applyAlignment=\"1\"><alignment horizontal=\"{a}\"/></xf>"),
                    None => "/>".into(),
                }
            ));
        }
        let mut out = String::from(HEAD);
        out.push_str(&format!("<styleSheet xmlns=\"{MAIN}\">"));
        if !numfmts.is_empty() {
            out.push_str(&format!("<numFmts count=\"{}\">", numfmts.len()));
            for (i, f) in numfmts.iter().enumerate() {
                out.push_str(&format!(
                    "<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>",
                    164 + i,
                    escape(f)
                ));
            }
            out.push_str("</numFmts>");
        }
        out.push_str(&format!("<fonts count=\"{}\">", fonts.len()));
        for (b, i, u, c) in &fonts {
            out.push_str("<font>");
            if *b {
                out.push_str("<b/>");
            }
            if *i {
                out.push_str("<i/>");
            }
            if *u {
                out.push_str("<u/>");
            }
            out.push_str("<sz val=\"11\"/>");
            match c {
                Some(c) => out.push_str(&format!("<color rgb=\"{}\"/>", rgb_hex(*c))),
                None => out.push_str("<color theme=\"1\"/>"),
            }
            out.push_str(
                "<name val=\"Calibri\"/><family val=\"2\"/><scheme val=\"minor\"/></font>",
            );
        }
        out.push_str("</fonts>");
        out.push_str(&format!("<fills count=\"{}\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill>", fills.len()));
        for c in fills.iter().skip(2).flatten() {
            out.push_str(&format!("<fill><patternFill patternType=\"solid\"><fgColor rgb=\"{}\"/><bgColor indexed=\"64\"/></patternFill></fill>", rgb_hex(*c)));
        }
        out.push_str("</fills>");
        out.push_str("<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>");
        out.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
        out.push_str(&format!(
            "<cellXfs count=\"{}\">{xf}</cellXfs>",
            self.xfs.len()
        ));
        out.push_str("<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles>");
        out.push_str("<dxfs count=\"0\"/><tableStyles count=\"0\" defaultTableStyle=\"TableStyleMedium2\" defaultPivotStyle=\"PivotStyleLight16\"/>");
        out.push_str("</styleSheet>");
        out
    }
}
fn iso_datetime(serial: f64) -> String {
    let (y, m, d) = crate::date::ymd(serial).unwrap_or((1980, 1, 1));
    let (h, mi, s) = crate::date::hms(serial);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}
fn chart_xml(wb: &Workbook, sheet: usize, chart: &Chart) -> String {
    let name = &wb.sheets[sheet].name;
    let (header, labels, by_cols) = wb.chart_layout(sheet, chart);
    let r = chart.range;
    let data_rows = (r.start.row + u32::from(header), r.end.row);
    let data_cols = (r.start.col + u32::from(labels), r.end.col);
    let mut sers = String::new();
    let lines: Vec<u32> = if by_cols {
        (data_cols.0..=data_cols.1).collect()
    } else {
        (data_rows.0..=data_rows.1).collect()
    };
    for (i, line) in lines.iter().enumerate() {
        let (tx, cat, val) = if by_cols {
            (
                header.then(|| Range::single(Cell::new(r.start.row, *line))),
                labels.then(|| {
                    Range::new(
                        Cell::new(data_rows.0, r.start.col),
                        Cell::new(data_rows.1, r.start.col),
                    )
                }),
                Range::new(Cell::new(data_rows.0, *line), Cell::new(data_rows.1, *line)),
            )
        } else {
            (
                labels.then(|| Range::single(Cell::new(*line, r.start.col))),
                header.then(|| {
                    Range::new(
                        Cell::new(r.start.row, data_cols.0),
                        Cell::new(r.start.row, data_cols.1),
                    )
                }),
                Range::new(Cell::new(*line, data_cols.0), Cell::new(*line, data_cols.1)),
            )
        };
        sers.push_str(&format!(
            "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>"
        ));
        if let Some(t) = tx {
            sers.push_str(&format!(
                "<c:tx><c:strRef><c:f>{}</c:f></c:strRef></c:tx>",
                escape(&abs_range(name, t))
            ));
        }
        if chart.kind == ChartKind::Line {
            sers.push_str("<c:marker><c:symbol val=\"none\"/></c:marker>");
        }
        if let Some(c) = cat {
            sers.push_str(&format!(
                "<c:cat><c:strRef><c:f>{}</c:f></c:strRef></c:cat>",
                escape(&abs_range(name, c))
            ));
        }
        sers.push_str(&format!(
            "<c:val><c:numRef><c:f>{}</c:f></c:numRef></c:val>",
            escape(&abs_range(name, val))
        ));
        if chart.kind == ChartKind::Line {
            sers.push_str("<c:smooth val=\"0\"/>");
        }
        sers.push_str("</c:ser>");
    }
    let axes = "<c:axId val=\"500000001\"/><c:axId val=\"500000002\"/>";
    let plot = match chart.kind {
        ChartKind::Column | ChartKind::Bar => format!(
            "<c:barChart><c:barDir val=\"{}\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>{sers}<c:gapWidth val=\"219\"/>{axes}</c:barChart>",
            if chart.kind == ChartKind::Bar { "bar" } else { "col" }
        ),
        ChartKind::Line => format!("<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>{sers}<c:marker val=\"1\"/>{axes}</c:lineChart>"),
        ChartKind::Pie => format!("<c:pieChart><c:varyColors val=\"1\"/>{sers}<c:firstSliceAng val=\"0\"/></c:pieChart>"),
    };
    let axis_xml = if chart.kind == ChartKind::Pie {
        String::new()
    } else {
        let (cat_pos, val_pos) = if chart.kind == ChartKind::Bar {
            ("l", "b")
        } else {
            ("b", "l")
        };
        format!(
            "<c:catAx><c:axId val=\"500000001\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{cat_pos}\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"500000002\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/></c:catAx>\
             <c:valAx><c:axId val=\"500000002\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{val_pos}\"/><c:majorGridlines/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"500000001\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"between\"/></c:valAx>"
        )
    };
    let title = if chart.title.is_empty() {
        "<c:autoTitleDeleted val=\"1\"/>".to_string()
    } else {
        format!(
            "<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr/></a:pPr><a:r><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>",
            escape(&chart.title)
        )
    };
    format!(
        "{HEAD}<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"{REL}\"><c:roundedCorners val=\"0\"/><c:chart>{title}<c:plotArea><c:layout/>{plot}{axis_xml}</c:plotArea><c:legend><c:legendPos val=\"{}\"/><c:overlay val=\"0\"/></c:legend><c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart></c:chartSpace>",
        if chart.kind == ChartKind::Pie { "r" } else { "b" }
    )
}
fn drawing_xml(charts: &[(usize, &Chart)]) -> String {
    let mut out = format!(
        "{HEAD}<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">"
    );
    for (i, (_, c)) in charts.iter().enumerate() {
        out.push_str(&format!(
            "<xdr:twoCellAnchor editAs=\"oneCell\"><xdr:from><xdr:col>{}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>{}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>\
             <xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{}\" name=\"Chart {}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/></xdr:xfrm>\
             <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:r=\"{REL}\" r:id=\"rId{}\"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor>",
            c.anchor.col,
            c.anchor.row,
            c.anchor.col + c.cols,
            c.anchor.row + c.rows,
            i + 2,
            i + 1,
            i + 1
        ));
    }
    out.push_str("</xdr:wsDr>");
    out
}

/// The workbook as `.xlsx` bytes. `application` names the program in the document
/// properties, as each spreadsheet application signs its own files.
pub fn write(wb: &Workbook, application: &str) -> Vec<u8> {
    let mut styles = Styles {
        xfs: vec![Style::default()],
    };
    let mut strings: Vec<String> = Vec::new();
    let mut string_index: BTreeMap<String, usize> = BTreeMap::new();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut overrides = String::new();
    let mut chart_no = 0;
    let mut drawing_no = 0;
    let mut sheet_parts = Vec::new();
    for (si, sheet) in wb.sheets.iter().enumerate() {
        let mut x = format!("{HEAD}<worksheet xmlns=\"{MAIN}\" xmlns:r=\"{REL}\">");
        let used = wb.used_range(si);
        x.push_str(&format!(
            "<dimension ref=\"{}\"/>",
            used.map_or("A1".into(), |r| r.a1())
        ));
        x.push_str("<sheetViews><sheetView workbookViewId=\"0\"");
        if si == 0 {
            x.push_str(" tabSelected=\"1\"");
        }
        let (fr, fc) = sheet.freeze;
        if fr > 0 || fc > 0 {
            let pane = match (fr > 0, fc > 0) {
                (true, true) => "bottomRight",
                (true, false) => "bottomLeft",
                _ => "topRight",
            };
            x.push_str(&format!(
                "><pane{}{} topLeftCell=\"{}\" activePane=\"{pane}\" state=\"frozen\"/><selection pane=\"{pane}\"/></sheetView></sheetViews>",
                if fc > 0 { format!(" xSplit=\"{fc}\"") } else { String::new() },
                if fr > 0 { format!(" ySplit=\"{fr}\"") } else { String::new() },
                Cell::new(fr, fc).a1()
            ));
        } else {
            x.push_str("/></sheetViews>");
        }
        x.push_str("<sheetFormatPr defaultRowHeight=\"15\"/>");
        if !sheet.col_widths.is_empty() {
            x.push_str("<cols>");
            for (c, px) in &sheet.col_widths {
                x.push_str(&format!(
                    "<col min=\"{0}\" max=\"{0}\" width=\"{1}\" customWidth=\"1\"/>",
                    c + 1,
                    width_chars(*px)
                ));
            }
            x.push_str("</cols>");
        }
        x.push_str("<sheetData>");
        let hidden = sheet.hidden_rows();
        let mut row: Option<u32> = None;
        for (c, d) in &sheet.cells {
            if row != Some(c.row) {
                if row.is_some() {
                    x.push_str("</row>");
                }
                x.push_str(&format!(
                    "<row r=\"{}\"{}>",
                    c.row + 1,
                    if hidden.contains(&c.row) {
                        " hidden=\"1\""
                    } else {
                        ""
                    }
                ));
                row = Some(c.row);
            }
            let s = styles.index(&d.style);
            let s_attr = if s == 0 {
                String::new()
            } else {
                format!(" s=\"{s}\"")
            };
            let r = c.a1();
            match &d.input {
                Input::Formula(f) => {
                    let text = parser::print(&future_names(&f.expr, true));
                    let (t, v) = match &d.value {
                        Value::Number(n) => ("", crate::value::general(*n)),
                        Value::Text(s) => (" t=\"str\"", escape(s)),
                        Value::Bool(b) => (" t=\"b\"", if *b { "1" } else { "0" }.into()),
                        Value::Error(k) => (
                            " t=\"e\"",
                            escape(if *k == ErrorKind::Circular {
                                "#REF!"
                            } else {
                                k.code()
                            }),
                        ),
                        Value::Empty => ("", String::new()),
                    };
                    let v = if matches!(d.value, Value::Number(_)) {
                        number_text(&d.value)
                    } else {
                        v
                    };
                    x.push_str(&format!(
                        "<c r=\"{r}\"{s_attr}{t}><f>{}</f><v>{v}</v></c>",
                        escape(&text)
                    ));
                }
                Input::Value(Value::Empty) => x.push_str(&format!("<c r=\"{r}\"{s_attr}/>")),
                Input::Value(Value::Number(_)) => x.push_str(&format!(
                    "<c r=\"{r}\"{s_attr}><v>{}</v></c>",
                    number_text(&d.value)
                )),
                Input::Value(Value::Text(t)) => {
                    let idx = *string_index.entry(t.clone()).or_insert_with(|| {
                        strings.push(t.clone());
                        strings.len() - 1
                    });
                    x.push_str(&format!("<c r=\"{r}\"{s_attr} t=\"s\"><v>{idx}</v></c>"));
                }
                Input::Value(Value::Bool(b)) => x.push_str(&format!(
                    "<c r=\"{r}\"{s_attr} t=\"b\"><v>{}</v></c>",
                    u8::from(*b)
                )),
                Input::Value(Value::Error(k)) => x.push_str(&format!(
                    "<c r=\"{r}\"{s_attr} t=\"e\"><v>{}</v></c>",
                    escape(k.code())
                )),
            }
        }
        if row.is_some() {
            x.push_str("</row>");
        }
        x.push_str("</sheetData>");
        if let Some(f) = &sheet.filter {
            x.push_str(&format!("<autoFilter ref=\"{}\"", f.range.a1()));
            if f.hidden.is_empty() {
                x.push_str("/>");
            } else {
                x.push('>');
                for (col, hidden) in &f.hidden {
                    let shown: Vec<String> = wb
                        .filter_values(si, *col)
                        .into_iter()
                        .filter(|v| !hidden.contains(v))
                        .collect();
                    x.push_str(&format!(
                        "<filterColumn colId=\"{}\"><filters>",
                        col - f.range.start.col
                    ));
                    for s in shown {
                        x.push_str(&format!("<filter val=\"{}\"/>", escape(&s)));
                    }
                    x.push_str("</filters></filterColumn>");
                }
                x.push_str("</autoFilter>");
            }
        }
        x.push_str("<pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/>");
        let mut sheet_rels = String::new();
        if !sheet.charts.is_empty() {
            drawing_no += 1;
            x.push_str("<drawing r:id=\"rId1\"/>");
            sheet_rels = format!(
                "{HEAD}<Relationships xmlns=\"{PKG_REL}\"><Relationship Id=\"rId1\" Type=\"{REL}/drawing\" Target=\"../drawings/drawing{drawing_no}.xml\"/></Relationships>"
            );
            let charts: Vec<(usize, &Chart)> = sheet.charts.iter().enumerate().collect();
            parts.push((
                format!("xl/drawings/drawing{drawing_no}.xml"),
                drawing_xml(&charts).into_bytes(),
            ));
            overrides.push_str(&format!("<Override PartName=\"/xl/drawings/drawing{drawing_no}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawing+xml\"/>"));
            let mut drels = format!("{HEAD}<Relationships xmlns=\"{PKG_REL}\">");
            for (i, c) in sheet.charts.iter().enumerate() {
                chart_no += 1;
                drels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"{REL}/chart\" Target=\"../charts/chart{chart_no}.xml\"/>", i + 1));
                parts.push((
                    format!("xl/charts/chart{chart_no}.xml"),
                    chart_xml(wb, si, c).into_bytes(),
                ));
                overrides.push_str(&format!("<Override PartName=\"/xl/charts/chart{chart_no}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.chart+xml\"/>"));
            }
            drels.push_str("</Relationships>");
            parts.push((
                format!("xl/drawings/_rels/drawing{drawing_no}.xml.rels"),
                drels.into_bytes(),
            ));
        }
        x.push_str("</worksheet>");
        sheet_parts.push((format!("xl/worksheets/sheet{}.xml", si + 1), x.into_bytes()));
        if !sheet_rels.is_empty() {
            sheet_parts.push((
                format!("xl/worksheets/_rels/sheet{}.xml.rels", si + 1),
                sheet_rels.into_bytes(),
            ));
        }
        overrides.push_str(&format!("<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>", si + 1));
    }
    let n = wb.sheets.len();
    let mut book = format!("{HEAD}<workbook xmlns=\"{MAIN}\" xmlns:r=\"{REL}\"><bookViews><workbookView activeTab=\"0\"/></bookViews><sheets>");
    for (i, s) in wb.sheets.iter().enumerate() {
        book.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>",
            escape(&s.name),
            i + 1,
            i + 1
        ));
    }
    book.push_str("</sheets>");
    let mut defined = Vec::new();
    for (name, (sheet, range)) in &wb.names {
        defined.push(format!(
            "<definedName name=\"{}\">{}</definedName>",
            escape(name),
            escape(&abs_range(sheet, *range))
        ));
    }
    for (i, s) in wb.sheets.iter().enumerate() {
        if let Some(f) = &s.filter {
            defined.push(format!("<definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"{i}\" hidden=\"1\">{}</definedName>", escape(&abs_range(&s.name, f.range))));
        }
    }
    if !defined.is_empty() {
        book.push_str(&format!(
            "<definedNames>{}</definedNames>",
            defined.join("")
        ));
    }
    book.push_str("<calcPr calcId=\"191029\" fullCalcOnLoad=\"1\"/></workbook>");
    let mut rels = format!("{HEAD}<Relationships xmlns=\"{PKG_REL}\">");
    for i in 0..n {
        rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"{REL}/worksheet\" Target=\"worksheets/sheet{}.xml\"/>", i + 1, i + 1));
    }
    rels.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"{REL}/styles\" Target=\"styles.xml\"/>",
        n + 1
    ));
    rels.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"{REL}/sharedStrings\" Target=\"sharedStrings.xml\"/>",
        n + 2
    ));
    rels.push_str("</Relationships>");
    let mut sst = format!(
        "{HEAD}<sst xmlns=\"{MAIN}\" count=\"{0}\" uniqueCount=\"{0}\">",
        strings.len()
    );
    for s in &strings {
        sst.push_str(&format!(
            "<si><t xml:space=\"preserve\">{}</t></si>",
            escape(s)
        ));
    }
    sst.push_str("</sst>");
    let stamp = iso_datetime(wb.now());
    let content_types = format!(
        "{HEAD}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>{overrides}\
         <Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/><Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/>\
         <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>"
    );
    let root_rels = format!(
        "{HEAD}<Relationships xmlns=\"{PKG_REL}\"><Relationship Id=\"rId1\" Type=\"{REL}/officeDocument\" Target=\"xl/workbook.xml\"/><Relationship Id=\"rId2\" Type=\"{PKG_REL}/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"{REL}/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>"
    );
    let core = format!(
        "{HEAD}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dcterms:created xsi:type=\"dcterms:W3CDTF\">{stamp}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{stamp}</dcterms:modified></cp:coreProperties>"
    );
    let app = format!(
        "{HEAD}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>{}</Application></Properties>",
        escape(application)
    );
    let mut entries = vec![
        (
            "[Content_Types].xml".to_string(),
            content_types.into_bytes(),
        ),
        ("_rels/.rels".to_string(), root_rels.into_bytes()),
        ("docProps/app.xml".to_string(), app.into_bytes()),
        ("docProps/core.xml".to_string(), core.into_bytes()),
        ("xl/workbook.xml".to_string(), book.into_bytes()),
        ("xl/_rels/workbook.xml.rels".to_string(), rels.into_bytes()),
    ];
    entries.extend(sheet_parts);
    entries.extend(parts);
    entries.push(("xl/styles.xml".to_string(), styles.xml().into_bytes()));
    entries.push(("xl/sharedStrings.xml".to_string(), sst.into_bytes()));
    let (y, m, d) = crate::date::ymd(wb.now()).unwrap_or((1980, 1, 1));
    let (h, mi, s) = crate::date::hms(wb.now());
    crate::zip::write(&entries, crate::zip::dos_time(y, m, d, h, mi, s))
}
fn number_text(v: &Value) -> String {
    match v {
        // Seventeen significant digits, so the double survives the round trip exactly.
        Value::Number(n) => {
            let short = cw_determinism::math::format_significant(*n, 15);
            if short.parse::<f64>().ok() == Some(*n) {
                short.replace('e', "E")
            } else {
                cw_determinism::math::format_significant(*n, 17).replace('e', "E")
            }
        }
        _ => String::new(),
    }
}

// ----- reading -----

fn part<'a>(parts: &'a BTreeMap<String, Vec<u8>>, name: &str) -> Option<&'a [u8]> {
    parts.get(name.trim_start_matches('/')).map(Vec::as_slice)
}
fn xml_part(parts: &BTreeMap<String, Vec<u8>>, name: &str) -> Result<Option<Element>, String> {
    match part(parts, name) {
        Some(b) => parse(&String::from_utf8_lossy(b))
            .map(Some)
            .map_err(|e| format!("{name}: {e}")),
        None => Ok(None),
    }
}
/// Resolve a relationship target against the part that holds it.
fn resolve(base: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_owned();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}
fn rels_of(
    parts: &BTreeMap<String, Vec<u8>>,
    owner: &str,
) -> Result<BTreeMap<String, String>, String> {
    let (dir, file) = owner.rsplit_once('/').unwrap_or(("", owner));
    let name = if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    };
    let mut out = BTreeMap::new();
    if let Some(root) = xml_part(parts, &name)? {
        for r in root.children_named("Relationship") {
            if let (Some(id), Some(t)) = (r.attr("Id"), r.attr("Target")) {
                out.insert(id.to_owned(), resolve(owner, t));
            }
        }
    }
    Ok(out)
}
struct ReadStyles {
    xfs: Vec<Style>,
}
fn read_styles(root: Option<Element>) -> ReadStyles {
    let Some(root) = root else {
        return ReadStyles {
            xfs: vec![Style::default()],
        };
    };
    let mut custom = BTreeMap::new();
    if let Some(nf) = root.child("numFmts") {
        for f in nf.children_named("numFmt") {
            if let (Some(id), Some(code)) = (
                f.attr("numFmtId").and_then(|i| i.parse::<u32>().ok()),
                f.attr("formatCode"),
            ) {
                custom.insert(id, code.to_owned());
            }
        }
    }
    let fonts: Vec<(bool, bool, bool, Option<[u8; 3]>)> = root
        .child("fonts")
        .map(|f| {
            f.children_named("font")
                .map(|font| {
                    let on = |n: &str| {
                        font.child(n).is_some_and(|e| {
                            e.attr("val") != Some("0") && e.attr("val") != Some("false")
                        })
                    };
                    let color = font
                        .child("color")
                        .and_then(|c| c.attr("rgb"))
                        .and_then(parse_rgb)
                        .filter(|c| *c != [0, 0, 0]);
                    (
                        on("b"),
                        on("i"),
                        font.child("u")
                            .is_some_and(|u| u.attr("val") != Some("none")),
                        color,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let fills: Vec<Option<[u8; 3]>> = root
        .child("fills")
        .map(|f| {
            f.children_named("fill")
                .map(|fill| {
                    let p = fill.child("patternFill")?;
                    if p.attr("patternType") != Some("solid") {
                        return None;
                    }
                    p.child("fgColor")
                        .and_then(|c| c.attr("rgb"))
                        .and_then(parse_rgb)
                })
                .collect()
        })
        .unwrap_or_default();
    let mut xfs = Vec::new();
    if let Some(cx) = root.child("cellXfs") {
        for xf in cx.children_named("xf") {
            let num: u32 = xf
                .attr("numFmtId")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let format = custom
                .get(&num)
                .cloned()
                .or_else(|| crate::format::builtin(num).map(str::to_owned))
                .unwrap_or_else(|| "General".into());
            let font = xf
                .attr("fontId")
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|i| fonts.get(i).copied())
                .unwrap_or_default();
            let fill = xf
                .attr("fillId")
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|i| fills.get(i).copied())
                .flatten();
            let align = match xf.child("alignment").and_then(|a| a.attr("horizontal")) {
                Some("left") => Align::Left,
                Some("center") | Some("centerContinuous") => Align::Center,
                Some("right") => Align::Right,
                _ => Align::General,
            };
            xfs.push(Style {
                format,
                bold: font.0,
                italic: font.1,
                underline: font.2,
                align,
                fill,
                color: font.3,
            });
        }
    }
    if xfs.is_empty() {
        xfs.push(Style::default());
    }
    ReadStyles { xfs }
}
/// Read an `.xlsx` workbook.
pub fn read(bytes: &[u8]) -> Result<Workbook, String> {
    let parts: BTreeMap<String, Vec<u8>> = crate::zip::read(bytes)?.into_iter().collect();
    let root_rels = rels_of(&parts, "")?;
    let book_path = xml_part(&parts, "_rels/.rels")?
        .and_then(|r| {
            r.children_named("Relationship")
                .find(|x| {
                    x.attr("Type")
                        .is_some_and(|t| t.ends_with("/officeDocument"))
                })
                .and_then(|x| {
                    x.attr("Target")
                        .map(|t| t.trim_start_matches('/').to_owned())
                })
        })
        .or_else(|| {
            root_rels
                .values()
                .find(|t| t.ends_with("workbook.xml"))
                .cloned()
        })
        .unwrap_or_else(|| "xl/workbook.xml".into());
    let book = xml_part(&parts, &book_path)?.ok_or("the file has no workbook part")?;
    let book_rels = rels_of(&parts, &book_path)?;
    let strings: Vec<String> = match book_rels
        .values()
        .find(|t| t.ends_with("sharedStrings.xml"))
    {
        Some(p) => xml_part(&parts, p)?
            .map(|sst| sst.children_named("si").map(shared_string).collect())
            .unwrap_or_default(),
        None => vec![],
    };
    let styles = read_styles(
        match book_rels.values().find(|t| t.ends_with("styles.xml")) {
            Some(p) => xml_part(&parts, p)?,
            None => None,
        },
    );
    let sheets_el = book.child("sheets").ok_or("the workbook lists no sheets")?;
    let mut wb = Workbook::new();
    wb.sheets.clear();
    let mut sheet_paths = Vec::new();
    for s in sheets_el.children_named("sheet") {
        let name = s.attr("name").unwrap_or("Sheet").to_owned();
        let rid = s.attr_exact("r:id").or_else(|| s.attr("id")).unwrap_or("");
        let path = book_rels
            .get(rid)
            .cloned()
            .ok_or_else(|| format!("sheet {name} has no part"))?;
        wb.sheets.push(Sheet::new(name));
        sheet_paths.push(path);
    }
    if wb.sheets.is_empty() {
        return Err("the workbook has no sheets".into());
    }
    if let Some(dn) = book.child("definedNames") {
        for d in dn.children_named("definedName") {
            let name = d.attr("name").unwrap_or("");
            if name.starts_with("_xlnm.") || name.is_empty() {
                continue;
            }
            if let Some((sheet, range)) = parse_ref(&d.text()) {
                if wb.sheet_index(&sheet).is_some() {
                    wb.names.insert(name.to_owned(), (sheet, range));
                }
            }
        }
    }
    for (si, path) in sheet_paths.iter().enumerate() {
        let Some(ws) = xml_part(&parts, path)? else {
            continue;
        };
        read_sheet(&mut wb, si, &ws, &strings, &styles)?;
        // Charts, through the sheet's drawing.
        let rels = rels_of(&parts, path)?;
        if let Some(drawing) = ws.child("drawing") {
            let rid = drawing
                .attr_exact("r:id")
                .or_else(|| drawing.attr("id"))
                .unwrap_or("");
            if let Some(dpath) = rels.get(rid) {
                read_charts(&mut wb, si, &parts, dpath)?;
            }
        }
    }
    Ok(wb.loaded())
}
fn shared_string(si: &Element) -> String {
    // Plain <t>, or rich-text runs <r><t>; phonetic runs (<rPh>) are not the text.
    let mut out = String::new();
    for e in si.elements() {
        match e.local() {
            "t" => out.push_str(&e.text()),
            "r" => {
                if let Some(t) = e.child("t") {
                    out.push_str(&t.text());
                }
            }
            _ => {}
        }
    }
    out
}
fn read_sheet(
    wb: &mut Workbook,
    si: usize,
    ws: &Element,
    strings: &[String],
    styles: &ReadStyles,
) -> Result<(), String> {
    if let Some(pane) = ws
        .child("sheetViews")
        .and_then(|v| v.child("sheetView"))
        .and_then(|v| v.child("pane"))
    {
        if matches!(pane.attr("state"), Some("frozen") | Some("frozenSplit")) {
            let x = pane
                .attr("xSplit")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0) as u32;
            let y = pane
                .attr("ySplit")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0) as u32;
            wb.sheets[si].freeze = (y, x);
        }
    }
    if let Some(cols) = ws.child("cols") {
        for c in cols.children_named("col") {
            let (Some(min), Some(max)) = (
                c.attr("min").and_then(|v| v.parse::<u32>().ok()),
                c.attr("max").and_then(|v| v.parse::<u32>().ok()),
            ) else {
                continue;
            };
            if let Some(w) = c.attr("width").and_then(|v| v.parse::<f64>().ok()) {
                let px = width_px(w);
                if px != DEFAULT_COL_WIDTH && max - min < 256 {
                    for col in min..=max {
                        wb.sheets[si].col_widths.insert(col - 1, px);
                    }
                }
            }
        }
    }
    let mut shared: BTreeMap<String, (Cell, Formula)> = BTreeMap::new();
    let data = ws.child("sheetData");
    let mut next_row = 0u32;
    for row in data.iter().flat_map(|d| d.children_named("row")) {
        let r = row
            .attr("r")
            .and_then(|v| v.parse::<u32>().ok())
            .map_or(next_row, |r| r - 1);
        next_row = r + 1;
        let mut next_col = 0u32;
        for c in row.children_named("c") {
            let at = c
                .attr("r")
                .and_then(Cell::parse)
                .unwrap_or(Cell::new(r, next_col));
            next_col = at.col + 1;
            let style = c
                .attr("s")
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|i| styles.xfs.get(i).cloned())
                .unwrap_or_default();
            let t = c.attr("t").unwrap_or("n");
            let v = c.child("v").map(|v| v.text());
            let cached = match (t, &v) {
                ("s", Some(i)) => i
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| strings.get(i).cloned())
                    .map(Value::Text)
                    .unwrap_or_default(),
                ("str", Some(s)) => Value::Text(s.clone()),
                ("inlineStr", _) => {
                    Value::Text(c.child("is").map(shared_string).unwrap_or_default())
                }
                ("b", Some(b)) => Value::Bool(b.trim() == "1"),
                ("e", Some(e)) => {
                    Value::Error(ErrorKind::parse(e.trim()).unwrap_or(ErrorKind::Value))
                }
                ("d", Some(d)) => {
                    crate::date::parse_datetime(&d.replace('T', " ").replace('Z', ""))
                        .map_or(Value::Text(d.clone()), |x| Value::Number(x.0))
                }
                (_, Some(n)) => n
                    .trim()
                    .parse::<f64>()
                    .map(Value::number)
                    .unwrap_or_default(),
                _ => Value::Empty,
            };
            let formula = match c.child("f") {
                Some(f) => {
                    let text = f.text();
                    let parsed = if f.attr("t") == Some("shared") {
                        let si_key = f.attr("si").unwrap_or("").to_owned();
                        if !text.trim().is_empty() {
                            let fm = Formula::parse(&text).ok();
                            if let Some(fm) = &fm {
                                shared.insert(si_key, (at, fm.clone()));
                            }
                            fm
                        } else {
                            shared.get(&si_key).map(|(origin, fm)| {
                                Workbook::shift_formula(
                                    fm,
                                    i64::from(at.row) - i64::from(origin.row),
                                    i64::from(at.col) - i64::from(origin.col),
                                )
                            })
                        }
                    } else if text.trim().is_empty() {
                        None
                    } else {
                        Formula::parse(&text).ok()
                    };
                    parsed.map(|fm| Formula {
                        expr: future_names(&fm.expr, false),
                    })
                }
                None => None,
            };
            let input = match formula {
                Some(f) => Input::Formula(f),
                None => Input::Value(cached.clone()),
            };
            wb.load_cell(si, at, input, cached, style);
        }
    }
    if let Some(af) = ws.child("autoFilter") {
        if let Some(range) = af.attr("ref").and_then(Range::parse) {
            let mut filter = AutoFilter {
                range,
                hidden: BTreeMap::new(),
            };
            // Shown values listed per column; everything else in the column is hidden.
            let mut shown_by_col = BTreeMap::new();
            for fc in af.children_named("filterColumn") {
                let Some(id) = fc.attr("colId").and_then(|v| v.parse::<u32>().ok()) else {
                    continue;
                };
                if let Some(filters) = fc.child("filters") {
                    let shown: Vec<String> = filters
                        .children_named("filter")
                        .filter_map(|f| f.attr("val").map(str::to_owned))
                        .collect();
                    shown_by_col.insert(range.start.col + id, shown);
                }
            }
            wb.sheets[si].filter = Some(filter.clone());
            for (col, shown) in shown_by_col {
                let all = wb.filter_values(si, col);
                let hidden: std::collections::BTreeSet<String> =
                    all.into_iter().filter(|v| !shown.contains(v)).collect();
                if !hidden.is_empty() {
                    filter.hidden.insert(col, hidden);
                }
            }
            wb.sheets[si].filter = Some(filter);
        }
    }
    Ok(())
}
fn read_charts(
    wb: &mut Workbook,
    si: usize,
    parts: &BTreeMap<String, Vec<u8>>,
    dpath: &str,
) -> Result<(), String> {
    let Some(drawing) = xml_part(parts, dpath)? else {
        return Ok(());
    };
    let rels = rels_of(parts, dpath)?;
    for anchor in drawing
        .elements()
        .filter(|e| matches!(e.local(), "twoCellAnchor" | "oneCellAnchor"))
    {
        let pos = |which: &str| -> Option<Cell> {
            let p = anchor.child(which)?;
            Some(Cell::new(
                p.child("row")?.text().trim().parse().ok()?,
                p.child("col")?.text().trim().parse().ok()?,
            ))
        };
        let Some(chart_ref) = anchor.find("chart") else {
            continue;
        };
        let rid = chart_ref
            .attr_exact("r:id")
            .or_else(|| chart_ref.attr("id"))
            .unwrap_or("");
        let Some(cpath) = rels.get(rid) else {
            continue;
        };
        let Some(space) = xml_part(parts, cpath)? else {
            continue;
        };
        let Some(plot) = space.find("plotArea") else {
            continue;
        };
        let (kind, group) =
            if let Some(b) = plot.child("barChart").or_else(|| plot.child("bar3DChart")) {
                (
                    if b.child("barDir").and_then(|d| d.attr("val")) == Some("bar") {
                        ChartKind::Bar
                    } else {
                        ChartKind::Column
                    },
                    b,
                )
            } else if let Some(l) = plot
                .child("lineChart")
                .or_else(|| plot.child("line3DChart"))
            {
                (ChartKind::Line, l)
            } else if let Some(p) = plot
                .child("pieChart")
                .or_else(|| plot.child("doughnutChart"))
                .or_else(|| plot.child("pie3DChart"))
            {
                (ChartKind::Pie, p)
            } else {
                continue;
            };
        // The chart's data is the rectangle its series' references span.
        let mut bounds: Option<Range> = None;
        for ser in group.children_named("ser") {
            for f in ["tx", "cat", "val"]
                .iter()
                .filter_map(|k| ser.child(k))
                .filter_map(|e| e.find("f"))
            {
                if let Some((sheet, r)) = parse_ref(&f.text()) {
                    if wb.sheet_index(&sheet) == Some(si) {
                        bounds = Some(match bounds {
                            None => r,
                            Some(b) => Range::new(
                                Cell::new(
                                    b.start.row.min(r.start.row),
                                    b.start.col.min(r.start.col),
                                ),
                                Cell::new(b.end.row.max(r.end.row), b.end.col.max(r.end.col)),
                            ),
                        });
                    }
                }
            }
        }
        let Some(range) = bounds else {
            continue;
        };
        let title = space
            .find("title")
            .map(|t| t.find("rich").map(|r| r.text()).unwrap_or_default())
            .unwrap_or_default();
        let from = pos("from").unwrap_or(Cell::new(range.start.row, range.end.col + 2));
        let to = pos("to").unwrap_or(Cell::new(from.row + 15, from.col + 7));
        wb.sheets[si].charts.push(Chart {
            kind,
            range,
            title,
            anchor: from,
            cols: to.col.saturating_sub(from.col).max(2),
            rows: to.row.saturating_sub(from.row).max(4),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_workbook_survives_xlsx() {
        let mut wb = Workbook::new();
        wb.set_now(1_789_635_600_000_000);
        wb.set_input(0, Cell::new(0, 0), "Region").unwrap();
        wb.set_input(0, Cell::new(0, 1), "Sales").unwrap();
        for (i, (r, s)) in [
            ("North", "1200"),
            ("South", "950.5"),
            ("East & West", "1,300"),
        ]
        .iter()
        .enumerate()
        {
            wb.set_input(0, Cell::new(i as u32 + 1, 0), r).unwrap();
            wb.set_input(0, Cell::new(i as u32 + 1, 1), s).unwrap();
        }
        wb.set_input(0, Cell::new(4, 1), "=SUM(B2:B4)").unwrap();
        wb.set_input(0, Cell::new(5, 1), "=XLOOKUP(\"South\",A2:A4,B2:B4)")
            .unwrap();
        wb.set_input(0, Cell::new(6, 1), "=B5>3000").unwrap();
        wb.set_input(0, Cell::new(7, 1), "=1/0").unwrap();
        wb.update_style(0, Range::parse("B2:B5").unwrap(), |s| {
            s.format = "$#,##0.00".into();
            s.bold = true;
            s.fill = Some([255, 242, 204]);
        })
        .unwrap();
        let second = wb.add_sheet(Some("Q2 Plan")).unwrap();
        wb.set_input(second, Cell::new(0, 0), "=Sheet1!B5*1.1")
            .unwrap();
        wb.define_name("Total", 0, Range::parse("B5").unwrap())
            .unwrap();
        wb.set_freeze(0, 1, 0).unwrap();
        wb.set_col_width(0, 0, 120).unwrap();
        wb.add_chart(
            0,
            ChartKind::Column,
            Range::parse("A1:B4").unwrap(),
            "Sales by region",
        )
        .unwrap();
        let bytes = write(&wb, "Microsoft Excel");
        assert_eq!(
            bytes,
            write(&wb, "Microsoft Excel"),
            "the same workbook writes the same bytes"
        );
        let back = read(&bytes).unwrap();
        assert_eq!(back.sheets.len(), 2);
        assert_eq!(back.sheets[1].name, "Q2 Plan");
        assert_eq!(back.input(0, Cell::new(4, 1)), "=SUM(B2:B4)");
        assert_eq!(
            back.input(0, Cell::new(5, 1)),
            "=XLOOKUP(\"South\",A2:A4,B2:B4)"
        );
        assert_eq!(back.value(0, Cell::new(4, 1)), Value::Number(3450.5));
        assert_eq!(back.value(0, Cell::new(5, 1)), Value::Number(950.5));
        assert_eq!(back.value(0, Cell::new(6, 1)), Value::Bool(true));
        assert_eq!(
            back.value(0, Cell::new(7, 1)),
            Value::Error(ErrorKind::Div0)
        );
        assert_eq!(back.value(1, Cell::new(0, 0)), Value::Number(3450.5 * 1.1));
        assert_eq!(back.display(0, Cell::new(1, 1)), "$1,200.00");
        assert!(back.style(0, Cell::new(1, 1)).bold);
        assert_eq!(back.style(0, Cell::new(1, 1)).fill, Some([255, 242, 204]));
        assert_eq!(
            back.value(0, Cell::new(3, 0)),
            Value::Text("East & West".into())
        );
        assert_eq!(back.name("total"), Some((0, Range::parse("B5").unwrap())));
        assert_eq!(back.sheets[0].freeze, (1, 0));
        assert_eq!(back.sheets[0].col_width(0), 120);
        let chart = &back.sheets[0].charts[0];
        assert_eq!(
            (chart.kind, chart.range, chart.title.as_str()),
            (
                ChartKind::Column,
                Range::parse("A1:B4").unwrap(),
                "Sales by region"
            )
        );
    }
}
