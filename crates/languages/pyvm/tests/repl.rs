//! The interactive interpreter and programs that read from a terminal.
//!
//! At a terminal both streams are one screen, so an interactive run returns one
//! stream: the transcripts here are what CPython 3.12 shows, with its prompts
//! (standard error) and echoed values (standard output) interleaved.
use cw_script_host::{memory::MemoryHost, Invocation, Outcome, ScriptHost};

fn typed(lines: &[&str], eof: bool) -> Outcome {
    let mut stdin = String::new();
    for l in lines {
        stdin.push_str(l);
        stdin.push('\n');
    }
    let mut host = MemoryHost::default();
    run_on(&mut host, &[], &stdin, eof)
}

fn run_on(host: &mut MemoryHost, args: &[&str], stdin: &str, eof: bool) -> Outcome {
    cw_pyvm::run(
        host,
        &Invocation {
            args: args.iter().map(|s| s.to_string()).collect(),
            env: vec![],
            stdin: stdin.to_string(),
            interactive: true,
            eof,
        },
    )
}

const BANNER: &str = "Python 3.12.3 (main, Mar 23 2026, 19:04:32) [GCC 13.3.0] on linux\nType \"help\", \"copyright\", \"credits\" or \"license\" for more information.\n";

/// What is on the screen after the banner.
fn body(out: &Outcome) -> String {
    assert_eq!(out.stderr, "", "an interactive run writes one stream");
    out.stdout
        .strip_prefix(BANNER)
        .unwrap_or_else(|| panic!("banner missing:\n{}", out.stdout))
        .to_string()
}

#[test]
fn the_console_echoes_values_and_keeps_its_namespace() {
    let out = typed(
        &["x = 6 * 7", "x", "'hi' * 2", "print(x + 1)", "None"],
        true,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    // CPython: stdout is "42\n'hihi'\n43\n", stderr ">>> >>> >>> >>> >>> >>> \n".
    assert_eq!(body(&out), ">>> >>> 42\n>>> 'hihi'\n>>> 43\n>>> >>> \n");
}

#[test]
fn a_block_ends_at_a_blank_line() {
    let out = typed(
        &[
            "total = 0",
            "for i in range(4):",
            "    total += i",
            "",
            "total",
        ],
        true,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    // CPython: stdout "6\n", stderr ">>> >>> ... ... >>> >>> \n".
    assert_eq!(body(&out), ">>> >>> ... ... >>> 6\n>>> \n");
}

#[test]
fn errors_are_reported_and_the_console_goes_on() {
    let out = typed(&["1/0", "undefined_name", "2 + 2"], true);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(
        body(&out),
        ">>> Traceback (most recent call last):\n  File \"<stdin>\", line 1, in <module>\nZeroDivisionError: division by zero\n\
         >>> Traceback (most recent call last):\n  File \"<stdin>\", line 1, in <module>\nNameError: name 'undefined_name' is not defined\n\
         >>> 4\n>>> \n"
    );
}

#[test]
fn a_syntax_error_is_reported_without_ending_the_console() {
    let out = typed(&["x = 1 +", "'after'"], true);
    assert_eq!(out.exit_code, 0);
    // CPython shows the offending line and a caret, with no traceback header.
    assert_eq!(
        body(&out),
        ">>>   File \"<stdin>\", line 1\n    x = 1 +\n           ^\nSyntaxError: invalid syntax\n>>> 'after'\n>>> \n"
    );
}

#[test]
fn an_open_bracket_continues_the_statement() {
    let out = typed(&["x = (", "  1 + 2", ")", "x"], true);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    // CPython: stdout "3\n", stderr ">>> ... ... >>> >>> \n".
    assert_eq!(body(&out), ">>> ... ... >>> 3\n>>> \n");
}

#[test]
fn the_console_stops_for_a_line_that_is_not_typed_yet() {
    let out = typed(&["x = 1", "x + 1"], false);
    assert!(out.awaiting_input, "{}", out.stdout);
    assert_eq!(out.exit_code, 0);
    // The screen ends at the prompt the next line will be typed behind.
    assert_eq!(body(&out), ">>> >>> 2\n>>> ");
}

#[test]
fn exit_leaves_the_console_with_its_status() {
    let out = typed(&["print('bye')", "exit(3)", "never"], false);
    assert_eq!(out.exit_code, 3, "{}", out.stdout);
    assert!(!out.awaiting_input);
    assert_eq!(body(&out), ">>> bye\n>>> ");
}

#[test]
fn input_suspends_a_program_until_the_line_is_typed() {
    let mut host = MemoryHost::default();
    let src = "name = input('Name: ')\nage = int(input('Age: '))\nprint(f'{name} is {age}')\n";
    host.write_file("/home/user/main.py", src.as_bytes(), false)
        .unwrap();
    // Nothing typed yet: the run stops at the first prompt.
    let out = run_on(&mut host, &["main.py"], "", false);
    assert!(out.awaiting_input);
    assert_eq!(out.stdout, "Name: ", "{}", out.stderr);
    // One line: it gets past the first prompt and stops at the second.
    let out = run_on(&mut host, &["main.py"], "Ada\n", false);
    assert!(out.awaiting_input);
    assert_eq!(out.stdout, "Name: Age: ");
    // Both lines: it runs to the end.
    let out = run_on(&mut host, &["main.py"], "Ada\n36\n", false);
    assert!(!out.awaiting_input);
    assert_eq!(out.stdout, "Name: Age: Ada is 36\n");
    assert_eq!(out.exit_code, 0);
}

#[test]
fn end_of_file_is_an_error_for_input_as_it_is_in_cpython() {
    let mut host = MemoryHost::default();
    let src = "print(input('? '))\n";
    host.write_file("/home/user/main.py", src.as_bytes(), false)
        .unwrap();
    let out = run_on(&mut host, &["main.py"], "", true);
    assert!(!out.awaiting_input);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.stdout.contains("EOFError: EOF when reading a line"),
        "{}",
        out.stdout
    );
}

#[test]
fn reading_all_of_stdin_waits_for_end_of_file() {
    let mut host = MemoryHost::default();
    let src = "import sys\ndata = sys.stdin.read()\nprint(len(data.split()))\n";
    host.write_file("/home/user/main.py", src.as_bytes(), false)
        .unwrap();
    let out = run_on(&mut host, &["main.py"], "one two\n", false);
    assert!(out.awaiting_input, "{}{}", out.stdout, out.stderr);
    let out = run_on(&mut host, &["main.py"], "one two\nthree\n", true);
    assert!(!out.awaiting_input);
    assert_eq!(out.stdout, "3\n", "{}", out.stderr);
}
