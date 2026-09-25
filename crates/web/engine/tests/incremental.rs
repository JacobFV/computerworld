//! Incremental style and layout against from-scratch passes. Every step of every
//! framework-parity fixture (`tests/framework-parity/<name>.steps.json`) is driven on
//! the Realm with `cw_web::style::profile::set_verify(true)`, under which each style
//! and layout flush is checked against a full cascade and a full layout of the same
//! document and state (the realm panics on the first difference). After each step
//! the settled page's paint (reusing what earlier paints cached) is compared with a
//! paint after `cw_web::paint::clear_caches`.
//!
//!     cargo test -p cw-web --features pipeline --test incremental
//!
//! Any other suite can run under the same check with `CW_WEB_VERIFY_INCREMENTAL=1`.

#[cfg(feature = "pipeline")]
mod cases {
    use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};
    use serde_json::Value;
    use std::path::PathBuf;

    const BASE: &str = "https://example.test/";

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }
    fn fixture_dir() -> PathBuf {
        crate_dir().join("tests/framework-parity")
    }

    fn host() -> MemoryHost {
        let mut h = MemoryHost::new();
        for dir in [crate_dir().join("tests/vendor"), fixture_dir()] {
            let mut entries: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.is_file())
                .collect();
            entries.sort();
            let vendor = dir.ends_with("vendor");
            for p in entries {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                let (ty, prefix) = if name.ends_with(".css") {
                    ("text/css", "vendor/")
                } else if name.ends_with(".js") {
                    ("text/javascript", if vendor { "vendor/" } else { "" })
                } else {
                    continue;
                };
                let Ok(body) = std::fs::read_to_string(&p) else {
                    continue;
                };
                h = h.with_response(&format!("{BASE}{prefix}{name}"), ty, &body);
            }
        }
        h
    }

    fn centre(r: &mut Realm, selector: &str) -> (i32, i32) {
        let src = format!(
            "(() => {{ const q = document.querySelector({selector:?}).getBoundingClientRect(); \
             return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()"
        );
        let v = r.eval(&src).unwrap_or_else(|e| panic!("{selector}: {e}"));
        let mut it = v.split(',').map(|n| n.parse::<f64>().unwrap() as i32);
        (it.next().unwrap(), it.next().unwrap())
    }

    fn settle(r: &mut Realm, advance_ms: u32) {
        for _ in 0..16 {
            if !r.run_until_idle(advance_ms) {
                break;
            }
        }
    }

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
            "focus" => {
                // What Playwright's `page.focus` does: the element's own focus().
                let selector = step["selector"].as_str().unwrap();
                r.eval(&format!("document.querySelector({selector:?}).focus()"))
                    .unwrap_or_else(|e| panic!("{selector}: {e}"));
            }
            other => panic!("unknown action {other:?}"),
        }
        settle(r, 20);
    }

    fn paint(r: &mut Realm) -> cw_scene::Scene {
        let tree = r.fragment_tree().clone();
        let styles = r.styles().clone();
        let doc = r.document();
        let images = cw_web::paint::ImageMap::from_document(&doc, &styles);
        let vp = cw_web::Viewport {
            width: 1280,
            height: 800,
            scale: 1,
            zoom: 100,
        };
        cw_web::paint::paint(
            &doc,
            &styles,
            &tree,
            vp,
            &cw_web::paint::PaintContext::new(&images),
        )
    }

    fn run(name: &str) {
        cw_web::style::profile::set_verify(true);
        let html = std::fs::read_to_string(fixture_dir().join(format!("{name}.html"))).unwrap();
        let text =
            std::fs::read_to_string(fixture_dir().join(format!("{name}.steps.json"))).unwrap();
        let states: serde_json::Map<String, Value> = serde_json::from_str(&text).unwrap();
        for (_, list) in states {
            let mut r = Realm::new(&html, &format!("{BASE}{name}.html"), Box::new(host()));
            r.set_overlay_scrollbars(true);
            r.run_document();
            settle(&mut r, 50);
            r.fragment_tree();
            for step in list.as_array().unwrap() {
                perform(&mut r, step);
                r.fragment_tree();
                // A paint that reuses what earlier paints cached equals one that
                // computes everything afresh.
                let warm = paint(&mut r);
                cw_web::paint::clear_caches();
                let cold = paint(&mut r);
                assert!(
                    warm == cold,
                    "{name}: a cached paint differs from a fresh one"
                );
                // Pointer moves over the page, hovering whatever lies under it.
                for (x, y) in [(5, 5), (640, 60), (300, 400), (1000, 700)] {
                    r.dispatch(UiEvent::PointerMove {
                        x,
                        y,
                        modifiers: Modifiers::default(),
                    });
                    r.fragment_tree();
                }
            }
        }
    }

    #[test]
    fn react18_tasks() {
        run("react18-tasks");
    }
    #[test]
    fn vue3_tasks() {
        run("vue3-tasks");
    }
    #[test]
    fn tsx_tasks() {
        run("tsx-tasks");
    }
    #[test]
    fn app_analytics() {
        run("app-analytics");
    }
    #[test]
    fn app_chat() {
        run("app-chat");
    }
    #[test]
    fn app_datatable() {
        run("app-datatable");
    }
    #[test]
    fn app_kanban() {
        run("app-kanban");
    }
    #[test]
    fn app_settings() {
        run("app-settings");
    }
    #[test]
    fn app_shop() {
        run("app-shop");
    }

    #[test]
    fn app_inbox() {
        run("app-inbox");
    }

    #[test]
    fn app_calendar() {
        run("app-calendar");
    }

    #[test]
    fn app_music() {
        run("app-music");
    }
}
