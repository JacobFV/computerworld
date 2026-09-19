//! Debugging a program on this machine: the runtime's DAP-shaped debugger
//! ([`cw_script_host::debug`]) driven by a session ([`cw_script_host::dap`]).
//!
//! A debug session outlives the actions of the world, so it cannot keep an
//! interpreter alive; it replays the program instead, answering the host calls
//! the earlier run made from a journal. [`MachineDebugger`] is what the session
//! reruns the program with.
use crate::debug::{DebugAdapter, Session};
use crate::runtimes::{MachineHost, Runtime};
use crate::{Computer, OfflineHost, ShellHost};
use cw_protocol::debug as proto;
use cw_protocol::debug::{ExceptionFilters, Launch};
use cw_script_host::dap::{self, DapSession, DebugRunner};
use cw_script_host::debug::{
    DebugRunInfo, Debugger, SourceBreakpoint, Step as HostStep, StopReason,
};
use cw_script_host::journal::{Journal, JournalHost};
use cw_script_host::{Invocation, Outcome};
use serde::{Deserialize, Serialize};

/// Runs one program on a machine as often as a debug session needs it.
pub struct MachineDebugger<'a> {
    pub computer: &'a mut Computer,
    pub shell: &'a mut dyn ShellHost,
    pub tick: u64,
    pub runtime: Runtime,
    /// The interpreter's arguments, `["main.py"]` and the program's own.
    pub args: Vec<String>,
    pub stdin: String,
    /// Where the program runs; the machine's own working directory when unset.
    pub cwd: Option<String>,
}

impl<'a> MachineDebugger<'a> {
    /// A debugger for `command` (`python3 main.py`, `node app.js …`), or `None`
    /// when the command does not name a runtime this machine debugs.
    pub fn for_command(
        computer: &'a mut Computer,
        shell: &'a mut dyn ShellHost,
        tick: u64,
        command: &[String],
    ) -> Option<Self> {
        let runtime = crate::runtimes::runtime_for(command.first()?)?;
        Some(Self {
            computer,
            shell,
            tick,
            runtime,
            args: command[1..].to_vec(),
            stdin: String::new(),
            cwd: None,
        })
    }
}

impl DebugRunner for MachineDebugger<'_> {
    fn run(
        &mut self,
        debugger: &mut dyn Debugger,
        journal: Journal,
    ) -> (Outcome, DebugRunInfo, Journal) {
        let env: Vec<(String, String)> = self
            .computer
            .env
            .iter()
            .filter(|(k, _)| {
                k.chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let invocation = Invocation {
            args: self.args.clone(),
            env,
            stdin: self.stdin.clone(),
            ..Invocation::default()
        };
        let mut machine = MachineHost::new(self.computer, self.shell, self.tick);
        if let Some(cwd) = &self.cwd {
            machine.cwd.clone_from(cwd);
        }
        let mut host = JournalHost::new(&mut machine, journal);
        let (out, info) = match self.runtime {
            Runtime::Python => cw_pyvm::run_debug(&mut host, &invocation, debugger),
            Runtime::Node => cw_jsvm::run_debug(&mut host, &invocation, debugger),
        };
        let journal = host.into_journal();
        self.computer.runtime_elapsed_micros = self
            .computer
            .runtime_elapsed_micros
            .saturating_add(out.elapsed_micros);
        (out, info, journal)
    }
}

// ------------------------------------------------------------ Run and Debug

/// `python3` under the debugger.
pub struct PythonAdapter;
/// `node` under the debugger.
pub struct NodeAdapter;

/// What it takes to be at this stop again: the command, every request that
/// moved the program, and the host calls the runs already made.
///
/// A session cannot hold a live interpreter across two actions of the world, so
/// this is what the machine stores instead. Serving a request rebuilds the DAP
/// session from it — replaying the program, with the journal answering the host
/// calls the earlier runs made so that nothing is done twice — and then serves
/// the new request against the program stopped where it was.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Replay {
    /// `python` or `node`.
    kind: String,
    /// The interpreter's arguments: the program, and the program's own.
    args: Vec<String>,
    cwd: String,
    /// The requests that made the program what it is, in order.
    acts: Vec<Act>,
    /// The recorded host calls, hex (`Journal::to_hex`).
    journal: String,
    /// The launched program's lines, to say whether a breakpoint can be set
    /// without having to run anything.
    source: Vec<String>,
    /// Where the program stands: what the last move answered.
    state: Option<proto::State>,
    /// The variables captured at that stop: reference `n` is `vars[n - 1]`.
    vars: Vec<Vec<proto::Variable>>,
}

/// One request that changed the program's course.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "act", rename_all = "snake_case")]
enum Act {
    Breakpoints {
        path: String,
        points: Vec<(u32, Option<String>)>,
    },
    Exceptions {
        raised: bool,
        uncaught: bool,
    },
    Launch {
        stop_on_entry: bool,
    },
    Resume {
        step: u8,
    },
    /// A Debug Console line: it may change the program, so a replay runs it too.
    Console {
        frame: u64,
        expression: String,
    },
    /// DAP's `pause`: runs here are synchronous, so it arms the next resume.
    Pause,
}

impl Act {
    /// The DAP requests this act is, in order.
    fn requests(&self) -> Vec<dap::Request> {
        match self {
            Act::Breakpoints { path, points } => vec![dap::Request::SetBreakpoints {
                path: path.clone(),
                breakpoints: points
                    .iter()
                    .map(|(line, condition)| SourceBreakpoint {
                        line: *line,
                        condition: condition.clone(),
                        ..SourceBreakpoint::default()
                    })
                    .collect(),
            }],
            Act::Exceptions { raised, uncaught } => {
                let mut filters = vec![];
                if *raised {
                    filters.push("raised".to_string());
                }
                if *uncaught {
                    filters.push("uncaught".to_string());
                }
                vec![dap::Request::SetExceptionBreakpoints { filters }]
            }
            Act::Launch { stop_on_entry } => vec![
                dap::Request::Launch {
                    stop_on_entry: *stop_on_entry,
                },
                dap::Request::ConfigurationDone,
            ],
            Act::Resume { step } => vec![match HostStep::from_tag(*step) {
                HostStep::Next => dap::Request::Next,
                HostStep::StepIn => dap::Request::StepIn,
                HostStep::StepOut => dap::Request::StepOut,
                _ => dap::Request::Continue,
            }],
            Act::Console { frame, expression } => vec![dap::Request::Evaluate {
                expression: expression.clone(),
                frame_id: Some(*frame),
            }],
            Act::Pause => vec![dap::Request::Pause],
        }
    }
}

/// How deep and how wide a stop's variables are captured. The view asks a
/// machine that holds data, not a running interpreter, so what is captured at
/// the stop is what it can be answered with.
const VAR_DEPTH: usize = 4;
const VAR_BREADTH: usize = 200;
const VAR_NODES: usize = 2000;

/// A DAP session on a machine, rebuilt from a [`Replay`].
struct Live<'a> {
    session: DapSession,
    runner: MachineDebugger<'a>,
}

fn runtime_of(kind: &str) -> Result<Runtime, String> {
    match kind {
        "python" => Ok(Runtime::Python),
        "node" => Ok(Runtime::Node),
        other => Err(crate::debug::unavailable(other)),
    }
}

impl<'a> Live<'a> {
    /// Replays everything the session has done, ending where it left off.
    fn rebuild(
        computer: &'a mut Computer,
        shell: &'a mut dyn ShellHost,
        tick: u64,
        replay: &Replay,
    ) -> Result<Live<'a>, String> {
        let runtime = runtime_of(&replay.kind)?;
        let mut runner = MachineDebugger {
            computer,
            shell,
            tick,
            runtime,
            args: replay.args.clone(),
            stdin: String::new(),
            cwd: Some(replay.cwd.clone()),
        };
        let journal = Journal::from_hex(&replay.journal).unwrap_or_default();
        let mut session = DapSession::with_journal(journal);
        session.handle(dap::Request::Initialize, &mut runner);
        for act in &replay.acts {
            for request in act.requests() {
                session.handle(request, &mut runner);
            }
        }
        Ok(Live { session, runner })
    }

    /// Serves one act and says where the program stands afterwards.
    fn advance(&mut self, act: &Act) -> Result<(proto::State, Vec<Vec<proto::Variable>>), String> {
        let mut events = vec![];
        for request in act.requests() {
            let (response, more) = self.session.handle(request, &mut self.runner);
            if let dap::Response::Err(message) = response {
                return Err(message);
            }
            events.extend(more);
        }
        Ok(self.capture(&events))
    }

    /// What a reply carries: why the program stopped, the stack, the innermost
    /// frame's scopes, what it wrote, and the variables to answer with.
    fn capture(&mut self, events: &[dap::Event]) -> (proto::State, Vec<Vec<proto::Variable>>) {
        let mut output = String::new();
        let mut stopped = None;
        let mut exited = None;
        for event in events {
            match event {
                dap::Event::Output { output: text, .. } => output.push_str(text),
                dap::Event::Stopped(e) => stopped = Some(e.clone()),
                dap::Event::Exited { exit_code } => exited = Some(*exit_code),
                _ => {}
            }
        }
        let mut vars: Vec<Vec<proto::Variable>> = vec![];
        let Some(event) = stopped else {
            let code = exited.or_else(|| self.session.exit_code()).unwrap_or(0);
            return (
                proto::State {
                    stopped: proto::Stopped::Exited { code },
                    frames: vec![],
                    scopes: vec![],
                    output,
                },
                vars,
            );
        };
        let frames = self.frames(event.thread_id);
        let reason = match event.reason {
            StopReason::Entry => proto::Stopped::Entry,
            StopReason::Step => proto::Stopped::Step,
            StopReason::Pause => proto::Stopped::Pause,
            StopReason::Breakpoint => proto::Stopped::Breakpoint {
                path: frames.first().map(|f| f.path.clone()).unwrap_or_default(),
                line: frames.first().map(|f| f.line).unwrap_or_default(),
            },
            StopReason::Exception => proto::Stopped::Exception {
                text: match (event.description.as_str(), event.text.as_str()) {
                    ("", text) => text.to_string(),
                    (description, "") => description.to_string(),
                    (description, text) => format!("{description}: {text}"),
                },
            },
        };
        let scopes = match frames.first() {
            Some(frame) => self.scopes(frame.id, &mut vars),
            None => vec![],
        };
        (
            proto::State {
                stopped: reason,
                frames,
                scopes,
                output,
            },
            vars,
        )
    }

    fn frames(&mut self, thread: u64) -> Vec<proto::Frame> {
        let (response, _) = self.session.handle(
            dap::Request::StackTrace { thread_id: thread },
            &mut self.runner,
        );
        let dap::Response::Ok(dap::Body::StackTrace(frames)) = response else {
            return vec![];
        };
        frames
            .into_iter()
            .map(|f| proto::Frame {
                id: f.id,
                name: f.name,
                path: f.path,
                line: f.line,
            })
            .collect()
    }

    /// The scopes of one frame, with their variables captured into `vars`.
    fn scopes(&mut self, frame: u64, vars: &mut Vec<Vec<proto::Variable>>) -> Vec<proto::Scope> {
        let (response, _) = self
            .session
            .handle(dap::Request::Scopes { frame_id: frame }, &mut self.runner);
        let dap::Response::Ok(dap::Body::Scopes(scopes)) = response else {
            return vec![];
        };
        let mut out = vec![];
        for scope in scopes {
            let reference = self.capture_children(scope.variables_reference, 0, vars);
            out.push(proto::Scope {
                name: scope.name,
                reference,
                expensive: scope.expensive,
            });
        }
        out
    }

    /// Reads a reference and everything under it, and gives back the reference
    /// the view will ask for: an index into `vars`, counting from 1.
    fn capture_children(
        &mut self,
        reference: u64,
        depth: usize,
        vars: &mut Vec<Vec<proto::Variable>>,
    ) -> u64 {
        if reference == 0 || depth >= VAR_DEPTH || vars.len() >= VAR_NODES {
            return 0;
        }
        let (response, _) = self.session.handle(
            dap::Request::Variables {
                variables_reference: reference,
            },
            &mut self.runner,
        );
        let dap::Response::Ok(dap::Body::Variables(children)) = response else {
            return 0;
        };
        vars.push(vec![]);
        let slot = vars.len();
        let mut captured = vec![];
        for child in children.into_iter().take(VAR_BREADTH) {
            let inner = self.capture_children(child.variables_reference, depth + 1, vars);
            captured.push(proto::Variable {
                name: child.name,
                value: child.value,
                kind: child.type_name,
                reference: inner,
            });
        }
        vars[slot - 1] = captured;
        slot as u64
    }
}

/// One request of a session: rebuild the program's course, add this to it, and
/// keep what it takes to do that again.
fn serve(
    kind: &str,
    machine: &mut Computer,
    tick: u64,
    session: &mut Session,
    act: Act,
) -> Result<proto::State, String> {
    let mut replay: Replay = read_state(session)?;
    if replay.kind != kind {
        return Err(format!(
            "debug session {} is not a {kind} program",
            session.id
        ));
    }
    let mut shell = OfflineHost;
    let (state, vars, journal) = {
        let mut live = Live::rebuild(machine, &mut shell, tick, &replay)?;
        let (state, vars) = live.advance(&act)?;
        (state, vars, live.session.journal().to_hex())
    };
    replay.acts.push(act);
    replay.journal = journal;
    replay.state = Some(state.clone());
    replay.vars = vars;
    write_state(session, &replay)?;
    Ok(state)
}

fn read_state(session: &Session) -> Result<Replay, String> {
    serde_json::from_value(session.state.clone())
        .map_err(|e| format!("debug session {}: {e}", session.id))
}

fn write_state(session: &mut Session, replay: &Replay) -> Result<(), String> {
    session.state =
        serde_json::to_value(replay).map_err(|e| format!("debug session {}: {e}", session.id))?;
    Ok(())
}

/// Whether a line is one a program can stop on: inside the file, not blank, and
/// not a comment on its own.
fn stoppable(source: &[String], line: u32, kind: &str) -> bool {
    let Some(text) = source.get(line.saturating_sub(1) as usize) else {
        return false;
    };
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    match kind {
        "python" => !text.starts_with('#'),
        _ => !(text.starts_with("//") || text.starts_with("/*") || text.starts_with('*')),
    }
}

/// Starts a program under either runtime.
fn launch_program(
    kind: &str,
    machine: &mut Computer,
    tick: u64,
    launch: &Launch,
) -> Result<(Session, proto::State), String> {
    runtime_of(kind)?;
    let mut args = vec![launch.program.clone()];
    args.extend(launch.args.iter().cloned());
    let cwd = if launch.cwd.is_empty() {
        machine.cwd.clone()
    } else {
        launch.cwd.clone()
    };
    let source = machine
        .vfs
        .read(&launch.program)
        .map_err(|e| format!("{}: {e}", launch.program))
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        })?;
    let mut replay = Replay {
        kind: kind.to_string(),
        args,
        cwd,
        source,
        ..Replay::default()
    };
    // The configuration first, as DAP orders it, then the program itself.
    let mut by_file: std::collections::BTreeMap<String, Vec<(u32, Option<String>)>> =
        Default::default();
    for point in launch.breakpoints.iter().filter(|b| b.enabled) {
        by_file
            .entry(point.path.clone())
            .or_default()
            .push((point.line, point.condition.clone()));
    }
    for (path, points) in by_file {
        replay.acts.push(Act::Breakpoints { path, points });
    }
    replay.acts.push(Act::Exceptions {
        raised: launch.exceptions.raised,
        uncaught: launch.exceptions.uncaught,
    });
    let mut session = Session {
        kind: kind.to_string(),
        program: launch.program.clone(),
        ..Session::default()
    };
    write_state(&mut session, &replay)?;
    let state = serve(
        kind,
        machine,
        tick,
        &mut session,
        Act::Launch {
            stop_on_entry: launch.stop_on_entry,
        },
    )?;
    Ok((session, state))
}

fn breakpoints_for(
    kind: &str,
    session: &mut Session,
    path: &str,
    points: &[proto::SourceBreakpoint],
) -> Vec<bool> {
    let Ok(mut replay) = read_state(session) else {
        return points.iter().map(|_| false).collect();
    };
    let enabled: Vec<(u32, Option<String>)> = points
        .iter()
        .filter(|p| p.enabled)
        .map(|p| (p.line, p.condition.clone()))
        .collect();
    replay.acts.push(Act::Breakpoints {
        path: path.to_string(),
        points: enabled,
    });
    // Only the launched program's own source is here to check against; a
    // breakpoint in another file is taken on trust until the program reaches it.
    let program = session.program.clone();
    let verified = points
        .iter()
        .map(|p| !p.enabled || path != program || stoppable(&replay.source, p.line, kind))
        .collect();
    let _ = write_state(session, &replay);
    verified
}

fn exceptions_for(session: &mut Session, filters: ExceptionFilters) {
    let Ok(mut replay) = read_state(session) else {
        return;
    };
    replay.acts.push(Act::Exceptions {
        raised: filters.raised,
        uncaught: filters.uncaught,
    });
    let _ = write_state(session, &replay);
}

fn variables_for(session: &Session, reference: u64) -> Result<Vec<proto::Variable>, String> {
    let replay = read_state(session)?;
    let index = reference.wrapping_sub(1) as usize;
    replay
        .vars
        .get(index)
        .cloned()
        .ok_or_else(|| format!("variable reference {reference} is not one of this stop's"))
}

fn evaluate_for(
    kind: &str,
    machine: &mut Computer,
    tick: u64,
    session: &mut Session,
    frame: u64,
    expression: &str,
    context: &str,
) -> Result<proto::Variable, String> {
    let replay = read_state(session)?;
    if replay.kind != kind {
        return Err(format!(
            "debug session {} is not a {kind} program",
            session.id
        ));
    }
    let mut shell = OfflineHost;
    let (result, journal) = {
        let mut live = Live::rebuild(machine, &mut shell, tick, &replay)?;
        let (response, _) = live.session.handle(
            dap::Request::Evaluate {
                expression: expression.to_string(),
                frame_id: Some(frame),
            },
            &mut live.runner,
        );
        (response, live.session.journal().to_hex())
    };
    let value = match result {
        dap::Response::Ok(dap::Body::Evaluate(v)) => v,
        dap::Response::Ok(_) => return Err("the runtime answered nothing".into()),
        dap::Response::Err(message) => return Err(message),
    };
    // A Debug Console line may change the program, so it becomes part of what a
    // replay does; a watch or a hover is read-only and does not.
    if context == "repl" {
        let mut replay = replay;
        replay.acts.push(Act::Console {
            frame,
            expression: expression.to_string(),
        });
        replay.journal = journal;
        write_state(session, &replay)?;
    }
    Ok(proto::Variable {
        name: expression.to_string(),
        value: value.value,
        kind: value.type_name,
        reference: 0,
    })
}

macro_rules! adapter {
    ($name:ident, $kind:literal) => {
        impl DebugAdapter for $name {
            fn kind(&self) -> &str {
                $kind
            }
            fn launch(
                &self,
                machine: &mut Computer,
                tick: u64,
                launch: &Launch,
            ) -> Result<(Session, proto::State), String> {
                launch_program($kind, machine, tick, launch)
            }
            fn breakpoints(
                &self,
                session: &mut Session,
                path: &str,
                points: &[proto::SourceBreakpoint],
            ) -> Vec<bool> {
                breakpoints_for($kind, session, path, points)
            }
            fn exceptions(&self, session: &mut Session, filters: ExceptionFilters) {
                exceptions_for(session, filters)
            }
            fn resume(
                &self,
                machine: &mut Computer,
                tick: u64,
                session: &mut Session,
                step: proto::Step,
            ) -> Result<proto::State, String> {
                let step = match step {
                    proto::Step::Continue => HostStep::Continue,
                    proto::Step::Over => HostStep::Next,
                    proto::Step::Into => HostStep::StepIn,
                    proto::Step::Out => HostStep::StepOut,
                };
                serve(
                    $kind,
                    machine,
                    tick,
                    session,
                    Act::Resume { step: step.tag() },
                )
            }
            fn pause(&self, session: &mut Session) -> Result<proto::State, String> {
                // A run is synchronous here: between two requests the program is
                // already stopped, so a pause arms the next resume and says where
                // the program stands now.
                let mut replay = read_state(session)?;
                replay.acts.push(Act::Pause);
                let state = replay
                    .state
                    .clone()
                    .ok_or_else(|| "the program has not started".to_string())?;
                write_state(session, &replay)?;
                Ok(state)
            }
            fn variables(
                &self,
                session: &Session,
                reference: u64,
            ) -> Result<Vec<proto::Variable>, String> {
                variables_for(session, reference)
            }
            fn evaluate(
                &self,
                machine: &mut Computer,
                session: &mut Session,
                frame: u64,
                expression: &str,
                context: &str,
            ) -> Result<proto::Variable, String> {
                evaluate_for($kind, machine, 0, session, frame, expression, context)
            }
            fn terminate(&self, _machine: &mut Computer, session: &mut Session) {
                // Nothing is running between two requests: ending a session is
                // forgetting how to reach its stop again.
                session.done = true;
                session.state = serde_json::Value::Null;
            }
        }
    };
}

adapter!(PythonAdapter, "python");
adapter!(NodeAdapter, "node");
