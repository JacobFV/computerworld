//! The DAP-shaped debugger: breakpoints, stepping and a view of the stopped
//! program (threads, frames, scopes, variables, evaluation in a frame).
//!
//! While a [`Debugger`] is attached the interpreter consults it at every new
//! source line. Everything the front end sees comes from the live interpreter:
//! an expression it evaluates runs in the frame it names, and a value it changes
//! is changed in the program.
use crate::value::*;
use crate::vm::*;
use cw_script_host::debug::*;
use std::collections::BTreeMap;
use std::rc::Rc;

/// The debugger attached to a run, kept in the interpreter.
pub struct Session<'d> {
    pub dbg: &'d mut dyn Debugger,
    pub state: State,
}

/// Where the debugger left the program.
pub struct State {
    /// Instructions since the last stop, for `pause_after`.
    since_stop: u64,
    /// Hits per breakpoint line, for hit conditions.
    hits: BTreeMap<(String, u32), u64>,
    /// What the front end said at the last stop.
    mode: Step,
    /// Call depth at the last stop (`next` and `step out` compare against it).
    depth_at_stop: usize,
    /// Where the last line hook fired, so one line stops once.
    last: Option<(String, u32, usize)>,
    /// Whether the main program has begun (for `stop_on_entry`); the
    /// interpreter's own setup imports run before it.
    entered: bool,
    /// The program's own file: `stop_on_entry` waits for it.
    pub main_path: String,
    /// The debugger is evaluating an expression of its own: its interpreter
    /// steps are not the program's.
    evaluating: bool,
    pub info: DebugRunInfo,
    /// The stopped program's frames, innermost first.
    frames: Vec<FrameRef>,
    /// What each variables reference the front end holds points at.
    refs: Vec<RefTarget>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            since_stop: 0,
            hits: BTreeMap::new(),
            mode: Step::Continue,
            depth_at_stop: 0,
            last: None,
            entered: false,
            main_path: String::new(),
            evaluating: false,
            info: DebugRunInfo::default(),
            frames: vec![],
            refs: vec![],
        }
    }
}

/// A frame of the stopped program.
struct FrameRef {
    name: String,
    path: String,
    line: u32,
    column: u32,
    /// Index into `vm.frames`, or `None` for the frame that is running.
    index: Option<usize>,
}

enum RefTarget {
    Locals(usize),
    Globals(usize),
    Value(Value),
}

impl<'d> Session<'d> {
    pub fn new(dbg: &'d mut dyn Debugger) -> Self {
        Self {
            dbg,
            state: State::default(),
        }
    }
}

/// Ends the run where it stands, keeping what it did.
fn stop_run(vm: &mut Vm, terminate: bool) -> Box<PyErr> {
    if let Some(s) = vm.debug.as_mut() {
        s.state.info.suspended = !terminate;
        s.state.info.terminated = terminate;
    }
    Box::new(PyErr {
        kind: ErrKind::Lazy("SystemExit", vec![Value::Int(0)]),
        reraise: false,
        fatal: true,
    })
}

/// The interpreter is about to run an instruction: stop if this is a new line
/// the front end cares about.
pub fn line_hook(vm: &mut Vm, f: &mut Frame) -> PyResult<()> {
    let pos = f.code.pos.get(f.pc).copied();
    let line = pos.map(|p| p.line).unwrap_or(f.code.firstlineno);
    let column = pos.map(|p| p.col + 1).unwrap_or(1);
    let path = f.code.filename.to_string();
    let depth = vm.frames.len();
    let (first, same_line) = {
        let Some(s) = vm.debug.as_mut() else {
            return Ok(());
        };
        if s.state.evaluating {
            return Ok(());
        }
        let same = s
            .state
            .last
            .as_ref()
            .is_some_and(|(p, l, d)| *l == line && *p == path && *d == depth);
        // The debugger watches the program, which starts at its own first
        // line: the imports the interpreter runs to set itself up are not it.
        let is_main = s.state.main_path.is_empty() || s.state.main_path == path;
        if !s.state.entered && !is_main {
            return Ok(());
        }
        let first = is_main && !s.state.entered;
        s.state.entered |= is_main;
        s.state.since_stop += 1;
        if !same {
            s.state.last = Some((path.clone(), line, depth));
        }
        (first, same)
    };
    if same_line {
        return Ok(());
    }
    let (mode, depth_at_stop, since, stop_on_entry, pause_after) = {
        let s = vm.debug.as_ref().unwrap();
        (
            s.state.mode,
            s.state.depth_at_stop,
            s.state.since_stop,
            s.dbg.config().stop_on_entry,
            s.dbg.config().pause_after,
        )
    };
    if first && stop_on_entry {
        return stop(vm, f, entry_event(vm), line, column, depth);
    }

    // Breakpoints on this line: hit counts, conditions and log messages.
    let bps: Vec<SourceBreakpoint> = vm
        .debug
        .as_ref()
        .unwrap()
        .dbg
        .config()
        .breakpoints_at(&path, line)
        .into_iter()
        .cloned()
        .collect();
    let mut hit_ids = vec![];
    for bp in bps {
        let count = {
            let s = vm.debug.as_mut().unwrap();
            let e = s.state.hits.entry((path.clone(), line)).or_insert(0);
            *e += 1;
            *e
        };
        if let Some(h) = &bp.hit_condition {
            if !hit_condition_met(h, count) {
                continue;
            }
        }
        if let Some(cond) = &bp.condition {
            let truthy = match eval_in_frame(vm, f, cond) {
                Ok(v) => vm.truthy(&v).unwrap_or(false),
                Err(_) => false,
            };
            if !truthy {
                continue;
            }
        }
        if let Some(msg) = &bp.log_message {
            let mut parts: Vec<(String, String)> = vec![];
            let text = format_log_message(msg, |expr| {
                let v = match eval_in_frame(vm, f, expr) {
                    Ok(v) => vm.str_of(&v).unwrap_or_default(),
                    Err(e) => e,
                };
                parts.push((expr.to_string(), v.clone()));
                v
            });
            let s = vm.debug.as_mut().unwrap();
            s.dbg.log(&text);
            continue;
        }
        let id = vm
            .debug
            .as_ref()
            .unwrap()
            .dbg
            .config()
            .breakpoint_id(&path, line);
        if let Some(id) = id {
            hit_ids.push(id);
        }
    }

    let stepping = match mode {
        Step::Next => depth <= depth_at_stop,
        Step::StepIn => true,
        Step::StepOut => depth < depth_at_stop,
        _ => false,
    };
    let reason = if !hit_ids.is_empty() {
        StopReason::Breakpoint
    } else if stepping {
        StopReason::Step
    } else if pause_after.is_some_and(|n| since >= n) {
        StopReason::Pause
    } else {
        return Ok(());
    };
    let event = StopEvent {
        reason,
        thread_id: thread_id(vm),
        description: String::new(),
        text: String::new(),
        hit_breakpoint_ids: hit_ids,
    };
    stop(vm, f, event, line, column, depth)
}

fn thread_id(vm: &Vm) -> u64 {
    vm.sched.as_ref().map(|s| s.current_id()).unwrap_or(1)
}

fn entry_event(vm: &Vm) -> StopEvent {
    StopEvent {
        reason: StopReason::Entry,
        thread_id: thread_id(vm),
        description: String::new(),
        text: String::new(),
        hit_breakpoint_ids: vec![],
    }
}

/// An exception was raised: stop when the front end asked for it.
pub fn exception_hook(vm: &mut Vm, f: &mut Frame, exc: &Value, uncaught: bool) -> PyResult<()> {
    let wanted = match vm.debug.as_ref() {
        Some(s) => {
            let e = s.dbg.config().exceptions;
            if uncaught {
                e.uncaught
            } else {
                e.raised
            }
        }
        None => false,
    };
    if !wanted {
        return Ok(());
    }
    let description = vm.type_name(exc).to_string();
    let text = vm.str_of(exc).unwrap_or_default();
    let pos = f.code.pos.get(f.pc.saturating_sub(1)).copied();
    let line = pos.map(|p| p.line).unwrap_or(f.code.firstlineno);
    let column = pos.map(|p| p.col + 1).unwrap_or(1);
    let depth = vm.frames.len();
    let event = StopEvent {
        reason: StopReason::Exception,
        thread_id: thread_id(vm),
        description,
        text,
        hit_breakpoint_ids: vec![],
    };
    stop(vm, f, event, line, column, depth)
}

/// Stops the program: builds the view, asks the front end, applies the answer.
fn stop(
    vm: &mut Vm,
    f: &mut Frame,
    event: StopEvent,
    line: u32,
    column: u32,
    depth: usize,
) -> PyResult<()> {
    let frames = collect_frames(vm, f, line, column);
    let mut session = vm.debug.take().expect("debug session");
    session.state.frames = frames;
    session.state.refs.clear();
    session.state.since_stop = 0;
    let step = {
        let Session { dbg, state } = &mut *session;
        let mut view = View { vm, state, top: f };
        dbg.stopped(&event, &mut view)
    };
    session.state.mode = step;
    session.state.depth_at_stop = depth;
    session.state.frames.clear();
    session.state.refs.clear();
    vm.debug = Some(session);
    match step {
        Step::Suspend => Err(stop_run(vm, false)),
        Step::Terminate => Err(stop_run(vm, true)),
        _ => Ok(()),
    }
}

/// The stopped thread's frames, innermost first.
fn collect_frames(vm: &Vm, f: &Frame, line: u32, column: u32) -> Vec<FrameRef> {
    let mut out = vec![FrameRef {
        name: frame_name(f),
        path: f.code.filename.to_string(),
        line,
        column,
        index: None,
    }];
    for (i, fr) in vm.frames.iter().enumerate().rev() {
        let pos = fr.code.pos.get(fr.pc.saturating_sub(1)).copied();
        out.push(FrameRef {
            name: frame_name(fr),
            path: fr.code.filename.to_string(),
            line: pos.map(|p| p.line).unwrap_or(fr.code.firstlineno),
            column: pos.map(|p| p.col + 1).unwrap_or(1),
            index: Some(i),
        });
    }
    out
}

fn frame_name(f: &Frame) -> String {
    f.code.name.to_string()
}

/// Evaluates `src` with `f`'s globals and locals, as a debug console does.
pub fn eval_in_frame(vm: &mut Vm, f: &mut Frame, src: &str) -> Result<Value, String> {
    // What the debugger runs is not the program running.
    let was = vm.debug.as_ref().map(|s| s.state.evaluating);
    if let Some(s) = vm.debug.as_mut() {
        s.state.evaluating = true;
    }
    let r = eval_in_frame_inner(vm, f, src);
    if let (Some(w), Some(s)) = (was, vm.debug.as_mut()) {
        s.state.evaluating = w;
    }
    r
}

fn eval_in_frame_inner(vm: &mut Vm, f: &mut Frame, src: &str) -> Result<Value, String> {
    let locals = frame_locals_dict(f);
    let globals = f.globals.clone();
    let code = crate::compile_source(vm, src.trim(), "<debug>", "eval")
        .map_err(|mut e| exc_text(vm, &mut e))?;
    let frame = vm.new_frame(code, globals, Some(locals));
    match vm.execute(frame, None) {
        Ok(Exit::Return(v)) => Ok(v),
        Ok(_) => Ok(Value::None),
        Err(mut e) => Err(exc_text(vm, &mut e)),
    }
}

fn exc_text(vm: &mut Vm, e: &mut Box<PyErr>) -> String {
    let exc = vm.materialize(e);
    let name = vm.type_name(&exc).to_string();
    let msg = vm.str_of(&exc).unwrap_or_default();
    if msg.is_empty() {
        name
    } else {
        format!("{name}: {msg}")
    }
}

/// A frame's locals as a dict: fast locals, cells and free variables, or the
/// namespace itself for a module or class body.
fn frame_locals_dict(f: &Frame) -> Ref<Dict> {
    if let Some(d) = &f.locals {
        return d.clone();
    }
    // A module or class body keeps its names in its namespace, not in slots.
    if f.code.uses_name_ops {
        return f.globals.clone();
    }
    let d = new_ref(Dict::new());
    {
        let mut m = d.borrow_mut();
        for (i, name) in f.code.varnames.iter().enumerate() {
            if let Some(v) = f.fast.get(i) {
                if !matches!(v, Value::Undefined) {
                    m.set_str(name, v.clone());
                }
            }
        }
        let cellnames = f.code.cellvars.iter().chain(f.code.freevars.iter());
        for (i, name) in cellnames.enumerate() {
            if let Some(c) = f.cells.get(i) {
                let v = c.borrow().clone();
                if !matches!(v, Value::Undefined) {
                    m.set_str(name, v);
                }
            }
        }
    }
    d
}

/// The stopped program as the front end sees it.
pub struct View<'a, 'h> {
    vm: &'a mut Vm<'h>,
    state: &'a mut State,
    top: &'a mut Frame,
}

impl View<'_, '_> {
    /// Hands out a reference the front end can expand.
    fn reference(&mut self, t: RefTarget) -> u64 {
        self.state.refs.push(t);
        self.state.refs.len() as u64
    }
    fn frame_of(&mut self, index: Option<usize>) -> &mut Frame {
        match index {
            None => self.top,
            Some(i) => &mut self.vm.frames[i],
        }
    }
    /// Describes a value the way the console shows it.
    fn describe(&mut self, name: &str, v: &Value) -> Variable {
        let value = self
            .vm
            .repr(v)
            .unwrap_or_else(|_| "<unrepresentable>".into());
        let type_name = self.vm.type_name(v).to_string();
        let (named, indexed) = child_counts(self.vm, v);
        let variables_reference = if named + indexed > 0 {
            self.reference(RefTarget::Value(v.clone()))
        } else {
            0
        };
        Variable {
            name: name.to_string(),
            value,
            type_name,
            variables_reference,
            named_variables: named,
            indexed_variables: indexed,
        }
    }
}

/// How many children a value has, split into named and indexed as DAP wants.
fn child_counts(vm: &mut Vm, v: &Value) -> (u64, u64) {
    match v {
        Value::List(l) => (0, l.borrow().len() as u64),
        Value::Tuple(t) => (0, t.len() as u64),
        Value::Dict(d) => (d.borrow().len() as u64, 0),
        Value::Set(s) => (0, s.borrow().len() as u64),
        Value::Instance(i) => (i.dict.borrow().len() as u64, 0),
        Value::Module(m) => (m.dict.borrow().len() as u64, 0),
        _ => {
            let _ = vm;
            (0, 0)
        }
    }
}

impl DebugTarget for View<'_, '_> {
    fn threads(&mut self) -> Vec<Thread> {
        match self.vm.sched.as_ref() {
            Some(s) => s
                .threads
                .iter()
                .filter(|t| t.status != crate::sched::Status::Finished)
                .map(|t| Thread {
                    id: t.id,
                    name: t.name.clone(),
                })
                .collect(),
            None => vec![Thread {
                id: 1,
                name: "MainThread".into(),
            }],
        }
    }

    fn stack_trace(&mut self, _thread_id: u64) -> Vec<StackFrame> {
        self.state
            .frames
            .iter()
            .enumerate()
            .map(|(i, f)| StackFrame {
                id: i as u64 + 1,
                name: f.name.clone(),
                path: f.path.clone(),
                line: f.line,
                column: f.column,
            })
            .collect()
    }

    fn scopes(&mut self, frame_id: u64) -> Vec<Scope> {
        let i = frame_id.saturating_sub(1) as usize;
        if i >= self.state.frames.len() {
            return vec![];
        }
        let locals = self.reference(RefTarget::Locals(i));
        let globals = self.reference(RefTarget::Globals(i));
        vec![
            Scope {
                name: "Locals".into(),
                variables_reference: locals,
                expensive: false,
            },
            Scope {
                name: "Globals".into(),
                variables_reference: globals,
                expensive: true,
            },
        ]
    }

    fn variables(&mut self, variables_reference: u64) -> Vec<Variable> {
        let i = variables_reference.saturating_sub(1) as usize;
        let target = match self.state.refs.get(i) {
            Some(RefTarget::Locals(f)) => RefTarget::Locals(*f),
            Some(RefTarget::Globals(f)) => RefTarget::Globals(*f),
            Some(RefTarget::Value(v)) => RefTarget::Value(v.clone()),
            None => return vec![],
        };
        let pairs: Vec<(String, Value)> = match target {
            RefTarget::Locals(fi) => {
                let index = self.state.frames.get(fi).and_then(|f| f.index);
                let f = self.frame_of(index);
                let d = frame_locals_dict(f);
                let items = d.borrow().items();
                items
                    .into_iter()
                    .filter_map(|(k, v)| key_name(&k).map(|n| (n, v)))
                    .collect()
            }
            RefTarget::Globals(fi) => {
                let index = self.state.frames.get(fi).and_then(|f| f.index);
                let g = self.frame_of(index).globals.clone();
                let items = g.borrow().items();
                items
                    .into_iter()
                    .filter_map(|(k, v)| key_name(&k).map(|n| (n, v)))
                    .filter(|(n, _)| n != "__builtins__")
                    .collect()
            }
            RefTarget::Value(v) => children_of(self.vm, &v),
        };
        pairs
            .into_iter()
            .map(|(n, v)| self.describe(&n, &v))
            .collect()
    }

    fn evaluate(&mut self, expression: &str, frame_id: Option<u64>) -> Result<Variable, String> {
        let fi = frame_id.map(|f| f.saturating_sub(1) as usize).unwrap_or(0);
        let index = match self.state.frames.get(fi) {
            Some(f) => f.index,
            None => return Err("no such frame".into()),
        };
        // The frame is borrowed from the interpreter for the evaluation.
        let empty = placeholder_frame(self.vm);
        let mut frame = std::mem::replace(self.frame_of(index), empty);
        let r = eval_in_frame(self.vm, &mut frame, expression);
        *self.frame_of(index) = frame;
        let v = r?;
        Ok(self.describe(expression, &v))
    }

    fn set_variable(
        &mut self,
        variables_reference: u64,
        name: &str,
        value: &str,
    ) -> Result<Variable, String> {
        let i = variables_reference.saturating_sub(1) as usize;
        let target = match self.state.refs.get(i) {
            Some(RefTarget::Locals(f)) => RefTarget::Locals(*f),
            Some(RefTarget::Globals(f)) => RefTarget::Globals(*f),
            Some(RefTarget::Value(v)) => RefTarget::Value(v.clone()),
            None => return Err("no such variable".into()),
        };
        let frame_index = match &target {
            RefTarget::Locals(fi) | RefTarget::Globals(fi) => {
                self.state.frames.get(*fi).and_then(|f| f.index)
            }
            RefTarget::Value(_) => self.state.frames.first().and_then(|f| f.index),
        };
        let empty = placeholder_frame(self.vm);
        let mut frame = std::mem::replace(self.frame_of(frame_index), empty);
        let evaluated = eval_in_frame(self.vm, &mut frame, value);
        let v = match evaluated {
            Ok(v) => v,
            Err(e) => {
                *self.frame_of(frame_index) = frame;
                return Err(e);
            }
        };
        let r = match &target {
            RefTarget::Locals(_) => store_local(&mut frame, name, v.clone()),
            RefTarget::Globals(_) => {
                frame.globals.borrow_mut().set_str(name, v.clone());
                Ok(())
            }
            RefTarget::Value(container) => store_child(self.vm, container, name, v.clone()),
        };
        *self.frame_of(frame_index) = frame;
        r?;
        Ok(self.describe(name, &v))
    }
}

/// A frame that stands in while a real one is borrowed for an evaluation.
fn placeholder_frame(vm: &mut Vm) -> Frame {
    let code = Rc::new(Code {
        name: "<placeholder>".into(),
        qualname: "<placeholder>".into(),
        filename: "<debug>".into(),
        ops: vec![],
        pos: vec![],
        consts: vec![],
        names: vec![],
        varnames: vec![],
        cellvars: vec![],
        freevars: vec![],
        cell2arg: vec![],
        argcount: 0,
        posonlyargcount: 0,
        kwonlyargcount: 0,
        varargs: false,
        varkw: false,
        is_generator: false,
        is_coroutine: false,
        firstlineno: 1,
        uses_name_ops: false,
        docstring: None,
    });
    *vm.new_frame(code, new_ref(Dict::new()), None)
}

fn key_name(k: &Value) -> Option<String> {
    match k {
        Value::Str(s) => Some(s.s.to_string()),
        _ => None,
    }
}

/// The children of a container, as the front end expands them.
fn children_of(vm: &mut Vm, v: &Value) -> Vec<(String, Value)> {
    match v {
        Value::List(l) => l
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, x)| (i.to_string(), x.clone()))
            .collect(),
        Value::Tuple(t) => t
            .iter()
            .enumerate()
            .map(|(i, x)| (i.to_string(), x.clone()))
            .collect(),
        Value::Set(s) => s
            .borrow()
            .items()
            .into_iter()
            .enumerate()
            .map(|(i, x)| (i.to_string(), x))
            .collect(),
        Value::Dict(d) => d
            .borrow()
            .items()
            .into_iter()
            .map(|(k, val)| {
                let name = vm.repr(&k).unwrap_or_else(|_| "?".into());
                (name, val)
            })
            .collect(),
        Value::Instance(i) => i
            .dict
            .borrow()
            .items()
            .into_iter()
            .filter_map(|(k, val)| key_name(&k).map(|n| (n, val)))
            .collect(),
        Value::Module(m) => m
            .dict
            .borrow()
            .items()
            .into_iter()
            .filter_map(|(k, val)| key_name(&k).map(|n| (n, val)))
            .filter(|(n, _)| n != "__builtins__")
            .collect(),
        _ => vec![],
    }
}

/// Writes a name in a frame: a fast local, a cell, or the frame's namespace.
fn store_local(f: &mut Frame, name: &str, v: Value) -> Result<(), String> {
    if let Some(i) = f.code.varnames.iter().position(|n| &**n == name) {
        if i < f.fast.len() {
            f.fast[i] = v;
            return Ok(());
        }
    }
    let cells = f.code.cellvars.iter().chain(f.code.freevars.iter());
    if let Some(i) = cells
        .enumerate()
        .find(|(_, n)| &***n == name)
        .map(|(i, _)| i)
    {
        if let Some(c) = f.cells.get(i) {
            *c.borrow_mut() = v;
            return Ok(());
        }
    }
    if let Some(d) = &f.locals {
        d.borrow_mut().set_str(name, v);
        return Ok(());
    }
    // A module or class body keeps its names in its namespace.
    if f.code.uses_name_ops {
        f.globals.borrow_mut().set_str(name, v);
        return Ok(());
    }
    Err(format!("no local named {name}"))
}

/// Writes a child of a container the front end expanded.
fn store_child(vm: &mut Vm, container: &Value, name: &str, v: Value) -> Result<(), String> {
    match container {
        Value::List(l) => {
            let i: usize = name.parse().map_err(|_| "not an index".to_string())?;
            let mut b = l.borrow_mut();
            if i >= b.len() {
                return Err("index out of range".into());
            }
            b[i] = v;
            Ok(())
        }
        Value::Dict(d) => {
            // The name is the key's repr; match it against the keys there are.
            let keys: Vec<Value> = d.borrow().items().into_iter().map(|(k, _)| k).collect();
            for k in keys {
                if vm.repr(&k).unwrap_or_default() == name {
                    vm.dict_set(d, k, v).map_err(|_| "cannot set".to_string())?;
                    return Ok(());
                }
            }
            Err(format!("no key {name}"))
        }
        Value::Instance(i) => {
            i.dict.borrow_mut().set_str(name, v);
            Ok(())
        }
        Value::Module(m) => {
            m.dict.borrow_mut().set_str(name, v);
            Ok(())
        }
        _ => Err("this value has no children to set".into()),
    }
}
