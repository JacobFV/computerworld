use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

fn world(theme: &str) -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({"alice-mac":theme});
    let mut world = World::new(definition, 42).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    (world, actor)
}
fn action(world: &mut World, actor: &str, family: &str, op: &str, payload: Value) {
    let result = world
        .step(
            actor,
            vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{:?}", result.outcomes[0]);
}
fn click(world: &mut World, actor: &str, target: &str) {
    let scene = world.scene(actor, 960, 640).unwrap();
    let node = scene
        .nodes
        .iter()
        .find(|node| node.interaction.as_deref() == Some(target))
        .unwrap_or_else(|| panic!("missing interaction {target}"));
    let bounds = node.transform.bounds(node.bounds);
    let x = bounds.x + (bounds.width / 2) as i32;
    let y = bounds.y + (bounds.height / 2) as i32;
    assert_eq!(
        scene
            .hit_test(x, y)
            .and_then(|node| node.interaction.as_deref()),
        Some(target)
    );
    action(
        world,
        actor,
        "pointer.v1",
        "click",
        json!({"x":x,"y":y,"width":960,"height":640}),
    );
}
#[test]
fn desktop_shell_lifecycle_is_interactive_and_snapshotted() {
    let (mut world, actor) = world("virtual-windows-11");
    let home = world.scene(&actor, 960, 640).unwrap();
    click(&mut world, &actor, "shell:launch:terminal");
    action(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text":"echo native-desktop"}),
    );
    action(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Enter"}),
    );
    let opened = world.scene(&actor, 960, 640).unwrap();
    assert_ne!(home, opened);
    let snapshot = world.snapshot();
    click(&mut world, &actor, "shell:minimize");
    click(&mut world, &actor, "shell:launch:terminal");
    assert_eq!(world.scene(&actor, 960, 640).unwrap(), opened);
    click(&mut world, &actor, "shell:maximize");
    assert_ne!(world.scene(&actor, 960, 640).unwrap(), opened);
    world.restore(&snapshot).unwrap();
    assert_eq!(world.scene(&actor, 960, 640).unwrap(), opened);
    click(&mut world, &actor, "shell:close");
    assert_eq!(world.scene(&actor, 960, 640).unwrap(), home);
}
#[test]
fn desktop_browser_address_and_page_hit_targets_use_canonical_network() {
    let (mut world, actor) = world("virtual-macos-golden-gate");
    click(&mut world, &actor, "shell:launch:browser");
    click(&mut world, &actor, "shell:address");
    action(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text":"http://intranet.internal/"}),
    );
    action(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Enter"}),
    );
    let scene = world.scene(&actor, 960, 640).unwrap();
    let target = scene
        .nodes
        .iter()
        .find_map(|node| {
            node.interaction
                .as_deref()
                .filter(|id| !id.starts_with("shell:"))
        })
        .expect("network response page has links")
        .to_owned();
    click(&mut world, &actor, &target);
    let events = serde_json::to_string(&world.trajectory()).unwrap();
    assert!(events.contains("intranet.internal"));
    assert!(events.contains("http"));
}
#[test]
fn five_profile_shells_are_distinct_and_reproducible() {
    let themes = [
        "virtual-macos-golden-gate",
        "virtual-windows-11",
        "virtual-ubuntu-24",
        "virtual-ios-18",
        "virtual-android-12",
    ];
    let mut scenes = vec![];
    for theme in themes {
        let (world, actor) = world(theme);
        let scene = world.scene(&actor, 960, 640).unwrap();
        assert_eq!(scene, world.scene(&actor, 960, 640).unwrap());
        assert!(!scenes.contains(&scene), "duplicate theme {theme}");
        scenes.push(scene);
    }
}
#[test]
fn browser_only_actor_has_no_desktop_launch_surface() {
    let (mut world, _) = world("virtual-windows-11");
    let mut config = EnvironmentConfig::desktop("alice", "alice-mac");
    config.actions.retain(|family| family != "application.v1");
    let actor = world.environment(config).unwrap();
    assert!(!world
        .scene(&actor, 960, 640)
        .unwrap()
        .nodes
        .iter()
        .any(|node| node
            .interaction
            .as_deref()
            .is_some_and(|id| id.starts_with("shell:"))));
}

#[test]
fn desktop_launch_and_address_entry_respect_capabilities_and_installation() {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({"alice-mac":"virtual-windows-11"});
    let computer = definition
        .computers
        .iter_mut()
        .find(|c| c.id == "alice-mac")
        .unwrap();
    computer.installed_apps.retain(|app| app != "editor");
    let mut world = World::new(definition, 42).unwrap();
    let mut config = EnvironmentConfig::desktop("alice", "alice-mac");
    config.actions.retain(|family| family != "browser.v1");
    let actor = world.environment(config).unwrap();
    click(&mut world, &actor, "shell:launch:browser");
    action(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text":"http://intranet.internal/"}),
    );
    let blocked = world
        .step(
            &actor,
            vec![ActionEnvelope::new(
                "keyboard.v1",
                "key",
                "alice-mac",
                json!({"key":"Enter"}),
            )],
        )
        .unwrap();
    assert!(!blocked.outcomes[0].success);
    assert_eq!(blocked.outcomes[0].error.as_ref().unwrap().code, "denied");
    let launch = world
        .step(
            &actor,
            vec![ActionEnvelope::new(
                "application.v1",
                "launch",
                "alice-mac",
                json!({"kind":"editor"}),
            )],
        )
        .unwrap();
    assert!(!launch.outcomes[0].success);
}
