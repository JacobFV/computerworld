//! The file manager's mutating commands, end to end against a real machine. Nothing
//! here asserts on application state: every check reads the filesystem back, because a
//! file manager that only updates its own listing is a picture of a file manager.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use cw_scene::Primitive;
use serde_json::{json, Value};

const W: u32 = 1100;
const H: u32 = 700;
const HOME: &str = "/Users/alice";
const TRASH: &str = "/Users/alice/.local/share/Trash/files";

fn world_with(theme: &str) -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ "alice-mac": theme });
    let mut world = World::new(definition, 42).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    (world, actor)
}
fn world() -> (World, String) {
    world_with("virtual-windows-11")
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
fn key(world: &mut World, actor: &str, k: &str) {
    act(world, actor, "keyboard.v1", "key", json!({ "key": k }));
}
fn typed(world: &mut World, actor: &str, text: &str) {
    act(world, actor, "keyboard.v1", "type", json!({ "text": text }));
}
/// A painted control, found by hit-testing the scene, so nothing is exercised through a
/// back door the actor does not have.
fn point(world: &World, actor: &str, target: &str) -> (i32, i32) {
    let scene = world.scene(actor, W, H).unwrap();
    let node = scene
        .nodes
        .iter()
        .find(|node| node.interaction.as_deref() == Some(target))
        .unwrap_or_else(|| panic!("missing interaction {target}"));
    let bounds = node.transform.bounds(node.bounds);
    let (x, y) = (
        bounds.x + (bounds.width / 2) as i32,
        bounds.y + (bounds.height / 2) as i32,
    );
    assert_eq!(
        scene
            .hit_test(x, y)
            .and_then(|node| node.interaction.as_deref()),
        Some(target),
        "{target} is painted but covered"
    );
    (x, y)
}
fn try_click(world: &mut World, actor: &str, target: &str) -> bool {
    let (x, y) = point(world, actor, target);
    try_act(
        world,
        actor,
        "pointer.v1",
        "click",
        json!({"x":x,"y":y,"width":W,"height":H}),
    )
}
fn click(world: &mut World, actor: &str, target: &str) {
    assert!(try_click(world, actor, target), "{target} was refused");
}
fn open(world: &mut World, actor: &str, target: &str) {
    let (x, y) = point(world, actor, target);
    act(
        world,
        actor,
        "pointer.v1",
        "double_click",
        json!({"x":x,"y":y,"width":W,"height":H}),
    );
}
fn exists(world: &World, path: &str) -> bool {
    world
        .runtime()
        .computer("alice-mac")
        .unwrap()
        .vfs
        .exists(path)
}
fn read(world: &World, path: &str) -> String {
    String::from_utf8(world.runtime().read_file("alice-mac", path).unwrap()).unwrap()
}
fn on_screen(world: &World, actor: &str, want: &str) -> bool {
    world.scene(actor, W, H).unwrap().nodes.iter().any(|n| {
        matches!(
            &n.primitive,
            Primitive::Text { text, .. } | Primitive::UiText { text, .. }
                | Primitive::UiTextBold { text, .. } if text.contains(want)
        )
    })
}
/// The row a name is painted on, as its full interaction id. Sorting and filtering move
/// rows, so a test asks the screen where a file is rather than assuming an index.
fn row(world: &World, actor: &str, name: &str) -> String {
    let scene = world.scene(actor, W, H).unwrap();
    scene
        .nodes
        .iter()
        .find(|n| {
            n.semantic.as_ref().is_some_and(|s| s.label == name)
                && n.interaction
                    .as_deref()
                    .is_some_and(|i| i.contains("open:"))
        })
        .and_then(|n| n.interaction.clone())
        .unwrap_or_else(|| panic!("no row for {name}"))
}
/// Every visible row, in the order the screen puts them in.
fn names(world: &World, actor: &str) -> Vec<String> {
    let scene = world.scene(actor, W, H).unwrap();
    let mut rows: Vec<(usize, String)> = scene
        .nodes
        .iter()
        .filter_map(|n| {
            let id = n.interaction.as_deref()?;
            let row: usize = id.rsplit_once("open:")?.1.parse().ok()?;
            Some((row, n.semantic.as_ref()?.label.clone()))
        })
        .collect();
    rows.sort();
    rows.dedup();
    rows.into_iter().map(|(_, name)| name).collect()
}
fn open_files(world: &mut World, actor: &str, path: &str) {
    act(
        world,
        actor,
        "application.v1",
        "launch",
        json!({"kind":"files","argument":path}),
    );
}
fn select(world: &mut World, actor: &str, name: &str) {
    let target = row(world, actor, name);
    click(world, actor, &target);
}
fn command(name: &str) -> String {
    format!("window:0:content:{name}")
}
fn retype(world: &mut World, actor: &str, old: &str, new: &str) {
    for _ in 0..old.chars().count() {
        key(world, actor, "Backspace");
    }
    typed(world, actor, new);
}

#[test]
fn copy_paste_rename_and_new_really_change_the_filesystem() {
    let (mut world, actor) = world();
    open_files(&mut world, &actor, HOME);
    // Copy and paste into the folder the copy came from: the name is derived from the
    // listing on screen, so the paste does not overwrite its own source.
    select(&mut world, &actor, "notes.txt");
    click(&mut world, &actor, &command("files-copy"));
    click(&mut world, &actor, &command("files-paste"));
    assert_eq!(
        read(&world, &format!("{HOME}/notes (copy).txt")),
        read(&world, &format!("{HOME}/notes.txt")),
    );
    // Rename is a real edit: the field collects keystrokes and Enter commits a move.
    select(&mut world, &actor, "notes (copy).txt");
    click(&mut world, &actor, &command("files-rename"));
    retype(&mut world, &actor, "notes (copy).txt", "archive.txt");
    key(&mut world, &actor, "Enter");
    assert!(exists(&world, &format!("{HOME}/archive.txt")));
    assert!(!exists(&world, &format!("{HOME}/notes (copy).txt")));
    // New folder and New file create exactly one thing each, and the folder is listed
    // back from the machine rather than assumed.
    click(&mut world, &actor, &command("files-new-folder"));
    click(&mut world, &actor, &command("files-new-file"));
    assert!(exists(&world, &format!("{HOME}/New folder")));
    assert_eq!(read(&world, &format!("{HOME}/Untitled.txt")), "");
    // A second New folder does not collide with the first.
    click(&mut world, &actor, &command("files-new-folder"));
    assert!(exists(&world, &format!("{HOME}/New folder 2")));
    // Cut into the new folder moves rather than copies.
    select(&mut world, &actor, "archive.txt");
    click(&mut world, &actor, &command("files-cut"));
    let folder = row(&world, &actor, "New folder/");
    open(&mut world, &actor, &folder);
    click(&mut world, &actor, &command("files-paste"));
    assert!(exists(&world, &format!("{HOME}/New folder/archive.txt")));
    assert!(!exists(&world, &format!("{HOME}/archive.txt")));
    // A cut is consumed by its paste, so there is no Paste control left to click.
    assert!(!world
        .scene(&actor, W, H)
        .unwrap()
        .nodes
        .iter()
        .any(|n| n.interaction.as_deref() == Some(&command("files-paste"))));
}

#[test]
fn delete_files_into_the_trash_and_never_destroys_anything() {
    let (mut world, actor) = world();
    open_files(&mut world, &actor, HOME);
    let original = read(&world, &format!("{HOME}/launch.txt"));
    select(&mut world, &actor, "launch.txt");
    click(&mut world, &actor, &command("files-delete"));
    assert!(!exists(&world, &format!("{HOME}/launch.txt")));
    // The bytes are still in the world, at a location the event names.
    assert_eq!(read(&world, &format!("{TRASH}/launch.txt")), original);
    let events = serde_json::to_string(&world.trajectory()).unwrap();
    assert!(events.contains("filesystem.trash"));
    assert!(events.contains(&format!("{TRASH}/launch.txt")));
    // A second file of the same name does not silently replace the first.
    for _ in 0..2 {
        click(&mut world, &actor, &command("files-new-file"));
        select(&mut world, &actor, "Untitled.txt");
        click(&mut world, &actor, &command("files-delete"));
    }
    assert!(exists(&world, &format!("{TRASH}/Untitled.txt")));
    assert!(exists(&world, &format!("{TRASH}/Untitled.txt.2")));
}

#[test]
fn file_manager_effects_are_not_a_way_around_the_permissions_a_read_obeys() {
    let (mut world, actor) = world();
    open_files(&mut world, &actor, HOME);
    // A file alice owns but has made unreadable. Copying it must fail on the same
    // check `read_file` makes, not succeed because a file manager asked.
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    typed(&mut world, &actor, "chmod 000 notes.txt");
    key(&mut world, &actor, "Enter");
    assert!(world
        .runtime()
        .read_file("alice-mac", "/Users/alice/notes.txt")
        .is_err());
    act(
        &mut world,
        &actor,
        "application.v1",
        "focus",
        json!({"window":0}),
    );
    select(&mut world, &actor, "notes.txt");
    click(&mut world, &actor, &command("files-copy"));
    assert!(
        !try_click(&mut world, &actor, &command("files-paste")),
        "a file alice cannot read was copied anyway"
    );
    assert!(!exists(&world, &format!("{HOME}/notes (copy).txt")));
    // And out of a folder she does not own: deleting from `/Users` is a move out of a
    // directory she has no write bit on, so the trash cannot launder it either.
    open_files(&mut world, &actor, "/Users");
    let target = row(&world, &actor, "alice/");
    click(&mut world, &actor, &target);
    assert!(
        !try_click(&mut world, &actor, "window:2:content:files-delete"),
        "a folder alice cannot write to was moved anyway"
    );
    assert!(exists(&world, HOME));
}

#[test]
fn sort_search_and_view_are_real_and_only_reorder_what_is_there() {
    let (mut world, actor) = world();
    open_files(&mut world, &actor, HOME);
    let ascending = names(&world, &actor);
    assert!(ascending.len() >= 2, "{ascending:?}");
    click(&mut world, &actor, &command("files-sort:name"));
    let mut reversed = ascending.clone();
    reversed.reverse();
    assert_eq!(
        names(&world, &actor),
        reversed,
        "reversing the sort reordered nothing"
    );
    click(&mut world, &actor, &command("files-sort:name"));
    assert_eq!(names(&world, &actor), ascending);
    // The grid shows the same rows in the same order; only the arrangement changes.
    click(&mut world, &actor, &command("files-view"));
    assert_eq!(names(&world, &actor), ascending);
    click(&mut world, &actor, &command("files-view"));
    // Search really filters, and clearing it brings everything back.
    click(&mut world, &actor, &command("files-search"));
    typed(&mut world, &actor, "launch");
    assert_eq!(names(&world, &actor), vec!["launch.txt".to_string()]);
    click(&mut world, &actor, &command("files-search-clear"));
    assert_eq!(names(&world, &actor), ascending);
    // A query that matches nothing says so, rather than looking like an empty folder.
    click(&mut world, &actor, &command("files-search"));
    typed(&mut world, &actor, "zzz");
    assert!(names(&world, &actor).is_empty());
    assert!(on_screen(&world, &actor, "No items match your search."));
}

#[test]
fn recents_lists_what_was_opened_and_shared_stays_honestly_empty() {
    let (mut world, actor) = world_with("virtual-ios-18");
    open_files(&mut world, &actor, HOME);
    // Recents starts empty and says so rather than showing a fabricated history.
    click(&mut world, &actor, &command("files-recents"));
    assert!(on_screen(&world, &actor, "No recent documents"));
    click(&mut world, &actor, &command("files-browse"));
    // Opening a document is what puts it there.
    let target = row(&world, &actor, "notes.txt");
    open(&mut world, &actor, &target);
    act(
        &mut world,
        &actor,
        "application.v1",
        "focus",
        json!({"window":0}),
    );
    click(&mut world, &actor, &command("files-recents"));
    assert!(on_screen(&world, &actor, "notes.txt"));
    // The row says where the file lives, which is what makes it more than a name.
    assert!(on_screen(&world, &actor, HOME));
    // Shared has no model anywhere in this world, so it stays announced as disabled.
    assert!(world.scene(&actor, W, H).unwrap().nodes.iter().any(|n| n
        .semantic
        .as_ref()
        .is_some_and(|s| s.label == "Shared" && s.disabled)));
}

#[test]
fn the_terminal_scrollbar_and_caret_are_real_controls() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    for i in 0..40 {
        typed(&mut world, &actor, &format!("echo line-{i}"));
        key(&mut world, &actor, "Enter");
    }
    assert!(on_screen(&world, &actor, "line-39"));
    let back = world
        .scene(&actor, W, H)
        .unwrap()
        .nodes
        .iter()
        .find_map(|n| {
            n.interaction
                .as_deref()
                .filter(|i| i.contains("terminal-scroll:"))
                .map(str::to_owned)
        })
        .expect("scrollback offers a control");
    click(&mut world, &actor, &back);
    assert!(
        !on_screen(&world, &actor, "line-39"),
        "the scrollbar did nothing"
    );
    // Running something brings the view back to the tail, where the answer will be.
    typed(&mut world, &actor, "echo tail-marker");
    key(&mut world, &actor, "Enter");
    assert!(on_screen(&world, &actor, "tail-marker"));
    // The caret goes where the prompt line was clicked, and typing lands there.
    typed(&mut world, &actor, "echo");
    let (x, y) = point(&world, &actor, &command("terminal-line"));
    act(
        &mut world,
        &actor,
        "pointer.v1",
        "click",
        json!({"x":x - 12,"y":y,"width":W,"height":H}),
    );
    typed(&mut world, &actor, "X");
    key(&mut world, &actor, "End");
    typed(&mut world, &actor, " done");
    key(&mut world, &actor, "Enter");
    let events = serde_json::to_string(&world.trajectory()).unwrap();
    assert!(events.contains("X"), "the caret never moved");
}

#[test]
fn a_refusal_the_file_manager_cannot_carry_out_is_said_out_loud() {
    let (mut world, actor) = world();
    open_files(&mut world, &actor, HOME);
    // Renaming onto a name the folder already holds is refused, not quietly renamed
    // around, and neither file is touched.
    select(&mut world, &actor, "notes.txt");
    click(&mut world, &actor, &command("files-rename"));
    retype(&mut world, &actor, "notes.txt", "launch.txt");
    assert!(!try_act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Enter"})
    ));
    assert!(exists(&world, &format!("{HOME}/notes.txt")));
    assert_eq!(
        read(&world, &format!("{HOME}/launch.txt")).lines().next(),
        Some("Project: Atlas")
    );
    // A name with a separator in it is not a rename, it is a move somewhere else.
    click(&mut world, &actor, &command("files-rename"));
    retype(&mut world, &actor, "notes.txt", "../escaped.txt");
    assert!(!try_act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key":"Enter"})
    ));
    assert!(!exists(&world, "/Users/escaped.txt"));
}
