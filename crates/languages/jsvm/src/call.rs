//! Function invocation: frames for closures, native calls, bound functions,
//! [[Construct]], generators and async activations.

use crate::ast::FuncKind;
use crate::bytecode::{Code, Op};
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

pub enum Invoked {
    Done(Value),
    Pushed,
}

enum Callee {
    Closure(Rc<Code>, Rc<[CellRef]>, bool, CtorKind),
    Native(NativeFn, CtorKind),
    Bound(Obj, Value, Vec<Value>),
    Proxy(Obj, Obj),
    Not,
}

impl<'h> Vm<'h> {
    fn callee_of(&self, f: &Obj) -> Callee {
        let d = f.borrow();
        match &d.kind {
            Kind::Function(fd) => match &fd.imp {
                FuncImpl::Closure { code, captures } => {
                    Callee::Closure(code.clone(), captures.clone(), fd.class_ctor, fd.ctor)
                }
                FuncImpl::Native { f, .. } => Callee::Native(*f, fd.ctor),
                FuncImpl::Bound { target, this, args } => {
                    Callee::Bound(target.clone(), this.clone(), args.clone())
                }
            },
            Kind::Proxy { target, handler } => Callee::Proxy(target.clone(), handler.clone()),
            _ => Callee::Not,
        }
    }

    pub fn not_a_function(&mut self, f: &Value, text: Option<&str>) -> Ctl {
        let t = match text {
            Some(t) => t.to_string(),
            None => self.describe_for_error(f),
        };
        self.type_error(format!("{t} is not a function"))
    }

    pub fn describe_for_error(&mut self, v: &Value) -> String {
        match v {
            Value::Str(s) => format!("\"{}\"", s.as_str()),
            Value::Obj(o) => {
                if o.is_callable() {
                    let n = Self::func_name(o);
                    format!("function {}", if n.is_empty() { "(anonymous)" } else { &n })
                } else if o.is_array() {
                    "object".into()
                } else {
                    "#<Object>".into()
                }
            }
            Value::Sym(s) => format!(
                "Symbol({})",
                s.desc.as_ref().map(|d| d.to_string()).unwrap_or_default()
            ),
            _ => self.to_str(v).unwrap_or_default(),
        }
    }

    /// Profiler bookkeeping around a native call; 0 when not profiling.
    #[inline]
    fn prof_native_enter(&mut self, f: NativeFn, fo: &Obj) -> usize {
        if self.prof.is_none() {
            return 0;
        }
        let name = Self::func_name(fo);
        let name = if name.is_empty() {
            "(anonymous native)".to_string()
        } else {
            name
        };
        if let Some(p) = &self.prof {
            crate::profile::Counters::bump(&p.counters.calls_native);
        }
        self.prof_enter_as(f as usize, name)
    }

    #[inline]
    fn prof_native_leave(&mut self, key: usize) {
        self.prof_leave(key)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn make_frame(
        &mut self,
        func: Option<&Obj>,
        code: Rc<Code>,
        captures: Rc<[CellRef]>,
        this: Value,
        args: Vec<Value>,
        new_target: Value,
        kind: FrameKind,
    ) -> Frame {
        self.charge_first_run(&code);
        if let Some(p) = &self.prof {
            crate::profile::Counters::bump(&p.counters.frames);
        }
        let n = code.nlocals as usize;
        let mut locals = self.pool.locals.pop().unwrap_or_default();
        locals.resize(n, Local::V(Value::Undefined));
        let mut args = args;
        if let Some(k) = code.simple_params {
            let k = (k as usize).min(args.len());
            if code.needs_args {
                for (slot, a) in locals.iter_mut().zip(args.iter()).take(k) {
                    *slot = Local::V(a.clone());
                }
            } else {
                // Nothing reads the list after entry: move the values in.
                for (slot, a) in locals.iter_mut().zip(args.drain(..)).take(k) {
                    *slot = Local::V(a);
                }
            }
        }
        if !code.needs_args {
            let spent = std::mem::take(&mut args);
            self.pool.give_vals(spent);
        }
        self.finish_frame(func, code, captures, this, locals, args, new_target, kind)
    }

    /// V8 compiles a function body the first time it runs: that costs
    /// simulated time, which is what makes the event loop's orderings follow
    /// from the program rather than from a fixed assumption. Charged once per
    /// body per realm.
    #[inline]
    pub(crate) fn charge_first_run(&mut self, code: &Code) {
        let by = code.compiled_by.get();
        if by == self.id {
            return;
        }
        let first_run = if by == 0 {
            code.compiled_by.set(self.id);
            true
        } else {
            self.compiled.insert(code.uid)
        };
        // Node's own builtins are in V8's startup snapshot: they are already
        // compiled, so only the program's own code is charged.
        if first_run && !code.file.starts_with("node:") {
            let bytes = code.own_bytes as usize;
            self.charge_compile(bytes);
        }
    }

    /// The rest of a frame once its parameter slots are filled: the special
    /// slots (`this`, `new.target`, home object, the function, `arguments`),
    /// captured slots made cells, and an operand stack.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish_frame(
        &mut self,
        func: Option<&Obj>,
        code: Rc<Code>,
        captures: Rc<[CellRef]>,
        this: Value,
        mut locals: Vec<Local>,
        args: Vec<Value>,
        new_target: Value,
        kind: FrameKind,
    ) -> Frame {
        if let Some(s) = code.this_slot {
            let t = if code.kind == FuncKind::DerivedConstructor {
                Value::Empty
            } else if !code.strict && code.kind != FuncKind::Arrow {
                match &this {
                    Value::Undefined | Value::Null => Value::Obj(self.global.clone()),
                    Value::Obj(_) => this.clone(),
                    Value::Empty => Value::Empty,
                    p => self
                        .to_object(p)
                        .map(Value::Obj)
                        .unwrap_or(Value::Undefined),
                }
            } else {
                this.clone()
            };
            locals[s as usize] = Local::V(t);
        }
        if let Some(s) = code.newtarget_slot {
            locals[s as usize] = Local::V(new_target);
        }
        if let Some(s) = code.home_slot {
            let home = func.and_then(|f| match &f.borrow().kind {
                Kind::Function(fd) => fd.home.clone(),
                _ => None,
            });
            locals[s as usize] = Local::V(home.map(Value::Obj).unwrap_or(Value::Undefined));
        }
        if let Some(s) = code.fn_slot {
            locals[s as usize] = Local::V(
                func.map(|f| Value::Obj(f.clone()))
                    .unwrap_or(Value::Undefined),
            );
        }
        if let Some(s) = code.args_slot {
            let a = self.make_arguments(&args);
            locals[s as usize] = Local::V(Value::Obj(a));
        }
        for &i in &code.cell_slots {
            let slot = &mut locals[i as usize];
            if let Local::V(v) = slot {
                let v = std::mem::replace(v, Value::Undefined);
                *slot = Local::C(new_cell(v));
            }
        }
        let stack = self
            .pool
            .vals
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(8));
        Frame {
            code,
            pc: 0,
            stack,
            locals,
            captures,
            handlers: vec![],
            args,
            func: func.cloned(),
            recv: this,
            kind,
            resume: None,
            ystar_mode: 0,
            resumed: false,
            timer: std::mem::take(&mut self.timer_frame),
        }
    }

    pub fn make_arguments(&mut self, args: &[Value]) -> Obj {
        let o = self.obj_with(Some(self.intr.object_proto.clone()), Kind::Arguments);
        {
            let mut d = o.borrow_mut();
            for (i, a) in args.iter().enumerate() {
                d.props.insert(
                    Key::Str(JsStr::new(i.to_string())),
                    Prop::data(a.clone(), ALL),
                );
            }
            d.props.insert(
                Key::str("length"),
                Prop::data(Value::Num(args.len() as f64), HIDDEN),
            );
            d.props.insert(
                Key::Sym(self.syms.iterator.clone()),
                Prop::data(Value::Obj(self.intr.array_values.clone()), HIDDEN),
            );
        }
        o
    }

    fn check_depth(&mut self) -> JsResult<()> {
        if self.frames.len() >= MAX_FRAMES {
            return Err(self.range_error("Maximum call stack size exceeded"));
        }
        Ok(())
    }

    /// Calls `f`; closures are pushed as frames for the run loop.
    pub fn invoke(
        &mut self,
        fv: &Value,
        this: Value,
        args: Vec<Value>,
        text: Option<&str>,
    ) -> JsResult<Invoked> {
        let Value::Obj(fo) = fv else {
            return Err(self.not_a_function(fv, text));
        };
        match self.callee_of(fo) {
            Callee::Closure(code, caps, class_ctor, _) => {
                if class_ctor {
                    let n = Self::func_name(fo);
                    return Err(self.type_error(format!(
                        "Class constructor {n} cannot be invoked without 'new'"
                    )));
                }
                self.check_depth()?;
                if let Some(p) = &self.prof {
                    crate::profile::Counters::bump(&p.counters.calls_js);
                }
                if code.is_generator {
                    let g = self.make_generator(fo, code, caps, this, args);
                    return Ok(Invoked::Done(Value::Obj(g)));
                }
                if code.is_async {
                    let promise = self.new_promise();
                    let co = self.obj_with(None, Kind::Coroutine(None));
                    let frame = self.make_frame(
                        Some(fo),
                        code,
                        caps,
                        this,
                        args,
                        Value::Undefined,
                        FrameKind::Async {
                            promise,
                            co,
                            first: true,
                        },
                    );
                    self.frames.push(frame);
                    return Ok(Invoked::Pushed);
                }
                let frame = self.make_frame(
                    Some(fo),
                    code,
                    caps,
                    this,
                    args,
                    Value::Undefined,
                    FrameKind::Normal,
                );
                self.frames.push(frame);
                Ok(Invoked::Pushed)
            }
            Callee::Native(f, _) => {
                self.natives.push(NativeMark {
                    depth: self.frames.len(),
                    callee: fo.clone(),
                    this: this.clone(),
                    construct: false,
                });
                let mut a = Args {
                    this,
                    args,
                    new_target: None,
                    callee: fo.clone(),
                };
                let key = self.prof_native_enter(f, fo);
                let r = f(self, &mut a);
                self.prof_native_leave(key);
                self.natives.pop();
                self.pool.give_vals(std::mem::take(&mut a.args));
                Ok(Invoked::Done(r?))
            }
            Callee::Bound(target, bthis, mut bargs) => {
                if let Some(p) = &self.prof {
                    crate::profile::Counters::bump(&p.counters.calls_bound);
                }
                bargs.extend(args);
                self.invoke(&Value::Obj(target), bthis, bargs, text)
            }
            Callee::Proxy(target, handler) => {
                let trap = self.get_str(&Value::Obj(handler.clone()), "apply")?;
                if trap.is_callable() {
                    let arr = self.arr(args);
                    let r = self.call(
                        &trap,
                        Value::Obj(handler),
                        vec![Value::Obj(target), this, arr],
                    )?;
                    return Ok(Invoked::Done(r));
                }
                self.invoke(&Value::Obj(target), this, args, text)
            }
            Callee::Not => Err(self.not_a_function(fv, text)),
        }
    }

    /// Calls a function from Rust, running pushed frames to completion.
    pub fn call(&mut self, f: &Value, this: Value, args: Vec<Value>) -> JsResult<Value> {
        let depth = self.frames.len();
        match self.invoke(f, this, args, None)? {
            Invoked::Done(v) => Ok(v),
            Invoked::Pushed => self.run_nested(depth),
        }
    }

    pub fn run_nested(&mut self, depth: usize) -> JsResult<Value> {
        if self.native_depth >= MAX_NATIVE_DEPTH {
            self.frames.truncate(depth);
            return Err(self.range_error("Maximum call stack size exceeded"));
        }
        self.native_depth += 1;
        let r = self.run(depth);
        self.native_depth -= 1;
        r
    }

    pub fn is_constructor(&self, v: &Value) -> bool {
        let Value::Obj(o) = v else { return false };
        match &o.borrow().kind {
            Kind::Function(fd) => match &fd.imp {
                FuncImpl::Bound { target, .. } => self.is_constructor(&Value::Obj(target.clone())),
                _ => fd.ctor != CtorKind::None,
            },
            Kind::Proxy { target, .. } => self.is_constructor(&Value::Obj(target.clone())),
            _ => false,
        }
    }

    /// Prototype for an object created by `new_target`.
    pub fn proto_from_ctor(&mut self, nt: &Obj, default: &Obj) -> JsResult<Obj> {
        let p = self.get_from(nt, &Key::str("prototype"), &Value::Obj(nt.clone()))?;
        Ok(match p {
            Value::Obj(p) => p,
            _ => default.clone(),
        })
    }

    pub fn construct_invoke(
        &mut self,
        fv: &Value,
        args: Vec<Value>,
        new_target: Option<Obj>,
        text: Option<&str>,
    ) -> JsResult<Invoked> {
        let not_ctor = |vm: &mut Self| {
            let t = match text {
                Some(t) => t.to_string(),
                None => vm.describe_for_error(fv),
            };
            vm.type_error(format!("{t} is not a constructor"))
        };
        let Value::Obj(fo) = fv else {
            return Err(not_ctor(self));
        };
        let nt = new_target.unwrap_or_else(|| fo.clone());
        if let Some(p) = &self.prof {
            crate::profile::Counters::bump(&p.counters.constructs);
        }
        match self.callee_of(fo) {
            Callee::Closure(code, caps, _, ctor) => {
                if ctor == CtorKind::None || code.is_generator || code.is_async {
                    return Err(not_ctor(self));
                }
                self.check_depth()?;
                if ctor == CtorKind::Derived {
                    let frame = self.make_frame(
                        Some(fo),
                        code,
                        caps,
                        Value::Empty,
                        args,
                        Value::Obj(nt),
                        FrameKind::Construct(Value::Undefined),
                    );
                    self.frames.push(frame);
                } else {
                    let op = self.intr.object_proto.clone();
                    let proto = self.proto_from_ctor(&nt, &op)?;
                    let this = Value::Obj(self.obj_with(Some(proto), Kind::Ordinary));
                    let frame = self.make_frame(
                        Some(fo),
                        code,
                        caps,
                        this.clone(),
                        args,
                        Value::Obj(nt),
                        FrameKind::Construct(this),
                    );
                    self.frames.push(frame);
                }
                Ok(Invoked::Pushed)
            }
            Callee::Native(f, ctor) => {
                if ctor == CtorKind::None {
                    return Err(not_ctor(self));
                }
                self.natives.push(NativeMark {
                    depth: self.frames.len(),
                    callee: fo.clone(),
                    this: Value::Undefined,
                    construct: true,
                });
                let mut a = Args {
                    this: Value::Undefined,
                    args,
                    new_target: Some(nt),
                    callee: fo.clone(),
                };
                let key = self.prof_native_enter(f, fo);
                let r = f(self, &mut a);
                self.prof_native_leave(key);
                self.natives.pop();
                self.pool.give_vals(std::mem::take(&mut a.args));
                Ok(Invoked::Done(r?))
            }
            Callee::Bound(target, _, mut bargs) => {
                bargs.extend(args);
                let nt2 = if nt.ptr_eq(fo) { target.clone() } else { nt };
                self.construct_invoke(&Value::Obj(target), bargs, Some(nt2), text)
            }
            Callee::Proxy(target, handler) => {
                let trap = self.get_str(&Value::Obj(handler.clone()), "construct")?;
                if trap.is_callable() {
                    let arr = self.arr(args);
                    let r = self.call(
                        &trap,
                        Value::Obj(handler),
                        vec![Value::Obj(target), arr, Value::Obj(nt)],
                    )?;
                    return Ok(Invoked::Done(r));
                }
                self.construct_invoke(&Value::Obj(target), args, None, text)
            }
            Callee::Not => Err(not_ctor(self)),
        }
    }

    pub fn construct(
        &mut self,
        f: &Value,
        args: Vec<Value>,
        new_target: Option<Obj>,
    ) -> JsResult<Value> {
        let depth = self.frames.len();
        match self.construct_invoke(f, args, new_target, None)? {
            Invoked::Done(v) => Ok(v),
            Invoked::Pushed => self.run_nested(depth),
        }
    }

    // ------------------------------------------------------------ generators
    fn make_generator(
        &mut self,
        fo: &Obj,
        code: Rc<Code>,
        caps: Rc<[CellRef]>,
        this: Value,
        args: Vec<Value>,
    ) -> Obj {
        let is_async = code.is_async;
        let default = if is_async {
            self.intr.async_generator_proto.clone()
        } else {
            self.intr.generator_proto.clone()
        };
        let proto = match fo.own_value("prototype") {
            Some(Value::Obj(p)) => p,
            _ => default,
        };
        let g = self.obj_with(
            Some(proto),
            Kind::Generator(Box::new(GenData {
                state: GenState::SuspendedStart,
                frame: None,
                queue: Default::default(),
                is_async,
            })),
        );
        let frame = self.make_frame(
            Some(fo),
            code,
            caps,
            this,
            args,
            Value::Undefined,
            FrameKind::Generator(g.clone()),
        );
        if let Kind::Generator(gd) = &mut g.borrow_mut().kind {
            gd.frame = Some(Box::new(frame));
        }
        g
    }

    /// Resumes a generator: mode 0 next, 1 throw, 2 return.
    /// Returns (value, done).
    pub fn gen_resume(&mut self, g: &Obj, mode: u8, value: Value) -> JsResult<(Value, bool)> {
        let (state, frame) = {
            let mut d = g.borrow_mut();
            match &mut d.kind {
                Kind::Generator(gd) => {
                    let st = gd.state;
                    if st == GenState::Running {
                        drop(d);
                        return Err(self.type_error("Generator is already running"));
                    }
                    let fr = if st == GenState::Completed {
                        None
                    } else {
                        gd.frame.take()
                    };
                    (st, fr)
                }
                _ => {
                    drop(d);
                    return Err(self.type_error("next method called on incompatible receiver"));
                }
            }
        };
        let set_state = |g: &Obj, s: GenState| {
            if let Kind::Generator(gd) = &mut g.borrow_mut().kind {
                gd.state = s;
            }
        };
        let Some(mut frame) = frame else {
            // Completed.
            return match mode {
                1 => Err(Ctl::Throw(value)),
                2 => Ok((value, true)),
                _ => Ok((Value::Undefined, true)),
            };
        };
        if state == GenState::SuspendedStart {
            match mode {
                1 => {
                    set_state(g, GenState::Completed);
                    return Err(Ctl::Throw(value));
                }
                2 => {
                    set_state(g, GenState::Completed);
                    return Ok((value, true));
                }
                _ => {}
            }
        } else {
            let at_ystar = matches!(frame.code.ops.get(frame.pc), Some(Op::YieldStar(_)));
            if at_ystar {
                if let Some(top) = frame.stack.last_mut() {
                    *top = value;
                }
                frame.ystar_mode = mode;
            } else {
                match mode {
                    0 => frame.stack.push(value),
                    1 => frame.resume = Some(Resume::Throw(value)),
                    _ => frame.resume = Some(Resume::Return(value)),
                }
            }
        }
        set_state(g, GenState::Running);
        let depth = self.frames.len();
        self.frames.push(*frame);
        let r = self.run_nested(depth);
        match r {
            Ok(v) => {
                if self.exit == Exit::Yield {
                    set_state(g, GenState::SuspendedYield);
                    Ok((v, false))
                } else {
                    set_state(g, GenState::Completed);
                    Ok((v, true))
                }
            }
            Err(e) => {
                set_state(g, GenState::Completed);
                Err(e)
            }
        }
    }

    pub fn iter_result(&mut self, value: Value, done: bool) -> Value {
        let o = self.new_object();
        {
            let mut d = o.borrow_mut();
            d.props.insert(Key::str("value"), Prop::data(value, ALL));
            d.props
                .insert(Key::str("done"), Prop::data(Value::Bool(done), ALL));
        }
        Value::Obj(o)
    }

    /// Resumes a suspended async activation with a settled value.
    pub fn resume_coroutine(&mut self, co: &Obj, value: Value, rejected: bool) -> JsResult<()> {
        let frame = match &mut co.borrow_mut().kind {
            Kind::Coroutine(f) => f.take(),
            _ => None,
        };
        let Some(mut frame) = frame else {
            return Ok(());
        };
        if rejected {
            frame.resume = Some(Resume::Throw(value));
        } else {
            frame.stack.push(value);
        }
        frame.resumed = true;
        if let FrameKind::Async { first, .. } = &mut frame.kind {
            *first = false;
        }
        let depth = self.frames.len();
        self.frames.push(*frame);
        self.run_nested(depth).map(|_| ())
    }
}
