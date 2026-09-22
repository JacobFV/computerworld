//! The parts of SpreadsheetML beyond cells: conditional formatting rules and pivot
//! tables (a pivot cache definition with its records, and the pivot table definition
//! that lays it out), written as Excel writes them and read back.
use crate::address::{Cell, Range};
use crate::conditional::{CellOp, Cfvo, CondFormat, Dxf, Period, Rule, TextOp};
use crate::pivot::{item_label, Agg, Pivot, PivotStyle};
use crate::value::{compare, Value};
use crate::workbook::Workbook;
use crate::xml::{escape, Element};
use std::collections::{BTreeMap, BTreeSet};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

fn argb(c: [u8; 3]) -> String {
    format!("FF{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}
fn quoted(t: &str) -> String {
    format!("\"{}\"", t.replace('"', "\"\""))
}
fn cfvo_xml(v: &Cfvo) -> String {
    match v.xml() {
        (t, Some(val)) => format!("<cfvo type=\"{t}\" val=\"{}\"/>", escape(&val)),
        (t, None) => format!("<cfvo type=\"{t}\"/>"),
    }
}

// ----- conditional formatting -----

/// One `<conditionalFormatting>` element holding one rule.
pub fn conditional_xml(cf: &CondFormat, priority: usize, dxf: Option<usize>) -> String {
    let cell = cf.ranges.first().map_or("A1".to_string(), |r| r.start.a1());
    let mut attrs = String::new();
    let mut body = String::new();
    let formula = |f: &str| format!("<formula>{}</formula>", escape(f.trim_start_matches('=')));
    let kind = match &cf.rule {
        Rule::CellIs { op, formulas, .. } => {
            attrs.push_str(&format!(" operator=\"{}\"", op.name()));
            for f in formulas {
                body.push_str(&formula(f));
            }
            "cellIs"
        }
        Rule::Text { op, text, .. } => {
            let q = quoted(text);
            attrs.push_str(&format!(
                " operator=\"{}\" text=\"{}\"",
                op.name(),
                escape(text)
            ));
            body.push_str(&formula(&match op {
                TextOp::Contains => format!("NOT(ISERROR(SEARCH({q},{cell})))"),
                TextOp::NotContains => format!("ISERROR(SEARCH({q},{cell}))"),
                TextOp::BeginsWith => format!("LEFT({cell},LEN({q}))={q}"),
                TextOp::EndsWith => format!("RIGHT({cell},LEN({q}))={q}"),
            }));
            match op {
                TextOp::Contains => "containsText",
                TextOp::NotContains => "notContainsText",
                TextOp::BeginsWith => "beginsWith",
                TextOp::EndsWith => "endsWith",
            }
        }
        Rule::Dates { period, .. } => {
            attrs.push_str(&format!(" timePeriod=\"{}\"", period.name()));
            let d = format!("ROUNDDOWN({cell},0)");
            body.push_str(&formula(&match period {
                Period::Today => format!("FLOOR({cell},1)=TODAY()"),
                Period::Yesterday => format!("FLOOR({cell},1)=TODAY()-1"),
                Period::Tomorrow => format!("FLOOR({cell},1)=TODAY()+1"),
                Period::Last7Days => {
                    format!("AND(TODAY()-FLOOR({cell},1)<=6,FLOOR({cell},1)<=TODAY())")
                }
                Period::LastWeek => format!(
                    "AND(TODAY()-{d}>=(WEEKDAY(TODAY())),TODAY()-{d}<(WEEKDAY(TODAY())+7))"
                ),
                Period::ThisWeek => format!(
                    "AND(TODAY()-{d}<=WEEKDAY(TODAY())-1,{d}-TODAY()<=7-WEEKDAY(TODAY()))"
                ),
                Period::NextWeek => format!(
                    "AND({d}-TODAY()>(7-WEEKDAY(TODAY())),{d}-TODAY()<(15-WEEKDAY(TODAY())))"
                ),
                Period::LastMonth => format!("AND(MONTH({cell})=MONTH(EDATE(TODAY(),0-1)),YEAR({cell})=YEAR(EDATE(TODAY(),0-1)))"),
                Period::ThisMonth => {
                    format!("AND(MONTH({cell})=MONTH(TODAY()),YEAR({cell})=YEAR(TODAY()))")
                }
                Period::NextMonth => format!("AND(MONTH({cell})=MONTH(EDATE(TODAY(),0+1)),YEAR({cell})=YEAR(EDATE(TODAY(),0+1)))"),
            }));
            "timePeriod"
        }
        Rule::Duplicates { unique, .. } => {
            if *unique {
                "uniqueValues"
            } else {
                "duplicateValues"
            }
        }
        Rule::Top {
            bottom,
            rank,
            percent,
            ..
        } => {
            attrs.push_str(&format!(" rank=\"{rank}\""));
            if *percent {
                attrs.push_str(" percent=\"1\"");
            }
            if *bottom {
                attrs.push_str(" bottom=\"1\"");
            }
            "top10"
        }
        Rule::Average { below, equal, .. } => {
            if *below {
                attrs.push_str(" aboveAverage=\"0\"");
            }
            if *equal {
                attrs.push_str(" equalAverage=\"1\"");
            }
            "aboveAverage"
        }
        Rule::Blanks { blanks, .. } => {
            body.push_str(&formula(&format!(
                "LEN(TRIM({cell})){}0",
                if *blanks { "=" } else { ">" }
            )));
            if *blanks {
                "containsBlanks"
            } else {
                "notContainsBlanks"
            }
        }
        Rule::Errors { errors, .. } => {
            body.push_str(&formula(&if *errors {
                format!("ISERROR({cell})")
            } else {
                format!("NOT(ISERROR({cell}))")
            }));
            if *errors {
                "containsErrors"
            } else {
                "notContainsErrors"
            }
        }
        Rule::Expression { formula: f, .. } => {
            body.push_str(&formula(f));
            "expression"
        }
        Rule::DataBar { color, min, max } => {
            body.push_str(&format!(
                "<dataBar>{}{}<color rgb=\"{}\"/></dataBar>",
                cfvo_xml(min),
                cfvo_xml(max),
                argb(*color)
            ));
            "dataBar"
        }
        Rule::ColorScale { stops } => {
            body.push_str("<colorScale>");
            for (v, _) in stops {
                body.push_str(&cfvo_xml(v));
            }
            for (_, c) in stops {
                body.push_str(&format!("<color rgb=\"{}\"/>", argb(*c)));
            }
            body.push_str("</colorScale>");
            "colorScale"
        }
        Rule::IconSet {
            set,
            points,
            reverse,
            show_value,
        } => {
            body.push_str(&format!("<iconSet iconSet=\"{}\"", escape(set)));
            if *reverse {
                body.push_str(" reverse=\"1\"");
            }
            if !show_value {
                body.push_str(" showValue=\"0\"");
            }
            body.push('>');
            for p in points {
                body.push_str(&cfvo_xml(p));
            }
            body.push_str("</iconSet>");
            "iconSet"
        }
    };
    let dxf = dxf.map_or(String::new(), |d| format!(" dxfId=\"{d}\""));
    let stop = if cf.stop_if_true {
        " stopIfTrue=\"1\""
    } else {
        ""
    };
    format!(
        "<conditionalFormatting sqref=\"{}\"><cfRule type=\"{kind}\"{dxf} priority=\"{priority}\"{stop}{attrs}>{body}</cfRule></conditionalFormatting>",
        cf.sqref()
    )
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
/// The differential formats of a styles part.
pub fn read_dxfs(styles: Option<&Element>) -> Vec<Dxf> {
    let Some(list) = styles.and_then(|s| s.child("dxfs")) else {
        return vec![];
    };
    list.children_named("dxf")
        .map(|d| {
            let font = d.child("font");
            let on = |n: &str| {
                font.and_then(|f| f.child(n))
                    .is_some_and(|e| e.attr("val") != Some("0"))
            };
            let fill = d
                .child("fill")
                .and_then(|f| f.child("patternFill"))
                .and_then(|p| {
                    p.child("bgColor")
                        .or_else(|| p.child("fgColor"))
                        .and_then(|c| c.attr("rgb"))
                        .and_then(parse_rgb)
                });
            Dxf {
                bold: on("b"),
                italic: on("i"),
                underline: on("u"),
                color: font
                    .and_then(|f| f.child("color"))
                    .and_then(|c| c.attr("rgb"))
                    .and_then(parse_rgb),
                fill,
            }
        })
        .collect()
}
fn read_cfvo(e: &Element) -> Option<Cfvo> {
    Cfvo::from_xml(e.attr("type")?, e.attr("val"))
}
/// The rules of a worksheet's `<conditionalFormatting>` elements, in priority order.
pub fn read_conditional(ws: &Element, dxfs: &[Dxf]) -> Vec<CondFormat> {
    let mut out: Vec<(i64, CondFormat)> = Vec::new();
    for cf in ws.children_named("conditionalFormatting") {
        let ranges: Vec<Range> = cf
            .attr("sqref")
            .unwrap_or("")
            .split_whitespace()
            .filter_map(Range::parse)
            .collect();
        if ranges.is_empty() {
            continue;
        }
        for r in cf.children_named("cfRule") {
            let style = r
                .attr("dxfId")
                .and_then(|d| d.parse::<usize>().ok())
                .and_then(|d| dxfs.get(d).copied())
                .unwrap_or_default();
            let formulas: Vec<String> = r.children_named("formula").map(|f| f.text()).collect();
            let flag = |n: &str| matches!(r.attr(n), Some("1") | Some("true"));
            let rule = match r.attr("type").unwrap_or("") {
                "cellIs" => {
                    let Some(op) = r.attr("operator").and_then(CellOp::parse) else {
                        continue;
                    };
                    Rule::CellIs {
                        op,
                        formulas,
                        style,
                    }
                }
                t @ ("containsText" | "notContainsText" | "beginsWith" | "endsWith") => {
                    Rule::Text {
                        op: match t {
                            "containsText" => TextOp::Contains,
                            "notContainsText" => TextOp::NotContains,
                            "beginsWith" => TextOp::BeginsWith,
                            _ => TextOp::EndsWith,
                        },
                        text: r.attr("text").unwrap_or("").to_owned(),
                        style,
                    }
                }
                "timePeriod" => {
                    let Some(period) = r.attr("timePeriod").and_then(Period::parse) else {
                        continue;
                    };
                    Rule::Dates { period, style }
                }
                "duplicateValues" => Rule::Duplicates {
                    unique: false,
                    style,
                },
                "uniqueValues" => Rule::Duplicates {
                    unique: true,
                    style,
                },
                "top10" => Rule::Top {
                    bottom: flag("bottom"),
                    percent: flag("percent"),
                    rank: r.attr("rank").and_then(|v| v.parse().ok()).unwrap_or(10),
                    style,
                },
                "aboveAverage" => Rule::Average {
                    below: matches!(r.attr("aboveAverage"), Some("0") | Some("false")),
                    equal: flag("equalAverage"),
                    style,
                },
                "containsBlanks" => Rule::Blanks {
                    blanks: true,
                    style,
                },
                "notContainsBlanks" => Rule::Blanks {
                    blanks: false,
                    style,
                },
                "containsErrors" => Rule::Errors {
                    errors: true,
                    style,
                },
                "notContainsErrors" => Rule::Errors {
                    errors: false,
                    style,
                },
                "expression" => {
                    let Some(f) = formulas.into_iter().next() else {
                        continue;
                    };
                    Rule::Expression { formula: f, style }
                }
                "dataBar" => {
                    let Some(bar) = r.child("dataBar") else {
                        continue;
                    };
                    let v: Vec<Cfvo> = bar.children_named("cfvo").filter_map(read_cfvo).collect();
                    let color = bar
                        .child("color")
                        .and_then(|c| c.attr("rgb"))
                        .and_then(parse_rgb)
                        .unwrap_or([99, 142, 198]);
                    if v.len() != 2 {
                        continue;
                    }
                    Rule::DataBar {
                        color,
                        min: v[0].clone(),
                        max: v[1].clone(),
                    }
                }
                "colorScale" => {
                    let Some(cs) = r.child("colorScale") else {
                        continue;
                    };
                    let v: Vec<Cfvo> = cs.children_named("cfvo").filter_map(read_cfvo).collect();
                    let c: Vec<[u8; 3]> = cs
                        .children_named("color")
                        .map(|c| c.attr("rgb").and_then(parse_rgb).unwrap_or([255, 255, 255]))
                        .collect();
                    if v.len() != c.len() || !(2..=3).contains(&v.len()) {
                        continue;
                    }
                    Rule::ColorScale {
                        stops: v.into_iter().zip(c).collect(),
                    }
                }
                "iconSet" => {
                    let Some(is) = r.child("iconSet") else {
                        continue;
                    };
                    Rule::IconSet {
                        set: is.attr("iconSet").unwrap_or("3TrafficLights1").to_owned(),
                        points: is.children_named("cfvo").filter_map(read_cfvo).collect(),
                        reverse: matches!(is.attr("reverse"), Some("1") | Some("true")),
                        show_value: !matches!(is.attr("showValue"), Some("0") | Some("false")),
                    }
                }
                _ => continue,
            };
            let priority = r.attr("priority").and_then(|p| p.parse().ok()).unwrap_or(0);
            out.push((
                priority,
                CondFormat {
                    ranges: ranges.clone(),
                    rule,
                    stop_if_true: flag("stopIfTrue"),
                },
            ));
        }
    }
    out.sort_by_key(|(p, _)| *p);
    out.into_iter().map(|(_, cf)| cf).collect()
}

// ----- pivot tables -----

pub struct PivotParts {
    pub table: String,
    pub cache: String,
    pub records: String,
}
#[derive(Clone, Debug, PartialEq)]
struct Shared(Value);
impl Eq for Shared {}
impl PartialOrd for Shared {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Shared {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        match (self.0.is_empty(), o.0.is_empty()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => compare(&self.0, &o.0),
        }
    }
}
fn item_xml(v: &Value) -> String {
    match v {
        Value::Empty => "<m/>".into(),
        Value::Number(n) => format!("<n v=\"{}\"/>", crate::value::general(*n)),
        Value::Bool(b) => format!("<b v=\"{}\"/>", u8::from(*b)),
        Value::Error(e) => format!("<e v=\"{}\"/>", escape(e.code())),
        Value::Text(t) => format!("<s v=\"{}\"/>", escape(t)),
    }
}
/// A pivot table's three parts, for pivot cache `index` (0-based).
pub fn pivot_parts(wb: &Workbook, p: &Pivot, index: usize) -> Option<PivotParts> {
    let si = wb.sheet_index(&p.source_sheet)?;
    let names = crate::pivot::fields(wb, si, p.source).ok()?;
    let extent = p.extent?;
    let width = names.len();
    let records: Vec<Vec<Value>> = (p.source.start.row + 1..=p.source.end.row)
        .map(|r| {
            (0..width)
                .map(|k| wb.value(si, Cell::new(r, p.source.start.col + k as u32)))
                .collect()
        })
        .collect();
    // Shared items per field, in order of first appearance, as Excel lists them.
    let mut shared: Vec<Vec<Value>> = vec![vec![]; width];
    for rec in &records {
        for (k, v) in rec.iter().enumerate() {
            if !shared[k].iter().any(|s| s == v) {
                shared[k].push(v.clone());
            }
        }
    }
    let index_of = |k: usize, v: &Value| shared[k].iter().position(|s| s == v).unwrap_or(0);
    // Cache definition.
    let mut cache = format!(
        "{HEAD}<pivotCacheDefinition xmlns=\"{MAIN}\" xmlns:r=\"{REL}\" r:id=\"rId1\" refreshOnLoad=\"1\" createdVersion=\"8\" refreshedVersion=\"8\" minRefreshableVersion=\"3\" recordCount=\"{}\"><cacheSource type=\"worksheet\"><worksheetSource ref=\"{}\" sheet=\"{}\"/></cacheSource><cacheFields count=\"{width}\">",
        records.len(),
        p.source.a1(),
        escape(&p.source_sheet)
    );
    for (k, name) in names.iter().enumerate() {
        let items = &shared[k];
        let numbers: Vec<f64> = items
            .iter()
            .filter_map(|v| match v {
                Value::Number(n) => Some(*n),
                _ => None,
            })
            .collect();
        let has_text = items.iter().any(|v| matches!(v, Value::Text(_)));
        let has_blank = items.iter().any(Value::is_empty);
        let has_bool = items.iter().any(|v| matches!(v, Value::Bool(_)));
        let mut attrs = String::new();
        if !has_text {
            attrs.push_str(" containsSemiMixedTypes=\"0\" containsString=\"0\"");
        }
        if !numbers.is_empty() {
            if has_text || has_bool {
                attrs.push_str(" containsMixedTypes=\"1\"");
            }
            attrs.push_str(" containsNumber=\"1\"");
            if numbers.iter().all(|n| n.fract() == 0.0) {
                attrs.push_str(" containsInteger=\"1\"");
            }
            let lo = numbers.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            attrs.push_str(&format!(
                " minValue=\"{}\" maxValue=\"{}\"",
                crate::value::general(lo),
                crate::value::general(hi)
            ));
        }
        if has_blank {
            attrs.push_str(" containsBlank=\"1\"");
        }
        cache.push_str(&format!(
            "<cacheField name=\"{}\" numFmtId=\"0\"><sharedItems{attrs} count=\"{}\">",
            escape(name),
            items.len()
        ));
        for v in items {
            cache.push_str(&item_xml(v));
        }
        cache.push_str("</sharedItems></cacheField>");
    }
    cache.push_str("</cacheFields></pivotCacheDefinition>");
    let mut recs = format!(
        "{HEAD}<pivotCacheRecords xmlns=\"{MAIN}\" xmlns:r=\"{REL}\" count=\"{}\">",
        records.len()
    );
    for rec in &records {
        recs.push_str("<r>");
        for (k, v) in rec.iter().enumerate() {
            recs.push_str(&format!("<x v=\"{}\"/>", index_of(k, v)));
        }
        recs.push_str("</r>");
    }
    recs.push_str("</pivotCacheRecords>");
    // Items each axis field shows, sorted as the report sorts them.
    let sorted = |k: usize| -> Vec<usize> {
        let mut idx: Vec<usize> = (0..shared[k].len()).collect();
        idx.sort_by(|a, b| Shared(shared[k][*a].clone()).cmp(&Shared(shared[k][*b].clone())));
        idx
    };
    let hidden = |k: usize, i: usize| {
        p.hidden
            .get(&k)
            .is_some_and(|h| h.contains(&item_label(&shared[k][i])))
    };
    let visible = |rec: &Vec<Value>| {
        !p.hidden
            .iter()
            .any(|(k, h)| rec.get(*k).is_some_and(|v| h.contains(&item_label(v))))
    };
    let mut fields_xml = String::new();
    for (k, items) in shared.iter().enumerate() {
        let axis = if p.rows.contains(&k) {
            Some("axisRow")
        } else if p.cols.contains(&k) {
            Some("axisCol")
        } else if p.filters.contains(&k) {
            Some("axisPage")
        } else {
            None
        };
        let data = p.values.iter().any(|(f, _)| *f == k);
        let compact = if p.rows.len() > 1 || p.style != PivotStyle::Excel {
            " compact=\"0\" outline=\"0\""
        } else {
            ""
        };
        match axis {
            Some(a) => {
                fields_xml.push_str(&format!(
                    "<pivotField axis=\"{a}\"{}{compact} showAll=\"0\"><items count=\"{}\">",
                    if data { " dataField=\"1\"" } else { "" },
                    items.len() + 1
                ));
                for i in sorted(k) {
                    fields_xml.push_str(&format!(
                        "<item{} x=\"{i}\"/>",
                        if hidden(k, i) { " h=\"1\"" } else { "" }
                    ));
                }
                fields_xml.push_str("<item t=\"default\"/></items></pivotField>");
            }
            None => fields_xml.push_str(&format!(
                "<pivotField{}{compact} showAll=\"0\"/>",
                if data { " dataField=\"1\"" } else { "" }
            )),
        }
    }
    // Row items: every row the body lists, as indexes into each row field's items.
    let order_pos = |k: usize, v: &Value| -> usize {
        sorted(k)
            .iter()
            .position(|i| shared[k][*i] == *v)
            .unwrap_or(0)
    };
    let tuples: BTreeSet<Vec<Shared>> = records
        .iter()
        .filter(|r| visible(r))
        .map(|r| p.rows.iter().map(|f| Shared(r[*f].clone())).collect())
        .collect();
    let nv = p.values.len().max(1);
    let mut row_items = String::new();
    let mut row_count = 0;
    if p.rows.is_empty() {
        row_items.push_str("<i/>");
        row_count = 1;
    } else {
        let list: Vec<&Vec<Shared>> = tuples.iter().collect();
        let n = p.rows.len();
        for (idx, t) in list.iter().enumerate() {
            let prev = idx.checked_sub(1).map(|q| list[q]);
            let same = prev.map_or(0, |q| {
                q.iter().zip(t.iter()).take_while(|(a, b)| a == b).count()
            });
            let same = same.min(n - 1);
            row_items.push_str(&if same > 0 {
                format!("<i r=\"{same}\">")
            } else {
                "<i>".to_string()
            });
            for (lvl, s) in t.iter().enumerate().skip(same) {
                let pos = order_pos(p.rows[lvl], &s.0);
                row_items.push_str(&if pos == 0 {
                    "<x/>".to_string()
                } else {
                    format!("<x v=\"{pos}\"/>")
                });
            }
            row_items.push_str("</i>");
            row_count += 1;
            let next = list.get(idx + 1);
            for lvl in (0..n - 1).rev() {
                if next.is_none_or(|q| q[..=lvl] != t[..=lvl]) {
                    let pos = order_pos(p.rows[lvl], &t[lvl].0);
                    row_items.push_str(&format!(
                        "<i t=\"default\"{}><x{}/></i>",
                        if lvl > 0 {
                            format!(" r=\"{lvl}\"")
                        } else {
                            String::new()
                        },
                        if pos == 0 {
                            String::new()
                        } else {
                            format!(" v=\"{pos}\"")
                        }
                    ));
                    row_count += 1;
                }
            }
        }
        row_items.push_str("<i t=\"grand\"><x/></i>");
        row_count += 1;
    }
    // Column items: each column item (with each value field), then the grand totals.
    let mut col_items = String::new();
    let mut col_count = 0;
    let xs = |v: usize| {
        if v == 0 {
            "<x/>".to_string()
        } else {
            format!("<x v=\"{v}\"/>")
        }
    };
    if let Some(cf) = p.cols.first() {
        let items: BTreeSet<Shared> = records
            .iter()
            .filter(|r| visible(r))
            .map(|r| Shared(r[*cf].clone()))
            .collect();
        for it in &items {
            let pos = order_pos(*cf, &it.0);
            for d in 0..nv {
                if nv == 1 {
                    col_items.push_str(&format!("<i>{}</i>", xs(pos)));
                } else if d == 0 {
                    col_items.push_str(&format!("<i>{}{}</i>", xs(pos), xs(0)));
                } else {
                    col_items.push_str(&format!("<i r=\"1\" i=\"{d}\">{}</i>", xs(d)));
                }
                col_count += 1;
            }
        }
        for d in 0..nv {
            col_items.push_str(&if d == 0 {
                "<i t=\"grand\"><x/></i>".to_string()
            } else {
                format!("<i t=\"grand\" i=\"{d}\"><x/></i>")
            });
            col_count += 1;
        }
    } else if nv > 1 {
        for d in 0..nv {
            col_items.push_str(&if d == 0 {
                "<i><x/></i>".to_string()
            } else {
                format!("<i i=\"{d}\">{}</i>", xs(d))
            });
            col_count += 1;
        }
    } else {
        col_items.push_str("<i/>");
        col_count = 1;
    }
    let label_cols = if p.rows.is_empty() {
        0
    } else if p.rows.len() > 1 || p.style != PivotStyle::Excel {
        p.rows.len()
    } else {
        1
    };
    let header_rows = if p.cols.is_empty() {
        1
    } else if p.values.len() > 1 {
        3
    } else {
        2
    };
    let location = Range::new(p.at, extent.end);
    let mut t = format!(
        "{HEAD}<pivotTableDefinition xmlns=\"{MAIN}\" name=\"{}\" cacheId=\"{}\" applyNumberFormats=\"0\" applyBorderFormats=\"0\" applyFontFormats=\"0\" applyPatternFormats=\"0\" applyAlignmentFormats=\"0\" applyWidthHeightFormats=\"1\" dataCaption=\"Values\" grandTotalCaption=\"{}\" updatedVersion=\"8\" minRefreshableVersion=\"3\" useAutoFormatting=\"1\" itemPrintTitles=\"1\" createdVersion=\"8\" indent=\"0\"{} multipleFieldFilters=\"0\">",
        escape(&p.name),
        index + 1,
        if p.style == PivotStyle::Calc { "Total Result" } else { "Grand Total" },
        if label_cols > 1 || p.style != PivotStyle::Excel {
            " compact=\"0\" compactData=\"0\" outline=\"1\" outlineData=\"1\" gridDropZones=\"1\""
        } else {
            " outline=\"1\" outlineData=\"1\""
        }
    );
    t.push_str(&format!(
        "<location ref=\"{}\" firstHeaderRow=\"1\" firstDataRow=\"{}\" firstDataCol=\"{label_cols}\"{}/>",
        location.a1(),
        header_rows,
        if p.filters.is_empty() {
            String::new()
        } else {
            format!(" rowPageCount=\"{}\" colPageCount=\"1\"", p.filters.len())
        }
    ));
    t.push_str(&format!(
        "<pivotFields count=\"{width}\">{fields_xml}</pivotFields>"
    ));
    if !p.rows.is_empty() {
        t.push_str(&format!("<rowFields count=\"{}\">", p.rows.len()));
        for f in &p.rows {
            t.push_str(&format!("<field x=\"{f}\"/>"));
        }
        t.push_str("</rowFields>");
    }
    t.push_str(&format!(
        "<rowItems count=\"{row_count}\">{row_items}</rowItems>"
    ));
    let mut col_fields: Vec<i64> = p.cols.iter().map(|c| *c as i64).collect();
    if p.values.len() > 1 {
        col_fields.push(-2);
    }
    if !col_fields.is_empty() {
        t.push_str(&format!("<colFields count=\"{}\">", col_fields.len()));
        for f in col_fields {
            t.push_str(&format!("<field x=\"{f}\"/>"));
        }
        t.push_str("</colFields>");
    }
    t.push_str(&format!(
        "<colItems count=\"{col_count}\">{col_items}</colItems>"
    ));
    if !p.filters.is_empty() {
        t.push_str(&format!("<pageFields count=\"{}\">", p.filters.len()));
        for f in &p.filters {
            t.push_str(&format!("<pageField fld=\"{f}\" hier=\"-1\"/>"));
        }
        t.push_str("</pageFields>");
    }
    if !p.values.is_empty() {
        t.push_str(&format!("<dataFields count=\"{}\">", p.values.len()));
        for (f, agg) in &p.values {
            t.push_str(&format!(
                "<dataField name=\"{}\" fld=\"{f}\"{} baseField=\"0\" baseItem=\"0\"/>",
                escape(&p.caption(&names[*f], *agg)),
                if *agg == Agg::Sum {
                    String::new()
                } else {
                    format!(" subtotal=\"{}\"", agg.name())
                }
            ));
        }
        t.push_str("</dataFields>");
    }
    t.push_str("<pivotTableStyleInfo name=\"PivotStyleLight16\" showRowHeaders=\"1\" showColHeaders=\"1\" showRowStripes=\"0\" showColStripes=\"0\" showLastColumn=\"1\"/></pivotTableDefinition>");
    Some(PivotParts {
        table: t,
        cache,
        records: recs,
    })
}
/// A pivot table from its definition and its cache definition.
pub fn read_pivot(def: &Element, cache: &Element) -> Option<Pivot> {
    let source = cache.find("worksheetSource")?;
    let range = Range::parse(source.attr("ref")?)?;
    let sheet = source.attr("sheet")?.to_owned();
    let shared: Vec<Vec<Value>> = cache
        .child("cacheFields")?
        .children_named("cacheField")
        .map(|f| {
            f.child("sharedItems")
                .map(|s| {
                    s.elements()
                        .map(|e| {
                            let v = e.attr("v").unwrap_or("");
                            match e.local() {
                                "n" => v.parse::<f64>().map(Value::Number).unwrap_or_default(),
                                "b" => Value::Bool(v == "1" || v == "true"),
                                "m" => Value::Empty,
                                _ => Value::Text(v.to_owned()),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    let at = Range::parse(def.child("location")?.attr("ref")?)?.start;
    let fields = |name: &str| -> Vec<usize> {
        def.child(name)
            .map(|f| {
                f.elements()
                    .filter_map(|x| x.attr("x").and_then(|v| v.parse::<i64>().ok()))
                    .filter(|x| *x >= 0)
                    .map(|x| x as usize)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut hidden: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    if let Some(pf) = def.child("pivotFields") {
        for (k, f) in pf.children_named("pivotField").enumerate() {
            for it in f
                .find("items")
                .iter()
                .flat_map(|i| i.children_named("item"))
            {
                if matches!(it.attr("h"), Some("1") | Some("true")) {
                    if let Some(v) = it
                        .attr("x")
                        .and_then(|x| x.parse::<usize>().ok())
                        .and_then(|x| shared.get(k)?.get(x))
                    {
                        hidden.entry(k).or_default().insert(item_label(v));
                    }
                }
            }
        }
    }
    let values = def
        .child("dataFields")
        .map(|d| {
            d.children_named("dataField")
                .filter_map(|f| {
                    let fld = f.attr("fld")?.parse::<usize>().ok()?;
                    let agg = f.attr("subtotal").and_then(Agg::parse).unwrap_or(Agg::Sum);
                    Some((fld, agg))
                })
                .collect()
        })
        .unwrap_or_default();
    let filters = def
        .child("pageFields")
        .map(|d| {
            d.children_named("pageField")
                .filter_map(|f| f.attr("fld")?.parse::<usize>().ok())
                .collect()
        })
        .unwrap_or_default();
    let style = if def.attr("grandTotalCaption") == Some("Total Result") {
        PivotStyle::Calc
    } else {
        PivotStyle::Excel
    };
    Some(Pivot {
        name: def.attr("name").unwrap_or("PivotTable1").to_owned(),
        source_sheet: sheet,
        source: range,
        at,
        rows: fields("rowFields"),
        cols: fields("colFields").into_iter().take(1).collect(),
        values,
        filters,
        hidden,
        style,
        auto: false,
        extent: None,
    })
}
