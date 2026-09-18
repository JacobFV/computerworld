//! Exports from the schematic: KiCad's S-expression netlist (the `.net` file Pcbnew and
//! CvPcb read), a SPICE deck for the simulator, and the bill of materials as CSV.
use crate::connectivity::{analyze, natural, Connectivity};
use crate::num::{format_si, parse_value};
use crate::schematic::Schematic;
use crate::sexpr::{self, Sexp};
use crate::symbols::Spice;
use std::collections::BTreeMap;

fn s(text: &str) -> Sexp {
    Sexp::string(text)
}
fn n(head: &str, args: Vec<Sexp>) -> Sexp {
    Sexp::node(head, args)
}

/// KiCad netlist, export format version "E".
pub fn kicad_netlist(sch: &Schematic, source: &str, date: &str) -> String {
    let conn = analyze(sch);
    let file = source.rsplit('/').next().unwrap_or(source);
    let mut comps = vec![];
    let mut symbols: Vec<_> = sch
        .symbols
        .iter()
        .filter(|s| !s.is_power() && s.on_board)
        .collect();
    symbols.sort_by_key(|s| natural(s.reference()));
    for sym in &symbols {
        let lib = sym.lib();
        let mut fields = vec![];
        for f in &sym.fields {
            if matches!(f.name.as_str(), "Reference" | "Value") {
                continue;
            }
            fields.push(n("field", vec![n("name", vec![s(&f.name)]), s(&f.value)]));
        }
        let (libname, part) = sym.lib_id.split_once(':').unwrap_or(("", &sym.lib_id));
        comps.push(n(
            "comp",
            vec![
                n("ref", vec![s(sym.reference())]),
                n("value", vec![s(sym.value())]),
                n("footprint", vec![s(sym.field("Footprint"))]),
                n("datasheet", vec![s(sym.field("Datasheet"))]),
                n("description", vec![s(lib.map_or("", |l| l.description))]),
                n("fields", fields),
                n(
                    "libsource",
                    vec![
                        n("lib", vec![s(libname)]),
                        n("part", vec![s(part)]),
                        n("description", vec![s(lib.map_or("", |l| l.description))]),
                    ],
                ),
                n(
                    "property",
                    vec![n("name", vec![s("Sheetname")]), n("value", vec![s("Root")])],
                ),
                n(
                    "sheetpath",
                    vec![n("names", vec![s("/")]), n("tstamps", vec![s("/")])],
                ),
                n(
                    "tstamps",
                    vec![s(&crate::schematic::item_uuid(&sch.uuid, sym.id))],
                ),
            ],
        ));
    }
    let mut libparts = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for sym in &symbols {
        let Some(lib) = sym.lib() else { continue };
        if !seen.insert(lib.lib_id) {
            continue;
        }
        let pins = lib
            .pins
            .iter()
            .map(|p| {
                n(
                    "pin",
                    vec![
                        n("num", vec![s(p.number)]),
                        n("name", vec![s(p.name)]),
                        n("type", vec![s(p.kind.keyword())]),
                    ],
                )
            })
            .collect();
        libparts.push(n(
            "libpart",
            vec![
                n("lib", vec![s(lib.library())]),
                n("part", vec![s(lib.name())]),
                n("description", vec![s(lib.description)]),
                n("docs", vec![s(lib.datasheet)]),
                n("pins", pins),
            ],
        ));
    }
    let mut nets = vec![];
    for net in &conn.nets {
        let mut items = vec![
            n("code", vec![s(&net.code.to_string())]),
            n("name", vec![s(&net.name)]),
            n("class", vec![s("Default")]),
        ];
        for p in net.pins.iter().filter(|p| !p.power) {
            if !symbols.iter().any(|s| s.id == p.symbol) {
                continue;
            }
            let mut node = vec![
                n("ref", vec![s(&p.reference)]),
                n("pin", vec![s(&p.number)]),
            ];
            if !p.name.is_empty() && p.name != "~" {
                node.push(n("pinfunction", vec![s(&p.name)]));
            }
            node.push(n("pintype", vec![s(p.kind.keyword())]));
            items.push(n("node", node));
        }
        nets.push(n("net", items));
    }
    let tb = &sch.title_block;
    let doc = n(
        "export",
        vec![
            n("version", vec![s("E")]),
            n(
                "design",
                vec![
                    n("source", vec![s(source)]),
                    n("date", vec![s(date)]),
                    n("tool", vec![s("Eeschema 8.0.4")]),
                    n(
                        "sheet",
                        vec![
                            n("number", vec![s("1")]),
                            n("name", vec![s("/")]),
                            n("tstamps", vec![s("/")]),
                            n(
                                "title_block",
                                vec![
                                    n("title", vec![s(&tb.title)]),
                                    n("company", vec![s(&tb.company)]),
                                    n("rev", vec![s(&tb.rev)]),
                                    n("date", vec![s(&tb.date)]),
                                    n("source", vec![s(file)]),
                                ],
                            ),
                        ],
                    ),
                ],
            ),
            n("components", comps),
            n("libparts", libparts),
            n("nets", nets),
        ],
    );
    doc.to_text()
}

/// A parsed netlist: components (reference, value, footprint) and nets with their nodes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedNetlist {
    pub components: Vec<(String, String, String)>,
    pub nets: Vec<(String, Vec<(String, String)>)>,
}
pub fn parse_kicad_netlist(text: &str) -> Result<ParsedNetlist, String> {
    let doc = sexpr::parse(text)?;
    if doc.head() != Some("export") {
        return Err("not a KiCad netlist (no (export …) root)".into());
    }
    let mut out = ParsedNetlist::default();
    if let Some(comps) = doc.child("components") {
        for c in comps.children("comp") {
            out.components.push((
                c.value_of("ref").unwrap_or("").into(),
                c.value_of("value").unwrap_or("").into(),
                c.value_of("footprint").unwrap_or("").into(),
            ));
        }
    }
    if let Some(nets) = doc.child("nets") {
        for net in nets.children("net") {
            let nodes = net
                .children("node")
                .map(|node| {
                    (
                        node.value_of("ref").unwrap_or("").to_owned(),
                        node.value_of("pin").unwrap_or("").to_owned(),
                    )
                })
                .collect();
            out.nets
                .push((net.value_of("name").unwrap_or("").to_owned(), nodes));
        }
    }
    Ok(out)
}

/// A value as written on a schematic (KiCad notation: `M` is mega, `4k7` allowed) in
/// SPICE notation, where `M` would mean milli.
pub fn spice_value(value: &str) -> Option<String> {
    let v = value.trim();
    let digits_end = v
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')
        })
        .unwrap_or(v.len());
    let after = &v[digits_end..];
    // KiCad reads "M" as mega; SPICE would read it as milli.
    let number = if after.starts_with('M') && !after.to_ascii_lowercase().starts_with("meg") {
        parse_value(&format!("{}meg{}", &v[..digits_end], &after[1..]))?
    } else {
        parse_value(v)?
    };
    Some(spice_number(number))
}
pub fn spice_number(v: f64) -> String {
    format_si(v, 6, "").replace('µ', "u").replace('M', "Meg")
}

/// SPICE node name for a net: ground is node 0, and characters SPICE cannot take in a
/// name become underscores (KiCad writes `Net-(R1-Pad1)` as `Net-_R1-Pad1_`).
pub fn spice_node(net: &str) -> String {
    if net == "GND" || net == "0" || net == "GNDA" || net == "GNDD" {
        return "0".into();
    }
    net.chars()
        .map(|c| match c {
            '(' | ')' | ' ' | ',' | '=' | '{' | '}' | '[' | ']' | '"' | '\'' => '_',
            c => c,
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpiceDeck {
    pub text: String,
    /// Schematic net name → the node it became.
    pub nodes: BTreeMap<String, String>,
    /// Symbols left out of the deck and why.
    pub skipped: Vec<String>,
}

/// Build the SPICE deck. Fails, naming the symbol, when something that is part of the
/// simulation has no model.
pub fn spice_netlist(
    sch: &Schematic,
    title: &str,
    command: Option<&str>,
) -> Result<SpiceDeck, String> {
    let conn = analyze(sch);
    spice_netlist_with(sch, &conn, title, command)
}

pub fn spice_netlist_with(
    sch: &Schematic,
    conn: &Connectivity,
    title: &str,
    command: Option<&str>,
) -> Result<SpiceDeck, String> {
    let mut deck = SpiceDeck::default();
    let mut lines = vec![format!(".title {title}")];
    let mut models = vec![];
    let mut body = vec![];
    let mut needs_opamp = None;
    let mut problems = vec![];
    for net in &conn.nets {
        deck.nodes.insert(net.name.clone(), spice_node(&net.name));
    }
    let mut syms: Vec<_> = sch.symbols.iter().collect();
    syms.sort_by_key(|s| natural(s.reference()));
    for sym in syms {
        let Some(lib) = sym.lib() else {
            problems.push(format!(
                "{} uses {}, which is not in the library",
                sym.reference(),
                sym.lib_id
            ));
            continue;
        };
        if lib.spice == Spice::Power || lib.lib_id == "power:PWR_FLAG" {
            continue;
        }
        let reference = sym.reference().to_owned();
        if sym.exclude_from_sim {
            deck.skipped
                .push(format!("{reference} is excluded from simulation"));
            continue;
        }
        if !sym.annotated() {
            problems.push(format!(
                "{reference} is not annotated; annotate the schematic first"
            ));
            continue;
        }
        let node = |number: &str| -> String {
            conn.net_of_pin(sym.id, number)
                .map(|n| spice_node(&n.name))
                .unwrap_or_else(|| format!("NC_{reference}_{number}"))
        };
        let named = |letter: char| -> String {
            if reference.to_ascii_uppercase().starts_with(letter) {
                reference.clone()
            } else {
                format!("{letter}{reference}")
            }
        };
        let params = sym.field("Sim.Params").to_owned();
        let value = sym.value().to_owned();
        let passive =
            |letter: char, body: &mut Vec<String>, problems: &mut Vec<String>| match spice_value(
                &value,
            ) {
                Some(v) => body.push(format!("{} {} {} {v}", named(letter), node("1"), node("2"))),
                None => problems.push(format!(
                    "{reference} has value \"{value}\", which is not a number"
                )),
            };
        match lib.spice {
            Spice::Resistor => passive('R', &mut body, &mut problems),
            Spice::Capacitor => passive('C', &mut body, &mut problems),
            Spice::Inductor => passive('L', &mut body, &mut problems),
            Spice::Diode => {
                let model = format!("__{reference}");
                body.push(format!("{} {} {} {model}", named('D'), node("2"), node("1")));
                models.push(format!(".model {model} D({params})"));
            }
            Spice::Npn | Spice::Pnp => {
                let model = format!("__{reference}");
                body.push(format!("{} {} {} {} {model}", named('Q'), node("2"), node("1"), node("3")));
                let kind = if lib.spice == Spice::Npn { "NPN" } else { "PNP" };
                models.push(format!(".model {model} {kind}({params})"));
            }
            Spice::Nmos | Spice::Pmos => {
                let model = format!("__{reference}");
                let s_ = node("3");
                body.push(format!("{} {} {} {s_} {s_} {model}", named('M'), node("2"), node("1")));
                let kind = if lib.spice == Spice::Nmos { "NMOS" } else { "PMOS" };
                models.push(format!(".model {model} {kind}(level=1 {params})"));
            }
            Spice::OpAmp => {
                let gain = params
                    .split_whitespace()
                    .find_map(|p| p.strip_prefix("gain="))
                    .and_then(spice_value)
                    .unwrap_or_else(|| "100k".into());
                needs_opamp = Some(gain);
                body.push(format!(
                    "X{reference} {} {} {} {} {} kicad_builtin_opamp",
                    node("1"),
                    node("2"),
                    node("3"),
                    node("4"),
                    node("5")
                ));
            }
            Spice::VoltageSource | Spice::CurrentSource => {
                let letter = if lib.spice == Spice::VoltageSource { 'V' } else { 'I' };
                let spec = match spice_value(&value) {
                    Some(v) => format!("dc {v}"),
                    None => value.clone(),
                };
                body.push(format!("{} {} {} {spec}", named(letter), node("1"), node("2")));
            }
            Spice::None => problems.push(format!(
                "{reference} ({}) has no simulation model; exclude it from simulation in its properties",
                lib.name()
            )),
            Spice::Power => {}
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    if let Some(gain) = needs_opamp {
        lines.push(".subckt kicad_builtin_opamp in+ in- vcc vee out".into());
        lines.push(format!("E1 out 0 in+ in- {gain}"));
        lines.push(".ends".into());
    }
    lines.extend(models);
    lines.extend(body);
    // Directives placed on the sheet as text travel with the deck.
    for t in &sch.texts {
        let l = t.text.trim();
        if l.starts_with('.') && !l.to_ascii_lowercase().starts_with(".end") {
            let is_analysis = [".tran", ".ac", ".dc", ".op"]
                .iter()
                .any(|k| l.to_ascii_lowercase().starts_with(k));
            if !is_analysis || command.is_none() {
                lines.push(l.to_owned());
            }
        }
    }
    if let Some(c) = command {
        lines.push(c.to_owned());
    }
    lines.push(".end".into());
    deck.text = lines.join("\n") + "\n";
    Ok(deck)
}

fn csv(field: &str) -> String {
    format!("\"{}\"", field.replace('"', "\"\""))
}

/// Bill of materials, grouped the way KiCad's Symbol Fields Table groups by default:
/// by value and footprint. Power symbols and simulation-only parts are not parts.
pub fn bom_rows(sch: &Schematic) -> Vec<(Vec<String>, String, String, String, bool)> {
    let mut groups: BTreeMap<(String, String, String, bool), Vec<String>> = BTreeMap::new();
    for sym in &sch.symbols {
        if sym.is_power() || !sym.in_bom {
            continue;
        }
        groups
            .entry((
                sym.value().to_owned(),
                sym.field("Footprint").to_owned(),
                sym.field("Datasheet").to_owned(),
                sym.dnp,
            ))
            .or_default()
            .push(sym.reference().to_owned());
    }
    let mut rows: Vec<_> = groups
        .into_iter()
        .map(|((value, fp, ds, dnp), mut refs)| {
            refs.sort_by_key(|r| natural(r));
            (refs, value, ds, fp, dnp)
        })
        .collect();
    rows.sort_by_key(|r| natural(r.0.first().map(String::as_str).unwrap_or("")));
    rows
}
pub fn bom_csv(sch: &Schematic) -> String {
    let mut out =
        String::from("\"Reference\",\"Value\",\"Datasheet\",\"Footprint\",\"Qty\",\"DNP\"\n");
    for (refs, value, ds, fp, dnp) in bom_rows(sch) {
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            csv(&refs.join(",")),
            csv(&value),
            csv(&ds),
            csv(&fp),
            csv(&refs.len().to_string()),
            csv(if dnp { "DNP" } else { "" })
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schematic_values_become_spice_numbers() {
        assert_eq!(spice_value("10k").as_deref(), Some("10k"));
        assert_eq!(spice_value("4k7").as_deref(), Some("4.7k"));
        assert_eq!(
            spice_value("1M").as_deref(),
            Some("1Meg"),
            "KiCad's M is mega"
        );
        assert_eq!(spice_value("1m").as_deref(), Some("1m"));
        assert_eq!(spice_value("100nF").as_deref(), Some("100n"));
        assert_eq!(spice_value("2.2u").as_deref(), Some("2.2u"));
        assert_eq!(spice_node("Net-(R1-Pad1)"), "Net-_R1-Pad1_");
        assert_eq!(spice_node("GND"), "0");
    }
}
