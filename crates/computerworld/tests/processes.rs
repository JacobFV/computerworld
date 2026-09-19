//! The process table as an actor sees it: an open application is something the machine
//! is running, `ps` says so, and a signal really reaches it.
use computerworld::{reference_world, ActionEnvelope, EnvironmentConfig, World};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";

fn desktop() -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ MACHINE: "virtual-ubuntu-24" });
    let mut world = World::new(definition, 3).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: "alice".into(),
            machines: vec![MACHINE.into()],
            actions: vec![
                "terminal.v1".into(),
                "application.v1".into(),
                "browser.v1".into(),
            ],
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
fn shell(world: &mut World, session: &str, command: &str) -> String {
    let value = act(
        world,
        session,
        "terminal.v1",
        "execute",
        json!({ "command": command }),
    );
    assert_eq!(
        value["exit_code"],
        0,
        "`{command}`: {}",
        value["stderr"].as_str().unwrap_or_default()
    );
    value["stdout"].as_str().unwrap_or_default().to_owned()
}
fn open_windows(world: &World, session: &str) -> Vec<String> {
    world
        .scene(session, 1280, 800)
        .unwrap()
        .windows
        .iter()
        .map(|w| w.app.clone())
        .collect()
}

#[test]
fn an_open_application_is_a_process_and_closing_it_ends_that_process() {
    let (mut world, session) = desktop();
    let before = shell(&mut world, &session, "ps -e -o comm").lines().count();
    let window = act(
        &mut world,
        &session,
        "application.v1",
        "launch",
        json!({"kind":"editor","argument":"/Users/alice/notes.txt"}),
    )["window"]
        .as_u64()
        .unwrap();
    let table = shell(&mut world, &session, "ps -e -o pid,rss,args");
    let row = table
        .lines()
        .find(|l| l.contains("editor "))
        .unwrap_or_else(|| panic!("the open editor should be in ps: {table}"));
    assert!(
        row.contains("notes.txt"),
        "a window's process names the document it holds: {row}"
    );
    // The document it has open is part of what it is modelled to hold.
    let rss: u64 = row.split_whitespace().nth(1).unwrap().parse().unwrap();
    assert!(
        rss >= 30 * 1024,
        "an editor holds at least its own 30 MiB: {row}"
    );
    act(
        &mut world,
        &session,
        "application.v1",
        "close",
        json!({ "window": window }),
    );
    let after = shell(&mut world, &session, "ps -e -o comm");
    assert!(
        !after.contains("editor"),
        "closing ends the process: {after}"
    );
    assert_eq!(after.lines().count(), before, "{after}");
}

#[test]
fn killing_an_applications_process_closes_its_window() {
    let (mut world, session) = desktop();
    act(
        &mut world,
        &session,
        "application.v1",
        "launch",
        json!({"kind":"browser"}),
    );
    act(
        &mut world,
        &session,
        "application.v1",
        "launch",
        json!({"kind":"files"}),
    );
    assert_eq!(open_windows(&world, &session).len(), 2);
    // `pgrep` finds it by name, and `pkill` really shuts it.
    let found = shell(&mut world, &session, "pgrep browser");
    assert!(found.trim().parse::<u64>().is_ok(), "{found}");
    shell(&mut world, &session, "pkill browser");
    let open = open_windows(&world, &session);
    assert_eq!(open, vec!["files".to_string()], "{open:?}");
    assert!(!shell(&mut world, &session, "ps -e -o comm").contains("browser"));
    // And `kill` by pid does the same for the one that is left.
    let pid = shell(&mut world, &session, "pgrep files");
    shell(&mut world, &session, &format!("kill {}", pid.trim()));
    assert!(open_windows(&world, &session).is_empty());
}

#[test]
fn what_is_running_and_what_is_left_are_both_answerable_from_the_session() {
    let (mut world, session) = desktop();
    for kind in ["browser", "editor", "files", "terminal"] {
        act(
            &mut world,
            &session,
            "application.v1",
            "launch",
            json!({ "kind": kind }),
        );
    }
    // Top ten by memory, largest first, with the browser at the head of it.
    let biggest = shell(
        &mut world,
        &session,
        "ps -e -o rss,comm --sort=-rss | head -n 11",
    );
    let rows: Vec<(u64, &str)> = biggest
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            Some((parts.next()?.parse().ok()?, parts.next()?))
        })
        .collect();
    assert!(rows.len() >= 4, "{biggest}");
    assert!(rows.windows(2).all(|w| w[0].0 >= w[1].0), "{biggest}");
    assert_eq!(rows[0].1, "browser", "{biggest}");
    // free and top agree with ps about what is resident.
    let free = shell(&mut world, &session, "free -b");
    let used: u64 = free
        .lines()
        .find(|l| l.starts_with("Mem:"))
        .unwrap()
        .split_whitespace()
        .nth(2)
        .unwrap()
        .parse()
        .unwrap();
    let summed: u64 = rows.iter().map(|(kib, _)| kib * 1024).sum();
    assert!(used >= summed, "free counts at least the windows: {free}");
    let top = shell(&mut world, &session, "top -b -n1");
    let first_row = top
        .lines()
        .skip_while(|l| !l.trim_start().starts_with("PID"))
        .nth(1)
        .unwrap_or_default();
    assert!(
        first_row.ends_with("browser"),
        "top sorts by memory like ps does: {top}"
    );
    // How much disk is left.
    let df = shell(&mut world, &session, "df -h /");
    let avail = df
        .lines()
        .nth(1)
        .unwrap()
        .split_whitespace()
        .nth(3)
        .unwrap();
    assert!(avail.ends_with('G') || avail.ends_with('M'), "{df}");
    // And lsof reports the descriptors the table really holds — no more: `init` holds
    // none, so asking about it is status 1 and no rows, exactly as on Linux.
    let lsof = shell(&mut world, &session, "lsof");
    assert!(lsof.starts_with("COMMAND"), "{lsof}");
    assert!(
        lsof.lines().any(|l| l.contains("/dev/pts/0")),
        "the running command's own streams are open: {lsof}"
    );
    let none = act(
        &mut world,
        &session,
        "terminal.v1",
        "execute",
        json!({"command":"lsof -p 1"}),
    );
    assert_eq!(none["exit_code"], 1, "{none}");
}

#[test]
fn a_service_is_a_process_an_actor_can_see_and_stop() {
    let (mut world, session) = desktop();
    let table = shell(&mut world, &session, "ps -e -o user,comm,tty");
    assert!(
        table
            .lines()
            .any(|l| l.contains("init") && l.ends_with('?')),
        "init is on no terminal: {table}"
    );
    // A command typed at the shell is on the machine's one pseudo-terminal.
    assert!(
        shell(&mut world, &session, "ps -o pid,tty,comm").contains("pts/0"),
        "a shell command runs on pts/0"
    );
    // Signals an actor may not send are refused rather than silently ignored.
    let refused = act(
        &mut world,
        &session,
        "terminal.v1",
        "execute",
        json!({"command":"kill 1"}),
    );
    assert_ne!(refused["exit_code"], 0, "init cannot be killed: {refused}");
}
