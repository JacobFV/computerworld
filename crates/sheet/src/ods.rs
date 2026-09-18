//! OpenDocument spreadsheets (`.ods`), LibreOffice Calc's own format: a ZIP whose
//! first entry is the uncompressed `mimetype`, with the cells in `content.xml`.
//! Formulas are OpenFormula (`of:=SUM([.A1:.A3];2)`), converted to and from the
//! engine's A1 syntax; number formats become ODF data styles and back.
use crate::address::{Cell, CellRef};
use crate::parser::{self, Expr, RangeKind};
use crate::value::{ErrorKind, Value};
use crate::workbook::{Align, Input, Sheet, Style, Workbook};
use crate::xml::{escape, parse, Element};
use std::collections::BTreeMap;

const MIME: &str = "application/vnd.oasis.opendocument.spreadsheet";
const HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
const NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:number=\"urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0\" xmlns:of=\"urn:oasis:names:tc:opendocument:xmlns:of:1.2\" xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"";

// ----- OpenFormula -----

fn odf_ref(sheet: &Option<String>, c: CellRef) -> String {
    let s = match sheet {
        Some(n) => format!("${}", odf_sheet(n)),
        None => String::new(),
    };
    format!("{s}.{}", c.a1())
}
fn odf_sheet(name: &str) -> String {
    if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        name.to_owned()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}
/// A formula in OpenFormula syntax, with the `of:=` prefix.
pub fn to_openformula(e: &Expr) -> String {
    fn go(e: &Expr) -> String {
        match e {
            Expr::Ref { sheet, cell } => format!("[{}]", odf_ref(sheet, *cell)),
            Expr::Range {
                sheet,
                start,
                end,
                kind,
            } => match kind {
                RangeKind::Cells => {
                    format!("[{}:{}]", odf_ref(sheet, *start), odf_ref(&None, *end))
                }
                _ => format!("[{}:{}]", odf_ref(sheet, *start), odf_ref(&None, *end)),
            },
            Expr::Call(name, args) => format!(
                "{name}({})",
                args.iter().map(go).collect::<Vec<_>>().join(";")
            ),
            Expr::Neg(a) => format!("-{}", go(a)),
            Expr::Plus(a) => format!("+{}", go(a)),
            Expr::Percent(a) => format!("{}%", go(a)),
            Expr::Group(a) => format!("({})", go(a)),
            Expr::Bin(op, a, b) => {
                let text = parser::print(&Expr::Bin(
                    *op,
                    Box::new(Expr::Missing),
                    Box::new(Expr::Missing),
                ));
                format!("{}{text}{}", go(a), go(b))
            }
            Expr::Array(rows) => format!(
                "{{{}}}",
                rows.iter()
                    .map(|r| r.iter().map(go).collect::<Vec<_>>().join(";"))
                    .collect::<Vec<_>>()
                    .join("|")
            ),
            other => parser::print(other),
        }
    }
    format!("of:={}", go(e))
}
/// OpenFormula text (`of:=…`, `oooc:=…` or bare) in the engine's A1 syntax.
pub fn from_openformula(text: &str) -> String {
    let body = text
        .strip_prefix("of:")
        .or_else(|| text.strip_prefix("oooc:"))
        .or_else(|| text.strip_prefix("msoxl:"))
        .unwrap_or(text);
    let body = body.strip_prefix('=').unwrap_or(body);
    let mut out = String::new();
    let mut chars = body.chars().peekable();
    let mut in_string = false;
    let mut braces = 0;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    out.push(chars.next().unwrap_or('"'));
                } else {
                    in_string = false;
                }
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' => {
                braces += 1;
                out.push(c);
            }
            '}' => {
                braces -= 1;
                out.push(c);
            }
            ';' => out.push(','),
            '|' if braces > 0 => out.push(';'),
            '[' => {
                let mut inner = String::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    inner.push(d);
                }
                out.push_str(&convert_ref(&inner));
            }
            c => out.push(c),
        }
    }
    out
}
fn convert_ref(inner: &str) -> String {
    // [$Sheet.A1:.B2], [.A1], ['My Sheet'.A1]
    let one = |part: &str| -> (Option<String>, String) {
        let part = part.trim();
        match part.rfind('.') {
            Some(i) => {
                let sheet = part[..i].trim_start_matches('$');
                let sheet = if sheet.is_empty() {
                    None
                } else {
                    Some(sheet.trim_matches('\'').replace("''", "'"))
                };
                (sheet, part[i + 1..].to_owned())
            }
            None => (None, part.to_owned()),
        }
    };
    let (a, b) = match inner.split_once(':') {
        Some((x, y)) => (x, Some(y)),
        None => (inner, None),
    };
    let (sheet, first) = one(a);
    let prefix = sheet
        .map(|s| format!("{}!", parser::quote_sheet(&s)))
        .unwrap_or_default();
    match b {
        Some(b) => format!("{prefix}{first}:{}", one(b).1),
        None => format!("{prefix}{first}"),
    }
}

// ----- number styles -----

fn data_style(name: &str, code: &str) -> Option<String> {
    let code_lc = code.to_ascii_lowercase();
    if code_lc == "general" || code == "@" {
        return None;
    }
    let decimals = code
        .split_once('.')
        .map_or(0, |(_, f)| f.chars().take_while(|c| *c == '0').count());
    let grouping = code.contains("#,##");
    let number = format!(
        "<number:number number:decimal-places=\"{decimals}\" number:min-decimal-places=\"{decimals}\" number:min-integer-digits=\"1\"{}/>",
        if grouping { " number:grouping=\"true\"" } else { "" }
    );
    if crate::format::is_date_format(code) {
        let mut parts = String::new();
        let mut rest = code_lc.as_str();
        while !rest.is_empty() {
            let (tok, len): (Option<&str>, usize) = if rest.starts_with("yyyy") {
                (Some("<number:year number:style=\"long\"/>"), 4)
            } else if rest.starts_with("yy") {
                (Some("<number:year/>"), 2)
            } else if rest.starts_with("mmmm") {
                (
                    Some("<number:month number:textual=\"true\" number:style=\"long\"/>"),
                    4,
                )
            } else if rest.starts_with("mmm") {
                (Some("<number:month number:textual=\"true\"/>"), 3)
            } else if rest.starts_with("mm") {
                (Some("<number:month number:style=\"long\"/>"), 2)
            } else if rest.starts_with("dd") {
                (Some("<number:day number:style=\"long\"/>"), 2)
            } else if rest.starts_with('m') {
                (Some("<number:month/>"), 1)
            } else if rest.starts_with('d') {
                (Some("<number:day/>"), 1)
            } else if rest.starts_with("hh") || rest.starts_with('h') {
                (
                    Some("<number:hours/>"),
                    if rest.starts_with("hh") { 2 } else { 1 },
                )
            } else if rest.starts_with("ss") {
                (Some("<number:seconds number:style=\"long\"/>"), 2)
            } else if rest.starts_with("am/pm") {
                (Some("<number:am-pm/>"), 5)
            } else {
                let c = rest.chars().next().unwrap_or(' ');
                parts.push_str(&format!(
                    "<number:text>{}</number:text>",
                    escape(&c.to_string())
                ));
                rest = &rest[c.len_utf8()..];
                continue;
            };
            parts.push_str(tok.unwrap_or(""));
            rest = &rest[len..];
        }
        return Some(format!(
            "<number:date-style style:name=\"{name}\">{parts}</number:date-style>"
        ));
    }
    if code.contains('%') {
        return Some(format!("<number:percentage-style style:name=\"{name}\">{number}<number:text>%</number:text></number:percentage-style>"));
    }
    if let Some(sym) = ['$', '€', '£'].iter().find(|s| code.contains(**s)) {
        return Some(format!(
            "<number:currency-style style:name=\"{name}\"><number:currency-symbol number:language=\"en\" number:country=\"US\">{sym}</number:currency-symbol>{number}</number:currency-style>"
        ));
    }
    Some(format!(
        "<number:number-style style:name=\"{name}\">{number}</number:number-style>"
    ))
}
fn code_from_style(el: &Element) -> String {
    let number = el.child("number");
    let decimals: usize = number
        .and_then(|n| n.attr("decimal-places"))
        .and_then(|d| d.parse().ok())
        .unwrap_or(0);
    let grouping = number.and_then(|n| n.attr("grouping")) == Some("true");
    let mut num = String::from(if grouping { "#,##0" } else { "0" });
    if decimals > 0 {
        num.push('.');
        num.push_str(&"0".repeat(decimals));
    }
    match el.local() {
        "percentage-style" => format!("{num}%"),
        "currency-style" => {
            // An empty symbol is the locale's own; this world's locale is en-US.
            let sym = el
                .child("currency-symbol")
                .map(|c| c.text())
                .filter(|t| !t.is_empty());
            format!("{}{num}", sym.unwrap_or_else(|| "$".into()))
        }
        "date-style" | "time-style" => {
            let mut code = String::new();
            for part in el.elements() {
                let long = part.attr("style") == Some("long");
                code.push_str(&match part.local() {
                    "year" => if long { "yyyy" } else { "yy" }.to_string(),
                    "month" if part.attr("textual") == Some("true") => {
                        if long { "mmmm" } else { "mmm" }.to_string()
                    }
                    "month" => if long { "mm" } else { "m" }.to_string(),
                    "day" => if long { "dd" } else { "d" }.to_string(),
                    "hours" => if long { "hh" } else { "h" }.to_string(),
                    "minutes" => "mm".to_string(),
                    "seconds" => "ss".to_string(),
                    "am-pm" => "AM/PM".to_string(),
                    "text" => part.text(),
                    _ => String::new(),
                });
            }
            code
        }
        _ => num,
    }
}

fn iso_date(serial: f64) -> String {
    let (y, m, d) = crate::date::ymd(serial).unwrap_or((1899, 12, 30));
    let (h, mi, s) = crate::date::hms(serial);
    if serial.fract() == 0.0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}")
    }
}

/// The workbook as `.ods` bytes.
pub fn write(wb: &Workbook, generator: &str) -> Vec<u8> {
    let mut styles: Vec<Style> = vec![Style::default()];
    let mut widths: std::collections::BTreeSet<u32> =
        [crate::workbook::DEFAULT_COL_WIDTH].into_iter().collect();
    let mut body = String::new();
    for (si, sheet) in wb.sheets.iter().enumerate() {
        body.push_str(&format!(
            "<table:table table:name=\"{}\">",
            escape(&sheet.name)
        ));
        let used = wb.used_range(si);
        let last_col = used.map_or(0, |u| u.end.col);
        for col in 0..=last_col {
            let w = sheet.col_width(col);
            widths.insert(w);
            body.push_str(&format!("<table:table-column table:style-name=\"co{w}\"/>"));
        }
        let last_row = used.map_or(0, |u| u.end.row);
        for row in 0..=last_row {
            body.push_str("<table:table-row>");
            let mut empty_run = 0;
            for col in 0..=last_col {
                let c = Cell::new(row, col);
                let Some(d) = sheet.cells.get(&c) else {
                    empty_run += 1;
                    continue;
                };
                if empty_run > 0 {
                    body.push_str(&format!(
                        "<table:table-cell table:number-columns-repeated=\"{empty_run}\"/>"
                    ));
                    empty_run = 0;
                }
                let style_index = match styles.iter().position(|s| *s == d.style) {
                    Some(i) => i,
                    None => {
                        styles.push(d.style.clone());
                        styles.len() - 1
                    }
                };
                let style_attr = if style_index == 0 {
                    String::new()
                } else {
                    format!(" table:style-name=\"ce{style_index}\"")
                };
                let formula = match &d.input {
                    Input::Formula(f) => {
                        format!(" table:formula=\"{}\"", escape(&to_openformula(&f.expr)))
                    }
                    _ => String::new(),
                };
                let shown = escape(&wb.display(si, c));
                let value = match &d.value {
                    Value::Number(n) => {
                        if crate::format::is_date_format(&d.style.format) {
                            format!(
                                " office:value-type=\"date\" office:date-value=\"{}\"",
                                iso_date(*n)
                            )
                        } else if d.style.format.contains('%') {
                            format!(
                                " office:value-type=\"percentage\" office:value=\"{}\"",
                                crate::value::general(*n)
                            )
                        } else {
                            format!(
                                " office:value-type=\"float\" office:value=\"{}\"",
                                number_attr(*n)
                            )
                        }
                    }
                    Value::Bool(b) => {
                        format!(" office:value-type=\"boolean\" office:boolean-value=\"{b}\"")
                    }
                    Value::Text(_) => " office:value-type=\"string\"".into(),
                    Value::Error(_) | Value::Empty => String::new(),
                };
                let text = if matches!(d.value, Value::Empty) {
                    String::new()
                } else {
                    format!("<text:p>{shown}</text:p>")
                };
                body.push_str(&format!(
                    "<table:table-cell{style_attr}{formula}{value}>{text}</table:table-cell>"
                ));
            }
            body.push_str("</table:table-row>");
        }
        body.push_str("</table:table>");
    }
    let mut named = String::new();
    for (name, (sheet, range)) in &wb.names {
        let abs = |c: Cell| {
            CellRef {
                row: c.row,
                col: c.col,
                row_abs: true,
                col_abs: true,
            }
            .a1()
        };
        named.push_str(&format!(
            "<table:named-range table:name=\"{}\" table:base-cell-address=\"${}.{}\" table:cell-range-address=\"${}.{}:.{}\"/>",
            escape(name),
            escape(&odf_sheet(sheet)),
            abs(range.start),
            escape(&odf_sheet(sheet)),
            abs(range.start),
            abs(range.end)
        ));
    }
    if !named.is_empty() {
        body.push_str(&format!(
            "<table:named-expressions>{named}</table:named-expressions>"
        ));
    }
    let mut auto = String::new();
    for w in widths {
        auto.push_str(&format!(
            "<style:style style:name=\"co{w}\" style:family=\"table-column\"><style:table-column-properties style:column-width=\"{:.4}in\"/></style:style>",
            f64::from(w) / 96.0
        ));
    }
    for (i, s) in styles.iter().enumerate().skip(1) {
        let data = data_style(&format!("N{i}"), &s.format);
        if let Some(d) = &data {
            auto.push_str(d);
        }
        auto.push_str(&format!(
            "<style:style style:name=\"ce{i}\" style:family=\"table-cell\" style:parent-style-name=\"Default\"{}>",
            if data.is_some() { format!(" style:data-style-name=\"N{i}\"") } else { String::new() }
        ));
        let mut cell_props = String::new();
        if let Some(f) = s.fill {
            cell_props.push_str(&format!(
                " fo:background-color=\"#{:02x}{:02x}{:02x}\"",
                f[0], f[1], f[2]
            ));
        }
        if !cell_props.is_empty() {
            auto.push_str(&format!("<style:table-cell-properties{cell_props}/>"));
        }
        match s.align {
            Align::General => {}
            a => auto.push_str(&format!(
                "<style:paragraph-properties fo:text-align=\"{}\"/>",
                match a {
                    Align::Left => "start",
                    Align::Center => "center",
                    _ => "end",
                }
            )),
        }
        let mut text = String::new();
        if s.bold {
            text.push_str(" fo:font-weight=\"bold\"");
        }
        if s.italic {
            text.push_str(" fo:font-style=\"italic\"");
        }
        if s.underline {
            text.push_str(" style:text-underline-style=\"solid\" style:text-underline-width=\"auto\" style:text-underline-color=\"font-color\"");
        }
        if let Some(c) = s.color {
            text.push_str(&format!(
                " fo:color=\"#{:02x}{:02x}{:02x}\"",
                c[0], c[1], c[2]
            ));
        }
        if !text.is_empty() {
            auto.push_str(&format!("<style:text-properties{text}/>"));
        }
        auto.push_str("</style:style>");
    }
    let content = format!(
        "{HEAD}<office:document-content {NS} office:version=\"1.3\"><office:automatic-styles>{auto}</office:automatic-styles><office:body><office:spreadsheet>{body}</office:spreadsheet></office:body></office:document-content>"
    );
    let styles_xml = format!(
        "{HEAD}<office:document-styles {NS} office:version=\"1.3\"><office:styles><style:style style:name=\"Default\" style:family=\"table-cell\"><style:text-properties style:font-name=\"Liberation Sans\" fo:font-size=\"10pt\"/></style:style></office:styles></office:document-styles>"
    );
    let meta = format!(
        "{HEAD}<office:document-meta {NS} office:version=\"1.3\"><office:meta><meta:generator>{}</meta:generator><meta:creation-date>{}</meta:creation-date><dc:date>{}</dc:date></office:meta></office:document-meta>",
        escape(generator),
        iso_date(wb.now().max(1.0).floor()),
        iso_date(wb.now().max(1.0).floor())
    );
    let manifest = format!(
        "{HEAD}<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{MIME}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    let entries = vec![
        ("mimetype".to_string(), MIME.as_bytes().to_vec()),
        ("content.xml".to_string(), content.into_bytes()),
        ("styles.xml".to_string(), styles_xml.into_bytes()),
        ("meta.xml".to_string(), meta.into_bytes()),
        ("META-INF/manifest.xml".to_string(), manifest.into_bytes()),
    ];
    let (y, m, d) = crate::date::ymd(wb.now()).unwrap_or((1980, 1, 1));
    let (h, mi, s) = crate::date::hms(wb.now());
    crate::zip::write_first_stored(&entries, crate::zip::dos_time(y, m, d, h, mi, s))
}
fn number_attr(n: f64) -> String {
    let short = cw_determinism::math::format_significant(n, 15);
    if short.parse::<f64>().ok() == Some(n) {
        short
    } else {
        cw_determinism::math::format_significant(n, 17)
    }
}

/// Read an `.ods` workbook.
pub fn read(bytes: &[u8]) -> Result<Workbook, String> {
    let parts: BTreeMap<String, Vec<u8>> = crate::zip::read(bytes)?.into_iter().collect();
    if let Some(m) = parts.get("mimetype") {
        if m.as_slice() != MIME.as_bytes() {
            return Err("this OpenDocument file is not a spreadsheet".into());
        }
    }
    let content = parts
        .get("content.xml")
        .ok_or("the file has no content.xml")?;
    let doc = parse(&String::from_utf8_lossy(content))?;
    // Styles: cell style name → (data style name, style).
    let mut data_styles: BTreeMap<String, String> = BTreeMap::new();
    let mut cell_styles: BTreeMap<String, (Option<String>, Style)> = BTreeMap::new();
    let mut col_widths: BTreeMap<String, u32> = BTreeMap::new();
    let mut style_roots = vec![doc.child("automatic-styles").cloned()];
    if let Some(s) = parts.get("styles.xml") {
        if let Ok(sd) = parse(&String::from_utf8_lossy(s)) {
            style_roots.push(sd.child("styles").cloned());
            style_roots.push(sd.child("automatic-styles").cloned());
        }
    }
    for root in style_roots.into_iter().flatten() {
        for el in root.elements() {
            let Some(name) = el.attr("name") else {
                continue;
            };
            match el.local() {
                "number-style" | "percentage-style" | "currency-style" | "date-style"
                | "time-style" => {
                    data_styles.insert(name.to_owned(), code_from_style(el));
                }
                "style" if el.attr("family") == Some("table-cell") => {
                    let mut st = Style::default();
                    if let Some(t) = el.child("text-properties") {
                        st.bold = t.attr("font-weight") == Some("bold");
                        st.italic = t.attr("font-style") == Some("italic");
                        st.underline = t.attr("text-underline-style").is_some_and(|u| u != "none");
                        st.color = t
                            .attr("color")
                            .and_then(hex_color)
                            .filter(|c| *c != [0, 0, 0]);
                    }
                    if let Some(c) = el.child("table-cell-properties") {
                        st.fill = c.attr("background-color").and_then(hex_color);
                    }
                    if let Some(p) = el.child("paragraph-properties") {
                        st.align = match p.attr("text-align") {
                            Some("start") | Some("left") => Align::Left,
                            Some("center") => Align::Center,
                            Some("end") | Some("right") => Align::Right,
                            _ => Align::General,
                        };
                    }
                    cell_styles.insert(
                        name.to_owned(),
                        (el.attr("data-style-name").map(str::to_owned), st),
                    );
                }
                "style" if el.attr("family") == Some("table-column") => {
                    if let Some(w) = el
                        .child("table-column-properties")
                        .and_then(|p| p.attr("column-width"))
                    {
                        if let Some(px) = length_px(w) {
                            col_widths.insert(name.to_owned(), px);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let style_for = |name: Option<&str>| -> Style {
        let Some(n) = name else {
            return Style::default();
        };
        match cell_styles.get(n) {
            Some((data, st)) => {
                let mut st = st.clone();
                if let Some(code) = data.as_ref().and_then(|d| data_styles.get(d)) {
                    st.format = code.clone();
                }
                st
            }
            None => Style::default(),
        }
    };
    let spreadsheet = doc
        .find("spreadsheet")
        .ok_or("the document holds no spreadsheet")?;
    let mut wb = Workbook::new();
    wb.sheets.clear();
    for table in spreadsheet.children_named("table") {
        let si = wb.sheets.len();
        wb.sheets
            .push(Sheet::new(table.attr("name").unwrap_or("Sheet")));
        let mut col = 0u32;
        for c in table
            .elements()
            .filter(|e| e.local() == "table-column" || e.local() == "table-columns")
        {
            let cols: Vec<&Element> = if c.local() == "table-columns" {
                c.children_named("table-column").collect()
            } else {
                vec![c]
            };
            for tc in cols {
                let rep: u32 = tc
                    .attr("number-columns-repeated")
                    .and_then(|r| r.parse().ok())
                    .unwrap_or(1);
                if let Some(px) = tc.attr("style-name").and_then(|s| col_widths.get(s)) {
                    if *px != crate::workbook::DEFAULT_COL_WIDTH && rep < 256 {
                        for k in 0..rep {
                            wb.sheets[si].col_widths.insert(col + k, *px);
                        }
                    }
                }
                col = col.saturating_add(rep);
            }
        }
        let mut row = 0u32;
        let rows = table.elements().flat_map(|e| match e.local() {
            "table-row" => vec![e],
            "table-rows" | "table-header-rows" | "table-row-group" => {
                e.children_named("table-row").collect()
            }
            _ => vec![],
        });
        for tr in rows {
            let rep: u32 = tr
                .attr("number-rows-repeated")
                .and_then(|r| r.parse().ok())
                .unwrap_or(1);
            let cells: Vec<&Element> = tr
                .elements()
                .filter(|e| e.local() == "table-cell" || e.local() == "covered-table-cell")
                .collect();
            let has_content = cells.iter().any(|c| {
                c.attr("value-type").is_some()
                    || c.attr("formula").is_some()
                    || c.child("p").is_some()
            });
            if !has_content {
                row = row.saturating_add(rep);
                continue;
            }
            for _ in 0..rep.min(1000) {
                let mut col = 0u32;
                for tc in &cells {
                    let crep: u32 = tc
                        .attr("number-columns-repeated")
                        .and_then(|r| r.parse().ok())
                        .unwrap_or(1);
                    let style = style_for(tc.attr("style-name"));
                    let text = tc
                        .children_named("p")
                        .map(|p| p.text())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let value = match tc.attr("value-type") {
                        Some("float") | Some("currency") | Some("percentage") => tc
                            .attr("value")
                            .and_then(|v| v.parse::<f64>().ok())
                            .map_or(Value::Empty, Value::number),
                        Some("date") => tc
                            .attr("date-value")
                            .and_then(|d| {
                                crate::date::parse_datetime(&d.replace('T', " ")).map(|x| x.0)
                            })
                            .map_or(Value::Empty, Value::Number),
                        Some("time") => tc
                            .attr("time-value")
                            .map_or(Value::Empty, |t| Value::Number(iso_duration(t))),
                        Some("boolean") => Value::Bool(tc.attr("boolean-value") == Some("true")),
                        Some("string") => {
                            Value::Text(tc.attr("string-value").map_or(text.clone(), str::to_owned))
                        }
                        _ if text.starts_with('#') => {
                            ErrorKind::parse(&text).map_or(Value::Text(text.clone()), Value::Error)
                        }
                        _ if !text.is_empty() => Value::Text(text.clone()),
                        _ => Value::Empty,
                    };
                    let input = match tc.attr("formula") {
                        Some(f) => match crate::parser::Formula::parse(&from_openformula(f)) {
                            Ok(fm) => Input::Formula(fm),
                            Err(_) => Input::Value(value.clone()),
                        },
                        None => Input::Value(value.clone()),
                    };
                    if !matches!(input, Input::Value(Value::Empty)) || !style.is_default() {
                        for k in 0..crep.min(1024) {
                            wb.load_cell(
                                si,
                                Cell::new(row, col + k),
                                input.clone(),
                                value.clone(),
                                style.clone(),
                            );
                        }
                    }
                    col = col.saturating_add(crep);
                }
                row += 1;
            }
            row = row.saturating_add(rep.saturating_sub(rep.min(1000)));
        }
    }
    if let Some(ne) = spreadsheet.child("named-expressions") {
        for nr in ne.children_named("named-range") {
            let (Some(name), Some(addr)) = (nr.attr("name"), nr.attr("cell-range-address")) else {
                continue;
            };
            let converted = convert_ref(addr);
            if let Some((sheet, body)) = converted.rsplit_once('!') {
                let sheet = sheet.trim_matches('\'').replace("''", "'");
                if let Some(range) = crate::address::Range::parse(body) {
                    wb.names.insert(name.to_owned(), (sheet, range));
                }
            }
        }
    }
    if wb.sheets.is_empty() {
        return Err("the spreadsheet has no tables".into());
    }
    Ok(wb.loaded())
}
fn hex_color(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}
fn length_px(s: &str) -> Option<u32> {
    let (num, unit) = s.split_at(s.find(|c: char| c.is_ascii_alphabetic())?);
    let v: f64 = num.parse().ok()?;
    let px = match unit {
        "in" => v * 96.0,
        "cm" => v * 96.0 / 2.54,
        "mm" => v * 96.0 / 25.4,
        "pt" => v * 96.0 / 72.0,
        "px" => v,
        _ => return None,
    };
    Some(px.round() as u32)
}
/// `PT13H30M00S` as a fraction of a day.
fn iso_duration(t: &str) -> f64 {
    let body = t.trim_start_matches("PT");
    let mut total = 0.0;
    let mut num = String::new();
    for c in body.chars() {
        match c {
            'H' => {
                total += num.parse::<f64>().unwrap_or(0.0) * 3600.0;
                num.clear();
            }
            'M' => {
                total += num.parse::<f64>().unwrap_or(0.0) * 60.0;
                num.clear();
            }
            'S' => {
                total += num.parse::<f64>().unwrap_or(0.0);
                num.clear();
            }
            c => num.push(c),
        }
    }
    total / 86_400.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::Range;
    #[test]
    fn openformula_converts_both_ways() {
        let e = crate::parser::parse("=SUM(A1:B2,'My Sheet'!$C$3)*2").unwrap();
        let of = to_openformula(&e);
        assert_eq!(of, "of:=SUM([.A1:.B2];[$'My Sheet'.$C$3])*2");
        assert_eq!(from_openformula(&of), "SUM(A1:B2,'My Sheet'!$C$3)*2");
        assert_eq!(
            from_openformula("of:=IF([.A1]>0;\"a;b\";{1;2|3;4})"),
            "IF(A1>0,\"a;b\",{1,2;3,4})"
        );
    }
    #[test]
    fn a_workbook_survives_ods() {
        let mut wb = Workbook::new();
        wb.set_input(0, Cell::new(0, 0), "Price").unwrap();
        wb.set_input(0, Cell::new(1, 0), "$12.50").unwrap();
        wb.set_input(0, Cell::new(2, 0), "25%").unwrap();
        wb.set_input(0, Cell::new(3, 0), "2026-09-18").unwrap();
        wb.set_input(0, Cell::new(4, 0), "=A2*(1+A3)").unwrap();
        wb.set_input(0, Cell::new(5, 0), "TRUE").unwrap();
        wb.update_style(0, Range::parse("A1").unwrap(), |s| s.bold = true)
            .unwrap();
        wb.define_name("Price", 0, Range::parse("A2").unwrap())
            .unwrap();
        let bytes = write(&wb, "LibreOffice/24.2");
        let entries = crate::zip::read(&bytes).unwrap();
        assert_eq!(entries[0].0, "mimetype");
        assert_eq!(
            &bytes[30..38],
            b"mimetype",
            "mimetype is the first entry, stored"
        );
        let back = read(&bytes).unwrap();
        assert_eq!(back.input(0, Cell::new(4, 0)), "=A2*(1+A3)");
        assert_eq!(back.value(0, Cell::new(4, 0)), Value::Number(15.625));
        assert_eq!(back.display(0, Cell::new(1, 0)), "$12.50");
        assert_eq!(back.display(0, Cell::new(2, 0)), "25%");
        assert_eq!(back.display(0, Cell::new(3, 0)), "9/18/2026");
        assert_eq!(back.value(0, Cell::new(5, 0)), Value::Bool(true));
        assert!(back.style(0, Cell::new(0, 0)).bold);
        assert_eq!(back.name("Price"), Some((0, Range::parse("A2").unwrap())));
    }
}
