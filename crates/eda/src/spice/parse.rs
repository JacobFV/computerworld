//! SPICE deck parser. Reads the subset of the Berkeley SPICE3 / ngspice input language
//! that a schematic exports: passive and controlled elements, independent sources with
//! DC/AC/PULSE/SIN/PWL specifications, diodes, Gummel–Poon BJTs, level-1 MOSFETs,
//! voltage-controlled switches, XSPICE `A` devices (digital gates, flip-flops, latches,
//! oscillators and the ADC/DAC bridges), `.model`, `.subckt` (flattened on the way in),
//! `.options` tolerances and the analysis commands.
use super::digital::{self, DacLevels, GateOp, Logic, Port};
use super::{
    Analysis, BjtModel, Circuit, Device, DiodeModel, MosModel, Options, Source, SwitchModel, Wave,
};
use crate::num::parse_value;
use std::collections::BTreeMap;

/// One logical line with its tokens and the physical line it started on.
#[derive(Clone, Debug)]
struct Card {
    line: usize,
    tokens: Vec<String>,
}

fn tokenize(text: &str) -> Vec<String> {
    let mut spaced = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '(' | ')' | ',' => spaced.push(' '),
            '=' => spaced.push_str(" = "),
            '[' => spaced.push_str(" [ "),
            ']' => spaced.push_str(" ] "),
            c => spaced.push(c),
        }
    }
    spaced.split_whitespace().map(str::to_owned).collect()
}

/// Physical lines to cards: the title, comments and continuations resolved.
fn cards(text: &str) -> (String, Vec<Card>) {
    let mut lines = text.lines().enumerate();
    // The first line is the title; KiCad writes it as a `.title` card.
    let title = lines
        .next()
        .map(|(_, l)| {
            let l = l.trim();
            match l.get(..7) {
                Some(head) if head.eq_ignore_ascii_case(".title ") => l[7..].trim().to_owned(),
                _ => l.to_owned(),
            }
        })
        .unwrap_or_default();
    let mut out: Vec<Card> = Vec::new();
    let mut in_control = false;
    for (number, raw) in lines {
        let mut line = raw.trim();
        for marker in [";", "$ "] {
            if let Some(at) = line.find(marker) {
                line = line[..at].trim_end();
            }
        }
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with(".control") {
            in_control = true;
            continue;
        }
        if lower.starts_with(".endc") {
            in_control = false;
            continue;
        }
        if in_control {
            continue;
        }
        if let Some(rest) = line.strip_prefix('+') {
            if let Some(last) = out.last_mut() {
                last.tokens.extend(tokenize(rest));
                continue;
            }
        }
        out.push(Card {
            line: number + 1,
            tokens: tokenize(line),
        });
    }
    (title, out)
}

fn value(token: &str, what: &str, line: usize) -> Result<f64, String> {
    parse_value(token).ok_or_else(|| format!("line {line}: {what} \"{token}\" is not a number"))
}

/// `key = value` pairs after position `from`.
fn params(tokens: &[String], line: usize) -> Result<BTreeMap<String, f64>, String> {
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens.get(i + 1).map(String::as_str) == Some("=") {
            let key = tokens[i].to_ascii_lowercase();
            let raw = tokens
                .get(i + 2)
                .ok_or_else(|| format!("line {line}: {key}= has no value"))?;
            out.insert(key.clone(), value(raw, &key, line)?);
            i += 3;
        } else {
            i += 1;
        }
    }
    Ok(out)
}

/// XSPICE model parameters: `key = value` or `key = [v1 v2 …]`.
fn array_params(tokens: &[String], line: usize) -> Result<BTreeMap<String, Vec<f64>>, String> {
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens.get(i + 1).map(String::as_str) != Some("=") {
            i += 1;
            continue;
        }
        let key = tokens[i].to_ascii_lowercase();
        if tokens.get(i + 2).map(String::as_str) == Some("[") {
            let mut j = i + 3;
            let mut vals = vec![];
            while j < tokens.len() && tokens[j] != "]" {
                vals.push(value(&tokens[j], &key, line)?);
                j += 1;
            }
            if j >= tokens.len() {
                return Err(format!("line {line}: {key}= array has no closing ]"));
            }
            out.insert(key, vals);
            i = j + 1;
        } else {
            let raw = tokens
                .get(i + 2)
                .ok_or_else(|| format!("line {line}: {key}= has no value"))?;
            out.insert(key.clone(), vec![value(raw, &key, line)?]);
            i += 3;
        }
    }
    Ok(out)
}

/// An XSPICE code model: its type (`d_and`, `adc_bridge`, …) and parameters.
#[derive(Clone, Debug)]
struct CodeModel {
    kind: String,
    params: BTreeMap<String, Vec<f64>>,
}
impl CodeModel {
    fn get(&self, key: &str, default: f64) -> f64 {
        self.params
            .get(key)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(default)
    }
}

/// The XSPICE code models this simulator implements.
pub const CODE_MODELS: [&str; 15] = [
    "d_and",
    "d_nand",
    "d_or",
    "d_nor",
    "d_xor",
    "d_xnor",
    "d_buffer",
    "d_inverter",
    "d_dff",
    "d_srlatch",
    "d_osc",
    "d_pullup",
    "d_pulldown",
    "adc_bridge",
    "dac_bridge",
];

#[derive(Clone, Debug)]
struct Subckt {
    ports: Vec<String>,
    cards: Vec<Card>,
}

#[derive(Clone, Debug)]
enum Model {
    Diode(DiodeModel),
    Bjt(BjtModel),
    Mos(MosModel),
    Switch(SwitchModel),
    Code(CodeModel),
}

fn parse_model(card: &Card) -> Result<(String, Model), String> {
    let t = &card.tokens;
    if t.len() < 3 {
        return Err(format!(
            "line {}: .model needs a name and a type",
            card.line
        ));
    }
    let name = t[1].to_ascii_lowercase();
    let kind = t[2].to_ascii_lowercase();
    if CODE_MODELS.contains(&kind.as_str()) {
        return Ok((
            name,
            Model::Code(CodeModel {
                kind,
                params: array_params(&t[3..], card.line)?,
            }),
        ));
    }
    let p = params(&t[3..], card.line)?;
    let get = |k: &str, d: f64| p.get(k).copied().unwrap_or(d);
    let opt = |k: &str| p.get(k).copied();
    let model = match kind.as_str() {
        "d" => Model::Diode(DiodeModel {
            is: get("is", 1e-14),
            n: get("n", 1.0),
            rs: get("rs", 0.0),
            bv: opt("bv"),
            ibv: get("ibv", 1e-3),
            cjo: get("cjo", get("cj0", get("cj", 0.0))),
            vj: get("vj", 1.0),
            m: get("m", 0.5),
            tt: get("tt", 0.0),
            fc: get("fc", 0.5),
        }),
        "npn" | "pnp" => {
            let d = BjtModel::default();
            let is = get("is", d.is);
            Model::Bjt(BjtModel {
                pnp: kind == "pnp",
                is,
                bf: get("bf", d.bf),
                nf: get("nf", d.nf),
                vaf: opt("vaf").or(opt("va")),
                ikf: opt("ikf").or(opt("ik")),
                // SPICE2's C2 and C4 give the leakage saturation currents as multiples of IS.
                ise: get("ise", get("c2", 0.0) * is),
                ne: get("ne", d.ne),
                br: get("br", d.br),
                nr: get("nr", d.nr),
                var: opt("var").or(opt("vb")),
                ikr: opt("ikr"),
                isc: get("isc", get("c4", 0.0) * is),
                nc: get("nc", d.nc),
                rb: get("rb", 0.0),
                re: get("re", 0.0),
                rc: get("rc", 0.0),
                cje: get("cje", 0.0),
                vje: get("vje", get("pe", d.vje)),
                mje: get("mje", get("me", d.mje)),
                cjc: get("cjc", 0.0),
                vjc: get("vjc", get("pc", d.vjc)),
                mjc: get("mjc", get("mc", d.mjc)),
                tf: get("tf", 0.0),
                tr: get("tr", 0.0),
                fc: get("fc", d.fc),
            })
        }
        "nmos" | "pmos" => {
            let level = get("level", 1.0);
            if level != 1.0 {
                return Err(format!(
                    "line {}: MOSFET model {name} is level {level}; only level 1 is supported",
                    card.line
                ));
            }
            let d = MosModel::default();
            let tox = opt("tox").filter(|t| *t > 0.0);
            // Without KP, SPICE derives it from the surface mobility and the oxide.
            let kp = match (opt("kp"), tox) {
                (Some(kp), _) => kp,
                (None, Some(t)) => {
                    get("uo", get("u0", 600.0)) * 1e-4 * 3.9 * 8.854_214_871e-12 / t
                }
                (None, None) => d.kp,
            };
            Model::Mos(MosModel {
                pmos: kind == "pmos",
                vto: get("vto", get("vt0", 0.0)),
                kp,
                gamma: get("gamma", 0.0),
                phi: get("phi", d.phi),
                lambda: get("lambda", 0.0),
                rd: get("rd", 0.0),
                rs: get("rs", 0.0),
                cbd: get("cbd", 0.0),
                cbs: get("cbs", 0.0),
                is: get("is", d.is),
                pb: get("pb", d.pb),
                mj: get("mj", d.mj),
                fc: get("fc", d.fc),
                cgso: get("cgso", 0.0),
                cgdo: get("cgdo", 0.0),
                cgbo: get("cgbo", 0.0),
                tox,
                ld: get("ld", 0.0),
            })
        }
        "sw" => {
            let ron = get("ron", 1.0);
            let roff = get("roff", 1e12);
            if ron <= 0.0 || roff <= 0.0 {
                return Err(format!(
                    "line {}: switch {name} needs positive RON and ROFF",
                    card.line
                ));
            }
            Model::Switch(SwitchModel {
                vt: get("vt", 0.0),
                vh: get("vh", 0.0).abs(),
                ron,
                roff,
            })
        }
        other => {
            return Err(format!(
                "line {}: model type {other} is not supported (d, npn, pnp, nmos, pmos, sw, or an XSPICE digital model)",
                card.line
            ))
        }
    };
    Ok((name, model))
}

/// Source specification after the two nodes: `[DC] v [AC mag [phase]] [PULSE(..)|SIN(..)|PWL(..)]`.
fn parse_source(tokens: &[String], line: usize) -> Result<Source, String> {
    let mut src = Source::default();
    let mut dc_given = false;
    let mut i = 0;
    let lower: Vec<String> = tokens.iter().map(|t| t.to_ascii_lowercase()).collect();
    let numbers_from = |start: usize| -> Vec<f64> {
        let mut v = Vec::new();
        for tok in &tokens[start..] {
            match parse_value(tok) {
                Some(x) => v.push(x),
                None => break,
            }
        }
        v
    };
    while i < tokens.len() {
        match lower[i].as_str() {
            "dc" => {
                let v = tokens
                    .get(i + 1)
                    .ok_or_else(|| format!("line {line}: DC needs a value"))?;
                src.dc = value(v, "DC value", line)?;
                dc_given = true;
                i += 2;
            }
            "ac" => {
                let args = numbers_from(i + 1);
                src.ac_mag = args.first().copied().unwrap_or(1.0);
                src.ac_phase = args.get(1).copied().unwrap_or(0.0);
                i += 1 + args.len().min(2);
            }
            "pulse" => {
                let a = numbers_from(i + 1);
                if a.len() < 2 {
                    return Err(format!("line {line}: PULSE needs at least V1 and V2"));
                }
                let g = |k: usize| a.get(k).copied();
                src.wave = Some(Wave::Pulse {
                    v1: a[0],
                    v2: a[1],
                    td: g(2).unwrap_or(0.0),
                    tr: g(3).unwrap_or(0.0),
                    tf: g(4).unwrap_or(0.0),
                    pw: g(5),
                    per: g(6),
                });
                i += 1 + a.len();
            }
            "sin" => {
                let a = numbers_from(i + 1);
                if a.len() < 3 {
                    return Err(format!("line {line}: SIN needs VO, VA and FREQ"));
                }
                let g = |k: usize, d: f64| a.get(k).copied().unwrap_or(d);
                src.wave = Some(Wave::Sin {
                    vo: a[0],
                    va: a[1],
                    freq: a[2],
                    td: g(3, 0.0),
                    theta: g(4, 0.0),
                    phase: g(5, 0.0),
                });
                i += 1 + a.len();
            }
            "pwl" => {
                let a = numbers_from(i + 1);
                if a.len() < 2 || a.len() % 2 != 0 {
                    return Err(format!("line {line}: PWL needs time/value pairs"));
                }
                let points: Vec<(f64, f64)> = a.chunks(2).map(|c| (c[0], c[1])).collect();
                if points.windows(2).any(|w| w[1].0 < w[0].0) {
                    return Err(format!("line {line}: PWL times must not decrease"));
                }
                src.wave = Some(Wave::Pwl(points));
                i += 1 + a.len();
            }
            _ => {
                if let Some(v) = parse_value(&tokens[i]) {
                    src.dc = v;
                    dc_given = true;
                    i += 1;
                } else {
                    return Err(format!(
                        "line {line}: unexpected \"{}\" in source specification",
                        tokens[i]
                    ));
                }
            }
        }
    }
    // A time-varying source's DC value is its value at t = 0 unless DC was given.
    if !dc_given {
        if let Some(wave) = &src.wave {
            src.dc = wave.at(0.0);
        }
    }
    Ok(src)
}

struct Builder {
    circuit: Circuit,
    node_index: BTreeMap<String, usize>,
    models: BTreeMap<String, Model>,
    subckts: BTreeMap<String, Subckt>,
}

impl Builder {
    fn node(&mut self, name: &str) -> usize {
        let key = name.to_ascii_lowercase();
        if key == "0" || key == "gnd" {
            return 0;
        }
        if let Some(i) = self.node_index.get(&key) {
            return *i;
        }
        let i = self.circuit.nodes.len();
        self.circuit.nodes.push(name.to_owned());
        self.node_index.insert(key, i);
        i
    }
    /// An XSPICE instance: `A<name> <connections…> <model>`, where a connection is a
    /// node, `~node` (inverted), `NULL`, or a `[ … ]` vector of them.
    fn code_device(
        &mut self,
        name: &str,
        t: &[String],
        line: usize,
        resolve: &dyn Fn(&str) -> String,
    ) -> Result<(), String> {
        if t.len() < 3 {
            return Err(format!(
                "line {line}: {} needs connections and a model",
                t[0]
            ));
        }
        let model_name = &t[t.len() - 1];
        let model = match self.models.get(&model_name.to_ascii_lowercase()) {
            Some(Model::Code(m)) => m.clone(),
            Some(_) => {
                return Err(format!(
                    "line {line}: {model_name} is not an XSPICE code model"
                ))
            }
            None => return Err(format!("line {line}: unknown model {model_name}")),
        };
        // Connection groups.
        let body = &t[..t.len() - 1];
        let mut groups: Vec<Vec<String>> = vec![];
        let mut i = 1;
        while i < body.len() {
            if body[i] == "[" {
                let mut g = vec![];
                i += 1;
                while i < body.len() && body[i] != "]" {
                    g.push(body[i].clone());
                    i += 1;
                }
                if i >= body.len() {
                    return Err(format!("line {line}: {} has an unclosed [", t[0]));
                }
                groups.push(g);
            } else {
                groups.push(vec![body[i].clone()]);
            }
            i += 1;
        }
        let kind = model.kind.as_str();
        let expect = |n: usize| -> Result<(), String> {
            if groups.len() != n {
                Err(format!(
                    "line {line}: {} ({kind}) needs {n} connections, has {}",
                    t[0],
                    groups.len()
                ))
            } else {
                Ok(())
            }
        };
        let single = |g: &Vec<String>| -> Result<String, String> {
            match g.as_slice() {
                [one] => Ok(one.clone()),
                _ => Err(format!("line {line}: {} expects a single node here", t[0])),
            }
        };
        let rise = model.get("rise_delay", 1e-9);
        let fall = model.get("fall_delay", 1e-9);
        if rise < 0.0 || fall < 0.0 {
            return Err(format!("line {line}: {} has a negative delay", t[0]));
        }
        let element =
            |kind: digital::Kind, inputs: Vec<Port>, outputs: Vec<Port>| digital::Element {
                name: name.to_owned(),
                kind,
                inputs,
                outputs,
                rise,
                fall,
            };
        match kind {
            "adc_bridge" | "dac_bridge" => {
                expect(2)?;
                if groups[0].len() != groups[1].len() {
                    return Err(format!(
                        "line {line}: {} has {} inputs but {} outputs",
                        t[0],
                        groups[0].len(),
                        groups[1].len()
                    ));
                }
                for (k, (a, b)) in groups[0].iter().zip(&groups[1]).enumerate() {
                    let label = if groups[0].len() == 1 {
                        name.to_owned()
                    } else {
                        format!("{name}#{k}")
                    };
                    if kind == "adc_bridge" {
                        let node = self.node(&resolve(a));
                        let net = self.circuit.digital.net(&resolve(b));
                        self.circuit.digital.adcs.push(digital::Adc {
                            name: label,
                            node,
                            net,
                            in_low: model.get("in_low", 1.0),
                            in_high: model.get("in_high", 2.0),
                            rise,
                            fall,
                        });
                    } else {
                        let net = self.circuit.digital.net(&resolve(a));
                        let p = self.node(&resolve(b));
                        let (lo, hi) = (model.get("out_low", 0.0), model.get("out_high", 1.0));
                        let levels = DacLevels {
                            out_low: lo,
                            out_high: hi,
                            out_undef: model.get("out_undef", (lo + hi) / 2.0),
                            t_rise: model.get("t_rise", 1e-9),
                            t_fall: model.get("t_fall", 1e-9),
                        };
                        self.circuit.devices.push(Device::Dac {
                            name: label,
                            p,
                            net,
                            levels,
                        });
                    }
                }
            }
            "d_buffer" | "d_inverter" | "d_and" | "d_nand" | "d_or" | "d_nor" | "d_xor"
            | "d_xnor" => {
                expect(2)?;
                let unary = matches!(kind, "d_buffer" | "d_inverter");
                if unary && groups[0].len() != 1 {
                    return Err(format!("line {line}: {} takes one input", t[0]));
                }
                if !unary && groups[0].len() < 2 {
                    return Err(format!("line {line}: {} needs at least two inputs", t[0]));
                }
                let y = single(&groups[1])?;
                let ins: Vec<Port> = groups[0].iter().map(|g| self.port(g, resolve)).collect();
                let out = self.port(&y, resolve);
                let e = element(
                    digital::Kind::Gate(GateOp::parse(kind).expect("a gate model")),
                    ins,
                    vec![out],
                );
                self.circuit.digital.elements.push(e);
            }
            "d_dff" | "d_srlatch" => {
                let n_in = if kind == "d_dff" { 4 } else { 5 };
                expect(n_in + 2)?;
                let mut ports = vec![];
                for g in &groups {
                    let s = single(g)?;
                    ports.push(self.port(&s, resolve));
                }
                let ic = if model.get("ic", 0.0) >= 0.5 {
                    Logic::One
                } else {
                    Logic::Zero
                };
                let k = if kind == "d_dff" {
                    digital::Kind::Dff {
                        clk_delay: model.get("clk_delay", 1e-9),
                        set_delay: model.get("set_delay", 1e-9),
                        reset_delay: model.get("reset_delay", 1e-9),
                        ic,
                    }
                } else {
                    digital::Kind::SrLatch {
                        sr_delay: model.get("sr_delay", 1e-9),
                        enable_delay: model.get("enable_delay", 1e-9),
                        set_delay: model.get("set_delay", 1e-9),
                        reset_delay: model.get("reset_delay", 1e-9),
                        ic,
                    }
                };
                let outs = ports.split_off(n_in);
                let e = element(k, ports, outs);
                self.circuit.digital.elements.push(e);
            }
            "d_osc" => {
                expect(2)?;
                let control = self.node(&resolve(&single(&groups[0])?));
                let out = single(&groups[1])?;
                let cntl = model.params.get("cntl_array").cloned().unwrap_or_default();
                let freq = model.params.get("freq_array").cloned().unwrap_or_default();
                if cntl.is_empty() || cntl.len() != freq.len() {
                    return Err(format!(
                        "line {line}: {} needs cntl_array and freq_array of the same length",
                        t[0]
                    ));
                }
                if freq.iter().any(|f| *f <= 0.0) {
                    return Err(format!(
                        "line {line}: {} has a non-positive frequency",
                        t[0]
                    ));
                }
                let out = self.port(&out, resolve);
                let e = element(
                    digital::Kind::Osc {
                        control,
                        cntl,
                        freq,
                        duty: model.get("duty_cycle", 0.5),
                        phase: model.get("init_phase", 0.0) / 360.0,
                    },
                    vec![],
                    vec![out],
                );
                self.circuit.digital.elements.push(e);
            }
            "d_pullup" | "d_pulldown" => {
                expect(1)?;
                let out = single(&groups[0])?;
                let out = self.port(&out, resolve);
                let e = element(
                    digital::Kind::Const(if kind == "d_pullup" {
                        Logic::One
                    } else {
                        Logic::Zero
                    }),
                    vec![],
                    vec![out],
                );
                self.circuit.digital.elements.push(e);
            }
            other => return Err(format!("line {line}: code model {other} is not supported")),
        }
        Ok(())
    }
    /// A digital connection: `NULL`, a net, or `~net` read inverted.
    fn port(&mut self, tok: &str, resolve: &dyn Fn(&str) -> String) -> Port {
        if tok.eq_ignore_ascii_case("null") {
            return Port {
                net: None,
                invert: false,
            };
        }
        let (invert, n) = match tok.strip_prefix('~') {
            Some(r) => (true, r),
            None => (false, tok),
        };
        Port {
            net: Some(self.circuit.digital.net(&resolve(n))),
            invert,
        }
    }

    fn expand(
        &mut self,
        cards: &[Card],
        prefix: &str,
        map: &BTreeMap<String, String>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 16 {
            return Err("subcircuits nest more than 16 deep".into());
        }
        let resolve = |n: &str| -> String {
            let key = n.to_ascii_lowercase();
            if key == "0" || key == "gnd" {
                return "0".into();
            }
            match map.get(&key) {
                Some(outer) => outer.clone(),
                None if prefix.is_empty() => n.to_owned(),
                None => format!("{prefix}{n}"),
            }
        };
        for card in cards {
            let t = &card.tokens;
            let line = card.line;
            let first = t[0].to_ascii_lowercase();
            if first.starts_with('.') {
                continue;
            }
            let name = format!("{prefix}{}", t[0]);
            let need = |n: usize| -> Result<(), String> {
                if t.len() < n {
                    Err(format!("line {line}: {} needs {} fields", t[0], n - 1))
                } else {
                    Ok(())
                }
            };
            let letter = first.chars().next().unwrap_or(' ');
            match letter {
                'r' | 'c' | 'l' => {
                    need(4)?;
                    let (a, b) = (resolve(&t[1]), resolve(&t[2]));
                    let v = value(&t[3], "value", line)?;
                    let extra = params(&t[4..], line)?;
                    let (a, b) = (self.node(&a), self.node(&b));
                    let device = match letter {
                        'r' => {
                            if v == 0.0 {
                                return Err(format!("line {line}: {} has zero resistance", t[0]));
                            }
                            Device::R { name, a, b, r: v }
                        }
                        'c' => Device::C {
                            name,
                            a,
                            b,
                            c: v,
                            ic: extra.get("ic").copied(),
                        },
                        _ => Device::L {
                            name,
                            a,
                            b,
                            l: v,
                            ic: extra.get("ic").copied(),
                        },
                    };
                    self.circuit.devices.push(device);
                }
                'v' | 'i' => {
                    need(3)?;
                    let (p, n) = (resolve(&t[1]), resolve(&t[2]));
                    let src = parse_source(&t[3..], line)?;
                    let (p, n) = (self.node(&p), self.node(&n));
                    self.circuit.devices.push(if letter == 'v' {
                        Device::V { name, p, n, src }
                    } else {
                        Device::I { name, p, n, src }
                    });
                }
                'e' | 'g' => {
                    need(6)?;
                    let nodes: Vec<String> = t[1..5].iter().map(|n| resolve(n)).collect();
                    let gain = value(&t[5], "gain", line)?;
                    let (p, n, cp, cn) = (
                        self.node(&nodes[0]),
                        self.node(&nodes[1]),
                        self.node(&nodes[2]),
                        self.node(&nodes[3]),
                    );
                    self.circuit.devices.push(if letter == 'e' {
                        Device::E {
                            name,
                            p,
                            n,
                            cp,
                            cn,
                            gain,
                        }
                    } else {
                        Device::G {
                            name,
                            p,
                            n,
                            cp,
                            cn,
                            gm: gain,
                        }
                    });
                }
                'f' | 'h' => {
                    need(5)?;
                    let (p, n) = (resolve(&t[1]), resolve(&t[2]));
                    let control = format!("{prefix}{}", t[3]).to_ascii_lowercase();
                    let gain = value(&t[4], "gain", line)?;
                    let (p, n) = (self.node(&p), self.node(&n));
                    self.circuit.devices.push(if letter == 'f' {
                        Device::F {
                            name,
                            p,
                            n,
                            control,
                            gain,
                        }
                    } else {
                        Device::H {
                            name,
                            p,
                            n,
                            control,
                            r: gain,
                        }
                    });
                }
                'd' => {
                    need(4)?;
                    let (a, k) = (resolve(&t[1]), resolve(&t[2]));
                    let model = match self.models.get(&t[3].to_ascii_lowercase()) {
                        Some(Model::Diode(m)) => m.clone(),
                        Some(_) => {
                            return Err(format!("line {line}: {} is not a diode model", t[3]))
                        }
                        None => return Err(format!("line {line}: unknown model {}", t[3])),
                    };
                    let area = t.get(4).and_then(|a| parse_value(a)).unwrap_or(1.0);
                    let (a, k) = (self.node(&a), self.node(&k));
                    self.circuit.devices.push(Device::D {
                        name,
                        a,
                        k,
                        model,
                        area,
                    });
                }
                'q' => {
                    need(5)?;
                    let nodes: Vec<String> = t[1..4].iter().map(|n| resolve(n)).collect();
                    // An optional substrate node sits before the model name.
                    let model_at =
                        if t.len() >= 6 && !self.models.contains_key(&t[4].to_ascii_lowercase()) {
                            5
                        } else {
                            4
                        };
                    let model_token = &t[model_at];
                    // An optional area follows it, bare or as AREA=.
                    let area = match t.get(model_at + 1) {
                        Some(a) if t.get(model_at + 2).map(String::as_str) != Some("=") => {
                            value(a, "area", line)?
                        }
                        _ => params(&t[model_at + 1..], line)?
                            .get("area")
                            .copied()
                            .unwrap_or(1.0),
                    };
                    if area <= 0.0 {
                        return Err(format!("line {line}: {} has a non-positive area", t[0]));
                    }
                    let model = match self.models.get(&model_token.to_ascii_lowercase()) {
                        Some(Model::Bjt(m)) => m.clone(),
                        Some(_) => {
                            return Err(format!("line {line}: {model_token} is not a BJT model"))
                        }
                        None => return Err(format!("line {line}: unknown model {model_token}")),
                    };
                    let (c, b, e) = (
                        self.node(&nodes[0]),
                        self.node(&nodes[1]),
                        self.node(&nodes[2]),
                    );
                    self.circuit.devices.push(Device::Q {
                        name,
                        c,
                        b,
                        e,
                        model,
                        area,
                    });
                }
                's' => {
                    need(6)?;
                    let nodes: Vec<String> = t[1..5].iter().map(|n| resolve(n)).collect();
                    let model = match self.models.get(&t[5].to_ascii_lowercase()) {
                        Some(Model::Switch(m)) => m.clone(),
                        Some(_) => {
                            return Err(format!("line {line}: {} is not a switch model", t[5]))
                        }
                        None => return Err(format!("line {line}: unknown model {}", t[5])),
                    };
                    let initial = match t.get(6).map(|x| x.to_ascii_lowercase()).as_deref() {
                        Some("on") => Some(true),
                        Some("off") => Some(false),
                        _ => None,
                    };
                    let (p, n, cp, cn) = (
                        self.node(&nodes[0]),
                        self.node(&nodes[1]),
                        self.node(&nodes[2]),
                        self.node(&nodes[3]),
                    );
                    self.circuit.devices.push(Device::S {
                        name,
                        p,
                        n,
                        cp,
                        cn,
                        model,
                        initial,
                    });
                }
                'a' => self.code_device(&name, t, line, &resolve)?,
                'm' => {
                    need(6)?;
                    let nodes: Vec<String> = t[1..5].iter().map(|n| resolve(n)).collect();
                    let model = match self.models.get(&t[5].to_ascii_lowercase()) {
                        Some(Model::Mos(m)) => m.clone(),
                        Some(_) => {
                            return Err(format!("line {line}: {} is not a MOSFET model", t[5]))
                        }
                        None => return Err(format!("line {line}: unknown model {}", t[5])),
                    };
                    let p = params(&t[6..], line)?;
                    let (d, g, s, b) = (
                        self.node(&nodes[0]),
                        self.node(&nodes[1]),
                        self.node(&nodes[2]),
                        self.node(&nodes[3]),
                    );
                    let w = p.get("w").copied().unwrap_or(100e-6);
                    let l = p.get("l").copied().unwrap_or(100e-6);
                    if w <= 0.0 || l <= 0.0 {
                        return Err(format!("line {line}: W and L must be positive"));
                    }
                    self.circuit.devices.push(Device::M {
                        name,
                        d,
                        g,
                        s,
                        b,
                        model,
                        w,
                        l,
                    });
                }
                'x' => {
                    need(2)?;
                    let sub_name = t[t.len() - 1].to_ascii_lowercase();
                    let sub = self.subckts.get(&sub_name).cloned().ok_or_else(|| {
                        format!("line {line}: unknown subcircuit {}", t[t.len() - 1])
                    })?;
                    let actual = &t[1..t.len() - 1];
                    if actual.len() != sub.ports.len() {
                        return Err(format!(
                            "line {line}: {} connects {} nodes but {} has {} ports",
                            t[0],
                            actual.len(),
                            sub_name,
                            sub.ports.len()
                        ));
                    }
                    let mut inner = BTreeMap::new();
                    for (port, node) in sub.ports.iter().zip(actual) {
                        inner.insert(port.to_ascii_lowercase(), resolve(node));
                    }
                    let prefix = format!("{name}.");
                    self.expand(&sub.cards, &prefix, &inner, depth + 1)?;
                }
                _ => {
                    return Err(format!(
                        "line {line}: element {} is not supported by this simulator",
                        t[0]
                    ))
                }
            }
        }
        Ok(())
    }
}

/// Parse a deck into a flat circuit with its analysis commands.
pub fn parse(text: &str) -> Result<Circuit, String> {
    let (title, all) = cards(text);
    let mut top = Vec::new();
    let mut subckts = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut analyses = Vec::new();
    let mut options = BTreeMap::new();
    let mut open: Option<(String, Subckt)> = None;
    for card in all {
        let head = card.tokens[0].to_ascii_lowercase();
        if head == ".subckt" {
            if open.is_some() {
                return Err(format!("line {}: nested .subckt definitions", card.line));
            }
            let name = card
                .tokens
                .get(1)
                .ok_or_else(|| format!("line {}: .subckt needs a name", card.line))?
                .to_ascii_lowercase();
            let ports = card.tokens[2..]
                .iter()
                .take_while(|t| !t.eq_ignore_ascii_case("params:") && t.as_str() != "=")
                .cloned()
                .collect();
            open = Some((
                name,
                Subckt {
                    ports,
                    cards: vec![],
                },
            ));
            continue;
        }
        if head == ".ends" {
            let (name, sub) = open
                .take()
                .ok_or_else(|| format!("line {}: .ends without .subckt", card.line))?;
            subckts.insert(name, sub);
            continue;
        }
        if head == ".model" {
            let (name, model) = parse_model(&card)?;
            models.insert(name, model);
            continue;
        }
        if let Some((_, sub)) = &mut open {
            sub.cards.push(card);
            continue;
        }
        match head.as_str() {
            ".end" => break,
            ".op" => analyses.push(Analysis::Op),
            ".tran" => {
                let t = &card.tokens;
                let nums: Vec<f64> = t[1..].iter().filter_map(|x| parse_value(x)).collect();
                if nums.len() < 2 {
                    return Err(format!("line {}: .tran needs TSTEP and TSTOP", card.line));
                }
                analyses.push(Analysis::Tran {
                    tstep: nums[0],
                    tstop: nums[1],
                    tstart: nums.get(2).copied().unwrap_or(0.0),
                    tmax: nums.get(3).copied(),
                    uic: t.iter().any(|x| x.eq_ignore_ascii_case("uic")),
                });
            }
            ".ac" => {
                let t = &card.tokens;
                if t.len() < 5 {
                    return Err(format!(
                        "line {}: .ac needs DEC|OCT|LIN, points, fstart and fstop",
                        card.line
                    ));
                }
                let scale = t[1].to_ascii_lowercase();
                if !matches!(scale.as_str(), "dec" | "oct" | "lin") {
                    return Err(format!("line {}: unknown AC sweep {}", card.line, t[1]));
                }
                analyses.push(Analysis::Ac {
                    scale,
                    points: value(&t[2], "points", card.line)?.max(1.0) as u32,
                    fstart: value(&t[3], "fstart", card.line)?,
                    fstop: value(&t[4], "fstop", card.line)?,
                });
            }
            ".dc" => {
                let t = &card.tokens;
                if t.len() < 5 {
                    return Err(format!(
                        "line {}: .dc needs a source, start, stop and step",
                        card.line
                    ));
                }
                analyses.push(Analysis::Dc {
                    source: t[1].clone(),
                    start: value(&t[2], "start", card.line)?,
                    stop: value(&t[3], "stop", card.line)?,
                    step: value(&t[4], "step", card.line)?,
                });
            }
            ".options" | ".option" | ".opt" => {
                let t = &card.tokens;
                let mut i = 1;
                while i < t.len() {
                    if t.get(i + 1).map(String::as_str) == Some("=") {
                        options.insert(
                            t[i].to_ascii_lowercase(),
                            t.get(i + 2)
                                .cloned()
                                .unwrap_or_default()
                                .to_ascii_lowercase(),
                        );
                        i += 3;
                    } else {
                        i += 1;
                    }
                }
            }
            ".include" | ".inc" | ".lib" => {
                return Err(format!(
                "line {}: {} files cannot be read by this simulator; put the models in the deck",
                card.line, card.tokens[0]
            ))
            }
            ".title" | ".save" | ".probe" | ".print" | ".plot" | ".temp" | ".ic" | ".nodeset"
            | ".param" | ".global" | ".csparam" | ".meas" | ".measure" => {}
            other if other.starts_with('.') => {
                return Err(format!("line {}: unknown control {other}", card.line))
            }
            _ => top.push(card),
        }
    }
    if open.is_some() {
        return Err(".subckt without .ends".into());
    }
    let mut builder = Builder {
        circuit: Circuit {
            title,
            nodes: vec!["0".into()],
            devices: vec![],
            analyses,
            trapezoidal: true,
            digital: Default::default(),
            options: Options::default(),
        },
        node_index: BTreeMap::new(),
        models,
        subckts,
    };
    for (key, slot) in [
        ("reltol", &mut builder.circuit.options.reltol),
        ("abstol", &mut builder.circuit.options.abstol),
        ("vntol", &mut builder.circuit.options.vntol),
        ("chgtol", &mut builder.circuit.options.chgtol),
        ("trtol", &mut builder.circuit.options.trtol),
    ] {
        if let Some(v) = options.get(key) {
            *slot = parse_value(v)
                .filter(|x| *x > 0.0)
                .ok_or_else(|| format!(".options {key}={v} is not a positive number"))?;
        }
    }
    if let Some(method) = options.get("method") {
        builder.circuit.trapezoidal = match method.as_str() {
            "trap" | "trapezoidal" => true,
            "gear" | "euler" | "be" => false,
            other => return Err(format!("integration method {other} is not supported")),
        };
    }
    builder.expand(&top, "", &BTreeMap::new(), 0)?;
    let mut seen = std::collections::BTreeSet::new();
    for d in &builder.circuit.devices {
        if !seen.insert(d.name().to_ascii_lowercase()) {
            return Err(format!("element {} is defined twice", d.name()));
        }
    }
    if builder.circuit.devices.is_empty() {
        return Err("the circuit has no elements".into());
    }
    Ok(builder.circuit)
}
