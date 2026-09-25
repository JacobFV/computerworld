//! The `cw` global over its host channel, and what a snapshot keeps of it: requests
//! still awaiting their replies, the promise chains hanging off them and the async
//! functions waiting on them. A restored app is answered as the original is. Each
//! test runs on the interpreter and on the Rust generated from the same IR
//! (`cw-ui-fixtures`), whose async functions are the interpreter's.

use std::collections::BTreeMap;

mod cw_host;

use cw_host::{CwHost, LAST_OUT};
use cw_ui::UiApp;
use cw_web::script::StorageArea;

const CW_D_TS: &str = include_str!("../../../applications/web/types/cw.d.ts");

const APP: &str = r#"/// <reference path="./cw.d.ts" />
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';

async function loadAll(folder: string): Promise<string[]> {
  const out: string[] = [];
  try {
    const names = await cw.fs.list(folder);
    for (const n of names) {
      const text = await cw.fs.readFile(folder + '/' + n);
      out.push(n + '=' + text);
    }
    const [a, b] = await Promise.all([cw.fs.readFile('x'), cw.fs.readFile('y')]);
    out.push(a + b);
  } catch (e) {
    out.push('failed: ' + (e instanceof Error ? e.message : String(e)));
  } finally {
    out.push('done');
  }
  return out;
}

function App() {
  const [lines, setLines] = useState<string[]>([]);
  const [first, setFirst] = useState('waiting');
  useEffect(() => {
    cw.fs.readFile('first.txt').then(
      (t) => setFirst('first ' + t.toUpperCase()),
      (e) => setFirst('no first: ' + (e instanceof Error ? e.message : '?')),
    );
    loadAll(cw.argument).then(setLines);
  }, []);
  return <div><p id="first">{first}</p><ul>{lines.map((l, i) => <li key={i}>{l}</li>)}</ul></div>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#;

const SHELL: &str = r#"<!DOCTYPE html><html><head><meta charset="utf-8"></head><body><div id="root"></div></body></html>"#;

/// A host answering the bridge's calls, as the desktop's web-app host does.
fn host() -> CwHost {
    CwHost::new(
        r#"{"kind":"t","argument":"/f","state":null,"env":{"platform":"macos","mobile":false,"width":800,"height":600,"css":""}}"#,
    )
}

fn module() -> cw_ui::ir::Module {
    let files: BTreeMap<&str, &str> = [("app.tsx", APP), ("cw.d.ts", CW_D_TS)].into();
    let sources = cw_tsx::load("app.tsx", &mut |f| files.get(f).map(|s| (*s).to_owned())).unwrap();
    let b = cw_tsx::build_modules(&sources);
    assert!(b.diagnostics.is_empty(), "{:?}", b.diagnostics);
    b.ir.unwrap()
}

/// `module` interpreted, or its generated program (registered, so a snapshot of it
/// restores by name).
fn app_on(module: cw_ui::ir::Module, generated: bool) -> UiApp {
    if generated {
        let p = cw_ui_fixtures::for_module(&module).expect("a generated program for this IR");
        cw_ui::program::register(p);
        let app = UiApp::generated(p, SHELL, "cw-app://t/", Box::new(host())).unwrap();
        assert!(app.is_generated());
        app
    } else {
        UiApp::new(module, SHELL, "cw-app://t/", Box::new(host())).unwrap()
    }
}

/// A snapshot of `module()` restored on the other form than the one it was taken
/// on: a suspended async function, its promises and the requests it awaits carry
/// over between the interpreter and generated code.
fn restore_on_other_form(saved: &cw_ui::UiState, generated: bool) -> UiApp {
    let m = module();
    let program: std::rc::Rc<dyn cw_ui::Program> = if generated {
        std::rc::Rc::new(cw_ui::IrProgram::new(m))
    } else {
        let p = cw_ui_fixtures::for_module(&m).unwrap();
        cw_ui::program::register(p);
        std::rc::Rc::new(cw_ui::program::StaticProgram(p))
    };
    let app = UiApp::restore_with(saved, program, Box::new(host())).unwrap();
    assert_eq!(app.is_generated(), !generated);
    app
}

fn text(app: &UiApp) -> String {
    app.document()
        .text_content(app.query_selector("#root").unwrap())
}

/// The last message the app wrote to its host.
fn last_out(app: &mut UiApp) -> String {
    app.inner()
        .host
        .storage_get(StorageArea::Local, LAST_OUT)
        .unwrap_or_default()
}

fn reply(app: &mut UiApp, json: &str) {
    app.cw_deliver(json).unwrap();
    app.run_until_idle(20);
}

#[test]
fn requests_in_flight_are_answered_after_a_restore() {
    requests_in_flight_are_answered_after_a_restore_on(false);
    requests_in_flight_are_answered_after_a_restore_on(true);
}

fn requests_in_flight_are_answered_after_a_restore_on(generated: bool) {
    let mut app = app_on(module(), generated);
    app.boot();
    app.run_until_idle(20);
    // 1: first.txt, 2: the listing.
    assert_eq!(text(&app), "waiting");
    assert!(
        last_out(&mut app).contains(r#""kind":"list""#),
        "{}",
        last_out(&mut app)
    );
    reply(&mut app, r#"[{"id":2,"value":["a","b"]}]"#);
    // 3: /f/a, now awaited inside the for...of inside the try.
    assert!(
        last_out(&mut app).contains(r#""path":"/f/a""#),
        "{}",
        last_out(&mut app)
    );

    let saved = cw_ui::UiState::from_json(&app.snapshot().to_json()).unwrap();
    let mut restored = restore_on_other_form(&saved, generated);
    assert_eq!(text(&restored), "waiting");

    for a in [&mut app, &mut restored] {
        reply(a, r#"[{"id":1,"value":"one"},{"id":3,"value":"A"}]"#);
        assert!(
            last_out(&mut *a).contains(r#""path":"/f/b""#),
            "{}",
            last_out(&mut *a)
        );
        reply(a, r#"[{"id":4,"value":"B"}]"#);
        // 5 and 6: the Promise.all.
        reply(a, r#"[{"id":6,"value":"-y"}]"#);
    }
    // Mid-Promise.all, and again after a second restore.
    let saved = cw_ui::UiState::from_json(&restored.snapshot().to_json()).unwrap();
    let mut again = UiApp::restore(&saved, Box::new(host())).unwrap();
    for a in [&mut app, &mut restored, &mut again] {
        reply(a, r#"[{"id":5,"value":"x"}]"#);
    }
    let want = "first ONEa=Ab=Bx-ydone";
    assert_eq!(text(&app), want);
    assert_eq!(text(&restored), want);
    assert_eq!(text(&again), want);
    // The same state, whichever form each runs on.
    let state = |a: &UiApp| {
        let mut s = a.snapshot();
        s.module = None;
        s.program = None;
        s.to_json()
    };
    assert_eq!(state(&app), state(&again));
}

#[test]
fn a_rejected_request_after_a_restore_reaches_the_catch() {
    a_rejected_request_after_a_restore_reaches_the_catch_on(false);
    a_rejected_request_after_a_restore_reaches_the_catch_on(true);
}

fn a_rejected_request_after_a_restore_reaches_the_catch_on(generated: bool) {
    let mut app = app_on(module(), generated);
    app.boot();
    app.run_until_idle(20);
    let saved = cw_ui::UiState::from_json(&app.snapshot().to_json()).unwrap();
    let mut restored = UiApp::restore(&saved, Box::new(host())).unwrap();
    for a in [&mut app, &mut restored] {
        reply(
            a,
            r#"[{"id":1,"error":"not found"},{"id":2,"error":"no such folder"}]"#,
        );
    }
    let want = "no first: not foundfailed: no such folderdone";
    assert_eq!(text(&app), want);
    assert_eq!(text(&restored), want);
}

#[test]
fn a_cw_fetch_reply_has_its_body_text_and_json() {
    a_cw_fetch_reply_has_its_body_text_and_json_on(false);
    a_cw_fetch_reply_has_its_body_text_and_json_on(true);
}

fn a_cw_fetch_reply_has_its_body_text_and_json_on(generated: bool) {
    let app_src = r#"/// <reference path="./cw.d.ts" />
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
interface Doc { title: string }
function App() {
  const [s, setS] = useState('');
  useEffect(() => {
    async function run() {
      const r = await cw.fetch('https://svc.test/doc', { method: 'post', body: 'q' });
      const d = await r.json<Doc>();
      const t = await r.text();
      setS(r.ok + ' ' + r.status + ' ' + r.body + ' ' + d.title + ' ' + t.length);
    }
    run();
  }, []);
  return <p id="s">{s}</p>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#;
    let files: BTreeMap<&str, &str> = [("app.tsx", app_src), ("cw.d.ts", CW_D_TS)].into();
    let sources = cw_tsx::load("app.tsx", &mut |f| files.get(f).map(|s| (*s).to_owned())).unwrap();
    let b = cw_tsx::build_modules(&sources);
    assert!(b.diagnostics.is_empty(), "{:?}", b.diagnostics);
    let mut app = app_on(b.ir.unwrap(), generated);
    app.boot();
    app.run_until_idle(20);
    let out = last_out(&mut app);
    assert!(
        out.contains(r#""kind":"http""#)
            && out.contains(r#""method":"POST""#)
            && out.contains(r#""body":"q""#),
        "{out}"
    );
    reply(
        &mut app,
        r#"[{"id":1,"value":{"status":201,"body":"{\"title\":\"T\"}"}}]"#,
    );
    assert_eq!(text(&app), r#"true 201 {"title":"T"} T 13"#);
}

#[test]
fn a_fetch_body_is_outside_the_subset() {
    let b = cw_tsx::build(
        "async function f() { const r = await fetch('/x'); return r.body; }\nfunction App() { return <p />; }",
        "app.tsx",
    );
    assert!(
        b.diagnostics
            .iter()
            .any(|d| d.message.contains("`body` does not exist on type Response")),
        "{:?}",
        b.diagnostics
    );
}
