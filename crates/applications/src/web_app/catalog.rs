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

fn catalog() -> &'static Mutex<BTreeMap<String, Entry>> {
    static CATALOG: OnceLock<Mutex<BTreeMap<String, Entry>>> = OnceLock::new();
    CATALOG.get_or_init(|| {
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
