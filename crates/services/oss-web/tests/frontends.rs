//! The apps' frontends booting on the engine's script layer (`cw_web::script::Realm`)
//! against their sites served by `node-app`, without the rest of the world: a quick
//! check that each app renders its first screen with no page errors, and a place to
//! see what a page logged when one does not. The end-to-end flows through the
//! world's browser are in crates/computerworld/tests/oss_webapps.rs.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_node_app::NodeApp;
use cw_web::script::{
    FetchRequest, FetchResponse, LogLevel, Realm, ScriptHostDocument, StorageArea,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

struct Sites {
    app: NodeApp,
    by_host: BTreeMap<String, Value>,
    tick: u64,
}

#[derive(Clone)]
struct Host {
    sites: Rc<RefCell<Sites>>,
    storage: Rc<RefCell<BTreeMap<String, String>>>,
    logs: Rc<RefCell<Vec<String>>>,
    /// Microseconds past the world epoch, moved on by `settle` as the world's
    /// clock moves while a person looks at the page.
    clock: Rc<std::cell::Cell<i64>>,
}

impl ScriptHostDocument for Host {
    fn fetch(&mut self, request: &FetchRequest) -> Result<FetchResponse, String> {
        let url = url::Url::parse(&request.url).map_err(|e| e.to_string())?;
        let host = url.host_str().unwrap_or_default().to_owned();
        let mut sites = self.sites.borrow_mut();
        sites.tick += 1000;
        let tick = sites.tick + self.clock.get() as u64;
        let Sites { app, by_host, .. } = &mut *sites;
        let Some(state) = by_host.get_mut(&host) else {
            return Err(format!("getaddrinfo ENOTFOUND {host}"));
        };
        let req = HttpRequest {
            method: request.method.clone(),
            url: request.url.clone(),
            headers: request.headers.iter().cloned().collect(),
            body: request.body.clone().unwrap_or_default(),
        };
        let ctx = ServiceContext {
            actor: "ada".into(),
            source: "workstation".into(),
            tick,
            seed: 3,
            instance: host.clone(),
        };
        let r = app.handle(state, &ctx, &req).map_err(|e| e.to_string())?;
        Ok(FetchResponse {
            status: r.status,
            status_text: String::new(),
            headers: r.headers.into_iter().collect(),
            body: r.body,
            url: request.url.clone(),
        })
    }
    fn now_micros(&self) -> i64 {
        1_789_635_600_000_000 + self.clock.get()
    }
    fn storage_get(&self, _: StorageArea, key: &str) -> Option<String> {
        self.storage.borrow().get(key).cloned()
    }
    fn storage_set(&mut self, _: StorageArea, key: &str, value: &str) {
        self.storage.borrow_mut().insert(key.into(), value.into());
    }
    fn storage_remove(&mut self, _: StorageArea, key: &str) {
        self.storage.borrow_mut().remove(key);
    }
    fn storage_keys(&self, _: StorageArea) -> Vec<String> {
        self.storage.borrow().keys().cloned().collect()
    }
    fn log(&mut self, level: LogLevel, text: &str) {
        self.logs.borrow_mut().push(format!("{level:?}: {text}"));
    }
}

fn sites() -> Host {
    let app = NodeApp::new(cw_oss_web::packages().unwrap());
    let ctx = ServiceContext {
        actor: "ada".into(),
        source: "workstation".into(),
        tick: 0,
        seed: 3,
        instance: "site".into(),
    };
    let mut by_host = BTreeMap::new();
    for (host, state) in [
        ("todomvc.com", json!({"package": "todomvc"})),
        (
            "conduit.realworld.show",
            json!({"package": "conduit-react"}),
        ),
        ("vue.realworld.show", json!({"package": "conduit-vue"})),
        ("api.realworld.show", json!({"package": "realworld-api"})),
        (
            "react-admin.marmelab.com",
            json!({"package": "react-admin"}),
        ),
        (
            "json-server.typicode.com",
            json!({"package": "json-server", "files": {"/data/db.json": {"posts": [{"id": 1, "title": "hello"}]}}}),
        ),
    ] {
        by_host.insert(host.to_owned(), app.initialize(state, &ctx).unwrap());
    }
    Host {
        sites: Rc::new(RefCell::new(Sites {
            app,
            by_host,
            tick: 0,
        })),
        storage: Rc::default(),
        logs: Rc::default(),
        clock: Rc::default(),
    }
}

/// Loads `url` as the browser would: the document, then its scripts and their
/// imports through the host, then the event loop until it settles.
fn boot(url: &str) -> (Realm, Host, f64) {
    let mut host = sites();
    let t = Instant::now();
    let page = host
        .fetch(&FetchRequest {
            url: url.into(),
            method: "GET".into(),
            headers: vec![("accept".into(), "text/html".into())],
            body: None,
        })
        .unwrap();
    assert_eq!(page.status, 200, "{url}");
    let html = String::from_utf8(page.body).unwrap();
    let mut realm = Realm::new(&html, url, Box::new(host.clone()));
    realm.run_document();
    settle(&mut realm, &host);
    (realm, host, t.elapsed().as_secs_f64() * 1000.0)
}

/// Runs timers and animation frames as the browser does between actions, until a
/// whole virtual second passes with nothing to do (so a fake provider's latency,
/// or a debounce, has passed; a far-off timer such as a cache's collection does
/// not keep it going).
fn settle(realm: &mut Realm, host: &Host) {
    let mut quiet = 0;
    for _ in 0..300 {
        host.clock.set(host.clock.get() + 100_000);
        let busy = realm.run_until_idle(100);
        realm.after_layout();
        let frame = realm.wants_animation_frame();
        if frame {
            realm.animation_frame();
        }
        quiet = if busy || frame { 0 } else { quiet + 1 };
        if quiet >= 10 {
            break;
        }
    }
}

fn text(realm: &mut Realm) -> String {
    realm
        .eval("document.body ? document.body.innerText : ''")
        .unwrap_or_default()
}

fn check(url: &str, expect: &[&str]) {
    let (mut realm, host, ms) = boot(url);
    let body = text(&mut realm);
    let logs = host.logs.borrow().clone();
    let errors: Vec<&String> = logs.iter().filter(|l| l.starts_with("Error")).collect();
    eprintln!(
        "{url}: booted in {ms:.0} ms\n--- text\n{}\n--- logs\n{}",
        &body[..body.len().min(1500)],
        logs.join("\n")
    );
    for e in expect {
        assert!(body.contains(e), "{url}: `{e}` is not on the page");
    }
    assert!(errors.is_empty(), "{url}: page errors {errors:?}");
}

#[test]
fn todomvc_react_renders() {
    check(
        "http://todomvc.com/examples/react/dist/",
        &["todos", "Double-click to edit a todo"],
    );
}

#[test]
fn todomvc_vue_renders() {
    check(
        "http://todomvc.com/examples/vue/dist/",
        &["todos", "Double-click to edit a todo"],
    );
}

#[test]
fn conduit_react_renders() {
    check(
        "http://conduit.realworld.show/",
        &["conduit", "Global Feed"],
    );
}

#[test]
fn conduit_vue_renders() {
    check("http://vue.realworld.show/", &["conduit", "Global Feed"]);
}

#[test]
fn react_admin_renders() {
    check(
        "http://react-admin.marmelab.com/",
        &["Posts", "Published at", "Nb comments"],
    );
}

#[test]
fn json_server_home_renders() {
    check("http://json-server.typicode.com/", &["Congrats", "/posts"]);
}

/// Development aid: `OSS_URL=http://... OSS_EVAL='js' cargo test -p cw-oss-web --test
/// frontends debug_page -- --ignored --nocapture` boots a page and prints the value of
/// each `;;`-separated expression.
#[test]
#[ignore]
fn debug_page() {
    let url = std::env::var("OSS_URL").unwrap();
    let (mut realm, host, ms) = boot(&url);
    eprintln!(
        "booted in {ms:.0} ms; logs:\n{}",
        host.logs.borrow().join("\n")
    );
    eprintln!(
        "next timer: {:?}; wants frame: {}",
        realm.next_timer_micros(),
        realm.wants_animation_frame()
    );
    for _ in 0..std::env::var("OSS_MORE")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
    {
        let busy = realm.run_until_idle(1000);
        eprintln!(
            "  more: busy {busy}, next timer {:?}",
            realm.next_timer_micros()
        );
    }
    for expr in std::env::var("OSS_EVAL").unwrap_or_default().split(";;") {
        if expr.trim().is_empty() {
            continue;
        }
        eprintln!("> {expr}\n{:?}", realm.eval(expr));
    }
}
