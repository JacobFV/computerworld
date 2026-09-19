//! The Symbol Editor: the project's own symbol libraries. A symbol's pins (electrical
//! type, number, name, orientation, length, unit), body graphics (rectangles, circles,
//! polylines, text), fields, units and simulation model, saved to `<lib>.kicad_sym` in
//! KiCad's syntax and listed in `sym-lib-table`, so the schematic's Add Symbol offers
//! them and placed copies follow the library when it is saved.
use super::draw::{self, Cv};
use super::widgets::{self as w, chrome, MenuItem};
use super::{act, icons, Dialog, Drag, Kicad, SymLib, View};
use crate::desktop_scene::{shared::Align, Painter};
use crate::{AppEffect, PointerPhase};
use cw_eda::geom::{Pt, Xf};
use cw_eda::symbols::{Fill, Graphic, LibPin, LibSymbol, LogicGate, PinType, Spice};
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

/// Width of the library tree on the left.
const LIB_W: u32 = 230;
const MIN_ZOOM: i64 = 20;
const MAX_ZOOM: i64 = 4000;
const UNDO_LIMIT: usize = 50;
/// Tools of the right toolbar: name, icon, tooltip.
const TOOLS: [(&str, w::Icon, &str); 6] = [
    ("select", icons::select, "Select item(s)"),
    ("pin", icons::pin, "Add a pin (P)"),
    ("rect", icons::rect, "Add a rectangle"),
    ("circle", icons::circle, "Add a circle"),
    ("line", icons::line, "Add a polyline (double-click ends it)"),
    ("text", icons::text, "Add text"),
];

/// What is selected in the symbol: a pin, or a graphic common to every unit or
/// belonging to one unit (indices into `graphics` and `unit_graphics`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum SymSel {
    Pin(usize),
    Common(usize),
    Unit(usize),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymEdUi {
    /// The library shown in the tree and edited.
    pub lib: Option<String>,
    /// The symbol open on the canvas (its `Lib:Name`).
    pub symbol: Option<String>,
    /// The unit shown and drawn into (1-based).
    pub unit: u32,
    /// `select`, `pin`, `rect`, `circle`, `line`, `text`.
    pub tool: String,
    pub selection: Option<SymSel>,
    /// Clicks of the shape being drawn, library coordinates (Y up).
    pub points: Vec<(i64, i64)>,
    /// Earlier states of the open symbol, for Undo, and undone ones for Redo.
    pub undo: Vec<LibSymbol>,
    pub redo: Vec<LibSymbol>,
}

/// World (canvas, Y down) to library coordinates (Y up), and back.
fn lib_pt(p: Pt) -> (i64, i64) {
    (p.x, -p.y)
}
fn world(p: (i64, i64)) -> Pt {
    Pt::new(p.0, -p.1)
}
fn canvas_rect(width: u32, height: u32) -> Rect {
    let top = (w::MENU_H + w::TOOL_H) as i32;
    Rect::new(
        LIB_W as i32 + 1,
        top,
        width.saturating_sub(LIB_W + w::SIDE_W + 2),
        height.saturating_sub(w::MENU_H + w::TOOL_H + w::STATUS_H),
    )
}
/// Distance from `p` to the segment `a`–`b`, in whole units.
fn seg_dist(p: (i64, i64), a: (i64, i64), b: (i64, i64)) -> i64 {
    let (dx, dy) = ((b.0 - a.0) as i128, (b.1 - a.1) as i128);
    let (px, py) = ((p.0 - a.0) as i128, (p.1 - a.1) as i128);
    let len2 = dx * dx + dy * dy;
    let (qx, qy) = if len2 == 0 {
        (px, py)
    } else {
        let t = (px * dx + py * dy).clamp(0, len2);
        (px - dx * t / len2, py - dy * t / len2)
    };
    ((qx * qx + qy * qy) as u128).isqrt() as i64
}
fn isqrt(v: i64) -> i64 {
    (v.max(0) as u64).isqrt() as i64
}
/// A graphic moved by `d` (library coordinates).
fn moved(g: &Graphic, d: (i64, i64)) -> Graphic {
    let m = |p: (i64, i64)| (p.0 + d.0, p.1 + d.1);
    match g.clone() {
        Graphic::Rect { a, b, fill } => Graphic::Rect {
            a: m(a),
            b: m(b),
            fill,
        },
        Graphic::Poly { pts, fill, width } => Graphic::Poly {
            pts: pts.into_iter().map(m).collect(),
            fill,
            width,
        },
        Graphic::Circle { c, r, fill } => Graphic::Circle { c: m(c), r, fill },
        Graphic::Arc { start, mid, end } => Graphic::Arc {
            start: m(start),
            mid: m(mid),
            end: m(end),
        },
        Graphic::Text { at, text } => Graphic::Text { at: m(at), text },
    }
}
/// How near (library units) `p` is to a graphic's strokes; inside a filled one is 0.
fn graphic_dist(g: &Graphic, p: (i64, i64)) -> i64 {
    match g {
        Graphic::Rect { a, b, fill } => {
            let (lo, hi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
            if *fill != Fill::None && p.0 >= lo.0 && p.0 <= hi.0 && p.1 >= lo.1 && p.1 <= hi.1 {
                return 0;
            }
            let c = [lo, (hi.0, lo.1), hi, (lo.0, hi.1), lo];
            c.windows(2)
                .map(|s| seg_dist(p, s[0], s[1]))
                .min()
                .unwrap_or(i64::MAX)
        }
        Graphic::Poly { pts, .. } => pts
            .windows(2)
            .map(|s| seg_dist(p, s[0], s[1]))
            .min()
            .unwrap_or(i64::MAX),
        Graphic::Circle { c, r, fill } => {
            let d = isqrt((p.0 - c.0).pow(2) + (p.1 - c.1).pow(2));
            if *fill != Fill::None && d <= *r {
                0
            } else {
                (d - r).abs()
            }
        }
        Graphic::Arc { start, mid, end } => seg_dist(p, *start, *mid).min(seg_dist(p, *mid, *end)),
        Graphic::Text { at, text } => {
            // Text is 50 mils high and about 30 wide per character, centred.
            let half = 15 * text.chars().count() as i64;
            if (p.0 - at.0).abs() <= half.max(25) && (p.1 - at.1).abs() <= 30 {
                0
            } else {
                i64::MAX
            }
        }
    }
}
/// The simulation model keyword of the symbol editor's Model choice.
pub(super) fn model_keyword(s: Spice) -> &'static str {
    match s {
        Spice::None | Spice::Power => "none",
        Spice::Resistor => "R",
        Spice::Capacitor => "C",
        Spice::Inductor => "L",
        Spice::Diode => "D",
        Spice::Npn => "NPN",
        Spice::Pnp => "PNP",
        Spice::Nmos => "NMOS",
        Spice::Pmos => "PMOS",
        Spice::OpAmp => "OPAMP",
        Spice::VoltageSource => "V",
        Spice::CurrentSource => "I",
        Spice::Gate(LogicGate::And) => "AND",
        Spice::Gate(LogicGate::Nand) => "NAND",
        Spice::Gate(LogicGate::Or) => "OR",
        Spice::Gate(LogicGate::Nor) => "NOR",
        Spice::Gate(LogicGate::Xor) => "XOR",
        Spice::Gate(LogicGate::Not) => "NOT",
        Spice::DFlipFlop => "DFF",
        Spice::Timer555 => "555",
        Spice::Mcu => "MCU",
    }
}
pub(super) fn model_from(k: &str) -> Option<Spice> {
    Some(match k {
        "none" => Spice::None,
        "R" => Spice::Resistor,
        "C" => Spice::Capacitor,
        "L" => Spice::Inductor,
        "D" => Spice::Diode,
        "NPN" => Spice::Npn,
        "PNP" => Spice::Pnp,
        "NMOS" => Spice::Nmos,
        "PMOS" => Spice::Pmos,
        "OPAMP" => Spice::OpAmp,
        "V" => Spice::VoltageSource,
        "I" => Spice::CurrentSource,
        "AND" => Spice::Gate(LogicGate::And),
        "NAND" => Spice::Gate(LogicGate::Nand),
        "OR" => Spice::Gate(LogicGate::Or),
        "NOR" => Spice::Gate(LogicGate::Nor),
        "XOR" => Spice::Gate(LogicGate::Xor),
        "NOT" => Spice::Gate(LogicGate::Not),
        "DFF" => Spice::DFlipFlop,
        "555" => Spice::Timer555,
        "MCU" => Spice::Mcu,
        _ => return None,
    })
}
/// A library, symbol or footprint name KiCad accepts: no separators, quotes or spaces.
pub(super) fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("a name is required".into());
    }
    if let Some(c) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')))
    {
        return Err(format!("'{c}' is not allowed in a name"));
    }
    Ok(())
}
/// Keep Reference and Value clear of the body, as KiCad lays out a new symbol's fields:
/// the reference above its top-left corner, the value below its bottom-left one. A
/// field already outside the body stays where it is.
fn place_fields(s: &mut LibSymbol) {
    let mut body = LibSymbol::blank("body:body", "");
    body.graphics = s
        .graphics
        .iter()
        .chain(s.unit_graphics.iter().map(|(_, g)| g))
        .filter(|g| !matches!(g, Graphic::Text { .. }))
        .cloned()
        .collect();
    if body.graphics.is_empty() {
        return;
    }
    let ((x0, y0), (x1, y1)) = body.bounds();
    let inside = |p: (i64, i64)| p.0 >= x0 && p.0 <= x1 && p.1 >= y0 && p.1 <= y1;
    if inside(s.ref_at) {
        s.ref_at = (x0, y1 + 50);
    }
    if inside(s.value_at) {
        s.value_at = (x0, y0 - 50);
    }
}
fn orient_name(a: u16) -> &'static str {
    match a {
        0 => "right",
        90 => "up",
        180 => "left",
        _ => "down",
    }
}

impl Kicad {
    fn symed_tool(&self) -> &str {
        if self.ui.symed.tool.is_empty() {
            "select"
        } else {
            &self.ui.symed.tool
        }
    }
    fn symed_lib(&self) -> Option<&SymLib> {
        let name = self.ui.symed.lib.as_deref()?;
        self.session.sym_libs.iter().find(|l| l.name == name)
    }
    /// The symbol open on the canvas.
    pub(super) fn symed_symbol(&self) -> Option<&LibSymbol> {
        let id = self.ui.symed.symbol.as_deref()?;
        self.symed_lib()?.symbols.iter().find(|s| s.lib_id == id)
    }
    fn symed_symbol_mut(&mut self) -> Option<&mut LibSymbol> {
        let id = self.ui.symed.symbol.clone()?;
        let name = self.ui.symed.lib.clone()?;
        self.session
            .sym_libs
            .iter_mut()
            .find(|l| l.name == name)?
            .symbols
            .iter_mut()
            .find(|s| s.lib_id == id)
    }
    fn symed_unit(&self) -> u32 {
        self.ui.symed.unit.max(1)
    }
    /// Change the open symbol: the old state goes on the undo stack and the library is
    /// marked changed. `f` says what it did, for the status bar, or why it could not.
    fn symed_edit(
        &mut self,
        f: impl FnOnce(&mut LibSymbol) -> Result<String, String>,
    ) -> Result<Vec<AppEffect>, String> {
        let before = self.symed_symbol().cloned().ok_or("open a symbol first")?;
        let sym = self.symed_symbol_mut().ok_or("open a symbol first")?;
        let message = f(sym)?;
        place_fields(sym);
        let after = sym.clone();
        if after != before {
            let ui = &mut self.ui.symed;
            ui.undo.push(before);
            if ui.undo.len() > UNDO_LIMIT {
                ui.undo.remove(0);
            }
            ui.redo.clear();
            let name = self.ui.symed.lib.clone();
            if let Some(l) = self
                .session
                .sym_libs
                .iter_mut()
                .find(|l| Some(&l.name) == name.as_ref())
            {
                l.dirty = true;
            }
        }
        self.ui.status = message;
        Ok(vec![])
    }
    /// Library-coordinate hit tolerance at the current zoom.
    fn symed_tol(&self) -> i64 {
        (4000 / self.ui.view.zoom.max(1)).max(15)
    }
    /// What is under `p` (library coordinates): pins before graphics.
    fn symed_hit(&self, p: (i64, i64)) -> Option<SymSel> {
        let s = self.symed_symbol()?;
        let unit = self.symed_unit();
        let tol = self.symed_tol();
        let pin = s
            .pins
            .iter()
            .enumerate()
            .filter(|(_, pn)| pn.unit == 0 || pn.unit == unit || s.units <= 1)
            .map(|(i, pn)| (seg_dist(p, pn.at, pn.inner()), i))
            .filter(|(d, _)| *d <= tol)
            .min();
        if let Some((_, i)) = pin {
            return Some(SymSel::Pin(i));
        }
        let mut best: Option<(i64, SymSel)> = None;
        for (i, g) in s.graphics.iter().enumerate() {
            let d = graphic_dist(g, p);
            if d <= tol && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, SymSel::Common(i)));
            }
        }
        for (i, (u, g)) in s.unit_graphics.iter().enumerate() {
            if *u != unit && *u != 0 {
                continue;
            }
            let d = graphic_dist(g, p);
            if d <= tol && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, SymSel::Unit(i)));
            }
        }
        best.map(|(_, s)| s)
    }
    fn symed_selection(&self) -> Option<SymSel> {
        let sel = self.ui.symed.selection?;
        let s = self.symed_symbol()?;
        let live = match sel {
            SymSel::Pin(i) => i < s.pins.len(),
            SymSel::Common(i) => i < s.graphics.len(),
            SymSel::Unit(i) => i < s.unit_graphics.len(),
        };
        live.then_some(sel)
    }
    /// Add a finished graphic: common when the symbol has one unit, else the current
    /// unit's own.
    fn symed_add_graphic(&mut self, g: Graphic, what: &str) -> Result<Vec<AppEffect>, String> {
        let unit = self.symed_unit();
        let what = what.to_owned();
        self.symed_edit(move |s| {
            if s.units > 1 {
                s.unit_graphics.push((unit, g));
                Ok(format!(
                    "Added {what} to unit {}",
                    LibSymbol::unit_letter(unit)
                ))
            } else {
                s.graphics.push(g);
                Ok(format!("Added {what}"))
            }
        })
    }
    fn symed_open(&mut self, lib_id: &str) -> Result<Vec<AppEffect>, String> {
        let lib = lib_id.split(':').next().unwrap_or("");
        let found = self
            .session
            .sym_libs
            .iter()
            .any(|l| l.name == lib && l.symbols.iter().any(|s| s.lib_id == lib_id));
        if !found {
            return Err(format!("{lib_id} is not in a project library"));
        }
        let ui = &mut self.ui.symed;
        ui.lib = Some(lib.to_owned());
        ui.symbol = Some(lib_id.to_owned());
        ui.unit = 1;
        ui.selection = None;
        ui.points.clear();
        ui.undo.clear();
        ui.redo.clear();
        self.ui.view.fit = true;
        self.ui.status = format!("Editing {lib_id}");
        Ok(vec![])
    }
    fn symed_view(&self, cw: u32, ch: u32) -> View {
        if !self.ui.view.fit && self.ui.view.zoom != 0 {
            return self.ui.view;
        }
        let ((x0, y0), (x1, y1)) = self
            .symed_symbol()
            .map(|s| s.bounds())
            .unwrap_or(((0, 0), (0, 0)));
        let m = 300;
        View::fitted(
            Pt::new(x0.min(-200) - m, -y1.max(200) - m),
            Pt::new(x1.max(200) + m, -y0.min(-200) + m),
            cw,
            ch,
        )
    }
    fn symed_zoom(&mut self, dir: &str, args: &[&str]) -> Result<Vec<AppEffect>, String> {
        let (cw, ch) = match (
            args.get(3).and_then(|v| v.parse().ok()),
            args.get(4).and_then(|v| v.parse().ok()),
        ) {
            (Some(w), Some(h)) => (w, h),
            _ if self.ui.canvas.0 > 0 => self.ui.canvas,
            _ => (800, 600),
        };
        self.ui.canvas = (cw, ch);
        let mut v = View::from_args(args).unwrap_or_else(|| self.symed_view(cw, ch));
        let centre = match (args.len() >= 3, self.ui.hover) {
            (false, Some(h)) => h,
            _ => v.world(cw as i32 / 2, ch as i32 / 2),
        };
        match dir {
            "fit" => {
                self.ui.view.fit = true;
                return Ok(vec![]);
            }
            "in" => v.zoom_about(centre, 3, 2, MIN_ZOOM, MAX_ZOOM),
            "out" => v.zoom_about(centre, 2, 3, MIN_ZOOM, MAX_ZOOM),
            other => return Err(format!("unknown zoom {other}")),
        }
        self.ui.view = v;
        Ok(vec![])
    }
    /// The pin dialog for a new pin at `pos`, or for pin `edit`.
    fn open_pin_dialog(&mut self, edit: Option<usize>, pos: (i64, i64)) -> Result<(), String> {
        let s = self.symed_symbol().ok_or("open a symbol first")?;
        let unit = self.symed_unit();
        let (name, number, length, kind, orient, pin_unit) = match edit {
            Some(i) => {
                let p = s.pins.get(i).ok_or("no such pin")?;
                (
                    p.name.clone(),
                    p.number.clone(),
                    p.length,
                    p.kind,
                    p.angle,
                    p.unit,
                )
            }
            None => {
                // The next free number, and the last pin's type and length.
                let next = s
                    .pins
                    .iter()
                    .filter_map(|p| p.number.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0)
                    + 1;
                let last = s.pins.last();
                (
                    "~".to_owned(),
                    next.to_string(),
                    last.map_or(100, |p| p.length),
                    last.map_or(PinType::Input, |p| p.kind),
                    last.map_or(0, |p| p.angle),
                    if s.units > 1 { unit } else { 0 },
                )
            }
        };
        let mut fields = vec![
            ("Name".to_owned(), name),
            ("Number".to_owned(), number),
            ("Length (mils)".to_owned(), length.to_string()),
        ];
        if s.units > 1 {
            fields.push(("Unit (0 = all)".to_owned(), pin_unit.to_string()));
        }
        let at = match edit {
            Some(i) => s.pins[i].at,
            None => pos,
        };
        self.ui.dialog = Some(Dialog::PinProperties {
            edit,
            pos: at,
            kind,
            orient,
            fields,
            error: String::new(),
        });
        self.ui.focus = Some("Name".into());
        Ok(())
    }
    fn open_symbol_fields(&mut self) -> Result<(), String> {
        let s = self.symed_symbol().ok_or("open a symbol first")?;
        let fields = vec![
            ("Reference".to_owned(), s.reference.clone()),
            ("Value".to_owned(), s.value.clone()),
            ("Footprint".to_owned(), s.footprint.clone()),
            ("Datasheet".to_owned(), s.datasheet.clone()),
            ("Description".to_owned(), s.description.clone()),
            ("Keywords".to_owned(), s.keywords.clone()),
            ("Units".to_owned(), s.units.max(1).to_string()),
            ("Sim.Params".to_owned(), s.sim_params.clone()),
        ];
        self.ui.dialog = Some(Dialog::SymbolFields {
            fields,
            model: model_keyword(s.spice).to_owned(),
            power: s.power,
            pin_names_hidden: s.pin_names_hidden,
            pin_numbers_hidden: s.pin_numbers_hidden,
            error: String::new(),
        });
        self.ui.focus = Some("Value".into());
        Ok(())
    }
    /// Finish the polyline being drawn.
    fn symed_finish_line(&mut self) -> Result<Vec<AppEffect>, String> {
        let pts = std::mem::take(&mut self.ui.symed.points);
        if pts.len() < 2 {
            return Err("a polyline needs two points".into());
        }
        self.symed_add_graphic(
            Graphic::Poly {
                pts,
                fill: Fill::None,
                width: 10,
            },
            "a polyline",
        )
    }
    fn symed_delete(&mut self) -> Result<Vec<AppEffect>, String> {
        let sel = self.symed_selection().ok_or("nothing is selected")?;
        self.ui.symed.selection = None;
        self.symed_edit(move |s| {
            Ok(match sel {
                SymSel::Pin(i) => {
                    let p = s.pins.remove(i);
                    format!("Deleted pin {}", p.number)
                }
                SymSel::Common(i) => {
                    s.graphics.remove(i);
                    "Deleted graphic".into()
                }
                SymSel::Unit(i) => {
                    s.unit_graphics.remove(i);
                    "Deleted graphic".into()
                }
            })
        })
    }
    /// Save the library, then bring placed copies in the schematic up to date.
    fn symed_save(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let name = self.ui.symed.lib.clone().ok_or("choose a library first")?;
        let effects = self.save_sym_lib(window, &name)?;
        let defs = self
            .session
            .sym_libs
            .iter()
            .find(|l| l.name == name)
            .map(|l| l.symbols.clone())
            .unwrap_or_default();
        let mut updated = 0;
        for d in &defs {
            updated += self.session.schematic.update_local_symbol(d);
        }
        if updated > 0 {
            self.session.sch_dirty = true;
            self.session.erc = None;
        }
        self.ui.status = if updated > 0 {
            format!("Saved library {name}; updated {updated} placed symbol(s)")
        } else {
            format!("Saved library {name}")
        };
        Ok(effects)
    }

    pub(super) fn symed_key(&mut self, window: u64, key: &str) -> Result<Vec<AppEffect>, String> {
        let cmd = match key {
            "Ctrl+s" | "Meta+s" => "save",
            "Ctrl+z" | "Meta+z" => "undo",
            "Ctrl+y" | "Meta+y" | "Ctrl+Shift+z" | "Meta+Shift+z" => "redo",
            "Ctrl+n" | "Meta+n" => "new-symbol",
            "Delete" | "Backspace" => "delete",
            "Escape" => "cancel",
            "p" | "P" => "tool:pin",
            "r" | "R" => "rotate",
            "e" | "E" => "edit",
            "Home" => "zoom:fit",
            "F1" => "zoom:in",
            "F2" => "zoom:out",
            other => return Err(format!("{other} does nothing in the Symbol Editor")),
        };
        self.symed_command(window, cmd)
    }
    pub(super) fn symed_activate(&mut self, _window: u64) -> Result<Vec<AppEffect>, String> {
        if self.symed_tool() == "line" && !self.ui.symed.points.is_empty() {
            return self.symed_finish_line();
        }
        let at = self
            .ui
            .hover
            .map(lib_pt)
            .ok_or("point at something first")?;
        match self.symed_hit(at) {
            Some(SymSel::Pin(i)) => {
                self.ui.symed.selection = Some(SymSel::Pin(i));
                self.open_pin_dialog(Some(i), at)?;
            }
            Some(sel @ (SymSel::Common(_) | SymSel::Unit(_))) => {
                let s = self.symed_symbol().ok_or("open a symbol first")?;
                let g = match sel {
                    SymSel::Common(i) => &s.graphics[i],
                    SymSel::Unit(i) => &s.unit_graphics[i].1,
                    SymSel::Pin(_) => unreachable!("pins were handled above"),
                };
                if let Graphic::Text { at, text } = g {
                    self.ui.dialog = Some(Dialog::SymText {
                        pos: *at,
                        fields: vec![("Text".into(), text.clone())],
                    });
                    self.ui.focus = Some("Text".into());
                } else {
                    self.ui.symed.selection = Some(sel);
                }
            }
            None => self.open_symbol_fields()?,
        }
        Ok(vec![])
    }
    pub(super) fn symed_pointer(
        &mut self,
        _window: u64,
        phase: PointerPhase,
        at: Pt,
    ) -> Result<Vec<AppEffect>, String> {
        let snapped = at.snap(cw_eda::schematic::GRID);
        self.ui.hover = Some(snapped);
        let lp = lib_pt(snapped);
        if self.symed_symbol().is_none() {
            return match phase {
                PointerPhase::Down => Err("open or create a symbol first".into()),
                _ => Ok(vec![]),
            };
        }
        match (self.symed_tool().to_owned().as_str(), phase) {
            ("select", PointerPhase::Down) => {
                let hit = self.symed_hit(lib_pt(at));
                self.ui.symed.selection = hit;
                self.ui.drag = Some(match hit {
                    Some(_) => Drag::Move {
                        start: snapped,
                        at: snapped,
                    },
                    None => Drag::Press { at: snapped },
                });
                Ok(vec![])
            }
            ("select", PointerPhase::Move) => {
                if let Some(Drag::Move { at, .. }) = &mut self.ui.drag {
                    *at = snapped;
                }
                Ok(vec![])
            }
            ("select", PointerPhase::Up) => {
                let drag = self.ui.drag.take();
                let (Some(Drag::Move { start, .. }), Some(sel)) = (drag, self.symed_selection())
                else {
                    return Ok(vec![]);
                };
                let d = lib_pt(snapped.sub(start));
                if d == (0, 0) {
                    return Ok(vec![]);
                }
                self.symed_edit(move |s| {
                    match sel {
                        SymSel::Pin(i) => {
                            let p = &mut s.pins[i];
                            p.at = (p.at.0 + d.0, p.at.1 + d.1);
                        }
                        SymSel::Common(i) => s.graphics[i] = moved(&s.graphics[i], d),
                        SymSel::Unit(i) => {
                            let (u, g) = &s.unit_graphics[i];
                            s.unit_graphics[i] = (*u, moved(g, d));
                        }
                    }
                    Ok(format!("Moved by {} mils, {} mils", d.0, d.1))
                })
            }
            (_, PointerPhase::Cancel) => {
                self.ui.drag = None;
                self.ui.symed.points.clear();
                Ok(vec![])
            }
            ("pin", PointerPhase::Up) => {
                self.open_pin_dialog(None, lp)?;
                Ok(vec![])
            }
            ("text", PointerPhase::Up) => {
                self.ui.dialog = Some(Dialog::SymText {
                    pos: lp,
                    fields: vec![("Text".into(), String::new())],
                });
                self.ui.focus = Some("Text".into());
                Ok(vec![])
            }
            ("rect", PointerPhase::Up) => match self.ui.symed.points.first().copied() {
                None => {
                    self.ui.symed.points = vec![lp];
                    self.ui.status = "Click the opposite corner".into();
                    Ok(vec![])
                }
                Some(a) => {
                    self.ui.symed.points.clear();
                    if a.0 == lp.0 || a.1 == lp.1 {
                        return Err("a rectangle needs width and height".into());
                    }
                    self.symed_add_graphic(
                        Graphic::Rect {
                            a,
                            b: lp,
                            fill: Fill::Background,
                        },
                        "a rectangle",
                    )
                }
            },
            ("circle", PointerPhase::Up) => match self.ui.symed.points.first().copied() {
                None => {
                    self.ui.symed.points = vec![lp];
                    self.ui.status = "Click a point on the circle".into();
                    Ok(vec![])
                }
                Some(c) => {
                    self.ui.symed.points.clear();
                    let r = isqrt((lp.0 - c.0).pow(2) + (lp.1 - c.1).pow(2));
                    if r == 0 {
                        return Err("a circle needs a radius".into());
                    }
                    self.symed_add_graphic(
                        Graphic::Circle {
                            c,
                            r,
                            fill: Fill::None,
                        },
                        "a circle",
                    )
                }
            },
            ("line", PointerPhase::Up) => {
                if self.ui.symed.points.last() != Some(&lp) {
                    self.ui.symed.points.push(lp);
                }
                self.ui.status = "Click the next point; double-click or Escape ends".into();
                Ok(vec![])
            }
            _ => Ok(vec![]),
        }
    }
    pub(super) fn symed_command(
        &mut self,
        window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let parts: Vec<&str> = rest.split(':').collect();
        match parts[0] {
            "new-lib" => {
                if self.session.project.is_none() {
                    return Err("open a project first: libraries belong to it".into());
                }
                self.ui.dialog = Some(Dialog::NewLibrary {
                    fp: false,
                    fields: vec![("Name".into(), String::new())],
                    error: String::new(),
                });
                self.ui.focus = Some("Name".into());
                Ok(vec![])
            }
            "lib" => {
                let name = rest.strip_prefix("lib:").unwrap_or("");
                if !self.session.sym_libs.iter().any(|l| l.name == name) {
                    return Err(format!("no project library {name}"));
                }
                self.ui.symed.lib = Some(name.to_owned());
                Ok(vec![])
            }
            "open" => self.symed_open(rest.strip_prefix("open:").unwrap_or("")),
            "new-symbol" => {
                if self.symed_lib().is_none() {
                    return Err("choose or create a library first".into());
                }
                self.ui.dialog = Some(Dialog::NewSymbol {
                    fields: vec![
                        ("Name".into(), String::new()),
                        ("Reference".into(), "U".into()),
                        ("Units".into(), "1".into()),
                    ],
                    error: String::new(),
                });
                self.ui.focus = Some("Name".into());
                Ok(vec![])
            }
            "delete-symbol" => {
                let id = self.ui.symed.symbol.clone().ok_or("open a symbol first")?;
                let name = self.ui.symed.lib.clone().ok_or("choose a library first")?;
                let lib = self
                    .session
                    .sym_libs
                    .iter_mut()
                    .find(|l| l.name == name)
                    .ok_or("no such library")?;
                lib.symbols.retain(|s| s.lib_id != id);
                lib.dirty = true;
                self.ui.symed.symbol = None;
                self.ui.symed.undo.clear();
                self.ui.symed.redo.clear();
                self.ui.status = format!("Deleted {id} from {name} (save to keep it deleted)");
                Ok(vec![])
            }
            "tool" => {
                let t = parts.get(1).copied().unwrap_or("select");
                if !TOOLS.iter().any(|(n, _, _)| *n == t) {
                    return Err(format!("unknown tool {t}"));
                }
                if t != "select" && self.symed_symbol().is_none() {
                    return Err("open or create a symbol first".into());
                }
                self.ui.symed.tool = t.to_owned();
                self.ui.symed.points.clear();
                Ok(vec![])
            }
            "unit" => {
                let n: u32 = parts
                    .get(1)
                    .and_then(|v| v.parse().ok())
                    .ok_or("bad unit")?;
                let units = self.symed_symbol().ok_or("open a symbol first")?.units;
                if n == 0 || n > units.max(1) {
                    return Err(format!("the symbol has {units} unit(s)"));
                }
                self.ui.symed.unit = n;
                self.ui.symed.selection = None;
                Ok(vec![])
            }
            "properties" => {
                self.open_symbol_fields()?;
                Ok(vec![])
            }
            "edit" => match self.symed_selection() {
                Some(SymSel::Pin(i)) => {
                    self.open_pin_dialog(Some(i), (0, 0))?;
                    Ok(vec![])
                }
                _ => {
                    self.open_symbol_fields()?;
                    Ok(vec![])
                }
            },
            "rotate" => {
                let Some(SymSel::Pin(i)) = self.symed_selection() else {
                    return Err("select a pin to rotate".into());
                };
                self.symed_edit(move |s| {
                    let p = &mut s.pins[i];
                    p.angle = (p.angle + 90) % 360;
                    Ok(format!(
                        "Pin {} now points {}",
                        p.number,
                        orient_name(p.angle)
                    ))
                })
            }
            "delete" => self.symed_delete(),
            "cancel" => {
                if self.symed_tool() == "line" && self.ui.symed.points.len() >= 2 {
                    return self.symed_finish_line();
                }
                self.ui.symed.points.clear();
                self.ui.symed.tool = "select".into();
                self.ui.symed.selection = None;
                Ok(vec![])
            }
            "finish" => self.symed_finish_line(),
            "undo" => {
                let prev = self.ui.symed.undo.pop().ok_or("nothing to undo")?;
                let cur = self.symed_symbol().cloned().ok_or("open a symbol first")?;
                self.ui.symed.redo.push(cur);
                *self.symed_symbol_mut().ok_or("open a symbol first")? = prev;
                self.ui.symed.selection = None;
                self.ui.status = "Undone".into();
                Ok(vec![])
            }
            "redo" => {
                let next = self.ui.symed.redo.pop().ok_or("nothing to redo")?;
                let cur = self.symed_symbol().cloned().ok_or("open a symbol first")?;
                self.ui.symed.undo.push(cur);
                *self.symed_symbol_mut().ok_or("open a symbol first")? = next;
                self.ui.symed.selection = None;
                self.ui.status = "Redone".into();
                Ok(vec![])
            }
            "save" => self.symed_save(window),
            "zoom" => self.symed_zoom(
                parts.get(1).copied().unwrap_or(""),
                &parts[2.min(parts.len())..],
            ),
            "schematic" => self.launch_frame(window, "sch"),
            other => Err(format!("unknown Symbol Editor command {other}")),
        }
    }
    pub(super) fn symed_dialog(
        &mut self,
        window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let dialog = self.ui.dialog.clone().ok_or("no dialog is open")?;
        let (cmd, arg) = rest.split_once(':').unwrap_or((rest, ""));
        // Choices change the dialog in place.
        if cmd != "ok" {
            let d = self.ui.dialog.as_mut().expect("checked above");
            match (d, cmd) {
                (Dialog::PinProperties { kind, .. }, "kind") => {
                    *kind = super::forms::PIN_TYPES
                        .into_iter()
                        .find(|t| t.keyword() == arg)
                        .ok_or("unknown pin type")?;
                }
                (Dialog::PinProperties { orient, .. }, "orient") => {
                    let a: u16 = arg.parse().map_err(|_| "bad orientation")?;
                    if !matches!(a, 0 | 90 | 180 | 270) {
                        return Err("bad orientation".into());
                    }
                    *orient = a;
                }
                (Dialog::SymbolFields { model, .. }, "model") => {
                    model_from(arg).ok_or("unknown model")?;
                    *model = arg.to_owned();
                }
                (
                    Dialog::SymbolFields {
                        power,
                        pin_names_hidden,
                        pin_numbers_hidden,
                        ..
                    },
                    "toggle",
                ) => match arg {
                    "power" => *power = !*power,
                    "names" => *pin_names_hidden = !*pin_names_hidden,
                    "numbers" => *pin_numbers_hidden = !*pin_numbers_hidden,
                    _ => return Err("unknown option".into()),
                },
                _ => return Err(format!("{rest} is not a control of this dialog")),
            }
            return Ok(vec![]);
        }
        let set_error = |k: &mut Kicad, e: String| {
            if let Some(
                Dialog::NewLibrary { error, .. }
                | Dialog::NewSymbol { error, .. }
                | Dialog::PinProperties { error, .. }
                | Dialog::SymbolFields { error, .. },
            ) = k.ui.dialog.as_mut()
            {
                *error = e.clone();
            }
            e
        };
        match dialog.clone() {
            Dialog::NewLibrary { .. } => {
                let name = dialog.value("Name");
                if let Err(e) = valid_name(&name) {
                    return Err(set_error(self, e));
                }
                if self.session.sym_libs.iter().any(|l| l.name == name) {
                    return Err(set_error(self, format!("a library {name} already exists")));
                }
                self.session.sym_libs.push(SymLib {
                    name: name.clone(),
                    file: format!("{name}.kicad_sym"),
                    symbols: vec![],
                    dirty: true,
                });
                self.close_dialog();
                self.ui.symed.lib = Some(name.clone());
                self.ui.symed.symbol = None;
                let effects = self.save_sym_lib(window, &name)?;
                self.ui.status = format!("Created library {name} ({name}.kicad_sym)");
                Ok(effects)
            }
            Dialog::NewSymbol { .. } => {
                let name = dialog.value("Name");
                let reference = dialog.value("Reference");
                let lib = self.ui.symed.lib.clone().ok_or("choose a library first")?;
                if let Err(e) = valid_name(&name) {
                    return Err(set_error(self, e));
                }
                if reference.is_empty()
                    || !reference
                        .chars()
                        .all(|c| c.is_ascii_alphabetic() || c == '#')
                {
                    return Err(set_error(
                        self,
                        "the reference designator is letters, such as U or R".into(),
                    ));
                }
                let units = match dialog.value("Units").parse::<u32>() {
                    Ok(n) if (1..=26).contains(&n) => n,
                    _ => return Err(set_error(self, "units is a number from 1 to 26".into())),
                };
                let lib_id = format!("{lib}:{name}");
                let l = self
                    .session
                    .sym_libs
                    .iter_mut()
                    .find(|l| l.name == lib)
                    .ok_or("no such library")?;
                if l.symbols.iter().any(|s| s.lib_id == lib_id) {
                    return Err(set_error(self, format!("{name} is already in {lib}")));
                }
                let mut s = LibSymbol::blank(&lib_id, &reference);
                s.units = units;
                l.symbols.push(s);
                l.symbols.sort_by(|a, b| a.lib_id.cmp(&b.lib_id));
                l.dirty = true;
                self.close_dialog();
                self.symed_open(&lib_id)?;
                self.ui.status = format!("New symbol {lib_id}: add pins with P");
                Ok(vec![])
            }
            Dialog::PinProperties {
                edit,
                pos,
                kind,
                orient,
                ..
            } => {
                let name = dialog.value("Name");
                let number = dialog.value("Number");
                let name = if name.is_empty() {
                    "~".to_owned()
                } else {
                    name
                };
                if number.is_empty() || number.contains(char::is_whitespace) {
                    return Err(set_error(
                        self,
                        "a pin needs a number without spaces".into(),
                    ));
                }
                let length = match dialog.value("Length (mils)").parse::<i64>() {
                    Ok(v) if (0..=5000).contains(&v) => v,
                    _ => {
                        return Err(set_error(
                            self,
                            "length is a whole number of mils from 0 to 5000".into(),
                        ))
                    }
                };
                let s = self.symed_symbol().ok_or("open a symbol first")?;
                let unit = if s.units > 1 {
                    match dialog.value("Unit (0 = all)").parse::<u32>() {
                        Ok(u) if u <= s.units => u,
                        _ => {
                            return Err(set_error(self, format!("unit is 0 (all) to {}", s.units)))
                        }
                    }
                } else {
                    0
                };
                let clash = s.pins.iter().enumerate().any(|(i, p)| {
                    Some(i) != edit
                        && p.number == number
                        && (p.unit == 0 || unit == 0 || p.unit == unit)
                });
                if clash {
                    return Err(set_error(
                        self,
                        format!("pin number {number} is already used"),
                    ));
                }
                self.close_dialog();
                let pin = LibPin {
                    number: number.clone(),
                    name,
                    at: pos,
                    angle: orient,
                    length,
                    kind,
                    hidden: false,
                    unit,
                };
                self.symed_edit(move |s| {
                    Ok(match edit {
                        Some(i) => {
                            let hidden = s.pins[i].hidden;
                            s.pins[i] = LibPin { hidden, ..pin };
                            format!("Changed pin {number}")
                        }
                        None => {
                            s.pins.push(pin);
                            format!("Added pin {number}")
                        }
                    })
                })
            }
            Dialog::SymbolFields {
                model,
                power,
                pin_names_hidden,
                pin_numbers_hidden,
                ..
            } => {
                let units = match dialog.value("Units").parse::<u32>() {
                    Ok(n) if (1..=26).contains(&n) => n,
                    _ => return Err(set_error(self, "units is a number from 1 to 26".into())),
                };
                let s = self.symed_symbol().ok_or("open a symbol first")?;
                let used = s
                    .pins
                    .iter()
                    .map(|p| p.unit)
                    .chain(s.unit_graphics.iter().map(|(u, _)| *u))
                    .max()
                    .unwrap_or(0);
                if used > units {
                    return Err(set_error(
                        self,
                        format!(
                            "unit {} still has pins or graphics; delete them first",
                            LibSymbol::unit_letter(used)
                        ),
                    ));
                }
                let reference = dialog.value("Reference");
                if reference.is_empty() {
                    return Err(set_error(
                        self,
                        "the reference designator is required".into(),
                    ));
                }
                let spice = if power {
                    Spice::Power
                } else {
                    match model_from(&model) {
                        Some(sp) => sp,
                        None => s.spice,
                    }
                };
                let params = dialog.value("Sim.Params");
                if let Err(e) = super::sch::check_model(spice, &params) {
                    return Err(set_error(self, e));
                }
                let d = dialog.clone();
                self.close_dialog();
                if self.ui.symed.unit > units {
                    self.ui.symed.unit = 1;
                }
                self.symed_edit(move |s| {
                    s.reference = reference;
                    s.value = d.value("Value");
                    s.footprint = d.value("Footprint");
                    s.datasheet = d.value("Datasheet");
                    s.description = d.value("Description");
                    s.keywords = d.value("Keywords");
                    s.units = units;
                    s.sim_params = params;
                    s.spice = spice;
                    s.power = power;
                    if power {
                        s.in_bom = false;
                        s.on_board = false;
                    }
                    s.pin_names_hidden = pin_names_hidden;
                    s.pin_numbers_hidden = pin_numbers_hidden;
                    s.footprints = if s.footprint.is_empty() {
                        vec![]
                    } else {
                        vec![s.footprint.clone()]
                    };
                    Ok("Symbol properties changed".into())
                })
            }
            Dialog::SymText { pos, .. } => {
                let text = dialog.value("Text");
                self.close_dialog();
                let unit = self.symed_unit();
                self.symed_edit(move |s| {
                    // Editing text found at that spot replaces it; otherwise it is new.
                    let spot = |g: &Graphic| matches!(g, Graphic::Text { at, .. } if *at == pos);
                    let existing = s
                        .graphics
                        .iter_mut()
                        .chain(s.unit_graphics.iter_mut().map(|(_, g)| g))
                        .find(|g| spot(g));
                    match (existing, text.is_empty()) {
                        (Some(g), false) => {
                            *g = Graphic::Text { at: pos, text };
                            Ok("Changed text".into())
                        }
                        (Some(_), true) => {
                            s.graphics.retain(|g| !spot(g));
                            s.unit_graphics.retain(|(_, g)| !spot(g));
                            Ok("Removed empty text".into())
                        }
                        (None, true) => Err("type the text first".into()),
                        (None, false) => {
                            let g = Graphic::Text { at: pos, text };
                            if s.units > 1 {
                                s.unit_graphics.push((unit, g));
                            } else {
                                s.graphics.push(g);
                            }
                            Ok("Added text".into())
                        }
                    }
                })
            }
            _ => Err("not a Symbol Editor dialog".into()),
        }
    }

    // ---- presentation ---------------------------------------------------------------
    pub(super) fn render_symed(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (w, h) = (env.width, env.height);
        let c = chrome(env.theme);
        p.scene.background = c.bar;
        let area = canvas_rect(w, h);
        let view = self.symed_view(area.width, area.height);
        let cv = Cv {
            ox: area.x,
            oy: area.y,
            w: area.width,
            h: area.height,
            view,
        };
        self.paint_symbol_canvas(p, &cv);
        p.region(
            area,
            &format!(
                "kicad:canvas:symed:{}:{}:{}",
                view.args(),
                area.width,
                area.height
            ),
            "Symbol canvas",
        );
        let top = w::MENU_H as i32;
        p.box_(Rect::new(0, top, w, w::TOOL_H), c.bar, 0);
        p.hline(0, top + w::TOOL_H as i32 - 1, w, w::EDGE);
        let has_lib = self.symed_lib().is_some();
        let has_sym = self.symed_symbol().is_some();
        let when = |ok: bool, t: &str, why: &'static str| -> Result<String, &'static str> {
            if ok {
                Ok(t.to_owned())
            } else {
                Err(why)
            }
        };
        let zoom_args = format!("{}:{}:{}", view.args(), area.width, area.height);
        let sel = self.symed_selection();
        let groups: Vec<Vec<w::ToolSpec>> = vec![
            vec![
                (
                    icons::new_project,
                    when(
                        self.session.project.is_some(),
                        "kicad:symed:new-lib",
                        "no project is open",
                    ),
                    "New library",
                ),
                (
                    icons::add_symbol,
                    when(has_lib, "kicad:symed:new-symbol", "choose a library first"),
                    "New symbol (Ctrl+N)",
                ),
                (
                    icons::save,
                    when(has_lib, "kicad:symed:save", "choose a library first"),
                    "Save library (Ctrl+S)",
                ),
            ],
            vec![
                (
                    icons::undo,
                    when(
                        !self.ui.symed.undo.is_empty(),
                        "kicad:symed:undo",
                        "nothing to undo",
                    ),
                    "Undo (Ctrl+Z)",
                ),
                (
                    icons::redo,
                    when(
                        !self.ui.symed.redo.is_empty(),
                        "kicad:symed:redo",
                        "nothing to redo",
                    ),
                    "Redo (Ctrl+Y)",
                ),
            ],
            vec![
                (
                    icons::zoom_in,
                    Ok(format!("kicad:symed:zoom:in:{zoom_args}")),
                    "Zoom in (F1)",
                ),
                (
                    icons::zoom_out,
                    Ok(format!("kicad:symed:zoom:out:{zoom_args}")),
                    "Zoom out (F2)",
                ),
                (
                    icons::zoom_fit,
                    Ok(format!("kicad:symed:zoom:fit:{zoom_args}")),
                    "Zoom to fit (Home)",
                ),
            ],
            vec![
                (
                    icons::properties,
                    when(has_sym, "kicad:symed:properties", "open a symbol first"),
                    "Symbol properties",
                ),
                (
                    icons::rotate,
                    when(
                        matches!(sel, Some(SymSel::Pin(_))),
                        "kicad:symed:rotate",
                        "select a pin first",
                    ),
                    "Rotate pin (R)",
                ),
                (
                    icons::delete,
                    when(sel.is_some(), "kicad:symed:delete", "nothing is selected"),
                    "Delete (Del)",
                ),
            ],
            vec![(
                icons::schematic,
                when(
                    self.session.project.is_some(),
                    "kicad:symed:schematic",
                    "no project is open",
                ),
                "Switch to Schematic Editor",
            )],
        ];
        let mut x = 6;
        let ty = top + 3;
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
        // The unit chooser of a multi-unit symbol.
        if let Some(s) = self.symed_symbol().filter(|s| s.units > 1) {
            x += 12;
            p.label(x, ty + 6, 40, "Unit:", 13, w::INK, false, Align::Left);
            x += 40;
            for u in 1..=s.units {
                let on = self.symed_unit() == u;
                let r = Rect::new(x, ty + 2, 26, 24);
                p.button(
                    r,
                    if on { c.selection } else { w::WHITE },
                    c.radius,
                    &format!("kicad:symed:unit:{u}"),
                    &format!("Unit {}", LibSymbol::unit_letter(u)),
                );
                p.border(r, Color::TRANSPARENT, c.radius, w::EDGE);
                p.label(
                    r.x,
                    r.y + 4,
                    26,
                    &LibSymbol::unit_letter(u),
                    13,
                    w::INK,
                    on,
                    Align::Center,
                );
                x += 28;
            }
        }
        self.paint_symbol_tree(p, &c, area);
        // Right toolbar: drawing tools.
        let rx = area.x + area.width as i32;
        p.box_(Rect::new(rx, area.y, w::SIDE_W, area.height), c.bar, 0);
        p.vline(rx, area.y, area.height, w::EDGE);
        let mut y = area.y + 4;
        for (tool, icon, tip) in TOOLS {
            let target = if tool == "select" || has_sym {
                Ok(format!("kicad:symed:tool:{tool}"))
            } else {
                Err("open or create a symbol first")
            };
            w::tool(
                p,
                &c,
                rx + 3,
                y,
                icon,
                target,
                tip,
                self.symed_tool() == tool,
            );
            y += 32;
        }
        let hv = self.ui.hover.map(lib_pt).unwrap_or_default();
        w::status_bar(
            p,
            &c,
            w,
            h,
            &[
                format!("Z {:.2}", view.zoom as f64 / 100.0),
                format!("X {} Y {}", hv.0, hv.1),
                "grid 50 mils".into(),
                "mils".into(),
                self.symed_tool().to_owned(),
                self.ui.status.clone(),
            ],
        );
        let menus = symed_menus(self);
        let titles: Vec<&str> = menus.iter().map(|m| m.0).collect();
        let xs = w::menubar(p, &c, w, &titles, self.ui.menu.as_deref());
        if let Some(open) = &self.ui.menu {
            if let Some(i) = titles.iter().position(|t| t == open) {
                p.z += 40;
                w::menu_panel(p, &c, xs[i], &menus[i].1);
                p.z -= 40;
            }
        }
    }

    fn paint_symbol_tree(&self, p: &mut Painter, c: &w::Chrome, area: Rect) {
        p.box_(Rect::new(0, area.y, LIB_W, area.height), c.panel, 0);
        p.vline(LIB_W as i32, area.y, area.height, w::EDGE);
        p.label(
            8,
            area.y + 6,
            LIB_W - 16,
            "Project Symbol Libraries",
            12,
            w::MUTED,
            true,
            Align::Left,
        );
        let mut y = area.y + 28;
        let bottom = area.y + area.height as i32;
        if self.session.sym_libs.is_empty() {
            p.paragraph(
                8,
                y,
                LIB_W - 16,
                if self.session.project.is_some() {
                    "The project has no symbol libraries yet. File ▸ New Library… makes one."
                } else {
                    "Open a project first: its libraries are listed here."
                },
                12,
                w::MUTED,
            );
            return;
        }
        for lib in &self.session.sym_libs {
            if y + 22 > bottom {
                break;
            }
            let chosen = self.ui.symed.lib.as_deref() == Some(lib.name.as_str());
            w::row(
                p,
                c,
                Rect::new(4, y, LIB_W - 8, 22),
                &format!(
                    "{} {}{}",
                    if chosen { "▾" } else { "▸" },
                    lib.name,
                    if lib.dirty { " *" } else { "" }
                ),
                &format!("kicad:symed:lib:{}", lib.name),
                false,
                w::INK,
            );
            y += 23;
            if !chosen {
                continue;
            }
            if lib.symbols.is_empty() {
                p.label(
                    24,
                    y + 2,
                    LIB_W - 30,
                    "(empty; New Symbol adds one)",
                    12,
                    w::MUTED,
                    false,
                    Align::Left,
                );
                y += 22;
            }
            for s in &lib.symbols {
                if y + 22 > bottom {
                    break;
                }
                w::row(
                    p,
                    c,
                    Rect::new(20, y, LIB_W - 24, 22),
                    s.name(),
                    &format!("kicad:symed:open:{}", s.lib_id),
                    self.ui.symed.symbol.as_deref() == Some(s.lib_id.as_str()),
                    w::INK,
                );
                y += 23;
            }
        }
    }

    fn paint_symbol_canvas(&self, p: &mut Painter, cv: &Cv) {
        p.box_(Rect::new(cv.ox, cv.oy, cv.w, cv.h), draw::SCH_BG, 0);
        // Grid dots at 50 mils, coarsened while they would crowd.
        let mut step = cw_eda::schematic::GRID;
        while cv.len(step) < 8 {
            step *= 2;
        }
        let tl = cv.view.world(0, 0);
        let br = cv.view.world(cv.w as i32, cv.h as i32);
        if (br.x - tl.x) / step * ((br.y - tl.y) / step) < 6000 {
            let mut gy = tl.y.div_euclid(step) * step;
            while gy <= br.y {
                let mut gx = tl.x.div_euclid(step) * step;
                while gx <= br.x {
                    let (x, y) = cv.pt(Pt::new(gx, gy));
                    p.box_(Rect::new(x, y, 1, 1), draw::SCH_GRID, 0);
                    gx += step;
                }
                gy += step;
            }
        }
        let Some(s) = self.symed_symbol() else {
            p.label(
                cv.ox,
                cv.oy + cv.h as i32 / 2 - 10,
                cv.w,
                if self.session.sym_libs.is_empty() {
                    "Create a library (File ▸ New Library…), then a symbol in it"
                } else {
                    "Choose a symbol in the library tree, or File ▸ New Symbol…"
                },
                14,
                w::MUTED,
                false,
                Align::Center,
            );
            return;
        };
        // The symbol's origin, as KiCad marks it.
        let (ox, oy) = cv.pt(Pt::new(0, 0));
        p.line(vec![(ox - 8, oy), (ox + 8, oy)], draw::SHEET, 1);
        p.line(vec![(ox, oy - 8), (ox, oy + 8)], draw::SHEET, 1);
        let unit = self.symed_unit();
        draw::symbol_unit(p, cv, s, unit, Pt::new(0, 0), Xf::IDENTITY, None, false);
        // Reference and value where they sit on a placed symbol.
        let reference = if s.units > 1 {
            format!("{}?{}", s.reference, LibSymbol::unit_letter(unit))
        } else {
            format!("{}?", s.reference)
        };
        cv.text(p, world(s.ref_at), &reference, 50, draw::FIELD, Align::Left);
        cv.text(p, world(s.value_at), &s.value, 50, draw::FIELD, Align::Left);
        // A small circle at each pin's connection point.
        for pn in s.unit_pins(unit) {
            let (x, y) = cv.pt(world(pn.at));
            p.ring(x, y, 3, 1, draw::BODY);
        }
        // Selection.
        if let Some(sel) = self.symed_selection() {
            let offset = match &self.ui.drag {
                Some(Drag::Move { start, at }) => lib_pt(at.sub(*start)),
                _ => (0, 0),
            };
            match sel {
                SymSel::Pin(i) => {
                    let pn = &s.pins[i];
                    let m = |q: (i64, i64)| world((q.0 + offset.0, q.1 + offset.1));
                    cv.stroke(p, &[m(pn.at), m(pn.inner())], draw::SELECT_EDGE, 20, 3);
                }
                SymSel::Common(_) | SymSel::Unit(_) => {
                    let g = match sel {
                        SymSel::Common(i) => &s.graphics[i],
                        SymSel::Unit(i) => &s.unit_graphics[i].1,
                        SymSel::Pin(_) => unreachable!("pins were handled above"),
                    };
                    let g = moved(g, offset);
                    let mut one = LibSymbol::blank("sel:sel", "");
                    one.graphics.push(g);
                    let ((x0, y0), (x1, y1)) = one.bounds();
                    let (a, b) = (world((x0 - 20, y1 + 20)), world((x1 + 20, y0 - 20)));
                    let (ax, ay) = cv.pt(a);
                    let (bx, by) = cv.pt(b);
                    let r = Rect::new(ax, ay, (bx - ax).max(1) as u32, (by - ay).max(1) as u32);
                    p.border(r, draw::SELECT, 0, draw::SELECT_EDGE);
                }
            }
        }
        // The shape being drawn, to the pointer.
        let ghost = Color(132, 0, 0, 150);
        if let (Some(first), Some(hv)) = (self.ui.symed.points.first(), self.ui.hover) {
            let h = lib_pt(hv);
            match self.symed_tool() {
                "rect" => {
                    let pts = [*first, (h.0, first.1), h, (first.0, h.1), *first].map(world);
                    cv.stroke(p, &pts, ghost, 6, 1);
                }
                "circle" => {
                    let r = isqrt((h.0 - first.0).pow(2) + (h.1 - first.1).pow(2));
                    let (x, y) = cv.pt(world(*first));
                    p.ring(x, y, cv.len(r).max(1) as u32, 1, ghost);
                }
                "line" => {
                    let mut pts: Vec<Pt> = self.ui.symed.points.iter().map(|q| world(*q)).collect();
                    pts.push(hv);
                    cv.stroke(p, &pts, ghost, 10, 1);
                }
                _ => {}
            }
        }
    }

    pub(super) fn symed_page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        for (id, label) in [
            ("kicad:symed:new-lib", "New Library"),
            ("kicad:symed:new-symbol", "New Symbol"),
            ("kicad:symed:save", "Save Library"),
            ("kicad:symed:properties", "Symbol Properties"),
            ("kicad:symed:tool:select", "Select"),
            ("kicad:symed:tool:pin", "Add Pin"),
            ("kicad:symed:tool:rect", "Add Rectangle"),
            ("kicad:symed:tool:circle", "Add Circle"),
            ("kicad:symed:tool:line", "Add Polyline"),
            ("kicad:symed:tool:text", "Add Text"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
        for lib in &self.session.sym_libs {
            let id = format!("kicad:symed:lib:{}", lib.name);
            page.elements.push(E::Button {
                id: id.clone(),
                text: format!(
                    "Library {} ({}){}",
                    lib.name,
                    lib.file,
                    if lib.dirty { ", unsaved" } else { "" }
                ),
                action: act(&id),
            });
            for s in &lib.symbols {
                let id = format!("kicad:symed:open:{}", s.lib_id);
                page.elements.push(E::Button {
                    id: id.clone(),
                    text: format!("Symbol {}", s.lib_id),
                    action: act(&id),
                });
            }
        }
        if let Some(s) = self.symed_symbol() {
            page.elements.push(E::Text {
                id: "kicad-symbol".into(),
                text: format!(
                    "{}: reference {}, value {}, {} unit(s), {} pin(s), {} graphic(s), model {}",
                    s.lib_id,
                    s.reference,
                    s.value,
                    s.units,
                    s.pins.len(),
                    s.graphics.len() + s.unit_graphics.len(),
                    model_keyword(s.spice)
                ),
            });
            for (i, pn) in s.pins.iter().enumerate() {
                page.elements.push(E::Text {
                    id: format!("kicad-pin-{i}"),
                    text: format!(
                        "Pin {} \"{}\" {} at ({}, {}) mils, pointing {}, length {}{}",
                        pn.number,
                        pn.name,
                        pn.kind.keyword(),
                        pn.at.0,
                        pn.at.1,
                        orient_name(pn.angle),
                        pn.length,
                        if pn.unit > 0 {
                            format!(", unit {}", LibSymbol::unit_letter(pn.unit))
                        } else {
                            String::new()
                        }
                    ),
                });
            }
        }
    }
}

fn symed_menus(k: &Kicad) -> Vec<(&'static str, Vec<MenuItem>)> {
    let lib = |t: &str| -> Result<String, &'static str> {
        if k.symed_lib().is_some() {
            Ok(t.to_owned())
        } else {
            Err("choose a library first")
        }
    };
    let sym = |t: &str| -> Result<String, &'static str> {
        if k.symed_symbol().is_some() {
            Ok(t.to_owned())
        } else {
            Err("open a symbol first")
        }
    };
    let sel = k.symed_selection();
    vec![
        (
            "File",
            vec![
                MenuItem::new(
                    "New Library...",
                    "",
                    if k.session.project.is_some() {
                        Ok("kicad:symed:new-lib".into())
                    } else {
                        Err("no project is open")
                    },
                ),
                MenuItem::new("New Symbol...", "Ctrl+N", lib("kicad:symed:new-symbol")),
                MenuItem::new("Save", "Ctrl+S", lib("kicad:symed:save")).sep(),
                MenuItem::new("Delete Symbol", "", sym("kicad:symed:delete-symbol")).sep(),
            ],
        ),
        (
            "Edit",
            vec![
                MenuItem::new(
                    "Undo",
                    "Ctrl+Z",
                    if k.ui.symed.undo.is_empty() {
                        Err("nothing to undo")
                    } else {
                        Ok("kicad:symed:undo".into())
                    },
                ),
                MenuItem::new(
                    "Redo",
                    "Ctrl+Y",
                    if k.ui.symed.redo.is_empty() {
                        Err("nothing to redo")
                    } else {
                        Ok("kicad:symed:redo".into())
                    },
                ),
                MenuItem::new(
                    "Delete",
                    "Del",
                    if sel.is_some() {
                        Ok("kicad:symed:delete".into())
                    } else {
                        Err("nothing is selected")
                    },
                )
                .sep(),
                MenuItem::new(
                    "Rotate Pin",
                    "R",
                    if matches!(sel, Some(SymSel::Pin(_))) {
                        Ok("kicad:symed:rotate".into())
                    } else {
                        Err("select a pin first")
                    },
                ),
                MenuItem::new("Symbol Properties...", "E", sym("kicad:symed:properties")).sep(),
            ],
        ),
        (
            "View",
            vec![
                MenuItem::new("Zoom In", "F1", Ok("kicad:symed:zoom:in".into())),
                MenuItem::new("Zoom Out", "F2", Ok("kicad:symed:zoom:out".into())),
                MenuItem::new("Zoom to Fit", "Home", Ok("kicad:symed:zoom:fit".into())),
            ],
        ),
        (
            "Place",
            vec![
                MenuItem::new("Pin", "P", sym("kicad:symed:tool:pin")),
                MenuItem::new("Rectangle", "", sym("kicad:symed:tool:rect")),
                MenuItem::new("Circle", "", sym("kicad:symed:tool:circle")),
                MenuItem::new("Polyline", "", sym("kicad:symed:tool:line")),
                MenuItem::new("Text", "", sym("kicad:symed:tool:text")),
            ],
        ),
        (
            "Help",
            vec![MenuItem::new("About KiCad", "", Ok("kicad:about".into()))],
        ),
    ]
}
