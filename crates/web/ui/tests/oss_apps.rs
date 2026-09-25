//! Real open-source React apps (docs/oss-webapps.md) on cw-ui against their own
//! React builds on the Realm: the same served files, the same page, the same
//! scripted session (the flows of crates/computerworld/tests/oss_webapps.rs), and
//! after boot and every step the document must be the same, node for node.
//!
//! The compiled app is `app.ui.json` beside the page, built by cw-tsx from the
//! app's sources at the pinned commit with its own node_modules
//! (scripts/oss-web/build.sh); its packages run on the island.

use std::path::{Path, PathBuf};
use std::time::Instant;

use cw_ui::UiApp;
use cw_web::dom::{Document, NodeId, NodeKind};
use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};

fn packages() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../services/oss-web/packages")
}

/// Every text file under `dir`, served at `base` + its relative path.
fn serve(dir: &Path, base: &str) -> MemoryHost {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let r = format!("{rel}{name}");
            if p.is_dir() {
                walk(&p, &format!("{r}/"), out);
            } else {
                out.push((r, p));
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, "", &mut files);
    let mut h = MemoryHost::new();
    for (rel, p) in files {
        let Ok(body) = std::fs::read_to_string(&p) else {
            continue;
        };
        let ty = match p.extension().and_then(|e| e.to_str()) {
            Some("css") => "text/css",
            Some("json") => "application/json",
            Some("html") => "text/html",
            _ => "text/javascript",
        };
        h = h.with_response(&format!("{base}{rel}"), ty, &body);
    }
    h
}

#[derive(Clone, Debug)]
enum Step {
    /// Clicks the element a selector finds (its centre).
    Click(&'static str),
    /// Clicks the `n`th element (from 1) a selector finds.
    ClickNth(&'static str, usize),
    /// Types into the focused control, key by key.
    Type(&'static str),
    Key(&'static str),
}

fn events(step: &Step, at: Option<(i32, i32)>) -> Vec<UiEvent> {
    let modifiers = Modifiers::default();
    match step {
        Step::Click(_) | Step::ClickNth(..) => {
            let (x, y) = at.unwrap();
            vec![
                UiEvent::PointerMove { x, y, modifiers },
                UiEvent::Click {
                    x,
                    y,
                    button: 0,
                    modifiers,
                    detail: 1,
                },
            ]
        }
        Step::Type(t) => vec![UiEvent::TypeText { text: (*t).into() }],
        Step::Key(k) => vec![UiEvent::Key {
            key: (*k).into(),
            code: String::new(),
            modifiers,
            repeat: false,
        }],
    }
}

fn selector(step: &Step) -> Option<(&'static str, usize)> {
    match step {
        Step::Click(s) => Some((s, 1)),
        Step::ClickNth(s, n) => Some((s, *n)),
        _ => None,
    }
}

/// The body's subtree: tags, attributes, text, form values and checkedness, and
/// where focus is.
fn dom_text(
    doc: &Document,
    values: &std::collections::BTreeMap<NodeId, String>,
    checked: &dyn Fn(NodeId) -> bool,
    focused: Option<NodeId>,
) -> String {
    fn walk(
        doc: &Document,
        n: NodeId,
        values: &std::collections::BTreeMap<NodeId, String>,
        checked: &dyn Fn(NodeId) -> bool,
        focused: Option<NodeId>,
        depth: usize,
        out: &mut String,
    ) {
        match doc.kind(n) {
            NodeKind::Element { tag, attrs, .. } => {
                if tag == "script" {
                    return;
                }
                out.push_str(&"  ".repeat(depth));
                out.push('<');
                out.push_str(tag);
                for a in attrs {
                    out.push_str(&format!(" {}={:?}", a.name, a.value));
                }
                out.push('>');
                if matches!(tag.as_str(), "input" | "textarea" | "select") {
                    out.push_str(&format!(
                        " value={:?} checked={}",
                        values.get(&n).cloned().unwrap_or_default(),
                        checked(n)
                    ));
                }
                if focused == Some(n) {
                    out.push_str(" [focused]");
                }
                out.push('\n');
                for c in doc.children(n) {
                    walk(doc, c, values, checked, focused, depth + 1, out);
                }
            }
            NodeKind::Text(t) => {
                let t = t.trim();
                if !t.is_empty() {
                    out.push_str(&"  ".repeat(depth));
                    out.push_str(&format!("{t:?}\n"));
                }
            }
            _ => {}
        }
    }
    let mut out = String::new();
    if let Some(body) = doc.body() {
        walk(doc, body, values, checked, focused, 0, &mut out);
    }
    out
}

fn first_difference(a: &str, b: &str) -> String {
    if std::env::var_os("CW_OSS_ALL").is_some() {
        let mut out = String::new();
        for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
            if x != y {
                out.push_str(&format!(
                    "line {}:\n  compiled: {x}\n  React:    {y}\n",
                    i + 1
                ));
            }
        }
        return out;
    }
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {}:\n  compiled: {x}\n  React:    {y}", i + 1);
        }
    }
    format!(
        "lengths differ: {} vs {} lines",
        a.lines().count(),
        b.lines().count()
    )
}

struct App {
    /// Where the package is served, and the page's path under it.
    base: &'static str,
    page: &'static str,
    /// The package's public directory.
    public: PathBuf,
    ir: PathBuf,
    /// World time the page may take to settle after boot and after each step.
    settle_ms: u32,
}

fn compare(app: &App, steps: &[Step]) {
    let html = std::fs::read_to_string(app.public.join(app.page)).unwrap();
    let url = format!("{}{}", app.base, app.page);
    let ir = std::fs::read_to_string(&app.ir).unwrap_or_else(|_| {
        panic!(
            "{}: no compiled app (scripts/oss-web/build.sh builds it)",
            app.ir.display()
        )
    });
    let module = UiApp::parse_ir(&ir).unwrap();

    let t = Instant::now();
    let mut r = Realm::new(&html, &url, Box::new(serve(&app.public, app.base)));
    r.run_document();
    r.run_until_idle(app.settle_ms);
    let react_boot = t.elapsed();
    let t = Instant::now();
    let mut ui = UiApp::new(module, &html, &url, Box::new(serve(&app.public, app.base))).unwrap();
    ui.boot();
    ui.run_until_idle(app.settle_ms);
    let ui_boot = t.elapsed();

    let snap = |r: &mut Realm, ui: &mut UiApp| -> (String, String) {
        let a = {
            let values = ui.form_values();
            let focused = ui.focused();
            let doc = ui.document().clone();
            let inner = ui.inner();
            let checked: Vec<NodeId> = doc
                .descendants(Document::ROOT)
                .filter(|n| inner.is_checked(*n))
                .collect();
            dom_text(&doc, &values, &|n| checked.contains(&n), focused)
        };
        let b = {
            let values = r.form_values();
            let focused = r.focused();
            let doc = r.document().clone();
            let inner = r.layout();
            let checked: Vec<NodeId> = doc
                .descendants(Document::ROOT)
                .filter(|n| inner.is_checked(*n))
                .collect();
            drop(inner);
            dom_text(&doc, &values, &|n| checked.contains(&n), focused)
        };
        (a, b)
    };
    let mut failures = Vec::new();
    let (a, b) = snap(&mut r, &mut ui);
    if std::env::var_os("CW_OSS_SHOW").is_some() {
        eprintln!(
            "compiled logs: {:?}\ncompiled:\n{a}\nReact logs: {:?}\nReact:\n{b}",
            ui.logs(),
            r.logs()
        );
    }
    if a != b {
        failures.push(format!("boot: {}", first_difference(&a, &b)));
    }
    let (mut react_steps, mut ui_steps) = (Vec::new(), Vec::new());
    for (i, s) in steps.iter().enumerate() {
        let at_r = selector(s).map(|(sel, n)| {
            let src = format!(
                "(() => {{ const q = document.querySelectorAll({sel:?})[{}].getBoundingClientRect(); return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()",
                n - 1
            );
            let v = r.eval(&src).unwrap_or_else(|e| panic!("{sel}: {e}"));
            let mut it = v.split(',').map(|x| x.parse::<f64>().unwrap() as i32);
            (it.next().unwrap(), it.next().unwrap())
        });
        let t = Instant::now();
        for ev in events(s, at_r) {
            r.dispatch(ev);
        }
        r.run_until_idle(app.settle_ms);
        react_steps.push(t.elapsed());
        let at_u = selector(s).map(|(sel, n)| {
            let nodes = ui.query_selector_all(sel);
            let node = *nodes
                .get(n - 1)
                .unwrap_or_else(|| panic!("step {i} {s:?}: no {sel} #{n} compiled"));
            ui.centre_of(node).expect("laid out")
        });
        let t = Instant::now();
        for ev in events(s, at_u) {
            ui.dispatch(ev);
        }
        ui.run_until_idle(app.settle_ms);
        ui_steps.push(t.elapsed());
        let (a, b) = snap(&mut r, &mut ui);
        if a != b {
            failures.push(format!(
                "step {} {s:?}: {}",
                i + 1,
                first_difference(&a, &b)
            ));
        }
    }
    let ms = |d: std::time::Duration| (d.as_secs_f64() * 1000.0 * 100.0).round() / 100.0;
    eprintln!(
        "{url}: boot compiled {:.2} ms, React {:.2} ms; steps compiled {:?} ms, React {:?} ms",
        ms(ui_boot),
        ms(react_boot),
        ui_steps.iter().map(|d| ms(*d)).collect::<Vec<_>>(),
        react_steps.iter().map(|d| ms(*d)).collect::<Vec<_>>(),
    );
    let errors: Vec<_> = ui
        .logs()
        .into_iter()
        .filter(|l| format!("{:?}", l.level).contains("Error"))
        .collect();
    assert!(errors.is_empty(), "{url}: compiled logged {errors:?}");
    assert!(failures.is_empty(), "{url}:\n{}", failures.join("\n\n"));
}

/// TodoMVC's React example through the oss_webapps flow: add three todos,
/// complete one, filter by the hash routes, clear the completed one.
#[test]
fn todomvc_react_matches_its_react_build() {
    let public = packages().join("todomvc/public");
    let ir = std::env::var_os("CW_OSS_TODOMVC_IR")
        .map(PathBuf::from)
        .unwrap_or_else(|| public.join("examples/react/dist/app.ui.json"));
    let app = App {
        base: "http://todomvc.com/",
        page: "examples/react/dist/index.html",
        public,
        ir,
        settle_ms: 50,
    };
    let mut steps = vec![Step::Click(".new-todo")];
    for todo in [
        "Review the CI caching PR",
        "Reply to Wren",
        "Book the offsite room",
    ] {
        steps.push(Step::Type(todo));
        steps.push(Step::Key("Enter"));
    }
    steps.extend([
        Step::ClickNth(".todo-list .toggle", 1),
        Step::Click("a[href='#/active']"),
        Step::Click("a[href='#/completed']"),
        Step::Click("a[href='#/']"),
        Step::Click(".clear-completed"),
    ]);
    compare(&app, &steps);
}

/// Where TodoMVC React's compiled boot goes: parsing the IR, the island's start
/// (its React shim and packages), the first render. Release, medians of 15.
///
///     cargo test --release -p cw-ui --test oss_apps -- --ignored --nocapture boot_phases
#[test]
#[ignore]
fn boot_phases() {
    let public = packages().join("todomvc/public");
    let html = std::fs::read_to_string(public.join("examples/react/dist/index.html")).unwrap();
    let ir = std::fs::read_to_string(public.join("examples/react/dist/app.ui.json")).unwrap();
    let url = "http://todomvc.com/examples/react/dist/index.html";
    let (mut parse, mut new, mut boot, mut snap, mut json) =
        (vec![], vec![], vec![], vec![], vec![]);
    let mut size = 0;
    for _ in 0..15 {
        let host = serve(&public, "http://todomvc.com/");
        let t = Instant::now();
        let module = UiApp::parse_ir(&ir).unwrap();
        parse.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let mut app = UiApp::new(module, &html, url, Box::new(host)).unwrap();
        new.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        app.boot();
        boot.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let state = app.snapshot();
        snap.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        size = state.to_json().len();
        json.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let median = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    eprintln!(
        "IR {} KB: parse {:.2} ms, new {:.2} ms, boot {:.2} ms, snapshot {:.2} ms, its JSON {:.2} ms ({} KB)",
        ir.len() / 1024,
        median(parse),
        median(new),
        median(boot),
        median(snap),
        median(json),
        size / 1024
    );
}

/// react-admin's simple example (frontend only: its fake REST provider is in the
/// bundle): the compiled app against its React build. It builds (cw-tsx, with
/// the monorepo's package aliases, as its Vite config has them), but does not run:
/// its libraries work the DOM directly (ProseMirror in the rich-text editor, MUI's
/// Popper), and the island has only the page APIs cw-ui models, not a browser's
/// DOM. Run with `CW_OSS_REACT_ADMIN_IR` naming an IR built from the checkout.
#[test]
#[ignore]
fn react_admin_matches_its_react_build() {
    let public = packages().join("react-admin/public");
    let ir = std::env::var_os("CW_OSS_REACT_ADMIN_IR")
        .map(PathBuf::from)
        .unwrap_or_else(|| public.join("app.ui.json"));
    let app = App {
        base: "http://react-admin.marmelab.com/",
        page: "index.html",
        public,
        ir,
        settle_ms: 2000,
    };
    compare(&app, &[]);
}
