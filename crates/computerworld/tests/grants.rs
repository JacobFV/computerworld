//! Capability gates. An actor may only do what its session was granted, and the
//! gateway still stands between a granted actor and anything outside the world.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, HttpRequest};
use serde_json::{json, Value};

fn session(world: &mut World, actions: &[&str]) -> String {
    world
        .environment(EnvironmentConfig {
            actor: "alice".into(),
            machines: vec!["alice-mac".into()],
            actions: actions.iter().map(|a| (*a).to_string()).collect(),
            observations: vec!["semantic.v1".into()],
            action_budget: 64,
        })
        .unwrap()
}
fn run(world: &mut World, id: &str, family: &str, op: &str, payload: Value) -> (bool, String) {
    let result = world
        .step(
            id,
            vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
        )
        .unwrap();
    let outcome = &result.outcomes[0];
    (
        outcome.success,
        outcome
            .error
            .as_ref()
            .map(|e| e.code.clone())
            .unwrap_or_default(),
    )
}
fn request(url: &str) -> Value {
    serde_json::to_value(HttpRequest::json("GET", url, &json!({})).unwrap()).unwrap()
}

#[test]
fn a_family_the_session_was_not_granted_is_refused_before_it_reaches_the_machine() {
    let mut world = World::new(reference_world(), 1).unwrap();
    // Raw HTTP is its own capability: holding the terminal does not imply holding it.
    let terminal_only = session(&mut world, &["terminal.v1"]);
    let (ok, code) = run(
        &mut world,
        &terminal_only,
        "http.v1",
        "request",
        request("http://intranet.internal/"),
    );
    assert!(!ok, "an ungranted family was dispatched");
    assert_eq!(code, "denied");
    // And the grant really is what unlocks it.
    let with_http = session(&mut world, &["http.v1"]);
    let (ok, _) = run(
        &mut world,
        &with_http,
        "http.v1",
        "request",
        request("http://intranet.internal/"),
    );
    assert!(ok, "a granted family was refused");
}

#[test]
fn a_machine_outside_the_session_is_refused_even_with_the_family() {
    let mut world = World::new(reference_world(), 1).unwrap();
    let id = session(&mut world, &["terminal.v1"]);
    let result = world
        .step(
            &id,
            vec![ActionEnvelope::new(
                "terminal.v1",
                "execute",
                "bob-windows",
                json!({"command":"whoami"}),
            )],
        )
        .unwrap();
    assert!(!result.outcomes[0].success);
    assert_eq!(result.outcomes[0].error.as_ref().unwrap().code, "denied");
}

#[test]
fn the_gateway_still_stands_between_a_granted_actor_and_the_host() {
    let mut world = World::new(reference_world(), 1).unwrap();
    let id = session(&mut world, &["http.v1"]);
    // `allow_host` is false, so a granted actor still cannot reach outside the world.
    for url in [
        "http://example.com/",
        "https://anthropic.com/",
        "http://127.0.0.1/",
    ] {
        let (ok, _) = run(&mut world, &id, "http.v1", "request", request(url));
        assert!(!ok, "{url} escaped the gateway");
    }
}

#[test]
fn an_actor_session_cannot_reach_owner_only_state() {
    let mut world = World::new(reference_world(), 1).unwrap();
    let id = session(&mut world, &["terminal.v1"]);
    // Privileged reads exist for grading, and must not be an action family.
    for family in ["snapshot.v1", "inspect.v1", "world.v1", "poison.v1"] {
        let (ok, code) = run(&mut world, &id, family, "read", json!({}));
        assert!(!ok, "{family} was dispatched to an actor session");
        assert_eq!(code, "denied", "{family}");
    }
}

#[test]
fn a_denied_action_reports_no_effect_and_changes_no_machine_state() {
    let mut world = World::new(reference_world(), 1).unwrap();
    let id = session(&mut world, &["terminal.v1"]);
    let tick = world.runtime().tick();
    let files = world
        .runtime()
        .computer("alice-mac")
        .unwrap()
        .vfs
        .all_files()
        .clone();
    let events = world.trajectory().len();
    let result = world
        .step(
            &id,
            vec![ActionEnvelope::new(
                "http.v1",
                "request",
                "alice-mac",
                request("http://intranet.internal/"),
            )],
        )
        .unwrap();
    // No effect, because the action never reached a machine to have one.
    assert!(result.outcomes[0].effect.is_none());
    assert_eq!(world.runtime().tick(), tick, "a refusal advanced the clock");
    assert_eq!(
        world
            .runtime()
            .computer("alice-mac")
            .unwrap()
            .vfs
            .all_files(),
        files,
        "a refusal touched the filesystem"
    );
    // It is recorded, though: a refusal an operator cannot see is worse than none.
    assert_eq!(
        world.trajectory().len(),
        events + 1,
        "a refusal left no trace in the trajectory"
    );
}
