//! The sixteen shipped static sites, served the way a reader meets them: every seeded
//! `Page` comes back as `text/html` the engine renders strictly, with the ids the Page
//! version addressed still on the elements that play the same role.
//!
//! `include_str!` rather than a file read: the seeds are build inputs, so a drift between
//! the world data and the crate that serves it fails to compile instead of at run time.
use cw_protocol::{HttpRequest, HttpResponse, Page};
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::{validate_strict, HTML_MEDIA_TYPE};
use cw_service_static_site::StaticSite;
use serde_json::Value;

static SITES: std::sync::LazyLock<Vec<(&'static str, &'static str)>> =
    std::sync::LazyLock::new(|| {
        vec![
            (
                "apple",
                cw_service_common::reference::reference_site_json("apple"),
            ),
            (
                "aws",
                cw_service_common::reference::reference_site_json("aws"),
            ),
            (
                "cloudflare",
                cw_service_common::reference::reference_site_json("cloudflare"),
            ),
            (
                "crates",
                cw_service_common::reference::reference_site_json("crates"),
            ),
            (
                "docsrs",
                cw_service_common::reference::reference_site_json("docsrs"),
            ),
            (
                "figma",
                cw_service_common::reference::reference_site_json("figma"),
            ),
            (
                "guide",
                include_str!("../../../../worlds/company-2026/services/guide/service.json"),
            ),
            (
                "intranet",
                include_str!("../../../../worlds/company-2026/services/intranet/service.json"),
            ),
            (
                "microsoft",
                cw_service_common::reference::reference_site_json("microsoft"),
            ),
            (
                "northstar-status",
                include_str!("../../../../worlds/company-2026/services/northstar-status/service.json"),
            ),
            (
                "northstar-www",
                include_str!("../../../../worlds/company-2026/services/northstar-www/service.json"),
            ),
            (
                "npmjs",
                cw_service_common::reference::reference_site_json("npmjs"),
            ),
            (
                "pypi",
                cw_service_common::reference::reference_site_json("pypi"),
            ),
            (
                "stripe",
                cw_service_common::reference::reference_site_json("stripe"),
            ),
            (
                "whatsapp",
                cw_service_common::reference::reference_site_json("whatsapp"),
            ),
            (
                "zoom",
                cw_service_common::reference::reference_site_json("zoom"),
            ),
        ]
    });

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 3,
        seed: 1,
        instance: "site".into(),
    }
}
/// The loaded state, the site's own origin, and the pages the search index points at.
fn site(source: &str) -> (Value, String, Vec<String>) {
    let file: Value = serde_json::from_str(source).expect("site file parses");
    let state = StaticSite
        .initialize(file["initial_state"].clone(), &ctx())
        .expect("seed passes the service's own gate");
    let origin = format!(
        "http://{}",
        file["domains"][0].as_str().expect("a first domain")
    );
    let entries = file["search_entries"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|e| e["url"].as_str().expect("entry has a url").to_owned())
                .collect()
        })
        .unwrap_or_default();
    (state, origin, entries)
}
fn get(state: &mut Value, url: &str) -> HttpResponse {
    StaticSite
        .handle(state, &ctx(), &HttpRequest::get(url))
        .expect("the service answers")
}
/// Every id a `Page` element tree declares, in document order.
fn page_ids(element: &Value, out: &mut Vec<String>) {
    if let Some(id) = element.get("id").and_then(Value::as_str) {
        out.push(id.to_owned());
    }
    for child in element
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        page_ids(child, out);
    }
}

/// Every seeded page is HTML by default, passes the strict validator, and keeps its ids:
/// the `"format": "page"` opt-in exists for a client that needs the native media type, and
/// no shipped site asks for it.
#[test]
fn every_seeded_page_is_strict_html_that_keeps_its_ids() {
    for &(name, source) in SITES.iter() {
        let (mut state, origin, entries) = site(source);
        assert!(
            state.get("format").is_none(),
            "{name} opts out of HTML with \"format\": {:?}; the escape hatch is for a client \
             that reads the element tree, not for a shipped site",
            state["format"]
        );
        let pages: Vec<(String, Value)> = state["pages"]
            .as_object()
            .expect("pages is an object")
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert!(!pages.is_empty(), "{name} seeds no pages");
        for (path, seeded) in &pages {
            let response = get(&mut state, &format!("{origin}{path}"));
            assert_eq!(response.status, 200, "{name}{path}");
            assert_eq!(
                response.header("content-type"),
                Some(HTML_MEDIA_TYPE),
                "{name}{path}"
            );
            let html = String::from_utf8(response.body).expect("utf-8");
            validate_strict(&html).unwrap_or_else(|e| panic!("{name}{path}: {e:?}"));
            let doc = cw_web::html::parse(&html);
            let page: Page = serde_json::from_value(seeded.clone()).expect("a version 1 page");
            let title = doc
                .descendants(cw_web::dom::Document::ROOT)
                .find(|n| doc.is(*n, "title"))
                .map(|n| doc.text_content(n))
                .unwrap_or_default();
            assert_eq!(title, page.title, "{name}{path} title");
            let mut ids = Vec::new();
            for element in seeded["elements"].as_array().into_iter().flatten() {
                page_ids(element, &mut ids);
            }
            assert!(!ids.is_empty(), "{name}{path} has no addressable element");
            for id in &ids {
                assert_eq!(doc.by_id(id).len(), 1, "{name}{path} lost #{id}");
            }
            // Every link that stays on the site has to resolve to a page of it.
            for node in doc.descendants(cw_web::dom::Document::ROOT) {
                let Some(href) = doc.attr(node, "href").filter(|h| h.starts_with('/')) else {
                    continue;
                };
                let href = href.to_owned();
                let landed = get(&mut state, &format!("{origin}{href}"));
                assert!(
                    matches!(landed.status, 200 | 301),
                    "{name}{path} links to {href}, which answers {}",
                    landed.status
                );
            }
        }
        // The pages the search engine indexes are the pages a reader can open.
        for url in &entries {
            let response = get(&mut state, url);
            assert!(
                matches!(response.status, 200 | 301),
                "{name}: indexed page {url} answers {}",
                response.status
            );
        }
        // Reading never mutates: the same request twice is the same answer.
        let (path, _) = &pages[0];
        let once = get(&mut state, &format!("{origin}{path}"));
        assert_eq!(
            once,
            get(&mut state, &format!("{origin}{path}")),
            "{name}{path} must be pure"
        );
    }
}

/// The shared record list is a page of its own, and it is HTML like everything else.
#[test]
fn the_records_page_is_strict_html_on_every_site() {
    for &(name, source) in SITES.iter() {
        let (mut state, origin, _) = site(source);
        let response = get(&mut state, &format!("{origin}/records"));
        assert_eq!(response.status, 200, "{name}/records");
        assert_eq!(
            response.header("content-type"),
            Some(HTML_MEDIA_TYPE),
            "{name}/records"
        );
        let html = String::from_utf8(response.body).expect("utf-8");
        validate_strict(&html).unwrap_or_else(|e| panic!("{name}/records: {e:?}"));
        assert!(
            cw_web::html::parse(&html).by_id("records").len() == 1,
            "{name}/records heading"
        );
    }
}
