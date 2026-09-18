//! Schematic document model and the editing operations the Schematic Editor performs.
//! Coordinates are page mils, Y down, origin at the sheet's top-left corner.
use crate::geom::{on_segment, Pt, Rect, Xf};
use crate::symbols::{self, LibPin, LibSymbol};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The schematic grid. Every connection point sits on it.
pub const GRID: i64 = 50;
/// A4 landscape, in mils.
pub const SHEET_W: i64 = 11_693;
pub const SHEET_H: i64 = 8_268;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub value: String,
    /// Position relative to the symbol's origin, page orientation.
    pub offset: Pt,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolInst {
    pub id: u64,
    pub lib_id: String,
    pub pos: Pt,
    pub xf: Xf,
    pub fields: Vec<Field>,
    #[serde(default)]
    pub exclude_from_sim: bool,
    #[serde(default = "yes")]
    pub in_bom: bool,
    #[serde(default = "yes")]
    pub on_board: bool,
    #[serde(default)]
    pub dnp: bool,
}
fn yes() -> bool {
    true
}
impl SymbolInst {
    pub fn lib(&self) -> Option<&'static LibSymbol> {
        symbols::find(&self.lib_id)
    }
    pub fn field(&self, name: &str) -> &str {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value.as_str())
            .unwrap_or("")
    }
    pub fn set_field(&mut self, name: &str, value: &str) {
        match self.fields.iter_mut().find(|f| f.name == name) {
            Some(f) => f.value = value.to_owned(),
            None => self.fields.push(Field {
                name: name.into(),
                value: value.into(),
                offset: Pt::default(),
                visible: false,
            }),
        }
    }
    pub fn reference(&self) -> &str {
        self.field("Reference")
    }
    pub fn value(&self) -> &str {
        self.field("Value")
    }
    pub fn is_power(&self) -> bool {
        self.lib().is_some_and(|l| l.power)
    }
    /// Map a library point (mils, Y up) to the page.
    pub fn to_page(&self, (x, y): (i64, i64)) -> Pt {
        self.pos.add(self.xf.apply(Pt::new(x, -y)))
    }
    /// Pins with their page connection points.
    pub fn pins(&self) -> Vec<(&'static LibPin, Pt)> {
        match self.lib() {
            Some(lib) => lib.pins.iter().map(|p| (p, self.to_page(p.at))).collect(),
            None => vec![],
        }
    }
    pub fn bounds(&self) -> Rect {
        let Some(lib) = self.lib() else {
            return Rect::around(self.pos, 50, 50);
        };
        let (lo, hi) = lib.bounds();
        Rect::new(self.to_page(lo), self.to_page(hi))
    }
    /// Text of the reference prefix ("R" for "R12", "#PWR" for "#PWR03").
    pub fn prefix(&self) -> String {
        let r = self.reference();
        r.trim_end_matches(|c: char| c.is_ascii_digit() || c == '?')
            .to_owned()
    }
    pub fn annotated(&self) -> bool {
        let r = self.reference();
        !r.ends_with('?') && r.chars().last().is_some_and(|c| c.is_ascii_digit())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wire {
    pub id: u64,
    pub a: Pt,
    pub b: Pt,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelKind {
    Local,
    Global,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub id: u64,
    pub pos: Pt,
    pub text: String,
    pub kind: LabelKind,
    /// Text direction in quarter turns CCW (0 = reads to the right).
    pub orient: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub id: u64,
    pub pos: Pt,
}
/// Free text on the sheet; a text starting with `.` is a SPICE directive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub id: u64,
    pub pos: Pt,
    pub text: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleBlock {
    pub title: String,
    pub date: String,
    pub rev: String,
    pub company: String,
}

/// Anything on the sheet a selection can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Item {
    Symbol(u64),
    Wire(u64),
    Junction(u64),
    Label(u64),
    NoConnect(u64),
    Text(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schematic {
    pub next_id: u64,
    pub title_block: TitleBlock,
    pub symbols: Vec<SymbolInst>,
    pub wires: Vec<Wire>,
    pub junctions: Vec<Marker>,
    pub labels: Vec<Label>,
    pub no_connects: Vec<Marker>,
    pub texts: Vec<Text>,
    /// Identity of the sheet in files (`(uuid …)`), fixed at creation.
    pub uuid: String,
}
impl Default for Schematic {
    fn default() -> Self {
        Self::new("")
    }
}

/// A deterministic UUID for item `id` within a document seeded by `seed`.
pub fn uuid(seed: &str, id: u64) -> String {
    // FNV-1a over the seed, so two projects do not share identifiers.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        (h >> 32) as u32,
        (h >> 16) as u16,
        (h & 0xfff) as u16,
        (id >> 48) as u16 & 0xfff,
        id & 0xffff_ffff_ffff
    )
}

/// The UUID a document writes for its item `id`: the document's own UUID with the item
/// number in the last group, so ids survive a save and load unchanged.
pub fn item_uuid(doc: &str, id: u64) -> String {
    let prefix = if doc.len() == 36 {
        &doc[..24]
    } else {
        "00000000-0000-4000-8000-"
    };
    format!("{prefix}{:012x}", id & 0xffff_ffff_ffff)
}
/// The item number in a UUID written by `item_uuid` for document `doc`.
pub fn item_id(doc: &str, u: &str) -> Option<u64> {
    if u.len() == 36 && doc.len() == 36 && u[..24] == doc[..24] {
        u64::from_str_radix(&u[24..], 16).ok()
    } else {
        None
    }
}

impl Schematic {
    pub fn new(seed: &str) -> Self {
        Self {
            next_id: 1,
            title_block: TitleBlock::default(),
            symbols: vec![],
            wires: vec![],
            junctions: vec![],
            labels: vec![],
            no_connects: vec![],
            texts: vec![],
            uuid: uuid(seed, 0),
        }
    }
    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn symbol(&self, id: u64) -> Option<&SymbolInst> {
        self.symbols.iter().find(|s| s.id == id)
    }
    pub fn symbol_mut(&mut self, id: u64) -> Option<&mut SymbolInst> {
        self.symbols.iter_mut().find(|s| s.id == id)
    }
    pub fn by_reference(&self, reference: &str) -> Option<&SymbolInst> {
        self.symbols.iter().find(|s| s.reference() == reference)
    }

    /// Place a library symbol with its default fields; unannotated ("R?").
    pub fn place(&mut self, lib_id: &str, pos: Pt, xf: Xf) -> Result<u64, String> {
        let lib = symbols::find(lib_id).ok_or_else(|| format!("{lib_id} is not in the library"))?;
        let id = self.take_id();
        let flip = |(x, y): (i64, i64)| Pt::new(x, -y);
        let mut fields = vec![
            Field {
                name: "Reference".into(),
                value: format!("{}?", lib.reference),
                offset: flip(lib.ref_at),
                visible: !lib.power,
            },
            Field {
                name: "Value".into(),
                value: lib.value.into(),
                offset: flip(lib.value_at),
                visible: true,
            },
            Field {
                name: "Footprint".into(),
                value: lib.footprint.into(),
                offset: Pt::default(),
                visible: false,
            },
            Field {
                name: "Datasheet".into(),
                value: lib.datasheet.into(),
                offset: Pt::default(),
                visible: false,
            },
        ];
        if !lib.sim_params.is_empty() {
            fields.push(Field {
                name: "Sim.Params".into(),
                value: lib.sim_params.into(),
                offset: Pt::default(),
                visible: false,
            });
        }
        let mut s = SymbolInst {
            id,
            lib_id: lib_id.into(),
            pos: pos.snap(GRID),
            xf,
            fields,
            exclude_from_sim: lib.exclude_from_sim,
            in_bom: lib.in_bom,
            on_board: lib.on_board,
            dnp: false,
        };
        // Fields keep their library placement relative to an unrotated body; a rotated
        // symbol carries its fields round with it so they stay beside the body.
        for f in &mut s.fields {
            f.offset = xf.apply(f.offset);
        }
        self.symbols.push(s);
        Ok(id)
    }
    pub fn add_wire(&mut self, a: Pt, b: Pt) -> Option<u64> {
        if a == b {
            return None;
        }
        let id = self.take_id();
        self.wires.push(Wire { id, a, b });
        Some(id)
    }
    pub fn add_label(&mut self, pos: Pt, text: &str, kind: LabelKind) -> u64 {
        let id = self.take_id();
        self.labels.push(Label {
            id,
            pos,
            text: text.into(),
            kind,
            orient: 0,
        });
        id
    }
    pub fn add_junction(&mut self, pos: Pt) -> u64 {
        if let Some(j) = self.junctions.iter().find(|j| j.pos == pos) {
            return j.id;
        }
        let id = self.take_id();
        self.junctions.push(Marker { id, pos });
        id
    }
    pub fn add_no_connect(&mut self, pos: Pt) -> u64 {
        if let Some(j) = self.no_connects.iter().find(|j| j.pos == pos) {
            return j.id;
        }
        let id = self.take_id();
        self.no_connects.push(Marker { id, pos });
        id
    }
    pub fn add_text(&mut self, pos: Pt, text: &str) -> u64 {
        let id = self.take_id();
        self.texts.push(Text {
            id,
            pos,
            text: text.into(),
        });
        id
    }
    pub fn exists(&self, item: Item) -> bool {
        match item {
            Item::Symbol(id) => self.symbols.iter().any(|s| s.id == id),
            Item::Wire(id) => self.wires.iter().any(|s| s.id == id),
            Item::Junction(id) => self.junctions.iter().any(|s| s.id == id),
            Item::Label(id) => self.labels.iter().any(|s| s.id == id),
            Item::NoConnect(id) => self.no_connects.iter().any(|s| s.id == id),
            Item::Text(id) => self.texts.iter().any(|s| s.id == id),
        }
    }
    pub fn delete(&mut self, items: &[Item]) -> usize {
        let before = self.count();
        let set: BTreeSet<Item> = items.iter().copied().collect();
        self.symbols.retain(|s| !set.contains(&Item::Symbol(s.id)));
        self.wires.retain(|s| !set.contains(&Item::Wire(s.id)));
        self.junctions
            .retain(|s| !set.contains(&Item::Junction(s.id)));
        self.labels.retain(|s| !set.contains(&Item::Label(s.id)));
        self.no_connects
            .retain(|s| !set.contains(&Item::NoConnect(s.id)));
        self.texts.retain(|s| !set.contains(&Item::Text(s.id)));
        self.cleanup_junctions();
        before - self.count()
    }
    fn count(&self) -> usize {
        self.symbols.len()
            + self.wires.len()
            + self.junctions.len()
            + self.labels.len()
            + self.no_connects.len()
            + self.texts.len()
    }
    /// Move items by `d`. Wires attached to a moved symbol's pins stretch to follow
    /// ("drag" behaviour) when `drag` is set.
    pub fn move_items(&mut self, items: &[Item], d: Pt, drag: bool) {
        let set: BTreeSet<Item> = items.iter().copied().collect();
        let mut moved_points: BTreeSet<Pt> = BTreeSet::new();
        if drag {
            for s in &self.symbols {
                if set.contains(&Item::Symbol(s.id)) {
                    moved_points.extend(s.pins().into_iter().map(|(_, p)| p));
                }
            }
        }
        for s in &mut self.symbols {
            if set.contains(&Item::Symbol(s.id)) {
                s.pos = s.pos.add(d);
            }
        }
        for w in &mut self.wires {
            if set.contains(&Item::Wire(w.id)) {
                w.a = w.a.add(d);
                w.b = w.b.add(d);
            } else if drag {
                if moved_points.contains(&w.a) {
                    w.a = w.a.add(d);
                }
                if moved_points.contains(&w.b) {
                    w.b = w.b.add(d);
                }
            }
        }
        self.wires.retain(|w| w.a != w.b);
        for j in &mut self.junctions {
            if set.contains(&Item::Junction(j.id)) || (drag && moved_points.contains(&j.pos)) {
                j.pos = j.pos.add(d);
            }
        }
        for l in &mut self.labels {
            if set.contains(&Item::Label(l.id)) || (drag && moved_points.contains(&l.pos)) {
                l.pos = l.pos.add(d);
            }
        }
        for n in &mut self.no_connects {
            if set.contains(&Item::NoConnect(n.id)) || (drag && moved_points.contains(&n.pos)) {
                n.pos = n.pos.add(d);
            }
        }
        for t in &mut self.texts {
            if set.contains(&Item::Text(t.id)) {
                t.pos = t.pos.add(d);
            }
        }
        self.cleanup_junctions();
    }
    /// Apply an orientation change to symbols (about their own origin) and labels.
    pub fn transform(&mut self, items: &[Item], op: Xf) {
        let set: BTreeSet<Item> = items.iter().copied().collect();
        for s in &mut self.symbols {
            if set.contains(&Item::Symbol(s.id)) {
                s.xf = s.xf.then(op);
                for f in &mut s.fields {
                    f.offset = op.apply(f.offset);
                }
            }
        }
        for l in &mut self.labels {
            if set.contains(&Item::Label(l.id)) {
                if op == Xf::ROT_CCW {
                    l.orient = (l.orient + 1) % 4;
                } else if (op == Xf::MIRROR_Y && l.orient % 2 == 0)
                    || (op == Xf::MIRROR_X && l.orient % 2 == 1)
                {
                    // A mirror across the text's own direction turns it round.
                    l.orient = (l.orient + 2) % 4;
                }
            }
        }
    }
    /// Every point where something connects: pin ends, wire ends, labels.
    pub fn connection_points(&self) -> Vec<Pt> {
        let mut pts = Vec::new();
        for s in &self.symbols {
            pts.extend(s.pins().into_iter().map(|(_, p)| p));
        }
        for w in &self.wires {
            pts.push(w.a);
            pts.push(w.b);
        }
        pts
    }
    /// KiCad places a junction wherever three or more connections meet, or a wire end
    /// lands part-way along another wire; this adds those and removes stale ones.
    pub fn cleanup_junctions(&mut self) {
        let needed = self.junction_points();
        self.junctions.retain(|j| needed.contains(&j.pos));
        for p in needed {
            if !self.junctions.iter().any(|j| j.pos == p) {
                let id = self.take_id();
                self.junctions.push(Marker { id, pos: p });
            }
        }
        self.junctions.sort_by_key(|j| (j.pos, j.id));
    }
    pub fn junction_points(&self) -> BTreeSet<Pt> {
        let mut count: BTreeMap<Pt, usize> = BTreeMap::new();
        for s in &self.symbols {
            for (_, p) in s.pins() {
                *count.entry(p).or_default() += 1;
            }
        }
        for w in &self.wires {
            *count.entry(w.a).or_default() += 1;
            *count.entry(w.b).or_default() += 1;
        }
        // A point in the middle of a wire counts that wire twice (it continues both ways).
        let points: Vec<Pt> = count.keys().copied().collect();
        for p in points {
            for w in &self.wires {
                if p != w.a && p != w.b && on_segment(p, w.a, w.b) {
                    *count.entry(p).or_default() += 2;
                }
            }
        }
        count
            .into_iter()
            .filter(|(_, n)| *n >= 3)
            .map(|(p, _)| p)
            .collect()
    }
    /// Items whose geometry is within `tol` of `p`, topmost (last drawn) first.
    pub fn hit(&self, p: Pt, tol: i64) -> Vec<Item> {
        let mut out = Vec::new();
        for j in self.junctions.iter().rev() {
            if j.pos.manhattan(p) <= tol {
                out.push(Item::Junction(j.id));
            }
        }
        for n in self.no_connects.iter().rev() {
            if n.pos.manhattan(p) <= tol * 2 {
                out.push(Item::NoConnect(n.id));
            }
        }
        for l in self.labels.iter().rev() {
            let w = 60 * l.text.chars().count() as i64 + 100;
            let r = match l.orient {
                0 => Rect::new(l.pos.add(Pt::new(0, -80)), l.pos.add(Pt::new(w, 20))),
                2 => Rect::new(l.pos.add(Pt::new(-w, -80)), l.pos.add(Pt::new(0, 20))),
                1 => Rect::new(l.pos.add(Pt::new(-80, -w)), l.pos.add(Pt::new(20, 0))),
                _ => Rect::new(l.pos.add(Pt::new(-80, 0)), l.pos.add(Pt::new(20, w))),
            };
            if r.inflate(tol).contains(p) {
                out.push(Item::Label(l.id));
            }
        }
        for t in self.texts.iter().rev() {
            let w = 60 * t.text.chars().count() as i64;
            if Rect::new(t.pos.add(Pt::new(0, -80)), t.pos.add(Pt::new(w, 20)))
                .inflate(tol)
                .contains(p)
            {
                out.push(Item::Text(t.id));
            }
        }
        for w in self.wires.iter().rev() {
            if crate::geom::point_segment(p, w.a, w.b) <= tol as f64 {
                out.push(Item::Wire(w.id));
            }
        }
        for s in self.symbols.iter().rev() {
            if s.bounds().inflate(tol).contains(p) {
                out.push(Item::Symbol(s.id));
            }
        }
        out
    }
    /// Items entirely inside `r` (a selection rectangle).
    pub fn inside(&self, r: &Rect) -> Vec<Item> {
        let mut out = Vec::new();
        for s in &self.symbols {
            let b = s.bounds();
            if r.contains(b.min) && r.contains(b.max) {
                out.push(Item::Symbol(s.id));
            }
        }
        for w in &self.wires {
            if r.contains(w.a) && r.contains(w.b) {
                out.push(Item::Wire(w.id));
            }
        }
        for j in &self.junctions {
            if r.contains(j.pos) {
                out.push(Item::Junction(j.id));
            }
        }
        for l in &self.labels {
            if r.contains(l.pos) {
                out.push(Item::Label(l.id));
            }
        }
        for n in &self.no_connects {
            if r.contains(n.pos) {
                out.push(Item::NoConnect(n.id));
            }
        }
        for t in &self.texts {
            if r.contains(t.pos) {
                out.push(Item::Text(t.id));
            }
        }
        out
    }
    /// Number every unannotated symbol ("R?") with the lowest free number for its
    /// prefix, in reading order. `reset` renumbers everything first. Returns what changed
    /// as (old, new) pairs.
    pub fn annotate(&mut self, by_x: bool, reset: bool) -> Vec<(String, String)> {
        let mut changes = Vec::new();
        if reset {
            for s in &mut self.symbols {
                let prefix = s.prefix();
                let old = s.reference().to_owned();
                s.set_field("Reference", &format!("{prefix}?"));
                changes.push((old, String::new()));
            }
            changes.clear();
        }
        let mut used: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        for s in &self.symbols {
            if s.annotated() {
                let prefix = s.prefix();
                if let Ok(n) = s.reference()[prefix.len()..].parse::<u32>() {
                    used.entry(prefix).or_default().insert(n);
                }
            }
        }
        let mut order: Vec<usize> = (0..self.symbols.len())
            .filter(|i| !self.symbols[*i].annotated())
            .collect();
        order.sort_by_key(|i| {
            let p = self.symbols[*i].pos;
            if by_x {
                (p.x, p.y, self.symbols[*i].id as i64)
            } else {
                (p.y, p.x, self.symbols[*i].id as i64)
            }
        });
        for i in order {
            let prefix = self.symbols[i].prefix();
            let taken = used.entry(prefix.clone()).or_default();
            let mut n = 1;
            while taken.contains(&n) {
                n += 1;
            }
            taken.insert(n);
            // Power symbols number with two digits, as KiCad writes #PWR01.
            let new = if prefix.starts_with('#') {
                format!("{prefix}{n:02}")
            } else {
                format!("{prefix}{n}")
            };
            let old = self.symbols[i].reference().to_owned();
            self.symbols[i].set_field("Reference", &new);
            changes.push((old, new));
        }
        changes
    }
    /// Clear every reference back to "R?".
    pub fn clear_annotation(&mut self) {
        for s in &mut self.symbols {
            let prefix = s.prefix();
            s.set_field("Reference", &format!("{prefix}?"));
        }
    }
    /// The SPICE analysis directive on the sheet (`.tran …`), if any.
    pub fn sim_command(&self) -> Option<&Text> {
        self.texts.iter().find(|t| {
            let l = t.text.trim_start().to_ascii_lowercase();
            [".tran", ".ac", ".dc", ".op"]
                .iter()
                .any(|k| l.starts_with(k))
        })
    }
    pub fn set_sim_command(&mut self, command: &str) {
        let at = Pt::new(1000, SHEET_H - 1200);
        match self.texts.iter_mut().find(|t| {
            let l = t.text.trim_start().to_ascii_lowercase();
            [".tran", ".ac", ".dc", ".op"]
                .iter()
                .any(|k| l.starts_with(k))
        }) {
            Some(t) => t.text = command.into(),
            None => {
                self.add_text(at, command);
            }
        }
    }
}

/// Undo/redo history of whole-document snapshots, bounded so a long session cannot
/// grow a snapshot without limit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History<T> {
    pub past: Vec<T>,
    pub future: Vec<T>,
}
pub const HISTORY_LIMIT: usize = 24;
impl<T: Clone> History<T> {
    pub fn record(&mut self, before: &T) {
        self.past.push(before.clone());
        if self.past.len() > HISTORY_LIMIT {
            self.past.remove(0);
        }
        self.future.clear();
    }
    pub fn undo(&mut self, current: &mut T) -> bool {
        match self.past.pop() {
            Some(prev) => {
                self.future.push(std::mem::replace(current, prev));
                true
            }
            None => false,
        }
    }
    pub fn redo(&mut self, current: &mut T) -> bool {
        match self.future.pop() {
            Some(next) => {
                self.past.push(std::mem::replace(current, next));
                true
            }
            None => false,
        }
    }
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }
}

/// The two-segment orthogonal path KiCad's 90° wire mode draws from `a` to `b`:
/// horizontal first unless `vertical_first`.
pub fn ortho(a: Pt, b: Pt, vertical_first: bool) -> Vec<(Pt, Pt)> {
    let corner = if vertical_first {
        Pt::new(a.x, b.y)
    } else {
        Pt::new(b.x, a.y)
    };
    [(a, corner), (corner, b)]
        .into_iter()
        .filter(|(p, q)| p != q)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placing_rotating_and_annotating() {
        let mut s = Schematic::new("t");
        let r = s
            .place("Device:R", Pt::new(1000, 1000), Xf::IDENTITY)
            .unwrap();
        let pins = s.symbol(r).unwrap().pins();
        assert_eq!(pins[0].1, Pt::new(1000, 850), "pin 1 at the top");
        assert_eq!(pins[1].1, Pt::new(1000, 1150));
        s.transform(&[Item::Symbol(r)], Xf::ROT_CCW);
        let pins = s.symbol(r).unwrap().pins();
        // Rotated CCW, pin 1 (was top) points left.
        assert_eq!(pins[0].1, Pt::new(850, 1000));
        s.place("Device:R", Pt::new(500, 1000), Xf::IDENTITY)
            .unwrap();
        s.place("Device:C", Pt::new(700, 1000), Xf::IDENTITY)
            .unwrap();
        let changes = s.annotate(true, false);
        assert_eq!(changes.len(), 3);
        let refs: Vec<&str> = s.symbols.iter().map(|x| x.reference()).collect();
        assert_eq!(refs, vec!["R2", "R1", "C1"]);
    }
    #[test]
    fn t_connections_get_junctions_and_stale_ones_go() {
        let mut s = Schematic::new("t");
        s.add_wire(Pt::new(0, 0), Pt::new(1000, 0));
        s.add_wire(Pt::new(500, 0), Pt::new(500, 500));
        s.cleanup_junctions();
        assert_eq!(s.junctions.len(), 1);
        assert_eq!(s.junctions[0].pos, Pt::new(500, 0));
        let w = s.wires[1].id;
        s.delete(&[Item::Wire(w)]);
        assert!(s.junctions.is_empty());
    }
    #[test]
    fn dragging_a_symbol_stretches_its_wires() {
        let mut s = Schematic::new("t");
        let r = s
            .place("Device:R", Pt::new(1000, 1000), Xf::IDENTITY)
            .unwrap();
        s.add_wire(Pt::new(1000, 850), Pt::new(1000, 500));
        s.move_items(&[Item::Symbol(r)], Pt::new(200, 0), true);
        assert_eq!(s.wires[0].a, Pt::new(1200, 850));
        assert_eq!(s.wires[0].b, Pt::new(1000, 500));
    }
    #[test]
    fn history_undoes_and_redoes() {
        let mut h = History::default();
        let mut doc = 1;
        h.record(&doc);
        doc = 2;
        assert!(h.undo(&mut doc));
        assert_eq!(doc, 1);
        assert!(h.redo(&mut doc));
        assert_eq!(doc, 2);
        assert!(!h.redo(&mut doc));
    }
}
