#![allow(unexpected_cfgs)]
//! Every TSX fixture in `crates/web/engine/tests/framework-parity/tsx-*.tsx`, checked
//! three ways over the same `<name>.steps.json`:
//!
//! 1. compiled: `cw-tsx` lowers the module to the UI IR and `cw-ui` runs it natively;
//! 2. fallback: the JS `cw-tsx` emits from the same source runs on React 18's
//!    production build on the engine's JS `Realm`;
//! 3. Chromium: the same fallback JS in real Chrome, dumped by
//!    `scripts/web-parity/dump.mjs` into `<name>.<state>.chromium.json`.
//!
//! The compiled and fallback documents must be identical node for node (tags,
//! attributes, text, form values, focus) after every state, and the compiled app's
//! layout must reach the state's entry in `thresholds.json` against Chromium. The
//! checked-in `<name>.js` and `<name>.ui.json` must be what `cw-tsx` builds today.
//!
//!     cargo test -p cw-ui --test tsx_parity -- --nocapture
//!     CW_TSX_BLESS=1 cargo test -p cw-ui --test tsx_parity   # rewrite <name>.js/.ui.json

#[path = "../../engine/tests/support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use cw_ui::UiApp;
use cw_web::dom::{Document, NodeId, NodeKind};
use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};
use serde_json::Value;
use support::*;

const BASE: &str = "https://example.test/";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/framework-parity")
}

fn tsx_fixtures() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(fixture_dir())
        .expect("framework-parity")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.strip_suffix(".tsx")
                .filter(|s| s.starts_with("tsx-"))
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

fn steps(name: &str) -> Vec<(String, Vec<Value>)> {
    let p = fixture_dir().join(format!("{name}.steps.json"));
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let v: serde_json::Map<String, Value> = serde_json::from_str(&text).expect("steps parse");
    v.into_iter()
        .map(|(s, l)| (s, l.as_array().expect("step list").clone()))
        .collect()
}

fn thresholds() -> BTreeMap<String, f64> {
    let p = fixture_dir().join("thresholds.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("thresholds"))
        .expect("thresholds parse")
}

/// `tests/vendor/*` under `/vendor/`, and the fixture directory at the root, as
/// `dump.mjs` serves them to Chromium.
fn host() -> MemoryHost {
    let mut h = MemoryHost::new();
    let vendor = fixture_dir().join("../vendor");
    let serve = |dir: PathBuf, prefix: &str, h: MemoryHost| -> MemoryHost {
        let mut h = h;
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let Ok(body) = std::fs::read_to_string(&p) else {
                continue;
            };
            let ty = if name.ends_with(".css") {
                "text/css"
            } else if name.ends_with(".json") {
                "application/json"
            } else {
                "text/javascript"
            };
            h = h.with_response(&format!("{BASE}{prefix}{name}"), ty, &body);
        }
        h
    };
    h = serve(vendor, "vendor/", h);
    h = serve(fixture_dir(), "", h);
    h
}

struct Built {
    module: cw_ui::ir::Module,
    js: String,
}

/// Compiles the fixture and checks (or with `CW_TSX_BLESS`, rewrites) the checked-in
/// outputs.
fn build(name: &str) -> Built {
    let src = std::fs::read_to_string(fixture_dir().join(format!("{name}.tsx"))).expect("tsx");
    let b = cw_tsx::build(&src, &format!("{name}.tsx"));
    assert!(
        b.diagnostics.is_empty(),
        "{name}.tsx is outside the compiled subset:\n{}",
        b.diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
    let module = b.ir.expect("ir");
    let js = b.js.expect("js");
    let ir_text = serde_json::to_string(&module).unwrap() + "\n";
    let js_path = fixture_dir().join(format!("{name}.js"));
    let ir_path = fixture_dir().join(format!("{name}.ui.json"));
    if std::env::var_os("CW_TSX_BLESS").is_some() {
        std::fs::write(&js_path, &js).unwrap();
        std::fs::write(&ir_path, &ir_text).unwrap();
    } else {
        let on_disk_js = std::fs::read_to_string(&js_path).unwrap_or_default();
        let on_disk_ir = std::fs::read_to_string(&ir_path).unwrap_or_default();
        assert!(
            on_disk_js == js && on_disk_ir == ir_text,
            "{name}.js / {name}.ui.json are stale: rerun with CW_TSX_BLESS=1"
        );
    }
    Built { module, js }
}

// ---------------------------------------------------------------- the two runtimes

fn click_step(step: &Value) -> Option<&str> {
    (step["action"] == "click").then(|| step["selector"].as_str().unwrap())
}

fn ui_event(step: &Value, at: Option<(i32, i32)>) -> Vec<UiEvent> {
    let modifiers = Modifiers::default();
    match step["action"].as_str().unwrap() {
        "click" => {
            let (x, y) = at.expect("click target");
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
        "type" => vec![UiEvent::TypeText {
            text: step["text"].as_str().unwrap().into(),
        }],
        "press" => vec![UiEvent::Key {
            key: step["key"].as_str().unwrap().into(),
            code: String::new(),
            modifiers,
            repeat: false,
        }],
        other => panic!("unknown action {other:?}"),
    }
}

struct Timing {
    boot: Duration,
    steps: Vec<Duration>,
}

fn run_compiled(
    module: &cw_ui::ir::Module,
    html: &str,
    name: &str,
    list: &[Value],
) -> (UiApp, Timing) {
    let t = Instant::now();
    let mut app = UiApp::new(
        module.clone(),
        html,
        &format!("{BASE}{name}.html"),
        Box::new(host()),
    )
    .expect("app");
    app.boot();
    let boot = t.elapsed();
    let mut steps = Vec::new();
    for step in list {
        let at = click_step(step).map(|sel| {
            let n = app
                .query_selector(sel)
                .unwrap_or_else(|| panic!("{sel}: no element"));
            app.centre_of(n).expect("laid out")
        });
        let t = Instant::now();
        for ev in ui_event(step, at) {
            app.dispatch(ev);
        }
        app.run_until_idle(20);
        steps.push(t.elapsed());
    }
    let errors: Vec<_> = app
        .logs()
        .into_iter()
        .filter(|l| format!("{:?}", l.level).contains("Error"))
        .collect();
    assert!(
        errors.is_empty(),
        "{name}: the compiled app logged errors: {errors:?}"
    );
    (app, Timing { boot, steps })
}

fn realm_centre(r: &mut Realm, selector: &str) -> (i32, i32) {
    let src = format!(
        "(() => {{ const q = document.querySelector({selector:?}).getBoundingClientRect(); \
         return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()"
    );
    let v = r.eval(&src).unwrap_or_else(|e| panic!("{selector}: {e}"));
    let mut it = v.split(',').map(|n| n.parse::<f64>().unwrap() as i32);
    (it.next().unwrap(), it.next().unwrap())
}

fn run_fallback(html: &str, name: &str, list: &[Value]) -> (Realm, Timing) {
    let t = Instant::now();
    let mut r = Realm::new(html, &format!("{BASE}{name}.html"), Box::new(host()));
    r.run_document();
    r.run_until_idle(50);
    let boot = t.elapsed();
    let mut steps = Vec::new();
    for step in list {
        let at = click_step(step).map(|sel| realm_centre(&mut r, sel));
        let t = Instant::now();
        for ev in ui_event(step, at) {
            r.dispatch(ev);
        }
        r.run_until_idle(20);
        steps.push(t.elapsed());
    }
    let errors: Vec<_> = r
        .logs()
        .into_iter()
        .filter(|l| format!("{:?}", l.level).contains("Error"))
        .collect();
    assert!(
        errors.is_empty(),
        "{name}: the fallback logged errors: {errors:?}"
    );
    (r, Timing { boot, steps })
}

// ---------------------------------------------------------------- DOM comparison

/// The body's subtree as text: tags, attributes in order, text nodes, and each form
/// control's live value and checkedness, and where focus is.
fn dom_text(
    doc: &Document,
    values: &BTreeMap<NodeId, String>,
    checked: &dyn Fn(NodeId) -> bool,
    focused: Option<NodeId>,
) -> String {
    fn walk(
        doc: &Document,
        n: NodeId,
        depth: usize,
        values: &BTreeMap<NodeId, String>,
        checked: &dyn Fn(NodeId) -> bool,
        focused: Option<NodeId>,
        out: &mut String,
    ) {
        let pad = "  ".repeat(depth);
        match doc.kind(n) {
            NodeKind::Element { tag, attrs, .. } => {
                if tag == "script" {
                    return;
                }
                out.push_str(&pad);
                out.push('<');
                out.push_str(tag);
                for a in attrs {
                    out.push_str(&format!(" {}={:?}", a.name, a.value));
                }
                out.push('>');
                if matches!(tag.as_str(), "input" | "textarea" | "select") {
                    out.push_str(&format!(
                        " value={:?}",
                        values.get(&n).cloned().unwrap_or_default()
                    ));
                    out.push_str(&format!(" checked={}", checked(n)));
                }
                if focused == Some(n) {
                    out.push_str(" [focused]");
                }
                out.push('\n');
                for c in doc.children(n) {
                    walk(doc, c, depth + 1, values, checked, focused, out);
                }
            }
            NodeKind::Text(t) => {
                out.push_str(&pad);
                out.push_str(&format!("{t:?}\n"));
            }
            _ => {}
        }
    }
    let mut out = String::new();
    if let Some(body) = doc.body() {
        walk(doc, body, 0, values, checked, focused, &mut out);
    }
    out
}

fn compiled_dom(app: &mut UiApp) -> String {
    let values = app.form_values();
    let focused = app.focused();
    let doc = app.document().clone();
    let inner = app.inner();
    let checked: Vec<NodeId> = doc
        .descendants(Document::ROOT)
        .filter(|n| inner.is_checked(*n))
        .collect();
    dom_text(&doc, &values, &|n| checked.contains(&n), focused)
}

fn fallback_dom(r: &mut Realm) -> String {
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
}

fn first_difference(a: &str, b: &str) -> String {
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {}:\n  compiled: {x}\n  fallback: {y}", i + 1);
        }
    }
    format!(
        "lengths differ: compiled {} lines, fallback {} lines",
        a.lines().count(),
        b.lines().count()
    )
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[test]
fn compiled_and_fallback_documents_are_identical_after_every_state() {
    let mut failures = Vec::new();
    for name in tsx_fixtures() {
        let built = build(&name);
        let _ = &built.js;
        let html =
            std::fs::read_to_string(fixture_dir().join(format!("{name}.html"))).expect("html");
        for (state, list) in steps(&name) {
            let (mut app, tc) = run_compiled(&built.module, &html, &name, &list);
            let (mut realm, tf) = run_fallback(&html, &name, &list);
            let a = compiled_dom(&mut app);
            let b = fallback_dom(&mut realm);
            eprintln!(
                "{name}.{state}: compiled boot {:.2} ms, steps {:?} ms; fallback boot {:.2} ms, steps {:?} ms",
                ms(tc.boot),
                tc.steps.iter().map(|d| (ms(*d) * 100.0).round() / 100.0).collect::<Vec<_>>(),
                ms(tf.boot),
                tf.steps.iter().map(|d| (ms(*d) * 100.0).round() / 100.0).collect::<Vec<_>>(),
            );
            if a != b {
                failures.push(format!("{name}.{state}: {}", first_difference(&a, &b)));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn compiled_layout_matches_chromium() {
    let thresholds = thresholds();
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/tsx-parity");
    std::fs::create_dir_all(&out).unwrap();
    let mut failures = Vec::new();
    for name in tsx_fixtures() {
        let built = build(&name);
        let html =
            std::fs::read_to_string(fixture_dir().join(format!("{name}.html"))).expect("html");
        for (state, list) in steps(&name) {
            let key = format!("{name}.{state}");
            let dump = fixture_dir().join(format!("{key}.chromium.json"));
            let expected = read_dump(&dump);
            let (mut app, _) = run_compiled(&built.module, &html, &name, &list);
            let values = app.form_values();
            let doc = app.document().clone();
            let styles = app.styles().clone();
            let tree = app.fragment_tree().clone();
            let vp = viewport();
            let images = cw_web::paint::ImageMap::from_document(&doc, &styles);
            let mut ctx = cw_web::paint::PaintContext::new(&images);
            ctx.values = values;
            let scene = cw_web::paint::paint(&doc, &styles, &tree, vp, &ctx);
            let rendered = Rendered {
                doc,
                styles,
                tree,
                scene,
            };
            let got = engine_dump(&format!("{name}.html"), &rendered, vp);
            let report = compare(&expected, &got);
            let threshold = thresholds.get(&key).copied().unwrap_or(1.0);
            write_png(
                &out.join(format!("{key}.compiled.png")),
                &rasterise(&rendered.scene),
            );
            std::fs::write(
                out.join(format!("{key}.compiled.report.md")),
                report.to_markdown(&[], threshold),
            )
            .unwrap();
            eprintln!(
                "{key}: compiled {}/{} nodes pass ({:.1}%), threshold {threshold}",
                report.passed,
                report.total,
                report.pass_rate() * 100.0
            );
            if report.pass_rate() < threshold {
                failures.push(format!(
                    "{key}: {:.3} below {threshold}: {:?}",
                    report.pass_rate(),
                    report.worst(3)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
