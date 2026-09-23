//! No dead controls. Every page of every skin is walked from `/`, and everything the page
//! offers to press is followed: each `<a href>` is fetched, each `<form>` is submitted with
//! the fields it carries, and each `<button>` is checked to be inside one. A 404 or a 405
//! fails, and so does a link back to the page it is on, a button that belongs to no form,
//! an anchor with no id for an agent to click, and an `href` that goes nowhere (`#`).
mod support;
use cw_protocol::{HttpRequest, HttpResponse};
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::href;
use cw_service_issues::IssuesService;
use cw_web::dom::{Document, NodeId};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const ACTOR: &str = "alice";

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: ACTOR.into(),
        source: "alice-mac".into(),
        tick: 60,
        seed: 1,
        instance: "issues".into(),
    }
}
fn get(state: &mut Value, host: &str, path: &str) -> HttpResponse {
    IssuesService
        .handle(
            state,
            &ctx(),
            &HttpRequest::get(format!("http://{host}{path}")),
        )
        .unwrap()
}

/// One form as the page offers it: where it goes, how, and what it would carry.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Form {
    page: String,
    id: String,
    action: String,
    method: String,
    fields: Vec<(String, String)>,
}
impl Form {
    /// The request a browser would send when its submit button is pressed.
    fn request(&self, host: &str) -> HttpRequest {
        let pairs: Vec<(&str, &str)> = self
            .fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        if self.method == "get" {
            return HttpRequest::get(format!("http://{host}{}", href(&self.action, &pairs)));
        }
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs.iter().copied())
            .finish();
        let mut req = HttpRequest {
            method: "POST".into(),
            url: format!("http://{host}{}", self.action),
            headers: BTreeMap::new(),
            body: body.into_bytes(),
        };
        req.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        req
    }
}

/// A `<form>`'s fields, with an empty text field filled the way a person would fill it, so
/// the submission exercises the route rather than bouncing off its validation.
fn fields(doc: &Document, form: NodeId) -> Vec<(String, String)> {
    let mut out = vec![];
    for node in doc.descendants(form) {
        let tag = match doc.tag(node) {
            Some(t @ ("input" | "textarea" | "select")) => t,
            _ => continue,
        };
        let Some(name) = doc.attr(node, "name") else {
            continue;
        };
        let value = doc.attr(node, "value").unwrap_or("");
        let hidden = tag == "input" && doc.attr(node, "type") == Some("hidden");
        let value = if value.is_empty() && !hidden {
            "probe"
        } else {
            value
        };
        out.push((name.to_owned(), value.to_owned()));
    }
    out
}

/// Walks one skin of one seed from `/`, fetching every link it finds and collecting every
/// form and button on the way. Returns the forms, for submission against a clean state.
fn crawl(label: &str, host: &str, state: &mut Value) -> BTreeSet<Form> {
    let mut queue: VecDeque<String> = VecDeque::from(["/".to_owned()]);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut forms: BTreeSet<Form> = BTreeSet::new();
    let mut pages = 0usize;
    while let Some(path) = queue.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let response = get(state, host, &path);
        assert_eq!(response.status, 200, "{label}: GET {path}");
        let html = String::from_utf8(response.body).unwrap();
        // The shared audit over the same page, alongside the checks below: a control with
        // nowhere to go, an `onclick` in a world with no script, a role or a tabindex on
        // something inert, a field with no name or label, a fragment that is not here —
        // and, from the engine's own cascade, an inert element with a pointer cursor, a
        // hover state, or the painted box this page's own controls wear.
        if let Some(fault) = cw_service_common::audit::page(&html)
            .into_iter()
            .chain(cw_service_common::audit::clothes(&html))
            .next()
        {
            panic!("{label} {path}: {fault}");
        }
        let page = support::Page::parse(&format!("{label} {path}"), html);
        pages += 1;
        let doc = &page.doc;
        for node in doc.descendants(Document::ROOT) {
            let Some(tag) = doc.tag(node) else { continue };
            let id = doc.attr(node, "id").unwrap_or("");
            match tag {
                "a" => {
                    let target = doc.attr(node, "href").unwrap_or("");
                    assert!(
                        !id.is_empty(),
                        "{label} {path}: an <a href={target:?}> has no id for an agent to click"
                    );
                    assert!(
                        !target.is_empty() && target != "#" && !target.starts_with("javascript:"),
                        "{label} {path}: #{id} goes nowhere ({target:?})"
                    );
                    if target.starts_with("http://") || target.starts_with("https://") {
                        continue; // Another site's page; the world's link test covers those.
                    }
                    assert!(
                        target.starts_with('/'),
                        "{label} {path}: #{id} has a relative href {target:?}"
                    );
                    let current = doc
                        .attr(node, "class")
                        .unwrap_or("")
                        .split_ascii_whitespace()
                        .any(|c| c == "current");
                    assert!(
                        target != path
                            || id == "workspace"
                            || id == "home"
                            || (current
                                && (id.starts_with("nav-")
                                    || id.starts_with("view-")
                                    || id.starts_with("filter-"))),
                        "{label} {path}: #{id} links to the page it is on"
                    );
                    queue.push_back(target.to_owned());
                }
                "button" => {
                    let owner = doc.ancestors(node).find(|a| doc.is(*a, "form"));
                    assert!(
                        owner.is_some() || doc.has_attr(node, "formaction"),
                        "{label} {path}: button #{id} is in no form and has no formaction, so pressing it does nothing"
                    );
                }
                "input" | "textarea" | "select" => {
                    assert!(
                        doc.ancestors(node).any(|a| doc.is(a, "form")),
                        "{label} {path}: #{id} is a field of no form"
                    );
                }
                "form" => {
                    assert!(!id.is_empty(), "{label} {path}: a <form> has no id");
                    forms.insert(Form {
                        page: path.clone(),
                        id: id.to_owned(),
                        action: doc.attr(node, "action").unwrap_or_default().to_owned(),
                        method: doc
                            .attr(node, "method")
                            .unwrap_or("get")
                            .to_ascii_lowercase(),
                        fields: fields(doc, node),
                    });
                    assert!(
                        doc.descendants(node)
                            .any(|n| doc.is(n, "button") || doc.attr(n, "type") == Some("submit")),
                        "{label} {path}: form #{id} has no button to submit it"
                    );
                }
                _ => {}
            }
        }
    }
    println!("{label}: {pages} pages, {} forms", forms.len());
    assert!(pages > 3, "{label}: the crawl found almost nothing");
    forms
}

/// Every form of every page, submitted against its own copy of the state: none of them may
/// answer 404 (the route is not there) or 405 (the route is there but not for this method).
fn submit_all(label: &str, host: &str, state: &Value, forms: &BTreeSet<Form>) {
    let mut counts: BTreeMap<u16, usize> = BTreeMap::new();
    for form in forms {
        let mut copy = state.clone();
        let status = IssuesService
            .handle(&mut copy, &ctx(), &form.request(host))
            .unwrap()
            .status;
        *counts.entry(status).or_default() += 1;
        assert!(
            status != 404 && status != 405 && status < 500,
            "{label} {}: form #{} ({} {}) answers {status}",
            form.page,
            form.id,
            form.method,
            form.action
        );
    }
    println!("{label}: {} submissions {counts:?}", forms.len());
}

/// The shipped seed, and a synthetic one for the page shapes the seed does not have: an
/// empty team, an unassigned issue with no labels and no body, a pull request with a review,
/// and a team this actor may not read (whose pages must never be linked to).
fn seeds() -> Vec<(String, Value)> {
    let raw = std::fs::read_to_string("../../../worlds/internet/sites/linear.json").unwrap();
    let site: Value = serde_json::from_str(&raw).unwrap();
    let synthetic = json!({"workspace":"Northstar","projects":{
        "EMPTY":{"name":"Empty","issues":{}},
        "ODD":{"name":"Odd","issues":{
            "1":{"id":1,"title":"Unassigned, unlabelled, undescribed"},
            "2":{"id":2,"title":"Ship the parser","body":"See http://git.internal/northstar/atlas for the branch.",
                 "status":"in_progress","kind":"pull_request","author":"bob","assignee":"alice",
                 "labels":["p0"],"source_ref":"refs/heads/parser","target_ref":"refs/heads/main",
                 "comments":[{"author":"carol","body":"Reads well.","tick":4}],
                 "reviews":[{"author":"carol","decision":"approve","body":"ship it","tick":5}]},
            "3":{"id":3,"title":"Closed and done","status":"closed","author":"bob","assignee":"bob"}}},
        "SECRET":{"name":"Secret","readers":["zed"],"writers":["zed"],"issues":{
            "1":{"id":1,"title":"Not for alice"}}}}});
    let mut out = vec![];
    for (name, initial) in [
        ("seed", site["initial_state"].clone()),
        ("synthetic", synthetic),
    ] {
        for skin in ["linear", "plain"] {
            let mut initial = initial.clone();
            initial["skin"] = skin.into();
            out.push((format!("{name}/{skin}"), initial));
        }
    }
    out
}

#[test]
fn every_link_form_and_button_of_every_page_of_every_skin_leads_somewhere() {
    for (label, initial) in seeds() {
        let host = if label.ends_with("linear") {
            "linear.app"
        } else {
            "issues.internal"
        };
        let clean = IssuesService.initialize(initial, &ctx()).unwrap();
        let mut state = clean.clone();
        let forms = crawl(&label, host, &mut state);
        // Reading the world never changes it, so the crawl's state is still the seed's.
        assert_eq!(state, clean, "{label}: a GET changed the world");
        submit_all(&label, host, &clean, &forms);
        // The crawl reached the sidebar's search and never the team it may not read.
        assert!(
            forms
                .iter()
                .any(|f| f.id == "search" && f.action == "/search"),
            "{label}: no search form"
        );
        assert_eq!(
            get(&mut state, host, "/projects/SECRET").status,
            if label.starts_with("synthetic") {
                403
            } else {
                404
            }
        );
    }
}

/// The search the sidebar offers is a real query over the issues that exist, in both skins,
/// and "My issues" is that same page filtered to the reader.
#[test]
fn the_search_box_finds_issues_and_my_issues_is_the_reader_s_own() {
    let raw = std::fs::read_to_string("../../../worlds/internet/sites/linear.json").unwrap();
    let site: Value = serde_json::from_str(&raw).unwrap();
    for skin in ["linear", "plain"] {
        let mut initial = site["initial_state"].clone();
        initial["skin"] = skin.into();
        let mut state = IssuesService.initialize(initial, &ctx()).unwrap();
        let host = if skin == "linear" {
            "linear.app"
        } else {
            "issues.internal"
        };
        let page = |state: &mut Value, path: &str| {
            let r = get(state, host, path);
            assert_eq!(r.status, 200, "{skin} {path}");
            support::Page::parse(path, String::from_utf8(r.body).unwrap())
        };
        // An empty box is an invitation, not a result list.
        let empty = page(&mut state, "/search");
        assert!(
            empty.all_text().contains("Type a word into the search box"),
            "{skin}: {}",
            empty.all_text()
        );
        assert!(empty.ids_with_prefix("result-").is_empty());
        // A word out of OPS-1's title finds OPS-1 and nothing that does not match.
        let found = page(&mut state, "/search?q=checklist");
        assert_eq!(found.attr("result-OPS-1", "href"), "/projects/OPS/issues/1");
        assert!(!found.has("result-OPS-7"));
        assert_eq!(
            found.attr("search-q", "value"),
            "checklist",
            "{skin}: the box forgot the query"
        );
        // Nothing matches: the page says so rather than showing an empty list.
        assert!(page(&mut state, "/search?q=zzzzz")
            .all_text()
            .contains("No issue match"));
        // My issues: everything assigned to the reader, across every team.
        let mine = page(&mut state, "/search?assignee=alice");
        assert!(mine.all_text().contains("Issues assigned to alice"));
        let ids = mine.ids_with_prefix("result-");
        assert!(
            ids.len() > 1 && ids.contains(&"result-OPS-1".to_owned()),
            "{skin}: {ids:?}"
        );
        for id in &ids {
            let (key, number) = id.trim_start_matches("result-").split_once('-').unwrap();
            assert_eq!(
                state["projects"][key]["issues"][number]["assignee"],
                "alice"
            );
        }
        // The rail offers it by that name, and the search route refuses a write.
        if skin == "linear" {
            assert_eq!(
                page(&mut state, "/").attr("nav-mine", "href"),
                "/search?assignee=alice"
            );
        }
        let mut post = HttpRequest::get(format!("http://{host}/search"));
        post.method = "POST".into();
        assert_eq!(
            IssuesService
                .handle(&mut state, &ctx(), &post)
                .unwrap()
                .status,
            405
        );
        // The JSON side answers the same question.
        let api = get(&mut state, host, "/api/search?q=checklist");
        assert_eq!(api.status, 200);
        let listed: Value = serde_json::from_slice(&api.body).unwrap();
        let hits: Vec<String> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|h| format!("{}-{}", h["project"].as_str().unwrap(), h["issue"]["id"]))
            .collect();
        assert!(hits.contains(&"OPS-1".to_owned()), "{skin}: {hits:?}");
    }
}
