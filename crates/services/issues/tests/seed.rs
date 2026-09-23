//! The shipped Linear seed: every view renders as strict HTML, OPS-1 and OPS-7 are where
//! the content bible says they are, and every search entry resolves.
mod support;
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_issues::IssuesService;
use serde_json::Value;

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 60,
        seed: 1,
        instance: "linear".into(),
    }
}
fn fetch(state: &mut Value, path: &str) -> support::Page {
    let r = IssuesService
        .handle(
            state,
            &ctx("alice"),
            &HttpRequest::get(format!("http://linear.app{path}")),
        )
        .unwrap();
    assert_eq!(r.status, 200, "{path}");
    support::Page::parse(path, String::from_utf8(r.body).unwrap())
}
#[test]
fn linear_seed_initialises_and_every_view_renders_in_both_skins() {
    let raw = std::fs::read_to_string(cw_service_common::reference::reference_site_path("linear"))
        .unwrap();
    let site: Value = serde_json::from_str(&raw).unwrap();
    for skin in ["linear", "plain"] {
        let mut initial = site["initial_state"].clone();
        initial["skin"] = skin.into();
        let mut state = IssuesService
            .initialize(initial, &ctx("alice"))
            .expect("linear seed initialises");
        let projects = state["projects"].as_object().unwrap().clone();
        let mut paths = vec!["/".to_owned(), "/projects/OPS?assignee=alice".to_owned()];
        for (key, project) in &projects {
            for view in [
                "",
                "?view=board",
                "?view=cycle",
                "?view=board&assignee=alice",
            ] {
                paths.push(format!("/projects/{key}{view}"));
            }
            for id in project["issues"].as_object().unwrap().keys() {
                paths.push(format!("/projects/{key}/issues/{id}"));
            }
        }
        for path in paths {
            let page = fetch(&mut state, &path);
            assert_eq!(page.has("rail"), skin == "linear", "{skin} {path}");
        }
        for entry in site["search_entries"].as_array().unwrap() {
            let url = entry["url"].as_str().unwrap();
            let r = IssuesService
                .handle(&mut state, &ctx("alice"), &HttpRequest::get(url))
                .unwrap();
            assert_eq!(r.status, 200, "search entry {url}");
        }
    }
    let mut state = IssuesService
        .initialize(site["initial_state"].clone(), &ctx("alice"))
        .unwrap();
    // The projects table lists every team; the list groups a team's issues by status.
    let home = fetch(&mut state, "/");
    assert_eq!(home.title(), "Northstar · Linear");
    assert_eq!(home.ids_with_prefix("team-name-").len(), 6);
    assert_eq!(home.attr("team-ATL", "href"), "/projects/ATL");
    let ops = fetch(&mut state, "/projects/OPS");
    assert_eq!(ops.text("board-title"), "Operations");
    assert_eq!(ops.text("card-key-1"), "OPS-1");
    assert_eq!(ops.text("card-title-1"), "Confirm Atlas launch checklist");
    assert_eq!(ops.text("card-assignee-1"), "alice");
    assert_eq!(ops.attr("card-state-7", "data-status"), "in_progress");
    assert_eq!(ops.attr("card-priority-7", "title"), "Urgent");
    let (action, method, fields) = ops.form("new-issue");
    assert_eq!(
        (action.as_str(), method.as_str()),
        ("/projects/OPS/issues", "post")
    );
    assert_eq!(
        fields.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        vec!["title", "body"]
    );
    let (action, _, fields) = ops.form("move-7-blocked-form");
    assert_eq!(action, "/projects/OPS/issues/7");
    assert_eq!(
        fields[..2],
        [
            ("status".to_owned(), "blocked".to_owned()),
            ("view".to_owned(), "board".to_owned())
        ]
    );
    // An issue links what its body mentions.
    let seven = fetch(&mut state, "/projects/OPS/issues/7");
    assert_eq!(
        seven.attr("body-link-8", "href"),
        "http://status.northstar.example/incidents/inc-4"
    );
    assert_eq!(seven.text("comment-author-1"), "carol");
    // Storylines 1 and 10 name these two by number; the bible depends on them staying put.
    let ops = &state["projects"]["OPS"]["issues"];
    assert_eq!(ops["1"]["title"], "Confirm Atlas launch checklist");
    assert_eq!(ops["1"]["assignee"], "alice");
    assert_eq!(ops["7"]["assignee"], "admin");
    assert!(ops["7"]["body"].as_str().unwrap().contains("inc-4"));
    assert!(!state.to_string().contains("ATLAS-2026"));
}
