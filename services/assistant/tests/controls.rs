//! Nothing on either assistant looks like a control it is not.
//!
//! `cw_service_common::audit` crawls the service the way the browser would: from the
//! home page it follows every same-origin link, probes every form `action` and every
//! `formaction` with the method written on it, and reports anything that answers 404,
//! 405 or worse, anything that only leads back to the page it is on, and — per page —
//! every anchor with nowhere to go, every button outside a form, every field with no
//! name or no label, every fake `role`, and every inert element the cascade draws with
//! `cursor: pointer`.
//!
//! Both skins and all three page kinds are covered: `chatgpt` through openai.json,
//! `claude` through anthropic.json, the conversation page through the seeded history the
//! crawl walks into, and the empty home a site bound before its content lands serves.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_assistant::{AssistantService, AssistantState};
use cw_service_common as web;
use cw_service_common::audit;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const OPENAI: &str = include_str!("../../../worlds/company-2026/sites/openai.json");
const ANTHROPIC: &str = include_str!("../../../worlds/company-2026/sites/anthropic.json");

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 7,
        seed: 1,
        instance: "assistant".into(),
    }
}

/// Every complaint the sweep has about one seed, read by one person.
///
/// The probes really run, and a probed `POST` really regenerates or deletes, so each
/// request is served by its own copy of the seeded state: the crawl always reads the
/// site the seed describes, whatever order the probes went in.
fn sweep(initial: &Value, actor: &str, origin: &str) -> Vec<String> {
    let base = AssistantService
        .initialize(initial.clone(), &ctx(actor))
        .unwrap_or_else(|e| panic!("{origin}: {e:?}"));
    // The open conversation keeps its row in the history, marked `.on` and still a
    // link to itself, which is what both products do.
    let state: AssistantState = serde_json::from_value(base.clone()).unwrap();
    let mut current: Vec<String> = state
        .conversations
        .keys()
        .map(|id| format!("side-{id}"))
        .collect();
    // Both products keep "New chat" in the sidebar of the new-chat page itself.
    current.push("side-new".into());
    let current: Vec<&str> = current.iter().map(String::as_str).collect();
    let mut call = |method: &str, path: &str| {
        let mut state = base.clone();
        let mut request = HttpRequest::get(format!("{origin}{path}"));
        request.method = method.to_owned();
        if method != "GET" {
            request.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
        }
        match AssistantService.handle(&mut state, &ctx(actor), &request) {
            Ok(reply) => {
                let body = String::from_utf8_lossy(&reply.body).into_owned();
                // A page the engine would refuse is not a page: duplicate ids and
                // unknown CSS are caught here, on every path the crawl reaches.
                if reply.header("content-type") == Some(web::html::HTML_MEDIA_TYPE) {
                    web::html::validate_strict(&body)
                        .unwrap_or_else(|e| panic!("{method} {path} is not strict: {e:?}"));
                }
                (reply.status, body)
            }
            // A request a browser can make must never be a simulation error.
            Err(e) => (500, format!("{e:?}")),
        }
    };
    audit::Sweep::new(&["/"], &mut call)
        .allow_self(&current)
        .run()
}

/// The owners the seed names, so the crawl sees the conversation pages, plus the actor
/// a fresh visitor arrives as, who sees the sidebar with nothing in it.
fn readers(initial: &Value) -> BTreeSet<String> {
    let loaded = AssistantService
        .initialize(initial.clone(), &ctx("alice"))
        .unwrap();
    let state: AssistantState = serde_json::from_value(loaded).unwrap();
    let mut out: BTreeSet<String> = state
        .conversations
        .values()
        .map(|c| c.owner.clone())
        .collect();
    out.insert("alice".into());
    out
}

#[test]
fn no_page_of_either_assistant_offers_a_control_that_cannot_act() {
    let mut faults = Vec::new();
    for (source, origin) in [
        (OPENAI, "http://chatgpt.com"),
        (ANTHROPIC, "http://claude.ai"),
    ] {
        let site: Value = serde_json::from_str(source).unwrap();
        let initial = site["initial_state"].clone();
        for actor in readers(&initial) {
            faults.extend(
                sweep(&initial, &actor, origin)
                    .into_iter()
                    .map(|fault| format!("{origin} as {actor}: {fault}")),
            );
        }
    }
    // The empty site, in both skins: a home with no history, no suggestions and no
    // model line is still a page whose every control must work.
    for (skin, origin) in [
        ("chatgpt", "http://chatgpt.com"),
        ("claude", "http://claude.ai"),
    ] {
        faults.extend(
            sweep(&json!({ "skin": skin }), "alice", origin)
                .into_iter()
                .map(|fault| format!("empty {skin}: {fault}")),
        );
    }
    assert!(faults.is_empty(), "dead controls:\n{}", faults.join("\n"));
}

/// The page has to read as prose as well as answer as a service: the counts agree with
/// the nouns, and no heading promises something the page does not then show.
#[test]
fn the_pages_say_what_is_on_them() {
    for (source, origin) in [
        (OPENAI, "http://chatgpt.com"),
        (ANTHROPIC, "http://claude.ai"),
    ] {
        let site: Value = serde_json::from_str(source).unwrap();
        let mut state = AssistantService
            .initialize(site["initial_state"].clone(), &ctx("alice"))
            .unwrap();
        let home = AssistantService
            .handle(
                &mut state,
                &ctx("alice"),
                &HttpRequest::get(format!("{origin}/")),
            )
            .unwrap();
        let html = String::from_utf8(home.body).unwrap();
        let doc = cw_web::html::parse(&html);
        let text = doc.text_content(cw_web::dom::Document::ROOT);
        for lie in [
            "1 items",
            "1 conversations",
            "TODO",
            "Lorem ipsum",
            "undefined",
            "null",
        ] {
            assert!(!text.contains(lie), "{origin} home says {lie:?}");
        }
        // "Chats"/"Recents" heads the history, and the history is there to head.
        let label = *doc.by_id("side-label").first().expect("a history label");
        assert!(matches!(
            doc.text_content(label).as_str(),
            "Chats" | "Recents"
        ));
        let listed = doc
            .descendants(cw_web::dom::Document::ROOT)
            .filter(|n| doc.is(*n, "a"))
            .filter(|n| {
                doc.attr(*n, "id")
                    .is_some_and(|id| id.starts_with("side-conv-"))
            })
            .count();
        assert!(listed > 0, "{origin} labels a history it does not show");
        assert!(
            doc.by_id("side-empty").is_empty(),
            "{origin} has history to show"
        );
    }
}
