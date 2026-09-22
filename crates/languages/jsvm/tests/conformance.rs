//! Runs every program in `tests/programs` and compares stdout, stderr and the exit
//! status with the output Node.js v24 produced for it (`NAME.expected`, written by
//! `generate_expected.py`, which is run by hand and never from these tests).
use cw_script_host::{memory::MemoryHost, Invocation, ScriptHost};

struct Expected {
    exit: i32,
    stdout: String,
    stderr: String,
}

fn parse_expected(text: &str) -> Expected {
    let rest = text.strip_prefix("--- exit: ").expect("exit header");
    let (code, rest) = rest.split_once('\n').expect("exit line");
    let rest = rest.strip_prefix("--- stdout\n").expect("stdout header");
    let (stdout, stderr) = rest.split_once("--- stderr\n").expect("stderr header");
    Expected {
        exit: code.trim().parse().expect("exit code"),
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

fn run_program(name: &str, ext: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/programs");
    let src = std::fs::read_to_string(format!("{dir}/{name}.{ext}")).unwrap();
    let expected =
        parse_expected(&std::fs::read_to_string(format!("{dir}/{name}.expected")).unwrap());
    let stdin = std::fs::read_to_string(format!("{dir}/{name}.stdin")).unwrap_or_default();
    let mut host = MemoryHost::default();
    let main = format!("main.{ext}");
    host.write_file(&format!("/home/user/{main}"), src.as_bytes(), false)
        .unwrap();
    let out = cw_jsvm::run(
        &mut host,
        &Invocation {
            args: vec![main],
            env: vec![],
            stdin,
            ..Default::default()
        },
    );
    assert_eq!(out.stdout, expected.stdout, "{name}: stdout differs");
    assert_eq!(out.stderr, expected.stderr, "{name}: stderr differs");
    assert_eq!(out.exit_code, expected.exit, "{name}: exit status differs");
}

macro_rules! programs {
    ($ext:literal: $($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                run_program(stringify!($name), $ext);
            }
        )*
    };
}

programs!("js":
    algorithms,
    async_generators,
    atomics,
    basics,
    builtins,
    classes_errors,
    err_assert,
    err_custom,
    err_reference,
    err_syntax,
    err_throw_value,
    err_type,
    err_unhandled_rejection,
    event_loop,
    event_loop_time,
    exit_code,
    framework_features,
    inspect,
    intl,
    json,
    language,
    modules_fs,
    packages,
    stdin_events,
    strings_regex,
    text_pipeline,
    util_events,
    web_fetch_classes,
    workers,
    zlib_streams,
);

programs!("mjs": esm_module);

/// Development aid: `JSVM_FILE=/path/x.js cargo test --test conformance scratch_file -- --ignored --nocapture`
/// runs a file and prints what it wrote.
#[test]
#[ignore]
fn scratch_file() {
    let Ok(path) = std::env::var("JSVM_FILE") else {
        return;
    };
    let src = std::fs::read_to_string(&path).unwrap();
    let mut host = MemoryHost::default();
    host.write_file("/home/user/main.js", src.as_bytes(), false)
        .unwrap();
    let out = cw_jsvm::run(
        &mut host,
        &Invocation {
            args: vec!["main.js".into()],
            env: vec![],
            stdin: String::new(),
            ..Default::default()
        },
    );
    eprintln!(
        "--- exit {}\n--- stdout\n{}--- stderr\n{}",
        out.exit_code, out.stdout, out.stderr
    );
}
