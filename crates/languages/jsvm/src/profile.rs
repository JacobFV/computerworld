//! A built-in, deterministic VM profiler.
//!
//! Off by default: `Vm::prof` is `None` and the run loop pays one predictable
//! branch per instruction for it, the same check the debugger hook shares. When
//! started (`Vm::profile_start`) it records, until `Vm::profile_stop`:
//!
//! * per JS function: calls, inclusive and exclusive instruction counts, and
//!   inclusive / exclusive wall time;
//! * per native function (built-ins and embedder bindings): calls and wall time,
//!   inclusive and exclusive of the JS it calls back into;
//! * an opcode histogram (counts, and wall time when `ProfileOptions::op_time`);
//! * counters for property reads (and how far up the prototype chain they had to
//!   walk), writes, calls, allocations, and inline-cache hits and misses;
//! * parse and compile time per source.
//!
//! Instruction counts are exact and deterministic; times come from the host's
//! monotonic clock (none on wasm32, where they read 0). Profiling observes and
//! never changes what the program sees: step counts, the virtual clock and every
//! value are the same with it on or off.

use crate::bytecode::{Code, Op};
use crate::value::FnvMap;
use std::cell::Cell;
use std::rc::Rc;

/// Nanoseconds on the host's monotonic clock since the first call.
#[cfg(not(target_arch = "wasm32"))]
pub fn now_ns() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

#[cfg(target_arch = "wasm32")]
pub fn now_ns() -> u64 {
    0
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProfileOptions {
    /// Time every instruction (attributing the gap between two instructions to
    /// the first). Costs a clock read per instruction, which inflates totals.
    pub op_time: bool,
    /// Count property reads by key name.
    pub prop_names: bool,
}

/// Counters the object model bumps (interior mutability so `&Vm` paths can).
#[derive(Default)]
pub struct Counters {
    pub get: Cell<u64>,
    /// Prototype links followed by reads (0 when found on the object itself).
    pub get_hops: Cell<u64>,
    /// Reads answered on the receiver itself by the ordinary fast path.
    pub get_own_fast: Cell<u64>,
    /// Reads that reached a getter.
    pub get_accessor: Cell<u64>,
    /// Reads of a primitive's property (through its prototype).
    pub get_primitive: Cell<u64>,
    pub set: Cell<u64>,
    pub set_own_fast: Cell<u64>,
    pub set_add: Cell<u64>,
    pub global_get: Cell<u64>,
    pub calls_js: Cell<u64>,
    pub calls_native: Cell<u64>,
    pub calls_bound: Cell<u64>,
    pub constructs: Cell<u64>,
    pub objects: Cell<u64>,
    pub closures: Cell<u64>,
    pub arrays: Cell<u64>,
    pub frames: Cell<u64>,
    pub ic_hit: Cell<u64>,
    pub ic_miss: Cell<u64>,
}

impl Counters {
    #[inline]
    pub fn bump(c: &Cell<u64>) {
        c.set(c.get() + 1);
    }
    #[inline]
    pub fn add(c: &Cell<u64>, n: u64) {
        c.set(c.get() + n);
    }
}

#[derive(Default, Clone)]
struct Stat {
    calls: u64,
    total_steps: u64,
    self_steps: u64,
    total_ns: u64,
    self_ns: u64,
    /// Activations on the shadow stack (recursion counts inclusive time once).
    active: u32,
}

enum Who {
    Js(usize),
    Native(usize),
}

struct Entry {
    who: Who,
    /// `frames.len()` when entered (JS: the frame's own depth).
    depth: usize,
    steps: u64,
    ns: u64,
    child_steps: u64,
    child_ns: u64,
}

pub struct Profiler {
    pub opts: ProfileOptions,
    pub counters: Counters,
    started_ns: u64,
    started_steps: u64,
    codes: FnvMap<usize, Rc<Code>>,
    js: FnvMap<usize, Stat>,
    natives: FnvMap<usize, (String, Stat)>,
    stack: Vec<Entry>,
    ops: Vec<(u64, u64)>,
    op_names: Vec<Option<String>>,
    last_op: Option<(usize, u64)>,
    prop_names: FnvMap<String, (u64, u64)>,
    sources: Vec<SourceStat>,
}

#[derive(Clone, Debug)]
pub struct SourceStat {
    pub file: String,
    pub bytes: usize,
    pub parse_ns: u64,
    pub compile_ns: u64,
    /// Answered by the compiled-code cache.
    pub cached: bool,
}

/// The discriminant of an instruction (its variant), for the histogram.
pub fn op_tag(op: &Op) -> usize {
    // SAFETY: `Op` is `#[repr(u8)]`, so its first byte is the discriminant.
    unsafe { *(op as *const Op as *const u8) as usize }
}

fn op_name(op: &Op) -> String {
    let s = format!("{op:?}");
    match s.find('(') {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

impl Profiler {
    pub fn new(opts: ProfileOptions, steps: u64) -> Profiler {
        Profiler {
            opts,
            counters: Counters::default(),
            started_ns: now_ns(),
            started_steps: steps,
            codes: FnvMap::default(),
            js: FnvMap::default(),
            natives: FnvMap::default(),
            stack: vec![],
            ops: vec![(0, 0); 256],
            op_names: vec![None; 256],
            last_op: None,
            prop_names: FnvMap::default(),
            sources: vec![],
        }
    }

    /// Called before each instruction with the frame depth, the running code,
    /// the instruction and the instruction count before it.
    #[inline]
    pub fn on_op(&mut self, depth: usize, code: &Rc<Code>, op: &Op, steps: u64) {
        let tag = op_tag(op);
        self.ops[tag].0 += 1;
        if self.op_names[tag].is_none() {
            self.op_names[tag] = Some(op_name(op));
        }
        if self.opts.op_time {
            let t = now_ns();
            if let Some((prev, t0)) = self.last_op {
                self.ops[prev].1 += t.saturating_sub(t0);
            }
            self.last_op = Some((tag, t));
        }
        let key = Rc::as_ptr(code) as usize;
        // Leave JS activations that have returned (deeper than this frame, or a
        // different function at this depth).
        while let Some(e) = self.stack.last() {
            match e.who {
                Who::Js(k) if e.depth > depth || (e.depth == depth && k != key) => {
                    self.leave(steps);
                }
                _ => break,
            }
        }
        let here = matches!(self.stack.last(), Some(Entry { who: Who::Js(k), depth: d, .. }) if *k == key && *d == depth);
        if !here {
            self.codes.entry(key).or_insert_with(|| code.clone());
            self.enter(Who::Js(key), depth, steps);
        }
    }

    /// The VM's frames are back to `depth`: JS activations above it are over.
    pub fn sync(&mut self, depth: usize, steps: u64) {
        while let Some(e) = self.stack.last() {
            match e.who {
                Who::Js(_) if e.depth > depth => self.leave(steps),
                _ => break,
            }
        }
    }

    fn enter(&mut self, who: Who, depth: usize, steps: u64) {
        let stat = match &who {
            Who::Js(k) => self.js.entry(*k).or_default(),
            Who::Native(k) => &mut self.natives.entry(*k).or_default().1,
        };
        stat.calls += 1;
        stat.active += 1;
        self.stack.push(Entry {
            who,
            depth,
            steps,
            ns: now_ns(),
            child_steps: 0,
            child_ns: 0,
        });
    }

    fn leave(&mut self, steps: u64) {
        let Some(e) = self.stack.pop() else { return };
        let ns = now_ns().saturating_sub(e.ns);
        let st = steps.saturating_sub(e.steps);
        let stat = match &e.who {
            Who::Js(k) => self.js.entry(*k).or_default(),
            Who::Native(k) => &mut self.natives.entry(*k).or_default().1,
        };
        stat.active -= 1;
        if stat.active == 0 {
            stat.total_ns += ns;
            stat.total_steps += st;
        }
        stat.self_ns += ns.saturating_sub(e.child_ns);
        stat.self_steps += st.saturating_sub(e.child_steps);
        if let Some(parent) = self.stack.last_mut() {
            parent.child_ns += ns;
            parent.child_steps += st;
        }
    }

    /// A native function is about to run (`key` identifies it, `name` labels it).
    pub fn native_enter(
        &mut self,
        key: usize,
        name: impl FnOnce() -> String,
        depth: usize,
        steps: u64,
    ) {
        // JS frames that returned before this call are over.
        while let Some(e) = self.stack.last() {
            match e.who {
                Who::Js(_) if e.depth > depth => self.leave(steps),
                _ => break,
            }
        }
        self.natives
            .entry(key)
            .or_insert_with(|| (name(), Stat::default()));
        self.enter(Who::Native(key), depth, steps);
    }

    /// The native function entered last returned (or threw).
    pub fn native_leave(&mut self, key: usize, steps: u64) {
        // Pop JS frames it ran, then itself.
        while let Some(e) = self.stack.last() {
            match e.who {
                Who::Native(k) if k == key => {
                    self.leave(steps);
                    break;
                }
                _ => self.leave(steps),
            }
        }
    }

    pub fn prop_name(&mut self, name: &str, hops: u64) {
        let e = self.prop_names.entry(name.to_string()).or_default();
        e.0 += 1;
        e.1 += hops;
    }

    pub fn source(&mut self, s: SourceStat) {
        self.sources.push(s);
    }

    /// Ends the profile and builds its report.
    pub fn finish(mut self, steps: u64) -> ProfileReport {
        while !self.stack.is_empty() {
            self.leave(steps);
        }
        if let Some((prev, t0)) = self.last_op {
            self.ops[prev].1 += now_ns().saturating_sub(t0);
        }
        let mut functions: Vec<FnReport> = self
            .js
            .iter()
            .map(|(k, s)| {
                let code = &self.codes[k];
                let pos = code
                    .pos
                    .iter()
                    .find(|p| p.line > 0)
                    .copied()
                    .unwrap_or_default();
                FnReport {
                    name: if code.name.is_empty() {
                        "(anonymous)".into()
                    } else {
                        code.name.to_string()
                    },
                    location: format!("{}:{}:{}", code.file, pos.line, pos.col),
                    native: false,
                    calls: s.calls,
                    total_steps: s.total_steps,
                    self_steps: s.self_steps,
                    total_ns: s.total_ns,
                    self_ns: s.self_ns,
                }
            })
            .collect();
        functions.extend(self.natives.values().map(|(name, s)| FnReport {
            name: name.clone(),
            location: "native".into(),
            native: true,
            calls: s.calls,
            total_steps: s.total_steps,
            self_steps: s.self_steps,
            total_ns: s.total_ns,
            self_ns: s.self_ns,
        }));
        // Deterministic order: by exclusive steps, then name and location.
        functions.sort_by(|a, b| {
            b.self_steps
                .cmp(&a.self_steps)
                .then(b.self_ns.cmp(&a.self_ns))
                .then(a.name.cmp(&b.name))
                .then(a.location.cmp(&b.location))
        });
        let mut ops: Vec<OpReport> = self
            .ops
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| *n > 0)
            .map(|(i, (n, ns))| OpReport {
                name: self.op_names[i].clone().unwrap_or_default(),
                count: *n,
                ns: *ns,
            })
            .collect();
        ops.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
        let mut props: Vec<(String, u64, u64)> = self
            .prop_names
            .into_iter()
            .map(|(k, (n, h))| (k, n, h))
            .collect();
        props.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let c = &self.counters;
        let counters = vec![
            ("property reads", c.get.get()),
            (
                "  answered on the receiver (fast path)",
                c.get_own_fast.get(),
            ),
            ("  prototype hops walked", c.get_hops.get()),
            ("  reached a getter", c.get_accessor.get()),
            ("  on a primitive", c.get_primitive.get()),
            ("global reads (slow path)", c.global_get.get()),
            ("property writes", c.set.get()),
            ("  own data property updated in place", c.set_own_fast.get()),
            ("  not yet an own property", c.set_add.get()),
            ("JS calls", c.calls_js.get()),
            ("native calls", c.calls_native.get()),
            ("bound calls", c.calls_bound.get()),
            ("constructs", c.constructs.get()),
            ("frames made", c.frames.get()),
            ("objects allocated", c.objects.get()),
            ("closures made", c.closures.get()),
            ("arrays allocated", c.arrays.get()),
            ("inline-cache hits", c.ic_hit.get()),
            ("inline-cache misses", c.ic_miss.get()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        ProfileReport {
            steps: steps.saturating_sub(self.started_steps),
            wall_ns: now_ns().saturating_sub(self.started_ns),
            functions,
            ops,
            counters,
            props,
            sources: self.sources,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FnReport {
    pub name: String,
    pub location: String,
    pub native: bool,
    pub calls: u64,
    pub total_steps: u64,
    pub self_steps: u64,
    pub total_ns: u64,
    pub self_ns: u64,
}

#[derive(Clone, Debug)]
pub struct OpReport {
    pub name: String,
    pub count: u64,
    pub ns: u64,
}

/// What a profile recorded. `to_text` renders the top entries of each table.
#[derive(Clone, Debug, Default)]
pub struct ProfileReport {
    /// Instructions executed while profiling.
    pub steps: u64,
    pub wall_ns: u64,
    pub functions: Vec<FnReport>,
    pub ops: Vec<OpReport>,
    pub counters: Vec<(String, u64)>,
    /// Property reads by key: (name, reads, prototype hops).
    pub props: Vec<(String, u64, u64)>,
    pub sources: Vec<SourceStat>,
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

impl ProfileReport {
    pub fn counter(&self, name: &str) -> u64 {
        self.counters
            .iter()
            .find(|(k, _)| k.trim() == name)
            .map(|(_, v)| *v)
            .unwrap_or(0)
    }

    pub fn to_text(&self, top: usize) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "{} instructions in {:.2} ms",
            self.steps,
            ms(self.wall_ns)
        );
        if !self.sources.is_empty() {
            let _ = writeln!(s, "\nsources (parse / compile ms):");
            for src in &self.sources {
                let _ = writeln!(
                    s,
                    "  {:>8.2} {:>8.2} {:>8} B{} {}",
                    ms(src.parse_ns),
                    ms(src.compile_ns),
                    src.bytes,
                    if src.cached { " cached" } else { "" },
                    src.file
                );
            }
        }
        let _ = writeln!(s, "\nfunctions by exclusive time:");
        let _ = writeln!(
            s,
            "  {:>8} {:>8} {:>10} {:>10} {:>8}  function",
            "self ms", "total ms", "self steps", "total st", "calls"
        );
        let mut by_time: Vec<&FnReport> = self.functions.iter().collect();
        by_time.sort_by(|a, b| b.self_ns.cmp(&a.self_ns).then(a.name.cmp(&b.name)));
        for f in by_time.iter().take(top) {
            let _ = writeln!(
                s,
                "  {:>8.3} {:>8.3} {:>10} {:>10} {:>8}  {} ({})",
                ms(f.self_ns),
                ms(f.total_ns),
                f.self_steps,
                f.total_steps,
                f.calls,
                f.name,
                f.location
            );
        }
        let _ = writeln!(s, "\nfunctions by exclusive instructions:");
        for f in self.functions.iter().filter(|f| !f.native).take(top) {
            let _ = writeln!(
                s,
                "  {:>10} {:>10} {:>8} {:>8.3}  {} ({})",
                f.self_steps,
                f.total_steps,
                f.calls,
                ms(f.self_ns),
                f.name,
                f.location
            );
        }
        let _ = writeln!(s, "\nnative functions by total time:");
        let mut natives: Vec<&FnReport> = self.functions.iter().filter(|f| f.native).collect();
        natives.sort_by(|a, b| b.total_ns.cmp(&a.total_ns).then(a.name.cmp(&b.name)));
        for f in natives.iter().take(top) {
            let _ = writeln!(
                s,
                "  {:>8.3} {:>8.3} {:>8}  {}",
                ms(f.self_ns),
                ms(f.total_ns),
                f.calls,
                f.name
            );
        }
        let _ = writeln!(s, "\nopcodes:");
        for o in self.ops.iter().take(top.max(40)) {
            let pct = o.count as f64 * 100.0 / self.steps.max(1) as f64;
            let _ = writeln!(
                s,
                "  {:>10} {:>5.1}% {:>8.3} ms  {}",
                o.count,
                pct,
                ms(o.ns),
                o.name
            );
        }
        let _ = writeln!(s, "\ncounters:");
        for (k, v) in &self.counters {
            let _ = writeln!(s, "  {v:>10}  {k}");
        }
        if !self.props.is_empty() {
            let _ = writeln!(s, "\nproperty reads by key (reads, prototype hops):");
            for (k, n, h) in self.props.iter().take(top) {
                let _ = writeln!(s, "  {n:>10} {h:>10}  {k}");
            }
        }
        s
    }
}
