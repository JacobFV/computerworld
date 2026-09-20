//! slack.com served as HTML by the Slack service and driven end to end through the
//! agent API: the browser renders the workspace with the web engine, the semantic
//! observation lists the sidebar rows, the composer and the message controls by the ids
//! the service documents, and sending, threading, reacting, marking read and opening a
//! DM are form posts that land back on the conversation.
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
        .step(session, vec![ActionEnvelope::new(channel, op, MACHINE, payload)])
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
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn has_text(all: &[Value], needle: &str) -> bool {
    all.iter()
        .any(|e| e["text"].as_str().is_some_and(|t| t.contains(needle)))
}

#[test]
fn slack_is_read_and_written_through_the_agent_api() {
    let (mut world, session) = world();
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://slack.com/"}));
    let home = page(&world, &session);
    // The root lands on the first channel the actor is in.
    assert_eq!(home["title"], "#atlas-release (Channel) - Northstar - Slack");
    let all = elements(&home);
    // The sidebar rows are links, the composer a form with a labelled field.
    let random = by_id(&all, "nav-random");
    assert_eq!(random["kind"], "link");
    assert_eq!(random["url"], "http://slack.com/channels/random");
    assert_eq!(by_id(&all, "dm-alice|bob")["url"], "http://slack.com/channels/alice|bob");
    assert_eq!(by_id(&all, "rail-dms")["kind"], "link");
    assert_eq!(by_id(&all, "send")["kind"], "form");
    assert_eq!(by_id(&all, "send-text")["kind"], "input");
    assert_eq!(by_id(&all, "send-text")["label"], "Message # atlas-release");
    assert_eq!(by_id(&all, "send-submit")["kind"], "button");
    assert_eq!(by_id(&all, "start-admin")["kind"], "button");

    // Open #eng from the sidebar and read it.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"nav-eng"}));
    assert_eq!(browser(&world, &session)["url"], "http://slack.com/channels/eng");
    let eng = page(&world, &session);
    assert_eq!(eng["title"], "#eng (Channel) - Northstar - Slack");
    let all = elements(&eng);
    assert!(has_text(&all, "Windows CI went red again."));
    assert!(has_text(&all, "17 new messages"));
    assert_eq!(by_id(&all, "chat-1-replies")["url"], "http://slack.com/channels/eng?thread=chat-1");
    assert_eq!(by_id(&all, "channel-members")["url"], "http://slack.com/channels/eng?members=1");

    // Mark the channel read: a one-button form.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"read-submit"}));
    let all = elements(&page(&world, &session));
    assert!(!all.iter().any(|e| e["id"] == "read-submit"));

    // Type a message into the composer and press Enter: the form posts and the page
    // that comes back holds the message.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"send-text"}));
    act(&mut world, &session, "keyboard.v1", "type", json!({"text":"Green three times in a row."}));
    act(&mut world, &session, "browser.v1", "key", json!({"key":"Enter"}));
    let all = elements(&page(&world, &session));
    assert!(has_text(&all, "Green three times in a row."));

    // Fill and click the send button: the same form.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://slack.com/channels/eng"}));
    act(&mut world, &session, "browser.v1", "fill", json!({"id":"send-text","value":"Merging after lunch."}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"send-submit"}));
    assert!(has_text(&elements(&page(&world, &session)), "Merging after lunch."));

    // Open a thread from its reply count, answer in the pane, and land back in it.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://slack.com/channels/eng"}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"chat-1-replies"}));
    assert_eq!(browser(&world, &session)["url"], "http://slack.com/channels/eng?thread=chat-1");
    let all = elements(&page(&world, &session));
    assert_eq!(by_id(&all, "chat-1-reply-body")["label"], "Reply in thread");
    assert_eq!(by_id(&all, "thread-close")["url"], "http://slack.com/channels/eng");
    act(&mut world, &session, "browser.v1", "fill", json!({"id":"chat-1-reply-body","value":"Closing this out."}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"chat-1-reply-submit"}));
    let all = elements(&page(&world, &session));
    assert!(has_text(&all, "Closing this out."));
    assert!(all.iter().any(|e| e["id"] == "thread-close"), "the reply lands in its thread");
    assert!(has_text(&all, "4 replies"));

    // A reaction chip reacts: carol and bob had eyes on chat-2, alice makes three.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://slack.com/channels/eng"}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"chat-2-react-eyes"}));
    let all = elements(&page(&world, &session));
    let chip = by_id(&all, "chat-2-react-eyes");
    assert_eq!(chip["kind"], "button");
    assert!(chip["text"].as_str().unwrap().ends_with('3'), "{chip:?}");

    // Someone with no DM yet is one button away from one.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"start-admin"}));
    let dm = page(&world, &session);
    assert_eq!(dm["title"], "admin (DM) - Northstar - Slack");
    let all = elements(&dm);
    assert_eq!(by_id(&all, "dm-admin|alice")["kind"], "link");
    assert_eq!(by_id(&all, "send-text")["label"], "Message admin");
}
