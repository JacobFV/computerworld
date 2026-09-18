//! Debugging a program on this machine: the seam between the Run and Debug view and
//! the language runtimes.
//!
//! The view speaks [`cw_protocol::debug`]: it asks a machine to launch a program, to
//! step it, to read its variables and to evaluate expressions in a frame. A machine
//! answers by handing the request to the [`DebugAdapter`] for that runtime.
//!
//! A paused program is state like any other here, so a world snapshot holds it: an
//! adapter keeps whatever it needs in [`Session::state`], which the machine stores and
//! hands back with the next request. An adapter that cannot serialise a live
//! interpreter can keep a recipe instead — the program and how far it has run — and
//! replay it, which is exact because the world is deterministic.
//!
//! There are no adapters in this build: `cw-pyvm` and `cw-jsvm` run a program to
//! completion and have no stepping API, and a debugger belongs in the interpreter that
//! executes the lines, not in a second one beside it. Until they have one, a machine
//! answers every debug request with the reason it cannot serve it, and the Run and
//! Debug view says so rather than pretending to stop anywhere. To finish the wiring:
//!
//! 1. Implement [`DebugAdapter`] over the runtime's stepping API (one type per runtime,
//!    `kind()` returning `python` or `node`).
//! 2. Return them from [`adapters`].
//!
//! Nothing above this file changes: the view, the effects and the environment already
//! carry every request and reply the protocol has.
use crate::Computer;
use cw_protocol::debug::{
    ExceptionFilters, Launch, Reply, Request, SourceBreakpoint, State, Step, Variable,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A program stopped under a debugger. `state` is the adapter's own, opaque to
/// everything else, and travels with the machine's snapshot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: u64,
    /// The runtime debugging it: `python` or `node`.
    pub kind: String,
    /// The program's absolute path on the machine.
    pub program: String,
    /// Whether the program has ended; a finished session answers nothing but
    /// `Terminate`.
    #[serde(default)]
    pub done: bool,
    /// The adapter's own state: its interpreter, its cursor, or the recipe to replay.
    #[serde(default)]
    pub state: Value,
}

/// Every session a machine is holding.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DebugTable {
    pub sessions: Vec<Session>,
    /// The handle the next session gets; ids are never reused within a machine.
    #[serde(default)]
    pub next: u64,
}
/// A debugger for one of a machine's language runtimes, shaped like the Debug Adapter
/// Protocol. Every method is handed the machine, so the debugger sees the same files,
/// clock and entropy the program does, and the session it is about.
pub trait DebugAdapter {
    /// The runtime this debugs: `python` or `node`.
    fn kind(&self) -> &str;
    /// Start `launch.program`, stopping at its first line when `stop_on_entry`, at the
    /// first breakpoint it reaches otherwise. The `Session` returned is stored by the
    /// machine and handed back with every later request.
    fn launch(
        &self,
        machine: &mut Computer,
        tick: u64,
        launch: &Launch,
    ) -> Result<(Session, State), String>;
    /// Replace the breakpoints in one file. The answer says, for each in order, whether
    /// the runtime can stop there: a blank line or a comment comes back `false`.
    fn breakpoints(
        &self,
        session: &mut Session,
        path: &str,
        points: &[SourceBreakpoint],
    ) -> Vec<bool>;
    /// Which exceptions stop the program from here on.
    fn exceptions(&self, session: &mut Session, filters: ExceptionFilters);
    /// Carry on, and stop again: at a breakpoint, after a step, on an exception, or at
    /// the end of the program.
    fn resume(
        &self,
        machine: &mut Computer,
        tick: u64,
        session: &mut Session,
        step: Step,
    ) -> Result<State, String>;
    /// Stop the program where it is. A runtime that cannot interrupt a run says so.
    fn pause(&self, session: &mut Session) -> Result<State, String>;
    /// What a reference holds: a frame's scope, or a value inside another value.
    fn variables(&self, session: &Session, reference: u64) -> Result<Vec<Variable>, String>;
    /// Evaluate `expression` in frame `frame`: the watch list and the Debug Console.
    fn evaluate(
        &self,
        machine: &mut Computer,
        session: &mut Session,
        frame: u64,
        expression: &str,
        context: &str,
    ) -> Result<Variable, String>;
    /// End the session, killing the program if it is still running.
    fn terminate(&self, machine: &mut Computer, session: &mut Session);
}

/// The debug adapters this build has, by runtime. Empty: see this module's note.
pub fn adapters() -> &'static [&'static dyn DebugAdapter] {
    &[]
}

/// Why a machine cannot debug `kind`.
pub fn unavailable(kind: &str) -> String {
    let kind = match kind {
        "python" => "Python",
        "node" => "Node.js",
        "" => "this program",
        other => other,
    };
    format!("no debug adapter for {kind} is installed on this machine")
}

impl Computer {
    /// Serve one Run and Debug request with the adapters this build has.
    pub fn debug(&mut self, tick: u64, request: &Request) -> Result<Reply, String> {
        self.debug_with(adapters(), tick, request)
    }
    /// Run `f` over one session: the machine holds the sessions, so the adapter is
    /// handed a copy to move and the machine keeps what it did with it.
    fn with_session<R>(
        &mut self,
        id: u64,
        f: impl FnOnce(&mut Computer, &mut Session) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut table = std::mem::take(&mut self.debug);
        let Some(mut session) = table.sessions.iter().find(|s| s.id == id).cloned() else {
            self.debug = table;
            return Err(format!("debug session {id} is not running"));
        };
        let out = f(self, &mut session);
        if let Some(slot) = table.sessions.iter_mut().find(|s| s.id == id) {
            *slot = session;
        }
        self.debug = table;
        out
    }
    /// Serve one request with `adapters`, whatever they are: the machine keeps the
    /// sessions, the adapter moves the program.
    pub fn debug_with(
        &mut self,
        adapters: &[&dyn DebugAdapter],
        tick: u64,
        request: &Request,
    ) -> Result<Reply, String> {
        let find = |kind: &str| {
            adapters
                .iter()
                .find(|a| a.kind() == kind)
                .copied()
                .ok_or_else(|| unavailable(kind))
        };
        // Every request but a launch is about a session the machine already holds, and
        // the adapter that serves it is the one whose runtime started it.
        let owner = |machine: &Computer, id: u64| -> Result<String, String> {
            machine
                .debug
                .sessions
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.kind.clone())
                .ok_or_else(|| format!("debug session {id} is not running"))
        };
        match request {
            Request::Launch(launch) => {
                let adapter = find(&launch.kind)?;
                let (mut session, state) = adapter.launch(self, tick, launch)?;
                session.id = self.debug.next.max(1);
                session.kind.clone_from(&launch.kind);
                session.program.clone_from(&launch.program);
                session.done = state.stopped.is_exit();
                self.debug.next = session.id + 1;
                let id = session.id;
                self.debug.sessions.push(session);
                Ok(Reply::Launched { session: id, state })
            }
            Request::Breakpoints {
                session,
                path,
                points,
            } => {
                let adapter = find(&owner(self, *session)?)?;
                let verified =
                    self.with_session(*session, |_, s| Ok(adapter.breakpoints(s, path, points)))?;
                Ok(Reply::Breakpoints { verified })
            }
            Request::Exceptions { session, filters } => {
                let adapter = find(&owner(self, *session)?)?;
                self.with_session(*session, |_, s| {
                    adapter.exceptions(s, *filters);
                    Ok(())
                })?;
                Ok(Reply::Breakpoints { verified: vec![] })
            }
            Request::Resume { session, step } => {
                let adapter = find(&owner(self, *session)?)?;
                let state = self.with_session(*session, |machine, s| {
                    let state = adapter.resume(machine, tick, s, *step)?;
                    s.done = state.stopped.is_exit();
                    Ok(state)
                })?;
                Ok(Reply::Stopped { state })
            }
            Request::Pause { session } => {
                let adapter = find(&owner(self, *session)?)?;
                let state = self.with_session(*session, |_, s| adapter.pause(s))?;
                Ok(Reply::Stopped { state })
            }
            Request::Variables { session, reference } => {
                let adapter = find(&owner(self, *session)?)?;
                let variables =
                    self.with_session(*session, |_, s| adapter.variables(s, *reference))?;
                Ok(Reply::Variables { variables })
            }
            Request::Evaluate {
                session,
                frame,
                expression,
                context,
            } => {
                let adapter = find(&owner(self, *session)?)?;
                let result = self.with_session(*session, |machine, s| {
                    adapter.evaluate(machine, s, *frame, expression, context)
                })?;
                Ok(Reply::Evaluated { result })
            }
            Request::Terminate { session } => {
                let kind = owner(self, *session)?;
                if let Ok(adapter) = find(&kind) {
                    self.with_session(*session, |machine, s| {
                        adapter.terminate(machine, s);
                        Ok(())
                    })?;
                }
                self.debug.sessions.retain(|s| s.id != *session);
                Ok(Reply::Terminated)
            }
        }
    }
}
