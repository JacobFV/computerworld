//! The deterministic thread scheduler.
//!
//! Python threads are real interpreter threads: each has its own frame stack, and
//! the interpreter switches between them at a fixed instruction quantum derived
//! from the world seed (as CPython switches on its GIL interval), so a race
//! interleaves the same way in every replay of a world. Blocking (a lock, a
//! queue, `join`, `time.sleep`) runs the other threads until the condition holds;
//! when every thread is waiting on time, the virtual clock jumps to the earliest
//! wake-up. A wait that nothing can satisfy is a deadlock and is reported as one
//! instead of hanging.
//!
//! Switching happens inside the interpreter loop: the running thread's frames are
//! swapped for another's. That is only possible where no native Rust frame sits in
//! between, so a thread that blocks inside a native call (or a generator's body)
//! runs the others nested, from where it stands.
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

/// CPython's default switch interval, in seconds.
pub const DEFAULT_SWITCH_INTERVAL: f64 = 0.005;
/// Interpreter steps per simulated second (matches `time.process_time`).
pub const STEPS_PER_SECOND: f64 = 50_000_000.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Runnable,
    /// Waiting for something; the predicate is held by the blocked call itself.
    Waiting,
    Finished,
}

pub struct Saved {
    pub top: Box<Frame>,
    pub frames: Vec<Box<Frame>>,
    pub exc: Vec<Value>,
    pub depth: usize,
}

/// What a parked thread is waiting for; the scheduler checks it at every switch.
pub enum Wait {
    /// Until the deadline passes (`time.sleep`).
    Deadline,
    /// Until this lock is free, then take it.
    Lock(Rc<Native>),
    /// Until this Python callable returns something truthy.
    Predicate(Value),
    /// Until the thread with this identifier has finished.
    Thread(u64),
}

/// A blocking call that unwound to the interpreter loop.
pub struct ParkInfo {
    pub wait: Wait,
    pub deadline: Option<i64>,
}

/// The signal that unwinds an interpreter level whose threads are all waiting.
pub fn yield_level() -> Box<PyErr> {
    Box::new(PyErr {
        kind: ErrKind::YieldLevel,
        reraise: false,
        fatal: false,
    })
}

pub struct ThreadState {
    pub id: u64,
    pub name: String,
    pub daemon: bool,
    pub status: Status,
    /// What the thread is waiting for while parked.
    pub wait: Option<Wait>,
    /// Pushed onto the parked frame's stack when the thread resumes.
    pub resume: Option<Value>,
    /// Set while this thread is the one running at some interpreter level: its
    /// state lives on the Rust stack and cannot be resumed from elsewhere.
    pub pinned: bool,
    pub saved: Option<Saved>,
    /// Virtual time this thread is sleeping until, if any.
    pub wake_at: Option<i64>,
    /// The value the thread's function returned, or the exception it died with.
    pub result: Option<Value>,
    pub failed: bool,
    /// The Python `Thread` object, when `threading` created it.
    pub obj: Value,
}

pub struct Level {
    pub base: usize,
    pub exc_base: usize,
    pub depth_base: usize,
    pub native_depth: usize,
    pub owner: usize,
}

pub struct Scheduler {
    pub threads: Vec<ThreadState>,
    pub current: usize,
    pub levels: Vec<Level>,
    /// Instructions between preemptions.
    pub quantum: u64,
    pub left: u64,
    pub next_id: u64,
    /// Steps executed since the virtual clock was last advanced.
    pub steps: u64,
    /// The trampoline that calls a thread's target with its arguments.
    pub trampoline: Option<Value>,
    pub switch_interval: f64,
    pub seed: u64,
}

impl Scheduler {
    fn quantum_for(seed: u64, interval: f64) -> u64 {
        // A seed-derived quantum in the 1000..4096 instruction range, scaled by
        // the switch interval a program may set.
        let base = 1000 + (seed % 3097);
        ((base as f64) * (interval / DEFAULT_SWITCH_INTERVAL)).max(1.0) as u64
    }
    pub fn new(seed: u64, main_name: &str) -> Self {
        let quantum = Self::quantum_for(seed, DEFAULT_SWITCH_INTERVAL);
        Self {
            threads: vec![ThreadState {
                id: 1,
                name: main_name.to_string(),
                daemon: false,
                status: Status::Runnable,
                pinned: true,
                saved: None,
                wait: None,
                resume: None,
                wake_at: None,
                result: None,
                failed: false,
                obj: Value::None,
            }],
            current: 0,
            levels: vec![Level {
                base: 0,
                exc_base: 0,
                depth_base: 0,
                native_depth: 1,
                owner: 0,
            }],
            quantum,
            left: quantum,
            next_id: 2,
            steps: 0,
            trampoline: None,
            switch_interval: DEFAULT_SWITCH_INTERVAL,
            seed,
        }
    }
    pub fn set_switch_interval(&mut self, interval: f64) {
        self.switch_interval = interval;
        self.quantum = Self::quantum_for(self.seed, interval);
        self.left = self.left.min(self.quantum);
    }
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.threads.iter().position(|t| t.id == id)
    }
    pub fn current_id(&self) -> u64 {
        self.threads[self.current].id
    }
    /// Threads that could run now: not finished, not waiting, not on the stack.
    fn runnable(&self, except: usize) -> Option<usize> {
        let n = self.threads.len();
        for k in 1..=n {
            let i = (except + k) % n;
            let t = &self.threads[i];
            if t.status == Status::Runnable && !t.pinned && t.saved.is_some() {
                return Some(i);
            }
        }
        None
    }
    /// Earliest virtual time a sleeping thread wants to wake at.
    fn earliest_wake(&self) -> Option<i64> {
        self.threads
            .iter()
            .filter(|t| t.status != Status::Finished && t.wake_at.is_some())
            .filter_map(|t| t.wake_at)
            .min()
    }
    /// A woken sleeper whose turn comes before `me`'s: waits end in deadline
    /// order (then thread order), not in the order the waits were nested.
    fn due_before(&self, me: usize) -> Option<usize> {
        let mine = (self.threads[me].wake_at?, self.threads[me].id);
        self.threads
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                *i != me
                    && t.status == Status::Runnable
                    && !t.pinned
                    && t.saved.is_some()
                    && t.wake_at.is_some_and(|w| (w, t.id) < mine)
            })
            .min_by_key(|(_, t)| (t.wake_at.unwrap_or(i64::MAX), t.id))
            .map(|(i, _)| i)
    }
    pub fn alive(&self) -> usize {
        self.threads
            .iter()
            .filter(|t| t.status != Status::Finished)
            .count()
    }
}

impl Vm<'_> {
    pub fn scheduler_started(&self) -> bool {
        self.sched.is_some()
    }

    /// Creates the scheduler on the first `threading` use.
    pub fn start_scheduler(&mut self) {
        if self.sched.is_none() {
            let seed = self.host.scheduler_seed();
            self.sched = Some(Box::new(Scheduler::new(seed, "MainThread")));
        }
    }

    /// The virtual clock in microseconds since the epoch.
    pub fn now_micros(&self) -> i64 {
        self.host.now_micros() + self.time_offset
    }

    /// Charges executed instructions to the virtual clock (threads make the time
    /// code takes observable, as CPython's GIL does).
    pub fn charge_time(&mut self) {
        let Some(s) = self.sched.as_mut() else {
            return;
        };
        let steps = std::mem::take(&mut s.steps);
        if steps > 0 {
            self.time_offset += (steps as f64 / STEPS_PER_SECOND * 1e6).ceil() as i64;
        }
    }

    /// Called once per instruction while threads exist; `true` when the quantum
    /// ran out and the interpreter should switch.
    #[inline]
    pub fn sched_step(&mut self) -> bool {
        let Some(s) = self.sched.as_mut() else {
            return false;
        };
        s.steps += 1;
        if s.left > 0 {
            s.left -= 1;
            return false;
        }
        true
    }

    /// Whether the interpreter level with this base may swap threads.
    pub fn switchable(&self, base: usize) -> bool {
        self.sched.as_ref().is_some_and(|s| {
            s.levels
                .last()
                .is_some_and(|l| l.base == base && l.native_depth == self.native_depth)
        })
    }

    /// Swaps the running thread for the next runnable one, if there is one.
    pub fn switch_threads(&mut self, f: &mut Box<Frame>) {
        self.charge_time();
        let Some(s) = self.sched.as_mut() else {
            return;
        };
        s.left = s.quantum;
        let cur = s.current;
        let Some(next) = s.runnable(cur) else {
            return;
        };
        self.save_and_resume(f, cur, next);
    }

    /// The running thread returned from its top frame: record that, and hand the
    /// level to another thread unless the level's own thread is the one that
    /// finished (then it ends, and its Rust frame unwinds).
    pub fn finish_current_thread(&mut self, value: Value, f: &mut Box<Frame>) -> PyResult<bool> {
        self.charge_time();
        let (cur, owner) = {
            let s = self.sched.as_mut().unwrap();
            let cur = s.current;
            s.threads[cur].status = Status::Finished;
            s.threads[cur].result = Some(value);
            s.threads[cur].pinned = false;
            (cur, s.levels.last().unwrap().owner)
        };
        if cur == owner {
            let s = self.sched.as_mut().unwrap();
            s.threads[cur].pinned = true;
            return Ok(false);
        }
        match self.pick_or_idle() {
            Ok(next) => {
                self.resume_into(f, next);
                Ok(true)
            }
            // Nothing left to run here: the level ends, and the thread that
            // owns it stays saved for whoever resumes it.
            Err(e) if matches!(e.kind, ErrKind::YieldLevel) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Whether a blocking call may park the running thread here (rather than
    /// running the other threads nested inside it).
    pub fn can_park(&self) -> bool {
        self.sched.as_ref().is_some_and(|s| {
            s.threads.len() > 1
                && s.levels
                    .last()
                    .is_some_and(|l| l.native_depth + 1 == self.native_depth)
        }) && self.park_ok_depth == Some(self.native_depth)
    }

    /// Blocks the running thread: the error unwinds to the interpreter loop.
    pub fn park(&self, wait: Wait, deadline: Option<i64>) -> Box<PyErr> {
        Box::new(PyErr {
            kind: ErrKind::Park(Box::new(ParkInfo { wait, deadline })),
            reraise: false,
            fatal: false,
        })
    }

    /// Checks every parked thread's condition and wakes those that can run,
    /// leaving each the value its blocking call returns.
    fn wake_ready(&mut self) -> PyResult<()> {
        let now = self.now_micros();
        let ids: Vec<usize> = match self.sched.as_ref() {
            Some(s) => (0..s.threads.len())
                .filter(|i| s.threads[*i].status == Status::Waiting && s.threads[*i].wait.is_some())
                .collect(),
            None => return Ok(()),
        };
        for i in ids {
            let (wait, deadline, id) = {
                let s = self.sched.as_ref().unwrap();
                let t = &s.threads[i];
                let w = match t.wait.as_ref().unwrap() {
                    Wait::Deadline => Wait::Deadline,
                    Wait::Lock(l) => Wait::Lock(l.clone()),
                    Wait::Predicate(p) => Wait::Predicate(p.clone()),
                    Wait::Thread(x) => Wait::Thread(*x),
                };
                (w, t.wake_at, t.id)
            };
            let ready = match &wait {
                Wait::Deadline => deadline.is_some_and(|d| now >= d),
                Wait::Lock(l) => with_lock(l, |d| !d.locked),
                Wait::Predicate(p) => {
                    let v = self.call(p, vec![])?;
                    self.truthy(&v)?
                }
                Wait::Thread(other) => {
                    let s = self.sched.as_ref().unwrap();
                    s.index_of(*other)
                        .is_none_or(|j| s.threads[j].status == Status::Finished)
                }
            };
            let timed_out = !ready && deadline.is_some_and(|d| now >= d);
            if !ready && !timed_out {
                continue;
            }
            if ready {
                if let Wait::Lock(l) = &wait {
                    with_lock(l, |d| {
                        d.locked = true;
                        d.owner = id;
                        d.count = 1;
                    });
                }
            }
            let value = match wait {
                Wait::Deadline => Value::None,
                _ => Value::Bool(ready),
            };
            let s = self.sched.as_mut().unwrap();
            let t = &mut s.threads[i];
            t.status = Status::Runnable;
            t.wait = None;
            t.resume = Some(value);
            t.wake_at = None;
        }
        Ok(())
    }

    /// Whether the thread running at this level is still alive (a dead one has
    /// no stack worth saving).
    pub fn thread_running(&self) -> bool {
        self.sched
            .as_ref()
            .is_some_and(|s| s.threads[s.current].status != Status::Finished)
    }

    /// Saves the running thread's stack at this level (it stays resumable) so
    /// the level can unwind.
    pub fn save_running_thread(&mut self, f: &mut Box<Frame>) {
        let (base, exc_base, depth_base) = {
            let l = self.sched.as_ref().unwrap().levels.last().unwrap();
            (l.base, l.exc_base, l.depth_base)
        };
        let placeholder = self.new_frame(f.code.clone(), f.globals.clone(), None);
        let top = std::mem::replace(f, placeholder);
        let frames = self.frames.split_off(base);
        let exc = self.exc_stack.split_off(exc_base);
        let depth = self.depth - depth_base;
        let s = self.sched.as_mut().unwrap();
        let cur = s.current;
        s.threads[cur].saved = Some(Saved {
            top,
            frames,
            exc,
            depth,
        });
        s.threads[cur].pinned = false;
    }

    /// Parks the running thread and gives the interpreter level to another one.
    /// `Ok(false)` when nothing can run here and the level must unwind.
    pub fn park_current(&mut self, f: &mut Box<Frame>, info: ParkInfo) -> PyResult<bool> {
        self.charge_time();
        if std::option_env!("CW_SCHED_DEBUG").is_some() {
            let s = self.sched.as_ref().unwrap();
            eprintln!(
                "[sched] park thread {} in {} pc {} stack {} frames {:?} kind {}",
                s.threads[s.current].id,
                f.code.name,
                f.pc,
                f.stack.len(),
                self.frames
                    .iter()
                    .rev()
                    .take(4)
                    .map(|fr| fr.code.name.to_string())
                    .collect::<Vec<_>>(),
                match &info.wait {
                    Wait::Deadline => "deadline".to_string(),
                    Wait::Lock(l) => {
                        with_lock(l, |d| format!("lock({:?} owner {})", d.locked, d.owner))
                    }
                    Wait::Predicate(_) => "predicate".to_string(),
                    Wait::Thread(_) => "thread".to_string(),
                }
            );
        }
        let sleeping = matches!(info.wait, Wait::Deadline);
        {
            let s = self.sched.as_mut().unwrap();
            let cur = s.current;
            s.threads[cur].status = Status::Waiting;
            s.threads[cur].wait = Some(info.wait);
            s.threads[cur].wake_at = info.deadline;
            s.threads[cur].resume = None;
        }
        let next = match self.pick_or_idle() {
            Ok(i) => i,
            // Nothing can run here, but a waiting thread below can: save this
            // one where it stands and let the level unwind.
            Err(e) if matches!(e.kind, ErrKind::YieldLevel) => {
                self.save_running_thread(f);
                return Ok(false);
            }
            Err(e) => {
                // Nothing can wake this thread: report it where it blocked.
                let s = self.sched.as_mut().unwrap();
                let cur = s.current;
                s.threads[cur].status = Status::Runnable;
                s.threads[cur].wait = None;
                s.threads[cur].wake_at = None;
                return Err(e);
            }
        };
        let _ = sleeping;
        let cur = self.sched.as_ref().unwrap().current;
        if next == cur {
            // The wait was over before another thread had to run: the parked
            // call simply returns its value into the frame it left.
            {
                let s = self.sched.as_mut().unwrap();
                s.threads[cur].pinned = true;
                s.left = s.quantum;
            }
            self.deliver_resume(f, cur);
            return Ok(true);
        }
        self.save_and_resume(f, cur, next);
        Ok(true)
    }

    /// The next thread to run: wakes what it can, and lets the clock reach the
    /// earliest deadline when nothing else is left. `YieldLevel` when only a
    /// thread waiting further down the Rust stack could go on.
    fn pick_or_idle(&mut self) -> PyResult<usize> {
        loop {
            self.wake_ready()?;
            let cur = self.sched.as_ref().unwrap().current;
            // The wait this thread just started is over already (a deadline in
            // the past, or a condition another thread had met): stay put.
            if self.sched.as_ref().unwrap().threads[cur].status == Status::Runnable {
                return Ok(cur);
            }
            if let Some(i) = self.sched.as_ref().unwrap().runnable(cur) {
                return Ok(i);
            }
            let nested_waiter = {
                let s = self.sched.as_ref().unwrap();
                s.threads
                    .iter()
                    .enumerate()
                    .any(|(i, t)| i != cur && t.pinned && t.status != Status::Finished)
            };
            if nested_waiter {
                return Err(yield_level());
            }
            let wake = self.sched.as_ref().unwrap().earliest_wake();
            match wake {
                // Nothing to run: the clock reaches the earliest deadline.
                Some(w) if w > self.now_micros() => {
                    self.time_offset += w - self.now_micros();
                }
                _ => {
                    if std::option_env!("CW_SCHED_DEBUG").is_some() {
                        let s = self.sched.as_ref().unwrap();
                        eprintln!(
                            "[sched] deadlock in pick_or_idle: {:?}",
                            s.threads
                                .iter()
                                .map(|t| (t.id, t.status, t.pinned, t.saved.is_some()))
                                .collect::<Vec<_>>()
                        );
                    }
                    return Err(err(
                        "RuntimeError",
                        "deadlock: every thread is waiting and none can make progress",
                    ));
                }
            }
        }
    }

    /// Makes `next` the running thread at this level, dropping what `f` held.
    fn resume_into(&mut self, f: &mut Box<Frame>, next: usize) {
        let (base, exc_base, depth_base) = {
            let l = self.sched.as_ref().unwrap().levels.last().unwrap();
            (l.base, l.exc_base, l.depth_base)
        };
        let resumed = {
            let s = self.sched.as_mut().unwrap();
            let cur = s.current;
            s.threads[cur].pinned = false;
            s.threads[next].pinned = true;
            s.current = next;
            s.left = s.quantum;
            s.threads[next].saved.take().expect("resumable thread")
        };
        *f = resumed.top;
        self.frames.truncate(base);
        self.exc_stack.truncate(exc_base);
        self.frames.extend(resumed.frames);
        self.exc_stack.extend(resumed.exc);
        self.depth = depth_base + resumed.depth;
        self.deliver_resume(f, next);
    }

    /// A thread that parked in a call resumes with that call's value.
    fn deliver_resume(&mut self, f: &mut Box<Frame>, index: usize) {
        let v = self.sched.as_mut().unwrap().threads[index].resume.take();
        if let Some(v) = v {
            if std::option_env!("CW_SCHED_DEBUG").is_some() {
                let s = self.sched.as_ref().unwrap();
                eprintln!(
                    "[sched] resume thread {} into {} pc {} stack {} value {:?}",
                    s.threads[index].id,
                    f.code.name,
                    f.pc,
                    f.stack.len(),
                    matches!(v, Value::None)
                );
            }
            f.stack.push(v);
        }
    }

    /// Saves the current thread's stack at the innermost level and resumes `next`.
    fn save_and_resume(&mut self, f: &mut Box<Frame>, cur: usize, next: usize) {
        let (base, exc_base, depth_base) = {
            let l = self.sched.as_ref().unwrap().levels.last().unwrap();
            (l.base, l.exc_base, l.depth_base)
        };
        let resumed = {
            let s = self.sched.as_mut().unwrap();
            s.threads[next].saved.take().expect("resumable thread")
        };
        let top = std::mem::replace(f, resumed.top);
        let frames = self.frames.split_off(base);
        let exc = self.exc_stack.split_off(exc_base);
        let depth = self.depth - depth_base;
        {
            let s = self.sched.as_mut().unwrap();
            s.threads[cur].saved = Some(Saved {
                top,
                frames,
                exc,
                depth,
            });
            s.threads[cur].pinned = false;
            s.threads[next].pinned = true;
            s.current = next;
            s.left = s.quantum;
        }
        self.frames.extend(resumed.frames);
        self.exc_stack.extend(resumed.exc);
        self.depth = depth_base + resumed.depth;
        self.deliver_resume(f, next);
    }

    /// Starts a thread running `target(*args, **kwargs)`; returns its identifier.
    pub fn spawn_thread(
        &mut self,
        target: Value,
        args: Value,
        kwargs: Value,
        name: Option<String>,
        daemon: bool,
        obj: Value,
    ) -> PyResult<u64> {
        self.start_scheduler();
        let tramp = match self.sched.as_ref().unwrap().trampoline.clone() {
            Some(t) => t,
            None => {
                let code = crate::compile_source(
                    self,
                    "def __cw_thread__(f, a, k):\n    return f(*a, **k)\n",
                    "<threading>",
                    "exec",
                )?;
                let g = new_ref(Dict::new());
                g.borrow_mut()
                    .set_str("__builtins__", Value::Dict(self.builtins.clone()));
                let frame = self.new_frame(code, g.clone(), None);
                self.charge_depth()?;
                self.execute(frame, None)?;
                let t = g
                    .borrow()
                    .get_str("__cw_thread__")
                    .ok_or_else(|| err("RuntimeError", "thread trampoline missing"))?;
                self.sched.as_mut().unwrap().trampoline = Some(t.clone());
                t
            }
        };
        let Value::Func(func) = &tramp else {
            return Err(err("RuntimeError", "thread trampoline missing"));
        };
        let frame = self.bind_frame(&func.clone(), vec![target, args, kwargs], vec![])?;
        let s = self.sched.as_mut().unwrap();
        let id = s.next_id;
        s.next_id += 1;
        let name = name.unwrap_or_else(|| format!("Thread-{}", id - 1));
        s.threads.push(ThreadState {
            id,
            name,
            daemon,
            status: Status::Runnable,
            pinned: false,
            saved: Some(Saved {
                top: frame,
                frames: vec![],
                exc: vec![],
                depth: 1,
            }),
            wait: None,
            resume: None,
            wake_at: None,
            result: None,
            failed: false,
            obj,
        });
        Ok(id)
    }

    /// Runs one thread from where it stands, at a new interpreter level, until it
    /// (or whichever thread the level ends up running) finishes.
    fn run_slice(&mut self, index: usize) -> PyResult<()> {
        let saved = {
            let s = self.sched.as_mut().unwrap();
            let Some(saved) = s.threads[index].saved.take() else {
                return Ok(());
            };
            s.threads[index].pinned = true;
            s.left = s.quantum;
            saved
        };
        let owner_before = self.sched.as_ref().unwrap().current;
        let base = self.frames.len();
        let exc_base = self.exc_stack.len();
        let depth_base = self.depth;
        {
            let s = self.sched.as_mut().unwrap();
            s.current = index;
            s.levels.push(Level {
                base,
                exc_base,
                depth_base,
                // execute() increments native_depth before running the loop.
                native_depth: self.native_depth + 1,
                owner: index,
            });
        }
        self.frames.extend(saved.frames);
        self.depth = depth_base + saved.depth - 1;
        // A thread that parked in a call resumes with that call's value, just
        // as it would when another level hands it the interpreter.
        let mut top = saved.top;
        self.deliver_resume(&mut top, index);
        let result = self.execute_from(top, None, base);
        // A level that merely gave way keeps its threads; otherwise the thread
        // running when it ended has finished.
        let yielded = matches!(&result, Err(e) if matches!(e.kind, ErrKind::YieldLevel));
        if std::option_env!("CW_SCHED_DEBUG").is_some() {
            let s = self.sched.as_ref().unwrap();
            eprintln!(
                "[sched] slice of {} ended: current {} yielded {} ok {}",
                s.threads[index].id,
                s.threads[s.current].id,
                yielded,
                result.is_ok(),
            );
        }
        let finished = {
            let s = self.sched.as_mut().unwrap();
            let f = s.current;
            s.levels.pop();
            if !yielded {
                s.threads[f].pinned = false;
                s.threads[f].status = Status::Finished;
            }
            s.current = owner_before;
            f
        };
        if yielded {
            self.frames.truncate(base);
            self.exc_stack.truncate(exc_base);
            self.depth = depth_base;
            return Ok(());
        }
        self.frames.truncate(base);
        self.exc_stack.truncate(exc_base);
        self.depth = depth_base;
        match result {
            Ok(Exit::Return(v)) => {
                let s = self.sched.as_mut().unwrap();
                s.threads[finished].result = Some(v);
            }
            Ok(Exit::Yield(..)) => {}
            Err(e) => {
                if e.fatal {
                    return Err(e);
                }
                self.report_thread_exception(finished, e);
            }
        }
        Ok(())
    }

    /// The running thread died with `e`. Unless it owns this interpreter level,
    /// its exception is reported (as `threading.excepthook` does), it ends, and
    /// another thread takes the level over; `Ok(Some(e))` gives the error back
    /// when the level must end with it.
    pub fn fail_current_thread(
        &mut self,
        e: Box<PyErr>,
        f: &mut Box<Frame>,
    ) -> PyResult<Option<Box<PyErr>>> {
        if e.fatal {
            return Ok(Some(e));
        }
        let (cur, owner) = {
            let s = self.sched.as_ref().unwrap();
            (s.current, s.levels.last().unwrap().owner)
        };
        if cur == owner {
            return Ok(Some(e));
        }
        self.report_thread_exception(cur, e);
        {
            let s = self.sched.as_mut().unwrap();
            s.threads[cur].status = Status::Finished;
            s.threads[cur].pinned = false;
        }
        match self.pick_or_idle() {
            Ok(next) => {
                self.resume_into(f, next);
                Ok(None)
            }
            // Nothing left here: unwind the level without blaming its owner.
            Err(e) if matches!(e.kind, ErrKind::YieldLevel) => Ok(Some(e)),
            Err(e) => Err(e),
        }
    }

    /// CPython's `threading.excepthook`: the thread dies, the program goes on.
    fn report_thread_exception(&mut self, index: usize, mut e: Box<PyErr>) {
        let exc = self.materialize(&mut e);
        if self.isinstance(&exc, &self.t.exc("SystemExit")) {
            return;
        }
        let name = self.sched.as_ref().unwrap().threads[index].name.clone();
        let text = crate::format_exception(self, &exc, 0);
        self.write_stderr(&format!("Exception in thread {name}:\n{text}"));
        let s = self.sched.as_mut().unwrap();
        s.threads[index].failed = true;
        s.threads[index].result = Some(exc);
    }

    /// Runs other threads until `ready` holds (or the deadline passes). Returns
    /// whether the condition was met.
    pub fn block_until(
        &mut self,
        mut ready: impl FnMut(&mut Vm) -> PyResult<bool>,
        deadline: Option<i64>,
    ) -> PyResult<bool> {
        if ready(self)? {
            return Ok(true);
        }
        if self.sched.is_none() {
            // No other thread can change anything.
            return match deadline {
                Some(d) => {
                    self.time_offset += (d - self.now_micros()).max(0);
                    Ok(false)
                }
                None => Err(err(
                    "RuntimeError",
                    "deadlock: this wait can never be satisfied (no other thread is running)",
                )),
            };
        }
        let me = self.sched.as_ref().unwrap().current;
        self.sched.as_mut().unwrap().threads[me].status = Status::Waiting;
        self.sched.as_mut().unwrap().threads[me].wake_at = deadline;
        let result = loop {
            self.charge_time();
            // Waits that other threads have satisfied end here too, not only
            // where a thread parks.
            self.wake_ready()?;
            // A sleeper whose deadline came first resumes first, however the
            // waits happen to be nested.
            let earlier = {
                let s = self.sched.as_ref().unwrap();
                s.due_before(me)
            };
            if let Some(i) = earlier {
                self.run_slice(i)?;
                continue;
            }
            if ready(self)? {
                break Ok(true);
            }
            if let Some(d) = deadline {
                if self.now_micros() >= d {
                    break Ok(false);
                }
            }
            let next = {
                let s = self.sched.as_ref().unwrap();
                s.runnable(s.current)
            };
            match next {
                Some(i) => self.run_slice(i)?,
                None => {
                    // Nothing can run: let the clock reach the earliest deadline.
                    let wake = {
                        let s = self.sched.as_ref().unwrap();
                        s.earliest_wake()
                    };
                    match wake {
                        Some(w) if w > self.now_micros() => {
                            self.time_offset += w - self.now_micros();
                            // Wake every thread whose time has come.
                            let now = self.now_micros();
                            let s = self.sched.as_mut().unwrap();
                            for t in s.threads.iter_mut() {
                                // `wake_at` stays set until the thread resumes,
                                // so waits can be ordered by deadline.
                                if t.wake_at.is_some_and(|x| x <= now)
                                    && t.status == Status::Waiting
                                {
                                    t.status = Status::Runnable;
                                }
                            }
                        }
                        _ => {
                            if std::option_env!("CW_SCHED_DEBUG").is_some() {
                                let s = self.sched.as_ref().unwrap();
                                eprintln!(
                                    "[sched] deadlock in block_until: {:?}",
                                    s.threads
                                        .iter()
                                        .map(|t| (t.id, t.status, t.pinned, t.saved.is_some()))
                                        .collect::<Vec<_>>()
                                );
                            }
                            break Err(err(
                                "RuntimeError",
                                "deadlock: every thread is waiting and none can make progress",
                            ));
                        }
                    }
                }
            }
        };
        let s = self.sched.as_mut().unwrap();
        s.threads[me].status = Status::Runnable;
        s.threads[me].wake_at = None;
        result
    }

    /// Waits until `ready` holds, parking the thread where that is possible and
    /// otherwise running the other threads nested inside this call.
    pub fn wait_for(
        &mut self,
        wait: Wait,
        deadline: Option<i64>,
        ready: impl FnMut(&mut Vm) -> PyResult<bool>,
    ) -> PyResult<Value> {
        if self.can_park() {
            return Err(self.park(wait, deadline));
        }
        if let (Some(_), Some(s)) = (std::option_env!("CW_SCHED_DEBUG"), self.sched.as_ref()) {
            eprintln!(
                "[sched] nested wait: threads {} level_nd {:?} nd {} park_ok {:?}",
                s.threads.len(),
                s.levels.last().map(|l| l.native_depth),
                self.native_depth,
                self.park_ok_depth
            );
        }
        let got = self.block_until(ready, deadline)?;
        Ok(match wait {
            Wait::Deadline => Value::None,
            _ => Value::Bool(got),
        })
    }

    /// Waits for a thread to finish (`join`).
    pub fn join_thread(&mut self, id: u64, deadline: Option<i64>) -> PyResult<Value> {
        let index = match self.sched.as_ref().and_then(|s| s.index_of(id)) {
            Some(i) => i,
            None => return Ok(Value::Bool(true)),
        };
        if self.sched.as_ref().unwrap().current == index {
            return Err(err("RuntimeError", "cannot join current thread"));
        }
        if self.sched.as_ref().unwrap().threads[index].status == Status::Finished {
            return Ok(Value::Bool(true));
        }
        self.wait_for(Wait::Thread(id), deadline, move |vm| {
            Ok(vm.sched.as_ref().unwrap().threads[index].status == Status::Finished)
        })
    }

    /// Sleeps the running thread; other threads run meanwhile.
    pub fn thread_sleep(&mut self, seconds: f64) -> PyResult<Value> {
        if self.sched.as_ref().is_none_or(|s| {
            s.threads
                .iter()
                .filter(|t| t.status != Status::Finished)
                .count()
                <= 1
        }) {
            self.time_offset += (seconds * 1e6).ceil() as i64;
            return Ok(Value::None);
        }
        let deadline = self.now_micros() + (seconds * 1e6).ceil() as i64;
        self.wait_for(Wait::Deadline, Some(deadline), move |vm| {
            Ok(vm.now_micros() >= deadline)
        })
    }

    /// Runs the remaining non-daemon threads, as the interpreter does at exit.
    pub fn shutdown_threads(&mut self) {
        if self.sched.is_none() {
            return;
        }
        loop {
            let next = {
                let s = self.sched.as_ref().unwrap();
                let pending: Option<usize> = s
                    .threads
                    .iter()
                    .position(|t| !t.daemon && t.status != Status::Finished && t.saved.is_some());
                match pending {
                    Some(i) if !s.threads[i].pinned => Some(i),
                    _ => None,
                }
            };
            match next {
                Some(i) => {
                    // A thread still waiting on a deadline gets its time.
                    let wake = self.sched.as_ref().unwrap().threads[i].wake_at;
                    if let Some(w) = wake {
                        if w > self.now_micros() {
                            self.time_offset += w - self.now_micros();
                        }
                        let s = self.sched.as_mut().unwrap();
                        s.threads[i].wake_at = None;
                        s.threads[i].status = Status::Runnable;
                    }
                    if self.run_slice(i).is_err() {
                        return;
                    }
                }
                None => return,
            }
        }
    }
}

/// A lock, shared by every Python value that refers to it.
#[derive(Default)]
pub struct LockData {
    pub locked: bool,
    pub owner: u64,
    pub count: u32,
    pub reentrant: bool,
}

pub fn lock_value(vm: &Vm, reentrant: bool) -> Value {
    Value::Native(Rc::new(Native {
        class: vm.t.object.clone(),
        data: std::cell::RefCell::new(NativeKind::Boxed(Box::new(std::cell::RefCell::new(
            LockData {
                reentrant,
                ..Default::default()
            },
        )))),
    }))
}

/// The shared cell behind a lock value.
pub fn lock_of(v: &Value) -> Option<Rc<Native>> {
    match v {
        Value::Native(n) => {
            let is_lock = matches!(&*n.data.borrow(), NativeKind::Boxed(b) if b.downcast_ref::<std::cell::RefCell<LockData>>().is_some());
            is_lock.then(|| n.clone())
        }
        _ => None,
    }
}

pub fn with_lock<R>(n: &Rc<Native>, f: impl FnOnce(&mut LockData) -> R) -> R {
    let data = n.data.borrow();
    let NativeKind::Boxed(b) = &*data else {
        unreachable!("lock")
    };
    let cell = b
        .downcast_ref::<std::cell::RefCell<LockData>>()
        .expect("lock");
    let mut d = cell.borrow_mut();
    f(&mut d)
}
