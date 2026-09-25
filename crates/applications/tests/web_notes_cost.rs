//! What Notes costs as a web application: launch, one interaction, one paint, the
//! heap a window holds and its snapshot. (Against the Painter Notes it replaced, see
//! the commit that introduced the web-app host.)
//! Measured, not asserted; run in release:
//!
//!     cargo test --release -p cw-applications --test web_notes_cost -- --ignored --nocapture
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use cw_applications::desktop_scene::{app_content_with, DesktopTheme};
use cw_applications::{AppEffect, AppEnv, AppState, NativeApp, SystemSettings};

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        LIVE.fetch_add(new as isize - layout.size() as isize, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

const FOLDER: &str = "/home/alice/Notes";
const NAMES: [&str; 3] = ["groceries.txt", "ideas.txt", "plans.txt"];

fn env() -> AppEnv<'static> {
    AppEnv {
        theme: DesktopTheme::Macos,
        width: 900,
        height: 600,
        clock_us: 0,
        settings: &SystemSettings::DEFAULT,
        clipboard: None,
        share_to: None,
        editor: None,
        pointer: None,
        files: Default::default(),
    }
}

fn names() -> Vec<String> {
    NAMES.iter().map(|n| (*n).to_owned()).collect()
}

fn web() -> NativeApp {
    let (mut app, effects) = NativeApp::launch("notes", FOLDER, 1, 0).unwrap();
    assert!(matches!(effects[..], [AppEffect::ListTree { .. }]));
    app.tree_listed(1, FOLDER, Ok(names())).unwrap();
    app
}

/// Opens a note, types into it and saves it: the effects answered as the machine would.
fn interact(app: &mut NativeApp, round: u64) {
    let name = NAMES[(round % 3) as usize];
    let effects = app.click(1, &format!("notes:open:{name}"), 0).unwrap();
    for effect in effects {
        match effect {
            AppEffect::ReadFiles { tag, paths, .. } => {
                app.files_read(1, &tag, vec![(paths[0].clone(), Ok("text".into()))])
                    .unwrap();
            }
            other => panic!("{other:?}"),
        }
    }
    app.text_effects(1, "abc").unwrap();
    let effects = app.click(1, "notes:save", 0).unwrap();
    for effect in effects {
        if let (AppEffect::WriteFile { path, .. }, Some(w)) = (&effect, app.web_mut()) {
            for more in w.written(1, path).unwrap() {
                assert!(matches!(more, AppEffect::ListTree { .. }));
                w.tree_listed(1, FOLDER, Ok(names())).unwrap();
            }
        }
    }
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn measure(label: &str, make: fn() -> NativeApp) {
    let e = env();
    let launches: Vec<Duration> = (0..9)
        .map(|_| {
            let t = Instant::now();
            let app = make();
            let d = t.elapsed();
            drop(app);
            d
        })
        .collect();
    let before = LIVE.load(Ordering::Relaxed);
    let mut app = make();
    let _ = app_content_with(&AppState::Native(app.clone()), &e);
    let held = LIVE.load(Ordering::Relaxed) - before;
    let rounds: Vec<Duration> = (0..30)
        .map(|round| {
            let t = Instant::now();
            interact(&mut app, round);
            t.elapsed()
        })
        .collect();
    let paints: Vec<Duration> = (0..30)
        .map(|round| {
            // A fresh clone per paint, as the compositor projects a window.
            app.text_effects(1, if round % 2 == 0 { "x" } else { "y" })
                .unwrap();
            let state = AppState::Native(app.clone());
            let t = Instant::now();
            let scene = app_content_with(&state, &e);
            let d = t.elapsed();
            assert!(!scene.nodes.is_empty());
            d
        })
        .collect();
    let snapshot = serde_json::to_string(&app).unwrap();
    let t = Instant::now();
    let restored: NativeApp = serde_json::from_str(&snapshot).unwrap();
    let _ = app_content_with(&AppState::Native(restored), &e);
    let restore = t.elapsed();
    println!(
        "{label}: launch {:?} (median of 9), open+type+save {:?} (median of 30), \
         paint {:?} (median of 30), heap held {} bytes, snapshot {} bytes, restore+paint {:?}",
        median(launches),
        median(rounds),
        median(paints),
        held,
        snapshot.len(),
        restore
    );
}

#[test]
#[ignore]
fn web_notes_costs() {
    measure("web", web);
}

/// Where one web entry's time goes: the event, the settle after it, and layout, on
/// cw-ui and on the React fallback compiled from the same source.
#[test]
#[ignore]
fn where_a_web_entry_spends_its_time() {
    use cw_applications::web_app::{definition, env_for, runtime::*};
    use cw_web::script::{Modifiers, UiEvent};
    let def = definition("notes").unwrap();
    let cw_sdk::WebSource::Compiled { ir, script, style } = &def.app.source else {
        panic!("Notes is compiled for cw-ui");
    };
    let env = env_for(DesktopTheme::Macos, 900, 600);
    let boot = Boot {
        kind: "notes",
        argument: FOLDER,
        state: None,
        env: &env,
    };
    for backend in ["cw-ui", "react"] {
        println!("{backend}:");
        let t = Instant::now();
        let mut rt: Box<dyn AppRuntime> = if backend == "cw-ui" {
            Box::new(UiRuntime::boot(ir, style, &boot, None, 0).unwrap())
        } else {
            Box::new(JsRuntime::boot(style, script, true, &boot, 0))
        };
        println!("boot {:?}", t.elapsed());
        let out = rt.drain();
        let id = out.requests[0].0;
        let t = Instant::now();
        rt.deliver(&[Reply::ok(id, serde_json::json!(NAMES))], 0);
        println!("deliver listing {:?}", t.elapsed());
        let mut node = None;
        let t = Instant::now();
        rt.view(&mut |v| node = v.doc.by_id("notes:new").first().copied());
        println!("layout {:?}", t.elapsed());
        for round in 0..3 {
            let t = Instant::now();
            rt.dispatch(
                UiEvent::ClickNode {
                    node: node.unwrap(),
                    modifiers: Modifiers::default(),
                    detail: 1,
                },
                round * 2_000_000,
            );
            let click = t.elapsed();
            let t = Instant::now();
            rt.view(&mut |v| node = v.doc.by_id("notes:new").first().copied());
            println!("click+settle {click:?}, layout after {:?}", t.elapsed());
        }
        for text in ["a", "b", "c"] {
            let t = Instant::now();
            rt.dispatch(UiEvent::TypeText { text: text.into() }, 9_000_000);
            let typed = t.elapsed();
            let t = Instant::now();
            rt.view(&mut |_| {});
            println!("type+settle {typed:?}, layout after {:?}", t.elapsed());
        }
    }
}

/// One app both ways behind the same host: compiled on cw-ui, and its React
/// fallback on the VM (the counter fixture).
#[test]
#[ignore]
fn compiled_against_react() {
    use cw_applications::web_app::{define, WebApp};
    for (kind, source) in [
        (
            "bench-compiled",
            cw_sdk::WebSource::Compiled {
                ir: include_str!("../src/web_app/fixtures/counter.ui.json").into(),
                script: include_str!("../src/web_app/fixtures/counter.js").into(),
                style: String::new(),
            },
        ),
        (
            "bench-react",
            cw_sdk::WebSource::Script {
                script: include_str!("../src/web_app/fixtures/counter.js").into(),
                style: String::new(),
                react: true,
            },
        ),
    ] {
        define(cw_sdk::WebApplication {
            kind: kind.into(),
            version: 1,
            titles: Default::default(),
            source,
        })
        .unwrap();
        let launches: Vec<Duration> = (0..9)
            .map(|_| {
                let t = Instant::now();
                let app = WebApp::launch(kind, "", 1, 0, DesktopTheme::Macos).unwrap();
                let d = t.elapsed();
                drop(app);
                d
            })
            .collect();
        let before = LIVE.load(Ordering::Relaxed);
        let (mut app, _) = WebApp::launch(kind, "", 1, 0, DesktopTheme::Macos).unwrap();
        let held = LIVE.load(Ordering::Relaxed) - before;
        let clicks: Vec<Duration> = (0..30)
            .map(|_| {
                let t = Instant::now();
                app.click(1, "counter:add", 0).unwrap();
                t.elapsed()
            })
            .collect();
        app.click(1, "counter:name", 0).unwrap();
        let keys: Vec<Duration> = (0..30)
            .map(|_| {
                let t = Instant::now();
                app.text_effects(1, "a").unwrap();
                t.elapsed()
            })
            .collect();
        let snapshot = serde_json::to_string(&app).unwrap();
        let t = Instant::now();
        let restored: WebApp = serde_json::from_str(&snapshot).unwrap();
        let _ = restored.text_field();
        let restore = t.elapsed();
        println!(
            "{kind}: launch {:?}, click {:?}, keystroke {:?} (medians), heap held {held} bytes, \
             snapshot {} bytes, restore {:?}",
            median(launches),
            median(clicks),
            median(keys),
            snapshot.len(),
            restore
        );
    }
}

/// A Notes window saved while its read of a note is outstanding, on each backend:
/// how large the saved window is, and what saving and restoring it cost.
#[test]
#[ignore]
fn saved_mid_request() {
    use cw_applications::web_app::{define, definition, WebApp};
    let entry = definition("notes").unwrap();
    let cw_sdk::WebSource::Compiled { script, style, .. } = &entry.app.source else {
        panic!("Notes ships its IR");
    };
    define(cw_sdk::WebApplication {
        kind: "notes-bench-react".into(),
        version: 1,
        titles: Default::default(),
        source: cw_sdk::WebSource::Script {
            script: script.clone(),
            style: style.clone(),
            react: true,
        },
    })
    .unwrap();
    for kind in ["notes", "notes-bench-react"] {
        let (mut app, _) = WebApp::launch(kind, FOLDER, 1, 0, DesktopTheme::Macos).unwrap();
        app.tree_listed(1, FOLDER, Ok(names())).unwrap();
        let quiet = serde_json::to_string(&app).unwrap().len();
        app.click(1, "notes:open:ideas.txt", 0).unwrap();
        let saves: Vec<Duration> = (0..9)
            .map(|_| {
                let t = Instant::now();
                let json = serde_json::to_string(&app).unwrap();
                let d = t.elapsed();
                assert!(json.contains("inflight"));
                d
            })
            .collect();
        let json = serde_json::to_string(&app).unwrap();
        let restores: Vec<Duration> = (0..9)
            .map(|_| {
                let t = Instant::now();
                let mut copy: WebApp = serde_json::from_str(&json).unwrap();
                copy.files_read(
                    1,
                    "web:2",
                    vec![(format!("{FOLDER}/ideas.txt"), Ok("x".into()))],
                )
                .unwrap();
                t.elapsed()
            })
            .collect();
        println!(
            "{kind}: saved quiet {quiet} bytes, mid-request {} bytes; save {:?}, restore+answer {:?} (medians)",
            json.len(),
            median(saves),
            median(restores)
        );
    }
}
