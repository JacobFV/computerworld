//! The two drives this crate ships with must load and draw. A seed typo that only shows up when
//! a world boots is the failure this test exists to move forward to `cargo test`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::{validate_strict, HTML_MEDIA_TYPE};
use cw_service_drive::{DriveService, DriveState, NodeKind};
use serde_json::Value;
const DRIVE: &str = include_str!("../../../../worlds/company-2026/sites/google-drive.json");
const DROPBOX: &str = include_str!("../../../../worlds/company-2026/sites/dropbox.json");
fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 7,
        instance: "drive".into(),
    }
}
fn boot(site: &str, actor: &str) -> (Value, DriveState) {
    let site: Value = serde_json::from_str(site).unwrap();
    assert_eq!(site["kind"], "drive");
    let state = DriveService
        .initialize(site["initial_state"].clone(), &ctx(actor))
        .expect("the shipped seed must load");
    let parsed = serde_json::from_value(state.clone()).unwrap();
    (state, parsed)
}
fn names(s: &DriveState, actor: &str, folder: &str) -> Vec<String> {
    s.children(actor, folder)
        .iter()
        .filter(|n| n.kind == NodeKind::File)
        .map(|n| n.name.clone())
        .collect()
}
/// Storyline 11 is a joke with a punchline only if the two file lists are equal.
#[test]
fn the_dropbox_and_drive_atlas_folders_hold_the_same_filenames() {
    let (_, drive) = boot(DRIVE, "alice");
    let (_, dropbox) = boot(DROPBOX, "alice");
    let here = names(&drive, "alice", "f-atlas");
    let there = names(&dropbox, "alice", "atlas-assets");
    assert_eq!(here.len(), 7, "seven files on each side");
    assert_eq!(here, there);
    // Drive has the shortcut on top of the seven, and it points at the real document.
    let shortcut = drive.read("alice", "atlas-launch").unwrap();
    assert_eq!(shortcut.kind, NodeKind::Shortcut);
    assert_eq!(
        shortcut.target_url,
        "http://docs.google.com/documents/atlas-launch"
    );
    // The release code belongs to the doc alone; a copy in a drive would break the search task.
    assert!(!DRIVE.contains("ATLAS-2026") && !DROPBOX.contains("ATLAS-2026"));
}
/// Every URL these two sites offer the search index must answer, and answer with a page the engine renders strictly.
#[test]
fn every_indexed_page_of_both_drives_renders() {
    for (site, actor) in [(DRIVE, "alice"), (DROPBOX, "carol")] {
        let (mut state, _) = boot(site, actor);
        let definition: Value = serde_json::from_str(site).unwrap();
        let entries = definition["search_entries"].as_array().unwrap();
        assert!(!entries.is_empty(), "a site nobody can find is not a site");
        for entry in entries {
            let url = entry["url"].as_str().unwrap();
            let response = DriveService
                .handle(&mut state, &ctx(actor), &HttpRequest::get(url))
                .unwrap();
            assert_eq!(response.status, 200, "{url}");
            assert_eq!(
                response.header("content-type"),
                Some(HTML_MEDIA_TYPE),
                "{url}"
            );
            let html = String::from_utf8(response.body).unwrap();
            validate_strict(&html).unwrap_or_else(|e| panic!("{url}: {e:?}"));
            let page = cw_web::html::parse(&html);
            for id in [
                "chrome-brand",
                "find",
                "find-q",
                "nav-drive",
                "nav-trash",
                "head-title",
            ] {
                assert_eq!(page.by_id(id).len(), 1, "{url}: #{id}");
            }
        }
    }
}
/// Alice holds a grant on the Dropbox folder, so the migration story is hers to follow.
#[test]
fn the_dropbox_share_reaches_alice_and_the_public_link_works() {
    let (mut state, dropbox) = boot(DROPBOX, "alice");
    assert_eq!(dropbox.shared_with_me("alice").len(), 10);
    let link = &dropbox.read("alice", "atlas-assets").unwrap().link;
    assert!(!link.is_empty(), "the folder ships with a share link");
    let response = DriveService
        .handle(
            &mut state,
            &ctx("dana"),
            &HttpRequest::get(format!("http://dropbox.com/s/{link}")),
        )
        .unwrap();
    assert_eq!(
        response.status, 200,
        "a link opens for someone with no grant"
    );
    let html = String::from_utf8(response.body).unwrap();
    validate_strict(&html).unwrap();
    let page = cw_web::html::parse(&html);
    assert_eq!(
        page.text_content(page.by_id("public")[0]),
        "Opened with a share link"
    );
    assert_eq!(
        page.text_content(page.by_id("head-title")[0]),
        "Atlas assets"
    );
}
