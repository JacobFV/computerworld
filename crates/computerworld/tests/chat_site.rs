//! chat.internal served as HTML by the chat service and driven through the agent API:
//! the sidebar lists the channels by their ids, opening one shows the composer, a
//! message typed into `send-text` is posted by the form and lands in the transcript,
//! and a reaction is added through the message's own form.
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

#[test]
fn a_channel_is_opened_a_message_is_sent_and_reacted_to_through_the_agent_api() {
    let (mut world, session) = world();
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url":"http://chat.internal/"}),
    );
    let home = page(&world, &session);
    assert_eq!(home["title"], "Chat");
    let all = elements(&home);
    let general = by_id(&all, "general");
    assert_eq!(general["kind"], "link");
    assert_eq!(general["url"], "http://chat.internal/channels/general");
    assert!(all.iter().any(|e| e["id"] == "title"));
    assert_eq!(by_id(&all, "dm")["kind"], "form");

    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id":"general"}),
    );
    assert_eq!(
        browser(&world, &session)["url"],
        "http://chat.internal/channels/general"
    );
    let all = elements(&page(&world, &session));
    assert_eq!(by_id(&all, "send")["kind"], "form");
    let field = by_id(&all, "send-text");
    assert_eq!(field["kind"], "input");
    assert_eq!(field["label"], "Message");
    assert_eq!(by_id(&all, "send-submit")["kind"], "button");

    // Type into the composer and press Enter: the form posts and the page is the channel.
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
        json!({"text":"standup moved to 10"}),
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "key",
        json!({"key":"Enter"}),
    );
    let sent = page(&world, &session);
    let all = elements(&sent);
    assert!(
        all.iter().any(|e| e["text"]
            .as_str()
            .is_some_and(|t| t.contains("standup moved to 10"))),
        "{all:?}"
    );
    assert_eq!(by_id(&all, "chat-1-react")["kind"], "form");

    // React through the message's form, with fill and a click on its button.
    act(
        &mut world,
        &session,
        "browser.v1",
        "fill",
        json!({"id":"chat-1-react-reaction","value":"tada"}),
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id":"chat-1-react-submit"}),
    );
    let all = elements(&page(&world, &session));
    assert!(
        all.iter()
            .any(|e| e["text"].as_str().is_some_and(|t| t.contains("tada"))),
        "{all:?}"
    );

    // Open a DM from the sidebar; it lands on the conversation and is listed by its key.
    act(
        &mut world,
        &session,
        "browser.v1",
        "fill",
        json!({"id":"dm-to","value":"bob"}),
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id":"dm-submit"}),
    );
    let all = elements(&page(&world, &session));
    assert_eq!(
        by_id(&all, "dm-alice|bob")["url"],
        "http://chat.internal/channels/alice|bob"
    );
    assert_eq!(by_id(&all, "send")["kind"], "form");
}
