//! A SPICE-class mixed-signal circuit simulator.
//!
//! Modified nodal analysis over a dense LU with partial pivoting. Nonlinear devices are
//! linearised by Newton–Raphson with SPICE3's junction limiting, falling back to gmin
//! and source stepping when a plain Newton solve of the operating point fails.
//! Analyses: DC operating point, DC sweep, AC small signal (complex MNA) and transient.
//!
//! The transient integrates every charge (capacitors, inductor flux, junction and
//! transit-time charge, Meyer gate charge) with the trapezoidal rule — backward Euler
//! for the first step after a breakpoint — and chooses each step from the local
//! truncation error as SPICE3's `CKTterr` does: divided differences of every charge give
//! the step that keeps the error within `TRTOL` times the `RELTOL`/`ABSTOL`/`CHGTOL`
//! tolerance. Steps grow at most twofold, a step whose error is too large is redone
//! shorter, and waveform corners, digital events and switch or comparator thresholds
//! are landed on exactly.
//!
//! Device models: resistor, capacitor, inductor, independent V/I sources (DC, AC,
//! PULSE, SIN, PWL), the four controlled sources E/F/G/H, the voltage-controlled switch,
//! the diode, the Gummel–Poon bipolar transistor and the level-1 MOSFET (see
//! [`models`]), and XSPICE digital code models and bridges (see [`digital`]). All
//! arithmetic goes through `crate::num`, so a waveform is bit-identical on every host.
pub mod digital;
pub mod matrix;
pub mod models;
mod parse;

pub use models::{BjtModel, DiodeModel, MosModel, VT};
pub use parse::parse;

use crate::num::{self, exp, ln};
use digital::{DacLevels, Digital, Logic, Ramp};
use matrix::{Complex, Real, C64};
use models::GMIN;
use serde::{Deserialize, Serialize};

/// Conductance from every node to ground, so a node only reached through capacitors
/// still has a DC solution (ngspice's `rshunt`).
const GSHUNT: f64 = 1e-12;

#[derive(Clone, Debug, PartialEq)]
pub enum Wave {
    Pulse {
        v1: f64,
        v2: f64,
        td: f64,
        tr: f64,
        tf: f64,
        pw: Option<f64>,
        per: Option<f64>,
    },
    Sin {
        vo: f64,
        va: f64,
        freq: f64,
        td: f64,
        theta: f64,
        phase: f64,
    },
    Pwl(Vec<(f64, f64)>),
}
fn floor(x: f64) -> f64 {
    let t = x as i64 as f64;
    if t > x {
        t - 1.0
    } else {
        t
    }
}
impl Wave {
    pub fn at(&self, t: f64) -> f64 {
        match self {
            Self::Pulse {
                v1,
                v2,
                td,
                tr,
                tf,
                pw,
                per,
            } => {
                if t < *td {
                    return *v1;
                }
                let mut tt = t - td;
                if let Some(per) = per.filter(|p| *p > 0.0) {
                    tt -= per * floor(tt / per);
                }
                let pw = pw.unwrap_or(f64::INFINITY);
                if tt < *tr {
                    v1 + (v2 - v1) * tt / tr
                } else if tt < tr + pw {
                    *v2
                } else if tt < tr + pw + tf {
                    v2 + (v1 - v2) * (tt - tr - pw) / tf
                } else {
                    *v1
                }
            }
            Self::Sin {
                vo,
                va,
                freq,
                td,
                theta,
                phase,
            } => {
                let ph = phase * num::PI / 180.0;
                if t < *td {
                    vo + va * num::sin(ph)
                } else {
                    let d = t - td;
                    let damp = if *theta == 0.0 { 1.0 } else { exp(-d * theta) };
                    vo + va * damp * num::sin(2.0 * num::PI * freq * d + ph)
                }
            }
            Self::Pwl(points) => {
                let Some(first) = points.first() else {
                    return 0.0;
                };
                if t <= first.0 {
                    return first.1;
                }
                for w in points.windows(2) {
                    let ((t0, v0), (t1, v1)) = (w[0], w[1]);
                    if t <= t1 {
                        if t1 == t0 {
                            return v1;
                        }
                        return v0 + (v1 - v0) * (t - t0) / (t1 - t0);
                    }
                }
                points.last().map(|p| p.1).unwrap_or(0.0)
            }
        }
    }
    /// Times inside [0, stop] where the waveform has a corner.
    fn breakpoints(&self, stop: f64, out: &mut Vec<f64>) {
        match self {
            Self::Pulse {
                td,
                tr,
                tf,
                pw,
                per,
                ..
            } => {
                let pw = pw.unwrap_or(f64::INFINITY);
                let mut base = *td;
                let mut k = 0;
                while base <= stop && k < 100_000 {
                    for t in [base, base + tr, base + tr + pw, base + tr + pw + tf] {
                        if t.is_finite() && t <= stop {
                            out.push(t);
                        }
                    }
                    match per {
                        Some(p) if *p > 0.0 => base = td + p * (k + 1) as f64,
                        _ => break,
                    }
                    k += 1;
                }
            }
            Self::Sin { td, .. } => out.push(*td),
            Self::Pwl(points) => out.extend(points.iter().map(|p| p.0).filter(|t| *t <= stop)),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub dc: f64,
    pub ac_mag: f64,
    pub ac_phase: f64,
    pub wave: Option<Wave>,
}

/// Voltage-controlled switch (`.model … SW`): on above `vt + vh`, off below `vt - vh`,
/// holding its state in between.
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchModel {
    pub vt: f64,
    pub vh: f64,
    pub ron: f64,
    pub roff: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Device {
    R {
        name: String,
        a: usize,
        b: usize,
        r: f64,
    },
    C {
        name: String,
        a: usize,
        b: usize,
        c: f64,
        ic: Option<f64>,
    },
    L {
        name: String,
        a: usize,
        b: usize,
        l: f64,
        ic: Option<f64>,
    },
    V {
        name: String,
        p: usize,
        n: usize,
        src: Source,
    },
    I {
        name: String,
        p: usize,
        n: usize,
        src: Source,
    },
    E {
        name: String,
        p: usize,
        n: usize,
        cp: usize,
        cn: usize,
        gain: f64,
    },
    G {
        name: String,
        p: usize,
        n: usize,
        cp: usize,
        cn: usize,
        gm: f64,
    },
    F {
        name: String,
        p: usize,
        n: usize,
        control: String,
        gain: f64,
    },
    H {
        name: String,
        p: usize,
        n: usize,
        control: String,
        r: f64,
    },
    D {
        name: String,
        a: usize,
        k: usize,
        model: DiodeModel,
        area: f64,
    },
    Q {
        name: String,
        c: usize,
        b: usize,
        e: usize,
        model: BjtModel,
        area: f64,
    },
    M {
        name: String,
        d: usize,
        g: usize,
        s: usize,
        b: usize,
        model: MosModel,
        w: f64,
        l: f64,
    },
    S {
        name: String,
        p: usize,
        n: usize,
        cp: usize,
        cn: usize,
        model: SwitchModel,
        /// Initial state (`ON`/`OFF` on the card); `None` lets the operating point decide.
        initial: Option<bool>,
    },
    /// The analog side of an XSPICE `dac_bridge`: a voltage source from `p` to ground
    /// following digital net `net`.
    Dac {
        name: String,
        p: usize,
        net: usize,
        levels: DacLevels,
    },
}
impl Device {
    pub fn name(&self) -> &str {
        match self {
            Self::R { name, .. }
            | Self::C { name, .. }
            | Self::L { name, .. }
            | Self::V { name, .. }
            | Self::I { name, .. }
            | Self::E { name, .. }
            | Self::G { name, .. }
            | Self::F { name, .. }
            | Self::H { name, .. }
            | Self::D { name, .. }
            | Self::Q { name, .. }
            | Self::M { name, .. }
            | Self::S { name, .. }
            | Self::Dac { name, .. } => name,
        }
    }
    /// Devices with a branch-current unknown in the MNA system.
    fn has_branch(&self) -> bool {
        matches!(
            self,
            Self::V { .. } | Self::L { .. } | Self::E { .. } | Self::H { .. } | Self::Dac { .. }
        )
    }
    fn nonlinear(&self) -> bool {
        matches!(
            self,
            Self::D { .. } | Self::Q { .. } | Self::M { .. } | Self::S { .. } | Self::Dac { .. }
        )
    }
    /// Charge-storage slots the transient keeps for this device.
    fn slot_count(&self) -> usize {
        match self {
            Self::C { .. } | Self::L { .. } | Self::D { .. } => 1,
            Self::Q { .. } => 2,
            // Gate-source, gate-drain, gate-bulk (Meyer), bulk-drain, bulk-source.
            Self::M { .. } => 5,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Analysis {
    Op,
    Dc {
        source: String,
        start: f64,
        stop: f64,
        step: f64,
    },
    Ac {
        scale: String,
        points: u32,
        fstart: f64,
        fstop: f64,
    },
    Tran {
        tstep: f64,
        tstop: f64,
        tstart: f64,
        tmax: Option<f64>,
        uic: bool,
    },
}
impl Analysis {
    /// The SPICE control line for this analysis.
    pub fn card(&self) -> String {
        let f = |v: f64| {
            crate::num::format_si(v, 6, "")
                .replace('µ', "u")
                .replace('M', "Meg")
        };
        match self {
            Self::Op => ".op".into(),
            Self::Dc {
                source,
                start,
                stop,
                step,
            } => format!(".dc {source} {} {} {}", f(*start), f(*stop), f(*step)),
            Self::Ac {
                scale,
                points,
                fstart,
                fstop,
            } => format!(".ac {scale} {points} {} {}", f(*fstart), f(*fstop)),
            Self::Tran {
                tstep,
                tstop,
                tstart,
                tmax,
                uic,
            } => {
                let mut s = format!(".tran {} {}", f(*tstep), f(*tstop));
                if *tstart != 0.0 || tmax.is_some() {
                    s.push_str(&format!(" {}", f(*tstart)));
                }
                if let Some(m) = tmax {
                    s.push_str(&format!(" {}", f(*m)));
                }
                if *uic {
                    s.push_str(" uic");
                }
                s
            }
        }
    }
}

/// Simulator tolerances (`.options`), SPICE's names and defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub reltol: f64,
    pub abstol: f64,
    pub vntol: f64,
    pub chgtol: f64,
    pub trtol: f64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            reltol: 1e-3,
            abstol: 1e-12,
            vntol: 1e-6,
            chgtol: 1e-14,
            trtol: 7.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Circuit {
    pub title: String,
    /// Node names; index 0 is ground.
    pub nodes: Vec<String>,
    pub devices: Vec<Device>,
    pub analyses: Vec<Analysis>,
    pub trapezoidal: bool,
    pub digital: Digital,
    pub options: Options,
}

/// One simulated quantity. AC results are complex: `im` is filled and has the same
/// length as `re`; every other analysis leaves it empty.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub name: String,
    pub re: Vec<f64>,
    pub im: Vec<f64>,
}
impl Signal {
    pub fn magnitude(&self, i: usize) -> f64 {
        C64::new(self.re[i], self.im.get(i).copied().unwrap_or(0.0)).abs()
    }
    pub fn db(&self, i: usize) -> f64 {
        20.0 * num::log10(self.magnitude(i).max(1e-300))
    }
    pub fn phase_deg(&self, i: usize) -> f64 {
        num::atan2(self.im.get(i).copied().unwrap_or(0.0), self.re[i]) * 180.0 / num::PI
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimResult {
    pub analysis: Analysis,
    /// Name of the independent variable (`time`, `frequency`, the swept source), or
    /// empty for an operating point.
    pub x_name: String,
    /// The independent variable. A transient reports every accepted time point from
    /// TSTART on, as SPICE does, so the step control is visible in the result.
    pub x: Vec<f64>,
    pub signals: Vec<Signal>,
    /// What the run did, in the words a SPICE console would print.
    pub log: Vec<String>,
    /// Transient steps rejected for truncation error or a threshold overshoot.
    pub rejected: usize,
}
impl SimResult {
    pub fn signal(&self, name: &str) -> Option<&Signal> {
        self.signals
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }
    /// Linear interpolation of a real signal at x.
    pub fn value_at(&self, name: &str, at: f64) -> Option<f64> {
        let s = self.signal(name)?;
        if self.x.is_empty() {
            return s.re.first().copied();
        }
        let i = self.x.partition_point(|v| *v < at);
        if i == 0 {
            return s.re.first().copied();
        }
        if i >= self.x.len() {
            return s.re.last().copied();
        }
        let (x0, x1) = (self.x[i - 1], self.x[i]);
        let (y0, y1) = (s.re[i - 1], s.re[i]);
        Some(if x1 == x0 {
            y1
        } else {
            y0 + (y1 - y0) * (at - x0) / (x1 - x0)
        })
    }
    /// Times at which a signal crosses `level` going the given way, interpolated.
    pub fn crossings(&self, name: &str, level: f64, rising: bool) -> Vec<f64> {
        let Some(s) = self.signal(name) else {
            return vec![];
        };
        let mut out = vec![];
        for i in 1..self.x.len().min(s.re.len()) {
            let (a, b) = (s.re[i - 1], s.re[i]);
            let hit = if rising {
                a < level && b >= level
            } else {
                a > level && b <= level
            };
            if hit {
                let (x0, x1) = (self.x[i - 1], self.x[i]);
                out.push(x0 + (level - a) * (x1 - x0) / (b - a));
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Device evaluation
// ---------------------------------------------------------------------------

/// A current flowing from node `p` to node `n` through a device, linearised: `i` at the
/// point, and its derivative `g` with respect to each controlling voltage
/// `v(cp) - v(cn)`, whose value at the point is `v0`. Circuit polarity throughout.
#[derive(Clone, Debug)]
struct Flow {
    p: usize,
    n: usize,
    i: f64,
    g: Vec<(usize, usize, f64, f64)>,
}
/// A charge stored between `p` and `n` (the current `dq/dt` flows from `p` to `n`),
/// with its derivatives (capacitances) with respect to controlling voltages.
#[derive(Clone, Debug)]
struct Charge {
    p: usize,
    n: usize,
    q: f64,
    c: Vec<(usize, usize, f64, f64)>,
    /// Meyer charge: the half-capacitance at this point, kept for the next point.
    half: f64,
    /// The controlling voltage, kept for the next Meyer increment.
    v: f64,
}
#[derive(Clone, Debug, Default)]
struct Eval {
    flows: Vec<Flow>,
    /// Linear conductances (series resistances, GMIN).
    res: Vec<(usize, usize, f64)>,
    charges: Vec<Charge>,
    /// Junction limiting changed a voltage this iteration: not converged yet.
    limited: bool,
}

/// One charge-storage slot's history: charges and companion currents at the last
/// accepted points (index 0 the newest), and a Meyer charge's voltage and half-cap.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Slot {
    q: [f64; 4],
    i: [f64; 2],
    v: f64,
    half: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    /// Operating point; `scale` multiplies every independent source (source stepping).
    /// `time` evaluates time-varying sources at that instant, for the point a
    /// transient starts from.
    Dc { scale: f64, time: Option<f64> },
    /// Transient step to time `t` of length `h`, trapezoidal when `trap`.
    Tran { t: f64, h: f64, trap: bool },
}

/// Everything that varies from one solve to the next besides the unknowns.
struct Ctx<'a> {
    mode: Mode,
    sweep: Option<(usize, f64)>,
    gmin_extra: f64,
    slots: &'a [Slot],
    /// Per device: a switch's state.
    switch_on: &'a [bool],
    /// Per device: a DAC's output voltage now.
    dac: &'a [f64],
    /// First transient point: Meyer charges start from `C·v`.
    first: bool,
}

struct Engine<'a> {
    c: &'a Circuit,
    /// Unknowns: node voltages 1..=nodes, then branch currents.
    size: usize,
    nodes: usize,
    names: Vec<String>,
    branch: Vec<usize>,
    /// Internal nodes: diode anode; BJT collector, base, emitter; MOSFET drain, source.
    /// Zero where the device has none (the external node is used).
    int: Vec<[usize; 3]>,
    slot: Vec<usize>,
    nslots: usize,
}

impl<'a> Engine<'a> {
    fn new(c: &'a Circuit) -> Result<Self, String> {
        let mut nodes = c.nodes.len() - 1;
        let mut names = c.nodes.clone();
        let mut int = vec![[0; 3]; c.devices.len()];
        let add = |nodes: &mut usize, names: &mut Vec<String>, label: String| {
            *nodes += 1;
            names.push(label);
            *nodes
        };
        for (i, d) in c.devices.iter().enumerate() {
            match d {
                Device::D { name, model, .. } if model.rs > 0.0 => {
                    int[i][0] = add(&mut nodes, &mut names, format!("{name}#internal"));
                }
                Device::Q { name, model, .. } => {
                    if model.rc > 0.0 {
                        int[i][0] = add(&mut nodes, &mut names, format!("{name}#collector"));
                    }
                    if model.rb > 0.0 {
                        int[i][1] = add(&mut nodes, &mut names, format!("{name}#base"));
                    }
                    if model.re > 0.0 {
                        int[i][2] = add(&mut nodes, &mut names, format!("{name}#emitter"));
                    }
                }
                Device::M { name, model, .. } => {
                    if model.rd > 0.0 {
                        int[i][0] = add(&mut nodes, &mut names, format!("{name}#drain"));
                    }
                    if model.rs > 0.0 {
                        int[i][1] = add(&mut nodes, &mut names, format!("{name}#source"));
                    }
                }
                _ => {}
            }
        }
        let mut branch = vec![0; c.devices.len()];
        let mut size = nodes;
        for (i, d) in c.devices.iter().enumerate() {
            if d.has_branch() {
                size += 1;
                branch[i] = size;
            }
        }
        let mut slot = vec![0; c.devices.len()];
        let mut nslots = 0;
        for (i, d) in c.devices.iter().enumerate() {
            slot[i] = nslots;
            nslots += d.slot_count();
        }
        // Controlled sources name their controlling voltage source; it must exist.
        for d in &c.devices {
            if let Device::F { control, name, .. } | Device::H { control, name, .. } = d {
                let found = c.devices.iter().any(|o| {
                    matches!(o, Device::V { .. }) && o.name().eq_ignore_ascii_case(control)
                });
                if !found {
                    return Err(format!(
                        "{name} is controlled by {control}, which is not a voltage source"
                    ));
                }
            }
        }
        Ok(Self {
            c,
            size,
            nodes,
            names,
            branch,
            int,
            slot,
            nslots,
        })
    }
    fn control_branch(&self, control: &str) -> usize {
        self.c
            .devices
            .iter()
            .position(|o| matches!(o, Device::V { .. }) && o.name().eq_ignore_ascii_case(control))
            .map(|i| self.branch[i])
            .unwrap_or(0)
    }
    fn source_value(src: &Source, mode: Mode, sweep: Option<f64>) -> f64 {
        if let Some(v) = sweep {
            return v;
        }
        match mode {
            Mode::Dc { scale, time } => {
                let base = match (time, &src.wave) {
                    (Some(t), Some(w)) => w.at(t),
                    _ => src.dc,
                };
                base * scale
            }
            Mode::Tran { t, .. } => src.wave.as_ref().map_or(src.dc, |w| w.at(t)),
        }
    }
    /// The node a device terminal is stamped at: its internal node when it has one.
    fn inner(&self, i: usize, k: usize, outer: usize) -> usize {
        if self.int[i][k] != 0 {
            self.int[i][k]
        } else {
            outer
        }
    }

    /// Currents and charges of nonlinear device `i` at `x`. `lim` holds its limited
    /// junction voltages from the last iteration and is updated when `limit` is set.
    fn eval(&self, i: usize, x: &[f64], lim: &mut [f64; 3], limit: bool, ctx: &Ctx) -> Eval {
        let v = |n: usize| x[n];
        let mut e = Eval::default();
        match &self.c.devices[i] {
            Device::D {
                a, k, model, area, ..
            } => {
                let ai = self.inner(i, 0, *a);
                if ai != *a {
                    e.res.push((*a, ai, *area / model.rs));
                }
                let raw = v(ai) - v(*k);
                let vt = VT * model.n;
                let vd = if limit {
                    let mut vd =
                        models::pnjlim(raw, lim[0], vt, models::vcrit(vt, model.is * area));
                    if let Some(bv) = model.bv {
                        // Limit the reflected voltage in breakdown the same way.
                        if vd < -bv + 10.0 * vt {
                            let r = models::pnjlim(
                                -(vd + bv),
                                -(lim[0] + bv),
                                vt,
                                models::vcrit(vt, model.ibv),
                            );
                            vd = -(r + bv);
                        }
                    }
                    vd
                } else {
                    raw
                };
                e.limited |= (vd - raw).abs() > 1e-9;
                lim[0] = vd;
                let (id, gd) = models::diode_iv(model, *area, vd);
                e.flows.push(Flow {
                    p: ai,
                    n: *k,
                    i: id,
                    g: vec![(ai, *k, gd, vd)],
                });
                let (q, cq) = models::diode_charge(model, *area, vd);
                e.charges.push(Charge {
                    p: ai,
                    n: *k,
                    q,
                    c: vec![(ai, *k, cq, vd)],
                    half: 0.0,
                    v: vd,
                });
            }
            Device::Q {
                c,
                b,
                e: em,
                model,
                area,
                ..
            } => {
                let (ci, bi, ei) = (
                    self.inner(i, 0, *c),
                    self.inner(i, 1, *b),
                    self.inner(i, 2, *em),
                );
                if ci != *c {
                    e.res.push((*c, ci, area / model.rc));
                }
                if bi != *b {
                    e.res.push((*b, bi, area / model.rb));
                }
                if ei != *em {
                    e.res.push((*em, ei, area / model.re));
                }
                let p = if model.pnp { -1.0 } else { 1.0 };
                let raw_be = p * (v(bi) - v(ei));
                let raw_bc = p * (v(bi) - v(ci));
                let (vbe, vbc) = if limit {
                    (
                        models::pnjlim(
                            raw_be,
                            lim[0],
                            VT * model.nf,
                            models::vcrit(VT * model.nf, model.is * area),
                        ),
                        models::pnjlim(
                            raw_bc,
                            lim[1],
                            VT * model.nr,
                            models::vcrit(VT * model.nr, model.is * area),
                        ),
                    )
                } else {
                    (raw_be, raw_bc)
                };
                e.limited |= (vbe - raw_be).abs() > 1e-9 || (vbc - raw_bc).abs() > 1e-9;
                lim[0] = vbe;
                lim[1] = vbc;
                let q = models::bjt_eval(model, *area, vbe, vbc);
                let (cbe, cbc) = (p * vbe, p * vbc);
                e.flows.push(Flow {
                    p: ci,
                    n: ei,
                    i: p * q.ic,
                    g: vec![(bi, ei, q.gcbe, cbe), (bi, ci, q.gcbc, cbc)],
                });
                e.flows.push(Flow {
                    p: bi,
                    n: ei,
                    i: p * q.ib,
                    g: vec![(bi, ei, q.gbbe, cbe), (bi, ci, q.gbbc, cbc)],
                });
                e.charges.push(Charge {
                    p: bi,
                    n: ei,
                    q: p * q.qbe,
                    c: vec![(bi, ei, q.cbe_be, cbe), (bi, ci, q.cbe_bc, cbc)],
                    half: 0.0,
                    v: cbe,
                });
                e.charges.push(Charge {
                    p: bi,
                    n: ci,
                    q: p * q.qbc,
                    c: vec![(bi, ci, q.cbc_bc, cbc)],
                    half: 0.0,
                    v: cbc,
                });
            }
            Device::M {
                d,
                g,
                s,
                b,
                model,
                w,
                l,
                ..
            } => {
                let (di, si) = (self.inner(i, 0, *d), self.inner(i, 1, *s));
                let wl = w / model.leff(*l);
                if di != *d {
                    e.res.push((*d, di, 1.0 / model.rd));
                }
                if si != *s {
                    e.res.push((*s, si, 1.0 / model.rs));
                }
                let _ = wl;
                let p = if model.pmos { -1.0 } else { 1.0 };
                let raw_gs = p * (v(*g) - v(si));
                let raw_ds = p * (v(di) - v(si));
                let raw_bs = p * (v(*b) - v(si));
                let (vgs, vds, vbs) = if limit {
                    (
                        lim[0] + (raw_gs - lim[0]).clamp(-2.0, 2.0),
                        lim[1] + (raw_ds - lim[1]).clamp(-4.0, 4.0),
                        lim[2] + (raw_bs - lim[2]).clamp(-2.0, 2.0),
                    )
                } else {
                    (raw_gs, raw_ds, raw_bs)
                };
                e.limited |= (vgs - raw_gs).abs() > 1e-9
                    || (vds - raw_ds).abs() > 1e-9
                    || (vbs - raw_bs).abs() > 1e-9;
                *lim = [vgs, vds, vbs];
                let vgd = vgs - vds;
                let vbd = vbs - vds;
                let vgb = vgs - vbs;
                let (von, vdsat) = if vds >= 0.0 {
                    let (id, gm, gds, gmbs, von, vdsat) =
                        models::mos_forward(model, *w, *l, vgs, vds, vbs);
                    e.flows.push(Flow {
                        p: di,
                        n: si,
                        i: p * id,
                        g: vec![
                            (*g, si, gm, p * vgs),
                            (di, si, gds, p * vds),
                            (*b, si, gmbs, p * vbs),
                        ],
                    });
                    (von, vdsat)
                } else {
                    // Drain and source exchange roles.
                    let (id, gm, gds, gmbs, von, vdsat) =
                        models::mos_forward(model, *w, *l, vgd, -vds, vbd);
                    e.flows.push(Flow {
                        p: si,
                        n: di,
                        i: p * id,
                        g: vec![
                            (*g, di, gm, p * vgd),
                            (si, di, gds, -p * vds),
                            (*b, di, gmbs, p * vbd),
                        ],
                    });
                    (von, vdsat)
                };
                e.res.push((di, si, GMIN));
                let (ibd, gbd) = models::bulk_diode(model.is, vbd);
                let (ibs, gbs) = models::bulk_diode(model.is, vbs);
                e.flows.push(Flow {
                    p: *b,
                    n: di,
                    i: p * ibd,
                    g: vec![(*b, di, gbd, p * vbd)],
                });
                e.flows.push(Flow {
                    p: *b,
                    n: si,
                    i: p * ibs,
                    g: vec![(*b, si, gbs, p * vbs)],
                });
                // Meyer gate charges, integrated from capacitances as SPICE3 does.
                let cox = model.cox() * w * model.leff(*l);
                let phi = model.phi.max(0.1);
                let (hgs, hgd, hgb) = if cox > 0.0 {
                    if vds >= 0.0 {
                        models::meyer(vgs, vgd, von, vdsat, phi, cox)
                    } else {
                        let (a, b, c) = models::meyer(vgd, vgs, von, vdsat, phi, cox);
                        (b, a, c)
                    }
                } else {
                    (0.0, 0.0, 0.0)
                };
                let overlaps = [model.cgso * w, model.cgdo * w, model.cgbo * model.leff(*l)];
                let base = self.slot[i];
                for (k, (half, other, vv, ov)) in [
                    (hgs, si, p * vgs, overlaps[0]),
                    (hgd, di, p * vgd, overlaps[1]),
                    (hgb, *b, p * vgb, overlaps[2]),
                ]
                .into_iter()
                .enumerate()
                {
                    let prev = ctx.slots.get(base + k).copied().unwrap_or_default();
                    let (q, cap) = if ctx.first || !matches!(ctx.mode, Mode::Tran { .. }) {
                        let cap = 2.0 * half + ov;
                        (cap * vv, cap)
                    } else {
                        let cap = half + prev.half + ov;
                        (prev.q[0] + cap * (vv - prev.v), cap)
                    };
                    e.charges.push(Charge {
                        p: *g,
                        n: other,
                        q,
                        c: vec![(*g, other, cap, vv)],
                        half,
                        v: vv,
                    });
                }
                let (qbd, cbd) = models::depletion(model.cbd, model.pb, model.mj, model.fc, vbd);
                let (qbs, cbs) = models::depletion(model.cbs, model.pb, model.mj, model.fc, vbs);
                e.charges.push(Charge {
                    p: *b,
                    n: di,
                    q: p * qbd,
                    c: vec![(*b, di, cbd, p * vbd)],
                    half: 0.0,
                    v: p * vbd,
                });
                e.charges.push(Charge {
                    p: *b,
                    n: si,
                    q: p * qbs,
                    c: vec![(*b, si, cbs, p * vbs)],
                    half: 0.0,
                    v: p * vbs,
                });
            }
            Device::C { a, b, c, .. } => {
                let vc = v(*a) - v(*b);
                e.charges.push(Charge {
                    p: *a,
                    n: *b,
                    q: c * vc,
                    c: vec![(*a, *b, *c, vc)],
                    half: 0.0,
                    v: vc,
                });
            }
            _ => {}
        }
        e
    }

    /// Stamp the linearised system at `x`. Returns whether junction limiting held any
    /// device short of the voltages at `x` (SPICE's `noncon`).
    fn load(&self, x: &[f64], ctx: &Ctx, lim: &mut [[f64; 3]], limit: bool, m: &mut Real) -> bool {
        let mut limited = false;
        m.clear();
        for node in 1..=self.nodes {
            m.add(node, node, GSHUNT + ctx.gmin_extra);
        }
        let conductance = |m: &mut Real, a: usize, b: usize, g: f64| {
            m.add(a, a, g);
            m.add(b, b, g);
            m.add(a, b, -g);
            m.add(b, a, -g);
        };
        let flow = |m: &mut Real, f: &Flow| {
            let mut ieq = f.i;
            for (cp, cn, g, v0) in &f.g {
                m.add(f.p, *cp, *g);
                m.add(f.p, *cn, -g);
                m.add(f.n, *cp, -g);
                m.add(f.n, *cn, *g);
                ieq -= g * v0;
            }
            m.rhs(f.p, -ieq);
            m.rhs(f.n, ieq);
        };
        for (i, d) in self.c.devices.iter().enumerate() {
            let swept = ctx.sweep.filter(|s| s.0 == i).map(|s| s.1);
            match d {
                Device::R { a, b, r, .. } => conductance(m, *a, *b, 1.0 / r),
                Device::L { a, b, l, .. } => {
                    let k = self.branch[i];
                    m.add(*a, k, 1.0);
                    m.add(*b, k, -1.0);
                    m.add(k, *a, 1.0);
                    m.add(k, *b, -1.0);
                    if let Mode::Tran { h, trap, .. } = ctx.mode {
                        // v = dφ/dt with φ = L·i, integrated like any charge.
                        let s = ctx.slots[self.slot[i]];
                        let f = if trap { 2.0 / h } else { 1.0 / h };
                        m.add(k, k, -f * l);
                        let prev_v = if trap { s.i[0] } else { 0.0 };
                        m.rhs(k, -f * s.q[0] - prev_v);
                    }
                }
                Device::V { p, n, src, .. } => {
                    let k = self.branch[i];
                    m.add(*p, k, 1.0);
                    m.add(*n, k, -1.0);
                    m.add(k, *p, 1.0);
                    m.add(k, *n, -1.0);
                    m.rhs(k, Self::source_value(src, ctx.mode, swept));
                }
                Device::Dac { p, .. } => {
                    let k = self.branch[i];
                    m.add(*p, k, 1.0);
                    m.add(k, *p, 1.0);
                    m.rhs(k, ctx.dac[i]);
                }
                Device::I { p, n, src, .. } => {
                    let val = Self::source_value(src, ctx.mode, swept);
                    m.rhs(*p, -val);
                    m.rhs(*n, val);
                }
                Device::E {
                    p, n, cp, cn, gain, ..
                } => {
                    let k = self.branch[i];
                    m.add(*p, k, 1.0);
                    m.add(*n, k, -1.0);
                    m.add(k, *p, 1.0);
                    m.add(k, *n, -1.0);
                    m.add(k, *cp, -gain);
                    m.add(k, *cn, *gain);
                }
                Device::G {
                    p, n, cp, cn, gm, ..
                } => {
                    m.add(*p, *cp, *gm);
                    m.add(*p, *cn, -gm);
                    m.add(*n, *cp, -gm);
                    m.add(*n, *cn, *gm);
                }
                Device::F {
                    p,
                    n,
                    control,
                    gain,
                    ..
                } => {
                    let kc = self.control_branch(control);
                    m.add(*p, kc, *gain);
                    m.add(*n, kc, -gain);
                }
                Device::H {
                    p, n, control, r, ..
                } => {
                    let k = self.branch[i];
                    let kc = self.control_branch(control);
                    m.add(*p, k, 1.0);
                    m.add(*n, k, -1.0);
                    m.add(k, *p, 1.0);
                    m.add(k, *n, -1.0);
                    m.add(k, kc, -r);
                }
                Device::S { p, n, model, .. } => {
                    let r = if ctx.switch_on[i] {
                        model.ron
                    } else {
                        model.roff
                    };
                    conductance(m, *p, *n, 1.0 / r);
                }
                Device::C { .. } | Device::D { .. } | Device::Q { .. } | Device::M { .. } => {
                    let e = self.eval(i, x, &mut lim[i], limit, ctx);
                    limited |= e.limited;
                    for (a, b, g) in &e.res {
                        conductance(m, *a, *b, *g);
                    }
                    for f in &e.flows {
                        flow(m, f);
                    }
                    if let Mode::Tran { h, trap, .. } = ctx.mode {
                        let f = if trap { 2.0 / h } else { 1.0 / h };
                        for (k, ch) in e.charges.iter().enumerate() {
                            let s = ctx.slots[self.slot[i] + k];
                            let prev_i = if trap { s.i[0] } else { 0.0 };
                            flow(
                                m,
                                &Flow {
                                    p: ch.p,
                                    n: ch.n,
                                    i: f * (ch.q - s.q[0]) - prev_i,
                                    g: ch
                                        .c
                                        .iter()
                                        .map(|(cp, cn, c, v0)| (*cp, *cn, f * c, *v0))
                                        .collect(),
                                },
                            );
                        }
                    }
                }
            }
        }
        limited
    }

    fn converged(&self, old: &[f64], new: &[f64]) -> bool {
        let o = &self.c.options;
        (1..=self.size).all(|k| {
            let (a, b) = (old[k], new[k]);
            let tol = o.reltol * 1e-3 * a.abs().max(b.abs())
                + if k <= self.nodes {
                    o.vntol * 1e-3
                } else {
                    o.abstol
                };
            (a - b).abs() <= tol
        })
    }

    fn newton(
        &self,
        x0: &[f64],
        ctx: &Ctx,
        lim: &mut [[f64; 3]],
        max_iter: usize,
    ) -> Result<Vec<f64>, String> {
        let mut x = x0.to_vec();
        let mut m = Real::new(self.size);
        let nonlinear = self.c.devices.iter().any(Device::nonlinear);
        for iter in 0..max_iter {
            let limited = self.load(
                &x,
                ctx,
                lim,
                iter > 0 || x0.iter().any(|v| *v != 0.0),
                &mut m,
            );
            let new = m.solve().map_err(|k| self.singular(k))?;
            if new.iter().any(|v| !v.is_finite()) {
                return Err("the solution diverged".into());
            }
            let done = !nonlinear || (iter > 0 && !limited && self.converged(&x, &new));
            x = new;
            if done {
                return Ok(x);
            }
        }
        Err(format!("no convergence after {max_iter} Newton iterations"))
    }

    fn singular(&self, k: usize) -> String {
        if k <= self.nodes {
            format!(
                "singular matrix: node {} has no DC path or is shorted by voltage sources",
                self.names[k]
            )
        } else {
            let device = self
                .branch
                .iter()
                .position(|b| *b == k)
                .map(|i| self.c.devices[i].name().to_owned())
                .unwrap_or_default();
            format!("singular matrix: {device} forms a loop of voltage sources or inductors")
        }
    }

    /// DC operating point with the standard SPICE fallbacks.
    fn operating_point(
        &self,
        base: &Ctx,
        guess: Option<&[f64]>,
        log: &mut Vec<String>,
    ) -> Result<(Vec<f64>, Vec<[f64; 3]>), String> {
        let zero = vec![0.0; self.size + 1];
        let start = guess.unwrap_or(&zero);
        let n = self.c.devices.len();
        let mut lim = vec![[0.0; 3]; n];
        if let Ok(x) = self.newton(start, base, &mut lim, 200) {
            return Ok((x, lim));
        }
        log.push("Note: Newton failed; trying gmin stepping".into());
        let mut x = zero.clone();
        let mut lim = vec![[0.0; 3]; n];
        let mut ok = true;
        let mut g = 1e-2;
        while g > 1e-13 {
            let ctx = Ctx {
                gmin_extra: g,
                ..*base
            };
            match self.newton(&x, &ctx, &mut lim, 200) {
                Ok(next) => x = next,
                Err(_) => {
                    ok = false;
                    break;
                }
            }
            g /= 10.0;
        }
        if ok {
            if let Ok(x) = self.newton(&x, base, &mut lim, 200) {
                log.push("Note: gmin stepping succeeded".into());
                return Ok((x, lim));
            }
        }
        log.push("Note: trying source stepping".into());
        let mut x = zero;
        let mut lim = vec![[0.0; 3]; n];
        let mut scale: f64 = 0.0;
        let mut step: f64 = 0.1;
        while scale < 1.0 {
            let next_scale = (scale + step).min(1.0);
            let mode = match base.mode {
                Mode::Dc { scale: s, time } => Mode::Dc {
                    scale: s * next_scale,
                    time,
                },
                other => other,
            };
            let ctx = Ctx { mode, ..*base };
            let mut trial_lim = lim.clone();
            match self.newton(&x, &ctx, &mut trial_lim, 200) {
                Ok(next) => {
                    x = next;
                    lim = trial_lim;
                    scale = next_scale;
                    step = (step * 2.0).min(0.25);
                }
                Err(e) => {
                    step /= 4.0;
                    if step < 1e-6 {
                        return Err(format!("operating point: {e}"));
                    }
                }
            }
        }
        log.push("Note: source stepping succeeded".into());
        Ok((x, lim))
    }

    /// Signals reported for a solution: every named node and every branch current.
    fn signal_names(&self) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        for (k, name) in self.c.nodes.iter().enumerate().skip(1) {
            out.push((format!("V({name})"), k));
        }
        for (i, d) in self.c.devices.iter().enumerate() {
            if matches!(d, Device::V { .. } | Device::L { .. }) {
                out.push((format!("I({})", d.name()), self.branch[i]));
            }
        }
        out
    }

    /// Switch states a solution implies, starting from `prev` (hysteresis).
    fn switch_states(&self, x: &[f64], prev: &[bool]) -> Vec<bool> {
        self.c
            .devices
            .iter()
            .enumerate()
            .map(|(i, d)| match d {
                Device::S { cp, cn, model, .. } => {
                    let vc = x[*cp] - x[*cn];
                    if vc > model.vt + model.vh {
                        true
                    } else if vc < model.vt - model.vh {
                        false
                    } else {
                        prev[i]
                    }
                }
                _ => false,
            })
            .collect()
    }
}

/// The DAC output levels for the digital nets as they stand (no ramps).
fn dac_levels(c: &Circuit, values: &[Logic]) -> Vec<f64> {
    c.devices
        .iter()
        .map(|d| match d {
            Device::Dac { net, levels, .. } => levels.level(values[*net]),
            _ => 0.0,
        })
        .collect()
}

/// Interval halving through the trapezoidal/Gear truncation-error coefficients, as
/// SPICE3's `CKTterr`: the largest step that keeps one charge's error within tolerance.
fn truncation_step(
    o: &Options,
    s: &Slot,
    q_new: f64,
    i_new: f64,
    deltas: &[f64],
    order: usize,
) -> f64 {
    let q = [q_new, s.q[0], s.q[1], s.q[2]];
    let h = deltas[0];
    let volttol = o.abstol + o.reltol * i_new.abs().max(s.i[0].abs());
    let chargetol = o.reltol * q[0].abs().max(q[1].abs()).max(o.chgtol) / h;
    let tol = volttol.max(chargetol);
    // Divided differences of order `order + 1` over the last order + 2 points.
    let mut diff = [0.0; 4];
    diff[..=order + 1].copy_from_slice(&q[..=order + 1]);
    let mut dt = [0.0; 4];
    dt[..=order].copy_from_slice(&deltas[..=order]);
    let mut j = order as isize;
    loop {
        for i in 0..=j as usize {
            diff[i] = (diff[i] - diff[i + 1]) / dt[i];
        }
        j -= 1;
        if j < 0 {
            break;
        }
        for i in 0..=j as usize {
            dt[i] = dt[i + 1] + deltas[i];
        }
    }
    // SPICE3's trapezoidal coefficients: 1/2 for order 1, 1/12 for order 2.
    let factor = if order == 1 { 0.5 } else { 1.0 / 12.0 };
    let del = o.trtol * tol / o.abstol.max(factor * diff[0].abs());
    if order == 1 {
        del
    } else {
        del.sqrt()
    }
}

impl Circuit {
    /// Run the first analysis the deck asks for, or an operating point if none.
    pub fn run(&self) -> Result<SimResult, String> {
        let analysis = self.analyses.first().cloned().unwrap_or(Analysis::Op);
        self.simulate(&analysis)
    }

    /// Operating point with the digital side settled against it: the analog solution
    /// sets what the ADCs and switches read, the logic settles, the DACs and switches
    /// drive the next solve, until nothing changes.
    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    fn mixed_op(
        &self,
        engine: &Engine,
        mode: Mode,
        sweep: Option<(usize, f64)>,
        guess: Option<&[f64]>,
        dsim: &mut digital::Sim,
        switches: &mut Vec<bool>,
        log: &mut Vec<String>,
    ) -> Result<(Vec<f64>, Vec<[f64; 3]>), String> {
        let slots = vec![Slot::default(); engine.nslots];
        let mut guess = guess.map(<[f64]>::to_vec);
        let mut last = None;
        for _ in 0..32 {
            let dac = dac_levels(self, &dsim.values);
            let ctx = Ctx {
                mode,
                sweep,
                gmin_extra: 0.0,
                slots: &slots,
                switch_on: switches,
                dac: &dac,
                first: true,
            };
            let (x, lim) = engine.operating_point(&ctx, guess.as_deref(), log)?;
            let changed_logic = if self.digital.is_empty() {
                false
            } else {
                dsim.settle(&self.digital, &x)
            };
            let next = engine.switch_states(&x, switches);
            let changed_switch = next != *switches;
            *switches = next;
            if !changed_logic && !changed_switch {
                return Ok((x, lim));
            }
            guess = Some(x.clone());
            last = Some((x, lim));
        }
        // An oscillator (a 555 astable, a ring of gates) has no DC state: the analog
        // solution goes round the loop for ever. Start from where it stands, as a
        // transient would after power-up.
        log.push(
            "Note: the logic does not settle at the operating point (the circuit oscillates); starting from its last state"
                .into(),
        );
        last.ok_or_else(|| "the operating point does not settle".to_owned())
    }

    pub fn simulate(&self, analysis: &Analysis) -> Result<SimResult, String> {
        let engine = Engine::new(self)?;
        let mut log = vec![format!(
            "Circuit: {}",
            if self.title.is_empty() {
                "untitled"
            } else {
                &self.title
            }
        )];
        let names = engine.signal_names();
        let mut result = SimResult {
            analysis: analysis.clone(),
            x_name: String::new(),
            x: vec![],
            signals: names
                .iter()
                .map(|(n, _)| Signal {
                    name: n.clone(),
                    re: vec![],
                    im: vec![],
                })
                .collect(),
            log: vec![],
            rejected: 0,
        };
        let initial_switches: Vec<bool> = self
            .devices
            .iter()
            .map(|d| {
                matches!(
                    d,
                    Device::S {
                        initial: Some(true),
                        ..
                    }
                )
            })
            .collect();
        let dc = Mode::Dc {
            scale: 1.0,
            time: None,
        };
        let fresh_digital = || {
            let mut s = digital::Sim::new(&self.digital);
            s.start(&self.digital, &[]);
            s
        };
        match analysis {
            Analysis::Op => {
                let mut dsim = fresh_digital();
                let mut sw = initial_switches.clone();
                let (x, _) =
                    self.mixed_op(&engine, dc, None, None, &mut dsim, &mut sw, &mut log)?;
                for (s, (_, k)) in result.signals.iter_mut().zip(&names) {
                    s.re.push(x[*k]);
                }
                log.push("Operating point found".into());
            }
            Analysis::Dc {
                source,
                start,
                stop,
                step,
            } => {
                let index = self
                    .devices
                    .iter()
                    .position(|d| {
                        matches!(d, Device::V { .. } | Device::I { .. })
                            && d.name().eq_ignore_ascii_case(source)
                    })
                    .ok_or_else(|| {
                        format!("DC sweep source {source} is not an independent source")
                    })?;
                if *step == 0.0 || (stop - start) / step < 0.0 {
                    return Err("DC sweep step must move from start towards stop".into());
                }
                let count = (((stop - start) / step) + 1e-9) as usize + 1;
                if count > 100_000 {
                    return Err("DC sweep has more than 100000 points".into());
                }
                result.x_name = source.clone();
                let mut guess: Option<Vec<f64>> = None;
                let mut dsim = fresh_digital();
                let mut sw = initial_switches.clone();
                for i in 0..count {
                    let value = start + step * i as f64;
                    let (x, _) = self.mixed_op(
                        &engine,
                        dc,
                        Some((index, value)),
                        guess.as_deref(),
                        &mut dsim,
                        &mut sw,
                        &mut log,
                    )?;
                    result.x.push(value);
                    for (s, (_, k)) in result.signals.iter_mut().zip(&names) {
                        s.re.push(x[*k]);
                    }
                    guess = Some(x);
                }
                log.push(format!("DC sweep: {count} points"));
            }
            Analysis::Ac {
                scale,
                points,
                fstart,
                fstop,
            } => {
                if *fstart <= 0.0 || fstop < fstart {
                    return Err("AC sweep needs 0 < fstart <= fstop".into());
                }
                let mut dsim = fresh_digital();
                let mut sw = initial_switches.clone();
                let (x, mut lim) =
                    self.mixed_op(&engine, dc, None, None, &mut dsim, &mut sw, &mut log)?;
                let slots = vec![Slot::default(); engine.nslots];
                let dac = dac_levels(self, &dsim.values);
                let ctx = Ctx {
                    mode: dc,
                    sweep: None,
                    gmin_extra: 0.0,
                    slots: &slots,
                    switch_on: &sw,
                    dac: &dac,
                    first: true,
                };
                let mut g = Real::new(engine.size);
                let _ = engine.load(&x, &ctx, &mut lim, false, &mut g);
                // Small-signal capacitances at the operating point.
                let mut caps: Vec<Charge> = vec![];
                for (i, d) in self.devices.iter().enumerate() {
                    if matches!(
                        d,
                        Device::C { .. } | Device::D { .. } | Device::Q { .. } | Device::M { .. }
                    ) {
                        let mut l = lim[i];
                        caps.extend(engine.eval(i, &x, &mut l, false, &ctx).charges);
                    }
                }
                let freqs = ac_frequencies(scale, *points, *fstart, *fstop)?;
                result.x_name = "frequency".into();
                for f in freqs {
                    let omega = 2.0 * num::PI * f;
                    let sol = self.ac_point(&engine, &g, &caps, omega)?;
                    result.x.push(f);
                    for (s, (_, k)) in result.signals.iter_mut().zip(&names) {
                        s.re.push(sol[*k].re);
                        s.im.push(sol[*k].im);
                    }
                }
                log.push(format!("AC analysis: {} frequencies", result.x.len()));
            }
            Analysis::Tran {
                tstep,
                tstop,
                tstart,
                tmax,
                uic,
            } => {
                self.transient(
                    &engine,
                    (*tstep, *tstop, *tstart, *tmax, *uic),
                    &names,
                    &mut result,
                    &mut log,
                )?;
            }
        }
        result.log = log;
        Ok(result)
    }

    fn ac_point(
        &self,
        engine: &Engine,
        g: &Real,
        caps: &[Charge],
        omega: f64,
    ) -> Result<Vec<C64>, String> {
        let mut m = Complex::new(engine.size);
        for (i, v) in g.a.iter().enumerate() {
            if *v != 0.0 {
                m.a[i] = C64::new(*v, 0.0);
            }
        }
        let jw = |v: f64| C64::new(0.0, omega * v);
        for ch in caps {
            for (cp, cn, c, _) in &ch.c {
                m.add(ch.p, *cp, jw(*c));
                m.add(ch.p, *cn, jw(-c));
                m.add(ch.n, *cp, jw(-c));
                m.add(ch.n, *cn, jw(*c));
            }
        }
        for (i, d) in self.devices.iter().enumerate() {
            match d {
                Device::L { l, .. } => {
                    let k = engine.branch[i];
                    m.add(k, k, jw(-l));
                }
                Device::V { src, .. } => {
                    let k = engine.branch[i];
                    let ph = src.ac_phase * num::PI / 180.0;
                    m.rhs(
                        k,
                        C64::new(src.ac_mag * num::cos(ph), src.ac_mag * num::sin(ph)),
                    );
                }
                Device::I { p, n, src, .. } => {
                    let ph = src.ac_phase * num::PI / 180.0;
                    let val = C64::new(src.ac_mag * num::cos(ph), src.ac_mag * num::sin(ph));
                    m.rhs(*p, C64::new(-val.re, -val.im));
                    m.rhs(*n, val);
                }
                _ => {}
            }
        }
        m.solve().map_err(|k| engine.singular(k))
    }

    /// Charges and companion currents of every slot at an accepted solution.
    fn slot_values(
        &self,
        engine: &Engine,
        x: &[f64],
        lim: &[[f64; 3]],
        ctx: &Ctx,
    ) -> Vec<(f64, f64, f64)> {
        let mut out = vec![(0.0, 0.0, 0.0); engine.nslots];
        for (i, d) in self.devices.iter().enumerate() {
            let base = engine.slot[i];
            match d {
                Device::L { l, .. } => {
                    let k = engine.branch[i];
                    out[base] = (l * x[k], 0.0, 0.0);
                }
                Device::C { .. } | Device::D { .. } | Device::Q { .. } | Device::M { .. } => {
                    let mut l = lim[i];
                    let e = engine.eval(i, x, &mut l, false, ctx);
                    for (k, ch) in e.charges.iter().enumerate() {
                        out[base + k] = (ch.q, ch.half, ch.v);
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn transient(
        &self,
        engine: &Engine,
        (tstep, tstop, tstart, tmax, uic): (f64, f64, f64, Option<f64>, bool),
        names: &[(String, usize)],
        result: &mut SimResult,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        if tstep <= 0.0 || tstop <= 0.0 || tstart < 0.0 || tstart >= tstop {
            return Err("transient needs 0 < TSTEP and 0 <= TSTART < TSTOP".into());
        }
        if (tstop - tstart) / tstep > 1e6 {
            return Err("transient would produce more than 1000000 points; raise TSTEP".into());
        }
        let o = &self.options;
        // SPICE3's maximum step: TMAX, else the smaller of TSTEP and a fiftieth of the span.
        let h_max = match tmax {
            Some(m) if m > 0.0 => m,
            _ => tstep.min((tstop - tstart) / 50.0),
        };
        let h_min = h_max * 1e-9;
        // Threshold crossings are located to within this.
        let t_tol = (h_max * 1e-4).max(1e-15);
        let n_dev = self.devices.len();
        let mut dsim = digital::Sim::new(&self.digital);
        let mut switches: Vec<bool> = self
            .devices
            .iter()
            .map(|d| {
                matches!(
                    d,
                    Device::S {
                        initial: Some(true),
                        ..
                    }
                )
            })
            .collect();
        // Initial state: the operating point at t = 0, or zero with the given ICs.
        let (mut x, mut lim) = if uic {
            // Power-up: the logic reads its inputs at the first accepted point; until
            // then every net is unknown and the DACs sit at their undefined level.
            dsim.start(&self.digital, &[]);
            (vec![0.0; engine.size + 1], vec![[0.0; 3]; n_dev])
        } else {
            dsim.start(&self.digital, &[]);
            self.mixed_op(
                engine,
                Mode::Dc {
                    scale: 1.0,
                    time: Some(0.0),
                },
                None,
                None,
                &mut dsim,
                &mut switches,
                log,
            )?
        };
        // The DAC outputs start at their settled levels.
        let mut ramps: Vec<Option<Ramp>> = self
            .devices
            .iter()
            .map(|d| match d {
                Device::Dac { net, levels, .. } => {
                    let v = levels.level(dsim.values[*net]);
                    Some(Ramp {
                        t0: 0.0,
                        v0: v,
                        v1: v,
                        dur: 0.0,
                    })
                }
                _ => None,
            })
            .collect();
        let dac_at = |ramps: &[Option<Ramp>], t: f64| -> Vec<f64> {
            ramps.iter().map(|r| r.map_or(0.0, |r| r.at(t))).collect()
        };
        // Slot history from the initial point.
        let mut slots = vec![Slot::default(); engine.nslots];
        {
            let dac = dac_at(&ramps, 0.0);
            for (i, d) in self.devices.iter().enumerate() {
                match d {
                    Device::C { ic: Some(v), .. } if uic => {
                        if let Device::C { c, .. } = d {
                            slots[engine.slot[i]].q = [c * v; 4];
                            slots[engine.slot[i]].v = *v;
                        }
                    }
                    Device::L { ic, l, .. } => {
                        let k = engine.branch[i];
                        if let (true, Some(v)) = (uic, ic) {
                            x[k] = *v;
                        }
                        slots[engine.slot[i]].q = [l * x[k]; 4];
                    }
                    _ => {}
                }
            }
            let ctx = Ctx {
                mode: Mode::Dc {
                    scale: 1.0,
                    time: Some(0.0),
                },
                sweep: None,
                gmin_extra: 0.0,
                slots: &slots,
                switch_on: &switches,
                dac: &dac,
                first: true,
            };
            let vals = self.slot_values(engine, &x, &lim, &ctx);
            for (i, d) in self.devices.iter().enumerate() {
                if matches!(d, Device::L { .. })
                    || (uic && matches!(d, Device::C { ic: Some(_), .. }))
                {
                    continue;
                }
                for k in 0..d.slot_count() {
                    let s = &mut slots[engine.slot[i] + k];
                    let (q, half, v) = vals[engine.slot[i] + k];
                    s.q = [q; 4];
                    s.half = half;
                    s.v = v;
                }
            }
        }
        let mut static_breaks = vec![tstop];
        for d in &self.devices {
            if let Device::V { src, .. } | Device::I { src, .. } = d {
                if let Some(w) = &src.wave {
                    w.breakpoints(tstop, &mut static_breaks);
                }
            }
        }
        static_breaks.retain(|t| *t > 0.0);
        static_breaks.sort_by(|a, b| a.total_cmp(b));
        static_breaks.dedup_by(|a, b| (*a - *b).abs() <= h_min);
        let mut times = vec![0.0];
        let mut states: Vec<Vec<f64>> = vec![x.clone()];
        let digital_levels =
            |d: &digital::Sim| d.values.iter().map(|v| v.level()).collect::<Vec<_>>();
        let mut dstates: Vec<Vec<f64>> = vec![digital_levels(&dsim)];
        let mut t = 0.0;
        // Steps taken so far, newest first, for the divided differences.
        let mut deltas: Vec<f64> = vec![];
        let mut h = h_max / 100.0;
        let mut order = 1usize;
        let mut first = true;
        let mut steps = 0usize;
        let mut rejected = 0usize;
        let mut rejected_newton = 0usize;
        let mut next_static = 0usize;
        while t < tstop * (1.0 - 1e-12) {
            while next_static < static_breaks.len() && static_breaks[next_static] <= t + h_min {
                next_static += 1;
            }
            // The next breakpoint: a waveform corner, a digital event, the end of a DAC edge.
            let mut bp = static_breaks.get(next_static).copied().unwrap_or(tstop);
            if let Some(te) = dsim.next_event() {
                if te > t + h_min * 0.5 && te < bp {
                    bp = te;
                }
            }
            for r in ramps.iter().flatten() {
                for edge in [r.t0, r.t0 + r.dur] {
                    if edge > t + h_min * 0.5 && edge < bp {
                        bp = edge;
                    }
                }
            }
            let mut target = t + h.min(h_max);
            let mut hits_break = false;
            if target >= bp - h_min {
                target = bp;
                hits_break = true;
            } else if bp - target < 0.1 * (target - t) {
                // Do not leave a sliver before the breakpoint: take two even steps.
                target = t + (bp - t) / 2.0;
            }
            let dt = target - t;
            let trap = self.trapezoidal && order == 2;
            let dac = dac_at(&ramps, target);
            let ctx = Ctx {
                mode: Mode::Tran {
                    t: target,
                    h: dt,
                    trap,
                },
                sweep: None,
                gmin_extra: 0.0,
                slots: &slots,
                switch_on: &switches,
                dac: &dac,
                first,
            };
            let mut trial_lim = lim.clone();
            let new = match engine.newton(&x, &ctx, &mut trial_lim, 50) {
                Ok(new) => new,
                Err(e) => {
                    rejected_newton += 1;
                    h = dt / 8.0;
                    order = 1;
                    if h < h_min {
                        return Err(format!(
                            "timestep too small at t = {}: {e}",
                            num::format_si(t, 4, "s")
                        ));
                    }
                    continue;
                }
            };
            // Charges and companion currents at the new point.
            let vals = self.slot_values(engine, &new, &trial_lim, &ctx);
            let f = if trap { 2.0 / dt } else { 1.0 / dt };
            let currents: Vec<f64> = (0..engine.nslots)
                .map(|k| {
                    let s = &slots[k];
                    f * (vals[k].0 - s.q[0]) - if trap { s.i[0] } else { 0.0 }
                })
                .collect();
            // Local truncation error: SPICE skips it on the first point.
            let mut hist = vec![dt];
            hist.extend(deltas.iter().take(3).copied());
            let lte_step = |ord: usize| -> f64 {
                if first || hist.len() < ord + 1 {
                    return f64::INFINITY;
                }
                let mut best = f64::INFINITY;
                for k in 0..engine.nslots {
                    let s = truncation_step(o, &slots[k], vals[k].0, currents[k], &hist, ord);
                    if s < best {
                        best = s;
                    }
                }
                best
            };
            let mut new_h = lte_step(order).min(2.0 * dt);
            // What a second-order step would allow, for raising the order afterwards.
            let second_order = if order == 1 && self.trapezoidal && !hits_break {
                lte_step(2).min(2.0 * dt)
            } else {
                0.0
            };
            if new_h < 0.9 * dt && !at_min_step(dt, h_min) {
                rejected += 1;
                h = new_h.max(h_min);
                if new_h < h_min {
                    return Err(format!(
                        "timestep too small at t = {}: the truncation error cannot be controlled",
                        num::format_si(t, 4, "s")
                    ));
                }
                continue;
            }
            // Thresholds: an ADC or switch that changed state inside the step is landed on.
            let mut crossing: Option<f64> = None;
            let mut note = |tc: f64| {
                if target - tc > t_tol && crossing.is_none_or(|c| tc < c) {
                    crossing = Some(tc);
                }
            };
            for (k, a) in self.digital.adcs.iter().enumerate() {
                let (v0, v1) = (x[a.node], new[a.node]);
                if a.read(v1) != dsim.adc_values[k] {
                    for thr in a.thresholds() {
                        if (v0 - thr) * (v1 - thr) <= 0.0 && v1 != v0 {
                            note(t + (thr - v0) / (v1 - v0) * dt);
                        }
                    }
                }
            }
            let next_sw = engine.switch_states(&new, &switches);
            for (i, d) in self.devices.iter().enumerate() {
                if let Device::S { cp, cn, model, .. } = d {
                    if next_sw[i] != switches[i] {
                        let (v0, v1) = (x[*cp] - x[*cn], new[*cp] - new[*cn]);
                        let thr = if next_sw[i] {
                            model.vt + model.vh
                        } else {
                            model.vt - model.vh
                        };
                        if v1 != v0 {
                            note(t + (thr - v0) / (v1 - v0) * dt);
                        }
                    }
                }
            }
            if let Some(tc) = crossing {
                if tc > t {
                    rejected += 1;
                    h = (tc - t + t_tol * 0.5).max(h_min);
                    order = 1;
                    continue;
                }
            }
            // Accept.
            for k in 0..engine.nslots {
                let s = &mut slots[k];
                s.q = [vals[k].0, s.q[0], s.q[1], s.q[2]];
                s.i = [currents[k], s.i[0]];
                s.half = vals[k].1;
                s.v = vals[k].2;
            }
            // An inductor's slot current is its voltage.
            for (i, d) in self.devices.iter().enumerate() {
                if let Device::L { a, b, .. } = d {
                    let s = &mut slots[engine.slot[i]];
                    s.i[0] = new[*a] - new[*b];
                }
            }
            x = new;
            lim = trial_lim;
            t = target;
            deltas.insert(0, dt);
            deltas.truncate(4);
            switches = next_sw;
            if !self.digital.is_empty() {
                dsim.sample(&self.digital, t, &x);
                for (net, _, te) in dsim.run_until(&self.digital, t + h_min * 0.5, &x) {
                    for (i, d) in self.devices.iter().enumerate() {
                        if let Device::Dac {
                            net: dn, levels, ..
                        } = d
                        {
                            if *dn == net {
                                let now = ramps[i].map_or(0.0, |r| r.at(te));
                                let target_v = levels.level(dsim.values[net]);
                                let dur = if target_v >= now {
                                    levels.t_rise
                                } else {
                                    levels.t_fall
                                };
                                ramps[i] = Some(Ramp {
                                    t0: te.max(t),
                                    v0: now,
                                    v1: target_v,
                                    dur: dur.max(digital::MIN_DELAY),
                                });
                            }
                        }
                    }
                }
            }
            times.push(t);
            states.push(x.clone());
            dstates.push(digital_levels(&dsim));
            steps += 1;
            first = false;
            if steps > 5_000_000 {
                return Err("transient took more than 5000000 steps".into());
            }
            // Order and step for what comes next.
            if hits_break {
                order = 1;
                let gap = static_breaks
                    .get(next_static + 1)
                    .map_or(h_max, |b| b - t)
                    .max(h_min);
                new_h = new_h.min(0.1 * gap.min(h_max)).min(dt.max(h_max / 100.0));
            } else if order == 1 && self.trapezoidal && second_order > 1.05 * dt {
                order = 2;
                new_h = second_order;
            }
            h = new_h.min(h_max).max(h_min);
            if h.is_infinite() {
                h = h_max;
            }
        }
        log.push(format!(
            "Transient: {steps} time points accepted, {rejected} rejected for truncation error or thresholds, {rejected_newton} for convergence, integration {}",
            if self.trapezoidal {
                "trapezoidal"
            } else {
                "backward Euler"
            }
        ));
        result.rejected = rejected + rejected_newton;
        // Report every accepted point from TSTART on; TSTART itself is interpolated.
        result.x_name = "time".into();
        for (sig, _) in self.digital.nets.iter().zip(0..) {
            result.signals.push(Signal {
                name: format!("D({sig})"),
                re: vec![],
                im: vec![],
            });
        }
        let n_analog = names.len();
        let push = |result: &mut SimResult, at: f64, s: &[f64], ds: &[f64]| {
            result.x.push(at);
            for (sig, (_, k)) in result.signals.iter_mut().zip(names) {
                sig.re.push(s[*k]);
            }
            for (j, sig) in result.signals[n_analog..].iter_mut().enumerate() {
                sig.re.push(ds.get(j).copied().unwrap_or(0.5));
            }
        };
        let first_kept = times.partition_point(|v| *v < tstart);
        if tstart > 0.0 && first_kept > 0 && first_kept < times.len() {
            let (t0, t1) = (times[first_kept - 1], times[first_kept]);
            let frac = if t1 > t0 {
                (tstart - t0) / (t1 - t0)
            } else {
                0.0
            };
            let (s0, s1) = (&states[first_kept - 1], &states[first_kept]);
            let s: Vec<f64> = s0.iter().zip(s1).map(|(a, b)| a + (b - a) * frac).collect();
            push(result, tstart, &s, &dstates[first_kept - 1]);
        }
        for j in first_kept..times.len() {
            if tstart > 0.0 && times[j] == tstart && !result.x.is_empty() {
                continue;
            }
            push(result, times[j], &states[j], &dstates[j]);
        }
        Ok(())
    }
}

/// A step already at the smallest size is accepted whatever its error, as SPICE does.
fn at_min_step(dt: f64, h_min: f64) -> bool {
    dt <= h_min * 1.5
}

fn ac_frequencies(scale: &str, points: u32, fstart: f64, fstop: f64) -> Result<Vec<f64>, String> {
    let mut out = Vec::new();
    match scale {
        "lin" => {
            let n = points.max(1);
            for i in 0..n {
                out.push(if n == 1 {
                    fstart
                } else {
                    fstart + (fstop - fstart) * i as f64 / (n - 1) as f64
                });
            }
        }
        "dec" | "oct" => {
            let base = if scale == "dec" { 10.0 } else { 2.0 };
            let span = ln(fstop / fstart) / ln(base);
            let n = (span * points as f64 + 1e-9) as u32 + 1;
            if n > 100_000 {
                return Err("AC sweep has more than 100000 points".into());
            }
            for i in 0..n {
                out.push(fstart * num::pow(base, i as f64 / points as f64));
            }
        }
        other => return Err(format!("unknown AC sweep {other}")),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_divider_solves_exactly() {
        let c = parse("divider\nV1 in 0 10\nR1 in out 1k\nR2 out 0 1k\n.op\n.end\n").unwrap();
        let r = c.run().unwrap();
        // The 1 pS shunt on every node moves the answer by parts per billion only.
        let out = r.value_at("V(out)", 0.0).unwrap();
        assert!((out - 5.0).abs() < 1e-8, "{out}");
        // 10 V across 2 kΩ: 5 mA flows out of the source's positive terminal.
        let i = r.value_at("I(V1)", 0.0).unwrap();
        assert!((i + 5e-3).abs() < 1e-10, "{i}");
    }
    #[test]
    fn pulse_and_pwl_waves_have_their_corners() {
        let p = Wave::Pulse {
            v1: 0.0,
            v2: 5.0,
            td: 1.0,
            tr: 1.0,
            tf: 1.0,
            pw: Some(2.0),
            per: Some(10.0),
        };
        assert_eq!(p.at(0.5), 0.0);
        assert_eq!(p.at(1.5), 2.5);
        assert_eq!(p.at(3.0), 5.0);
        assert_eq!(p.at(4.5), 2.5);
        assert_eq!(p.at(11.5), 2.5);
        let w = Wave::Pwl(vec![(0.0, 0.0), (1.0, 2.0)]);
        assert_eq!(w.at(0.5), 1.0);
        assert_eq!(w.at(5.0), 2.0);
    }
    #[test]
    fn truncation_step_of_a_quadratic_charge() {
        // q = t^2 has a zero third derivative: trapezoidal integration is exact, so the
        // error bound allows any step; a cubic's third divided difference is exactly 1.
        let o = Options::default();
        let h = 1e-6;
        let at = |t: f64| t * t * t;
        let s = Slot {
            q: [at(3.0 * h), at(2.0 * h), at(h), 0.0],
            i: [0.0, 0.0],
            v: 0.0,
            half: 0.0,
        };
        let step = truncation_step(&o, &s, at(4.0 * h), 0.0, &[h, h, h], 2);
        // diff = 1 (q''' / 3!), tol = reltol * |q| / h at q = 64e-18.
        let tol = o.abstol.max(o.reltol * at(4.0 * h).max(o.chgtol) / h);
        let expected = (o.trtol * tol / o.abstol.max(1.0 / 12.0)).sqrt();
        assert!(
            (step - expected).abs() < expected * 1e-9,
            "{step} vs {expected}"
        );
    }
}
