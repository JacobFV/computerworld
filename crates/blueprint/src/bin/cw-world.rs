//! Build a world definition from a blueprint.
//!
//! This is the one place in the crate that touches the host: it reads the
//! blueprint's directory, reads the environment, and writes the world file. The
//! resolver itself is pure, so what a world is built from is always an argument
//! and never something the process happened to be able to reach.
use cw_blueprint::{resolve, Entry, Error, Files};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(message) => {
            println!("{message}");
            std::process::ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
usage: cw-world <command> [options] <blueprint.yml>

  build      resolve the blueprint and write the world definition
  check      resolve it and verify the written world is up to date
  validate   resolve it and report, writing nothing
  inputs     list the inputs the blueprint declares

options:
  -o, --out <path>     where to write (default: world.json beside the blueprint)
  --root <dir>         the directory paths resolve against (default: the
                       blueprint's own directory)
  --set NAME=VALUE     supply an input, in preference to the environment
";

struct Args {
    command: String,
    blueprint: PathBuf,
    out: Option<PathBuf>,
    root: Option<PathBuf>,
    set: BTreeMap<String, String>,
}

fn parse_args() -> Result<Args, String> {
    let mut rest = std::env::args().skip(1);
    let mut command = None;
    let mut blueprint = None;
    let mut out = None;
    let mut root = None;
    let mut set = BTreeMap::new();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(USAGE.to_string()),
            "-o" | "--out" => {
                out = Some(PathBuf::from(rest.next().ok_or("--out needs a path")?));
            }
            "--root" => {
                root = Some(PathBuf::from(
                    rest.next().ok_or("--root needs a directory")?,
                ));
            }
            "--set" => {
                let pair = rest.next().ok_or("--set needs NAME=VALUE")?;
                let (name, value) = pair.split_once('=').ok_or("--set takes NAME=VALUE")?;
                set.insert(name.to_string(), value.to_string());
            }
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other if command.is_none() => command = Some(other.to_string()),
            other if blueprint.is_none() => blueprint = Some(PathBuf::from(other)),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    let command = command.ok_or(USAGE)?;
    if !["build", "check", "validate", "inputs"].contains(&command.as_str()) {
        return Err(format!("unknown command {command}\n\n{USAGE}"));
    }
    Ok(Args {
        command,
        blueprint: blueprint.ok_or("no blueprint given")?,
        out,
        root,
        set,
    })
}

fn run() -> Result<String, String> {
    let args = parse_args()?;
    let blueprint = args
        .blueprint
        .canonicalize()
        .map_err(|e| format!("{}: {e}", args.blueprint.display()))?;
    let root = match &args.root {
        Some(root) => root
            .canonicalize()
            .map_err(|e| format!("{}: {e}", root.display()))?,
        None => blueprint.parent().unwrap_or(Path::new(".")).to_path_buf(),
    };
    let relative = blueprint
        .strip_prefix(&root)
        .map_err(|_| format!("{} is not inside {}", blueprint.display(), root.display()))?
        .to_string_lossy()
        .replace('\\', "/");

    let files = HostFiles { root: root.clone() };
    if args.command == "inputs" {
        return describe_inputs(&relative, &files);
    }

    let mut supplied: BTreeMap<String, String> = std::env::vars().collect();
    supplied.extend(args.set);
    let resolved = resolve(&relative, &files, &supplied).map_err(|e: Error| e.to_string())?;
    let text = resolved.to_world_json();
    let definition = resolved.definition().map_err(|e| e.to_string())?;

    let out = args.out.unwrap_or_else(|| {
        blueprint
            .parent()
            .unwrap_or(Path::new("."))
            .join("world.json")
    });
    let summary = format!(
        "{}: {} computers, {} services, {} network nodes, {} DNS records, {} bytes",
        definition.id,
        definition.computers.len(),
        definition.services.len(),
        definition.network.nodes.len(),
        definition.network.dns.len(),
        text.len(),
    );

    match args.command.as_str() {
        "validate" => Ok(summary),
        "check" => match std::fs::read_to_string(&out) {
            Ok(current) if current == text => {
                Ok(format!("{} is up to date\n{summary}", out.display()))
            }
            Ok(current) => Err(stale(&out, &current, &text)),
            Err(e) => Err(format!(
                "{}: {e}\nrun: cw-world build {}",
                out.display(),
                relative
            )),
        },
        _ => {
            std::fs::write(&out, &text).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(format!("wrote {}\n{summary}", out.display()))
        }
    }
}

/// Say where the written world stopped matching its blueprint. A diff of a
/// multi-megabyte generated file is unreadable; the offset and its surroundings
/// are what actually locates the change.
fn stale(out: &Path, current: &str, fresh: &str) -> String {
    let at = current
        .bytes()
        .zip(fresh.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(current.len().min(fresh.len()));
    let window = |text: &str| {
        let start = text[..at.min(text.len())]
            .char_indices()
            .rev()
            .nth(60)
            .map_or(0, |(i, _)| i);
        let end = text.len().min(at + 60);
        text[start..end].escape_debug().to_string()
    };
    format!(
        "{} is stale: it differs from its blueprint at byte {at}\n  written:  {}\n  blueprint: {}\n\
         \nrun: cw-world build",
        out.display(),
        window(current),
        window(fresh),
    )
}

fn describe_inputs(relative: &str, files: &HostFiles) -> Result<String, String> {
    // Resolving with nothing supplied reports every input that has no default,
    // which is exactly the list someone needs before they can build.
    let empty = BTreeMap::new();
    match resolve(relative, files, &empty) {
        Ok(resolved) if resolved.inputs.is_empty() => Ok("declares no inputs".to_string()),
        Ok(resolved) => Ok(resolved
            .inputs
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("\n")),
        Err(e) => Ok(e.to_string()),
    }
}

/// The blueprint's directory, and nothing outside it.
struct HostFiles {
    root: PathBuf,
}

impl HostFiles {
    fn locate(&self, path: &str) -> Result<PathBuf, String> {
        let full = self.root.join(path);
        // `canonicalize` resolves symlinks, so a link pointing out of the tree is
        // caught here rather than silently seeding a world from elsewhere.
        let real = full.canonicalize().map_err(|e| e.to_string())?;
        if !real.starts_with(&self.root) {
            return Err(format!("{path} resolves outside {}", self.root.display()));
        }
        Ok(real)
    }
}

impl Files for HostFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        std::fs::read(self.locate(path)?).map_err(|e| e.to_string())
    }
    fn list_dir(&self, path: &str) -> Result<Vec<Entry>, String> {
        let dir = if path.is_empty() {
            self.root.clone()
        } else {
            self.locate(path)?
        };
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            // A symlink is neither followed nor seeded: what it points at is not
            // part of the blueprint, and following one makes a world depend on
            // the shape of the checkout rather than on its contents.
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                return Err(format!("{path}/{name} is a symbolic link"));
            }
            entries.push(Entry {
                name,
                directory: kind.is_dir(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }
    fn exists(&self, path: &str) -> bool {
        self.locate(path).is_ok()
    }
    fn is_dir(&self, path: &str) -> bool {
        self.locate(path).map(|p| p.is_dir()).unwrap_or(false)
    }
}
