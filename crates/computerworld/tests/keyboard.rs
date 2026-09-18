//! The soft keyboard. A painted key must type what it shows, and the shell's decision to
//! show a keyboard must be the same decision the keystroke router actually makes.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const W: u32 = 390;
const H: u32 = 844;

fn world(theme: &str) -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ "alice-mac": theme });
    // Notes is the machine's one native document application, and a native document is
    // one of the routes the keystroke router treats as text entry.
    definition.metadata["desktop_apps"] =
        json!([{"id":"notes","label":"Notes","kind":"native","url":"","icon":"notes"}]);
    for computer in &mut definition.computers {
        if computer.id == "alice-mac" {
            computer.installed_apps.push("notes".to_owned());
        }
    }
    let mut world = World::new(definition, 5).unwrap();
    let session = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
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
/// Click a control by its interaction id, wherever the shell painted it.
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
fn desktop(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap()
}
fn terminal_input(world: &World, actor: &str) -> String {
    let d = desktop(world, actor);
    let id = d["focused"].as_u64().expect("a focused window").to_string();
    d["windows"][&id]["state"]["input"]
        .as_str()
        .expect("a terminal window")
        .to_owned()
}

#[test]
fn a_painted_key_types_the_character_it_shows() {
    let (mut world, actor) = world("virtual-ios-18");
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    for key in ['h', 'i'] {
        click(&mut world, &actor, &format!("shell:type:{key}"));
    }
    assert_eq!(terminal_input(&world, &actor), "hi");
    click(&mut world, &actor, "shell:key:Backspace");
    assert_eq!(terminal_input(&world, &actor), "h");
}

#[test]
fn shift_is_a_real_modifier_and_a_one_shot_releases_after_one_character() {
    let (mut world, actor) = world("virtual-ios-18");
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    // One tap: the next character only.
    click(&mut world, &actor, "shell:key:Shift");
    click(&mut world, &actor, "shell:type:a");
    click(&mut world, &actor, "shell:type:b");
    assert_eq!(terminal_input(&world, &actor), "Ab");
    // Two taps: locked until released.
    click(&mut world, &actor, "shell:key:Shift");
    click(&mut world, &actor, "shell:key:Shift");
    click(&mut world, &actor, "shell:type:c");
    click(&mut world, &actor, "shell:type:d");
    assert_eq!(terminal_input(&world, &actor), "AbCD");
    // Three taps returns to off.
    click(&mut world, &actor, "shell:key:Shift");
    click(&mut world, &actor, "shell:type:e");
    assert_eq!(terminal_input(&world, &actor), "AbCDe");
}

#[test]
fn the_shell_shows_a_keyboard_exactly_when_a_keystroke_would_insert_text() {
    // The shells decide before the scene exists; the router decides after. If those two
    // ever disagree, a phone paints a keyboard that types nowhere, or hides one that would.
    let (mut world, actor) = world("virtual-ios-18");
    let published = |w: &World| -> bool {
        w.scene(&actor, W, H)
            .unwrap()
            .focus
            .map(|f| f.keyboard.text_entry)
            .unwrap_or(false)
    };
    let painted = |w: &World| -> bool {
        w.scene(&actor, W, H).unwrap().nodes.iter().any(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.starts_with("shell:type:"))
        })
    };
    // A bare home screen: no text entry, no keyboard.
    assert!(!published(&world));
    assert!(!painted(&world));
    // A terminal takes text.
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    assert!(published(&world), "a terminal should take text");
    assert_eq!(painted(&world), published(&world));
    // A file manager does not.
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"files"}),
    );
    assert!(!published(&world), "a file manager takes no text");
    assert_eq!(painted(&world), published(&world));
    // ...until its own folder search has focus, at which point a keystroke really does
    // reach the query, so the keyboard must appear.
    click(&mut world, &actor, "files-search");
    assert!(published(&world), "a searching file manager takes text");
    assert_eq!(painted(&world), published(&world));
    // A native application with a document open takes text.
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"notes","argument":"/Users/alice/Notes"}),
    );
    click(&mut world, &actor, "notes:new");
    assert!(published(&world), "an open note should take text");
    assert_eq!(painted(&world), published(&world));
    // A form field inside a page takes text; the page around it does not.
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"browser"}),
    );
    act(
        &mut world,
        &actor,
        "browser.v1",
        "navigate",
        json!({"url":"http://guide.example/"}),
    );
    // The form is below the fold on a phone, so scroll it into view before tapping it.
    act(
        &mut world,
        &actor,
        "browser.v1",
        "scroll",
        json!({"y": 600}),
    );
    assert!(
        !published(&world),
        "a page with nothing focused takes no text"
    );
    assert_eq!(painted(&world), published(&world));
    click(&mut world, &actor, "question-question");
    assert!(published(&world), "a focused form field should take text");
    assert_eq!(painted(&world), published(&world));
    // What the keys type really lands in that field.
    click(&mut world, &actor, "shell:key:Shift");
    for key in ['h', 'i'] {
        click(&mut world, &actor, &format!("shell:type:{key}"));
    }
    let session = world.interfaces().session(&actor).unwrap();
    assert_eq!(
        session.machines["alice-mac"].browser.tab().fields["question-question"],
        "Hi"
    );
}

/// `text_entry_of` answers before the scene exists and `focus_of` answers after it. They
/// have drifted apart twice; this walks a set of real states and pins them together.
#[test]
fn the_two_answers_about_text_entry_never_disagree() {
    let (mut world, actor) = world("virtual-ios-18");
    let agree = |w: &World, what: &str| {
        let scene = w.scene(&actor, W, H).unwrap();
        let published = scene.focus.map(|f| f.keyboard.text_entry).unwrap_or(false);
        let painted = scene.nodes.iter().any(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.starts_with("shell:type:"))
        });
        assert_eq!(painted, published, "{what}: painted != published");
    };
    agree(&world, "home screen");
    for kind in ["terminal", "files", "editor", "browser"] {
        act(
            &mut world,
            &actor,
            "application.v1",
            "launch",
            json!({ "kind": kind }),
        );
        agree(&world, kind);
    }
    // The file manager's two text fields, and leaving them again.
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"files"}),
    );
    click(&mut world, &actor, "files-search");
    agree(&world, "files searching");
    click(&mut world, &actor, "files-search-clear");
    agree(&world, "files after clearing the search");
    // The launcher's own query.
    act(&mut world, &actor, "application.v1", "launcher", json!({}));
    agree(&world, "launcher");
}

#[test]
fn a_suggestion_chip_completes_the_word_being_typed() {
    let (mut world, actor) = world("virtual-android-12");
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"terminal"}),
    );
    let offered = |world: &World| -> Vec<String> {
        world
            .scene(&actor, W, H)
            .unwrap()
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
            .filter(|i| i.starts_with("shell:insert:"))
            .collect()
    };
    // Nothing is offered before a letter is typed.
    assert!(offered(&world).is_empty());
    for key in ['m', 'e', 'e'] {
        click(&mut world, &actor, &format!("shell:type:{key}"));
    }
    assert!(
        offered(&world).contains(&"shell:insert:ting ".to_owned()),
        "{:?}",
        offered(&world)
    );
    click(&mut world, &actor, "shell:insert:ting ");
    assert_eq!(terminal_input(&world, &actor), "meeting ");
    // A finished word offers nothing more.
    assert!(offered(&world).is_empty());
}

#[test]
fn a_phone_system_surface_puts_the_keyboard_away_and_takes_no_keystrokes() {
    for theme in ["virtual-ios-18", "virtual-android-12"] {
        let (mut world, actor) = world(theme);
        act(
            &mut world,
            &actor,
            "application.v1",
            "launch",
            json!({"kind":"terminal"}),
        );
        let painted = |w: &World| {
            w.scene(&actor, W, H).unwrap().nodes.iter().any(|n| {
                n.interaction
                    .as_deref()
                    .is_some_and(|i| i.starts_with("shell:type:"))
            })
        };
        let published = |w: &World| {
            w.scene(&actor, W, H)
                .unwrap()
                .focus
                .is_some_and(|f| f.keyboard.text_entry)
        };
        assert!(painted(&world) && published(&world), "{theme}");
        for panel in ["quick", "notifications", "overview", "settings"] {
            act(
                &mut world,
                &actor,
                "application.v1",
                "shell",
                json!({ "target": format!("shell:panel:{panel}") }),
            );
            assert!(
                !painted(&world),
                "{theme} {panel}: keyboard over a system surface"
            );
            assert_eq!(painted(&world), published(&world), "{theme} {panel}");
            // Typing reaches nothing behind it, exactly as the missing keyboard says.
            act(
                &mut world,
                &actor,
                "keyboard.v1",
                "type",
                json!({"text":"x"}),
            );
            assert_eq!(terminal_input(&world, &actor), "", "{theme} {panel}");
            act(
                &mut world,
                &actor,
                "application.v1",
                "shell",
                json!({"target":"shell:dismiss"}),
            );
        }
        act(
            &mut world,
            &actor,
            "keyboard.v1",
            "type",
            json!({"text":"ok"}),
        );
        assert_eq!(terminal_input(&world, &actor), "ok", "{theme}");
    }
}
