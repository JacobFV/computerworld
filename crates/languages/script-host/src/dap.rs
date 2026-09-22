//! A Debug Adapter Protocol session over a deterministic runtime.
//!
//! [`DapSession`] is the stateful half of a debug adapter: it takes DAP-shaped
//! [`Request`]s and answers with a [`Response`] and the [`Event`]s DAP would send
//! (`stopped`, `output`, `exited`, `terminated`...). It never keeps an interpreter
//! alive between requests. Every request that needs the stopped program reruns it
//! from the start through a [`DebugRunner`]: host calls are answered from the
//! session's [`Journal`] (so nothing happens twice in the world), and at each
//! earlier stop the recorded actions (breakpoint edits, evaluations, the resume
//! command) are re-applied, which brings the program back to exactly the state it
//! was stopped in. The session is plain data, so it can
//! live in serialized application state and survive snapshots.
//!
//! Frame ids come from the runtime and are stable at a given stop. Variable
//! references handed to the client are the session's own: each names a *path*
//! (frame, scope, child names...) that is walked again on the replayed program.
use crate::debug::*;
use crate::journal::Journal;
use crate::Outcome;

/// Runs the program from the start under a debugger.
pub trait DebugRunner {
    /// Rerun the debuggee under `debugger`, answering host calls from `journal`
    /// first. Returns the run's streams and status, how it ended, and the journal
    /// extended with the calls it made past the recorded ones.
    fn run(
        &mut self,
        debugger: &mut dyn Debugger,
        journal: Journal,
    ) -> (Outcome, DebugRunInfo, Journal);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Initialize,
    /// Start the program (after the configuration requests, as DAP orders them).
    Launch {
        stop_on_entry: bool,
    },
    SetBreakpoints {
        path: String,
        breakpoints: Vec<SourceBreakpoint>,
    },
    /// Filters: `"raised"`, `"uncaught"`.
    SetExceptionBreakpoints {
        filters: Vec<String>,
    },
    ConfigurationDone,
    Threads,
    StackTrace {
        thread_id: u64,
    },
    Scopes {
        frame_id: u64,
    },
    Variables {
        variables_reference: u64,
    },
    Evaluate {
        expression: String,
        frame_id: Option<u64>,
    },
    SetVariable {
        variables_reference: u64,
        name: String,
        value: String,
    },
    Continue,
    Next,
    StepIn,
    StepOut,
    /// Stop a running program. Runs are synchronous here, so this arms the
    /// session's instruction slice: the next resume stops with reason `pause`
    /// after `pause_after` instructions if nothing else stops it first.
    Pause,
    Disconnect,
    Terminate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_conditional_breakpoints: bool,
    pub supports_hit_conditional_breakpoints: bool,
    pub supports_log_points: bool,
    pub supports_evaluate_for_hovers: bool,
    pub supports_set_variable: bool,
    pub supports_terminate_request: bool,
    /// `raised` and `uncaught`.
    pub exception_breakpoint_filters: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointInfo {
    pub id: u64,
    pub verified: bool,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    None,
    Capabilities(Capabilities),
    Breakpoints(Vec<BreakpointInfo>),
    Threads(Vec<Thread>),
    StackTrace(Vec<StackFrame>),
    Scopes(Vec<Scope>),
    Variables(Vec<Variable>),
    Evaluate(Variable),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Ok(Body),
    Err(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Initialized,
    Stopped(StopEvent),
    /// `category` is `stdout`, `stderr` or `console` (logpoints).
    Output {
        category: String,
        output: String,
    },
    Exited {
        exit_code: i32,
    },
    Terminated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    SetBreakpoints(String, Vec<SourceBreakpoint>),
    SetExceptions(ExceptionFilters),
    Evaluate(String, Option<u64>),
    SetVariable(VarPath, String, String),
    Resume(Step),
}

/// Where a variable lives, re-walkable on a replayed program.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VarRoot {
    Scope {
        frame_id: u64,
        scope: String,
    },
    Eval {
        expression: String,
        frame_id: Option<u64>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct VarPath {
    root: VarRoot,
    children: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Configuring,
    Stopped(StopEvent),
    Ended(i32),
}

/// The session: configuration, what was done at each stop, the host journal and
/// what the client has already been shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DapSession {
    config: DebugConfig,
    /// Configuration in force when the program was launched.
    launch_config: DebugConfig,
    /// `stops[k]`: what was done while stopped at the k-th stop, in order.
    stops: Vec<Vec<Action>>,
    /// `slices[k]`: the pause slice the run that reached stop `k` ran under.
    slices: Vec<Option<u64>>,
    journal: Journal,
    state: State,
    refs: Vec<VarPath>,
    stdout_seen: usize,
    stderr_seen: usize,
    log_seen: usize,
    pause_armed: bool,
    /// Instruction slice used when a pause is armed.
    pub pause_after: u64,
}

impl Default for DapSession {
    fn default() -> Self {
        Self {
            config: DebugConfig::default(),
            launch_config: DebugConfig::default(),
            stops: vec![],
            slices: vec![],
            journal: Journal::default(),
            state: State::Configuring,
            refs: vec![],
            stdout_seen: 0,
            stderr_seen: 0,
            log_seen: 0,
            pause_armed: false,
            pause_after: 1_000_000,
        }
    }
}

/// What the replaying debugger does at the stop it was sent to.
enum Query {
    /// Only record the stop (after a resume).
    None,
    Threads,
    StackTrace(u64),
    Scopes(u64),
    Variables(VarPath),
    Evaluate(String, Option<u64>),
    SetVariable(VarPath, String, String),
}
enum Answer {
    None,
    Threads(Vec<Thread>),
    StackTrace(Vec<StackFrame>),
    Scopes(Vec<Scope>),
    Variables(Vec<Variable>),
    Value(Result<Variable, String>),
}

struct Replayer<'s> {
    config: DebugConfig,
    stops: &'s [Vec<Action>],
    /// Slice for the segment ending at each stop (and one past, for a new stop).
    slices: Vec<Option<u64>>,
    /// Index of the stop where the query runs (== stops.len() - 1 when stopped,
    /// stops.len() after a resume: the next new stop).
    target: usize,
    seen: usize,
    query: Option<Query>,
    answer: Answer,
    event: Option<StopEvent>,
    logs: String,
}

fn walk(target: &mut dyn DebugTarget, path: &VarPath) -> Result<u64, String> {
    let mut reference = match &path.root {
        VarRoot::Scope { frame_id, scope } => target
            .scopes(*frame_id)
            .into_iter()
            .find(|s| &s.name == scope)
            .map(|s| s.variables_reference)
            .ok_or_else(|| format!("no scope {scope}"))?,
        VarRoot::Eval {
            expression,
            frame_id,
        } => target.evaluate(expression, *frame_id)?.variables_reference,
    };
    for name in &path.children {
        reference = target
            .variables(reference)
            .into_iter()
            .find(|v| &v.name == name)
            .map(|v| v.variables_reference)
            .filter(|r| *r != 0)
            .ok_or_else(|| format!("no variable {name}"))?;
    }
    Ok(reference)
}

fn apply(config: &mut DebugConfig, target: &mut dyn DebugTarget, a: &Action) {
    match a {
        Action::SetBreakpoints(p, b) => {
            config.breakpoints.insert(p.clone(), b.clone());
        }
        Action::SetExceptions(f) => config.exceptions = *f,
        Action::Evaluate(e, f) => {
            let _ = target.evaluate(e, *f);
        }
        Action::SetVariable(path, name, value) => {
            if let Ok(r) = walk(target, path) {
                let _ = target.set_variable(r, name, value);
            }
        }
        Action::Resume(_) => {}
    }
}

impl Debugger for Replayer<'_> {
    fn config(&self) -> &DebugConfig {
        &self.config
    }
    fn log(&mut self, text: &str) {
        self.logs.push_str(text);
    }
    fn stopped(&mut self, event: &StopEvent, target: &mut dyn DebugTarget) -> Step {
        let k = self.seen;
        self.seen += 1;
        let actions: &[Action] = self.stops.get(k).map(|v| v.as_slice()).unwrap_or(&[]);
        let mut resume = None;
        for a in actions {
            if let Action::Resume(s) = a {
                resume = Some(*s);
            } else {
                apply(&mut self.config, target, a);
            }
        }
        if k < self.target {
            // An earlier stop: go on exactly as the client did, under the slice the
            // next segment originally ran with.
            self.config.pause_after = self.slices.get(k + 1).copied().flatten();
            return resume.unwrap_or(Step::Continue);
        }
        self.event = Some(event.clone());
        self.answer = match self.query.take() {
            None | Some(Query::None) => Answer::None,
            Some(Query::Threads) => Answer::Threads(target.threads()),
            Some(Query::StackTrace(t)) => Answer::StackTrace(target.stack_trace(t)),
            Some(Query::Scopes(f)) => Answer::Scopes(target.scopes(f)),
            Some(Query::Variables(p)) => match walk(target, &p) {
                Ok(r) => Answer::Variables(target.variables(r)),
                Err(_) => Answer::Variables(vec![]),
            },
            Some(Query::Evaluate(e, f)) => Answer::Value(target.evaluate(&e, f)),
            Some(Query::SetVariable(p, n, v)) => {
                Answer::Value(walk(target, &p).and_then(|r| target.set_variable(r, &n, &v)))
            }
        };
        Step::Suspend
    }
}

impl DapSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// A session that carries on from host calls already made: the journal
    /// answers them, so replaying the program does not repeat them. This is
    /// how a session kept as data (in a world's snapshot) comes back to life.
    pub fn with_journal(journal: Journal) -> Self {
        Self {
            journal,
            ..Self::default()
        }
    }

    pub fn capabilities() -> Capabilities {
        Capabilities {
            supports_configuration_done_request: true,
            supports_conditional_breakpoints: true,
            supports_hit_conditional_breakpoints: true,
            supports_log_points: true,
            supports_evaluate_for_hovers: true,
            supports_set_variable: true,
            supports_terminate_request: true,
            exception_breakpoint_filters: vec![
                ("raised".into(), "Raised Exceptions".into()),
                ("uncaught".into(), "Uncaught Exceptions".into()),
            ],
        }
    }

    /// Whether the debuggee is stopped (inspection requests are answerable).
    pub fn is_stopped(&self) -> bool {
        matches!(self.state, State::Stopped(_))
    }
    pub fn exit_code(&self) -> Option<i32> {
        match self.state {
            State::Ended(c) => Some(c),
            _ => None,
        }
    }
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    fn stop_index(&self) -> usize {
        self.stops.len().saturating_sub(1)
    }

    /// Replays to stop `target` and runs `query` there.
    fn replay(
        &mut self,
        runner: &mut dyn DebugRunner,
        target: usize,
        query: Query,
        slice: Option<u64>,
    ) -> (Answer, Option<StopEvent>, Outcome, DebugRunInfo, String) {
        let mut slices = self.slices.clone();
        slices.truncate(target);
        slices.push(slice);
        let mut config = self.launch_config.clone();
        config.pause_after = slices[0];
        let mut r = Replayer {
            config,
            stops: &self.stops,
            slices,
            target,
            seen: 0,
            query: Some(query),
            answer: Answer::None,
            event: None,
            logs: String::new(),
        };
        let (outcome, info, journal) = runner.run(&mut r, self.journal.clone());
        let (answer, event, logs) = (r.answer, r.event, r.logs);
        self.journal = journal;
        (answer, event, outcome, info, logs)
    }

    fn new_output(&mut self, outcome: &Outcome, logs: &str, events: &mut Vec<Event>) {
        for (category, text, seen) in [
            ("stdout", &outcome.stdout, &mut self.stdout_seen),
            ("stderr", &outcome.stderr, &mut self.stderr_seen),
            ("console", &logs.to_string(), &mut self.log_seen),
        ] {
            if text.len() > *seen {
                let mut from = *seen;
                while !text.is_char_boundary(from) {
                    from -= 1;
                }
                events.push(Event::Output {
                    category: category.into(),
                    output: text[from..].to_string(),
                });
                *seen = text.len();
            }
        }
    }

    fn register(&mut self, path: VarPath) -> u64 {
        if let Some(i) = self.refs.iter().position(|p| *p == path) {
            return i as u64 + 1;
        }
        self.refs.push(path);
        self.refs.len() as u64
    }

    fn rewrite(&mut self, parent: &VarPath, vars: Vec<Variable>) -> Vec<Variable> {
        vars.into_iter()
            .map(|mut v| {
                if v.variables_reference != 0 {
                    let mut p = parent.clone();
                    p.children.push(v.name.clone());
                    v.variables_reference = self.register(p);
                }
                v
            })
            .collect()
    }

    /// Runs forward from the current stop with `step` (or starts the program).
    fn resume(&mut self, runner: &mut dyn DebugRunner, step: Option<Step>) -> Vec<Event> {
        if let Some(s) = step {
            if let Some(last) = self.stops.last_mut() {
                last.push(Action::Resume(s));
            }
        }
        let target = self.stops.len();
        let slice = if std::mem::take(&mut self.pause_armed) {
            Some(self.pause_after)
        } else {
            None
        };
        // A pause slice counts instructions from the resume point; every earlier
        // segment replays under the slice it originally had.
        let (_, event, outcome, info, logs) = self.replay(runner, target, Query::None, slice);
        let mut events = vec![];
        self.new_output(&outcome, &logs, &mut events);
        self.refs.clear();
        match event {
            Some(ev) if info.suspended => {
                self.stops.push(vec![]);
                self.slices.push(slice);
                self.state = State::Stopped(ev.clone());
                events.push(Event::Stopped(ev));
            }
            _ => {
                self.state = State::Ended(outcome.exit_code);
                events.push(Event::Exited {
                    exit_code: outcome.exit_code,
                });
                events.push(Event::Terminated);
            }
        }
        events
    }

    fn query(&mut self, runner: &mut dyn DebugRunner, q: Query) -> Result<Answer, String> {
        if !self.is_stopped() {
            return Err("the program is not stopped".into());
        }
        let target = self.stop_index();
        let slice = self.slices.get(target).copied().flatten();
        let (answer, _, _, _, _) = self.replay(runner, target, q, slice);
        Ok(answer)
    }

    /// Handles one request. Inspection and stepping rerun the program through
    /// `runner`; configuration requests do not.
    pub fn handle(
        &mut self,
        request: Request,
        runner: &mut dyn DebugRunner,
    ) -> (Response, Vec<Event>) {
        let ok = |b| Response::Ok(b);
        match request {
            Request::Initialize => (
                ok(Body::Capabilities(Self::capabilities())),
                vec![Event::Initialized],
            ),
            Request::SetBreakpoints { path, breakpoints } => {
                let infos: Vec<BreakpointInfo> = {
                    let mut c = self.config.clone();
                    c.breakpoints.insert(path.clone(), breakpoints.clone());
                    breakpoints
                        .iter()
                        .map(|b| BreakpointInfo {
                            id: c.breakpoint_id(&path, b.line).unwrap_or(0),
                            verified: true,
                            line: b.line,
                        })
                        .collect()
                };
                self.config
                    .breakpoints
                    .insert(path.clone(), breakpoints.clone());
                match &self.state {
                    State::Configuring => {
                        self.launch_config.breakpoints = self.config.breakpoints.clone()
                    }
                    State::Stopped(_) => self
                        .stops
                        .last_mut()
                        .unwrap()
                        .push(Action::SetBreakpoints(path, breakpoints)),
                    State::Ended(_) => {}
                }
                (ok(Body::Breakpoints(infos)), vec![])
            }
            Request::SetExceptionBreakpoints { filters } => {
                let f = ExceptionFilters {
                    raised: filters.iter().any(|f| f == "raised"),
                    uncaught: filters.iter().any(|f| f == "uncaught"),
                };
                self.config.exceptions = f;
                match &self.state {
                    State::Configuring => self.launch_config.exceptions = f,
                    State::Stopped(_) => self
                        .stops
                        .last_mut()
                        .unwrap()
                        .push(Action::SetExceptions(f)),
                    State::Ended(_) => {}
                }
                (ok(Body::None), vec![])
            }
            Request::Launch { stop_on_entry } => {
                self.launch_config.stop_on_entry = stop_on_entry;
                self.config.stop_on_entry = stop_on_entry;
                (ok(Body::None), vec![])
            }
            Request::ConfigurationDone => {
                if self.state != State::Configuring {
                    return (Response::Err("already running".into()), vec![]);
                }
                let events = self.resume(runner, None);
                (ok(Body::None), events)
            }
            Request::Continue | Request::Next | Request::StepIn | Request::StepOut => {
                if !self.is_stopped() {
                    return (Response::Err("the program is not stopped".into()), vec![]);
                }
                let step = match request {
                    Request::Continue => Step::Continue,
                    Request::Next => Step::Next,
                    Request::StepIn => Step::StepIn,
                    _ => Step::StepOut,
                };
                let events = self.resume(runner, Some(step));
                (ok(Body::None), events)
            }
            Request::Pause => {
                self.pause_armed = true;
                (ok(Body::None), vec![])
            }
            Request::Threads => match self.query(runner, Query::Threads) {
                Ok(Answer::Threads(t)) => (ok(Body::Threads(t)), vec![]),
                Ok(_) => (ok(Body::Threads(vec![])), vec![]),
                Err(e) => (Response::Err(e), vec![]),
            },
            Request::StackTrace { thread_id } => {
                match self.query(runner, Query::StackTrace(thread_id)) {
                    Ok(Answer::StackTrace(f)) => (ok(Body::StackTrace(f)), vec![]),
                    Ok(_) => (ok(Body::StackTrace(vec![])), vec![]),
                    Err(e) => (Response::Err(e), vec![]),
                }
            }
            Request::Scopes { frame_id } => match self.query(runner, Query::Scopes(frame_id)) {
                Ok(Answer::Scopes(scopes)) => {
                    let scopes = scopes
                        .into_iter()
                        .map(|mut s| {
                            s.variables_reference = self.register(VarPath {
                                root: VarRoot::Scope {
                                    frame_id,
                                    scope: s.name.clone(),
                                },
                                children: vec![],
                            });
                            s
                        })
                        .collect();
                    (ok(Body::Scopes(scopes)), vec![])
                }
                Ok(_) => (ok(Body::Scopes(vec![])), vec![]),
                Err(e) => (Response::Err(e), vec![]),
            },
            Request::Variables {
                variables_reference,
            } => {
                let Some(path) = self
                    .refs
                    .get((variables_reference as usize).wrapping_sub(1))
                    .cloned()
                else {
                    return (Response::Err("unknown variables reference".into()), vec![]);
                };
                match self.query(runner, Query::Variables(path.clone())) {
                    Ok(Answer::Variables(v)) => {
                        let v = self.rewrite(&path, v);
                        (ok(Body::Variables(v)), vec![])
                    }
                    Ok(_) => (ok(Body::Variables(vec![])), vec![]),
                    Err(e) => (Response::Err(e), vec![]),
                }
            }
            Request::Evaluate {
                expression,
                frame_id,
            } => {
                let q = Query::Evaluate(expression.clone(), frame_id);
                let result = self.query(runner, q);
                if self.is_stopped() {
                    self.stops
                        .last_mut()
                        .unwrap()
                        .push(Action::Evaluate(expression.clone(), frame_id));
                }
                match result {
                    Ok(Answer::Value(Ok(mut v))) => {
                        if v.variables_reference != 0 {
                            v.variables_reference = self.register(VarPath {
                                root: VarRoot::Eval {
                                    expression,
                                    frame_id,
                                },
                                children: vec![],
                            });
                        }
                        (ok(Body::Evaluate(v)), vec![])
                    }
                    Ok(Answer::Value(Err(e))) => (Response::Err(e), vec![]),
                    Ok(_) => (Response::Err("evaluation failed".into()), vec![]),
                    Err(e) => (Response::Err(e), vec![]),
                }
            }
            Request::SetVariable {
                variables_reference,
                name,
                value,
            } => {
                let Some(path) = self
                    .refs
                    .get((variables_reference as usize).wrapping_sub(1))
                    .cloned()
                else {
                    return (Response::Err("unknown variables reference".into()), vec![]);
                };
                let q = Query::SetVariable(path.clone(), name.clone(), value.clone());
                let result = self.query(runner, q);
                if self.is_stopped() {
                    self.stops
                        .last_mut()
                        .unwrap()
                        .push(Action::SetVariable(path, name, value));
                }
                match result {
                    Ok(Answer::Value(Ok(v))) => (ok(Body::Evaluate(v)), vec![]),
                    Ok(Answer::Value(Err(e))) => (Response::Err(e), vec![]),
                    Ok(_) => (Response::Err("assignment failed".into()), vec![]),
                    Err(e) => (Response::Err(e), vec![]),
                }
            }
            Request::Disconnect | Request::Terminate => {
                if self.is_stopped() {
                    self.stops
                        .last_mut()
                        .unwrap()
                        .push(Action::Resume(Step::Terminate));
                }
                self.state = State::Ended(match self.state {
                    State::Ended(c) => c,
                    _ => 143,
                });
                (ok(Body::None), vec![Event::Terminated])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toy "program": lines 1..=5, variable `n` equals the line number; a
    /// debugger is consulted at every line.
    struct Toy {
        runs: usize,
    }
    struct ToyTarget {
        line: u32,
    }
    impl DebugTarget for ToyTarget {
        fn threads(&mut self) -> Vec<Thread> {
            vec![Thread {
                id: 1,
                name: "MainThread".into(),
            }]
        }
        fn stack_trace(&mut self, _: u64) -> Vec<StackFrame> {
            vec![StackFrame {
                id: 1,
                name: "<module>".into(),
                path: "/toy".into(),
                line: self.line,
                column: 1,
            }]
        }
        fn scopes(&mut self, _: u64) -> Vec<Scope> {
            vec![Scope {
                name: "Locals".into(),
                variables_reference: 7,
                expensive: false,
            }]
        }
        fn variables(&mut self, r: u64) -> Vec<Variable> {
            assert_eq!(r, 7);
            vec![Variable {
                name: "n".into(),
                value: self.line.to_string(),
                type_name: "int".into(),
                variables_reference: 0,
                named_variables: 0,
                indexed_variables: 0,
            }]
        }
        fn evaluate(&mut self, e: &str, _: Option<u64>) -> Result<Variable, String> {
            Ok(Variable {
                name: e.into(),
                value: format!("{}", self.line * 10),
                type_name: "int".into(),
                variables_reference: 0,
                named_variables: 0,
                indexed_variables: 0,
            })
        }
        fn set_variable(&mut self, _: u64, _: &str, _: &str) -> Result<Variable, String> {
            Err("read-only".into())
        }
    }
    impl DebugRunner for Toy {
        fn run(&mut self, d: &mut dyn Debugger, j: Journal) -> (Outcome, DebugRunInfo, Journal) {
            self.runs += 1;
            let mut out = String::new();
            let mut stepping = d.config().stop_on_entry;
            for line in 1..=5u32 {
                out.push_str(&format!("line {line}\n"));
                let bp = d.config().breakpoint_id("/toy", line);
                if stepping || bp.is_some() {
                    let ev = StopEvent {
                        reason: if bp.is_some() && !stepping {
                            StopReason::Breakpoint
                        } else {
                            StopReason::Step
                        },
                        thread_id: 1,
                        description: String::new(),
                        text: String::new(),
                        hit_breakpoint_ids: bp.into_iter().collect(),
                    };
                    match d.stopped(&ev, &mut ToyTarget { line }) {
                        Step::Suspend => {
                            return (
                                Outcome::new(out, "", 0),
                                DebugRunInfo {
                                    suspended: true,
                                    terminated: false,
                                },
                                j,
                            )
                        }
                        Step::Next | Step::StepIn => stepping = true,
                        _ => stepping = false,
                    }
                }
            }
            (Outcome::new(out, "", 0), DebugRunInfo::default(), j)
        }
    }

    #[test]
    fn breakpoints_stepping_and_inspection_replay_deterministically() {
        let mut toy = Toy { runs: 0 };
        let mut s = DapSession::new();
        s.handle(Request::Initialize, &mut toy);
        s.handle(
            Request::SetBreakpoints {
                path: "/toy".into(),
                breakpoints: vec![SourceBreakpoint {
                    line: 2,
                    ..Default::default()
                }],
            },
            &mut toy,
        );
        let (_, ev) = s.handle(Request::ConfigurationDone, &mut toy);
        assert!(
            matches!(&ev[..], [Event::Output { output, .. }, Event::Stopped(e)]
            if output == "line 1\nline 2\n" && e.reason == StopReason::Breakpoint)
        );
        let (_, ev) = s.handle(Request::Next, &mut toy);
        assert!(
            matches!(&ev[..], [Event::Output { output, .. }, Event::Stopped(_)]
            if output == "line 3\n")
        );
        let (r, _) = s.handle(Request::StackTrace { thread_id: 1 }, &mut toy);
        assert!(matches!(r, Response::Ok(Body::StackTrace(f)) if f[0].line == 3));
        let (r, _) = s.handle(Request::Scopes { frame_id: 1 }, &mut toy);
        let Response::Ok(Body::Scopes(scopes)) = r else {
            panic!()
        };
        let (r, _) = s.handle(
            Request::Variables {
                variables_reference: scopes[0].variables_reference,
            },
            &mut toy,
        );
        assert!(matches!(r, Response::Ok(Body::Variables(v)) if v[0].value == "3"));
        let (_, ev) = s.handle(Request::Continue, &mut toy);
        assert!(matches!(ev.last(), Some(Event::Terminated)));
        assert_eq!(s.exit_code(), Some(0));
    }
}
