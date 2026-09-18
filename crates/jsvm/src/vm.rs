//! Interpreter state: frames, intrinsics, the job queues, errors and stack
//! traces. Execution lives in `interp.rs`; the object model in `props.rs`.

use crate::bytecode::Code;
use crate::value::*;
use cw_script_host::ScriptHost;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// Deepest JS call stack before `RangeError: Maximum call stack size exceeded`.
pub const MAX_FRAMES: usize = 9_000;
/// Deepest nesting of native -> JS re-entry (bounded by the Rust stack).
pub const MAX_NATIVE_DEPTH: usize = 200;
/// Instructions a program may execute before it is stopped.
pub const STEP_BUDGET: u64 = 200_000_000;
pub const NODE_VERSION: &str = "v24.21.0";

#[derive(Clone)]
pub enum Local {
    V(Value),
    C(CellRef),
}

#[derive(Clone, Copy)]
pub struct Handler {
    pub pc: u32,
    pub depth: u32,
    pub finally: bool,
}

pub enum Resume {
    Throw(Value),
    Return(Value),
}

pub enum FrameKind {
    Normal,
    /// `new F()`: the object created for a base constructor.
    Construct(Value),
    Generator(Obj),
    Async {
        promise: Obj,
        co: Obj,
        first: bool,
    },
}

pub struct Frame {
    pub code: Rc<Code>,
    pub pc: usize,
    pub stack: Vec<Value>,
    pub locals: Vec<Local>,
    pub captures: Rc<[CellRef]>,
    pub handlers: Vec<Handler>,
    pub args: Vec<Value>,
    pub func: Option<Obj>,
    /// Receiver as called (stack-trace naming).
    pub recv: Value,
    pub kind: FrameKind,
    pub resume: Option<Resume>,
    /// yield* resumption mode: 0 next, 1 throw, 2 return.
    pub ystar_mode: u8,
    /// Resumed after an await.
    pub resumed: bool,
    /// Timer callback frame: `TIMEOUT_FRAME` (`Timeout._onTimeout`) or
    /// `IMMEDIATE_FRAME` (`Immediate._onImmediate`).
    pub timer: u8,
}

/// Instructions per virtual millisecond.
pub const STEPS_PER_MS: f64 = 100_000.0;

pub const TIMEOUT_FRAME: u8 = 1;
pub const IMMEDIATE_FRAME: u8 = 2;

pub struct NativeMark {
    pub depth: usize,
    pub callee: Obj,
    pub this: Value,
    pub construct: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exit {
    Return,
    Yield,
    Await,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrKind {
    Error,
    TypeError,
    RangeError,
    ReferenceError,
    SyntaxError,
    EvalError,
    URIError,
    AggregateError,
}

impl ErrKind {
    pub fn name(self) -> &'static str {
        match self {
            ErrKind::Error => "Error",
            ErrKind::TypeError => "TypeError",
            ErrKind::RangeError => "RangeError",
            ErrKind::ReferenceError => "ReferenceError",
            ErrKind::SyntaxError => "SyntaxError",
            ErrKind::EvalError => "EvalError",
            ErrKind::URIError => "URIError",
            ErrKind::AggregateError => "AggregateError",
        }
    }
    pub const ALL: [ErrKind; 8] = [
        ErrKind::Error,
        ErrKind::TypeError,
        ErrKind::RangeError,
        ErrKind::ReferenceError,
        ErrKind::SyntaxError,
        ErrKind::EvalError,
        ErrKind::URIError,
        ErrKind::AggregateError,
    ];
}

/// The internal loop that calls `runNextTicks` between two callbacks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Batch {
    /// Between two immediates (`processImmediate`).
    Immediates,
    /// Between two timers of the same duration list (`listOnTimeout`).
    List,
    /// Between two timer lists (`processTimers`).
    Lists,
}

/// How queued ticks and promise jobs are being drained: after a callback
/// batch (`between: None`) or between two callbacks of one batch; `tick` is
/// set when Node would run them from `processTicksAndRejections` (a tick
/// was queued, which every stream write does).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Drain {
    pub between: Option<Batch>,
    pub tick: bool,
}

/// What sits below user code on the stack (the internal frames Node prints).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tail {
    Main,
    Esm,
    Timer,
    Immediate,
    /// A `process.nextTick` callback.
    Tick(Option<Batch>),
    /// A promise job; `true` when run from `processTicksAndRejections`.
    Microtask(Option<Batch>, bool),
    Eval,
    /// `node --check`.
    Check,
    None,
}

pub struct Timer {
    pub id: u64,
    pub when: f64,
    pub seq: u64,
    pub callback: Value,
    pub args: Vec<Value>,
    pub interval: Option<f64>,
    pub obj: Obj,
    pub immediate: bool,
    /// A simulated I/O completion (poll phase), not a user timer.
    pub io: bool,
    /// The requested delay: timers of one duration share a Node list.
    pub dur: f64,
}

pub enum Job {
    Reaction {
        reaction: Reaction,
        arg: Value,
        rejected: bool,
    },
    Thenable {
        promise: Obj,
        thenable: Value,
        then: Value,
    },
    Callback(Value, Vec<Value>),
}

pub struct Intrinsics {
    pub object_proto: Obj,
    pub function_proto: Obj,
    pub array_proto: Obj,
    pub string_proto: Obj,
    pub number_proto: Obj,
    pub boolean_proto: Obj,
    pub symbol_proto: Obj,
    pub bigint_proto: Obj,
    pub error_protos: Vec<Obj>,
    pub error_ctors: Vec<Obj>,
    pub iterator_proto: Obj,
    pub async_iterator_proto: Obj,
    pub array_iter_proto: Obj,
    pub map_iter_proto: Obj,
    pub set_iter_proto: Obj,
    pub string_iter_proto: Obj,
    pub regexp_str_iter_proto: Obj,
    pub generator_proto: Obj,
    pub async_generator_proto: Obj,
    pub generator_function_proto: Obj,
    pub async_generator_function_proto: Obj,
    pub async_function_proto: Obj,
    pub promise_proto: Obj,
    pub promise_ctor: Obj,
    pub regexp_proto: Obj,
    pub date_proto: Obj,
    pub map_proto: Obj,
    pub set_proto: Obj,
    pub weakmap_proto: Obj,
    pub weakset_proto: Obj,
    pub weakref_proto: Obj,
    pub arraybuffer_proto: Obj,
    pub typed_protos: Vec<Obj>,
    pub array_iter_next: Obj,
    pub array_values: Obj,
    pub object_ctor: Obj,
    pub array_ctor: Obj,
    pub function_ctor: Obj,
    pub buffer_proto: Obj,
}

pub struct Syms {
    pub iterator: Rc<Symbol>,
    pub async_iterator: Rc<Symbol>,
    pub has_instance: Rc<Symbol>,
    pub to_primitive: Rc<Symbol>,
    pub to_string_tag: Rc<Symbol>,
    pub species: Rc<Symbol>,
    pub is_concat_spreadable: Rc<Symbol>,
    pub unscopables: Rc<Symbol>,
    pub match_: Rc<Symbol>,
    pub match_all: Rc<Symbol>,
    pub replace: Rc<Symbol>,
    pub search: Rc<Symbol>,
    pub split: Rc<Symbol>,
    pub inspect_custom: Rc<Symbol>,
}

pub fn new_symbol(desc: Option<&str>) -> Rc<Symbol> {
    Rc::new(Symbol {
        desc: desc.map(JsStr::new),
        private: false,
        registered: false,
    })
}

pub struct Vm<'h> {
    pub host: &'h mut dyn ScriptHost,
    pub frames: Vec<Frame>,
    pub natives: Vec<NativeMark>,
    pub intr: Intrinsics,
    pub syms: Syms,
    pub global: Obj,
    pub stdout: String,
    pub stderr: String,
    pub stdin: Option<String>,
    pub stdin_consumed: bool,
    pub steps: u64,
    pub budget: u64,
    pub native_depth: usize,
    pub throw_site: Option<Site>,
    pub exit: Exit,
    pub microtasks: VecDeque<Job>,
    pub ticks: VecDeque<(Value, Vec<Value>)>,
    pub timers: Vec<Timer>,
    pub timer_seq: u64,
    pub timer_id: u64,
    /// Virtual milliseconds elapsed (timers advance it).
    pub elapsed_ms: f64,
    /// `module.parent` for the next module object created.
    pub loading_parent: Option<Value>,
    /// Instruction count already folded into `elapsed_ms`.
    pub clock_steps: u64,
    pub start_micros: i64,
    pub pending_rejections: Vec<Obj>,
    pub modules: Vec<(String, Value)>,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub exit_code: i32,
    pub sources: Vec<(Rc<str>, Rc<str>)>,
    pub symbol_registry: Vec<(String, Rc<Symbol>)>,
    pub tail: Tail,
    pub main_file: String,
    pub inspect_seen: Vec<usize>,
    pub process: Option<Obj>,
    pub console_indent: usize,
    pub console_counts: Vec<(String, u64)>,
    pub console_timers: Vec<(String, f64)>,
    pub exit_handlers: Vec<Value>,
    pub listeners: Vec<(String, Value, bool)>,
    pub stdin_listeners: Vec<(String, Value)>,
    pub stdin_flowing: bool,
    pub readline_ifaces: Vec<Obj>,
    pub stack_limit: usize,
    pub rng_state: u64,
    pub is_esm_main: bool,
    pub import_meta: Option<Obj>,
    /// Function object of each entry of the last `stack_frames` result.
    pub trace_funcs: Vec<Option<Obj>>,
    pub exit_code_set: bool,
    pub esm_promises: Vec<Obj>,
    /// The next frame is a timer callback (see `Frame::timer`).
    pub timer_frame: u8,
    /// Current drain context for ticks and promise jobs.
    pub drain: Drain,
    /// Output length when the current callback started (writes queue ticks).
    pub out_mark: usize,
    pub open_fds: Vec<Option<(String, usize)>>,
    /// Completion value of `-e` / `-p` / eval programs.
    pub completion: Value,
}

impl<'h> Vm<'h> {
    // ------------------------------------------------------------ objects
    pub fn obj_with(&self, proto: Option<Obj>, kind: Kind) -> Obj {
        Obj::new(ObjData::new(proto, kind))
    }
    pub fn new_object(&self) -> Obj {
        self.obj_with(Some(self.intr.object_proto.clone()), Kind::Ordinary)
    }
    pub fn new_array(&self, v: Vec<Value>) -> Obj {
        self.obj_with(Some(self.intr.array_proto.clone()), Kind::Array(v))
    }
    pub fn arr(&self, v: Vec<Value>) -> Value {
        Value::Obj(self.new_array(v))
    }
    pub fn native_fn(&self, name: &str, len: u32, f: NativeFn) -> Obj {
        self.native_fn_slots(name, len, f, vec![])
    }
    pub fn native_fn_slots(&self, name: &str, len: u32, f: NativeFn, slots: Vec<Value>) -> Obj {
        let o = self.obj_with(
            Some(self.intr.function_proto.clone()),
            Kind::Function(Box::new(FuncData {
                imp: FuncImpl::Native { f, slots },
                ctor: CtorKind::None,
                class_ctor: false,
                home: None,
                fields: None,
            })),
        );
        {
            let mut d = o.borrow_mut();
            d.props.insert(
                Key::str("length"),
                Prop::data(Value::Num(len as f64), CONFIGURABLE),
            );
            d.props
                .insert(Key::str("name"), Prop::data(Value::str(name), CONFIGURABLE));
        }
        o
    }
    /// Defines a built-in method (non-enumerable).
    pub fn method(&self, target: &Obj, name: &str, len: u32, f: NativeFn) -> Obj {
        let func = self.native_fn(name, len, f);
        target.set_hidden(name, Value::Obj(func.clone()));
        func
    }
    pub fn method_sym(
        &self,
        target: &Obj,
        sym: &Rc<Symbol>,
        name: &str,
        len: u32,
        f: NativeFn,
    ) -> Obj {
        let func = self.native_fn(name, len, f);
        target.set_sym(sym, Value::Obj(func.clone()), HIDDEN);
        func
    }
    pub fn getter(&self, target: &Obj, name: &str, f: NativeFn) {
        let func = self.native_fn(&format!("get {name}"), 0, f);
        target.borrow_mut().props.insert(
            Key::str(name),
            Prop {
                slot: Slot::Accessor(Some(func), None),
                flags: CONFIGURABLE,
            },
        );
    }

    pub fn source_for(&self, file: &str) -> Option<Rc<str>> {
        self.sources
            .iter()
            .find(|(f, _)| &**f == file)
            .map(|(_, s)| s.clone())
    }

    // ------------------------------------------------------------ errors
    pub fn make_error(&mut self, kind: ErrKind, msg: &str) -> Obj {
        let proto = self.intr.error_protos[kind as usize].clone();
        let e = self.obj_with(
            Some(proto),
            Kind::Error(Box::new(ErrorData {
                frames: vec![],
                site: None,
                arrow: None,
                from_async: false,
            })),
        );
        if !msg.is_empty() {
            e.set_hidden("message", Value::str(msg));
        }
        self.capture_stack(&e, None);
        e
    }

    pub fn error(&mut self, kind: ErrKind, msg: impl AsRef<str>) -> Ctl {
        let e = self.make_error(kind, msg.as_ref());
        self.set_site_here();
        Ctl::Throw(Value::Obj(e))
    }
    pub fn type_error(&mut self, msg: impl AsRef<str>) -> Ctl {
        self.error(ErrKind::TypeError, msg)
    }
    pub fn range_error(&mut self, msg: impl AsRef<str>) -> Ctl {
        self.error(ErrKind::RangeError, msg)
    }
    pub fn reference_error(&mut self, msg: impl AsRef<str>) -> Ctl {
        self.error(ErrKind::ReferenceError, msg)
    }
    pub fn syntax_error(&mut self, msg: impl AsRef<str>) -> Ctl {
        self.error(ErrKind::SyntaxError, msg)
    }

    /// Records the current JS position as the throw site.
    pub fn set_site_here(&mut self) {
        if let Some(f) = self.frames.last() {
            let pc =
                f.pc.saturating_sub(1)
                    .min(f.code.pos.len().saturating_sub(1));
            if let Some(p) = f.code.pos.get(pc) {
                self.throw_site = Some(Site {
                    file: f.code.file.clone(),
                    line: p.line,
                    col: p.col,
                });
            }
        }
    }

    /// Name of a receiver's class for stack frames (`Foo.bar`).
    pub fn type_name(&mut self, v: &Value) -> Option<String> {
        match v {
            Value::Undefined | Value::Null | Value::Empty => None,
            Value::Str(_) => Some("String".into()),
            Value::Num(_) => Some("Number".into()),
            Value::Bool(_) => Some("Boolean".into()),
            Value::Sym(_) => Some("Symbol".into()),
            Value::BigInt(_) => Some("BigInt".into()),
            Value::Obj(o) => {
                if o.ptr_eq(&self.global) {
                    return None;
                }
                Some(self.constructor_name(o).unwrap_or_else(|| "Object".into()))
            }
        }
    }

    /// `obj.constructor.name` found along the prototype chain without
    /// running getters.
    pub fn constructor_name(&self, o: &Obj) -> Option<String> {
        if o.is_callable() {
            return Some("Function".into());
        }
        let mut cur = Some(o.clone());
        let mut hops = 0;
        while let Some(c) = cur {
            if let Some(Value::Obj(ctor)) = c.own_value("constructor") {
                if let Some(Value::Str(n)) = ctor.own_value("name") {
                    if !n.is_empty() {
                        return Some(n.to_string());
                    }
                }
            }
            if let Some(Value::Str(tag)) = c
                .borrow()
                .props
                .get(&Key::Sym(self.syms.to_string_tag.clone()))
                .and_then(|p| match &p.slot {
                    Slot::Data(v) => Some(v.clone()),
                    _ => None,
                })
            {
                return Some(tag.to_string());
            }
            cur = c.proto();
            hops += 1;
            if hops > 100 {
                break;
            }
        }
        None
    }

    pub fn func_name(o: &Obj) -> String {
        match o.own_value("name") {
            Some(Value::Str(s)) => s.to_string(),
            _ => String::new(),
        }
    }

    /// Formats the frames currently on the stack (innermost first).
    pub fn stack_frames(&mut self, skip_top_native: bool) -> (Vec<String>, Option<Site>) {
        let mut out = vec![];
        let mut funcs: Vec<Option<Obj>> = vec![];
        let mut site = None;
        let mut ni = self.natives.len();
        let nframes = self.frames.len();
        // Walk from the top: natives whose depth == i+1 sit above frame i.
        let mut i = nframes;
        let mut first_native = true;
        loop {
            // Natives called from frame i-1 (depth == i).
            while ni > 0 && self.natives[ni - 1].depth >= i {
                ni -= 1;
                let (callee, this, construct) = {
                    let m = &self.natives[ni];
                    (m.callee.clone(), m.this.clone(), m.construct)
                };
                // Builtins V8 never shows in traces.
                let nm = Self::func_name(&callee);
                if ((nm == "call" || nm == "apply" || nm == "bind") && this.is_callable())
                    || (construct && nm == "Array")
                    || nm == "%hidden"
                {
                    continue;
                }
                if skip_top_native && first_native && out.is_empty() {
                    first_native = false;
                    let n = Self::func_name(&callee);
                    if matches!(
                        n.as_str(),
                        "Error"
                            | "TypeError"
                            | "RangeError"
                            | "SyntaxError"
                            | "ReferenceError"
                            | "EvalError"
                            | "URIError"
                            | "AggregateError"
                            | "captureStackTrace"
                    ) {
                        continue;
                    }
                }
                first_native = false;
                // Natives standing in for Node internals render their frames.
                let fsym = self
                    .symbol_registry
                    .iter()
                    .find(|(k, _)| k == "%frames")
                    .map(|(_, s)| s.clone());
                if let Some(fs) = fsym {
                    let ov = callee.borrow().props.get(&Key::Sym(fs)).cloned();
                    if let Some(Prop {
                        slot: Slot::Data(Value::Str(lines)),
                        ..
                    }) = ov
                    {
                        for l in lines.split('\n') {
                            out.push(l.to_string());
                            funcs.push(Some(callee.clone()));
                        }
                        continue;
                    }
                }
                let name = Self::func_name(&callee);
                let text = if construct {
                    format!("new {name}")
                } else {
                    match self.type_name(&this) {
                        Some(t) if !name.is_empty() => {
                            if this.is_callable() {
                                let fname = match &this {
                                    Value::Obj(o) => Self::func_name(o),
                                    _ => String::new(),
                                };
                                format!("{fname}.{name}")
                            } else {
                                format!("{t}.{name}")
                            }
                        }
                        _ => {
                            if name.is_empty() {
                                "<anonymous>".into()
                            } else {
                                name
                            }
                        }
                    }
                };
                out.push(format!("{text} (<anonymous>)"));
                funcs.push(Some(callee.clone()));
            }
            if i == 0 {
                break;
            }
            i -= 1;
            let (code, pc, recv, construct, resumed, func, timer) = {
                let f = &self.frames[i];
                (
                    f.code.clone(),
                    f.pc,
                    f.recv.clone(),
                    matches!(f.kind, FrameKind::Construct(_)),
                    f.resumed,
                    f.func.clone(),
                    f.timer,
                )
            };
            let p = code
                .pos
                .get(pc.saturating_sub(1))
                .copied()
                .unwrap_or_default();
            if site.is_none() {
                site = Some(Site {
                    file: code.file.clone(),
                    line: p.line,
                    col: p.col,
                });
            }
            let loc = format!("{}:{}:{}", display_file(&code.file), p.line, p.col);
            let fname = match &func {
                Some(f) => Self::func_name(f),
                None => code.name.to_string(),
            };
            let name = if code.is_top {
                if code.file.starts_with("file://") || code.file.starts_with('[') {
                    String::new()
                } else {
                    "Object.<anonymous>".to_string()
                }
            } else if timer != 0 {
                let (owner, method) = if timer == TIMEOUT_FRAME {
                    ("Timeout", "_onTimeout")
                } else {
                    ("Immediate", "_onImmediate")
                };
                // V8 infers the method alias only for the first segment of
                // an async immediate callback.
                if resumed && timer == IMMEDIATE_FRAME {
                    if fname.is_empty() {
                        format!("{owner}.<anonymous>")
                    } else {
                        format!("{owner}.{fname}")
                    }
                } else if fname.is_empty() {
                    format!("{owner}.{method}")
                } else {
                    format!("{owner}.{fname} [as {method}]")
                }
            } else if construct {
                format!(
                    "new {}",
                    if fname.is_empty() {
                        "<anonymous>"
                    } else {
                        &fname
                    }
                )
            } else if code.kind == crate::ast::FuncKind::Arrow {
                fname.clone()
            } else {
                let tn = if code.strict && recv.is_undefined() {
                    None
                } else {
                    self.type_name(&recv)
                };
                match tn {
                    Some(t) if !matches!(recv, Value::Undefined | Value::Null) => {
                        if fname.is_empty() {
                            format!("{t}.<anonymous>")
                        } else if recv.is_callable() {
                            let rn = match &recv {
                                Value::Obj(o) => Self::func_name(o),
                                _ => String::new(),
                            };
                            format!("{rn}.{fname}")
                        } else {
                            format!("{t}.{fname}")
                        }
                    }
                    _ => fname.clone(),
                }
            };
            if name.is_empty() {
                out.push(loc);
            } else {
                out.push(format!("{name} ({loc})"));
            }
            funcs.push(func.clone());
        }
        // Async callers waiting on the innermost async activation.
        if let Some(f) = self.frames.first() {
            if let FrameKind::Async { promise, .. } = &f.kind {
                let mut p = promise.clone();
                for _ in 0..10 {
                    let next = {
                        let d = p.borrow();
                        match &d.kind {
                            Kind::Promise(pd) => pd.fulfill.iter().find_map(|r| match r {
                                Reaction::Resume(co) => Some(co.clone()),
                                _ => None,
                            }),
                            _ => None,
                        }
                    };
                    let Some(co) = next else { break };
                    let info = {
                        let d = co.borrow();
                        match &d.kind {
                            Kind::Coroutine(Some(fr)) => {
                                let pc = fr.pc.saturating_sub(1);
                                let pos = fr.code.pos.get(pc).copied().unwrap_or_default();
                                let promise = match &fr.kind {
                                    FrameKind::Async { promise, .. } => Some(promise.clone()),
                                    _ => None,
                                };
                                let name = match &fr.func {
                                    Some(f) => Self::func_name(f),
                                    None => fr.code.name.to_string(),
                                };
                                Some((name, fr.code.file.clone(), pos, promise, fr.code.is_top))
                            }
                            _ => None,
                        }
                    };
                    let Some((name, file, pos, promise, is_top)) = info else {
                        break;
                    };
                    if is_top {
                        break;
                    }
                    let loc = format!("{}:{}:{}", display_file(&file), pos.line, pos.col);
                    out.push(if name.is_empty() {
                        format!("async {loc}")
                    } else {
                        format!("async {name} ({loc})")
                    });
                    funcs.push(None);
                    match promise {
                        Some(pp) => p = pp,
                        None => break,
                    }
                }
            }
        }
        self.trace_funcs = funcs;
        (out, site)
    }

    pub fn tail_frames(&self) -> Vec<&'static str> {
        const TICKS: &str = "processTicksAndRejections (node:internal/process/task_queues:85:11)";
        const JOBS: &str = "processTicksAndRejections (node:internal/process/task_queues:104:5)";
        const RUN_TICKS: &str = "runNextTicks (node:internal/process/task_queues:69:3)";
        let between = |b: Batch, first: &[&'static str]| -> Vec<&'static str> {
            let mut v = first.to_vec();
            match b {
                Batch::Immediates => {
                    v.push("process.processImmediate (node:internal/timers:541:9)")
                }
                Batch::List => {
                    v.push("listOnTimeout (node:internal/timers:644:9)");
                    v.push("process.processTimers (node:internal/timers:618:7)");
                }
                Batch::Lists => v.push("process.processTimers (node:internal/timers:615:9)"),
            }
            v
        };
        match self.tail {
            Tail::Tick(Some(b)) => return between(b, &[TICKS, RUN_TICKS]),
            Tail::Microtask(Some(b), true) => return between(b, &[JOBS, RUN_TICKS]),
            Tail::Microtask(Some(b), false) => {
                return between(
                    b,
                    &["runNextTicks (node:internal/process/task_queues:65:5)"],
                )
            }
            _ => {}
        }
        let frames: &'static [&'static str] = match self.tail {
            Tail::Main => &[
                "Module._compile (node:internal/modules/cjs/loader:1929:14)",
                "Object..js (node:internal/modules/cjs/loader:2060:10)",
                "Module.load (node:internal/modules/cjs/loader:1651:32)",
                "Module._load (node:internal/modules/cjs/loader:1443:12)",
                "wrapModuleLoad (node:internal/modules/cjs/loader:261:19)",
                "Module.executeUserEntryPoint [as runMain] (node:internal/modules/run_main:154:5)",
                "node:internal/main/run_main_module:33:47",
            ],
            Tail::Esm => &[
                "ModuleJob.run (node:internal/modules/esm/module_job:561:25)",
                "async node:internal/modules/esm/loader:647:26",
                "async asyncRunEntryPointWithESMLoader (node:internal/modules/run_main:101:5)",
            ],
            Tail::Timer => &[
                "listOnTimeout (node:internal/timers:685:17)",
                "process.processTimers (node:internal/timers:618:7)",
            ],
            Tail::Immediate => &["process.processImmediate (node:internal/timers:491:21)"],
            Tail::Tick(_) => {
                &["process.processTicksAndRejections (node:internal/process/task_queues:85:11)"]
            }
            Tail::Microtask(_, true) => {
                &["process.processTicksAndRejections (node:internal/process/task_queues:104:5)"]
            }
            Tail::Eval => &[
                "runScriptInThisContext (node:internal/vm:219:10)",
                "node:internal/process/execution:451:12",
                "[eval]-wrapper:6:24",
                "runScriptInContext (node:internal/process/execution:449:60)",
                "evalFunction (node:internal/process/execution:283:30)",
                "evalTypeScript (node:internal/process/execution:295:3)",
                "node:internal/main/eval_string:71:3",
            ],
            Tail::Check => &["checkSyntax (node:internal/main/check_syntax:88:3)"],
            Tail::Microtask(..) | Tail::None => &[],
        };
        frames.to_vec()
    }

    /// Appends the internal frames below user code. They precede the
    /// `async f` frames V8 appends for awaiting callers.
    pub fn append_tail(&self, frames: &mut Vec<String>) {
        let at = frames
            .iter()
            .position(|f| f.starts_with("async "))
            .unwrap_or(frames.len());
        let tail: Vec<String> = self.tail_frames().iter().map(|t| t.to_string()).collect();
        frames.splice(at..at, tail);
    }

    /// Captures the current stack into an error object.
    /// Frames up to and including `skip_until` (a subclass constructor
    /// running `super(...)`) are left out, as V8 does.
    pub fn capture_stack(&mut self, e: &Obj, skip_until: Option<&Obj>) {
        let (mut frames, site) = self.stack_frames(true);
        if let Some(f) = skip_until {
            if let Some(i) = self
                .trace_funcs
                .iter()
                .position(|x| matches!(x, Some(g) if g.ptr_eq(f)))
            {
                frames.drain(..=i.min(frames.len().saturating_sub(1)));
            }
        }
        self.append_tail(&mut frames);
        frames.truncate(self.stack_limit);
        if let Kind::Error(ed) = &mut e.borrow_mut().kind {
            ed.frames = frames;
            ed.site = site;
        }
        e.borrow_mut()
            .props
            .insert(Key::str("stack"), Prop::data(Value::Empty, HIDDEN));
    }

    /// Materialises a lazily formatted `stack`.
    pub fn format_stack(&mut self, e: &Obj) -> JsResult<Value> {
        let frames = match &e.borrow().kind {
            Kind::Error(ed) => ed.frames.clone(),
            _ => vec![],
        };
        let header = self.error_header(e)?;
        let mut s = header;
        for f in frames {
            s.push_str("\n    at ");
            s.push_str(&f);
        }
        let v = Value::string(s);
        if let Some(p) = e.borrow_mut().props.get_mut(&Key::str("stack")) {
            p.slot = Slot::Data(v.clone());
        }
        Ok(v)
    }

    /// `Error.prototype.toString` of an error-like object.
    pub fn error_header(&mut self, e: &Obj) -> JsResult<String> {
        let ev = Value::Obj(e.clone());
        let name = self.get_str(&ev, "name")?;
        let name = if name.is_undefined() {
            "Error".to_string()
        } else {
            self.to_string(&name)?.to_string()
        };
        let msg = self.get_str(&ev, "message")?;
        let msg = if msg.is_undefined() {
            String::new()
        } else {
            self.to_string(&msg)?.to_string()
        };
        Ok(if name.is_empty() {
            msg
        } else if msg.is_empty() {
            name
        } else {
            format!("{name}: {msg}")
        })
    }

    // ------------------------------------------------------------ time
    /// Virtual milliseconds since start. Executing code takes time too: the
    /// clock advances one millisecond per `STEPS_PER_MS` instructions, so
    /// busy-waiting on `Date.now()` terminates deterministically.
    pub fn clock(&mut self) -> f64 {
        if self.steps > self.clock_steps {
            self.elapsed_ms += (self.steps - self.clock_steps) as f64 / STEPS_PER_MS;
            self.clock_steps = self.steps;
        }
        self.elapsed_ms
    }

    pub fn now_ms(&mut self) -> f64 {
        (self.start_micros as f64) / 1000.0 + self.clock()
    }

    pub fn random(&mut self) -> f64 {
        // xorshift over the host-seeded state (world entropy).
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn register_source(&mut self, file: Rc<str>, src: Rc<str>) {
        self.sources.push((file, src));
    }
}

pub fn display_file(file: &str) -> String {
    file.to_string()
}

pub fn new_cell(v: Value) -> CellRef {
    Rc::new(RefCell::new(v))
}
