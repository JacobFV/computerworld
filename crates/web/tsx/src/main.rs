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
//!     cw-tsx corpus <dir> [--split dev|test|all] [--json out.json] [--top n] [--grep text] [--no-islands]
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
    let mut root_arg: Option<PathBuf> = None;
    let mut env = std::collections::BTreeMap::new();
    let mut aliases: Vec<(String, String)> = Vec::new();
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
            "--root" => root_arg = it.next().map(PathBuf::from),
            "--alias" => {
                // `--alias FROM=TO`: an import of FROM (a package name, or a prefix
                // ending in `/`) resolves to TO, relative to the root.
                let Some((k, v)) = it.next().and_then(|a| a.split_once('=')) else {
                    return usage();
                };
                aliases.push((k.to_owned(), v.to_owned()));
            }
            "--env" => {
                // `--env NAME=VALUE`: `process.env.NAME` in the app and its packages.
                let Some((k, v)) = it.next().and_then(|a| a.split_once('=')) else {
                    return usage();
                };
                env.insert(k.to_owned(), v.to_owned());
            }
            s if input.is_none() => input = Some(PathBuf::from(s)),
            _ => return usage(),
        }
    }
    let Some(input) = input else { return usage() };
    // The app's root: `--root`, else the nearest directory above the entry whose
    // node_modules has React (an npm project's root), else the entry's own.
    let dir = input.parent().map(Path::to_path_buf).unwrap_or_default();
    let root = match root_arg {
        Some(r) => r,
        None => dir
            .ancestors()
            .find(|a| a.join("node_modules/react/package.json").is_file())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| dir.clone()),
    };
    let abs = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let file_name = abs(&input)
        .strip_prefix(abs(&root))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| {
            input
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "app.tsx".into())
        });
    let options = cw_tsx::LoadOptions {
        aliases,
        node_modules: root
            .join("node_modules")
            .is_dir()
            .then(|| "node_modules".to_owned()),
        env,
    };
    let mut read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let (sources, load_errors) = cw_tsx::load_with(&file_name, &mut read, &options);
    if !load_errors.is_empty() {
        for d in &load_errors {
            eprintln!("{}: {d}", root.display());
        }
        return ExitCode::from(1);
    }
    let mut build = cw_tsx::build_modules(&sources);
    // The React the app is built with (its installed `react`), whose DOM writes
    // the runtime reproduces.
    if let Some(ir) = build.ir.as_mut() {
        let version = std::fs::read_to_string(root.join("node_modules/react/package.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["version"].as_str().map(str::to_owned));
        if let Some(major) = version.and_then(|v| v.split('.').next()?.parse::<u32>().ok()) {
            ir.react = major;
        }
    }
    for d in &build.diagnostics {
        if d.file.is_empty() {
            eprintln!("{}: {d}", input.display());
        } else {
            eprintln!("{}/{d}", root.display());
        }
    }
    if !build.island_modules.is_empty() {
        for d in &build.outside {
            eprintln!("{}/{d} (runs on the island)", root.display());
        }
        eprintln!(
            "{}: on the island, outside the subset: {}",
            input.display(),
            build.island_modules.join(", ")
        );
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
    let mut grep: Option<String> = None;
    let mut lower = cw_tsx::lower::LowerOptions::default();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--split" => split = it.next().cloned().unwrap_or_default(),
            "--json" => json = it.next().map(PathBuf::from),
            "--top" => top = it.next().and_then(|n| n.parse().ok()).unwrap_or(top),
            "--grep" => grep = it.next().cloned(),
            "--no-islands" => lower.islands = false,
            s if dir.is_none() => dir = Some(PathBuf::from(s)),
            _ => return usage(),
        }
    }
    let Some(dir) = dir else { return usage() };
    let report = cw_tsx::corpus::evaluate_opts(&dir, &split, &lower, &mut |project, d| {
        if let Some(g) = &grep {
            if d.message.contains(g.as_str()) {
                println!("{project}: {d}");
            }
        }
    });
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
