//! OpenDocument parts beyond cells, as LibreOffice Calc writes them: conditional
//! formats (`calcext:conditional-formats`, with a named cell style per format) and
//! pivot tables (`table:data-pilot-tables`).
use crate::address::Range;
use crate::conditional::{CellOp, Cfvo, CondFormat, Dxf, Period, Rule, TextOp};
use crate::pivot::{Agg, Pivot, PivotStyle};
use crate::workbook::Workbook;
use crate::xml::{escape, Element};
use std::collections::{BTreeMap, BTreeSet};

fn sheet_ref(sheet: &str) -> String {
    if sheet.chars().all(|c| c.is_alphanumeric() || c == '_') {
        sheet.to_owned()
    } else {
        format!("'{}'", sheet.replace('\'', "''"))
    }
}
fn address(sheet: &str, r: Range) -> String {
    let s = sheet_ref(sheet);
    if r.is_single() {
        format!("{s}.{}", r.start.a1())
    } else {
        format!("{s}.{}:{s}.{}", r.start.a1(), r.end.a1())
    }
}
/// `Sheet1.A1:Sheet1.B2` (or `$Sheet1.$A$1…`) as a sheet name and a range.
fn parse_address(text: &str) -> Option<(String, Range)> {
    let one = |p: &str| -> Option<(String, String)> {
        let p = p.trim().trim_start_matches('$');
        let dot = p.rfind('.')?;
        let sheet = p[..dot].trim_start_matches('$');
        Some((
            sheet.trim_matches('\'').replace("''", "'"),
            p[dot + 1..].replace('$', ""),
        ))
    };
    match text.split_once(':') {
        Some((a, b)) => {
            let (s, x) = one(a)?;
            let y = one(b)
                .map(|(_, y)| y)
                .unwrap_or_else(|| b.replace(['$', '.'], ""));
            Some((s, Range::parse(&format!("{x}:{y}"))?))
        }
        None => {
            let (s, x) = one(text)?;
            Some((s, Range::parse(&x)?))
        }
    }
}
fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
fn from_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}
/// A rule formula (A1 syntax, no `=`) in OpenFormula, without the `of:=` prefix.
fn of_formula(f: &str) -> String {
    match crate::parser::parse(f.trim_start_matches('=')) {
        Ok(e) => crate::ods::to_openformula(&e)
            .trim_start_matches("of:=")
            .to_owned(),
        Err(_) => f.to_owned(),
    }
}
/// The named cell style a conditional format applies.
pub fn dxf_style(n: usize, d: &Dxf) -> String {
    let mut cell = String::new();
    if let Some(f) = d.fill {
        cell = format!(
            "<style:table-cell-properties fo:background-color=\"{}\"/>",
            hex(f)
        );
    }
    let mut text = String::new();
    if d.bold {
        text.push_str(" fo:font-weight=\"bold\"");
    }
    if d.italic {
        text.push_str(" fo:font-style=\"italic\"");
    }
    if d.underline {
        text.push_str(" style:text-underline-style=\"solid\" style:text-underline-width=\"auto\" style:text-underline-color=\"font-color\"");
    }
    if let Some(c) = d.color {
        text.push_str(&format!(" fo:color=\"{}\"", hex(c)));
    }
    let text = if text.is_empty() {
        String::new()
    } else {
        format!("<style:text-properties{text}/>")
    };
    format!("<style:style style:name=\"ConditionalStyle_{n}\" style:family=\"table-cell\" style:parent-style-name=\"Default\">{cell}{text}</style:style>")
}
fn entry(v: &Cfvo, tag: &str, color: Option<[u8; 3]>, bar: bool) -> String {
    let (kind, val) = match v {
        Cfvo::Min => (
            if bar { "auto-minimum" } else { "minimum" },
            "0".to_string(),
        ),
        Cfvo::Max => (
            if bar { "auto-maximum" } else { "maximum" },
            "0".to_string(),
        ),
        Cfvo::Num(x) => ("number", crate::value::general(*x)),
        Cfvo::Percent(x) => ("percent", crate::value::general(*x)),
        Cfvo::Percentile(x) => ("percentile", crate::value::general(*x)),
        Cfvo::Formula(f) => ("formula", of_formula(f)),
    };
    format!(
        "<calcext:{tag} calcext:value=\"{}\" calcext:type=\"{kind}\"{}/>",
        escape(&val),
        color.map_or(String::new(), |c| format!(" calcext:color=\"{}\"", hex(c)))
    )
}
/// A sheet's conditional formats; named styles for their formats are added to `styles`.
pub fn conditional_xml(wb: &Workbook, sheet: usize, styles: &mut Vec<Dxf>) -> String {
    let s = &wb.sheets[sheet];
    if s.conditional.is_empty() {
        return String::new();
    }
    let mut out = String::from("<calcext:conditional-formats>");
    for cf in &s.conditional {
        let target: Vec<String> = cf.ranges.iter().map(|r| address(&s.name, *r)).collect();
        let base = address(&s.name, Range::single(cf.ranges[0].start));
        out.push_str(&format!(
            "<calcext:conditional-format calcext:target-range-address=\"{}\">",
            escape(&target.join(" "))
        ));
        let mut style_name = || {
            let d = cf.rule.style().unwrap_or_default();
            let i = match styles.iter().position(|x| *x == d) {
                Some(i) => i,
                None => {
                    styles.push(d);
                    styles.len() - 1
                }
            };
            format!("ConditionalStyle_{}", i + 1)
        };
        let q = |t: &str| format!("\"{}\"", t.replace('"', "\"\""));
        let condition = match &cf.rule {
            Rule::CellIs { op, formulas, .. } => {
                let a = |i: usize| of_formula(formulas.get(i).map_or("0", String::as_str));
                // LibreOffice's own spelling: the operator and its operands.
                Some(match op {
                    CellOp::Between => format!("between({},{})", a(0), a(1)),
                    CellOp::NotBetween => format!("not-between({},{})", a(0), a(1)),
                    CellOp::Equal => format!("={}", a(0)),
                    CellOp::NotEqual => format!("!={}", a(0)),
                    CellOp::Greater => format!(">{}", a(0)),
                    CellOp::Less => format!("<{}", a(0)),
                    CellOp::GreaterEqual => format!(">={}", a(0)),
                    CellOp::LessEqual => format!("<={}", a(0)),
                })
            }
            Rule::Text { op, text, .. } => Some(match op {
                TextOp::Contains => format!("contains-text({})", q(text)),
                TextOp::NotContains => format!("not-contains-text({})", q(text)),
                TextOp::BeginsWith => format!("begins-with({})", q(text)),
                TextOp::EndsWith => format!("ends-with({})", q(text)),
            }),
            Rule::Duplicates { unique, .. } => {
                Some(if *unique { "unique" } else { "duplicate" }.into())
            }
            Rule::Top {
                bottom,
                rank,
                percent,
                ..
            } => Some(format!(
                "{}-{}({rank})",
                if *bottom { "bottom" } else { "top" },
                if *percent { "percent" } else { "elements" }
            )),
            Rule::Average { below, equal, .. } => Some(format!(
                "{}{}-average",
                if *below { "below" } else { "above" },
                if *equal { "-equal" } else { "" }
            )),
            Rule::Errors { errors, .. } => {
                Some(if *errors { "is-error" } else { "is-no-error" }.into())
            }
            Rule::Blanks { blanks, .. } => Some(format!(
                "formula-is(LEN(TRIM([.{}])){}0)",
                cf.ranges[0].start.a1(),
                if *blanks { "=" } else { ">" }
            )),
            Rule::Expression { formula, .. } => {
                Some(format!("formula-is({})", of_formula(formula)))
            }
            _ => None,
        };
        match (&cf.rule, condition) {
            (Rule::Dates { period, .. }, _) => {
                let date = match period {
                    Period::Today => "today",
                    Period::Yesterday => "yesterday",
                    Period::Tomorrow => "tomorrow",
                    Period::Last7Days => "last-7-days",
                    Period::ThisWeek => "this-week",
                    Period::LastWeek => "last-week",
                    Period::NextWeek => "next-week",
                    Period::ThisMonth => "this-month",
                    Period::LastMonth => "last-month",
                    Period::NextMonth => "next-month",
                };
                out.push_str(&format!(
                    "<calcext:date-is calcext:date=\"{date}\" calcext:style=\"{}\"/>",
                    style_name()
                ));
            }
            (_, Some(value)) => out.push_str(&format!(
                "<calcext:condition calcext:apply-style-name=\"{}\" calcext:value=\"{}\" calcext:base-cell-address=\"{}\"/>",
                style_name(),
                escape(&value),
                escape(&base)
            )),
            (Rule::DataBar { color, min, max }, None) => {
                out.push_str(&format!(
                    "<calcext:data-bar calcext:max-length=\"100\" calcext:negative-color=\"#ff0000\" calcext:positive-color=\"{}\" calcext:axis-color=\"#000000\">{}{}</calcext:data-bar>",
                    hex(*color),
                    entry(min, "formatting-entry", None, true),
                    entry(max, "formatting-entry", None, true)
                ));
            }
            (Rule::ColorScale { stops }, None) => {
                out.push_str("<calcext:color-scale>");
                for (v, c) in stops {
                    out.push_str(&entry(v, "color-scale-entry", Some(*c), false));
                }
                out.push_str("</calcext:color-scale>");
            }
            (
                Rule::IconSet {
                    set,
                    points,
                    show_value,
                    ..
                },
                None,
            ) => {
                out.push_str(&format!(
                    "<calcext:icon-set calcext:icon-set-type=\"{}\"{}>",
                    escape(set),
                    if *show_value {
                        ""
                    } else {
                        " calcext:show-value=\"false\""
                    }
                ));
                for p in points {
                    out.push_str(&entry(p, "formatting-entry", None, false));
                }
                out.push_str("</calcext:icon-set>");
            }
            _ => {}
        }
        out.push_str("</calcext:conditional-format>");
    }
    out.push_str("</calcext:conditional-formats>");
    out
}
fn read_entry(e: &Element) -> Option<Cfvo> {
    let val = e.attr("value").unwrap_or("0");
    let num = || val.trim().parse::<f64>().ok();
    Some(match e.attr("type")? {
        "minimum" | "auto-minimum" => Cfvo::Min,
        "maximum" | "auto-maximum" => Cfvo::Max,
        "number" => Cfvo::Num(num()?),
        "percent" => Cfvo::Percent(num()?),
        "percentile" => Cfvo::Percentile(num()?),
        "formula" => Cfvo::Formula(crate::ods::from_openformula(val)),
        _ => return None,
    })
}
/// Split `f(a,b)` arguments at top-level commas (OpenFormula lists use `;` inside).
fn args(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut quoted = false;
    let mut cur = String::new();
    for c in inner.chars() {
        match c {
            '"' => quoted = !quoted,
            '(' | '[' if !quoted => depth += 1,
            ')' | ']' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}
fn unquote(s: &str) -> String {
    s.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .replace("\"\"", "\"")
}
fn a1(of: &str) -> String {
    crate::ods::from_openformula(of)
}
/// Conditional formats of one `table:table`; `styles` maps named cell styles to formats.
pub fn read_conditional(table: &Element, styles: &BTreeMap<String, Dxf>) -> Vec<CondFormat> {
    let Some(list) = table.child("conditional-formats") else {
        return vec![];
    };
    let mut out = Vec::new();
    for f in list.children_named("conditional-format") {
        let ranges: Vec<Range> = f
            .attr("target-range-address")
            .unwrap_or("")
            .split_whitespace()
            .filter_map(|a| parse_address(a).map(|(_, r)| r))
            .collect();
        if ranges.is_empty() {
            continue;
        }
        for c in f.elements() {
            let style =
                |n: Option<&str>| n.and_then(|n| styles.get(n)).copied().unwrap_or_default();
            let rule = match c.local() {
                "condition" => {
                    let style = style(c.attr("apply-style-name"));
                    let v = c.attr("value").unwrap_or("").trim().to_owned();
                    let call = |name: &str| -> Option<Vec<String>> {
                        let rest = v.strip_prefix(name)?.strip_prefix('(')?;
                        Some(args(rest.strip_suffix(')')?))
                    };
                    let bare = [">=", "<=", "!=", "<>", "=", ">", "<"]
                        .iter()
                        .any(|p| v.starts_with(p));
                    if let Some(rest) = v
                        .strip_prefix("cell-content()")
                        .or(bare.then_some(v.as_str()))
                    {
                        let (op, arg) = [
                            (">=", CellOp::GreaterEqual),
                            ("<=", CellOp::LessEqual),
                            ("!=", CellOp::NotEqual),
                            ("<>", CellOp::NotEqual),
                            ("=", CellOp::Equal),
                            (">", CellOp::Greater),
                            ("<", CellOp::Less),
                        ]
                        .iter()
                        .find_map(|(s, o)| rest.strip_prefix(s).map(|a| (*o, a)))
                        .unwrap_or((CellOp::Equal, rest));
                        Rule::CellIs {
                            op,
                            formulas: vec![a1(arg)],
                            style,
                        }
                    } else if let Some(a) =
                        call("between").or_else(|| call("cell-content-is-between"))
                    {
                        Rule::CellIs {
                            op: CellOp::Between,
                            formulas: a.iter().map(|x| a1(x)).collect(),
                            style,
                        }
                    } else if let Some(a) =
                        call("not-between").or_else(|| call("cell-content-is-not-between"))
                    {
                        Rule::CellIs {
                            op: CellOp::NotBetween,
                            formulas: a.iter().map(|x| a1(x)).collect(),
                            style,
                        }
                    } else if let Some(a) = call("formula-is").or_else(|| call("is-true-formula")) {
                        Rule::Expression {
                            formula: a1(&a.join(",")),
                            style,
                        }
                    } else if let Some((op, name)) = [
                        (TextOp::NotContains, "not-contains-text"),
                        (TextOp::Contains, "contains-text"),
                        (TextOp::BeginsWith, "begins-with"),
                        (TextOp::EndsWith, "ends-with"),
                    ]
                    .into_iter()
                    .find(|(_, n)| v.starts_with(n))
                    {
                        let Some(a) = call(name) else { continue };
                        Rule::Text {
                            op,
                            text: unquote(&a.join(",")),
                            style,
                        }
                    } else if let Some((bottom, percent, name)) = [
                        (false, false, "top-elements"),
                        (true, false, "bottom-elements"),
                        (false, true, "top-percent"),
                        (true, true, "bottom-percent"),
                    ]
                    .into_iter()
                    .find(|(_, _, n)| v.starts_with(n))
                    {
                        let Some(a) = call(name) else { continue };
                        Rule::Top {
                            bottom,
                            percent,
                            rank: a[0].trim().parse().unwrap_or(10),
                            style,
                        }
                    } else {
                        match v.as_str() {
                            "duplicate" => Rule::Duplicates {
                                unique: false,
                                style,
                            },
                            "unique" => Rule::Duplicates {
                                unique: true,
                                style,
                            },
                            "above-average"
                            | "below-average"
                            | "above-equal-average"
                            | "below-equal-average" => Rule::Average {
                                below: v.starts_with("below"),
                                equal: v.contains("equal"),
                                style,
                            },
                            "is-error" => Rule::Errors {
                                errors: true,
                                style,
                            },
                            "is-no-error" => Rule::Errors {
                                errors: false,
                                style,
                            },
                            _ => continue,
                        }
                    }
                }
                "date-is" => {
                    let period = match c.attr("date").unwrap_or("") {
                        "today" => Period::Today,
                        "yesterday" => Period::Yesterday,
                        "tomorrow" => Period::Tomorrow,
                        "last-7-days" => Period::Last7Days,
                        "this-week" => Period::ThisWeek,
                        "last-week" => Period::LastWeek,
                        "next-week" => Period::NextWeek,
                        "this-month" => Period::ThisMonth,
                        "last-month" => Period::LastMonth,
                        "next-month" => Period::NextMonth,
                        _ => continue,
                    };
                    Rule::Dates {
                        period,
                        style: style(c.attr("style")),
                    }
                }
                "data-bar" => {
                    let v: Vec<Cfvo> = c
                        .children_named("formatting-entry")
                        .filter_map(read_entry)
                        .collect();
                    if v.len() != 2 {
                        continue;
                    }
                    Rule::DataBar {
                        color: c
                            .attr("positive-color")
                            .and_then(from_hex)
                            .unwrap_or([99, 142, 198]),
                        min: v[0].clone(),
                        max: v[1].clone(),
                    }
                }
                "color-scale" => {
                    let stops: Vec<(Cfvo, [u8; 3])> = c
                        .children_named("color-scale-entry")
                        .filter_map(|e| Some((read_entry(e)?, e.attr("color").and_then(from_hex)?)))
                        .collect();
                    if !(2..=3).contains(&stops.len()) {
                        continue;
                    }
                    Rule::ColorScale { stops }
                }
                "icon-set" => Rule::IconSet {
                    set: c.attr("icon-set-type").unwrap_or("3Arrows").to_owned(),
                    points: c
                        .children_named("formatting-entry")
                        .filter_map(read_entry)
                        .collect(),
                    reverse: false,
                    show_value: c.attr("show-value") != Some("false"),
                },
                _ => continue,
            };
            out.push(CondFormat {
                ranges: ranges.clone(),
                rule,
                stop_if_true: false,
            });
        }
    }
    out
}
/// Named cell styles that conditional formats can apply, as formats.
pub fn dxf_of(el: &Element) -> Dxf {
    let t = el.child("text-properties");
    Dxf {
        bold: t.and_then(|t| t.attr("font-weight")) == Some("bold"),
        italic: t.and_then(|t| t.attr("font-style")) == Some("italic"),
        underline: t
            .and_then(|t| t.attr("text-underline-style"))
            .is_some_and(|u| u != "none"),
        color: t.and_then(|t| t.attr("color")).and_then(from_hex),
        fill: el
            .child("table-cell-properties")
            .and_then(|c| c.attr("background-color"))
            .and_then(from_hex),
    }
}

// ----- pivot tables -----

/// Every pivot table of the workbook as `table:data-pilot-tables`.
pub fn pivots_xml(wb: &Workbook) -> String {
    let mut out = String::new();
    for s in &wb.sheets {
        for p in &s.pivots {
            let Some(extent) = p.extent else {
                continue;
            };
            let Some(si) = wb.sheet_index(&p.source_sheet) else {
                continue;
            };
            let Ok(names) = crate::pivot::fields(wb, si, p.source) else {
                continue;
            };
            out.push_str(&format!(
                "<table:data-pilot-table table:name=\"{}\" table:target-range-address=\"{}\" table:grand-total=\"both\" table:show-filter-button=\"true\"><table:source-cell-range table:cell-range-address=\"{}\"/>",
                escape(&p.name),
                escape(&address(&s.name, extent)),
                escape(&address(&p.source_sheet, p.source))
            ));
            let members = |f: usize| -> String {
                let Some(h) = p.hidden.get(&f).filter(|h| !h.is_empty()) else {
                    return "<table:data-pilot-level table:show-empty=\"false\"/>".into();
                };
                let mut m = String::from(
                    "<table:data-pilot-level table:show-empty=\"false\"><table:data-pilot-members>",
                );
                for name in h {
                    m.push_str(&format!(
                        "<table:data-pilot-member table:name=\"{}\" table:display=\"false\" table:show-details=\"true\"/>",
                        escape(name)
                    ));
                }
                m.push_str("</table:data-pilot-members></table:data-pilot-level>");
                m
            };
            let field = |f: usize, orientation: &str, extra: &str| -> String {
                format!(
                    "<table:data-pilot-field table:source-field-name=\"{}\" table:orientation=\"{orientation}\"{extra}>{}</table:data-pilot-field>",
                    escape(&names[f]),
                    if orientation == "data" { "<table:data-pilot-level table:show-empty=\"false\"/>".to_string() } else { members(f) }
                )
            };
            for f in &p.filters {
                out.push_str(&field(*f, "page", ""));
            }
            for f in &p.rows {
                out.push_str(&field(*f, "row", ""));
            }
            for f in &p.cols {
                out.push_str(&field(*f, "column", ""));
            }
            for (f, agg) in &p.values {
                out.push_str(&field(
                    *f,
                    "data",
                    &format!(" table:function=\"{}\"", agg.name()),
                ));
            }
            if p.values.len() > 1 {
                out.push_str("<table:data-pilot-field table:source-field-name=\"\" table:is-data-layout-field=\"true\" table:orientation=\"column\"><table:data-pilot-level table:show-empty=\"false\"/></table:data-pilot-field>");
            }
            out.push_str("</table:data-pilot-table>");
        }
    }
    if out.is_empty() {
        out
    } else {
        format!("<table:data-pilot-tables>{out}</table:data-pilot-tables>")
    }
}
/// Pivot tables of a spreadsheet, with the name of the sheet each sits on.
pub fn read_pivots(spreadsheet: &Element, wb: &Workbook) -> Vec<(String, Pivot)> {
    let Some(list) = spreadsheet.child("data-pilot-tables") else {
        return vec![];
    };
    let mut out = Vec::new();
    for t in list.children_named("data-pilot-table") {
        let Some((sheet, target)) = t.attr("target-range-address").and_then(parse_address) else {
            continue;
        };
        let Some((src_sheet, source)) = t
            .child("source-cell-range")
            .and_then(|s| s.attr("cell-range-address"))
            .and_then(parse_address)
        else {
            continue;
        };
        let Some(si) = wb.sheet_index(&src_sheet) else {
            continue;
        };
        let Ok(names) = crate::pivot::fields(wb, si, source) else {
            continue;
        };
        let mut p = Pivot {
            name: t.attr("name").unwrap_or("DataPilot1").to_owned(),
            source_sheet: src_sheet,
            source,
            at: target.start,
            rows: vec![],
            cols: vec![],
            values: vec![],
            filters: vec![],
            hidden: BTreeMap::new(),
            style: PivotStyle::Calc,
            auto: false,
            extent: Some(target),
        };
        for f in t.children_named("data-pilot-field") {
            if f.attr("is-data-layout-field") == Some("true") {
                continue;
            }
            let name = f.attr("source-field-name").unwrap_or("");
            let Some(k) = names.iter().position(|n| n == name) else {
                continue;
            };
            let hidden: BTreeSet<String> = f
                .find("data-pilot-members")
                .iter()
                .flat_map(|m| m.children_named("data-pilot-member"))
                .filter(|m| m.attr("display") == Some("false"))
                .filter_map(|m| m.attr("name").map(str::to_owned))
                .collect();
            if !hidden.is_empty() {
                p.hidden.insert(k, hidden);
            }
            match f.attr("orientation").unwrap_or("hidden") {
                "row" => p.rows.push(k),
                "column" => p.cols.push(k),
                "page" => p.filters.push(k),
                "data" => p.values.push((
                    k,
                    f.attr("function").and_then(Agg::parse).unwrap_or(Agg::Sum),
                )),
                _ => {}
            }
        }
        p.cols.truncate(1);
        if !p.filters.is_empty() {
            p.at.row += p.filters.len() as u32 + 1;
        }
        out.push((sheet, p));
    }
    out
}
