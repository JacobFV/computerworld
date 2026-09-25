//! `cw-tsx`: the command-line compiler.
//!
//!     cw-tsx build app.tsx [-o out/] [--name app]
//!                                      writes out/app.ui.json (when the app is in the
//!                                      subset), out/app.js (the React fallback) and
//!                                      out/app.diagnostics.json. `app.tsx` is the
//!                                      entry: the modules it imports (`./x`) are
//!                                      compiled with it into one IR and one script.
//!     cw-tsx build app.tsx --emit rust [--mod name]
//!                                      also writes out/app.ui.rs: the IR as a Rust
//!                                      module (`pub mod name`, defining
//!                                      `PROGRAM: cw_ui::GenProgram`) for an app
//!                                      built into the binary
//!     cw-tsx check app.tsx             prints the diagnostics; exit 1 if any
//!     cw-tsx corpus <dir> [--split dev|test|all] [--json out.json] [--top n]
//!                                      evaluates the held-out corpus in <dir>
//!                                      (crates/web/tsx/corpus): how much of it is
//!                                      inside the compiled subset, and why not
//!
//! Exit status of `build`: 0 when both outputs were written, 3 when only the fallback
//! was (the module runs on React), 1 when neither could be.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("usage: cw-tsx build <entry.tsx> [-o <dir>] [--name <stem>] [--emit rust [--mod <name>]]\n       cw-tsx check <entry.tsx>");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => return usage(),
    };
    if cmd == "corpus" {
        return corpus(rest);
    }
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut name: Option<String> = None;
    let mut rust = false;
    let mut mod_name: Option<String> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--out" => out = it.next().map(PathBuf::from),
            "--name" => name = it.next().cloned(),
            "--emit" => match it.next().map(String::as_str) {
                Some("rust") => rust = true,
                _ => return usage(),
            },
            "--mod" => mod_name = it.next().cloned(),
            s if input.is_none() => input = Some(PathBuf::from(s)),
            _ => return usage(),
        }
    }
    let Some(input) = input else { return usage() };
    let root = input.parent().map(Path::to_path_buf).unwrap_or_default();
    let file_name = input
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app.tsx".into());
    let mut read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let sources = match cw_tsx::load(&file_name, &mut read) {
        Ok(s) => s,
        Err(diags) => {
            for d in &diags {
                eprintln!("{}: {d}", root.display());
            }
            return ExitCode::from(1);
        }
    };
    let build = cw_tsx::build_modules(&sources);
    for d in &build.diagnostics {
        if d.file.is_empty() {
            eprintln!("{}: {d}", input.display());
        } else {
            eprintln!("{}/{d}", root.display());
        }
    }
    match cmd {
        "check" => {
            if build.diagnostics.is_empty() {
                eprintln!("{}: compiles to the UI IR", input.display());
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        "build" => {
            let dir =
                out.unwrap_or_else(|| input.parent().map(Path::to_path_buf).unwrap_or_default());
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("cw-tsx: {}: {e}", dir.display());
                return ExitCode::from(1);
            }
            let stem = name.clone().unwrap_or_else(|| {
                file_name
                    .strip_suffix(".tsx")
                    .or_else(|| file_name.strip_suffix(".ts"))
                    .unwrap_or(&file_name)
                    .to_owned()
            });
            let write = |name: String, body: &str| -> bool {
                let p = dir.join(name);
                match std::fs::write(&p, body) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("cw-tsx: {}: {e}", p.display());
                        false
                    }
                }
            };
            let diags = serde_json::to_string_pretty(&build.diagnostics).expect("diagnostics");
            if !write(format!("{stem}.diagnostics.json"), &(diags + "\n")) {
                return ExitCode::from(1);
            }
            let Some(js) = &build.js else {
                return ExitCode::from(1);
            };
            if !write(format!("{stem}.js"), js) {
                return ExitCode::from(1);
            }
            let ir_path = dir.join(format!("{stem}.ui.json"));
            match &build.ir {
                Some(ir) => {
                    let text = serde_json::to_string(ir).expect("ir");
                    if !write(format!("{stem}.ui.json"), &(text + "\n")) {
                        return ExitCode::from(1);
                    }
                    if rust {
                        let m = mod_name.clone().unwrap_or_else(|| rust_ident(&stem));
                        if !write(format!("{stem}.ui.rs"), &cw_tsx::emit_rust::emit(ir, &m)) {
                            return ExitCode::from(1);
                        }
                    }
                    ExitCode::SUCCESS
                }
                None => {
                    // A stale IR next to a module that left the subset would run the
                    // old app; remove it so the page falls back.
                    let _ = std::fs::remove_file(&ir_path);
                    let _ = std::fs::remove_file(dir.join(format!("{stem}.ui.rs")));
                    eprintln!(
                        "{}: outside the compiled subset; the page will run on React",
                        input.display()
                    );
                    ExitCode::from(3)
                }
            }
        }
        _ => usage(),
    }
}

fn corpus(rest: &[String]) -> ExitCode {
    let mut dir: Option<PathBuf> = None;
    let mut split = "dev".to_owned();
    let mut json: Option<PathBuf> = None;
    let mut top = 40usize;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--split" => split = it.next().cloned().unwrap_or_default(),
            "--json" => json = it.next().map(PathBuf::from),
            "--top" => top = it.next().and_then(|n| n.parse().ok()).unwrap_or(top),
            s if dir.is_none() => dir = Some(PathBuf::from(s)),
            _ => return usage(),
        }
    }
    let Some(dir) = dir else { return usage() };
    let report = cw_tsx::corpus::evaluate(&dir, &split);
    print!("{}", report.summary(top));
    if let Some(p) = json {
        let text = serde_json::to_string_pretty(&report).expect("report") + "\n";
        if let Err(e) = std::fs::write(&p, text) {
            eprintln!("cw-tsx: {}: {e}", p.display());
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

/// A Rust module name for an output stem (`tsx-tasks` → `tsx_tasks`).
fn rust_ident(stem: &str) -> String {
    let mut s: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}
