//! The debug seam: a machine keeps the sessions, an adapter moves the program. There
//! is no adapter for the interpreters yet, so this drives the seam with one of its own
//! to prove the machine's half — the half the Run and Debug view talks to.
use cw_computer::debug::{DebugAdapter, Session};
use cw_computer::Computer;
use cw_protocol::debug::{
    ExceptionFilters, Frame, Launch, Reply, Request, Scope, SourceBreakpoint, State, Step, Stopped,
    Variable,
};
use serde_json::json;

/// A stand-in for a real debugger: it walks the lines of the file on the machine and
/// keeps everything it knows in the session's own state, as an adapter must.
struct LineWalker;
impl LineWalker {
    fn lines(machine: &Computer, path: &str) -> Vec<String> {
        String::from_utf8_lossy(&machine.vfs.read(path).unwrap_or_default())
            .lines()
            .map(str::to_owned)
            .collect()
    }
    fn state(session: &Session, machine_lines: usize) -> State {
        let line = session.state["line"].as_u64().unwrap_or(1) as u32;
        if line as usize > machine_lines {
            return State {
                stopped: Stopped::Exited { code: 0 },
                frames: vec![],
                scopes: vec![],
                output: String::new(),
            };
        }
        State {
            stopped: match session.state["reason"].as_str() {
                Some("breakpoint") => Stopped::Breakpoint {
                    path: session.program.clone(),
                    line,
                },
                Some("step") => Stopped::Step,
                _ => Stopped::Entry,
            },
            frames: vec![Frame {
                id: 1,
                name: "<module>".into(),
                path: session.program.clone(),
                line,
            }],
            scopes: vec![Scope {
                name: "Locals".into(),
                reference: 10,
                expensive: false,
            }],
            output: session.state["output"].as_str().unwrap_or("").to_owned(),
        }
    }
}
impl DebugAdapter for LineWalker {
    fn kind(&self) -> &str {
        "python"
    }
    fn launch(
        &self,
        machine: &mut Computer,
        _tick: u64,
        launch: &Launch,
    ) -> Result<(Session, State), String> {
        let lines = Self::lines(machine, &launch.program);
        if lines.is_empty() {
            return Err(format!("{} has nothing to run", launch.program));
        }
        let mut session = Session {
            program: launch.program.clone(),
            state: json!({
                "line": 1,
                "reason": "entry",
                "breakpoints": launch.breakpoints.iter().map(|b| b.line).collect::<Vec<_>>(),
                "output": "",
            }),
            ..Session::default()
        };
        if !launch.stop_on_entry {
            let first = launch.breakpoints.first().map_or(1, |b| b.line);
            session.state["line"] = json!(first);
            session.state["reason"] = json!("breakpoint");
        }
        let state = Self::state(&session, lines.len());
        Ok((session, state))
    }
    fn breakpoints(
        &self,
        session: &mut Session,
        _path: &str,
        points: &[SourceBreakpoint],
    ) -> Vec<bool> {
        session.state["breakpoints"] = json!(points.iter().map(|b| b.line).collect::<Vec<_>>());
        points.iter().map(|b| b.line > 0).collect()
    }
    fn exceptions(&self, session: &mut Session, filters: ExceptionFilters) {
        session.state["uncaught"] = json!(filters.uncaught);
    }
    fn resume(
        &self,
        machine: &mut Computer,
        _tick: u64,
        session: &mut Session,
        step: Step,
    ) -> Result<State, String> {
        let lines = Self::lines(machine, &session.program);
        let line = session.state["line"].as_u64().unwrap_or(1) as u32;
        let next = match step {
            Step::Continue => session.state["breakpoints"]
                .as_array()
                .and_then(|b| {
                    b.iter()
                        .filter_map(serde_json::Value::as_u64)
                        .find(|b| *b as u32 > line)
                })
                .map_or(lines.len() as u32 + 1, |b| b as u32),
            _ => line + 1,
        };
        session.state["line"] = json!(next);
        session.state["reason"] = json!(if matches!(step, Step::Continue) {
            "breakpoint"
        } else {
            "step"
        });
        // Whatever the line prints is the program's output.
        if let Some(text) = lines.get(next as usize - 1).and_then(|l| {
            l.trim()
                .strip_prefix("print(\"")
                .and_then(|r| r.strip_suffix("\")"))
        }) {
            session.state["output"] = json!(format!("{text}\n"));
        }
        Ok(Self::state(session, lines.len()))
    }
    fn pause(&self, session: &mut Session) -> Result<State, String> {
        Ok(Self::state(session, usize::MAX))
    }
    fn variables(&self, session: &Session, reference: u64) -> Result<Vec<Variable>, String> {
        if reference != 10 {
            return Err(format!("no variables under {reference}"));
        }
        Ok(vec![Variable {
            name: "line".into(),
            value: session.state["line"].to_string(),
            kind: "int".into(),
            reference: 0,
        }])
    }
    fn evaluate(
        &self,
        _machine: &mut Computer,
        session: &mut Session,
        _frame: u64,
        expression: &str,
        _context: &str,
    ) -> Result<Variable, String> {
        if expression != "line" {
            return Err(format!("name '{expression}' is not defined"));
        }
        Ok(Variable {
            name: expression.into(),
            value: session.state["line"].to_string(),
            kind: "int".into(),
            reference: 0,
        })
    }
    fn terminate(&self, _machine: &mut Computer, session: &mut Session) {
        session.done = true;
    }
}

fn machine() -> Computer {
    let mut c = Computer::new("dev", "alice", "linux", true);
    c.vfs
        .write(
            "/home/alice/main.py",
            b"x = 1\nprint(\"one\")\nprint(\"two\")\n",
            "alice",
            0,
        )
        .unwrap();
    c
}

#[test]
fn a_machine_with_no_adapter_says_so_rather_than_pretending() {
    let mut c = machine();
    let err = c
        .debug(
            0,
            &Request::Launch(Launch {
                kind: "python".into(),
                program: "/home/alice/main.py".into(),
                cwd: "/home/alice".into(),
                stop_on_entry: true,
                ..Launch::default()
            }),
        )
        .unwrap_err();
    assert_eq!(
        err,
        "no debug adapter for Python is installed on this machine"
    );
    assert!(c.debug.sessions.is_empty());
    // And a request about a session nothing started is refused by handle.
    assert_eq!(
        c.debug(
            0,
            &Request::Resume {
                session: 1,
                step: Step::Over
            }
        )
        .unwrap_err(),
        "debug session 1 is not running"
    );
}

#[test]
fn the_machine_carries_a_session_from_launch_to_exit() {
    let adapter = LineWalker;
    let adapters: [&dyn DebugAdapter; 1] = [&adapter];
    let mut c = machine();
    let reply = c
        .debug_with(
            &adapters,
            5,
            &Request::Launch(Launch {
                kind: "python".into(),
                program: "/home/alice/main.py".into(),
                cwd: "/home/alice".into(),
                stop_on_entry: true,
                breakpoints: vec![SourceBreakpoint {
                    path: "/home/alice/main.py".into(),
                    line: 3,
                    enabled: true,
                    ..SourceBreakpoint::default()
                }],
                ..Launch::default()
            }),
        )
        .unwrap();
    let Reply::Launched { session, state } = reply else {
        panic!("a launch answers with the session it made");
    };
    assert_eq!(session, 1);
    assert_eq!(state.stopped, Stopped::Entry);
    assert_eq!(state.frames[0].line, 1);
    assert_eq!(c.debug.sessions.len(), 1);
    // A step moves the program and the machine keeps where it got to.
    let Reply::Stopped { state } = c
        .debug_with(
            &adapters,
            6,
            &Request::Resume {
                session,
                step: Step::Over,
            },
        )
        .unwrap()
    else {
        panic!("a step answers with where it stopped");
    };
    assert_eq!(state.stopped, Stopped::Step);
    assert_eq!(state.frames[0].line, 2);
    assert_eq!(state.output, "one\n");
    assert_eq!(c.debug.sessions[0].state["line"], 2);
    // The variables and an evaluation come from that same session.
    let Reply::Variables { variables } = c
        .debug_with(
            &adapters,
            7,
            &Request::Variables {
                session,
                reference: 10,
            },
        )
        .unwrap()
    else {
        panic!("variables");
    };
    assert_eq!(variables[0].value, "2");
    let Reply::Evaluated { result } = c
        .debug_with(
            &adapters,
            7,
            &Request::Evaluate {
                session,
                frame: 1,
                expression: "line".into(),
                context: "repl".into(),
            },
        )
        .unwrap()
    else {
        panic!("evaluate");
    };
    assert_eq!(result.value, "2");
    assert_eq!(
        c.debug_with(
            &adapters,
            7,
            &Request::Evaluate {
                session,
                frame: 1,
                expression: "nope".into(),
                context: "repl".into(),
            },
        )
        .unwrap_err(),
        "name 'nope' is not defined"
    );
    // Running on reaches the end, and the session is marked finished.
    let Reply::Stopped { state } = c
        .debug_with(
            &adapters,
            8,
            &Request::Resume {
                session,
                step: Step::Continue,
            },
        )
        .unwrap()
    else {
        panic!("continue");
    };
    assert_eq!(
        state.stopped,
        Stopped::Breakpoint {
            path: "/home/alice/main.py".into(),
            line: 3
        }
    );
    let Reply::Stopped { state } = c
        .debug_with(
            &adapters,
            9,
            &Request::Resume {
                session,
                step: Step::Continue,
            },
        )
        .unwrap()
    else {
        panic!("continue");
    };
    assert_eq!(state.stopped, Stopped::Exited { code: 0 });
    assert!(c.debug.sessions[0].done);
    // Terminating forgets it, and a snapshot of the machine keeps whatever it held.
    let round: Computer = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(round.debug, c.debug);
    assert_eq!(
        c.debug_with(&adapters, 9, &Request::Terminate { session })
            .unwrap(),
        Reply::Terminated
    );
    assert!(c.debug.sessions.is_empty());
}
