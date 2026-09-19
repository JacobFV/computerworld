//! A whole debug session against a program on a machine: the DAP-shaped
//! session driving the interpreter, which is replayed between requests.
use cw_computer::debugger::MachineDebugger;
use cw_computer::{Computer, OfflineHost};
use cw_script_host::dap::{Body, DapSession, Event, Request, Response};
use cw_script_host::debug::SourceBreakpoint;

const PROGRAM: &str = r#"import json

items = []
for i in range(3):
    items.append({"n": i, "square": i * i})
print(json.dumps(items))
"#;

fn machine() -> Computer {
    let mut c = Computer::new("pc", "user", "linux", true);
    c.vfs
        .write("/home/user/main.py", PROGRAM.as_bytes(), "user", 0)
        .unwrap();
    c
}

fn body(r: Response) -> Body {
    match r {
        Response::Ok(b) => b,
        Response::Err(e) => panic!("request failed: {e}"),
    }
}

#[test]
fn a_session_walks_a_program_on_the_machine() {
    let mut c = machine();
    let mut h = OfflineHost;
    let mut session = DapSession::new();
    let command: Vec<String> = ["python3", "main.py"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut send = |session: &mut DapSession, c: &mut Computer, req: Request| {
        let mut runner = MachineDebugger::for_command(c, &mut h, 0, &command).expect("a runtime");
        session.handle(req, &mut runner)
    };

    let (r, events) = send(&mut session, &mut c, Request::Initialize);
    assert!(matches!(body(r), Body::Capabilities(_)));
    assert_eq!(events, vec![Event::Initialized]);

    let (r, _) = send(
        &mut session,
        &mut c,
        Request::SetBreakpoints {
            path: "/home/user/main.py".into(),
            breakpoints: vec![SourceBreakpoint {
                line: 5,
                ..SourceBreakpoint::default()
            }],
        },
    );
    match body(r) {
        Body::Breakpoints(b) => assert_eq!((b.len(), b[0].line, b[0].verified), (1, 5, true)),
        other => panic!("{other:?}"),
    }

    let (_, _) = send(
        &mut session,
        &mut c,
        Request::Launch {
            stop_on_entry: false,
        },
    );
    let (_, events) = send(&mut session, &mut c, Request::ConfigurationDone);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Stopped(s) if s.reason.as_str() == "breakpoint")),
        "{events:?}"
    );
    assert!(session.is_stopped());

    // The stopped program is inspected where it stands.
    let (r, _) = send(&mut session, &mut c, Request::Threads);
    match body(r) {
        Body::Threads(t) => assert_eq!(t.len(), 1),
        other => panic!("{other:?}"),
    }
    let frames = match body(send(&mut session, &mut c, Request::StackTrace { thread_id: 1 }).0) {
        Body::StackTrace(f) => f,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        (frames[0].path.as_str(), frames[0].line),
        ("/home/user/main.py", 5)
    );
    let scopes = match body(
        send(
            &mut session,
            &mut c,
            Request::Scopes {
                frame_id: frames[0].id,
            },
        )
        .0,
    ) {
        Body::Scopes(s) => s,
        other => panic!("{other:?}"),
    };
    let locals = scopes.iter().find(|s| s.name == "Locals").expect("locals");
    let vars = match body(
        send(
            &mut session,
            &mut c,
            Request::Variables {
                variables_reference: locals.variables_reference,
            },
        )
        .0,
    ) {
        Body::Variables(v) => v,
        other => panic!("{other:?}"),
    };
    let i = vars.iter().find(|v| v.name == "i").expect("i");
    assert_eq!((i.value.as_str(), i.type_name.as_str()), ("0", "int"));
    let items = vars.iter().find(|v| v.name == "items").expect("items");
    assert_eq!(items.value, "[]");

    // Expressions run in the frame.
    let (r, _) = send(
        &mut session,
        &mut c,
        Request::Evaluate {
            expression: "i * 100".into(),
            frame_id: Some(frames[0].id),
        },
    );
    match body(r) {
        Body::Evaluate(v) => assert_eq!(v.value, "0"),
        other => panic!("{other:?}"),
    }

    // Stepping and continuing walk the same program forward.
    let (_, events) = send(&mut session, &mut c, Request::Next);
    assert!(
        events.iter().any(|e| matches!(e, Event::Stopped(_))),
        "{events:?}"
    );
    let (_, events) = send(&mut session, &mut c, Request::Continue);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Stopped(s) if s.reason.as_str() == "breakpoint")),
        "{events:?}"
    );

    // Running to the end reports the program's output and status.
    let mut ended = None;
    for _ in 0..6 {
        let (_, events) = send(&mut session, &mut c, Request::Continue);
        if let Some(code) = events.iter().find_map(|e| match e {
            Event::Exited { exit_code } => Some(*exit_code),
            _ => None,
        }) {
            ended = Some((code, events));
            break;
        }
    }
    let (code, events) = ended.expect("the program ended");
    assert_eq!(code, 0);
    let out: String = events
        .iter()
        .filter_map(|e| match e {
            Event::Output { category, output } if category == "stdout" => Some(output.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        out,
        "[{\"n\": 0, \"square\": 0}, {\"n\": 1, \"square\": 1}, {\"n\": 2, \"square\": 4}]\n"
    );
    assert_eq!(session.exit_code(), Some(0));
}

#[test]
fn changing_a_variable_changes_what_the_program_prints() {
    let mut c = machine();
    let mut h = OfflineHost;
    let mut session = DapSession::new();
    let command: Vec<String> = ["python3", "main.py"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut send = |session: &mut DapSession, c: &mut Computer, req: Request| {
        let mut runner = MachineDebugger::for_command(c, &mut h, 0, &command).expect("a runtime");
        session.handle(req, &mut runner)
    };
    send(&mut session, &mut c, Request::Initialize);
    send(
        &mut session,
        &mut c,
        Request::SetBreakpoints {
            path: "/home/user/main.py".into(),
            breakpoints: vec![SourceBreakpoint {
                line: 5,
                condition: Some("i == 2".into()),
                ..SourceBreakpoint::default()
            }],
        },
    );
    send(
        &mut session,
        &mut c,
        Request::Launch {
            stop_on_entry: false,
        },
    );
    send(&mut session, &mut c, Request::ConfigurationDone);
    assert!(session.is_stopped());
    let frames = match body(send(&mut session, &mut c, Request::StackTrace { thread_id: 1 }).0) {
        Body::StackTrace(f) => f,
        other => panic!("{other:?}"),
    };
    let scopes = match body(
        send(
            &mut session,
            &mut c,
            Request::Scopes {
                frame_id: frames[0].id,
            },
        )
        .0,
    ) {
        Body::Scopes(s) => s,
        other => panic!("{other:?}"),
    };
    let locals = scopes.iter().find(|s| s.name == "Locals").expect("locals");
    let (r, _) = send(
        &mut session,
        &mut c,
        Request::SetVariable {
            variables_reference: locals.variables_reference,
            name: "i".into(),
            value: "9".into(),
        },
    );
    match body(r) {
        Body::Evaluate(v) => assert_eq!(v.value, "9"),
        other => panic!("{other:?}"),
    }
    let mut out = String::new();
    for _ in 0..8 {
        let (_, events) = send(&mut session, &mut c, Request::Continue);
        for e in &events {
            if let Event::Output { category, output } = e {
                if category == "stdout" {
                    out.push_str(output);
                }
            }
        }
        if session.exit_code().is_some() {
            break;
        }
    }
    assert_eq!(
        out,
        "[{\"n\": 0, \"square\": 0}, {\"n\": 1, \"square\": 1}, {\"n\": 9, \"square\": 81}]\n"
    );
}
