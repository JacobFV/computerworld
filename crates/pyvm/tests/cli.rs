//! Command-line behaviour, limits and determinism of the simulated `python3`.
use cw_script_host::{memory::MemoryHost, Invocation, Outcome, ScriptHost};

fn run(host: &mut MemoryHost, args: &[&str], stdin: &str) -> Outcome {
    cw_pyvm::run(
        host,
        &Invocation {
            args: args.iter().map(|s| s.to_string()).collect(),
            env: vec![],
            stdin: stdin.to_string(),
            ..Default::default()
        },
    )
}

fn script(src: &str) -> Outcome {
    let mut host = MemoryHost::default();
    host.write_file("/home/user/main.py", src.as_bytes(), false)
        .unwrap();
    run(&mut host, &["main.py"], "")
}

#[test]
fn version_and_inline_code() {
    let mut h = MemoryHost::default();
    let out = run(&mut h, &["--version"], "");
    assert_eq!((out.stdout.as_str(), out.exit_code), ("Python 3.12.3\n", 0));
    let out = run(
        &mut h,
        &["-c", "import sys; print(sys.argv, 6 * 7)", "a", "b"],
        "",
    );
    assert_eq!(out.stdout, "['-c', 'a', 'b'] 42\n");
    let out = run(&mut h, &["-c", "print(undefined)"], "");
    assert_eq!(out.exit_code, 1);
    assert_eq!(
        out.stderr,
        "Traceback (most recent call last):\n  File \"<string>\", line 1, in <module>\nNameError: name 'undefined' is not defined\n"
    );
}

#[test]
fn program_from_stdin_when_no_script_is_named() {
    let mut h = MemoryHost::default();
    let out = run(&mut h, &[], "x = 5\nprint(x * 2)\n");
    assert_eq!((out.stdout.as_str(), out.exit_code), ("10\n", 0));
    let out = run(&mut h, &["-"], "print('dash')\n");
    assert_eq!(out.stdout, "dash\n");
}

#[test]
fn missing_script_and_bad_option() {
    let mut h = MemoryHost::default();
    let out = run(&mut h, &["nope.py"], "");
    assert_eq!(out.exit_code, 2);
    assert_eq!(
        out.stderr,
        "/usr/bin/python3: can't open file '/home/user/nope.py': [Errno 2] No such file or directory\n"
    );
    let out = run(&mut h, &["-Z"], "");
    assert_eq!(out.exit_code, 2);
    assert!(out.stderr.starts_with("Unknown option: -Z\nusage: python3"));
}

#[test]
fn script_arguments_reach_sys_argv() {
    let mut h = MemoryHost::default();
    h.write_file(
        "/home/user/app.py",
        b"import sys\nprint(sys.argv)\nprint(__file__, __name__)\n",
        false,
    )
    .unwrap();
    let out = run(&mut h, &["app.py", "--flag", "value"], "");
    assert_eq!(
        out.stdout,
        "['app.py', '--flag', 'value']\n/home/user/app.py __main__\n"
    );
}

#[test]
fn infinite_loops_stop_with_a_timeout_error() {
    let out = script("n = 0\nwhile True:\n    n += 1\n");
    assert_eq!(out.exit_code, cw_pyvm::TIMEOUT_EXIT);
    assert!(
        out.stderr
            .starts_with("Traceback (most recent call last):\n  File \"/home/user/main.py\", line"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.ends_with(
        "TimeoutError: execution step limit exceeded (50000000 steps); the simulated CPU budget is exhausted\n"
    ));
    // The budget cannot be caught.
    let out = script("try:\n    while True: pass\nexcept BaseException:\n    print('caught')\n");
    assert_eq!(out.exit_code, cw_pyvm::TIMEOUT_EXIT);
    assert!(!out.stdout.contains("caught"));
}

#[test]
fn deep_recursion_is_a_recursion_error_not_a_crash() {
    let out = script("def f(n):\n    return f(n + 1)\nf(0)\n");
    assert_eq!(out.exit_code, 1);
    assert!(out
        .stderr
        .ends_with("RecursionError: maximum recursion depth exceeded\n"));
    assert!(out
        .stderr
        .contains("[Previous line repeated 996 more times]"));
    // Recursion through natively-driven calls (generators, sort keys, dunders).
    let out = script(
        "def walk(n):\n    if n:\n        yield from walk(n - 1)\n    yield n\nprint(sum(walk(120)))\n\nclass N:\n    def __init__(self, k): self.k = k\n    def __eq__(self, o): return self.k == 0 or N(self.k - 1) == N(o.k - 1)\nprint(N(100) == N(100))\ndef g(n):\n    return sorted([n], key=lambda x: g(x - 1) if x else 0)\nprint(g(60))\n",
    );
    assert_eq!(out.stdout, "7260\nTrue\n[60]\n", "{}", out.stderr);
    let out = script("class R:\n    def __repr__(self): return repr(self)\nrepr(R())\n");
    assert_eq!(out.exit_code, 1);
    assert!(out.stderr.contains("RecursionError"), "{}", out.stderr);
}

#[test]
fn unseeded_random_is_deterministic_per_world_seed() {
    let src = "import random\nprint(random.random(), random.randint(1, 10**6))\n";
    let a = script(src);
    let b = script(src);
    assert_eq!(a.stdout, b.stdout);
    let mut host = MemoryHost {
        rng: 7,
        ..MemoryHost::default()
    };
    host.write_file("/home/user/main.py", src.as_bytes(), false)
        .unwrap();
    let c = run(&mut host, &["main.py"], "");
    assert_ne!(
        a.stdout, c.stdout,
        "a different world seed gives a different stream"
    );
    // Seeded streams match CPython exactly.
    let out = script("import random\nrandom.seed(1)\nprint(random.random(), random.randrange(100), random.choice('xyz'))\n");
    assert_eq!(out.stdout, "0.13436424411240122 97 x\n");
}

#[test]
fn time_comes_from_the_world_clock() {
    let out = script("import time, datetime\nprint(time.time())\nprint(datetime.datetime.now())\ntime.sleep(2.5)\nprint(time.time())\n");
    assert_eq!(
        out.stdout,
        "1789635600.0\n2026-09-17 09:00:00\n1789635602.5\n"
    );
}

#[test]
fn user_modules_and_packages_import_from_the_script_directory() {
    let mut h = MemoryHost::default();
    h.mkdir("/home/user/proj", false).unwrap();
    h.mkdir("/home/user/proj/pkg", false).unwrap();
    h.write_file(
        "/home/user/proj/helpers.py",
        b"def double(x):\n    return 2 * x\nNAME = 'helpers'\n",
        false,
    )
    .unwrap();
    h.write_file(
        "/home/user/proj/pkg/__init__.py",
        b"from .core import VALUE\n",
        false,
    )
    .unwrap();
    h.write_file("/home/user/proj/pkg/core.py", b"VALUE = 42\n", false)
        .unwrap();
    h.write_file(
        "/home/user/proj/main.py",
        b"import helpers\nfrom helpers import double as d\nimport pkg\nfrom pkg.core import VALUE\nprint(helpers.double(4), d(5), pkg.VALUE, VALUE, helpers.NAME)\nimport missing_mod\n",
        false,
    )
    .unwrap();
    let out = run(&mut h, &["proj/main.py"], "");
    assert_eq!(out.stdout, "8 10 42 42 helpers\n");
    assert!(out
        .stderr
        .ends_with("ModuleNotFoundError: No module named 'missing_mod'\n"));
    assert_eq!(out.exit_code, 1);
}

#[test]
fn syntax_warnings_and_errors_use_cpython_format() {
    let out = script("import re\nprint(re.findall('\\d+', 'a1b22'))\n");
    assert_eq!(out.stdout, "['1', '22']\n");
    assert_eq!(
        out.stderr,
        "/home/user/main.py:2: SyntaxWarning: invalid escape sequence '\\d'\n  print(re.findall('\\d+', 'a1b22'))\n"
    );
    let out = script("print('unclosed\n");
    assert_eq!(out.exit_code, 1);
    assert_eq!(
        out.stderr,
        "  File \"/home/user/main.py\", line 1\n    print('unclosed\n          ^\nSyntaxError: unterminated string literal (detected at line 1)\n"
    );
}

#[test]
fn files_written_without_close_are_flushed_at_exit() {
    let mut h = MemoryHost::default();
    h.write_file(
        "/home/user/main.py",
        b"f = open('out.txt', 'w')\nf.write('kept')\n",
        false,
    )
    .unwrap();
    let out = run(&mut h, &["main.py"], "");
    assert_eq!(out.exit_code, 0);
    assert_eq!(h.files["/home/user/out.txt"], b"kept");
}
