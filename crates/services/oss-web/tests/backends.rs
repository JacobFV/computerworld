//! The apps' own Node backends answering over the service interface, as the world's
//! HTTP reaches them: the RealWorld API (Express + its Prisma queries) and JSON
//! Server (Express + lowdb), each keeping its database in the instance's files.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_node_app::NodeApp;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Instant;

fn app() -> NodeApp {
    NodeApp::new(cw_oss_web::packages().unwrap())
}

fn ctx(tick: u64) -> ServiceContext {
    ServiceContext {
        actor: "ada".into(),
        source: "workstation".into(),
        tick,
        seed: 7,
        instance: "api".into(),
    }
}

struct Site {
    app: NodeApp,
    state: Value,
    host: &'static str,
    tick: u64,
}

impl Site {
    fn new(initial: Value, host: &'static str) -> Site {
        let app = app();
        let state = app.initialize(initial, &ctx(0)).unwrap();
        Site {
            app,
            state,
            host,
            tick: 0,
        }
    }
    fn call(
        &mut self,
        method: &str,
        path: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> (u16, Value) {
        self.tick += 1_000_000;
        let mut headers = BTreeMap::new();
        headers.insert("accept".to_owned(), "application/json".to_owned());
        if body.is_some() {
            headers.insert("content-type".to_owned(), "application/json".to_owned());
        }
        if let Some(t) = token {
            headers.insert("authorization".to_owned(), format!("Token {t}"));
        }
        let req = HttpRequest {
            method: method.into(),
            url: format!("http://{}{path}", self.host),
            headers,
            body: body
                .map(|b| serde_json::to_vec(&b).unwrap())
                .unwrap_or_default(),
        };
        let t = Instant::now();
        let r = self
            .app
            .handle(&mut self.state, &ctx(self.tick), &req)
            .unwrap();
        eprintln!(
            "{method} {path}: {} in {:.1} ms",
            r.status,
            t.elapsed().as_secs_f64() * 1000.0
        );
        let text = String::from_utf8_lossy(&r.body).into_owned();
        let v = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (r.status, v)
    }
}

#[test]
fn the_realworld_api_registers_writes_and_reads_through_its_own_code() {
    let mut api = Site::new(json!({"package": "realworld-api"}), "api.realworld.show");
    let (status, v) = api.call(
        "POST",
        "/api/users",
        Some(json!({"user": {"username": "wren", "email": "wren@example.com", "password": "hunter22", "demo": true}})),
        None,
    );
    assert_eq!(status, 201, "{v}");
    let token = v["user"]["token"].as_str().unwrap().to_owned();
    let (status, v) = api.call(
        "POST",
        "/api/users/login",
        Some(json!({"user": {"email": "wren@example.com", "password": "hunter22"}})),
        None,
    );
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["user"]["username"], "wren");
    let (status, v) = api.call(
        "POST",
        "/api/articles",
        Some(json!({"article": {"title": "Retry logic considered harmful", "description": "d", "body": "b", "tagList": ["ops", "reliability"]}})),
        Some(&token),
    );
    assert_eq!(status, 201, "{v}");
    let slug = v["article"]["slug"].as_str().unwrap().to_owned();
    assert_eq!(v["article"]["tagList"], json!(["ops", "reliability"]));
    // A registered (not seeded) author's articles are listed to that author only:
    // the demo backend's own rule against spam.
    let (_, v) = api.call("GET", "/api/articles?limit=10&offset=0", None, None);
    assert_eq!(v["articlesCount"], 0);
    let (status, v) = api.call("GET", "/api/articles?limit=10&offset=0", None, Some(&token));
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["articlesCount"], 1);
    assert_eq!(v["articles"][0]["author"]["username"], "wren");
    let (status, v) = api.call("GET", "/api/tags", None, Some(&token));
    assert_eq!(
        (status, v["tags"].clone()),
        (200, json!(["ops", "reliability"]))
    );
    let (status, v) = api.call(
        "POST",
        &format!("/api/articles/{slug}/comments"),
        Some(json!({"comment": {"body": "Agreed."}})),
        Some(&token),
    );
    assert_eq!(status, 200, "{v}");
    let (status, v) = api.call(
        "POST",
        &format!("/api/articles/{slug}/favorite"),
        None,
        Some(&token),
    );
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["article"]["favoritesCount"], 1);
    let (status, _) = api.call(
        "DELETE",
        &format!("/api/articles/{slug}"),
        None,
        Some(&token),
    );
    assert_eq!(status, 204);
    let (_, v) = api.call("GET", "/api/articles", None, Some(&token));
    assert_eq!(v["articlesCount"], 0);
    // The database is the instance's file, and nothing else carried over.
    let db: Value =
        serde_json::from_str(api.state["files"]["/data/db.json"].as_str().unwrap()).unwrap();
    assert_eq!(db["User"][0]["username"], "wren");
    assert_eq!(db["Comment"], json!([]));
    assert_eq!(api.state["requests"], 10);
    let (status, _) = api.call("GET", "/images/smiley-cyrus.jpeg", None, None);
    assert_eq!(status, 200);
}

#[test]
fn json_server_serves_its_rest_api_over_a_seeded_file() {
    let mut site = Site::new(
        json!({"package": "json-server", "files": {"/data/db.json": {
            "posts": [{"id": 1, "title": "json-server", "author": "typicode"}],
            "comments": [{"id": 1, "body": "some comment", "postId": 1}],
            "profile": {"name": "typicode"}
        }}}),
        "json-server.typicode.com",
    );
    let (status, v) = site.call("GET", "/posts", None, None);
    assert_eq!(status, 200, "{v}");
    assert_eq!(v[0]["title"], "json-server");
    let (status, v) = site.call(
        "POST",
        "/posts",
        Some(json!({"title": "second", "author": "ada"})),
        None,
    );
    assert_eq!(status, 201, "{v}");
    assert_eq!(v["id"], 2);
    let (_, v) = site.call("GET", "/posts?author=ada", None, None);
    assert_eq!(v.as_array().unwrap().len(), 1);
    let (_, v) = site.call("GET", "/posts/1?_embed=comments", None, None);
    assert_eq!(v["comments"][0]["body"], "some comment");
    let (status, _) = site.call("DELETE", "/posts/1", None, None);
    assert_eq!(status, 200);
    let db: Value =
        serde_json::from_str(site.state["files"]["/data/db.json"].as_str().unwrap()).unwrap();
    assert_eq!(db["posts"].as_array().unwrap().len(), 1);
    let (status, home) = site.call("GET", "/", None, None);
    assert_eq!(status, 200);
    assert!(home.as_str().unwrap().contains("JSON Server"));
}
