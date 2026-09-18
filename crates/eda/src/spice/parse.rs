//! SPICE deck parser. Reads the subset of the Berkeley SPICE3 / ngspice input language
//! that a schematic exports: passive and controlled elements, independent sources with
//! DC/AC/PULSE/SIN/PWL specifications, diodes, BJTs, level-1 MOSFETs, `.model`,
//! `.subckt` (flattened on the way in) and the analysis commands.
use super::{Analysis, BjtModel, Circuit, Device, DiodeModel, MosModel, Source, Wave};
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
    let p = params(&t[3..], card.line)?;
    let get = |k: &str, d: f64| p.get(k).copied().unwrap_or(d);
    let model = match kind.as_str() {
        "d" => Model::Diode(DiodeModel {
            is: get("is", 1e-14),
            n: get("n", 1.0),
            rs: get("rs", 0.0),
            bv: p.get("bv").copied(),
            ibv: get("ibv", 1e-3),
            cjo: get("cjo", get("cj0", 0.0)),
            vj: get("vj", 1.0),
            m: get("m", 0.5),
            tt: get("tt", 0.0),
            fc: get("fc", 0.5),
        }),
        "npn" | "pnp" => Model::Bjt(BjtModel {
            pnp: kind == "pnp",
            is: get("is", 1e-16),
            bf: get("bf", 100.0),
            br: get("br", 1.0),
            nf: get("nf", 1.0),
            nr: get("nr", 1.0),
            vaf: p.get("vaf").or(p.get("va")).copied(),
        }),
        "nmos" | "pmos" => {
            let level = get("level", 1.0);
            if level != 1.0 {
                return Err(format!(
                    "line {}: MOSFET model {name} is level {level}; only level 1 is supported",
                    card.line
                ));
            }
            Model::Mos(MosModel {
                pmos: kind == "pmos",
                vto: get("vto", 0.0),
                kp: get("kp", 2e-5),
                lambda: get("lambda", 0.0),
            })
        }
        other => {
            return Err(format!(
                "line {}: model type {other} is not supported (d, npn, pnp, nmos, pmos)",
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
                        internal: None,
                        model,
                        area,
                    });
                }
                'q' => {
                    need(5)?;
                    let nodes: Vec<String> = t[1..4].iter().map(|n| resolve(n)).collect();
                    // An optional substrate node sits before the model name.
                    let model_token =
                        if t.len() >= 6 && !self.models.contains_key(&t[4].to_ascii_lowercase()) {
                            &t[5]
                        } else {
                            &t[4]
                        };
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
                    });
                }
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
        },
        node_index: BTreeMap::new(),
        models,
        subckts,
    };
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
