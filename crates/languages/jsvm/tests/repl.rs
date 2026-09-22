//! The Node console and programs that read from a terminal.
//!
//! The transcripts are what Node 24.21 shows on its screen (it writes the
//! console's prompts, values and errors to standard output).
use cw_script_host::{memory::MemoryHost, Invocation, Outcome, ScriptHost};

fn run_on(host: &mut MemoryHost, args: &[&str], stdin: &str, eof: bool) -> Outcome {
    cw_jsvm::run(
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

fn typed(lines: &[&str], eof: bool) -> Outcome {
    let mut stdin = String::new();
    for l in lines {
        stdin.push_str(l);
        stdin.push('\n');
    }
    run_on(&mut MemoryHost::default(), &[], &stdin, eof)
}

const BANNER: &str = "Welcome to Node.js v24.21.0.\nType \".help\" for more information.\n";

fn body(out: &Outcome) -> String {
    assert_eq!(out.stderr, "", "an interactive run writes one stream");
    out.stdout
        .strip_prefix(BANNER)
        .unwrap_or_else(|| panic!("banner missing:\n{}", out.stdout))
        .to_string()
}

#[test]
fn the_console_echoes_values_and_keeps_its_bindings() {
    let out = typed(
        &[
            "const x = 6 * 7",
            "x",
            "\"hi\".repeat(2)",
            "console.log(x + 1)",
            "undefined",
            "[1, 2, 3].map((n) => n * 2)",
        ],
        true,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(
        body(&out),
        "> undefined\n> 42\n> 'hihi'\n> 43\nundefined\n> undefined\n> [ 2, 4, 6 ]\n> "
    );
}

#[test]
fn functions_and_classes_outlive_the_line_that_made_them() {
    let out = typed(
        &[
            "function double(n) { return n * 2 }",
            "class Point { constructor(x) { this.x = x } }",
            "double(21)",
            "new Point(3).x",
            "let total = 0",
            "total += 5",
            "total",
        ],
        true,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(
        body(&out),
        "> undefined\n> undefined\n> 42\n> 3\n> undefined\n> 5\n> 5\n> "
    );
}

#[test]
fn an_unfinished_statement_continues_on_the_next_line() {
    let out = typed(&["function f(a) {", "  return a * 2", "}", "f(21)"], true);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(body(&out), "> | | undefined\n> 42\n> ");
}

#[test]
fn errors_are_reported_and_the_console_goes_on() {
    let out = typed(
        &[
            "null.x",
            "throw new SyntaxError(\"bad\")",
            "throw new Error(\"boom\")",
            "1 + 1",
        ],
        true,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(
        body(&out),
        "> Uncaught TypeError: Cannot read properties of null (reading 'x')\n\
         > Uncaught SyntaxError: bad\n\
         > Uncaught Error: boom\n> 2\n> "
    );
}

#[test]
fn timers_run_while_the_console_waits() {
    let out = typed(
        &["setTimeout(() => console.log('later'), 5) && 0", "'next'"],
        true,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(body(&out), "> 0\nlater\n> 'next'\n> ");
}

#[test]
fn dot_commands_work() {
    let out = typed(&[".help", "1", ".exit", "never"], true);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let text = body(&out);
    assert!(
        text.starts_with("> .break   Sometimes you get stuck"),
        "{text}"
    );
    assert!(text.ends_with("> 1\n> "), "{text}");
    assert!(!text.contains("never"), "{text}");
}

#[test]
fn the_console_stops_for_a_line_that_is_not_typed_yet() {
    let out = typed(&["const a = 2", "a * 3"], false);
    assert!(out.awaiting_input, "{}", out.stdout);
    assert_eq!(out.exit_code, 0);
    assert_eq!(body(&out), "> undefined\n> 6\n> ");
}

#[test]
fn reading_all_of_stdin_waits_for_end_of_file() {
    let mut host = MemoryHost::default();
    let src = "const data = require('fs').readFileSync(0, 'utf8');\nconsole.log(data.trim().split(/\\s+/).length);\n";
    host.write_file("/home/user/main.js", src.as_bytes(), false)
        .unwrap();
    let out = run_on(&mut host, &["main.js"], "one two\n", false);
    assert!(out.awaiting_input, "{}{}", out.stdout, out.stderr);
    let out = run_on(&mut host, &["main.js"], "one two\nthree\n", true);
    assert!(!out.awaiting_input, "{}", out.stderr);
    assert_eq!(out.stdout, "3\n", "{}", out.stderr);
}

#[test]
fn readline_question_waits_for_the_line() {
    let mut host = MemoryHost::default();
    let src = "const readline = require('readline');\n\
        const rl = readline.createInterface({ input: process.stdin, output: process.stdout });\n\
        rl.question('Name: ', (name) => {\n  console.log('hello ' + name);\n  rl.close();\n});\n";
    host.write_file("/home/user/main.js", src.as_bytes(), false)
        .unwrap();
    let out = run_on(&mut host, &["main.js"], "", false);
    assert!(out.awaiting_input, "{}{}", out.stdout, out.stderr);
    assert_eq!(out.stdout, "Name: ");
    let out = run_on(&mut host, &["main.js"], "Ada\n", false);
    assert!(!out.awaiting_input, "{}", out.stderr);
    assert_eq!(out.stdout, "Name: hello Ada\n", "{}", out.stderr);
}
