//! The ready-made single-agent world, driven the way a consumer drives it: one Ubuntu
//! desktop, one actor session, and nothing the owner handle has to do for it.
//!
//! Everything here goes through `World::actor`, so a change that only works from the
//! owner side cannot make this file pass.
use computerworld::{ActionEnvelope, EnvironmentConfig, World, WorldDefinition};
use serde_json::{json, Value};

const DEFINITION: &str = include_str!("../../../examples/worlds/agent-desktop.json");
const MACHINE: &str = "workstation";

/// The world and an actor session built from the grants the file itself publishes.
/// A consumer copies exactly this much.
fn agent_world() -> (World, String) {
    let definition = WorldDefinition::from_json(DEFINITION).expect("example world parses");
    let config: EnvironmentConfig =
        serde_json::from_value(definition.metadata["actor_session"].clone())
            .expect("metadata.actor_session is an EnvironmentConfig");
    let mut world = World::new(definition, 7).unwrap();
    let session = world.environment(config).unwrap();
    (world, session)
}
fn act(world: &mut World, session: &str, family: &str, op: &str, payload: Value) -> Value {
    let mut actor = world.actor(session).unwrap();
    let result = actor
        .step(vec![ActionEnvelope::new(family, op, MACHINE, payload)])
        .unwrap();
    let outcome = &result.outcomes[0];
    assert!(
        outcome.success,
        "{family} {op} should succeed: {:?}",
        outcome.error
    );
    outcome.value.clone()
}
fn shell(world: &mut World, session: &str, command: &str) -> String {
    let value = act(
        world,
        session,
        "terminal.v1",
        "execute",
        json!({ "command": command }),
    );
    assert_eq!(
        value["exit_code"],
        0,
        "`{command}`: {}",
        value["stderr"].as_str().unwrap_or_default()
    );
    value["stdout"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn the_example_world_is_a_working_agent_desktop_out_of_the_box() {
    let (mut world, session) = agent_world();
    // The seeded documents are where the file says they are.
    let listing = shell(&mut world, &session, "ls ~/Documents");
    for document in ["handbook.txt", "expenses.csv", "report.md"] {
        assert!(listing.contains(document), "{listing}");
    }
    assert!(shell(&mut world, &session, "cat ~/Documents/handbook.txt").contains("team wiki"));
    // The everyday applications are installed and the actor can enumerate them.
    let apps = act(&mut world, &session, "application.v1", "list", json!({}));
    let ids: Vec<&str> = apps
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    for id in ["browser", "files", "editor", "terminal"] {
        assert!(ids.contains(&id), "{id} should be listed: {ids:?}");
    }
    assert!(
        apps.as_array()
            .unwrap()
            .iter()
            .all(|a| a["launchable"] == json!(true)),
        "every installed application in this world is launchable: {apps}"
    );
    // The browser reaches the world's own intranet and nothing beyond it.
    let page = act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url":"http://wiki.internal/"}),
    );
    assert_eq!(page["title"], json!("Team wiki"), "{page}");
    // Applications launch, and an open window is a process on the machine.
    act(
        &mut world,
        &session,
        "application.v1",
        "launch",
        json!({"kind":"editor","argument":"/home/ada/Documents/report.md"}),
    );
    let table = shell(&mut world, &session, "ps -e -o pid,rss,comm");
    assert!(
        table.contains("editor"),
        "an open editor window is a running process: {table}"
    );
}

#[test]
fn the_example_world_answers_the_two_questions_a_consumer_asks_first() {
    let (mut world, session) = agent_world();
    // "The ten processes using the most memory."
    act(
        &mut world,
        &session,
        "application.v1",
        "launch",
        json!({"kind":"browser"}),
    );
    let biggest = shell(
        &mut world,
        &session,
        "ps -e -o rss,comm --sort=-rss | head -n 11",
    );
    let sizes: Vec<u64> = biggest
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next()?.parse().ok())
        .collect();
    assert!(sizes.len() >= 2, "{biggest}");
    assert!(
        sizes.windows(2).all(|w| w[0] >= w[1]),
        "largest first: {biggest}"
    );
    assert!(
        biggest
            .lines()
            .nth(1)
            .is_some_and(|l| l.contains("browser")),
        "a browser window is the biggest thing running: {biggest}"
    );
    // "How much disk is left."
    let df = shell(&mut world, &session, "df -h /");
    assert!(df.lines().count() >= 2, "{df}");
    assert!(df.contains("Avail") || df.contains("Available"), "{df}");
}

#[test]
fn the_example_world_is_deterministic_and_snapshot_clean() {
    let (mut world, session) = agent_world();
    shell(&mut world, &session, "echo one > /tmp/one");
    let hash = world.state_hash().unwrap();
    let (mut again, session_again) = agent_world();
    shell(&mut again, &session_again, "echo one > /tmp/one");
    assert_eq!(hash, again.state_hash().unwrap());
    let checkpoint = world.snapshot();
    shell(&mut world, &session, "echo two > /tmp/two");
    world.restore(&checkpoint).unwrap();
    assert_eq!(hash, world.state_hash().unwrap());
}
