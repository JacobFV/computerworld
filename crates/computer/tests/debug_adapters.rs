//! The machine's debug adapters: `python3` and `node` stopped, stepped, read and
//! evaluated through `cw_protocol::debug`, which is what the Run and Debug view
//! speaks.
use cw_computer::Computer;
use cw_protocol::debug::{
    ExceptionFilters, Launch, Reply, Request, SourceBreakpoint, State, Step, Stopped,
};

const PY: &str = r#"import json

total = 0
for i in range(3):
    total += i * i
items = {"total": total, "parts": [1, 2, 3]}
print(json.dumps(items))
"#;

const JS: &str = r#"const parts = [1, 2, 3];
let total = 0;
for (const p of parts) {
  total += p * p;
}
const items = { total, parts };
console.log(JSON.stringify(items));
"#;

fn machine() -> Computer {
    let mut c = Computer::new("pc", "user", "linux", true);
    c.vfs
        .write("/home/user/main.py", PY.as_bytes(), "user", 0)
        .unwrap();
    c.vfs
        .write("/home/user/main.js", JS.as_bytes(), "user", 0)
        .unwrap();
    c
}

fn launch(kind: &str, program: &str, lines: &[u32]) -> Request {
    Request::Launch(Launch {
        kind: kind.into(),
        program: program.into(),
        cwd: "/home/user".into(),
        args: vec![],
        stop_on_entry: false,
        breakpoints: lines
            .iter()
            .map(|line| SourceBreakpoint {
                path: program.into(),
                line: *line,
                condition: None,
                enabled: true,
            })
            .collect(),
        exceptions: ExceptionFilters::default(),
    })
}

fn launched(reply: Reply) -> (u64, State) {
    match reply {
        Reply::Launched { session, state } => (session, state),
        other => panic!("expected a launch, got {other:?}"),
    }
}

fn stopped(reply: Reply) -> State {
    match reply {
        Reply::Stopped { state } => state,
        other => panic!("expected a stop, got {other:?}"),
    }
}

#[test]
fn python_stops_at_a_breakpoint_and_steps() {
    let mut c = machine();
    let (id, state) = launched(
        c.debug(0, &launch("python", "/home/user/main.py", &[5]))
            .expect("a Python adapter"),
    );
    assert_eq!(
        state.stopped,
        Stopped::Breakpoint {
            path: "/home/user/main.py".into(),
            line: 5,
        }
    );
    let frame = state.frames.first().expect("a frame");
    assert_eq!((frame.name.as_str(), frame.line), ("<module>", 5));
    let locals = state
        .scopes
        .iter()
        .find(|s| s.name == "Locals")
        .expect("a locals scope");

    // The variables of the stop are answered from what the machine holds.
    let Reply::Variables { variables } = c
        .debug(
            0,
            &Request::Variables {
                session: id,
                reference: locals.reference,
            },
        )
        .expect("variables")
    else {
        panic!("expected variables")
    };
    let i = variables.iter().find(|v| v.name == "i").expect("i");
    assert_eq!((i.value.as_str(), i.kind.as_str()), ("0", "int"));

    // Stepping over the line runs it: the same breakpoint is hit again next time
    // round the loop, so stepping twice lands on the next iteration.
    let state = stopped(
        c.debug(
            0,
            &Request::Resume {
                session: id,
                step: Step::Over,
            },
        )
        .unwrap(),
    );
    assert_eq!(state.stopped, Stopped::Step);
    assert_eq!(state.frames.first().map(|f| f.line), Some(4));

    // A watch expression reads the program where it stands.
    let Reply::Evaluated { result } = c
        .debug(
            0,
            &Request::Evaluate {
                session: id,
                frame: state.frames[0].id,
                expression: "total".into(),
                context: "watch".into(),
            },
        )
        .unwrap()
    else {
        panic!("expected a value")
    };
    assert_eq!(result.value, "0");

    // Carrying on to the end gives the program's output and its status.
    let mut state = state;
    for _ in 0..10 {
        state = stopped(
            c.debug(
                0,
                &Request::Resume {
                    session: id,
                    step: Step::Continue,
                },
            )
            .unwrap(),
        );
        if state.stopped.is_exit() {
            break;
        }
    }
    assert_eq!(state.stopped, Stopped::Exited { code: 0 });
    assert_eq!(
        state.output, "{\"total\": 5, \"parts\": [1, 2, 3]}\n",
        "the program's own output comes back with the last reply"
    );
    assert!(matches!(
        c.debug(0, &Request::Terminate { session: id }),
        Ok(Reply::Terminated)
    ));
    assert!(c.debug.sessions.is_empty());
}

#[test]
fn node_stops_at_a_breakpoint_and_reads_its_variables() {
    let mut c = machine();
    let (id, state) = launched(
        c.debug(0, &launch("node", "/home/user/main.js", &[4]))
            .expect("a Node adapter"),
    );
    assert_eq!(
        state.stopped,
        Stopped::Breakpoint {
            path: "/home/user/main.js".into(),
            line: 4,
        }
    );
    let locals = state
        .scopes
        .iter()
        .find(|s| s.name == "Locals")
        .expect("a locals scope");
    let Reply::Variables { variables } = c
        .debug(
            0,
            &Request::Variables {
                session: id,
                reference: locals.reference,
            },
        )
        .unwrap()
    else {
        panic!("expected variables")
    };
    let p = variables.iter().find(|v| v.name == "p").expect("p");
    assert_eq!(p.value, "1");

    // A structure is expandable: its children were captured with it.
    let parts = variables
        .iter()
        .find(|v| v.name == "parts")
        .expect("parts")
        .clone();
    assert!(parts.reference > 0, "an array can be opened");
    let Reply::Variables { variables } = c
        .debug(
            0,
            &Request::Variables {
                session: id,
                reference: parts.reference,
            },
        )
        .unwrap()
    else {
        panic!("expected variables")
    };
    assert_eq!(
        variables
            .iter()
            .map(|v| format!("{}={}", v.name, v.value))
            .collect::<Vec<_>>()
            .join(","),
        "0=1,1=2,2=3"
    );
}

#[test]
fn a_debug_console_line_changes_the_program() {
    let mut c = machine();
    let (id, state) = launched(
        c.debug(0, &launch("python", "/home/user/main.py", &[6]))
            .expect("a Python adapter"),
    );
    let frame = state.frames[0].id;
    let Reply::Evaluated { result } = c
        .debug(
            0,
            &Request::Evaluate {
                session: id,
                frame,
                expression: "total = 100".into(),
                context: "repl".into(),
            },
        )
        .unwrap()
    else {
        panic!("expected a value")
    };
    assert_eq!(result.value, "None");
    let state = stopped(
        c.debug(
            0,
            &Request::Resume {
                session: id,
                step: Step::Continue,
            },
        )
        .unwrap(),
    );
    assert_eq!(state.stopped, Stopped::Exited { code: 0 });
    assert_eq!(state.output, "{\"total\": 100, \"parts\": [1, 2, 3]}\n");
}

#[test]
fn an_exception_stops_the_program_where_it_is_raised() {
    let mut c = machine();
    c.vfs
        .write(
            "/home/user/bad.py",
            b"def half(x):\n    return 10 / x\n\nprint(half(0))\n",
            "user",
            0,
        )
        .unwrap();
    let mut request = launch("python", "/home/user/bad.py", &[]);
    if let Request::Launch(l) = &mut request {
        l.exceptions = ExceptionFilters {
            uncaught: true,
            raised: true,
        };
    }
    let (id, state) = launched(c.debug(0, &request).unwrap());
    match &state.stopped {
        Stopped::Exception { text } => assert_eq!(text, "ZeroDivisionError: division by zero"),
        other => panic!("expected an exception, got {other:?}"),
    }
    // `raised` stops where it happened, with the frames that raised it.
    assert_eq!(state.frames.first().map(|f| f.line), Some(2));
    assert_eq!(
        state
            .frames
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        vec!["half", "<module>"]
    );
    // `uncaught` stops again once nothing has handled it, where the call was.
    let state = stopped(
        c.debug(
            0,
            &Request::Resume {
                session: id,
                step: Step::Continue,
            },
        )
        .unwrap(),
    );
    assert!(matches!(state.stopped, Stopped::Exception { .. }));
    assert_eq!(state.frames.first().map(|f| f.line), Some(4));
    let state = stopped(
        c.debug(
            0,
            &Request::Resume {
                session: id,
                step: Step::Continue,
            },
        )
        .unwrap(),
    );
    assert_eq!(state.stopped, Stopped::Exited { code: 1 });
    assert!(state.output.contains("ZeroDivisionError"));
}

#[test]
fn breakpoints_are_verified_against_the_source() {
    let mut c = machine();
    let (id, _) = launched(
        c.debug(0, &launch("python", "/home/user/main.py", &[5]))
            .unwrap(),
    );
    let Reply::Breakpoints { verified } = c
        .debug(
            0,
            &Request::Breakpoints {
                session: id,
                path: "/home/user/main.py".into(),
                points: [3, 2, 1, 99]
                    .iter()
                    .map(|line| SourceBreakpoint {
                        path: "/home/user/main.py".into(),
                        line: *line,
                        condition: None,
                        enabled: true,
                    })
                    .collect(),
            },
        )
        .unwrap()
    else {
        panic!("expected breakpoints")
    };
    // Line 3 is code, line 2 is blank, line 1 is code, line 99 is past the end.
    assert_eq!(verified, vec![true, false, true, false]);
}

#[test]
fn a_stopped_program_survives_a_snapshot() {
    let mut c = machine();
    let (id, _) = launched(
        c.debug(0, &launch("python", "/home/user/main.py", &[5]))
            .unwrap(),
    );
    // The whole machine goes to JSON and comes back, paused program and all.
    let text = serde_json::to_string(&c).unwrap();
    let mut restored: Computer = serde_json::from_str(&text).unwrap();
    let state = stopped(
        restored
            .debug(
                0,
                &Request::Resume {
                    session: id,
                    step: Step::Over,
                },
            )
            .unwrap(),
    );
    assert_eq!(state.stopped, Stopped::Step);
    assert_eq!(state.frames.first().map(|f| f.line), Some(4));
}

#[test]
fn a_runtime_without_an_adapter_says_so() {
    let mut c = machine();
    let e = c
        .debug(0, &launch("ruby", "/home/user/main.rb", &[]))
        .expect_err("no adapter");
    assert_eq!(e, "no debug adapter for ruby is installed on this machine");
}
