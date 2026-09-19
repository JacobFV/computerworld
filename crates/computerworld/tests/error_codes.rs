//! Every refusal an actor can see: the four codes, the documented reasons, and the
//! promise that a refusal never carries world state. The table in `docs/agent-api.md`
//! is checked against `cw_protocol::reason::ALL`, so neither can drift from the other.
use computerworld::{reference_world, ActionEnvelope, EnvironmentConfig, SimError, World};
use serde_json::{json, Value};

fn session(world: &mut World, actions: &[&str], machines: &[&str]) -> String {
    world
        .environment(EnvironmentConfig {
            actor: "alice".into(),
            machines: machines.iter().map(|m| (*m).to_string()).collect(),
            actions: actions.iter().map(|a| (*a).to_string()).collect(),
            observations: vec!["semantic.v1".into()],
            action_budget: 8,
        })
        .unwrap()
}
fn refusal(
    world: &mut World,
    id: &str,
    machine: &str,
    family: &str,
    op: &str,
    payload: Value,
) -> SimError {
    let result = world
        .step(id, vec![ActionEnvelope::new(family, op, machine, payload)])
        .unwrap();
    let outcome = &result.outcomes[0];
    assert!(!outcome.success, "{family} {op} should be refused");
    outcome.error.clone().expect("a refusal carries an error")
}

#[test]
fn every_documented_reason_is_one_the_environment_can_return() {
    // The vocabulary is closed: each entry has a code from the four and a fixed message.
    for (name, code, message) in cw_protocol::reason::ALL {
        assert!(
            ["denied", "not_found", "invalid", "action_failed"].contains(code),
            "{name} travels under an undocumented code {code}"
        );
        assert!(!message.is_empty(), "{name} carries no message");
        assert_eq!(
            cw_protocol::reason::resolve(name),
            Some((*code, *message)),
            "{name} must resolve to its own row"
        );
    }
    assert_eq!(cw_protocol::reason::resolve("invented"), None);
}

#[test]
fn the_documented_error_table_lists_exactly_the_reasons_that_exist() {
    let doc = include_str!("../../../docs/agent-api.md");
    for (name, code, message) in cw_protocol::reason::ALL {
        assert!(
            doc.contains(&format!("`{name}`")),
            "docs/agent-api.md does not document the reason `{name}`"
        );
        assert!(
            doc.contains(&format!("`{code}`")),
            "docs/agent-api.md does not document the code `{code}`"
        );
        assert!(
            doc.contains(message),
            "docs/agent-api.md does not carry the fixed message for `{name}`: {message}"
        );
    }
    // And nothing extra: every backticked reason-shaped slug in the table is real.
    let table = doc
        .split("<!-- error-codes -->")
        .nth(1)
        .expect("docs/agent-api.md must mark its error table with <!-- error-codes -->");
    let mut documented = 0;
    for line in table.lines().take_while(|l| !l.starts_with("<!--")) {
        // Only the first column of a table row names a reason.
        let Some(first) = line.split('|').nth(1).map(str::trim) else {
            continue;
        };
        let Some(slug) = first.strip_prefix('`').and_then(|c| c.strip_suffix('`')) else {
            continue;
        };
        // The header row names the column, not a reason.
        if slug == "reason" {
            continue;
        }
        assert!(
            cw_protocol::reason::resolve(slug).is_some(),
            "docs/agent-api.md documents `{slug}`, which no longer exists"
        );
        documented += 1;
    }
    assert_eq!(
        documented,
        cw_protocol::reason::ALL.len(),
        "the table and the vocabulary must have the same number of rows"
    );
}

#[test]
fn a_refused_action_says_which_grant_is_missing() {
    let mut world = World::new(reference_world(), 1).unwrap();
    // A family the session does not hold.
    let terminal_only = session(&mut world, &["terminal.v1"], &["alice-mac"]);
    let e = refusal(
        &mut world,
        &terminal_only,
        "alice-mac",
        "application.v1",
        "launch",
        json!({"kind":"editor"}),
    );
    assert_eq!(e.code, "denied");
    assert_eq!(
        e.reason.as_deref(),
        Some(cw_protocol::reason::FAMILY_NOT_GRANTED),
        "{e:?}"
    );
    // A machine the session does not hold, with a family it does.
    let e = refusal(
        &mut world,
        &terminal_only,
        "bob-windows",
        "terminal.v1",
        "execute",
        json!({"command":"pwd"}),
    );
    assert_eq!(e.code, "denied");
    assert_eq!(
        e.reason.as_deref(),
        Some(cw_protocol::reason::MACHINE_NOT_GRANTED),
        "{e:?}"
    );
    // The cross-family gate the report tripped over: the browser needs browser.v1 too.
    let no_browser = session(
        &mut world,
        &["terminal.v1", "application.v1"],
        &["alice-mac"],
    );
    let e = refusal(
        &mut world,
        &no_browser,
        "alice-mac",
        "application.v1",
        "launch",
        json!({"kind":"browser"}),
    );
    assert_eq!(e.code, "denied");
    assert_eq!(
        e.reason.as_deref(),
        Some(cw_protocol::reason::BROWSER_FAMILY_REQUIRED),
        "an installed browser refused for want of browser.v1 must say so: {e:?}"
    );
    assert!(
        e.message.contains("browser.v1"),
        "the message must name the grant to add: {e:?}"
    );
    // An application the machine does not have.
    let e = refusal(
        &mut world,
        &no_browser,
        "alice-mac",
        "application.v1",
        "launch",
        json!({"kind":"kdenlive"}),
    );
    assert_eq!(e.code, "not_found");
    assert_eq!(
        e.reason.as_deref(),
        Some(cw_protocol::reason::APPLICATION_NOT_INSTALLED),
        "{e:?}"
    );
    // An operation the family does not implement.
    let e = refusal(
        &mut world,
        &no_browser,
        "alice-mac",
        "application.v1",
        "levitate",
        json!({}),
    );
    assert_eq!(e.code, "invalid");
    assert_eq!(
        e.reason.as_deref(),
        Some(cw_protocol::reason::UNSUPPORTED_OPERATION),
        "{e:?}"
    );
}

#[test]
fn a_reasoned_refusal_still_carries_no_world_state() {
    let mut world = World::new(reference_world(), 1).unwrap();
    let id = session(
        &mut world,
        &["terminal.v1", "application.v1"],
        &["alice-mac"],
    );
    // Every message an actor can see is one of the published constants, so no refusal
    // can leak a path, a machine id or an evaluator secret through its text.
    let published: Vec<&str> = cw_protocol::reason::ALL
        .iter()
        .map(|(_, _, message)| *message)
        .chain([
            "action is not permitted",
            "requested resource not found",
            "invalid action",
            "action failed",
        ])
        .collect();
    for (family, op, payload) in [
        ("application.v1", "launch", json!({"kind":"browser"})),
        ("application.v1", "launch", json!({"kind":"kdenlive"})),
        ("application.v1", "focus", json!({"window": 9999})),
        (
            "application.v1",
            "shell",
            json!({"target":"shell:nonsense"}),
        ),
        ("terminal.v1", "explode", json!({})),
    ] {
        let e = refusal(&mut world, &id, "alice-mac", family, op, payload);
        assert!(
            published.contains(&e.message.as_str()),
            "{family} {op} returned an unpublished message: {e:?}"
        );
    }
}

#[test]
fn the_budget_and_an_unknown_session_refuse_with_their_own_reasons() {
    let mut world = World::new(reference_world(), 1).unwrap();
    let id = session(&mut world, &["terminal.v1"], &["alice-mac"]);
    let oversized: Vec<ActionEnvelope> = (0..9)
        .map(|_| {
            ActionEnvelope::new(
                "terminal.v1",
                "execute",
                "alice-mac",
                json!({"command":"pwd"}),
            )
        })
        .collect();
    let e = world.step(&id, oversized).unwrap_err();
    assert_eq!(e.code, "denied");
    assert_eq!(
        e.reason.as_deref(),
        Some(cw_protocol::reason::BUDGET_EXCEEDED),
        "{e:?}"
    );
    let e = world.step("session-nope", vec![]).unwrap_err();
    assert_eq!(e.code, "denied");
    assert_eq!(
        e.reason.as_deref(),
        Some(cw_protocol::reason::UNKNOWN_SESSION),
        "{e:?}"
    );
}
