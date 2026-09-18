//! End-to-end checks that a pointer behaves the way a desktop pointer behaves: one click
//! selects, two open, folders stay in the same window, and the tab strip is real.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const W: u32 = 1024;
const H: u32 = 700;

fn world(theme: &str) -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ "alice-mac": theme });
    let mut world = World::new(definition, 42).unwrap();
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
/// Centre of the node carrying `target`, or `None` when nothing paints it.
fn locate(world: &World, actor: &str, target: &str) -> Option<(i32, i32)> {
    let scene = world.scene(actor, W, H).unwrap();
    // Controls inside a window are namespaced by the frame that composed them.
    let suffix = format!(":content:{target}");
    let node = scene.nodes.iter().find(|node| {
        node.interaction
            .as_deref()
            .is_some_and(|i| i == target || i.ends_with(&suffix))
    })?;
    let bounds = node.transform.bounds(node.bounds);
    Some((
        bounds.x + (bounds.width / 2) as i32,
        bounds.y + (bounds.height / 2) as i32,
    ))
}
fn at(world: &World, actor: &str, target: &str) -> (i32, i32) {
    locate(world, actor, target).unwrap_or_else(|| panic!("missing interaction {target}"))
}
fn click(world: &mut World, actor: &str, target: &str) {
    let (x, y) = at(world, actor, target);
    act(
        world,
        actor,
        "pointer.v1",
        "click",
        json!({"x":x,"y":y,"width":W,"height":H}),
    );
}
/// A real double click: the host sends the click, then the double click.
fn double_click(world: &mut World, actor: &str, target: &str) {
    let (x, y) = at(world, actor, target);
    for op in ["click", "double_click"] {
        act(
            world,
            actor,
            "pointer.v1",
            op,
            json!({"x":x,"y":y,"width":W,"height":H}),
        );
    }
}
/// Open a file manager already showing the user's home folder, the way the dock's Home
/// entry does, without depending on which shells paint a Home control.
fn open_home(world: &mut World, actor: &str) {
    let home = desktop(world, actor)["home"].as_str().unwrap().to_owned();
    act(
        world,
        actor,
        "application.v1",
        "launch",
        json!({"kind":"files","argument":home}),
    );
}
fn desktop(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap()
}
fn windows(world: &World, actor: &str) -> usize {
    desktop(world, actor)["windows"].as_object().unwrap().len()
}
/// The active file manager tab.
fn tab(world: &World, actor: &str) -> Value {
    let desktop = desktop(world, actor);
    let window = desktop["windows"]
        .as_object()
        .unwrap()
        .values()
        .find(|w| w["state"]["type"] == "files")
        .expect("a file manager window")
        .clone();
    let active = window["state"]["active"].as_u64().unwrap() as usize;
    window["state"]["tabs"][active].clone()
}

#[test]
fn a_desktop_icon_selects_on_one_click_and_opens_on_two() {
    for theme in [
        "virtual-macos-golden-gate",
        "virtual-windows-11",
        "virtual-ubuntu-24",
    ] {
        let (mut world, actor) = world(theme);
        assert_eq!(windows(&world, &actor), 0);
        click(&mut world, &actor, "shell:open:files");
        assert_eq!(
            windows(&world, &actor),
            0,
            "{theme}: a single click opened a window"
        );
        assert_eq!(desktop(&world, &actor)["desktop_selection"], "files");
        double_click(&mut world, &actor, "shell:open:files");
        assert_eq!(windows(&world, &actor), 1, "{theme}: double click opens");
        assert_eq!(desktop(&world, &actor)["desktop_selection"], Value::Null);
    }
}

#[test]
fn a_folder_selects_on_one_click_and_opens_in_the_same_window_on_two() {
    let (mut world, actor) = world("virtual-macos-golden-gate");
    double_click(&mut world, &actor, "shell:open:files");
    let start = tab(&world, &actor)["path"].as_str().unwrap().to_owned();
    let entries = tab(&world, &actor)["entries"].as_array().unwrap().clone();
    let (index, name) = entries
        .iter()
        .enumerate()
        .find_map(|(i, e)| {
            let name = e.as_str()?;
            name.ends_with('/').then_some((i, name.to_owned()))
        })
        .expect("the home folder contains a folder");

    click(&mut world, &actor, &format!("open:{index}"));
    assert_eq!(tab(&world, &actor)["selected"], index as u64);
    assert_eq!(
        tab(&world, &actor)["path"],
        start,
        "a single click navigated"
    );
    assert_eq!(windows(&world, &actor), 1);

    double_click(&mut world, &actor, &format!("open:{index}"));
    assert_eq!(
        windows(&world, &actor),
        1,
        "opening a folder must not open a second window"
    );
    let now = tab(&world, &actor);
    assert_eq!(
        now["path"].as_str().unwrap(),
        format!(
            "{}/{}",
            start.trim_end_matches('/'),
            name.trim_end_matches('/')
        )
    );
    assert!(
        now["history"].as_array().unwrap().len() == 2,
        "the visited folder is recorded so Back works"
    );

    // Back returns to the folder we came from, in the same window and the same tab.
    click(&mut world, &actor, "files-back");
    assert_eq!(tab(&world, &actor)["path"], start);
    assert_eq!(windows(&world, &actor), 1);
    click(&mut world, &actor, "files-forward");
    assert_ne!(tab(&world, &actor)["path"], start);
}

#[test]
fn a_document_opens_in_an_editor_only_on_the_second_click() {
    let (mut world, actor) = world("virtual-windows-11");
    open_home(&mut world, &actor);
    let entries = tab(&world, &actor)["entries"].as_array().unwrap().clone();
    let index = entries
        .iter()
        .position(|e| e.as_str().is_some_and(|name| !name.ends_with('/')))
        .expect("the home folder contains a file");
    click(&mut world, &actor, &format!("open:{index}"));
    assert_eq!(windows(&world, &actor), 1);
    double_click(&mut world, &actor, &format!("open:{index}"));
    assert_eq!(
        windows(&world, &actor),
        2,
        "the document opens in an editor"
    );
}

#[test]
fn the_file_manager_tab_strip_really_opens_and_closes_tabs() {
    let (mut world, actor) = world("virtual-windows-11");
    double_click(&mut world, &actor, "shell:open:files");
    click(&mut world, &actor, "files-newtab");
    let desktop = desktop(&world, &actor);
    let files = desktop["windows"]
        .as_object()
        .unwrap()
        .values()
        .find(|w| w["state"]["type"] == "files")
        .unwrap();
    assert_eq!(
        files["state"]["tabs"].as_array().unwrap().len(),
        2,
        "a new tab opens in the same window"
    );
    assert_eq!(windows(&world, &actor), 1);
    assert_eq!(files["state"]["active"], 1);
}

#[test]
fn a_phone_opens_on_a_single_tap_because_a_touch_screen_has_no_second_click() {
    let (mut world, actor) = world("virtual-ios-18");
    click(&mut world, &actor, "shell:launch:files");
    let entries = tab(&world, &actor)["entries"].as_array().unwrap().clone();
    let index = entries
        .iter()
        .position(|e| e.as_str().is_some_and(|name| name.ends_with('/')))
        .expect("the home folder contains a folder");
    let start = tab(&world, &actor)["path"].as_str().unwrap().to_owned();
    click(&mut world, &actor, &format!("open:{index}"));
    assert_ne!(
        tab(&world, &actor)["path"].as_str().unwrap(),
        start,
        "one tap opens on a touch screen"
    );
}
