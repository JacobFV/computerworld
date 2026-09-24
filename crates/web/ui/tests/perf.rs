//! Compiled (cw-ui) against fallback (React 18 on the Realm) on the TSX fixtures,
//! timed as `crates/web/engine/tests/script_perf.rs` times React: boot (load the page
//! and settle), a click that re-renders (`#check-2`), a keystroke into a controlled
//! input (`#new-task`), each over fresh apps, median and best reported. Also the
//! memory a mounted app holds (bytes live after boot, through a counting allocator),
//! the snapshot's size and the time to restore from it.
//!
//!     cargo test --release -p cw-ui --test perf -- --ignored --nocapture
//!
//! `UI_PERF_RUNS` sets the runs (default 15).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use cw_ui::UiApp;
use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LIVE.fetch_add(n as isize - l.size() as isize, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

const BASE: &str = "https://example.test/";

fn dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/framework-parity")
}

fn host() -> MemoryHost {
    let mut h = MemoryHost::new();
    let vendor = dir().join("../vendor");
    for name in [
        "react-18.3.1.production.min.js",
        "react-dom-18.3.1.production.min.js",
    ] {
        let body = std::fs::read_to_string(vendor.join(name)).unwrap();
        h = h.with_response(&format!("{BASE}vendor/{name}"), "text/javascript", &body);
    }
    for entry in std::fs::read_dir(dir()).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|x| x == "js") {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            h = h.with_response(
                &format!("{BASE}{name}"),
                "text/javascript",
                &std::fs::read_to_string(&p).unwrap(),
            );
        }
    }
    h
}

fn stats(mut v: Vec<f64>) -> (f64, f64) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (v[v.len() / 2], v[0])
}

fn click(app_click: &mut dyn FnMut(UiEvent), (x, y): (i32, i32)) {
    let modifiers = Modifiers::default();
    app_click(UiEvent::PointerMove { x, y, modifiers });
    app_click(UiEvent::Click {
        x,
        y,
        button: 0,
        modifiers,
        detail: 1,
    });
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn realm_centre(r: &mut Realm, selector: &str) -> (i32, i32) {
    let src = format!(
        "(() => {{ const q = document.querySelector({selector:?}).getBoundingClientRect(); return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()"
    );
    let v = r.eval(&src).unwrap();
    let mut it = v.split(',').map(|n| n.parse::<f64>().unwrap() as i32);
    (it.next().unwrap(), it.next().unwrap())
}

#[test]
#[ignore]
fn compiled_against_fallback() {
    let runs: usize = std::env::var("UI_PERF_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    let name = "tsx-tasks";
    let html = std::fs::read_to_string(dir().join(format!("{name}.html"))).unwrap();
    let ir = std::fs::read_to_string(dir().join(format!("{name}.ui.json"))).unwrap();
    let url = format!("{BASE}{name}.html");

    // ---------------------------------------------------------------- compiled
    let (mut boots, mut parses, mut clicks, mut keys, mut mems) =
        (vec![], vec![], vec![], vec![], vec![]);
    let mut layouts = vec![];
    let mut click_scripts = vec![];
    let (mut agains, mut again_scripts) = (vec![], vec![]);
    let (mut snap_sizes, mut restores, mut snaps) = (vec![], vec![], vec![]);
    for _ in 0..runs {
        let h = host();
        let before = LIVE.load(Ordering::Relaxed);
        let t = Instant::now();
        let module = UiApp::parse_ir(&ir).unwrap();
        parses.push(ms(t));
        let mut app = UiApp::new(module, &html, &url, Box::new(h)).unwrap();
        app.boot();
        app.run_until_idle(50);
        boots.push(ms(t));
        app.fragment_tree();
        // A full style and layout pass of this document, for scale.
        let t = Instant::now();
        app.inner().sheet_changed();
        app.fragment_tree();
        layouts.push(ms(t));
        mems.push((LIVE.load(Ordering::Relaxed) - before) as f64);
        let at = {
            let n = app.query_selector("#check-2").unwrap();
            app.centre_of(n).unwrap()
        };
        let t = Instant::now();
        let script = app.stats().script_micros;
        click(&mut |e| drop(app.dispatch(e)), at);
        app.run_until_idle(20);
        clicks.push(ms(t));
        click_scripts.push((app.stats().script_micros - script) as f64 / 1000.0);
        let t = Instant::now();
        let script = app.stats().script_micros;
        click(&mut |e| drop(app.dispatch(e)), at);
        app.run_until_idle(20);
        agains.push(ms(t));
        again_scripts.push((app.stats().script_micros - script) as f64 / 1000.0);
        let at = {
            let n = app.query_selector("#new-task").unwrap();
            app.centre_of(n).unwrap()
        };
        click(&mut |e| drop(app.dispatch(e)), at);
        app.run_until_idle(20);
        let t = Instant::now();
        app.dispatch(UiEvent::TypeText { text: "x".into() });
        app.run_until_idle(20);
        keys.push(ms(t));
        let t = Instant::now();
        let json = app.snapshot().to_json();
        snaps.push(ms(t));
        snap_sizes.push(json.len() as f64);
        let t = Instant::now();
        let state = cw_ui::UiState::from_json(&json).unwrap();
        let mut again = UiApp::restore(&state, Box::new(host())).unwrap();
        again.fragment_tree();
        restores.push(ms(t));
    }
    let module_bytes = ir.len();
    eprintln!(
        "compiled {name}: boot {:.3} ms (best {:.3}; IR parse {:.3}; a full restyle+layout {:.3}), click {:.3} ms (best {:.3}; script {:.3}), click again {:.3} ms (script {:.3}), key {:.3} ms (best {:.3}), \
         live after boot {:.0} KB, snapshot {:.0} KB (IR {:.0} KB of it) in {:.3} ms, restore {:.3} ms (best {:.3}) [median of {runs}]",
        stats(boots.clone()).0,
        stats(boots).1,
        stats(parses).0,
        stats(layouts).0,
        stats(clicks.clone()).0,
        stats(clicks).1,
        stats(click_scripts).0,
        stats(agains).0,
        stats(again_scripts).0,
        stats(keys.clone()).0,
        stats(keys).1,
        stats(mems).0 / 1024.0,
        stats(snap_sizes).0 / 1024.0,
        module_bytes as f64 / 1024.0,
        stats(snaps).0,
        stats(restores.clone()).0,
        stats(restores).1,
    );

    // ---------------------------------------------------------------- fallback
    let (mut boots, mut clicks, mut keys, mut mems) = (vec![], vec![], vec![], vec![]);
    let mut firsts = vec![];
    let mut agains = vec![];
    let (mut snap_sizes, mut restores) = (vec![], vec![]);
    for _ in 0..runs {
        // A first load: nothing compiled yet on this thread.
        cw_jsvm::codecache::clear();
        let t = Instant::now();
        let mut r = Realm::new(&html, &url, Box::new(host()));
        r.run_document();
        r.run_until_idle(50);
        firsts.push(ms(t));
        drop(r);
        let h = host();
        let before = LIVE.load(Ordering::Relaxed);
        let t = Instant::now();
        let mut r = Realm::new(&html, &url, Box::new(h));
        r.run_document();
        r.run_until_idle(50);
        boots.push(ms(t));
        mems.push((LIVE.load(Ordering::Relaxed) - before) as f64);
        let at = realm_centre(&mut r, "#check-2");
        let t = Instant::now();
        click(&mut |e| drop(r.dispatch(e)), at);
        r.run_until_idle(20);
        clicks.push(ms(t));
        let t = Instant::now();
        click(&mut |e| drop(r.dispatch(e)), at);
        r.run_until_idle(20);
        agains.push(ms(t));
        let at = realm_centre(&mut r, "#new-task");
        click(&mut |e| drop(r.dispatch(e)), at);
        r.run_until_idle(20);
        let t = Instant::now();
        r.dispatch(UiEvent::TypeText { text: "x".into() });
        r.run_until_idle(20);
        keys.push(ms(t));
        let json = serde_json::to_string(&r.snapshot()).unwrap();
        snap_sizes.push(json.len() as f64);
        let t = Instant::now();
        let state: cw_web::script::RealmState = serde_json::from_str(&json).unwrap();
        let mut again = Realm::restore(&state, Box::new(host()));
        again.fragment_tree();
        restores.push(ms(t));
    }
    eprintln!(
        "fallback {name}: first boot {:.3} ms (best {:.3}), boot {:.3} ms (best {:.3}), click {:.3} ms (best {:.3}), click again {:.3} ms, key {:.3} ms (best {:.3}), \
         live after boot {:.0} KB, snapshot {:.0} KB, restore {:.3} ms (best {:.3}) [median of {runs}]",
        stats(firsts.clone()).0,
        stats(firsts).1,
        stats(boots.clone()).0,
        stats(boots).1,
        stats(clicks.clone()).0,
        stats(clicks).1,
        stats(agains).0,
        stats(keys.clone()).0,
        stats(keys).1,
        stats(mems).0 / 1024.0,
        stats(snap_sizes).0 / 1024.0,
        stats(restores.clone()).0,
        stats(restores).1,
    );
}
