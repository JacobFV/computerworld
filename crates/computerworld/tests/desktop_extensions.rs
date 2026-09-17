use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

fn configured(grants_browser: bool, installed_mail: bool) -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({"alice-mac":"virtual-macos-golden-gate"});
    definition.metadata["desktop_apps"] = json!([
        {"id":"mail","label":"Mail","kind":"browser","url":"http://mail.internal/","icon":"mail"},
        {"id":"docs","label":"Documents","kind":"browser","url":"http://docs.internal/","icon":"docs"}
    ]);
    let machine = definition
        .computers
        .iter_mut()
        .find(|computer| computer.id == "alice-mac")
        .unwrap();
    machine.installed_apps.retain(|app| app != "mail");
    if installed_mail {
        machine.installed_apps.push("mail".into());
    }
    machine.installed_apps.push("docs".into());
    let mut world = World::new(definition, 42).unwrap();
    let mut config = EnvironmentConfig::desktop("alice", "alice-mac");
    if !grants_browser {
        config.actions.retain(|family| family != "browser.v1");
    }
    let actor = world.environment(config).unwrap();
    (world, actor)
}
fn step(world: &mut World, actor: &str, family: &str, op: &str, payload: Value) -> bool {
    world
        .step(
            actor,
            vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
        )
        .unwrap()
        .outcomes[0]
        .success
}
#[test]
fn installed_service_aliases_launch_independent_network_backed_windows() {
    let (mut world, actor) = configured(true, true);
    assert!(step(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"mail"})
    ));
    assert!(step(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"docs"})
    ));
    let state = &world.interfaces().session(&actor).unwrap().machines["alice-mac"];
    assert_eq!(state.desktop.windows.len(), 2);
    assert!(state
        .desktop
        .windows
        .values()
        .any(|window| window.app_id == "mail" && window.title == "Mail"));
    assert!(state
        .desktop
        .windows
        .values()
        .any(|window| window.app_id == "docs" && window.title == "Documents"));
    let trace = serde_json::to_string(&world.trajectory()).unwrap();
    assert!(trace.contains("mail.internal"));
    assert!(trace.contains("docs.internal"));
    let snapshot = world.snapshot();
    let hash = world.state_hash().unwrap();
    world.reset(42).unwrap();
    world.restore(&snapshot).unwrap();
    assert_eq!(world.state_hash().unwrap(), hash);
}
#[test]
fn service_aliases_require_installation_and_browser_capability() {
    for (browser, installed) in [(false, true), (true, false)] {
        let (mut world, actor) = configured(browser, installed);
        assert!(!step(
            &mut world,
            &actor,
            "application.v1",
            "launch",
            json!({"kind":"mail"})
        ));
        assert!(
            world.interfaces().session(&actor).unwrap().machines["alice-mac"]
                .desktop
                .windows
                .is_empty()
        );
    }
}
#[test]
fn launcher_search_edits_query_and_enters_real_installed_application() {
    let (mut world, actor) = configured(true, true);
    assert!(step(
        &mut world,
        &actor,
        "application.v1",
        "launcher",
        json!({})
    ));
    assert!(step(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text":"Maix"})
    ));
    assert!(step(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Backspace"})
    ));
    assert!(step(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text":"l"})
    ));
    assert_eq!(
        world.interfaces().session(&actor).unwrap().machines["alice-mac"]
            .desktop
            .search,
        "Mail"
    );
    assert!(step(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Enter"})
    ));
    let state = &world.interfaces().session(&actor).unwrap().machines["alice-mac"];
    assert_eq!(state.desktop.windows.len(), 1);
    assert_eq!(
        state.desktop.windows.values().next().unwrap().app_id,
        "mail"
    );
    assert!(!state.desktop.launcher_open);
    assert!(state.desktop.search.is_empty());
    assert!(step(
        &mut world,
        &actor,
        "application.v1",
        "launcher",
        json!({})
    ));
    assert!(step(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text":"nonexistent"})
    ));
    assert!(step(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Enter"})
    ));
    assert_eq!(
        world.interfaces().session(&actor).unwrap().machines["alice-mac"]
            .desktop
            .windows
            .len(),
        1
    );
    assert!(step(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Escape"})
    ));
    assert!(
        !world.interfaces().session(&actor).unwrap().machines["alice-mac"]
            .desktop
            .launcher_open
    );
}

fn click(world: &mut World, actor: &str, target: &str) {
    let scene = world.scene(actor, 1200, 800).unwrap();
    let position = scene
        .nodes
        .iter()
        .filter(|node| node.interaction.as_deref() == Some(target))
        .find_map(|node| {
            let rect = node.transform.bounds(node.bounds);
            let x = rect.x + rect.width as i32 / 2;
            let y = rect.y + rect.height as i32 / 2;
            (scene
                .hit_test(x, y)
                .and_then(|hit| hit.interaction.as_deref())
                == Some(target))
            .then_some((x, y))
        })
        .unwrap_or_else(|| panic!("missing reachable target {target}"));
    assert!(step(
        world,
        actor,
        "pointer.v1",
        "click",
        json!({"x":position.0,"y":position.1,"width":1200,"height":800})
    ));
}

#[test]
fn native_menu_and_search_pointer_regions_change_canonical_shell_state() {
    let (mut world, actor) = configured(true, true);
    click(&mut world, &actor, "shell:panel:spotlight");
    assert_eq!(
        world.interfaces().session(&actor).unwrap().machines["alice-mac"]
            .desktop
            .panel
            .as_deref(),
        Some("search")
    );
    assert!(step(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text":"Mail"})
    ));
    click(&mut world, &actor, "shell:launch:mail");
    assert_eq!(
        world.interfaces().session(&actor).unwrap().machines["alice-mac"]
            .desktop
            .windows
            .len(),
        1
    );
    click(&mut world, &actor, "shell:panel:file");
    click(&mut world, &actor, "shell:new");
    let state = &world.interfaces().session(&actor).unwrap().machines["alice-mac"];
    assert_eq!(state.desktop.windows.len(), 2);
    assert!(state
        .desktop
        .windows
        .values()
        .all(|window| window.app_id == "mail"));
    assert!(state.desktop.panel.is_none());
    click(&mut world, &actor, "shell:panel:control");
    assert_eq!(
        world.interfaces().session(&actor).unwrap().machines["alice-mac"]
            .desktop
            .panel
            .as_deref(),
        Some("quick")
    );
    assert!(step(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Escape"})
    ));
    assert!(
        world.interfaces().session(&actor).unwrap().machines["alice-mac"]
            .desktop
            .panel
            .is_none()
    );
}

#[test]
fn search_enter_recognizes_the_visible_platform_application_names() {
    for (theme, query, expected) in [
        ("virtual-macos-golden-gate", "Finder", "files"),
        ("virtual-macos-golden-gate", "Safari", "browser"),
        ("virtual-windows-11", "Notepad", "editor"),
        ("virtual-ubuntu-24", "Web Browser", "browser"),
        ("virtual-ios-18", "Notes", "editor"),
        ("virtual-android-12", "Editor", "editor"),
    ] {
        let (base, _) = configured(true, true);
        let mut definition = base.definition().clone();
        definition.metadata["desktop_themes"] = json!({"alice-mac": theme});
        let mut world = World::new(definition, 42).unwrap();
        let actor = world
            .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
            .unwrap();
        assert!(step(
            &mut world,
            &actor,
            "application.v1",
            "launcher",
            json!({})
        ));
        assert!(step(
            &mut world,
            &actor,
            "keyboard.v1",
            "type",
            json!({"text":query})
        ));
        assert!(step(
            &mut world,
            &actor,
            "keyboard.v1",
            "key",
            json!({"key":"Enter"})
        ));
        let state = &world.interfaces().session(&actor).unwrap().machines["alice-mac"];
        assert_eq!(
            state.desktop.windows.values().next().unwrap().app_id,
            expected,
            "{theme} {query}"
        );
    }
}
