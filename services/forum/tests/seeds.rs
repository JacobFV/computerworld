//! The six seeded discussion sites, loaded from the files the world is built from. A seed that
//! no longer initialises, or a `search_entries` URL that does not resolve to a page, is a broken
//! site — the search engines index those URLs and an agent will click them.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_forum::{ForumService, ForumState};
use serde_json::Value;
const ACTORS: [&str; 4] = ["alice", "bob", "carol", "admin"];
const SITES: [&str; 6] = [
    "reddit",
    "stackoverflow",
    "hackernews",
    "quora",
    "yelp",
    "craigslist",
];
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
    for name in SITES {
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
                    let html = String::from_utf8(r.body).unwrap();
                    cw_service_common::html::validate_strict(&html)
                        .unwrap_or_else(|e| panic!("{url}: {e:?}"));
                    let page = cw_web::html::parse(&html);
                    let skin = format!("skin-{name} ");
                    assert!(
                        page.body()
                            .and_then(|b| page.attr(b, "class"))
                            .is_some_and(|c| c.starts_with(&skin)),
                        "{url}: not in the {name} skin"
                    );
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
        ("quora", "http://quora.com"),
        ("yelp", "http://yelp.com"),
        ("craigslist", "http://craigslist.org"),
    ] {
        let s = state_of(name);
        let mut state = load(name).1;
        for t in s.threads.values() {
            let url = format!("{host}{}", s.permalink(t));
            let r = ForumService
                .handle(&mut state, &ctx("bob"), &HttpRequest::get(&url))
                .unwrap();
            assert_eq!(r.status, 200, "{url}");
            let html = String::from_utf8(r.body).unwrap();
            cw_service_common::html::validate_strict(&html)
                .unwrap_or_else(|e| panic!("{url}: {e:?}"));
            let page = cw_web::html::parse(&html);
            let title = page
                .by_id("thread-title")
                .first()
                .map(|n| page.text_content(*n));
            assert_eq!(
                title.as_deref(),
                Some(t.title.as_str()),
                "{url}: the page does not carry its own title"
            );
            // Every reply is on the page with its vote control and its reply or comment box.
            for r in &t.replies {
                for id in [format!("reply-{}", r.id), format!("{}-up", r.id), format!("{}-body", r.id)] {
                    assert!(!page.by_id(&id).is_empty(), "{url}: no #{id}");
                }
            }
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

/// Every route of every seeded skin, rendered from the real seed and pushed through the engine's
/// strict validator. The unit tests do this with hand-built seeds; this is the content the world
/// actually ships, where a long body, an empty board or an odd tag is what breaks a page.
#[test]
fn every_route_of_every_seeded_skin_validates() {
    for name in SITES {
        let (file, base) = load(name);
        let host = format!("http://{}", file["domains"][0].as_str().unwrap());
        let s = state_of(name);
        let thread = s.threads.values().next().expect("a seeded thread");
        let tag = s
            .threads
            .values()
            .flat_map(|t| t.tags.iter())
            .next()
            .cloned()
            .unwrap_or_else(|| "rust".into());
        let member = s.members.keys().next().expect("a seeded member").clone();
        let board = s.boards.keys().next().cloned().unwrap_or_default();
        let mut paths = vec![
            "/".to_owned(),
            "/?p=2".to_owned(),
            "/newest".to_owned(),
            "/submit".to_owned(),
            "/ask".to_owned(),
            "/search".to_owned(),
            "/search?q=a".to_owned(),
            "/search?q=%C3%A9".to_owned(),
            "/r".to_owned(),
            "/boards".to_owned(),
            "/questions".to_owned(),
            format!("/questions/tagged/{}", tag.replace(' ', "%20")),
            format!("/u/{member}"),
            format!("/users/{member}"),
            s.permalink(thread),
        ];
        if !board.is_empty() {
            paths.push(format!("/r/{board}"));
        }
        for path in &paths {
            let mut rendered = 0;
            for actor in ACTORS {
                let mut state = base.clone();
                let r = ForumService
                    .handle(&mut state, &ctx(actor), &HttpRequest::get(format!("{host}{path}")))
                    .unwrap();
                if r.status != 200 {
                    continue;
                }
                rendered += 1;
                assert_eq!(
                    r.header("content-type"),
                    Some(cw_service_common::html::HTML_MEDIA_TYPE),
                    "{name} {path}: not HTML"
                );
                let html = String::from_utf8(r.body).unwrap();
                cw_service_common::html::validate_strict(&html)
                    .unwrap_or_else(|e| panic!("{name} {path} as {actor}: {e:?}"));
                let page = cw_web::html::parse(&html);
                assert!(
                    page.body()
                        .and_then(|b| page.attr(b, "class"))
                        .is_some_and(|c| c.starts_with(&format!("skin-{name} "))),
                    "{name} {path}: not in the {name} skin"
                );
            }
            assert!(rendered > 0, "{name} {path}: renders for nobody");
        }
    }
}

/// Every control that mutates re-renders through the router, so the page a form lands on has to
/// validate too — on the real seeds, in every skin.
#[test]
fn every_form_lands_on_a_valid_page() {
    for name in SITES {
        let (file, base) = load(name);
        let host = format!("http://{}", file["domains"][0].as_str().unwrap());
        let s = state_of(name);
        // An actor with an account here, so the mutating routes are not refused for want of one.
        let actor = ACTORS
            .iter()
            .find(|a| s.member_of(a).is_some())
            .unwrap_or_else(|| panic!("{name}: no seeded actor"));
        let thread = s
            .threads
            .values()
            .find(|t| !t.replies.is_empty())
            .expect("a seeded thread with replies");
        let reply = &thread.replies[0];
        let view = s.permalink(thread);
        let board = s.boards.keys().next().cloned().unwrap_or_default();
        let mut posts = vec![
            (
                format!("/threads/{}/vote", thread.id),
                format!("dir=1&view={}", encode(&view)),
            ),
            (
                format!("/threads/{}/replies", thread.id),
                format!("body=A+reply+from+the+test&view={}", encode(&view)),
            ),
            (
                format!("/replies/{}/comments", reply.id),
                format!("body=A+comment+from+the+test&view={}", encode(&view)),
            ),
            (
                format!("/replies/{}/vote", reply.id),
                format!("dir=1&view={}", encode(&view)),
            ),
            (
                "/threads".to_owned(),
                format!(
                    "board={board}&title=A+title+from+the+test&body=A+body&url=http%3A%2F%2Fexample.com%2F&tags=testing&view={}",
                    encode(&view)
                ),
            ),
            ("/search".to_owned(), "q=a".to_owned()),
        ];
        if !board.is_empty() {
            posts.push((
                format!("/boards/{board}/subscribe"),
                format!("view={}", encode("/r")),
            ));
        }
        if s.qa() && thread.author == s.member_of(actor).unwrap().handle {
            posts.push((
                format!("/threads/{}/accept", thread.id),
                format!("reply={}&view={}", reply.id, encode(&view)),
            ));
        }
        for (path, body) in posts {
            let mut state = base.clone();
            let mut r = HttpRequest::get(format!("{host}{path}"));
            r.method = "POST".into();
            r.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
            r.body = body.into_bytes();
            let response = ForumService.handle(&mut state, &ctx(actor), &r).unwrap();
            assert_eq!(response.status, 200, "{name} POST {path}");
            assert_eq!(
                response.header("content-type"),
                Some(cw_service_common::html::HTML_MEDIA_TYPE),
                "{name} POST {path}: not HTML"
            );
            let html = String::from_utf8(response.body).unwrap();
            cw_service_common::html::validate_strict(&html)
                .unwrap_or_else(|e| panic!("{name} POST {path}: {e:?}"));
        }
    }
}
/// The `view=` round trip is a URL inside a form field, so the test has to send it encoded.
fn encode(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' => c.to_string(),
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}
