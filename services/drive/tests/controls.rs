//! Every control on every page of every skin, followed to the end. The crawl starts at each
//! screen a drive can be entered by, walks each link it finds, submits each form and each
//! button with the values the page renders, and holds the service to three rules:
//!
//! * a link or a form must be answered by a route — never 404, never 405, never a 500;
//! * a button with nothing to fill in must succeed, or it should not have been drawn;
//! * a control must not point at the page it was clicked on, and a `#fragment` it offers must
//!   name something on the page it lands on.
//!
//! The two shipped seeds supply the state, a third synthetic one the `plain` skin, and each is
//! crawled by an owner and by a guest, with a star, a share link and a deletion applied first so
//! the starred, public and trash screens are reached with something on them.
use cw_protocol::{HttpRequest, HttpResponse};
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::{validate_strict, HTML_MEDIA_TYPE};
use cw_service_drive::{DriveService, DriveState};
use cw_web::dom::{Document, NodeId};
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};

const DRIVE: &str = include_str!("../../../worlds/company-2026/sites/google-drive.json");
const DROPBOX: &str = include_str!("../../../worlds/company-2026/sites/dropbox.json");
/// A crawl that runs away is a bug in the links, not a reason to wait.
const MAX_PAGES: usize = 600;
/// The sidebar marks the entry for the screen you are on; the real products link it anyway.
const SELF_LINKS_ALLOWED: [&str; 4] = ["nav-drive", "nav-shared", "nav-starred", "nav-trash"];

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 7,
        instance: "drive".into(),
    }
}
fn get(state: &mut Value, actor: &str, url: &str) -> HttpResponse {
    DriveService
        .handle(state, &ctx(actor), &HttpRequest::get(url))
        .unwrap()
}
fn post(state: &mut Value, actor: &str, url: &str, body: &str) -> HttpResponse {
    let mut request = HttpRequest::get(url);
    request.method = "POST".into();
    request
        .headers
        .insert("content-type".into(), "application/x-www-form-urlencoded".into());
    request.body = body.as_bytes().to_vec();
    DriveService.handle(state, &ctx(actor), &request).unwrap()
}
/// A drive to crawl: the state, who is looking, and where its pages live.
struct Site {
    label: String,
    host: String,
    actor: String,
    state: Value,
}
/// One thing on a page that can be acted on.
#[derive(Debug)]
struct Control {
    /// The id, or a description of an element that has none.
    what: String,
    method: &'static str,
    /// Site-relative, fragment included.
    target: String,
    /// Field name and rendered value, in the order the form holds them.
    fields: Vec<(String, String)>,
}
impl Control {
    /// The body a browser would send for this form.
    fn body(&self) -> String {
        fn encode(s: &str) -> String {
            let mut out = String::new();
            for b in s.bytes() {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
                    b' ' => out.push('+'),
                    _ => out.push_str(&format!("%{b:02X}")),
                }
            }
            out
        }
        self.fields
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    }
}

fn boot(site: &str, actor: &str, label: &str) -> Site {
    let definition: Value = serde_json::from_str(site).unwrap();
    let state = DriveService
        .initialize(definition["initial_state"].clone(), &ctx(actor))
        .expect("the shipped seed must load");
    Site {
        label: format!("{label}/{actor}"),
        host: definition["domains"][0].as_str().unwrap().to_owned(),
        actor: actor.into(),
        state,
    }
}
/// The unskinned drive, which no shipped world uses but the service still serves.
fn plain(actor: &str) -> Site {
    let seed = json!({
        "root": "root",
        "nodes": {
            "root": {"kind": "folder", "name": "My Drive", "owner": "alice"},
            "f-atlas": {"kind": "folder", "name": "Atlas", "parent": "root", "owner": "carol",
                        "shared_with": ["alice", "bob"]},
            "f-logo": {"kind": "file", "name": "atlas-logo.txt", "parent": "f-atlas",
                       "owner": "alice", "content": "Wordmark. See http://northstar.example/brand."},
            "d-launch": {"kind": "shortcut", "name": "Launch checklist", "parent": "f-atlas",
                         "owner": "carol", "shared_with": ["alice"],
                         "target_url": "http://docs.google.com/documents/atlas-launch"},
            "f-private": {"kind": "file", "name": "salary.txt", "parent": "root", "owner": "alice",
                          "content": "not for bob"}
        }
    });
    Site {
        label: format!("plain/{actor}"),
        host: "drive".to_owned(),
        actor: actor.into(),
        state: DriveService.initialize(seed, &ctx(actor)).unwrap(),
    }
}
/// Star something, mint a share link and delete something else, so the screens that only have
/// anything on them after a change are crawled with something on them.
fn stirred(mut site: Site, star: &str, link: &str, delete: &str) -> (Site, String) {
    let (actor, host) = (site.actor.clone(), site.host.clone());
    let at = |path: &str| format!("http://{host}/api{path}");
    assert_eq!(post(&mut site.state, &actor, &at(&format!("/nodes/{star}/star")), "").status, 200);
    let minted = post(&mut site.state, &actor, &at(&format!("/nodes/{link}/link")), "");
    assert_eq!(minted.status, 200, "{}", site.label);
    let token = serde_json::from_slice::<Value>(&minted.body).unwrap()["link"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(post(&mut site.state, &actor, &at(&format!("/nodes/{delete}/trash")), "").status, 200);
    (site, format!("/s/{token}"))
}

/// Every control an HTML page offers: its links, its forms with the values in them, and any
/// button that posts somewhere else of its own.
fn controls_of_html(doc: &Document, host: &str) -> Vec<Control> {
    // An element with no id is named by its class, which is how the brand anchor is known.
    let name_of = |n: NodeId, fallback: &str| {
        doc.attr(n, "id").map(str::to_owned).unwrap_or_else(|| {
            format!("{fallback}.{}", doc.attr(n, "class").unwrap_or("-"))
        })
    };
    let fields_of = |form: NodeId| {
        doc.descendants(form)
            .filter(|n| doc.is(*n, "input") || doc.is(*n, "textarea"))
            .filter(|n| doc.attr(*n, "type") != Some("submit"))
            .filter_map(|n| {
                let name = doc.attr(n, "name")?.to_owned();
                let value = match doc.is(n, "textarea") {
                    true => doc.text_content(n),
                    false => doc.attr(n, "value").unwrap_or_default().to_owned(),
                };
                Some((name, value))
            })
            .collect::<Vec<_>>()
    };
    let mut found = vec![];
    for node in doc.descendants(Document::ROOT) {
        if doc.is(node, "a") {
            let Some(href) = doc.attr(node, "href") else {
                panic!("an <a> with no href is not a link");
            };
            found.push(Control {
                what: name_of(node, "link"),
                method: "GET",
                target: href.to_owned(),
                fields: vec![],
            });
        }
        if doc.is(node, "form") {
            let action = doc.attr(node, "action").unwrap_or_default().to_owned();
            let method = match doc.attr(node, "method") {
                Some("post") => "POST",
                _ => "GET",
            };
            found.push(Control {
                what: name_of(node, "form"),
                method,
                target: action,
                fields: fields_of(node),
            });
        }
        // A button that posts elsewhere is its own control; the rest submit the form above.
        if doc.is(node, "button") {
            if let Some(action) = doc.attr(node, "formaction") {
                let form = doc
                    .descendants(Document::ROOT)
                    .find(|f| doc.is(*f, "form") && doc.descendants(*f).any(|n| n == node));
                found.push(Control {
                    what: name_of(node, "button"),
                    method: "POST",
                    target: action.to_owned(),
                    fields: form.map(fields_of).unwrap_or_default(),
                });
            }
        }
    }
    found.retain(|c| !c.target.starts_with("http") || c.target.starts_with(&format!("http://{host}/")));
    for c in &mut found {
        if let Some(rest) = c.target.strip_prefix(&format!("http://{host}")) {
            c.target = rest.to_owned();
        }
    }
    found
}
/// The same, for the `Page` JSON the plain skin still serves: every object carrying a `url`
/// is somewhere this page offers to go, and a form's `$field` references name its inputs.
fn controls_of_page(page: &Value) -> Vec<Control> {
    fn values(v: &Value, ids: &mut Vec<(String, String)>) {
        if let Some(map) = v.as_object() {
            if let (Some(id), Some(value)) = (map.get("id"), map.get("value")) {
                if let (Some(id), Some(value)) = (id.as_str(), value.as_str()) {
                    ids.push((id.to_owned(), value.to_owned()));
                }
            }
            for child in map.values() {
                values(child, ids);
            }
        }
        if let Some(list) = v.as_array() {
            for child in list {
                values(child, ids);
            }
        }
    }
    fn walk(v: &Value, id: &str, inputs: &[(String, String)], out: &mut Vec<Control>) {
        if let Some(map) = v.as_object() {
            let id = map.get("id").and_then(Value::as_str).unwrap_or(id);
            if let Some(url) = map.get("url").and_then(Value::as_str) {
                let method = match map.get("method").and_then(Value::as_str) {
                    Some(m) if m.eq_ignore_ascii_case("post") => "POST",
                    _ => "GET",
                };
                let fields = map
                    .get("fields")
                    .and_then(Value::as_object)
                    .map(|f| {
                        f.iter()
                            .map(|(key, reference)| {
                                let reference = reference.as_str().unwrap_or_default();
                                let value = reference
                                    .strip_prefix('$')
                                    .and_then(|r| inputs.iter().find(|(id, _)| id == r))
                                    .map(|(_, v)| v.clone())
                                    .unwrap_or_default();
                                (key.clone(), value)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(Control { what: id.to_owned(), method, target: url.to_owned(), fields });
            }
            for child in map.values() {
                walk(child, id, inputs, out);
            }
        }
        if let Some(list) = v.as_array() {
            for child in list {
                walk(child, id, inputs, out);
            }
        }
    }
    let mut inputs = vec![];
    values(page, &mut inputs);
    let mut out = vec![];
    walk(page, "", &inputs, &mut out);
    // The same action appears on a form and on its button; one visit answers for both.
    out.dedup_by(|a, b| a.method == b.method && a.target == b.target && a.fields == b.fields);
    out.retain(|c| c.target.starts_with('/') || c.target.starts_with('#'));
    out
}

/// Path and sorted query, so two spellings of one screen compare equal.
fn canonical(target: &str) -> String {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut params: Vec<&str> = query.split('&').filter(|p| !p.is_empty()).collect();
    params.sort_unstable();
    format!("{}?{}", path.trim_end_matches('/'), params.join("&"))
}

/// Walk one drive from `seeds` until nothing new is reachable.
fn crawl(site: &Site, seeds: &[&str]) -> usize {
    let mut queue: VecDeque<String> = seeds.iter().map(|s| (*s).to_owned()).collect();
    let mut seen: BTreeSet<String> = queue.iter().map(|t| canonical(t)).collect();
    // `page#id` pairs to check once the crawl has the page they name.
    let mut anchors: Vec<(String, String, String)> = vec![];
    let mut bodies: Vec<(String, String)> = vec![];
    let mut pages = 0;
    while let Some(path) = queue.pop_front() {
        let label = format!("{} {path}", site.label);
        let mut state = site.state.clone();
        let response = get(&mut state, &site.actor, &format!("http://{}{path}", site.host));
        assert!(
            ![404, 405].contains(&response.status) && response.status < 500,
            "{label}: answered {}",
            response.status
        );
        assert_eq!(state, site.state, "{label}: a GET must not write");
        pages += 1;
        assert!(pages <= MAX_PAGES, "{label}: the crawl is not converging");
        if response.status != 200 {
            continue;
        }
        let body = String::from_utf8(response.body.clone()).unwrap();
        let html = response.header("content-type") == Some(HTML_MEDIA_TYPE);
        let controls = if html {
            validate_strict(&body).unwrap_or_else(|e| panic!("{label}: {e:?}"));
            controls_of_html(&cw_web::html::parse(&body), &site.host)
        } else {
            controls_of_page(&serde_json::from_slice(&response.body).unwrap())
        };
        bodies.push((path.clone(), body));
        for control in controls {
            let (target, fragment) = match control.target.split_once('#') {
                Some((target, fragment)) => (target, Some(fragment.to_owned())),
                None => (control.target.as_str(), None),
            };
            let target = match target.is_empty() {
                true => path.clone(),
                false => target.to_owned(),
            };
            let what = format!("{label}: {} ({})", control.what, control.target);
            assert!(target.starts_with('/'), "{what} is not a path");
            let jump = fragment.is_some();
            if let Some(fragment) = fragment {
                anchors.push((target.clone(), fragment, what.clone()));
            }
            if control.method == "GET" {
                // A link back to the page it sits on is a control that does nothing — unless
                // it is a jump to a place on that page, which is a control that does something.
                assert!(
                    jump
                        || canonical(&target) != canonical(&path)
                        || SELF_LINKS_ALLOWED.contains(&control.what.as_str())
                        || control.what == "link.brand",
                    "{what} leads back to this same page"
                );
                let key = canonical(&target);
                if seen.insert(key) {
                    queue.push_back(target);
                }
                continue;
            }
            let mut state = site.state.clone();
            let url = format!("http://{}{target}", site.host);
            let status = post(&mut state, &site.actor, &url, &control.body()).status;
            assert!(
                ![404, 405].contains(&status) && status < 500,
                "{what} posts to nothing: {status}"
            );
            // A blank field earns a 400; nothing the actor could type earns a 403, so a form
            // drawn for someone who may not use it is a lie about what the page can do.
            assert_ne!(status, 403, "{what} was drawn for an actor who may not use it");
            // Nothing was left to fill in, so nothing the actor did can explain a refusal.
            if control.fields.is_empty() || control.fields.iter().all(|(_, v)| !v.is_empty()) {
                assert!(
                    (200..300).contains(&status),
                    "{what} was drawn but refused: {status}"
                );
            }
        }
    }
    // Every `#name` offered has to name something on the page it lands on.
    for (target, fragment, what) in anchors {
        let body = bodies
            .iter()
            .find(|(path, _)| canonical(path) == canonical(&target))
            .map(|(_, body)| body.clone())
            .unwrap_or_else(|| {
                let mut state = site.state.clone();
                let response = get(&mut state, &site.actor, &format!("http://{}{target}", site.host));
                String::from_utf8(response.body).unwrap()
            });
        assert!(
            !cw_web::html::parse(&body).by_id(&fragment).is_empty(),
            "{what} names #{fragment}, which is not on {target}"
        );
    }
    pages
}

/// The screens a drive is entered by. Everything else is reached by clicking.
const ENTRIES: [&str; 6] = [
    "/",
    "/shared-with-me",
    "/starred",
    "/trash",
    "/search?q=atlas",
    "/search",
];

#[test]
fn every_link_form_and_button_of_every_skin_is_answered_by_a_route() {
    let cases = [
        stirred(boot(DRIVE, "alice", "gdrive"), "press-kit", "benchmarks", "faq"),
        stirred(boot(DRIVE, "bob", "gdrive"), "press-kit", "benchmarks", "demo-script"),
        stirred(boot(DROPBOX, "carol", "dropbox"), "press-kit", "benchmarks", "faq"),
        stirred(boot(DROPBOX, "alice", "dropbox"), "press-kit", "benchmarks", "faq"),
        stirred(plain("alice"), "f-logo", "f-logo", "f-private"),
        stirred(plain("carol"), "f-atlas", "f-atlas", "d-launch"),
    ];
    let mut total = 0;
    for (site, public) in &cases {
        let mut entries: Vec<&str> = ENTRIES.to_vec();
        entries.push(public);
        // Every node the actor can see, by both of the routes that can name it.
        let s: DriveState = serde_json::from_value(site.state.clone()).unwrap();
        let mut routes: Vec<String> = vec![];
        for (id, node) in &s.nodes {
            if s.visible(&site.actor, id) {
                routes.push(match node.kind {
                    cw_service_drive::NodeKind::Folder => format!("/drive/folders/{id}"),
                    _ => format!("/file/{id}"),
                });
            }
        }
        entries.extend(routes.iter().map(String::as_str));
        let pages = crawl(site, &entries);
        assert!(pages >= entries.len(), "{}: {pages} pages", site.label);
        total += pages;
    }
    // A share link opens for someone with no grant at all, and that page has controls too.
    for (site, public) in &cases {
        let guest = Site {
            label: format!("{}-as-dana", site.label),
            host: site.host.clone(),
            actor: "dana".into(),
            state: site.state.clone(),
        };
        total += crawl(&guest, &[public]);
    }
    println!("{total} pages crawled");
}
