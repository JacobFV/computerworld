//! A deterministic JavaScript interpreter that behaves like Node.js v24 for
//! the simulated computers.
//!
//! Source is tokenized, parsed to an AST and compiled to bytecode run by a
//! stack VM with heap frames (JS-to-JS calls never recurse on the Rust
//! stack). Everything the program can observe of the outside world comes
//! from a [`ScriptHost`]: files from the machine's VFS, time from the world
//! clock, randomness from the world's seeded entropy. Output, error formats
//! (headers, stack traces, `util.inspect`) and exit codes follow Node.

pub mod ast;
pub mod bigint;
pub mod builtins;
pub mod bytecode;
pub mod call;
pub mod compiler;
pub mod conv;
pub mod fs;
pub mod inspect;
pub mod interp;
pub mod lexer;
pub mod node;
pub mod nodelib;
pub mod numconv;
pub mod parser;
pub mod promise;
pub mod props;
pub mod realm;
pub mod regexp;
pub mod value;
pub mod vm;

use cw_script_host::{Invocation, Outcome, ScriptHost};
use value::{Ctl, Value};
use vm::{Tail, Vm};

pub use vm::NODE_VERSION;

/// Exit status when the step budget runs out (like `timeout(1)`).
pub const TIMEOUT_EXIT: i32 = 124;

const USAGE: &str = "Usage: node [options] [ script.js ] [arguments]
       node inspect [options] [ script.js | host:port ] [arguments]

Options:
  -                           script read from stdin (default if no file name is provided, interactive mode if a tty)
  --                          indicate the end of node options
  -c, --check                 syntax check script without executing
  -e, --eval=...              evaluate script
  -h, --help                  print node command line options (currently set)
  -i, --interactive           always enter the REPL even if stdin does not appear to be a terminal
  -p, --print [...]           evaluate script and print result
  -r, --require=...           CommonJS module to preload (option can be repeated)
  --input-type=...            set module type for string input
  --stack-trace-limit=...     maximum number of stack frames shown
  -v, --version               print Node.js version

Documentation can be found at https://nodejs.org/
";

enum Target {
    File(String),
    Eval(String, bool),
    Stdin,
}

/// Options that take no argument and are accepted (and ignored).
fn ignorable(a: &str) -> bool {
    let base = a.split('=').next().unwrap_or(a);
    matches!(
        base,
        "--no-warnings"
            | "--trace-uncaught"
            | "--trace-warnings"
            | "--enable-source-maps"
            | "--no-deprecation"
            | "--throw-deprecation"
            | "--trace-deprecation"
            | "--max-old-space-size"
            | "--max-semi-space-size"
            | "--stack-size"
            | "--unhandled-rejections"
            | "--pending-deprecation"
            | "--preserve-symlinks"
            | "--no-experimental-fetch"
            | "--harmony"
            | "--use-strict"
            | "--abort-on-uncaught-exception"
            | "--expose-gc"
            | "--title"
            | "--disable-warning"
            | "--no-experimental-strip-types"
            | "--experimental-strip-types"
    ) || base.starts_with("--experimental-")
        || base.starts_with("--no-experimental-")
        || base.starts_with("--trace-")
        || base.starts_with("--max-")
}

/// Runs `node <args>` against the host and returns both streams and the status.
pub fn run(host: &mut dyn ScriptHost, invocation: &Invocation) -> Outcome {
    let args = &invocation.args;
    let mut i = 0;
    let mut target = None;
    let mut check = false;
    let mut preload: Vec<String> = vec![];
    let mut input_module = false;
    let mut stack_limit = None;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-v" | "--version" => {
                return Outcome {
                    stdout: format!("{NODE_VERSION}\n"),
                    stderr: String::new(),
                    exit_code: 0,
                }
            }
            "-h" | "--help" => {
                return Outcome {
                    stdout: USAGE.into(),
                    stderr: String::new(),
                    exit_code: 0,
                }
            }
            "-e" | "--eval" | "-p" | "--print" | "-pe" => {
                let print = a.contains('p');
                let Some(code) = args.get(i + 1) else {
                    return Outcome {
                        stdout: String::new(),
                        stderr: format!("node: {a} requires an argument\n"),
                        exit_code: 9,
                    };
                };
                target = Some((Target::Eval(code.clone(), print), i + 2));
                break;
            }
            "-c" | "--check" => {
                check = true;
                i += 1;
            }
            "-r" | "--require" | "--import" => {
                if let Some(m) = args.get(i + 1) {
                    preload.push(m.clone());
                }
                i += 2;
            }
            "-i" | "--interactive" => i += 1,
            "--" => {
                if let Some(f) = args.get(i + 1) {
                    target = Some((Target::File(f.clone()), i + 2));
                } else {
                    target = Some((Target::Stdin, i + 1));
                }
                break;
            }
            "-" => {
                target = Some((Target::Stdin, i + 1));
                break;
            }
            _ if a.starts_with("--eval=") || a.starts_with("--print=") => {
                let print = a.starts_with("--print");
                let code = a.split_once('=').unwrap().1.to_string();
                target = Some((Target::Eval(code, print), i + 1));
                break;
            }
            _ if a.starts_with("--require=") || a.starts_with("--import=") => {
                preload.push(a.split_once('=').unwrap().1.to_string());
                i += 1;
            }
            _ if a.starts_with("--input-type=") => {
                input_module = a.ends_with("=module");
                i += 1;
            }
            _ if a.starts_with("--stack-trace-limit=") => {
                stack_limit = a.split_once('=').unwrap().1.parse::<usize>().ok();
                i += 1;
            }
            _ if a.starts_with('-') && a.len() > 1 => {
                if ignorable(a) {
                    // Space-separated values for a few options.
                    if matches!(
                        a,
                        "--title" | "--disable-warning" | "--max-old-space-size" | "--stack-size"
                    ) && !a.contains('=')
                    {
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                return Outcome {
                    stdout: String::new(),
                    stderr: format!("/usr/bin/node: bad option: {a}\n"),
                    exit_code: 9,
                };
            }
            _ => {
                target = Some((Target::File(a.to_string()), i + 1));
                break;
            }
        }
    }
    let (target, rest) = target.unwrap_or((Target::Stdin, args.len()));
    let script_args: Vec<String> = args[rest.min(args.len())..].to_vec();
    let reads_program = matches!(target, Target::Stdin);
    let stdin = if reads_program {
        None
    } else {
        Some(invocation.stdin.clone())
    };
    let cwd = host.cwd();
    let explicit_dash = args
        .get(rest.wrapping_sub(1))
        .map(|a| a == "-")
        .unwrap_or(false);
    let (argv1, main_path) = match &target {
        Target::File(f) => {
            let abs = host.resolve(f);
            (Some(abs.clone()), Some(abs))
        }
        Target::Stdin if explicit_dash => (Some("-".to_string()), None),
        _ => (None, None),
    };
    let mut argv = vec!["/usr/bin/node".to_string()];
    if let Some(a) = &argv1 {
        argv.push(a.clone());
    }
    argv.extend(script_args);
    let mut vm = Vm::new(host, argv, invocation.env.clone(), stdin);
    if let Some(n) = stack_limit {
        vm.stack_limit = n;
    }
    let result = run_program(
        &mut vm,
        &target,
        main_path.as_deref(),
        &cwd,
        invocation,
        check,
        &preload,
        input_module,
    );
    let code = finish(&mut vm, result);
    Outcome {
        stdout: std::mem::take(&mut vm.stdout),
        stderr: std::mem::take(&mut vm.stderr),
        exit_code: code,
    }
}

#[allow(clippy::too_many_arguments)]
fn run_program(
    vm: &mut Vm,
    target: &Target,
    main: Option<&str>,
    cwd: &str,
    inv: &Invocation,
    check: bool,
    preload: &[String],
    input_module: bool,
) -> Result<(), Ctl> {
    for p in preload {
        vm.require(p, cwd, "")?;
    }
    match target {
        Target::File(f) => {
            let path = main.unwrap_or(f).to_string();
            let resolved = vm.resolve_main(&path);
            let Some(file) = resolved else {
                return Err(vm.main_not_found(&path));
            };
            vm.main_file = file.clone();
            if check {
                vm.tail = Tail::Check;
                let src = match vm.host.read_file(&file) {
                    Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                    Err(_) => return Err(vm.main_not_found(&path)),
                };
                vm.compile_source(
                    &src,
                    &file,
                    None,
                    &["exports", "require", "module", "__filename", "__dirname"],
                )?;
                return Ok(());
            }
            vm.tail = Tail::Main;
            let m = vm.load_file_module(&file);
            if vm.modules.iter().any(|(k, v)| {
                k == &file && matches!(v, Value::Obj(o) if o.own_value("%esm").is_some())
            }) {
                vm.is_esm_main = true;
            }
            m?;
            Ok(())
        }
        Target::Eval(code, print) => {
            vm.tail = Tail::Eval;
            let v = vm.run_eval_source(code, "[eval]", cwd, input_module)?;
            if *print {
                let s = match &v {
                    Value::Str(s) => s.to_string(),
                    other => vm.inspect_default(other)?,
                };
                vm.stdout.push_str(&s);
                vm.stdout.push('\n');
            }
            Ok(())
        }
        Target::Stdin => {
            vm.tail = Tail::Eval;
            let code = inv.stdin.clone();
            if code.trim().is_empty() {
                return Ok(());
            }
            vm.run_eval_source(&code, "[stdin]", cwd, input_module)?;
            Ok(())
        }
    }
}

/// Runs the event loop, reports uncaught errors, emits `exit`; returns the
/// exit status.
fn finish(vm: &mut Vm, result: Result<(), Ctl>) -> i32 {
    let r = result.and_then(|_| {
        vm.tail = Tail::Microtask(None, false);
        // Loading and running the main module takes about a millisecond in
        // Node; 1 ms timers set by it are due when the loop first turns.
        vm.elapsed_ms += 1.0;
        vm.event_loop()
    });
    let code = match r {
        Ok(()) => vm.exit_code,
        Err(Ctl::Exit(c)) => c,
        Err(Ctl::Throw(v)) => {
            // process.on('uncaughtException') handlers take over.
            match vm.emit_process_event(
                "uncaughtException",
                vec![v.clone(), Value::str("uncaughtException")],
            ) {
                Ok(true) => {
                    let r2 = vm.event_loop();
                    match r2 {
                        Ok(()) => vm.exit_code,
                        Err(Ctl::Exit(c)) => c,
                        Err(Ctl::Throw(v2)) => {
                            vm.report_uncaught(&v2);
                            7
                        }
                        Err(Ctl::Fatal(e)) => {
                            vm.report_uncaught(&e);
                            TIMEOUT_EXIT
                        }
                    }
                }
                _ => {
                    vm.report_uncaught(&v);
                    1
                }
            }
        }
        Err(Ctl::Fatal(e)) => {
            vm.report_uncaught(&e);
            return TIMEOUT_EXIT;
        }
    };
    vm.exit_code = code;
    // 'exit' listeners run with the final code (they may change it).
    vm.budget = vm.steps + 5_000_000;
    match vm.emit_process_event("exit", vec![Value::Num(code as f64)]) {
        Ok(_) => vm.exit_code,
        Err(Ctl::Exit(c)) => c,
        Err(Ctl::Throw(v)) => {
            vm.report_uncaught(&v);
            7
        }
        Err(Ctl::Fatal(_)) => TIMEOUT_EXIT,
    }
}

impl<'h> Vm<'h> {
    /// Resolution of the entry point (exact file, then extensions / index).
    pub fn resolve_main(&mut self, path: &str) -> Option<String> {
        if let Ok(st) = self.host.stat(path) {
            if !st.is_dir {
                return Some(path.to_string());
            }
        }
        self.resolve_module(path, "/")
    }

    pub fn main_not_found(&mut self, path: &str) -> Ctl {
        let e = self.make_error(vm::ErrKind::Error, &format!("Cannot find module '{path}'"));
        if let value::Kind::Error(ed) = &mut e.borrow_mut().kind {
            ed.frames = vec![
                "Module._resolveFilename (node:internal/modules/cjs/loader:1564:15)".into(),
                "wrapResolveFilename (node:internal/modules/cjs/loader:1118:27)".into(),
                "defaultResolveImplForCJSLoading (node:internal/modules/cjs/loader:1142:10)".into(),
                "resolveForCJSWithHooks (node:internal/modules/cjs/loader:1169:12)".into(),
                "Module._load (node:internal/modules/cjs/loader:1341:5)".into(),
                "wrapModuleLoad (node:internal/modules/cjs/loader:261:19)".into(),
                "Module.executeUserEntryPoint [as runMain] (node:internal/modules/run_main:154:5)"
                    .into(),
                "node:internal/main/run_main_module:33:47".into(),
            ];
            ed.arrow = Some("node:internal/modules/cjs/loader:1568\n  throw err;\n  ^\n".into());
        }
        e.set_prop("code", Value::str("MODULE_NOT_FOUND"), value::ALL);
        let rs = self.arr(vec![]);
        e.set_prop("requireStack", rs, value::ALL);
        Ctl::Throw(Value::Obj(e))
    }

    /// `node -e` / stdin programs: CommonJS-like wrapper in the cwd.
    pub fn run_eval_source(
        &mut self,
        code: &str,
        file: &str,
        cwd: &str,
        module: bool,
    ) -> Result<Value, Ctl> {
        let force = if module { Some(true) } else { None };
        let (c, is_module) = self.compile_source(
            code,
            file,
            force,
            &["exports", "require", "module", "__filename", "__dirname"],
        )?;
        let caps: std::rc::Rc<[value::CellRef]> = std::rc::Rc::from(Vec::new());
        let f = self.make_closure(c, caps);
        let fake = format!("{}/{file}", cwd.trim_end_matches('/'));
        if is_module {
            let ns = self.obj_with(None, value::Kind::Ordinary);
            let imp = self.native_fn_slots(
                "import",
                1,
                esm_import_eval,
                vec![Value::string(cwd.to_string())],
            );
            let meta = self.new_object();
            let p = self.call(
                &Value::Obj(f),
                Value::Undefined,
                vec![Value::Obj(ns), Value::Obj(imp), Value::Obj(meta)],
            )?;
            return self.settled_value(&p);
        }
        let m = self.new_object();
        let exports = self.new_object();
        m.set_prop("exports", Value::Obj(exports.clone()), value::ALL);
        m.set_prop("filename", Value::string(fake.clone()), value::ALL);
        let req = self.make_require(&fake);
        self.call(
            &Value::Obj(f),
            Value::Obj(exports.clone()),
            vec![
                Value::Obj(exports),
                Value::Obj(req),
                Value::Obj(m),
                Value::string(file.to_string()),
                Value::str("."),
            ],
        )
    }
}

fn esm_import_eval(vm: &mut Vm, a: &mut value::Args) -> value::JsResult<Value> {
    let s = promise::slots(a);
    let dir = match &s[0] {
        Value::Str(d) => d.to_string(),
        _ => "/".into(),
    };
    let spec = builtins::str_arg(vm, a, 0)?;
    vm.import_namespace(&spec, &dir, "[eval]")
}
