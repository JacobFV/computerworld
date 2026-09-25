//! The `cw` global over its host channel, and what a snapshot keeps of it: requests
//! still awaiting their replies, the promise chains hanging off them and the async
//! functions waiting on them. A restored app is answered as the original is.

use std::collections::BTreeMap;

use cw_ui::UiApp;
use cw_web::script::{MemoryHost, ScriptHostDocument, StorageArea};

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

fn host() -> MemoryHost {
    let mut h = MemoryHost::new();
    h.storage_set(
        StorageArea::Local,
        "\u{1}cw:boot",
        r#"{"kind":"t","argument":"/f","state":null,"env":{"platform":"macos","mobile":false,"width":800,"height":600,"css":""}}"#,
    );
    h
}

fn module() -> cw_ui::ir::Module {
    let files: BTreeMap<&str, &str> = [("app.tsx", APP), ("cw.d.ts", CW_D_TS)].into();
    let sources = cw_tsx::load("app.tsx", &mut |f| files.get(f).map(|s| (*s).to_owned())).unwrap();
    let b = cw_tsx::build_modules(&sources);
    assert!(b.diagnostics.is_empty(), "{:?}", b.diagnostics);
    b.ir.unwrap()
}

fn text(app: &UiApp) -> String {
    app.document()
        .text_content(app.query_selector("#root").unwrap())
}

/// The last message the app wrote to its host.
fn last_out(app: &mut UiApp) -> String {
    app.inner()
        .host
        .storage_get(StorageArea::Local, "\u{1}cw:out")
        .unwrap_or_default()
}

fn reply(app: &mut UiApp, json: &str) {
    app.cw_deliver(json).unwrap();
    app.run_until_idle(20);
}

#[test]
fn requests_in_flight_are_answered_after_a_restore() {
    let mut app = UiApp::new(module(), SHELL, "cw-app://t/", Box::new(host())).unwrap();
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
    let mut restored = UiApp::restore(&saved, Box::new(host())).unwrap();
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
    assert_eq!(app.snapshot().to_json(), again.snapshot().to_json());
}

#[test]
fn a_rejected_request_after_a_restore_reaches_the_catch() {
    let mut app = UiApp::new(module(), SHELL, "cw-app://t/", Box::new(host())).unwrap();
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
