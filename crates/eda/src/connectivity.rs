//! Schematic connectivity: which pins, wires and labels form each net, and what each net
//! is called — the same rules KiCad's netlister applies.
//!
//! Things connect when they share a point; a wire end or pin that lands part-way along a
//! wire connects to it (KiCad marks it with a junction); wires that merely cross do not.
//! Labels with the same text join their nets, global labels across the whole design,
//! and a power symbol names its net after its value.
use crate::geom::{on_segment, Pt};
use crate::schematic::{LabelKind, Schematic};
use crate::symbols::PinType;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetPin {
    pub symbol: u64,
    pub reference: String,
    pub number: String,
    pub name: String,
    pub kind: PinType,
    pub at: Pt,
    /// The symbol is a power symbol or flag (not a real component).
    pub power: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Net {
    pub code: u32,
    pub name: String,
    pub pins: Vec<NetPin>,
    pub wires: Vec<u64>,
    pub labels: Vec<u64>,
    /// Named by a power symbol, a global label or a local label (not auto-named).
    pub named: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connectivity {
    pub nets: Vec<Net>,
    /// Net index by (symbol id, pin number).
    pub pin_net: BTreeMap<(u64, String), usize>,
    pub wire_net: BTreeMap<u64, usize>,
    pub label_net: BTreeMap<u64, usize>,
    /// Points where a no-connect flag sits on something.
    pub no_connect_points: Vec<Pt>,
}

struct Dsu(Vec<usize>);
impl Dsu {
    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            // Keep the smaller index as root so results do not depend on call order.
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            self.0[hi] = lo;
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Node {
    Pin(usize, usize),
    Wire(usize),
    Label(usize),
}

impl Connectivity {
    pub fn net_of_pin(&self, symbol: u64, number: &str) -> Option<&Net> {
        self.pin_net
            .get(&(symbol, number.to_owned()))
            .map(|i| &self.nets[*i])
    }
    pub fn net_named(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.name == name)
    }
}

pub fn analyze(sch: &Schematic) -> Connectivity {
    let mut nodes: Vec<Node> = Vec::new();
    // Every point where a node touches, with the node.
    let mut at: BTreeMap<Pt, Vec<usize>> = BTreeMap::new();
    let pins: Vec<_> = sch.symbols.iter().map(|s| s.pins()).collect();
    for (si, list) in pins.iter().enumerate() {
        for (pi, (pin, p)) in list.iter().enumerate() {
            if pin.kind == PinType::NoConnect && pin.hidden {
                continue;
            }
            let n = nodes.len();
            nodes.push(Node::Pin(si, pi));
            at.entry(*p).or_default().push(n);
        }
    }
    let first_wire = nodes.len();
    for (wi, w) in sch.wires.iter().enumerate() {
        let n = nodes.len();
        nodes.push(Node::Wire(wi));
        at.entry(w.a).or_default().push(n);
        at.entry(w.b).or_default().push(n);
    }
    for (li, l) in sch.labels.iter().enumerate() {
        let n = nodes.len();
        nodes.push(Node::Label(li));
        at.entry(l.pos).or_default().push(n);
    }
    let mut dsu = Dsu((0..nodes.len()).collect());
    for list in at.values() {
        for w in list.windows(2) {
            dsu.union(w[0], w[1]);
        }
    }
    // Points that land part-way along a wire.
    for (p, list) in &at {
        for (wi, w) in sch.wires.iter().enumerate() {
            if *p != w.a && *p != w.b && on_segment(*p, w.a, w.b) {
                for n in list {
                    dsu.union(*n, first_wire + wi);
                }
            }
        }
    }
    // Same-named labels, and power symbols of the same value.
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    for (i, node) in nodes.iter().enumerate() {
        let key = match node {
            Node::Label(li) => {
                let l = &sch.labels[*li];
                Some(match l.kind {
                    LabelKind::Global => format!("g:{}", l.text),
                    LabelKind::Local => format!("l:{}", l.text),
                })
            }
            Node::Pin(si, _) => {
                let s = &sch.symbols[*si];
                if s.is_power() && s.lib_id != "power:PWR_FLAG" {
                    Some(format!("g:{}", s.value()))
                } else {
                    None
                }
            }
            Node::Wire(_) => None,
        };
        if let Some(key) = key {
            match by_name.get(&key) {
                Some(first) => dsu.union(*first, i),
                None => {
                    by_name.insert(key, i);
                }
            }
        }
    }
    // Power names and global labels share the "g:" key space above, so a global label
    // spelled like a power net joins it; local labels stay local, as in KiCad.
    // Group by root.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..nodes.len() {
        let r = dsu.find(i);
        groups.entry(r).or_default().push(i);
    }
    let mut nets: Vec<Net> = Vec::new();
    let mut out = Connectivity::default();
    for members in groups.values() {
        let mut net = Net::default();
        let mut power_name: Option<String> = None;
        let mut global_name: Option<String> = None;
        let mut local_name: Option<String> = None;
        for &m in members {
            match nodes[m] {
                Node::Pin(si, pi) => {
                    let s = &sch.symbols[si];
                    let (pin, p) = &pins[si][pi];
                    if s.is_power() && s.lib_id != "power:PWR_FLAG" {
                        let v = s.value().to_owned();
                        if power_name.as_ref().is_none_or(|n| v < *n) {
                            power_name = Some(v);
                        }
                    }
                    net.pins.push(NetPin {
                        symbol: s.id,
                        reference: s.reference().to_owned(),
                        number: pin.number.to_owned(),
                        name: pin.name.to_owned(),
                        kind: pin.kind,
                        at: *p,
                        power: s.is_power(),
                    });
                }
                Node::Wire(wi) => net.wires.push(sch.wires[wi].id),
                Node::Label(li) => {
                    let l = &sch.labels[li];
                    net.labels.push(l.id);
                    let slot = match l.kind {
                        LabelKind::Global => &mut global_name,
                        LabelKind::Local => &mut local_name,
                    };
                    if slot.as_ref().is_none_or(|n| l.text < *n) {
                        *slot = Some(l.text.clone());
                    }
                }
            }
        }
        net.pins.sort_by(|a, b| {
            natural(&a.reference)
                .cmp(&natural(&b.reference))
                .then(natural(&a.number).cmp(&natural(&b.number)))
        });
        let named = power_name.or(global_name).or(local_name);
        net.named = named.is_some();
        net.name = match named {
            Some(n) => n,
            None => {
                let real: Vec<&NetPin> = net.pins.iter().filter(|p| !p.power).collect();
                match real.first() {
                    Some(p) if real.len() == 1 && net.wires.is_empty() => {
                        format!("unconnected-({}-Pad{})", p.reference, p.number)
                    }
                    Some(p) => format!("Net-({}-Pad{})", p.reference, p.number),
                    None if !net.wires.is_empty() => {
                        let min = net.wires.iter().min().copied().unwrap_or(0);
                        format!("Net-(wire{min})")
                    }
                    None => continue,
                }
            }
        };
        nets.push(net);
    }
    nets.sort_by_key(|n| natural(&n.name));
    // Two separate groups can end up with the same auto name only if they share a pin,
    // which cannot happen; same explicit names were merged above.
    for (i, net) in nets.iter_mut().enumerate() {
        net.code = i as u32 + 1;
    }
    for (i, net) in nets.iter().enumerate() {
        for p in &net.pins {
            out.pin_net.insert((p.symbol, p.number.clone()), i);
        }
        for w in &net.wires {
            out.wire_net.insert(*w, i);
        }
        for l in &net.labels {
            out.label_net.insert(*l, i);
        }
    }
    out.no_connect_points = sch.no_connects.iter().map(|n| n.pos).collect();
    out.nets = nets;
    out
}

/// Sort key that orders "R2" before "R10".
pub fn natural(s: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut digits = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            if !digits.is_empty() {
                out.push((
                    std::mem::take(&mut text),
                    digits.parse().unwrap_or(u64::MAX),
                ));
                digits.clear();
            }
            text.push(ch);
        }
    }
    out.push((
        text,
        if digits.is_empty() {
            0
        } else {
            digits.parse().unwrap_or(u64::MAX)
        },
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Xf;
    #[test]
    fn an_rc_divider_has_three_nets() {
        let mut s = Schematic::new("t");
        let r = s
            .place("Device:R", Pt::new(1000, 1000), Xf::IDENTITY)
            .unwrap();
        let c = s
            .place("Device:C", Pt::new(1000, 1500), Xf::IDENTITY)
            .unwrap();
        let g = s
            .place("power:GND", Pt::new(1000, 1800), Xf::IDENTITY)
            .unwrap();
        s.add_wire(Pt::new(1000, 1150), Pt::new(1000, 1350));
        s.add_wire(Pt::new(1000, 1650), Pt::new(1000, 1800));
        s.add_label(Pt::new(1000, 1250), "out", LabelKind::Local);
        s.annotate(true, false);
        let c_ = analyze(&s);
        let out = c_.net_named("out").expect("labelled net");
        assert_eq!(out.pins.len(), 2);
        assert!(c_.net_named("GND").is_some());
        assert_eq!(c_.net_of_pin(g, "1").unwrap().name, "GND");
        assert_eq!(c_.net_of_pin(c, "2").unwrap().name, "GND");
        let top = c_.net_of_pin(r, "1").unwrap();
        assert_eq!(top.name, "unconnected-(R1-Pad1)");
    }
    #[test]
    fn crossing_wires_do_not_connect_but_touching_ends_do() {
        let mut s = Schematic::new("t");
        s.add_wire(Pt::new(0, 500), Pt::new(1000, 500));
        s.add_wire(Pt::new(500, 0), Pt::new(500, 1000));
        s.add_label(Pt::new(0, 500), "a", LabelKind::Local);
        s.add_label(Pt::new(500, 0), "b", LabelKind::Local);
        let c = analyze(&s);
        assert!(c.net_named("a").is_some() && c.net_named("b").is_some());
        s.add_wire(Pt::new(500, 500), Pt::new(700, 700));
        // A wire ending at the crossing touches both mid-points: now one net.
        let c = analyze(&s);
        assert_eq!(c.nets.len(), 1);
    }
    #[test]
    fn natural_order_sorts_numbers_numerically() {
        let mut v = vec!["R10", "R2", "C1", "R1"];
        v.sort_by_key(|s| natural(s));
        assert_eq!(v, vec!["C1", "R1", "R2", "R10"]);
    }
}
