pub use crate::script::{LogLevel, MemoryHost, Realm};

/// The host the area tests share: a few canned URLs and in-memory storage.
pub fn default_host() -> MemoryHost {
    MemoryHost::new()
        .with_response("https://example.test/api/data", "application/json", "{\"n\":7,\"s\":\"str\"}")
        .with_response("https://example.test/s.css", "text/css", "#d { color: red }")
        .with_response("https://example.test/ext.js", "text/javascript", "console.log('EXT'); window.ext = 1;")
        .with_response("https://example.test/pic.png", "image/png", "")
        .with_response("https://example.test/x", "text/plain", "x")
        .with_response("https://example.test/echo", "text/plain", "echoed")
}

/// A realm for `html` at a fixed URL, with the page's scripts run and the event
/// loop drained (up to a second of virtual time).
pub fn realm(html: &str) -> Realm {
    realm_with(html, default_host())
}

pub fn realm_with(html: &str, host: MemoryHost) -> Realm {
    let mut r = Realm::new(html, "https://example.test/page.html", Box::new(host));
    r.run_document();
    r.run_until_idle(1000);
    r
}

/// Runs `script` in a realm whose body holds `body_html`; returns the realm.
pub fn run(body_html: &str, script: &str) -> Realm {
    let html = format!("<!DOCTYPE html><html><head><title>t</title></head><body>{body_html}<script>{script}</script></body></html>");
    realm(&html)
}

/// Console output lines (log level), joined with newlines.
pub fn logs(r: &Realm) -> String {
    r.logs().iter().filter(|l| l.level == LogLevel::Log || l.level == LogLevel::Info).map(|l| l.text.clone()).collect::<Vec<_>>().join("\n")
}

/// Console error lines.
pub fn errors(r: &Realm) -> String {
    r.logs().iter().filter(|l| l.level == LogLevel::Error || l.level == LogLevel::Warn).map(|l| l.text.clone()).collect::<Vec<_>>().join("\n")
}

/// Runs `script` and returns what it logged; panics on a page error.
pub fn eval_logs(body_html: &str, script: &str) -> String {
    let r = run(body_html, script);
    let e = errors(&r);
    assert!(e.is_empty(), "page errors: {e}\nlogs: {}", logs(&r));
    logs(&r)
}

/// The body's `innerHTML` after the scripts ran.
pub fn body_html(r: &Realm) -> String {
    let doc = r.document();
    let body = doc.body().expect("body");
    crate::html::serialize(&doc, body)
}

/// A test that runs `script` against `body` and compares the console output.
#[macro_export]
macro_rules! check {
    ($name:ident, $body:expr, $script:expr, $expect:expr) => {
        #[test]
        fn $name() {
            let out = $crate::script::tests::eval_logs($body, $script);
            assert_eq!(out, $expect);
        }
    };
}
