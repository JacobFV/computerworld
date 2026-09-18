//! Event-driven digital simulation coupled to the analog engine, after ngspice's XSPICE
//! code models: logic gates (`d_and`, `d_nand`, `d_or`, `d_nor`, `d_xor`, `d_xnor`,
//! `d_buffer`, `d_inverter`), the edge-triggered `d_dff`, the `d_srlatch`, the `d_osc`
//! controlled oscillator and `d_pullup`/`d_pulldown`, with `adc_bridge` and `dac_bridge`
//! carrying levels between the two domains.
//!
//! Nets hold three-valued logic (0, 1, unknown). Every change is an event at a time; an
//! element whose input changes computes its outputs and schedules them after its delay.
//! Events at the same time run in the order they were scheduled, so a run is
//! reproducible. The analog engine lands on every event time as a breakpoint, samples
//! the ADC inputs at each accepted time point, and reads DAC outputs as ramps.
use std::collections::BTreeMap;
use std::ops::Not;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Logic {
    Zero,
    One,
    Unknown,
}
impl Not for Logic {
    type Output = Logic;
    /// Logical complement; unknown stays unknown.
    fn not(self) -> Logic {
        match self {
            Logic::Zero => Logic::One,
            Logic::One => Logic::Zero,
            Logic::Unknown => Logic::Unknown,
        }
    }
}
impl Logic {
    pub fn from_bool(b: bool) -> Logic {
        if b {
            Logic::One
        } else {
            Logic::Zero
        }
    }
    /// The value a plot shows: 0, 1 or ½ for unknown.
    pub fn level(self) -> f64 {
        match self {
            Logic::Zero => 0.0,
            Logic::One => 1.0,
            Logic::Unknown => 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateOp {
    And,
    Nand,
    Or,
    Nor,
    Xor,
    Xnor,
    Buffer,
    Inverter,
}
impl GateOp {
    pub fn parse(model_type: &str) -> Option<GateOp> {
        Some(match model_type {
            "d_and" => GateOp::And,
            "d_nand" => GateOp::Nand,
            "d_or" => GateOp::Or,
            "d_nor" => GateOp::Nor,
            "d_xor" => GateOp::Xor,
            "d_xnor" => GateOp::Xnor,
            "d_buffer" => GateOp::Buffer,
            "d_inverter" => GateOp::Inverter,
            _ => return None,
        })
    }
    pub fn eval(self, inputs: &[Logic]) -> Logic {
        use Logic::*;
        let all = |v: Logic| inputs.iter().all(|x| *x == v);
        let any = |v: Logic| inputs.contains(&v);
        let and = if any(Zero) {
            Zero
        } else if all(One) {
            One
        } else {
            Unknown
        };
        let or = if any(One) {
            One
        } else if all(Zero) {
            Zero
        } else {
            Unknown
        };
        let xor = if any(Unknown) {
            Unknown
        } else {
            Logic::from_bool(inputs.iter().filter(|x| **x == One).count() % 2 == 1)
        };
        let first = inputs.first().copied().unwrap_or(Unknown);
        match self {
            GateOp::And => and,
            GateOp::Nand => and.not(),
            GateOp::Or => or,
            GateOp::Nor => or.not(),
            GateOp::Xor => xor,
            GateOp::Xnor => xor.not(),
            GateOp::Buffer => first,
            GateOp::Inverter => first.not(),
        }
    }
}

/// A digital port: a net (or none, XSPICE's `NULL`) read or driven inverted when `~`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Port {
    pub net: Option<usize>,
    pub invert: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Inputs: every input. Outputs: one.
    Gate(GateOp),
    /// Inputs: data, clk, set, reset. Outputs: out, nout.
    Dff {
        clk_delay: f64,
        set_delay: f64,
        reset_delay: f64,
        ic: Logic,
    },
    /// Inputs: s, r, enable, set, reset. Outputs: out, nout.
    SrLatch {
        sr_delay: f64,
        enable_delay: f64,
        set_delay: f64,
        reset_delay: f64,
        ic: Logic,
    },
    /// Output: out. The frequency follows the analog control voltage through a
    /// piecewise-linear table.
    Osc {
        control: usize,
        cntl: Vec<f64>,
        freq: Vec<f64>,
        duty: f64,
        phase: f64,
    },
    /// A constant driver (`d_pullup`, `d_pulldown`).
    Const(Logic),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub name: String,
    pub kind: Kind,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub rise: f64,
    pub fall: f64,
}

/// `adc_bridge`: an analog node read as logic. Below `in_low` is 0, above `in_high` 1,
/// between them unknown; equal thresholds make a comparator.
#[derive(Clone, Debug, PartialEq)]
pub struct Adc {
    pub name: String,
    pub node: usize,
    pub net: usize,
    pub in_low: f64,
    pub in_high: f64,
    pub rise: f64,
    pub fall: f64,
}
impl Adc {
    pub fn read(&self, v: f64) -> Logic {
        if self.in_low >= self.in_high {
            Logic::from_bool(v >= self.in_high)
        } else if v < self.in_low {
            Logic::Zero
        } else if v > self.in_high {
            Logic::One
        } else {
            Logic::Unknown
        }
    }
    /// The thresholds a waveform crosses to change what this bridge reads.
    pub fn thresholds(&self) -> [f64; 2] {
        [self.in_low, self.in_high]
    }
}

/// `dac_bridge` levels and edge times; the analog side is a voltage source.
#[derive(Clone, Debug, PartialEq)]
pub struct DacLevels {
    pub out_low: f64,
    pub out_high: f64,
    pub out_undef: f64,
    pub t_rise: f64,
    pub t_fall: f64,
}
impl DacLevels {
    pub fn level(&self, l: Logic) -> f64 {
        match l {
            Logic::Zero => self.out_low,
            Logic::One => self.out_high,
            Logic::Unknown => self.out_undef,
        }
    }
}

/// The digital half of a circuit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Digital {
    pub nets: Vec<String>,
    pub elements: Vec<Element>,
    pub adcs: Vec<Adc>,
}
impl Digital {
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty() && self.adcs.is_empty()
    }
    pub fn net(&mut self, name: &str) -> usize {
        match self.nets.iter().position(|n| n.eq_ignore_ascii_case(name)) {
            Some(i) => i,
            None => {
                self.nets.push(name.to_owned());
                self.nets.len() - 1
            }
        }
    }
}

/// A DAC output ramping from `v0` at `t0` to `v1` over `dur`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ramp {
    pub t0: f64,
    pub v0: f64,
    pub v1: f64,
    pub dur: f64,
}
impl Ramp {
    pub fn at(&self, t: f64) -> f64 {
        if t <= self.t0 {
            self.v0
        } else if t >= self.t0 + self.dur {
            self.v1
        } else {
            self.v0 + (self.v1 - self.v0) * (t - self.t0) / self.dur
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ElemState {
    None,
    /// Stored bit of a flip-flop or latch.
    Bit(Logic),
    /// An oscillator's output level.
    Osc(Logic),
}

/// Queue key: time (non-negative, so its bits order like the number) and a sequence
/// number that keeps same-time events in scheduling order.
type Key = (u64, u64);

/// A running digital simulation.
#[derive(Clone, Debug)]
pub struct Sim {
    pub values: Vec<Logic>,
    queue: BTreeMap<Key, (usize, Logic)>,
    seq: u64,
    fanout: Vec<Vec<usize>>,
    state: Vec<ElemState>,
    pub adc_values: Vec<Logic>,
    /// Every change of every net: (time, net, value), for plotting.
    pub history: Vec<(f64, usize, Logic)>,
}

/// Smallest delay an element may have, so a zero-delay loop still advances time.
pub const MIN_DELAY: f64 = 1e-12;

impl Sim {
    pub fn new(d: &Digital) -> Sim {
        let mut fanout = vec![vec![]; d.nets.len()];
        for (i, e) in d.elements.iter().enumerate() {
            for p in &e.inputs {
                if let Some(n) = p.net {
                    if !fanout[n].contains(&i) {
                        fanout[n].push(i);
                    }
                }
            }
        }
        let state = d
            .elements
            .iter()
            .map(|e| match &e.kind {
                Kind::Dff { ic, .. } | Kind::SrLatch { ic, .. } => ElemState::Bit(*ic),
                Kind::Osc { phase, duty, .. } => {
                    // Phase 0 starts at the rising edge: high for the duty fraction.
                    ElemState::Osc(Logic::from_bool(phase.rem_euclid(1.0) < *duty))
                }
                _ => ElemState::None,
            })
            .collect();
        Sim {
            values: vec![Logic::Unknown; d.nets.len()],
            queue: BTreeMap::new(),
            seq: 0,
            fanout,
            state,
            adc_values: vec![Logic::Unknown; d.adcs.len()],
            history: vec![],
        }
    }

    fn read(&self, p: &Port) -> Logic {
        match p.net {
            Some(n) => {
                if p.invert {
                    self.values[n].not()
                } else {
                    self.values[n]
                }
            }
            None => Logic::Zero,
        }
    }

    /// The value net `net` will settle to once its pending events have happened.
    fn final_value(&self, net: usize) -> Logic {
        self.queue
            .values()
            .rev()
            .find(|(n, _)| *n == net)
            .map(|(_, v)| *v)
            .unwrap_or(self.values[net])
    }

    fn schedule(&mut self, net: usize, value: Logic, at: f64) {
        // Inertial: a newer decision replaces pending changes that have not happened.
        self.queue
            .retain(|k, (n, _)| !(*n == net && f64::from_bits(k.0) >= at));
        if self.final_value(net) == value {
            return;
        }
        self.seq += 1;
        self.queue
            .insert((at.max(0.0).to_bits(), self.seq), (net, value));
    }

    fn drive(&mut self, port: &Port, value: Logic, at: f64) {
        if let Some(n) = port.net {
            let v = if port.invert { value.not() } else { value };
            self.schedule(n, v, at);
        }
    }

    /// Evaluate element `i` at time `t` after an input changed, scheduling its outputs.
    /// `rising` lists the input ports that went 0→1 at this instant.
    fn evaluate(&mut self, d: &Digital, i: usize, t: f64, changed: &[(usize, Logic)], x: &[f64]) {
        let e = &d.elements[i];
        let delay = |v: Logic| {
            match v {
                Logic::One => e.rise,
                Logic::Zero => e.fall,
                Logic::Unknown => e.rise.min(e.fall),
            }
            .max(MIN_DELAY)
        };
        match &e.kind {
            Kind::Gate(op) => {
                let ins: Vec<Logic> = e.inputs.iter().map(|p| self.read(p)).collect();
                let out = op.eval(&ins);
                if let Some(p) = e.outputs.first().copied() {
                    self.drive(&p, out, t + delay(out));
                }
            }
            Kind::Dff {
                clk_delay,
                set_delay,
                reset_delay,
                ..
            } => {
                let port = |k: usize| e.inputs.get(k).copied();
                let set = port(2).map_or(Logic::Zero, |p| self.read(&p));
                let reset = port(3).map_or(Logic::Zero, |p| self.read(&p));
                let ElemState::Bit(q) = self.state[i] else {
                    return;
                };
                let clk_rose = port(1).is_some_and(|p| {
                    p.net.is_some_and(|n| {
                        changed.iter().any(|(cn, old)| {
                            *cn == n && {
                                let (was, now) = if p.invert {
                                    (old.not(), self.values[n].not())
                                } else {
                                    (*old, self.values[n])
                                };
                                was == Logic::Zero && now == Logic::One
                            }
                        })
                    })
                });
                let (next, dly) = if set == Logic::One && reset == Logic::One {
                    (Logic::Unknown, set_delay.min(*reset_delay))
                } else if set == Logic::One {
                    (Logic::One, *set_delay)
                } else if reset == Logic::One {
                    (Logic::Zero, *reset_delay)
                } else if clk_rose {
                    (
                        port(0).map_or(Logic::Unknown, |p| self.read(&p)),
                        *clk_delay,
                    )
                } else {
                    return;
                };
                self.state[i] = ElemState::Bit(next);
                if next != q || self.outputs_differ(e, next) {
                    self.drive_pair(e, next, t + dly.max(MIN_DELAY));
                }
            }
            Kind::SrLatch {
                sr_delay,
                enable_delay,
                set_delay,
                reset_delay,
                ..
            } => {
                let port = |k: usize| e.inputs.get(k).copied();
                let get = |k: usize, default: Logic| port(k).map_or(default, |p| self.read(&p));
                let (s, r) = (get(0, Logic::Zero), get(1, Logic::Zero));
                let enable = match port(2) {
                    Some(p) if p.net.is_some() => self.read(&p),
                    _ => Logic::One,
                };
                let (set, reset) = (get(3, Logic::Zero), get(4, Logic::Zero));
                let ElemState::Bit(q) = self.state[i] else {
                    return;
                };
                let (next, dly) = if set == Logic::One && reset == Logic::One {
                    (Logic::Unknown, set_delay.min(*reset_delay))
                } else if set == Logic::One {
                    (Logic::One, *set_delay)
                } else if reset == Logic::One {
                    (Logic::Zero, *reset_delay)
                } else if enable == Logic::One {
                    let enable_changed = port(2)
                        .and_then(|p| p.net)
                        .is_some_and(|n| changed.iter().any(|(c, _)| *c == n));
                    let dly = if enable_changed {
                        *enable_delay
                    } else {
                        *sr_delay
                    };
                    match (s, r) {
                        (Logic::One, Logic::Zero) => (Logic::One, dly),
                        (Logic::Zero, Logic::One) => (Logic::Zero, dly),
                        (Logic::Zero, Logic::Zero) => (q, dly),
                        _ => (Logic::Unknown, dly),
                    }
                } else {
                    (q, *sr_delay)
                };
                self.state[i] = ElemState::Bit(next);
                if next != q || self.outputs_differ(e, next) {
                    self.drive_pair(e, next, t + dly.max(MIN_DELAY));
                }
            }
            Kind::Osc { .. } => {
                let _ = x;
            }
            Kind::Const(v) => {
                if let Some(p) = e.outputs.first().copied() {
                    self.drive(&p, *v, t + MIN_DELAY);
                }
            }
        }
    }

    fn outputs_differ(&self, e: &Element, q: Logic) -> bool {
        let want = |k: usize| if k == 0 { q } else { q.not() };
        e.outputs.iter().enumerate().any(|(k, p)| {
            p.net.is_some_and(|n| {
                let v = if p.invert {
                    self.values[n].not()
                } else {
                    self.values[n]
                };
                let fin = self.final_value(n);
                let fin = if p.invert { fin.not() } else { fin };
                let _ = v;
                fin != want(k)
            })
        })
    }

    fn drive_pair(&mut self, e: &Element, q: Logic, at: f64) {
        let outs = e.outputs.clone();
        if let Some(p) = outs.first() {
            self.drive(p, q, at);
        }
        if let Some(p) = outs.get(1) {
            self.drive(p, q.not(), at);
        }
    }

    /// The oscillator's period at its control voltage.
    fn osc_period(kind: &Kind, x: &[f64]) -> Option<(f64, f64)> {
        let Kind::Osc {
            control,
            cntl,
            freq,
            duty,
            ..
        } = kind
        else {
            return None;
        };
        let v = x.get(*control).copied().unwrap_or(0.0);
        let f = pwl(cntl, freq, v);
        if f <= 0.0 || !f.is_finite() {
            return None;
        }
        Some((1.0 / f, duty.clamp(0.01, 0.99)))
    }

    /// Start at t = 0: constant drivers drive, oscillators output their initial level
    /// and schedule their first edge, flip-flops show their initial state.
    pub fn start(&mut self, d: &Digital, x: &[f64]) {
        for (i, e) in d.elements.iter().enumerate() {
            match (&e.kind, self.state[i]) {
                (Kind::Const(v), _) => {
                    if let Some(p) = e.outputs.first().copied() {
                        self.force(&p, *v);
                    }
                }
                (Kind::Osc { phase, .. }, ElemState::Osc(level)) => {
                    if let Some(p) = e.outputs.first().copied() {
                        self.force(&p, level);
                    }
                    if let Some((period, duty)) = Self::osc_period(&e.kind, x) {
                        let ph = phase.rem_euclid(1.0);
                        let next = if ph < duty {
                            (duty - ph) * period
                        } else {
                            (1.0 - ph) * period
                        };
                        self.seq += 1;
                        self.queue
                            .insert((next.to_bits(), self.seq), (usize::MAX - i, Logic::Unknown));
                    }
                }
                (Kind::Dff { .. } | Kind::SrLatch { .. }, ElemState::Bit(q)) => {
                    let outs = e.outputs.clone();
                    if let Some(p) = outs.first() {
                        self.force(p, q);
                    }
                    if let Some(p) = outs.get(1) {
                        self.force(p, q.not());
                    }
                }
                _ => {}
            }
        }
    }

    fn force(&mut self, p: &Port, v: Logic) {
        if let Some(n) = p.net {
            self.values[n] = if p.invert { v.not() } else { v };
        }
    }

    /// Settle the logic at the operating point: ADCs read `x` at once and every element
    /// is evaluated with its delays ignored until nothing changes. Returns whether any
    /// net changed.
    pub fn settle(&mut self, d: &Digital, x: &[f64]) -> bool {
        let mut any = false;
        for (k, a) in d.adcs.iter().enumerate() {
            let v = a.read(x.get(a.node).copied().unwrap_or(0.0));
            self.adc_values[k] = v;
            if self.values[a.net] != v {
                self.values[a.net] = v;
                any = true;
            }
        }
        for _ in 0..(4 * d.elements.len() + 8) {
            let mut changed = false;
            for (i, e) in d.elements.iter().enumerate() {
                let outs: Vec<(Port, Logic)> = match &e.kind {
                    Kind::Gate(op) => {
                        let ins: Vec<Logic> = e.inputs.iter().map(|p| self.read(p)).collect();
                        e.outputs
                            .first()
                            .map(|p| (*p, op.eval(&ins)))
                            .into_iter()
                            .collect()
                    }
                    Kind::Dff { .. } | Kind::SrLatch { .. } => {
                        let ElemState::Bit(mut q) = self.state[i] else {
                            continue;
                        };
                        let (set_k, reset_k) = if matches!(e.kind, Kind::Dff { .. }) {
                            (2, 3)
                        } else {
                            (3, 4)
                        };
                        let get = |k: usize| e.inputs.get(k).map_or(Logic::Zero, |p| self.read(p));
                        let (set, reset) = (get(set_k), get(reset_k));
                        if set == Logic::One && reset == Logic::One {
                            q = Logic::Unknown;
                        } else if set == Logic::One {
                            q = Logic::One;
                        } else if reset == Logic::One {
                            q = Logic::Zero;
                        } else if let Kind::SrLatch { .. } = e.kind {
                            let enable = match e.inputs.get(2) {
                                Some(p) if p.net.is_some() => self.read(p),
                                _ => Logic::One,
                            };
                            if enable == Logic::One {
                                match (get(0), get(1)) {
                                    (Logic::One, Logic::Zero) => q = Logic::One,
                                    (Logic::Zero, Logic::One) => q = Logic::Zero,
                                    (Logic::Zero, Logic::Zero) => {}
                                    _ => q = Logic::Unknown,
                                }
                            }
                        }
                        self.state[i] = ElemState::Bit(q);
                        let mut v = vec![];
                        if let Some(p) = e.outputs.first() {
                            v.push((*p, q));
                        }
                        if let Some(p) = e.outputs.get(1) {
                            v.push((*p, q.not()));
                        }
                        v
                    }
                    _ => vec![],
                };
                for (p, v) in outs {
                    if let Some(n) = p.net {
                        let v = if p.invert { v.not() } else { v };
                        if self.values[n] != v {
                            self.values[n] = v;
                            changed = true;
                        }
                    }
                }
            }
            any |= changed;
            if !changed {
                break;
            }
        }
        any
    }

    /// Sample every ADC at an accepted analog point, scheduling the changes.
    pub fn sample(&mut self, d: &Digital, t: f64, x: &[f64]) {
        for (k, a) in d.adcs.iter().enumerate() {
            let v = a.read(x.get(a.node).copied().unwrap_or(0.0));
            if v != self.adc_values[k] {
                self.adc_values[k] = v;
                let delay = match v {
                    Logic::One => a.rise,
                    Logic::Zero => a.fall,
                    Logic::Unknown => a.rise.min(a.fall),
                }
                .max(MIN_DELAY);
                self.schedule(a.net, v, t + delay);
            }
        }
    }

    /// Time of the next event, if any.
    pub fn next_event(&self) -> Option<f64> {
        self.queue.keys().next().map(|(t, _)| f64::from_bits(*t))
    }

    /// Run every event due by `t`. Returns the nets that changed, with their old value
    /// and the time they changed.
    pub fn run_until(&mut self, d: &Digital, t: f64, x: &[f64]) -> Vec<(usize, Logic, f64)> {
        let mut out = vec![];
        let mut guard = 0usize;
        while let Some((&key, _)) = self.queue.iter().next() {
            let te = f64::from_bits(key.0);
            if te > t {
                break;
            }
            guard += 1;
            if guard > 1_000_000 {
                break;
            }
            // Everything scheduled for this same instant happens together.
            let batch: Vec<(Key, (usize, Logic))> = self
                .queue
                .range(key..)
                .take_while(|(k, _)| k.0 == key.0)
                .map(|(k, v)| (*k, *v))
                .collect();
            for (k, _) in &batch {
                self.queue.remove(k);
            }
            let mut changed: Vec<(usize, Logic)> = vec![];
            let mut osc_fired: Vec<usize> = vec![];
            for (_, (net, value)) in batch {
                if net > usize::MAX / 2 {
                    osc_fired.push(usize::MAX - net);
                    continue;
                }
                if self.values[net] != value {
                    changed.push((net, self.values[net]));
                    self.values[net] = value;
                    self.history.push((te, net, value));
                    out.push((net, changed.last().unwrap().1, te));
                }
            }
            for i in osc_fired {
                let e = &d.elements[i];
                let ElemState::Osc(level) = self.state[i] else {
                    continue;
                };
                let next = level.not();
                self.state[i] = ElemState::Osc(next);
                if let Some(p) = e.outputs.first().copied() {
                    let v = if p.invert { next.not() } else { next };
                    if let Some(n) = p.net {
                        if self.values[n] != v {
                            changed.push((n, self.values[n]));
                            self.values[n] = v;
                            self.history.push((te, n, v));
                            out.push((n, changed.last().unwrap().1, te));
                        }
                    }
                }
                if let Some((period, duty)) = Self::osc_period(&e.kind, x) {
                    let span = if next == Logic::One {
                        duty * period
                    } else {
                        (1.0 - duty) * period
                    };
                    self.seq += 1;
                    self.queue.insert(
                        ((te + span).to_bits(), self.seq),
                        (usize::MAX - i, Logic::Unknown),
                    );
                }
            }
            let mut touched: Vec<usize> = vec![];
            for (net, _) in &changed {
                for &i in &self.fanout[*net] {
                    if !touched.contains(&i) {
                        touched.push(i);
                    }
                }
            }
            touched.sort_unstable();
            for i in touched {
                self.evaluate(d, i, te, &changed, x);
            }
        }
        out
    }
}

/// Piecewise-linear lookup with constant extension at both ends.
pub fn pwl(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len().min(ys.len());
    if n == 0 {
        return 0.0;
    }
    if x <= xs[0] {
        return ys[0];
    }
    for k in 1..n {
        if x <= xs[k] {
            let (x0, x1, y0, y1) = (xs[k - 1], xs[k], ys[k - 1], ys[k]);
            return if x1 == x0 {
                y1
            } else {
                y0 + (y1 - y0) * (x - x0) / (x1 - x0)
            };
        }
    }
    ys[n - 1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use Logic::*;
    #[test]
    fn gate_truth_tables_with_unknowns() {
        let cases = [
            (GateOp::And, [Zero, One], Zero),
            (GateOp::And, [One, One], One),
            (GateOp::And, [Unknown, One], Unknown),
            (GateOp::And, [Unknown, Zero], Zero),
            (GateOp::Nand, [One, One], Zero),
            (GateOp::Or, [Unknown, One], One),
            (GateOp::Nor, [Zero, Zero], One),
            (GateOp::Xor, [One, Zero], One),
            (GateOp::Xor, [One, One], Zero),
            (GateOp::Xnor, [One, One], One),
        ];
        for (op, ins, out) in cases {
            assert_eq!(op.eval(&ins), out, "{op:?} {ins:?}");
        }
        assert_eq!(GateOp::Inverter.eval(&[One]), Zero);
    }
    #[test]
    fn a_dff_takes_data_on_the_rising_clock_edge() {
        let mut d = Digital::default();
        let (dn, clk, q, nq) = (d.net("d"), d.net("clk"), d.net("q"), d.net("nq"));
        let port = |n: usize| Port {
            net: Some(n),
            invert: false,
        };
        d.elements.push(Element {
            name: "a1".into(),
            kind: Kind::Dff {
                clk_delay: 1e-9,
                set_delay: 1e-9,
                reset_delay: 1e-9,
                ic: Zero,
            },
            inputs: vec![
                port(dn),
                port(clk),
                Port {
                    net: None,
                    invert: false,
                },
                Port {
                    net: None,
                    invert: false,
                },
            ],
            outputs: vec![port(q), port(nq)],
            rise: 1e-9,
            fall: 1e-9,
        });
        let mut s = Sim::new(&d);
        s.values[dn] = Zero;
        s.values[clk] = Zero;
        s.start(&d, &[]);
        assert_eq!((s.values[q], s.values[nq]), (Zero, One));
        s.schedule(dn, One, 1e-6);
        s.run_until(&d, 2e-6, &[]);
        assert_eq!(s.values[q], Zero, "data alone does not load");
        s.schedule(clk, One, 3e-6);
        s.run_until(&d, 3e-6 + 0.5e-9, &[]);
        assert_eq!(s.values[q], Zero, "clock-to-q delay not yet over");
        s.run_until(&d, 3e-6 + 2e-9, &[]);
        assert_eq!((s.values[q], s.values[nq]), (One, Zero));
        s.schedule(dn, Zero, 4e-6);
        s.schedule(clk, Zero, 5e-6);
        s.run_until(&d, 6e-6, &[]);
        assert_eq!(s.values[q], One, "a falling edge does not load");
    }
}
