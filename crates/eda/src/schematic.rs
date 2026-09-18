//! Schematic document model and the editing operations the Schematic Editor performs.
//! Coordinates are page mils, Y down, origin at the sheet's top-left corner.
//!
//! A schematic is a tree of sheets, as KiCad's hierarchy is: the root sheet holds sheet
//! symbols, each of which carries the sheet it stands for (saved to its own
//! `.kicad_sch` file). Sheet pins on a sheet symbol meet hierarchical labels of the same
//! name inside it. Wires may be buses, joined to single wires by bus entries, and
//! labels may name vector buses (`D[0..7]`) or bus groups (`{SDA SCL}`).
use crate::geom::{on_segment, Pt, Rect, Xf};
use crate::symbols::{self, LibPin, LibSymbol, PinType};
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
    /// Which unit of a multi-unit part this is, 1-based.
    #[serde(default = "one")]
    pub unit: u32,
    /// The library definition, carried with the instance when it came from a project
    /// library (KiCad keeps every used definition in the schematic's `lib_symbols`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<Box<LibSymbol>>,
}
fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}
impl SymbolInst {
    pub fn lib(&self) -> Option<&LibSymbol> {
        match &self.local {
            Some(l) => Some(l),
            None => symbols::find(&self.lib_id),
        }
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
    /// Pins of this unit with their page connection points.
    pub fn pins(&self) -> Vec<(&LibPin, Pt)> {
        match self.lib() {
            Some(lib) => lib
                .unit_pins(self.unit)
                .map(|p| (p, self.to_page(p.at)))
                .collect(),
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
    /// The reference as a multi-unit part shows it: "U1A", "U1B".
    pub fn unit_reference(&self) -> String {
        match self.lib() {
            Some(l) if l.units > 1 => {
                format!("{}{}", self.reference(), LibSymbol::unit_letter(self.unit))
            }
            _ => self.reference().to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wire {
    pub id: u64,
    pub a: Pt,
    pub b: Pt,
    /// A bus rather than a single wire.
    #[serde(default)]
    pub bus: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelKind {
    Local,
    Global,
    /// A hierarchical label: it meets the sheet pin of the same name on the sheet
    /// symbol that holds this sheet.
    Hierarchical,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub id: u64,
    pub pos: Pt,
    pub text: String,
    pub kind: LabelKind,
    /// Text direction in quarter turns CCW (0 = reads to the right).
    pub orient: u8,
    /// Electrical shape of a global or hierarchical label.
    #[serde(default = "passive")]
    pub shape: PinType,
}
fn passive() -> PinType {
    PinType::Passive
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
/// A bus entry: the 45° stub from a point on a bus (`pos`) to where a wire starts
/// (`pos + size`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusEntry {
    pub id: u64,
    pub pos: Pt,
    pub size: Pt,
}
impl BusEntry {
    pub fn end(&self) -> Pt {
        self.pos.add(self.size)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleBlock {
    pub title: String,
    pub date: String,
    pub rev: String,
    pub company: String,
}

/// A pin on a sheet symbol's edge, meeting the hierarchical label of the same name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetPin {
    pub name: String,
    pub shape: PinType,
    /// Connection point, on the sheet symbol's outline.
    pub pos: Pt,
}

/// A sheet symbol: a box on its parent sheet standing for a child sheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetSymbol {
    pub id: u64,
    /// Identity of this sheet instance across the whole design (the root is 0), so
    /// symbols in different sheets never share an identity.
    pub uid: u32,
    /// Top-left corner and size.
    pub pos: Pt,
    pub size: Pt,
    pub name: String,
    /// File the child sheet is saved to, relative to the project.
    pub file: String,
    pub pins: Vec<SheetPin>,
    pub contents: Box<Schematic>,
}
impl SheetSymbol {
    pub fn rect(&self) -> Rect {
        Rect::new(self.pos, self.pos.add(self.size))
    }
    /// The side of the outline a point is on: 0 left, 1 right, 2 top, 3 bottom.
    pub fn side_of(&self, p: Pt) -> u8 {
        let r = self.rect();
        if p.x <= r.min.x {
            0
        } else if p.x >= r.max.x {
            1
        } else if p.y <= r.min.y {
            2
        } else {
            3
        }
    }
    /// The nearest point on the outline to `p`, on the grid.
    pub fn edge_point(&self, p: Pt) -> Pt {
        let r = self.rect();
        let p = p.snap(GRID);
        let cx = p.x.clamp(r.min.x, r.max.x);
        let cy = p.y.clamp(r.min.y, r.max.y);
        let d = [
            (cx - r.min.x).abs(),
            (r.max.x - cx).abs(),
            (cy - r.min.y).abs(),
            (r.max.y - cy).abs(),
        ];
        let side = (0..4).min_by_key(|i| d[*i]).unwrap_or(0);
        match side {
            0 => Pt::new(r.min.x, cy),
            1 => Pt::new(r.max.x, cy),
            2 => Pt::new(cx, r.min.y),
            _ => Pt::new(cx, r.max.y),
        }
    }
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
    Sheet(u64),
    BusEntry(u64),
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
    #[serde(default)]
    pub bus_entries: Vec<BusEntry>,
    #[serde(default)]
    pub sheets: Vec<SheetSymbol>,
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

/// The design-wide identity of item `id` on the sheet instance `uid`. The root's
/// items keep their own ids.
pub fn global_id(uid: u32, id: u64) -> u64 {
    ((uid as u64) << 32) | (id & 0xffff_ffff)
}

/// A sub-sheet's instance identity, from the UUID its file keeps (never 0, the root's).
pub fn sheet_uid(sheet_uuid: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in sheet_uuid.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    (h & 0x0fff_ffff).max(1)
}

/// One sheet of a design as the hierarchy reaches it.
#[derive(Clone, Copy, Debug)]
pub struct SheetRef<'a> {
    /// KiCad's sheet path: "/" for the root, "/amp/" for sheet "amp" under it.
    pub path: &'a str,
    pub uid: u32,
    pub sheet: &'a Schematic,
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
            bus_entries: vec![],
            sheets: vec![],
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
    pub fn sheet(&self, id: u64) -> Option<&SheetSymbol> {
        self.sheets.iter().find(|s| s.id == id)
    }
    pub fn sheet_mut(&mut self, id: u64) -> Option<&mut SheetSymbol> {
        self.sheets.iter_mut().find(|s| s.id == id)
    }
    /// The first symbol anywhere in the hierarchy with this reference.
    pub fn by_reference(&self, reference: &str) -> Option<&SymbolInst> {
        self.all_symbols()
            .into_iter()
            .map(|(_, _, s)| s)
            .find(|s| s.reference() == reference)
    }

    // ---- hierarchy ---------------------------------------------------------------------
    /// The sheet reached by following sheet-symbol ids from this one.
    pub fn at_path(&self, path: &[u64]) -> Option<&Schematic> {
        let mut s = self;
        for id in path {
            s = &s.sheet(*id)?.contents;
        }
        Some(s)
    }
    pub fn at_path_mut(&mut self, path: &[u64]) -> Option<&mut Schematic> {
        let mut s = self;
        for id in path {
            s = &mut s.sheet_mut(*id)?.contents;
        }
        Some(s)
    }
    /// KiCad's path string for a sheet-symbol id path ("/", "/amp/", "/amp/filter/").
    pub fn path_name(&self, path: &[u64]) -> String {
        let mut out = String::from("/");
        let mut s = self;
        for id in path {
            let Some(sheet) = s.sheet(*id) else { break };
            out.push_str(&sheet.name);
            out.push('/');
            s = &sheet.contents;
        }
        out
    }
    /// The sheet instance identity at an id path.
    pub fn uid_at(&self, path: &[u64]) -> u32 {
        let mut s = self;
        let mut uid = 0;
        for id in path {
            let Some(sheet) = s.sheet(*id) else { break };
            uid = sheet.uid;
            s = &sheet.contents;
        }
        uid
    }
    /// Every sheet, root first, in the depth-first order KiCad numbers pages in, with
    /// its path names.
    pub fn sheets_flat(&self) -> Vec<(String, Vec<u64>, u32, &Schematic)> {
        fn walk<'a>(
            s: &'a Schematic,
            name: String,
            ids: Vec<u64>,
            uid: u32,
            out: &mut Vec<(String, Vec<u64>, u32, &'a Schematic)>,
        ) {
            out.push((name.clone(), ids.clone(), uid, s));
            for sh in &s.sheets {
                let mut next = ids.clone();
                next.push(sh.id);
                walk(
                    &sh.contents,
                    format!("{name}{}/", sh.name),
                    next,
                    sh.uid,
                    out,
                );
            }
        }
        let mut out = vec![];
        walk(self, "/".into(), vec![], 0, &mut out);
        out
    }
    /// Every symbol in the design with its global identity and sheet path.
    pub fn all_symbols(&self) -> Vec<(u64, String, &SymbolInst)> {
        let mut out = vec![];
        for (path, _, uid, s) in self.sheets_flat() {
            for sym in &s.symbols {
                out.push((global_id(uid, sym.id), path.clone(), sym));
            }
        }
        out
    }
    /// A symbol by its global identity.
    pub fn symbol_global(&self, gid: u64) -> Option<&SymbolInst> {
        let uid = (gid >> 32) as u32;
        let id = gid & 0xffff_ffff;
        self.sheets_flat()
            .into_iter()
            .find(|(_, _, u, _)| *u == uid)
            .and_then(|(_, _, _, s)| s.symbol(id))
    }
    fn for_each_sheet_mut(&mut self, f: &mut dyn FnMut(&mut Schematic)) {
        f(self);
        for sh in &mut self.sheets {
            sh.contents.for_each_sheet_mut(f);
        }
    }
    /// Add a sheet symbol for a new, empty child sheet. The caller checks the file is
    /// not used elsewhere in the design (`sheet_files` on the root).
    pub fn add_sheet(&mut self, pos: Pt, size: Pt, name: &str, file: &str) -> Result<u64, String> {
        if name.trim().is_empty() {
            return Err("a sheet needs a name".into());
        }
        if !file.ends_with(".kicad_sch") {
            return Err("the sheet file must end in .kicad_sch".into());
        }
        if self.sheets.iter().any(|s| s.name == name) {
            return Err(format!("a sheet named '{name}' is already on this sheet"));
        }
        let id = self.take_id();
        let contents = Schematic::new(&format!("{}{file}", self.uuid));
        self.sheets.push(SheetSymbol {
            id,
            uid: sheet_uid(&contents.uuid),
            pos: pos.snap(GRID),
            size: Pt::new(size.x.max(500), size.y.max(300)).snap(GRID),
            name: name.into(),
            file: file.into(),
            pins: vec![],
            contents: Box::new(contents),
        });
        Ok(id)
    }
    /// Put `contents` (read from `file`) into every sheet symbol that names that file,
    /// as loading a hierarchy does. Returns how many took it.
    pub fn attach_sheet(&mut self, file: &str, contents: &Schematic) -> usize {
        let mut n = 0;
        self.for_each_sheet_mut(&mut |s| {
            for sh in &mut s.sheets {
                if sh.file == file {
                    *sh.contents = contents.clone();
                    sh.uid = sheet_uid(&contents.uuid);
                    n += 1;
                }
            }
        });
        n
    }
    /// Files every sheet in the design is saved to (not the root's).
    pub fn sheet_files(&self) -> Vec<String> {
        let mut out = vec![];
        for (_, _, _, s) in self.sheets_flat() {
            for sh in &s.sheets {
                out.push(sh.file.clone());
            }
        }
        out
    }

    /// Place a library symbol with its default fields; unannotated ("R?").
    pub fn place(&mut self, lib_id: &str, pos: Pt, xf: Xf) -> Result<u64, String> {
        let lib = symbols::find(lib_id).ok_or_else(|| format!("{lib_id} is not in the library"))?;
        Ok(self.place_symbol(lib, false, pos, xf))
    }
    /// Place `lib` (unit 1). A definition from a project library is embedded in the
    /// instance (`local`) so the design carries it, as KiCad's `lib_symbols` do.
    pub fn place_symbol(&mut self, lib: &LibSymbol, local: bool, pos: Pt, xf: Xf) -> u64 {
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
                value: lib.value.clone(),
                offset: flip(lib.value_at),
                visible: true,
            },
            Field {
                name: "Footprint".into(),
                value: lib.footprint.clone(),
                offset: Pt::default(),
                visible: false,
            },
            Field {
                name: "Datasheet".into(),
                value: lib.datasheet.clone(),
                offset: Pt::default(),
                visible: false,
            },
        ];
        if !lib.sim_params.is_empty() {
            fields.push(Field {
                name: "Sim.Params".into(),
                value: lib.sim_params.clone(),
                offset: Pt::default(),
                visible: false,
            });
        }
        for (k, v) in &lib.fields {
            fields.push(Field {
                name: k.clone(),
                value: v.clone(),
                offset: Pt::default(),
                visible: false,
            });
        }
        let mut s = SymbolInst {
            id,
            lib_id: lib.lib_id.clone(),
            pos: pos.snap(GRID),
            xf,
            fields,
            exclude_from_sim: lib.exclude_from_sim,
            in_bom: lib.in_bom,
            on_board: lib.on_board,
            dnp: false,
            unit: 1,
            local: if local {
                Some(Box::new(lib.clone()))
            } else {
                None
            },
        };
        // Fields keep their library placement relative to an unrotated body; a rotated
        // symbol carries its fields round with it so they stay beside the body.
        for f in &mut s.fields {
            f.offset = xf.apply(f.offset);
        }
        self.symbols.push(s);
        id
    }
    pub fn add_wire(&mut self, a: Pt, b: Pt) -> Option<u64> {
        self.add_segment(a, b, false)
    }
    pub fn add_bus(&mut self, a: Pt, b: Pt) -> Option<u64> {
        self.add_segment(a, b, true)
    }
    fn add_segment(&mut self, a: Pt, b: Pt, bus: bool) -> Option<u64> {
        if a == b {
            return None;
        }
        let id = self.take_id();
        self.wires.push(Wire { id, a, b, bus });
        Some(id)
    }
    pub fn add_bus_entry(&mut self, pos: Pt, size: Pt) -> u64 {
        let id = self.take_id();
        self.bus_entries.push(BusEntry { id, pos, size });
        id
    }
    pub fn add_label(&mut self, pos: Pt, text: &str, kind: LabelKind) -> u64 {
        let id = self.take_id();
        self.labels.push(Label {
            id,
            pos,
            text: text.into(),
            kind,
            orient: 0,
            shape: if kind == LabelKind::Local {
                PinType::Passive
            } else {
                PinType::Bidirectional
            },
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
            Item::Sheet(id) => self.sheets.iter().any(|s| s.id == id),
            Item::BusEntry(id) => self.bus_entries.iter().any(|s| s.id == id),
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
        self.sheets.retain(|s| !set.contains(&Item::Sheet(s.id)));
        self.bus_entries
            .retain(|s| !set.contains(&Item::BusEntry(s.id)));
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
            + self.sheets.len()
            + self.bus_entries.len()
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
            for s in &self.sheets {
                if set.contains(&Item::Sheet(s.id)) {
                    moved_points.extend(s.pins.iter().map(|p| p.pos));
                }
            }
        }
        for s in &mut self.symbols {
            if set.contains(&Item::Symbol(s.id)) {
                s.pos = s.pos.add(d);
            }
        }
        for s in &mut self.sheets {
            if set.contains(&Item::Sheet(s.id)) {
                s.pos = s.pos.add(d);
                for p in &mut s.pins {
                    p.pos = p.pos.add(d);
                }
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
        for e in &mut self.bus_entries {
            if set.contains(&Item::BusEntry(e.id)) {
                e.pos = e.pos.add(d);
            }
        }
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
        for e in &mut self.bus_entries {
            if set.contains(&Item::BusEntry(e.id)) {
                e.size = op.apply(e.size);
            }
        }
    }
    /// Every point where something connects: pin ends, wire ends, sheet pins.
    pub fn connection_points(&self) -> Vec<Pt> {
        let mut pts = Vec::new();
        for s in &self.symbols {
            pts.extend(s.pins().into_iter().map(|(_, p)| p));
        }
        for w in &self.wires {
            pts.push(w.a);
            pts.push(w.b);
        }
        for s in &self.sheets {
            pts.extend(s.pins.iter().map(|p| p.pos));
        }
        for e in &self.bus_entries {
            pts.push(e.pos);
            pts.push(e.end());
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
        for s in &self.sheets {
            for p in &s.pins {
                *count.entry(p.pos).or_default() += 1;
            }
        }
        for w in &self.wires {
            *count.entry(w.a).or_default() += 1;
            *count.entry(w.b).or_default() += 1;
        }
        // A point in the middle of a wire counts that wire twice (it continues both ways).
        // A bus entry's bus end sitting on a bus does not make a junction.
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
        for e in self.bus_entries.iter().rev() {
            if crate::geom::point_segment(p, e.pos, e.end()) <= tol as f64 {
                out.push(Item::BusEntry(e.id));
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
        for s in self.sheets.iter().rev() {
            if s.rect().inflate(tol).contains(p) {
                out.push(Item::Sheet(s.id));
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
        for s in &self.sheets {
            let b = s.rect();
            if r.contains(b.min) && r.contains(b.max) {
                out.push(Item::Sheet(s.id));
            }
        }
        for e in &self.bus_entries {
            if r.contains(e.pos) && r.contains(e.end()) {
                out.push(Item::BusEntry(e.id));
            }
        }
        out
    }
    /// Number every unannotated symbol in the design ("R?") with the lowest free number
    /// for its prefix, sheet by sheet in page order and in reading order on each sheet.
    /// `reset` renumbers everything first. The units of one multi-unit part placed
    /// together are left as they are. Returns what changed as (old, new) pairs.
    pub fn annotate(&mut self, by_x: bool, reset: bool) -> Vec<(String, String)> {
        let mut changes = Vec::new();
        if reset {
            self.for_each_sheet_mut(&mut |s| {
                for sym in &mut s.symbols {
                    let prefix = sym.prefix();
                    sym.set_field("Reference", &format!("{prefix}?"));
                }
            });
        }
        let mut used: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        // Parts with free units: (lib_id, number) → units taken, so the units of a
        // multi-unit part share one reference.
        let mut parts: BTreeMap<(String, u32), BTreeSet<u32>> = BTreeMap::new();
        for (_, _, s) in self.all_symbols() {
            if s.annotated() {
                let prefix = s.prefix();
                if let Ok(n) = s.reference()[prefix.len()..].parse::<u32>() {
                    used.entry(prefix).or_default().insert(n);
                    if s.lib().is_some_and(|l| l.units > 1) {
                        parts
                            .entry((s.lib_id.clone(), n))
                            .or_default()
                            .insert(s.unit);
                    }
                }
            }
        }
        self.for_each_sheet_mut(&mut |sheet| {
            let mut order: Vec<usize> = (0..sheet.symbols.len())
                .filter(|i| !sheet.symbols[*i].annotated())
                .collect();
            order.sort_by_key(|i| {
                let p = sheet.symbols[*i].pos;
                if by_x {
                    (p.x, p.y, sheet.symbols[*i].id as i64)
                } else {
                    (p.y, p.x, sheet.symbols[*i].id as i64)
                }
            });
            for i in order {
                let prefix = sheet.symbols[i].prefix();
                let units = sheet.symbols[i].lib().map_or(1, |l| l.units);
                let (lib_id, unit) = (sheet.symbols[i].lib_id.clone(), sheet.symbols[i].unit);
                // A multi-unit part with this unit still free takes this unit.
                let shared = if units > 1 {
                    parts
                        .iter()
                        .find(|((l, _), taken)| *l == lib_id && !taken.contains(&unit))
                        .map(|((_, n), _)| *n)
                } else {
                    None
                };
                let n = match shared {
                    Some(n) => n,
                    None => {
                        let taken = used.entry(prefix.clone()).or_default();
                        let mut n = 1;
                        while taken.contains(&n) {
                            n += 1;
                        }
                        taken.insert(n);
                        n
                    }
                };
                if units > 1 {
                    parts.entry((lib_id, n)).or_default().insert(unit);
                }
                // Power symbols number with two digits, as KiCad writes #PWR01.
                let new = if prefix.starts_with('#') {
                    format!("{prefix}{n:02}")
                } else {
                    format!("{prefix}{n}")
                };
                let old = sheet.symbols[i].reference().to_owned();
                sheet.symbols[i].set_field("Reference", &new);
                changes.push((old, new));
            }
        });
        changes
    }
    /// Clear every reference in the design back to "R?".
    pub fn clear_annotation(&mut self) {
        self.for_each_sheet_mut(&mut |s| {
            for sym in &mut s.symbols {
                let prefix = sym.prefix();
                sym.set_field("Reference", &format!("{prefix}?"));
            }
        });
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
    /// Give every symbol whose library definition lives in `lib` (a project library)
    /// the new definition `def`, as Update Symbols from Library does.
    pub fn update_local_symbol(&mut self, def: &LibSymbol) -> usize {
        let mut n = 0;
        self.for_each_sheet_mut(&mut |s| {
            for sym in &mut s.symbols {
                if sym.lib_id == def.lib_id && sym.local.is_some() {
                    sym.local = Some(Box::new(def.clone()));
                    sym.unit = sym.unit.clamp(1, def.units.max(1));
                    n += 1;
                }
            }
        });
        n
    }
}

/// Bus member names a label or pin name stands for: `D[0..3]` is D0…D3 (either
/// direction), `{SDA SCL}` a group, `I2C{SDA SCL}` the group's members prefixed
/// `I2C.`; members of a group may be vectors themselves. `None` for a plain net name.
pub fn bus_members(name: &str) -> Option<Vec<String>> {
    let name = name.trim();
    if let Some(open) = name.find('{') {
        let close = name.rfind('}')?;
        if close < open {
            return None;
        }
        let prefix = &name[..open];
        let mut out = vec![];
        for m in name[open + 1..close].split_whitespace() {
            let members = bus_members(m).unwrap_or_else(|| vec![m.to_owned()]);
            for x in members {
                out.push(if prefix.is_empty() {
                    x
                } else {
                    format!("{prefix}.{x}")
                });
            }
        }
        return if out.is_empty() { None } else { Some(out) };
    }
    let open = name.find('[')?;
    if !name.ends_with(']') {
        return None;
    }
    let prefix = &name[..open];
    let (a, b) = name[open + 1..name.len() - 1].split_once("..")?;
    let (a, b): (i64, i64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    if (a - b).abs() > 1024 {
        return None;
    }
    let range: Vec<i64> = if a <= b {
        (a..=b).collect()
    } else {
        (b..=a).rev().collect()
    };
    Some(range.into_iter().map(|i| format!("{prefix}{i}")).collect())
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
    #[test]
    fn bus_names_expand_to_members() {
        assert_eq!(
            bus_members("D[0..3]").unwrap(),
            vec!["D0", "D1", "D2", "D3"]
        );
        assert_eq!(bus_members("A[2..0]").unwrap(), vec!["A2", "A1", "A0"]);
        assert_eq!(bus_members("{SDA SCL}").unwrap(), vec!["SDA", "SCL"]);
        assert_eq!(
            bus_members("I2C{SDA SCL}").unwrap(),
            vec!["I2C.SDA", "I2C.SCL"]
        );
        assert_eq!(bus_members("{EN D[0..1]}").unwrap(), vec!["EN", "D0", "D1"]);
        assert!(bus_members("VCC").is_none());
        assert!(bus_members("D[x..3]").is_none());
    }
    #[test]
    fn sheets_nest_and_annotate_across_the_hierarchy() {
        let mut root = Schematic::new("t");
        root.place("Device:R", Pt::new(1000, 1000), Xf::IDENTITY)
            .unwrap();
        let sh = root
            .add_sheet(
                Pt::new(3000, 1000),
                Pt::new(1000, 800),
                "amp",
                "amp.kicad_sch",
            )
            .unwrap();
        let sub = root.at_path_mut(&[sh]).unwrap();
        sub.place("Device:R", Pt::new(1000, 1000), Xf::IDENTITY)
            .unwrap();
        root.annotate(true, false);
        let all = root.all_symbols();
        let refs: Vec<(&str, &str)> = all
            .iter()
            .map(|(_, p, s)| (p.as_str(), s.reference()))
            .collect();
        assert_eq!(refs, vec![("/", "R1"), ("/amp/", "R2")]);
        assert_eq!(root.path_name(&[sh]), "/amp/");
        assert_ne!(all[0].0, all[1].0, "identities differ across sheets");
        assert!(root
            .add_sheet(Pt::new(0, 0), Pt::new(600, 600), "amp", "x.kicad_sch")
            .is_err());
    }
}
