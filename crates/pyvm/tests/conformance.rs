//! Runs every program in `tests/programs` and compares stdout, stderr and the exit
//! status with the output CPython 3.12 produced for it (`NAME.expected`, written by
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

fn run_program(name: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/programs");
    let src = std::fs::read_to_string(format!("{dir}/{name}.py")).unwrap();
    let expected =
        parse_expected(&std::fs::read_to_string(format!("{dir}/{name}.expected")).unwrap());
    let stdin = std::fs::read_to_string(format!("{dir}/{name}.stdin")).unwrap_or_default();
    let mut host = MemoryHost::default();
    host.write_file("/home/user/main.py", src.as_bytes(), false)
        .unwrap();
    let out = cw_pyvm::run(
        &mut host,
        &Invocation {
            args: vec!["main.py".into()],
            env: vec![],
            stdin,
            ..Default::default()
        },
    );
    let fix = |s: &str| {
        // os.listdir order is unspecified; the simulated one is sorted.
        s.replace("['out.txt', 'main.py']", "['main.py', 'out.txt']")
    };
    assert_eq!(
        fix(&out.stdout),
        fix(&expected.stdout),
        "{name}: stdout differs"
    );
    assert_eq!(out.stderr, expected.stderr, "{name}: stderr differs");
    assert_eq!(out.exit_code, expected.exit, "{name}: exit status differs");
}

macro_rules! programs {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                run_program(stringify!($name));
            }
        )*
    };
}

programs!(
    algorithms,
    classes,
    compression,
    urls,
    err_chain,
    err_exit,
    err_name,
    err_nested,
    err_recursion,
    err_syntax,
    exceptions,
    files,
    formatting,
    generators,
    language,
    stdin_io,
    stdlib,
    text_processing,
    tools,
);
