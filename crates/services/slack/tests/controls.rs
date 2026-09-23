//! No dead controls. The workspace is crawled from every page kind it has — the
//! conversation, a DM, the thread pane, the member list, the DMs tab, Activity and
//! search — and every link, form action and `formaction` it draws is asked for:
//! nothing answers 404 or 405, no control leads back to the page it is on, and no
//! element is drawn as if it could be pressed when it cannot.
//!
//! Each request runs against a scratch copy of the seed, so the `POST` probes the
//! sweep makes (sending, reacting, pinning, marking read, opening a DM, setting a
//! status) never change what the crawl is reading.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::audit;
use cw_service_slack::SlackService;
use serde_json::Value;

const ACTOR: &str = "alice";

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: ACTOR.into(),
        source: "alice-mac".into(),
        tick: 60,
        seed: 1,
        instance: "slack".into(),
    }
}
fn seeded() -> Value {
    let raw = std::fs::read_to_string(cw_service_common::reference::reference_site_path("slack"))
        .expect("site");
    let site: Value = serde_json::from_str(&raw).expect("site file parses");
    SlackService
        .initialize(site["initial_state"].clone(), &ctx())
        .unwrap()
}

#[test]
fn every_control_of_the_workspace_leads_somewhere() {
    let seed = seeded();
    // Slack keeps the open channel, the open thread and the tab you are on clickable,
    // as the real client does; everything else must lead somewhere new.
    let mut allow: Vec<String> = ["rail-home", "rail-dms", "rail-activity", "search"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    for id in seed["channels"].as_object().unwrap().keys() {
        allow.push(format!("nav-{id}"));
    }
    for id in seed["dms"].as_object().unwrap().keys() {
        allow.push(format!("dm-{id}"));
    }
    for conversation in seed["channels"]
        .as_object()
        .unwrap()
        .values()
        .chain(seed["dms"].as_object().unwrap().values())
    {
        for m in conversation["messages"].as_array().unwrap() {
            let id = m["id"].as_str().unwrap();
            allow.push(format!("{id}-open-thread"));
            allow.push(format!("{id}-replies"));
        }
    }
    let allow: Vec<&str> = allow.iter().map(String::as_str).collect();

    let mut call = |method: &str, path: &str| {
        let mut scratch = seed.clone();
        let mut request = HttpRequest::get(format!("http://slack.com{path}"));
        request.method = method.to_ascii_uppercase();
        if request.method == "POST" {
            request.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
        }
        let response = SlackService.handle(&mut scratch, &ctx(), &request).unwrap();
        (
            response.status,
            String::from_utf8(response.body).unwrap_or_default(),
        )
    };
    let faults = audit::Sweep::new(
        &[
            "/",
            "/dms",
            "/activity",
            "/search",
            "/search?q=windows",
            "/channels/eng?members=1",
            "/channels/eng?thread=chat-1",
            "/channels/alice|bob",
            "/archives/random",
        ],
        &mut call,
    )
    .allow_self(&allow)
    .limit(150)
    .run();
    assert!(faults.is_empty(), "dead controls:\n{}", faults.join("\n"));
}

/// The classes the sheet draws as pressable belong to controls and nothing else.
///
/// A sweep cannot catch a `<span>` that only *looks* like a button — a pill with a
/// border, a disc with an icon in it — when no `cursor` or `:hover` rule gives it
/// away. So the look itself is pinned here: every element wearing one of the
/// stylesheet's control classes has to be an `<a>` or a `<button>`.
#[test]
fn nothing_inert_wears_a_control_s_clothes() {
    use cw_web::dom::Document as Dom;
    const PRESSABLE: &[&str] = &["chip", "send", "tab", "row", "tool", "close", "hit", "lens"];
    let seed = seeded();
    for path in [
        "/",
        "/dms",
        "/activity",
        "/search?q=windows",
        "/channels/eng",
        "/channels/eng?members=1",
        "/channels/eng?thread=chat-1",
        "/channels/alice|bob",
    ] {
        let mut scratch = seed.clone();
        let response = SlackService
            .handle(
                &mut scratch,
                &ctx(),
                &HttpRequest::get(format!("http://slack.com{path}")),
            )
            .unwrap();
        let doc = cw_web::html::parse(&String::from_utf8(response.body).unwrap());
        for node in doc.descendants(Dom::ROOT) {
            if !doc.is_element(node) {
                continue;
            }
            for class in PRESSABLE {
                if doc.has_class(node, class) {
                    let tag = doc.tag(node).unwrap_or("?");
                    assert!(
                        matches!(tag, "a" | "button"),
                        "{path}: <{tag} id={:?}> wears .{class} but cannot be pressed",
                        doc.attr(node, "id").unwrap_or_default()
                    );
                }
            }
        }
    }
}
