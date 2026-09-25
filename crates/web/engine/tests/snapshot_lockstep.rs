//! Heap snapshots of realms running unmodified framework builds: every framework
//! parity fixture is driven through its steps with the snapshot check on (before
//! each entry point the realm is written, a second realm restored from the bytes
//! runs the same entry point, and both must return the same and write the same
//! bytes afterwards), and at the end of every state a realm restored from its heap
//! image must be the realm replaying its journal rebuilds.
//!
//! With `--nocapture` it reports, per state, the image's size and the time to write
//! it, to restore from it and to replay the journal instead.
//!
//!     cargo test -p cw-web --release --test snapshot_lockstep -- --nocapture

use cw_web::script::{set_verify_snapshots, MemoryHost, Modifiers, Realm, UiEvent};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Instant;

const BASE: &str = "https://example.test/";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_dir() -> PathBuf {
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

fn steps(name: &str) -> Vec<(String, Vec<Value>)> {
    let p = fixture_dir().join(format!("{name}.steps.json"));
    let v: serde_json::Map<String, Value> =
        serde_json::from_str(&std::fs::read_to_string(&p).expect("steps")).expect("steps parse");
    v.into_iter()
        .map(|(state, list)| (state, list.as_array().expect("a list").clone()))
        .collect()
}

/// The vendored bundles and the fixtures' own scripts, as `framework_parity` serves
/// them.
fn host() -> MemoryHost {
    let mut h = MemoryHost::new();
    let mut add = |dir: PathBuf, prefix: &str, only_js: bool| {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .filter(|p| !only_js || p.extension().is_some_and(|x| x == "js"))
            .collect();
        entries.sort();
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
            h = std::mem::take(&mut h).with_response(&format!("{BASE}{prefix}{name}"), ty, &body);
        }
    };
    add(crate_dir().join("tests/vendor"), "vendor/", false);
    add(fixture_dir(), "", true);
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
        other => panic!("unknown action {other:?}"),
    }
    settle(r, 20);
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn run_fixture(name: &str) {
    let html = std::fs::read_to_string(fixture_dir().join(format!("{name}.html"))).unwrap();
    let url = format!("{BASE}{name}.html");
    for (state, list) in steps(name) {
        set_verify_snapshots(true);
        let mut r = Realm::new(&html, &url, Box::new(host()));
        r.set_overlay_scrollbars(true);
        r.run_document();
        settle(&mut r, 50);
        let booted = r.snapshot_with_image();
        for step in &list {
            perform(&mut r, step);
        }
        let _ = r.fragment_tree();
        set_verify_snapshots(false);

        let image = r.heap_image().expect("the realm images");
        let state_only = r.snapshot();
        let t = Instant::now();
        let mut with_image = r.snapshot_with_image();
        let write_ms = ms(t);
        assert!(with_image.image.0.is_some());
        let (h1, h2) = (Box::new(host()), Box::new(host()));
        let t = Instant::now();
        let mut restored = Realm::restore(&with_image, h1);
        let restore_ms = ms(t);
        let t = Instant::now();
        let mut replayed = Realm::replay(&state_only, h2);
        let replay_ms = ms(t);
        let a = restored.heap_image().unwrap();
        let b = replayed.heap_image().unwrap();
        assert!(
            a == image && b == image,
            "{name}.{state}: restore and replay disagree with the live realm"
        );
        // An image of the booted page and the inputs after it rebuild the realm too.
        let mut tail = r.snapshot();
        tail.image = booted.image.clone();
        let from_boot = Realm::restore(&tail, Box::new(host()));
        assert!(
            from_boot.heap_image().unwrap() == image,
            "{name}.{state}: the booted image and the inputs after it disagree with the live realm"
        );
        drop(from_boot);
        // Both go on the same way.
        for x in [&mut r, &mut restored, &mut replayed] {
            x.eval("document.body.setAttribute('data-after', String(document.querySelectorAll('*').length))")
                .unwrap();
            settle(x, 20);
        }
        let (a, b, c) = (
            r.heap_image().unwrap(),
            restored.heap_image().unwrap(),
            replayed.heap_image().unwrap(),
        );
        assert!(a == b && a == c, "{name}.{state}: the realms diverged");
        assert_eq!(*r.document(), *restored.document());
        let shared = with_image.image.0.take().unwrap();
        eprintln!(
            "{name}.{state}: image {} KiB ({} KiB besides {} shared sources), write {write_ms:.2} ms, \
             restore {restore_ms:.2} ms, replay {replay_ms:.2} ms ({} inputs)",
            image.len() / 1024,
            shared.bytes.len() / 1024,
            shared.sources.len(),
            state_only.inputs.len()
        );
    }
}

#[test]
fn every_framework_fixture_survives_a_snapshot_at_every_step() {
    for name in fixtures() {
        run_fixture(&name);
    }
}
