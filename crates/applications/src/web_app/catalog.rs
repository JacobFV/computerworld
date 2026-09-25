//! The web applications this process can run, by kind. Application code is supplied
//! by the embedding runtime and never read from a snapshot, as for
//! `cw_sdk::Registry`: a snapshot names the kind and version it needs and the host
//! looks the code up here. The built-in applications are always present; a world's
//! own are added by `Environment::register_web_application`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use cw_sdk::{WebApplication, WebSource};

/// One entry: the definition and its kind as a `&'static str`, which is how every
/// native application names its kind.
#[derive(Clone)]
pub struct Entry {
    pub kind: &'static str,
    pub app: Arc<WebApplication>,
}

/// The built-in applications whose IR is also built in as Rust (`cw-tsx build
/// --emit rust`, checked in beside the IR by `crates/applications/web/build.mjs`),
/// each with the IR it was generated from.
mod generated {
    include!("../../web/notes/notes.ui.rs");

    pub static PROGRAMS: [(&cw_ui::GenProgram, &str); 1] = [(
        &notes::PROGRAM,
        include_str!("../../web/notes/notes.ui.json"),
    )];
}

/// The generated program of the IR `ir`, when one is built in: cw-ui runs it
/// instead of interpreting the IR. Only the very IR it was generated from selects
/// it, so a world's own application never runs a built-in's code.
pub fn generated_program(ir: &str) -> Option<&'static cw_ui::GenProgram> {
    generated::PROGRAMS
        .iter()
        .find(|(_, source)| *source == ir)
        .map(|(p, _)| *p)
}

fn catalog() -> &'static Mutex<BTreeMap<String, Entry>> {
    static CATALOG: OnceLock<Mutex<BTreeMap<String, Entry>>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        // A snapshot of a generated app names its program: make them findable.
        for (p, _) in generated::PROGRAMS {
            cw_ui::program::register(p);
        }
        let mut map = BTreeMap::new();
        for app in builtin() {
            let kind: &'static str = Box::leak(app.kind.clone().into_boxed_str());
            map.insert(
                app.kind.clone(),
                Entry {
                    kind,
                    app: Arc::new(app),
                },
            );
        }
        Mutex::new(map)
    })
}

/// The applications that ship as web apps.
fn builtin() -> Vec<WebApplication> {
    vec![WebApplication {
        kind: "notes".into(),
        version: 1,
        titles: [
            ("windows", "Sticky Notes"),
            ("android", "Keep"),
            ("*", "Notes"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect(),
        source: WebSource::Compiled {
            ir: include_str!("../../web/notes/notes.ui.json").into(),
            script: include_str!("../../web/notes/notes.js").into(),
            style: include_str!("../../web/notes/notes.css").into(),
        },
    }]
}

/// Adds `app`. Defining the same application twice is harmless; a different
/// application under a kind already defined is refused, since a snapshot names only
/// the kind and version and could not tell the two apart.
pub fn define(app: WebApplication) -> Result<(), String> {
    let mut map = match catalog().lock() {
        Ok(m) => m,
        Err(p) => p.into_inner(),
    };
    if let Some(existing) = map.get(&app.kind) {
        if *existing.app == app {
            return Ok(());
        }
        return Err(format!(
            "a different web application is already defined as {}",
            app.kind
        ));
    }
    let kind: &'static str = Box::leak(app.kind.clone().into_boxed_str());
    map.insert(
        app.kind.clone(),
        Entry {
            kind,
            app: Arc::new(app),
        },
    );
    Ok(())
}

/// The application defined as `kind`.
pub fn get(kind: &str) -> Option<Entry> {
    let map = match catalog().lock() {
        Ok(m) => m,
        Err(p) => p.into_inner(),
    };
    map.get(kind).cloned()
}

#[cfg(test)]
mod generated_tests {
    /// Each generated program is the program of the IR it is paired with (the IR
    /// is checked in as its canonical JSON and a newline, what the hash covers).
    #[test]
    fn generated_programs_are_of_their_ir() {
        for (p, ir) in super::generated::PROGRAMS {
            assert_eq!(
                cw_ui::program::fnv1a(ir.trim_end().as_bytes()),
                p.hash,
                "{} is stale: run node crates/applications/web/build.mjs",
                p.name
            );
            let module = cw_ui::UiApp::parse_ir(ir).unwrap();
            assert_eq!(cw_ui::program::ir_hash(&module), p.hash);
        }
    }

    /// Notes as the catalog defines it runs its generated program.
    #[test]
    fn notes_boots_its_generated_program() {
        let entry = super::get("notes").unwrap();
        let cw_sdk::WebSource::Compiled { ir, .. } = &entry.app.source else {
            panic!("notes is compiled");
        };
        let p = super::generated_program(ir).expect("notes has generated code");
        assert!(std::ptr::eq(p, super::generated::PROGRAMS[0].0));
        assert!(super::generated_program(&format!("{ir} ")).is_none());
    }
}
