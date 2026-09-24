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
    settle(&mut r, 50);
    let boot = t.elapsed();
    let mut steps = Vec::new();
    for step in list {
        let at = click_step(step).map(|sel| realm_centre(&mut r, sel));
        let t = Instant::now();
        for ev in ui_event(step, at) {
            r.dispatch(ev);
        }
        settle(&mut r, 20);
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
            NodeKind::Element { tag, attrs, ns } => {
                if tag == "script" {
                    return;
                }
                out.push_str(&pad);
                out.push('<');
                if *ns != cw_web::dom::Namespace::Html {
                    out.push_str(&format!("{ns:?}:"));
                }
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

/// The page as the parity harness shows it: Playwright launches Chromium with
/// `--hide-scrollbars`, so scroll containers there reserve no space for bars; the
/// engine's pages get `scrollbar-width: none` to match (as framework_parity.rs does).
fn as_dumped(html: &str) -> String {
    html.replacen(
        "<head>",
        "<head><style>* { scrollbar-width: none }</style>",
        1,
    )
}

/// Runs a realm's event loop until a pass runs nothing (React's scheduler posts
/// itself tasks; framework_parity.rs settles the same way).
fn settle(r: &mut Realm, advance_ms: u32) {
    for _ in 0..16 {
        if !r.run_until_idle(advance_ms) {
            break;
        }
    }
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
        let html = as_dumped(
            &std::fs::read_to_string(fixture_dir().join(format!("{name}.html"))).expect("html"),
        );
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
        let html = as_dumped(
            &std::fs::read_to_string(fixture_dir().join(format!("{name}.html"))).expect("html"),
        );
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

// ---------------------------------------------------------------- agent-written apps

/// The React apps in `framework-parity/app-src/<name>/` (TSX, several modules, as a
/// coding agent writes them), whose pages `app-<name>.html` run the agent's own
/// esbuild bundle of them. Each is compiled here from its `main.tsx`.
fn agent_apps() -> Vec<String> {
    let dir = fixture_dir().join("app-src");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("app-src")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("main.tsx").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn build_agent_app(name: &str) -> (cw_ui::ir::Module, String) {
    let root = fixture_dir().join("app-src").join(name);
    let mut read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let sources = cw_tsx::load("main.tsx", &mut read).unwrap_or_else(|d| panic!("{name}: {d:?}"));
    let b = cw_tsx::build_modules(&sources);
    assert!(
        b.diagnostics.is_empty(),
        "app-src/{name} is outside the compiled subset:\n{}",
        b.diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
    (b.ir.unwrap(), b.js.unwrap())
}

/// Runs a page on the Realm (whatever script it loads) through `list`.
fn run_realm_page(html: &str, name: &str, list: &[Value], host: MemoryHost) -> Realm {
    let mut r = Realm::new(html, &format!("{BASE}{name}.html"), Box::new(host));
    r.run_document();
    settle(&mut r, 50);
    for step in list {
        let at = click_step(step).map(|sel| realm_centre(&mut r, sel));
        for ev in ui_event(step, at) {
            r.dispatch(ev);
        }
        settle(&mut r, 20);
    }
    let errors: Vec<_> = r
        .logs()
        .into_iter()
        .filter(|l| format!("{:?}", l.level).contains("Error"))
        .collect();
    assert!(
        errors.is_empty(),
        "{name}: the page logged errors: {errors:?}"
    );
    r
}

#[test]
fn agent_apps_compiled_match_react_and_chromium() {
    let thresholds = thresholds();
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/tsx-parity");
    std::fs::create_dir_all(&out).unwrap();
    let mut failures = Vec::new();
    for app in agent_apps() {
        let fixture = format!("app-{app}");
        let html_path = fixture_dir().join(format!("{fixture}.html"));
        let Ok(html) = std::fs::read_to_string(&html_path) else {
            continue;
        };
        let html = as_dumped(&html);
        let (module, js) = build_agent_app(&app);
        // The same page, loading cw-tsx's bundle of the sources instead of esbuild's.
        let bundle = format!("/vendor/{fixture}.js");
        assert!(
            html.contains(&bundle),
            "{fixture}.html no longer loads {bundle}"
        );
        let ours = html.replace(&bundle, "/cw-tsx/app.js");
        for (state, list) in steps(&fixture) {
            let key = format!("{fixture}.{state}");
            let (mut app_ui, t) = run_compiled(&module, &html, &fixture, &list);
            let mut react = run_realm_page(&html, &fixture, &list, host());
            let mut fallback = run_realm_page(
                &ours,
                &fixture,
                &list,
                host().with_response(&format!("{BASE}cw-tsx/app.js"), "text/javascript", &js),
            );
            let a = compiled_dom(&mut app_ui);
            let b = fallback_dom(&mut react);
            let c = fallback_dom(&mut fallback);
            if a != b {
                failures.push(format!(
                    "{key}: compiled vs React: {}",
                    first_difference(&a, &b)
                ));
            }
            if c != b {
                failures.push(format!(
                    "{key}: cw-tsx's bundle vs esbuild's: {}",
                    first_difference(&c, &b)
                ));
            }
            // Layout against Chromium's dump of the page.
            let expected = read_dump(&fixture_dir().join(format!("{key}.chromium.json")));
            let values = app_ui.form_values();
            let doc = app_ui.document().clone();
            let styles = app_ui.styles().clone();
            let tree = app_ui.fragment_tree().clone();
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
            let got = engine_dump(&format!("{fixture}.html"), &rendered, vp);
            let report = compare(&expected, &got);
            let threshold = thresholds.get(&key).copied().unwrap_or(1.0);
            write_png(
                &out.join(format!("{key}.compiled.png")),
                &rasterise(&rendered.scene),
            );
            eprintln!(
                "{key}: compiled {}/{} nodes pass ({:.1}%), threshold {threshold}; boot {:.2} ms, steps {:?} ms",
                report.passed,
                report.total,
                report.pass_rate() * 100.0,
                ms(t.boot),
                t.steps.iter().map(|d| (ms(*d) * 100.0).round() / 100.0).collect::<Vec<_>>(),
            );
            if report.pass_rate() < threshold {
                // Where the compiled page's layout parts from React's on the same
                // engine: the first place to look.
                let react_render = {
                    let doc = react.document().clone();
                    let styles = react.styles().clone();
                    let tree = react.fragment_tree().clone();
                    let images = cw_web::paint::ImageMap::from_document(&doc, &styles);
                    let scene = cw_web::paint::paint(
                        &doc,
                        &styles,
                        &tree,
                        vp,
                        &cw_web::paint::PaintContext::new(&images),
                    );
                    Rendered {
                        doc,
                        styles,
                        tree,
                        scene,
                    }
                };
                let theirs = engine_dump(&format!("{fixture}.html"), &react_render, vp);
                let vs_react = compare(&theirs, &got);
                let react_vs_chromium = compare(&expected, &theirs);
                eprintln!(
                    "{key}: React on the Realm here passes {:.3} against Chromium",
                    react_vs_chromium.pass_rate()
                );
                failures.push(format!(
                    "{key}: layout {:.3} below {threshold}: {:?}\n  {key} against React on the Realm {:.3}: {:?}",
                    report.pass_rate(),
                    report.worst(3),
                    vs_react.pass_rate(),
                    vs_react.worst(3)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
