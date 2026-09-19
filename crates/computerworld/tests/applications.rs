//! Applications from the actor's side: enumerating them, launching them, opening a
//! file in the right one from the shell, and the promise that the name on a launcher
//! is the name on the window.
use computerworld::{reference_world, ActionEnvelope, EnvironmentConfig, World};
use cw_applications::desktop_scene::DesktopTheme;
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const FULL: &[&str] = &[
    "terminal.v1",
    "filesystem.v1",
    "application.v1",
    "browser.v1",
    "keyboard.v1",
    "pointer.v1",
];

/// A world whose one desktop wears `theme`, and an actor session on it.
fn themed(theme: &str, actions: &[&str]) -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ MACHINE: theme });
    let mut world = World::new(definition, 5).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: "alice".into(),
            machines: vec![MACHINE.into()],
            actions: actions.iter().map(|a| (*a).to_string()).collect(),
            observations: vec!["semantic.v1".into(), "terminal.v1".into()],
            action_budget: 64,
        })
        .unwrap();
    (world, session)
}
fn act(world: &mut World, session: &str, family: &str, op: &str, payload: Value) -> Value {
    let mut actor = world.actor(session).unwrap();
    let result = actor
        .step(vec![ActionEnvelope::new(family, op, MACHINE, payload)])
        .unwrap();
    let outcome = &result.outcomes[0];
    assert!(
        outcome.success,
        "{family} {op} should succeed: {:?}",
        outcome.error
    );
    outcome.value.clone()
}
fn listed(world: &mut World, session: &str, include_missing: bool) -> Vec<Value> {
    act(
        world,
        session,
        "application.v1",
        "list",
        json!({ "installed": !include_missing }),
    )
    .as_array()
    .expect("list returns an array")
    .clone()
}

#[test]
fn an_actor_can_enumerate_what_is_installed_and_what_it_may_open() {
    let (mut world, session) = themed("virtual-ubuntu-24", FULL);
    let apps = listed(&mut world, &session, false);
    assert!(!apps.is_empty(), "the reference desktop has applications");
    for app in &apps {
        for field in ["id", "label", "kind", "installed", "launchable"] {
            assert!(app.get(field).is_some(), "{field} missing from {app}");
        }
        assert_eq!(app["installed"], json!(true), "{app}");
        assert!(
            !app["label"].as_str().unwrap().is_empty(),
            "every application has a name: {app}"
        );
        assert!(
            ["builtin", "native", "web"].contains(&app["kind"].as_str().unwrap()),
            "{app}"
        );
    }
    // The wider listing says what is *not* here, and why, instead of staying silent.
    let all = listed(&mut world, &session, true);
    assert!(all.len() > apps.len(), "the full listing includes the rest");
    let missing = all
        .iter()
        .find(|a| a["installed"] == json!(false))
        .expect("something is not installed on this machine");
    assert_eq!(missing["launchable"], json!(false), "{missing}");
    assert_eq!(
        missing["blocked_by"],
        json!(cw_protocol::reason::APPLICATION_NOT_INSTALLED),
        "{missing}"
    );
    // Every id the catalogue offers really launches.
    for app in &apps {
        let id = app["id"].as_str().unwrap().to_owned();
        act(
            &mut world,
            &session,
            "application.v1",
            "launch",
            json!({ "kind": id }),
        );
    }
}

#[test]
fn an_installed_browser_is_launchable_and_says_what_is_missing_when_it_is_not() {
    // With the browser grant the installed browser opens, as the world says it should.
    let (mut world, session) = themed("virtual-ubuntu-24", FULL);
    let window = act(
        &mut world,
        &session,
        "application.v1",
        "launch",
        json!({"kind":"browser"}),
    );
    assert!(window["window"].is_u64(), "{window}");
    // Without it, the catalogue marks the browser unlaunchable and names the grant,
    // rather than leaving an agent to guess at a bare denial.
    let (mut world, session) = themed(
        "virtual-ubuntu-24",
        &["terminal.v1", "application.v1", "keyboard.v1", "pointer.v1"],
    );
    let all = listed(&mut world, &session, true);
    let browser = all
        .iter()
        .find(|a| a["id"] == json!("browser"))
        .expect("the browser is still listed");
    assert_eq!(browser["installed"], json!(true), "{browser}");
    assert_eq!(browser["launchable"], json!(false), "{browser}");
    assert_eq!(
        browser["blocked_by"],
        json!(cw_protocol::reason::BROWSER_FAMILY_REQUIRED),
        "{browser}"
    );
}

#[test]
fn xdg_open_opens_a_file_in_the_application_that_handles_it() {
    let (mut world, session) = themed("virtual-ubuntu-24", FULL);
    let run = |world: &mut World, session: &str, command: &str| -> Value {
        act(
            world,
            session,
            "terminal.v1",
            "execute",
            json!({ "command": command }),
        )
    };
    run(&mut world, &session, "printf 'hello\\n' > ~/report.txt");
    let opened = run(&mut world, &session, "xdg-open ~/report.txt");
    assert_eq!(opened["exit_code"], json!(0), "{opened}");
    let windows = world.scene(&session, 1280, 800).unwrap().windows;
    let editor = windows
        .iter()
        .find(|w| w.app == "editor")
        .unwrap_or_else(|| panic!("xdg-open should open an editor: {windows:?}"));
    assert!(editor.document.ends_with("report.txt"), "{editor:?}");
    // A folder goes to the file manager and a URL to the browser, by the same rule the
    // file manager uses when a document is double-clicked.
    run(&mut world, &session, "mkdir -p ~/Pictures");
    run(&mut world, &session, "xdg-open ~/Pictures");
    run(&mut world, &session, "xdg-open http://intranet.internal/");
    let windows = world.scene(&session, 1280, 800).unwrap().windows;
    assert!(windows.iter().any(|w| w.app == "files"), "{windows:?}");
    assert!(windows.iter().any(|w| w.app == "browser"), "{windows:?}");
    // A request from a nested shell reaches the desktop too, not only a bare command.
    run(&mut world, &session, "printf 'x\\n' > ~/nested.txt");
    run(&mut world, &session, "sh -c 'xdg-open ~/nested.txt'");
    let windows = world.scene(&session, 1280, 800).unwrap().windows;
    assert!(
        windows.iter().any(|w| w.document.ends_with("nested.txt")),
        "sh -c 'xdg-open' must open a window too: {windows:?}"
    );
    // A target that is not there fails, and says so, instead of opening nothing.
    let missing = run(&mut world, &session, "xdg-open ~/nowhere.txt");
    assert_eq!(missing["exit_code"], json!(2), "{missing}");
    assert!(
        missing["stderr"]
            .as_str()
            .unwrap()
            .contains("no such file or directory"),
        "{missing}"
    );
}

#[test]
fn xdg_open_reports_the_desktop_refusal_instead_of_silently_doing_nothing() {
    // A session that may run commands but not drive applications: `xdg-open` must not
    // pretend to have opened a window.
    let (mut world, session) = themed("virtual-ubuntu-24", &["terminal.v1"]);
    let result = act(
        &mut world,
        &session,
        "terminal.v1",
        "execute",
        json!({"command":"xdg-open /etc"}),
    );
    assert_eq!(result["exit_code"], json!(3), "{result}");
    assert!(
        result["stderr"].as_str().unwrap().contains("xdg-open"),
        "{result}"
    );
}

/// What each file manager calls the place it opens on when that path has no name of its
/// own: the user's home, a phone's storage root, a Mac volume.
const PLACE_NAMES: &[&str] = &[
    "Home",
    "Browse",
    "Internal storage",
    "This PC",
    "Computer",
    "Macintosh HD",
];
/// What each platform calls a document or page that has not been saved or loaded yet.
const BLANK_DOCUMENTS: &[&str] = &["Untitled", "Untitled Document", "New Tab", "New tab"];

/// Every theme, every installed application: the name the catalogue gives an agent is
/// the name that theme's launcher paints, and the window that opens is never titled
/// with a bare application id.
#[test]
fn launcher_labels_and_window_titles_agree_on_every_shell() {
    for (profile, theme) in [
        ("virtual-ubuntu-24", DesktopTheme::Ubuntu),
        ("virtual-windows-11", DesktopTheme::Windows),
        ("virtual-macos-golden-gate", DesktopTheme::Macos),
        ("virtual-ios-18", DesktopTheme::Ios),
        ("virtual-android-12", DesktopTheme::Android),
    ] {
        let (mut world, session) = themed(profile, FULL);
        for app in listed(&mut world, &session, false) {
            let id = app["id"].as_str().unwrap().to_owned();
            let label = app["label"].as_str().unwrap().to_owned();
            // One table: what an agent is told is what the shell paints.
            if let Some(painted) = theme.app_label(&id) {
                assert_eq!(
                    label, painted,
                    "{profile}: `{id}` is `{label}` in the catalogue but `{painted}` on screen"
                );
            }
            assert_ne!(label, id, "{profile}: `{id}` has no name of its own");
            act(
                &mut world,
                &session,
                "application.v1",
                "launch",
                json!({ "kind": id }),
            );
            let windows = world.scene(&session, 1280, 800).unwrap().windows;
            let window = windows
                .iter()
                .find(|w| w.app == id)
                .unwrap_or_else(|| panic!("{profile}: `{id}` opened no window of its own"));
            assert!(
                !window.title.is_empty(),
                "{profile}: `{id}` opened an untitled window"
            );
            assert_ne!(
                window.title, id,
                "{profile}: `{id}` opened a window titled with its own id"
            );
            // A window is titled with its application's name, or with what it is
            // showing. Never with something else's name, and never with an id — which
            // is what the report found was not true of the text editor.
            let document = window
                .document
                .rsplit('/')
                .find(|part| !part.is_empty())
                .unwrap_or_default();
            let names_what_it_shows = (!document.is_empty() && window.title.contains(document))
                || PLACE_NAMES.contains(&window.title.as_str())
                || BLANK_DOCUMENTS.contains(&window.title.as_str())
                || (id == "terminal" && window.title.contains("alice"));
            // A title may carry the application's own version after its name.
            let names_itself =
                window.title == label || window.title.starts_with(&format!("{label} "));
            assert!(
                names_itself || names_what_it_shows,
                "{profile}: the `{label}` launcher opened a window titled `{}`",
                window.title
            );
            act(
                &mut world,
                &session,
                "application.v1",
                "close",
                json!({ "window": window.id }),
            );
        }
    }
}
