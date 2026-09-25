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
    let mut files: Vec<_> = std::fs::read_dir(&vendor)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    for p in files {
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
    let name = "tsx-tasks";
    let html = std::fs::read_to_string(dir().join(format!("{name}.html"))).unwrap();
    let ir = std::fs::read_to_string(dir().join(format!("{name}.ui.json"))).unwrap();
    measure(name, &html, &ir, "#check-2", &["#new-task"]);
    // An agent-written app: the kanban board of app-src/kanban, compiled from its
    // sources; the fallback is the page as the agent built it (esbuild's bundle).
    let root = dir().join("app-src/kanban");
    let mut read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let sources = cw_tsx::load("main.tsx", &mut read).unwrap();
    let ir = serde_json::to_string(&cw_tsx::build_modules(&sources).ir.unwrap()).unwrap();
    let html = std::fs::read_to_string(dir().join("app-kanban.html")).unwrap();
    measure(
        "app-kanban",
        &html,
        &ir,
        "button[data-card=\"3\"]",
        &["#new-task", "#task-title"],
    );
}

/// Times the compiled app: interpreted from `ir`, or as its generated `program`.
fn measure_compiled(
    name: &str,
    html: &str,
    ir: &str,
    target: &str,
    focus: &[&str],
    runs: usize,
    program: Option<&'static cw_ui::GenProgram>,
) {
    let url = format!("{BASE}{name}.html");
    let (mut boots, mut parses, mut clicks, mut keys, mut mems) =
        (vec![], vec![], vec![], vec![], vec![]);
    let mut layouts = vec![];
    let mut click_scripts = vec![];
    let mut key_scripts = vec![];
    let (mut agains, mut again_scripts) = (vec![], vec![]);
    let (mut snap_sizes, mut restores, mut snaps) = (vec![], vec![], vec![]);
    for _ in 0..runs {
        let h = host();
        let before = LIVE.load(Ordering::Relaxed);
        let t = Instant::now();
        let mut app = match program {
            Some(p) => {
                parses.push(0.0);
                UiApp::generated(p, html, &url, Box::new(h)).unwrap()
            }
            None => {
                let module = UiApp::parse_ir(ir).unwrap();
                parses.push(ms(t));
                UiApp::new(module, html, &url, Box::new(h)).unwrap()
            }
        };
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
            let n = app.query_selector(target).unwrap();
            app.centre_of(n).unwrap()
        };
        let t = Instant::now();
        let script = app.stats().script_nanos;
        click(&mut |e| drop(app.dispatch(e)), at);
        app.run_until_idle(20);
        clicks.push(ms(t));
        click_scripts.push((app.stats().script_nanos - script) as f64 / 1e6);
        let at = {
            let n = app.query_selector(target).unwrap();
            app.centre_of(n).unwrap()
        };
        let t = Instant::now();
        let script = app.stats().script_nanos;
        click(&mut |e| drop(app.dispatch(e)), at);
        app.run_until_idle(20);
        agains.push(ms(t));
        again_scripts.push((app.stats().script_nanos - script) as f64 / 1e6);
        for sel in focus {
            let at = {
                let n = app.query_selector(sel).unwrap();
                app.centre_of(n).unwrap()
            };
            click(&mut |e| drop(app.dispatch(e)), at);
            app.run_until_idle(20);
        }
        let t = Instant::now();
        let script = app.stats().script_nanos;
        app.dispatch(UiEvent::TypeText { text: "x".into() });
        app.run_until_idle(20);
        keys.push(ms(t));
        key_scripts.push((app.stats().script_nanos - script) as f64 / 1e6);
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
    let module_bytes = if program.is_some() { 0 } else { ir.len() };
    let form = if program.is_some() {
        "generated"
    } else {
        "interpreted"
    };
    eprintln!(
        "{form} {name}: boot {:.3} ms (best {:.3}; IR parse {:.3}; a full restyle+layout {:.3}), click {:.3} ms (best {:.3}; script {:.4}), click again {:.3} ms (script {:.4}), key {:.4} ms (best {:.4}; script {:.4}), \
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
        stats(key_scripts).0,
        stats(mems).0 / 1024.0,
        stats(snap_sizes).0 / 1024.0,
        module_bytes as f64 / 1024.0,
        stats(snaps).0,
        stats(restores.clone()).0,
        stats(restores).1,
    );
}

/// Times one page both ways: boot, a click on `target` (twice), and a keystroke
/// after clicking each of `focus` in turn.
fn measure(name: &str, html: &str, ir: &str, target: &str, focus: &[&str]) {
    let runs: usize = std::env::var("UI_PERF_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    let url = format!("{BASE}{name}.html");

    // ------------------------------------------------- compiled: interpreted, generated
    let module = UiApp::parse_ir(ir).unwrap();
    let program = cw_ui_fixtures::for_module(&module).expect("a generated program for this IR");
    cw_ui::program::register(program);
    for generated in [false, true] {
        measure_compiled(
            name,
            html,
            ir,
            target,
            focus,
            runs,
            generated.then_some(program),
        );
    }

    // ---------------------------------------------------------------- fallback
    let (mut boots, mut clicks, mut keys, mut mems) = (vec![], vec![], vec![], vec![]);
    let mut firsts = vec![];
    let mut agains = vec![];
    let (mut snap_sizes, mut restores) = (vec![], vec![]);
    for _ in 0..runs {
        // A first load: nothing compiled yet on this thread.
        cw_jsvm::codecache::clear();
        let t = Instant::now();
        let mut r = Realm::new(html, &url, Box::new(host()));
        r.run_document();
        r.run_until_idle(50);
        firsts.push(ms(t));
        drop(r);
        let h = host();
        let before = LIVE.load(Ordering::Relaxed);
        let t = Instant::now();
        let mut r = Realm::new(html, &url, Box::new(h));
        r.run_document();
        r.run_until_idle(50);
        boots.push(ms(t));
        mems.push((LIVE.load(Ordering::Relaxed) - before) as f64);
        let at = realm_centre(&mut r, target);
        let t = Instant::now();
        click(&mut |e| drop(r.dispatch(e)), at);
        r.run_until_idle(20);
        clicks.push(ms(t));
        let at = realm_centre(&mut r, target);
        let t = Instant::now();
        click(&mut |e| drop(r.dispatch(e)), at);
        r.run_until_idle(20);
        agains.push(ms(t));
        for sel in focus {
            let at = realm_centre(&mut r, sel);
            click(&mut |e| drop(r.dispatch(e)), at);
            r.run_until_idle(20);
        }
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

/// Where a compiled Notes entry's time goes (crates/applications' Notes on its
/// `cw` channel, stubbed): the event with its render, the restyle, the layout.
#[test]
#[ignore]
fn notes_phases() {
    use cw_web::script::{ScriptHostDocument, StorageArea};
    let web =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../applications/web/notes");
    let ir = std::fs::read_to_string(web.join("notes.ui.json")).unwrap();
    let css = std::fs::read_to_string(web.join("notes.css")).unwrap();
    let html = format!(
        "<!DOCTYPE html><html data-platform=\"macos\"><head><meta charset=\"utf-8\">\
         <style id=\"cw-theme\"></style><style>{css}</style></head>\
         <body><div id=\"root\"></div></body></html>"
    );
    let mut host = MemoryHost::new();
    host.storage_set(
        StorageArea::Local,
        "\u{1}cw:boot",
        r#"{"kind":"notes","argument":"/n","state":null,"env":{"platform":"macos","mobile":false,"width":900,"height":600,"css":""}}"#,
    );
    let module = UiApp::parse_ir(&ir).unwrap();
    let mut app = UiApp::new(module, &html, "cw-app://application/", Box::new(host)).unwrap();
    app.boot();
    app.run_until_idle(20);
    app.cw_deliver(r#"[{"id":1,"value":["a.txt","b.txt","c.txt"]}]"#)
        .unwrap();
    app.inner().ensure_layout();
    let phase = |app: &mut UiApp, label: &str, f: &mut dyn FnMut(&mut UiApp)| {
        let t = Instant::now();
        f(app);
        let script = t.elapsed();
        let t = Instant::now();
        app.inner().ensure_styles();
        let styles = t.elapsed();
        let t = Instant::now();
        app.inner().ensure_layout();
        let layout = t.elapsed();
        println!("{label}: script {script:?}, restyle {styles:?}, layout {layout:?}");
    };
    for (next, name) in (2..).zip(["a", "b", "c"]) {
        let sel = format!("[id=\"notes:open:{name}.txt\"]");
        phase(&mut app, &format!("open {name}"), &mut |app| {
            let n = app.query_selector(&sel).unwrap();
            let (x, y) = app.centre_of(n).unwrap();
            app.dispatch(UiEvent::Click {
                x,
                y,
                button: 0,
                modifiers: Modifiers::default(),
                detail: 1,
            });
            app.run_until_idle(20);
        });
        phase(&mut app, "  read", &mut |app| {
            app.cw_deliver(&format!(r#"[{{"id":{next},"value":"text"}}]"#))
                .unwrap();
        });
        for k in ["x", "y"] {
            phase(&mut app, &format!("  type {k}"), &mut |app| {
                app.dispatch(UiEvent::TypeText { text: k.into() });
                app.run_until_idle(20);
            });
        }
    }
    // The fixed cost of a flush: building the cascade engine from the sheets, which
    // a restyle with no mutations does and nothing else.
    let inner = app.inner();
    let sheets: Vec<_> = inner.sheets.iter().map(|e| e.sheet.clone()).collect();
    let media = inner.media();
    let mut styles = inner.styles.clone();
    let ctx = cw_web::css::MatchContext::new();
    let mut times = Vec::new();
    for _ in 0..15 {
        let t = Instant::now();
        let _ = cw_web::style::restyle(
            &inner.doc,
            &mut styles,
            &[],
            &sheets,
            &media,
            &ctx,
            cw_web::Strictness::Lenient,
        );
        times.push(t.elapsed());
    }
    times.sort();
    println!(
        "restyle of nothing: {:?} (median of 15) over {} sheets",
        times[7],
        sheets.len()
    );
    println!(
        "{} elements",
        app.document()
            .descendants(cw_web::dom::Document::ROOT)
            .filter(|n| app.document().is_element(*n))
            .count()
    );
}

/// Notes (crates/applications' desktop notes app, on a stubbed `cw` channel)
/// interpreted from its IR and as its checked-in generated Rust: launch with the
/// listing delivered, the first style and layout, a session (open a note, its text
/// arriving, a keystroke, save, the new listing arriving; a style and layout pass
/// after each, as a host paints), the heap the app holds, its snapshot and a restore.
/// The React fallback's side is cw-applications' `web_notes_cost` harness, which
/// runs Notes on the Realm through the web-app host.
#[test]
#[ignore]
fn notes_interpreted_against_generated() {
    use cw_web::script::{ScriptHostDocument, StorageArea};
    let runs: usize = std::env::var("UI_PERF_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    let web =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../applications/web/notes");
    let ir = std::fs::read_to_string(web.join("notes.ui.json")).unwrap();
    let css = std::fs::read_to_string(web.join("notes.css")).unwrap();
    let html = format!(
        "<!DOCTYPE html><html data-platform=\"macos\"><head><meta charset=\"utf-8\">\
         <style id=\"cw-theme\"></style><style>{css}</style></head>\
         <body><div id=\"root\"></div></body></html>"
    );
    let notes_host = || {
        let mut host = MemoryHost::new();
        host.storage_set(
            StorageArea::Local,
            "\u{1}cw:boot",
            r#"{"kind":"notes","argument":"/n","state":null,"env":{"platform":"macos","mobile":false,"width":900,"height":600,"css":""}}"#,
        );
        host
    };
    let module = UiApp::parse_ir(&ir).unwrap();
    let program = cw_ui_fixtures::for_module(&module).expect("Notes' generated program");
    cw_ui::program::register(program);
    let url = "cw-app://application/";
    for generated in [false, true] {
        let (mut boots, mut firsts, mut mems) = (vec![], vec![], vec![]);
        let (mut sessions, mut session_scripts) = (vec![], vec![]);
        let (mut keys, mut key_scripts) = (vec![], vec![]);
        let (mut snap_sizes, mut restores) = (vec![], vec![]);
        for _ in 0..runs {
            let before = LIVE.load(Ordering::Relaxed);
            let t = Instant::now();
            let mut app = if generated {
                UiApp::generated(program, &html, url, Box::new(notes_host())).unwrap()
            } else {
                let module = UiApp::parse_ir(&ir).unwrap();
                UiApp::new(module, &html, url, Box::new(notes_host())).unwrap()
            };
            app.boot();
            app.run_until_idle(20);
            app.cw_deliver(r#"[{"id":1,"value":["a.txt","b.txt","c.txt"]}]"#)
                .unwrap();
            boots.push(ms(t));
            let t = Instant::now();
            app.fragment_tree();
            firsts.push(ms(t));
            mems.push((LIVE.load(Ordering::Relaxed) - before) as f64);
            assert_eq!(app.is_generated(), generated);
            // The session.
            let mut script = 0.0;
            let t = Instant::now();
            let s0 = app.stats().script_nanos;
            let n = app.query_selector("[id=\"notes:open:b.txt\"]").unwrap();
            let at = app.centre_of(n).unwrap();
            click(&mut |e| drop(app.dispatch(e)), at);
            app.run_until_idle(20);
            app.fragment_tree();
            let d = Instant::now();
            app.cw_deliver(r#"[{"id":2,"value":"text"}]"#).unwrap();
            script += ms(d);
            app.fragment_tree();
            let n = app.query_selector("[id=\"notes:body\"]").unwrap();
            let at = app.centre_of(n).unwrap();
            click(&mut |e| drop(app.dispatch(e)), at);
            app.run_until_idle(20);
            app.fragment_tree();
            let k = Instant::now();
            let ks = app.stats().script_nanos;
            app.dispatch(UiEvent::TypeText { text: "x".into() });
            app.run_until_idle(20);
            keys.push(ms(k));
            key_scripts.push((app.stats().script_nanos - ks) as f64 / 1e6);
            app.fragment_tree();
            let n = app.query_selector("[id=\"notes:save\"]").unwrap();
            let at = app.centre_of(n).unwrap();
            click(&mut |e| drop(app.dispatch(e)), at);
            app.run_until_idle(20);
            app.fragment_tree();
            let d = Instant::now();
            app.cw_deliver(r#"[{"id":3,"value":null},{"id":4,"value":null}]"#)
                .unwrap();
            let asked = app
                .inner()
                .host
                .storage_get(StorageArea::Local, "\u{1}cw:out")
                .unwrap_or_default();
            app.cw_deliver(r#"[{"id":5,"value":["a.txt","b.txt","c.txt"]}]"#)
                .unwrap();
            script += ms(d);
            app.fragment_tree();
            sessions.push(ms(t));
            script += (app.stats().script_nanos - s0) as f64 / 1e6;
            session_scripts.push(script);
            assert!(
                app.form_values().values().any(|v| v == "textx"),
                "the session ran: {:?}",
                app.form_values()
            );
            assert!(
                asked.contains(r#""id":5,"kind":"list""#),
                "the save asks for a new listing: {asked}"
            );
            let json = app.snapshot().to_json();
            snap_sizes.push(json.len() as f64);
            let t = Instant::now();
            let state = cw_ui::UiState::from_json(&json).unwrap();
            let mut again = UiApp::restore(&state, Box::new(notes_host())).unwrap();
            again.fragment_tree();
            restores.push(ms(t));
            assert_eq!(again.is_generated(), generated);
        }
        eprintln!(
            "{} notes: launch with listing {:.4} ms (best {:.4}), first style+layout {:.3} ms, \
             session {:.3} ms (script {:.4}), key {:.4} ms (script {:.4}), live after launch {:.0} KB, \
             snapshot {:.1} KB, restore+layout {:.3} ms [median of {runs}]",
            if generated { "generated" } else { "interpreted" },
            stats(boots.clone()).0,
            stats(boots).1,
            stats(firsts).0,
            stats(sessions).0,
            stats(session_scripts).0,
            stats(keys).0,
            stats(key_scripts).0,
            stats(mems).0 / 1024.0,
            stats(snap_sizes).0 / 1024.0,
            stats(restores).0,
        );
    }
}
