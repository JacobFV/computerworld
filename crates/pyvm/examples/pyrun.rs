//! Development helper: runs a host file through the simulated interpreter on an
//! in-memory filesystem. `cargo run -p cw-pyvm --example pyrun -- file.py [args]`
use cw_script_host::{memory::MemoryHost, Invocation};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = &args[0];
    let src = std::fs::read_to_string(path).expect("read script");
    let mut host = MemoryHost::default();
    host.files
        .insert("/home/user/main.py".into(), src.into_bytes());
    let mut a = vec!["main.py".to_string()];
    a.extend(args[1..].iter().cloned());
    let stdin = std::env::var("PYRUN_STDIN").unwrap_or_default();
    let out = cw_pyvm::run(
        &mut host,
        &Invocation {
            args: a,
            env: vec![],
            stdin,
        },
    );
    print!("{}", out.stdout);
    eprint!("{}", out.stderr);
    std::process::exit(out.exit_code);
}
