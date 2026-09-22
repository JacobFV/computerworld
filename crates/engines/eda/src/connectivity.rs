//! Schematic connectivity: which pins, wires and labels form each net, and what each net
//! is called — the same rules KiCad's netlister applies, across the sheet hierarchy.
//!
//! Things connect when they share a point; a wire end or pin that lands part-way along a
//! wire connects to it (KiCad marks it with a junction); wires that merely cross do not.
//! Labels with the same text join their nets: local and hierarchical labels within their
//! own sheet, global labels across the whole design, and a power symbol names its net
//! after its value. A hierarchical label meets the sheet pin of the same name on the
//! sheet symbol above it.
//!
//! Buses carry the members their labels name (`D[0..7]` is D0…D7): a member is the net
//! of that name on the bus's sheet, and a bus passing through a sheet pin joins the
//! child sheet's bus member by member, in order. Net names follow KiCad's driver
//! priority — power, global label, local label, hierarchical label, sheet pin — and,
//! among equals, the one nearest the root; names from inside a sheet carry its path
//! (`/amp/in`).
use crate::geom::{on_segment, Pt};
use crate::schematic::{bus_members, global_id, LabelKind, Schematic, SheetSymbol};
use crate::symbols::PinType;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetPin {
    /// The symbol's design-wide identity (`schematic::global_id`).
    pub symbol: u64,
    pub reference: String,
    pub number: String,
    pub name: String,
    pub kind: PinType,
    pub at: Pt,
    /// The symbol is a power symbol or flag (not a real component).
    pub power: bool,
    /// Sheet path of the pin ("/" on the root).
    pub sheet: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Net {
    pub code: u32,
    pub name: String,
    pub pins: Vec<NetPin>,
    /// Global identities of the wires and labels on the net.
    pub wires: Vec<u64>,
    pub labels: Vec<u64>,
    /// Named by a power symbol, a label, a sheet pin or a bus member (not auto-named).
    pub named: bool,
}

/// A connectivity problem the electrical rules check reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub rule: &'static str,
    pub message: String,
    pub sheet: String,
    pub at: Pt,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connectivity {
    pub nets: Vec<Net>,
    /// Net index by (symbol global id, pin number).
    pub pin_net: BTreeMap<(u64, String), usize>,
    pub wire_net: BTreeMap<u64, usize>,
    pub label_net: BTreeMap<u64, usize>,
    /// Points where a no-connect flag sits, by sheet instance.
    pub no_connect_points: Vec<(u32, Pt)>,
    /// Members each bus wire carries, by its global identity.
    pub bus_wires: BTreeMap<u64, Vec<String>>,
    pub issues: Vec<Issue>,
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
    fn push(&mut self) -> usize {
        self.0.push(self.0.len());
        self.0.len() - 1
    }
}

#[derive(Clone, Copy, Debug)]
enum Node {
    Pin(usize, usize, usize),
    Wire(usize, usize),
    Label(usize, usize),
    /// Sheet pin: parent sheet instance, sheet symbol index, pin index.
    SheetPin(usize, usize, usize),
    /// A bus member's net on a sheet: (sheet instance, member name).
    Member(usize, usize),
}

/// A sheet instance of the design.
struct Inst<'a> {
    path: String,
    uid: u32,
    sheet: &'a Schematic,
    /// The instance holding it and the sheet symbol standing for it.
    parent: Option<(usize, &'a SheetSymbol)>,
    depth: usize,
}

fn instances(root: &Schematic) -> Vec<Inst<'_>> {
    fn walk<'a>(
        s: &'a Schematic,
        path: String,
        uid: u32,
        parent: Option<(usize, &'a SheetSymbol)>,
        depth: usize,
        out: &mut Vec<Inst<'a>>,
    ) {
        let me = out.len();
        out.push(Inst {
            path: path.clone(),
            uid,
            sheet: s,
            parent,
            depth,
        });
        for sh in &s.sheets {
            walk(
                &sh.contents,
                format!("{path}{}/", sh.name),
                sh.uid,
                Some((me, sh)),
                depth + 1,
                out,
            );
        }
    }
    let mut out = vec![];
    walk(root, "/".into(), 0, None, 0, &mut out);
    out
}

impl Connectivity {
    pub fn net_of_pin(&self, symbol: u64, number: &str) -> Option<&Net> {
        self.pin_net
            .get(&(symbol, number.to_owned()))
            .map(|i| &self.nets[*i])
    }
    /// The net of pin `number` of the part `reference`, whichever unit carries it.
    pub fn net_of_ref_pin(&self, reference: &str, number: &str) -> Option<&Net> {
        self.nets.iter().find(|n| {
            n.pins
                .iter()
                .any(|p| p.reference == reference && p.number == number)
        })
    }
    pub fn net_named(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.name == name)
    }
}

/// Driver priority of a net name, KiCad's order.
const PRIO_POWER: u8 = 6;
const PRIO_GLOBAL: u8 = 5;
const PRIO_LOCAL: u8 = 4;
const PRIO_HIER: u8 = 3;
const PRIO_SHEET_PIN: u8 = 2;

pub fn analyze(root: &Schematic) -> Connectivity {
    let inst = instances(root);
    let mut out = Connectivity::default();
    let mut nodes: Vec<Node> = Vec::new();
    let mut dsu = Dsu(vec![]);
    let add = |nodes: &mut Vec<Node>, dsu: &mut Dsu, n: Node| {
        nodes.push(n);
        dsu.push()
    };
    // Per sheet: every point where a net-level node touches, and the bus-level items.
    let mut pins_of: Vec<Vec<Vec<(crate::symbols::LibPin, Pt)>>> = vec![];
    let mut label_nodes: Vec<Vec<Option<usize>>> = vec![];
    let mut sheet_pin_nodes: BTreeMap<(usize, usize, usize), usize> = BTreeMap::new();
    let mut member_names: Vec<(usize, String)> = vec![];
    let mut member_index: BTreeMap<(usize, String), usize> = BTreeMap::new();
    for (k, it) in inst.iter().enumerate() {
        let s = it.sheet;
        let mut at: BTreeMap<Pt, Vec<usize>> = BTreeMap::new();
        let pins: Vec<Vec<(crate::symbols::LibPin, Pt)>> = s
            .symbols
            .iter()
            .map(|sym| {
                sym.pins()
                    .into_iter()
                    .map(|(p, q)| (p.clone(), q))
                    .collect()
            })
            .collect();
        for (si, list) in pins.iter().enumerate() {
            for (pi, (pin, p)) in list.iter().enumerate() {
                if pin.kind == PinType::NoConnect && pin.hidden {
                    continue;
                }
                let n = add(&mut nodes, &mut dsu, Node::Pin(k, si, pi));
                at.entry(*p).or_default().push(n);
            }
        }
        pins_of.push(pins);
        let mut wire_node = vec![None; s.wires.len()];
        for (wi, w) in s.wires.iter().enumerate() {
            if w.bus {
                continue;
            }
            let n = add(&mut nodes, &mut dsu, Node::Wire(k, wi));
            wire_node[wi] = Some(n);
            at.entry(w.a).or_default().push(n);
            at.entry(w.b).or_default().push(n);
        }
        // Labels: net labels join the net graph; bus labels name buses.
        let bus_point = |p: Pt| s.wires.iter().any(|w| w.bus && on_segment(p, w.a, w.b));
        let mut lnodes = vec![None; s.labels.len()];
        for (li, l) in s.labels.iter().enumerate() {
            let is_bus = bus_members(&l.text).is_some();
            if is_bus {
                if !bus_point(l.pos) && l.kind == LabelKind::Local {
                    out.issues.push(Issue {
                        rule: "bus_to_net_conflict",
                        message: format!("Bus label '{}' is attached to a wire, not a bus", l.text),
                        sheet: it.path.clone(),
                        at: l.pos,
                    });
                }
                continue;
            }
            if bus_point(l.pos) && !at.contains_key(&l.pos) {
                out.issues.push(Issue {
                    rule: "bus_to_net_conflict",
                    message: format!("Net label '{}' is attached to a bus", l.text),
                    sheet: it.path.clone(),
                    at: l.pos,
                });
            }
            let n = add(&mut nodes, &mut dsu, Node::Label(k, li));
            lnodes[li] = Some(n);
            at.entry(l.pos).or_default().push(n);
        }
        label_nodes.push(lnodes);
        for (si, sh) in s.sheets.iter().enumerate() {
            for (pi, p) in sh.pins.iter().enumerate() {
                if bus_members(&p.name).is_some() {
                    continue;
                }
                let n = add(&mut nodes, &mut dsu, Node::SheetPin(k, si, pi));
                sheet_pin_nodes.insert((k, si, pi), n);
                at.entry(p.pos).or_default().push(n);
            }
        }
        for list in at.values() {
            for w in list.windows(2) {
                dsu.union(w[0], w[1]);
            }
        }
        // Points that land part-way along a wire.
        for (p, list) in &at {
            for (wi, w) in s.wires.iter().enumerate() {
                let Some(wn) = wire_node[wi] else { continue };
                if *p != w.a && *p != w.b && on_segment(*p, w.a, w.b) {
                    for n in list {
                        dsu.union(*n, wn);
                    }
                }
            }
        }
        for nc in &s.no_connects {
            out.no_connect_points.push((it.uid, nc.pos));
        }
        // ---- buses on this sheet ----------------------------------------------------
        let bus_idx: Vec<usize> = (0..s.wires.len()).filter(|i| s.wires[*i].bus).collect();
        let mut bdsu = Dsu((0..bus_idx.len()).collect());
        for (x, &i) in bus_idx.iter().enumerate() {
            for (y, &j) in bus_idx.iter().enumerate().skip(x + 1) {
                let (a, b) = (&s.wires[i], &s.wires[j]);
                if on_segment(a.a, b.a, b.b)
                    || on_segment(a.b, b.a, b.b)
                    || on_segment(b.a, a.a, a.b)
                    || on_segment(b.b, a.a, a.b)
                {
                    bdsu.union(x, y);
                }
            }
        }
        let group_at = |bdsu: &mut Dsu, p: Pt| -> Option<usize> {
            bus_idx
                .iter()
                .position(|&i| on_segment(p, s.wires[i].a, s.wires[i].b))
                .map(|x| bdsu.find(x))
        };
        // Members of each bus group: from its labels, else its pins.
        let mut group_names: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        // Bus sheet pins on this sheet, and this sheet's bus hierarchical labels.
        let mut group_pins: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for l in &s.labels {
            if bus_members(&l.text).is_none() {
                continue;
            }
            if let Some(g) = group_at(&mut bdsu, l.pos) {
                let list = if l.kind == LabelKind::Hierarchical {
                    &mut group_pins
                } else {
                    &mut group_names
                };
                list.entry(g).or_default().push(l.text.clone());
            }
        }
        for sh in &s.sheets {
            for p in &sh.pins {
                if bus_members(&p.name).is_some() {
                    if let Some(g) = group_at(&mut bdsu, p.pos) {
                        group_pins.entry(g).or_default().push(p.name.clone());
                    }
                }
            }
        }
        let mut group_members: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (x, &wi) in bus_idx.iter().enumerate() {
            let g = bdsu.find(x);
            if group_members.contains_key(&g) {
                continue;
            }
            let mut names = group_names.get(&g).cloned().unwrap_or_default();
            names.sort();
            names.dedup();
            if names.len() > 1 {
                out.issues.push(Issue {
                    rule: "multiple_net_names",
                    message: format!(
                        "Bus has more than one label ({}); {} is used",
                        names.join(", "),
                        names[0]
                    ),
                    sheet: it.path.clone(),
                    at: s.wires[wi].a,
                });
            }
            let source = names
                .first()
                .cloned()
                .or_else(|| group_pins.get(&g).and_then(|v| v.first().cloned()));
            let members = source.and_then(|n| bus_members(&n)).unwrap_or_default();
            if members.is_empty() {
                out.issues.push(Issue {
                    rule: "bus_to_net_conflict",
                    message: "Bus has no label naming its members".into(),
                    sheet: it.path.clone(),
                    at: s.wires[wi].a,
                });
            }
            group_members.insert(g, members);
        }
        for (x, &i) in bus_idx.iter().enumerate() {
            let g = bdsu.find(x);
            out.bus_wires.insert(
                global_id(it.uid, s.wires[i].id),
                group_members.get(&g).cloned().unwrap_or_default(),
            );
            for m in group_members.get(&g).cloned().unwrap_or_default() {
                member_index.entry((k, m.clone())).or_insert_with(|| {
                    let n = add(&mut nodes, &mut dsu, Node::Member(k, member_names.len()));
                    member_names.push((k, m));
                    n
                });
            }
        }
    }
    // Same-named labels, power symbols of the same value, and bus members.
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    let key_of = |nodes: &[Node], i: usize, member_names: &[(usize, String)]| -> Option<String> {
        match nodes[i] {
            Node::Label(k, li) => {
                let l = &inst[k].sheet.labels[li];
                Some(match l.kind {
                    LabelKind::Global => format!("g:{}", l.text),
                    LabelKind::Local | LabelKind::Hierarchical => {
                        format!("l:{}:{}", inst[k].uid, l.text)
                    }
                })
            }
            Node::Pin(k, si, _) => {
                let s = &inst[k].sheet.symbols[si];
                if s.is_power() && s.lib_id != "power:PWR_FLAG" {
                    Some(format!("g:{}", s.value()))
                } else {
                    None
                }
            }
            Node::Member(k, mi) => Some(format!("l:{}:{}", inst[k].uid, member_names[mi].1)),
            Node::Wire(..) | Node::SheetPin(..) => None,
        }
    };
    for i in 0..nodes.len() {
        if let Some(key) = key_of(&nodes, i, &member_names) {
            match by_name.get(&key) {
                Some(first) => dsu.union(*first, i),
                None => {
                    by_name.insert(key, i);
                }
            }
        }
    }
    // Hierarchy: sheet pins meet hierarchical labels; bus pins meet member by member.
    for (k, it) in inst.iter().enumerate() {
        let Some((pk, sheet_sym)) = it.parent else {
            continue;
        };
        let si = inst[pk]
            .sheet
            .sheets
            .iter()
            .position(|x| std::ptr::eq(x, sheet_sym))
            .unwrap_or(0);
        let hier: Vec<(usize, &crate::schematic::Label)> = it
            .sheet
            .labels
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kind == LabelKind::Hierarchical)
            .collect();
        for (pi, pin) in sheet_sym.pins.iter().enumerate() {
            let matching: Vec<&(usize, &crate::schematic::Label)> =
                hier.iter().filter(|(_, l)| l.text == pin.name).collect();
            if matching.is_empty() {
                out.issues.push(Issue {
                    rule: "hier_label_mismatch",
                    message: format!(
                        "Sheet pin {} has no matching hierarchical label inside sheet '{}'",
                        pin.name, sheet_sym.name
                    ),
                    sheet: inst[pk].path.clone(),
                    at: pin.pos,
                });
                continue;
            }
            match bus_members(&pin.name) {
                None => {
                    let Some(&pn) = sheet_pin_nodes.get(&(pk, si, pi)) else {
                        continue;
                    };
                    for (li, _) in &matching {
                        if let Some(ln) = label_nodes[k][*li] {
                            dsu.union(pn, ln);
                        }
                    }
                }
                Some(pin_members) => {
                    // The parent's bus at the pin names the outer members, the child's
                    // bus at its label the inner ones; they pair up in order.
                    let outer = bus_at(inst[pk].sheet, pin.pos).unwrap_or(pin_members.clone());
                    let inner = matching
                        .iter()
                        .find_map(|(_, l)| bus_at(it.sheet, l.pos))
                        .unwrap_or(pin_members.clone());
                    if outer.len() != inner.len() {
                        out.issues.push(Issue {
                            rule: "bus_to_bus_conflict",
                            message: format!(
                                "Bus of {} members meets a bus of {} at sheet pin {}",
                                outer.len(),
                                inner.len(),
                                pin.name
                            ),
                            sheet: inst[pk].path.clone(),
                            at: pin.pos,
                        });
                    }
                    for (a, b) in outer.iter().zip(&inner) {
                        let na = member_node(
                            &mut nodes,
                            &mut dsu,
                            &mut member_names,
                            &mut member_index,
                            &mut by_name,
                            pk,
                            inst[pk].uid,
                            a,
                        );
                        let nb = member_node(
                            &mut nodes,
                            &mut dsu,
                            &mut member_names,
                            &mut member_index,
                            &mut by_name,
                            k,
                            it.uid,
                            b,
                        );
                        dsu.union(na, nb);
                    }
                }
            }
        }
        for (_, l) in &hier {
            if !sheet_sym.pins.iter().any(|p| p.name == l.text) {
                out.issues.push(Issue {
                    rule: "hier_label_mismatch",
                    message: format!(
                        "Hierarchical label {} has no matching sheet pin on sheet '{}'",
                        l.text, sheet_sym.name
                    ),
                    sheet: it.path.clone(),
                    at: l.pos,
                });
            }
        }
    }
    // Hierarchical labels on the root sheet lead nowhere.
    for l in &root.labels {
        if l.kind == LabelKind::Hierarchical {
            out.issues.push(Issue {
                rule: "hier_label_mismatch",
                message: format!("Hierarchical label {} is on the root sheet", l.text),
                sheet: "/".into(),
                at: l.pos,
            });
        }
    }
    // Group by root.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..nodes.len() {
        let r = dsu.find(i);
        groups.entry(r).or_default().push(i);
    }
    let mut nets: Vec<Net> = Vec::new();
    for members in groups.values() {
        let mut net = Net::default();
        // Best name: (priority, nearest the root, alphabetical).
        let mut best: Option<(u8, std::cmp::Reverse<usize>, std::cmp::Reverse<String>)> = None;
        let mut offer = |prio: u8, depth: usize, name: String| {
            let cand = (prio, std::cmp::Reverse(depth), std::cmp::Reverse(name));
            if best.as_ref().is_none_or(|b| cand > *b) {
                best = Some(cand);
            }
        };
        let scoped = |k: usize, text: &str| -> String {
            if inst[k].depth == 0 {
                text.to_owned()
            } else {
                format!("{}{text}", inst[k].path)
            }
        };
        for &m in members {
            match nodes[m] {
                Node::Pin(k, si, pi) => {
                    let it = &inst[k];
                    let s = &it.sheet.symbols[si];
                    let (pin, p) = &pins_of[k][si][pi];
                    if s.is_power() && s.lib_id != "power:PWR_FLAG" {
                        offer(PRIO_POWER, 0, s.value().to_owned());
                    }
                    net.pins.push(NetPin {
                        symbol: global_id(it.uid, s.id),
                        reference: s.reference().to_owned(),
                        number: pin.number.clone(),
                        name: pin.name.clone(),
                        kind: pin.kind,
                        at: *p,
                        power: s.is_power(),
                        sheet: it.path.clone(),
                    });
                }
                Node::Wire(k, wi) => net
                    .wires
                    .push(global_id(inst[k].uid, inst[k].sheet.wires[wi].id)),
                Node::Label(k, li) => {
                    let l = &inst[k].sheet.labels[li];
                    net.labels.push(global_id(inst[k].uid, l.id));
                    match l.kind {
                        LabelKind::Global => offer(PRIO_GLOBAL, 0, l.text.clone()),
                        LabelKind::Local => offer(PRIO_LOCAL, inst[k].depth, scoped(k, &l.text)),
                        LabelKind::Hierarchical => {
                            offer(PRIO_HIER, inst[k].depth, scoped(k, &l.text))
                        }
                    }
                }
                Node::SheetPin(k, si, pi) => {
                    let sh = &inst[k].sheet.sheets[si];
                    let child = format!("{}{}/", inst[k].path, sh.name);
                    offer(
                        PRIO_SHEET_PIN,
                        inst[k].depth + 1,
                        format!("{child}{}", sh.pins[pi].name),
                    );
                }
                Node::Member(k, mi) => {
                    offer(PRIO_LOCAL, inst[k].depth, scoped(k, &member_names[mi].1));
                }
            }
        }
        net.pins.sort_by(|a, b| {
            natural(&a.reference)
                .cmp(&natural(&b.reference))
                .then(natural(&a.number).cmp(&natural(&b.number)))
                .then(a.symbol.cmp(&b.symbol))
        });
        net.named = best.is_some();
        net.name = match best {
            Some((_, _, std::cmp::Reverse(n))) => n,
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
    out.nets = nets;
    out
}

/// Members of the bus group touching `p` on sheet `s`, from its bus labels.
fn bus_at(s: &Schematic, p: Pt) -> Option<Vec<String>> {
    let buses: Vec<(Pt, Pt)> = s
        .wires
        .iter()
        .filter(|w| w.bus)
        .map(|w| (w.a, w.b))
        .collect();
    let start = buses.iter().position(|(a, b)| on_segment(p, *a, *b))?;
    // Flood the connected bus segments from the one at `p`.
    let mut seen = vec![false; buses.len()];
    let mut stack = vec![start];
    while let Some(i) = stack.pop() {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        for (j, (c, d)) in buses.iter().enumerate() {
            let (a, b) = buses[i];
            if !seen[j]
                && (on_segment(*c, a, b)
                    || on_segment(*d, a, b)
                    || on_segment(a, *c, *d)
                    || on_segment(b, *c, *d))
            {
                stack.push(j);
            }
        }
    }
    let mut names: Vec<&str> = s
        .labels
        .iter()
        .filter(|l| {
            bus_members(&l.text).is_some()
                && l.kind != LabelKind::Hierarchical
                && buses
                    .iter()
                    .enumerate()
                    .any(|(i, (a, b))| seen[i] && on_segment(l.pos, *a, *b))
        })
        .map(|l| l.text.as_str())
        .collect();
    names.sort_unstable();
    names.first().and_then(|n| bus_members(n))
}

#[allow(clippy::too_many_arguments)]
fn member_node(
    nodes: &mut Vec<Node>,
    dsu: &mut Dsu,
    member_names: &mut Vec<(usize, String)>,
    member_index: &mut BTreeMap<(usize, String), usize>,
    by_name: &mut BTreeMap<String, usize>,
    k: usize,
    uid: u32,
    name: &str,
) -> usize {
    if let Some(n) = member_index.get(&(k, name.to_owned())) {
        return *n;
    }
    nodes.push(Node::Member(k, member_names.len()));
    let n = dsu.push();
    member_names.push((k, name.to_owned()));
    member_index.insert((k, name.to_owned()), n);
    let key = format!("l:{uid}:{name}");
    match by_name.get(&key) {
        Some(first) => dsu.union(*first, n),
        None => {
            by_name.insert(key, n);
        }
    }
    n
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
