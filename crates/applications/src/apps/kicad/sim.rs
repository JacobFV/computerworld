//! The SPICE simulator window: the analysis command, running the schematic through
//! `cw_eda::spice`, the plot with its signals, two cursors and the console.
use super::widgets::{self as w, chrome, MenuItem};
use super::{act, icons, Dialog, Drag, Kicad};
use crate::desktop_scene::{shared::Align, Painter};
use crate::{AppEffect, PointerPhase};
use cw_eda::netlist;
use cw_eda::num::{format_si, parse_value};
use cw_eda::spice::{self, Analysis};
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

/// Longest a stored trace may be; longer results are thinned evenly, which a plot of a
/// few hundred pixels cannot tell from the original.
pub const TRACE_LIMIT: usize = 2001;
const RIGHT_W: u32 = 300;
const CONSOLE_H: u32 = 120;
const PALETTE: [Color; 7] = [
    Color::rgb(0, 114, 178),
    Color::rgb(213, 94, 0),
    Color::rgb(0, 158, 115),
    Color::rgb(204, 121, 167),
    Color::rgb(230, 159, 0),
    Color::rgb(86, 180, 233),
    Color::rgb(120, 110, 20),
];

/// One plotted quantity. Values are f32 bit patterns: exact in every snapshot, and
/// plenty for a plot and its cursors.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trace {
    pub name: String,
    pub y: Vec<u32>,
    /// AC phase in degrees; empty for every other analysis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phase: Vec<u32>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plot {
    /// `op`, `dc`, `ac` or `tran`.
    pub kind: String,
    pub x_label: String,
    pub x: Vec<u32>,
    pub traces: Vec<Trace>,
}
pub fn bits(v: f64) -> u32 {
    (v as f32).to_bits()
}
pub fn val(b: u32) -> f64 {
    f32::from_bits(b) as f64
}
impl Plot {
    pub fn trace(&self, name: &str) -> Option<&Trace> {
        self.traces
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }
    pub fn xs(&self) -> Vec<f64> {
        self.x.iter().map(|b| val(*b)).collect()
    }
    /// A trace's value at `x`, linearly interpolated.
    pub fn value_at(&self, name: &str, x: f64) -> Option<f64> {
        let t = self.trace(name)?;
        let xs = self.xs();
        if xs.is_empty() {
            return t.y.first().map(|b| val(*b));
        }
        let i = xs.partition_point(|v| *v < x);
        if i == 0 {
            return Some(val(t.y[0]));
        }
        if i >= xs.len() {
            return t.y.last().map(|b| val(*b));
        }
        let (x0, x1) = (xs[i - 1], xs[i]);
        let (y0, y1) = (val(t.y[i - 1]), val(t.y[i]));
        Some(if x1 == x0 {
            y1
        } else {
            y0 + (y1 - y0) * (x - x0) / (x1 - x0)
        })
    }
    fn from_result(r: &spice::SimResult) -> Plot {
        let kind = match r.analysis {
            Analysis::Op => "op",
            Analysis::Dc { .. } => "dc",
            Analysis::Ac { .. } => "ac",
            Analysis::Tran { .. } => "tran",
        };
        let n = r.x.len();
        let stride = n.div_ceil(TRACE_LIMIT).max(1);
        let keep: Vec<usize> = if n == 0 {
            vec![0]
        } else {
            let mut v: Vec<usize> = (0..n).step_by(stride).collect();
            if v.last() != Some(&(n - 1)) {
                v.push(n - 1);
            }
            v
        };
        let x = if n == 0 {
            vec![]
        } else {
            keep.iter().map(|i| bits(r.x[*i])).collect()
        };
        let traces = r
            .signals
            .iter()
            .map(|s| {
                if kind == "ac" {
                    Trace {
                        name: s.name.clone(),
                        y: keep.iter().map(|i| bits(s.db(*i))).collect(),
                        phase: keep.iter().map(|i| bits(s.phase_deg(*i))).collect(),
                    }
                } else {
                    Trace {
                        name: s.name.clone(),
                        y: keep
                            .iter()
                            .filter_map(|i| s.re.get(*i))
                            .map(|v| bits(*v))
                            .collect(),
                        phase: vec![],
                    }
                }
            })
            .collect();
        Plot {
            kind: kind.into(),
            x_label: match kind {
                "tran" => "Time (s)".into(),
                "ac" => "Frequency (Hz)".into(),
                "dc" => format!("{} (V)", r.x_name),
                _ => String::new(),
            },
            x,
            traces,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimState {
    /// The analysis, as the schematic's SPICE directive (`.tran 10u 5m`).
    pub command: String,
    pub plot: Option<Plot>,
    /// Signals on the plot, in the order they were added.
    pub shown: Vec<String>,
    pub log: Vec<String>,
    /// The schematic's probe tool is armed.
    pub probing: bool,
    /// Cursor positions on the x axis (f32 bits); `None` hides the cursor.
    pub cursors: [Option<u32>; 2],
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimUi {}

fn sim_menus(k: &Kicad) -> Vec<(&'static str, Vec<MenuItem>)> {
    let ran = |t: &str| -> Result<String, &'static str> {
        if k.session.sim.plot.as_ref().is_some_and(|p| p.kind != "op") {
            Ok(t.to_owned())
        } else {
            Err("run a sweep or transient simulation first")
        }
    };
    vec![
        (
            "Simulation",
            vec![
                MenuItem::new("Run Simulation", "R", Ok("kicad:sim:run".into())),
                MenuItem::new(
                    "Simulation Analysis...",
                    "",
                    Ok("kicad:sim:settings".into()),
                ),
                MenuItem::new("Probe Schematic...", "P", Ok("kicad:sim:probe".into())).sep(),
                MenuItem::new("Add Signals...", "A", Ok("kicad:sim:signals".into())),
            ],
        ),
        (
            "View",
            vec![
                MenuItem::new("Show Cursor 1", "", ran("kicad:sim:cursor:0")),
                MenuItem::new("Show Cursor 2", "", ran("kicad:sim:cursor:1")),
            ],
        ),
        (
            "Help",
            vec![MenuItem::new("About KiCad", "", Ok("kicad:about".into()))],
        ),
    ]
}

/// Engineering-notation tick spacing: 1, 2 or 5 times a power of ten.
fn nice_step(span: f64, target: usize) -> f64 {
    if span <= 0.0 || !span.is_finite() {
        return 1.0;
    }
    let raw = span / target.max(1) as f64;
    let mag = cw_eda::num::pow10(floor(cw_eda::num::log10(raw)));
    let n = raw / mag;
    let m = if n < 1.5 {
        1.0
    } else if n < 3.5 {
        2.0
    } else if n < 7.5 {
        5.0
    } else {
        10.0
    };
    m * mag
}
fn floor(x: f64) -> f64 {
    let t = x as i64 as f64;
    if t > x {
        t - 1.0
    } else {
        t
    }
}

impl Kicad {
    fn run_simulation(&mut self) {
        let sim = &mut self.session.sim;
        sim.log.clear();
        let command = if sim.command.trim().is_empty() {
            self.session
                .schematic
                .sim_command()
                .map(|t| t.text.clone())
                .unwrap_or_default()
        } else {
            sim.command.clone()
        };
        if command.trim().is_empty() {
            self.session
                .sim
                .log
                .push("No simulation command: choose Simulation Analysis... to set one up.".into());
            self.ui.status = "No simulation command".into();
            return;
        }
        let name = self
            .session
            .project
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "untitled".into());
        let deck = match netlist::spice_netlist(&self.session.schematic, &name, Some(&command)) {
            Ok(d) => d,
            Err(e) => {
                for line in e.lines() {
                    self.session.sim.log.push(format!("Error: {line}"));
                }
                self.ui.status = "Simulation failed".into();
                return;
            }
        };
        let log = &mut self.session.sim.log;
        log.extend(deck.skipped.iter().cloned());
        let result = spice::parse(&deck.text).and_then(|c| {
            let a = c.analyses.first().cloned().unwrap_or(Analysis::Op);
            c.simulate(&a)
        });
        match result {
            Ok(r) => {
                log.extend(r.log.iter().cloned());
                let plot = Plot::from_result(&r);
                if plot.kind == "op" {
                    for t in &plot.traces {
                        let unit = if t.name.starts_with("I(") { "A" } else { "V" };
                        log.push(format!("{}: {}", t.name, format_si(val(t.y[0]), 4, unit)));
                    }
                }
                // Signals that no longer exist (a renamed net) drop off the plot.
                self.session.sim.shown.retain(|s| plot.trace(s).is_some());
                if self.session.sim.shown.is_empty() && plot.kind != "op" {
                    log.push("Probe the schematic or use Add Signals to plot a signal.".into());
                }
                self.session.sim.cursors = [None, None];
                self.ui.status = format!("Simulation complete ({})", command);
                self.session.sim.plot = Some(plot);
                self.session.sim.command = command;
            }
            Err(e) => {
                log.push(format!("Error: {e}"));
                self.ui.status = "Simulation failed".into();
            }
        }
    }
    /// Signals the plot could show: the last run's, or every net of the schematic.
    fn available_signals(&self) -> Vec<String> {
        if let Some(p) = &self.session.sim.plot {
            return p.traces.iter().map(|t| t.name.clone()).collect();
        }
        cw_eda::connectivity::analyze(&self.session.schematic)
            .nets
            .iter()
            .map(|n| netlist::spice_node(&n.name))
            .filter(|n| n != "0")
            .map(|n| format!("V({n})"))
            .collect()
    }
    fn x_range(&self) -> Option<(f64, f64)> {
        let p = self.session.sim.plot.as_ref()?;
        let xs = p.xs();
        Some((*xs.first()?, *xs.last()?))
    }
    pub(super) fn sim_command(
        &mut self,
        window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let parts: Vec<&str> = rest.split(':').collect();
        match parts[0] {
            "run" => {
                self.run_simulation();
                Ok(vec![])
            }
            "settings" => {
                self.open_sim_settings();
                Ok(vec![])
            }
            "probe" => {
                self.session.sim.probing = true;
                self.ui.status = "Click a wire or pin in the schematic to plot it".into();
                self.launch_frame(window, "sch")
            }
            "signals" => {
                self.ui.dialog = Some(Dialog::AddSignals {
                    chosen: self.session.sim.shown.clone(),
                });
                Ok(vec![])
            }
            "toggle" => {
                let name = parts[1..].join(":");
                if let Some(i) = self.session.sim.shown.iter().position(|s| *s == name) {
                    self.session.sim.shown.remove(i);
                } else {
                    return Err(format!("{name} is not on the plot"));
                }
                Ok(vec![])
            }
            "cursor" => {
                let i: usize = parts
                    .get(1)
                    .and_then(|v| v.parse().ok())
                    .filter(|i| *i < 2)
                    .ok_or("bad cursor")?;
                let (x0, x1) = self.x_range().ok_or("run a simulation first")?;
                self.session.sim.cursors[i] = match self.session.sim.cursors[i] {
                    Some(_) => None,
                    None => Some(bits(x0 + (x1 - x0) * if i == 0 { 0.25 } else { 0.75 })),
                };
                Ok(vec![])
            }
            other => Err(format!("unknown simulator command {other}")),
        }
    }
    pub(super) fn sim_key(&mut self, window: u64, key: &str) -> Result<Vec<AppEffect>, String> {
        match key {
            "r" | "R" | "F5" => self.sim_command(window, "run"),
            "p" | "P" => self.sim_command(window, "probe"),
            "a" | "A" => self.sim_command(window, "signals"),
            other => Err(format!("unsupported simulator key {other}")),
        }
    }
    /// The plot canvas: dragging moves the nearest cursor to the pointer.
    pub(super) fn plot_pointer(
        &mut self,
        phase: PointerPhase,
        args: &[&str],
        x: i32,
    ) -> Result<Vec<AppEffect>, String> {
        let x0: f64 = args
            .first()
            .and_then(|v| v.parse().ok())
            .ok_or("bad plot")?;
        let x1: f64 = args.get(1).and_then(|v| v.parse().ok()).ok_or("bad plot")?;
        let width: f64 = args
            .get(2)
            .and_then(|v| v.parse().ok())
            .filter(|w: &f64| *w > 0.0)
            .ok_or("bad plot")?;
        let log = args.get(3) == Some(&"log");
        let t = (x as f64 / width).clamp(0.0, 1.0);
        let at = if log && x0 > 0.0 && x1 > 0.0 {
            cw_eda::num::pow10(
                cw_eda::num::log10(x0) + (cw_eda::num::log10(x1) - cw_eda::num::log10(x0)) * t,
            )
        } else {
            x0 + (x1 - x0) * t
        };
        match phase {
            PointerPhase::Down => {
                let cursors = self.session.sim.cursors;
                // Grab the nearest cursor that is showing, else bring up cursor 1.
                let index = (0..2)
                    .filter(|i| cursors[*i].is_some())
                    .min_by(|a, b| {
                        let da = (val(cursors[*a].unwrap()) - at).abs();
                        let db = (val(cursors[*b].unwrap()) - at).abs();
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap_or(0) as u8;
                self.ui.drag = Some(Drag::Cursor { index });
                self.session.sim.cursors[index as usize] = Some(bits(at));
            }
            PointerPhase::Move | PointerPhase::Up => {
                if let Some(Drag::Cursor { index }) = self.ui.drag {
                    self.session.sim.cursors[index as usize] = Some(bits(at));
                }
                if phase == PointerPhase::Up {
                    self.ui.drag = None;
                }
            }
            PointerPhase::Cancel => self.ui.drag = None,
        }
        Ok(vec![])
    }
    fn open_sim_settings(&mut self) {
        let cmd = if self.session.sim.command.is_empty() {
            self.session
                .schematic
                .sim_command()
                .map(|t| t.text.clone())
                .unwrap_or_default()
        } else {
            self.session.sim.command.clone()
        };
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let get = |i: usize, d: &str| {
            words
                .get(i)
                .map(|s| s.to_string())
                .unwrap_or_else(|| d.into())
        };
        let head = words
            .first()
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let tab = match head.as_str() {
            ".op" => 0,
            ".dc" => 1,
            ".ac" => 2,
            _ => 3,
        };
        let is = |h: &str| head == h;
        let source = self
            .session
            .schematic
            .symbols
            .iter()
            .find(|s| {
                s.lib().is_some_and(|l| {
                    matches!(
                        l.spice,
                        cw_eda::symbols::Spice::VoltageSource
                            | cw_eda::symbols::Spice::CurrentSource
                    )
                })
            })
            .map(|s| s.reference().to_owned())
            .unwrap_or_else(|| "V1".into());
        self.ui.dialog = Some(Dialog::SimSettings {
            tab,
            fields: vec![
                (
                    "Source".into(),
                    if is(".dc") { get(1, &source) } else { source },
                ),
                (
                    "Start value".into(),
                    if is(".dc") { get(2, "0") } else { "0".into() },
                ),
                (
                    "Stop value".into(),
                    if is(".dc") { get(3, "5") } else { "5".into() },
                ),
                (
                    "Increment".into(),
                    if is(".dc") {
                        get(4, "0.1")
                    } else {
                        "0.1".into()
                    },
                ),
                (
                    "Sweep type".into(),
                    if is(".ac") {
                        get(1, "dec")
                    } else {
                        "dec".into()
                    },
                ),
                (
                    "Points per decade".into(),
                    if is(".ac") { get(2, "10") } else { "10".into() },
                ),
                (
                    "Start frequency".into(),
                    if is(".ac") { get(3, "1") } else { "1".into() },
                ),
                (
                    "Stop frequency".into(),
                    if is(".ac") {
                        get(4, "1Meg")
                    } else {
                        "1Meg".into()
                    },
                ),
                (
                    "Time step".into(),
                    if is(".tran") {
                        get(1, "10u")
                    } else {
                        "10u".into()
                    },
                ),
                (
                    "Final time".into(),
                    if is(".tran") {
                        get(2, "5m")
                    } else {
                        "5m".into()
                    },
                ),
                (
                    "Initial time".into(),
                    if is(".tran") { get(3, "0") } else { "0".into() },
                ),
            ],
            error: String::new(),
        });
        self.ui.focus = None;
    }
    pub(super) fn sim_dialog(
        &mut self,
        _window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let dialog = self.ui.dialog.clone().ok_or("no dialog is open")?;
        let (cmd, arg) = rest.split_once(':').unwrap_or((rest, ""));
        match dialog {
            Dialog::SimSettings {
                tab, mut fields, ..
            } => match cmd {
                "tab" => {
                    let t: u8 = arg.parse().map_err(|_| "bad tab")?;
                    self.ui.dialog = Some(Dialog::SimSettings {
                        tab: t.min(3),
                        fields,
                        error: String::new(),
                    });
                    self.ui.focus = None;
                    Ok(vec![])
                }
                "sweep" => {
                    if !matches!(arg, "dec" | "oct" | "lin") {
                        return Err("unknown sweep type".into());
                    }
                    if let Some(f) = fields.iter_mut().find(|(k, _)| k == "Sweep type") {
                        f.1 = arg.into();
                    }
                    self.ui.dialog = Some(Dialog::SimSettings {
                        tab,
                        fields,
                        error: String::new(),
                    });
                    Ok(vec![])
                }
                "ok" => {
                    let get = |k: &str| {
                        fields
                            .iter()
                            .find(|(n, _)| n == k)
                            .map(|(_, v)| v.trim().to_owned())
                            .unwrap_or_default()
                    };
                    let numbers: &[&str] = match tab {
                        1 => &["Start value", "Stop value", "Increment"],
                        2 => &["Points per decade", "Start frequency", "Stop frequency"],
                        3 => &["Time step", "Final time", "Initial time"],
                        _ => &[],
                    };
                    for n in numbers {
                        let v = get(n);
                        if parse_value(&v).is_none() {
                            self.ui.dialog = Some(Dialog::SimSettings {
                                tab,
                                error: format!("{n}: '{v}' is not a number"),
                                fields,
                            });
                            return Ok(vec![]);
                        }
                    }
                    let command = match tab {
                        0 => ".op".to_owned(),
                        1 => format!(
                            ".dc {} {} {} {}",
                            get("Source"),
                            get("Start value"),
                            get("Stop value"),
                            get("Increment")
                        ),
                        2 => format!(
                            ".ac {} {} {} {}",
                            get("Sweep type"),
                            get("Points per decade"),
                            get("Start frequency"),
                            get("Stop frequency")
                        ),
                        _ => {
                            let init = get("Initial time");
                            if parse_value(&init) == Some(0.0) {
                                format!(".tran {} {}", get("Time step"), get("Final time"))
                            } else {
                                format!(".tran {} {} {init}", get("Time step"), get("Final time"))
                            }
                        }
                    };
                    // Check it the way the simulator will read it.
                    if let Err(e) = spice::parse(&format!("check\nR1 a 0 1\n{command}\n.end")) {
                        self.ui.dialog = Some(Dialog::SimSettings {
                            tab,
                            error: e,
                            fields,
                        });
                        return Ok(vec![]);
                    }
                    // The command lives on the sheet as a SPICE directive, as in KiCad.
                    let before = self.session.schematic.clone();
                    self.session.sch_history.record(&before);
                    self.session.schematic.set_sim_command(&command);
                    self.session.sch_dirty = true;
                    self.session.sim.command = command.clone();
                    self.ui.status = format!("Simulation command: {command}");
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown analysis command {other}")),
            },
            Dialog::AddSignals { mut chosen } => match cmd {
                "signal" => {
                    let name = arg.to_owned();
                    if !self.available_signals().contains(&name) {
                        return Err(format!("{name} is not a signal of this circuit"));
                    }
                    if let Some(i) = chosen.iter().position(|s| *s == name) {
                        chosen.remove(i);
                    } else {
                        chosen.push(name);
                    }
                    self.ui.dialog = Some(Dialog::AddSignals { chosen });
                    Ok(vec![])
                }
                "ok" => {
                    self.session.sim.shown = chosen;
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown signals command {other}")),
            },
            _ => Err("not a simulator dialog".into()),
        }
    }

    // ---- painting ---------------------------------------------------------------------
    pub(super) fn render_sim(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (w, h) = (env.width, env.height);
        let c = chrome(env.theme);
        p.scene.background = c.panel;
        let menus = sim_menus(self);
        let titles: Vec<&str> = menus.iter().map(|m| m.0).collect();
        let top = w::MENU_H as i32;
        p.box_(Rect::new(0, top, w, w::TOOL_H), c.bar, 0);
        p.hline(0, top + w::TOOL_H as i32 - 1, w, w::EDGE);
        let ran = self
            .session
            .sim
            .plot
            .as_ref()
            .is_some_and(|p| p.kind != "op");
        let cursor = |i: usize| -> Result<String, &'static str> {
            if ran {
                Ok(format!("kicad:sim:cursor:{i}"))
            } else {
                Err("run a sweep or transient simulation first")
            }
        };
        let mut x = 6;
        let ty = top + 3;
        for (i, (icon, target, tip, on)) in [
            (
                icons::settings as w::Icon,
                Ok("kicad:sim:settings".to_owned()),
                "Simulation analysis (Sim Command)",
                false,
            ),
            (
                icons::run,
                Ok("kicad:sim:run".to_owned()),
                "Run simulation (R)",
                false,
            ),
            (
                icons::probe,
                Ok("kicad:sim:probe".to_owned()),
                "Probe signals on the schematic (P)",
                self.session.sim.probing,
            ),
            (
                icons::add_signals,
                Ok("kicad:sim:signals".to_owned()),
                "Add signals to the plot (A)",
                false,
            ),
            (
                icons::cursor,
                cursor(0),
                "Show cursor 1",
                self.session.sim.cursors[0].is_some(),
            ),
            (
                icons::cursor,
                cursor(1),
                "Show cursor 2",
                self.session.sim.cursors[1].is_some(),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            if i == 2 || i == 4 {
                w::separator_v(p, x + 2, ty);
                x += 8;
            }
            w::tool(p, &c, x, ty, icon, target, tip, on);
            x += 30;
        }
        let cmd = if self.session.sim.command.is_empty() {
            self.session
                .schematic
                .sim_command()
                .map(|t| t.text.clone())
                .unwrap_or_else(|| "(no analysis)".into())
        } else {
            self.session.sim.command.clone()
        };
        p.label(x + 12, ty + 5, 300, &cmd, 13, w::MUTED, false, Align::Left);
        // Plot.
        let body_top = top + w::TOOL_H as i32;
        let pw = w.saturating_sub(RIGHT_W + 1);
        let ph = h.saturating_sub(body_top as u32 + CONSOLE_H + 1);
        let area = Rect::new(0, body_top, pw, ph);
        p.box_(area, w::WHITE, 0);
        self.paint_plot(p, area);
        // Console.
        let cy = body_top + ph as i32;
        p.box_(
            Rect::new(0, cy, pw, CONSOLE_H),
            Color::rgb(252, 252, 252),
            0,
        );
        p.hline(0, cy, pw, w::EDGE);
        let lines = &self.session.sim.log;
        let visible = (CONSOLE_H as usize - 8) / 16;
        for (i, line) in lines
            .iter()
            .skip(lines.len().saturating_sub(visible))
            .enumerate()
        {
            let color = if line.starts_with("Error") {
                Color::rgb(190, 30, 30)
            } else {
                w::INK
            };
            p.label(
                8,
                cy + 4 + i as i32 * 16,
                pw.saturating_sub(16),
                line,
                12,
                color,
                false,
                Align::Left,
            );
        }
        // Signals and cursors.
        let rx = pw as i32 + 1;
        p.box_(Rect::new(rx, body_top, RIGHT_W, h), c.panel, 0);
        p.vline(rx - 1, body_top, h, w::EDGE);
        p.label(
            rx + 8,
            body_top + 6,
            RIGHT_W - 16,
            "Signals",
            13,
            w::INK,
            true,
            Align::Left,
        );
        let mut y = body_top + 28;
        for (i, name) in self.session.sim.shown.iter().enumerate() {
            let color = PALETTE[i % PALETTE.len()];
            w::checkbox(
                p,
                &c,
                rx + 8,
                y,
                "",
                true,
                &format!("kicad:sim:toggle:{name}"),
            );
            p.box_(Rect::new(rx + 32, y + 5, 18, 10), color, 2);
            p.label(
                rx + 56,
                y + 1,
                RIGHT_W - 64,
                name,
                13,
                w::INK,
                false,
                Align::Left,
            );
            y += 24;
        }
        if self.session.sim.shown.is_empty() {
            p.paragraph(
                rx + 8,
                y,
                RIGHT_W - 16,
                "No signals. Probe the schematic or Add Signals.",
                12,
                w::MUTED,
            );
            y += 36;
        }
        y += 10;
        p.label(
            rx + 8,
            y,
            RIGHT_W - 16,
            "Cursors",
            13,
            w::INK,
            true,
            Align::Left,
        );
        y += 22;
        if let Some(plot) = &self.session.sim.plot {
            for (i, cx) in self.session.sim.cursors.iter().enumerate() {
                let Some(cx) = cx else { continue };
                let xv = val(*cx);
                let xunit = match plot.kind.as_str() {
                    "tran" => "s",
                    "ac" => "Hz",
                    _ => "V",
                };
                p.label(
                    rx + 8,
                    y,
                    RIGHT_W - 16,
                    &format!("Cursor {}: {}", i + 1, format_si(xv, 4, xunit)),
                    12,
                    w::INK,
                    true,
                    Align::Left,
                );
                y += 18;
                for name in &self.session.sim.shown {
                    if let Some(v) = plot.value_at(name, xv) {
                        let unit = if plot.kind == "ac" {
                            "dB"
                        } else if name.starts_with("I(") {
                            "A"
                        } else {
                            "V"
                        };
                        let text = if unit == "dB" {
                            format!("{name}: {v:.2} dB")
                        } else {
                            format!("{name}: {}", format_si(v, 4, unit))
                        };
                        p.label(
                            rx + 16,
                            y,
                            RIGHT_W - 24,
                            &text,
                            12,
                            w::MUTED,
                            false,
                            Align::Left,
                        );
                        y += 16;
                    }
                }
                y += 4;
            }
            if let (Some(a), Some(b)) = (self.session.sim.cursors[0], self.session.sim.cursors[1]) {
                p.label(
                    rx + 8,
                    y,
                    RIGHT_W - 16,
                    &format!(
                        "Diff: {}",
                        format_si(
                            val(b) - val(a),
                            4,
                            if plot.kind == "tran" { "s" } else { "" }
                        )
                    ),
                    12,
                    w::INK,
                    false,
                    Align::Left,
                );
            }
        }
        let xs = w::menubar(p, &c, w, &titles, self.ui.menu.as_deref());
        if let Some(open) = &self.ui.menu {
            if let Some(i) = titles.iter().position(|t| t == open) {
                p.z += 40;
                w::menu_panel(p, &c, xs[i], &menus[i].1);
                p.z -= 40;
            }
        }
    }

    fn paint_plot(&self, p: &mut Painter, area: Rect) {
        let Some(plot) = &self.session.sim.plot else {
            p.label(
                area.x,
                area.y + area.height as i32 / 2 - 10,
                area.width,
                "Run a simulation to see its results here",
                14,
                w::MUTED,
                false,
                Align::Center,
            );
            return;
        };
        if plot.kind == "op" {
            p.label(
                area.x + 20,
                area.y + 20,
                area.width - 40,
                "Operating point",
                14,
                w::INK,
                true,
                Align::Left,
            );
            for (i, t) in plot.traces.iter().enumerate() {
                let unit = if t.name.starts_with("I(") { "A" } else { "V" };
                p.label(
                    area.x + 20,
                    area.y + 48 + i as i32 * 20,
                    area.width - 40,
                    &format!("{} = {}", t.name, format_si(val(t.y[0]), 4, unit)),
                    13,
                    w::INK,
                    false,
                    Align::Left,
                );
            }
            return;
        }
        let (ml, mr, mt, mb) = (70, if plot.kind == "ac" { 60 } else { 20 }, 20, 44);
        let inner = Rect::new(
            area.x + ml,
            area.y + mt,
            area.width.saturating_sub((ml + mr) as u32),
            area.height.saturating_sub((mt + mb) as u32),
        );
        let xs = plot.xs();
        let (x0, x1) = (
            xs.first().copied().unwrap_or(0.0),
            xs.last().copied().unwrap_or(1.0),
        );
        let log = plot.kind == "ac" && x0 > 0.0;
        let lx = |v: f64| if log { cw_eda::num::log10(v) } else { v };
        let (lx0, lx1) = (lx(x0), lx(x1).max(lx(x0) + 1e-30));
        let px = |v: f64| inner.x + ((lx(v) - lx0) / (lx1 - lx0) * inner.width as f64) as i32;
        // Y range over the shown traces.
        let shown: Vec<&Trace> = self
            .session
            .sim
            .shown
            .iter()
            .filter_map(|n| plot.trace(n))
            .collect();
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for t in &shown {
            for b in &t.y {
                let v = val(*b);
                if v.is_finite() {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        if !lo.is_finite() {
            lo = 0.0;
            hi = 1.0;
        }
        if hi - lo < 1e-12 {
            hi += 0.5;
            lo -= 0.5;
        }
        let pad = (hi - lo) * 0.08;
        let (y0, y1) = (lo - pad, hi + pad);
        let py = |v: f64| {
            inner.y + inner.height as i32 - ((v - y0) / (y1 - y0) * inner.height as f64) as i32
        };
        // Grid and ticks.
        let grid = Color::rgb(225, 225, 225);
        let ystep = nice_step(y1 - y0, 6);
        let mut v = (y0 / ystep).ceil() * ystep;
        let yunit = if plot.kind == "ac" { "dB" } else { "V" };
        while v <= y1 {
            let y = py(v);
            p.hline(inner.x, y, inner.width, grid);
            let text = if plot.kind == "ac" {
                format!("{v:.0}")
            } else {
                format_si(v, 3, "")
            };
            p.label(
                area.x + 4,
                y - 8,
                ml as u32 - 10,
                &text,
                11,
                w::MUTED,
                false,
                Align::Right,
            );
            v += ystep;
        }
        if log {
            let mut decade = cw_eda::num::pow10(floor(cw_eda::num::log10(x0)));
            while decade <= x1 * 1.0001 {
                if decade >= x0 * 0.9999 {
                    let x = px(decade);
                    p.vline(x, inner.y, inner.height, grid);
                    p.label(
                        x - 30,
                        inner.y + inner.height as i32 + 4,
                        60,
                        &format_si(decade, 3, ""),
                        11,
                        w::MUTED,
                        false,
                        Align::Center,
                    );
                }
                decade *= 10.0;
            }
        } else {
            let xstep = nice_step(x1 - x0, 8);
            let mut v = (x0 / xstep).ceil() * xstep;
            while v <= x1 * 1.000_001 {
                let x = px(v);
                p.vline(x, inner.y, inner.height, grid);
                p.label(
                    x - 30,
                    inner.y + inner.height as i32 + 4,
                    60,
                    &format_si(v, 3, ""),
                    11,
                    w::MUTED,
                    false,
                    Align::Center,
                );
                v += xstep;
            }
        }
        p.border(inner, Color::TRANSPARENT, 0, Color::rgb(120, 120, 120));
        p.label(
            inner.x,
            area.y + area.height as i32 - 20,
            inner.width,
            &plot.x_label,
            12,
            w::INK,
            false,
            Align::Center,
        );
        p.label(
            area.x + 4,
            area.y + 2,
            ml as u32,
            yunit,
            11,
            w::INK,
            false,
            Align::Left,
        );
        // Traces.
        for (i, name) in self.session.sim.shown.iter().enumerate() {
            let Some(t) = plot.trace(name) else { continue };
            let color = PALETTE[i % PALETTE.len()];
            let pts: Vec<(i32, i32)> = xs
                .iter()
                .zip(&t.y)
                .map(|(x, y)| (px(*x), py(val(*y))))
                .collect();
            p.line(pts, color, 2);
            if !t.phase.is_empty() {
                // Phase on the right axis, dashed by drawing every other run.
                let pv = |v: f64| {
                    inner.y + inner.height as i32
                        - ((v + 180.0) / 360.0 * inner.height as f64) as i32
                };
                let pts: Vec<(i32, i32)> = xs
                    .iter()
                    .zip(&t.phase)
                    .map(|(x, y)| (px(*x), pv(val(*y))))
                    .collect();
                for (k, pair) in pts.windows(2).enumerate() {
                    if k % 2 == 0 {
                        p.line(pair.to_vec(), Color(color.0, color.1, color.2, 150), 1);
                    }
                }
            }
        }
        if plot.kind == "ac" {
            for deg in [-180, -90, 0, 90, 180] {
                let y = inner.y + inner.height as i32
                    - ((deg as f64 + 180.0) / 360.0 * inner.height as f64) as i32;
                p.label(
                    inner.x + inner.width as i32 + 4,
                    y - 8,
                    mr as u32 - 6,
                    &format!("{deg}°"),
                    11,
                    w::MUTED,
                    false,
                    Align::Left,
                );
            }
        }
        // Cursors.
        for (i, c) in self.session.sim.cursors.iter().enumerate() {
            if let Some(c) = c {
                let x = px(val(*c).clamp(x0, x1));
                p.vline(x, inner.y, inner.height, Color::rgb(80, 80, 80));
                p.label(
                    x + 3,
                    inner.y + 2,
                    30,
                    &format!("{}", i + 1),
                    11,
                    w::INK,
                    true,
                    Align::Left,
                );
            }
        }
        // The drag surface carries the axis it maps onto.
        p.region(
            inner,
            &format!(
                "kicad:canvas:plot:{x0}:{x1}:{}:{}",
                inner.width,
                if log { "log" } else { "lin" }
            ),
            "Plot",
        );
    }

    pub(super) fn render_sim_dialog(
        &self,
        p: &mut Painter,
        env: &crate::AppEnv<'_>,
        dialog: &Dialog,
    ) {
        let c = chrome(env.theme);
        let (w, h) = (env.width, env.height);
        let focus = self.ui.focus.as_deref();
        match dialog {
            Dialog::SimSettings { tab, fields, error } => {
                let body = w::dialog(p, &c, env.theme, w, h, 560, 380, dialog.title());
                let y = w::tabs(
                    p,
                    &c,
                    body.x,
                    body.y,
                    &["Operating Point", "DC Sweep", "AC", "Transient"],
                    *tab as usize,
                    "kicad:dlg:tab:",
                );
                let names: &[&str] = match tab {
                    1 => &["Source", "Start value", "Stop value", "Increment"],
                    2 => &["Points per decade", "Start frequency", "Stop frequency"],
                    3 => &["Time step", "Final time", "Initial time"],
                    _ => &[],
                };
                let mut fy = y + 8;
                if *tab == 0 {
                    p.paragraph(body.x, fy, body.width, "Compute the DC operating point: every node voltage and source current with capacitors open and inductors shorted.", 13, w::INK);
                }
                if *tab == 2 {
                    let sweep = fields
                        .iter()
                        .find(|(k, _)| k == "Sweep type")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("dec");
                    p.label(
                        body.x,
                        fy + 1,
                        150,
                        "Sweep type:",
                        13,
                        w::INK,
                        false,
                        Align::Left,
                    );
                    for (i, (key, label)) in
                        [("dec", "Decade"), ("oct", "Octave"), ("lin", "Linear")]
                            .iter()
                            .enumerate()
                    {
                        w::radio(
                            p,
                            &c,
                            body.x + 150 + i as i32 * 100,
                            fy,
                            label,
                            sweep == *key,
                            &format!("kicad:dlg:sweep:{key}"),
                        );
                    }
                    fy += 30;
                }
                for n in names {
                    let v = fields
                        .iter()
                        .find(|(k, _)| k == n)
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    w::label_field(p, &c, body.x, fy, 150, 180, n, v, n, focus == Some(*n));
                    fy += 34;
                }
                if !error.is_empty() {
                    p.label(
                        body.x,
                        body.y + body.height as i32 - 58,
                        body.width,
                        error,
                        12,
                        Color::rgb(190, 30, 30),
                        false,
                        Align::Left,
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
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                    "OK",
                    "kicad:dlg:ok",
                    true,
                );
            }
            Dialog::AddSignals { chosen } => {
                let body = w::dialog(p, &c, env.theme, w, h, 420, 420, dialog.title());
                let list = Rect::new(body.x, body.y, body.width, body.height - 42);
                p.border(list, w::WHITE, 2, w::EDGE);
                for (i, name) in self.available_signals().iter().enumerate() {
                    let y = list.y + 4 + i as i32 * 24;
                    if y + 24 > list.y + list.height as i32 {
                        break;
                    }
                    w::checkbox(
                        p,
                        &c,
                        list.x + 8,
                        y,
                        name,
                        chosen.contains(name),
                        &format!("kicad:dlg:signal:{name}"),
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
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                    "OK",
                    "kicad:dlg:ok",
                    true,
                );
            }
            _ => {}
        }
    }

    pub(super) fn sim_page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        for (id, label) in [
            ("kicad:sim:settings", "Simulation Analysis"),
            ("kicad:sim:run", "Run Simulation"),
            ("kicad:sim:probe", "Probe"),
            ("kicad:sim:signals", "Add Signals"),
            ("kicad:sim:cursor:0", "Cursor 1"),
            ("kicad:sim:cursor:1", "Cursor 2"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
                style: None,
            });
        }
        page.elements.push(E::Text {
            id: "kicad-sim-command".into(),
            text: self.session.sim.command.clone(),
        });
        for (i, line) in self.session.sim.log.iter().enumerate() {
            page.elements.push(E::Text {
                id: format!("kicad-sim-log-{i}"),
                text: line.clone(),
            });
        }
        if let Some(plot) = &self.session.sim.plot {
            for (i, c) in self.session.sim.cursors.iter().enumerate() {
                let Some(c) = c else { continue };
                for name in &self.session.sim.shown {
                    if let Some(v) = plot.value_at(name, val(*c)) {
                        page.elements.push(E::Text {
                            id: format!("kicad-cursor-{i}-{name}"),
                            text: format!("Cursor {} at {}: {name} = {v}", i + 1, val(*c)),
                        });
                    }
                }
            }
        }
        if let Some(Dialog::AddSignals { .. }) = &self.ui.dialog {
            for name in self.available_signals() {
                let id = format!("kicad:dlg:signal:{name}");
                page.elements.push(E::Button {
                    id: id.clone(),
                    text: name,
                    action: act(&id),
                    style: None,
                });
            }
        }
    }
}
