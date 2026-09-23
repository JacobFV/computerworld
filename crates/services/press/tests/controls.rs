//! No dead controls: every publication, every skin, every page kind.
//!
//! The sweep starts at the front page and the standing lists, follows every same-origin
//! link it meets, probes every form `action` and `formaction` with the method written on
//! it, and asserts the service answers each with something other than a 404, a 405 or an
//! error. Along the way it audits each rendered page for the subtler lies — a control
//! with nowhere to go, a field outside a form, a role or a pointer cursor on something
//! that cannot act.
//!
//! A `POST` probe really mutates, so the probes run against a scratch clone of the state
//! and never disturb the crawl.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::audit;
use cw_service_press::PressService;
use serde_json::Value;

static SITES: std::sync::LazyLock<Vec<(&'static str, &'static str)>> =
    std::sync::LazyLock::new(|| {
        vec![
            (
                "reuters",
                cw_service_common::reference::reference_site_json("reuters"),
            ),
            (
                "theverge",
                cw_service_common::reference::reference_site_json("theverge"),
            ),
            (
                "arstechnica",
                cw_service_common::reference::reference_site_json("arstechnica"),
            ),
            (
                "alice-blog",
                include_str!("../../../../worlds/company-2026/services/alice-blog/service.json"),
            ),
            (
                "bob-blog",
                include_str!("../../../../worlds/company-2026/services/bob-blog/service.json"),
            ),
            (
                "northstar-eng",
                include_str!("../../../../worlds/company-2026/services/northstar-eng/service.json"),
            ),
            (
                "nytimes",
                cw_service_common::reference::reference_site_json("nytimes"),
            ),
            (
                "bbc",
                cw_service_common::reference::reference_site_json("bbc"),
            ),
            (
                "cnn",
                cw_service_common::reference::reference_site_json("cnn"),
            ),
            (
                "google-news",
                cw_service_common::reference::reference_site_json("google-news"),
            ),
            (
                "medium",
                cw_service_common::reference::reference_site_json("medium"),
            ),
            (
                "substack",
                cw_service_common::reference::reference_site_json("substack"),
            ),
        ]
    });

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 1,
        instance: "press".into(),
    }
}

/// The ids whose link may lead back to the page it is on, because the publication it
/// stands in for does the same: the wordmark is always a link home, the section a reader
/// is already in is still the nav entry for it, and the footer repeats the nav.
fn self_links(state: &Value) -> Vec<String> {
    let mut ids = vec![
        "masthead-home".to_owned(),
        "masthead-archive".to_owned(),
        "masthead-saved".to_owned(),
        "foot-archive".to_owned(),
        "foot-saved".to_owned(),
    ];
    for section in state["sections"].as_array().into_iter().flatten() {
        let id = section["id"].as_str().unwrap_or_default();
        ids.push(format!("masthead-{id}"));
        ids.push(format!("foot-{id}"));
    }
    for tag in tags(state) {
        ids.push(format!("topic-{tag}"));
    }
    ids
}

fn tags(state: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for article in state["articles"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, a)| a)
    {
        for tag in article["tags"].as_array().into_iter().flatten() {
            let tag = tag.as_str().unwrap_or_default().to_owned();
            if !tag.is_empty() && !out.contains(&tag) {
                out.push(tag);
            }
        }
    }
    out
}

/// Every seeded publication, crawled end to end with every control exercised.
#[test]
fn no_control_on_any_publication_is_a_lie() {
    for (name, source) in SITES.iter().copied() {
        let file: Value = serde_json::from_str(source).expect("site file parses");
        let mut state = PressService
            .initialize(file["initial_state"].clone(), &ctx())
            .expect("seed passes the service's own gate");
        let origin = format!(
            "http://{}",
            file["domains"][0].as_str().expect("a first domain")
        );
        let allow: Vec<String> = self_links(&state);
        let allow: Vec<&str> = allow.iter().map(String::as_str).collect();
        let mut call = |method: &str, path: &str| {
            let mut request = HttpRequest::get(format!("{origin}{path}"));
            request.method = method.to_owned();
            // A POST probe writes, so it writes to a copy: the crawl keeps reading the
            // state the seed made.
            let mut scratch = state.clone();
            let target = if method == "GET" {
                &mut state
            } else {
                &mut scratch
            };
            let response = PressService
                .handle(target, &ctx(), &request)
                .expect("the service answers");
            (
                response.status,
                String::from_utf8_lossy(&response.body).into_owned(),
            )
        };
        let faults = audit::Sweep::new(&["/", "/archive", "/saved"], &mut call)
            .allow_self(&allow)
            .run();
        assert!(faults.is_empty(), "{name}:\n  {}", faults.join("\n  "));
    }
}

/// The same sweep over every one of the twelve stylesheets on every one of the three
/// layouts: a skin is CSS, and CSS is where a decorative pointer cursor hides.
#[test]
fn no_skin_draws_a_control_it_cannot_honour() {
    let base: Value = serde_json::from_str(SITES[0].1).expect("site file parses");
    for layout in ["wire", "magazine", "blog"] {
        for skin in [
            "plain", "blog", "nyt", "bbc", "cnn", "reuters", "verge", "ars", "gnews", "medium",
            "substack",
        ] {
            let mut initial = base["initial_state"].clone();
            initial["layout"] = Value::String(layout.to_owned());
            initial["skin"] = Value::String(skin.to_owned());
            let mut state = PressService
                .initialize(initial, &ctx())
                .expect("seed loads");
            let allow: Vec<String> = self_links(&state);
            let allow: Vec<&str> = allow.iter().map(String::as_str).collect();
            let mut call = |method: &str, path: &str| {
                let mut request = HttpRequest::get(format!("http://press.example{path}"));
                request.method = method.to_owned();
                let mut scratch = state.clone();
                let target = if method == "GET" {
                    &mut state
                } else {
                    &mut scratch
                };
                let response = PressService
                    .handle(target, &ctx(), &request)
                    .expect("the service answers");
                (
                    response.status,
                    String::from_utf8_lossy(&response.body).into_owned(),
                )
            };
            let faults = audit::Sweep::new(&["/", "/archive", "/saved"], &mut call)
                .allow_self(&allow)
                .limit(40)
                .run();
            assert!(
                faults.is_empty(),
                "{skin}/{layout}:\n  {}",
                faults.join("\n  ")
            );
        }
    }
}

/// The two pages a crawl from the front page cannot reach: the brand splash a seed with
/// no articles serves, and a topic nobody has written about.
#[test]
fn the_splash_and_an_empty_topic_promise_nothing_either() {
    for skin in [
        "plain", "blog", "nyt", "bbc", "cnn", "reuters", "verge", "ars", "gnews", "medium",
        "substack",
    ] {
        for layout in ["wire", "magazine", "blog"] {
            let seed = serde_json::json!({"layout": layout, "skin": skin, "brand": "Soon"});
            let mut state = PressService.initialize(seed, &ctx()).expect("seed loads");
            let response = PressService
                .handle(
                    &mut state,
                    &ctx(),
                    &HttpRequest::get("http://press.example/"),
                )
                .expect("the service answers");
            let html = String::from_utf8_lossy(&response.body).into_owned();
            let faults = audit::page(&html);
            assert!(
                faults.is_empty(),
                "{skin}/{layout} splash:\n  {}",
                faults.join("\n  ")
            );
        }
    }
    let file: Value = serde_json::from_str(SITES[0].1).expect("site file parses");
    let mut state = PressService
        .initialize(file["initial_state"].clone(), &ctx())
        .expect("seed loads");
    let response = PressService
        .handle(
            &mut state,
            &ctx(),
            &HttpRequest::get("http://press.example/tag/nothing-tagged"),
        )
        .expect("the service answers");
    let html = String::from_utf8_lossy(&response.body).into_owned();
    let faults = audit::page(&html);
    assert!(faults.is_empty(), "empty topic:\n  {}", faults.join("\n  "));
}

/// The masthead Follow and the newsletter sit on every page, so the `return` they carry has
/// to be the page they were pressed from. A control that silently throws the reader back to
/// the front page is not the control it says it is.
#[test]
fn a_control_in_the_furniture_comes_back_to_the_page_it_was_pressed_from() {
    for (name, source) in SITES.iter().copied() {
        let file: Value = serde_json::from_str(source).expect("site file parses");
        let mut state = PressService
            .initialize(file["initial_state"].clone(), &ctx())
            .expect("seed loads");
        let blog = state["layout"] == "blog";
        let first = state["articles"]
            .as_object()
            .expect("articles is an object")
            .keys()
            .next()
            .expect("at least one article")
            .clone();
        let year = state["articles"][&first]["year"]
            .as_str()
            .unwrap_or("2026")
            .to_owned();
        let story = match blog {
            true => format!("/posts/{first}"),
            false => format!("/{year}/{first}"),
        };
        for path in ["/", "/archive", "/saved", story.as_str()] {
            let response = PressService
                .handle(
                    &mut state,
                    &ctx(),
                    &HttpRequest::get(format!("http://press.example{path}")),
                )
                .expect("the service answers");
            assert_eq!(response.status, 200, "{name}{path}");
            let html = String::from_utf8_lossy(&response.body).into_owned();
            let doc = cw_web::html::parse(&html);
            for form in ["masthead-follow-form", "subscribe"] {
                let node = *doc
                    .by_id(form)
                    .first()
                    .unwrap_or_else(|| panic!("{name}{path}: no #{form}"));
                let back = doc
                    .descendants(node)
                    .filter(|n| doc.is(*n, "input") && doc.attr(*n, "name") == Some("return"))
                    .find_map(|n| doc.attr(n, "value").map(str::to_owned))
                    .unwrap_or_else(|| panic!("{name}{path}: #{form} carries no return"));
                assert_eq!(back, path, "{name}{path}: #{form} returns elsewhere");
            }
        }
    }
}

/// The classes the twelve sheets draw as pressable — the rounded `.pill`, the `.chip` of a
/// topic, a `.card` — must only ever land on a control. A border and a radius are an
/// affordance even with no pointer cursor and no hover rule, which is more than a
/// cascade-based audit can see, so the sheets' own vocabulary is asserted here.
const PRESSABLE: &[&str] = &["pill", "chip", "card", "saved", "brand"];

#[test]
fn every_class_the_sheets_draw_as_pressable_lands_on_a_control() {
    for (name, source) in SITES.iter().copied() {
        let file: Value = serde_json::from_str(source).expect("site file parses");
        let mut state = PressService
            .initialize(file["initial_state"].clone(), &ctx())
            .expect("seed loads");
        let origin = format!(
            "http://{}",
            file["domains"][0].as_str().expect("a first domain")
        );
        let blog = state["layout"] == "blog";
        let mut paths = vec!["/".to_owned(), "/archive".to_owned(), "/saved".to_owned()];
        for tag in tags(&state) {
            paths.push(format!("/tag/{tag}"));
        }
        let articles: Vec<String> = state["articles"]
            .as_object()
            .expect("articles is an object")
            .keys()
            .cloned()
            .collect();
        for id in &articles {
            let year = state["articles"][id]["year"]
                .as_str()
                .unwrap_or("2026")
                .to_owned();
            paths.push(match blog {
                true => format!("/posts/{id}"),
                false => format!("/{year}/{id}"),
            });
        }
        let mut chips = 0usize;
        for path in &paths {
            let response = PressService
                .handle(
                    &mut state,
                    &ctx(),
                    &HttpRequest::get(format!("{origin}{path}")),
                )
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
                    tag == "button"
                        || tag == "a"
                            && doc.attr(node, "href").is_some_and(|h| !h.trim().is_empty()),
                    "{name}{path}: <{tag} id={id:?}> is drawn as pressable but is not a control"
                );
                chips += 1;
            }
        }
        assert!(
            chips > 0,
            "{name}: the sheets' pressable classes went unused"
        );
    }
}
