//! The three seeded feeds, loaded from the files the world is built from. A seed that no longer
//! initialises, or a `search_entries` URL that does not resolve to a page, is a broken site — the
//! search engines index those URLs and an agent will click them.
use cw_protocol::HttpRequest;
use cw_service_common::html::validate_strict;
use cw_sdk::{Service, ServiceContext};
use cw_service_social::{SocialService, SocialState};
use serde_json::Value;
const ACTORS: [&str; 4] = ["alice", "bob", "carol", "admin"];
const SITES: [&str; 7] = [
    "x-social",
    "bsky",
    "mastodon",
    "facebook",
    "instagram",
    "linkedin",
    "pinterest",
];
fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 1,
        instance: "social".into(),
    }
}
fn site(name: &str) -> Value {
    let path = format!(
        "{}/../../worlds/company-2026/sites/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).unwrap()
}
fn load(name: &str) -> (Value, Value) {
    let file = site(name);
    assert_eq!(file["id"], name, "{name}: id must match the file name");
    assert_eq!(file["kind"], "social");
    let state = SocialService
        .initialize(file["initial_state"].clone(), &ctx("alice"))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    (file, state)
}
/// Every indexed URL must answer 200 for somebody. A direct-message page is membership scoped,
/// so "somebody" is the point: private pages stay private and still exist.
#[test]
fn every_indexed_url_resolves_for_at_least_one_actor() {
    for name in SITES {
        let (file, state) = load(name);
        let entries = file["search_entries"].as_array().unwrap();
        assert!(entries.len() >= 8, "{name}: index the site properly");
        for entry in entries {
            let url = entry["url"].as_str().unwrap();
            let mut reached = false;
            for actor in ACTORS {
                let mut state = state.clone();
                let r = SocialService
                    .handle(&mut state, &ctx(actor), &HttpRequest::get(url))
                    .unwrap();
                if r.status == 200 {
                    let body = String::from_utf8(r.body).unwrap();
                    validate_strict(&body).unwrap_or_else(|e| panic!("{url}: {e:?}"));
                    reached = true;
                }
            }
            assert!(reached, "{name}: {url} resolves for nobody");
        }
    }
}
/// The content bible pins these handles across every site; a rename here silently breaks the
/// cross-site storylines that name them.
#[test]
fn seeded_identities_match_the_content_bible() {
    let expect: [(&str, [(&str, &str); 3]); 3] = [
        (
            "x-social",
            [
                ("alicechen", "alice"),
                ("bmartinez", "bob"),
                ("carolnk", "carol"),
            ],
        ),
        (
            "mastodon",
            [("alice", "alice"), ("bmartinez", "bob"), ("carol", "carol")],
        ),
        (
            "linkedin",
            [
                ("alice-chen", "alice"),
                ("bob-martinez", "bob"),
                ("carol-nakamura", "carol"),
            ],
        ),
    ];
    for (name, handles) in expect {
        let (_, state) = load(name);
        let s: SocialState = serde_json::from_value(state).unwrap();
        for (handle, actor) in handles {
            let account = s
                .accounts
                .get(handle)
                .unwrap_or_else(|| panic!("{name}: no @{handle}"));
            assert_eq!(account.actor.as_deref(), Some(actor), "{name}: @{handle}");
        }
        // Nobody may operate two accounts on one site: `account_of` would pick arbitrarily.
        for actor in ACTORS {
            assert!(
                s.accounts
                    .values()
                    .filter(|a| a.actor.as_deref() == Some(actor))
                    .count()
                    <= 1,
                "{name}: {actor} has two accounts"
            );
        }
    }
}
/// Storyline 2 (Tom Weber asks Alice for comment) and storyline 9 (the recruiter) both live in a
/// message thread, and both are only worth seeding if the right person can read them.
#[test]
fn the_seeded_conversations_reach_their_readers() {
    let (_, x) = load("x-social");
    let s: SocialState = serde_json::from_value(x).unwrap();
    let verge = &s.messages["c-verge"];
    assert!(verge.members.contains("alicechen") && verge.members.contains("tweber"));
    assert!(verge.messages.iter().any(|m| m.from == "alicechen"));
    assert_eq!(s.inbox("alice").len(), 1);
    assert_eq!(s.inbox("carol").len(), 0, "a DM is not a public page");
    let (_, li) = load("linkedin");
    let s: SocialState = serde_json::from_value(li).unwrap();
    let recruiter = &s.messages["c-meridian"];
    assert!(recruiter.members.contains("bob-martinez"));
    assert!(recruiter.messages[0].text.contains("Meridian Labs"));
    assert_eq!(s.inbox("bob").len(), 1);
    assert!(s.accounts["bob-martinez"]
        .experience
        .iter()
        .any(|r| r.company == "Northstar" && r.period.starts_with("2023")));
}
/// A seeded site has to keep working after it is used: post, like, follow, snapshot, reload.
#[test]
fn the_seeded_world_survives_being_used() {
    let (_, mut state) = load("x-social");
    let mut request = HttpRequest::get("http://x.com/posts");
    request.method = "POST".into();
    request.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    request.body = b"text=Replying+from+the+seeded+world&view=/".to_vec();
    assert_eq!(
        SocialService
            .handle(&mut state, &ctx("alice"), &request)
            .unwrap()
            .status,
        200
    );
    let restored: Value = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    let s: SocialState = serde_json::from_value(restored).unwrap();
    assert!(s
        .posts
        .values()
        .any(|p| p.text == "Replying from the seeded world"));
    assert_eq!(s.likes_of("p-1107"), 412, "seeded crowd is untouched");
}
/// Each seeded site wears the skin of the product it stands in for, and every page of it
/// (timelines, search, every profile, every thread, every conversation) passes the strict
/// validator for every actor: unique ids and only HTML and CSS the engine renders.
#[test]
fn every_page_of_every_seeded_site_is_strictly_valid_html() {
    let skins = ["x", "bsky", "mastodon", "facebook", "instagram", "linkedin", "pinterest"];
    for (name, skin) in SITES.iter().zip(skins) {
        let (file, state) = load(name);
        let s: SocialState = serde_json::from_value(state.clone()).unwrap();
        assert_eq!(cw_service_social::skin_of(&s), skin, "{name}");
        let domain = file["domains"][0].as_str().unwrap();
        let mut paths: Vec<String> = ["/", "/explore", "/local", "/search", "/search?q=a", "/messages", "/messaging"]
            .iter()
            .map(|p| (*p).to_owned())
            .collect();
        paths.extend(s.accounts.keys().map(|h| format!("/{h}")));
        paths.extend(s.posts.values().map(|p| format!("/{}/status/{}", p.author, p.id)));
        for actor in ACTORS {
            let mut state = state.clone();
            let mut paths = paths.clone();
            let root = if s.professional() { "messaging" } else { "messages" };
            paths.extend(s.inbox(actor).iter().map(|c| format!("/{root}/{}", c.id)));
            for path in &paths {
                let url = format!("http://{domain}{path}");
                let r = SocialService
                    .handle(&mut state, &ctx(actor), &HttpRequest::get(&url))
                    .unwrap();
                assert_eq!(r.status, 200, "{url} for {actor}");
                assert_eq!(r.header("content-type"), Some("text/html; charset=utf-8"));
                let body = String::from_utf8(r.body).unwrap();
                validate_strict(&body).unwrap_or_else(|e| panic!("{url} for {actor}: {e:?}"));
                assert!(body.contains(&format!("class=\"skin-{skin} ")), "{url}");
            }
        }
    }
}
