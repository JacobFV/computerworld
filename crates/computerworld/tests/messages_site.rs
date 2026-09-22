//! messages.internal served as HTML by the messages service and driven through the
//! agent API: the sidebar lists the conversations as links, opening one shows the
//! bubbles and the composer, a text typed into `send-text` is sent with Enter, a
//! tapback is given with its button, and `read-submit` marks the thread read.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";

fn world() -> (World, String) {
    let mut world = World::new(reference_world(), 42).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: ACTOR.into(),
            machines: vec![MACHINE.into()],
            actions: vec!["browser.v1".into(), "keyboard.v1".into()],
            observations: vec!["semantic.v1".into(), "browser.v1".into()],
            action_budget: 1 << 20,
        })
        .unwrap();
    (world, session)
}
fn act(world: &mut World, session: &str, channel: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new(channel, op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn browser(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE].clone()
}
/// Every element of the semantic tree, flattened.
fn elements(page: &Value) -> Vec<Value> {
    fn walk(elements: &[Value], out: &mut Vec<Value>) {
        for e in elements {
            out.push(e.clone());
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    out
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no element {id} in {all:?}"))
}

const AB: &str = "+14155550100|+14155550101";

#[test]
fn a_conversation_is_opened_texted_tapped_back_and_read_through_the_agent_api() {
    let (mut world, session) = world();
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url":"http://messages.internal/"}),
    );
    let inbox = page(&world, &session);
    assert_eq!(inbox["title"], "Messages");
    let all = elements(&inbox);
    let row = by_id(&all, &format!("row-{AB}"));
    assert_eq!(row["kind"], "link");
    assert!(
        row["text"].as_str().unwrap().contains("Bob Martinez"),
        "{row:?}"
    );
    assert_eq!(by_id(&all, "new")["kind"], "form");
    assert_eq!(by_id(&all, "new-to")["kind"], "input");

    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id": format!("row-{AB}")}),
    );
    let thread = page(&world, &session);
    assert_eq!(thread["title"], "Bob Martinez · Messages");
    let all = elements(&thread);
    let field = by_id(&all, "send-text");
    assert_eq!(field["kind"], "input");
    assert_eq!(field["label"], "iMessage");
    assert_eq!(by_id(&all, "send-submit")["kind"], "button");
    assert_eq!(by_id(&all, "back")["kind"], "link");
    assert!(all
        .iter()
        .any(|e| e["text"].as_str().is_some_and(|t| t.contains("lunch?"))));

    // Text with the keyboard and Enter; the newest outgoing bubble reads Delivered.
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id":"send-text"}),
    );
    act(
        &mut world,
        &session,
        "keyboard.v1",
        "type",
        json!({"text":"spicy it is"}),
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "key",
        json!({"key":"Enter"}),
    );
    let all = elements(&page(&world, &session));
    assert!(
        all.iter().any(|e| e["text"]
            .as_str()
            .is_some_and(|t| t.contains("spicy it is"))),
        "{all:?}"
    );
    assert!(
        all.iter()
            .any(|e| e["text"].as_str().is_some_and(|t| t.contains("Delivered"))),
        "{all:?}"
    );

    // A tapback is one button; the bubble then carries its glyph ("HA" for laughed).
    let has_glyph = |all: &[Value]| all.iter().any(|e| e["kind"] == "text" && e["text"] == "HA");
    assert!(!has_glyph(&all));
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id":"sms-7-tapback-laughed"}),
    );
    let all = elements(&page(&world, &session));
    assert!(has_glyph(&all), "{all:?}");

    // Mark as read, then go back to the list.
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id":"read-submit"}),
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id":"back"}),
    );
    assert_eq!(
        browser(&world, &session)["url"],
        "http://messages.internal/"
    );
    let all = elements(&page(&world, &session));
    assert!(
        !all.iter().any(|e| e["id"] == format!("row-{AB}-unread")),
        "the thread is read"
    );

    // A new conversation from the sidebar form lands on its thread.
    act(
        &mut world,
        &session,
        "browser.v1",
        "fill",
        json!({"id":"new-to","value":"carol"}),
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id":"new-submit"}),
    );
    assert_eq!(page(&world, &session)["title"], "Carol Okafor · Messages");
}
