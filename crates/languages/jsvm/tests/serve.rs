//! `cw_jsvm::serve`: a Node program's own HTTP server answering a request from
//! outside the program, one fresh VM per request, with the program's durable
//! state kept in the host's files.
use cw_jsvm::serve::{serve, ServeRequest};
use cw_script_host::{memory::MemoryHost, ScriptHost};

const SERVER: &str = r#"
const http = require('http');
const fs = require('fs');
const FILE = '/data/count.json';
function load() { try { return JSON.parse(fs.readFileSync(FILE, 'utf8')); } catch { return { n: 0 }; } }
const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => { body += c; });
  req.on('end', () => {
    const state = load();
    if (req.method === 'POST') {
      state.n += JSON.parse(body).by;
      fs.writeFileSync(FILE, JSON.stringify(state));
    }
    // Answer from a later tick, as a database driver's callback would.
    setTimeout(() => {
      res.setHeader('content-type', 'application/json');
      res.setHeader('x-path', req.url);
      res.end(JSON.stringify({ n: state.n, at: Date.now(), r: Math.random() < 2 }));
    }, 5);
  });
});
// A housekeeping interval must not keep the exchange from finishing.
setInterval(() => {}, 60000);
// Listening only after some asynchronous start-up.
setTimeout(() => server.listen(3000), 10);
"#;

fn host() -> MemoryHost {
    let mut h = MemoryHost::default();
    h.mkdir("/app", true).unwrap();
    h.mkdir("/data", true).unwrap();
    h.write_file("/app/server.js", SERVER.as_bytes(), false)
        .unwrap();
    h
}

fn request(method: &str, path: &str, body: &str) -> ServeRequest {
    ServeRequest {
        method: method.into(),
        path: path.into(),
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: body.as_bytes().to_vec(),
    }
}

#[test]
fn a_node_server_answers_requests_and_keeps_its_state_in_files() {
    let mut h = host();
    let first = serve(
        &mut h,
        "/app/server.js",
        vec![],
        vec![],
        3000,
        &request("POST", "/count?x=1", r#"{"by":2}"#),
        50_000_000,
    );
    let r = first.response.expect("answered");
    assert_eq!(r.status, 200, "{:?}", first.error);
    let body = String::from_utf8(r.body.clone()).unwrap();
    assert!(body.starts_with(r#"{"n":2,"at":"#), "{body}");
    assert!(r
        .headers
        .iter()
        .any(|(k, v)| k == "x-path" && v == "/count?x=1"));
    // The next request is a new VM: only the file carries the count over.
    let second = serve(
        &mut h,
        "/app/server.js",
        vec![],
        vec![],
        3000,
        &request("POST", "/count", r#"{"by":3}"#),
        50_000_000,
    );
    let body = String::from_utf8(second.response.unwrap().body).unwrap();
    assert!(body.starts_with(r#"{"n":5,"at":"#), "{body}");
    assert_eq!(
        h.read_file("/data/count.json").unwrap(),
        br#"{"n":5}"#.to_vec()
    );
}

#[test]
fn the_same_host_state_answers_the_same_bytes() {
    let run = || {
        let mut h = host();
        serve(
            &mut h,
            "/app/server.js",
            vec![],
            vec![],
            3000,
            &request("GET", "/", ""),
            50_000_000,
        )
        .response
        .unwrap()
    };
    assert_eq!(run(), run());
}

#[test]
fn failures_are_reported_rather_than_hanging() {
    let mut h = host();
    let wrong_port = serve(
        &mut h,
        "/app/server.js",
        vec![],
        vec![],
        4000,
        &request("GET", "/", ""),
        50_000_000,
    );
    assert!(wrong_port.response.is_none());
    assert!(wrong_port
        .error
        .unwrap()
        .contains("did not listen on port 4000"));
    h.write_file("/app/bad.js", b"throw new TypeError('boom')", false)
        .unwrap();
    let thrown = serve(
        &mut h,
        "/app/bad.js",
        vec![],
        vec![],
        3000,
        &request("GET", "/", ""),
        50_000_000,
    );
    assert!(thrown.error.unwrap().contains("TypeError: boom"));
    h.write_file(
        "/app/silent.js",
        b"require('http').createServer(() => {}).listen(3000)",
        false,
    )
    .unwrap();
    let silent = serve(
        &mut h,
        "/app/silent.js",
        vec![],
        vec![],
        3000,
        &request("GET", "/", ""),
        50_000_000,
    );
    assert!(silent.error.unwrap().contains("did not answer"));
}
