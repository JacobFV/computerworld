//! The `node-app` kind with small packages: static files and a single-page app's
//! fallback, a backend whose data lives in the instance state, and the determinism
//! the world depends on (same state and request, same bytes; a copy of the state
//! answers as the original would).
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_node_app::{NodeApp, Package};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const SERVER: &str = r#"
const http = require('http');
const fs = require('fs');
const crypto = require('crypto');
const DB = '/data/notes.json';
const load = () => JSON.parse(fs.readFileSync(DB, 'utf8'));
http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => { body += c; });
  req.on('end', () => {
    const notes = load();
    if (req.method === 'POST' && req.url === '/api/notes') {
      const note = { id: crypto.randomUUID(), text: JSON.parse(body).text, at: new Date().toISOString() };
      notes.push(note);
      fs.writeFileSync(DB, JSON.stringify(notes));
      res.writeHead(201, { 'Content-Type': 'application/json' });
      return res.end(JSON.stringify(note));
    }
    if (req.url === '/api/notes') {
      res.setHeader('Content-Type', 'application/json');
      return res.end(JSON.stringify(notes));
    }
    if (req.url === '/api/readonly') {
      try { fs.writeFileSync('/app/server.js', 'x'); } catch (e) { return res.end(e.code); }
    }
    res.statusCode = 404;
    res.end('no route');
  });
}).listen(process.env.PORT);
"#;

fn package(id: &str, manifest: Value, files: &[(&str, &'static str)]) -> Package {
    let mut map: BTreeMap<String, &'static [u8]> = files
        .iter()
        .map(|(p, b)| ((*p).to_owned(), b.as_bytes()))
        .collect();
    let mut manifest = manifest;
    manifest["id"] = json!(id);
    let text: &'static str = Box::leak(manifest.to_string().into_boxed_str());
    map.insert("manifest.json".into(), text.as_bytes());
    Package::new(map).unwrap()
}

fn app() -> NodeApp {
    NodeApp::new(vec![
        package(
            "spa",
            json!({"name": "an SPA", "spa_fallback": "/index.html"}),
            &[
                (
                    "public/index.html",
                    "<!doctype html><title>SPA</title><div id=app></div>",
                ),
                ("public/assets/app.js", "console.log(1)"),
                ("public/docs/index.html", "<p>docs</p>"),
            ],
        ),
        package(
            "notes",
            json!({"name": "notes", "server": {"main": "/app/server.js", "port": 8080}}),
            &[
                ("server/server.js", SERVER),
                ("public/index.html", "<p>notes</p>"),
            ],
        ),
    ])
}

fn ctx(tick: u64) -> ServiceContext {
    ServiceContext {
        actor: "ada".into(),
        source: "workstation".into(),
        tick,
        seed: 42,
        instance: "notes".into(),
    }
}

fn request(method: &str, path: &str, body: &str) -> HttpRequest {
    HttpRequest {
        method: method.into(),
        url: format!("http://site.test{path}"),
        headers: BTreeMap::from([("accept".into(), "text/html".into())]),
        body: body.as_bytes().to_vec(),
    }
}

#[test]
fn static_files_directories_and_the_spa_fallback() {
    let app = app();
    let mut state = app.initialize(json!({"package": "spa"}), &ctx(0)).unwrap();
    let get = |state: &mut Value, path: &str| {
        app.handle(state, &ctx(1), &request("GET", path, ""))
            .unwrap()
    };
    let js = get(&mut state, "/assets/app.js");
    assert_eq!(
        (js.status, js.header("content-type")),
        (200, Some("text/javascript; charset=utf-8"))
    );
    assert_eq!(get(&mut state, "/docs/").body, b"<p>docs</p>");
    let redirect = get(&mut state, "/docs");
    assert_eq!(
        (redirect.status, redirect.header("location")),
        (301, Some("/docs/"))
    );
    // A deep link is the app's index, for its router to read.
    let deep = get(&mut state, "/article/some-slug");
    assert_eq!(deep.status, 200);
    assert!(String::from_utf8(deep.body)
        .unwrap()
        .contains("<title>SPA</title>"));
    assert_eq!(
        app.handle(&mut state, &ctx(1), &request("POST", "/x", ""))
            .unwrap()
            .status,
        404
    );
    assert!(app.initialize(json!({"package": "nope"}), &ctx(0)).is_err());
    assert!(app
        .initialize(
            json!({"package": "notes", "files": {"/etc/passwd": "x"}}),
            &ctx(0)
        )
        .is_err());
}

#[test]
fn a_backend_keeps_its_data_in_the_state_and_answers_deterministically() {
    let app = app();
    let mut state = app
        .initialize(
            json!({"package": "notes", "files": {"/data/notes.json": []}}),
            &ctx(0),
        )
        .unwrap();
    // The frontend's files do not go through the backend.
    assert_eq!(
        app.handle(&mut state, &ctx(1), &request("GET", "/", ""))
            .unwrap()
            .body,
        b"<p>notes</p>"
    );
    let fork = state.clone();
    let created = app
        .handle(
            &mut state,
            &ctx(5_000_000),
            &request("POST", "/api/notes", r#"{"text":"hello"}"#),
        )
        .unwrap();
    assert_eq!(created.status, 201);
    assert_eq!(created.header("content-type"), Some("application/json"));
    let note: Value = serde_json::from_slice(&created.body).unwrap();
    // The world clock: tick 5 s past 2026-09-17T09:00:00Z (the VM's own clock then
    // runs on with the instructions it executes, deterministically).
    assert!(
        note["at"]
            .as_str()
            .unwrap()
            .starts_with("2026-09-17T09:00:05."),
        "{note}"
    );
    // A copy of the state (a snapshot, a fork) answers the same request identically.
    let mut replay = fork.clone();
    let again = app
        .handle(
            &mut replay,
            &ctx(5_000_000),
            &request("POST", "/api/notes", r#"{"text":"hello"}"#),
        )
        .unwrap();
    assert_eq!(again.body, created.body);
    assert_eq!(replay, state);
    // The next request draws new entropy.
    let second = app
        .handle(
            &mut state,
            &ctx(6_000_000),
            &request("POST", "/api/notes", r#"{"text":"again"}"#),
        )
        .unwrap();
    let second: Value = serde_json::from_slice(&second.body).unwrap();
    assert_ne!(second["id"], note["id"]);
    let list = app
        .handle(
            &mut state,
            &ctx(7_000_000),
            &request("GET", "/api/notes", ""),
        )
        .unwrap();
    let list: Value = serde_json::from_slice(&list.body).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(state["requests"], 3);
    let stored: Value =
        serde_json::from_str(state["files"]["/data/notes.json"].as_str().unwrap()).unwrap();
    assert_eq!(stored, list);
    // The package itself is read-only to the app.
    let ro = app
        .handle(
            &mut state,
            &ctx(8_000_000),
            &request("GET", "/api/readonly", ""),
        )
        .unwrap();
    assert_eq!(ro.body, b"EACCES");
}
