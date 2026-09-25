//! Generates Rust for every TSX app cw-ui's tests run (see src/lib.rs).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ui = here.join("..");
    let parity = ui.join("../engine/tests/framework-parity");
    let cw_d_ts_path = ui.join("../../applications/web/types/cw.d.ts");
    let cw_d_ts = std::fs::read_to_string(&cw_d_ts_path).expect("cw.d.ts");
    println!("cargo:rerun-if-changed={}", cw_d_ts_path.display());

    let mut modules: Vec<cw_ui::ir::Module> = Vec::new();

    // Inline sources in the tests: every raw string that renders an app.
    for test in ["react_semantics.rs", "cw_bridge.rs", "perf.rs"] {
        let p = ui.join("tests").join(test);
        println!("cargo:rerun-if-changed={}", p.display());
        let text = std::fs::read_to_string(&p).expect("test source");
        for src in raw_strings(&text) {
            if !src.contains("createRoot(") {
                continue;
            }
            let b = if let Some(files) = cw_tsx::virtual_files(&src) {
                let packages = ui.join("tests/islands/packages");
                match cw_tsx::build_virtual_with(&files, Some(&packages)) {
                    Ok(b) => b,
                    Err(_) => continue,
                }
            } else if src.contains("cw.d.ts") {
                let files: BTreeMap<&str, &str> =
                    [("app.tsx", src.as_str()), ("cw.d.ts", cw_d_ts.as_str())].into();
                match cw_tsx::load("app.tsx", &mut |f| files.get(f).map(|s| (*s).to_owned())) {
                    Ok(sources) => cw_tsx::build_modules(&sources),
                    Err(_) => continue,
                }
            } else {
                cw_tsx::build(&src, "app.tsx")
            };
            if let Some(ir) = b.ir {
                modules.push(ir);
            }
        }
    }

    // framework-parity/tsx-*.tsx.
    println!("cargo:rerun-if-changed={}", parity.display());
    let mut names: Vec<String> = std::fs::read_dir(&parity)
        .expect("framework-parity")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("tsx-") && n.ends_with(".tsx"))
        .collect();
    names.sort();
    for n in names {
        let p = parity.join(&n);
        println!("cargo:rerun-if-changed={}", p.display());
        let src = std::fs::read_to_string(&p).unwrap();
        if let Some(ir) = cw_tsx::build(&src, &n).ir {
            modules.push(ir);
        }
    }

    // framework-parity/app-src/<app>/main.tsx.
    let app_src = parity.join("app-src");
    let mut apps: Vec<PathBuf> = std::fs::read_dir(&app_src)
        .expect("app-src")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("main.tsx").is_file())
        .collect();
    apps.sort();
    watch_dir(&app_src);
    watch_dir(&ui.join("tests/islands"));
    for root in apps {
        let mut read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
        if let Ok(sources) = cw_tsx::load("main.tsx", &mut read) {
            if let Some(ir) = cw_tsx::build_modules(&sources).ir {
                modules.push(ir);
            }
        }
    }

    // One module per distinct IR.
    let mut seen = std::collections::BTreeSet::new();
    let mut out = String::new();
    let mut names = Vec::new();
    for m in &modules {
        let h = cw_ui::program::ir_hash(m);
        if !seen.insert(h) {
            continue;
        }
        let name = format!("fx_{h:016x}");
        out.push_str(&cw_tsx::emit_rust::emit(m, &name));
        out.push('\n');
        names.push(name);
    }
    // Notes, from its checked-in generated module.
    let notes = ui.join("../../applications/web/notes/notes.ui.rs");
    println!("cargo:rerun-if-changed={}", notes.display());
    if notes.is_file() {
        let _ = writeln!(out, "include!({:?});", notes.display().to_string());
        names.push("notes".into());
    }
    let _ = writeln!(
        out,
        "/// Every generated fixture.\npub static PROGRAMS: [&cw_ui::GenProgram; {}] = [{}];",
        names.len(),
        names
            .iter()
            .map(|n| format!("&{n}::PROGRAM"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("fixtures.rs");
    std::fs::write(dest, out).unwrap();
}

fn watch_dir(dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                watch_dir(&p);
            } else {
                println!("cargo:rerun-if-changed={}", p.display());
            }
        }
    }
}

/// The contents of every raw string literal (`r#"…"#`, any number of `#`).
fn raw_strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'r' && (b[i + 1] == b'#' || b[i + 1] == b'"') && (i == 0 || !is_ident(b[i - 1]))
        {
            let mut j = i + 1;
            let mut hashes = 0;
            while j < b.len() && b[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < b.len() && b[j] == b'"' {
                let start = j + 1;
                let end_pat = format!("\"{}", "#".repeat(hashes));
                if let Some(e) = text[start..].find(&end_pat) {
                    out.push(text[start..start + e].to_owned());
                    i = start + e + end_pat.len();
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}
