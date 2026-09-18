//! Development helper: runs a host file through the simulated interpreter on an
//! in-memory filesystem. `cargo run -p cw-jsvm --example jsrun -- file.js [args]`
//! Extra files: JSRUN_FILES="a.js:b.json" (copied next to main.js);
//! JSRUN_CWD sets the simulated working directory (default /home/user).
use cw_script_host::{memory::MemoryHost, Invocation};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a == "--raw").unwrap_or(false) {
        // `jsrun --raw <node args...>` with JSRUN_FILES placed in the cwd.
        let mut host = MemoryHost::default();
        let cwd = std::env::var("JSRUN_CWD").unwrap_or_else(|_| "/home/user".into());
        let mut d = String::new();
        for part in cwd.split('/').filter(|s| !s.is_empty()) {
            d.push('/');
            d.push_str(part);
            host.dirs.insert(d.clone());
        }
        host.cwd = cwd.clone();
        if let Ok(extra) = std::env::var("JSRUN_FILES") {
            for f in extra.split(':').filter(|s| !s.is_empty()) {
                let data = std::fs::read(f).expect("read extra file");
                let base = std::path::Path::new(f)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                host.files.insert(format!("{cwd}/{base}"), data);
            }
        }
        let stdin = std::env::var("JSRUN_STDIN").unwrap_or_default();
        let out = cw_jsvm::run(
            &mut host,
            &Invocation {
                args: args[1..].to_vec(),
                env: vec![],
                stdin,
                interactive: std::env::var("JSRUN_INTERACTIVE").is_ok(),
                eof: std::env::var("JSRUN_EOF").is_ok(),
            },
        );
        if out.awaiting_input {
            eprintln!("[awaiting input]");
        }
        print!("{}", out.stdout);
        eprint!("{}", out.stderr);
        std::process::exit(out.exit_code);
    }
    let path = &args[0];
    let src = std::fs::read_to_string(path).expect("read script");
    let mut host = MemoryHost::default();
    let cwd = std::env::var("JSRUN_CWD").unwrap_or_else(|_| "/home/user".into());
    let mut d = String::new();
    for part in cwd.split('/').filter(|s| !s.is_empty()) {
        d.push('/');
        d.push_str(part);
        host.dirs.insert(d.clone());
    }
    host.cwd = cwd.clone();
    let name = if path.ends_with(".mjs") {
        "main.mjs"
    } else {
        "main.js"
    };
    host.files.insert(format!("{cwd}/{name}"), src.into_bytes());
    if let Ok(extra) = std::env::var("JSRUN_FILES") {
        for f in extra.split(':').filter(|s| !s.is_empty()) {
            let data = std::fs::read(f).expect("read extra file");
            let base = std::path::Path::new(f)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            host.files.insert(format!("{cwd}/{base}"), data);
        }
    }
    let mut a = vec![name.to_string()];
    a.extend(args[1..].iter().cloned());
    let stdin = std::env::var("JSRUN_STDIN").unwrap_or_default();
    let out = cw_jsvm::run(
        &mut host,
        &Invocation {
            args: a,
            env: vec![],
            stdin,
            ..Default::default()
        },
    );
    print!("{}", out.stdout);
    eprint!("{}", out.stderr);
    std::process::exit(out.exit_code);
}
