//! The shipped Slack and Discord seeds: they initialise, every channel renders, and every
//! search entry resolves to a real page.
use cw_protocol::{HttpRequest, Page};
use cw_sdk::{Service, ServiceContext};
use cw_service_chat::ChatService;
use serde_json::Value;

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 60,
        seed: 1,
        instance: "slack".into(),
    }
}
fn site(name: &str) -> Value {
    let raw = std::fs::read_to_string(format!("../../worlds/company-2026/sites/{name}.json"))
        .expect("site file");
    serde_json::from_str(&raw).expect("site file parses")
}
fn check(name: &str, paths: &[&str]) -> Value {
    let site = site(name);
    let host = site["domains"][0].as_str().unwrap().to_owned();
    let mut state = ChatService
        .initialize(site["initial_state"].clone(), &ctx("alice"))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    for path in paths {
        let r = ChatService
            .handle(
                &mut state,
                &ctx("alice"),
                &HttpRequest::get(format!("http://{host}{path}")),
            )
            .unwrap();
        assert_eq!(r.status, 200, "{name}{path}");
        serde_json::from_slice::<Page>(&r.body)
            .unwrap()
            .validate()
            .unwrap_or_else(|e| panic!("{name}{path}: {e}"));
    }
    for entry in site["search_entries"].as_array().unwrap() {
        let url = entry["url"].as_str().unwrap();
        let r = ChatService
            .handle(&mut state, &ctx("alice"), &HttpRequest::get(url))
            .unwrap();
        assert_eq!(r.status, 200, "{name} search entry {url}");
    }
    state
}

#[test]
fn slack_seed_carries_the_storylines_it_is_supposed_to() {
    let state = check(
        "slack",
        &[
            "/",
            "/channels/eng",
            "/channels/random",
            "/channels/incidents",
            "/channels/general",
            "/archives/eng",
            "/channels/alice|bob",
        ],
    );
    let all = state.to_string();
    // 3 the flaky test, 5 the monitor, 7 the soundtrack, 10 the outage, 1 the launch.
    assert!(all.contains("stackoverflow.com/questions/t-4411"));
    assert!(all.contains("amazon.com/dp/b0monitor27"));
    assert!(all.contains("open.spotify.com/playlist/ship-it"));
    assert!(all.contains("status.northstar.example"));
    assert!(all.contains("docs.google.com/documents/atlas-launch"));
    // The release code is a search task, and Slack is not one of its three homes.
    assert!(!all.contains("ATLAS-2026"));
}

#[test]
fn discord_seed_renders_its_community_channels() {
    check(
        "discord",
        &[
            "/",
            "/channels/welcome",
            "/channels/atlas-help",
            "/channels/showcase",
        ],
    );
}
