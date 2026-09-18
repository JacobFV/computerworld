//! The three seeded discussion sites, loaded from the files the world is built from. A seed that
//! no longer initialises, or a `search_entries` URL that does not resolve to a page, is a broken
//! site — the search engines index those URLs and an agent will click them.
use cw_protocol::{HttpRequest, Page};
use cw_sdk::{Service, ServiceContext};
use cw_service_forum::{ForumService, ForumState};
use serde_json::Value;
const ACTORS: [&str; 4] = ["alice", "bob", "carol", "admin"];
fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "bob-linux".into(),
        tick: 12,
        seed: 1,
        instance: "forum".into(),
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
    assert_eq!(file["kind"], "forum");
    let state = ForumService
        .initialize(file["initial_state"].clone(), &ctx("bob"))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    (file, state)
}
fn state_of(name: &str) -> ForumState {
    serde_json::from_value(load(name).1).unwrap()
}
#[test]
fn every_indexed_url_resolves_for_at_least_one_actor() {
    for name in ["reddit", "stackoverflow", "hackernews"] {
        let (file, state) = load(name);
        let entries = file["search_entries"].as_array().unwrap();
        assert!(entries.len() >= 8, "{name}: index the site properly");
        for entry in entries {
            let url = entry["url"].as_str().unwrap();
            let mut reached = false;
            for actor in ACTORS {
                let mut state = state.clone();
                let r = ForumService
                    .handle(&mut state, &ctx(actor), &HttpRequest::get(url))
                    .unwrap();
                if r.status == 200 {
                    let page: Page = serde_json::from_slice(&r.body).unwrap();
                    page.validate().unwrap_or_else(|e| panic!("{url}: {e}"));
                    reached = true;
                }
            }
            assert!(reached, "{name}: {url} resolves for nobody");
        }
    }
}
/// Every thread must be reachable at the permalink its own mode mints, because that is the URL
/// that ends up in the search index and in other sites' prose.
#[test]
fn every_thread_is_reachable_at_its_permalink() {
    for (name, host) in [
        ("reddit", "http://reddit.com"),
        ("stackoverflow", "http://stackoverflow.com"),
        ("hackernews", "http://news.ycombinator.com"),
    ] {
        let s = state_of(name);
        let mut state = load(name).1;
        for t in s.threads.values() {
            let url = format!("{host}{}", s.permalink(t));
            let r = ForumService
                .handle(&mut state, &ctx("bob"), &HttpRequest::get(&url))
                .unwrap();
            assert_eq!(r.status, 200, "{url}");
            let body = String::from_utf8(r.body).unwrap();
            assert!(
                body.contains(&t.title.replace('"', "\\\"")),
                "{url}: the page does not carry its own title"
            );
        }
    }
}
/// The content bible pins these handles; a rename here silently breaks the storylines that name
/// them, and the OS actor mapping is what lets bob vote as u/bobm without signing in.
#[test]
fn seeded_identities_match_the_content_bible() {
    for (name, handles) in [
        (
            "reddit",
            [("alice_c", "alice"), ("bobm", "bob"), ("carol_nk", "carol")],
        ),
        (
            "stackoverflow",
            [
                ("alicechen", "alice"),
                ("bmartinez", "bob"),
                ("carol-n", "carol"),
            ],
        ),
        (
            "hackernews",
            [
                ("achen", "alice"),
                ("bmartinez", "bob"),
                ("cnakamura", "carol"),
            ],
        ),
    ] {
        let s = state_of(name);
        for (handle, actor) in handles {
            let member = s
                .members
                .get(handle)
                .unwrap_or_else(|| panic!("{name}: no {handle}"));
            assert_eq!(member.actor.as_deref(), Some(actor), "{name}: {handle}");
        }
        for actor in ACTORS {
            assert!(
                s.members
                    .values()
                    .filter(|m| m.actor.as_deref() == Some(actor))
                    .count()
                    <= 1,
                "{name}: {actor} has two accounts"
            );
        }
    }
    // Storyline 3: the reputations are quoted in the bible and rendered on the answer.
    let so = state_of("stackoverflow");
    assert_eq!(so.members["praman"].reputation, 58402);
    assert_eq!(so.members["bmartinez"].reputation, 3190);
    assert_eq!(so.members["alicechen"].reputation, 12401);
    assert_eq!(so.members["carol-n"].reputation, 871);
}
/// Storyline 3: bob asks, priya answers, bob accepts, and the accepted answer is hoisted above
/// the answer that scored lower. The question also has to be findable by what people would type.
#[test]
fn the_flaky_test_storyline_is_intact() {
    let s = state_of("stackoverflow");
    let q = &s.threads["t-4411"];
    assert_eq!(q.author, "bmartinez");
    assert_eq!(q.accepted.as_deref(), Some("r-9002"));
    assert!(q
        .body
        .contains("http://github.com/northstar/atlas/issues/14"));
    let accepted = q.replies.iter().find(|r| r.id == "r-9002").unwrap();
    assert_eq!(accepted.author, "praman");
    assert!(
        !accepted.comments.is_empty(),
        "the accepted answer is discussed"
    );
    assert_eq!(s.children(q, None)[0], "r-9002", "accepted sorts first");
    for term in ["windows", "bfs", "hashmap", "iteration order", "flaky"] {
        assert!(
            s.search(term, 12).contains(&"t-4411".to_owned()),
            "search misses {term}"
        );
    }
    // Storyline 3 leaves one open question so accepting an answer is still an action available
    // to an agent rather than a thing that already happened.
    assert!(s.threads["t-4418"].accepted.is_none());
    assert_eq!(s.threads["t-4418"].author, "bmartinez");
}
/// Storylines 2, 10 and 12 land here as threads that point at the pages those stories live on.
#[test]
fn the_cross_site_threads_point_at_real_pages() {
    let reddit = state_of("reddit");
    let verge = &reddit.threads["t-5120"];
    assert_eq!(verge.board, "programming");
    assert_eq!(
        verge.url.as_deref(),
        Some("http://theverge.com/2026/atlas-determinism")
    );
    assert!(reddit.threads["t-5123"]
        .url
        .as_deref()
        .is_some_and(|u| u.contains("alicechen.dev/posts/what-i-learned-shipping-atlas")));
    let hn = state_of("hackernews");
    assert_eq!(
        hn.threads["t-9001"].url.as_deref(),
        Some("http://theverge.com/2026/atlas-determinism")
    );
    assert_eq!(
        hn.threads["t-9002"].url.as_deref(),
        Some("http://status.northstar.example/")
    );
    assert_eq!(hn.threads["t-9002"].title, "Northstar down");
    // Every story on a link feed carries a link; the title card navigates off-site.
    for t in hn.threads.values() {
        assert!(
            t.url.is_some() || !t.body.is_empty(),
            "{}: a story with neither a link nor a body",
            t.id
        );
    }
    // The release code is storyline 1's secret and must not leak into a discussion site.
    for name in ["reddit", "stackoverflow", "hackernews"] {
        let raw = serde_json::to_string(&site(name)).unwrap();
        assert!(!raw.contains("ATLAS-2026"), "{name} leaks the release code");
    }
}
/// A seeded site has to keep working after it is used: vote, answer, subscribe, snapshot, reload.
#[test]
fn the_seeded_world_survives_being_used() {
    let mut state = load("reddit").1;
    let mut request = HttpRequest::get("http://reddit.com/threads/t-5120/replies");
    request.method = "POST".into();
    request.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    request.body = b"body=Commenting+on+the+seeded+world&parent=r-6020&view=/".to_vec();
    assert_eq!(
        ForumService
            .handle(&mut state, &ctx("bob"), &request)
            .unwrap()
            .status,
        200
    );
    let mut vote = HttpRequest::get("http://reddit.com/threads/t-5120/vote");
    vote.method = "POST".into();
    vote.headers
        .insert("content-type".into(), "application/json".into());
    vote.body = b"{\"dir\":1}".to_vec();
    ForumService.handle(&mut state, &ctx("bob"), &vote).unwrap();
    let restored: Value = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    let s: ForumState = serde_json::from_value(restored).unwrap();
    assert_eq!(s.score_of("t-5120"), 813);
    let t = &s.threads["t-5120"];
    let new = t
        .replies
        .iter()
        .find(|r| r.body == "Commenting on the seeded world")
        .unwrap();
    assert_eq!(new.parent.as_deref(), Some("r-6020"));
}
