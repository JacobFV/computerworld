//! Embeds every file under `packages/` in the library, as a table of
//! `(package, path, bytes)`: the apps are code, compiled into the registry.
use std::fmt::Write;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("packages");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut table = String::from("pub static FILES: &[(&str, &str, &[u8])] = &[\n");
    for f in files {
        println!("cargo:rerun-if-changed={}", f.display());
        let rel = f.strip_prefix(&root).unwrap();
        let mut parts = rel.components();
        let package = parts
            .next()
            .unwrap()
            .as_os_str()
            .to_string_lossy()
            .into_owned();
        let path: Vec<String> = parts
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        writeln!(
            table,
            "    ({package:?}, {:?}, include_bytes!({:?})),",
            path.join("/"),
            f.display().to_string()
        )
        .unwrap();
    }
    table.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("packages.rs");
    std::fs::write(out, table).unwrap();
}
