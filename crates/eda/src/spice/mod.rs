//! A SPICE-class circuit simulator.
//!
//! Modified nodal analysis over a dense LU with partial pivoting. Nonlinear devices are
//! linearised by Newton–Raphson with SPICE3's junction limiting, falling back to gmin
//! and source stepping when a plain Newton solve of the operating point fails.
//! Analyses: DC operating point, DC sweep, AC small signal (complex MNA) and transient
//! (trapezoidal, with backward Euler after every breakpoint; the step is cut on a
//! non-converging point and grown back).
//!
//! Device models: resistor, capacitor, inductor, independent V/I sources (DC, AC,
//! PULSE, SIN, PWL), the four controlled sources E/F/G/H, the diode (Shockley with
//! series resistance, reverse breakdown, junction and transit-time charge), the bipolar
//! transistor (Ebers–Moll transport model with forward Early effect) and the level-1
//! (Shichman–Hodges) MOSFET. All arithmetic goes through `crate::num`, so a waveform is
//! bit-identical on every host.
pub mod matrix;
mod parse;

pub use parse::parse;

use crate::num::{self, exp, ln};
use matrix::{Complex, Real, C64};
use serde::{Deserialize, Serialize};

/// Thermal voltage kT/q at 27 °C, as SPICE uses by default.
pub const VT: f64 = 0.025_852_017_444_120_28;
const GMIN: f64 = 1e-12;
/// Conductance from every node to ground, so a node only reached through capacitors
/// still has a DC solution (ngspice's `rshunt`).
const GSHUNT: f64 = 1e-12;

#[derive(Clone, Debug, PartialEq)]
pub struct DiodeModel {
    pub is: f64,
    pub n: f64,
    pub rs: f64,
    pub bv: Option<f64>,
    pub ibv: f64,
    pub cjo: f64,
    pub vj: f64,
    pub m: f64,
    pub tt: f64,
    pub fc: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BjtModel {
    pub pnp: bool,
    pub is: f64,
    pub bf: f64,
    pub br: f64,
    pub nf: f64,
    pub nr: f64,
    pub vaf: Option<f64>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct MosModel {
    pub pmos: bool,
    pub vto: f64,
    pub kp: f64,
    pub lambda: f64,
}

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
        /// Node between the series resistance and the junction, assigned at setup.
        internal: Option<usize>,
        model: DiodeModel,
        area: f64,
    },
    Q {
        name: String,
        c: usize,
        b: usize,
        e: usize,
        model: BjtModel,
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
            | Self::M { name, .. } => name,
        }
    }
    /// Devices with a branch-current unknown in the MNA system.
    fn has_branch(&self) -> bool {
        matches!(
            self,
            Self::V { .. } | Self::L { .. } | Self::E { .. } | Self::H { .. }
        )
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

#[derive(Clone, Debug, PartialEq)]
pub struct Circuit {
    pub title: String,
    /// Node names; index 0 is ground.
    pub nodes: Vec<String>,
    pub devices: Vec<Device>,
    pub analyses: Vec<Analysis>,
    pub trapezoidal: bool,
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
    pub x: Vec<f64>,
    pub signals: Vec<Signal>,
    /// What the run did, in the words a SPICE console would print.
    pub log: Vec<String>,
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
}

// ---------------------------------------------------------------------------
// Device equations
// ---------------------------------------------------------------------------

/// SPICE3 `pnjlim`: keep a junction voltage from jumping so far up the exponential
/// that the next linearisation is useless.
fn pnjlim(vnew: f64, vold: f64, vt: f64, vcrit: f64) -> f64 {
    if vnew > vcrit && (vnew - vold).abs() > vt + vt {
        if vold > 0.0 {
            let arg = 1.0 + (vnew - vold) / vt;
            if arg > 0.0 {
                vold + vt * ln(arg)
            } else {
                vcrit
            }
        } else {
            vt * ln(vnew / vt)
        }
    } else {
        vnew
    }
}
fn vcrit(vt: f64, is: f64) -> f64 {
    vt * ln(vt / (std::f64::consts::SQRT_2 * is))
}

/// Diode current and conductance at junction voltage `vd`.
fn diode_iv(m: &DiodeModel, area: f64, vd: f64) -> (f64, f64) {
    let vt = VT * m.n;
    let is = m.is * area;
    let (mut id, mut gd) = if vd >= -3.0 * vt {
        let e = exp(vd / vt);
        (is * (e - 1.0), is * e / vt)
    } else {
        let a = 3.0 * vt / (vd * std::f64::consts::E);
        let a3 = a * a * a;
        (-is * (1.0 + a3), is * 3.0 * a3 / vd)
    };
    if let Some(bv) = m.bv {
        if vd < -bv + 40.0 * vt {
            let e = exp(-(bv + vd) / vt);
            id -= m.ibv * area * e;
            gd += m.ibv * area * e / vt;
        }
    }
    (id + GMIN * vd, gd + GMIN)
}
/// Depletion charge and capacitance of a junction (SPICE's forward-bias linearisation
/// above `fc * vj`).
fn junction_charge(cjo: f64, vj: f64, m: f64, fc: f64, v: f64) -> (f64, f64) {
    if cjo == 0.0 {
        return (0.0, 0.0);
    }
    let knee = fc * vj;
    if v < knee {
        let arg = 1.0 - v / vj;
        let s = num::pow(arg, -m);
        (cjo * vj * (1.0 - arg * s) / (1.0 - m), cjo * s)
    } else {
        let (q0, _) = junction_charge(cjo, vj, m, fc, knee - 1e-12);
        let f1 = num::pow(1.0 - fc, -(1.0 + m));
        let c0 = 1.0 - fc * (1.0 + m);
        let dq = cjo * f1 * (c0 * (v - knee) + m / (2.0 * vj) * (v * v - knee * knee));
        (q0 + dq, cjo * f1 * (c0 + m * v / vj))
    }
}
fn diode_charge(m: &DiodeModel, area: f64, vd: f64) -> (f64, f64) {
    let (id, gd) = diode_iv(m, area, vd);
    let (qj, cj) = junction_charge(m.cjo * area, m.vj, m.m, m.fc, vd);
    (m.tt * id + qj, m.tt * gd + cj)
}

/// Bipolar transistor in the transport form with a forward Early voltage. Voltages are
/// already polarity-corrected (a PNP's vbe is v(e) - v(b) in circuit terms).
struct BjtPoint {
    ic: f64,
    ib: f64,
    /// dIc/dVbe, dIc/dVbc, dIb/dVbe, dIb/dVbc.
    gcbe: f64,
    gcbc: f64,
    gbbe: f64,
    gbbc: f64,
}
fn bjt_eval(m: &BjtModel, vbe: f64, vbc: f64) -> BjtPoint {
    let vtf = VT * m.nf;
    let vtr = VT * m.nr;
    let ef = exp(vbe / vtf);
    let er = exp(vbc / vtr);
    let ibf = m.is * (ef - 1.0);
    let ibr = m.is * (er - 1.0);
    let gbf = m.is * ef / vtf;
    let gbr = m.is * er / vtr;
    let (k, dk) = match m.vaf {
        Some(va) if va > 0.0 => {
            let k = 1.0 - vbc / va;
            if k < 0.1 {
                (0.1, 0.0)
            } else {
                (k, -1.0 / va)
            }
        }
        _ => (1.0, 0.0),
    };
    let ic = (ibf - ibr) * k - ibr / m.br;
    let ib = ibf / m.bf + ibr / m.br;
    BjtPoint {
        ic: ic - GMIN * vbc,
        ib: ib + GMIN * vbe + GMIN * vbc,
        gcbe: gbf * k,
        gcbc: -gbr * k + (ibf - ibr) * dk - gbr / m.br - GMIN,
        gbbe: gbf / m.bf + GMIN,
        gbbc: gbr / m.br + GMIN,
    }
}

/// Level-1 MOSFET drain current with the source/drain exchange for negative vds.
/// Returns (id, dId/dVgs, dId/dVds) in the polarity-corrected frame.
fn mos_eval(m: &MosModel, w: f64, l: f64, vgs: f64, vds: f64) -> (f64, f64, f64) {
    let beta = m.kp * w / l;
    let vt = if m.pmos { -m.vto } else { m.vto };
    let core = |vgs: f64, vds: f64| -> (f64, f64, f64) {
        let vov = vgs - vt;
        if vov <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        let clm = 1.0 + m.lambda * vds;
        if vds < vov {
            let id = beta * (vov - vds / 2.0) * vds * clm;
            let gm = beta * vds * clm;
            let gds = beta * (vov - vds) * clm + beta * (vov - vds / 2.0) * vds * m.lambda;
            (id, gm, gds)
        } else {
            let id = beta / 2.0 * vov * vov * clm;
            (id, beta * vov * clm, beta / 2.0 * vov * vov * m.lambda)
        }
    };
    if vds >= 0.0 {
        let (id, gm, gds) = core(vgs, vds);
        (id + GMIN * vds, gm, gds + GMIN)
    } else {
        // Drain and source swap roles: the current flows the other way.
        let (id, gm, gds) = core(vgs - vds, -vds);
        (-id + GMIN * vds, -gm, gm + gds + GMIN)
    }
}

// ---------------------------------------------------------------------------
// The MNA system
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    /// Operating point; `scale` multiplies every independent source (source stepping).
    /// `time` evaluates time-varying sources at that instant, for the point a
    /// transient starts from.
    Dc {
        scale: f64,
        time: Option<f64>,
    },
    Tran {
        t: f64,
        h: f64,
        trap: bool,
    },
}

/// Per-device state carried from one accepted time point to the next.
#[derive(Clone, Debug)]
struct History {
    /// Capacitor voltage / inductor current / diode charge at the previous point.
    x: Vec<f64>,
    /// Capacitor current / inductor voltage / diode charge current there.
    y: Vec<f64>,
}

struct Engine<'a> {
    c: &'a Circuit,
    /// Unknowns: node voltages 1..=nodes, then branch currents.
    size: usize,
    nodes: usize,
    names: Vec<String>,
    branch: Vec<usize>,
    internal: Vec<usize>,
}

impl<'a> Engine<'a> {
    fn new(c: &'a Circuit) -> Result<Self, String> {
        let mut nodes = c.nodes.len() - 1;
        let mut names = c.nodes.clone();
        let mut internal = vec![0; c.devices.len()];
        for (i, d) in c.devices.iter().enumerate() {
            if let Device::D { name, model, .. } = d {
                if model.rs > 0.0 {
                    nodes += 1;
                    internal[i] = nodes;
                    names.push(format!("{name}#internal"));
                }
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
            internal,
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
    /// Stamp the linearised system at `x`. `lim` holds each nonlinear device's junction
    /// voltages from the last iteration and is updated with the limited values used.
    #[allow(clippy::too_many_arguments)]
    fn load(
        &self,
        x: &[f64],
        mode: Mode,
        hist: &History,
        lim: &mut [[f64; 2]],
        limit: bool,
        sweep: Option<(usize, f64)>,
        gmin_extra: f64,
        m: &mut Real,
    ) {
        m.clear();
        for node in 1..=self.nodes {
            m.add(node, node, GSHUNT + gmin_extra);
        }
        let v = |n: usize| x[n];
        for (i, d) in self.c.devices.iter().enumerate() {
            let swept = sweep.filter(|s| s.0 == i).map(|s| s.1);
            match d {
                Device::R { a, b, r, .. } => {
                    let g = 1.0 / r;
                    m.add(*a, *a, g);
                    m.add(*b, *b, g);
                    m.add(*a, *b, -g);
                    m.add(*b, *a, -g);
                }
                Device::C { a, b, c, .. } => {
                    if let Mode::Tran { h, trap, .. } = mode {
                        let geq = if trap { 2.0 * c / h } else { c / h };
                        let ieq = if trap {
                            -(geq * hist.x[i] + hist.y[i])
                        } else {
                            -geq * hist.x[i]
                        };
                        m.add(*a, *a, geq);
                        m.add(*b, *b, geq);
                        m.add(*a, *b, -geq);
                        m.add(*b, *a, -geq);
                        m.rhs(*a, -ieq);
                        m.rhs(*b, ieq);
                    }
                }
                Device::L { a, b, l, .. } => {
                    let k = self.branch[i];
                    m.add(*a, k, 1.0);
                    m.add(*b, k, -1.0);
                    m.add(k, *a, 1.0);
                    m.add(k, *b, -1.0);
                    if let Mode::Tran { h, trap, .. } = mode {
                        let req = if trap { 2.0 * l / h } else { l / h };
                        m.add(k, k, -req);
                        let rhs = if trap {
                            -req * hist.x[i] - hist.y[i]
                        } else {
                            -req * hist.x[i]
                        };
                        m.rhs(k, rhs);
                    }
                }
                Device::V { p, n, src, .. } => {
                    let k = self.branch[i];
                    m.add(*p, k, 1.0);
                    m.add(*n, k, -1.0);
                    m.add(k, *p, 1.0);
                    m.add(k, *n, -1.0);
                    m.rhs(k, Self::source_value(src, mode, swept));
                }
                Device::I { p, n, src, .. } => {
                    let val = Self::source_value(src, mode, swept);
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
                Device::D {
                    a, k, model, area, ..
                } => {
                    let ai = if self.internal[i] != 0 {
                        let ri = self.internal[i];
                        let g = 1.0 / (model.rs / area);
                        m.add(*a, *a, g);
                        m.add(ri, ri, g);
                        m.add(*a, ri, -g);
                        m.add(ri, *a, -g);
                        ri
                    } else {
                        *a
                    };
                    let raw = v(ai) - v(*k);
                    let vt = VT * model.n;
                    let vd = if limit {
                        let mut vd = pnjlim(raw, lim[i][0], vt, vcrit(vt, model.is * area));
                        if let Some(bv) = model.bv {
                            // Limit the reflected voltage in breakdown the same way.
                            if vd < -bv + 10.0 * vt {
                                let r =
                                    pnjlim(-(vd + bv), -(lim[i][0] + bv), vt, vcrit(vt, model.ibv));
                                vd = -(r + bv);
                            }
                        }
                        vd
                    } else {
                        raw
                    };
                    lim[i][0] = vd;
                    let (id, gd) = diode_iv(model, *area, vd);
                    let mut g = gd;
                    let mut ieq = id - gd * vd;
                    if let Mode::Tran { h, trap, .. } = mode {
                        if model.cjo != 0.0 || model.tt != 0.0 {
                            let (q, cq) = diode_charge(model, *area, vd);
                            let f = if trap { 2.0 / h } else { 1.0 / h };
                            let prev_i = if trap { hist.y[i] } else { 0.0 };
                            // i = f (q(v) - q_prev) - i_prev, linearised around vd.
                            g += f * cq;
                            ieq += f * (q - cq * vd - hist.x[i]) - prev_i;
                        }
                    }
                    m.add(ai, ai, g);
                    m.add(*k, *k, g);
                    m.add(ai, *k, -g);
                    m.add(*k, ai, -g);
                    m.rhs(ai, -ieq);
                    m.rhs(*k, ieq);
                }
                Device::Q { c, b, e, model, .. } => {
                    let p = if model.pnp { -1.0 } else { 1.0 };
                    let raw_be = p * (v(*b) - v(*e));
                    let raw_bc = p * (v(*b) - v(*c));
                    let vt = VT * model.nf;
                    let crit = vcrit(vt, model.is);
                    let (vbe, vbc) = if limit {
                        (
                            pnjlim(raw_be, lim[i][0], vt, crit),
                            pnjlim(raw_bc, lim[i][1], VT * model.nr, crit),
                        )
                    } else {
                        (raw_be, raw_bc)
                    };
                    lim[i] = [vbe, vbc];
                    let q = bjt_eval(model, vbe, vbc);
                    // Currents into the terminals in circuit polarity, linearised.
                    let ic0 = p * (q.ic - q.gcbe * vbe - q.gcbc * vbc);
                    let ib0 = p * (q.ib - q.gbbe * vbe - q.gbbc * vbc);
                    // dIc/dv(b) = gcbe + gcbc, dIc/dv(e) = -gcbe, dIc/dv(c) = -gcbc.
                    let rows = [
                        (*c, q.gcbe + q.gcbc, -q.gcbe, -q.gcbc, ic0),
                        (*b, q.gbbe + q.gbbc, -q.gbbe, -q.gbbc, ib0),
                        (
                            *e,
                            -(q.gcbe + q.gcbc + q.gbbe + q.gbbc),
                            q.gcbe + q.gbbe,
                            q.gcbc + q.gbbc,
                            -(ic0 + ib0),
                        ),
                    ];
                    for (row, gb, ge, gc, i0) in rows {
                        m.add(row, *b, gb);
                        m.add(row, *e, ge);
                        m.add(row, *c, gc);
                        m.rhs(row, -i0);
                    }
                }
                Device::M {
                    d,
                    g,
                    s,
                    model,
                    w,
                    l,
                    ..
                } => {
                    let p = if model.pmos { -1.0 } else { 1.0 };
                    let raw_gs = p * (v(*g) - v(*s));
                    let raw_ds = p * (v(*d) - v(*s));
                    let (vgs, vds) = if limit {
                        (
                            lim[i][0] + (raw_gs - lim[i][0]).clamp(-2.0, 2.0),
                            lim[i][1] + (raw_ds - lim[i][1]).clamp(-4.0, 4.0),
                        )
                    } else {
                        (raw_gs, raw_ds)
                    };
                    lim[i] = [vgs, vds];
                    let (id, gm, gds) = mos_eval(model, *w, *l, vgs, vds);
                    let i0 = p * (id - gm * vgs - gds * vds);
                    // Drain current: gm (vg - vs) + gds (vd - vs) + i0; source the negative.
                    m.add(*d, *g, gm);
                    m.add(*d, *d, gds);
                    m.add(*d, *s, -(gm + gds));
                    m.rhs(*d, -i0);
                    m.add(*s, *g, -gm);
                    m.add(*s, *d, -gds);
                    m.add(*s, *s, gm + gds);
                    m.rhs(*s, i0);
                }
            }
        }
    }

    fn converged(&self, old: &[f64], new: &[f64]) -> bool {
        (1..=self.size).all(|k| {
            let (a, b) = (old[k], new[k]);
            let tol = 1e-6 * a.abs().max(b.abs()) + if k <= self.nodes { 1e-9 } else { 1e-12 };
            (a - b).abs() <= tol
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn newton(
        &self,
        x0: &[f64],
        mode: Mode,
        hist: &History,
        lim: &mut [[f64; 2]],
        sweep: Option<(usize, f64)>,
        gmin_extra: f64,
        max_iter: usize,
    ) -> Result<Vec<f64>, String> {
        let mut x = x0.to_vec();
        let mut m = Real::new(self.size);
        let nonlinear = self
            .c
            .devices
            .iter()
            .any(|d| matches!(d, Device::D { .. } | Device::Q { .. } | Device::M { .. }));
        for iter in 0..max_iter {
            self.load(
                &x,
                mode,
                hist,
                lim,
                iter > 0 || x0.iter().any(|v| *v != 0.0),
                sweep,
                gmin_extra,
                &mut m,
            );
            let new = m.solve().map_err(|k| self.singular(k))?;
            if new.iter().any(|v| !v.is_finite()) {
                return Err("the solution diverged".into());
            }
            let done = !nonlinear || (iter > 0 && self.converged(&x, &new));
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

    fn fresh_history(&self) -> History {
        History {
            x: vec![0.0; self.c.devices.len()],
            y: vec![0.0; self.c.devices.len()],
        }
    }

    /// DC operating point with the standard SPICE fallbacks.
    fn operating_point(
        &self,
        sweep: Option<(usize, f64)>,
        guess: Option<&[f64]>,
        scale_mode: Mode,
        log: &mut Vec<String>,
    ) -> Result<(Vec<f64>, Vec<[f64; 2]>), String> {
        let hist = self.fresh_history();
        let zero = vec![0.0; self.size + 1];
        let start = guess.unwrap_or(&zero);
        let mut lim = vec![[0.0; 2]; self.c.devices.len()];
        if let Ok(x) = self.newton(start, scale_mode, &hist, &mut lim, sweep, 0.0, 200) {
            return Ok((x, lim));
        }
        log.push("Note: Newton failed; trying gmin stepping".into());
        let mut x = zero.clone();
        let mut lim = vec![[0.0; 2]; self.c.devices.len()];
        let mut ok = true;
        let mut g = 1e-2;
        while g > 1e-13 {
            match self.newton(&x, scale_mode, &hist, &mut lim, sweep, g, 200) {
                Ok(next) => x = next,
                Err(_) => {
                    ok = false;
                    break;
                }
            }
            g /= 10.0;
        }
        if ok {
            if let Ok(x) = self.newton(&x, scale_mode, &hist, &mut lim, sweep, 0.0, 200) {
                log.push("Note: gmin stepping succeeded".into());
                return Ok((x, lim));
            }
        }
        log.push("Note: trying source stepping".into());
        let mut x = zero;
        let mut lim = vec![[0.0; 2]; self.c.devices.len()];
        let mut scale: f64 = 0.0;
        let mut step: f64 = 0.1;
        while scale < 1.0 {
            let next_scale = (scale + step).min(1.0);
            let mode = match scale_mode {
                Mode::Dc { scale: s, time } => Mode::Dc {
                    scale: s * next_scale,
                    time,
                },
                other => other,
            };
            let mut trial_lim = lim.clone();
            match self.newton(&x, mode, &hist, &mut trial_lim, sweep, 0.0, 200) {
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
}

impl Circuit {
    /// Run the first analysis the deck asks for, or an operating point if none.
    pub fn run(&self) -> Result<SimResult, String> {
        let analysis = self.analyses.first().cloned().unwrap_or(Analysis::Op);
        self.simulate(&analysis)
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
        };
        match analysis {
            Analysis::Op => {
                let (x, _) = engine.operating_point(
                    None,
                    None,
                    Mode::Dc {
                        scale: 1.0,
                        time: None,
                    },
                    &mut log,
                )?;
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
                for i in 0..count {
                    let value = start + step * i as f64;
                    let (x, _) = engine.operating_point(
                        Some((index, value)),
                        guess.as_deref(),
                        Mode::Dc {
                            scale: 1.0,
                            time: None,
                        },
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
                let (x, mut lim) = engine.operating_point(
                    None,
                    None,
                    Mode::Dc {
                        scale: 1.0,
                        time: None,
                    },
                    &mut log,
                )?;
                let mut g = Real::new(engine.size);
                let hist = engine.fresh_history();
                engine.load(
                    &x,
                    Mode::Dc {
                        scale: 1.0,
                        time: None,
                    },
                    &hist,
                    &mut lim,
                    false,
                    None,
                    0.0,
                    &mut g,
                );
                let freqs = ac_frequencies(scale, *points, *fstart, *fstop)?;
                result.x_name = "frequency".into();
                for f in freqs {
                    let omega = 2.0 * num::PI * f;
                    let sol = self.ac_point(&engine, &g, &x, omega)?;
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
        x: &[f64],
        omega: f64,
    ) -> Result<Vec<C64>, String> {
        let mut m = Complex::new(engine.size);
        for (i, v) in g.a.iter().enumerate() {
            if *v != 0.0 {
                m.a[i] = C64::new(*v, 0.0);
            }
        }
        let jw = |v: f64| C64::new(0.0, omega * v);
        for (i, d) in self.devices.iter().enumerate() {
            match d {
                Device::C { a, b, c, .. } => {
                    m.add(*a, *a, jw(*c));
                    m.add(*b, *b, jw(*c));
                    m.add(*a, *b, jw(-c));
                    m.add(*b, *a, jw(-c));
                }
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
                Device::D {
                    a, k, model, area, ..
                } if model.cjo != 0.0 || model.tt != 0.0 => {
                    let ai = if engine.internal[i] != 0 {
                        engine.internal[i]
                    } else {
                        *a
                    };
                    let (_, cq) = diode_charge(model, *area, x[ai] - x[*k]);
                    m.add(ai, ai, jw(cq));
                    m.add(*k, *k, jw(cq));
                    m.add(ai, *k, jw(-cq));
                    m.add(*k, ai, jw(-cq));
                }
                _ => {}
            }
        }
        m.solve().map_err(|k| engine.singular(k))
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
        let outputs = ((tstop - tstart) / tstep + 1e-9) as usize + 1;
        if outputs > 200_000 {
            return Err("transient would produce more than 200000 points; raise TSTEP".into());
        }
        let mut h_nom = tstep.min((tstop - tstart) / 50.0).min(tstop / 50.0);
        if let Some(m) = tmax {
            if m > 0.0 {
                h_nom = h_nom.min(m);
            }
        }
        let n_dev = self.devices.len();
        // Initial state: the operating point at t = 0, or zero with the given ICs.
        let (mut x, mut lim) = if uic {
            (vec![0.0; engine.size + 1], vec![[0.0; 2]; n_dev])
        } else {
            engine.operating_point(
                None,
                None,
                Mode::Dc {
                    scale: 1.0,
                    time: Some(0.0),
                },
                log,
            )?
        };
        let mut hist = engine.fresh_history();
        for (i, d) in self.devices.iter().enumerate() {
            match d {
                Device::C { a, b, ic, .. } => {
                    hist.x[i] = match (uic, ic) {
                        (true, Some(v)) => *v,
                        _ => x[*a] - x[*b],
                    };
                    hist.y[i] = 0.0;
                }
                Device::L { ic, .. } => {
                    let k = engine.branch[i];
                    if let (true, Some(v)) = (uic, ic) {
                        x[k] = *v;
                    }
                    hist.x[i] = x[k];
                    hist.y[i] = 0.0;
                }
                Device::D {
                    a, k, model, area, ..
                } => {
                    let ai = if engine.internal[i] != 0 {
                        engine.internal[i]
                    } else {
                        *a
                    };
                    hist.x[i] = diode_charge(model, *area, x[ai] - x[*k]).0;
                    hist.y[i] = 0.0;
                }
                _ => {}
            }
        }
        // Operating point at t = 0 is computed with sources at their t = 0 values.
        let mut breaks = vec![tstop];
        for d in &self.devices {
            if let Device::V { src, .. } | Device::I { src, .. } = d {
                if let Some(w) = &src.wave {
                    w.breakpoints(tstop, &mut breaks);
                }
            }
        }
        breaks.retain(|t| *t > 0.0);
        breaks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        breaks.dedup_by(|a, b| (*a - *b).abs() <= h_nom * 1e-9);
        let mut times = vec![0.0];
        let mut states: Vec<Vec<f64>> = vec![x.clone()];
        let mut t = 0.0;
        let mut h = h_nom / 64.0;
        let mut after_break = true;
        let mut next_break = 0;
        let mut steps = 0usize;
        let mut rejected = 0usize;
        let h_min = h_nom * 1e-9;
        while t < tstop * (1.0 - 1e-12) {
            while next_break < breaks.len() && breaks[next_break] <= t * (1.0 + 1e-12) + h_min {
                next_break += 1;
            }
            let mut target = t + h;
            let mut hits_break = false;
            // Land on every reporting instant, so the waveform on the TSTEP grid is
            // computed there rather than interpolated between neighbouring points.
            let grid = tstart + tstep * (floor((t - tstart) / tstep + 1e-9) + 1.0);
            if grid > t + h_min && target > grid - h_min {
                target = grid;
            }
            if let Some(b) = breaks.get(next_break) {
                if target >= *b - h_min {
                    target = *b;
                    hits_break = true;
                }
            }
            let dt = target - t;
            let mode = Mode::Tran {
                t: target,
                h: dt,
                trap: self.trapezoidal && !after_break,
            };
            let mut trial_lim = lim.clone();
            match engine.newton(&x, mode, &hist, &mut trial_lim, None, 0.0, 60) {
                Ok(new) => {
                    // Update the reactive elements' history from the accepted point.
                    for (i, d) in self.devices.iter().enumerate() {
                        let Mode::Tran { h, trap, .. } = mode else {
                            unreachable!()
                        };
                        match d {
                            Device::C { a, b, c, .. } => {
                                let v = new[*a] - new[*b];
                                let i_new = if trap {
                                    2.0 * c / h * (v - hist.x[i]) - hist.y[i]
                                } else {
                                    c / h * (v - hist.x[i])
                                };
                                hist.x[i] = v;
                                hist.y[i] = i_new;
                            }
                            Device::L { a, b, .. } => {
                                let k = engine.branch[i];
                                hist.x[i] = new[k];
                                hist.y[i] = new[*a] - new[*b];
                            }
                            Device::D {
                                a, k, model, area, ..
                            } if model.cjo != 0.0 || model.tt != 0.0 => {
                                let ai = if engine.internal[i] != 0 {
                                    engine.internal[i]
                                } else {
                                    *a
                                };
                                let (q, _) = diode_charge(model, *area, new[ai] - new[*k]);
                                let f = if trap { 2.0 / h } else { 1.0 / h };
                                let iq = if trap {
                                    f * (q - hist.x[i]) - hist.y[i]
                                } else {
                                    f * (q - hist.x[i])
                                };
                                hist.x[i] = q;
                                hist.y[i] = iq;
                            }
                            _ => {}
                        }
                    }
                    x = new;
                    lim = trial_lim;
                    t = target;
                    times.push(t);
                    states.push(x.clone());
                    steps += 1;
                    after_break = hits_break;
                    if hits_break {
                        // A corner restarts the integration: a short first-order step,
                        // then the step doubles back to nominal as SPICE does.
                        h = h_nom / 64.0;
                    } else if h < h_nom {
                        h = (h * 2.0).min(h_nom);
                    }
                    if steps > 2_000_000 {
                        return Err("transient took more than 2000000 steps".into());
                    }
                }
                Err(e) => {
                    rejected += 1;
                    h = dt / 8.0;
                    after_break = true;
                    if h < h_min {
                        return Err(format!(
                            "timestep too small at t = {}: {e}",
                            num::format_si(t, 4, "s")
                        ));
                    }
                }
            }
        }
        log.push(format!(
            "Transient: {steps} time points accepted, {rejected} rejected, integration {}",
            if self.trapezoidal {
                "trapezoidal"
            } else {
                "backward Euler"
            }
        ));
        // Report on the TSTEP grid by interpolating between accepted points.
        result.x_name = "time".into();
        let mut j = 0;
        for o in 0..outputs {
            let at = (tstart + tstep * o as f64).min(tstop);
            while j + 1 < times.len() && times[j + 1] < at {
                j += 1;
            }
            let (t0, t1) = (times[j], times[(j + 1).min(times.len() - 1)]);
            let frac = if t1 > t0 {
                ((at - t0) / (t1 - t0)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (s0, s1) = (&states[j], &states[(j + 1).min(states.len() - 1)]);
            result.x.push(at);
            for (s, (_, k)) in result.signals.iter_mut().zip(names) {
                s.re.push(s0[*k] + (s1[*k] - s0[*k]) * frac);
            }
        }
        Ok(())
    }
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
}
