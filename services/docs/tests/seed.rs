//! docs.google.com as it is actually shipped: the seed must load, every indexed page must draw,
//! and the release code must be where storyline 1 says it is.
use cw_protocol::{HttpRequest, Page};
use cw_sdk::{Service, ServiceContext};
use cw_service_docs::{DocType, DocsService, DocsState};
use serde_json::Value;
const SITE: &str = include_str!("../../../worlds/company-2026/sites/google-docs.json");
fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 7,
        instance: "docs".into(),
    }
}
fn boot(actor: &str) -> (Value, DocsState) {
    let site: Value = serde_json::from_str(SITE).unwrap();
    assert_eq!(site["kind"], "docs");
    let state = DocsService
        .initialize(site["initial_state"].clone(), &ctx(actor))
        .expect("the shipped seed must load");
    let parsed = serde_json::from_value(state.clone()).unwrap();
    (state, parsed)
}
#[test]
fn the_seed_carries_one_of_each_kind_and_the_release_code() {
    let (_, s) = boot("alice");
    let launch = s.read("alice", "atlas-launch").unwrap();
    assert!(launch.body.contains("Release code: ATLAS-2026"));
    // The body, the revision that introduced it, and the one search keyword that finds it.
    assert_eq!(SITE.matches("ATLAS-2026").count(), 3);
    let sheet = s.read("alice", "q3-metrics").unwrap();
    assert_eq!(sheet.doc_type, DocType::Sheet);
    assert_eq!(sheet.cells["C2"], "184 ms");
    let deck = s.read("alice", "atlas-launch-review").unwrap();
    assert_eq!(deck.doc_type, DocType::Slides);
    assert_eq!(deck.slides.len(), 5);
    // Drive points at doc ids through `folder`, which is the join between the two crates.
    assert_eq!(launch.folder, "f-atlas");
}
#[test]
fn every_indexed_page_renders() {
    let (mut state, _) = boot("alice");
    let site: Value = serde_json::from_str(SITE).unwrap();
    for entry in site["search_entries"].as_array().unwrap() {
        let url = entry["url"].as_str().unwrap();
        let response = DocsService
            .handle(&mut state, &ctx("alice"), &HttpRequest::get(url))
            .unwrap();
        assert_eq!(response.status, 200, "{url}");
        let page: Page = serde_json::from_slice(&response.body).unwrap();
        page.validate().unwrap_or_else(|e| panic!("{url}: {e}"));
        assert!(page.theme.is_some(), "{url} must wear the gdocs palette");
    }
}
/// Bob may read the checklist and comment on it; only alice and carol may rewrite it.
#[test]
fn the_seeded_grants_are_the_ones_the_storyline_needs() {
    let (_, mut s) = boot("alice");
    assert!(s.read("bob", "atlas-launch").is_ok());
    assert!(s.edit("bob", "atlas-launch", 4, "nope", 12).is_err());
    assert!(s.edit("alice", "atlas-launch", 4, "signed off", 12).is_ok());
    assert!(s.set_cell("alice", "q3-metrics", "C5", "0", 12).is_ok());
    assert!(s.set_cell("bob", "q3-metrics", "C5", "0", 12).is_err());
    assert!(s.read("dana", "atlas-launch").is_err());
}
