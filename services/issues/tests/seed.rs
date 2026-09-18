//! The shipped Linear seed: the board renders, OPS-1 and OPS-7 are where the content bible
//! says they are, and every search entry resolves.
use cw_protocol::{HttpRequest, Page};
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
#[test]
fn linear_seed_initialises_and_every_board_renders() {
    let raw = std::fs::read_to_string("../../worlds/company-2026/sites/linear.json").unwrap();
    let site: Value = serde_json::from_str(&raw).unwrap();
    let mut state = IssuesService
        .initialize(site["initial_state"].clone(), &ctx("alice"))
        .expect("linear seed initialises");
    for path in [
        "/",
        "/projects/OPS",
        "/projects/ATL",
        "/projects/OPS/issues/1",
        "/projects/OPS/issues/7",
        "/projects/ATL/issues/1",
        "/projects/OPS?assignee=alice",
    ] {
        let r = IssuesService
            .handle(
                &mut state,
                &ctx("alice"),
                &HttpRequest::get(format!("http://linear.app{path}")),
            )
            .unwrap();
        assert_eq!(r.status, 200, "{path}");
        serde_json::from_slice::<Page>(&r.body)
            .unwrap()
            .validate()
            .unwrap_or_else(|e| panic!("{path}: {e}"));
    }
    for entry in site["search_entries"].as_array().unwrap() {
        let url = entry["url"].as_str().unwrap();
        let r = IssuesService
            .handle(&mut state, &ctx("alice"), &HttpRequest::get(url))
            .unwrap();
        assert_eq!(r.status, 200, "search entry {url}");
    }
    // Storylines 1 and 10 name these two by number; the bible depends on them staying put.
    let ops = &state["projects"]["OPS"]["issues"];
    assert_eq!(ops["1"]["title"], "Confirm Atlas launch checklist");
    assert_eq!(ops["1"]["assignee"], "alice");
    assert_eq!(ops["7"]["assignee"], "admin");
    assert!(ops["7"]["body"].as_str().unwrap().contains("inc-4"));
    assert!(!state.to_string().contains("ATLAS-2026"));
}
