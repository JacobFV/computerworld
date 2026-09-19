//! `worker_threads`: several JavaScript contexts in one interpreter, run one at
//! a time on a deterministic schedule.
//!
//! A worker is a context of its own — its own global object, module registry,
//! frames, microtasks, ticks and timers — that the interpreter swaps in when it
//! is that context's turn. Contexts share nothing but what they are given: a
//! message is structured-cloned on its way over, while a `SharedArrayBuffer`
//! keeps its backing store, which is how `Atomics` makes two contexts see the
//! same bytes.
//!
//! Only one context runs at a time, and it runs until it could make no more
//! progress on its own, so a race between two workers comes out the same way in
//! every run of a world.
use crate::value::*;
use crate::vm::*;
use std::collections::VecDeque;
use std::rc::Rc;

/// What starting a worker costs: a new context has its own global object and
/// its own copy of the Node bootstrap, which a real thread spends a few
/// milliseconds on before the worker's first line runs.
pub const WORKER_START_MS: f64 = 10.0;

/// A message on its way to a port, already cloned into the receiving context.
pub struct Envelope {
    pub port: u32,
    pub value: Value,
}

#[derive(Clone, PartialEq, Debug)]
pub enum WorkerState {
    /// Created, its entry not yet run.
    Starting,
    Running,
    /// Ended, by itself or by `terminate`, with its exit code.
    Exited(i32),
}

/// The part of a context the interpreter swaps in and out.
pub struct Context {
    pub global: Obj,
    pub frames: Vec<Frame>,
    pub natives: Vec<NativeMark>,
    pub microtasks: VecDeque<Job>,
    pub ticks: VecDeque<(Value, Vec<Value>)>,
    pub timers: Vec<Timer>,
    pub modules: Vec<(String, Value)>,
    pub tail: Tail,
    pub inbox: VecDeque<Envelope>,
    pub main_file: String,
    /// What the context has written but its parent has not passed on yet.
    /// Node gives a worker a pipe to its parent, not the terminal itself.
    pub stdout: String,
    pub stderr: String,
    pub out_mark: usize,
    /// A worker's `process.exit` ends the worker, not the program.
    pub exit_code: i32,
    pub exit_code_set: bool,
}

impl Context {
    /// An empty context, to swap the running one into.
    fn empty(vm: &mut Vm) -> Context {
        Context {
            global: vm.new_object(),
            frames: vec![],
            natives: vec![],
            microtasks: VecDeque::new(),
            ticks: VecDeque::new(),
            timers: vec![],
            modules: vec![],
            tail: Tail::Main,
            inbox: VecDeque::new(),
            main_file: String::new(),
            stdout: String::new(),
            stderr: String::new(),
            out_mark: 0,
            exit_code: 0,
            exit_code_set: false,
        }
    }

    fn swap(&mut self, vm: &mut Vm) {
        std::mem::swap(&mut self.global, &mut vm.global);
        std::mem::swap(&mut self.frames, &mut vm.frames);
        std::mem::swap(&mut self.natives, &mut vm.natives);
        std::mem::swap(&mut self.microtasks, &mut vm.microtasks);
        std::mem::swap(&mut self.ticks, &mut vm.ticks);
        std::mem::swap(&mut self.timers, &mut vm.timers);
        std::mem::swap(&mut self.modules, &mut vm.modules);
        std::mem::swap(&mut self.tail, &mut vm.tail);
        std::mem::swap(&mut self.inbox, &mut vm.inbox);
        std::mem::swap(&mut self.main_file, &mut vm.main_file);
        std::mem::swap(&mut self.stdout, &mut vm.stdout);
        std::mem::swap(&mut self.stderr, &mut vm.stderr);
        std::mem::swap(&mut self.out_mark, &mut vm.out_mark);
        std::mem::swap(&mut self.exit_code, &mut vm.exit_code);
        std::mem::swap(&mut self.exit_code_set, &mut vm.exit_code_set);
    }

    fn idle(&self) -> bool {
        self.frames.is_empty()
            && self.microtasks.is_empty()
            && self.ticks.is_empty()
            && self.inbox.is_empty()
    }
}

/// What a context is, apart from the state that is swapped: this part stays
/// put whether the context is running or parked.
pub struct Meta {
    pub id: u32,
    /// The context that made this one (0 for a worker of the main thread).
    pub parent: u32,
    pub state: WorkerState,
    /// This context's own end of the pair of ports joining it to its parent.
    pub port: u32,
    pub peer: u32,
    /// What the worker runs: a file path, or source to evaluate.
    pub entry: String,
    pub eval: bool,
    /// `workerData`, cloned into this context before it starts.
    pub data: Value,
    /// The simulated time this worker's first turn can come at.
    pub start_at: f64,
    /// Whether the parent has been told this worker started.
    pub online: bool,
    /// Whether the parent has been told this worker ended.
    pub reported: bool,
}

/// Everything the worker machinery keeps for a run.
#[derive(Default)]
pub struct Workers {
    /// The swappable state of every context that is not running.
    parked: Vec<(u32, Context)>,
    pub metas: Vec<Meta>,
    /// The context running now; 0 is the main one.
    pub current: u32,
    /// The contexts on the Rust stack, innermost last; they must not be
    /// entered again while they are there.
    active: Vec<u32>,
    next_id: u32,
    next_port: u32,
    /// Which context each port belongs to.
    ports: Vec<(u32, u32)>,
    /// The contexts inside `Atomics.wait`, by the cell they wait on:
    /// (context, backing store, byte offset).
    waits: Vec<(u32, usize, usize)>,
    /// The promises `Atomics.waitAsync` has still to settle.
    async_waits: Vec<AsyncWait>,
}

/// A cell a context is watching for `Atomics.waitAsync`.
struct AsyncWait {
    ctx: u32,
    buf: Rc<std::cell::RefCell<Vec<u8>>>,
    at: usize,
    width: usize,
    expected: i64,
    deadline: f64,
    promise: Obj,
}

impl AsyncWait {
    fn read(&self) -> i64 {
        let b = self.buf.borrow();
        let mut v: i64 = 0;
        for i in (0..self.width).rev() {
            v = (v << 8) | *b.get(self.at + i).unwrap_or(&0) as i64;
        }
        if self.width == 4 && v & 0x8000_0000 != 0 {
            v -= 1 << 32;
        }
        v
    }
}

impl Workers {
    fn meta(&mut self, id: u32) -> Option<&mut Meta> {
        self.metas.iter_mut().find(|m| m.id == id)
    }
}

impl Vm<'_> {
    fn workers(&mut self) -> &mut Workers {
        self.workers.get_or_insert_with(Default::default)
    }

    /// A fresh global for a worker: the same intrinsics, its own bindings.
    fn worker_global(&mut self) -> Obj {
        let proto = self.intr.object_proto.clone();
        let g = self.obj_with(Some(proto), Kind::Ordinary);
        let names: Vec<Key> = self
            .global
            .borrow()
            .props
            .entries
            .iter()
            .map(|(k, _)| k.clone())
            .collect();
        for key in names {
            if matches!(&key, Key::Str(s) if s.as_str() == "globalThis"
                || s.as_str() == "%requireCache")
            {
                continue;
            }
            let prop = self.global.borrow().props.get(&key).cloned();
            if let Some(p) = prop {
                g.borrow_mut().props.insert(key, p);
            }
        }
        g.set_hidden("globalThis", Value::Obj(g.clone()));
        g
    }

    /// Creates a worker and the pair of ports that joins it to its parent.
    /// Returns `(worker id, parent's port, worker's port)`.
    pub fn spawn_worker(
        &mut self,
        entry: &str,
        eval: bool,
        data: &Value,
    ) -> JsResult<(u32, u32, u32)> {
        let global = self.worker_global();
        let data = crate::builtins::global::structured_clone_value(self, data)?;
        let parent = self.workers().current;
        let start_at = self.clock() + WORKER_START_MS;
        let (id, parent_side, worker_side) = {
            let w = self.workers();
            w.next_id += 1;
            w.next_port += 2;
            (w.next_id, w.next_port - 1, w.next_port)
        };
        let mut ctx = Context::empty(self);
        ctx.global = global;
        let w = self.workers();
        w.parked.push((id, ctx));
        w.metas.push(Meta {
            id,
            parent,
            state: WorkerState::Starting,
            port: worker_side,
            peer: parent_side,
            entry: entry.to_string(),
            eval,
            data,
            start_at,
            online: false,
            reported: false,
        });
        w.ports.push((parent_side, parent));
        w.ports.push((worker_side, id));
        Ok((id, parent_side, worker_side))
    }

    /// A pair of joined ports, both owned by the context that asked for them.
    pub fn port_pair(&mut self) -> (u32, u32) {
        let current = self.workers().current;
        let w = self.workers();
        w.next_port += 2;
        let (a, b) = (w.next_port - 1, w.next_port);
        w.ports.push((a, current));
        w.ports.push((b, current));
        (a, b)
    }

    /// Hands `port` to the context that owns `via`, as a transfer list does.
    pub fn move_port(&mut self, port: u32, via: u32) -> bool {
        let Some(target) = self.owner_of(via) else {
            return false;
        };
        let w = self.workers();
        match w.ports.iter_mut().find(|(p, _)| *p == port) {
            Some(e) => {
                e.1 = target;
                true
            }
            None => false,
        }
    }

    /// Which context a port belongs to.
    fn owner_of(&mut self, port: u32) -> Option<u32> {
        let w = self.workers();
        w.ports.iter().find(|(p, _)| *p == port).map(|(_, c)| *c)
    }

    /// Sends a message to the context that owns `port`.
    pub fn post_to_port(&mut self, port: u32, value: &Value) -> JsResult<bool> {
        let Some(target) = self.owner_of(port) else {
            return Ok(false);
        };
        let cloned = crate::builtins::global::structured_clone_value(self, value)?;
        let current = self.workers().current;
        let env = Envelope {
            port,
            value: cloned,
        };
        if target == current {
            // A port of this context: the message still goes through the queue,
            // so it arrives in a later turn as Node's does.
            self.inbox.push_back(env);
            return Ok(true);
        }
        let w = self.workers();
        match w.parked.iter_mut().find(|(id, _)| *id == target) {
            Some((_, c)) => {
                c.inbox.push_back(env);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// The place in `parked` of a context that could run now.
    fn next_runnable(&mut self) -> Option<usize> {
        let now = self.clock();
        let w = self.workers();
        let active: Vec<u32> = w.active.clone();
        let metas: Vec<(u32, WorkerState, f64)> = w
            .metas
            .iter()
            .map(|m| (m.id, m.state.clone(), m.start_at))
            .collect();
        w.parked.iter().position(|(id, c)| {
            if active.contains(id) {
                return false;
            }
            let m = metas.iter().find(|(m, _, _)| m == id);
            match m.map(|(_, s, _)| s) {
                Some(WorkerState::Exited(_)) => false,
                // A worker's first turn only comes once it has started up.
                Some(WorkerState::Starting) => m.map(|(_, _, t)| now >= *t).unwrap_or(false),
                Some(WorkerState::Running) => !c.idle() || c.timers.iter().any(|t| t.when <= now),
                // The main context is only ever entered from the outside.
                None => false,
            }
        })
    }

    /// The id of the context running now; 0 is the main one.
    pub fn workers_current(&mut self) -> u32 {
        self.workers().current
    }

    /// Whether any context but the running one still has work.
    pub fn workers_pending(&mut self) -> bool {
        self.workers.is_some() && self.next_runnable().is_some()
    }

    /// Runs the other contexts until none of them can go on. Returns whether
    /// anything ran.
    pub fn run_workers(&mut self) -> JsResult<bool> {
        if self.workers.is_none() {
            return Ok(false);
        }
        let mut ran = false;
        while let Some(index) = self.next_runnable() {
            ran = true;
            let id = self.workers().parked[index].0;
            // A worker is `online` the moment it gets its first turn, before
            // any of its own code runs.
            let announce = {
                let w = self.workers();
                let current = w.current;
                match w.meta(id) {
                    Some(m) if m.state == WorkerState::Starting && m.parent == current => {
                        m.online = true;
                        true
                    }
                    _ => false,
                }
            };
            if announce {
                self.worker_event(id, "online", Value::Undefined)?;
                self.drain_after(None)?;
            }
            // Announcing may have run code that moved the contexts about.
            let at = self.workers().parked.iter().position(|(i, _)| *i == id);
            let Some(at) = at else { continue };
            self.run_context(at)?;
        }
        Ok(ran)
    }

    /// Swaps in the parked context at `index`, runs what it has, swaps back.
    fn run_context(&mut self, index: usize) -> JsResult<()> {
        let (id, mut ctx) = {
            let w = self.workers();
            w.parked.remove(index)
        };
        let previous = self.workers().current;
        // Park the context that was running, so a message can still reach it.
        let mut mine = Context::empty(self);
        mine.swap(self);
        ctx.swap(self);
        {
            let w = self.workers();
            w.parked.push((previous, mine));
            w.current = id;
            // The context that was running is parked but still on the Rust
            // stack, so it must not be entered again from in here.
            w.active.push(previous);
        }
        let result = self.run_context_inner(id);
        // Take this context's state back out of the interpreter, and put the
        // one that was running back in.
        let mut out = Context::empty(self);
        out.swap(self);
        let mine = {
            let w = self.workers();
            w.current = previous;
            w.active.pop();
            w.parked.push((id, out));
            let at = w.parked.iter().position(|(i, _)| *i == previous);
            at.map(|at| w.parked.remove(at).1)
        };
        let mut mine = match mine {
            Some(c) => c,
            None => Context::empty(self),
        };
        mine.swap(self);
        match result {
            Ok(()) | Err(Ctl::Exit(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn run_context_inner(&mut self, id: u32) -> JsResult<()> {
        let starting = matches!(
            self.workers().meta(id).map(|m| m.state.clone()),
            Some(WorkerState::Starting)
        );
        if starting {
            if let Some(m) = self.workers().meta(id) {
                m.state = WorkerState::Running;
            }
            if let Err(e) = self.start_worker_entry(id) {
                return self.end_worker(id, e);
            }
        }
        if let Err(e) = self.deliver_inbox() {
            return self.end_worker(id, e);
        }
        match self.event_loop() {
            Ok(()) => Ok(()),
            Err(e) => self.end_worker(id, e),
        }
    }

    /// A worker that throws tells its parent, as Node's `error` event does.
    fn end_worker(&mut self, id: u32, e: Ctl) -> JsResult<()> {
        let (state, err) = match e {
            Ctl::Exit(code) => (WorkerState::Exited(code), None),
            Ctl::Throw(v) | Ctl::Fatal(v) => (WorkerState::Exited(1), Some(v)),
        };
        if let Some(m) = self.workers().meta(id) {
            m.state = state;
        }
        if let Some(v) = err {
            let cloned = clone_value(self, &v);
            self.worker_errors.push((id, cloned));
        }
        // Whatever the worker still had queued is dropped, as in Node.
        self.frames.clear();
        self.microtasks.clear();
        self.ticks.clear();
        self.timers.clear();
        self.inbox.clear();
        Ok(())
    }

    fn start_worker_entry(&mut self, id: u32) -> JsResult<()> {
        let (port, peer, entry, eval, data) = {
            let w = self.workers();
            match w.meta(id) {
                Some(m) => (
                    m.port,
                    m.peer,
                    m.entry.clone(),
                    m.eval,
                    std::mem::replace(&mut m.data, Value::Undefined),
                ),
                None => return Ok(()),
            }
        };
        // The context's own `worker_threads` state, before its code runs.
        self.global.set_hidden("%workerData", data);
        let setup = format!("globalThis['%workerSetup']({id}, {port}, {peer})");
        let cwd = self.host.cwd();
        self.run_eval_source(&setup, "[worker]", &cwd, false)?;
        if eval {
            self.run_eval_source(&entry, "[worker eval]", &cwd, false)?;
            return Ok(());
        }
        match self.resolve_main(&entry) {
            Some(p) => {
                self.main_file = p.clone();
                self.load_file_module(&p)?;
                Ok(())
            }
            None => {
                let e = self.make_error(ErrKind::Error, &format!("Cannot find module '{entry}'"));
                e.set_prop("code", Value::str("MODULE_NOT_FOUND"), ALL);
                Err(Ctl::Throw(Value::Obj(e)))
            }
        }
    }

    /// Hands the messages waiting for this context to their port objects.
    /// Returns whether there were any.
    pub fn deliver_inbox(&mut self) -> JsResult<bool> {
        self.deliver_inbox_as(false)
    }

    /// The same, but only filling the ports' queues: a message that arrives
    /// while a program is between two lines waits for the event loop before it
    /// reaches a listener, as Node's does.
    pub fn deliver_inbox_quietly(&mut self) -> JsResult<bool> {
        self.deliver_inbox_as(true)
    }

    fn deliver_inbox_as(&mut self, quiet: bool) -> JsResult<bool> {
        let mut any = false;
        while let Some(m) = self.inbox.pop_front() {
            any = true;
            self.deliver_message(m, quiet)?;
        }
        Ok(any)
    }

    /// Hands one message to the port object that is waiting for it.
    fn deliver_message(&mut self, m: Envelope, quiet: bool) -> JsResult<()> {
        let f = self.global.own_value("%workerDeliver");
        let Some(f) = f else { return Ok(()) };
        self.call(
            &f,
            Value::Undefined,
            vec![Value::Num(m.port as f64), m.value, Value::Bool(quiet)],
        )?;
        Ok(())
    }

    /// Hands the ports' queued messages to their listeners.
    pub fn flush_ports(&mut self) -> JsResult<bool> {
        let f = self.global.own_value("%workerFlush");
        let Some(f) = f else { return Ok(false) };
        let r = self.call(&f, Value::Undefined, vec![])?;
        Ok(r.truthy())
    }

    /// Tells this context about its workers' errors and endings. Returns
    /// whether anything was reported.
    pub fn worker_events(&mut self) -> JsResult<bool> {
        if self.workers.is_none() {
            return Ok(false);
        }
        let current = self.workers().current;
        let mut any = false;
        loop {
            let next = {
                let w = self.workers();
                w.metas
                    .iter_mut()
                    .find(|m| m.parent == current && !m.online && m.state != WorkerState::Starting)
                    .map(|m| {
                        m.online = true;
                        m.id
                    })
            };
            let Some(id) = next else { break };
            any = true;
            self.worker_event(id, "online", Value::Undefined)?;
        }
        // Each worker is heard out in turn: what it sent, then how it ended.
        // Two threads that both finish while the parent is busy reach it in
        // the order they were started.
        let mine: Vec<(u32, u32)> = self
            .workers()
            .metas
            .iter()
            .filter(|m| m.parent == current)
            .map(|m| (m.id, m.peer))
            .collect();
        for (id, port) in mine {
            while let Some(at) = self.inbox.iter().position(|e| e.port == port) {
                let Some(m) = self.inbox.remove(at) else {
                    break;
                };
                any = true;
                self.deliver_message(m, false)?;
            }
            while let Some(at) = self.worker_errors.iter().position(|(i, _)| *i == id) {
                let (_, v) = self.worker_errors.remove(at);
                any = true;
                self.worker_event(id, "error", v)?;
            }
            let ended = {
                let w = self.workers();
                match w.meta(id) {
                    Some(m) if !m.reported && matches!(m.state, WorkerState::Exited(_)) => {
                        m.reported = true;
                        match m.state {
                            WorkerState::Exited(c) => Some(c),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            };
            if let Some(code) = ended {
                any = true;
                self.worker_event(id, "exit", Value::Num(code as f64))?;
            }
        }
        Ok(any)
    }

    fn worker_event(&mut self, id: u32, kind: &str, payload: Value) -> JsResult<()> {
        let f = self.global.own_value("%workerEvent");
        let Some(f) = f else { return Ok(()) };
        self.call(
            &f,
            Value::Undefined,
            vec![Value::Num(id as f64), Value::str(kind), payload],
        )?;
        Ok(())
    }

    /// When nothing anywhere can run, a worker still waiting for a message it
    /// will never get ends, so the program can finish. Returns whether any
    /// worker was ended this way.
    pub fn end_idle_workers(&mut self) -> bool {
        if self.workers.is_none() {
            return false;
        }
        let w = self.workers();
        let live: Vec<u32> = w
            .metas
            .iter()
            .filter(|m| !matches!(m.state, WorkerState::Exited(_)))
            .map(|m| m.id)
            .collect();
        for id in &live {
            if let Some(m) = w.meta(*id) {
                m.state = WorkerState::Exited(0);
            }
        }
        !live.is_empty()
    }

    /// Passes on what this context's workers have written, as the parent
    /// reading their pipes does. Returns whether there was anything.
    pub fn flush_worker_output(&mut self) -> bool {
        if self.workers.is_none() {
            return false;
        }
        let current = self.workers_current();
        let w = self.workers();
        let mine: Vec<u32> = w
            .metas
            .iter()
            .filter(|m| m.parent == current)
            .map(|m| m.id)
            .collect();
        let mut out = String::new();
        let mut err = String::new();
        for (id, c) in w.parked.iter_mut() {
            if !mine.contains(id) {
                continue;
            }
            out.push_str(&std::mem::take(&mut c.stdout));
            err.push_str(&std::mem::take(&mut c.stderr));
            c.out_mark = 0;
        }
        let any = !out.is_empty() || !err.is_empty();
        self.stdout.push_str(&out);
        self.stderr.push_str(&err);
        any
    }

    /// When a parked context could next have something to do: its timers, and
    /// the moment a worker that has not started yet will be ready to.
    pub fn worker_timer_times(&mut self) -> Vec<f64> {
        let w = self.workers();
        let active = w.active.clone();
        let mut times: Vec<f64> = w
            .parked
            .iter()
            .filter(|(id, _)| !active.contains(id))
            .flat_map(|(_, c)| c.timers.iter().map(|t| t.when))
            .collect();
        times.extend(
            w.metas
                .iter()
                .filter(|m| m.state == WorkerState::Starting)
                .map(|m| m.start_at),
        );
        times
    }

    /// What a worker's `workerData` will be, set before it starts.
    pub fn set_worker_data(&mut self, id: u32, data: Value) {
        if let Some(m) = self.workers().meta(id) {
            m.data = data;
        }
    }

    /// The state of a worker, for `worker.threadId` and the `exit` event.
    pub fn worker_state(&mut self, id: u32) -> Option<WorkerState> {
        self.workers().meta(id).map(|m| m.state.clone())
    }

    pub fn terminate_worker(&mut self, id: u32) {
        let w = self.workers();
        let running = w.current == id || w.active.contains(&id);
        if let Some(m) = self.workers().meta(id) {
            if !matches!(m.state, WorkerState::Exited(_)) {
                m.state = WorkerState::Exited(1);
            }
        }
        if running {
            return;
        }
        let w = self.workers();
        if let Some((_, c)) = w.parked.iter_mut().find(|(i, _)| *i == id) {
            c.frames.clear();
            c.microtasks.clear();
            c.ticks.clear();
            c.timers.clear();
            c.inbox.clear();
        }
    }

    /// `Atomics.wait`: runs the other contexts until the cell changes, the
    /// timeout passes, or nothing anywhere can move.
    pub fn atomics_wait(
        &mut self,
        buf: &Rc<std::cell::RefCell<Vec<u8>>>,
        byte_offset: usize,
        width: usize,
        expected: i64,
        timeout_ms: f64,
    ) -> JsResult<&'static str> {
        let key = (
            self.workers_current(),
            Rc::as_ptr(buf) as usize,
            byte_offset,
        );
        self.workers().waits.push(key);
        let r = self.atomics_wait_inner(buf, byte_offset, width, expected, timeout_ms);
        let w = self.workers();
        if let Some(at) = w.waits.iter().rposition(|k| *k == key) {
            w.waits.remove(at);
        }
        r
    }

    /// `Atomics.waitAsync`: a promise settled once the cell changes or the
    /// wait times out. Like Node's, it does not keep the program running.
    pub fn atomics_wait_async(
        &mut self,
        buf: &Rc<std::cell::RefCell<Vec<u8>>>,
        byte_offset: usize,
        width: usize,
        expected: i64,
        timeout_ms: f64,
    ) -> Obj {
        let promise = self.new_promise();
        let deadline = self.clock()
            + if timeout_ms.is_nan() {
                f64::INFINITY
            } else {
                timeout_ms
            };
        let ctx = self.workers_current();
        self.workers().async_waits.push(AsyncWait {
            ctx,
            buf: buf.clone(),
            at: byte_offset,
            width,
            expected,
            deadline,
            promise: promise.clone(),
        });
        promise
    }

    /// Settles the asynchronous waits of this context whose cell has changed
    /// or whose time is up. Returns whether any did.
    pub fn poll_async_waits(&mut self) -> JsResult<bool> {
        if self.workers.is_none() {
            return Ok(false);
        }
        let now = self.clock();
        let current = self.workers_current();
        let mut done: Vec<(Obj, &'static str)> = vec![];
        {
            let w = self.workers();
            let mut i = 0;
            while i < w.async_waits.len() {
                let a = &w.async_waits[i];
                let settle = if a.ctx != current {
                    None
                } else if a.read() != a.expected {
                    Some("ok")
                } else if now >= a.deadline {
                    Some("timed-out")
                } else {
                    None
                };
                match settle {
                    Some(s) => {
                        let a = w.async_waits.remove(i);
                        done.push((a.promise, s));
                    }
                    None => i += 1,
                }
            }
        }
        let any = !done.is_empty();
        for (p, s) in done {
            self.fulfill_promise(&p, Value::str(s));
        }
        Ok(any)
    }

    /// How many contexts are waiting on this cell, up to `count`. Since a
    /// waiting context re-reads the cell on its own, waking it is just saying
    /// so.
    pub fn atomics_notify(
        &mut self,
        buf: &Rc<std::cell::RefCell<Vec<u8>>>,
        byte_offset: usize,
        count: f64,
    ) -> usize {
        let addr = Rc::as_ptr(buf) as usize;
        let current = self.workers_current();
        let w = self.workers();
        let n = w
            .waits
            .iter()
            .filter(|(c, a, o)| *c != current && *a == addr && *o == byte_offset)
            .count();
        if count.is_finite() {
            n.min(count as usize)
        } else {
            n
        }
    }

    fn atomics_wait_inner(
        &mut self,
        buf: &Rc<std::cell::RefCell<Vec<u8>>>,
        byte_offset: usize,
        width: usize,
        expected: i64,
        timeout_ms: f64,
    ) -> JsResult<&'static str> {
        let read = |buf: &Rc<std::cell::RefCell<Vec<u8>>>| -> i64 {
            let b = buf.borrow();
            let mut v: i64 = 0;
            for i in (0..width).rev() {
                v = (v << 8) | *b.get(byte_offset + i).unwrap_or(&0) as i64;
            }
            if width == 4 && v & 0x8000_0000 != 0 {
                v -= 1 << 32;
            }
            v
        };
        if read(buf) != expected {
            return Ok("not-equal");
        }
        let deadline = self.clock()
            + if timeout_ms.is_nan() {
                f64::INFINITY
            } else {
                timeout_ms
            };
        loop {
            let ran = self.run_workers()?;
            self.deliver_inbox()?;
            if read(buf) != expected {
                return Ok("ok");
            }
            if self.clock() >= deadline {
                return Ok("timed-out");
            }
            if !ran {
                // Nothing else can run: only the clock can still change things.
                let mut times: Vec<f64> = self.timers.iter().map(|t| t.when).collect();
                times.extend(self.worker_timer_times());
                let now = self.clock();
                let next = times
                    .into_iter()
                    .filter(|w| *w > now)
                    .fold(f64::INFINITY, f64::min);
                if next.is_finite() && next <= deadline {
                    self.elapsed_ms = next;
                    continue;
                }
                if deadline.is_finite() {
                    self.elapsed_ms = self.elapsed_ms.max(deadline);
                    return Ok("timed-out");
                }
                // A wait nothing can ever end: Node would hang here for good.
                return Err(Ctl::Fatal(Value::str(
                    "Atomics.wait: every thread is waiting",
                )));
            }
        }
    }
}

/// A structured clone, as `postMessage` makes: the value is copied, except a
/// `SharedArrayBuffer`, whose bytes the two contexts go on sharing.
pub fn clone_value(vm: &mut Vm, v: &Value) -> Value {
    crate::builtins::global::structured_clone_value(vm, v).unwrap_or(Value::Undefined)
}

// ------------------------------------------------------- the JS side's hooks

fn port_arg(vm: &mut Vm, a: &Args, i: usize) -> JsResult<u32> {
    Ok(vm.to_number(&a.arg(i))?.max(0.0) as u32)
}

/// `binding.workerNew(entry, eval, workerData)` → `{ id, port, peer }`.
fn w_new(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let entry = vm.to_str(&a.arg(0))?;
    let eval = a.arg(1).truthy();
    let data = a.arg(2);
    let (id, port, peer) = vm.spawn_worker(&entry, eval, &data)?;
    let o = crate::builtins::new_obj_from(
        vm,
        vec![
            ("id", Value::Num(id as f64)),
            ("port", Value::Num(port as f64)),
            ("peer", Value::Num(peer as f64)),
        ],
    );
    Ok(Value::Obj(o))
}

/// `binding.workerSetData(id, value)`: the `workerData` the worker starts with,
/// set once its ports exist so one of them can travel in it.
fn w_set_data(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let id = port_arg(vm, a, 0)?;
    let v = a.arg(1);
    let data = crate::builtins::global::structured_clone_value(vm, &v)?;
    vm.set_worker_data(id, data);
    Ok(Value::Undefined)
}

/// `binding.workerPost(port, value)`: the port is the receiver's own id.
fn w_post(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let port = port_arg(vm, a, 0)?;
    let v = a.arg(1);
    Ok(Value::Bool(vm.post_to_port(port, &v)?))
}

fn w_terminate(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let id = port_arg(vm, a, 0)?;
    vm.terminate_worker(id);
    Ok(Value::Undefined)
}

/// `binding.workerStatus(id)` → `{ state, code }`.
fn w_status(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let id = port_arg(vm, a, 0)?;
    let (state, code) = match vm.worker_state(id) {
        Some(WorkerState::Starting) => ("starting", Value::Undefined),
        Some(WorkerState::Running) => ("running", Value::Undefined),
        Some(WorkerState::Exited(c)) => ("exited", Value::Num(c as f64)),
        None => ("unknown", Value::Undefined),
    };
    let o = crate::builtins::new_obj_from(vm, vec![("state", Value::str(state)), ("code", code)]);
    Ok(Value::Obj(o))
}

/// `binding.workerPortPair()` → `[a, b]`, the two ends of a channel.
fn w_port_pair(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let (a, b) = vm.port_pair();
    Ok(Value::Obj(vm.new_array(vec![
        Value::Num(a as f64),
        Value::Num(b as f64),
    ])))
}

/// `binding.workerMovePort(port, via)`: hands a port to the context that owns
/// `via`, which is what a transfer list does.
fn w_move_port(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let port = port_arg(vm, a, 0)?;
    let via = port_arg(vm, a, 1)?;
    Ok(Value::Bool(vm.move_port(port, via)))
}

/// `binding.workerDrain()`: lets the other contexts run and takes in whatever
/// they sent, for code that waits without going back to the event loop.
fn w_drain(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let mut any = vm.run_workers()?;
    any |= vm.deliver_inbox_quietly()?;
    Ok(Value::Bool(any))
}

pub fn install(vm: &mut Vm, b: &Obj) {
    let fns: &[(&str, u32, NativeFn)] = &[
        ("workerNew", 3, w_new),
        ("workerSetData", 2, w_set_data),
        ("workerPost", 2, w_post),
        ("workerTerminate", 1, w_terminate),
        ("workerStatus", 1, w_status),
        ("workerPortPair", 0, w_port_pair),
        ("workerMovePort", 2, w_move_port),
        ("workerDrain", 0, w_drain),
    ];
    for (n, l, f) in fns {
        vm.method(b, n, *l, *f);
    }
}
