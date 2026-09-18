//! Exports from the schematic: KiCad's S-expression netlist (the `.net` file Pcbnew and
//! CvPcb read), a SPICE deck for the simulator, and the bill of materials as CSV. Every
//! export covers the whole sheet hierarchy; the units of a multi-unit part are one
//! component.
use crate::connectivity::{analyze, natural, Connectivity};
use crate::num::{format_si, parse_value};
use crate::schematic::{item_uuid, Schematic, SymbolInst};
use crate::sexpr::{self, Sexp};
use crate::symbols::{LibSymbol, Spice};
use std::collections::{BTreeMap, BTreeSet};

fn s(text: &str) -> Sexp {
    Sexp::string(text)
}
fn n(head: &str, args: Vec<Sexp>) -> Sexp {
    Sexp::node(head, args)
}

/// One component of the design: the first unit of each reference, with the sheet it is
/// on and its design-wide identity.
pub fn components(sch: &Schematic) -> Vec<(u64, String, &SymbolInst)> {
    let mut seen = BTreeSet::new();
    let mut out: Vec<(u64, String, &SymbolInst)> = sch
        .all_symbols()
        .into_iter()
        .filter(|(_, _, s)| {
            // Unannotated symbols ("R?") are all distinct.
            !s.annotated() || seen.insert(s.reference().to_owned())
        })
        .collect();
    out.sort_by_key(|a| natural(a.2.reference()));
    out
}

/// KiCad netlist, export format version "E".
pub fn kicad_netlist(sch: &Schematic, source: &str, date: &str) -> String {
    let conn = analyze(sch);
    let file = source.rsplit('/').next().unwrap_or(source);
    let mut comps = vec![];
    let symbols: Vec<_> = components(sch)
        .into_iter()
        .filter(|(_, _, s)| !s.is_power() && s.on_board)
        .collect();
    let sheets = sch.sheets_flat();
    for (_, path, sym) in &symbols {
        let lib = sym.lib();
        let mut fields = vec![];
        for f in &sym.fields {
            if matches!(f.name.as_str(), "Reference" | "Value") {
                continue;
            }
            fields.push(n("field", vec![n("name", vec![s(&f.name)]), s(&f.value)]));
        }
        let (libname, part) = sym.lib_id.split_once(':').unwrap_or(("", &sym.lib_id));
        let sheet_doc = sheets
            .iter()
            .find(|(p, ..)| p == path)
            .map(|(_, _, _, s)| s.uuid.clone())
            .unwrap_or_default();
        let sheet_name = if path == "/" {
            "Root".to_owned()
        } else {
            path.trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_owned()
        };
        comps.push(n(
            "comp",
            vec![
                n("ref", vec![s(sym.reference())]),
                n("value", vec![s(sym.value())]),
                n("footprint", vec![s(sym.field("Footprint"))]),
                n("datasheet", vec![s(sym.field("Datasheet"))]),
                n("description", vec![s(lib.map_or("", |l| &l.description))]),
                n("fields", fields),
                n(
                    "libsource",
                    vec![
                        n("lib", vec![s(libname)]),
                        n("part", vec![s(part)]),
                        n("description", vec![s(lib.map_or("", |l| &l.description))]),
                    ],
                ),
                n(
                    "property",
                    vec![
                        n("name", vec![s("Sheetname")]),
                        n("value", vec![s(&sheet_name)]),
                    ],
                ),
                n(
                    "sheetpath",
                    vec![n("names", vec![s(path)]), n("tstamps", vec![s(path)])],
                ),
                n("tstamps", vec![s(&item_uuid(&sheet_doc, sym.id))]),
            ],
        ));
    }
    let mut libparts = vec![];
    let mut seen = BTreeSet::new();
    for (_, _, sym) in &symbols {
        let Some(lib) = sym.lib() else { continue };
        if !seen.insert(lib.lib_id.clone()) {
            continue;
        }
        let pins = lib
            .pins
            .iter()
            .map(|p| {
                n(
                    "pin",
                    vec![
                        n("num", vec![s(&p.number)]),
                        n("name", vec![s(&p.name)]),
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
                n("description", vec![s(&lib.description)]),
                n("docs", vec![s(&lib.datasheet)]),
                n("pins", pins),
            ],
        ));
    }
    let on_board: BTreeSet<&str> = symbols.iter().map(|(_, _, s)| s.reference()).collect();
    let mut nets = vec![];
    for net in &conn.nets {
        let mut items = vec![
            n("code", vec![s(&net.code.to_string())]),
            n("name", vec![s(&net.name)]),
            n("class", vec![s("Default")]),
        ];
        let mut listed = BTreeSet::new();
        for p in net.pins.iter().filter(|p| !p.power) {
            if !on_board.contains(p.reference.as_str())
                || !listed.insert((p.reference.clone(), p.number.clone()))
            {
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
    let sheet_nodes: Vec<Sexp> = sheets
        .iter()
        .enumerate()
        .map(|(i, (path, _, _, sheet))| {
            n(
                "sheet",
                vec![
                    n("number", vec![s(&(i + 1).to_string())]),
                    n("name", vec![s(path)]),
                    n("tstamps", vec![s(path)]),
                    n(
                        "title_block",
                        vec![
                            n("title", vec![s(&sheet.title_block.title)]),
                            n("company", vec![s(&tb.company)]),
                            n("rev", vec![s(&tb.rev)]),
                            n("date", vec![s(&tb.date)]),
                            n("source", vec![s(file)]),
                        ],
                    ),
                ],
            )
        })
        .collect();
    let mut design = vec![
        n("source", vec![s(source)]),
        n("date", vec![s(date)]),
        n("tool", vec![s("Eeschema 8.0.4")]),
    ];
    design.extend(sheet_nodes);
    let doc = n(
        "export",
        vec![
            n("version", vec![s("E")]),
            n("design", design),
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

/// The shared input and output stages of the logic models: inputs compared with half
/// the supply (a CMOS input's switching point), outputs a push-pull pair of switches to
/// the supply pins.
const LOGIC_MODELS: &str = "\
.model kicad_logic_in adc_bridge(in_low=0 in_high=0 rise_delay=0.1n fall_delay=0.1n)
.model kicad_logic_out dac_bridge(out_low=0 out_high=1 out_undef=0.5 t_rise=1n t_fall=1n)
.model kicad_logic_hi sw(vt=0.5 vh=0.1 ron=25 roff=1G)
.model kicad_logic_lo sw(vt=-0.5 vh=0.1 ron=25 roff=1G)";

fn gate_subckt(code: &str, two_inputs: bool) -> String {
    let (ports, sense, adc, ins) = if two_inputs {
        (
            "a b y vcc gnd",
            "Ea da 0 a mid 1\nEb db 0 b mid 1",
            "Ain [da db] [ai bi] kicad_logic_in",
            "[ai bi]",
        )
    } else {
        (
            "a y vcc gnd",
            "Ea da 0 a mid 1",
            "Ain [da] [ai] kicad_logic_in",
            "ai",
        )
    };
    format!(
        ".subckt kicad_logic_{code} {ports}\n\
         Rm1 vcc mid 1Meg\nRm2 mid gnd 1Meg\n{sense}\n{adc}\n\
         Agate {ins} yo kicad_logic_{code}_fn\n\
         Aout [yo] [yc] kicad_logic_out\n\
         Shi vcc y yc 0 kicad_logic_hi\nSlo y gnd 0 yc kicad_logic_lo\n\
         .ends\n\
         .model kicad_logic_{code}_fn {code}(rise_delay=3n fall_delay=3n)"
    )
}

const DFF_SUBCKT: &str = "\
.subckt kicad_logic_dff d clk q vcc gnd
Rm1 vcc mid 1Meg
Rm2 mid gnd 1Meg
Ed dd 0 d mid 1
Ec dc 0 clk mid 1
Ain [dd dc] [di ci] kicad_logic_in
Aff di ci NULL NULL qo NULL kicad_logic_dff_fn
Aout [qo] [qc] kicad_logic_out
Shi vcc q qc 0 kicad_logic_hi
Slo q gnd 0 qc kicad_logic_lo
.ends
.model kicad_logic_dff_fn d_dff(clk_delay=4n set_delay=4n reset_delay=4n ic=0)";

/// The NE555 as its datasheet's block diagram draws it: a 5k/5k/5k divider setting the
/// thresholds at ⅔ and ⅓ of VCC (CV brings out the upper one), two comparators, the
/// latch (the trigger sets it and wins over the threshold, the threshold resets it, RESET
/// below 0.7 V clears it), the output stage and the discharge transistor it switches.
pub const NE555_SUBCKT: &str = "\
.subckt kicad_ne555 gnd trig out reset ctrl thres disch vcc
R1 vcc ctrl 5k
R2 ctrl lo 5k
R3 lo gnd 5k
Ethr dthr 0 thres ctrl 1
Etrg dtrg 0 lo trig 1
Erst drst 0 reset gnd 1
Acmp [dthr dtrg] [thr trg] kicad_555_cmp
Arst [drst] [run] kicad_555_rst
Aen hi kicad_555_hi
Ares [thr ~trg] rth kicad_555_and
Aclr [~run] clr kicad_555_buf
Alatch trg rth hi NULL clr q nq kicad_555_latch
Aout [q] [qc] kicad_555_out
Shi vcc out qc 0 kicad_555_on
Slo out gnd 0 qc kicad_555_off
Sdis disch gnd 0 qc kicad_555_off
.ends
.model kicad_555_cmp adc_bridge(in_low=0 in_high=0 rise_delay=50n fall_delay=50n)
.model kicad_555_rst adc_bridge(in_low=0.7 in_high=0.7 rise_delay=50n fall_delay=50n)
.model kicad_555_hi d_pullup
.model kicad_555_and d_and(rise_delay=10n fall_delay=10n)
.model kicad_555_buf d_buffer(rise_delay=10n fall_delay=10n)
.model kicad_555_latch d_srlatch(sr_delay=20n enable_delay=20n set_delay=20n reset_delay=20n ic=0)
.model kicad_555_out dac_bridge(out_low=0 out_high=1 out_undef=0.5 t_rise=20n t_fall=20n)
.model kicad_555_on sw(vt=0.5 vh=0.1 ron=10 roff=1G)
.model kicad_555_off sw(vt=-0.5 vh=0.1 ron=10 roff=1G)";

/// A microcontroller's pins as a behavioral model. `Sim.Params` is a script of
/// assignments, `PB0=square(1k) PB1=!PB3 PB2=PB3&PB4`: the named pins become push-pull
/// outputs of the expression, every other pin an input read at half the supply.
/// Expressions: `0`, `1`, a pin, `square(freq[,duty])`, `pwm(freq,duty)`, `!x`, `x&y`,
/// `x^y`, `x|y` and parentheses. The model runs no firmware: it is what the pins do.
pub fn mcu_model(
    name: &str,
    lib: &LibSymbol,
    script: &str,
) -> Result<(String, Vec<String>), String> {
    let pins: Vec<(String, String)> = lib
        .pins
        .iter()
        .map(|p| (p.number.clone(), p.name.clone()))
        .collect();
    let find = |pin: &str| -> Option<String> {
        pins.iter()
            .find(|(_, n)| n.eq_ignore_ascii_case(pin))
            .map(|(_, n)| n.to_ascii_lowercase())
    };
    let (vcc, gnd) = (
        find("VCC").ok_or("the part has no VCC pin")?,
        find("GND").ok_or("the part has no GND pin")?,
    );
    let mut outputs: Vec<(String, String)> = vec![];
    for stmt in script
        .split(|c: char| c.is_whitespace() || c == ';')
        .filter(|s| !s.is_empty())
    {
        let (lhs, rhs) = stmt
            .split_once('=')
            .ok_or_else(|| format!("'{stmt}' is not PIN=expression"))?;
        let pin = find(lhs.trim()).ok_or_else(|| format!("{} has no pin {lhs}", lib.lib_id))?;
        if pin == vcc || pin == gnd {
            return Err(format!("{lhs} is a supply pin"));
        }
        if outputs.iter().any(|(p, _)| *p == pin) {
            return Err(format!("{lhs} is assigned twice"));
        }
        outputs.push((pin, rhs.trim().to_owned()));
    }
    let mut body = vec![];
    let mut models = vec![];
    let mut inputs: BTreeSet<String> = BTreeSet::new();
    let mut counter = 0usize;
    let mut compile = |expr: &str,
                       body: &mut Vec<String>,
                       models: &mut Vec<String>,
                       inputs: &mut BTreeSet<String>|
     -> Result<String, String> {
        let toks = expr_tokens(expr)?;
        let mut p = ExprParser {
            toks: &toks,
            at: 0,
            body,
            models,
            inputs,
            counter: &mut counter,
            outputs: &outputs,
            find: &find,
            name,
        };
        let net = p.or()?;
        if p.at != toks.len() {
            return Err(format!("unexpected '{}' in {expr}", toks[p.at]));
        }
        Ok(net)
    };
    for (pin, expr) in outputs.clone() {
        let net = compile(&expr, &mut body, &mut models, &mut inputs)?;
        body.push(format!("Ab_{pin} {net} d_{pin} kicad_mcu_buf"));
        body.push(format!("Ao_{pin} [d_{pin}] [c_{pin}] kicad_logic_out"));
        body.push(format!("Shi_{pin} {vcc} {pin} c_{pin} 0 kicad_mcu_hi"));
        body.push(format!("Slo_{pin} {pin} {gnd} 0 c_{pin} kicad_mcu_lo"));
    }
    for pin in &inputs {
        if outputs.iter().any(|(p, _)| p == pin) {
            continue;
        }
        body.push(format!("Ei_{pin} s_{pin} 0 {pin} mid 1"));
        body.push(format!("Ai_{pin} [s_{pin}] [i_{pin}] kicad_logic_in"));
    }
    let ports: Vec<String> = pins.iter().map(|(_, n)| n.to_ascii_lowercase()).collect();
    let mut text = vec![format!(".subckt {name} {}", ports.join(" "))];
    text.push(format!("Rm1 {vcc} mid 1Meg"));
    text.push(format!("Rm2 mid {gnd} 1Meg"));
    text.extend(body);
    text.push(".ends".into());
    text.push(".model kicad_mcu_buf d_buffer(rise_delay=10n fall_delay=10n)".into());
    text.push(".model kicad_mcu_hi sw(vt=0.5 vh=0.1 ron=25 roff=1G)".into());
    text.push(".model kicad_mcu_lo sw(vt=-0.5 vh=0.1 ron=25 roff=1G)".into());
    text.extend(models);
    Ok((
        text.join("\n"),
        pins.into_iter().map(|(num, _)| num).collect(),
    ))
}

fn expr_tokens(expr: &str) -> Result<Vec<String>, String> {
    let mut out = vec![];
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if "!&|^(),".contains(c) {
            out.push(c.to_string());
            i += 1;
        } else if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '%' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '.' | '_' | '%'))
            {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
        } else if c.is_whitespace() {
            i += 1;
        } else {
            return Err(format!("unexpected '{c}' in {expr}"));
        }
    }
    Ok(out)
}

struct ExprParser<'a> {
    toks: &'a [String],
    at: usize,
    body: &'a mut Vec<String>,
    models: &'a mut Vec<String>,
    inputs: &'a mut BTreeSet<String>,
    counter: &'a mut usize,
    outputs: &'a [(String, String)],
    find: &'a dyn Fn(&str) -> Option<String>,
    name: &'a str,
}
impl ExprParser<'_> {
    fn peek(&self) -> Option<&str> {
        self.toks.get(self.at).map(String::as_str)
    }
    fn fresh(&mut self) -> String {
        *self.counter += 1;
        format!("n{}", self.counter)
    }
    fn binary(
        &mut self,
        op: &str,
        code: &str,
        next: fn(&mut Self) -> Result<String, String>,
    ) -> Result<String, String> {
        let mut left = next(self)?;
        while self.peek() == Some(op) {
            self.at += 1;
            let right = next(self)?;
            let out = self.fresh();
            self.body
                .push(format!("A{out} [{left} {right}] {out} kicad_mcu_{code}"));
            let model = format!(".model kicad_mcu_{code} {code}(rise_delay=10n fall_delay=10n)");
            if !self.models.contains(&model) {
                self.models.push(model);
            }
            left = out;
        }
        Ok(left)
    }
    fn or(&mut self) -> Result<String, String> {
        self.binary("|", "d_or", Self::xor)
    }
    fn xor(&mut self) -> Result<String, String> {
        self.binary("^", "d_xor", Self::and)
    }
    fn and(&mut self) -> Result<String, String> {
        self.binary("&", "d_and", Self::unary)
    }
    fn unary(&mut self) -> Result<String, String> {
        if self.peek() == Some("!") {
            self.at += 1;
            let inner = self.unary()?;
            // Inverting twice reads the net itself.
            return Ok(match inner.strip_prefix('~') {
                Some(plain) => plain.to_owned(),
                None => format!("~{inner}"),
            });
        }
        self.atom()
    }
    fn atom(&mut self) -> Result<String, String> {
        let tok = self
            .peek()
            .ok_or("an expression ends too early")?
            .to_owned();
        self.at += 1;
        match tok.as_str() {
            "(" => {
                let e = self.or()?;
                if self.peek() != Some(")") {
                    return Err("missing )".into());
                }
                self.at += 1;
                Ok(e)
            }
            "0" | "1" => {
                let out = self.fresh();
                let kind = if tok == "1" { "d_pullup" } else { "d_pulldown" };
                self.body.push(format!("A{out} {out} kicad_mcu_{kind}"));
                let model = format!(".model kicad_mcu_{kind} {kind}");
                if !self.models.contains(&model) {
                    self.models.push(model);
                }
                Ok(out)
            }
            "square" | "pwm" => {
                if self.peek() != Some("(") {
                    return Err(format!("{tok} needs (frequency, duty)"));
                }
                self.at += 1;
                let mut args = vec![];
                while let Some(t) = self.peek() {
                    if t == ")" {
                        break;
                    }
                    if t != "," {
                        args.push(t.to_owned());
                    }
                    self.at += 1;
                }
                if self.peek() != Some(")") {
                    return Err(format!("{tok}( is not closed"));
                }
                self.at += 1;
                let freq = args
                    .first()
                    .and_then(|a| parse_value(a))
                    .filter(|f| *f > 0.0)
                    .ok_or(format!("{tok} needs a positive frequency"))?;
                let duty = match args.get(1) {
                    Some(d) if d.ends_with('%') => {
                        parse_value(d.trim_end_matches('%')).map(|v| v / 100.0)
                    }
                    Some(d) => parse_value(d),
                    None if tok == "square" => Some(0.5),
                    None => None,
                }
                .filter(|d| *d > 0.0 && *d < 1.0)
                .ok_or(format!("{tok} needs a duty cycle between 0 and 1"))?;
                let out = self.fresh();
                let model = format!("kicad_mcu_osc_{}_{out}", self.name.to_ascii_lowercase());
                self.body.push(format!("A{out} 0 {out} {model}"));
                self.models.push(format!(
                    ".model {model} d_osc(cntl_array=[-1 1] freq_array=[{f} {f}] duty_cycle={duty} init_phase=0 rise_delay=10n fall_delay=10n)",
                    f = spice_number(freq)
                ));
                Ok(out)
            }
            name => {
                let pin = (self.find)(name).ok_or_else(|| format!("no pin named {name}"))?;
                if self.outputs.iter().any(|(p, _)| *p == pin) {
                    Ok(format!("d_{pin}"))
                } else {
                    self.inputs.insert(pin.clone());
                    Ok(format!("i_{pin}"))
                }
            }
        }
    }
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
    let mut subckts: BTreeMap<String, String> = BTreeMap::new();
    let mut body = vec![];
    let mut problems = vec![];
    for net in &conn.nets {
        deck.nodes.insert(net.name.clone(), spice_node(&net.name));
    }
    for (gid, _, sym) in components(sch) {
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
        // A pin's net, whichever unit of the part carries it.
        let node = |number: &str| -> String {
            conn.net_of_pin(gid, number)
                .or_else(|| conn.net_of_ref_pin(&reference, number))
                .map(|n| spice_node(&n.name))
                .unwrap_or_else(|| format!("NC_{reference}_{number}"))
        };
        let by_name = |name: &str| -> Option<String> {
            lib.pins
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name))
                .map(|p| node(&p.number))
        };
        let named = |letter: char| -> String {
            if reference.to_ascii_uppercase().starts_with(letter) {
                reference.clone()
            } else {
                format!("{letter}{reference}")
            }
        };
        let params = sym.field("Sim.Params").to_owned();
        // A model defined by a `.model` directive on the sheet, named in Sim.Name.
        let model_name = sym.field("Sim.Name").trim().to_owned();
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
        let model = |kind: &str, extra: &str, models: &mut Vec<String>| -> String {
            if !model_name.is_empty() {
                return model_name.clone();
            }
            let m = format!("__{reference}");
            models.push(format!(".model {m} {kind}({extra}{params})"));
            m
        };
        match lib.spice {
            Spice::Resistor => passive('R', &mut body, &mut problems),
            Spice::Capacitor => passive('C', &mut body, &mut problems),
            Spice::Inductor => passive('L', &mut body, &mut problems),
            Spice::Diode => {
                let m = model("D", "", &mut models);
                body.push(format!("{} {} {} {m}", named('D'), node("2"), node("1")));
            }
            Spice::Npn | Spice::Pnp => {
                let kind = if lib.spice == Spice::Npn { "NPN" } else { "PNP" };
                let m = model(kind, "", &mut models);
                body.push(format!(
                    "{} {} {} {} {m}",
                    named('Q'),
                    node("2"),
                    node("1"),
                    node("3")
                ));
            }
            Spice::Nmos | Spice::Pmos => {
                let kind = if lib.spice == Spice::Nmos { "NMOS" } else { "PMOS" };
                let m = model(kind, "level=1 ", &mut models);
                let s_ = node("3");
                body.push(format!(
                    "{} {} {} {s_} {s_} {m}",
                    named('M'),
                    node("2"),
                    node("1")
                ));
            }
            Spice::OpAmp => {
                let gain = params
                    .split_whitespace()
                    .find_map(|p| p.strip_prefix("gain="))
                    .and_then(spice_value)
                    .unwrap_or_else(|| "100k".into());
                let sub = format!("kicad_builtin_opamp_{}", gain.replace('.', "p"));
                subckts.entry(sub.clone()).or_insert_with(|| {
                    format!(".subckt {sub} in+ in- vcc vee out\nE1 out 0 in+ in- {gain}\n.ends")
                });
                body.push(format!(
                    "X{reference} {} {} {} {} {} {sub}",
                    node("1"),
                    node("2"),
                    node("3"),
                    node("4"),
                    node("5")
                ));
            }
            Spice::VoltageSource | Spice::CurrentSource => {
                let letter = if lib.spice == Spice::VoltageSource {
                    'V'
                } else {
                    'I'
                };
                let spec = match spice_value(&value) {
                    Some(v) => format!("dc {v}"),
                    None => value.clone(),
                };
                body.push(format!("{} {} {} {spec}", named(letter), node("1"), node("2")));
            }
            Spice::Gate(g) => {
                let code = g.code_model();
                let two = g != crate::symbols::LogicGate::Not;
                let pins: Option<Vec<String>> = if two {
                    ["A", "B", "Y", "VCC", "GND"].iter().map(|p| by_name(p)).collect()
                } else {
                    ["A", "Y", "VCC", "GND"].iter().map(|p| by_name(p)).collect()
                };
                let Some(pins) = pins else {
                    problems.push(format!(
                        "{reference}: a logic gate model needs pins named A{}, Y, VCC and GND",
                        if two { ", B" } else { "" }
                    ));
                    continue;
                };
                subckts
                    .entry(format!("kicad_logic_{code}"))
                    .or_insert_with(|| gate_subckt(code, two));
                subckts
                    .entry("kicad_logic".into())
                    .or_insert_with(|| LOGIC_MODELS.into());
                body.push(format!("X{reference} {} kicad_logic_{code}", pins.join(" ")));
            }
            Spice::DFlipFlop => {
                let Some(pins) = ["D", "CLK", "Q", "VCC", "GND"]
                    .iter()
                    .map(|p| by_name(p))
                    .collect::<Option<Vec<String>>>()
                else {
                    problems.push(format!(
                        "{reference}: a D flip-flop model needs pins named D, CLK, Q, VCC and GND"
                    ));
                    continue;
                };
                subckts
                    .entry("kicad_logic_dff".into())
                    .or_insert_with(|| DFF_SUBCKT.into());
                subckts
                    .entry("kicad_logic".into())
                    .or_insert_with(|| LOGIC_MODELS.into());
                body.push(format!("X{reference} {} kicad_logic_dff", pins.join(" ")));
            }
            Spice::Timer555 => {
                let Some(pins) = ["GND", "TR", "Q", "R", "CV", "THR", "DIS", "VCC"]
                    .iter()
                    .map(|p| by_name(p))
                    .collect::<Option<Vec<String>>>()
                else {
                    problems.push(format!(
                        "{reference}: the 555 model needs pins GND, TR, Q, R, CV, THR, DIS and VCC"
                    ));
                    continue;
                };
                subckts
                    .entry("kicad_ne555".into())
                    .or_insert_with(|| NE555_SUBCKT.into());
                body.push(format!("X{reference} {} kicad_ne555", pins.join(" ")));
            }
            Spice::Mcu => {
                let sub = format!("kicad_mcu_{}", reference.to_ascii_lowercase());
                match mcu_model(&sub, lib, &params) {
                    Ok((text, numbers)) => {
                        subckts.insert(sub.clone(), text);
                        subckts
                            .entry("kicad_logic".into())
                            .or_insert_with(|| LOGIC_MODELS.into());
                        let nodes: Vec<String> = numbers.iter().map(|n| node(n)).collect();
                        body.push(format!("X{reference} {} {sub}", nodes.join(" ")));
                    }
                    Err(e) => problems.push(format!("{reference} pin script: {e}")),
                }
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
    lines.extend(subckts.into_values());
    lines.extend(models);
    lines.extend(body);
    // Directives placed on any sheet as text travel with the deck.
    for (_, _, _, sheet) in sch.sheets_flat() {
        for t in &sheet.texts {
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
    for (_, _, sym) in components(sch) {
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
    #[test]
    fn mcu_scripts_compile_to_digital_models() {
        let lib = crate::symbols::find("MCU_Microchip_ATtiny:ATtiny85-20P").unwrap();
        let (text, pins) = mcu_model("m1", lib, "PB0=square(2k) PB1=!PB3&PB4").unwrap();
        assert_eq!(pins.len(), 8);
        assert!(
            text.contains("d_osc(cntl_array=[-1 1] freq_array=[2k 2k]"),
            "{text}"
        );
        assert!(text.contains("[~i_pb3 i_pb4]"), "{text}");
        crate::spice::parse(&format!(
            "t\n{}\n{text}\nX1 a b c 0 e f g vcc m1\nV1 vcc 0 5\n.tran 1u 1m\n.end",
            LOGIC_MODELS
        ))
        .unwrap();
        for bad in [
            "PB9=1",
            "PB0=square(0)",
            "PB0=(1",
            "VCC=1",
            "PB0=1 PB0=0",
            "PB0=pwm(1k)",
        ] {
            assert!(mcu_model("m", lib, bad).is_err(), "{bad}");
        }
    }
}
