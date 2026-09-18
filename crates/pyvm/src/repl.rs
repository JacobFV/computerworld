//! The interactive interpreter: `python3` with nothing to run and a terminal on
//! standard input.
//!
//! The session is not kept alive between two lines (an interpreter heap cannot be
//! serialized); it is replayed. Every line typed so far is standard input, the run
//! stops where it asks for the next one, and the terminal shows only what the last
//! line produced. Prompts are written to standard output where a real interpreter
//! writes them, so whatever stands after the last newline is what the terminal puts
//! the next typed line behind.
use crate::value::*;
use crate::vm::*;
use crate::{compile_source, format_exception};

pub const PS1: &str = ">>> ";
pub const PS2: &str = "... ";

/// The next line the user typed, or `None` at end-of-file. Suspends the run when
/// nothing has been typed yet.
fn next_line(vm: &mut Vm) -> PyResult<Option<String>> {
    if vm.stdin_pos >= vm.stdin.len() {
        if vm.stdin_eof {
            return Ok(None);
        }
        return Err(crate::need_input(vm));
    }
    let rest = &vm.stdin[vm.stdin_pos..];
    let end = rest.find('\n').map(|p| p + 1).unwrap_or(rest.len());
    let line = rest[..end].to_string();
    vm.stdin_pos += end;
    Ok(Some(line.strip_suffix('\n').unwrap_or(&line).to_string()))
}

/// Whether the parser stopped because the statement is not finished, rather than
/// because it is wrong.
fn incomplete(msg: &str) -> bool {
    msg.starts_with("unexpected EOF")
        || msg.ends_with("was never closed")
        || msg.starts_with("expected an indented block")
        || msg.contains("incomplete input")
}

/// Whether more lines are expected after `buffer` compiled cleanly: a compound
/// statement ends at a blank line, as it does in CPython's console.
fn needs_blank_line(buffer: &str, last: &str) -> bool {
    if last.trim().is_empty() {
        return false;
    }
    let first = buffer.lines().next().unwrap_or("").trim_end();
    let header = first.ends_with(':')
        || first.ends_with('\\')
        || matches!(
            first.split_whitespace().next(),
            Some("if" | "for" | "while" | "def" | "class" | "with" | "try" | "match" | "async")
        );
    header
}

/// Runs the console until end-of-file, `exit()` or a line nobody has typed yet.
pub fn run(vm: &mut Vm, globals: Ref<Dict>) -> i32 {
    let banner = format!(
        "Python {} on linux\nType \"help\", \"copyright\", \"credits\" or \"license\" for more information.\n",
        crate::modules::sys::VERSION
    );
    vm.write_stderr(&banner);
    let mut buffer = String::new();
    loop {
        let prompt = if buffer.is_empty() { PS1 } else { PS2 };
        // The console flushes standard output before it writes a prompt, and the
        // prompt itself goes to standard error, as CPython's does.
        crate::io::flush_all(vm);
        vm.stdout_flushed = vm.stdout.len();
        vm.write_stderr(prompt);
        let line = match next_line(vm) {
            Ok(Some(l)) => l,
            Ok(None) => {
                // Ctrl-D at the prompt leaves the interpreter.
                vm.write_stderr("\n");
                return 0;
            }
            Err(e) => return finish(vm, e),
        };
        if buffer.is_empty() && line.trim().is_empty() {
            continue;
        }
        buffer.push_str(&line);
        buffer.push('\n');
        let code = match compile_source(vm, &buffer, "<stdin>", "single") {
            Ok(c) => {
                if needs_blank_line(&buffer, &line) {
                    continue;
                }
                c
            }
            Err(mut e) => {
                let exc = vm.materialize(&mut e);
                let msg = vm.str_of(&exc).unwrap_or_default();
                if incomplete(&msg) {
                    continue;
                }
                let text = format_exception(vm, &exc, 0);
                vm.write_stderr(&text);
                buffer.clear();
                continue;
            }
        };
        buffer.clear();
        let frame = vm.new_frame(code, globals.clone(), None);
        let r = vm.charge_depth().and_then(|()| vm.execute(frame, None));
        crate::io::flush_all(vm);
        if let Err(e) = r {
            let status = finish(vm, e);
            if status >= 0 {
                return status;
            }
        }
    }
}

/// Reports what ended a statement. `-1` means the console goes on.
fn finish(vm: &mut Vm, mut e: Box<PyErr>) -> i32 {
    if vm.awaiting_input {
        return 0;
    }
    let exc = vm.materialize(&mut e);
    if e.fatal {
        let text = format_exception(vm, &exc, 0);
        vm.write_stderr(&text);
        return crate::TIMEOUT_EXIT;
    }
    if vm.isinstance(&exc, &vm.t.exc("SystemExit")) {
        let code = crate::builtins::exc_args(vm, &exc);
        return match code.first() {
            None | Some(Value::None) => 0,
            Some(Value::Int(i)) => (*i as i32) & 0xff,
            Some(Value::Bool(b)) => *b as i32,
            Some(other) => {
                let s = vm.str_of(other).unwrap_or_default();
                vm.write_stderr(&format!("{s}\n"));
                1
            }
        };
    }
    let text = format_exception(vm, &exc, 0);
    vm.write_stderr(&text);
    -1
}
