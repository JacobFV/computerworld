//! The twelve shipped publications, exercised the way a reader would meet them: every page
//! is HTML the engine renders strictly, and every control on it goes where the seed says.
//!
//! `include_str!` rather than a file read: the seed is a build input, so a drift between the world
//! data and the crate that serves it fails to compile instead of failing at simulation time.
use cw_protocol::{HttpRequest, HttpResponse};
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::{validate_strict, HTML_MEDIA_TYPE};
use cw_service_press::PressService;
use cw_web::dom::Document as Dom;
use serde_json::Value;
const SITES: &[(&str, &str)] = &[
    (
        "reuters",
        include_str!("../../../worlds/company-2026/sites/reuters.json"),
    ),
    (
        "theverge",
        include_str!("../../../worlds/company-2026/sites/theverge.json"),
    ),
    (
        "arstechnica",
        include_str!("../../../worlds/company-2026/sites/arstechnica.json"),
    ),
    (
        "alice-blog",
        include_str!("../../../worlds/company-2026/sites/alice-blog.json"),
    ),
    (
        "bob-blog",
        include_str!("../../../worlds/company-2026/sites/bob-blog.json"),
    ),
    (
        "northstar-eng",
        include_str!("../../../worlds/company-2026/sites/northstar-eng.json"),
    ),
    (
        "nytimes",
        include_str!("../../../worlds/company-2026/sites/nytimes.json"),
    ),
    (
        "bbc",
        include_str!("../../../worlds/company-2026/sites/bbc.json"),
    ),
    (
        "cnn",
        include_str!("../../../worlds/company-2026/sites/cnn.json"),
    ),
    (
        "google-news",
        include_str!("../../../worlds/company-2026/sites/google-news.json"),
    ),
    (
        "medium",
        include_str!("../../../worlds/company-2026/sites/medium.json"),
    ),
    (
        "substack",
        include_str!("../../../worlds/company-2026/sites/substack.json"),
    ),
];
fn raw(id: &str) -> &'static str {
    SITES
        .iter()
        .find(|(name, _)| *name == id)
        .expect("known site")
        .1
}
fn ctx() -> ServiceContext {
    ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 1,
        instance: "press".into(),
    }
}
/// The loaded state, the site's own origin, and the build-only entries the engines will index.
fn site(source: &str) -> (Value, String, Vec<String>) {
    let file: Value = serde_json::from_str(source).expect("site file parses");
    let state = PressService
        .initialize(file["initial_state"].clone(), &ctx())
        .expect("seed passes the service's own gate");
    let origin = format!(
        "http://{}",
        file["domains"][0].as_str().expect("a first domain")
    );
    let entries = file["search_entries"]
        .as_array()
        .expect("search entries are an array")
        .iter()
        .map(|e| e["url"].as_str().expect("entry has a url").to_owned())
        .collect();
    (state, origin, entries)
}
fn get(state: &mut Value, url: &str) -> HttpResponse {
    PressService
        .handle(state, &ctx(), &HttpRequest::get(url))
        .expect("the service answers")
}
/// A 200 HTML page, run through the strict validator and parsed.
fn page(state: &mut Value, url: &str) -> Dom {
    let response = get(state, url);
    assert_eq!(response.status, 200, "{url}");
    assert_eq!(
        response.header("content-type"),
        Some(HTML_MEDIA_TYPE),
        "{url}"
    );
    let html = String::from_utf8(response.body).expect("utf-8");
    validate_strict(&html).unwrap_or_else(|e| panic!("{url}: {e:?}"));
    cw_web::html::parse(&html)
}
fn node(doc: &Dom, id: &str) -> cw_web::dom::NodeId {
    *doc.by_id(id)
        .first()
        .unwrap_or_else(|| panic!("no element #{id}"))
}
fn attr(doc: &Dom, id: &str, name: &str) -> String {
    doc.attr(node(doc, id), name)
        .unwrap_or_else(|| panic!("#{id} has no {name}"))
        .to_owned()
}
fn doc_text(doc: &Dom, id: &str) -> String {
    doc.text_content(node(doc, id))
}
fn ids(state: &Value) -> Vec<String> {
    state["articles"]
        .as_object()
        .expect("articles is an object")
        .keys()
        .cloned()
        .collect()
}
#[test]
fn every_page_a_seed_names_resolves_on_every_publication() {
    for (name, source) in SITES {
        let (mut state, origin, entries) = site(source);
        let blog = state["layout"] == "blog";
        let front = page(&mut state, &format!("{origin}/"));
        let skin = state["skin"]
            .as_str()
            .expect("every shipped publication names its skin");
        assert!(
            front.has_class(front.body().unwrap(), &format!("skin-{skin}")),
            "{name} wears {skin}"
        );
        assert_eq!(
            doc_text(&front, "masthead-brand"),
            state["brand"].as_str().unwrap(),
            "{name} wordmark"
        );
        assert_eq!(attr(&front, "masthead-home", "href"), "/");
        assert_eq!(attr(&front, "masthead-saved", "href"), "/saved");
        assert_eq!(attr(&front, "masthead-follow-form", "action"), "/follow");
        assert_eq!(attr(&front, "subscribe", "action"), "/subscribe");
        assert_eq!(attr(&front, "subscribe-email", "name"), "email");
        // Every story is a card on the front page, one link to the path the layout publishes.
        for id in ids(&state) {
            let expected = if blog {
                format!("/posts/{id}")
            } else {
                format!(
                    "/{}/{id}",
                    state["articles"][&id]["year"].as_str().unwrap_or("2026")
                )
            };
            assert_eq!(
                attr(&front, &format!("card-{id}"), "href"),
                expected,
                "{name} card {id}"
            );
            assert_eq!(
                doc_text(&front, &format!("card-{id}-title")),
                state["articles"][&id]["title"].as_str().unwrap()
            );
        }
        // Every link on the front page that stays on the site resolves.
        let hrefs: Vec<String> = front
            .descendants(Dom::ROOT)
            .filter(|n| front.is(*n, "a"))
            .filter_map(|n| front.attr(n, "href").map(str::to_owned))
            .filter(|h| h.starts_with('/'))
            .collect();
        assert!(hrefs.len() > 8, "{name} front page links");
        for href in hrefs {
            assert_eq!(
                get(&mut state, &format!("{origin}{href}")).status,
                200,
                "{name}{href}"
            );
        }
        page(&mut state, &format!("{origin}/archive"));
        page(&mut state, &format!("{origin}/saved"));
        let sections: Vec<String> = state["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_owned())
            .collect();
        for id in &sections {
            let listing = page(&mut state, &format!("{origin}/{id}"));
            assert_eq!(
                attr(&listing, &format!("masthead-{id}"), "href"),
                format!("/{id}"),
                "{name}/{id}"
            );
        }
        for id in ids(&state) {
            let article = state["articles"][&id].clone();
            let path = if blog {
                format!("/posts/{id}")
            } else {
                format!("/{}/{id}", article["year"].as_str().unwrap_or("2026"))
            };
            let story = page(&mut state, &format!("{origin}{path}"));
            assert_eq!(
                doc_text(&story, "article-title"),
                article["title"].as_str().unwrap()
            );
            assert_eq!(
                attr(&story, "comment", "action"),
                format!("/articles/{id}/comments")
            );
            assert_eq!(attr(&story, "comment-text", "name"), "text");
            assert_eq!(
                attr(&story, "article-save-form", "action"),
                format!("/articles/{id}/save")
            );
            for (i, link) in article["links"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    attr(&story, &format!("article-ref-{i}"), "href"),
                    link["url"].as_str().unwrap()
                );
            }
            for comment in article["comments"].as_array().unwrap() {
                let cid = comment["id"].as_str().unwrap();
                assert_eq!(
                    attr(&story, &format!("comment-{cid}-like-form"), "action"),
                    format!("/articles/{id}/comments/{cid}/like")
                );
            }
            // Every kicker, tag chip and Read more entry the article renders has to go somewhere.
            let section = article["section"].as_str().unwrap_or_default();
            assert!(
                sections.iter().any(|s| s == section),
                "{name}: {id} is filed under an undeclared section {section}"
            );
            for tag in article["tags"].as_array().unwrap() {
                let tag = tag.as_str().unwrap();
                assert_eq!(
                    attr(&story, &format!("article-tag-{tag}"), "href"),
                    format!("/tag/{tag}")
                );
                let tagged = page(&mut state, &format!("{origin}/tag/{tag}"));
                assert_eq!(
                    attr(&tagged, "list-follow-form", "action"),
                    format!("/tags/{tag}/follow")
                );
                assert!(
                    !tagged.by_id(&format!("card-{id}")).is_empty(),
                    "{name} tag {tag} lists {id}"
                );
            }
        }
        for url in &entries {
            assert_eq!(
                get(&mut state, url).status,
                200,
                "{name}: indexed page {url} must resolve"
            );
        }
    }
}
#[test]
fn a_reader_can_comment_save_follow_and_subscribe_on_every_publication() {
    for (name, source) in SITES {
        let (mut state, origin, _) = site(source);
        let id = ids(&state)
            .into_iter()
            .next()
            .expect("at least one article");
        let before = state["articles"][&id]["comments"]
            .as_array()
            .map_or(0, Vec::len);
        let request = HttpRequest::json(
            "POST",
            format!("{origin}/api/articles/{id}/comments"),
            &serde_json::json!({"text": "Read it twice."}),
        )
        .unwrap();
        assert_eq!(
            PressService
                .handle(&mut state, &ctx(), &request)
                .unwrap()
                .status,
            200,
            "{name} takes a comment"
        );
        assert_eq!(
            state["articles"][&id]["comments"].as_array().unwrap().len(),
            before + 1
        );
        for (route, key) in [
            (format!("/api/articles/{id}/save"), "saved"),
            ("/api/tags/determinism/follow".into(), "follows"),
            ("/api/follow".into(), "follows"),
        ] {
            let request =
                HttpRequest::json("POST", format!("{origin}{route}"), &serde_json::json!({}))
                    .unwrap();
            assert_eq!(
                PressService
                    .handle(&mut state, &ctx(), &request)
                    .unwrap()
                    .status,
                200,
                "{name}{route}"
            );
            assert!(
                state[key]["alice"].is_array(),
                "{name}{route} recorded nothing"
            );
        }
        let request = HttpRequest::json(
            "POST",
            format!("{origin}/api/subscribe"),
            &serde_json::json!({"email": "carol.nakamura@gmail.com"}),
        )
        .unwrap();
        assert_eq!(
            PressService
                .handle(&mut state, &ctx(), &request)
                .unwrap()
                .status,
            200
        );
        assert!(state["subscribers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == "carol.nakamura@gmail.com"));
        // The same controls pressed in a browser: a urlencoded form post answers with the page
        // the reader was on, as strict HTML, showing what changed.
        let mut form = HttpRequest::get(format!("{origin}/articles/{id}/comments"));
        form.method = "POST".into();
        form.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        form.body = b"text=Posted+from+the+form.".to_vec();
        let shown = PressService.handle(&mut state, &ctx(), &form).unwrap();
        assert_eq!(shown.status, 200, "{name} form comment");
        let html = String::from_utf8(shown.body).unwrap();
        validate_strict(&html).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let shown = cw_web::html::parse(&html);
        assert_eq!(
            doc_text(&shown, "comments-heading"),
            format!("{} comments", before + 2)
        );
        assert!(
            html.contains("Posted from the form."),
            "{name} shows the new comment"
        );
        assert_eq!(
            doc_text(&shown, "article-save"),
            "Saved",
            "{name} shows the saved state"
        );
        // Everything that just happened has to survive a checkpoint.
        let bytes = serde_json::to_vec(&state).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            state,
            "{name} round trip"
        );
    }
}
#[test]
fn the_storylines_the_content_bible_pins_are_present() {
    // Storyline 2: the Verge story, bylined Tom Weber, with six comments, and a Reuters wire
    // version that cites it.
    let (verge, _, _) = site(raw("theverge"));
    let story = &verge["articles"]["atlas-determinism"];
    assert_eq!(story["byline"], "Tom Weber");
    assert_eq!(story["comments"].as_array().unwrap().len(), 6);
    let (reuters, _, _) = site(raw("reuters"));
    let wire = &reuters["articles"]["northstar-atlas-determinism"];
    assert_eq!(
        wire["body"].as_array().unwrap().len(),
        4,
        "the wire version is four paragraphs"
    );
    assert!(wire["body"].as_array().unwrap().iter().any(|p| p
        .as_str()
        .unwrap()
        .contains("theverge.com/2026/atlas-determinism")));
    // Storyline 12: Alice's write-up links the Verge story, the repo and the walkthrough, and
    // carries three comments.
    let (alice, _, _) = site(raw("alice-blog"));
    let post = &alice["articles"]["what-i-learned-shipping-atlas"];
    assert_eq!(post["comments"].as_array().unwrap().len(), 3);
    let linked: Vec<&str> = post["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["url"].as_str().unwrap())
        .collect();
    for expected in [
        "http://theverge.com/2026/atlas-determinism",
        "http://github.com/northstar/atlas",
        "http://youtube.com/watch?v=atlas-walkthrough",
    ] {
        assert!(
            linked.contains(&expected),
            "alice-blog is missing {expected}"
        );
    }
    // Storyline 3: Bob's write-up of the flaky test.
    let (bob, _, _) = site(raw("bob-blog"));
    assert!(bob["articles"]["hash-order-bit-me"]["body"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p
            .as_str()
            .unwrap()
            .contains("stackoverflow.com/questions/4411")));
    // Storyline 7: the eng blog's launch post embeds the walkthrough.
    let (eng, _, _) = site(raw("northstar-eng"));
    assert!(eng["articles"]["shipping-atlas"]["body"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p
            .as_str()
            .unwrap()
            .contains("youtube.com/watch?v=atlas-walkthrough")));
    // Storyline 1 keeps the release code to the doc, the mail and the tracker; not in the press.
    for (name, source) in SITES {
        assert!(
            !source.contains("ATLAS-2026"),
            "{name} leaks the release code"
        );
    }
}
