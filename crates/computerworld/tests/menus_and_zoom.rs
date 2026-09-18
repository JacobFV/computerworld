//! Word wrap, the application menus that toggle it, and browser zoom. Each is driven by
//! pointer clicks on painted controls and read back from machine state or the scene.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use cw_scene::Primitive;
use serde_json::{json, Value};

const W: u32 = 1100;
const H: u32 = 720;

fn world(theme: &str) -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ "alice-mac": theme });
    let mut world = World::new(definition, 3).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    (world, actor)
}
fn act(world: &mut World, actor: &str, family: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(
            actor,
            vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn targets(world: &World, actor: &str, width: u32, height: u32) -> Vec<String> {
    world
        .scene(actor, width, height)
        .unwrap()
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .collect()
}
/// Click the topmost node whose target is `target` or ends with `:content:<target>`,
/// or, with a trailing `*`, starts with the rest.
fn click_at(world: &mut World, actor: &str, target: &str, width: u32, height: u32) {
    let scene = world.scene(actor, width, height).unwrap();
    let suffix = format!(":content:{target}");
    let matches = |i: &str| match target.strip_suffix('*') {
        Some(prefix) => i.starts_with(prefix) || i.contains(&format!(":content:{prefix}")),
        None => i == target || i.ends_with(&suffix),
    };
    let node = scene
        .nodes
        .iter()
        .filter(|n| n.interaction.as_deref().is_some_and(matches))
        .max_by_key(|n| n.z)
        .unwrap_or_else(|| panic!("missing interaction {target}"));
    let b = node.transform.bounds(node.bounds);
    act(
        world,
        actor,
        "pointer.v1",
        "click",
        json!({"x": b.x + b.width as i32 / 2, "y": b.y + b.height as i32 / 2,
               "width": width, "height": height}),
    );
}
fn click(world: &mut World, actor: &str, target: &str) {
    click_at(world, actor, target, W, H);
}
fn desktop(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap()
}
fn focused_state(world: &World, actor: &str) -> Value {
    let d = desktop(world, actor);
    let id = d["focused"].as_u64().unwrap().to_string();
    d["windows"][&id]["state"].clone()
}
fn editor_target(world: &World, actor: &str) -> String {
    targets(world, actor, W, H)
        .into_iter()
        .find(|t| t.contains(":content:editor-text:"))
        .expect("an editor text target")
}
fn launch(world: &mut World, actor: &str, kind: &str) {
    act(
        world,
        actor,
        "application.v1",
        "launch",
        json!({ "kind": kind }),
    );
}

#[test]
fn notepad_word_wrap_really_wraps_and_a_click_lands_on_the_wrapped_row() {
    let (mut world, actor) = world("virtual-windows-11");
    launch(&mut world, &actor, "editor");
    let text = "word ".repeat(60);
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({ "text": text }),
    );
    assert!(editor_target(&world, &actor).ends_with("editor-text:0"));
    // The gear opens Notepad's settings; Word wrap there is a real machine setting,
    // and the flyout stays open like a settings page does.
    click(&mut world, &actor, "shell:panel:app-settings");
    click(&mut world, &actor, "shell:toggle:word_wrap");
    let state = desktop(&world, &actor);
    assert_eq!(state["settings"]["word_wrap"], true);
    assert_eq!(state["panel"], "app-settings");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Escape"}),
    );
    let target = editor_target(&world, &actor);
    let columns: usize = target.rsplit(':').next().unwrap().parse().unwrap();
    assert!(columns > 0 && columns < 300, "{target}");
    // Clicking the start of the second painted row puts the caret right after the
    // first row, which breaks after the last whole word that fits.
    let scene = world.scene(&actor, W, H).unwrap();
    let region = scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some(target.as_str()))
        .unwrap();
    let b = region.transform.bounds(region.bounds);
    act(
        &mut world,
        &actor,
        "pointer.v1",
        "click",
        json!({"x": b.x + 1, "y": b.y + 18 + 9, "width": W, "height": H}),
    );
    let cursor = focused_state(&world, &actor)["cursor"].as_u64().unwrap() as usize;
    assert_eq!(cursor, columns / 5 * 5);
    // The published caret agrees: logical column, second painted row.
    let caret = world
        .scene(&actor, W, H)
        .unwrap()
        .focus
        .unwrap()
        .caret
        .unwrap();
    assert_eq!(caret.column as usize, cursor);
    assert_eq!(caret.bounds.y, b.y + 18);
}

#[test]
fn notepad_menus_open_under_their_titles_and_do_what_they_say() {
    let (mut world, actor) = world("virtual-windows-11");
    launch(&mut world, &actor, "editor");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({ "text": "Log " }),
    );
    // Edit ▸ Time/Date types the machine's own clock, and the menu closes.
    click(&mut world, &actor, "shell:panel:edit");
    click(&mut world, &actor, "shell:insert:*");
    assert_eq!(
        focused_state(&world, &actor)["text"],
        "Log 9:00 AM 9/17/2026"
    );
    assert_eq!(desktop(&world, &actor)["panel"], Value::Null);
    // View ▸ Word wrap, and the menu closes behind it.
    click(&mut world, &actor, "shell:panel:view");
    click(&mut world, &actor, "shell:toggle:word_wrap");
    let state = desktop(&world, &actor);
    assert_eq!(state["settings"]["word_wrap"], true);
    assert_eq!(state["panel"], Value::Null);
    // File ▸ New window opens a second Notepad.
    click(&mut world, &actor, "shell:panel:file");
    click(&mut world, &actor, "shell:new");
    assert_eq!(
        desktop(&world, &actor)["windows"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn gnome_primary_menus_wrap_the_editor_and_clear_the_terminal() {
    let (mut world, actor) = world("virtual-ubuntu-24");
    launch(&mut world, &actor, "editor");
    click(&mut world, &actor, "shell:panel:app-menu");
    click(&mut world, &actor, "shell:toggle:word_wrap");
    let state = desktop(&world, &actor);
    assert_eq!(state["settings"]["word_wrap"], true);
    assert_eq!(state["panel"], Value::Null);

    launch(&mut world, &actor, "terminal");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({ "text": "echo hi" }),
    );
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Enter"}),
    );
    assert!(!focused_state(&world, &actor)["transcript"]
        .as_array()
        .unwrap()
        .is_empty());
    click(&mut world, &actor, "shell:panel:app-menu");
    click(&mut world, &actor, "shell:terminal:clear");
    assert!(focused_state(&world, &actor)["transcript"]
        .as_array()
        .unwrap()
        .is_empty());
    // Full Screen maximizes the window it belongs to.
    let window = desktop(&world, &actor)["focused"].as_u64().unwrap();
    click(&mut world, &actor, "shell:panel:app-menu");
    click(&mut world, &actor, &format!("window:{window}:maximize"));
    assert_eq!(
        desktop(&world, &actor)["windows"][window.to_string()]["maximized"],
        true
    );
}

fn heading_size(world: &World, actor: &str, width: u32, height: u32) -> u16 {
    world
        .scene(actor, width, height)
        .unwrap()
        .nodes
        .iter()
        .find_map(|n| match &n.primitive {
            Primitive::UiTextBold { text, size, .. } | Primitive::UiText { text, size, .. }
                if text.starts_with("Software you") =>
            {
                Some(*size)
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "no hero heading in {:?}",
                world.interfaces().session(actor).unwrap().machines["alice-mac"]
                    .browser
                    .url()
            )
        })
}
fn zoom_of(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    serde_json::to_value(&session.machines["alice-mac"].browser).unwrap()["zoom"].clone()
}

#[test]
fn browser_zoom_relays_the_page_larger_and_is_remembered_per_site() {
    let (mut world, actor) = world("virtual-macos-golden-gate");
    let go = |world: &mut World, url: &str| {
        act(
            world,
            &actor,
            "browser.v1",
            "navigate",
            json!({ "url": url }),
        );
    };
    go(&mut world, "http://northstar.example/");
    let normal = heading_size(&world, &actor, W, H);
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Meta+="}),
    );
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Meta+="}),
    );
    assert_eq!(zoom_of(&world, &actor)["http://northstar.example"], 125);
    let zoomed = heading_size(&world, &actor, W, H);
    assert!(zoomed > normal, "{zoomed} <= {normal}");
    // Safari's View menu offers Actual Size once the page is zoomed.
    click(&mut world, &actor, "shell:panel:view");
    assert!(targets(&world, &actor, W, H).contains(&"shell:zoom:reset".to_owned()));
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Escape"}),
    );
    // Another site is at its own zoom, and the first keeps its own.
    go(&mut world, "http://news.ycombinator.com/");
    assert_eq!(
        zoom_of(&world, &actor).get("http://news.ycombinator.com"),
        None
    );
    go(&mut world, "http://northstar.example/");
    assert_eq!(heading_size(&world, &actor, W, H), zoomed);
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Meta+0"}),
    );
    assert_eq!(heading_size(&world, &actor, W, H), normal);
}

#[test]
fn safari_aa_menu_changes_the_text_size() {
    let (mut world, actor) = world("virtual-ios-18");
    act(
        &mut world,
        &actor,
        "browser.v1",
        "navigate",
        json!({ "url": "http://northstar.example/" }),
    );
    let (w, h) = (390, 844);
    let normal = heading_size(&world, &actor, w, h);
    click_at(&mut world, &actor, "shell:panel:page", w, h);
    click_at(&mut world, &actor, "shell:zoom:in", w, h);
    assert_eq!(zoom_of(&world, &actor)["http://northstar.example"], 115);
    assert!(heading_size(&world, &actor, w, h) > normal);
    click_at(&mut world, &actor, "shell:zoom:reset", w, h);
    assert_eq!(heading_size(&world, &actor, w, h), normal);
}
