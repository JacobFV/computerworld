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
    // The pointer is part of the scene, so the shell is compared with it parked in one place.
    let park = |world: &mut World| {
        action(
            world,
            &actor,
            "pointer.v1",
            "move",
            json!({"x":480,"y":300,"width":960,"height":640}),
        );
    };
    park(&mut world);
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
    click(&mut world, &actor, "window:0:minimize");
    click(&mut world, &actor, "window:0:focus");
    assert_eq!(world.scene(&actor, 960, 640).unwrap(), opened);
    click(&mut world, &actor, "window:0:maximize");
    assert_ne!(world.scene(&actor, 960, 640).unwrap(), opened);
    world.restore(&snapshot).unwrap();
    assert_eq!(world.scene(&actor, 960, 640).unwrap(), opened);
    click(&mut world, &actor, "window:0:close");
    park(&mut world);
    assert_eq!(world.scene(&actor, 960, 640).unwrap(), home);
}
#[test]
fn desktop_browser_address_and_page_hit_targets_use_canonical_network() {
    let (mut world, actor) = world("virtual-macos-golden-gate");
    click(&mut world, &actor, "shell:launch:browser");
    click(&mut world, &actor, "window:0:content:shell:address");
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
    // The page is longer than the window, and the scene lists its links below the fold
    // too; the one clicked is the first that is under the pointer where it is drawn.
    let target = scene
        .nodes
        .iter()
        .filter_map(|node| {
            let id = node.interaction.as_deref().filter(|id| {
                id.starts_with("window:0:content:") && !id.starts_with("window:0:content:shell:")
            })?;
            let bounds = node.transform.bounds(node.bounds);
            let x = bounds.x + (bounds.width / 2) as i32;
            let y = bounds.y + (bounds.height / 2) as i32;
            (scene.hit_test(x, y).and_then(|hit| hit.interaction.as_deref()) == Some(id)).then(|| id.to_owned())
        })
        .next()
        .expect("network response page has links");
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
    assert!(!world
        .scene(&actor, 960, 640)
        .unwrap()
        .nodes
        .iter()
        .any(|node| node.interaction.as_deref() == Some("shell:launch:browser")));
    assert!(!world
        .scene(&actor, 960, 640)
        .unwrap()
        .nodes
        .iter()
        .any(|node| node.interaction.as_deref() == Some("shell:launch:editor")));
    for (family, op, payload) in [
        ("application.v1", "launch", json!({"kind":"browser"})),
        (
            "browser.v1",
            "navigate",
            json!({"url":"http://intranet.internal/"}),
        ),
    ] {
        let blocked = world
            .step(
                &actor,
                vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
            )
            .unwrap();
        assert!(!blocked.outcomes[0].success);
        assert_eq!(blocked.outcomes[0].error.as_ref().unwrap().code, "denied");
    }
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
/// The terminal is legible to an observer, not just to an API caller: the prompt names
/// the machine, the command is echoed, a failure carries `[exit N]`, and `clear` wipes
/// the frame without losing the cwd.
#[test]
fn terminal_transcript_carries_prompt_echo_and_exit_status() {
    let (mut world, actor) = world("virtual-ubuntu-lts");
    click(&mut world, &actor, "shell:launch:terminal");
    let run = |world: &mut World, actor: &str, command: &str| {
        action(
            world,
            actor,
            "keyboard.v1",
            "type",
            json!({ "text": command }),
        );
        action(world, actor, "keyboard.v1", "key", json!({"key":"Enter"}));
    };
    let page = |world: &World, actor: &str| -> Value {
        world.observe(actor).unwrap().channels["semantic.v1"]["alice-mac"].clone()
    };
    let texts = |page: &Value| -> Vec<String> {
        fn walk(element: &Value, out: &mut Vec<String>) {
            if let Some(text) = element["text"].as_str() {
                out.push(text.to_owned());
            }
            if let Some(children) = element["children"].as_array() {
                children.iter().for_each(|c| walk(c, out));
            }
        }
        let mut out = vec![];
        page["elements"]
            .as_array()
            .unwrap()
            .iter()
            .for_each(|e| walk(e, &mut out));
        out
    };
    run(&mut world, &actor, "cd /tmp");
    run(&mut world, &actor, "definitely-not-a-command");
    let observed = page(&world, &actor);
    let lines = texts(&observed);
    assert!(lines.contains(&"alice@alice-mac ~ % cd /tmp".to_string()));
    // The prompt followed the cwd, so the echo of the next command proves the move.
    assert!(lines.contains(&"alice@alice-mac tmp % definitely-not-a-command".to_string()));
    assert!(lines.contains(&"[exit 0]".to_string()));
    assert!(lines
        .iter()
        .any(|l| l.starts_with("[exit ") && l != "[exit 0]"));
    // The input label is the live prompt, so a reader knows where the next command lands.
    let input = observed["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "terminal-input")
        .unwrap()
        .clone();
    assert_eq!(input["label"], "alice@alice-mac tmp %");
    // The echoed lines are on screen too, not only in the projection.
    let scene = world.scene(&actor, 960, 640).unwrap();
    assert!(scene
        .nodes
        .iter()
        .any(|n| format!("{:?}", n.primitive)
            .contains("alice@alice-mac tmp % definitely-not-a-command")));
    run(&mut world, &actor, "clear");
    let cleared = page(&world, &actor);
    assert!(!texts(&cleared)
        .iter()
        .any(|l| l.contains("definitely-not-a-command")));
    // cwd survives the clear: the next command still runs in /tmp.
    assert_eq!(
        cleared["elements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == "terminal-input")
            .unwrap()["label"],
        "alice@alice-mac tmp %"
    );
}
/// The centre of the control carrying `target`, and the whole scene it was found in.
fn centre_of(world: &mut World, actor: &str, target: &str, width: u32, height: u32) -> (i32, i32) {
    let scene = world.scene(actor, width, height).unwrap();
    let node = scene
        .nodes
        .iter()
        .find(|node| node.interaction.as_deref() == Some(target))
        .unwrap_or_else(|| panic!("missing interaction {target}"));
    let bounds = node.transform.bounds(node.bounds);
    (
        bounds.x + (bounds.width / 2) as i32,
        bounds.y + (bounds.height / 2) as i32,
    )
}
#[test]
fn desktop_frames_draw_the_pointer_where_it_last_moved() {
    use cw_scene::Primitive;
    for theme in [
        "virtual-macos-golden-gate",
        "virtual-windows-11",
        "virtual-ubuntu-24",
    ] {
        let (mut world, actor) = world(theme);
        let before = world.scene(&actor, 960, 640).unwrap();
        let idle = world.render(&actor, 960, 640).unwrap();
        // No pointer yet: the pointer node is there, empty, so the node sequence never
        // changes when one arrives and only its own pixels are repainted.
        let empty = before.nodes.last().unwrap();
        assert_eq!(empty.bounds.width, 0, "{theme}");
        assert!(empty.interaction.is_none() && empty.semantic.is_none());

        // Over the bare desktop nothing else reacts to the pointer: the node sequence
        // is the same as without one.
        action(
            &mut world,
            &actor,
            "pointer.v1",
            "move",
            json!({"x":480,"y":300,"width":960,"height":640}),
        );
        let parked = world.scene(&actor, 960, 640).unwrap();
        assert!(
            before
                .nodes
                .iter()
                .map(|n| n.id)
                .eq(parked.nodes.iter().map(|n| n.id)),
            "{theme}: the pointer arriving must not change the node sequence"
        );
        let (x, y) = centre_of(&mut world, &actor, "shell:launch:terminal", 960, 640);
        action(
            &mut world,
            &actor,
            "pointer.v1",
            "move",
            json!({"x":x,"y":y,"width":960,"height":640}),
        );
        let scene = world.scene(&actor, 960, 640).unwrap();
        let [outline, cursor] = &scene.nodes[scene.nodes.len() - 2..] else {
            unreachable!()
        };
        assert!(
            matches!(
                outline.primitive,
                Primitive::Path {
                    fill: None,
                    stroke: Some(_),
                    ..
                }
            ) && matches!(
                cursor.primitive,
                Primitive::Path {
                    fill: Some(_),
                    stroke: None,
                    ..
                }
            ),
            "{theme}: {:?} under {:?}",
            outline.primitive,
            cursor.primitive
        );
        assert!(cursor.interaction.is_none() && cursor.semantic.is_none());
        assert!(outline.interaction.is_none() && outline.semantic.is_none());
        let painted = cursor.painted_bounds();
        assert!(
            painted.contains(x, y),
            "{theme}: {painted:?} misses ({x}, {y})"
        );
        assert!(
            painted.width <= 40 && painted.height <= 40,
            "{theme}: {painted:?}"
        );
        assert!(
            scene.nodes.iter().all(|n| n.z <= cursor.z),
            "{theme}: the pointer is painted over everything"
        );
        // The glyph is in the frame, and what it points at is still what a click hits.
        let moved = world.render(&actor, 960, 640).unwrap();
        assert_ne!(idle.rgba, moved.rgba, "{theme}");
        assert_eq!(
            scene
                .hit_test(x, y)
                .and_then(|node| node.interaction.as_deref()),
            Some("shell:launch:terminal"),
            "{theme}"
        );
        // The launcher is a control, so the pointer over it is a hand; over the empty
        // desktop it is an arrow, and the two differ in the frame.
        let arrow_at = |world: &mut World, x: i32, y: i32| {
            action(
                world,
                &actor,
                "pointer.v1",
                "move",
                json!({"x":x,"y":y,"width":960,"height":640}),
            );
            let scene = world.scene(&actor, 960, 640).unwrap();
            match &scene.nodes.last().unwrap().primitive {
                Primitive::Path { points, .. } => points.clone(),
                other => panic!("{other:?}"),
            }
        };
        let hand = arrow_at(&mut world, x, y);
        let arrow = arrow_at(&mut world, 480, 300);
        assert_ne!(hand, arrow, "{theme}");
    }
    // A phone has no pointer at all: the node stays empty and the frame is unchanged.
    for theme in ["virtual-ios-18", "virtual-android-12"] {
        let (mut world, actor) = world(theme);
        let idle = world.render(&actor, 390, 844).unwrap();
        action(
            &mut world,
            &actor,
            "pointer.v1",
            "move",
            json!({"x":195,"y":600,"width":390,"height":844}),
        );
        let scene = world.scene(&actor, 390, 844).unwrap();
        assert!(
            !scene
                .nodes
                .iter()
                .any(|n| matches!(n.primitive, Primitive::Path { .. })
                    && n.painted_bounds().contains(195, 600)
                    && n.z >= 2_000_000),
            "{theme}"
        );
        assert_eq!(
            idle.rgba,
            world.render(&actor, 390, 844).unwrap().rgba,
            "{theme}"
        );
    }
}
