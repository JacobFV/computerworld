//! Electrical rules check, after KiCad's: unconnected pins, pin-type conflicts from the
//! pin-conflict matrix, power inputs no power output drives (the missing PWR_FLAG),
//! undriven inputs, dangling wires and labels, annotation problems, sheet pins without
//! hierarchical labels (and the reverse), and bus conflicts — over every sheet.
use crate::connectivity::{analyze, Connectivity, NetPin};
use crate::geom::{on_segment, Pt};
use crate::schematic::{global_id, Schematic};
use crate::symbols::PinType;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    pub severity: Severity,
    /// KiCad's rule key, e.g. `pin_not_connected`.
    pub rule: String,
    pub message: String,
    pub pos: Pt,
    /// Human description of the items involved ("Pin 1 of R1").
    pub items: Vec<String>,
    /// The sheet the marker is on ("/" for the root, "/amp/" for a sub-sheet).
    #[serde(default = "root_path")]
    pub sheet: String,
}
fn root_path() -> String {
    "/".into()
}

/// KiCad's default pin conflict matrix: `None` is fine.
pub fn conflict(a: PinType, b: PinType) -> Option<Severity> {
    use PinType::*;
    use Severity::*;
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    match (a, b) {
        (Output, Output) => Some(Error),
        (Output, PowerOut) | (PowerOut, PowerOut) => Some(Error),
        (Output, OpenCollector) | (Output, OpenEmitter) => Some(Error),
        (Bidirectional, PowerOut) | (TriState, PowerOut) => Some(Error),
        (PowerOut, OpenCollector) | (PowerOut, OpenEmitter) => Some(Error),
        (Output, TriState) => Some(Warning),
        (Input, NoConnect)
        | (Output, NoConnect)
        | (Bidirectional, NoConnect)
        | (TriState, NoConnect)
        | (Passive, NoConnect)
        | (Unspecified, NoConnect)
        | (PowerIn, NoConnect)
        | (PowerOut, NoConnect)
        | (OpenCollector, NoConnect)
        | (OpenEmitter, NoConnect)
        | (NoConnect, NoConnect) => Some(Error),
        (Unspecified, _) | (_, Unspecified) => Some(Warning),
        _ => None,
    }
}

fn describe(p: &NetPin) -> String {
    if p.name.is_empty() || p.name == "~" {
        format!("Pin {} of {}", p.number, p.reference)
    } else {
        format!("Pin {} ({}) of {}", p.number, p.name, p.reference)
    }
}

pub fn check(sch: &Schematic) -> Vec<Violation> {
    let conn = analyze(sch);
    check_with(sch, &conn)
}

pub fn check_with(sch: &Schematic, conn: &Connectivity) -> Vec<Violation> {
    let mut out = Vec::new();
    let v = |severity: Severity,
             rule: &str,
             message: String,
             sheet: &str,
             pos: Pt,
             items: Vec<String>| Violation {
        severity,
        rule: rule.into(),
        message,
        pos,
        items,
        sheet: sheet.into(),
    };
    // Annotation. The units of one multi-unit part share a reference.
    let mut refs: BTreeMap<String, Vec<(String, &crate::schematic::SymbolInst)>> = BTreeMap::new();
    for (_, path, s) in sch.all_symbols() {
        if !s.annotated() {
            out.push(v(
                Severity::Error,
                "unannotated",
                format!("Symbol {} has not been annotated", s.reference()),
                &path,
                s.pos,
                vec![format!("Symbol {} [{}]", s.reference(), s.value())],
            ));
        } else {
            refs.entry(s.reference().to_owned())
                .or_default()
                .push((path, s));
        }
    }
    for (r, list) in &refs {
        let clash = list.iter().enumerate().any(|(i, (_, a))| {
            list[i + 1..].iter().any(|(_, b)| {
                let multi = a.lib().is_some_and(|l| l.units > 1);
                !(multi && a.lib_id == b.lib_id && a.unit != b.unit)
            })
        });
        if clash {
            out.push(v(
                Severity::Error,
                "duplicate_reference",
                format!("Duplicate reference designator {r}"),
                &list[1].0,
                list[1].1.pos,
                list.iter()
                    .map(|(_, s)| format!("Symbol {} [{}]", s.unit_reference(), s.value()))
                    .collect(),
            ));
        }
    }
    for issue in &conn.issues {
        let severity = if issue.rule == "multiple_net_names" {
            Severity::Warning
        } else {
            Severity::Error
        };
        out.push(v(
            severity,
            issue.rule,
            issue.message.clone(),
            &issue.sheet,
            issue.at,
            vec![],
        ));
    }
    for net in &conn.nets {
        let real: Vec<&NetPin> = net.pins.iter().collect();
        let flagged = |p: &NetPin| {
            let uid = (p.symbol >> 32) as u32;
            conn.no_connect_points.contains(&(uid, p.at))
        };
        // Unconnected pins: alone on their net, with nothing else attached.
        if real.len() == 1 && net.wires.is_empty() && net.labels.is_empty() && !net.named {
            let p = real[0];
            if !flagged(p) && p.kind != PinType::NoConnect && !p.power {
                out.push(v(
                    Severity::Error,
                    "pin_not_connected",
                    "Pin not connected".into(),
                    &p.sheet,
                    p.at,
                    vec![describe(p)],
                ));
            }
        }
        // No-connect flags on connected pins.
        if real.len() > 1 {
            for p in &real {
                if flagged(p) {
                    out.push(v(
                        Severity::Warning,
                        "no_connect_connected",
                        "A pin with a \"no connection\" flag is connected".into(),
                        &p.sheet,
                        p.at,
                        vec![describe(p)],
                    ));
                }
            }
        }
        // Pin-type conflicts, one report per offending pair of types.
        let mut seen = std::collections::BTreeSet::new();
        for (i, a) in real.iter().enumerate() {
            for b in &real[i + 1..] {
                if a.power && b.power {
                    continue;
                }
                if let Some(sev) = conflict(a.kind, b.kind) {
                    let key = (a.kind.min(b.kind), a.kind.max(b.kind));
                    if seen.insert(key) {
                        out.push(v(
                            sev,
                            "pin_to_pin",
                            format!(
                                "Pins of type {} and {} are connected",
                                a.kind.label(),
                                b.kind.label()
                            ),
                            &b.sheet,
                            b.at,
                            vec![describe(a), describe(b)],
                        ));
                    }
                }
            }
        }
        // Power inputs need a power output on the net (a regulator, or a PWR_FLAG).
        let has_power_out = real.iter().any(|p| p.kind == PinType::PowerOut);
        if !has_power_out {
            if let Some(p) = real.iter().find(|p| p.kind == PinType::PowerIn) {
                out.push(v(
                    Severity::Error,
                    "power_pin_not_driven",
                    format!(
                        "Input Power pin not driven by any Output Power pins (net {})",
                        net.name
                    ),
                    &p.sheet,
                    p.at,
                    vec![describe(p)],
                ));
            }
        }
        // Inputs need something that drives the net.
        let driven = real
            .iter()
            .any(|p| p.kind.drives() || p.kind == PinType::PowerIn);
        if !driven {
            if let Some(p) = real.iter().find(|p| p.kind == PinType::Input) {
                out.push(v(
                    Severity::Error,
                    "pin_not_driven",
                    "Input pin not driven by any Output pins".into(),
                    &p.sheet,
                    p.at,
                    vec![describe(p)],
                ));
            }
        }
        // Labels on nets that reach no pin.
        if real.is_empty() {
            for (path, _, uid, s) in sch.sheets_flat() {
                for l in s
                    .labels
                    .iter()
                    .filter(|l| net.labels.contains(&global_id(uid, l.id)))
                {
                    out.push(v(
                        Severity::Warning,
                        "label_dangling",
                        format!("Label '{}' not connected to anything", l.text),
                        &path,
                        l.pos,
                        vec![format!("Label '{}'", l.text)],
                    ));
                }
            }
        }
    }
    // Wire and bus ends touching nothing.
    for (path, _, _, s) in sch.sheets_flat() {
        let points = s.connection_points();
        for w in &s.wires {
            for end in [w.a, w.b] {
                let touching = points.iter().filter(|p| **p == end).count() > 1
                    || s.wires
                        .iter()
                        .any(|o| o.id != w.id && on_segment(end, o.a, o.b))
                    || s.labels.iter().any(|l| l.pos == end)
                    || s.no_connects.iter().any(|n| n.pos == end);
                if !touching {
                    out.push(v(
                        Severity::Warning,
                        "unconnected_wire_endpoint",
                        if w.bus {
                            "Unconnected bus endpoint".into()
                        } else {
                            "Unconnected wire endpoint".into()
                        },
                        &path,
                        end,
                        vec![if w.bus { "Bus".into() } else { "Wire".into() }],
                    ));
                }
            }
        }
        for e in &s.bus_entries {
            let on_bus = s.wires.iter().any(|w| w.bus && on_segment(e.pos, w.a, w.b))
                || s.wires
                    .iter()
                    .any(|w| w.bus && on_segment(e.end(), w.a, w.b));
            if !on_bus {
                out.push(v(
                    Severity::Warning,
                    "bus_entry_needed",
                    "Bus entry is not on a bus".into(),
                    &path,
                    e.pos,
                    vec!["Bus entry".into()],
                ));
            }
        }
    }
    out.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then(a.sheet.cmp(&b.sheet))
            .then(a.pos.cmp(&b.pos))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Xf;
    #[test]
    fn missing_power_flag_and_unconnected_pins_are_reported() {
        let mut s = Schematic::new("t");
        s.place("Timer:NE555P", Pt::new(3000, 3000), Xf::IDENTITY)
            .unwrap();
        s.place("power:GND", Pt::new(3000, 3400), Xf::IDENTITY)
            .unwrap();
        s.annotate(true, false);
        let v = check(&s);
        assert!(v.iter().any(|v| v.rule == "power_pin_not_driven"), "{v:#?}");
        assert!(v.iter().filter(|v| v.rule == "pin_not_connected").count() >= 6);
        s.place("power:PWR_FLAG", Pt::new(3000, 3400), Xf::IDENTITY)
            .unwrap();
        s.annotate(true, false);
        let v = check(&s);
        assert!(
            !v.iter()
                .any(|v| v.rule == "power_pin_not_driven" && v.message.contains("GND")),
            "{v:#?}"
        );
    }
    #[test]
    fn two_outputs_on_a_net_conflict() {
        assert_eq!(
            conflict(PinType::Output, PinType::Output),
            Some(Severity::Error)
        );
        assert_eq!(conflict(PinType::Passive, PinType::Input), None);
        assert_eq!(
            conflict(PinType::Unspecified, PinType::Passive),
            Some(Severity::Warning)
        );
        let mut s = Schematic::new("t");
        let a = s
            .place("74xGxx:74LVC1G04", Pt::new(1000, 1000), Xf::IDENTITY)
            .unwrap();
        let b = s
            .place("74xGxx:74LVC1G04", Pt::new(1000, 2000), Xf::IDENTITY)
            .unwrap();
        let ya = s
            .symbol(a)
            .unwrap()
            .pins()
            .iter()
            .find(|p| p.0.number == "4")
            .unwrap()
            .1;
        let yb = s
            .symbol(b)
            .unwrap()
            .pins()
            .iter()
            .find(|p| p.0.number == "4")
            .unwrap()
            .1;
        s.add_wire(ya, Pt::new(ya.x + 200, ya.y));
        s.add_wire(Pt::new(ya.x + 200, ya.y), Pt::new(yb.x + 200, yb.y));
        s.add_wire(Pt::new(yb.x + 200, yb.y), yb);
        s.annotate(true, false);
        let v = check(&s);
        assert!(
            v.iter()
                .any(|v| v.rule == "pin_to_pin" && v.severity == Severity::Error),
            "{v:#?}"
        );
    }
    #[test]
    fn unannotated_and_duplicate_references_are_errors() {
        let mut s = Schematic::new("t");
        s.place("Device:R", Pt::new(1000, 1000), Xf::IDENTITY)
            .unwrap();
        assert!(check(&s).iter().any(|v| v.rule == "unannotated"));
        s.annotate(true, false);
        let id = s
            .place("Device:R", Pt::new(2000, 1000), Xf::IDENTITY)
            .unwrap();
        s.symbol_mut(id).unwrap().set_field("Reference", "R1");
        assert!(check(&s).iter().any(|v| v.rule == "duplicate_reference"));
    }
}
