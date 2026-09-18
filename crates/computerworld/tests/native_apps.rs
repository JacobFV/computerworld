//! Native applications are real programs, not links dressed as icons: they launch into
//! their own window, reach their service over the simulated network, and change it.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, WorldDefinition};
use serde_json::{json, Value};

const W: u32 = 1100;
const H: u32 = 720;
/// One hour and one day in the microseconds the world clock counts.
const HOUR: u64 = 3_600_000_000;

fn definition() -> WorldDefinition {
    let mut d = reference_world();
    d.metadata["desktop_themes"] = json!({ "alice-mac": "virtual-macos-golden-gate" });
    d.metadata["desktop_apps"] = json!([
        {"id":"calendar","label":"Calendar","kind":"native","url":"http://calendar.internal/","icon":"calendar"},
        {"id":"mail","label":"Mail","kind":"native","url":"http://mail.internal/","icon":"mail"},
        {"id":"notes","label":"Notes","kind":"native","url":"","icon":"notes"},
        {"id":"calculator","label":"Calculator","kind":"native","url":"","icon":"calculator"},
    ]);
    for computer in &mut d.computers {
        if computer.id == "alice-mac" {
            computer.installed_apps.extend(
                ["calendar", "mail", "notes", "calculator"]
                    .into_iter()
                    .map(str::to_owned),
            );
        }
    }
    d
}
fn world() -> (World, String) {
    let mut world = World::new(definition(), 42).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    (world, actor)
}
fn act(world: &mut World, actor: &str, family: &str, op: &str, payload: Value) {
    let result = world
        .step(
            actor,
            vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{:?}", result.outcomes[0]);
}
fn launch(world: &mut World, actor: &str, kind: &str) {
    act(
        world,
        actor,
        "application.v1",
        "launch",
        json!({"kind":kind}),
    );
}
fn click(world: &mut World, actor: &str, target: &str) {
    let scene = world.scene(actor, W, H).unwrap();
    let suffix = format!(":content:{target}");
    let node = scene
        .nodes
        .iter()
        .find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i == target || i.ends_with(&suffix))
        })
        .unwrap_or_else(|| panic!("missing interaction {target}"));
    let bounds = node.transform.bounds(node.bounds);
    act(
        world,
        actor,
        "pointer.v1",
        "click",
        json!({
            "x": bounds.x + (bounds.width / 2) as i32,
            "y": bounds.y + (bounds.height / 2) as i32,
            "width": W, "height": H
        }),
    );
}
fn state(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap()
}
/// State of the single open window.
fn app(world: &World, actor: &str) -> Value {
    let desktop = state(world, actor);
    let windows = desktop["windows"].as_object().unwrap();
    assert_eq!(windows.len(), 1, "expected exactly one window");
    windows.values().next().unwrap()["state"].clone()
}

#[test]
fn a_native_app_opens_in_its_own_window_and_never_in_the_browser() {
    for kind in ["calendar", "mail", "notes", "calculator"] {
        let (mut world, actor) = world();
        launch(&mut world, &actor, kind);
        let desktop = state(&world, &actor);
        let window = desktop["windows"]["0"].clone();
        assert_eq!(
            window["state"]["type"], "native",
            "{kind} did not open a native window"
        );
        assert_eq!(window["state"]["app"], kind);
        let session = world.interfaces().session(&actor).unwrap();
        assert!(
            !session.machines["alice-mac"].browser_visible,
            "{kind} opened the browser instead of an application"
        );
    }
}

#[test]
fn the_calendar_loads_real_events_from_the_service_and_writes_one_back() {
    let (mut world, actor) = world();
    launch(&mut world, &actor, "calendar");
    // Launching fetched the month; the events are the service's own records.
    let loaded = app(&world, &actor)["events"].as_array().unwrap().clone();
    assert!(
        !loaded.is_empty(),
        "the calendar showed nothing though the service has events"
    );
    assert_eq!(app(&world, &actor)["status"], "idle");

    click(&mut world, &actor, "cal:new");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text":"Determinism review"}),
    );
    click(&mut world, &actor, "cal:save");
    let after = app(&world, &actor)["events"].as_array().unwrap().clone();
    assert_eq!(
        after.len(),
        loaded.len() + 1,
        "the new event did not come back from the service"
    );
    let created = after
        .iter()
        .find(|e| e["title"] == "Determinism review")
        .expect("the service stored the event we sent");
    assert_eq!(created["start"].as_u64().unwrap() % HOUR, 0);

    // The service, not the application, is the record: a second reader sees it too.
    let second = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    launch(&mut world, &second, "calendar");
    assert!(app(&world, &second)["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["title"] == "Determinism review"));
}

#[test]
fn mail_reads_the_real_mailbox_and_marks_a_message_read_on_the_service() {
    let (mut world, actor) = world();
    launch(&mut world, &actor, "mail");
    let messages = app(&world, &actor)["messages"].as_array().unwrap().clone();
    assert!(!messages.is_empty(), "the inbox came back empty");
    let id = messages[0]["id"].as_str().unwrap().to_owned();
    assert!(
        messages[0]["mailboxes"]["alice"]["read"] == json!(false),
        "the seeded message should start unread"
    );
    click(&mut world, &actor, &format!("mail:open:{id}"));
    let after = app(&world, &actor)["messages"].as_array().unwrap().clone();
    assert_eq!(
        after[0]["mailboxes"]["alice"]["read"],
        json!(true),
        "opening a message did not mark it read on the service"
    );
}

#[test]
fn a_missing_notes_folder_is_shown_not_thrown() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"notes","argument":"/Users/alice/NoSuchFolder"}),
    );
    // The window opens and says what is wrong, rather than the click failing.
    assert!(app(&world, &actor)["problem"].is_string());
}

#[test]
fn notes_are_real_files_on_the_machine() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"notes","argument":"/Users/alice/Notes"}),
    );
    click(&mut world, &actor, "notes:new");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text":"remember the milk"}),
    );
    click(&mut world, &actor, "notes:save");
    let name = app(&world, &actor)["open"].as_str().unwrap().to_owned();
    let bytes = world
        .runtime()
        .read_file("alice-mac", &format!("/Users/alice/Notes/{name}"))
        .expect("the note was written to the filesystem");
    assert_eq!(String::from_utf8(bytes).unwrap(), "remember the milk");
}

#[test]
fn the_calculator_is_exact_and_needs_no_network_at_all() {
    let (mut world, actor) = world();
    launch(&mut world, &actor, "calculator");
    for key in ["1", "2", ".", "5", "+", "2", "="] {
        click(&mut world, &actor, &format!("calc:{key}"));
    }
    assert_eq!(app(&world, &actor)["accumulator"], 1450);
}

#[test]
fn a_native_app_survives_a_snapshot_round_trip() {
    let (mut world, actor) = world();
    launch(&mut world, &actor, "calendar");
    click(&mut world, &actor, "cal:day");
    let before = app(&world, &actor);
    let snapshot = world.snapshot();
    let restored = world.fork(&snapshot).unwrap();
    assert_eq!(app(&restored, &actor), before);
    assert_eq!(restored.state_hash().unwrap(), world.state_hash().unwrap());
}

#[test]
fn a_day_in_the_notification_center_opens_calendar_on_that_day() {
    // The date is clicked where it is painted, so it must win over the whole-widget
    // entry point that sits underneath it.
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "shell",
        json!({"target":"shell:panel:notifications"}),
    );
    click(&mut world, &actor, "shell:launch:calendar/2026-09-20");
    let calendar = app(&world, &actor);
    assert_eq!(calendar["app"], "calendar", "{calendar}");
    let text = calendar.to_string();
    assert!(text.contains("\"cursor_day\":3"), "{text}");
    assert!(text.contains("\"view\":\"day\""), "{text}");
    // It is still the calendar the machine is configured with, not a default one.
    assert!(text.contains("http://calendar.internal/"), "{text}");
    // A day before the world began offers nothing to click.
    let scene = world.scene(&actor, W, H).unwrap();
    assert!(!scene.nodes.iter().any(|n| n
        .interaction
        .as_deref()
        .is_some_and(|i| i.ends_with("calendar/2026-09-16"))));
}
