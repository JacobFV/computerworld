//! The standard places in Files, Finder and Explorer, end to end: every sidebar row is
//! found by hit-testing the painted scene and clicked the way a pointer clicks it, and
//! every check reads the file manager's state or the machine back. The home folders
//! the rows lead to are the ones a desktop's first login really made.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const W: u32 = 1280;
const H: u32 = 800;
const HOME: &str = "/Users/alice";
const TRASH: &str = "/Users/alice/.local/share/Trash/files";

fn world(theme: &str) -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ "alice-mac": theme });
    let mut world = World::new(definition, 42).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    (world, actor)
}
fn try_act(world: &mut World, actor: &str, family: &str, op: &str, payload: Value) -> bool {
    world
        .step(
            actor,
            vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
        )
        .unwrap()
        .outcomes[0]
        .success
}
fn act(world: &mut World, actor: &str, family: &str, op: &str, payload: Value) {
    assert!(try_act(world, actor, family, op, payload), "{family} {op}");
}
/// Centre of the painted control ending in `target`, checked to be on top.
fn point(world: &World, actor: &str, target: &str) -> Option<(i32, i32)> {
    let scene = world.scene(actor, W, H).unwrap();
    let suffix = format!(":content:{target}");
    let node = scene.nodes.iter().find(|n| {
        n.interaction
            .as_deref()
            .is_some_and(|i| i == target || i.ends_with(&suffix))
    })?;
    let b = node.transform.bounds(node.bounds);
    let (x, y) = (b.x + b.width as i32 / 2, b.y + b.height as i32 / 2);
    let top = scene.hit_test(x, y).and_then(|n| n.interaction.clone());
    assert_eq!(
        top.as_deref(),
        node.interaction.as_deref(),
        "{target} is painted but covered"
    );
    Some((x, y))
}
fn painted(world: &World, actor: &str, target: &str) -> bool {
    point(world, actor, target).is_some()
}
fn pointer(world: &mut World, actor: &str, target: &str, op: &str) {
    let (x, y) = point(world, actor, target).unwrap_or_else(|| panic!("{target} is not painted"));
    act(
        world,
        actor,
        "pointer.v1",
        op,
        json!({"x":x,"y":y,"width":W,"height":H}),
    );
}
fn click(world: &mut World, actor: &str, target: &str) {
    pointer(world, actor, target, "click");
}
fn double_click(world: &mut World, actor: &str, target: &str) {
    pointer(world, actor, target, "click");
    pointer(world, actor, target, "double_click");
}
fn desktop(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap()
}
/// The active tab of the focused file manager.
fn tab(world: &World, actor: &str) -> Value {
    let desktop = desktop(world, actor);
    let focused = desktop["focused"].as_u64().unwrap().to_string();
    let state = &desktop["windows"][&focused]["state"];
    assert_eq!(state["type"], "files", "a file manager is focused");
    state["tabs"][state["active"].as_u64().unwrap() as usize].clone()
}
fn scope(world: &World, actor: &str) -> String {
    tab(world, actor)["scope"]
        .as_str()
        .unwrap_or("folder")
        .to_owned()
}
fn path(world: &World, actor: &str) -> String {
    tab(world, actor)["path"].as_str().unwrap().to_owned()
}
fn entries(world: &World, actor: &str) -> Vec<String> {
    tab(world, actor)["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_owned())
        .collect()
}
fn open_files(world: &mut World, actor: &str) {
    act(
        world,
        actor,
        "application.v1",
        "launch",
        json!({"kind":"files"}),
    );
}
fn is_dir(world: &World, path: &str) -> bool {
    world
        .runtime()
        .computer("alice-mac")
        .unwrap()
        .vfs
        .stat(path)
        .is_ok_and(|m| m.is_dir)
}
/// The row a name is painted on, as its `open:<i>` target.
fn row(world: &World, actor: &str, name: &str) -> String {
    let scene = world.scene(actor, W, H).unwrap();
    scene
        .nodes
        .iter()
        .find(|n| {
            n.semantic
                .as_ref()
                .is_some_and(|s| s.label.trim_end_matches('/').ends_with(name))
                && n.interaction
                    .as_deref()
                    .is_some_and(|i| i.contains("open:"))
        })
        .and_then(|n| {
            n.interaction
                .as_deref()?
                .split(":content:")
                .nth(1)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| panic!("no row for {name}"))
}

#[test]
fn a_desktop_login_makes_the_platforms_home_folders_and_the_trash() {
    for (theme, folders) in [
        (
            "virtual-ubuntu-24",
            &[
                "Desktop",
                "Documents",
                "Downloads",
                "Music",
                "Pictures",
                "Public",
                "Templates",
                "Videos",
            ][..],
        ),
        (
            "virtual-macos-golden-gate",
            &[
                "Desktop",
                "Documents",
                "Downloads",
                "Movies",
                "Music",
                "Pictures",
                "Public",
            ][..],
        ),
        (
            "virtual-windows-11",
            &[
                "Desktop",
                "Documents",
                "Downloads",
                "Music",
                "Pictures",
                "Videos",
            ][..],
        ),
    ] {
        let (world, _) = world(theme);
        for folder in folders {
            assert!(
                is_dir(&world, &format!("{HOME}/{folder}")),
                "{theme}: {folder}"
            );
        }
        assert!(is_dir(&world, TRASH), "{theme}: trash");
        // The folders are the platform's own: no Movies on Ubuntu, no Videos on a Mac.
        let foreign = if theme.contains("macos") {
            "Videos"
        } else {
            "Movies"
        };
        assert!(!is_dir(&world, &format!("{HOME}/{foreign}")), "{theme}");
    }
    // A session without a desktop logs nobody in and makes nothing.
    let mut world = World::new(reference_world(), 42).unwrap();
    world
        .environment(EnvironmentConfig::terminal("alice", "alice-mac"))
        .unwrap();
    assert!(!is_dir(&world, &format!("{HOME}/Documents")));
}

#[test]
fn logging_in_again_restoring_and_resetting_all_agree_with_a_fresh_world() {
    let (mut world, _) = world("virtual-ubuntu-24");
    let events = world.trajectory().len();
    let hash = world.state_hash().unwrap();
    let snapshot = world.snapshot();
    // A second login finds everything already there and makes nothing.
    world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    assert_eq!(
        world.trajectory().len(),
        events,
        "a second login made folders"
    );
    // A snapshot carries the folders; restoring it reproduces the same world.
    world.restore(&snapshot).unwrap();
    assert_eq!(world.state_hash().unwrap(), hash);
    assert!(is_dir(&world, &format!("{HOME}/Templates")));
    let json = world.export_snapshot().unwrap();
    let (mut other, _) = self::world("virtual-ubuntu-24");
    other.import_snapshot(&json).unwrap();
    assert_eq!(other.state_hash().unwrap(), hash);
    // A reset world is logged into afresh, so it has them again.
    world.reset(42).unwrap();
    assert!(is_dir(&world, &format!("{HOME}/Templates")));
    assert!(is_dir(&world, TRASH));
}

#[test]
fn every_files_sidebar_place_opens_what_it_names() {
    let (mut world, actor) = world("virtual-ubuntu-24");
    open_files(&mut world, &actor);
    assert_eq!(path(&world, &actor), HOME, "Files opens on Home");
    for (target, want_scope, want_path) in [
        ("files-recents", "recents", None),
        ("files-starred", "starred", None),
        ("files-home", "folder", Some(HOME.to_owned())),
        (
            "files-location:/Users/alice/Desktop",
            "folder",
            Some(format!("{HOME}/Desktop")),
        ),
        (
            "files-location:/Users/alice/Documents",
            "folder",
            Some(format!("{HOME}/Documents")),
        ),
        (
            "files-location:/Users/alice/Downloads",
            "folder",
            Some(format!("{HOME}/Downloads")),
        ),
        (
            "files-location:/Users/alice/Music",
            "folder",
            Some(format!("{HOME}/Music")),
        ),
        (
            "files-location:/Users/alice/Pictures",
            "folder",
            Some(format!("{HOME}/Pictures")),
        ),
        (
            "files-location:/Users/alice/Videos",
            "folder",
            Some(format!("{HOME}/Videos")),
        ),
        ("files-trash", "folder", Some(TRASH.to_owned())),
        ("files-root", "folder", Some("/".to_owned())),
    ] {
        click(&mut world, &actor, target);
        assert_eq!(scope(&world, &actor), want_scope, "{target}");
        if let Some(want) = want_path {
            assert_eq!(path(&world, &actor), want, "{target}");
        }
    }
    // The places Files has no model for are not painted at all.
    for absent in ["files-quick-access", "files-gallery"] {
        assert!(!painted(&world, &actor, absent), "{absent}");
    }
}

#[test]
fn a_standard_folder_that_is_gone_is_not_offered() {
    let (mut world, actor) = world("virtual-ubuntu-24");
    act(
        &mut world,
        &actor,
        "terminal.v1",
        "execute",
        json!({"command":"rmdir /Users/alice/Music"}),
    );
    open_files(&mut world, &actor);
    assert!(!painted(
        &world,
        &actor,
        "files-location:/Users/alice/Music"
    ));
    assert!(painted(
        &world,
        &actor,
        "files-location:/Users/alice/Videos"
    ));
}

#[test]
fn finder_favorites_and_locations_open_what_they_name() {
    let (mut world, actor) = world("virtual-macos-golden-gate");
    open_files(&mut world, &actor);
    for (target, want_scope, want_path) in [
        ("files-recents", "recents", None),
        (
            "files-location:/Users/alice/Desktop",
            "folder",
            Some(format!("{HOME}/Desktop")),
        ),
        (
            "files-location:/Users/alice/Documents",
            "folder",
            Some(format!("{HOME}/Documents")),
        ),
        (
            "files-location:/Users/alice/Downloads",
            "folder",
            Some(format!("{HOME}/Downloads")),
        ),
        ("files-root", "folder", Some("/".to_owned())),
    ] {
        click(&mut world, &actor, target);
        assert_eq!(scope(&world, &actor), want_scope, "{target}");
        if let Some(want) = want_path {
            assert_eq!(path(&world, &actor), want, "{target}");
        }
    }
    // Finder's Go menu reaches the same places, and Home, from the menu bar.
    click(&mut world, &actor, "shell:panel:go");
    let home = format!("files-location:{HOME}");
    click(&mut world, &actor, &home);
    assert_eq!(path(&world, &actor), HOME);
}

#[test]
fn explorer_home_gallery_pins_and_this_pc_open_what_they_name() {
    let (mut world, actor) = world("virtual-windows-11");
    act(
        &mut world,
        &actor,
        "filesystem.v1",
        "write",
        json!({"path":"/Users/alice/Pictures/beach.png","content":"not really a png"}),
    );
    act(
        &mut world,
        &actor,
        "filesystem.v1",
        "write",
        json!({"path":"/Users/alice/Pictures/caption.txt","content":"words"}),
    );
    open_files(&mut world, &actor);
    // Home: the pinned folders the home really holds, in Explorer's pinned order.
    click(&mut world, &actor, "files-quick-access");
    assert_eq!(scope(&world, &actor), "quick_access");
    let pinned: Vec<String> = [
        "Desktop",
        "Downloads",
        "Documents",
        "Pictures",
        "Music",
        "Videos",
    ]
    .iter()
    .map(|f| format!("{HOME}/{f}/"))
    .collect();
    assert_eq!(entries(&world, &actor), pinned);
    // Opening one from Home goes there.
    let documents = row(&world, &actor, "Documents");
    double_click(&mut world, &actor, &documents);
    assert_eq!(path(&world, &actor), format!("{HOME}/Documents"));
    // Gallery: the images in Pictures, and nothing else that is there.
    click(&mut world, &actor, "files-gallery");
    assert_eq!(scope(&world, &actor), "gallery");
    assert_eq!(path(&world, &actor), format!("{HOME}/Pictures"));
    assert_eq!(entries(&world, &actor), ["beach.png"]);
    for folder in [
        "Desktop",
        "Downloads",
        "Documents",
        "Pictures",
        "Music",
        "Videos",
    ] {
        let target = format!("files-location:{HOME}/{folder}");
        click(&mut world, &actor, &target);
        assert_eq!(path(&world, &actor), format!("{HOME}/{folder}"));
        assert_eq!(scope(&world, &actor), "folder");
    }
    click(&mut world, &actor, "files-root");
    assert_eq!(path(&world, &actor), "/");
    // OneDrive and Network have nothing behind them and are not painted.
    let scene = world.scene(&actor, W, H).unwrap();
    for absent in ["OneDrive", "Network"] {
        assert!(
            !scene
                .nodes
                .iter()
                .any(|n| n.semantic.as_ref().is_some_and(|s| s.label == absent)),
            "{absent}"
        );
    }
}

#[test]
fn a_star_puts_a_row_in_starred_and_takes_it_out_again() {
    let (mut world, actor) = world("virtual-ubuntu-24");
    open_files(&mut world, &actor);
    // Files opens in its icon grid; the star column is the list view's.
    assert_eq!(tab(&world, &actor)["view"], "grid");
    click(&mut world, &actor, "files-view");
    assert_eq!(tab(&world, &actor)["view"], "list");
    let notes = row(&world, &actor, "notes.txt");
    let star = notes.replace("open:", "files-star:");
    click(&mut world, &actor, &star);
    assert_eq!(
        desktop(&world, &actor)["starred"],
        json!([format!("{HOME}/notes.txt")])
    );
    // The row now says it is starred, so the next click is an unstar.
    let scene = world.scene(&actor, W, H).unwrap();
    assert!(scene.nodes.iter().any(|n| n
        .semantic
        .as_ref()
        .is_some_and(|s| s.label == "Unstar notes.txt")));
    // Folders star too, and keep their trailing separator so they open as folders.
    let documents = row(&world, &actor, "Documents");
    click(
        &mut world,
        &actor,
        &documents.replace("open:", "files-star:"),
    );
    click(&mut world, &actor, "files-starred");
    assert_eq!(
        entries(&world, &actor),
        [format!("{HOME}/Documents/"), format!("{HOME}/notes.txt")]
    );
    let documents = row(&world, &actor, "Documents");
    double_click(&mut world, &actor, &documents);
    assert_eq!(path(&world, &actor), format!("{HOME}/Documents"));
    // Unstarring from the Starred list takes the row off it at once.
    click(&mut world, &actor, "files-starred");
    let notes = row(&world, &actor, "notes.txt");
    click(&mut world, &actor, &notes.replace("open:", "files-star:"));
    assert_eq!(entries(&world, &actor), [format!("{HOME}/Documents/")]);
    assert_eq!(
        desktop(&world, &actor)["starred"],
        json!([format!("{HOME}/Documents/")])
    );
}

/// Nautilus's icon grid has no star column: the selected item carries a star button,
/// a starred one keeps a star on its icon, and the context menu stars the selection.
#[test]
fn the_icon_grid_stars_from_the_item_and_from_the_context_menu() {
    let (mut world, actor) = world("virtual-ubuntu-24");
    open_files(&mut world, &actor);
    assert_eq!(tab(&world, &actor)["view"], "grid");
    let notes = row(&world, &actor, "notes.txt");
    let star = notes.replace("open:", "files-star:");
    // Nothing is starred or selected yet, so no item carries a star.
    assert!(!painted(&world, &actor, &star));
    click(&mut world, &actor, &notes);
    click(&mut world, &actor, &star);
    assert_eq!(
        desktop(&world, &actor)["starred"],
        json!([format!("{HOME}/notes.txt")])
    );
    // The context menu over the selection offers the opposite: Unstar.
    act(
        &mut world,
        &actor,
        "application.v1",
        "shell",
        json!({"target":"shell:panel:context"}),
    );
    let scene = world.scene(&actor, W, H).unwrap();
    let unstar = scene
        .nodes
        .iter()
        .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == "Unstar"))
        .and_then(|n| n.interaction.clone())
        .expect("the context menu offers Unstar");
    assert!(unstar.ends_with(":content:files-star"), "{unstar}");
    click(&mut world, &actor, &unstar);
    assert_eq!(desktop(&world, &actor)["starred"], json!([]));
}

#[test]
fn recent_lists_the_documents_really_opened_newest_first() {
    let (mut world, actor) = world("virtual-ubuntu-24");
    open_files(&mut world, &actor);
    click(&mut world, &actor, "files-recents");
    assert!(
        entries(&world, &actor).is_empty(),
        "nothing has been opened yet"
    );
    click(&mut world, &actor, "files-home");
    for name in ["launch.txt", "notes.txt"] {
        let target = row(&world, &actor, name);
        double_click(&mut world, &actor, &target);
        // The document opened in an editor; the dock brings Files back to the front.
        act(
            &mut world,
            &actor,
            "application.v1",
            "shell",
            json!({"target": "shell:launch:files"}),
        );
    }
    click(&mut world, &actor, "files-recents");
    assert_eq!(
        entries(&world, &actor),
        [format!("{HOME}/notes.txt"), format!("{HOME}/launch.txt")]
    );
}

#[test]
fn dot_files_stay_hidden_until_asked_for() {
    let (mut world, actor) = world("virtual-ubuntu-24");
    open_files(&mut world, &actor);
    // The trash lives in ~/.local, which the listing holds and the screen hides.
    assert!(entries(&world, &actor).iter().any(|e| e == ".local/"));
    let hidden = |world: &World| {
        let scene = world.scene(&actor, W, H).unwrap();
        scene
            .nodes
            .iter()
            .any(|n| n.semantic.as_ref().is_some_and(|s| s.label == ".local/"))
    };
    assert!(!hidden(&world));
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Ctrl+h"}),
    );
    assert!(hidden(&world));
}
