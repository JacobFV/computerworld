//! Interactive runtime sessions: a console, or a program that reads a line,
//! keeps running across terminal commands by being replayed.
use cw_computer::{Computer, OfflineHost};

fn machine() -> Computer {
    let mut c = Computer::new("pc", "user", "linux", true);
    // The terminal is the command's standard input.
    c.tty = true;
    c
}

#[test]
fn the_python_console_keeps_its_namespace_between_lines() {
    let mut c = machine();
    let mut h = OfflineHost;
    let start = c.execute("python3", 0, &mut h);
    assert!(
        start.stdout.starts_with("Python 3.12.3 (main,"),
        "{}",
        start.stdout
    );
    assert_eq!(c.session_prompt(), Some(">>> "));
    assert_eq!(start.exit_code, 0);

    let r = c.execute("x = 6 * 7", 1, &mut h);
    assert_eq!((r.stdout.as_str(), c.session_prompt()), ("", Some(">>> ")));
    let r = c.execute("x + 1", 2, &mut h);
    assert_eq!(r.stdout, "43\n");
    let r = c.execute("for i in range(3):", 3, &mut h);
    assert_eq!((r.stdout.as_str(), c.session_prompt()), ("", Some("... ")));
    let r = c.execute("    print(i, x)", 4, &mut h);
    assert_eq!(c.session_prompt(), Some("... "), "{}", r.stdout);
    let r = c.execute("", 5, &mut h);
    assert_eq!(r.stdout, "0 42\n1 42\n2 42\n");
    assert_eq!(c.session_prompt(), Some(">>> "));

    let r = c.execute("exit()", 6, &mut h);
    assert_eq!(r.exit_code, 0, "{}", r.stdout);
    assert!(c.session.is_none(), "the console is gone");
    assert_eq!(c.session_prompt(), None);
    // The shell has the line again.
    let r = c.execute("echo back", 7, &mut h);
    assert_eq!(r.stdout, "back\n");
}

#[test]
fn a_console_error_does_not_end_the_session() {
    let mut c = machine();
    let mut h = OfflineHost;
    c.execute("python3", 0, &mut h);
    let r = c.execute("1/0", 1, &mut h);
    assert!(
        r.stdout.ends_with("ZeroDivisionError: division by zero\n"),
        "{}",
        r.stdout
    );
    assert_eq!(c.session_prompt(), Some(">>> "));
    let r = c.execute("2 + 2", 2, &mut h);
    assert_eq!(r.stdout, "4\n");
}

#[test]
fn side_effects_of_earlier_lines_are_not_repeated() {
    let mut c = machine();
    let mut h = OfflineHost;
    c.execute("python3", 0, &mut h);
    c.execute("f = open('/home/user/log.txt', 'a')", 1, &mut h);
    c.execute("f.write('one\\n'); f.flush()", 2, &mut h);
    c.execute("f.write('two\\n'); f.flush()", 3, &mut h);
    c.execute("exit()", 4, &mut h);
    let r = c.execute("cat /home/user/log.txt", 5, &mut h);
    assert_eq!(r.stdout, "one\ntwo\n", "each line ran once");
}

#[test]
fn a_program_that_reads_a_line_waits_for_it() {
    let mut c = machine();
    let mut h = OfflineHost;
    c.vfs
        .write(
            "/home/user/ask.py",
            b"name = input('Name: ')\nprint('hello', name)\n",
            "user",
            0,
        )
        .unwrap();
    let r = c.execute("python3 ask.py", 0, &mut h);
    assert_eq!(r.stdout, "");
    assert_eq!(c.session_prompt(), Some("Name: "));
    let r = c.execute("Ada", 1, &mut h);
    assert_eq!(r.stdout, "hello Ada\n");
    assert!(c.session.is_none());
}

#[test]
fn the_node_console_evaluates_and_keeps_bindings() {
    let mut c = machine();
    let mut h = OfflineHost;
    let start = c.execute("node", 0, &mut h);
    assert_eq!(
        start.stdout,
        "Welcome to Node.js v24.21.0.\nType \".help\" for more information.\n"
    );
    assert_eq!(c.session_prompt(), Some("> "));
    let r = c.execute("const x = 21", 1, &mut h);
    assert_eq!(r.stdout, "undefined\n");
    let r = c.execute("x * 2", 2, &mut h);
    assert_eq!(r.stdout, "42\n");
    let r = c.execute(".exit", 3, &mut h);
    assert_eq!(r.exit_code, 0, "{}", r.stdout);
    assert!(c.session.is_none());
}

#[test]
fn a_pipe_is_not_a_terminal() {
    let mut c = machine();
    let mut h = OfflineHost;
    // With something piped in, `python3` reads a program, as it does off a tty.
    let r = c.execute("echo 'print(1 + 1)' | python3", 0, &mut h);
    assert_eq!(r.stdout, "2\n");
    assert!(c.session.is_none());
    let mut c = Computer::new("pc", "user", "linux", true);
    let r = c.execute("python3 -c 'print(2 + 2)'", 0, &mut h);
    assert_eq!(r.stdout, "4\n");
    assert!(c.session.is_none());
}
