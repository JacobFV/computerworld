//! No dead controls. Every page of every skin is swept: every link is followed, every form
//! and `formaction` is probed against the service's own router, and every page is audited on
//! its own for the subtler lies — a decorative `<button>`, a field outside a form, a fake
//! `role`, an inert element drawn with a pointer cursor or lighting up under one.
//!
//! The sweep only sees this site. A link that leaves it is checked in the second test: it
//! has to name a host some site in this world actually answers on.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::audit::{controls, Sweep};
use cw_service_shop::ShopService;
use serde_json::Value;
use std::collections::BTreeSet;

const ACTORS: [&str; 2] = ["alice", "carol"];
const SITES: [&str; 8] = [
    "amazon",
    "ebay",
    "etsy",
    "airbnb",
    "booking",
    "uber",
    "doordash",
    "ticketmaster",
];
/// Where a sweep starts. Everything else is reached by following links.
const SEEDS: &[&str] = &["/", "/s", "/cart", "/orders", "/my-tickets", "/favorites"];
/// Links that may land on the page they sit on, because the real product's do too.
const ALLOW_SELF: &[&str] = &["wordmark"];

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 1,
        instance: "shop".into(),
    }
}
fn site(name: &str) -> Value {
    let path = cw_service_common::reference::reference_site_path(name);
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).unwrap()
}
fn load(name: &str) -> (String, Value) {
    let file = site(name);
    let host = file["domains"][0].as_str().unwrap().to_ascii_lowercase();
    let state = ShopService
        .initialize(file["initial_state"].clone(), &ctx("alice"))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    (host, state)
}
/// One request. A `POST` probe runs against a scratch copy, so a sweep that presses every
/// button on the site does not change what the next page it reads says.
fn caller<'a>(
    state: &'a mut Value,
    actor: &'a str,
    host: &'a str,
) -> impl FnMut(&str, &str) -> (u16, String) + 'a {
    move |method: &str, path: &str| {
        let mut request = HttpRequest::get(format!("http://{host}{path}"));
        let mut scratch;
        let state: &mut Value = if method == "POST" {
            request.method = "POST".into();
            request.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
            scratch = state.clone();
            &mut scratch
        } else {
            state
        };
        let reply = ShopService
            .handle(state, &ctx(actor), &request)
            .unwrap_or_else(|e| panic!("{method} {path}: {e}"));
        (
            reply.status,
            String::from_utf8(reply.body).unwrap_or_default(),
        )
    }
}

#[test]
fn no_page_of_any_seeded_site_draws_a_control_that_cannot_act() {
    for name in SITES {
        let (host, seed) = load(name);
        for actor in ACTORS {
            let mut state = seed.clone();
            let mut call = caller(&mut state, actor, &host);
            let faults = Sweep::new(SEEDS, &mut call).allow_self(ALLOW_SELF).run();
            assert!(
                faults.is_empty(),
                "{name} as {actor}:\n  {}",
                faults.join("\n  ")
            );
        }
    }
}

/// Every host the world answers on, so a link off this site is checked to lead somewhere.
fn world_domains() -> BTreeSet<String> {
    // The reference company's own sites, and the internet it joins.
    let dirs = ["company-2026", "internet"]
        .map(|w| format!("{}/../../../worlds/{w}/services", env!("CARGO_MANIFEST_DIR")));
    let mut out = BTreeSet::new();
    for entry in dirs
        .iter()
        .flat_map(|dir| std::fs::read_dir(dir).expect(dir).flatten())
    {
        let Ok(text) = std::fs::read_to_string(entry.path().join("service.json")) else {
            continue;
        };
        let Ok(file) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        for domain in file["domains"].as_array().into_iter().flatten() {
            if let Some(d) = domain.as_str() {
                out.insert(d.to_ascii_lowercase());
            }
        }
    }
    assert!(out.len() > 20, "the world's site files must be readable");
    out
}

/// The pages that carry a link off the site (an event's map link) and the ones that carry
/// the controls: every product, the cart, the orders and the favourites.
fn off_site_pages(seed: &Value) -> Vec<String> {
    let mut out: Vec<String> = SEEDS.iter().map(|s| (*s).to_owned()).collect();
    for (id, _) in seed["products"].as_object().into_iter().flatten() {
        out.push(format!("/event/{id}"));
        out.push(format!("/dp/{id}"));
    }
    out
}

#[test]
fn a_link_that_leaves_the_site_names_a_host_this_world_answers_on() {
    let domains = world_domains();
    let mut checked = 0;
    for name in SITES {
        let (host, seed) = load(name);
        let mut state = seed.clone();
        let mut call = caller(&mut state, "alice", &host);
        for path in off_site_pages(&seed) {
            let (status, body) = call("GET", &path);
            if status != 200 {
                continue;
            }
            for control in controls(&cw_web::html::parse(&body)) {
                let Some(rest) = control
                    .target
                    .strip_prefix("http://")
                    .or_else(|| control.target.strip_prefix("https://"))
                else {
                    continue;
                };
                let authority = rest.split('/').next().unwrap_or("").to_ascii_lowercase();
                assert!(
                    domains.contains(&authority),
                    "{name} {path}: #{} points at {}, and no site in this world answers on {authority}",
                    control.id,
                    control.target
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked > 0,
        "no off-site link was checked: the seeds must carry some"
    );
}

/// One request, with a body when it is a `POST`.
fn request(
    state: &mut Value,
    actor: &str,
    host: &str,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, String) {
    let mut r = HttpRequest::get(format!("http://{host}{path}"));
    if method == "POST" {
        r.method = "POST".into();
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        r.body = body.as_bytes().to_vec();
    }
    let reply = ShopService
        .handle(state, &ctx(actor), &r)
        .unwrap_or_else(|e| panic!("{method} {path}: {e}"));
    (
        reply.status,
        String::from_utf8(reply.body).unwrap_or_default(),
    )
}

/// A control with nothing to fill in — like, follow, vote, join, save, remove, checkout —
/// must not merely avoid a 404: pressing it has to work. Every form whose fields are all
/// hidden or already filled is submitted with exactly what the page put in it, on a scratch
/// copy of the state, and the answer has to be a page rather than a refusal.
#[test]
fn every_control_that_needs_no_typing_works_when_it_is_pressed() {
    let mut pressed = 0;
    for name in SITES {
        let (host, seed) = load(name);
        for actor in ACTORS {
            let mut state = seed.clone();
            for path in off_site_pages(&seed) {
                let (status, body) = request(&mut state, actor, &host, "GET", &path, "");
                if status != 200 || !body.starts_with("<!DOCTYPE html>") {
                    continue;
                }
                for (id, method, action, fields) in ready_forms(&body) {
                    let mut scratch = state.clone();
                    let target = if method == "GET" && !fields.is_empty() {
                        format!("{action}?{fields}")
                    } else {
                        action.clone()
                    };
                    let (status, answer) =
                        request(&mut scratch, actor, &host, &method, &target, &fields);
                    assert!(
                        status < 400,
                        "{name} as {actor}, {path}: pressing #{id} ({method} {action}) answers {status}: {}",
                        answer.chars().take(200).collect::<String>()
                    );
                    pressed += 1;
                }
            }
        }
    }
    assert!(
        pressed > 20,
        "only {pressed} controls were pressed: the walk found too little"
    );
}

/// The forms on a page that need nothing typed into them: every field is hidden, or a text
/// field the page already filled. `(id, method, action, encoded fields)`.
fn ready_forms(body: &str) -> Vec<(String, String, String, String)> {
    let doc = cw_web::html::parse(body);
    let mut out = Vec::new();
    for node in doc.descendants(cw_web::dom::Document::ROOT) {
        if !doc.is(node, "form") {
            continue;
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut waiting = false;
        for field in doc.descendants(node) {
            let tag = doc.tag(field).unwrap_or_default();
            if !matches!(tag, "input" | "textarea" | "select") {
                continue;
            }
            let value = match tag {
                "textarea" => doc.text_content(field),
                _ => doc.attr(field, "value").unwrap_or_default().to_owned(),
            };
            if doc.attr(field, "type") != Some("hidden") && value.trim().is_empty() {
                waiting = true;
            }
            if let Some(name) = doc.attr(field, "name") {
                fields.push((name.to_owned(), value));
            }
        }
        if waiting {
            continue;
        }
        let action = doc.attr(node, "action").unwrap_or_default();
        if action.is_empty() || !action.starts_with('/') {
            continue;
        }
        out.push((
            doc.attr(node, "id").unwrap_or_default().to_owned(),
            doc.attr(node, "method")
                .unwrap_or("get")
                .to_ascii_uppercase(),
            action.to_owned(),
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .finish(),
        ));
    }
    out
}
