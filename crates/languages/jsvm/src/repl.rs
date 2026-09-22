//! The Node console: `node` with nothing to run and a terminal on standard input.
//!
//! The session is not kept alive between two lines (an interpreter heap cannot be
//! serialized); it is replayed. Every line typed so far is standard input, the run
//! stops where it asks for the next one, and the terminal shows only what the last
//! line produced. A binding a line makes is copied into the global object when the
//! line finishes, which is how the next line sees it.
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

pub const PS1: &str = "> ";
pub const PS2: &str = "| ";

const HELP: &str = ".break   Sometimes you get stuck, this gets you out\n\
.clear   Alias for .break\n\
.exit    Exit the REPL\n\
.help    Print this help message\n\
.load    Load JS from a file into the REPL session\n\
.save    Save all evaluated commands in this REPL session to a file\n\
\n\
Press Ctrl+C to abort current expression, Ctrl+D to exit the REPL\n";

/// The next line typed, or `None` at end-of-file.
fn next_line(vm: &mut Vm) -> Result<Option<String>, Ctl> {
    let stdin = vm.stdin.clone().unwrap_or_default();
    if vm.stdin_pos >= stdin.len() {
        if vm.stdin_eof {
            return Ok(None);
        }
        return Err(vm.need_input());
    }
    let rest = &stdin[vm.stdin_pos..];
    let end = rest.find('\n').map(|p| p + 1).unwrap_or(rest.len());
    let line = rest[..end].to_string();
    vm.stdin_pos += end;
    Ok(Some(line.strip_suffix('\n').unwrap_or(&line).to_string()))
}

/// Whether the parser stopped at the end of the input, so the statement may yet
/// be finished by another line.
fn recoverable(msg: &str) -> bool {
    msg.contains("Unexpected end of input")
        || msg.contains("Unterminated template")
        || msg.contains("Unterminated string")
        || msg.contains("Invalid or unexpected token") && msg.contains("end")
}

/// Runs the console until end-of-file, `.exit` or a line nobody has typed yet.
pub fn run(vm: &mut Vm) -> Result<(), Ctl> {
    let banner = format!(
        "Welcome to Node.js {}.\nType \".help\" for more information.\n",
        NODE_VERSION
    );
    vm.stdout.push_str(&banner);
    let mut buffer = String::new();
    loop {
        vm.stdout
            .push_str(if buffer.is_empty() { PS1 } else { PS2 });
        let line = match next_line(vm)? {
            Some(l) => l,
            None => return Ok(()),
        };
        if buffer.is_empty() {
            match line.trim() {
                ".exit" => return Ok(()),
                ".help" => {
                    vm.stdout.push_str(HELP);
                    continue;
                }
                ".break" | ".clear" => {
                    buffer.clear();
                    continue;
                }
                "" => continue,
                _ => {}
            }
        }
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(&line);
        let source = buffer.clone();
        match eval_line(vm, &source) {
            Ok(Some(value)) => {
                buffer.clear();
                let text = vm.inspect_default(&value)?;
                vm.stdout.push_str(&text);
                vm.stdout.push('\n');
            }
            // Not finished: the next line continues it.
            Ok(None) => continue,
            Err(Ctl::Exit(c)) => return Err(Ctl::Exit(c)),
            Err(Ctl::Fatal(v)) => return Err(Ctl::Fatal(v)),
            Err(Ctl::Throw(v)) => {
                buffer.clear();
                let text = uncaught(vm, &v)?;
                vm.stdout.push_str(&text);
            }
        }
        // While the console waits for the next line the loop keeps turning.
        if let Err(e) = vm.event_loop() {
            match e {
                Ctl::Throw(v) => {
                    let text = uncaught(vm, &v)?;
                    vm.stdout.push_str(&text);
                }
                other => return Err(other),
            }
        }
    }
}

/// How the console reports what a line threw.
fn uncaught(vm: &mut Vm, v: &Value) -> Result<String, Ctl> {
    if let Value::Obj(o) = v {
        if matches!(o.borrow().kind, Kind::Error(_)) {
            let name = vm.get_str(v, "name")?;
            let name = vm.to_str(&name)?;
            let msg = vm.get_str(v, "message")?;
            let msg = vm.to_str(&msg)?;
            let head = if msg.is_empty() {
                name.to_string()
            } else {
                format!("{name}: {msg}")
            };
            // Node prints the error's stack with the console's own frames
            // stripped; for an error raised inside a builtin, nothing is left
            // of it but this line.
            return Ok(format!("Uncaught {head}\n"));
        }
    }
    Ok(format!("Uncaught {}\n", vm.inspect_default(v)?))
}

/// Compiles and runs one console entry. `Ok(None)` means the statement is not
/// finished yet.
fn eval_line(vm: &mut Vm, src: &str) -> Result<Option<Value>, Ctl> {
    let compiled = vm.compile_source(src, "[eval]", Some(false), &[]);
    let code = match compiled {
        Ok((c, _)) => c,
        Err(Ctl::Throw(v)) => {
            let msg = vm.get_str(&v, "message").and_then(|m| vm.to_str(&m));
            if let Ok(m) = msg {
                if recoverable(&m) {
                    return Ok(None);
                }
            }
            return Err(Ctl::Throw(v));
        }
        Err(other) => return Err(other),
    };
    // What the line declares has to outlive it: the console keeps its bindings
    // in the global object, so the next line resolves them there.
    let declared: Vec<String> = code
        .local_names
        .iter()
        .map(|n| n.to_string())
        .filter(|n| {
            !n.is_empty()
                && !n.starts_with('%')
                && n.chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
                && n.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        })
        .collect();
    let code = if declared.is_empty() {
        code
    } else {
        // A `for` initialiser is not an expression statement, so copying the
        // bindings out does not become the entry's value.
        let copies = declared
            .iter()
            .map(|n| format!("globalThis.{n} = {n}"))
            .collect::<Vec<_>>()
            .join(", ");
        let with_epilogue = format!("{src}\n;for ({copies}; false; );");
        match vm.compile_source(&with_epilogue, "[eval]", Some(false), &[]) {
            Ok((c, _)) => c,
            // The epilogue did not parse (an odd declaration): run the line as
            // it stands and let its bindings end with it.
            Err(_) => code,
        }
    };
    let caps: Rc<[CellRef]> = Rc::from(Vec::new());
    let f = vm.make_closure(code, caps);
    let v = vm.call(&Value::Obj(f), Value::Undefined, vec![])?;
    Ok(Some(v))
}
