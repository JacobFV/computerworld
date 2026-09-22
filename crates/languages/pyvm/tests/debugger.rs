//! The DAP-shaped debugger in the simulated `python3`.
use cw_pyvm::run_debug;
use cw_script_host::debug::*;
use cw_script_host::{memory::MemoryHost, Invocation, Outcome, ScriptHost};
use std::collections::BTreeMap;

const PROGRAM: &str = r#"total = 0


def add(a, b):
    result = a + b
    return result


def run(n):
    global total
    for i in range(n):
        total = add(total, i)
    return total


print(run(4))
"#;

/// A front end that answers each stop from a script and records what it saw.
struct Recorder {
    config: DebugConfig,
    answers: Vec<Step>,
    seen: Vec<String>,
    logs: Vec<String>,
    /// Expressions to evaluate at every stop, and what came back.
    evaluate: Vec<String>,
    /// `(reference-name, value)` to set at the first stop.
    set: Option<(String, String)>,
}

impl Recorder {
    fn new(config: DebugConfig) -> Self {
        Self {
            config,
            answers: vec![],
            seen: vec![],
            logs: vec![],
            evaluate: vec![],
            set: None,
        }
    }
}

impl Debugger for Recorder {
    fn config(&self) -> &DebugConfig {
        &self.config
    }
    fn stopped(&mut self, event: &StopEvent, target: &mut dyn DebugTarget) -> Step {
        let frames = target.stack_trace(event.thread_id);
        let top = frames.first().expect("a frame");
        let mut line = format!(
            "{:?} {}:{} in {}",
            event.reason, top.path, top.line, top.name
        );
        if event.reason == StopReason::Exception {
            line.push_str(&format!(" [{}: {}]", event.description, event.text));
        }
        let scopes = target.scopes(top.id);
        let locals = scopes
            .iter()
            .find(|s| s.name == "Locals")
            .map(|s| target.variables(s.variables_reference))
            .unwrap_or_default();
        let shown: Vec<String> = locals
            .iter()
            .map(|v| format!("{}={}", v.name, v.value))
            .collect();
        line.push_str(&format!(" {{{}}}", shown.join(", ")));
        for expr in self.evaluate.clone() {
            let r = match target.evaluate(&expr, Some(top.id)) {
                Ok(v) => format!("{}={}", expr, v.value),
                Err(e) => format!("{expr}!{e}"),
            };
            line.push(' ');
            line.push_str(&r);
        }
        if let Some((name, value)) = self.set.take() {
            let locals_ref = scopes
                .iter()
                .find(|s| s.name == "Locals")
                .map(|s| s.variables_reference)
                .unwrap_or(0);
            match target.set_variable(locals_ref, &name, &value) {
                Ok(v) => line.push_str(&format!(" set {}={}", v.name, v.value)),
                Err(e) => line.push_str(&format!(" set!{e}")),
            }
        }
        self.seen.push(line);
        if self.answers.is_empty() {
            Step::Continue
        } else {
            self.answers.remove(0)
        }
    }
    fn log(&mut self, text: &str) {
        self.logs.push(text.to_string());
    }
}

/// `(line, condition, hit condition, log message)`.
type Bp<'a> = (u32, Option<&'a str>, Option<&'a str>, Option<&'a str>);

fn breakpoints(path: &str, lines: &[Bp]) -> DebugConfig {
    let mut map = BTreeMap::new();
    map.insert(
        path.to_string(),
        lines
            .iter()
            .map(|(line, cond, hit, log)| SourceBreakpoint {
                line: *line,
                condition: cond.map(|s| s.to_string()),
                hit_condition: hit.map(|s| s.to_string()),
                log_message: log.map(|s| s.to_string()),
            })
            .collect(),
    );
    DebugConfig {
        breakpoints: map,
        ..DebugConfig::default()
    }
}

fn debug(src: &str, rec: &mut Recorder) -> Outcome {
    let mut host = MemoryHost::default();
    host.write_file("/home/user/main.py", src.as_bytes(), false)
        .unwrap();
    let (out, _info) = run_debug(
        &mut host,
        &Invocation {
            args: vec!["main.py".into()],
            ..Default::default()
        },
        rec,
    );
    out
}

#[test]
fn a_breakpoint_stops_with_the_frame_and_its_locals() {
    let mut rec = Recorder::new(breakpoints("/home/user/main.py", &[(5, None, None, None)]));
    rec.answers = vec![Step::Continue];
    let out = debug(PROGRAM, &mut rec);
    assert_eq!(out.stdout, "6\n", "{}", out.stderr);
    assert_eq!(
        rec.seen,
        vec![
            "Breakpoint /home/user/main.py:5 in add {a=0, b=0}".to_string(),
            "Breakpoint /home/user/main.py:5 in add {a=0, b=1}".to_string(),
            "Breakpoint /home/user/main.py:5 in add {a=1, b=2}".to_string(),
            "Breakpoint /home/user/main.py:5 in add {a=3, b=3}".to_string(),
        ]
    );
}

#[test]
fn a_condition_and_a_hit_count_decide_when_to_stop() {
    let mut rec = Recorder::new(breakpoints(
        "/home/user/main.py",
        &[(5, Some("b >= 2"), None, None)],
    ));
    let out = debug(PROGRAM, &mut rec);
    assert_eq!(out.stdout, "6\n", "{}", out.stderr);
    assert_eq!(rec.seen.len(), 2, "{:?}", rec.seen);
    assert!(rec.seen[0].ends_with("{a=1, b=2}"), "{:?}", rec.seen);

    let mut rec = Recorder::new(breakpoints(
        "/home/user/main.py",
        &[(5, None, Some(">= 3"), None)],
    ));
    debug(PROGRAM, &mut rec);
    assert_eq!(rec.seen.len(), 2, "{:?}", rec.seen);
    assert!(rec.seen[0].ends_with("{a=1, b=2}"), "{:?}", rec.seen);
}

#[test]
fn a_logpoint_prints_instead_of_stopping() {
    let mut rec = Recorder::new(breakpoints(
        "/home/user/main.py",
        &[(5, None, None, Some("add {a} and {b}"))],
    ));
    let out = debug(PROGRAM, &mut rec);
    assert_eq!(out.stdout, "6\n");
    assert!(rec.seen.is_empty(), "{:?}", rec.seen);
    assert_eq!(
        rec.logs,
        vec!["add 0 and 0", "add 0 and 1", "add 1 and 2", "add 3 and 3"]
    );
}

#[test]
fn stepping_walks_lines_and_calls() {
    let mut rec = Recorder::new(breakpoints("/home/user/main.py", &[(11, None, None, None)]));
    rec.answers = vec![
        Step::Next,    // line 11 -> 12
        Step::StepIn,  // into add
        Step::Next,    // add's second line
        Step::StepOut, // back where add was called
        Step::Suspend, // and stop looking
    ];
    let out = debug(PROGRAM, &mut rec);
    assert_eq!(
        out.stdout, "",
        "suspended before it printed: {}",
        out.stderr
    );
    let lines: Vec<&str> = rec.seen.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        lines,
        vec![
            // `i` is not bound yet where the loop starts.
            "Breakpoint /home/user/main.py:11 in run {n=4}",
            "Step /home/user/main.py:12 in run {n=4, i=0}",
            "Step /home/user/main.py:5 in add {a=0, b=0}",
            "Step /home/user/main.py:6 in add {a=0, b=0, result=0}",
            // Stepping out lands back on the line that made the call.
            "Step /home/user/main.py:12 in run {n=4, i=0}",
        ]
    );
}

#[test]
fn the_front_end_can_evaluate_and_change_the_program() {
    let mut rec = Recorder::new(breakpoints("/home/user/main.py", &[(5, None, None, None)]));
    rec.evaluate = vec!["a * 10".into(), "total".into(), "nope".into()];
    rec.set = Some(("b".into(), "100".into()));
    rec.answers = vec![Step::Continue];
    let out = debug(PROGRAM, &mut rec);
    // b was changed at the first stop, so the sum is 100 rather than 0.
    assert_eq!(out.stdout, "106\n", "{}", out.stderr);
    let first = &rec.seen[0];
    assert!(first.contains("a * 10=0"), "{first}");
    assert!(first.contains("total=0"), "{first}");
    assert!(
        first.contains("nope!NameError: name 'nope' is not defined"),
        "{first}"
    );
    assert!(first.contains("set b=100"), "{first}");
}

#[test]
fn an_exception_stops_the_program_when_asked() {
    let src = "def boom():\n    raise ValueError('nope')\n\ntry:\n    boom()\nexcept ValueError:\n    print('caught')\nboom()\n";
    let mut rec = Recorder::new(DebugConfig {
        exceptions: ExceptionFilters {
            raised: true,
            uncaught: false,
        },
        ..DebugConfig::default()
    });
    let out = debug(src, &mut rec);
    assert_eq!(out.stdout, "caught\n");
    assert_eq!(out.exit_code, 1);
    assert_eq!(rec.seen.len(), 2, "{:?}", rec.seen);
    assert!(
        rec.seen[0].starts_with("Exception /home/user/main.py:2 in boom [ValueError: nope]"),
        "{:?}",
        rec.seen
    );

    let mut rec = Recorder::new(DebugConfig {
        exceptions: ExceptionFilters {
            raised: false,
            uncaught: true,
        },
        ..DebugConfig::default()
    });
    debug(src, &mut rec);
    assert_eq!(
        rec.seen.len(),
        1,
        "only the one nothing handled: {:?}",
        rec.seen
    );
}

#[test]
fn stop_on_entry_stops_before_the_first_line() {
    let mut rec = Recorder::new(DebugConfig {
        stop_on_entry: true,
        ..DebugConfig::default()
    });
    let out = debug(PROGRAM, &mut rec);
    assert_eq!(out.stdout, "6\n");
    assert_eq!(rec.seen.len(), 1);
    assert!(
        rec.seen[0].starts_with("Entry /home/user/main.py:1 in <module>"),
        "{:?}",
        rec.seen
    );
}

#[test]
fn suspending_ends_the_run_where_it_stands() {
    let mut rec = Recorder::new(breakpoints("/home/user/main.py", &[(5, None, None, None)]));
    rec.answers = vec![Step::Suspend];
    let mut host = MemoryHost::default();
    host.write_file("/home/user/main.py", PROGRAM.as_bytes(), false)
        .unwrap();
    let (out, info) = run_debug(
        &mut host,
        &Invocation {
            args: vec!["main.py".into()],
            ..Default::default()
        },
        &mut rec,
    );
    assert!(info.suspended && !info.terminated);
    assert_eq!(out.stdout, "", "the program never reached its print");
    assert_eq!(rec.seen.len(), 1);
}

#[test]
fn a_runaway_program_can_be_paused() {
    let mut rec = Recorder::new(DebugConfig {
        pause_after: Some(50),
        ..DebugConfig::default()
    });
    rec.answers = vec![Step::Terminate];
    let mut host = MemoryHost::default();
    host.write_file(
        "/home/user/main.py",
        b"i = 0\nwhile True:\n    i += 1\n",
        false,
    )
    .unwrap();
    let (_out, info) = run_debug(
        &mut host,
        &Invocation {
            args: vec!["main.py".into()],
            ..Default::default()
        },
        &mut rec,
    );
    assert!(info.terminated, "{:?}", rec.seen);
    assert_eq!(rec.seen.len(), 1);
    assert!(
        rec.seen[0].starts_with("Pause /home/user/main.py:2"),
        "{:?}",
        rec.seen
    );
}
