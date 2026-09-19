//! The Footprint Editor: the project's own footprint libraries. A footprint's pads
//! (through-hole or SMD, shape, size, drill), silkscreen strokes, fabrication outline
//! (the body the 3D viewer extrudes), courtyard and body height, saved one
//! `<name>.kicad_mod` per footprint in `<lib>.pretty/` and listed in `fp-lib-table`, so
//! a symbol's Footprint field can name them and Update PCB places them.
use super::draw::Cv;
use super::widgets::{self as w, chrome, MenuItem};
use super::{act, icons, Dialog, Drag, FpLib, Kicad, View};
use crate::desktop_scene::{shared::Align, Painter};
use crate::{AppEffect, PointerPhase};
use cw_eda::footprints::{LibFootprint, LibPad, PadKind, PadShape, MM};
use cw_eda::geom::{mm, parse_mm, Pt};
use cw_eda::pcb::Layer;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

const LIB_W: u32 = 230;
const MIN_ZOOM: i64 = 2;
const MAX_ZOOM: i64 = 2000;
const UNDO_LIMIT: usize = 50;
/// Grid choices, nanometres.
const GRIDS: [i64; 5] = [50_000, 100_000, 250_000, 500_000, 1_270_000];
const TOOLS: [(&str, w::Icon, &str); 5] = [
    ("select", icons::select, "Select item(s)"),
    ("pad", icons::pad, "Add a pad"),
    (
        "silk",
        icons::line,
        "Draw silkscreen lines (double-click ends)",
    ),
    ("fab", icons::rect, "Draw the fabrication (body) outline"),
    ("courtyard", icons::zone, "Draw the courtyard"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum FpSel {
    Pad(usize),
    Silk(usize),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FpEdUi {
    pub lib: Option<String>,
    /// The footprint open on the canvas (`Lib:Name`).
    pub footprint: Option<String>,
    /// Grid, nanometres.
    pub grid: i64,
    /// `select`, `pad`, `silk`, `fab`, `courtyard`.
    pub tool: String,
    pub selection: Option<FpSel>,
    /// Clicks of the shape being drawn, footprint coordinates (nm, Y down).
    pub points: Vec<(i64, i64)>,
    /// The kind and shape the next pad gets: the last ones chosen.
    pub smd: bool,
    pub shape: String,
    pub undo: Vec<LibFootprint>,
    pub redo: Vec<LibFootprint>,
}
impl Default for FpEdUi {
    fn default() -> Self {
        Self {
            lib: None,
            footprint: None,
            grid: 250_000,
            tool: String::new(),
            selection: None,
            points: vec![],
            smd: true,
            shape: "roundrect".into(),
            undo: vec![],
            redo: vec![],
        }
    }
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
fn mm_text(v: i64) -> String {
    mm(v, MM)
}
fn um(p: (i64, i64)) -> Pt {
    Pt::new(p.0 / 1000, p.1 / 1000)
}
fn layer_rgb(l: Layer) -> Color {
    let (r, g, b) = cw_eda::svg::color(l);
    Color::rgb(r, g, b)
}
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
fn norm_rect(a: (i64, i64), b: (i64, i64)) -> ((i64, i64), (i64, i64)) {
    ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
}

impl Kicad {
    fn fped_tool(&self) -> &str {
        if self.ui.fped.tool.is_empty() {
            "select"
        } else {
            &self.ui.fped.tool
        }
    }
    fn fped_lib(&self) -> Option<&FpLib> {
        let name = self.ui.fped.lib.as_deref()?;
        self.session.fp_libs.iter().find(|l| l.name == name)
    }
    pub(super) fn fped_footprint(&self) -> Option<&LibFootprint> {
        let id = self.ui.fped.footprint.as_deref()?;
        self.fped_lib()?.footprints.iter().find(|f| f.id == id)
    }
    fn fped_footprint_mut(&mut self) -> Option<&mut LibFootprint> {
        let id = self.ui.fped.footprint.clone()?;
        let name = self.ui.fped.lib.clone()?;
        self.session
            .fp_libs
            .iter_mut()
            .find(|l| l.name == name)?
            .footprints
            .iter_mut()
            .find(|f| f.id == id)
    }
    fn fped_edit(
        &mut self,
        f: impl FnOnce(&mut LibFootprint) -> Result<String, String>,
    ) -> Result<Vec<AppEffect>, String> {
        let before = self
            .fped_footprint()
            .cloned()
            .ok_or("open a footprint first")?;
        let fp = self.fped_footprint_mut().ok_or("open a footprint first")?;
        let message = f(fp)?;
        let after = fp.clone();
        if after != before {
            let ui = &mut self.ui.fped;
            ui.undo.push(before);
            if ui.undo.len() > UNDO_LIMIT {
                ui.undo.remove(0);
            }
            ui.redo.clear();
            let name = self.ui.fped.lib.clone();
            if let Some(l) = self
                .session
                .fp_libs
                .iter_mut()
                .find(|l| Some(&l.name) == name.as_ref())
            {
                l.dirty = true;
            }
        }
        self.ui.status = message;
        Ok(vec![])
    }
    fn fped_tol(&self) -> i64 {
        (4_000_000 / self.ui.view.zoom.max(1)).max(50_000)
    }
    fn fped_hit(&self, p: (i64, i64)) -> Option<FpSel> {
        let f = self.fped_footprint()?;
        let tol = self.fped_tol();
        for (i, pad) in f.pads.iter().enumerate().rev() {
            let (hx, hy) = (pad.size.0 / 2 + tol / 2, pad.size.1 / 2 + tol / 2);
            if (p.0 - pad.at.0).abs() <= hx && (p.1 - pad.at.1).abs() <= hy {
                return Some(FpSel::Pad(i));
            }
        }
        f.silk
            .iter()
            .enumerate()
            .map(|(i, (a, b))| (seg_dist(p, *a, *b), i))
            .filter(|(d, _)| *d <= tol)
            .min()
            .map(|(_, i)| FpSel::Silk(i))
    }
    fn fped_selection(&self) -> Option<FpSel> {
        let sel = self.ui.fped.selection?;
        let f = self.fped_footprint()?;
        let live = match sel {
            FpSel::Pad(i) => i < f.pads.len(),
            FpSel::Silk(i) => i < f.silk.len(),
        };
        live.then_some(sel)
    }
    fn fped_open(&mut self, id: &str) -> Result<Vec<AppEffect>, String> {
        let lib = id.split(':').next().unwrap_or("");
        let found = self
            .session
            .fp_libs
            .iter()
            .any(|l| l.name == lib && l.footprints.iter().any(|f| f.id == id));
        if !found {
            return Err(format!("{id} is not in a project library"));
        }
        let ui = &mut self.ui.fped;
        ui.lib = Some(lib.to_owned());
        ui.footprint = Some(id.to_owned());
        ui.selection = None;
        ui.points.clear();
        ui.undo.clear();
        ui.redo.clear();
        self.ui.view.fit = true;
        self.ui.status = format!("Editing {id}");
        Ok(vec![])
    }
    fn fped_view(&self, cw: u32, ch: u32) -> View {
        if !self.ui.view.fit && self.ui.view.zoom != 0 {
            return self.ui.view;
        }
        let (mut lo, mut hi) = ((-3 * MM, -3 * MM), (3 * MM, 3 * MM));
        if let Some(f) = self.fped_footprint() {
            let (a, b) = norm_rect(f.courtyard.0, f.courtyard.1);
            lo = (lo.0.min(a.0), lo.1.min(a.1));
            hi = (hi.0.max(b.0), hi.1.max(b.1));
            for p in &f.pads {
                lo = (lo.0.min(p.at.0 - p.size.0), lo.1.min(p.at.1 - p.size.1));
                hi = (hi.0.max(p.at.0 + p.size.0), hi.1.max(p.at.1 + p.size.1));
            }
        }
        let m = MM;
        View::fitted(um((lo.0 - m, lo.1 - m)), um((hi.0 + m, hi.1 + m)), cw, ch)
    }
    fn fped_zoom(&mut self, dir: &str, args: &[&str]) -> Result<Vec<AppEffect>, String> {
        let (cw, ch) = match (
            args.get(3).and_then(|v| v.parse().ok()),
            args.get(4).and_then(|v| v.parse().ok()),
        ) {
            (Some(w), Some(h)) => (w, h),
            _ if self.ui.canvas.0 > 0 => self.ui.canvas,
            _ => (800, 600),
        };
        self.ui.canvas = (cw, ch);
        let mut v = View::from_args(args).unwrap_or_else(|| self.fped_view(cw, ch));
        let centre = match (args.len() >= 3, self.ui.hover) {
            (false, Some(h)) => um((h.x, h.y)),
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
    fn open_pad_dialog(&mut self, edit: Option<usize>, pos: (i64, i64)) -> Result<(), String> {
        let f = self.fped_footprint().ok_or("open a footprint first")?;
        let (pad, smd, shape) = match edit {
            Some(i) => {
                let p = f.pads.get(i).ok_or("no such pad")?.clone();
                let smd = p.kind == PadKind::Smd;
                let shape = p.shape.keyword().to_owned();
                (p, smd, shape)
            }
            None => {
                let next = f
                    .pads
                    .iter()
                    .filter_map(|p| p.number.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0)
                    + 1;
                // The last pad's size, as KiCad's pad tool repeats it.
                let (size, drill) = f.pads.last().map_or(
                    if self.ui.fped.smd {
                        ((1_500_000, 1_500_000), 0)
                    } else {
                        ((1_700_000, 1_700_000), 1_000_000)
                    },
                    |p| (p.size, p.drill),
                );
                (
                    LibPad {
                        number: next.to_string(),
                        kind: if self.ui.fped.smd {
                            PadKind::Smd
                        } else {
                            PadKind::ThroughHole
                        },
                        shape: PadShape::parse(&self.ui.fped.shape).unwrap_or(PadShape::Rect),
                        at: pos,
                        size,
                        drill: if drill > 0 { drill } else { 1_000_000 },
                    },
                    self.ui.fped.smd,
                    self.ui.fped.shape.clone(),
                )
            }
        };
        self.ui.dialog = Some(Dialog::PadProperties {
            edit,
            pos: pad.at,
            smd,
            shape,
            fields: vec![
                ("Number".into(), pad.number.clone()),
                ("Position X (mm)".into(), mm_text(pad.at.0)),
                ("Position Y (mm)".into(), mm_text(pad.at.1)),
                ("Size X (mm)".into(), mm_text(pad.size.0)),
                ("Size Y (mm)".into(), mm_text(pad.size.1)),
                (
                    "Drill (mm)".into(),
                    mm_text(if pad.drill > 0 { pad.drill } else { 1_000_000 }),
                ),
            ],
            error: String::new(),
        });
        self.ui.focus = Some("Number".into());
        Ok(())
    }
    fn open_fp_fields(&mut self) -> Result<(), String> {
        let f = self.fped_footprint().ok_or("open a footprint first")?;
        self.ui.dialog = Some(Dialog::FootprintFields {
            fields: vec![
                ("Description".into(), f.description.clone()),
                ("Body height (mm)".into(), mm_text(f.height)),
            ],
            error: String::new(),
        });
        self.ui.focus = Some("Description".into());
        Ok(())
    }
    fn fped_finish_silk(&mut self) -> Result<Vec<AppEffect>, String> {
        let pts = std::mem::take(&mut self.ui.fped.points);
        if pts.len() < 2 {
            return Err("a silkscreen line needs two points".into());
        }
        self.fped_edit(move |f| {
            let n = pts.len() - 1;
            for pair in pts.windows(2) {
                f.silk.push((pair[0], pair[1]));
            }
            Ok(format!("Added {n} silkscreen segment(s)"))
        })
    }
    fn fped_save(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let name = self.ui.fped.lib.clone().ok_or("choose a library first")?;
        let effects = self.save_fp_lib(window, &name)?;
        let dir = self.fped_lib().map(|l| l.dir.clone()).unwrap_or_default();
        self.ui.status = format!("Saved library {name} ({dir}/)");
        Ok(effects)
    }

    pub(super) fn fped_key(&mut self, window: u64, key: &str) -> Result<Vec<AppEffect>, String> {
        let cmd = match key {
            "Ctrl+s" | "Meta+s" => "save",
            "Ctrl+z" | "Meta+z" => "undo",
            "Ctrl+y" | "Meta+y" | "Ctrl+Shift+z" | "Meta+Shift+z" => "redo",
            "Ctrl+n" | "Meta+n" => "new-footprint",
            "Delete" | "Backspace" => "delete",
            "Escape" => "cancel",
            "r" | "R" => "rotate",
            "e" | "E" => "edit",
            "n" | "N" => "grid",
            "Home" => "zoom:fit",
            "F1" => "zoom:in",
            "F2" => "zoom:out",
            other => return Err(format!("{other} does nothing in the Footprint Editor")),
        };
        self.fped_command(window, cmd)
    }
    pub(super) fn fped_activate(&mut self, _window: u64) -> Result<Vec<AppEffect>, String> {
        if self.fped_tool() == "silk" && !self.ui.fped.points.is_empty() {
            return self.fped_finish_silk();
        }
        let at = self.ui.hover.ok_or("point at something first")?;
        match self.fped_hit((at.x, at.y)) {
            Some(FpSel::Pad(i)) => {
                self.ui.fped.selection = Some(FpSel::Pad(i));
                self.open_pad_dialog(Some(i), (at.x, at.y))?;
            }
            Some(sel) => self.ui.fped.selection = Some(sel),
            None => self.open_fp_fields()?,
        }
        Ok(vec![])
    }
    pub(super) fn fped_pointer(
        &mut self,
        _window: u64,
        phase: PointerPhase,
        at: Pt,
    ) -> Result<Vec<AppEffect>, String> {
        let snapped = at.snap(self.ui.fped.grid.max(1));
        self.ui.hover = Some(snapped);
        let sp = (snapped.x, snapped.y);
        if self.fped_footprint().is_none() {
            return match phase {
                PointerPhase::Down => Err("open or create a footprint first".into()),
                _ => Ok(vec![]),
            };
        }
        match (self.fped_tool().to_owned().as_str(), phase) {
            ("select", PointerPhase::Down) => {
                let hit = self.fped_hit((at.x, at.y));
                self.ui.fped.selection = hit;
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
                let (Some(Drag::Move { start, .. }), Some(sel)) = (drag, self.fped_selection())
                else {
                    return Ok(vec![]);
                };
                let d = snapped.sub(start);
                if d == Pt::default() {
                    return Ok(vec![]);
                }
                self.fped_edit(move |f| {
                    let m = |p: (i64, i64)| (p.0 + d.x, p.1 + d.y);
                    match sel {
                        FpSel::Pad(i) => f.pads[i].at = m(f.pads[i].at),
                        FpSel::Silk(i) => f.silk[i] = (m(f.silk[i].0), m(f.silk[i].1)),
                    }
                    Ok(format!("Moved by {} mm, {} mm", mm_text(d.x), mm_text(d.y)))
                })
            }
            (_, PointerPhase::Cancel) => {
                self.ui.drag = None;
                self.ui.fped.points.clear();
                Ok(vec![])
            }
            ("pad", PointerPhase::Up) => {
                self.open_pad_dialog(None, sp)?;
                Ok(vec![])
            }
            ("silk", PointerPhase::Up) => {
                if self.ui.fped.points.last() != Some(&sp) {
                    self.ui.fped.points.push(sp);
                }
                self.ui.status = "Click the next point; double-click or Escape ends".into();
                Ok(vec![])
            }
            (tool @ ("fab" | "courtyard"), PointerPhase::Up) => {
                match self.ui.fped.points.first().copied() {
                    None => {
                        self.ui.fped.points = vec![sp];
                        self.ui.status = "Click the opposite corner".into();
                        Ok(vec![])
                    }
                    Some(a) => {
                        self.ui.fped.points.clear();
                        if a.0 == sp.0 || a.1 == sp.1 {
                            return Err("the outline needs width and height".into());
                        }
                        let r = norm_rect(a, sp);
                        let fab = tool == "fab";
                        self.fped_edit(move |f| {
                            if fab {
                                f.fab = r;
                                Ok("Set the fabrication (body) outline".into())
                            } else {
                                f.courtyard = r;
                                Ok("Set the courtyard".into())
                            }
                        })
                    }
                }
            }
            _ => Ok(vec![]),
        }
    }
    pub(super) fn fped_command(
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
                    fp: true,
                    fields: vec![("Name".into(), String::new())],
                    error: String::new(),
                });
                self.ui.focus = Some("Name".into());
                Ok(vec![])
            }
            "lib" => {
                let name = rest.strip_prefix("lib:").unwrap_or("");
                if !self.session.fp_libs.iter().any(|l| l.name == name) {
                    return Err(format!("no project library {name}"));
                }
                self.ui.fped.lib = Some(name.to_owned());
                Ok(vec![])
            }
            "open" => self.fped_open(rest.strip_prefix("open:").unwrap_or("")),
            "new-footprint" => {
                if self.fped_lib().is_none() {
                    return Err("choose or create a library first".into());
                }
                self.ui.dialog = Some(Dialog::NewFootprint {
                    fields: vec![("Name".into(), String::new())],
                    error: String::new(),
                });
                self.ui.focus = Some("Name".into());
                Ok(vec![])
            }
            "delete-footprint" => {
                let id = self
                    .ui
                    .fped
                    .footprint
                    .clone()
                    .ok_or("open a footprint first")?;
                let name = self.ui.fped.lib.clone().ok_or("choose a library first")?;
                let lib = self
                    .session
                    .fp_libs
                    .iter_mut()
                    .find(|l| l.name == name)
                    .ok_or("no such library")?;
                lib.footprints.retain(|f| f.id != id);
                lib.dirty = true;
                self.ui.fped.footprint = None;
                self.ui.fped.undo.clear();
                self.ui.fped.redo.clear();
                self.ui.status =
                    format!("Removed {id} from {name}; its .kicad_mod stays until deleted on disk");
                Ok(vec![])
            }
            "tool" => {
                let t = parts.get(1).copied().unwrap_or("select");
                if !TOOLS.iter().any(|(n, _, _)| *n == t) {
                    return Err(format!("unknown tool {t}"));
                }
                if t != "select" && self.fped_footprint().is_none() {
                    return Err("open or create a footprint first".into());
                }
                self.ui.fped.tool = t.to_owned();
                self.ui.fped.points.clear();
                Ok(vec![])
            }
            "grid" => {
                let i = GRIDS
                    .iter()
                    .position(|g| *g == self.ui.fped.grid)
                    .map_or(0, |i| (i + 1) % GRIDS.len());
                self.ui.fped.grid = GRIDS[i];
                self.ui.status = format!("Grid {} mm", mm_text(GRIDS[i]));
                Ok(vec![])
            }
            "properties" => {
                self.open_fp_fields()?;
                Ok(vec![])
            }
            "edit" => match self.fped_selection() {
                Some(FpSel::Pad(i)) => {
                    self.open_pad_dialog(Some(i), (0, 0))?;
                    Ok(vec![])
                }
                _ => {
                    self.open_fp_fields()?;
                    Ok(vec![])
                }
            },
            "rotate" => {
                let Some(FpSel::Pad(i)) = self.fped_selection() else {
                    return Err("select a pad to rotate".into());
                };
                self.fped_edit(move |f| {
                    let p = &mut f.pads[i];
                    p.size = (p.size.1, p.size.0);
                    Ok(format!("Pad {} turned a quarter", p.number))
                })
            }
            "fit-courtyard" => self.fped_edit(|f| {
                f.fit_courtyard();
                Ok("Courtyard fitted to the pads and body with a 0.25 mm margin".into())
            }),
            "delete" => {
                let sel = self.fped_selection().ok_or("nothing is selected")?;
                self.ui.fped.selection = None;
                self.fped_edit(move |f| {
                    Ok(match sel {
                        FpSel::Pad(i) => format!("Deleted pad {}", f.pads.remove(i).number),
                        FpSel::Silk(i) => {
                            f.silk.remove(i);
                            "Deleted silkscreen segment".into()
                        }
                    })
                })
            }
            "cancel" => {
                if self.fped_tool() == "silk" && self.ui.fped.points.len() >= 2 {
                    return self.fped_finish_silk();
                }
                self.ui.fped.points.clear();
                self.ui.fped.tool = "select".into();
                self.ui.fped.selection = None;
                Ok(vec![])
            }
            "finish" => self.fped_finish_silk(),
            "undo" => {
                let prev = self.ui.fped.undo.pop().ok_or("nothing to undo")?;
                let cur = self
                    .fped_footprint()
                    .cloned()
                    .ok_or("open a footprint first")?;
                self.ui.fped.redo.push(cur);
                *self.fped_footprint_mut().ok_or("open a footprint first")? = prev;
                self.ui.fped.selection = None;
                self.ui.status = "Undone".into();
                Ok(vec![])
            }
            "redo" => {
                let next = self.ui.fped.redo.pop().ok_or("nothing to redo")?;
                let cur = self
                    .fped_footprint()
                    .cloned()
                    .ok_or("open a footprint first")?;
                self.ui.fped.undo.push(cur);
                *self.fped_footprint_mut().ok_or("open a footprint first")? = next;
                self.ui.fped.selection = None;
                self.ui.status = "Redone".into();
                Ok(vec![])
            }
            "save" => self.fped_save(window),
            "zoom" => self.fped_zoom(
                parts.get(1).copied().unwrap_or(""),
                &parts[2.min(parts.len())..],
            ),
            "board" => self.launch_frame(window, "pcb"),
            other => Err(format!("unknown Footprint Editor command {other}")),
        }
    }
    pub(super) fn fped_dialog(
        &mut self,
        window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let dialog = self.ui.dialog.clone().ok_or("no dialog is open")?;
        let (cmd, arg) = rest.split_once(':').unwrap_or((rest, ""));
        if cmd != "ok" {
            let d = self.ui.dialog.as_mut().expect("checked above");
            match (d, cmd) {
                (Dialog::PadProperties { smd, .. }, "smd") => match arg {
                    "yes" => *smd = true,
                    "no" => *smd = false,
                    _ => return Err("bad pad type".into()),
                },
                (Dialog::PadProperties { shape, .. }, "shape") => {
                    PadShape::parse(arg).ok_or("unknown pad shape")?;
                    *shape = arg.to_owned();
                }
                _ => return Err(format!("{rest} is not a control of this dialog")),
            }
            return Ok(vec![]);
        }
        let set_error = |k: &mut Kicad, e: String| {
            if let Some(
                Dialog::NewLibrary { error, .. }
                | Dialog::NewFootprint { error, .. }
                | Dialog::PadProperties { error, .. }
                | Dialog::FootprintFields { error, .. },
            ) = k.ui.dialog.as_mut()
            {
                *error = e.clone();
            }
            e
        };
        match dialog.clone() {
            Dialog::NewLibrary { .. } => {
                let name = dialog.value("Name");
                if let Err(e) = super::symed::valid_name(&name) {
                    return Err(set_error(self, e));
                }
                if self.session.fp_libs.iter().any(|l| l.name == name) {
                    return Err(set_error(self, format!("a library {name} already exists")));
                }
                self.session.fp_libs.push(FpLib {
                    name: name.clone(),
                    dir: format!("{name}.pretty"),
                    footprints: vec![],
                    dirty: true,
                });
                self.close_dialog();
                self.ui.fped.lib = Some(name.clone());
                self.ui.fped.footprint = None;
                let effects = self.save_fp_lib(window, &name)?;
                self.ui.status = format!("Created library {name} ({name}.pretty/)");
                Ok(effects)
            }
            Dialog::NewFootprint { .. } => {
                let name = dialog.value("Name");
                let lib = self.ui.fped.lib.clone().ok_or("choose a library first")?;
                if let Err(e) = super::symed::valid_name(&name) {
                    return Err(set_error(self, e));
                }
                let id = format!("{lib}:{name}");
                let l = self
                    .session
                    .fp_libs
                    .iter_mut()
                    .find(|l| l.name == lib)
                    .ok_or("no such library")?;
                if l.footprints.iter().any(|f| f.id == id) {
                    return Err(set_error(self, format!("{name} is already in {lib}")));
                }
                l.footprints.push(LibFootprint::blank(&id));
                l.footprints.sort_by(|a, b| a.id.cmp(&b.id));
                l.dirty = true;
                self.close_dialog();
                self.fped_open(&id)?;
                self.ui.status = format!("New footprint {id}: add pads with the pad tool");
                Ok(vec![])
            }
            Dialog::PadProperties {
                edit, smd, shape, ..
            } => {
                let number = dialog.value("Number");
                if number.is_empty() || number.contains(char::is_whitespace) {
                    return Err(set_error(
                        self,
                        "a pad needs a number without spaces".into(),
                    ));
                }
                let num = |k: &str| parse_mm(&dialog.value(k), MM);
                let (Some(x), Some(y)) = (num("Position X (mm)"), num("Position Y (mm)")) else {
                    return Err(set_error(self, "the position is two numbers in mm".into()));
                };
                let (Some(sx), Some(sy)) = (num("Size X (mm)"), num("Size Y (mm)")) else {
                    return Err(set_error(self, "the size is two numbers in mm".into()));
                };
                if sx <= 0 || sy <= 0 || sx > 100 * MM || sy > 100 * MM {
                    return Err(set_error(
                        self,
                        "pad sizes are above 0 and at most 100 mm".into(),
                    ));
                }
                let pad_shape = PadShape::parse(&shape).ok_or("unknown pad shape")?;
                // A circular pad is as tall as it is wide.
                let sy = if pad_shape == PadShape::Circle {
                    sx
                } else {
                    sy
                };
                let drill = if smd {
                    0
                } else {
                    match num("Drill (mm)") {
                        Some(d) if d > 0 && d < sx.min(sy) => d,
                        _ => {
                            return Err(set_error(
                                self,
                                "the drill is above 0 and smaller than the pad".into(),
                            ))
                        }
                    }
                };
                let f = self.fped_footprint().ok_or("open a footprint first")?;
                if f.pads
                    .iter()
                    .enumerate()
                    .any(|(i, p)| Some(i) != edit && p.number == number && p.at == (x, y))
                {
                    return Err(set_error(
                        self,
                        format!("pad {number} is already at that position"),
                    ));
                }
                self.close_dialog();
                self.ui.fped.smd = smd;
                self.ui.fped.shape = shape.clone();
                let pad = LibPad {
                    number: number.clone(),
                    kind: if smd {
                        PadKind::Smd
                    } else {
                        PadKind::ThroughHole
                    },
                    shape: pad_shape,
                    at: (x, y),
                    size: (sx, sy),
                    drill,
                };
                self.fped_edit(move |f| {
                    Ok(match edit {
                        Some(i) => {
                            f.pads[i] = pad;
                            format!("Changed pad {number}")
                        }
                        None => {
                            f.pads.push(pad);
                            format!("Added pad {number}")
                        }
                    })
                })
            }
            Dialog::FootprintFields { .. } => {
                let Some(height) = parse_mm(&dialog.value("Body height (mm)"), MM)
                    .filter(|h| *h > 0 && *h <= 50 * MM)
                else {
                    return Err(set_error(
                        self,
                        "the body height is above 0 and at most 50 mm".into(),
                    ));
                };
                let description = dialog.value("Description");
                self.close_dialog();
                self.fped_edit(move |f| {
                    f.description = description;
                    f.height = height;
                    Ok("Footprint properties changed".into())
                })
            }
            _ => Err("not a Footprint Editor dialog".into()),
        }
    }

    // ---- presentation ---------------------------------------------------------------
    pub(super) fn render_fped(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (w, h) = (env.width, env.height);
        let c = chrome(env.theme);
        p.scene.background = c.bar;
        let area = canvas_rect(w, h);
        let view = self.fped_view(area.width, area.height);
        let cv = Cv {
            ox: area.x,
            oy: area.y,
            w: area.width,
            h: area.height,
            view,
        };
        self.paint_footprint_canvas(p, &cv);
        p.region(
            area,
            &format!(
                "kicad:canvas:fped:{}:{}:{}",
                view.args(),
                area.width,
                area.height
            ),
            "Footprint canvas",
        );
        let top = w::MENU_H as i32;
        p.box_(Rect::new(0, top, w, w::TOOL_H), c.bar, 0);
        p.hline(0, top + w::TOOL_H as i32 - 1, w, w::EDGE);
        let has_lib = self.fped_lib().is_some();
        let has_fp = self.fped_footprint().is_some();
        let when = |ok: bool, t: &str, why: &'static str| -> Result<String, &'static str> {
            if ok {
                Ok(t.to_owned())
            } else {
                Err(why)
            }
        };
        let sel = self.fped_selection();
        let zoom_args = format!("{}:{}:{}", view.args(), area.width, area.height);
        let groups: Vec<Vec<w::ToolSpec>> = vec![
            vec![
                (
                    icons::new_project,
                    when(
                        self.session.project.is_some(),
                        "kicad:fped:new-lib",
                        "no project is open",
                    ),
                    "New library",
                ),
                (
                    icons::footprint_editor,
                    when(
                        has_lib,
                        "kicad:fped:new-footprint",
                        "choose a library first",
                    ),
                    "New footprint (Ctrl+N)",
                ),
                (
                    icons::save,
                    when(has_lib, "kicad:fped:save", "choose a library first"),
                    "Save library (Ctrl+S)",
                ),
            ],
            vec![
                (
                    icons::undo,
                    when(
                        !self.ui.fped.undo.is_empty(),
                        "kicad:fped:undo",
                        "nothing to undo",
                    ),
                    "Undo (Ctrl+Z)",
                ),
                (
                    icons::redo,
                    when(
                        !self.ui.fped.redo.is_empty(),
                        "kicad:fped:redo",
                        "nothing to redo",
                    ),
                    "Redo (Ctrl+Y)",
                ),
            ],
            vec![
                (
                    icons::zoom_in,
                    Ok(format!("kicad:fped:zoom:in:{zoom_args}")),
                    "Zoom in (F1)",
                ),
                (
                    icons::zoom_out,
                    Ok(format!("kicad:fped:zoom:out:{zoom_args}")),
                    "Zoom out (F2)",
                ),
                (
                    icons::zoom_fit,
                    Ok(format!("kicad:fped:zoom:fit:{zoom_args}")),
                    "Zoom to fit (Home)",
                ),
                (icons::grid, Ok("kicad:fped:grid".into()), "Change grid (N)"),
            ],
            vec![
                (
                    icons::properties,
                    when(has_fp, "kicad:fped:properties", "open a footprint first"),
                    "Footprint properties",
                ),
                (
                    icons::rotate,
                    when(
                        matches!(sel, Some(FpSel::Pad(_))),
                        "kicad:fped:rotate",
                        "select a pad first",
                    ),
                    "Rotate pad (R)",
                ),
                (
                    icons::delete,
                    when(sel.is_some(), "kicad:fped:delete", "nothing is selected"),
                    "Delete (Del)",
                ),
                (
                    icons::fill,
                    when(has_fp, "kicad:fped:fit-courtyard", "open a footprint first"),
                    "Fit the courtyard to the pads and body",
                ),
            ],
            vec![(
                icons::board,
                when(
                    self.session.project.is_some(),
                    "kicad:fped:board",
                    "no project is open",
                ),
                "Switch to PCB Editor",
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
        self.paint_fp_tree(p, &c, area);
        let rx = area.x + area.width as i32;
        p.box_(Rect::new(rx, area.y, w::SIDE_W, area.height), c.bar, 0);
        p.vline(rx, area.y, area.height, w::EDGE);
        let mut y = area.y + 4;
        for (tool, icon, tip) in TOOLS {
            let target = if tool == "select" || has_fp {
                Ok(format!("kicad:fped:tool:{tool}"))
            } else {
                Err("open or create a footprint first")
            };
            w::tool(
                p,
                &c,
                rx + 3,
                y,
                icon,
                target,
                tip,
                self.fped_tool() == tool,
            );
            y += 32;
        }
        let hv = self.ui.hover.unwrap_or_default();
        w::status_bar(
            p,
            &c,
            w,
            h,
            &[
                format!("Z {:.2}", view.zoom as f64 / 10.0),
                format!("X {} Y {}", mm_text(hv.x), mm_text(hv.y)),
                format!("grid {} mm", mm_text(self.ui.fped.grid)),
                "mm".into(),
                self.fped_tool().to_owned(),
                self.ui.status.clone(),
            ],
        );
        let menus = fped_menus(self);
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

    fn paint_fp_tree(&self, p: &mut Painter, c: &w::Chrome, area: Rect) {
        p.box_(Rect::new(0, area.y, LIB_W, area.height), c.panel, 0);
        p.vline(LIB_W as i32, area.y, area.height, w::EDGE);
        p.label(
            8,
            area.y + 6,
            LIB_W - 16,
            "Project Footprint Libraries",
            12,
            w::MUTED,
            true,
            Align::Left,
        );
        let mut y = area.y + 28;
        let bottom = area.y + area.height as i32;
        if self.session.fp_libs.is_empty() {
            p.paragraph(
                8,
                y,
                LIB_W - 16,
                if self.session.project.is_some() {
                    "The project has no footprint libraries yet. File ▸ New Library… makes one."
                } else {
                    "Open a project first: its libraries are listed here."
                },
                12,
                w::MUTED,
            );
            return;
        }
        for lib in &self.session.fp_libs {
            if y + 22 > bottom {
                break;
            }
            let chosen = self.ui.fped.lib.as_deref() == Some(lib.name.as_str());
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
                &format!("kicad:fped:lib:{}", lib.name),
                false,
                w::INK,
            );
            y += 23;
            if !chosen {
                continue;
            }
            if lib.footprints.is_empty() {
                p.label(
                    24,
                    y + 2,
                    LIB_W - 30,
                    "(empty; New Footprint adds one)",
                    12,
                    w::MUTED,
                    false,
                    Align::Left,
                );
                y += 22;
            }
            for f in &lib.footprints {
                if y + 22 > bottom {
                    break;
                }
                w::row(
                    p,
                    c,
                    Rect::new(20, y, LIB_W - 24, 22),
                    f.name(),
                    &format!("kicad:fped:open:{}", f.id),
                    self.ui.fped.footprint.as_deref() == Some(f.id.as_str()),
                    w::INK,
                );
                y += 23;
            }
        }
    }

    fn paint_footprint_canvas(&self, p: &mut Painter, cv: &Cv) {
        p.box_(Rect::new(cv.ox, cv.oy, cv.w, cv.h), super::pcb::PCB_BG, 0);
        let mut step = self.ui.fped.grid.max(1) / 1000;
        while cv.len(step) < 10 {
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
                    p.box_(Rect::new(x, y, 1, 1), Color::rgb(90, 90, 110), 0);
                    gx += step;
                }
                gy += step;
            }
        }
        let Some(f) = self.fped_footprint() else {
            p.label(
                cv.ox,
                cv.oy + cv.h as i32 / 2 - 10,
                cv.w,
                if self.session.fp_libs.is_empty() {
                    "Create a library (File ▸ New Library…), then a footprint in it"
                } else {
                    "Choose a footprint in the library tree, or File ▸ New Footprint…"
                },
                14,
                Color::rgb(200, 200, 210),
                false,
                Align::Center,
            );
            return;
        };
        let (ox, oy) = cv.pt(Pt::new(0, 0));
        let white = Color::rgb(230, 230, 230);
        p.line(vec![(ox - 8, oy), (ox + 8, oy)], white, 1);
        p.line(vec![(ox, oy - 8), (ox, oy + 8)], white, 1);
        let offset = match &self.ui.drag {
            Some(Drag::Move { start, at }) => at.sub(*start),
            _ => Pt::default(),
        };
        let sel = self.fped_selection();
        let shift = |s: FpSel, p: (i64, i64)| {
            if Some(s) == sel {
                (p.0 + offset.x, p.1 + offset.y)
            } else {
                p
            }
        };
        let rect_pts = |r: ((i64, i64), (i64, i64))| {
            let (a, b) = r;
            [a, (b.0, a.1), b, (a.0, b.1), a].map(um)
        };
        // Courtyard and fabrication outline.
        cv.stroke(p, &rect_pts(f.courtyard), layer_rgb(Layer::FCrtYd), 50, 1);
        cv.stroke(p, &rect_pts(f.fab), layer_rgb(Layer::FFab), 100, 1);
        // The reference placeholder on the silkscreen, above the courtyard.
        let (clo, chi) = norm_rect(f.courtyard.0, f.courtyard.1);
        cv.text(
            p,
            um(((clo.0 + chi.0) / 2, clo.1 - 700_000)),
            "REF**",
            1000,
            layer_rgb(Layer::FSilkS),
            Align::Center,
        );
        // Pads.
        for (i, pad) in f.pads.iter().enumerate() {
            let at = shift(FpSel::Pad(i), pad.at);
            let (x, y) = cv.pt(um(at));
            let (sw, sh) = (
                cv.len(pad.size.0 / 1000).max(2),
                cv.len(pad.size.1 / 1000).max(2),
            );
            let col = if pad.kind == PadKind::ThroughHole {
                Color::rgb(194, 162, 64)
            } else {
                layer_rgb(Layer::FCu)
            };
            match pad.shape {
                PadShape::Circle => p.circle(x, y, (sw / 2).max(1) as u32, col),
                PadShape::Rect => p.box_(
                    Rect::new(x - sw / 2, y - sh / 2, sw as u32, sh as u32),
                    col,
                    0,
                ),
                PadShape::Oval => p.box_(
                    Rect::new(x - sw / 2, y - sh / 2, sw as u32, sh as u32),
                    col,
                    (sw.min(sh) / 2) as u32,
                ),
                PadShape::RoundRect => p.box_(
                    Rect::new(x - sw / 2, y - sh / 2, sw as u32, sh as u32),
                    col,
                    (sw.min(sh) / 4) as u32,
                ),
            }
            if pad.drill > 0 {
                p.circle(
                    x,
                    y,
                    (cv.len(pad.drill / 1000) / 2).max(1) as u32,
                    Color::rgb(20, 20, 20),
                );
            }
            cv.text(
                p,
                um(at),
                &pad.number,
                (pad.size.0.min(pad.size.1) / 2000).max(200),
                white,
                Align::Center,
            );
            if sel == Some(FpSel::Pad(i)) {
                p.border(
                    Rect::new(x - sw / 2 - 3, y - sh / 2 - 3, sw as u32 + 6, sh as u32 + 6),
                    Color::TRANSPARENT,
                    2,
                    Color::rgb(255, 255, 255),
                );
            }
        }
        // Silkscreen.
        for (i, (a, b)) in f.silk.iter().enumerate() {
            let (a, b) = (shift(FpSel::Silk(i), *a), shift(FpSel::Silk(i), *b));
            let chosen = sel == Some(FpSel::Silk(i));
            cv.stroke(
                p,
                &[um(a), um(b)],
                if chosen {
                    Color::rgb(255, 255, 255)
                } else {
                    layer_rgb(Layer::FSilkS)
                },
                if chosen { 250 } else { 120 },
                1,
            );
        }
        // The shape being drawn.
        if let (Some(first), Some(hv)) = (self.ui.fped.points.first(), self.ui.hover) {
            let h = (hv.x, hv.y);
            match self.fped_tool() {
                "silk" => {
                    let mut pts: Vec<Pt> = self.ui.fped.points.iter().map(|q| um(*q)).collect();
                    pts.push(um(h));
                    cv.stroke(p, &pts, layer_rgb(Layer::FSilkS), 120, 1);
                }
                "fab" => cv.stroke(p, &rect_pts((*first, h)), layer_rgb(Layer::FFab), 100, 1),
                "courtyard" => {
                    cv.stroke(p, &rect_pts((*first, h)), layer_rgb(Layer::FCrtYd), 50, 1)
                }
                _ => {}
            }
        }
    }

    pub(super) fn fped_page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        for (id, label) in [
            ("kicad:fped:new-lib", "New Library"),
            ("kicad:fped:new-footprint", "New Footprint"),
            ("kicad:fped:save", "Save Library"),
            ("kicad:fped:properties", "Footprint Properties"),
            ("kicad:fped:fit-courtyard", "Fit Courtyard"),
            ("kicad:fped:tool:select", "Select"),
            ("kicad:fped:tool:pad", "Add Pad"),
            ("kicad:fped:tool:silk", "Draw Silkscreen"),
            ("kicad:fped:tool:fab", "Draw Fabrication Outline"),
            ("kicad:fped:tool:courtyard", "Draw Courtyard"),
            ("kicad:fped:grid", "Change Grid"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
                style: None,
            });
        }
        for lib in &self.session.fp_libs {
            let id = format!("kicad:fped:lib:{}", lib.name);
            page.elements.push(E::Button {
                id: id.clone(),
                text: format!(
                    "Library {} ({}/){}",
                    lib.name,
                    lib.dir,
                    if lib.dirty { ", unsaved" } else { "" }
                ),
                action: act(&id),
                style: None,
            });
            for f in &lib.footprints {
                let id = format!("kicad:fped:open:{}", f.id);
                page.elements.push(E::Button {
                    id: id.clone(),
                    text: format!("Footprint {}", f.id),
                    action: act(&id),
                    style: None,
                });
            }
        }
        if let Some(f) = self.fped_footprint() {
            page.elements.push(E::Text {
                id: "kicad-footprint".into(),
                text: format!(
                    "{}: {} pad(s), {} silkscreen segment(s), body height {} mm, courtyard {} mm × {} mm",
                    f.id,
                    f.pads.len(),
                    f.silk.len(),
                    mm_text(f.height),
                    mm_text((f.courtyard.1 .0 - f.courtyard.0 .0).abs()),
                    mm_text((f.courtyard.1 .1 - f.courtyard.0 .1).abs()),
                ),
            });
            for (i, pad) in f.pads.iter().enumerate() {
                page.elements.push(E::Text {
                    id: format!("kicad-pad-{i}"),
                    text: format!(
                        "Pad {} {} {} at ({}, {}) mm, {} × {} mm{}",
                        pad.number,
                        if pad.kind == PadKind::Smd {
                            "smd"
                        } else {
                            "thru_hole"
                        },
                        pad.shape.keyword(),
                        mm_text(pad.at.0),
                        mm_text(pad.at.1),
                        mm_text(pad.size.0),
                        mm_text(pad.size.1),
                        if pad.drill > 0 {
                            format!(", drill {} mm", mm_text(pad.drill))
                        } else {
                            String::new()
                        }
                    ),
                });
            }
        }
    }
}

fn fped_menus(k: &Kicad) -> Vec<(&'static str, Vec<MenuItem>)> {
    let lib = |t: &str| -> Result<String, &'static str> {
        if k.fped_lib().is_some() {
            Ok(t.to_owned())
        } else {
            Err("choose a library first")
        }
    };
    let fp = |t: &str| -> Result<String, &'static str> {
        if k.fped_footprint().is_some() {
            Ok(t.to_owned())
        } else {
            Err("open a footprint first")
        }
    };
    let sel = k.fped_selection();
    vec![
        (
            "File",
            vec![
                MenuItem::new(
                    "New Library...",
                    "",
                    if k.session.project.is_some() {
                        Ok("kicad:fped:new-lib".into())
                    } else {
                        Err("no project is open")
                    },
                ),
                MenuItem::new(
                    "New Footprint...",
                    "Ctrl+N",
                    lib("kicad:fped:new-footprint"),
                ),
                MenuItem::new("Save", "Ctrl+S", lib("kicad:fped:save")).sep(),
                MenuItem::new("Delete Footprint", "", fp("kicad:fped:delete-footprint")).sep(),
            ],
        ),
        (
            "Edit",
            vec![
                MenuItem::new(
                    "Undo",
                    "Ctrl+Z",
                    if k.ui.fped.undo.is_empty() {
                        Err("nothing to undo")
                    } else {
                        Ok("kicad:fped:undo".into())
                    },
                ),
                MenuItem::new(
                    "Redo",
                    "Ctrl+Y",
                    if k.ui.fped.redo.is_empty() {
                        Err("nothing to redo")
                    } else {
                        Ok("kicad:fped:redo".into())
                    },
                ),
                MenuItem::new(
                    "Delete",
                    "Del",
                    if sel.is_some() {
                        Ok("kicad:fped:delete".into())
                    } else {
                        Err("nothing is selected")
                    },
                )
                .sep(),
                MenuItem::new(
                    "Rotate Pad",
                    "R",
                    if matches!(sel, Some(FpSel::Pad(_))) {
                        Ok("kicad:fped:rotate".into())
                    } else {
                        Err("select a pad first")
                    },
                ),
                MenuItem::new("Footprint Properties...", "E", fp("kicad:fped:properties")).sep(),
                MenuItem::new("Fit Courtyard", "", fp("kicad:fped:fit-courtyard")),
            ],
        ),
        (
            "View",
            vec![
                MenuItem::new("Zoom In", "F1", Ok("kicad:fped:zoom:in".into())),
                MenuItem::new("Zoom Out", "F2", Ok("kicad:fped:zoom:out".into())),
                MenuItem::new("Zoom to Fit", "Home", Ok("kicad:fped:zoom:fit".into())),
                MenuItem::new("Change Grid", "N", Ok("kicad:fped:grid".into())).sep(),
            ],
        ),
        (
            "Place",
            vec![
                MenuItem::new("Pad", "", fp("kicad:fped:tool:pad")),
                MenuItem::new("Silkscreen Line", "", fp("kicad:fped:tool:silk")),
                MenuItem::new("Fabrication Outline", "", fp("kicad:fped:tool:fab")),
                MenuItem::new("Courtyard", "", fp("kicad:fped:tool:courtyard")),
            ],
        ),
        (
            "Help",
            vec![MenuItem::new("About KiCad", "", Ok("kicad:about".into()))],
        ),
    ]
}
