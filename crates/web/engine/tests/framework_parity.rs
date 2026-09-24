//! Layout parity for pages a framework renders. Each `tests/framework-parity/<name>.html`
//! is an app drawn by an unmodified production framework build from `tests/vendor/`
//! (React 18, Vue 3); `<name>.steps.json` names states and the clicks, typing and key
//! presses that reach each one from a fresh load. Chromium was driven through the same
//! steps by `scripts/web-parity/dump.mjs --state <steps> --state-name <state>`, which
//! wrote `<name>.<state>.chromium.json`. Here the page runs on the engine's own script
//! layer (`Realm`), the steps are dispatched as UI events, and the realm's live
//! document, styles and fragment tree are dumped and compared with the parity rules in
//! `support/mod.rs`. Each state must reach its entry in `thresholds.json`.
//!
//! This is what the static parity fixtures cannot show: that the DOM a framework builds
//! and then mutates on the engine lays out as Chromium lays out the DOM the same
//! framework built there. The two fixtures render the same tracker, and Chromium lays
//! them out identically node for node, so they also check each other.
//!
//! A `tsx-*` fixture is a React app written in TSX (`<name>.tsx`); its page loads the
//! React fallback `cw-tsx` emits (`<name>.js`). Here that fallback runs on the Realm;
//! `crates/web/ui/tests/tsx_parity.rs` runs the same module compiled (`cw-ui`) and
//! requires the two documents to be identical after every state.
//!
//!     cargo test -p cw-web --features pipeline --test framework_parity -- --nocapture

mod support;

use serde_json::Value;
use std::collections::BTreeMap;
use support::*;

fn fixture_dir() -> std::path::PathBuf {
    crate_dir().join("tests/framework-parity")
}

fn fixtures() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(fixture_dir())
        .expect("tests/framework-parity")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".html").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

/// A fixture's named step lists, in the file's order.
fn steps(name: &str) -> Vec<(String, Vec<Value>)> {
    let p = fixture_dir().join(format!("{name}.steps.json"));
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let v: serde_json::Map<String, Value> =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    v.into_iter()
        .map(|(state, list)| {
            let list = list
                .as_array()
                .unwrap_or_else(|| panic!("{name}: `{state}` is not a list"))
                .clone();
            (state, list)
        })
        .collect()
}

fn framework_thresholds() -> BTreeMap<String, f64> {
    let p = fixture_dir().join("thresholds.json");
    serde_json::from_str(&std::fs::read_to_string(&p).expect("thresholds.json"))
        .expect("thresholds.json parses")
}

#[test]
fn every_state_has_a_chromium_dump_and_a_threshold() {
    let thresholds = framework_thresholds();
    let mut keys = Vec::new();
    for name in fixtures() {
        let states = steps(&name);
        assert!(!states.is_empty(), "{name}: no states");
        for (state, list) in states {
            for step in &list {
                let action = step["action"].as_str().unwrap_or("");
                assert!(
                    matches!(action, "click" | "type" | "press"),
                    "{name}.{state}: unknown action {action:?}"
                );
            }
            let key = format!("{name}.{state}");
            let dump = read_dump(&fixture_dir().join(format!("{key}.chromium.json")));
            assert_eq!(dump.engine, "chromium", "{key}: dump is not Chromium's");
            assert_eq!(
                (dump.viewport.width, dump.viewport.height),
                (WIDTH, HEIGHT),
                "{key}: dumped at another viewport"
            );
            assert_eq!(dump.properties, PROPERTIES, "{key}: property list drifted");
            let t = thresholds
                .get(&key)
                .unwrap_or_else(|| panic!("{key}: no entry in thresholds.json"));
            assert!((0.0..=1.0).contains(t), "{key}: threshold {t} out of range");
            keys.push(key);
        }
    }
    for key in thresholds.keys() {
        assert!(
            keys.contains(key),
            "thresholds.json names `{key}` but there is no such state"
        );
    }
}

#[cfg(feature = "pipeline")]
mod cases {
    use super::*;
    use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};
    use std::time::{Duration, Instant};

    const BASE: &str = "https://example.test/";

    /// Every file of `tests/vendor/` served under `/vendor/`, as `dump.mjs` serves them
    /// to Chromium, and the fixtures' own scripts (a TSX fixture's compiled fallback,
    /// `tsx-*.js`) beside the page, as Chromium loads them from the fixture's directory.
    fn host() -> MemoryHost {
        let dir = crate_dir().join("tests/vendor");
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("tests/vendor")
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        let mut h = MemoryHost::new();
        for p in entries {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let Ok(body) = std::fs::read_to_string(&p) else {
                continue;
            };
            let ty = if name.ends_with(".css") {
                "text/css"
            } else {
                "text/javascript"
            };
            h = h.with_response(&format!("{BASE}vendor/{name}"), ty, &body);
        }
        let mut scripts: Vec<_> = std::fs::read_dir(fixture_dir())
            .expect("tests/framework-parity")
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "js"))
            .collect();
        scripts.sort();
        for p in scripts {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let body = std::fs::read_to_string(&p).expect("fixture script");
            h = h.with_response(&format!("{BASE}{name}"), "text/javascript", &body);
        }
        h
    }

    /// The centre of the element `selector` matches, in viewport pixels.
    fn centre(r: &mut Realm, selector: &str) -> (i32, i32) {
        let src = format!(
            "(() => {{ const q = document.querySelector({selector:?}).getBoundingClientRect(); \
             return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()"
        );
        let v = r.eval(&src).unwrap_or_else(|e| panic!("{selector}: {e}"));
        let mut it = v.split(',').map(|n| n.parse::<f64>().unwrap() as i32);
        (it.next().unwrap(), it.next().unwrap())
    }

    /// One step as Playwright performs it: a click moves the pointer onto the
    /// element's centre and clicks there (so hit testing picks the target), typing
    /// goes to the focused element, a key press is a key press.
    fn perform(r: &mut Realm, step: &Value) {
        let modifiers = Modifiers::default();
        match step["action"].as_str().unwrap() {
            "click" => {
                let (x, y) = centre(r, step["selector"].as_str().unwrap());
                r.dispatch(UiEvent::PointerMove { x, y, modifiers });
                r.dispatch(UiEvent::Click {
                    x,
                    y,
                    button: 0,
                    modifiers,
                    detail: 1,
                });
            }
            "type" => {
                r.dispatch(UiEvent::TypeText {
                    text: step["text"].as_str().unwrap().into(),
                });
            }
            "press" => {
                r.dispatch(UiEvent::Key {
                    key: step["key"].as_str().unwrap().into(),
                    code: String::new(),
                    modifiers,
                    repeat: false,
                });
            }
            other => panic!("unknown action {other:?}"),
        }
        r.run_until_idle(20);
    }

    struct Timing {
        boot: Duration,
        steps: Duration,
        layout: Duration,
    }

    /// Loads the fixture, performs `list`, and returns the realm's live state as the
    /// parity runner's `Rendered`.
    fn render(name: &str, html: &str, list: &[Value]) -> (Rendered, Timing) {
        let t = Instant::now();
        // Playwright launches Chromium with `--hide-scrollbars`, so a scroll container
        // there gives no space to its bars. A realm has no host setting for overlay
        // scrollbars, so the page's elements get `scrollbar-width: none`, which takes
        // the same (none); the rule sits in <head>, which neither dump includes.
        let html = html.replacen(
            "<head>",
            "<head><style>* { scrollbar-width: none }</style>",
            1,
        );
        let mut r = Realm::new(&html, &format!("{BASE}{name}.html"), Box::new(host()));
        r.run_document();
        r.run_until_idle(50);
        let boot = t.elapsed();
        let t = Instant::now();
        for step in list {
            perform(&mut r, step);
        }
        let steps = t.elapsed();
        let errors: Vec<_> = r
            .logs()
            .into_iter()
            .filter(|l| format!("{:?}", l.level).contains("Error"))
            .collect();
        assert!(
            errors.is_empty(),
            "{name}: the page logged errors: {errors:?}"
        );
        let t = Instant::now();
        let tree = r.fragment_tree().clone();
        let layout = t.elapsed();
        let styles = r.styles().clone();
        let doc = r.document().clone();
        let vp = viewport();
        let images = cw_web::paint::ImageMap::from_document(&doc, &styles);
        let scene = cw_web::paint::paint(
            &doc,
            &styles,
            &tree,
            vp,
            &cw_web::paint::PaintContext::new(&images),
        );
        (
            Rendered {
                doc,
                styles,
                tree,
                scene,
            },
            Timing {
                boot,
                steps,
                layout,
            },
        )
    }

    fn run_fixture(name: &str) {
        let html =
            std::fs::read_to_string(fixture_dir().join(format!("{name}.html"))).expect("fixture");
        let thresholds = framework_thresholds();
        let out = out_dir();
        let mut failures = Vec::new();
        for (state, list) in steps(name) {
            let key = format!("{name}.{state}");
            let expected = read_dump(&fixture_dir().join(format!("{key}.chromium.json")));
            let (rendered, timing) = render(name, &html, &list);
            let got = engine_dump(&format!("{name}.html"), &rendered, viewport());
            std::fs::write(
                out.join(format!("{key}.engine.json")),
                serde_json::to_string_pretty(&got).unwrap(),
            )
            .expect("write engine dump");
            write_png(
                &out.join(format!("{key}.engine.png")),
                &rasterise(&rendered.scene),
            );
            let report = compare(&expected, &got);
            let threshold = thresholds.get(&key).copied().unwrap_or(0.0);
            std::fs::write(
                out.join(format!("{key}.report.md")),
                report.to_markdown(&[], threshold),
            )
            .expect("write report");
            eprintln!(
                "{key}: {}/{} nodes pass ({:.1}%), {} missing; boot {:.1} ms, {} steps {:.1} ms, layout {:.1} ms",
                report.passed,
                report.total,
                report.pass_rate() * 100.0,
                report.missing,
                timing.boot.as_secs_f64() * 1000.0,
                list.len(),
                timing.steps.as_secs_f64() * 1000.0,
                timing.layout.as_secs_f64() * 1000.0,
            );
            if report.pass_rate() < threshold {
                failures.push(format!(
                    "{key}: {:.3} below {threshold} (target-parity/{key}.report.md): {:?}",
                    report.pass_rate(),
                    report.worst(3)
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn react18_tasks() {
        run_fixture("react18-tasks");
    }

    #[test]
    fn vue3_tasks() {
        run_fixture("vue3-tasks");
    }

    /// The tracker written in TSX, here as its compiled React fallback (`tsx-tasks.js`)
    /// on the Realm. The compiled app itself, and the check that both render the same
    /// document, are in `crates/web/ui/tests/tsx_parity.rs`.
    #[test]
    fn tsx_tasks() {
        run_fixture("tsx-tasks");
    }

    #[test]
    fn app_analytics() {
        run_fixture("app-analytics");
    }

    #[test]
    fn app_chat() {
        run_fixture("app-chat");
    }

    #[test]
    fn app_datatable() {
        run_fixture("app-datatable");
    }

    #[test]
    fn app_kanban() {
        run_fixture("app-kanban");
    }

    #[test]
    fn app_settings() {
        run_fixture("app-settings");
    }

    #[test]
    fn app_shop() {
        run_fixture("app-shop");
    }
}
