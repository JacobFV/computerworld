//! The DAP-shaped debugger in the simulated `node`.
use cw_jsvm::run_debug;
use cw_script_host::debug::*;
use cw_script_host::{memory::MemoryHost, Invocation, Outcome, ScriptHost};
use std::collections::BTreeMap;

const PROGRAM: &str = r#"let total = 0;

function add(a, b) {
  const result = a + b;
  return result;
}

function run(n) {
  for (let i = 0; i < n; i++) {
    total = add(total, i);
  }
  return total;
}

console.log(run(4));
"#;

struct Recorder {
    config: DebugConfig,
    answers: Vec<Step>,
    seen: Vec<String>,
    logs: Vec<String>,
    evaluate: Vec<String>,
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
        let locals_ref = scopes
            .iter()
            .find(|s| s.name == "Locals")
            .map(|s| s.variables_reference)
            .unwrap_or(0);
        let shown: Vec<String> = target
            .variables(locals_ref)
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

fn debug(src: &str, rec: &mut Recorder) -> (Outcome, DebugRunInfo) {
    let mut host = MemoryHost::default();
    host.write_file("/home/user/main.js", src.as_bytes(), false)
        .unwrap();
    run_debug(
        &mut host,
        &Invocation {
            args: vec!["main.js".into()],
            ..Default::default()
        },
        rec,
    )
}

#[test]
fn a_breakpoint_stops_with_the_frame_and_its_locals() {
    let mut rec = Recorder::new(breakpoints("/home/user/main.js", &[(4, None, None, None)]));
    let (out, _) = debug(PROGRAM, &mut rec);
    assert_eq!(out.stdout, "6\n", "{}", out.stderr);
    assert_eq!(
        rec.seen,
        vec![
            "Breakpoint /home/user/main.js:4 in add {a=0, b=0}".to_string(),
            "Breakpoint /home/user/main.js:4 in add {a=0, b=1}".to_string(),
            "Breakpoint /home/user/main.js:4 in add {a=1, b=2}".to_string(),
            "Breakpoint /home/user/main.js:4 in add {a=3, b=3}".to_string(),
        ]
    );
}

#[test]
fn a_condition_and_a_hit_count_decide_when_to_stop() {
    let mut rec = Recorder::new(breakpoints(
        "/home/user/main.js",
        &[(4, Some("b >= 2"), None, None)],
    ));
    let (out, _) = debug(PROGRAM, &mut rec);
    assert_eq!(out.stdout, "6\n", "{}", out.stderr);
    assert_eq!(rec.seen.len(), 2, "{:?}", rec.seen);
    assert!(rec.seen[0].ends_with("{a=1, b=2}"), "{:?}", rec.seen);

    let mut rec = Recorder::new(breakpoints(
        "/home/user/main.js",
        &[(4, None, Some("% 2"), None)],
    ));
    debug(PROGRAM, &mut rec);
    assert_eq!(rec.seen.len(), 2, "{:?}", rec.seen);
    assert!(rec.seen[0].ends_with("{a=0, b=1}"), "{:?}", rec.seen);
}

#[test]
fn a_logpoint_prints_instead_of_stopping() {
    let mut rec = Recorder::new(breakpoints(
        "/home/user/main.js",
        &[(4, None, None, Some("add {a} and {b}"))],
    ));
    let (out, _) = debug(PROGRAM, &mut rec);
    assert_eq!(out.stdout, "6\n", "{}", out.stderr);
    assert!(rec.seen.is_empty(), "{:?}", rec.seen);
    assert_eq!(
        rec.logs,
        vec!["add 0 and 0", "add 0 and 1", "add 1 and 2", "add 3 and 3"]
    );
}

#[test]
fn stepping_walks_lines_and_calls() {
    let mut rec = Recorder::new(breakpoints("/home/user/main.js", &[(10, None, None, None)]));
    rec.answers = vec![Step::StepIn, Step::Next, Step::StepOut, Step::Suspend];
    let (out, info) = debug(PROGRAM, &mut rec);
    assert!(info.suspended, "{:?}", rec.seen);
    assert_eq!(out.stdout, "");
    let lines: Vec<&str> = rec.seen.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        lines,
        vec![
            "Breakpoint /home/user/main.js:10 in run {n=4, i=0}",
            "Step /home/user/main.js:4 in add {a=0, b=0}",
            "Step /home/user/main.js:5 in add {a=0, b=0, result=0}",
            // Stepping out lands on the line that made the call, where the
            // breakpoint is: a stop that is both is reported as a breakpoint.
            "Breakpoint /home/user/main.js:10 in run {n=4, i=0}",
        ]
    );
}

#[test]
fn the_front_end_can_evaluate_and_change_the_program() {
    let mut rec = Recorder::new(breakpoints("/home/user/main.js", &[(4, None, None, None)]));
    rec.evaluate = vec!["a * 10".into(), "total".into(), "nope".into()];
    rec.set = Some(("b".into(), "100".into()));
    let (out, _) = debug(PROGRAM, &mut rec);
    assert_eq!(out.stdout, "106\n", "{}", out.stderr);
    let first = &rec.seen[0];
    assert!(first.contains("a * 10=0"), "{first}");
    assert!(first.contains("total=0"), "{first}");
    assert!(
        first.contains("nope!ReferenceError: nope is not defined"),
        "{first}"
    );
    assert!(first.contains("set b=100"), "{first}");
}

#[test]
fn an_exception_stops_the_program_when_asked() {
    let src = "function boom() {\n  throw new TypeError('nope');\n}\ntry {\n  boom();\n} catch (e) {\n  console.log('caught');\n}\nboom();\n";
    let mut rec = Recorder::new(DebugConfig {
        exceptions: ExceptionFilters {
            raised: true,
            uncaught: false,
        },
        ..DebugConfig::default()
    });
    let (out, _) = debug(src, &mut rec);
    assert_eq!(out.stdout, "caught\n");
    assert_eq!(out.exit_code, 1);
    assert_eq!(rec.seen.len(), 1, "{:?}", rec.seen);
    assert!(rec.seen[0].contains("[TypeError: nope]"), "{:?}", rec.seen);

    let mut rec = Recorder::new(DebugConfig {
        exceptions: ExceptionFilters {
            raised: false,
            uncaught: true,
        },
        ..DebugConfig::default()
    });
    debug(src, &mut rec);
    assert_eq!(rec.seen.len(), 1, "{:?}", rec.seen);
}

#[test]
fn stop_on_entry_stops_before_the_first_line() {
    let mut rec = Recorder::new(DebugConfig {
        stop_on_entry: true,
        ..DebugConfig::default()
    });
    let (out, _) = debug(PROGRAM, &mut rec);
    assert_eq!(out.stdout, "6\n", "{}", out.stderr);
    assert_eq!(rec.seen.len(), 1);
    assert!(
        rec.seen[0].starts_with("Entry /home/user/main.js:1 in"),
        "{:?}",
        rec.seen
    );
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
        "/home/user/main.js",
        b"let i = 0;\nwhile (true) {\n  i += 1;\n}\n",
        false,
    )
    .unwrap();
    let (_out, info) = run_debug(
        &mut host,
        &Invocation {
            args: vec!["main.js".into()],
            ..Default::default()
        },
        &mut rec,
    );
    assert!(info.terminated, "{:?}", rec.seen);
    assert_eq!(rec.seen.len(), 1);
    assert!(
        rec.seen[0].starts_with("Pause /home/user/main.js:"),
        "{:?}",
        rec.seen
    );
}
