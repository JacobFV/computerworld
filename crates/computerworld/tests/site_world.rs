//! The world the live machines on the project site run in, checked against the engine.
//!
//! `scripts/content/build-live-world.mjs` writes `site/generated/world-definition.js` from the reference
//! world: seven devices on five graphical OS profiles, with the native applications and
//! documents each platform ships. `site/live.js` imports that file and nothing else, so
//! a definition that no longer boots is a broken home page.
//!
//! This carries the engine half of the retired `scripts/test-browser.mjs`: every computer
//! in that world boots and runs a command, the services it declares are exactly the
//! reference world's (so `--test internet_links`, which asks every declared domain to
//! answer, covers this world too), and the gateway still refuses a name the world does
//! not declare. The half that drove the console's own controls went with the console.
use computerworld::{reference_world, World, WorldDefinition};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::json;

/// The generated definition, read from the file the site imports.
fn live_world() -> WorldDefinition {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../site/generated/world-definition.js"
    ))
    .expect("site/generated/world-definition.js is missing: run scripts/build-content.sh");
    let json = text
        .split_once("export default ")
        .expect("site/generated/world-definition.js is not the generated module")
        .1
        .trim()
        .trim_end_matches(';');
    serde_json::from_str(json).expect("the generated world definition no longer parses")
}

fn session(world: &mut World, machine: &str) -> String {
    let user = world
        .definition()
        .computers
        .iter()
        .find(|c| c.id == machine)
        .expect("machine")
        .user
        .clone();
    world
        .environment(EnvironmentConfig::desktop(user, machine))
        .expect("session")
}

#[test]
fn the_live_world_is_the_reference_world_with_the_phones_and_the_five_shells() {
    let live = live_world();
    let reference = reference_world();
    for computer in &reference.computers {
        assert!(
            live.computers.iter().any(|c| c.id == computer.id),
            "{} was dropped from the live world",
            computer.id
        );
    }
    assert_eq!(live.computers.len(), reference.computers.len() + 2);
    for profile in [
        "virtual-macos-golden-gate",
        "virtual-windows-11",
        "virtual-ubuntu-24",
        "virtual-ios-18",
        "virtual-android-12",
    ] {
        assert!(
            live.profiles.iter().any(|p| p.id == profile),
            "the live world has no {profile} profile"
        );
        assert!(
            live.computers.iter().any(|c| c.profile == profile),
            "no machine runs {profile}"
        );
    }
    // Identical services, not merely as many: `--test internet_links` asks every domain
    // the reference world declares to answer, and that is what this world serves.
    let named = |d: &WorldDefinition| {
        let mut all: Vec<(String, Vec<String>)> = d
            .services
            .iter()
            .map(|s| (s.id.clone(), s.domains.clone()))
            .collect();
        all.sort();
        all
    };
    assert_eq!(named(&live), named(&reference));
}

/// Every device in the world boots and runs a command, as the console's own sweep did.
#[test]
fn every_machine_in_the_live_world_runs_a_command() {
    let definition = live_world();
    let ids: Vec<String> = definition.computers.iter().map(|c| c.id.clone()).collect();
    let mut world = World::new(definition, 42).expect("the live world does not build");
    for id in &ids {
        let actor = session(&mut world, id);
        let result = world
            .step(
                &actor,
                vec![ActionEnvelope::new(
                    "terminal.v1",
                    "execute",
                    id,
                    json!({"command": "echo live-world"}),
                )],
            )
            .expect("step");
        assert!(
            result.outcomes[0].success,
            "{id} could not run a command: {:?}",
            result.outcomes[0]
        );
        assert!(
            serde_json::to_string(&result)
                .unwrap()
                .contains("live-world"),
            "{id} produced no terminal output"
        );
    }
}

/// The gateway stands between the world and anything outside it, whatever the page asks
/// for. This was the console's "outbound network unexpectedly allowed" check.
#[test]
fn a_machine_in_the_live_world_cannot_reach_a_name_outside_it() {
    let mut world = World::new(live_world(), 42).expect("the live world does not build");
    let actor = session(&mut world, "alice-mac");
    // Names the world does not declare. The world's own synthetic public web is large
    // (github.com, anthropic.com and the rest are in it), so the guard below keeps this
    // list honest: a name that turns out to be declared fails here rather than passing.
    let declared: Vec<String> = world
        .definition()
        .services
        .iter()
        .flat_map(|s| s.domains.clone())
        .collect();
    let outside = [
        "https://real-internet.invalid/",
        "http://localhost:8000/",
        "https://not-a-host-in-this-world.test/",
    ];
    for url in outside {
        assert!(
            !declared.iter().any(|d| url.contains(d.as_str())),
            "{url} is declared by this world, so it is not an outbound test"
        );
        let result = world
            .step(
                &actor,
                vec![ActionEnvelope::new(
                    "browser.v1",
                    "navigate",
                    "alice-mac",
                    json!({ "url": url }),
                )],
            )
            .expect("step");
        assert!(
            !result.outcomes[0].success,
            "{url} was reachable from inside the world"
        );
    }
}
