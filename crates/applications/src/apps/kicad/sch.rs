//! The Schematic Editor: a sheet with grid and title block, symbols from the library,
//! wires with junctions, labels, annotation, ERC, netlists, BOM, and the way to the
//! board editor and the simulator.
use super::draw::{self, Cv};
use super::widgets::{self as w, chrome, MenuItem};
use super::{act, icons, live_items, Dialog, Drag, Kicad, View};
use crate::desktop_scene::{shared::Align, Painter};
use crate::{AppEffect, PointerPhase};
use cw_eda::connectivity;
use cw_eda::erc;
use cw_eda::geom::{Pt, Rect as WRect, Xf};
use cw_eda::netlist;
use cw_eda::schematic::{ortho, Item, LabelKind, GRID, SHEET_H, SHEET_W};
use cw_eda::symbols;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchUi {
    /// `select`, `symbol`, `power`, `wire`, `label`, `global`, `noconnect`, `junction`.
    pub tool: String,
    pub selection: Vec<Item>,
    /// A library symbol attached to the cursor, waiting to be placed.
    pub placing: Option<(String, Xf)>,
    /// Corner points of the wire being drawn.
    pub wire: Vec<Pt>,
    pub vertical_first: bool,
    /// 0 mm, 1 mils, 2 inches.
    pub units: u8,
    pub grid_hidden: bool,
    /// Annotation of newly placed symbols is on unless turned off.
    pub no_auto_annotate: bool,
}

pub(super) const MIN_ZOOM: i64 = 20;
pub(super) const MAX_ZOOM: i64 = 2400;

pub(super) fn units_label(u: u8) -> &'static str {
    match u {
        1 => "mils",
        2 => "in",
        _ => "mm",
    }
}
/// A schematic length (mils) in the chosen display units.
pub(super) fn show(v: i64, u: u8) -> String {
    match u {
        1 => format!("{v}"),
        2 => format!("{:.4}", v as f64 / 1000.0),
        _ => format!("{:.4}", v as f64 * 0.0254),
    }
}

fn canvas(width: u32, height: u32) -> Rect {
    let top = (w::MENU_H + w::TOOL_H) as i32;
    Rect::new(
        w::SIDE_W as i32 + 1,
        top,
        width.saturating_sub(2 * w::SIDE_W + 2),
        height.saturating_sub(w::MENU_H + w::TOOL_H + w::STATUS_H),
    )
}

impl Kicad {
    fn tool(&self) -> &str {
        if self.ui.sch.tool.is_empty() {
            "select"
        } else {
            &self.ui.sch.tool
        }
    }
    /// The view the canvas paints with: the stored one, or the page fitted.
    pub(super) fn sch_view(&self, cw: u32, ch: u32) -> View {
        if self.ui.view.fit || self.ui.view.zoom == 0 {
            View::fitted(Pt::new(0, 0), Pt::new(SHEET_W, SHEET_H), cw, ch)
        } else {
            self.ui.view
        }
    }
    fn sch_edit(&mut self) {
        let before = self.session.schematic.clone();
        self.session.sch_history.record(&before);
        self.session.sch_dirty = true;
        self.session.erc = None;
    }
    fn selected(&self) -> Vec<Item> {
        live_items(&self.session.schematic, &self.ui.sch.selection)
    }
    fn hit_tolerance(&self) -> i64 {
        (4000 / self.ui.view.zoom.max(1)).max(15)
    }
    /// Net name under a point: a wire, pin or label there.
    fn net_at(&self, at: Pt) -> Option<String> {
        let s = &self.session.schematic;
        let conn = connectivity::analyze(s);
        let tol = self.hit_tolerance();
        for item in s.hit(at, tol) {
            match item {
                Item::Wire(id) => {
                    if let Some(i) = conn.wire_net.get(&id) {
                        return Some(conn.nets[*i].name.clone());
                    }
                }
                Item::Label(id) => {
                    if let Some(i) = conn.label_net.get(&id) {
                        return Some(conn.nets[*i].name.clone());
                    }
                }
                Item::Symbol(id) => {
                    let sym = s.symbol(id)?;
                    for (pin, p) in sym.pins() {
                        if p.manhattan(at) <= tol * 2 {
                            if let Some(n) = conn.net_of_pin(id, pin.number) {
                                return Some(n.name.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    /// Whether a point is somewhere a wire can end: a pin, a wire, a label, a junction.
    fn connects_at(&self, at: Pt) -> bool {
        let s = &self.session.schematic;
        s.connection_points().contains(&at)
            || s.wires
                .iter()
                .any(|w| cw_eda::geom::on_segment(at, w.a, w.b))
            || s.labels.iter().any(|l| l.pos == at)
    }
    fn finish_wire(&mut self) {
        let pts = std::mem::take(&mut self.ui.sch.wire);
        if pts.len() < 2 {
            return;
        }
        self.sch_edit();
        for pair in pts.windows(2) {
            self.session.schematic.add_wire(pair[0], pair[1]);
        }
        self.session.schematic.cleanup_junctions();
        self.ui.status = format!("Wire of {} segment(s) added", pts.len() - 1);
    }
    fn set_tool(&mut self, tool: &str) {
        self.ui.sch.tool = tool.to_owned();
        self.ui.sch.wire.clear();
        self.ui.sch.placing = None;
        self.ui.drag = None;
        if tool != "select" {
            self.ui.sch.selection.clear();
        }
        if self.session.sim.probing && tool != "probe" {
            self.session.sim.probing = false;
        }
    }
    fn place_symbol(&mut self, at: Pt) {
        let Some((lib_id, xf)) = self.ui.sch.placing.take() else {
            return;
        };
        self.sch_edit();
        match self.session.schematic.place(&lib_id, at, xf) {
            Ok(id) => {
                if !self.ui.sch.no_auto_annotate {
                    self.session.schematic.annotate(true, false);
                }
                self.session.schematic.cleanup_junctions();
                let r = self
                    .session
                    .schematic
                    .symbol(id)
                    .map(|s| s.reference().to_owned())
                    .unwrap_or_default();
                self.ui.status = format!("Placed {r} ({lib_id})");
                self.ui.sch.selection = vec![Item::Symbol(id)];
            }
            Err(e) => self.ui.status = e,
        }
    }
    fn probe(&mut self, at: Pt) -> Result<Vec<AppEffect>, String> {
        let net = self
            .net_at(at)
            .ok_or("Nothing to probe here: click a wire or pin")?;
        let signal = format!("V({})", netlist::spice_node(&net));
        if !self.session.sim.shown.contains(&signal) {
            self.session.sim.shown.push(signal.clone());
        }
        self.ui.status = format!("Probed {signal}");
        Ok(vec![])
    }

    pub(super) fn sch_pointer(
        &mut self,
        _window: u64,
        phase: PointerPhase,
        at: Pt,
    ) -> Result<Vec<AppEffect>, String> {
        let snapped = at.snap(GRID);
        self.ui.hover = Some(snapped);
        if self.session.sim.probing {
            return match phase {
                PointerPhase::Up => self.probe(at),
                _ => Ok(vec![]),
            };
        }
        match self.tool().to_owned().as_str() {
            "select" => match phase {
                PointerPhase::Down => {
                    let hits = self.session.schematic.hit(at, self.hit_tolerance());
                    match hits.first() {
                        Some(item) => {
                            if !self.ui.sch.selection.contains(item) {
                                self.ui.sch.selection = vec![*item];
                            }
                            self.ui.drag = Some(Drag::Move {
                                start: snapped,
                                at: snapped,
                            });
                        }
                        None => {
                            self.ui.sch.selection.clear();
                            self.ui.drag = Some(Drag::Box { start: at, at });
                        }
                    }
                    Ok(vec![])
                }
                PointerPhase::Move => {
                    match &mut self.ui.drag {
                        Some(Drag::Move { at: a, .. }) => *a = snapped,
                        Some(Drag::Box { at: a, .. }) => *a = at,
                        _ => {}
                    }
                    Ok(vec![])
                }
                PointerPhase::Up => {
                    match self.ui.drag.take() {
                        Some(Drag::Move { start, .. }) => {
                            let d = snapped.sub(start);
                            let items = self.selected();
                            if d != Pt::default() && !items.is_empty() {
                                self.sch_edit();
                                self.session.schematic.move_items(&items, d, true);
                                self.ui.status = format!("Moved {} item(s)", items.len());
                            } else {
                                self.ui.status = describe_selection(self);
                            }
                        }
                        Some(Drag::Box { start, .. }) => {
                            let r = WRect::new(start, at);
                            if r.width() > self.hit_tolerance() || r.height() > self.hit_tolerance()
                            {
                                self.ui.sch.selection = self.session.schematic.inside(&r);
                            }
                            self.ui.status = describe_selection(self);
                        }
                        _ => {}
                    }
                    Ok(vec![])
                }
                PointerPhase::Cancel => {
                    self.ui.drag = None;
                    Ok(vec![])
                }
            },
            "wire" => {
                if phase != PointerPhase::Up {
                    return Ok(vec![]);
                }
                let p = snapped;
                match self.ui.sch.wire.last().copied() {
                    None => {
                        self.ui.sch.wire.push(p);
                        self.ui.status = "Click to add wire corners; click a pin or wire, or double-click, to finish".into();
                    }
                    Some(last) if last == p => {}
                    Some(last) => {
                        for (_, q) in ortho(last, p, self.ui.sch.vertical_first) {
                            self.ui.sch.wire.push(q);
                        }
                        // Landing on something connectable ends the wire, as in KiCad.
                        if self.connects_at(p) && Some(p) != self.ui.sch.wire.first().copied() {
                            self.finish_wire();
                        }
                    }
                }
                Ok(vec![])
            }
            "symbol" | "power" => {
                if phase != PointerPhase::Up {
                    return Ok(vec![]);
                }
                if self.ui.sch.placing.is_some() {
                    self.place_symbol(snapped);
                } else {
                    self.open_chooser(self.tool() == "power");
                }
                Ok(vec![])
            }
            "label" | "global" => {
                if phase == PointerPhase::Up {
                    self.ui.dialog = Some(Dialog::Label {
                        pos: snapped,
                        global: self.tool() == "global",
                        text: String::new(),
                        edit: None,
                    });
                    self.ui.focus = Some("text".into());
                }
                Ok(vec![])
            }
            "noconnect" | "junction" => {
                if phase == PointerPhase::Up {
                    self.sch_edit();
                    if self.tool() == "noconnect" {
                        self.session.schematic.add_no_connect(snapped);
                    } else {
                        self.session.schematic.add_junction(snapped);
                    }
                }
                Ok(vec![])
            }
            other => Err(format!("unknown schematic tool {other}")),
        }
    }
    pub(super) fn sch_activate(&mut self, _window: u64) -> Result<Vec<AppEffect>, String> {
        match self.tool() {
            "wire" => {
                self.finish_wire();
                Ok(vec![])
            }
            _ => {
                let sel = self.selected();
                match sel.as_slice() {
                    [Item::Symbol(id)] => self.open_properties(*id),
                    [Item::Label(id)] => {
                        let l = self
                            .session
                            .schematic
                            .labels
                            .iter()
                            .find(|l| l.id == *id)
                            .ok_or("label not found")?;
                        self.ui.dialog = Some(Dialog::Label {
                            pos: l.pos,
                            global: l.kind == LabelKind::Global,
                            text: l.text.clone(),
                            edit: Some(l.id),
                        });
                        self.ui.focus = Some("text".into());
                        Ok(vec![])
                    }
                    _ => Ok(vec![]),
                }
            }
        }
    }
    fn open_chooser(&mut self, power: bool) {
        self.ui.dialog = Some(Dialog::Chooser {
            power,
            filter: String::new(),
            selected: None,
            collapsed: vec![],
        });
        self.ui.focus = Some("filter".into());
    }
    fn open_properties(&mut self, id: u64) -> Result<Vec<AppEffect>, String> {
        let s = self
            .session
            .schematic
            .symbol(id)
            .ok_or("symbol not found")?;
        self.ui.dialog = Some(Dialog::SymbolProperties {
            id,
            fields: s
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.value.clone()))
                .collect(),
            exclude_sim: s.exclude_from_sim,
            on_board: s.on_board,
            in_bom: s.in_bom,
            dnp: s.dnp,
            error: String::new(),
        });
        self.ui.focus = Some("Value".into());
        Ok(vec![])
    }
    fn transform_selection(&mut self, op: Xf) -> Result<Vec<AppEffect>, String> {
        if let Some((_, xf)) = &mut self.ui.sch.placing {
            *xf = xf.then(op);
            return Ok(vec![]);
        }
        let sel = self.selected();
        if sel.is_empty() {
            return Err("select a symbol or label first".into());
        }
        self.sch_edit();
        self.session.schematic.transform(&sel, op);
        self.session.schematic.cleanup_junctions();
        Ok(vec![])
    }
    fn zoom(&mut self, dir: &str, args: &[&str]) -> Result<Vec<AppEffect>, String> {
        let (cw, ch) = match (
            args.get(3).and_then(|v| v.parse().ok()),
            args.get(4).and_then(|v| v.parse().ok()),
        ) {
            (Some(w), Some(h)) => (w, h),
            _ if self.ui.canvas.0 > 0 => self.ui.canvas,
            _ => (900, 600),
        };
        self.ui.canvas = (cw, ch);
        let mut v = View::from_args(args).unwrap_or_else(|| self.sch_view(cw, ch));
        match dir {
            "fit" => {
                self.ui.view = View { fit: true, ..v };
                return Ok(vec![]);
            }
            "objects" => {
                // Zoom to fit what is drawn, not the whole sheet.
                let s = &self.session.schematic;
                let mut r: Option<WRect> = None;
                let mut add = |b: WRect| r = Some(r.map_or(b, |x| x.union(&b)));
                for sym in &s.symbols {
                    add(sym.bounds());
                }
                for w in &s.wires {
                    add(WRect::new(w.a, w.b));
                }
                for l in &s.labels {
                    add(WRect::around(l.pos, 100, 100));
                }
                let r = r.ok_or("the sheet is empty")?.inflate(200);
                self.ui.view = View::fitted(r.min, r.max, cw, ch);
                return Ok(vec![]);
            }
            "in" | "out" => {
                let centre = self
                    .ui
                    .hover
                    .unwrap_or_else(|| v.world(cw as i32 / 2, ch as i32 / 2));
                let centre = if args.len() >= 3 {
                    v.world(cw as i32 / 2, ch as i32 / 2)
                } else {
                    centre
                };
                if dir == "in" {
                    v.zoom_about(centre, 3, 2, MIN_ZOOM, MAX_ZOOM);
                } else {
                    v.zoom_about(centre, 2, 3, MIN_ZOOM, MAX_ZOOM);
                }
            }
            other => return Err(format!("unknown zoom {other}")),
        }
        self.ui.view = v;
        Ok(vec![])
    }
    pub(super) fn sch_command(
        &mut self,
        window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let parts: Vec<&str> = rest.split(':').collect();
        match parts[0] {
            "save" => self.save_schematic(window),
            "undo" => {
                if !self.session.sch_history.undo(&mut self.session.schematic) {
                    return Err("nothing to undo".into());
                }
                self.session.sch_dirty = true;
                self.ui.status = "Undo".into();
                Ok(vec![])
            }
            "redo" => {
                if !self.session.sch_history.redo(&mut self.session.schematic) {
                    return Err("nothing to redo".into());
                }
                self.session.sch_dirty = true;
                self.ui.status = "Redo".into();
                Ok(vec![])
            }
            "zoom" => self.zoom(
                parts.get(1).copied().unwrap_or(""),
                &parts[2.min(parts.len())..],
            ),
            "rotate" => self.transform_selection(Xf::ROT_CCW),
            "mirror-x" => self.transform_selection(Xf::MIRROR_X),
            "mirror-y" => self.transform_selection(Xf::MIRROR_Y),
            "delete" => {
                let sel = self.selected();
                if sel.is_empty() {
                    return Err("nothing is selected".into());
                }
                self.sch_edit();
                let n = self.session.schematic.delete(&sel);
                self.ui.sch.selection.clear();
                self.ui.status = format!("Deleted {n} item(s)");
                Ok(vec![])
            }
            "properties" => match self.selected().as_slice() {
                [Item::Symbol(id)] => self.open_properties(*id),
                _ => Err("select one symbol to edit its properties".into()),
            },
            "tool" => {
                let tool = parts.get(1).copied().unwrap_or("select");
                if !matches!(
                    tool,
                    "select"
                        | "symbol"
                        | "power"
                        | "wire"
                        | "label"
                        | "global"
                        | "noconnect"
                        | "junction"
                ) {
                    return Err(format!("unknown tool {tool}"));
                }
                self.set_tool(tool);
                if matches!(tool, "symbol" | "power") {
                    self.open_chooser(tool == "power");
                }
                Ok(vec![])
            }
            "units" => {
                self.ui.sch.units = (self.ui.sch.units + 1) % 3;
                Ok(vec![])
            }
            "grid" => {
                self.ui.sch.grid_hidden = !self.ui.sch.grid_hidden;
                Ok(vec![])
            }
            "posture" => {
                self.ui.sch.vertical_first = !self.ui.sch.vertical_first;
                Ok(vec![])
            }
            "auto-annotate" => {
                self.ui.sch.no_auto_annotate = !self.ui.sch.no_auto_annotate;
                Ok(vec![])
            }
            "annotate" => {
                self.ui.dialog = Some(Dialog::Annotate {
                    by_x: true,
                    reset: false,
                    log: vec![],
                });
                Ok(vec![])
            }
            "erc" => {
                self.ui.dialog = Some(Dialog::Erc { selected: None });
                Ok(vec![])
            }
            "netlist" => {
                self.ui.dialog = Some(Dialog::Netlist {
                    spice: false,
                    log: vec![],
                });
                Ok(vec![])
            }
            "bom" => {
                self.ui.dialog = Some(Dialog::Bom { log: vec![] });
                Ok(vec![])
            }
            "update-pcb" => self.launch_frame(window, "pcb-update"),
            "pcb" => self.launch_frame(window, "pcb"),
            "simulator" => self.launch_frame(window, "sim"),
            other => Err(format!("unknown schematic command {other}")),
        }
    }
    pub(super) fn sch_key(&mut self, window: u64, key: &str) -> Result<Vec<AppEffect>, String> {
        let command = match key {
            "r" | "R" => "rotate",
            "x" | "X" => "mirror-x",
            "y" | "Y" => "mirror-y",
            "Delete" | "Backspace" => "delete",
            "e" | "E" => "properties",
            "Ctrl+z" | "Meta+z" => "undo",
            "Ctrl+y" | "Ctrl+Shift+z" | "Meta+Shift+z" => "redo",
            "w" | "W" => "tool:wire",
            "a" | "A" => "tool:symbol",
            "p" | "P" => "tool:power",
            "l" | "L" => "tool:label",
            "Ctrl+l" | "Meta+l" => "tool:global",
            "q" | "Q" => "tool:noconnect",
            "j" | "J" => "tool:junction",
            "/" => "posture",
            "Home" => "zoom:fit",
            "Ctrl+Home" | "Meta+Home" => "zoom:objects",
            "F1" | "+" | "=" => "zoom:in",
            "F2" | "-" => "zoom:out",
            "End" | "Enter" if self.tool() == "wire" => {
                self.finish_wire();
                return Ok(vec![]);
            }
            "Escape" => {
                if self.session.sim.probing {
                    self.session.sim.probing = false;
                    self.ui.status = "Probe tool ended".into();
                } else if !self.ui.sch.wire.is_empty() || self.ui.sch.placing.is_some() {
                    self.ui.sch.wire.clear();
                    self.ui.sch.placing = None;
                } else if self.tool() != "select" {
                    self.set_tool("select");
                } else {
                    self.ui.sch.selection.clear();
                }
                return Ok(vec![]);
            }
            other => return Err(format!("unsupported schematic key {other}")),
        };
        self.sch_command(window, command)
    }

    pub(super) fn sch_dialog(
        &mut self,
        window: u64,
        rest: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let dialog = self.ui.dialog.clone().ok_or("no dialog is open")?;
        let (cmd, arg) = rest.split_once(':').unwrap_or((rest, ""));
        match dialog {
            Dialog::Chooser {
                power,
                filter,
                selected,
                mut collapsed,
            } => match cmd {
                "pick" => {
                    let lib = symbols::find(arg).ok_or("symbol not in the library")?;
                    if lib.power != power && power {
                        return Err("choose a power symbol".into());
                    }
                    self.ui.dialog = Some(Dialog::Chooser {
                        power,
                        filter,
                        selected: Some(arg.to_owned()),
                        collapsed,
                    });
                    Ok(vec![])
                }
                "lib" => {
                    if let Some(i) = collapsed.iter().position(|c| c == arg) {
                        collapsed.remove(i);
                    } else {
                        collapsed.push(arg.to_owned());
                    }
                    self.ui.dialog = Some(Dialog::Chooser {
                        power,
                        filter,
                        selected,
                        collapsed,
                    });
                    Ok(vec![])
                }
                "ok" => {
                    let lib_id = selected.ok_or("select a symbol first")?;
                    self.close_dialog();
                    self.ui.sch.tool = if power {
                        "power".into()
                    } else {
                        "symbol".into()
                    };
                    self.ui.sch.placing = Some((lib_id.clone(), Xf::IDENTITY));
                    self.ui.status = format!("Click to place {lib_id}; R rotates, X/Y mirror");
                    Ok(vec![])
                }
                other => Err(format!("unknown chooser command {other}")),
            },
            Dialog::SymbolProperties {
                id,
                mut fields,
                mut exclude_sim,
                mut on_board,
                mut in_bom,
                mut dnp,
                ..
            } => match cmd {
                "toggle" => {
                    match arg {
                        "exclude_sim" => exclude_sim = !exclude_sim,
                        "on_board" => on_board = !on_board,
                        "in_bom" => in_bom = !in_bom,
                        "dnp" => dnp = !dnp,
                        other => return Err(format!("unknown option {other}")),
                    }
                    self.ui.dialog = Some(Dialog::SymbolProperties {
                        id,
                        fields,
                        exclude_sim,
                        on_board,
                        in_bom,
                        dnp,
                        error: String::new(),
                    });
                    Ok(vec![])
                }
                "footprint" => {
                    let fp = if arg.is_empty() || cw_eda::footprints::find(arg).is_some() {
                        arg.to_owned()
                    } else {
                        return Err("footprint not in the library".into());
                    };
                    if let Some(f) = fields.iter_mut().find(|(k, _)| k == "Footprint") {
                        f.1 = fp;
                    }
                    self.ui.dialog = Some(Dialog::SymbolProperties {
                        id,
                        fields,
                        exclude_sim,
                        on_board,
                        in_bom,
                        dnp,
                        error: String::new(),
                    });
                    Ok(vec![])
                }
                "ok" => {
                    let reference = fields
                        .iter()
                        .find(|(k, _)| k == "Reference")
                        .map(|(_, v)| v.trim().to_owned())
                        .unwrap_or_default();
                    let error = if reference.is_empty() {
                        Some("A reference designator is required.")
                    } else if reference.contains(char::is_whitespace) {
                        Some("Reference designators cannot contain spaces.")
                    } else {
                        None
                    };
                    if let Some(e) = error {
                        self.ui.dialog = Some(Dialog::SymbolProperties {
                            id,
                            fields,
                            exclude_sim,
                            on_board,
                            in_bom,
                            dnp,
                            error: e.into(),
                        });
                        return Ok(vec![]);
                    }
                    self.sch_edit();
                    let s = self
                        .session
                        .schematic
                        .symbol_mut(id)
                        .ok_or("symbol not found")?;
                    for (k, v) in &fields {
                        s.set_field(k, v.trim());
                    }
                    s.exclude_from_sim = exclude_sim;
                    s.on_board = on_board;
                    s.in_bom = in_bom;
                    s.dnp = dnp;
                    self.ui.status = format!("{reference} updated");
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown properties command {other}")),
            },
            Dialog::Label {
                pos,
                global,
                text,
                edit,
            } => match cmd {
                "ok" => {
                    let text = text.trim().to_owned();
                    if text.is_empty() {
                        return Err("a label needs text".into());
                    }
                    self.sch_edit();
                    match edit {
                        Some(id) => {
                            if let Some(l) = self
                                .session
                                .schematic
                                .labels
                                .iter_mut()
                                .find(|l| l.id == id)
                            {
                                l.text = text;
                            }
                        }
                        None => {
                            let kind = if global {
                                LabelKind::Global
                            } else {
                                LabelKind::Local
                            };
                            let id = self.session.schematic.add_label(pos, &text, kind);
                            self.ui.sch.selection = vec![Item::Label(id)];
                        }
                    }
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown label command {other}")),
            },
            Dialog::Annotate {
                by_x,
                reset,
                mut log,
            } => match cmd {
                "order" => {
                    self.ui.dialog = Some(Dialog::Annotate {
                        by_x: arg == "x",
                        reset,
                        log,
                    });
                    Ok(vec![])
                }
                "scope" => {
                    self.ui.dialog = Some(Dialog::Annotate {
                        by_x,
                        reset: arg == "reset",
                        log,
                    });
                    Ok(vec![])
                }
                "run" => {
                    self.sch_edit();
                    let changes = self.session.schematic.annotate(by_x, reset);
                    log.clear();
                    if changes.is_empty() {
                        log.push("No symbols needed annotation.".into());
                    }
                    for (old, new) in &changes {
                        log.push(format!("Annotated {old} as {new}."));
                    }
                    log.push("Annotation complete.".into());
                    self.ui.dialog = Some(Dialog::Annotate { by_x, reset, log });
                    Ok(vec![])
                }
                "clear" => {
                    self.sch_edit();
                    self.session.schematic.clear_annotation();
                    self.ui.dialog = Some(Dialog::Annotate {
                        by_x,
                        reset,
                        log: vec!["Annotation cleared.".into()],
                    });
                    Ok(vec![])
                }
                "ok" => {
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown annotate command {other}")),
            },
            Dialog::Erc { .. } => match cmd {
                "run" => {
                    let v = erc::check(&self.session.schematic);
                    self.ui.status = format!(
                        "ERC: {} errors, {} warnings",
                        v.iter()
                            .filter(|x| x.severity == erc::Severity::Error)
                            .count(),
                        v.iter()
                            .filter(|x| x.severity == erc::Severity::Warning)
                            .count()
                    );
                    self.session.erc = Some(v);
                    self.ui.dialog = Some(Dialog::Erc { selected: None });
                    Ok(vec![])
                }
                "select" => {
                    let i: usize = arg.parse().map_err(|_| "bad row")?;
                    let v = self
                        .session
                        .erc
                        .as_ref()
                        .and_then(|v| v.get(i))
                        .ok_or("violation not found")?
                        .pos;
                    // Centre the view on the marker.
                    let (cw, ch) = if self.ui.canvas.0 > 0 {
                        self.ui.canvas
                    } else {
                        (900, 600)
                    };
                    let mut view = self.sch_view(cw, ch);
                    view.x0 = v.x - (cw as i64 * 1000 / 2 / view.zoom.max(1));
                    view.y0 = v.y - (ch as i64 * 1000 / 2 / view.zoom.max(1));
                    view.fit = false;
                    self.ui.view = view;
                    self.ui.dialog = Some(Dialog::Erc { selected: Some(i) });
                    Ok(vec![])
                }
                "clear" => {
                    self.session.erc = None;
                    self.ui.dialog = Some(Dialog::Erc { selected: None });
                    Ok(vec![])
                }
                "ok" => {
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown ERC command {other}")),
            },
            Dialog::Netlist { spice, mut log } => match cmd {
                "format" => {
                    // The notebook's tabs: 0 is KiCad, 1 is Spice.
                    self.ui.dialog = Some(Dialog::Netlist {
                        spice: arg == "1" || arg == "spice",
                        log,
                    });
                    Ok(vec![])
                }
                "export" | "ok" => {
                    let project = self.session.project.clone().ok_or("No project is open")?;
                    let (path, content) = if spice {
                        let command = self.session.schematic.sim_command().map(|t| t.text.clone());
                        match netlist::spice_netlist(
                            &self.session.schematic,
                            &project.name,
                            command.as_deref(),
                        ) {
                            Ok(deck) => (project.file("cir"), deck.text),
                            Err(e) => {
                                log.push(format!("Error: {e}"));
                                self.ui.dialog = Some(Dialog::Netlist { spice, log });
                                return Ok(vec![]);
                            }
                        }
                    } else {
                        (
                            project.file("net"),
                            netlist::kicad_netlist(
                                &self.session.schematic,
                                &project.file("kicad_sch"),
                                &date(clock_us),
                            ),
                        )
                    };
                    log.push(format!("Netlist written to {path}"));
                    self.ui.status = format!("Netlist written to {path}");
                    self.ui.dialog = Some(Dialog::Netlist { spice, log });
                    Ok(vec![
                        AppEffect::WriteFile {
                            window,
                            path,
                            content,
                        },
                        AppEffect::ListDirectory {
                            window,
                            tab: 0,
                            path: project.dir,
                        },
                    ])
                }
                other => Err(format!("unknown netlist command {other}")),
            },
            Dialog::Bom { mut log } => match cmd {
                "export" | "ok" => {
                    let project = self.session.project.clone().ok_or("No project is open")?;
                    let path = project.file("csv");
                    log.push(format!("Bill of materials written to {path}"));
                    self.ui.status = format!("BOM written to {path}");
                    self.ui.dialog = Some(Dialog::Bom { log });
                    Ok(vec![
                        AppEffect::WriteFile {
                            window,
                            path,
                            content: netlist::bom_csv(&self.session.schematic),
                        },
                        AppEffect::ListDirectory {
                            window,
                            tab: 0,
                            path: project.dir,
                        },
                    ])
                }
                other => Err(format!("unknown BOM command {other}")),
            },
            _ => Err("not a schematic dialog".into()),
        }
    }

    // ---- painting ---------------------------------------------------------------------
    pub(super) fn render_sch(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (w, h) = (env.width, env.height);
        let c = chrome(env.theme);
        p.scene.background = c.bar;
        let area = canvas(w, h);
        let view = self.sch_view(area.width, area.height);
        let cv = Cv {
            ox: area.x,
            oy: area.y,
            w: area.width,
            h: area.height,
            view,
        };
        self.paint_sheet(p, &cv);
        p.region(
            area,
            &format!(
                "kicad:canvas:sch:{}:{}:{}",
                view.args(),
                area.width,
                area.height
            ),
            "Schematic canvas",
        );
        // Chrome over the canvas.
        let menus = sch_menus(self);
        let titles: Vec<&str> = menus.iter().map(|m| m.0).collect();
        let top = w::MENU_H as i32;
        p.box_(Rect::new(0, top, w, w::TOOL_H), c.bar, 0);
        p.hline(0, top + w::TOOL_H as i32 - 1, w, w::EDGE);
        let zoom_args = format!("{}:{}:{}", view.args(), area.width, area.height);
        let has_sel = !self.selected().is_empty();
        let sel = |t: &str| -> Result<String, &'static str> {
            if has_sel {
                Ok(t.to_owned())
            } else {
                Err("nothing is selected")
            }
        };
        let proj = |t: &str| -> Result<String, &'static str> {
            if self.session.project.is_some() {
                Ok(t.to_owned())
            } else {
                Err("no project is open")
            }
        };
        let undo = if self.session.sch_history.past.is_empty() {
            Err("nothing to undo")
        } else {
            Ok("kicad:sch:undo".to_owned())
        };
        let redo = if self.session.sch_history.future.is_empty() {
            Err("nothing to redo")
        } else {
            Ok("kicad:sch:redo".to_owned())
        };
        let mut x = 6;
        let ty = top + 3;
        let groups: Vec<Vec<w::ToolSpec>> = vec![
            vec![(icons::save, proj("kicad:sch:save"), "Save (Ctrl+S)")],
            vec![
                (icons::undo, undo, "Undo (Ctrl+Z)"),
                (icons::redo, redo, "Redo (Ctrl+Y)"),
            ],
            vec![
                (
                    icons::zoom_in,
                    Ok(format!("kicad:sch:zoom:in:{zoom_args}")),
                    "Zoom in (F1)",
                ),
                (
                    icons::zoom_out,
                    Ok(format!("kicad:sch:zoom:out:{zoom_args}")),
                    "Zoom out (F2)",
                ),
                (
                    icons::zoom_fit,
                    Ok(format!("kicad:sch:zoom:fit:{zoom_args}")),
                    "Zoom to fit (Home)",
                ),
                (
                    icons::zoom_objects,
                    if self.session.schematic.symbols.is_empty()
                        && self.session.schematic.wires.is_empty()
                    {
                        Err("the sheet is empty")
                    } else {
                        Ok(format!("kicad:sch:zoom:objects:{zoom_args}"))
                    },
                    "Zoom to objects (Ctrl+Home)",
                ),
            ],
            vec![
                (
                    icons::rotate,
                    sel("kicad:sch:rotate"),
                    "Rotate counterclockwise (R)",
                ),
                (
                    icons::mirror_v,
                    sel("kicad:sch:mirror-x"),
                    "Mirror vertically (X)",
                ),
                (
                    icons::mirror_h,
                    sel("kicad:sch:mirror-y"),
                    "Mirror horizontally (Y)",
                ),
                (icons::delete, sel("kicad:sch:delete"), "Delete (Del)"),
            ],
            vec![
                (
                    icons::annotate,
                    Ok("kicad:sch:annotate".into()),
                    "Annotate schematic symbols",
                ),
                (
                    icons::erc,
                    Ok("kicad:sch:erc".into()),
                    "Perform electrical rules check",
                ),
                (icons::netlist, proj("kicad:sch:netlist"), "Export netlist"),
                (
                    icons::bom,
                    proj("kicad:sch:bom"),
                    "Generate bill of materials",
                ),
            ],
            vec![
                (icons::simulator, proj("kicad:sch:simulator"), "Simulator"),
                (
                    icons::update,
                    proj("kicad:sch:update-pcb"),
                    "Update PCB with changes made to schematic (F8)",
                ),
                (icons::board, proj("kicad:sch:pcb"), "Switch to PCB Editor"),
            ],
        ];
        for (gi, group) in groups.into_iter().enumerate() {
            if gi > 0 {
                w::separator_v(p, x + 2, ty);
                x += 8;
            }
            for (icon, target, tip) in group {
                w::tool(p, &c, x, ty, icon, target, tip, false);
                x += 30;
            }
        }
        // Left toolbar: display options.
        p.box_(Rect::new(0, area.y, w::SIDE_W, area.height), c.bar, 0);
        p.vline(w::SIDE_W as i32, area.y, area.height, w::EDGE);
        let mut y = area.y + 4;
        for (icon, target, tip, on) in [
            (
                icons::grid as w::Icon,
                "kicad:sch:grid",
                "Show grid",
                !self.ui.sch.grid_hidden,
            ),
            (icons::units, "kicad:sch:units", "Change units", false),
            (
                icons::posture,
                "kicad:sch:posture",
                "Wire posture: vertical first",
                self.ui.sch.vertical_first,
            ),
            (
                icons::annotate,
                "kicad:sch:auto-annotate",
                "Automatically annotate symbols",
                !self.ui.sch.no_auto_annotate,
            ),
        ] {
            w::tool(p, &c, 3, y, icon, Ok(target.into()), tip, on);
            y += 32;
        }
        // Right toolbar: tools.
        let rx = w as i32 - w::SIDE_W as i32;
        p.box_(Rect::new(rx, area.y, w::SIDE_W, area.height), c.bar, 0);
        p.vline(rx, area.y, area.height, w::EDGE);
        let mut y = area.y + 4;
        for (icon, tool, tip) in [
            (icons::select as w::Icon, "select", "Select item(s)"),
            (icons::add_symbol, "symbol", "Add a symbol (A)"),
            (icons::add_power, "power", "Add a power port (P)"),
            (icons::wire, "wire", "Add a wire (W)"),
            (
                icons::no_connect,
                "noconnect",
                "Add a no-connection flag (Q)",
            ),
            (icons::junction, "junction", "Add a junction (J)"),
            (icons::label, "label", "Add a label (L)"),
            (icons::global_label, "global", "Add a global label (Ctrl+L)"),
        ] {
            w::tool(
                p,
                &c,
                rx + 3,
                y,
                icon,
                Ok(format!("kicad:sch:tool:{tool}")),
                tip,
                self.tool() == tool && !self.session.sim.probing,
            );
            y += 32;
        }
        // Status bar.
        let u = self.ui.sch.units;
        let hv = self.ui.hover.unwrap_or_default();
        let tool_name = if self.session.sim.probing {
            "Probe".to_owned()
        } else {
            match self.tool() {
                "select" => "Select item(s)",
                "symbol" => "Add a symbol",
                "power" => "Add a power port",
                "wire" => "Add a wire",
                "noconnect" => "Add a no-connection flag",
                "junction" => "Add a junction",
                "label" => "Add a label",
                "global" => "Add a global label",
                _ => "",
            }
            .to_owned()
        };
        w::status_bar(
            p,
            &c,
            w,
            h,
            &[
                format!("Z {:.2}", view.zoom as f64 / 100.0),
                format!("X {} Y {}", show(hv.x, u), show(hv.y, u)),
                format!("grid X {} Y {}", show(GRID, u), show(GRID, u)),
                units_label(u).into(),
                tool_name,
                self.ui.status.clone(),
            ],
        );
        let xs = w::menubar(p, &c, w, &titles, self.ui.menu.as_deref());
        if let Some(open) = &self.ui.menu {
            if let Some(i) = titles.iter().position(|t| t == open) {
                p.z += 40;
                w::menu_panel(p, &c, xs[i], &menus[i].1);
                p.z -= 40;
            }
        }
    }

    fn paint_sheet(&self, p: &mut Painter, cv: &Cv) {
        let s = &self.session.schematic;
        p.box_(Rect::new(cv.ox, cv.oy, cv.w, cv.h), draw::SCH_BG, 0);
        // Grid dots, coarsened until there are few enough to paint.
        if !self.ui.sch.grid_hidden {
            let mut step = GRID;
            while cv.len(step) < 8
                || (cv.w as i64 / cv.len(step).max(1) as i64)
                    * (cv.h as i64 / cv.len(step).max(1) as i64)
                    > 2500
            {
                step *= 2;
                if step > 10_000 {
                    break;
                }
            }
            let tl = cv.view.world(0, 0);
            let br = cv.view.world(cv.w as i32, cv.h as i32);
            let gx0 = (tl.x.max(0) / step) * step;
            let gy0 = (tl.y.max(0) / step) * step;
            let mut gy = gy0;
            while gy <= br.y.min(SHEET_H) {
                let mut gx = gx0;
                while gx <= br.x.min(SHEET_W) {
                    let (x, y) = cv.pt(Pt::new(gx, gy));
                    p.box_(Rect::new(x, y, 1, 1), draw::SCH_GRID, 0);
                    gx += step;
                }
                gy += step;
            }
        }
        // Drawing sheet: border with zone references and the title block.
        let m = 394;
        let inner = 79;
        let outer = [
            Pt::new(m, m),
            Pt::new(SHEET_W - m, m),
            Pt::new(SHEET_W - m, SHEET_H - m),
            Pt::new(m, SHEET_H - m),
            Pt::new(m, m),
        ];
        cv.stroke(p, &outer, draw::SHEET, 6, 1);
        let inr = [
            Pt::new(m + inner, m + inner),
            Pt::new(SHEET_W - m - inner, m + inner),
            Pt::new(SHEET_W - m - inner, SHEET_H - m - inner),
            Pt::new(m + inner, SHEET_H - m - inner),
            Pt::new(m + inner, m + inner),
        ];
        cv.stroke(p, &inr, draw::SHEET, 6, 1);
        let cols = 6;
        let span = (SHEET_W - 2 * m) / cols;
        for i in 0..cols {
            let x = m + span * i;
            if i > 0 {
                cv.stroke(
                    p,
                    &[Pt::new(x, m), Pt::new(x, m + inner)],
                    draw::SHEET,
                    6,
                    1,
                );
                cv.stroke(
                    p,
                    &[Pt::new(x, SHEET_H - m - inner), Pt::new(x, SHEET_H - m)],
                    draw::SHEET,
                    6,
                    1,
                );
            }
            let label = (i + 1).to_string();
            cv.text(
                p,
                Pt::new(x + span / 2, m + inner / 2),
                &label,
                50,
                draw::SHEET,
                Align::Center,
            );
            cv.text(
                p,
                Pt::new(x + span / 2, SHEET_H - m - inner / 2),
                &label,
                50,
                draw::SHEET,
                Align::Center,
            );
        }
        let rows = 4;
        let rspan = (SHEET_H - 2 * m) / rows;
        for i in 0..rows {
            let y = m + rspan * i;
            if i > 0 {
                cv.stroke(
                    p,
                    &[Pt::new(m, y), Pt::new(m + inner, y)],
                    draw::SHEET,
                    6,
                    1,
                );
                cv.stroke(
                    p,
                    &[Pt::new(SHEET_W - m - inner, y), Pt::new(SHEET_W - m, y)],
                    draw::SHEET,
                    6,
                    1,
                );
            }
            let label = ((b'A' + i as u8) as char).to_string();
            cv.text(
                p,
                Pt::new(m + inner / 2, y + rspan / 2),
                &label,
                50,
                draw::SHEET,
                Align::Center,
            );
            cv.text(
                p,
                Pt::new(SHEET_W - m - inner / 2, y + rspan / 2),
                &label,
                50,
                draw::SHEET,
                Align::Center,
            );
        }
        let (tx1, ty1) = (SHEET_W - m - inner, SHEET_H - m - inner);
        let tx0 = tx1 - 4331;
        let ty0 = ty1 - 1260;
        cv.stroke(
            p,
            &[Pt::new(tx0, ty1), Pt::new(tx0, ty0), Pt::new(tx1, ty0)],
            draw::SHEET,
            6,
            1,
        );
        for dy in [300, 600, 900] {
            cv.stroke(
                p,
                &[Pt::new(tx0, ty0 + dy), Pt::new(tx1, ty0 + dy)],
                draw::SHEET,
                6,
                1,
            );
        }
        let name = self
            .session
            .project
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let tb = &s.title_block;
        cv.text(
            p,
            Pt::new(tx0 + 60, ty0 + 150),
            "Sheet: /",
            50,
            draw::SHEET,
            Align::Left,
        );
        cv.text(
            p,
            Pt::new(tx0 + 60, ty0 + 450),
            &format!("File: {name}.kicad_sch"),
            50,
            draw::SHEET,
            Align::Left,
        );
        cv.text(
            p,
            Pt::new(tx0 + 60, ty0 + 750),
            &format!("Title: {}", tb.title),
            60,
            draw::SHEET,
            Align::Left,
        );
        cv.text(
            p,
            Pt::new(tx0 + 60, ty0 + 1050),
            &format!("Size: A4   Date: {}   Rev: {}", tb.date, tb.rev),
            50,
            draw::SHEET,
            Align::Left,
        );
        cv.text(
            p,
            Pt::new(tx0 + 60, ty0 + 1190),
            "KiCad E.D.A. 8.0.4",
            40,
            draw::SHEET,
            Align::Left,
        );
        cv.text(
            p,
            Pt::new(tx1 - 600, ty0 + 1190),
            "Id: 1/1",
            40,
            draw::SHEET,
            Align::Left,
        );
        // Selection highlights under the items.
        let sel = self.selected();
        for item in &sel {
            let r = match item {
                Item::Symbol(id) => s.symbol(*id).map(|x| x.bounds().inflate(20)),
                Item::Wire(id) => s
                    .wires
                    .iter()
                    .find(|w| w.id == *id)
                    .map(|w| WRect::new(w.a, w.b).inflate(15)),
                Item::Label(id) => s
                    .labels
                    .iter()
                    .find(|l| l.id == *id)
                    .map(|l| WRect::around(l.pos, 40, 40)),
                Item::Junction(id) | Item::NoConnect(id) => s
                    .junctions
                    .iter()
                    .chain(&s.no_connects)
                    .find(|j| j.id == *id)
                    .map(|j| WRect::around(j.pos, 40, 40)),
                Item::Text(id) => s
                    .texts
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| WRect::around(t.pos, 60, 60)),
            };
            if let Some(r) = r {
                let (x0, y0) = cv.pt(r.min);
                let (x1, y1) = cv.pt(r.max);
                p.border(
                    Rect::new(x0, y0, (x1 - x0).max(2) as u32, (y1 - y0).max(2) as u32),
                    draw::SELECT,
                    2,
                    draw::SELECT_EDGE,
                );
            }
        }
        // Items displaced by a move in progress.
        let offset = match &self.ui.drag {
            Some(Drag::Move { start, at }) => at.sub(*start),
            _ => Pt::default(),
        };
        let moved = |item: Item| {
            if sel.contains(&item) {
                offset
            } else {
                Pt::default()
            }
        };
        for wire in &s.wires {
            let d = moved(Item::Wire(wire.id));
            cv.stroke(p, &[wire.a.add(d), wire.b.add(d)], draw::WIRE, 6, 2);
        }
        for t in &s.texts {
            cv.text(
                p,
                t.pos.add(moved(Item::Text(t.id))),
                &t.text,
                50,
                draw::LABEL,
                Align::Left,
            );
        }
        for sym in &s.symbols {
            if let Some(lib) = sym.lib() {
                let d = moved(Item::Symbol(sym.id));
                draw::symbol(p, cv, lib, sym.pos.add(d), sym.xf, Some(&sym.fields), false);
            }
        }
        for j in &s.junctions {
            let (x, y) = cv.pt(j.pos.add(moved(Item::Junction(j.id))));
            p.circle(x, y, cv.len(18).max(2) as u32, draw::WIRE);
        }
        for n in &s.no_connects {
            let c = n.pos.add(moved(Item::NoConnect(n.id)));
            cv.stroke(
                p,
                &[c.add(Pt::new(-25, -25)), c.add(Pt::new(25, 25))],
                draw::NO_CONNECT,
                6,
                1,
            );
            cv.stroke(
                p,
                &[c.add(Pt::new(25, -25)), c.add(Pt::new(-25, 25))],
                draw::NO_CONNECT,
                6,
                1,
            );
        }
        for l in &s.labels {
            let at = l.pos.add(moved(Item::Label(l.id)));
            let size = 50;
            let tw = (60 * l.text.chars().count()) as i64;
            let right = l.orient == 2;
            match l.kind {
                LabelKind::Local => {
                    let anchor = at.add(Pt::new(if right { -10 } else { 10 }, -45));
                    cv.text(
                        p,
                        anchor,
                        &l.text,
                        size,
                        draw::LABEL,
                        if right { Align::Right } else { Align::Left },
                    );
                }
                LabelKind::Global => {
                    let dir = if right { -1 } else { 1 };
                    let hh = 40;
                    let pts = [
                        at,
                        at.add(Pt::new(dir * hh, -hh)),
                        at.add(Pt::new(dir * (hh + tw + 40), -hh)),
                        at.add(Pt::new(dir * (hh + tw + 40), hh)),
                        at.add(Pt::new(dir * hh, hh)),
                        at,
                    ];
                    cv.stroke(p, &pts, draw::BODY, 6, 1);
                    cv.text(
                        p,
                        at.add(Pt::new(dir * (hh + 20), 0)),
                        &l.text,
                        size,
                        draw::BODY,
                        if right { Align::Right } else { Align::Left },
                    );
                }
            }
        }
        // ERC markers.
        if let Some(v) = &self.session.erc {
            for (i, x) in v.iter().enumerate() {
                let color = if x.severity == erc::Severity::Error {
                    Color::rgb(255, 0, 0)
                } else {
                    Color::rgb(0, 190, 0)
                };
                let (px, py) = cv.pt(x.pos);
                let big =
                    matches!(&self.ui.dialog, Some(Dialog::Erc { selected: Some(s) }) if *s == i);
                let k = if big { 2 } else { 1 };
                p.path(
                    vec![
                        (px, py),
                        (px + 14 * k, py - 5 * k),
                        (px + 5 * k, py - 14 * k),
                    ],
                    color,
                );
            }
        }
        // What the current tool is drawing under the pointer.
        if let Some(hover) = self.ui.hover {
            if let Some(last) = self.ui.sch.wire.last() {
                let mut pts = self.ui.sch.wire.clone();
                for (_, q) in ortho(*last, hover, self.ui.sch.vertical_first) {
                    pts.push(q);
                }
                cv.stroke(p, &pts, draw::WIRE, 6, 2);
            }
            if let Some((lib_id, xf)) = &self.ui.sch.placing {
                if let Some(lib) = symbols::find(lib_id) {
                    draw::symbol(p, cv, lib, hover, *xf, None, true);
                }
            }
            if let Some(Drag::Box { start, at }) = &self.ui.drag {
                let (x0, y0) = cv.pt(Pt::new(start.x.min(at.x), start.y.min(at.y)));
                let (x1, y1) = cv.pt(Pt::new(start.x.max(at.x), start.y.max(at.y)));
                p.border(
                    Rect::new(x0, y0, (x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32),
                    Color(0, 120, 215, 25),
                    0,
                    draw::SELECT_EDGE,
                );
            }
        }
    }

    pub(super) fn render_sch_dialog(
        &self,
        p: &mut Painter,
        env: &crate::AppEnv<'_>,
        dialog: &Dialog,
    ) {
        let c = chrome(env.theme);
        let (w, h) = (env.width, env.height);
        let focus = self.ui.focus.as_deref();
        let ok_cancel = |p: &mut Painter, body: Rect, ok: &str| {
            let by = body.y + body.height as i32 - 30;
            w::button(
                p,
                &c,
                Rect::new(body.x + body.width as i32 - 180, by, 84, 28),
                "Cancel",
                "kicad:dlg:cancel",
                false,
            );
            w::button(
                p,
                &c,
                Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                ok,
                "kicad:dlg:ok",
                true,
            );
        };
        match dialog {
            Dialog::Chooser {
                power,
                filter,
                selected,
                collapsed,
            } => {
                let body = w::dialog(p, &c, env.theme, w, h, 760, 520, dialog.title());
                let lw = body.width / 2 - 6;
                w::field(
                    p,
                    &c,
                    Rect::new(body.x, body.y, lw, 26),
                    filter,
                    "filter",
                    focus == Some("filter"),
                );
                if filter.is_empty() {
                    p.label(
                        body.x + 8,
                        body.y + 4,
                        lw - 16,
                        "Filter",
                        13,
                        w::FAINT,
                        false,
                        Align::Left,
                    );
                }
                let list = Rect::new(body.x, body.y + 32, lw, body.height - 74);
                p.border(list, w::WHITE, 2, w::EDGE);
                let needle = filter.to_ascii_lowercase();
                let mut y = list.y + 2;
                let bottom = list.y + list.height as i32 - 22;
                for lib in symbols::libraries() {
                    let items: Vec<_> = symbols::library()
                        .iter()
                        .filter(|s| s.library() == lib && s.power == *power)
                        .filter(|s| {
                            needle.is_empty()
                                || s.lib_id.to_ascii_lowercase().contains(&needle)
                                || s.description.to_ascii_lowercase().contains(&needle)
                                || s.keywords.to_ascii_lowercase().contains(&needle)
                        })
                        .collect();
                    if items.is_empty() || y > bottom {
                        continue;
                    }
                    let shut = collapsed.iter().any(|x| x == lib);
                    let r = Rect::new(list.x + 2, y, lw - 4, 22);
                    p.button(
                        r,
                        Color::TRANSPARENT,
                        2,
                        &format!("kicad:dlg:lib:{lib}"),
                        lib,
                    );
                    p.label(
                        r.x + 4,
                        y + 2,
                        14,
                        if shut { "▸" } else { "▾" },
                        12,
                        w::MUTED,
                        false,
                        Align::Left,
                    );
                    p.label(
                        r.x + 18,
                        y + 2,
                        r.width - 20,
                        lib,
                        13,
                        w::INK,
                        true,
                        Align::Left,
                    );
                    y += 22;
                    if shut {
                        continue;
                    }
                    for s in items {
                        if y > bottom {
                            break;
                        }
                        let r = Rect::new(list.x + 18, y, lw - 20, 22);
                        let on = selected.as_deref() == Some(s.lib_id);
                        p.button(
                            r,
                            if on { c.selection } else { Color::TRANSPARENT },
                            2,
                            &format!("kicad:dlg:pick:{}", s.lib_id),
                            s.lib_id,
                        );
                        p.label(
                            r.x + 4,
                            y + 2,
                            150,
                            s.name(),
                            13,
                            w::INK,
                            false,
                            Align::Left,
                        );
                        p.label(
                            r.x + 150,
                            y + 3,
                            r.width.saturating_sub(154),
                            s.description,
                            12,
                            w::MUTED,
                            false,
                            Align::Left,
                        );
                        y += 22;
                    }
                }
                // Preview.
                let px = body.x + lw as i32 + 12;
                let prev = Rect::new(px, body.y, lw, (body.height - 42) * 2 / 3);
                p.border(prev, draw::SCH_BG, 2, w::EDGE);
                if let Some(lib) = selected.as_deref().and_then(symbols::find) {
                    let (lo, hi) = lib.bounds();
                    let view = View::fitted(
                        Pt::new(lo.0 - 100, -hi.1 - 100),
                        Pt::new(hi.0 + 100, -lo.1 + 100),
                        prev.width,
                        prev.height,
                    );
                    let cv = Cv {
                        ox: prev.x,
                        oy: prev.y,
                        w: prev.width,
                        h: prev.height,
                        view,
                    };
                    draw::symbol(p, &cv, lib, Pt::new(0, 0), Xf::IDENTITY, None, false);
                    let ty = prev.y + prev.height as i32 + 8;
                    p.label(px, ty, lw, lib.lib_id, 13, w::INK, true, Align::Left);
                    p.paragraph(px, ty + 22, lw, lib.description, 12, w::MUTED);
                    let fp = if lib.footprint.is_empty() {
                        "(no footprint)"
                    } else {
                        lib.footprint
                    };
                    p.label(
                        px,
                        ty + 58,
                        lw,
                        &format!("Footprint: {fp}"),
                        12,
                        w::MUTED,
                        false,
                        Align::Left,
                    );
                } else {
                    p.label(
                        prev.x,
                        prev.y + prev.height as i32 / 2 - 8,
                        prev.width,
                        "Select a symbol to preview it",
                        13,
                        w::MUTED,
                        false,
                        Align::Center,
                    );
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 180, by, 84, 28),
                    "Cancel",
                    "kicad:dlg:cancel",
                    false,
                );
                if selected.is_some() {
                    w::button(
                        p,
                        &c,
                        Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                        "OK",
                        "kicad:dlg:ok",
                        true,
                    );
                } else {
                    w::button_disabled(
                        p,
                        &c,
                        Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                        "OK",
                        "select a symbol first",
                    );
                }
            }
            Dialog::SymbolProperties {
                id,
                fields,
                exclude_sim,
                on_board,
                in_bom,
                dnp,
                error,
            } => {
                let body = w::dialog(p, &c, env.theme, w, h, 620, 470, dialog.title());
                p.label(body.x, body.y, 140, "Name", 12, w::MUTED, true, Align::Left);
                p.label(
                    body.x + 150,
                    body.y,
                    200,
                    "Value",
                    12,
                    w::MUTED,
                    true,
                    Align::Left,
                );
                let mut y = body.y + 22;
                for (k, v) in fields {
                    p.label(body.x, y + 5, 140, k, 13, w::INK, false, Align::Left);
                    w::field(
                        p,
                        &c,
                        Rect::new(body.x + 150, y, body.width - 150, 26),
                        v,
                        k,
                        focus == Some(k.as_str()),
                    );
                    y += 32;
                }
                // Footprints this symbol is drawn for, one click to assign.
                let lib = self.session.schematic.symbol(*id).and_then(|s| s.lib());
                if let Some(lib) = lib.filter(|l| !l.footprints.is_empty()) {
                    p.label(
                        body.x,
                        y + 4,
                        140,
                        "Assign footprint:",
                        12,
                        w::MUTED,
                        false,
                        Align::Left,
                    );
                    let mut fx = body.x + 150;
                    for fp in lib.footprints {
                        let name = fp.split(':').nth(1).unwrap_or(fp);
                        let tw = (p.measure(name, 12, false) + 16).min(body.width / 2);
                        let r = Rect::new(fx, y, tw, 24);
                        let current = fields.iter().any(|(k, v)| k == "Footprint" && v == fp);
                        p.button(
                            r,
                            if current { c.selection } else { w::WHITE },
                            3,
                            &format!("kicad:dlg:footprint:{fp}"),
                            fp,
                        );
                        p.border(r, Color::TRANSPARENT, 3, w::EDGE);
                        p.label(r.x, y + 4, tw, name, 12, w::INK, false, Align::Center);
                        fx += tw as i32 + 6;
                    }
                    y += 32;
                }
                w::checkbox(
                    p,
                    &c,
                    body.x,
                    y,
                    "Exclude from simulation",
                    *exclude_sim,
                    "kicad:dlg:toggle:exclude_sim",
                );
                w::checkbox(
                    p,
                    &c,
                    body.x + 260,
                    y,
                    "Exclude from board",
                    !*on_board,
                    "kicad:dlg:toggle:on_board",
                );
                y += 26;
                w::checkbox(
                    p,
                    &c,
                    body.x,
                    y,
                    "Exclude from bill of materials",
                    !*in_bom,
                    "kicad:dlg:toggle:in_bom",
                );
                w::checkbox(
                    p,
                    &c,
                    body.x + 260,
                    y,
                    "Do not populate",
                    *dnp,
                    "kicad:dlg:toggle:dnp",
                );
                if !error.is_empty() {
                    p.label(
                        body.x,
                        y + 30,
                        body.width,
                        error,
                        12,
                        Color::rgb(190, 30, 30),
                        false,
                        Align::Left,
                    );
                }
                ok_cancel(p, body, "OK");
            }
            Dialog::Label { text, .. } => {
                let body = w::dialog(p, &c, env.theme, w, h, 440, 170, dialog.title());
                w::label_field(
                    p,
                    &c,
                    body.x,
                    body.y,
                    80,
                    body.width - 80,
                    "Label:",
                    text,
                    "text",
                    focus == Some("text"),
                );
                ok_cancel(p, body, "OK");
            }
            Dialog::Annotate { by_x, reset, log } => {
                let body = w::dialog(p, &c, env.theme, w, h, 520, 400, dialog.title());
                p.label(body.x, body.y, 240, "Order", 13, w::INK, true, Align::Left);
                w::radio(
                    p,
                    &c,
                    body.x,
                    body.y + 22,
                    "Sort symbols by X position",
                    *by_x,
                    "kicad:dlg:order:x",
                );
                w::radio(
                    p,
                    &c,
                    body.x,
                    body.y + 46,
                    "Sort symbols by Y position",
                    !*by_x,
                    "kicad:dlg:order:y",
                );
                let ox = body.x + body.width as i32 / 2;
                p.label(ox, body.y, 240, "Options", 13, w::INK, true, Align::Left);
                w::radio(
                    p,
                    &c,
                    ox,
                    body.y + 22,
                    "Keep existing annotations",
                    !*reset,
                    "kicad:dlg:scope:keep",
                );
                w::radio(
                    p,
                    &c,
                    ox,
                    body.y + 46,
                    "Reset existing annotations",
                    *reset,
                    "kicad:dlg:scope:reset",
                );
                let list = Rect::new(body.x, body.y + 80, body.width, body.height - 122);
                p.border(list, w::WHITE, 2, w::EDGE);
                for (i, line) in log.iter().enumerate() {
                    let y = list.y + 4 + i as i32 * 18;
                    if y + 18 > list.y + list.height as i32 {
                        break;
                    }
                    p.label(
                        list.x + 6,
                        y,
                        list.width - 12,
                        line,
                        12,
                        w::INK,
                        false,
                        Align::Left,
                    );
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x, by, 130, 28),
                    "Clear Annotation",
                    "kicad:dlg:clear",
                    false,
                );
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 186, by, 90, 28),
                    "Annotate",
                    "kicad:dlg:run",
                    true,
                );
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                    "Close",
                    "kicad:dlg:ok",
                    false,
                );
            }
            Dialog::Erc { selected } => {
                let body = w::dialog(p, &c, env.theme, w, h, 640, 460, dialog.title());
                let v = self.session.erc.as_deref().unwrap_or(&[]);
                let errors = v
                    .iter()
                    .filter(|x| x.severity == erc::Severity::Error)
                    .count();
                let warnings = v.len() - errors;
                let summary = match &self.session.erc {
                    None => "ERC has not been run.".to_owned(),
                    Some(_) => format!(
                        "Violations ({})   Errors: {errors}   Warnings: {warnings}",
                        v.len()
                    ),
                };
                p.label(
                    body.x,
                    body.y,
                    body.width,
                    &summary,
                    13,
                    w::INK,
                    true,
                    Align::Left,
                );
                let list = Rect::new(body.x, body.y + 24, body.width, body.height - 66);
                p.border(list, w::WHITE, 2, w::EDGE);
                let mut y = list.y + 2;
                for (i, x) in v.iter().enumerate() {
                    let rh = 40;
                    if y + rh > list.y + list.height as i32 {
                        break;
                    }
                    let r = Rect::new(list.x + 2, y, list.width - 4, rh as u32);
                    let on = *selected == Some(i);
                    p.button(
                        r,
                        if on { c.selection } else { Color::TRANSPARENT },
                        2,
                        &format!("kicad:dlg:select:{i}"),
                        &x.message,
                    );
                    let (tag, color) = match x.severity {
                        erc::Severity::Error => ("[Error]", Color::rgb(200, 30, 30)),
                        erc::Severity::Warning => ("[Warning]", Color::rgb(190, 120, 0)),
                    };
                    p.label(r.x + 6, y + 2, 80, tag, 12, color, true, Align::Left);
                    p.label(
                        r.x + 84,
                        y + 2,
                        r.width - 90,
                        &x.message,
                        13,
                        w::INK,
                        false,
                        Align::Left,
                    );
                    p.label(
                        r.x + 84,
                        y + 20,
                        r.width - 90,
                        &x.items.join("; "),
                        12,
                        w::MUTED,
                        false,
                        Align::Left,
                    );
                    y += rh + 2;
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x, by, 150, 28),
                    "Delete All Markers",
                    "kicad:dlg:clear",
                    false,
                );
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 186, by, 90, 28),
                    "Run ERC",
                    "kicad:dlg:run",
                    true,
                );
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                    "Close",
                    "kicad:dlg:ok",
                    false,
                );
            }
            Dialog::Netlist { spice, log } => {
                let body = w::dialog(p, &c, env.theme, w, h, 520, 320, dialog.title());
                let y = w::tabs(
                    p,
                    &c,
                    body.x,
                    body.y,
                    &["KiCad", "Spice"],
                    usize::from(*spice),
                    "kicad:dlg:format:",
                );
                let _ = y;
                // Tab targets carry an index; map it back to the format names.
                let name = self
                    .session
                    .project
                    .as_ref()
                    .map(|p| p.file(if *spice { "cir" } else { "net" }))
                    .unwrap_or_default();
                p.label(
                    body.x,
                    body.y + 40,
                    body.width,
                    &format!("Output file: {name}"),
                    12,
                    w::MUTED,
                    false,
                    Align::Left,
                );
                for (i, line) in log.iter().enumerate() {
                    p.label(
                        body.x,
                        body.y + 68 + i as i32 * 18,
                        body.width,
                        line,
                        12,
                        w::INK,
                        false,
                        Align::Left,
                    );
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 216, by, 120, 28),
                    "Export Netlist",
                    "kicad:dlg:export",
                    true,
                );
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                    "Close",
                    "kicad:dlg:cancel",
                    false,
                );
            }
            Dialog::Bom { log } => {
                let body = w::dialog(p, &c, env.theme, w, h, 640, 420, dialog.title());
                let cols = [
                    (0, "Reference"),
                    (150, "Value"),
                    (260, "Footprint"),
                    (560, "Qty"),
                ];
                for (x, t) in cols {
                    p.label(body.x + x, body.y, 120, t, 12, w::MUTED, true, Align::Left);
                }
                let rows = netlist::bom_rows(&self.session.schematic);
                for (i, (refs, value, _, fp, _)) in rows.iter().enumerate() {
                    let y = body.y + 20 + i as i32 * 20;
                    if y > body.y + body.height as i32 - 110 {
                        break;
                    }
                    p.label(
                        body.x,
                        y,
                        145,
                        &refs.join(","),
                        12,
                        w::INK,
                        false,
                        Align::Left,
                    );
                    p.label(body.x + 150, y, 105, value, 12, w::INK, false, Align::Left);
                    p.label(
                        body.x + 260,
                        y,
                        295,
                        fp.split(':').nth(1).unwrap_or(fp),
                        12,
                        w::INK,
                        false,
                        Align::Left,
                    );
                    p.label(
                        body.x + 560,
                        y,
                        40,
                        &refs.len().to_string(),
                        12,
                        w::INK,
                        false,
                        Align::Left,
                    );
                }
                let name = self
                    .session
                    .project
                    .as_ref()
                    .map(|p| p.file("csv"))
                    .unwrap_or_default();
                p.label(
                    body.x,
                    body.y + body.height as i32 - 80,
                    body.width,
                    &format!("Output file: {name}"),
                    12,
                    w::MUTED,
                    false,
                    Align::Left,
                );
                for (i, line) in log.iter().rev().take(1).enumerate() {
                    p.label(
                        body.x,
                        body.y + body.height as i32 - 60 + i as i32 * 18,
                        body.width,
                        line,
                        12,
                        w::INK,
                        false,
                        Align::Left,
                    );
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 186, by, 90, 28),
                    "Export",
                    "kicad:dlg:export",
                    true,
                );
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                    "Close",
                    "kicad:dlg:cancel",
                    false,
                );
            }
            _ => {}
        }
    }

    pub(super) fn sch_page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        for (id, label) in [
            ("kicad:sch:save", "Save"),
            ("kicad:sch:tool:select", "Select"),
            ("kicad:sch:tool:symbol", "Add Symbol"),
            ("kicad:sch:tool:power", "Add Power"),
            ("kicad:sch:tool:wire", "Add Wire"),
            ("kicad:sch:tool:label", "Add Label"),
            ("kicad:sch:tool:global", "Add Global Label"),
            ("kicad:sch:tool:noconnect", "Add No Connect"),
            ("kicad:sch:tool:junction", "Add Junction"),
            ("kicad:sch:annotate", "Annotate"),
            ("kicad:sch:erc", "ERC"),
            ("kicad:sch:netlist", "Export Netlist"),
            ("kicad:sch:bom", "Generate BOM"),
            ("kicad:sch:simulator", "Simulator"),
            ("kicad:sch:update-pcb", "Update PCB from Schematic"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
        let s = &self.session.schematic;
        for sym in &s.symbols {
            page.elements.push(E::Text {
                id: format!("kicad-symbol-{}", sym.id),
                text: format!(
                    "{} {} ({}) at {}, {} mils",
                    sym.reference(),
                    sym.value(),
                    sym.lib_id,
                    sym.pos.x,
                    sym.pos.y
                ),
            });
        }
        page.elements.push(E::Text {
            id: "kicad-wires".into(),
            text: format!(
                "{} wires, {} labels, {} junctions",
                s.wires.len(),
                s.labels.len(),
                s.junctions.len()
            ),
        });
        if let Some(v) = &self.session.erc {
            for (i, x) in v.iter().enumerate() {
                page.elements.push(E::Text {
                    id: format!("kicad-erc-{i}"),
                    text: format!("{:?}: {} ({})", x.severity, x.message, x.items.join("; ")),
                });
            }
        }
        if let Some(Dialog::Chooser { power, .. }) = &self.ui.dialog {
            for lib in symbols::library().iter().filter(|l| l.power == *power) {
                let id = format!("kicad:dlg:pick:{}", lib.lib_id);
                page.elements.push(E::Button {
                    id: id.clone(),
                    text: lib.lib_id.into(),
                    action: act(&id),
                });
            }
        }
    }
}

fn describe_selection(k: &Kicad) -> String {
    let sel = k.selected();
    match sel.as_slice() {
        [] => String::new(),
        [Item::Symbol(id)] => k
            .session
            .schematic
            .symbol(*id)
            .map(|s| format!("{} {} ({})", s.reference(), s.value(), s.lib_id))
            .unwrap_or_default(),
        items => format!("{} items selected", items.len()),
    }
}

/// The world's date, as a netlist's `(date …)` records when it was exported.
pub(super) fn date(clock_us: u64) -> String {
    let d = crate::desktop_scene::shared::CalendarDate::from_clock(clock_us);
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

fn sch_menus(k: &Kicad) -> Vec<(&'static str, Vec<MenuItem>)> {
    let has_sel = !k.selected().is_empty();
    let sel = |t: &str| -> Result<String, &'static str> {
        if has_sel {
            Ok(t.to_owned())
        } else {
            Err("nothing is selected")
        }
    };
    let proj = |t: &str| -> Result<String, &'static str> {
        if k.session.project.is_some() {
            Ok(t.to_owned())
        } else {
            Err("no project is open")
        }
    };
    vec![
        (
            "File",
            vec![
                MenuItem::new("Save", "Ctrl+S", proj("kicad:sch:save")),
                MenuItem::new("Export Netlist...", "", proj("kicad:sch:netlist")).sep(),
                MenuItem::new("Generate Bill of Materials...", "", proj("kicad:sch:bom")),
            ],
        ),
        (
            "Edit",
            vec![
                MenuItem::new(
                    "Undo",
                    "Ctrl+Z",
                    if k.session.sch_history.past.is_empty() {
                        Err("nothing to undo")
                    } else {
                        Ok("kicad:sch:undo".into())
                    },
                ),
                MenuItem::new(
                    "Redo",
                    "Ctrl+Y",
                    if k.session.sch_history.future.is_empty() {
                        Err("nothing to redo")
                    } else {
                        Ok("kicad:sch:redo".into())
                    },
                ),
                MenuItem::new("Delete", "Del", sel("kicad:sch:delete")).sep(),
                MenuItem::new("Properties...", "E", sel("kicad:sch:properties")),
                MenuItem::new("Rotate", "R", sel("kicad:sch:rotate")).sep(),
                MenuItem::new("Mirror Vertically", "X", sel("kicad:sch:mirror-x")),
                MenuItem::new("Mirror Horizontally", "Y", sel("kicad:sch:mirror-y")),
            ],
        ),
        (
            "View",
            vec![
                MenuItem::new("Zoom In", "F1", Ok("kicad:sch:zoom:in".into())),
                MenuItem::new("Zoom Out", "F2", Ok("kicad:sch:zoom:out".into())),
                MenuItem::new("Zoom to Fit", "Home", Ok("kicad:sch:zoom:fit".into())),
                MenuItem::new(
                    "Zoom to Objects",
                    "Ctrl+Home",
                    if k.session.schematic.symbols.is_empty()
                        && k.session.schematic.wires.is_empty()
                    {
                        Err("the sheet is empty")
                    } else {
                        Ok("kicad:sch:zoom:objects".into())
                    },
                ),
                MenuItem::new("Show Grid", "", Ok("kicad:sch:grid".into())).sep(),
            ],
        ),
        (
            "Place",
            vec![
                MenuItem::new("Add Symbol", "A", Ok("kicad:sch:tool:symbol".into())),
                MenuItem::new("Add Power", "P", Ok("kicad:sch:tool:power".into())),
                MenuItem::new("Add Wire", "W", Ok("kicad:sch:tool:wire".into())).sep(),
                MenuItem::new(
                    "Add No Connect Flag",
                    "Q",
                    Ok("kicad:sch:tool:noconnect".into()),
                ),
                MenuItem::new("Add Junction", "J", Ok("kicad:sch:tool:junction".into())),
                MenuItem::new("Add Label", "L", Ok("kicad:sch:tool:label".into())).sep(),
                MenuItem::new(
                    "Add Global Label",
                    "Ctrl+L",
                    Ok("kicad:sch:tool:global".into()),
                ),
            ],
        ),
        (
            "Inspect",
            vec![
                MenuItem::new("Electrical Rules Checker", "", Ok("kicad:sch:erc".into())),
                MenuItem::new("Simulator", "", proj("kicad:sch:simulator")).sep(),
            ],
        ),
        (
            "Tools",
            vec![
                MenuItem::new(
                    "Update PCB from Schematic...",
                    "F8",
                    proj("kicad:sch:update-pcb"),
                ),
                MenuItem::new("Switch to PCB Editor", "", proj("kicad:sch:pcb")),
                MenuItem::new("Annotate Schematic...", "", Ok("kicad:sch:annotate".into())).sep(),
            ],
        ),
        (
            "Help",
            vec![MenuItem::new("About KiCad", "", Ok("kicad:about".into()))],
        ),
    ]
}
