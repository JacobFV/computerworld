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

/// Where one web entry's time goes: the event, the settle after it, and layout.
#[test]
#[ignore]
fn where_a_web_entry_spends_its_time() {
    use cw_applications::web_app::{definition, env_for, runtime::*};
    use cw_web::script::{Modifiers, UiEvent};
    let def = definition("notes").unwrap();
    let cw_sdk::WebSource::Script {
        script,
        style,
        react,
    } = &def.app.source;
    let env = env_for(DesktopTheme::Macos, 900, 600);
    let t = Instant::now();
    let mut rt = JsRuntime::boot(
        style,
        script,
        *react,
        &Boot {
            kind: "notes",
            argument: FOLDER,
            state: None,
            env: &env,
        },
        0,
    );
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
