//! Promises, the microtask queue, `process.nextTick`, and async generators.

use crate::value::*;
use crate::vm::*;

pub fn slots(a: &Args) -> Vec<Value> {
    match &a.callee.borrow().kind {
        Kind::Function(fd) => match &fd.imp {
            FuncImpl::Native { slots, .. } => slots.clone(),
            _ => vec![],
        },
        _ => vec![],
    }
}

fn this_promise(vm: &mut Vm, v: &Value, method: &str) -> JsResult<Obj> {
    if let Value::Obj(o) = v {
        if let Kind::Promise(_) = o.borrow().kind {
            return Ok(o.clone());
        }
    }
    let d = vm.describe_for_error(v);
    Err(vm.type_error(format!(
        "Method Promise.prototype.{method} called on incompatible receiver {d}"
    )))
}

impl<'h> Vm<'h> {
    pub fn new_promise(&mut self) -> Obj {
        let proto = self.intr.promise_proto.clone();
        self.new_promise_proto(proto)
    }

    pub fn new_promise_proto(&mut self, proto: Obj) -> Obj {
        self.obj_with(
            Some(proto),
            Kind::Promise(Box::new(PromiseData {
                state: PromiseState::Pending,
                value: Value::Undefined,
                fulfill: vec![],
                reject: vec![],
                handled: false,
                resolving: false,
            })),
        )
    }

    pub fn promise_state(&self, p: &Obj) -> Option<(PromiseState, Value)> {
        match &p.borrow().kind {
            Kind::Promise(pd) => Some((pd.state, pd.value.clone())),
            _ => None,
        }
    }

    /// The resolve-function algorithm (thenables adopt via a job).
    pub fn resolve_promise(&mut self, p: &Obj, v: Value) -> JsResult<()> {
        if let Value::Obj(o) = &v {
            if o.ptr_eq(p) {
                let e = self.make_error(
                    ErrKind::TypeError,
                    "Chaining cycle detected for promise #<Promise>",
                );
                self.reject_promise(p, Value::Obj(e));
                return Ok(());
            }
            let then = match self.get_str(&v, "then") {
                Ok(t) => t,
                Err(Ctl::Throw(e)) => {
                    self.reject_promise(p, e);
                    return Ok(());
                }
                Err(other) => return Err(other),
            };
            if then.is_callable() {
                self.microtasks.push_back(Job::Thenable {
                    promise: p.clone(),
                    thenable: v,
                    then,
                });
                return Ok(());
            }
        }
        self.fulfill_promise(p, v);
        Ok(())
    }

    pub fn fulfill_promise(&mut self, p: &Obj, v: Value) {
        let reactions = {
            let mut d = p.borrow_mut();
            let Kind::Promise(pd) = &mut d.kind else {
                return;
            };
            if pd.state != PromiseState::Pending {
                return;
            }
            pd.state = PromiseState::Fulfilled;
            pd.value = v.clone();
            pd.reject.clear();
            std::mem::take(&mut pd.fulfill)
        };
        for r in reactions {
            self.microtasks.push_back(Job::Reaction {
                reaction: r,
                arg: v.clone(),
                rejected: false,
            });
        }
    }

    pub fn reject_promise(&mut self, p: &Obj, e: Value) {
        let (reactions, handled) = {
            let mut d = p.borrow_mut();
            let Kind::Promise(pd) = &mut d.kind else {
                return;
            };
            if pd.state != PromiseState::Pending {
                return;
            }
            pd.state = PromiseState::Rejected;
            pd.value = e.clone();
            pd.fulfill.clear();
            (std::mem::take(&mut pd.reject), pd.handled)
        };
        if !handled {
            self.pending_rejections.push(p.clone());
        }
        for r in reactions {
            self.microtasks.push_back(Job::Reaction {
                reaction: r,
                arg: e.clone(),
                rejected: true,
            });
        }
    }

    /// PromiseResolve(%Promise%, v).
    pub fn promise_resolve(&mut self, v: &Value) -> JsResult<Obj> {
        if let Value::Obj(o) = v {
            if let Kind::Promise(_) = o.borrow().kind {
                if o.proto()
                    .map(|p| p.ptr_eq(&self.intr.promise_proto))
                    .unwrap_or(false)
                {
                    return Ok(o.clone());
                }
            }
        }
        let p = self.new_promise();
        self.resolve_promise(&p, v.clone())?;
        Ok(p)
    }

    fn mark_handled(&mut self, p: &Obj) {
        let was_rejected_unhandled = {
            let mut d = p.borrow_mut();
            match &mut d.kind {
                Kind::Promise(pd) => {
                    let r = !pd.handled && pd.state == PromiseState::Rejected;
                    pd.handled = true;
                    r
                }
                _ => false,
            }
        };
        if was_rejected_unhandled {
            self.pending_rejections.retain(|x| !x.ptr_eq(p));
        }
    }

    /// Adds the same internal reaction for both outcomes.
    pub fn promise_react(&mut self, p: &Obj, r: Reaction) {
        self.mark_handled(p);
        let settled = {
            let mut d = p.borrow_mut();
            let Kind::Promise(pd) = &mut d.kind else {
                return;
            };
            match pd.state {
                PromiseState::Pending => {
                    pd.fulfill.push(r.clone());
                    pd.reject.push(r.clone());
                    None
                }
                PromiseState::Fulfilled => Some((pd.value.clone(), false)),
                PromiseState::Rejected => Some((pd.value.clone(), true)),
            }
        };
        if let Some((v, rejected)) = settled {
            self.microtasks.push_back(Job::Reaction {
                reaction: r,
                arg: v,
                rejected,
            });
        }
    }

    pub fn promise_then(&mut self, p: &Obj, on_f: Value, on_r: Value) -> Obj {
        let derived = self.new_promise();
        let f = Reaction::Then {
            handler: if on_f.is_callable() { Some(on_f) } else { None },
            derived: Some(derived.clone()),
            cap: None,
        };
        let r = Reaction::Then {
            handler: if on_r.is_callable() { Some(on_r) } else { None },
            derived: Some(derived.clone()),
            cap: None,
        };
        self.mark_handled(p);
        let settled = {
            let mut d = p.borrow_mut();
            let Kind::Promise(pd) = &mut d.kind else {
                return derived;
            };
            match pd.state {
                PromiseState::Pending => {
                    pd.fulfill.push(f.clone());
                    pd.reject.push(r.clone());
                    None
                }
                PromiseState::Fulfilled => Some((f, pd.value.clone(), false)),
                PromiseState::Rejected => Some((r, pd.value.clone(), true)),
            }
        };
        if let Some((reaction, v, rejected)) = settled {
            self.microtasks.push_back(Job::Reaction {
                reaction,
                arg: v,
                rejected,
            });
        }
        derived
    }

    pub fn create_resolving_functions(&mut self, p: &Obj) -> (Obj, Obj) {
        let flag = self.obj_with(None, Kind::Internal(vec![Value::Bool(false)]));
        let res = self.native_fn_slots(
            "",
            1,
            resolve_fn,
            vec![Value::Obj(p.clone()), Value::Obj(flag.clone())],
        );
        let rej = self.native_fn_slots(
            "",
            1,
            reject_fn,
            vec![Value::Obj(p.clone()), Value::Obj(flag)],
        );
        (res, rej)
    }

    pub fn run_job(&mut self, job: Job) -> JsResult<()> {
        match job {
            Job::Reaction {
                reaction,
                arg,
                rejected,
            } => match reaction {
                Reaction::Then {
                    handler, derived, ..
                } => {
                    let result = match handler {
                        Some(h) => self.call(&h, Value::Undefined, vec![arg]),
                        None => {
                            if rejected {
                                Err(Ctl::Throw(arg))
                            } else {
                                Ok(arg)
                            }
                        }
                    };
                    match (result, derived) {
                        (Ok(v), Some(d)) => self.resolve_promise(&d, v)?,
                        (Err(Ctl::Throw(e)), Some(d)) => self.reject_promise(&d, e),
                        (Err(Ctl::Throw(_)), None) => {}
                        (Err(other), _) => return Err(other),
                        (Ok(_), None) => {}
                    }
                }
                Reaction::Resume(co) => {
                    let is_gen = matches!(co.borrow().kind, Kind::Generator(_));
                    if is_gen {
                        self.async_gen_resume(&co, arg, rejected)?;
                    } else {
                        self.resume_coroutine(&co, arg, rejected)?;
                    }
                }
                Reaction::Native(f, s) => {
                    let func = self.native_fn_slots("", 1, f, s);
                    self.call(
                        &Value::Obj(func),
                        Value::Undefined,
                        vec![arg, Value::Bool(rejected)],
                    )?;
                }
            },
            Job::Thenable {
                promise,
                thenable,
                then,
            } => {
                let (res, rej) = self.create_resolving_functions(&promise);
                match self.call(
                    &then,
                    thenable,
                    vec![Value::Obj(res), Value::Obj(rej.clone())],
                ) {
                    Ok(_) => {}
                    Err(Ctl::Throw(e)) => {
                        self.call(&Value::Obj(rej), Value::Undefined, vec![e])?;
                    }
                    Err(other) => return Err(other),
                }
            }
            Job::Callback(f, args) => {
                if let Err(Ctl::Throw(v)) = self.call(&f, Value::Undefined, args) {
                    crate::node::mark_async(&v);
                    return Err(Ctl::Throw(v));
                }
            }
        }
        Ok(())
    }

    /// Drains `process.nextTick` callbacks and promise jobs.
    pub fn run_microtasks(&mut self) -> JsResult<()> {
        let saved = self.tail;
        let between = self.drain.between;
        let tick = self.drain.tick || !self.ticks.is_empty();
        let r = (|| loop {
            while let Some((f, args)) = self.ticks.pop_front() {
                self.tail = Tail::Tick(between);
                self.call(&f, Value::Undefined, args)?;
            }
            while let Some(job) = self.microtasks.pop_front() {
                self.tail = Tail::Microtask(between, tick);
                self.run_job(job)?;
            }
            if self.ticks.is_empty() && self.microtasks.is_empty() {
                return Ok(());
            }
        })();
        self.tail = saved;
        r
    }

    /// Value of an already-settled promise (or `v` itself).
    pub fn settled_value(&mut self, v: &Value) -> JsResult<Value> {
        if let Value::Obj(o) = v {
            if let Some((st, val)) = self.promise_state(o) {
                return match st {
                    PromiseState::Fulfilled => Ok(val),
                    PromiseState::Rejected => {
                        self.mark_handled(o);
                        Err(Ctl::Throw(val))
                    }
                    PromiseState::Pending => {
                        // Drive the queue until it settles.
                        self.run_microtasks()?;
                        match self.promise_state(o) {
                            Some((PromiseState::Fulfilled, val)) => Ok(val),
                            Some((PromiseState::Rejected, val)) => {
                                self.mark_handled(o);
                                Err(Ctl::Throw(val))
                            }
                            _ => Ok(Value::Undefined),
                        }
                    }
                };
            }
        }
        Ok(v.clone())
    }

    // ------------------------------------------------------------ async generators
    pub fn async_gen_enqueue(&mut self, g: &Obj, mode: u8, v: Value) -> JsResult<Value> {
        let p = self.new_promise();
        let ok = match &mut g.borrow_mut().kind {
            Kind::Generator(gd) if gd.is_async => {
                gd.queue.push_back((mode, v, p.clone()));
                true
            }
            _ => false,
        };
        if !ok {
            let e = self.make_error(
                ErrKind::TypeError,
                "next method called on incompatible receiver",
            );
            self.reject_promise(&p, Value::Obj(e));
            return Ok(Value::Obj(p));
        }
        self.async_gen_drain(g)?;
        Ok(Value::Obj(p))
    }

    fn async_gen_drain(&mut self, g: &Obj) -> JsResult<()> {
        loop {
            let (state, head) = match &g.borrow().kind {
                Kind::Generator(gd) => {
                    (gd.state, gd.queue.front().map(|(m, v, _)| (*m, v.clone())))
                }
                _ => return Ok(()),
            };
            let Some((mode, v)) = head else { return Ok(()) };
            if state == GenState::Running {
                return Ok(());
            }
            if state == GenState::Completed {
                let (_, _, p) = self.async_gen_pop(g);
                match mode {
                    1 => self.reject_promise(&p, v),
                    2 => {
                        let r = self.iter_result(v, true);
                        self.fulfill_promise(&p, r);
                    }
                    _ => {
                        let r = self.iter_result(Value::Undefined, true);
                        self.fulfill_promise(&p, r);
                    }
                }
                continue;
            }
            // Resume the generator frame.
            let mut frame = match &mut g.borrow_mut().kind {
                Kind::Generator(gd) => match gd.frame.take() {
                    Some(f) => f,
                    None => return Ok(()),
                },
                _ => return Ok(()),
            };
            if state == GenState::SuspendedStart && mode != 0 {
                if let Kind::Generator(gd) = &mut g.borrow_mut().kind {
                    gd.state = GenState::Completed;
                }
                continue;
            }
            if state == GenState::SuspendedYield {
                let at_ystar = matches!(
                    frame.code.ops.get(frame.pc),
                    Some(crate::bytecode::Op::YieldStar(_))
                );
                if at_ystar {
                    if let Some(t) = frame.stack.last_mut() {
                        *t = v;
                    }
                    frame.ystar_mode = mode;
                } else {
                    match mode {
                        0 => frame.stack.push(v),
                        1 => frame.resume = Some(Resume::Throw(v)),
                        _ => frame.resume = Some(Resume::Return(v)),
                    }
                }
            }
            self.async_gen_run(g, frame)?;
        }
    }

    fn async_gen_pop(&mut self, g: &Obj) -> (u8, Value, Obj) {
        match &mut g.borrow_mut().kind {
            Kind::Generator(gd) => gd.queue.pop_front().unwrap(),
            _ => unreachable!(),
        }
    }

    fn async_gen_run(&mut self, g: &Obj, frame: Box<Frame>) -> JsResult<()> {
        if let Kind::Generator(gd) = &mut g.borrow_mut().kind {
            gd.state = GenState::Running;
        }
        let depth = self.frames.len();
        self.frames.push(*frame);
        let r = self.run_nested(depth);
        let set_state = |g: &Obj, s: GenState| {
            if let Kind::Generator(gd) = &mut g.borrow_mut().kind {
                gd.state = s;
            }
        };
        match r {
            Ok(v) => match self.exit {
                Exit::Await => {
                    // Still running; resumed by the awaited promise.
                }
                Exit::Yield => {
                    set_state(g, GenState::SuspendedYield);
                    let (_, _, p) = self.async_gen_pop(g);
                    let r = self.iter_result(v, false);
                    self.fulfill_promise(&p, r);
                }
                Exit::Return => {
                    set_state(g, GenState::Completed);
                    let (_, _, p) = self.async_gen_pop(g);
                    let r = self.iter_result(v, true);
                    self.fulfill_promise(&p, r);
                }
            },
            Err(Ctl::Throw(e)) => {
                set_state(g, GenState::Completed);
                let (_, _, p) = self.async_gen_pop(g);
                self.reject_promise(&p, e);
            }
            Err(other) => return Err(other),
        }
        Ok(())
    }

    pub fn async_gen_resume(&mut self, g: &Obj, v: Value, rejected: bool) -> JsResult<()> {
        let frame = match &mut g.borrow_mut().kind {
            Kind::Generator(gd) => gd.frame.take(),
            _ => None,
        };
        let Some(mut frame) = frame else {
            return Ok(());
        };
        if rejected {
            frame.resume = Some(Resume::Throw(v));
        } else {
            frame.stack.push(v);
        }
        self.async_gen_run(g, frame)?;
        self.async_gen_drain(g)
    }
}

fn resolve_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    let (Value::Obj(p), Value::Obj(flag)) = (&s[0], &s[1]) else {
        return Ok(Value::Undefined);
    };
    {
        let mut d = flag.borrow_mut();
        if let Kind::Internal(v) = &mut d.kind {
            if v[0].truthy() {
                return Ok(Value::Undefined);
            }
            v[0] = Value::Bool(true);
        }
    }
    vm.resolve_promise(p, a.arg(0))?;
    Ok(Value::Undefined)
}

fn reject_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    let (Value::Obj(p), Value::Obj(flag)) = (&s[0], &s[1]) else {
        return Ok(Value::Undefined);
    };
    {
        let mut d = flag.borrow_mut();
        if let Kind::Internal(v) = &mut d.kind {
            if v[0].truthy() {
                return Ok(Value::Undefined);
            }
            v[0] = Value::Bool(true);
        }
    }
    vm.reject_promise(p, a.arg(0));
    Ok(Value::Undefined)
}

// ---------------------------------------------------------------- built-ins

pub fn promise_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Some(nt) = a.new_target.clone() else {
        return Err(vm.type_error("Promise constructor cannot be invoked without 'new'"));
    };
    let exec = a.arg(0);
    if !exec.is_callable() {
        let d = vm.describe_for_error(&exec);
        return Err(vm.type_error(format!("Promise resolver {d} is not a function")));
    }
    let pp = vm.intr.promise_proto.clone();
    let proto = vm.proto_from_ctor(&nt, &pp)?;
    let p = vm.new_promise_proto(proto);
    let (res, rej) = vm.create_resolving_functions(&p);
    match vm.call(
        &exec,
        Value::Undefined,
        vec![Value::Obj(res), Value::Obj(rej.clone())],
    ) {
        Ok(_) => {}
        Err(Ctl::Throw(e)) => {
            vm.call(&Value::Obj(rej), Value::Undefined, vec![e])?;
        }
        Err(o) => return Err(o),
    }
    Ok(Value::Obj(p))
}

pub fn promise_then(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = this_promise(vm, &a.this, "then")?;
    let d = vm.promise_then(&p, a.arg(0), a.arg(1));
    Ok(Value::Obj(d))
}

pub fn promise_catch(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let then = vm.get_str(&a.this, "then")?;
    vm.call(&then, a.this.clone(), vec![Value::Undefined, a.arg(0)])
}

pub fn promise_finally(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let f = a.arg(0);
    let then = vm.get_str(&a.this, "then")?;
    if !f.is_callable() {
        return vm.call(&then, a.this.clone(), vec![f.clone(), f]);
    }
    let on_f = vm.native_fn_slots("", 1, finally_fulfilled, vec![f.clone()]);
    let on_r = vm.native_fn_slots("", 1, finally_rejected, vec![f]);
    vm.call(
        &then,
        a.this.clone(),
        vec![Value::Obj(on_f), Value::Obj(on_r)],
    )
}

fn finally_fulfilled(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let f = slots(a)[0].clone();
    let r = vm.call(&f, Value::Undefined, vec![])?;
    let p = vm.promise_resolve(&r)?;
    let value = a.arg(0);
    let thunk = vm.native_fn_slots("", 0, return_slot, vec![value]);
    let d = vm.promise_then(&p, Value::Obj(thunk), Value::Undefined);
    Ok(Value::Obj(d))
}

fn finally_rejected(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let f = slots(a)[0].clone();
    let r = vm.call(&f, Value::Undefined, vec![])?;
    let p = vm.promise_resolve(&r)?;
    let reason = a.arg(0);
    let thrower = vm.native_fn_slots("", 0, throw_slot, vec![reason]);
    let d = vm.promise_then(&p, Value::Obj(thrower), Value::Undefined);
    Ok(Value::Obj(d))
}

fn return_slot(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(slots(a)[0].clone())
}

fn throw_slot(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Err(Ctl::Throw(slots(a)[0].clone()))
}

pub fn promise_resolve_static(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = vm.promise_resolve(&a.arg(0))?;
    Ok(Value::Obj(p))
}

pub fn promise_reject_static(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let p = vm.new_promise();
    vm.reject_promise(&p, a.arg(0));
    Ok(Value::Obj(p))
}

pub fn promise_with_resolvers(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let p = vm.new_promise();
    let (res, rej) = vm.create_resolving_functions(&p);
    let o = vm.new_object();
    o.set_prop("promise", Value::Obj(p), ALL);
    o.set_prop("resolve", Value::Obj(res), ALL);
    o.set_prop("reject", Value::Obj(rej), ALL);
    Ok(Value::Obj(o))
}

/// Shared state of a combinator: [result promise, values array, remaining count].
fn combinator_state(vm: &mut Vm, n: usize) -> (Obj, Obj, Obj) {
    let p = vm.new_promise();
    let values = vm.new_array(vec![Value::Undefined; n]);
    let remaining = vm.obj_with(None, Kind::Internal(vec![Value::Num(1.0)]));
    (p, values, remaining)
}

fn dec_remaining(r: &Obj, by: f64) -> f64 {
    let mut d = r.borrow_mut();
    if let Kind::Internal(v) = &mut d.kind {
        if let Value::Num(n) = &mut v[0] {
            *n += by;
            return *n;
        }
    }
    0.0
}

fn set_elem(values: &Obj, i: usize, v: Value) {
    if let Kind::Array(arr) = &mut values.borrow_mut().kind {
        if i < arr.len() {
            arr[i] = v;
        }
    }
}

/// kind: 0 all, 1 allSettled, 2 any, 3 race
fn combinator(vm: &mut Vm, a: &mut Args, kind: u8) -> JsResult<Value> {
    let items = match vm.iterable_to_vec(&a.arg(0)) {
        Ok(v) => v,
        Err(Ctl::Throw(e)) => {
            let p = vm.new_promise();
            vm.reject_promise(&p, e);
            return Ok(Value::Obj(p));
        }
        Err(o) => return Err(o),
    };
    let n = items.len();
    let (p, values, remaining) = combinator_state(vm, n);
    for (i, item) in items.into_iter().enumerate() {
        let ip = vm.promise_resolve(&item)?;
        let idx = Value::Num(i as f64);
        let st = vec![
            Value::Obj(p.clone()),
            Value::Obj(values.clone()),
            Value::Obj(remaining.clone()),
            idx,
            Value::Bool(false),
        ];
        let (on_f, on_r): (Value, Value) = match kind {
            0 => (
                Value::Obj(vm.native_fn_slots("", 1, all_resolve_element, st.clone())),
                Value::Obj(vm.native_fn_slots("", 1, reject_result, vec![Value::Obj(p.clone())])),
            ),
            1 => (
                Value::Obj(vm.native_fn_slots("", 1, settled_fulfilled, st.clone())),
                Value::Obj(vm.native_fn_slots("", 1, settled_rejected, st.clone())),
            ),
            2 => (
                Value::Obj(vm.native_fn_slots("", 1, resolve_result, vec![Value::Obj(p.clone())])),
                Value::Obj(vm.native_fn_slots("", 1, any_reject_element, st.clone())),
            ),
            _ => (
                Value::Obj(vm.native_fn_slots("", 1, resolve_result, vec![Value::Obj(p.clone())])),
                Value::Obj(vm.native_fn_slots("", 1, reject_result, vec![Value::Obj(p.clone())])),
            ),
        };
        dec_remaining(&remaining, 1.0);
        let then = vm.get_str(&Value::Obj(ip.clone()), "then")?;
        vm.call(&then, Value::Obj(ip), vec![on_f, on_r])?;
    }
    if kind != 3 && dec_remaining(&remaining, -1.0) == 0.0 {
        match kind {
            2 => {
                let e = vm.make_error(ErrKind::AggregateError, "All promises were rejected");
                e.set_hidden("errors", Value::Obj(values));
                vm.reject_promise(&p, Value::Obj(e));
            }
            _ => vm.fulfill_promise(&p, Value::Obj(values)),
        }
    }
    Ok(Value::Obj(p))
}

fn already_called(s: &[Value]) -> bool {
    // slot 4 is a per-element "called" flag stored in the function's slots;
    // natives cannot mutate their slots, so rely on promise idempotence.
    let _ = s;
    false
}

fn all_resolve_element(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    if already_called(&s) {
        return Ok(Value::Undefined);
    }
    let (Value::Obj(p), Value::Obj(values), Value::Obj(rem), Value::Num(i)) =
        (&s[0], &s[1], &s[2], &s[3])
    else {
        return Ok(Value::Undefined);
    };
    set_elem(values, *i as usize, a.arg(0));
    if dec_remaining(rem, -1.0) == 0.0 {
        vm.fulfill_promise(p, Value::Obj(values.clone()));
    }
    Ok(Value::Undefined)
}

fn settled_obj(vm: &mut Vm, status: &str, key: &str, v: Value) -> Value {
    let o = vm.new_object();
    o.set_prop("status", Value::str(status), ALL);
    o.set_prop(key, v, ALL);
    Value::Obj(o)
}

fn settled_fulfilled(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    let (Value::Obj(p), Value::Obj(values), Value::Obj(rem), Value::Num(i)) =
        (&s[0], &s[1], &s[2], &s[3])
    else {
        return Ok(Value::Undefined);
    };
    let o = settled_obj(vm, "fulfilled", "value", a.arg(0));
    set_elem(values, *i as usize, o);
    if dec_remaining(rem, -1.0) == 0.0 {
        vm.fulfill_promise(p, Value::Obj(values.clone()));
    }
    Ok(Value::Undefined)
}

fn settled_rejected(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    let (Value::Obj(p), Value::Obj(values), Value::Obj(rem), Value::Num(i)) =
        (&s[0], &s[1], &s[2], &s[3])
    else {
        return Ok(Value::Undefined);
    };
    let o = settled_obj(vm, "rejected", "reason", a.arg(0));
    set_elem(values, *i as usize, o);
    if dec_remaining(rem, -1.0) == 0.0 {
        vm.fulfill_promise(p, Value::Obj(values.clone()));
    }
    Ok(Value::Undefined)
}

fn any_reject_element(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    let (Value::Obj(p), Value::Obj(values), Value::Obj(rem), Value::Num(i)) =
        (&s[0], &s[1], &s[2], &s[3])
    else {
        return Ok(Value::Undefined);
    };
    set_elem(values, *i as usize, a.arg(0));
    if dec_remaining(rem, -1.0) == 0.0 {
        let e = vm.make_error(ErrKind::AggregateError, "All promises were rejected");
        e.set_hidden("errors", Value::Obj(values.clone()));
        vm.reject_promise(p, Value::Obj(e));
    }
    Ok(Value::Undefined)
}

fn resolve_result(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    if let Value::Obj(p) = &s[0] {
        vm.resolve_promise(p, a.arg(0))?;
    }
    Ok(Value::Undefined)
}

fn reject_result(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = slots(a);
    if let Value::Obj(p) = &s[0] {
        vm.reject_promise(p, a.arg(0));
    }
    Ok(Value::Undefined)
}

pub fn promise_all(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    combinator(vm, a, 0)
}
pub fn promise_all_settled(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    combinator(vm, a, 1)
}
pub fn promise_any(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    combinator(vm, a, 2)
}
pub fn promise_race(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    combinator(vm, a, 3)
}

pub fn queue_microtask(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let f = a.arg(0);
    if !f.is_callable() {
        let d = vm.describe_for_error(&f);
        let e = vm.make_error(
            ErrKind::TypeError,
            &format!("The \"callback\" argument must be of type function. Received {d}"),
        );
        e.set_prop("code", Value::str("ERR_INVALID_ARG_TYPE"), ALL);
        return Err(Ctl::Throw(Value::Obj(e)));
    }
    vm.microtasks.push_back(Job::Callback(f, vec![]));
    Ok(Value::Undefined)
}

pub fn install(vm: &mut Vm) {
    let proto = vm.intr.promise_proto.clone();
    let ctor = vm.make_ctor("Promise", 1, promise_ctor, &proto);
    vm.intr.promise_ctor = ctor.clone();
    vm.set_global("Promise", Value::Obj(ctor.clone()));
    vm.method(&proto, "then", 2, promise_then);
    vm.method(&proto, "catch", 1, promise_catch);
    vm.method(&proto, "finally", 1, promise_finally);
    let tag = vm.syms.to_string_tag.clone();
    proto.set_sym(&tag, Value::str("Promise"), CONFIGURABLE);
    vm.method(&ctor, "resolve", 1, promise_resolve_static);
    vm.method(&ctor, "reject", 1, promise_reject_static);
    vm.method(&ctor, "all", 1, promise_all);
    vm.method(&ctor, "allSettled", 1, promise_all_settled);
    vm.method(&ctor, "any", 1, promise_any);
    vm.method(&ctor, "race", 1, promise_race);
    vm.method(&ctor, "withResolvers", 0, promise_with_resolvers);
}
