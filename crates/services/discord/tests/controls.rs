//! No dead controls. The server is crawled from every page kind it has — the channel,
//! the channel with the member list closed, the composer answering a message, the
//! server with nothing open, and search — and every link, form action and `formaction`
//! it draws is asked for: nothing answers 404 or 405, no control leads back to the
//! page it is on, and no element is drawn as if it could be pressed when it cannot.
//!
//! Each request runs against a scratch copy of the seed, so the `POST` probes the
//! sweep makes (sending, reacting, joining and leaving voice) never change what the
//! crawl is reading.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::audit;
use cw_service_discord::DiscordService;
use serde_json::Value;

const ACTOR: &str = "alice";

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: ACTOR.into(),
        source: "alice-mac".into(),
        tick: 60,
        seed: 1,
        instance: "discord".into(),
    }
}
fn seeded() -> Value {
    let raw =
        std::fs::read_to_string("../../../worlds/company-2026/sites/discord.json").expect("site");
    let site: Value = serde_json::from_str(&raw).expect("site file parses");
    DiscordService
        .initialize(site["initial_state"].clone(), &ctx())
        .unwrap()
}

#[test]
fn every_control_of_the_server_leads_somewhere() {
    let seed = seeded();
    // Discord keeps the open channel, the server you are in and the search box you
    // searched from clickable; everything else must lead somewhere new.
    let mut allow: Vec<String> = ["rail-home", "rail-server", "search"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    for id in seed["server"]["channels"].as_object().unwrap().keys() {
        allow.push(format!("nav-{id}"));
    }
    let allow: Vec<&str> = allow.iter().map(String::as_str).collect();

    let mut call = |method: &str, path: &str| {
        let mut scratch = seed.clone();
        let mut request = HttpRequest::get(format!("http://discord.com{path}"));
        request.method = method.to_ascii_uppercase();
        if request.method == "POST" {
            request.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
        }
        let response = DiscordService
            .handle(&mut scratch, &ctx(), &request)
            .unwrap();
        (
            response.status,
            String::from_utf8(response.body).unwrap_or_default(),
        )
    };
    let faults = audit::Sweep::new(
        &[
            "/",
            "/channels/@me",
            "/channels/atlas",
            "/channels/atlas/welcome",
            "/channels/atlas/atlas-help",
            "/channels/atlas/atlas-help?members=0",
            "/channels/atlas/atlas-help?reply_to=chat-45",
            "/channels/atlas/mod-log",
            "/channels/showcase",
            "/search",
            "/search?q=replay",
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
    const PRESSABLE: &[&str] = &[
        "server",
        "channel",
        "tool",
        "reaction",
        "send",
        "hit",
        "search-icon",
        "quick",
    ];
    let seed = seeded();
    for path in [
        "/",
        "/channels/@me",
        "/channels/atlas/welcome",
        "/channels/atlas/atlas-help",
        "/channels/atlas/atlas-help?members=0",
        "/channels/atlas/atlas-help?reply_to=chat-45",
        "/search?q=replay",
    ] {
        let mut scratch = seed.clone();
        let response = DiscordService
            .handle(
                &mut scratch,
                &ctx(),
                &HttpRequest::get(format!("http://discord.com{path}")),
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
