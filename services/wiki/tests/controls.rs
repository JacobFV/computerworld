//! No dead controls: wikipedia.org, archive.org and imdb.com, every page kind.
//!
//! The sweep starts at the portal and the search results, follows every same-origin link
//! it meets — article, Talk, History, per-section editor, Random — probes every form
//! `action` with the method written on it, and asserts the service answers each with
//! something other than a 404, a 405 or an error. Each rendered page is audited for the
//! subtler lies: a control with nowhere to go, a field outside a form, a `role` or a
//! pointer cursor on something that cannot act.
//!
//! A `POST` probe really edits, so the probes run against a scratch clone of the state
//! and never disturb the crawl.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::audit;
use cw_service_wiki::{WikiService, WikiState};
use serde_json::Value;

const SITES: &[(&str, &str)] = &[
    ("wikipedia", include_str!("../../../worlds/company-2026/sites/wikipedia.json")),
    ("archive", include_str!("../../../worlds/company-2026/sites/archive.json")),
    ("imdb", include_str!("../../../worlds/company-2026/sites/imdb.json")),
];

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: "bob".into(),
        source: "bob-linux".into(),
        tick: 11,
        seed: 1,
        instance: "wiki".into(),
    }
}

/// The ids whose link may lead back to the page it is on, because the site it stands in
/// for does the same: the wordmark is always a link home, the tab of the view a reader
/// is already in is still a tab, and a listing of the whole corpus lists the page you
/// are reading too.
fn self_links(state: &WikiState) -> Vec<String> {
    let mut ids = vec![
        "masthead-logo".to_owned(),
        "nav-main".to_owned(),
        "nav-home".to_owned(),
        "nav-top".to_owned(),
        "tab-0".to_owned(),
        "tab-1".to_owned(),
        "tab-2".to_owned(),
    ];
    for i in 0..state.articles.len().max(16) {
        ids.push(format!("nav-art-{i}"));
        ids.push(format!("nav-col-{i}"));
        ids.push(format!("nav-site-{i}"));
    }
    ids
}

#[test]
fn no_control_on_any_of_the_three_sites_is_a_lie() {
    for (name, source) in SITES {
        let file: Value = serde_json::from_str(source).expect("site file parses");
        let mut state = WikiService
            .initialize(file["initial_state"].clone(), &ctx())
            .expect("seed passes the service's own gate");
        let loaded: WikiState = serde_json::from_value(state.clone()).expect("typed state");
        let origin = format!("http://{}", file["domains"][0].as_str().expect("a first domain"));
        let allow = self_links(&loaded);
        let allow: Vec<&str> = allow.iter().map(String::as_str).collect();
        // Every article, every one of its sections, its Talk and its History, so no page
        // kind depends on the crawl happening to reach it.
        let mut seeds: Vec<String> = vec!["/".to_owned(), "/search?q=simulation".to_owned()];
        for article in loaded.articles.values() {
            seeds.push(format!("/wiki/{}", article.id));
            seeds.push(format!("/wiki/Talk:{}", article.id));
            seeds.push(format!("/wiki/Special:History/{}", article.id));
            for section in &article.sections {
                seeds.push(format!("/wiki/{}?section={}", article.id, section.id));
            }
        }
        let seeds: Vec<&str> = seeds.iter().map(String::as_str).collect();
        let mut call = |method: &str, path: &str| {
            let mut request = HttpRequest::get(format!("{origin}{path}"));
            request.method = method.to_owned();
            // A POST probe edits an article, so it edits a copy.
            let mut scratch = state.clone();
            let target = if method == "GET" { &mut state } else { &mut scratch };
            let response = WikiService.handle(target, &ctx(), &request).expect("the service answers");
            (response.status, String::from_utf8_lossy(&response.body).into_owned())
        };
        let faults = audit::Sweep::new(&seeds, &mut call)
            .allow_self(&allow)
            .limit(400)
            .run();
        assert!(faults.is_empty(), "{name}:\n  {}", faults.join("\n  "));
    }
}

/// The pages a crawl from the portal never reaches: a search that matches nothing, a
/// corpus small enough that Random is not offered, and an article with no talk and no
/// revisions.
#[test]
fn the_thin_pages_promise_nothing_either() {
    for (name, source) in SITES {
        let file: Value = serde_json::from_str(source).expect("site file parses");
        let mut state = WikiService
            .initialize(file["initial_state"].clone(), &ctx())
            .expect("seed loads");
        for path in ["/search?q=zzzznothingmatches", "/search?q="] {
            let response = WikiService
                .handle(&mut state, &ctx(), &HttpRequest::get(format!("http://wiki.example{path}")))
                .expect("the service answers");
            assert_eq!(response.status, 200, "{name}{path}");
            let html = String::from_utf8_lossy(&response.body).into_owned();
            let faults = audit::page(&html);
            assert!(faults.is_empty(), "{name}{path}:\n  {}", faults.join("\n  "));
        }
    }
    // One bare article on one skin each: no talk, no revisions, no references, one section.
    for skin in ["vector", "archive", "imdb"] {
        let seed = serde_json::json!({
            "brand": "Thin", "skin": skin,
            "articles": {"Only": {"title": "Only", "summary": "The only page.",
                                  "sections": [{"id": "s1", "heading": "Background", "body": "Nothing yet."}]}}
        });
        let mut state = WikiService.initialize(seed, &ctx()).expect("seed loads");
        for path in ["/", "/wiki/Only", "/wiki/Talk:Only", "/wiki/Special:History/Only", "/wiki/Only?section=s1"] {
            let response = WikiService
                .handle(&mut state, &ctx(), &HttpRequest::get(format!("http://wiki.example{path}")))
                .expect("the service answers");
            assert_eq!(response.status, 200, "{skin}{path}");
            let html = String::from_utf8_lossy(&response.body).into_owned();
            let faults = audit::page(&html);
            assert!(faults.is_empty(), "{skin}{path}:\n  {}", faults.join("\n  "));
        }
    }
}

/// Random is a control, not a decoration: pressing it again on the page it landed you on
/// has to reach a different article, which is what the `n` the page carries is for.
#[test]
fn pressing_random_again_walks_on_instead_of_serving_the_same_article() {
    for (name, source) in SITES {
        let file: Value = serde_json::from_str(source).expect("site file parses");
        let mut state = WikiService
            .initialize(file["initial_state"].clone(), &ctx())
            .expect("seed loads");
        let host = file["domains"][0].as_str().expect("a first domain").to_owned();
        let mut path = "/wiki/Special:Random".to_owned();
        let mut seen: Vec<String> = Vec::new();
        for step in 0..3 {
            let response = WikiService
                .handle(&mut state, &ctx(), &HttpRequest::get(format!("http://{host}{path}")))
                .expect("the service answers");
            assert_eq!(response.status, 200, "{name} {path}");
            let html = String::from_utf8_lossy(&response.body).into_owned();
            let doc = cw_web::html::parse(&html);
            let title = *doc.by_id("article-title").first().expect("a title");
            let title = doc.text_content(title);
            assert!(!seen.contains(&title), "{name}: step {step} served {title} again");
            seen.push(title);
            let next = *doc.by_id("nav-random").first().expect("a Random control");
            let next = doc.attr(next, "href").expect("Random has an href").to_owned();
            assert_ne!(next, path, "{name}: Random points at the page it is on");
            path = next;
        }
    }
}

/// The classes the three sheets draw as pressable — the bordered pill of an IMDb genre, the
/// filled `.btn`, a raised `.card` — must only ever land on a control. A border, a radius and
/// a filled background are an affordance even with no pointer cursor and no hover rule, which
/// is more than a cascade-based audit can see, so the sheets' own vocabulary is asserted here.
const PRESSABLE: &[&str] = &["cat", "btn", "card", "chip", "pill", "headline"];

#[test]
fn every_class_the_sheets_draw_as_pressable_lands_on_a_control() {
    for (name, source) in SITES {
        let file: Value = serde_json::from_str(source).expect("site file parses");
        let mut state = WikiService
            .initialize(file["initial_state"].clone(), &ctx())
            .expect("seed loads");
        let loaded: WikiState = serde_json::from_value(state.clone()).expect("typed state");
        let host = file["domains"][0].as_str().expect("a first domain").to_owned();
        let mut paths = vec!["/".to_owned(), "/search?q=the".to_owned()];
        for article in loaded.articles.values() {
            paths.push(format!("/wiki/{}", article.id));
            paths.push(format!("/wiki/Talk:{}", article.id));
            paths.push(format!("/wiki/Special:History/{}", article.id));
            for category in &article.categories {
                paths.push(format!("/wiki/Category:{}", category.replace(' ', "_")));
            }
            for section in &article.sections {
                paths.push(format!("/wiki/{}?section={}", article.id, section.id));
            }
        }
        let mut chips = 0usize;
        for path in &paths {
            let response = WikiService
                .handle(&mut state, &ctx(), &HttpRequest::get(format!("http://{host}{path}")))
                .expect("the service answers");
            assert_eq!(response.status, 200, "{name}{path}");
            let html = String::from_utf8_lossy(&response.body).into_owned();
            let doc = cw_web::html::parse(&html);
            for node in doc.descendants(cw_web::dom::Document::ROOT) {
                if !doc.is_element(node) || !PRESSABLE.iter().any(|c| doc.has_class(node, c)) {
                    continue;
                }
                let tag = doc.tag(node).unwrap_or("");
                let id = doc.attr(node, "id").unwrap_or("");
                assert!(
                    tag == "button" || tag == "a" && doc.attr(node, "href").is_some_and(|h| !h.trim().is_empty()),
                    "{name}{path}: <{tag} id={id:?}> is drawn as pressable but is not a control"
                );
                chips += 1;
            }
        }
        assert!(chips > 0, "{name}: the sheets' pressable classes went unused");
    }
}
