//! Command-line behaviour, limits and determinism of the simulated `node`.
use cw_script_host::{memory::MemoryHost, Invocation, Outcome, ScriptHost};

fn run(host: &mut MemoryHost, args: &[&str], stdin: &str) -> Outcome {
    cw_jsvm::run(
        host,
        &Invocation {
            args: args.iter().map(|s| s.to_string()).collect(),
            env: vec![],
            stdin: stdin.to_string(),
            ..Default::default()
        },
    )
}

fn script_on(host: &mut MemoryHost, src: &str) -> Outcome {
    host.write_file("/home/user/main.js", src.as_bytes(), false)
        .unwrap();
    run(host, &["main.js"], "")
}

fn script(src: &str) -> Outcome {
    script_on(&mut MemoryHost::default(), src)
}

#[test]
fn version_eval_and_print() {
    let mut h = MemoryHost::default();
    let out = run(&mut h, &["--version"], "");
    assert_eq!((out.stdout.as_str(), out.exit_code), ("v24.21.0\n", 0));
    let out = run(
        &mut h,
        &["-e", "console.log(process.argv, 6 * 7)", "a", "b"],
        "",
    );
    assert_eq!(out.stdout, "[ '/usr/bin/node', 'a', 'b' ] 42\n");
    let out = run(&mut h, &["-p", "[1, 2].map((x) => x * 2)"], "");
    assert_eq!(out.stdout, "[ 2, 4 ]\n");
    let out = run(&mut h, &["-p", "'text'"], "");
    assert_eq!(out.stdout, "text\n");
    let out = run(&mut h, &["-e", "undefinedName"], "");
    assert_eq!(out.exit_code, 1);
    assert!(
        out.stderr.starts_with(
            "[eval]:1\nundefinedName\n^\n\nReferenceError: undefinedName is not defined\n    at [eval]:1:1\n"
        ),
        "{}",
        out.stderr
    );
    assert!(out.stderr.ends_with("\n\nNode.js v24.21.0\n"));
}

#[test]
fn program_from_stdin_and_syntax_check() {
    let mut h = MemoryHost::default();
    let out = run(&mut h, &[], "const x = 5;\nconsole.log(x * 2);\n");
    assert_eq!((out.stdout.as_str(), out.exit_code), ("10\n", 0));
    let out = run(&mut h, &["-"], "console.log(require('path').sep)\n");
    assert_eq!(out.stdout, "/\n");
    h.write_file("/home/user/ok.js", b"let a = 1;\n", false)
        .unwrap();
    h.write_file("/home/user/bad.js", b"let a = ;\n", false)
        .unwrap();
    let out = run(&mut h, &["--check", "ok.js"], "");
    assert_eq!((out.stdout.as_str(), out.exit_code), ("", 0));
    let out = run(&mut h, &["-c", "bad.js"], "");
    assert_eq!(out.exit_code, 1);
    assert!(
        out.stderr.starts_with(
            "/home/user/bad.js:1\nlet a = ;\n        ^\n\nSyntaxError: Unexpected token ';'\n"
        ),
        "{}",
        out.stderr
    );
}

#[test]
fn missing_script_and_bad_option() {
    let mut h = MemoryHost::default();
    let out = run(&mut h, &["nope.js"], "");
    assert_eq!(out.exit_code, 1);
    assert!(
        out.stderr.starts_with("node:internal/modules/cjs/loader:"),
        "{}",
        out.stderr
    );
    assert!(out
        .stderr
        .contains("Error: Cannot find module '/home/user/nope.js'\n"));
    assert!(out
        .stderr
        .contains("  code: 'MODULE_NOT_FOUND',\n  requireStack: []\n}\n"));
    let out = run(&mut h, &["--bogus"], "");
    assert_eq!(out.exit_code, 9);
    assert_eq!(out.stderr, "/usr/bin/node: bad option: --bogus\n");
}

#[test]
fn script_arguments_reach_process_argv() {
    let mut h = MemoryHost::default();
    h.write_file(
        "/home/user/app.js",
        b"console.log(process.argv.slice(1), __filename, require.main === module)\n",
        false,
    )
    .unwrap();
    let out = run(&mut h, &["app.js", "--flag", "value"], "");
    assert_eq!(
        out.stdout,
        "[ '/home/user/app.js', '--flag', 'value' ] /home/user/app.js true\n"
    );
}

#[test]
fn infinite_loops_stop_with_a_timeout_error() {
    let out = script("let n = 0;\nwhile (true) n++;\n");
    assert_eq!(out.exit_code, cw_jsvm::TIMEOUT_EXIT);
    assert!(
        out.stderr
            .contains("TimeoutError: execution step limit exceeded"),
        "{}",
        out.stderr
    );
    // The budget cannot be caught, and pending timers do not run.
    let out = script(
        "setTimeout(() => console.log('timer'), 0);\ntry { for (;;) {} } catch (e) { console.log('caught'); }\n",
    );
    assert_eq!(out.exit_code, cw_jsvm::TIMEOUT_EXIT);
    assert_eq!(out.stdout, "");
    // Runaway promise chains and timers are bounded too.
    let out = script("function spin() { Promise.resolve().then(spin); }\nspin();\n");
    assert_eq!(out.exit_code, cw_jsvm::TIMEOUT_EXIT);
    let out = script("setInterval(() => {}, 1);\n");
    assert_eq!(out.exit_code, cw_jsvm::TIMEOUT_EXIT);
}

#[test]
fn deep_recursion_is_a_range_error_not_a_crash() {
    let out = script("function f(n) { return f(n + 1); }\nf(0);\n");
    assert_eq!(out.exit_code, 1);
    assert!(
        out.stderr
            .contains("RangeError: Maximum call stack size exceeded\n"),
        "{}",
        out.stderr
    );
    let out = script(
        "function f(n) { return f(n + 1); }\ntry { f(0); } catch (e) { console.log(e instanceof RangeError); }\nconst o = {}; o.toString = function () { return String(this); };\ntry { String(o); } catch (e) { console.log(e.message); }\n",
    );
    assert_eq!(
        out.stdout, "true\nMaximum call stack size exceeded\n",
        "{}",
        out.stderr
    );
}

#[test]
fn randomness_and_time_come_from_the_world() {
    let src = "console.log(Math.random(), require('crypto').randomUUID(), Date.now(), new Date().toISOString());\n";
    let a = script(src);
    let b = script(src);
    assert_eq!(a.stdout, b.stdout, "same world, same output");
    assert!(
        a.stdout.contains(" 1789635600000 2026-09-17T09:00:00.000Z"),
        "{}",
        a.stdout
    );
    let mut other = MemoryHost {
        rng: 7,
        now_micros: 1_000_000_000_000_000,
        ..MemoryHost::default()
    };
    let c = script_on(&mut other, src);
    assert_ne!(a.stdout, c.stdout, "a different world seed and clock");
    assert!(
        c.stdout.contains("2001-09-09T01:46:40.000Z"),
        "{}",
        c.stdout
    );
}

#[test]
fn virtual_time_passes_for_timers_and_busy_loops() {
    let out = script(
        "const t0 = Date.now();\nsetTimeout(() => console.log('slept', Date.now() - t0 >= 1500), 1500);\nconst p0 = performance.now();\nwhile (performance.now() - p0 < 50) {}\nconsole.log('busy loop ended');\n",
    );
    assert_eq!(
        out.stdout, "busy loop ended\nslept true\n",
        "{}",
        out.stderr
    );
    assert_eq!(out.exit_code, 0);
}

#[test]
fn files_written_by_node_persist_on_the_host() {
    let mut h = MemoryHost::default();
    let out = script_on(
        &mut h,
        "const fs = require('fs');\nfs.mkdirSync('out/deep', { recursive: true });\nfs.writeFileSync('out/deep/a.txt', 'hello');\nfs.appendFileSync('/tmp/log', 'x');\nconsole.log(fs.readdirSync('out'));\n",
    );
    assert_eq!(out.stdout, "[ 'deep' ]\n", "{}", out.stderr);
    assert_eq!(h.files["/home/user/out/deep/a.txt"], b"hello");
    assert_eq!(h.files["/tmp/log"], b"x");
}

#[test]
fn stdin_is_readable_through_fs_and_process_stdin() {
    let mut h = MemoryHost::default();
    h.write_file(
        "/home/user/main.js",
        b"const text = require('fs').readFileSync(0, 'utf8');\nconsole.log(text.trim().split('\\n').map(Number).reduce((a, b) => a + b));\n",
        false,
    )
    .unwrap();
    let out = run(&mut h, &["main.js"], "1\n2\n3\n");
    assert_eq!(out.stdout, "6\n");
    h.write_file(
        "/home/user/lines.js",
        b"const rl = require('readline').createInterface({ input: process.stdin });\nconst seen = [];\nrl.on('line', (l) => seen.push(l.toUpperCase()));\nrl.on('close', () => console.log(seen.join(',')));\n",
        false,
    )
    .unwrap();
    let out = run(&mut h, &["lines.js"], "a\nb\nc");
    assert_eq!(out.stdout, "A,B,C\n");
}

#[test]
fn esm_by_extension_and_flag() {
    let mut h = MemoryHost::default();
    h.write_file(
        "/home/user/m.mjs",
        b"import { basename } from 'path';\nconst v = await Promise.resolve(basename('/a/b.txt'));\nconsole.log(v, typeof require);\n",
        false,
    )
    .unwrap();
    let out = run(&mut h, &["m.mjs"], "");
    assert_eq!(out.stdout, "b.txt undefined\n", "{}", out.stderr);
    let out = run(
        &mut h,
        &[
            "--input-type=module",
            "-e",
            "import os from 'os'; console.log(os.EOL === '\\n')",
        ],
        "",
    );
    assert_eq!(out.stdout, "true\n", "{}", out.stderr);
}
