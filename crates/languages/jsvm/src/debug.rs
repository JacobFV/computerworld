//! The DAP-shaped debugger: breakpoints, stepping and a view of the stopped
//! program (frames, scopes, variables, evaluation in a frame).
//!
//! While a [`Debugger`] is attached the interpreter consults it at every new
//! source line. Everything the front end sees comes from the live interpreter:
//! an expression it evaluates runs with the frame's bindings in scope, and a
//! value it changes is changed in the program.
use crate::value::*;
use crate::vm::*;
use cw_script_host::debug::*;
use std::collections::BTreeMap;
use std::rc::Rc;

/// The debugger attached to a run.
pub struct Session<'d> {
    pub dbg: &'d mut dyn Debugger,
    pub state: State,
}

pub struct State {
    since_stop: u64,
    hits: BTreeMap<(String, u32), u64>,
    mode: Step,
    depth_at_stop: usize,
    last: Option<(String, u32, usize)>,
    entered: bool,
    /// The program's own file: `stop_on_entry` waits for it.
    pub main_path: String,
    /// The debugger is running an expression of its own.
    evaluating: bool,
    pub info: DebugRunInfo,
    frames: Vec<FrameRef>,
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

impl<'d> Session<'d> {
    pub fn new(dbg: &'d mut dyn Debugger) -> Self {
        Self {
            dbg,
            state: State::default(),
        }
    }
}

struct FrameRef {
    name: String,
    path: String,
    line: u32,
    column: u32,
    /// Index into `vm.frames`.
    index: usize,
}

enum RefTarget {
    Locals(usize),
    Globals,
    Value(Value),
}

/// Ends the run where it stands, keeping what it did. Like `process.exit`, it
/// is not catchable.
fn stop_run(vm: &mut Vm, terminate: bool) -> Ctl {
    if let Some(s) = vm.debug.as_mut() {
        s.state.info.suspended = !terminate;
        s.state.info.terminated = terminate;
    }
    Ctl::Exit(0)
}

/// The interpreter is about to run an instruction: stop if this is a new line
/// the front end cares about.
pub fn line_hook(vm: &mut Vm) -> JsResult<()> {
    let Some((path, line, column, depth)) = position(vm) else {
        return Ok(());
    };
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
        // The debugger watches the program, which starts at its own first line.
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
        let event = event_at(vm, StopReason::Entry, vec![]);
        return stop(vm, event, depth);
    }

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
            let truthy = match eval_in_frame(vm, depth, cond) {
                Ok(v) => v.truthy(),
                Err(_) => false,
            };
            if !truthy {
                continue;
            }
        }
        if let Some(msg) = &bp.log_message {
            let text = format_log_message(msg, |expr| match eval_in_frame(vm, depth, expr) {
                Ok(v) => vm.to_str(&v).unwrap_or_default(),
                Err(e) => e,
            });
            let s = vm.debug.as_mut().unwrap();
            s.dbg.log(&text);
            continue;
        }
        if let Some(id) = vm
            .debug
            .as_ref()
            .unwrap()
            .dbg
            .config()
            .breakpoint_id(&path, line)
        {
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
    let _ = column;
    let event = event_at(vm, reason, hit_ids);
    stop(vm, event, depth)
}

/// An error is being thrown: stop when the front end asked for it.
pub fn exception_hook(vm: &mut Vm, v: &Value, uncaught: bool) -> JsResult<()> {
    let wanted = match vm.debug.as_ref() {
        Some(s) => {
            let e = s.dbg.config().exceptions;
            if s.state.evaluating {
                return Ok(());
            }
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
    let (description, text) = describe_error(vm, v);
    let depth = vm.frames.len();
    let mut event = event_at(vm, StopReason::Exception, vec![]);
    event.description = description;
    event.text = text;
    stop(vm, event, depth)
}

fn describe_error(vm: &mut Vm, v: &Value) -> (String, String) {
    if let Value::Obj(o) = v {
        if matches!(o.borrow().kind, Kind::Error(_)) {
            let name = vm
                .get_str(v, "name")
                .and_then(|n| vm.to_str(&n))
                .unwrap_or_default();
            let msg = vm
                .get_str(v, "message")
                .and_then(|m| vm.to_str(&m))
                .unwrap_or_default();
            return (name, msg);
        }
    }
    let text = vm.inspect_default(v).unwrap_or_default();
    (String::new(), text)
}

fn event_at(vm: &Vm, reason: StopReason, hit_breakpoint_ids: Vec<u64>) -> StopEvent {
    let _ = vm;
    StopEvent {
        reason,
        thread_id: 1,
        description: String::new(),
        text: String::new(),
        hit_breakpoint_ids,
    }
}

/// Where the running frame stands: `(path, line, column, depth)`.
fn position(vm: &Vm) -> Option<(String, u32, u32, usize)> {
    let f = vm.frames.last()?;
    let pos = f.code.pos.get(f.pc).copied()?;
    // Instructions that belong to no source line (a function's prologue) are
    // not a place to stop.
    if pos.line == 0 {
        return None;
    }
    Some((f.code.file.to_string(), pos.line, pos.col, vm.frames.len()))
}

/// Stops the program: builds the view, asks the front end, applies the answer.
fn stop(vm: &mut Vm, event: StopEvent, depth: usize) -> JsResult<()> {
    let frames = collect_frames(vm);
    let mut session = vm.debug.take().expect("debug session");
    session.state.frames = frames;
    session.state.refs.clear();
    session.state.since_stop = 0;
    let step = {
        let Session { dbg, state } = &mut *session;
        let mut view = View { vm, state };
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

/// The program's frames, innermost first.
fn collect_frames(vm: &Vm) -> Vec<FrameRef> {
    let n = vm.frames.len();
    vm.frames
        .iter()
        .enumerate()
        .rev()
        .map(|(i, f)| {
            // The running frame stands before the instruction it will run; the
            // others before the call they are in the middle of.
            let pc = if i + 1 == n {
                f.pc
            } else {
                f.pc.saturating_sub(1)
            };
            let pos = f.code.pos.get(pc).copied();
            FrameRef {
                name: frame_name(f),
                path: f.code.file.to_string(),
                line: pos.map(|p| p.line).unwrap_or(0),
                column: pos.map(|p| p.col).unwrap_or(1),
                index: i,
            }
        })
        .collect()
}

fn frame_name(f: &Frame) -> String {
    if f.code.is_top {
        return "(anonymous)".into();
    }
    let n = f.code.name.to_string();
    if n.is_empty() {
        "(anonymous)".into()
    } else {
        n
    }
}

/// What an expression evaluated in the frame at `index` can see: the names of
/// the frames around it (outermost first, so the innermost wins), the frame's
/// own bindings, and the variables it captured.
fn visible_bindings(vm: &Vm, index: usize) -> Vec<(String, Value)> {
    let mut seen: Vec<(String, Value)> = vec![];
    let mut push = |name: String, v: Value| {
        if let Some(e) = seen.iter_mut().find(|(n, _)| *n == name) {
            e.1 = v;
        } else {
            seen.push((name, v));
        }
    };
    for (i, f) in vm.frames.iter().enumerate() {
        if i > index {
            break;
        }
        for (n, v) in frame_bindings(f) {
            push(n, v);
        }
    }
    if let Some(f) = vm.frames.get(index) {
        for (i, name) in f.code.free_names.iter().enumerate() {
            if let Some(c) = f.captures.get(i) {
                push(name.to_string(), c.borrow().clone());
            }
        }
    }
    seen.retain(|(n, _)| is_identifier(n));
    seen
}

/// Whether a name can stand as a parameter of the evaluation wrapper.
fn is_identifier(n: &str) -> bool {
    !n.is_empty()
        && n.chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && n.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        && !matches!(
            n,
            "arguments"
                | "await"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "debugger"
                | "default"
                | "delete"
                | "do"
                | "else"
                | "enum"
                | "export"
                | "extends"
                | "false"
                | "finally"
                | "for"
                | "function"
                | "if"
                | "import"
                | "in"
                | "instanceof"
                | "new"
                | "null"
                | "return"
                | "super"
                | "switch"
                | "this"
                | "throw"
                | "true"
                | "try"
                | "typeof"
                | "var"
                | "void"
                | "while"
                | "with"
                | "yield"
        )
}

/// The names a frame binds, with their values.
fn frame_bindings(f: &Frame) -> Vec<(String, Value)> {
    let mut out = vec![];
    for (i, name) in f.code.local_names.iter().enumerate() {
        let n = name.to_string();
        if n.is_empty() || n.starts_with('%') {
            continue;
        }
        let v = match f.locals.get(i) {
            Some(Local::V(v)) => v.clone(),
            Some(Local::C(c)) => c.borrow().clone(),
            None => continue,
        };
        if matches!(v, Value::Empty) {
            continue;
        }
        out.push((n, v));
    }
    out
}

/// Evaluates `src` with the bindings of the frame at `depth` in scope.
pub fn eval_in_frame(vm: &mut Vm, depth: usize, src: &str) -> Result<Value, String> {
    let was = vm.debug.as_ref().map(|s| s.state.evaluating);
    if let Some(s) = vm.debug.as_mut() {
        s.state.evaluating = true;
    }
    let r = eval_inner(vm, depth, src);
    if let (Some(w), Some(s)) = (was, vm.debug.as_mut()) {
        s.state.evaluating = w;
    }
    r
}

fn eval_inner(vm: &mut Vm, depth: usize, src: &str) -> Result<Value, String> {
    let index = depth.saturating_sub(1);
    let bindings = visible_bindings(vm, index);
    let names: Vec<String> = bindings.iter().map(|(n, _)| n.clone()).collect();
    let values: Vec<Value> = bindings.into_iter().map(|(_, v)| v).collect();
    // The frame's names become the parameters of a wrapper, so the expression
    // reads them as the program does; everything else resolves globally.
    let wrapper = format!("(function ({}) {{ return ({src}\n); }})", names.join(", "));
    // `[eval]` code keeps its completion value, which is the wrapper function.
    let (code, _) = vm
        .compile_source(&wrapper, "[eval]", Some(false), &[])
        .map_err(|e| ctl_text(vm, e))?;
    let caps: Rc<[CellRef]> = Rc::from(Vec::new());
    let f = vm.make_closure(code, caps);
    let fun = vm
        .call(&Value::Obj(f), Value::Undefined, vec![])
        .map_err(|e| ctl_text(vm, e))?;
    vm.call(&fun, Value::Undefined, values)
        .map_err(|e| ctl_text(vm, e))
}

fn ctl_text(vm: &mut Vm, e: Ctl) -> String {
    match e {
        Ctl::Throw(v) | Ctl::Fatal(v) => {
            let (name, msg) = describe_error(vm, &v);
            if name.is_empty() {
                msg
            } else if msg.is_empty() {
                name
            } else {
                format!("{name}: {msg}")
            }
        }
        Ctl::Exit(c) => format!("exited with {c}"),
    }
}

/// The stopped program as the front end sees it.
pub struct View<'a, 'h> {
    vm: &'a mut Vm<'h>,
    state: &'a mut State,
}

impl View<'_, '_> {
    fn reference(&mut self, t: RefTarget) -> u64 {
        self.state.refs.push(t);
        self.state.refs.len() as u64
    }
    fn describe(&mut self, name: &str, v: &Value) -> Variable {
        let value = self
            .vm
            .inspect_default(v)
            .unwrap_or_else(|_| "<uninspectable>".into());
        let type_name = type_of(self.vm, v);
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

fn type_of(vm: &mut Vm, v: &Value) -> String {
    match v {
        Value::Obj(o) => {
            if o.is_array() {
                "array".into()
            } else if o.is_callable() {
                "function".into()
            } else {
                "object".into()
            }
        }
        _ => {
            let _ = vm;
            v.type_of().to_string()
        }
    }
}

fn child_counts(vm: &mut Vm, v: &Value) -> (u64, u64) {
    let _ = vm;
    let Value::Obj(o) = v else {
        return (0, 0);
    };
    if let Kind::Array(items) = &o.borrow().kind {
        return (0, items.len() as u64);
    }
    (o.borrow().props.len() as u64, 0)
}

impl DebugTarget for View<'_, '_> {
    fn threads(&mut self) -> Vec<Thread> {
        vec![Thread {
            id: 1,
            name: "main".into(),
        }]
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
        let index = self.state.frames[i].index;
        let locals = self.reference(RefTarget::Locals(index));
        let globals = self.reference(RefTarget::Globals);
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
            Some(RefTarget::Globals) => RefTarget::Globals,
            Some(RefTarget::Value(v)) => RefTarget::Value(v.clone()),
            None => return vec![],
        };
        let pairs: Vec<(String, Value)> = match target {
            RefTarget::Locals(fi) => match self.vm.frames.get(fi) {
                Some(f) => frame_bindings(f),
                None => vec![],
            },
            RefTarget::Globals => {
                let g = self.vm.global.clone();
                let names = own_names(&g);
                names
                    .into_iter()
                    .filter_map(|name| {
                        let v = self.vm.get_str(&Value::Obj(g.clone()), &name).ok()?;
                        Some((name, v))
                    })
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
        let depth = match self.state.frames.get(fi) {
            Some(f) => f.index + 1,
            None => return Err("no such frame".into()),
        };
        let v = eval_in_frame(self.vm, depth, expression)?;
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
            Some(RefTarget::Globals) => RefTarget::Globals,
            Some(RefTarget::Value(v)) => RefTarget::Value(v.clone()),
            None => return Err("no such variable".into()),
        };
        let depth = match &target {
            RefTarget::Locals(fi) => fi + 1,
            _ => self.state.frames.first().map(|f| f.index + 1).unwrap_or(0),
        };
        let v = eval_in_frame(self.vm, depth, value)?;
        match &target {
            RefTarget::Locals(fi) => store_binding(self.vm, *fi, name, v.clone())?,
            RefTarget::Globals => {
                let g = self.vm.global.clone();
                g.set_prop(name, v.clone(), ALL);
            }
            RefTarget::Value(container) => {
                let Value::Obj(o) = container else {
                    return Err("this value has no children to set".into());
                };
                o.set_prop(name, v.clone(), ALL);
            }
        }
        Ok(self.describe(name, &v))
    }
}

/// Writes a name the frame at `index` can see: its own binding, one it
/// captured, or one of an enclosing frame.
fn store_binding(vm: &mut Vm, index: usize, name: &str, v: Value) -> Result<(), String> {
    if let Some(f) = vm.frames.get(index) {
        for (i, n) in f.code.free_names.iter().enumerate() {
            if n.to_string() == name {
                if let Some(c) = f.captures.get(i) {
                    *c.borrow_mut() = v;
                    return Ok(());
                }
            }
        }
    }
    for i in (0..=index).rev() {
        let Some(f) = vm.frames.get_mut(i) else {
            continue;
        };
        let slot = f
            .code
            .local_names
            .iter()
            .position(|n| n.to_string() == name);
        if let Some(slot) = slot {
            match f.locals.get_mut(slot) {
                Some(Local::V(slot_v)) => *slot_v = v,
                Some(Local::C(c)) => *c.borrow_mut() = v,
                None => return Err(format!("no binding named {name}")),
            }
            return Ok(());
        }
    }
    Err(format!("no binding named {name}"))
}

/// The string-keyed own properties of an object, in insertion order.
fn own_names(o: &Obj) -> Vec<String> {
    o.borrow()
        .props
        .entries
        .iter()
        .filter_map(|(k, _)| match k {
            Key::Str(s) => Some(s.to_string()),
            _ => None,
        })
        .collect()
}

/// The children of a value, as the front end expands them.
fn children_of(vm: &mut Vm, v: &Value) -> Vec<(String, Value)> {
    let Value::Obj(o) = v else {
        return vec![];
    };
    let array_len = match &o.borrow().kind {
        Kind::Array(items) => Some(items.len()),
        _ => None,
    };
    if let Some(len) = array_len {
        return (0..len)
            .map(|i| {
                let name = i.to_string();
                let val = vm
                    .get_str(&Value::Obj(o.clone()), &name)
                    .unwrap_or(Value::Undefined);
                (name, val)
            })
            .collect();
    }
    own_names(o)
        .into_iter()
        .filter_map(|name| {
            let val = vm.get_str(&Value::Obj(o.clone()), &name).ok()?;
            Some((name, val))
        })
        .collect()
}
