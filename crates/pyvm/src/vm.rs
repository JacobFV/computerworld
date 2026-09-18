//! The bytecode interpreter: frames on a heap stack (Python-to-Python calls never
//! recurse in Rust), exceptions with tracebacks, generators as suspended frames.
use crate::ast::{BinOp, CmpOp, UnaryOp};
use crate::value::*;
use cw_script_host::ScriptHost;
use std::cell::RefCell;
use std::rc::Rc;

pub enum ErrKind {
    /// A materialized exception instance.
    Exc(Value),
    /// A builtin exception not yet instantiated.
    Lazy(&'static str, Vec<Value>),
}

pub struct PyErr {
    pub kind: ErrKind,
    /// Re-raised as-is: do not record the current frame again.
    pub reraise: bool,
    /// Uncatchable (the step budget ran out): no handler runs.
    pub fatal: bool,
}
impl PyErr {
    pub fn from_exc(v: Value, reraise: bool) -> Box<PyErr> {
        Box::new(PyErr {
            kind: ErrKind::Exc(v),
            reraise,
            fatal: false,
        })
    }
}
pub type PyResult<T> = Result<T, Box<PyErr>>;

pub fn err(cls: &'static str, msg: impl Into<String>) -> Box<PyErr> {
    Box::new(PyErr {
        kind: ErrKind::Lazy(cls, vec![Value::string(msg.into())]),
        reraise: false,
        fatal: false,
    })
}
pub fn err_args(cls: &'static str, args: Vec<Value>) -> Box<PyErr> {
    Box::new(PyErr {
        kind: ErrKind::Lazy(cls, args),
        reraise: false,
        fatal: false,
    })
}
pub fn type_err(msg: impl Into<String>) -> Box<PyErr> {
    err("TypeError", msg)
}
pub fn value_err(msg: impl Into<String>) -> Box<PyErr> {
    err("ValueError", msg)
}

#[derive(Clone, Copy)]
pub enum BlockKind {
    Finally,
    ExceptHandler,
}
#[derive(Clone, Copy)]
pub struct Block {
    pub kind: BlockKind,
    pub handler: u32,
    pub level: u32,
    pub exc_depth: u32,
}

pub enum FrameKind {
    Normal,
    /// `__init__` run inline for a class call: the instance is the call's value.
    Init(Value),
}

pub struct Frame {
    pub code: Rc<Code>,
    pub pc: usize,
    pub stack: Vec<Value>,
    pub fast: Vec<Value>,
    pub cells: Vec<Ref<Value>>,
    pub blocks: Vec<Block>,
    pub globals: Ref<Dict>,
    pub locals: Option<Ref<Dict>>,
    pub kind: FrameKind,
}

pub enum Flow {
    Continue,
    Return(Value),
    Yield(Value),
    Call(Box<Frame>),
}

pub enum Exit {
    Return(Value),
    Yield(Value, Box<Frame>),
}

pub enum GenResult {
    Yielded(Value),
    Returned(Value),
}

pub const STEP_BUDGET: u64 = 50_000_000;
pub const MAX_NATIVE_DEPTH: usize = 160;

pub struct Vm<'h> {
    pub host: &'h mut dyn ScriptHost,
    pub t: crate::builtins::Types,
    pub builtins: Ref<Dict>,
    pub modules: Ref<Dict>,
    pub stdout: String,
    pub stderr: String,
    pub stdin: String,
    pub stdin_pos: usize,
    pub fuel: u64,
    pub depth: usize,
    pub native_depth: usize,
    pub recursion_limit: usize,
    pub exc_stack: Vec<Value>,
    pub frames: Vec<Box<Frame>>,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub int_max_str_digits: usize,
    pub time_offset: i64,
    pub repr_guard: Vec<usize>,
    pub script_dir: String,
    pub sources: std::collections::HashMap<String, Rc<str>>,
    pub warnings: Vec<String>,
    pub set_finger: usize,
    pub output_limit: usize,
    pub classes: Vec<std::rc::Weak<Class>>,
    pub id_map: RefCell<IdMap<usize, (Value, usize)>>,
    pub open_files: Vec<Ref<FileObj>>,
}

impl<'h> Vm<'h> {
    // ------------------------------------------------------------------
    // Exceptions
    // ------------------------------------------------------------------

    /// Turns a lazy error into an exception instance.
    pub fn materialize(&mut self, e: &mut PyErr) -> Value {
        if let ErrKind::Lazy(cls, args) = &mut e.kind {
            let cls = self.t.exc(cls);
            let args = std::mem::take(args);
            let v = self.new_exception(&cls, args);
            e.kind = ErrKind::Exc(v);
        }
        match &e.kind {
            ErrKind::Exc(v) => v.clone(),
            _ => Value::None,
        }
    }
    pub fn new_exception(&mut self, cls: &Rc<Class>, args: Vec<Value>) -> Value {
        let inst = Instance {
            class: RefCell::new(cls.clone()),
            dict: new_ref(Dict::new()),
            native: RefCell::new(NativeData::Exc(Box::new(ExcData {
                args: Value::tuple(args),
                traceback: vec![],
                cause: None,
                context: None,
                suppress_context: false,
            }))),
        };
        Value::Instance(Rc::new(inst))
    }
    pub fn exc_value(&mut self, mut e: Box<PyErr>) -> Value {
        self.materialize(&mut e)
    }
    pub fn exc_class_of(&self, e: &PyErr) -> Option<Rc<Class>> {
        match &e.kind {
            ErrKind::Exc(Value::Instance(i)) => Some(i.class()),
            ErrKind::Lazy(name, _) => Some(self.t.exc(name)),
            _ => None,
        }
    }
    /// Does the error match `except cls`?
    pub fn err_matches(&self, e: &PyErr, cls: &str) -> bool {
        if e.fatal {
            return false;
        }
        let target = self.t.exc(cls);
        self.exc_class_of(e).is_some_and(|c| c.is_subclass(&target))
    }
    pub fn exc_data<R>(&self, v: &Value, f: impl FnOnce(&mut ExcData) -> R) -> Option<R> {
        if let Value::Instance(i) = v {
            if let NativeData::Exc(d) = &mut *i.native.borrow_mut() {
                return Some(f(d));
            }
        }
        None
    }
    fn add_traceback(&mut self, e: &mut PyErr, frame: &Frame) {
        if e.reraise {
            e.reraise = false;
            return;
        }
        if frame.code.name.starts_with("<listcomp")
            || frame.code.name.starts_with("<dictcomp")
            || frame.code.name.starts_with("<setcomp")
        {
            return;
        }
        let v = self.materialize(e);
        let pc = frame
            .pc
            .saturating_sub(1)
            .min(frame.code.pos.len().saturating_sub(1));
        let pos = frame.code.pos.get(pc).copied().unwrap_or_default();
        let entry = TbEntry {
            filename: frame.code.filename.clone(),
            name: frame.code.name.clone(),
            pos,
        };
        self.exc_data(&v, |d| d.traceback.push(entry));
    }

    // ------------------------------------------------------------------
    // Frames and the interpreter loop
    // ------------------------------------------------------------------

    pub fn charge_depth(&mut self) -> PyResult<()> {
        if self.depth >= self.recursion_limit {
            return Err(err("RecursionError", "maximum recursion depth exceeded"));
        }
        Ok(())
    }

    /// Runs `f` (and any frames it calls inline) until it returns or yields.
    pub fn execute(&mut self, f: Box<Frame>, pending: Option<Box<PyErr>>) -> PyResult<Exit> {
        if self.native_depth > MAX_NATIVE_DEPTH {
            return Err(err("RecursionError", "maximum recursion depth exceeded"));
        }
        self.native_depth += 1;
        self.depth += 1;
        let r = self.execute_inner(f, pending);
        self.native_depth -= 1;
        r
    }

    fn execute_inner(&mut self, mut f: Box<Frame>, pending: Option<Box<PyErr>>) -> PyResult<Exit> {
        let base = self.frames.len();
        let mut pending = pending;
        loop {
            let result = match pending.take() {
                Some(e) => Err(e),
                None => {
                    if self.fuel == 0 {
                        Err(Box::new(PyErr {
                            kind: ErrKind::Lazy(
                                "TimeoutError",
                                vec![Value::string(format!(
                                    "execution step limit exceeded ({STEP_BUDGET} steps); the simulated CPU budget is exhausted"
                                ))],
                            ),
                            reraise: false,
                            fatal: true,
                        }))
                    } else {
                        self.fuel -= 1;
                        let op = f.code.ops[f.pc];
                        f.pc += 1;
                        self.step(&mut f, op)
                    }
                }
            };
            match result {
                Ok(Flow::Continue) => {}
                Ok(Flow::Call(new)) => {
                    if let Err(e) = self.charge_depth() {
                        pending = Some(e);
                        continue;
                    }
                    self.depth += 1;
                    let caller = std::mem::replace(&mut f, new);
                    self.frames.push(caller);
                }
                Ok(Flow::Return(v)) => {
                    self.depth -= 1;
                    if self.frames.len() == base {
                        return Ok(Exit::Return(v));
                    }
                    let done = std::mem::replace(&mut f, self.frames.pop().unwrap());
                    match &done.kind {
                        FrameKind::Init(inst) => {
                            if !v.is_none() {
                                pending = Some(type_err(format!(
                                    "__init__() should return None, not '{}'",
                                    self.type_name(&v)
                                )));
                                continue;
                            }
                            f.stack.push(inst.clone());
                        }
                        FrameKind::Normal => f.stack.push(v),
                    }
                }
                Ok(Flow::Yield(v)) => {
                    self.depth -= 1;
                    return Ok(Exit::Yield(v, f));
                }
                Err(mut e) => {
                    if !e.reraise && !e.fatal {
                        self.set_context(&mut e);
                    }
                    loop {
                        if e.fatal {
                            self.add_traceback(&mut e, &f);
                            // Unwind everything to the base without running handlers.
                            while self.frames.len() > base {
                                self.frames.pop();
                                self.depth -= 1;
                            }
                            self.depth -= 1;
                            return Err(e);
                        }
                        self.add_traceback(&mut e, &f);
                        if self.find_handler(&mut f, &mut e) {
                            break;
                        }
                        self.depth -= 1;
                        if self.frames.len() == base {
                            return Err(e);
                        }
                        f = self.frames.pop().unwrap();
                    }
                }
            }
        }
    }

    /// Unwinds the block stack of `f` looking for a handler. On success the
    /// exception is pushed and control transfers to the handler.
    fn find_handler(&mut self, f: &mut Frame, e: &mut Box<PyErr>) -> bool {
        while let Some(b) = f.blocks.pop() {
            match b.kind {
                BlockKind::ExceptHandler => {
                    self.exc_stack.truncate(b.exc_depth as usize);
                }
                BlockKind::Finally => {
                    let exc = self.materialize(e);
                    f.stack.truncate(b.level as usize);
                    f.blocks.push(Block {
                        kind: BlockKind::ExceptHandler,
                        handler: 0,
                        level: f.stack.len() as u32,
                        exc_depth: self.exc_stack.len() as u32,
                    });
                    self.exc_stack.push(exc.clone());
                    f.stack.push(exc);
                    f.pc = b.handler as usize;
                    return true;
                }
            }
        }
        false
    }

    pub fn new_frame(
        &mut self,
        code: Rc<Code>,
        globals: Ref<Dict>,
        locals: Option<Ref<Dict>>,
    ) -> Box<Frame> {
        let ncells = code.cellvars.len() + code.freevars.len();
        let mut cells = Vec::with_capacity(ncells);
        for _ in 0..code.cellvars.len() {
            cells.push(new_ref(Value::Undefined));
        }
        Box::new(Frame {
            fast: vec![Value::Undefined; code.varnames.len()],
            stack: Vec::with_capacity(8),
            code,
            pc: 0,
            cells,
            blocks: vec![],
            globals,
            locals,
            kind: FrameKind::Normal,
        })
    }

    /// Binds arguments to a new frame for `func` (CPython's argument rules).
    pub fn bind_frame(
        &mut self,
        func: &Rc<Function>,
        mut args: Vec<Value>,
        kwargs: Vec<(Rc<str>, Value)>,
    ) -> PyResult<Box<Frame>> {
        let code = func.code.clone();
        let mut f = self.new_frame(code.clone(), func.globals.clone(), None);
        let n = code.argcount as usize;
        let kwonly = code.kwonlyargcount as usize;
        let fname = || format!("{}()", func.qualname.borrow());
        let given = args.len();
        if args.len() > n {
            if code.varargs {
                let extra = args.split_off(n);
                f.fast[n + kwonly] = Value::tuple(extra);
            } else {
                let defaults = func.defaults.borrow().len();
                let kw_given = kwargs
                    .iter()
                    .filter(|(k, _)| code.varnames[..n + kwonly].iter().any(|v| v == k))
                    .count();
                let takes = if defaults > 0 {
                    format!("from {} to {}", n - defaults, n)
                } else {
                    n.to_string()
                };
                let plural = if n == 1 && defaults == 0 { "" } else { "s" };
                let was = if given == 1 { "was" } else { "were" };
                let extra_kw = if kw_given > 0 {
                    format!(
                        " positional argument{} (and {} keyword-only argument{})",
                        if given == 1 { "" } else { "s" },
                        kw_given,
                        if kw_given == 1 { "" } else { "s" }
                    )
                } else {
                    String::new()
                };
                if kw_given > 0 {
                    return Err(type_err(format!(
                        "{} takes {takes} positional argument{plural} but {given}{extra_kw} were given",
                        fname()
                    )));
                }
                return Err(type_err(format!(
                    "{} takes {takes} positional argument{plural} but {given} {was} given",
                    fname()
                )));
            }
        } else if code.varargs {
            f.fast[n + kwonly] = Value::tuple(vec![]);
        }
        for (i, a) in args.into_iter().enumerate() {
            f.fast[i] = a;
        }
        let kw_slot = n + kwonly + code.varargs as usize;
        let mut kwdict: Option<Dict> = if code.varkw { Some(Dict::new()) } else { None };
        let posonly = code.posonlyargcount as usize;
        let mut posonly_passed: Vec<Rc<str>> = vec![];
        for (k, v) in kwargs {
            let idx = code.varnames[posonly..n + kwonly]
                .iter()
                .position(|p| **p == *k)
                .map(|i| i + posonly);
            match idx {
                Some(i) => {
                    if !f.fast[i].is_undefined() {
                        return Err(type_err(format!(
                            "{} got multiple values for argument '{}'",
                            fname(),
                            k
                        )));
                    }
                    f.fast[i] = v;
                }
                None => match &mut kwdict {
                    Some(d) => {
                        let key = Rc::new(PyStr::new(k.to_string()));
                        d.set_pystr(key, v);
                    }
                    None => {
                        if code.varnames[..posonly].iter().any(|p| **p == *k) {
                            posonly_passed.push(k.clone());
                            continue;
                        }
                        return Err(type_err(format!(
                            "{} got an unexpected keyword argument '{}'",
                            fname(),
                            k
                        )));
                    }
                },
            }
        }
        if !posonly_passed.is_empty() {
            return Err(type_err(format!(
                "{} got some positional-only arguments passed as keyword arguments: '{}'",
                fname(),
                posonly_passed.join(", ")
            )));
        }
        if let Some(d) = kwdict {
            f.fast[kw_slot] = Value::dict(d);
        }
        // Defaults for missing positionals.
        let defaults = func.defaults.borrow();
        let first_default = n - defaults.len();
        let mut missing = vec![];
        for i in 0..n {
            if f.fast[i].is_undefined() {
                if i >= first_default {
                    f.fast[i] = defaults[i - first_default].clone();
                } else {
                    missing.push(code.varnames[i].clone());
                }
            }
        }
        drop(defaults);
        if !missing.is_empty() {
            return Err(type_err(format!(
                "{} missing {} required positional argument{}: {}",
                fname(),
                missing.len(),
                if missing.len() == 1 { "" } else { "s" },
                name_list(&missing)
            )));
        }
        let kwdefaults = func.kwdefaults.borrow();
        let mut missing = vec![];
        for i in n..n + kwonly {
            if f.fast[i].is_undefined() {
                match kwdefaults.iter().find(|(k, _)| **k == *code.varnames[i]) {
                    Some((_, v)) => f.fast[i] = v.clone(),
                    None => missing.push(code.varnames[i].clone()),
                }
            }
        }
        drop(kwdefaults);
        if !missing.is_empty() {
            return Err(type_err(format!(
                "{} missing {} required keyword-only argument{}: {}",
                fname(),
                missing.len(),
                if missing.len() == 1 { "" } else { "s" },
                name_list(&missing)
            )));
        }
        // Cells: arguments captured by inner functions start with the argument.
        for (ci, arg) in code.cell2arg.iter().enumerate() {
            if let Some(a) = arg {
                let v = std::mem::replace(&mut f.fast[*a as usize], Value::Undefined);
                *f.cells[ci].borrow_mut() = v;
            }
        }
        f.cells.extend(func.closure.iter().cloned());
        Ok(f)
    }

    /// Calls any callable and returns its value.
    pub fn call(&mut self, callee: &Value, args: Vec<Value>) -> PyResult<Value> {
        self.call_kw(callee, args, vec![])
    }

    pub fn call_kw(
        &mut self,
        callee: &Value,
        mut args: Vec<Value>,
        kwargs: Vec<(Rc<str>, Value)>,
    ) -> PyResult<Value> {
        match callee {
            Value::Func(func) => {
                let f = self.bind_frame(func, args, kwargs)?;
                self.run_function_frame(func, f)
            }
            Value::Builtin(b) => {
                if self.native_depth > MAX_NATIVE_DEPTH {
                    return Err(err("RecursionError", "maximum recursion depth exceeded"));
                }
                self.native_depth += 1;
                let r = (b.func)(self, Args { args, kwargs });
                self.native_depth -= 1;
                r
            }
            Value::Method(m) => {
                args.insert(0, m.0.clone());
                self.call_kw(&m.1, args, kwargs)
            }
            Value::Class(cls) => self.call_class(cls, args, kwargs),
            Value::StaticMethod(f) => self.call_kw(f, args, kwargs),
            _ => {
                if let Some(m) = self.lookup_special(callee, "__call__") {
                    args.insert(0, callee.clone());
                    return self.call_kw(&m, args, kwargs);
                }
                Err(type_err(format!(
                    "'{}' object is not callable",
                    self.type_name(callee)
                )))
            }
        }
    }

    pub fn run_function_frame(&mut self, func: &Rc<Function>, f: Box<Frame>) -> PyResult<Value> {
        if func.code.is_generator || func.code.is_coroutine {
            return Ok(self.make_generator(func, f));
        }
        self.charge_depth()?;
        match self.execute(f, None)? {
            Exit::Return(v) => Ok(v),
            Exit::Yield(..) => Err(err("SystemError", "unexpected yield")),
        }
    }

    pub fn make_generator(&mut self, func: &Rc<Function>, f: Box<Frame>) -> Value {
        Value::Gen(new_ref(Generator {
            frame: Some(f),
            state: GenState::Created,
            name: func.name.borrow().clone(),
            qualname: func.qualname.borrow().clone(),
            is_coroutine: func.code.is_coroutine,
        }))
    }

    /// Resumes a generator with a sent value or a thrown exception.
    pub fn gen_resume(
        &mut self,
        gen: &Ref<Generator>,
        send: Value,
        throw: Option<Box<PyErr>>,
    ) -> PyResult<GenResult> {
        let (mut frame, started) = {
            let mut g = gen.borrow_mut();
            match g.state {
                GenState::Running => {
                    return Err(value_err("generator already executing"));
                }
                GenState::Done => {
                    drop(g);
                    if let Some(e) = throw {
                        return Err(e);
                    }
                    return Ok(GenResult::Returned(Value::None));
                }
                GenState::Created => {
                    if throw.is_none() && !send.is_none() {
                        return Err(type_err(
                            "can't send non-None value to a just-started generator",
                        ));
                    }
                }
                GenState::Suspended => {}
            }
            let started = matches!(g.state, GenState::Suspended);
            g.state = GenState::Running;
            (g.frame.take().unwrap(), started)
        };
        let mut pending = throw;
        if started {
            // A throw into a frame delegating with `yield from` goes to the delegate.
            if let (Some(_), Some(Op::YieldFrom)) = (&pending, frame.code.ops.get(frame.pc)) {
                if let Some(Value::Gen(sub)) = frame.stack.last().cloned() {
                    let e = pending.take().unwrap();
                    match self.gen_resume(&sub, Value::None, Some(e)) {
                        Ok(GenResult::Yielded(y)) => {
                            let mut g = gen.borrow_mut();
                            g.frame = Some(frame);
                            g.state = GenState::Suspended;
                            return Ok(GenResult::Yielded(y));
                        }
                        Ok(GenResult::Returned(r)) => {
                            frame.stack.pop();
                            frame.stack.push(r);
                            frame.pc += 1;
                        }
                        Err(e) => {
                            frame.pc += 1;
                            pending = Some(e);
                        }
                    }
                }
            }
            if pending.is_none() {
                frame.stack.push(send);
            }
        } else if pending.is_some() {
            // Thrown before starting: raises at the first line without running.
            gen.borrow_mut().state = GenState::Done;
            return Err(pending.unwrap());
        }
        if let Err(e) = self.charge_depth() {
            let mut g = gen.borrow_mut();
            g.frame = Some(frame);
            g.state = if started {
                GenState::Suspended
            } else {
                GenState::Created
            };
            return Err(e);
        }
        let r = self.execute(frame, pending);
        let mut g = gen.borrow_mut();
        match r {
            Ok(Exit::Yield(v, frame)) => {
                g.frame = Some(frame);
                g.state = GenState::Suspended;
                Ok(GenResult::Yielded(v))
            }
            Ok(Exit::Return(v)) => {
                g.state = GenState::Done;
                Ok(GenResult::Returned(v))
            }
            Err(mut e) => {
                g.state = GenState::Done;
                let is_coro = g.is_coroutine;
                drop(g);
                if self.err_matches(&e, "StopIteration") {
                    let cause = self.materialize(&mut e);
                    let what = if is_coro { "coroutine" } else { "generator" };
                    let mut ne = err("RuntimeError", format!("{what} raised StopIteration"));
                    let nv = self.materialize(&mut ne);
                    self.exc_data(&nv, |d| {
                        d.cause = Some(cause.clone());
                        d.context = Some(cause);
                        d.suppress_context = true;
                    });
                    return Err(ne);
                }
                Err(e)
            }
        }
    }

    pub fn gen_close(&mut self, gen: &Ref<Generator>) -> PyResult<()> {
        if !matches!(gen.borrow().state, GenState::Suspended) {
            gen.borrow_mut().state = GenState::Done;
            return Ok(());
        }
        let e = err_args("GeneratorExit", vec![]);
        match self.gen_resume(gen, Value::None, Some(e)) {
            Ok(GenResult::Yielded(_)) => {
                Err(err("RuntimeError", "generator ignored GeneratorExit"))
            }
            Ok(GenResult::Returned(_)) => Ok(()),
            Err(e) => {
                if self.err_matches(&e, "GeneratorExit") || self.err_matches(&e, "StopIteration") {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // One instruction
    // ------------------------------------------------------------------

    #[inline(always)]
    fn pop(f: &mut Frame) -> Value {
        f.stack.pop().unwrap_or(Value::None)
    }

    fn step(&mut self, f: &mut Box<Frame>, op: Op) -> PyResult<Flow> {
        match op {
            Op::Nop => {}
            Op::LoadConst(i) => {
                let v = f.code.consts[i as usize].clone();
                f.stack.push(v);
            }
            Op::LoadFast(i) => {
                let v = &f.fast[i as usize];
                if v.is_undefined() {
                    return Err(self.unbound_local(f, i));
                }
                let v = v.clone();
                f.stack.push(v);
            }
            Op::StoreFast(i) => {
                let v = Self::pop(f);
                f.fast[i as usize] = v;
            }
            Op::DeleteFast(i) => {
                if f.fast[i as usize].is_undefined() {
                    return Err(self.unbound_local(f, i));
                }
                f.fast[i as usize] = Value::Undefined;
            }
            Op::LoadDeref(i) => {
                let v = f.cells[i as usize].borrow().clone();
                if v.is_undefined() {
                    return Err(self.unbound_deref(f, i));
                }
                f.stack.push(v);
            }
            Op::LoadClassDeref(i) => {
                let name = self.cell_name(f, i);
                let found = f.locals.as_ref().and_then(|l| l.borrow().get_str(&name));
                match found {
                    Some(v) => f.stack.push(v),
                    None => {
                        let v = f.cells[i as usize].borrow().clone();
                        if v.is_undefined() {
                            return Err(self.unbound_deref(f, i));
                        }
                        f.stack.push(v);
                    }
                }
            }
            Op::StoreDeref(i) => {
                let v = Self::pop(f);
                *f.cells[i as usize].borrow_mut() = v;
            }
            Op::DeleteDeref(i) => {
                *f.cells[i as usize].borrow_mut() = Value::Undefined;
            }
            Op::LoadClosure(i) => {
                let c = f.cells[i as usize].clone();
                f.stack.push(Value::Cell(c));
            }
            Op::LoadGlobal(i) => {
                let name = &f.code.names[i as usize];
                let v = f.globals.borrow().get_pystr(name);
                match v {
                    Some(v) => f.stack.push(v),
                    None => {
                        let b = self.builtins.borrow().get_pystr(name);
                        match b {
                            Some(v) => f.stack.push(v),
                            None => return Err(self.name_error(&name.s.clone(), f)),
                        }
                    }
                }
            }
            Op::StoreGlobal(i) => {
                let v = Self::pop(f);
                let name = f.code.names[i as usize].clone();
                f.globals.borrow_mut().set_pystr(name, v);
            }
            Op::DeleteGlobal(i) => {
                let name = f.code.names[i as usize].clone();
                if f.globals.borrow_mut().del_str(&name.s).is_none() {
                    return Err(self.name_error(&name.s, f));
                }
            }
            Op::LoadName(i) => {
                let name = f.code.names[i as usize].clone();
                let local = f.locals.as_ref().and_then(|l| l.borrow().get_pystr(&name));
                let v = match local {
                    Some(v) => Some(v),
                    None => {
                        let g = f.globals.borrow().get_pystr(&name);
                        match g {
                            Some(v) => Some(v),
                            None => self.builtins.borrow().get_pystr(&name),
                        }
                    }
                };
                match v {
                    Some(v) => f.stack.push(v),
                    None => return Err(self.name_error(&name.s, f)),
                }
            }
            Op::StoreName(i) => {
                let v = Self::pop(f);
                let name = f.code.names[i as usize].clone();
                match &f.locals {
                    Some(l) => l.borrow_mut().set_pystr(name, v),
                    None => f.globals.borrow_mut().set_pystr(name, v),
                }
            }
            Op::DeleteName(i) => {
                let name = f.code.names[i as usize].clone();
                let removed = match &f.locals {
                    Some(l) => l.borrow_mut().del_str(&name.s),
                    None => f.globals.borrow_mut().del_str(&name.s),
                };
                if removed.is_none() {
                    return Err(self.name_error(&name.s, f));
                }
            }
            Op::LoadAttr(i) => {
                let obj = Self::pop(f);
                let name = f.code.names[i as usize].clone();
                let v = self.getattr(&obj, &name)?;
                f.stack.push(v);
            }
            Op::StoreAttr(i) => {
                let obj = Self::pop(f);
                let v = Self::pop(f);
                let name = f.code.names[i as usize].clone();
                self.setattr(&obj, &name, v)?;
            }
            Op::DeleteAttr(i) => {
                let obj = Self::pop(f);
                let name = f.code.names[i as usize].clone();
                self.delattr(&obj, &name)?;
            }
            Op::LoadMethod(i) => {
                let obj = Self::pop(f);
                let name = f.code.names[i as usize].clone();
                // Plain functions found on the type are called with the object as
                // the first argument, without allocating a bound method.
                if let Value::Instance(inst) = &obj {
                    let cls = inst.class();
                    if let Some(attr) = cls.lookup_pystr(&name) {
                        if matches!(attr, Value::Func(_))
                            && inst.dict.borrow().get_pystr(&name).is_none()
                        {
                            f.stack.push(attr);
                            f.stack.push(obj);
                            return Ok(Flow::Continue);
                        }
                    }
                }
                let v = self.getattr(&obj, &name)?;
                f.stack.push(v);
                f.stack.push(Value::Undefined);
            }
            Op::CallMethod(n) => {
                let n = n as usize;
                let at = f.stack.len() - n;
                let mut args: Vec<Value> = f.stack.split_off(at);
                let slf = Self::pop(f);
                let func = Self::pop(f);
                if !slf.is_undefined() {
                    args.insert(0, slf);
                }
                return self.call_inline(f, func, args, vec![]);
            }
            Op::Call(n) => {
                let at = f.stack.len() - n as usize;
                let args: Vec<Value> = f.stack.split_off(at);
                let func = Self::pop(f);
                return self.call_inline(f, func, args, vec![]);
            }
            Op::CallKw(n) => {
                let names = Self::pop(f);
                let Value::Tuple(names) = names else {
                    return Err(err("SystemError", "bad keyword names"));
                };
                let at = f.stack.len() - n as usize;
                let mut args: Vec<Value> = f.stack.split_off(at);
                let kwvals = args.split_off(args.len() - names.len());
                let kwargs = names
                    .iter()
                    .zip(kwvals)
                    .map(|(k, v)| {
                        let k: Rc<str> = match k {
                            Value::Str(s) => s.s.as_str().into(),
                            _ => "".into(),
                        };
                        (k, v)
                    })
                    .collect();
                let func = Self::pop(f);
                return self.call_inline(f, func, args, kwargs);
            }
            Op::CallEx(has_kw) => {
                let kwargs = if has_kw {
                    let d = Self::pop(f);
                    self.kwargs_from(&d)?
                } else {
                    vec![]
                };
                let args = Self::pop(f);
                let args = match args {
                    Value::Tuple(t) => (*t).clone(),
                    other => self.iterate(&other)?,
                };
                let func = Self::pop(f);
                return self.call_inline(f, func, args, kwargs);
            }
            Op::BinarySubscr => {
                let idx = Self::pop(f);
                let obj = Self::pop(f);
                // Fast path: list[int].
                if let (Value::List(l), Value::Int(i)) = (&obj, &idx) {
                    let l = l.borrow();
                    let n = l.len() as i64;
                    let j = if *i < 0 { i + n } else { *i };
                    if j >= 0 && j < n {
                        let v = l[j as usize].clone();
                        drop(l);
                        f.stack.push(v);
                        return Ok(Flow::Continue);
                    }
                }
                let v = self.getitem(&obj, &idx)?;
                f.stack.push(v);
            }
            Op::StoreSubscr => {
                let idx = Self::pop(f);
                let obj = Self::pop(f);
                let v = Self::pop(f);
                self.setitem(&obj, &idx, v)?;
            }
            Op::DeleteSubscr => {
                let idx = Self::pop(f);
                let obj = Self::pop(f);
                self.delitem(&obj, &idx)?;
            }
            Op::Binary(op) => {
                let b = Self::pop(f);
                let a = Self::pop(f);
                if let (Value::Int(x), Value::Int(y)) = (&a, &b) {
                    let r = match op {
                        BinOp::Add => x.checked_add(*y),
                        BinOp::Sub => x.checked_sub(*y),
                        BinOp::Mul => x.checked_mul(*y),
                        _ => None,
                    };
                    if let Some(r) = r {
                        f.stack.push(Value::Int(r));
                        return Ok(Flow::Continue);
                    }
                }
                let v = self.binary_op(&a, &b, op)?;
                f.stack.push(v);
            }
            Op::Inplace(op) => {
                let b = Self::pop(f);
                let a = Self::pop(f);
                if let (Value::Int(x), Value::Int(y)) = (&a, &b) {
                    let r = match op {
                        BinOp::Add => x.checked_add(*y),
                        BinOp::Sub => x.checked_sub(*y),
                        _ => None,
                    };
                    if let Some(r) = r {
                        f.stack.push(Value::Int(r));
                        return Ok(Flow::Continue);
                    }
                }
                let v = self.inplace_op(&a, &b, op)?;
                f.stack.push(v);
            }
            Op::Unary(op) => {
                let a = Self::pop(f);
                let v = self.unary_op(&a, op)?;
                f.stack.push(v);
            }
            Op::Compare(op) => {
                let b = Self::pop(f);
                let a = Self::pop(f);
                if let (Value::Int(x), Value::Int(y)) = (&a, &b) {
                    let r = match op {
                        CmpOp::Lt => Some(x < y),
                        CmpOp::LtE => Some(x <= y),
                        CmpOp::Gt => Some(x > y),
                        CmpOp::GtE => Some(x >= y),
                        CmpOp::Eq => Some(x == y),
                        CmpOp::NotEq => Some(x != y),
                        _ => None,
                    };
                    if let Some(r) = r {
                        f.stack.push(Value::Bool(r));
                        return Ok(Flow::Continue);
                    }
                }
                let v = self.compare_op(&a, &b, op)?;
                f.stack.push(v);
            }
            Op::Pop => {
                f.stack.pop();
            }
            Op::Dup => {
                let v = f.stack.last().cloned().unwrap_or(Value::None);
                f.stack.push(v);
            }
            Op::DupTwo => {
                let n = f.stack.len();
                let a = f.stack[n - 2].clone();
                let b = f.stack[n - 1].clone();
                f.stack.push(a);
                f.stack.push(b);
            }
            Op::Rot2 => {
                let n = f.stack.len();
                f.stack.swap(n - 1, n - 2);
            }
            Op::Rot3 => {
                let v = Self::pop(f);
                let n = f.stack.len();
                f.stack.insert(n - 2, v);
            }
            Op::Rot4 => {
                let v = Self::pop(f);
                let n = f.stack.len();
                f.stack.insert(n - 3, v);
            }
            Op::BuildTuple(n) => {
                let at = f.stack.len() - n as usize;
                let items = f.stack.split_off(at);
                f.stack.push(Value::tuple(items));
            }
            Op::BuildList(n) => {
                let at = f.stack.len() - n as usize;
                let items = f.stack.split_off(at);
                f.stack.push(Value::list(items));
            }
            Op::BuildSet(n) => {
                let at = f.stack.len() - n as usize;
                let items = f.stack.split_off(at);
                let s = new_ref(SetData::new());
                for it in items {
                    self.set_add(&s, it)?;
                }
                f.stack.push(Value::Set(s));
            }
            Op::BuildMap(n) => {
                let at = f.stack.len() - 2 * n as usize;
                let items = f.stack.split_off(at);
                let d = new_ref(Dict::new());
                let mut it = items.into_iter();
                while let (Some(k), Some(v)) = (it.next(), it.next()) {
                    self.dict_set(&d, k, v)?;
                }
                f.stack.push(Value::Dict(d));
            }
            Op::BuildString(n) => {
                let at = f.stack.len() - n as usize;
                let items = f.stack.split_off(at);
                let mut s = String::new();
                for it in items {
                    if let Value::Str(p) = it {
                        s.push_str(&p.s);
                    }
                }
                f.stack.push(Value::string(s));
            }
            Op::BuildSlice(n) => {
                let step = if n == 3 { Self::pop(f) } else { Value::None };
                let stop = Self::pop(f);
                let start = Self::pop(f);
                f.stack.push(Value::Slice(Rc::new([start, stop, step])));
            }
            Op::ListAppend(d) => {
                let v = Self::pop(f);
                let n = f.stack.len();
                if let Value::List(l) = &f.stack[n - d as usize] {
                    l.borrow_mut().push(v);
                }
            }
            Op::SetAdd(d) => {
                let v = Self::pop(f);
                let n = f.stack.len();
                if let Value::Set(s) = f.stack[n - d as usize].clone() {
                    self.set_add(&s, v)?;
                }
            }
            Op::MapAdd(d) => {
                let v = Self::pop(f);
                let k = Self::pop(f);
                let n = f.stack.len();
                if let Value::Dict(m) = f.stack[n - d as usize].clone() {
                    self.dict_set(&m, k, v)?;
                }
            }
            Op::ListExtend(d) => {
                let v = Self::pop(f);
                let items = self.iterate(&v).map_err(|e| {
                    if self.err_matches(&e, "TypeError") {
                        type_err(format!(
                            "Value after * must be an iterable, not {}",
                            self.type_name(&v)
                        ))
                    } else {
                        e
                    }
                })?;
                let n = f.stack.len();
                if let Value::List(l) = &f.stack[n - d as usize] {
                    l.borrow_mut().extend(items);
                }
            }
            Op::SetUpdate(d) => {
                let v = Self::pop(f);
                let items = self.iterate(&v)?;
                let n = f.stack.len();
                if let Value::Set(s) = f.stack[n - d as usize].clone() {
                    for it in items {
                        self.set_add(&s, it)?;
                    }
                }
            }
            Op::DictUpdate(d) => {
                let v = Self::pop(f);
                let n = f.stack.len();
                if let Value::Dict(m) = f.stack[n - d as usize].clone() {
                    let items = self.mapping_items(&v).map_err(|_| {
                        type_err(format!("'{}' object is not a mapping", self.type_name(&v)))
                    })?;
                    for (k, val) in items {
                        self.dict_set(&m, k, val)?;
                    }
                }
            }
            Op::DictMerge(d) => {
                let v = Self::pop(f);
                let n = f.stack.len();
                if let Value::Dict(m) = f.stack[n - d as usize].clone() {
                    let items = self.mapping_items(&v).map_err(|_| {
                        type_err(format!(
                            "argument after ** must be a mapping, not {}",
                            self.type_name(&v)
                        ))
                    })?;
                    for (k, val) in items {
                        if self.dict_get(&m, &k)?.is_some() {
                            let ks = self.str_of(&k)?;
                            return Err(type_err(format!(
                                "got multiple values for keyword argument '{ks}'"
                            )));
                        }
                        self.dict_set(&m, k, val)?;
                    }
                }
            }
            Op::ListToTuple => {
                let v = Self::pop(f);
                if let Value::List(l) = v {
                    let items = std::mem::take(&mut *l.borrow_mut());
                    f.stack.push(Value::tuple(items));
                }
            }
            Op::UnpackSequence(n) => {
                let v = Self::pop(f);
                let items = match &v {
                    Value::Tuple(t) if t.len() == n as usize => (**t).clone(),
                    Value::List(l) if l.borrow().len() == n as usize => l.borrow().clone(),
                    _ => self.unpack_iter(&v, n as usize)?,
                };
                for it in items.into_iter().rev() {
                    f.stack.push(it);
                }
            }
            Op::UnpackEx(spec) => {
                let before = (spec & 0xff) as usize;
                let after = (spec >> 8) as usize;
                let v = Self::pop(f);
                let items = self.iterate(&v).map_err(|e| {
                    if self.err_matches(&e, "TypeError") {
                        type_err(format!(
                            "cannot unpack non-iterable {} object",
                            self.type_name(&v)
                        ))
                    } else {
                        e
                    }
                })?;
                if items.len() < before + after {
                    return Err(value_err(format!(
                        "not enough values to unpack (expected at least {}, got {})",
                        before + after,
                        items.len()
                    )));
                }
                let mut items = items;
                let tail = items.split_off(items.len() - after);
                let mid = items.split_off(before);
                for it in tail.into_iter().rev() {
                    f.stack.push(it);
                }
                f.stack.push(Value::list(mid));
                for it in items.into_iter().rev() {
                    f.stack.push(it);
                }
            }
            Op::Jump(t) => f.pc = t as usize,
            Op::PopJumpIfFalse(t) => {
                let v = Self::pop(f);
                let b = match v {
                    Value::Bool(b) => b,
                    _ => self.truthy(&v)?,
                };
                if !b {
                    f.pc = t as usize;
                }
            }
            Op::PopJumpIfTrue(t) => {
                let v = Self::pop(f);
                let b = match v {
                    Value::Bool(b) => b,
                    _ => self.truthy(&v)?,
                };
                if b {
                    f.pc = t as usize;
                }
            }
            Op::JumpIfFalseOrPop(t) => {
                let v = f.stack.last().cloned().unwrap_or(Value::None);
                if !self.truthy(&v)? {
                    f.pc = t as usize;
                } else {
                    f.stack.pop();
                }
            }
            Op::JumpIfTrueOrPop(t) => {
                let v = f.stack.last().cloned().unwrap_or(Value::None);
                if self.truthy(&v)? {
                    f.pc = t as usize;
                } else {
                    f.stack.pop();
                }
            }
            Op::GetIter => {
                let v = Self::pop(f);
                let it = self.get_iter(&v)?;
                f.stack.push(it);
            }
            Op::ForIter(t) => {
                let it = f.stack.last().cloned().unwrap_or(Value::None);
                // Fast paths for the common iterators.
                if let Value::Iter(cell) = &it {
                    let mut st = cell.borrow_mut();
                    match &mut *st {
                        IterObj::Range {
                            cur,
                            step,
                            remaining,
                        } => {
                            if *remaining <= 0 {
                                drop(st);
                                f.stack.pop();
                                f.pc = t as usize;
                            } else {
                                let v = *cur;
                                *cur = cur.wrapping_add(*step);
                                *remaining -= 1;
                                drop(st);
                                f.stack.push(Value::Int(v));
                            }
                            return Ok(Flow::Continue);
                        }
                        IterObj::Seq {
                            seq: Value::List(l),
                            idx,
                        } => {
                            let l = l.borrow();
                            if *idx < l.len() {
                                let v = l[*idx].clone();
                                *idx += 1;
                                drop(l);
                                drop(st);
                                f.stack.push(v);
                            } else {
                                drop(l);
                                *st = IterObj::Done;
                                drop(st);
                                f.stack.pop();
                                f.pc = t as usize;
                            }
                            return Ok(Flow::Continue);
                        }
                        _ => {}
                    }
                }
                match self.next(&it)? {
                    Some(v) => f.stack.push(v),
                    None => {
                        f.stack.pop();
                        f.pc = t as usize;
                    }
                }
            }
            Op::Return => {
                let v = Self::pop(f);
                return Ok(Flow::Return(v));
            }
            Op::SetupFinally(h) => {
                f.blocks.push(Block {
                    kind: BlockKind::Finally,
                    handler: h,
                    level: f.stack.len() as u32,
                    exc_depth: 0,
                });
            }
            Op::PopBlock => {
                f.blocks.pop();
            }
            Op::PopExcept => {
                // Pop the ExceptHandler block and the exception below any kept values.
                if let Some(b) = f.blocks.pop() {
                    self.exc_stack.truncate(b.exc_depth as usize);
                    let level = b.level as usize;
                    if level < f.stack.len() {
                        f.stack.remove(level);
                    }
                }
            }
            Op::Reraise => {
                let exc = Self::pop(f);
                if let Some(b) = f.blocks.pop() {
                    self.exc_stack.truncate(b.exc_depth as usize);
                }
                return Err(PyErr::from_exc(exc, true));
            }
            Op::Raise(n) => return Err(self.do_raise(f, n)),
            Op::JumpIfNotExcMatch(t) => {
                let cls = Self::pop(f);
                let exc = Self::pop(f);
                if !self.exception_matches(&exc, &cls)? {
                    f.pc = t as usize;
                }
            }
            Op::SetupWith(h) => {
                let mgr = Self::pop(f);
                let enter = self.lookup_special(&mgr, "__enter__");
                let exit = self.lookup_special(&mgr, "__exit__");
                let (Some(enter), Some(exit)) = (enter, exit) else {
                    let tn = self.type_name(&mgr);
                    let missing = if self.lookup_special(&mgr, "__enter__").is_none() {
                        ""
                    } else {
                        " (missed __exit__ method)"
                    };
                    return Err(type_err(format!(
                        "'{tn}' object does not support the context manager protocol{missing}"
                    )));
                };
                let exit_bound = self.bind_method(&exit, &mgr);
                f.stack.push(exit_bound);
                f.blocks.push(Block {
                    kind: BlockKind::Finally,
                    handler: h,
                    level: f.stack.len() as u32,
                    exc_depth: 0,
                });
                let r = self.call(&enter, vec![mgr])?;
                f.stack.push(r);
            }
            Op::WithExit => {
                let exit = Self::pop(f);
                self.call(&exit, vec![Value::None, Value::None, Value::None])?;
            }
            Op::WithExceptStart => {
                let n = f.stack.len();
                let exc = f.stack[n - 1].clone();
                let exit = f.stack[n - 2].clone();
                let typ = self.type_of(&exc);
                let tb = Value::None;
                let r = self.call(&exit, vec![Value::Class(typ), exc, tb])?;
                f.stack.push(r);
            }
            Op::Yield => {
                let v = Self::pop(f);
                return Ok(Flow::Yield(v));
            }
            Op::YieldFrom => {
                let v = Self::pop(f);
                let sub = f.stack.last().cloned().unwrap_or(Value::None);
                let r = match &sub {
                    Value::Gen(g) => self.gen_resume(g, v, None)?,
                    _ => {
                        if v.is_none() {
                            match self.next(&sub)? {
                                Some(y) => GenResult::Yielded(y),
                                None => GenResult::Returned(Value::None),
                            }
                        } else {
                            let send = self.getattr_str(&sub, "send")?;
                            match self.call(&send, vec![v]) {
                                Ok(y) => GenResult::Yielded(y),
                                Err(e) if self.err_matches(&e, "StopIteration") => {
                                    let ev = self.exc_value(e);
                                    GenResult::Returned(self.stop_value(&ev))
                                }
                                Err(e) => return Err(e),
                            }
                        }
                    }
                };
                match r {
                    GenResult::Yielded(y) => {
                        f.pc -= 1;
                        return Ok(Flow::Yield(y));
                    }
                    GenResult::Returned(r) => {
                        f.stack.pop();
                        f.stack.push(r);
                    }
                }
            }
            Op::GetYieldFromIter => {
                let v = Self::pop(f);
                let it = match &v {
                    Value::Gen(_) => v,
                    _ => self.get_iter(&v)?,
                };
                f.stack.push(it);
            }
            Op::GetAwaitable => {
                let v = Self::pop(f);
                let it = match &v {
                    Value::Gen(g) if g.borrow().is_coroutine => v,
                    _ => match self.lookup_special(&v, "__await__") {
                        Some(m) => self.call(&m, vec![v.clone()])?,
                        None => {
                            return Err(type_err(format!(
                                "object {} can't be used in 'await' expression",
                                self.type_name(&v)
                            )))
                        }
                    },
                };
                f.stack.push(it);
            }
            Op::MakeFunction(flags) => {
                let qualname = Self::pop(f);
                let code = Self::pop(f);
                let closure = if flags & 8 != 0 {
                    match Self::pop(f) {
                        Value::Tuple(t) => t
                            .iter()
                            .filter_map(|c| match c {
                                Value::Cell(c) => Some(c.clone()),
                                _ => None,
                            })
                            .collect(),
                        _ => vec![],
                    }
                } else {
                    vec![]
                };
                let annotations = if flags & 4 != 0 {
                    Self::pop(f)
                } else {
                    Value::None
                };
                let kwdefaults = if flags & 2 != 0 {
                    match Self::pop(f) {
                        Value::Dict(d) => d
                            .borrow()
                            .iter()
                            .map(|(k, v)| {
                                let k: Rc<str> = match k {
                                    Value::Str(s) => s.s.as_str().into(),
                                    _ => "".into(),
                                };
                                (k, v.clone())
                            })
                            .collect(),
                        _ => vec![],
                    }
                } else {
                    vec![]
                };
                let defaults = if flags & 1 != 0 {
                    match Self::pop(f) {
                        Value::Tuple(t) => (*t).clone(),
                        _ => vec![],
                    }
                } else {
                    vec![]
                };
                let Value::Code(code) = code else {
                    return Err(err("SystemError", "MAKE_FUNCTION without code"));
                };
                let qualname: Rc<str> = match qualname {
                    Value::Str(s) => s.s.as_str().into(),
                    _ => code.name.clone(),
                };
                let module = f
                    .globals
                    .borrow()
                    .get_str("__name__")
                    .unwrap_or(Value::None);
                let doc = match &code.docstring {
                    Some(d) => Value::str(d),
                    None => Value::None,
                };
                let func = Function {
                    name: RefCell::new(code.name.clone()),
                    qualname: RefCell::new(qualname),
                    code,
                    globals: f.globals.clone(),
                    defaults: RefCell::new(defaults),
                    kwdefaults: RefCell::new(kwdefaults),
                    closure,
                    dict: new_ref(Dict::new()),
                    module,
                    doc: RefCell::new(doc),
                    annotations: RefCell::new(annotations),
                };
                f.stack.push(Value::Func(Rc::new(func)));
            }
            Op::LoadBuildClass => {
                let v = self
                    .builtins
                    .borrow()
                    .get_str("__build_class__")
                    .unwrap_or(Value::None);
                f.stack.push(v);
            }
            Op::ImportName(i) => {
                let fromlist = Self::pop(f);
                let level = Self::pop(f);
                let name = f.code.names[i as usize].s.clone();
                let level = match level {
                    Value::Int(l) => l as usize,
                    _ => 0,
                };
                let m = self.import(&name, &fromlist, level, &f.globals)?;
                f.stack.push(m);
            }
            Op::ImportFrom(i) => {
                let m = f.stack.last().cloned().unwrap_or(Value::None);
                let name = f.code.names[i as usize].clone();
                let v = self.import_from(&m, &name)?;
                f.stack.push(v);
            }
            Op::ImportStar => {
                let m = Self::pop(f);
                self.import_star(&m, f)?;
            }
            Op::FormatValue(flags) => {
                let spec = if flags & 4 != 0 {
                    Some(Self::pop(f))
                } else {
                    None
                };
                let v = Self::pop(f);
                let v = match flags & 3 {
                    1 => Value::string(self.str_of(&v)?),
                    2 => Value::string(self.repr(&v)?),
                    3 => Value::string(crate::format::ascii_escape(&self.repr(&v)?)),
                    _ => v,
                };
                let spec = match &spec {
                    Some(Value::Str(s)) => s.s.clone(),
                    _ => String::new(),
                };
                let s = if spec.is_empty() {
                    match &v {
                        Value::Str(_) => v,
                        _ => Value::string(self.str_of(&v)?),
                    }
                } else {
                    Value::string(self.format(&v, &spec)?)
                };
                f.stack.push(s);
            }
            Op::LoadAssertionError => {
                f.stack.push(Value::Class(self.t.exc("AssertionError")));
            }
            Op::SetupAnnotations => {
                let target = f.locals.clone().unwrap_or_else(|| f.globals.clone());
                if !target.borrow().contains_str("__annotations__") {
                    target
                        .borrow_mut()
                        .set_str("__annotations__", Value::dict(Dict::new()));
                }
            }
            Op::PrintExpr => {
                let v = Self::pop(f);
                if !v.is_none() {
                    let r = self.repr(&v)?;
                    self.write_stdout(&format!("{r}\n"));
                    self.builtins.borrow_mut().set_str("_", v);
                }
            }
            Op::MatchSequence(spec) => {
                let subject = f.stack.last().cloned().unwrap_or(Value::None);
                let ok = self.match_sequence(&subject, spec)?;
                f.stack.push(Value::Bool(ok));
            }
            Op::MatchStar(..) => {}
            Op::MatchMapping => {
                let subject = f.stack.last().cloned().unwrap_or(Value::None);
                let ok = matches!(subject, Value::Dict(_))
                    || matches!(&subject, Value::Instance(i) if i.class().kind == Kind::Dict);
                f.stack.push(Value::Bool(ok));
            }
            Op::MatchKeys(with_rest) => {
                let keys = Self::pop(f);
                let subject = Self::pop(f);
                let (vals, rest) = self.match_keys(&subject, &keys, with_rest != 0)?;
                if let Some(r) = rest {
                    f.stack.push(r);
                }
                f.stack.push(vals);
            }
            Op::MatchClass(nargs) => {
                let names = Self::pop(f);
                let cls = Self::pop(f);
                let subject = Self::pop(f);
                let v = self.match_class(&subject, &cls, nargs as usize, &names)?;
                f.stack.push(v);
            }
            Op::GetLen => {
                let v = f.stack.last().cloned().unwrap_or(Value::None);
                let n = self.len(&v)?;
                f.stack.push(Value::Int(n as i64));
            }
        }
        Ok(Flow::Continue)
    }

    fn cell_name(&self, f: &Frame, i: u32) -> String {
        let i = i as usize;
        let nc = f.code.cellvars.len();
        if i < nc {
            f.code.cellvars[i].to_string()
        } else {
            f.code.freevars[i - nc].to_string()
        }
    }
    fn unbound_local(&self, f: &Frame, i: u32) -> Box<PyErr> {
        let name = f.code.varnames[i as usize].clone();
        err(
            "UnboundLocalError",
            format!(
                "cannot access local variable '{name}' where it is not associated with a value"
            ),
        )
    }
    fn unbound_deref(&self, f: &Frame, i: u32) -> Box<PyErr> {
        let name = self.cell_name(f, i);
        if (i as usize) < f.code.cellvars.len() {
            err(
                "UnboundLocalError",
                format!(
                    "cannot access local variable '{name}' where it is not associated with a value"
                ),
            )
        } else {
            err(
                "NameError",
                format!("cannot access free variable '{name}' where it is not associated with a value in enclosing scope"),
            )
        }
    }
    fn name_error(&mut self, name: &str, f: &Frame) -> Box<PyErr> {
        let mut candidates: Vec<String> = f.code.varnames.iter().map(|s| s.to_string()).collect();
        candidates.extend(f.globals.borrow().keys().iter().filter_map(|k| match k {
            Value::Str(s) => Some(s.s.clone()),
            _ => None,
        }));
        if let Some(l) = &f.locals {
            candidates.extend(l.borrow().keys().iter().filter_map(|k| match k {
                Value::Str(s) => Some(s.s.clone()),
                _ => None,
            }));
        }
        candidates.extend(
            self.builtins
                .borrow()
                .keys()
                .iter()
                .filter_map(|k| match k {
                    Value::Str(s) => Some(s.s.clone()),
                    _ => None,
                }),
        );
        let mut msg = format!("name '{name}' is not defined");
        if let Some(s) = crate::format::suggest(name, &candidates) {
            msg.push_str(&format!(". Did you mean: '{s}'?"));
        }
        let e = err("NameError", msg);
        e
    }

    fn do_raise(&mut self, f: &mut Frame, n: u32) -> Box<PyErr> {
        if n == 0 {
            return match self.exc_stack.last().cloned() {
                Some(v) => PyErr::from_exc(v, true),
                None => err("RuntimeError", "No active exception to reraise"),
            };
        }
        let cause = if n == 2 { Some(Self::pop(f)) } else { None };
        let exc = Self::pop(f);
        let exc = match self.make_exception(&exc) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if let Some(c) = cause {
            let c = if c.is_none() {
                None
            } else {
                match self.make_exception(&c) {
                    Ok(v) => Some(v),
                    Err(e) => return e,
                }
            };
            self.exc_data(&exc, |d| {
                d.cause = c;
                d.suppress_context = true;
            });
        }
        PyErr::from_exc(exc, false)
    }

    /// `raise X`: X may be an exception class or instance.
    pub fn make_exception(&mut self, v: &Value) -> PyResult<Value> {
        match v {
            Value::Class(c) if c.is_subclass(&self.t.exc("BaseException")) => self.call(v, vec![]),
            Value::Instance(i) if i.class().is_subclass(&self.t.exc("BaseException")) => {
                Ok(v.clone())
            }
            _ => Err(type_err("exceptions must derive from BaseException")),
        }
    }

    pub fn exception_matches(&mut self, exc: &Value, cls: &Value) -> PyResult<bool> {
        let et = self.type_of(exc);
        match cls {
            Value::Class(c) => {
                if !c.is_subclass(&self.t.exc("BaseException")) {
                    return Err(type_err(
                        "catching classes that do not inherit from BaseException is not allowed",
                    ));
                }
                Ok(et.is_subclass(c))
            }
            Value::Tuple(t) => {
                for c in t.iter() {
                    if self.exception_matches(exc, c)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Err(type_err(
                "catching classes that do not inherit from BaseException is not allowed",
            )),
        }
    }

    /// Records the exception being handled as the implicit `__context__`.
    pub fn set_context(&mut self, e: &mut Box<PyErr>) {
        if let Some(ctx) = self.exc_stack.last().cloned() {
            let v = self.materialize(e);
            if v.is(&ctx) {
                return;
            }
            let has = self.exc_data(&v, |d| d.context.is_some()).unwrap_or(true);
            if !has {
                // Avoid cycles: don't set if ctx chain already contains v.
                let mut cur = Some(ctx.clone());
                let mut depth = 0;
                while let Some(c) = cur {
                    if c.is(&v) || depth > 100 {
                        return;
                    }
                    cur = self.exc_data(&c, |d| d.context.clone()).flatten();
                    depth += 1;
                }
                self.exc_data(&v, |d| d.context = Some(ctx));
            }
        }
    }

    /// A call from the interpreter loop: Python functions run inline.
    fn call_inline(
        &mut self,
        f: &mut Box<Frame>,
        func: Value,
        mut args: Vec<Value>,
        kwargs: Vec<(Rc<str>, Value)>,
    ) -> PyResult<Flow> {
        if let Value::Builtin(b) = &func {
            if matches!(
                &*b.name,
                "globals" | "locals" | "vars" | "dir" | "exec" | "eval" | "super"
            ) {
                if let Some(r) = self.frame_builtin(f, &b.name.clone(), &args, &kwargs) {
                    let v = r?;
                    f.stack.push(v);
                    return Ok(Flow::Continue);
                }
            }
        }
        if let Value::Class(c) = &func {
            if Rc::ptr_eq(c, &self.t.super_) && args.is_empty() {
                let v = self.frame_builtin(f, "super", &args, &kwargs).unwrap()?;
                f.stack.push(v);
                return Ok(Flow::Continue);
            }
        }
        let r = match &func {
            Value::Func(pf) if !pf.code.is_generator && !pf.code.is_coroutine => {
                let frame = self.bind_frame(pf, args, kwargs)?;
                return Ok(Flow::Call(frame));
            }
            Value::Method(m) => {
                if let Value::Func(pf) = &m.1 {
                    if !pf.code.is_generator && !pf.code.is_coroutine {
                        args.insert(0, m.0.clone());
                        let frame = self.bind_frame(pf, args, kwargs)?;
                        return Ok(Flow::Call(frame));
                    }
                }
                self.call_kw(&func, args, kwargs)
            }
            Value::Class(cls) if !cls.builtin => {
                // Instantiate; run a Python __init__ inline.
                let new = cls.lookup("__new__");
                let init = cls.lookup("__init__");
                let default_new =
                    matches!(&new, Some(Value::Builtin(b)) if &*b.name == "object.__new__");
                if let (true, Some(Value::Func(init_f))) = (default_new, &init) {
                    if cls.metaclass.borrow().is_none()
                        && !init_f.code.is_generator
                        && cls.abstract_methods.borrow().is_empty()
                    {
                        let inst = self.new_instance(cls);
                        let mut a = Vec::with_capacity(args.len() + 1);
                        a.push(inst.clone());
                        a.extend(args);
                        let mut frame = self.bind_frame(init_f, a, kwargs)?;
                        frame.kind = FrameKind::Init(inst);
                        return Ok(Flow::Call(frame));
                    }
                }
                self.call_kw(&func, args, kwargs)
            }
            _ => self.call_kw(&func, args, kwargs),
        };
        let v = r?;
        f.stack.push(v);
        Ok(Flow::Continue)
    }

    pub fn new_instance(&mut self, cls: &Rc<Class>) -> Value {
        let native = if cls.kind == Kind::Exception {
            NativeData::Exc(Box::new(ExcData {
                args: Value::tuple(vec![]),
                traceback: vec![],
                cause: None,
                context: None,
                suppress_context: false,
            }))
        } else {
            NativeData::None
        };
        Value::Instance(Rc::new(Instance {
            class: RefCell::new(cls.clone()),
            dict: new_ref(Dict::new()),
            native: RefCell::new(native),
        }))
    }

    pub fn kwargs_from(&mut self, d: &Value) -> PyResult<Vec<(Rc<str>, Value)>> {
        let items = self.mapping_items(d)?;
        let mut out = Vec::with_capacity(items.len());
        for (k, v) in items {
            match k {
                Value::Str(s) => out.push((s.s.as_str().into(), v)),
                _ => return Err(type_err("keywords must be strings")),
            }
        }
        Ok(out)
    }

    pub fn stop_value(&mut self, exc: &Value) -> Value {
        self.exc_data(exc, |d| match &d.args {
            Value::Tuple(t) => t.first().cloned().unwrap_or(Value::None),
            _ => Value::None,
        })
        .unwrap_or(Value::None)
    }

    fn unpack_iter(&mut self, v: &Value, n: usize) -> PyResult<Vec<Value>> {
        let it = self.get_iter(v).map_err(|e| {
            if self.err_matches(&e, "TypeError") {
                type_err(format!(
                    "cannot unpack non-iterable {} object",
                    self.type_name(v)
                ))
            } else {
                e
            }
        })?;
        let mut out = Vec::with_capacity(n);
        while let Some(x) = self.next(&it)? {
            if out.len() == n {
                return Err(value_err(format!(
                    "too many values to unpack (expected {n})"
                )));
            }
            out.push(x);
        }
        if out.len() < n {
            return Err(value_err(format!(
                "not enough values to unpack (expected {n}, got {})",
                out.len()
            )));
        }
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Output
    // ------------------------------------------------------------------

    pub fn write_stdout(&mut self, s: &str) {
        if self.stdout.len() + s.len() > self.output_limit {
            return;
        }
        self.stdout.push_str(s);
    }
    pub fn write_stderr(&mut self, s: &str) {
        if self.stderr.len() + s.len() > self.output_limit {
            return;
        }
        self.stderr.push_str(s);
    }
}

fn name_list(names: &[Rc<str>]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
    match quoted.len() {
        1 => quoted[0].clone(),
        2 => format!("{} and {}", quoted[0], quoted[1]),
        n => format!("{}, and {}", quoted[..n - 1].join(", "), quoted[n - 1]),
    }
}

pub fn unary_symbol(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Pos => "+",
        UnaryOp::Invert => "~",
        UnaryOp::Not => "not",
    }
}
