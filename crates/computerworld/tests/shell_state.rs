//! Bookmarks, downloads, screenshots, sharing, the trash and virtual desktops. Each of
//! these was a painted-but-dead control until the state behind it existed; these tests
//! read the result back from the machine rather than trusting the shell's own report.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

fn fresh_world() -> (World, String) {
    world()
}
fn world() -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ "alice-mac": "virtual-macos-golden-gate" });
    definition.metadata["desktop_apps"] = json!([
        {"id":"messages","label":"Messages","kind":"native","url":"http://messages.internal/","icon":"messages"},
    ]);
    if let Some(computer) = definition
        .computers
        .iter_mut()
        .find(|c| c.id == "alice-mac")
    {
        computer.installed_apps.push("messages".into());
    }
    let mut world = World::new(definition, 11).unwrap();
    let mut config = EnvironmentConfig::desktop("alice", "alice-mac");
    // A screenshot is a pixel capture, and is refused without that grant.
    config.observations.push("pixels.v1".into());
    let session = world.environment(config).unwrap();
    (world, session)
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
/// Drive a shell control directly, the way a painted node's interaction would.
fn shell(world: &mut World, actor: &str, target: &str) -> Value {
    act(
        world,
        actor,
        "application.v1",
        "shell",
        json!({"target": target}),
    )
}
fn desktop(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap()
}
fn browse(world: &mut World, actor: &str, url: &str) {
    act(
        world,
        actor,
        "browser.v1",
        "navigate",
        json!({ "url": url }),
    );
}

#[test]
fn a_bookmark_is_saved_reopened_and_removed() {
    let (mut world, actor) = world();
    browse(&mut world, &actor, "http://intranet.internal/");
    shell(&mut world, &actor, "shell:bookmark");
    let saved = desktop(&world, &actor)["bookmarks"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0]["url"], "http://intranet.internal/");
    assert!(!saved[0]["title"].as_str().unwrap().is_empty());
    // Saving the same page twice is one bookmark.
    shell(&mut world, &actor, "shell:bookmark");
    shell(&mut world, &actor, "shell:bookmark");
    assert_eq!(
        desktop(&world, &actor)["bookmarks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Opening it really navigates.
    browse(&mut world, &actor, "http://guide.example/");
    shell(&mut world, &actor, "shell:bookmark:open:0");
    let session = world.interfaces().session(&actor).unwrap();
    assert_eq!(
        session.machines["alice-mac"].browser.url(),
        Some("http://intranet.internal/")
    );
}

#[test]
fn a_download_writes_a_real_file_to_the_machine() {
    let (mut world, actor) = world();
    browse(&mut world, &actor, "http://intranet.internal/");
    shell(&mut world, &actor, "shell:download");
    let downloads = desktop(&world, &actor)["downloads"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(downloads.len(), 1, "nothing was recorded");
    let path = downloads[0]["path"].as_str().unwrap();
    let bytes = world
        .runtime()
        .read_file("alice-mac", path)
        .expect("the download is on the filesystem");
    assert!(!bytes.is_empty());
    assert_eq!(downloads[0]["bytes"].as_u64().unwrap(), bytes.len() as u64);
    // A page that was never loaded cannot be saved.
    let (mut fresh, other) = fresh_world();
    assert!(fresh
        .step(
            &other,
            vec![ActionEnvelope::new(
                "application.v1",
                "shell",
                "alice-mac",
                json!({"target":"shell:download"})
            )]
        )
        .unwrap()
        .outcomes[0]
        .error
        .is_some());
}

#[test]
fn a_screenshot_is_a_real_png_of_the_screen() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    shell(&mut world, &actor, "shell:screenshot");
    let notices = desktop(&world, &actor)["notifications"]
        .as_array()
        .unwrap()
        .clone();
    let saved = notices
        .iter()
        .find(|n| n["app"] == "screenshot")
        .expect("the machine said it saved one");
    let path = saved["body"].as_str().unwrap();
    let bytes = world
        .runtime()
        .read_file("alice-mac", path)
        .expect("the screenshot is on the filesystem");
    // A PNG, not a file named .png.
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    assert!(bytes.len() > 1024, "suspiciously small for a screen");

    // Without the pixel grant there is no screenshot, because there is no way to look.
    let mut blind = World::new(reference_world(), 11).unwrap();
    let id = blind
        .environment(EnvironmentConfig {
            actor: "alice".into(),
            machines: vec!["alice-mac".into()],
            actions: vec!["application.v1".into()],
            observations: vec!["semantic.v1".into()],
            action_budget: 16,
        })
        .unwrap();
    let refused = blind
        .step(
            &id,
            vec![ActionEnvelope::new(
                "application.v1",
                "shell",
                "alice-mac",
                json!({"target":"shell:screenshot"}),
            )],
        )
        .unwrap();
    assert_eq!(refused.outcomes[0].error.as_ref().unwrap().code, "denied");
}

#[test]
fn sharing_hands_the_thing_on_screen_to_an_application_that_can_receive_it() {
    let (mut world, actor) = world();
    browse(&mut world, &actor, "http://intranet.internal/");
    shell(&mut world, &actor, "shell:share:messages");
    let state = desktop(&world, &actor);
    assert!(
        state["windows"]
            .as_object()
            .unwrap()
            .values()
            .any(|w| w["app_id"] == "messages" || w["state"]["app"] == "messages"),
        "sharing opened no messaging application"
    );
    let notice = state["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["app"] == "messages")
        .expect("the share was announced");
    assert_eq!(notice["body"], "http://intranet.internal/");
    // The page is really in hand: it is the Messages draft, waiting to be sent.
    let chat = state["windows"]
        .as_object()
        .unwrap()
        .values()
        .find(|w| w["state"]["app"] == "messages")
        .unwrap();
    assert_eq!(chat["state"]["draft"], "http://intranet.internal/");
    assert_eq!(chat["state"]["base"], "http://messages.internal/");
    // Only applications that can actually receive something are share targets.
    assert!(world
        .step(
            &actor,
            vec![ActionEnvelope::new(
                "application.v1",
                "shell",
                "alice-mac",
                json!({"target":"shell:share:terminal"})
            )]
        )
        .unwrap()
        .outcomes[0]
        .error
        .is_some());
}

#[test]
fn the_trash_is_a_folder_and_opening_it_shows_what_was_deleted() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "terminal.v1",
        "execute",
        json!({"command":"echo gone > /Users/alice/doomed.txt"}),
    );
    // Delete through the file manager, then open the Trash.
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"files","argument":"/Users/alice"}),
    );
    let state = desktop(&world, &actor);
    let window = state["windows"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let tab = &state["windows"][&window]["state"]["tabs"][0];
    // `open:<i>` is a row on screen, and the screen hides dot files.
    let index = tab["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| !e.as_str().unwrap_or_default().starts_with('.'))
        .position(|e| e == "doomed.txt")
        .expect("the file is in the listing");
    shell(
        &mut world,
        &actor,
        &format!("window:{window}:content:open:{index}"),
    );
    shell(
        &mut world,
        &actor,
        &format!("window:{window}:content:files-delete"),
    );
    shell(&mut world, &actor, "shell:trash");
    let trash = world.runtime().read_file(
        "alice-mac",
        "/Users/alice/.local/share/Trash/files/doomed.txt",
    );
    assert!(trash.is_ok(), "the file was not moved to the trash");
    assert!(world
        .runtime()
        .read_file("alice-mac", "/Users/alice/doomed.txt")
        .is_err());
}

#[test]
fn virtual_desktops_hold_separate_windows() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    shell(&mut world, &actor, "shell:workspace:new");
    assert_eq!(desktop(&world, &actor)["workspace"], 1);
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"files"}),
    );
    let state = desktop(&world, &actor);
    let on = |w: &Value, n: u64| w["workspace"].as_u64().unwrap_or(0) == n;
    let windows: Vec<_> = state["windows"]
        .as_object()
        .unwrap()
        .values()
        .cloned()
        .collect();
    assert_eq!(windows.iter().filter(|w| on(w, 0)).count(), 1);
    assert_eq!(windows.iter().filter(|w| on(w, 1)).count(), 1);
    // Going back focuses the window that is actually on screen.
    shell(&mut world, &actor, "shell:workspace:0");
    let state = desktop(&world, &actor);
    let focused = state["focused"].as_u64().unwrap().to_string();
    assert_eq!(state["windows"][&focused]["state"]["type"], "terminal");
    // Closing a desktop moves its windows rather than losing them.
    shell(&mut world, &actor, "shell:workspace:1");
    shell(&mut world, &actor, "shell:workspace:close");
    let state = desktop(&world, &actor);
    assert_eq!(state["windows"].as_object().unwrap().len(), 2);
    assert_eq!(state["workspaces"].as_u64().unwrap_or(1), 1);
}

#[test]
fn switching_desktops_really_changes_what_is_on_screen() {
    // The point of a virtual desktop is that the other one's windows are not there. A
    // switcher that changes only a number is a picture of a feature.
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    let titles = |w: &World| -> Vec<String> {
        w.scene(&actor, 1100, 700)
            .unwrap()
            .windows
            .iter()
            .map(|win| win.app.clone())
            .collect()
    };
    assert_eq!(titles(&world), vec!["terminal"]);
    shell(&mut world, &actor, "shell:workspace:new");
    assert!(
        titles(&world).is_empty(),
        "a new desktop still shows the old desktop's windows"
    );
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"files"}),
    );
    assert_eq!(titles(&world), vec!["files"]);
    shell(&mut world, &actor, "shell:workspace:0");
    assert_eq!(
        titles(&world),
        vec!["terminal"],
        "going back showed the wrong desktop"
    );
    // Moving a window carries it across and follows it.
    shell(&mut world, &actor, "shell:workspace:move:1");
    assert_eq!(desktop(&world, &actor)["workspace"], 1);
    let mut there = titles(&world);
    there.sort();
    assert_eq!(there, vec!["files", "terminal"]);
    shell(&mut world, &actor, "shell:workspace:0");
    assert!(titles(&world).is_empty(), "the window did not leave");
}

#[test]
fn a_flyout_can_sit_over_the_launcher_instead_of_replacing_it() {
    let (mut world, actor) = world();
    act(&mut world, &actor, "application.v1", "launcher", json!({}));
    assert_eq!(desktop(&world, &actor)["launcher_open"], true);
    shell(&mut world, &actor, "shell:panel:power");
    let state = desktop(&world, &actor);
    assert_eq!(state["panel"], "power");
    assert_eq!(
        state["launcher_open"], true,
        "the power flyout closed the launcher instead of covering it"
    );
    // Dismissing the flyout returns to the launcher it covered.
    shell(&mut world, &actor, "shell:dismiss");
    let state = desktop(&world, &actor);
    assert_eq!(state["panel"], Value::Null);
    assert_eq!(state["launcher_open"], true);
    // Any other panel still takes the screen.
    act(&mut world, &actor, "application.v1", "launcher", json!({}));
    shell(&mut world, &actor, "shell:panel:quick");
    assert_eq!(desktop(&world, &actor)["launcher_open"], false);
}

#[test]
fn sharing_from_files_hands_over_the_selected_file() {
    // Finder carries Share in its window toolbar; Explorer in its command bar.
    for theme in ["virtual-macos-golden-gate", "virtual-windows-11"] {
        let mut definition = reference_world();
        definition.metadata["desktop_themes"] = json!({ "alice-mac": theme });
        definition.metadata["desktop_apps"] = json!([
            {"id":"messages","label":"Messages","kind":"native","url":"http://messages.internal/","icon":"messages"},
        ]);
        for c in &mut definition.computers {
            if c.id == "alice-mac" {
                c.installed_apps.push("messages".into());
            }
        }
        let mut world = World::new(definition, 11).unwrap();
        let actor = world
            .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
            .unwrap();
        act(
            &mut world,
            &actor,
            "application.v1",
            "launch",
            json!({"kind":"files","argument":"/home"}),
        );
        let find = |world: &World, target: &str| {
            let scene = world.scene(&actor, 1100, 700).unwrap();
            let suffix = format!(":content:{target}");
            scene
                .nodes
                .iter()
                .find(|n| {
                    n.interaction
                        .as_deref()
                        .is_some_and(|i| i == target || i.ends_with(&suffix))
                })
                .map(|n| n.transform.bounds(n.bounds))
        };
        // Nothing selected: Share is greyed, not a refusal waiting to happen.
        assert!(find(&world, "shell:share:messages").is_none(), "{theme}");
        for target in ["open:0", "shell:share:messages"] {
            let b = find(&world, target).unwrap_or_else(|| panic!("{theme}: missing {target}"));
            act(
                &mut world,
                &actor,
                "pointer.v1",
                "click",
                json!({"x": b.x + b.width as i32 / 2, "y": b.y + b.height as i32 / 2,
                       "width": 1100, "height": 700}),
            );
        }
        let state = desktop(&world, &actor);
        let chat = state["windows"]
            .as_object()
            .unwrap()
            .values()
            .find(|w| w["state"]["app"] == "messages")
            .unwrap_or_else(|| panic!("{theme}: sharing opened no Messages window"));
        assert_eq!(chat["state"]["draft"], "/home/alice", "{theme}");
    }
}

#[test]
fn start_pages_its_pinned_apps_and_stays_open() {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ "alice-mac": "virtual-windows-11" });
    // Every Windows application, more than one page of pins holds.
    for c in &mut definition.computers {
        if c.id == "alice-mac" {
            for kind in [
                "browser",
                "files",
                "mail",
                "calendar",
                "messages",
                "docs",
                "editor",
                "terminal",
                "code",
                "photos",
                "paint",
                "music",
                "maps",
                "weather",
                "notes",
                "contacts",
                "calculator",
                "clock",
                "settings",
            ] {
                if !c.installed_apps.iter().any(|a| a == kind) {
                    c.installed_apps.push(kind.into());
                }
            }
        }
    }
    let mut world = World::new(definition, 11).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    act(&mut world, &actor, "application.v1", "launcher", json!({}));
    let targets = |world: &World| -> Vec<String> {
        world
            .scene(&actor, 1280, 800)
            .unwrap()
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
            .collect()
    };
    let first = targets(&world);
    assert!(
        first.iter().any(|t| t == "shell:home-page:1"),
        "no second page"
    );
    shell(&mut world, &actor, "shell:home-page:1");
    let state = desktop(&world, &actor);
    assert_eq!(state["launcher_open"], true, "paging closed Start");
    let second = targets(&world);
    assert!(second.iter().any(|t| t.starts_with("shell:launch:")));
    assert_ne!(first, second, "the second page shows the same apps");
    assert!(
        first
            .iter()
            .chain(&second)
            .any(|t| t == "shell:launch:settings"),
        "Settings is on no page"
    );
}
