//! Every control the Messages pages draw, checked against what the service answers.
//!
//! The sweep starts at the inbox, follows every same-origin link it finds, probes every
//! form `action` and `formaction` with the method written on it, and complains about
//! anything that answers 404, 405 or an error, about every dead control [`audit::page`]
//! finds on a page (an anchor with nowhere to go, a button in no form, a field with no
//! name or no label, an inert element drawn with `cursor: pointer`), and about any link
//! that leads back to the page it is on.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::audit;
use cw_service_messages::{MessagesService, MessagesState};
use serde_json::Value;

fn ctx(actor: &str, tick: u64) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: format!("{actor}-mac"),
        tick,
        seed: 1,
        instance: "messages".into(),
    }
}
fn seeded() -> Value {
    let raw =
        std::fs::read_to_string("../../../worlds/company-2026/services/messages/service.json").expect("site");
    let site: Value = serde_json::from_str(&raw).expect("site file parses");
    MessagesService
        .initialize(site["initial_state"].clone(), &ctx("alice", 0))
        .unwrap()
}

/// The sidebar row for the conversation already open points at the page it is on, as
/// the selected row does in Messages for Mac. Every other control must go somewhere.
fn open_rows(state: &Value) -> Vec<String> {
    let s: MessagesState = serde_json::from_value(state.clone()).unwrap();
    s.conversations
        .keys()
        .map(|id| format!("row-{id}"))
        .collect()
}

#[test]
fn every_control_on_every_messages_page_can_act() {
    let seed = seeded();
    let rows = open_rows(&seed);
    let allow: Vec<&str> = rows.iter().map(String::as_str).collect();
    // Every kind of person the seed has: two iMessage people, a third in the group, the
    // SMS-only contact, and a stranger whose device has no handle at all.
    for actor in ["alice", "bob", "carol", "dentist", "eve"] {
        let mut state = seed.clone();
        let mut call = |method: &str, path: &str| -> (u16, String) {
            let url = format!("http://messages.internal{path}");
            let response = if method == "POST" {
                let mut request = HttpRequest::get(url);
                request.method = "POST".into();
                request.headers.insert(
                    "content-type".into(),
                    "application/x-www-form-urlencoded".into(),
                );
                // A POST probe really runs, so it runs against a scratch copy of the
                // state and leaves the pages the crawl is reading alone.
                let mut scratch = state.clone();
                MessagesService.handle(&mut scratch, &ctx(actor, 61), &request)
            } else {
                MessagesService.handle(&mut state, &ctx(actor, 60), &HttpRequest::get(url))
            }
            .unwrap();
            (response.status, String::from_utf8(response.body).unwrap())
        };
        let faults = audit::Sweep::new(&["/", "/conversations"], &mut call)
            .allow_self(&allow)
            .run();
        assert!(faults.is_empty(), "{actor}:\n{}", faults.join("\n"));
    }
}

/// The composer draws only what it can do. It carried an app-strip "+" with an "Apps"
/// tooltip and nothing behind it: a round grey button that swallowed every click. This
/// world runs no page script, so there is no attach, camera, emoji or app drawer to be
/// had — anything in the composer that is not a field or the button that sends it is a
/// promise the service cannot keep.
#[test]
fn the_composer_promises_nothing_it_cannot_do() {
    let mut state = seeded();
    let response = MessagesService
        .handle(
            &mut state,
            &ctx("alice", 60),
            &HttpRequest::get("http://messages.internal/conversations/+14155550100|+14155550101"),
        )
        .unwrap();
    assert_eq!(response.status, 200);
    let body = String::from_utf8(response.body).unwrap();
    cw_service_common::html::validate_strict(&body).unwrap_or_else(|e| panic!("{e:?}"));
    let doc = cw_web::html::parse(&body);
    let composer = *doc.by_id("send").first().expect("the composer");
    for node in doc.descendants(composer) {
        if !doc.is_element(node) || node == composer {
            continue;
        }
        let tag = doc.tag(node).unwrap_or("?");
        assert!(
            matches!(tag, "div" | "input" | "button"),
            "the composer draws <{tag}>, which is neither a field nor the button that sends it"
        );
    }
}
