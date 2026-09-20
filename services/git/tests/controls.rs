//! No dead controls. Every page of every skin is walked, and everything on it that
//! invites a click is followed: links, GET forms, POST forms and the buttons that carry
//! their own `formaction`. A target that 404s or 405s is a lie to the person reading the
//! page and to the agent reading the semantic tree, so each one fails this test.
//!
//! It also holds the two rules that catch a control which is only dressed as one: a
//! button or a field outside any form submits nowhere, and a link back to the page it is
//! already on does nothing unless it is the one marking where you are.
mod support;
use cw_protocol::{HttpRequest, HttpResponse};
use cw_sdk::{Service, ServiceContext};
use cw_service_git::GitService;
use cw_web::dom::{Document, NodeId};
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};
use support::Page;

const ACTOR: &str = "alicechen";

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext { actor: actor.into(), source: format!("{actor}-pc"), tick: 240, seed: 5, instance: "github".into() }
}
fn request(state: &mut Value, method: &str, url: &str, body: &[(String, String)]) -> HttpResponse {
    let mut req = HttpRequest::get(format!("http://github.com{url}"));
    req.method = method.into();
    if method == "POST" {
        req.headers.insert("content-type".into(), "application/x-www-form-urlencoded".into());
        req.body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(body.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish()
            .into_bytes();
    }
    GitService.handle(state, &ctx(ACTOR), &req).unwrap()
}
fn seeded(site: &str, skin: &str) -> Value {
    let raw = std::fs::read_to_string(format!("../../worlds/company-2026/sites/{site}.json")).unwrap();
    let mut site: Value = serde_json::from_str(&raw).unwrap();
    site["initial_state"]["skin"] = skin.into();
    GitService.initialize(site["initial_state"].clone(), &ctx(ACTOR)).unwrap()
}

/// A control the page paints: where it points, how it gets there, and what it carries.
struct Control {
    id: String,
    method: &'static str,
    target: String,
    fields: Vec<(String, String)>,
}
/// The chrome that points at the page it is on because the real product's does too: the
/// mark goes home from the home page, the header's icons stay lit on their own dashboards,
/// and the repository header keeps naming the repository and counting its people.
const SELF_LINKS: &[&str] = &["mark", "home", "repo-name", "nav-issues", "nav-pulls", "nav-inbox", "nav-profile", "stargazers", "watch-count"];
/// Is this the link that marks where you already are? Those are allowed to point at the
/// page they are on; anything else that does is a control with no effect.
fn marks_here(doc: &Document, node: NodeId) -> bool {
    doc.attr(node, "id").is_some_and(|id| SELF_LINKS.contains(&id))
        || doc.attr(node, "aria-current").is_some()
        || doc
            .attr(node, "class")
            .is_some_and(|c| c.split_ascii_whitespace().any(|k| ["active", "last", "here"].contains(&k)))
}
/// The form an element submits with: its action, its method and its fields.
fn owning_form(doc: &Document, node: NodeId) -> Option<(NodeId, String, String)> {
    let form = doc.ancestors(node).find(|a| doc.is(*a, "form"))?;
    Some((
        form,
        doc.attr(form, "action").unwrap_or("").to_owned(),
        doc.attr(form, "method").unwrap_or("get").to_ascii_lowercase(),
    ))
}
fn fields_of(doc: &Document, form: NodeId) -> Vec<(String, String)> {
    doc.descendants(form)
        .filter(|n| matches!(doc.tag(*n), Some("input" | "textarea" | "select")))
        .filter_map(|n| Some((doc.attr(n, "name")?.to_owned(), doc.attr(n, "value").unwrap_or("").to_owned())))
        .collect()
}
/// Everything on the page that is, or looks like, a control, with what it would do.
/// `dead` collects the ones that cannot do anything at all.
fn controls(page: &Page, here: &str, dead: &mut Vec<String>) -> Vec<Control> {
    let doc = &page.doc;
    let mut out = vec![];
    for node in doc.descendants(Document::ROOT) {
        let id = doc.attr(node, "id").unwrap_or("").to_owned();
        match doc.tag(node) {
            Some("a") => {
                let Some(href) = doc.attr(node, "href") else {
                    dead.push(format!("{here}: <a id={id:?}> has no href"));
                    continue;
                };
                if href.is_empty() || href == "#" {
                    dead.push(format!("{here}: <a id={id:?}> points at {href:?}"));
                    continue;
                }
                if href.starts_with(here) && href.len() == here.len() && !marks_here(doc, node) {
                    dead.push(format!("{here}: <a id={id:?}> only reloads this page"));
                    continue;
                }
                if href.starts_with('/') {
                    out.push(Control { id, method: "GET", target: href.to_owned(), fields: vec![] });
                }
            }
            // A button or a field outside a form submits nowhere at all.
            Some("button") => match owning_form(doc, node) {
                None => dead.push(format!("{here}: <button id={id:?}> is in no form")),
                Some((form, action, method)) => {
                    let mut fields = fields_of(doc, form);
                    if let (Some(name), Some(value)) = (doc.attr(node, "name"), doc.attr(node, "value")) {
                        fields.push((name.to_owned(), value.to_owned()));
                    }
                    let target = doc.attr(node, "formaction").unwrap_or(&action).to_owned();
                    let method = if method == "post" { "POST" } else { "GET" };
                    out.push(Control { id, method, target, fields });
                }
            },
            Some("input" | "textarea" | "select") if doc.attr(node, "type") != Some("hidden") => {
                if owning_form(doc, node).is_none() {
                    dead.push(format!("{here}: <{}> id={id:?} is in no form", doc.tag(node).unwrap_or("")));
                }
            }
            Some("form") => {
                let action = doc.attr(node, "action").unwrap_or("").to_owned();
                let method = doc.attr(node, "method").unwrap_or("get").to_ascii_lowercase();
                if action.is_empty() {
                    dead.push(format!("{here}: <form id={id:?}> posts nowhere"));
                    continue;
                }
                out.push(Control {
                    id,
                    method: if method == "post" { "POST" } else { "GET" },
                    target: action,
                    fields: fields_of(doc, node),
                });
            }
            _ => {}
        }
    }
    out
}
/// A GET target with the fields of its form appended, the way a browser submits one.
fn submitted(target: &str, fields: &[(String, String)]) -> String {
    if fields.is_empty() {
        return target.to_owned();
    }
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .finish();
    format!("{}{}{query}", target, if target.contains('?') { "&" } else { "?" })
}

/// The classes this page's own stylesheet paints as pressable: anything given a pointer
/// cursor, and anything with a hover state. `(tag, class)` of the last compound of each
/// such selector, which is the element the rule lands on.
fn pressable_classes(html: &str) -> Vec<(Option<String>, String)> {
    let mut out = vec![];
    let sheet = html.split("<style>").nth(1).unwrap_or("").split("</style>").next().unwrap_or("");
    for rule in sheet.split('}') {
        let Some((selectors, body)) = rule.split_once('{') else { continue };
        let pressable = body.contains("cursor: pointer") || selectors.contains(":hover");
        if !pressable {
            continue;
        }
        for selector in selectors.split(',') {
            let last = selector.trim().rsplit(char::is_whitespace).next().unwrap_or("").trim();
            let last = last.split("::").next().unwrap_or(last);
            let (before, _) = last.split_once(":hover").unwrap_or((last, ""));
            let Some((tag, classes)) = before.split_once('.') else { continue };
            let class = classes.split('.').next().unwrap_or("").to_owned();
            if class.is_empty() {
                continue;
            }
            out.push((if tag.is_empty() { None } else { Some(tag.to_owned()) }, class));
        }
    }
    out
}
/// Nothing may be painted as pressable unless it is, holds, or sits inside a control. A
/// `<span class="btn">` is the shape this catches: it lights up under the pointer and
/// swallows the click, and the semantic tree never mentions it at all.
fn nothing_is_dressed_as_a_control(page: &Page, here: &str, dead: &mut Vec<String>) {
    let doc = &page.doc;
    let control = |n: NodeId| matches!(doc.tag(n), Some("a" | "button" | "input" | "select" | "textarea" | "label"));
    for (tag, class) in pressable_classes(&page.html) {
        for node in doc.descendants(Document::ROOT) {
            if tag.as_deref().is_some_and(|t| doc.tag(node) != Some(t)) {
                continue;
            }
            if !doc.attr(node, "class").is_some_and(|c| c.split_ascii_whitespace().any(|k| k == class)) {
                continue;
            }
            let reaches = control(node)
                || doc.descendants(node).any(control)
                || doc.ancestors(node).any(control);
            if !reaches {
                let id = doc.attr(node, "id").unwrap_or("");
                dead.push(format!("{here}: <{} id={id:?} class={class:?}> is painted pressable but is no control", doc.tag(node).unwrap_or("?")));
            }
        }
    }
}

/// Walks a whole site: every page reachable from its entry points, every control on every
/// page followed. POSTs run against a copy of the state, so a star toggled or a branch
/// deleted while probing cannot change what the rest of the walk sees.
fn walk(site: &str, mut state: Value, roots: &[&str]) {
    let mut queue: VecDeque<String> = roots.iter().map(|r| r.to_string()).collect();
    let mut seen: BTreeSet<String> = queue.iter().cloned().collect();
    let mut dead: Vec<String> = vec![];
    let mut pages = 0;
    let mut posts = 0;
    while let Some(url) = queue.pop_front() {
        let response = request(&mut state, "GET", &url, &[]);
        if response.status == 404 || response.status == 405 {
            dead.push(format!("{site}: GET {url} -> {}", response.status));
            continue;
        }
        assert!(response.status < 400, "{site}: GET {url} -> {}", response.status);
        let body = String::from_utf8(response.body).unwrap();
        if response.headers.get("content-type").is_some_and(|c| !c.starts_with("text/html")) {
            continue;
        }
        pages += 1;
        let page = Page::parse(&format!("{site} {url}"), body);
        nothing_is_dressed_as_a_control(&page, &url, &mut dead);
        for control in controls(&page, &url, &mut dead) {
            if !control.target.starts_with('/') {
                continue;
            }
            if control.method == "GET" {
                let next = submitted(&control.target, &control.fields);
                if seen.insert(next.clone()) {
                    queue.push_back(next);
                }
                continue;
            }
            // Probe the POST on a copy: it must be routed, whatever it answers about the
            // request itself (422 for an empty title is a real answer; 404 is not).
            let mut copy = state.clone();
            let answer = request(&mut copy, "POST", &control.target, &control.fields);
            posts += 1;
            if answer.status == 404 || answer.status == 405 {
                dead.push(format!("{site}: POST {} (#{}) -> {}", control.target, control.id, answer.status));
            }
        }
    }
    assert!(pages > 20, "{site}: only {pages} pages were reached");
    assert!(posts > 0, "{site}: no form was probed");
    assert!(dead.is_empty(), "{site}: {} dead controls out of {pages} pages and {posts} posts:\n{dead:#?}", dead.len());
    println!("{site}: {pages} pages, {posts} form submissions, no dead controls");
}

#[test]
fn every_control_on_every_github_page_does_something() {
    walk("github.com", seeded("github", "github"), &["/"]);
}

#[test]
fn every_control_on_every_gitlab_page_does_something() {
    walk("gitlab.com", seeded("gitlab", "gitlab"), &["/"]);
}

/// git.internal: the plain index and repository pages, and the owner paths it also serves.
#[test]
fn every_control_on_the_plain_server_does_something() {
    walk("git.internal", seeded("gitlab", "plain"), &["/", "/opensim/replay-tools"]);
}

/// A repository whose every branch has been merged away, a thread with no comments and a
/// draft pull request: the states the seeds do not cover, reached from their own pages.
#[test]
fn the_states_the_seeds_do_not_cover_have_no_dead_controls_either() {
    let state = GitService
        .initialize(
            json!({"skin":"github","repositories":{"spare":{"owner":"aria",
                "description":"A repository with the awkward states in it.","topics":["edge"],
                "readers":["alicechen","aria"],"writers":["alicechen","aria"],"next_number":9,
                "files":{"README.md":"# Spare\nNothing here yet.\n"},
                "issues":{"7":{"number":7,"title":"An issue nobody answered","body":"","state":"open","author":"aria","tick":40}},
                "pull_requests":{"8":{"number":8,"title":"A draft","body":"","state":"open","draft":true,
                    "author":"aria","head":"refs/heads/main","base":"refs/heads/main","tick":41}}}}}),
            &ctx(ACTOR),
        )
        .unwrap();
    walk("edge cases", state, &["/", "/aria/spare", "/aria/spare/issues?state=closed", "/aria/spare/pulls?state=closed"]);
}

/// The two controls the brief turned from decoration into behaviour: watching a
/// repository, and the file finder behind "Go to file".
#[test]
fn watching_is_a_real_subscription_and_go_to_file_really_finds_files() {
    let mut state = seeded("github", "github");
    let repo = Page::parse("repo", String::from_utf8(request(&mut state, "GET", "/northstar/atlas", &[]).body).unwrap());
    assert_eq!(repo.tag("watch"), "button");
    assert_eq!(repo.text("watch-label"), "Watch");
    assert_eq!(repo.form_of("watch"), "watch-form");
    let (action, method, _) = repo.form("watch-form");
    assert_eq!((action.as_str(), method.as_str()), ("/northstar/atlas/watch", "post"));
    // Posting it lands on the watchers page, which now names the actor.
    let watched = Page::parse("watchers", String::from_utf8(request(&mut state, "POST", &action, &[]).body).unwrap());
    assert_eq!(watched.text("watchers-count"), "1 person is watching northstar/atlas");
    assert_eq!(watched.text("watcher-name-0"), ACTOR);
    let again = Page::parse("repo", String::from_utf8(request(&mut state, "GET", "/northstar/atlas", &[]).body).unwrap());
    assert_eq!(again.text("watch-label"), "Unwatch");
    assert_eq!(again.text("about-watching-link"), "1 watching");
    // Unwatching is the same button.
    request(&mut state, "POST", &action, &[]);
    assert_eq!(state["repositories"]["atlas"]["watchers"], json!(null));

    // "Go to file" reaches a page that lists the tree and filters it.
    assert_eq!(again.attr("go-to-file", "href"), "/northstar/atlas/find/main");
    let find = Page::parse("find", String::from_utf8(request(&mut state, "GET", "/northstar/atlas/find/main", &[]).body).unwrap());
    let (action, method, fields) = find.form("find");
    assert_eq!((action.as_str(), method.as_str()), ("/northstar/atlas/find/main", "get"));
    assert_eq!(fields, vec![("q".to_owned(), String::new())]);
    let filtered = Page::parse("find bfs", String::from_utf8(request(&mut state, "GET", "/northstar/atlas/find/main?q=bfs", &[]).body).unwrap());
    assert_eq!(filtered.attr("find-link-0", "href"), "/northstar/atlas/blob/main/src/bfs.rs");
    assert_eq!(filtered.text("find-count"), "1 of 10 files on main");
}

/// The header icons and the profile they lead to: the dashboards read the same threads the
/// repository pages do, and every name a page paints has a page behind it.
fn header_dashboards_and_every_name_have_a_page() {
    let mut state = seeded("github", "github");
    let issues = Page::parse("issues", String::from_utf8(request(&mut state, "GET", "/issues", &[]).body).unwrap());
    assert_eq!(issues.text("dash-title"), "Issues");
    assert!(issues.has("dash-issue-14"));
    assert_eq!(issues.attr("dash-issue-link-14", "href"), "/northstar/atlas/issues/14");
    let pulls = Page::parse("pulls", String::from_utf8(request(&mut state, "GET", "/pulls", &[]).body).unwrap());
    assert_eq!(pulls.text("dash-title"), "Pull requests");
    // The inbox is a reading of the threads, so it needs no state of its own.
    let inbox = Page::parse("inbox", String::from_utf8(request(&mut state, "GET", "/notifications", &[]).body).unwrap());
    assert!(inbox.text("inbox-sub").ends_with("you have taken part in or are watching."));
    // Everyone a page names — stargazers, commit authors, commenters — has a profile.
    let stars = Page::parse("stars", String::from_utf8(request(&mut state, "GET", "/northstar/atlas/stargazers", &[]).body).unwrap());
    for id in stars.ids_with_prefix("stargazer-name-") {
        let who = stars.attr(&id, "href");
        assert_eq!(request(&mut state, "GET", &who, &[]).status, 200, "{who} has no profile");
    }
    // And a name nobody here has ever used still does not.
    assert_eq!(request(&mut state, "GET", "/nobody", &[]).status, 404);
}

#[test]
fn the_header_reaches_the_dashboards_and_the_profiles() {
    header_dashboards_and_every_name_have_a_page();
}
