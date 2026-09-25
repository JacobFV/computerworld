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
//! An `oss-*` fixture with a `<name>.site.json` instead of a page is a real app served
//! as `worlds/oss-web` serves it: its package's `public/` files at the site's URL and
//! its API's answers from `<name>.api.json`, recorded from the world's own services
//! (crates/computerworld/tests/oss_parity_record.rs). Chromium's dumps come from the
//! same two through `dump.mjs <name>.site.json`.
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
            name.strip_suffix(".html")
                .or_else(|| name.strip_suffix(".site.json"))
                .map(str::to_owned)
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
                    matches!(action, "click" | "type" | "press" | "focus"),
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
    use cw_web::script::{FetchResponse, MemoryHost, Modifiers, Realm, UiEvent};
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

    /// A site fixture's page and URL, and a host serving its package's files at the
    /// site's origin and the recorded API answers at theirs; anything else is not
    /// found, as it is offline in the world.
    fn site(name: &str) -> (String, String, MemoryHost) {
        let spec: Value = serde_json::from_str(
            &std::fs::read_to_string(fixture_dir().join(format!("{name}.site.json")))
                .expect("site.json"),
        )
        .expect("site.json parses");
        let repo = crate_dir().join("../../..");
        let mut h = MemoryHost::new();
        let url = spec["url"].as_str().expect("site url").to_owned();
        let mut page = None;
        for (host, dir) in spec["static"].as_object().expect("site static hosts") {
            let root = repo.join(dir.as_str().unwrap()).join("public");
            let mut stack = vec![root.clone()];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).expect("package dir") {
                    let p = e.unwrap().path();
                    if p.is_dir() {
                        stack.push(p);
                        continue;
                    }
                    let rel = p
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    let body = std::fs::read(&p).unwrap();
                    let at = format!("http://{host}/{rel}");
                    let ty = content_type(&rel);
                    if rel == "index.html" {
                        let dir_url = format!("http://{host}/");
                        if url == dir_url {
                            page = Some(String::from_utf8(body.clone()).unwrap());
                        }
                        h.responses
                            .insert(dir_url.clone(), FetchResponse::ok(&dir_url, ty, &body));
                    }
                    h.responses
                        .insert(at.clone(), FetchResponse::ok(&at, ty, &body));
                }
            }
        }
        if let Some(api) = spec["api"].as_str() {
            let rec: Value = serde_json::from_str(
                &std::fs::read_to_string(fixture_dir().join(api)).expect("api recording"),
            )
            .expect("api recording parses");
            for r in rec["requests"].as_array().unwrap() {
                let (u, resp) = (r["url"].as_str().unwrap(), &r["response"]);
                let body: Vec<u8> = match resp["text"].as_str() {
                    Some(t) => t.as_bytes().to_vec(),
                    None => serde_json::from_value(resp["bytes"].clone()).unwrap(),
                };
                let mut f = FetchResponse::ok(u, "", &body);
                f.status = resp["status"].as_u64().unwrap() as u16;
                f.headers = resp["headers"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect();
                h.responses.insert(u.to_owned(), f);
            }
        }
        (
            page.expect("the site's URL is a package's index.html"),
            url,
            h,
        )
    }

    fn site_settle_ms(name: &str) -> u32 {
        let spec: Value = serde_json::from_str(
            &std::fs::read_to_string(fixture_dir().join(format!("{name}.site.json"))).unwrap(),
        )
        .unwrap();
        spec["settle_ms"].as_u64().unwrap_or(0) as u32
    }

    fn content_type(path: &str) -> &'static str {
        match path.rsplit('.').next().unwrap_or("") {
            "html" => "text/html",
            "css" => "text/css",
            "js" | "mjs" => "text/javascript",
            "json" => "application/json",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "ico" => "image/x-icon",
            "woff2" => "font/woff2",
            "woff" => "font/woff",
            "ttf" => "font/ttf",
            _ => "application/octet-stream",
        }
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

    /// Runs the event loop until a pass finds nothing to run, as Chromium's settle
    /// (two frames and a task) lets every queued task run. One pass can leave work
    /// behind: React's scheduler yields after a few milliseconds and posts itself a
    /// message to continue, which is where a first render's `useEffect`s run.
    fn settle(r: &mut Realm, advance_ms: u32) {
        for _ in 0..16 {
            if !r.run_until_idle(advance_ms) {
                break;
            }
        }
    }

    /// One step as Playwright performs it: a click moves the pointer onto the
    /// element's centre and clicks there (so hit testing picks the target), typing
    /// goes to the focused element, a key press is a key press.
    fn perform(r: &mut Realm, step: &Value, settle_ms: u32) {
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
            "focus" => {
                // What Playwright's `page.focus` does: the element's own focus().
                let selector = step["selector"].as_str().unwrap();
                r.eval(&format!("document.querySelector({selector:?}).focus()"))
                    .unwrap_or_else(|e| panic!("{selector}: {e}"));
            }
            other => panic!("unknown action {other:?}"),
        }
        settle(r, 20);
        if settle_ms > 0 {
            settle(r, settle_ms);
        }
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
        // there gives no space to its bars: the realm's host draws overlay scrollbars.
        let is_site = fixture_dir().join(format!("{name}.site.json")).exists();
        let mut r = if is_site {
            let (page, url, h) = site(name);
            Realm::new(&page, &url, Box::new(h))
        } else {
            Realm::new(html, &format!("{BASE}{name}.html"), Box::new(host()))
        };
        r.set_overlay_scrollbars(true);
        // Fonts as on the machine the dumps were taken on (the static parity runner's
        // environment too): a family it lacks falls through to the list's next.
        r.set_font_environment(cw_web::css::FontEnvironment::LinuxBaseline);
        r.run_document();
        settle(&mut r, 50);
        // A site's own timers (react-admin's fake provider answers after 300 ms) get
        // the `settle_ms` of virtual time that dump.mjs gives them in wall time.
        let settle_ms = if is_site { site_settle_ms(name) } else { 0 };
        if settle_ms > 0 {
            settle(&mut r, settle_ms);
        }
        let boot = t.elapsed();
        let t = Instant::now();
        for step in list {
            perform(&mut r, step, settle_ms);
        }
        let steps = t.elapsed();
        // Development aid: CW_PARITY_EVAL=<js> prints what the page evaluates to
        // after the steps.
        if let Some(js) = std::env::var_os("CW_PARITY_EVAL") {
            eprintln!("{name}: {:?}", r.eval(&js.to_string_lossy()));
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
            std::fs::read_to_string(fixture_dir().join(format!("{name}.html"))).unwrap_or_default();
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

    #[test]
    fn app_inbox() {
        run_fixture("app-inbox");
    }

    #[test]
    fn app_calendar() {
        run_fixture("app-calendar");
    }

    #[test]
    fn app_music() {
        run_fixture("app-music");
    }

    // Real open-source apps, as worlds/oss-web serves them (docs/oss-webapps.md).
    #[test]
    fn oss_todomvc_react() {
        run_fixture("oss-todomvc-react");
    }

    #[test]
    fn oss_todomvc_vue() {
        run_fixture("oss-todomvc-vue");
    }

    #[test]
    fn oss_json_server() {
        run_fixture("oss-json-server");
    }

    #[test]
    fn oss_conduit_react() {
        run_fixture("oss-conduit-react");
    }

    #[test]
    fn oss_conduit_vue() {
        run_fixture("oss-conduit-vue");
    }

    #[test]
    fn oss_react_admin() {
        run_fixture("oss-react-admin");
    }
}
