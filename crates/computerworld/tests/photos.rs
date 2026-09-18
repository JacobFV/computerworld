//! Photos shows the machine's own pictures. The world can write one — a screenshot is a
//! real PNG — so this takes one and then looks at it, which is the whole loop.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

fn world() -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ "alice-mac": "virtual-macos-golden-gate" });
    definition.metadata["desktop_apps"] = json!([
        {"id":"photos","label":"Photos","kind":"native","url":"","icon":"photos"},
    ]);
    if let Some(computer) = definition
        .computers
        .iter_mut()
        .find(|c| c.id == "alice-mac")
    {
        computer.installed_apps.push("photos".into());
    }
    let mut world = World::new(definition, 4).unwrap();
    let mut config = EnvironmentConfig::desktop("alice", "alice-mac");
    config.observations.push("pixels.v1".into());
    let session = world.environment(config).unwrap();
    (world, session)
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
fn shell(world: &mut World, actor: &str, target: &str) {
    act(
        world,
        actor,
        "application.v1",
        "shell",
        json!({"target": target}),
    );
}
fn photos(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    let desktop = serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap();
    desktop["windows"]
        .as_object()
        .unwrap()
        .values()
        .find(|w| w["state"]["app"] == "photos")
        .expect("a photos window")["state"]
        .clone()
}

#[test]
fn a_photo_the_world_wrote_is_shown_with_its_own_pixels() {
    let (mut world, actor) = world();
    // The world takes a screenshot, which lands in Pictures as a real PNG.
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    shell(&mut world, &actor, "shell:screenshot");
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"photos","argument":"/Users/alice/Pictures"}),
    );
    let state = photos(&world, &actor);
    let entries = state["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "the screenshot is not in the library");
    let name = entries[0].as_str().unwrap();
    assert!(name.ends_with(".png"), "{name}");

    // ...and its pixels were decoded, not stood in for.
    let pixels = state["pixels"].as_object().expect("decoded pixels");
    let picture = pixels.get(name).expect("this photo was decoded");
    let (w, h) = (
        picture["width"].as_u64().unwrap(),
        picture["height"].as_u64().unwrap(),
    );
    assert!(w > 0 && h > 0);
    // Downsampled for the snapshot, not the full 1280x800 frame.
    assert!(w <= 192 && h <= 192, "thumbnail was not reduced: {w}x{h}");
    assert_eq!(
        picture["rgba"].as_array().unwrap().len() as u64,
        w * h * 4,
        "pixel buffer does not match its own dimensions"
    );
    // The scene really draws them.
    let scene = world.scene(&actor, 1100, 700).unwrap();
    let drawn = scene
        .nodes
        .iter()
        .filter(|n| matches!(n.primitive, cw_scene::Primitive::Image { .. }))
        .count();
    assert!(drawn > 0, "no image primitive reached the scene");
}

#[test]
fn a_file_that_will_not_decode_says_so_instead_of_showing_a_gap() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "terminal.v1",
        "execute",
        json!({"command":"mkdir -p /Users/alice/Pictures"}),
    );
    act(
        &mut world,
        &actor,
        "terminal.v1",
        "execute",
        json!({"command":"echo not-a-png > /Users/alice/Pictures/broken.png"}),
    );
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"photos","argument":"/Users/alice/Pictures"}),
    );
    let state = photos(&world, &actor);
    assert_eq!(state["entries"].as_array().unwrap().len(), 1);
    assert!(
        state["pixels"].as_object().is_none_or(|p| p.is_empty()),
        "a file that is not an image was decoded anyway"
    );
    let bad = state["undecodable"].as_array().expect("recorded as broken");
    assert_eq!(bad[0], "broken.png");
}

#[test]
fn an_empty_library_asks_for_nothing() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "terminal.v1",
        "execute",
        json!({"command":"mkdir -p /Users/alice/Pictures"}),
    );
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"photos","argument":"/Users/alice/Pictures"}),
    );
    let state = photos(&world, &actor);
    assert!(state["entries"].as_array().unwrap().is_empty());
    assert!(state["pixels"].as_object().is_none_or(|p| p.is_empty()));
}
