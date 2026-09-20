//! chatgpt.com and claude.ai served as HTML by the assistant service and driven end to
//! end through the agent API: the browser renders each through the web engine, the
//! semantic observation lists the composer, the suggestions and the history by the ids
//! the service documents, typing a prompt and pressing Enter starts a conversation whose
//! reply cites a page in the world, and the conversation's own forms (another turn,
//! regenerate, rename, delete) work from the page.
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
        .unwrap_or_else(|| panic!("no element {id} in {all:?}"))
}
fn says(all: &[Value], needle: &str) -> bool {
    all.iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains(needle)))
}

/// The flow both products share, against one origin.
fn drive(origin: &str, brand: &str, seeded: &str) {
    let (mut world, session) = world();
    act(&mut world, &session, "browser.v1", "navigate", json!({"url": format!("{origin}/")}));
    let home = page(&world, &session);
    assert_eq!(home["title"], brand);
    let all = elements(&home);
    assert_eq!(by_id(&all, "composer")["kind"], "form");
    let message = by_id(&all, "composer-message");
    assert_eq!(message["kind"], "input");
    assert_eq!(message["label"], "Message");
    assert_eq!(by_id(&all, "composer-submit")["kind"], "button");
    assert_eq!(by_id(&all, "suggestion-0")["kind"], "button");
    assert_eq!(by_id(&all, "side-new")["url"], format!("{origin}/"));
    let history = by_id(&all, &format!("side-{seeded}"));
    assert_eq!(history["kind"], "link");
    assert_eq!(history["url"], format!("{origin}/c/{seeded}"));

    // Type a prompt into the composer and press Enter: a conversation starts and the
    // reply is a fact of this world with a citation to open.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"composer-message"}));
    act(&mut world, &session, "keyboard.v1", "type", json!({"text":"What is the Atlas release code?"}));
    act(&mut world, &session, "browser.v1", "key", json!({"key":"Enter"}));
    let chat = page(&world, &session);
    assert_eq!(chat["title"], format!("What is the Atlas release code? - {brand}"));
    let all = elements(&chat);
    assert!(says(&all, "What is the Atlas release code?"), "{all:?}");
    assert!(says(&all, "ATLAS-2026"), "{all:?}");
    let cite = by_id(&all, "msg-1-cite-0");
    assert_eq!(cite["kind"], "link");
    let source = cite["url"].as_str().unwrap().to_owned();
    assert!(source.starts_with("http://docs.google.com/"), "{source}");
    // The new conversation is in the history, and that is its address.
    let mine = all
        .iter()
        .filter(|e| e["kind"] == "link" && e["id"].as_str().is_some_and(|i| i.starts_with("side-conv-")))
        .map(|e| e["url"].as_str().unwrap().to_owned())
        .next_back()
        .unwrap();

    // A second turn through the composer's button, then regenerate and rename.
    act(&mut world, &session, "browser.v1", "fill", json!({"id":"composer-message","value":"Who owns the Atlas launch?"}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"composer-submit"}));
    let all = elements(&page(&world, &session));
    assert!(says(&all, "Carol"), "{all:?}");
    assert!(all.iter().any(|e| e["id"] == "msg-3-cite-0"));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"regenerate"}));
    let all = elements(&page(&world, &session));
    assert!(says(&all, "Carol"), "a fact has one answer: {all:?}");
    act(&mut world, &session, "browser.v1", "fill", json!({"id":"rename-title","value":"Release code"}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"rename-submit"}));
    assert_eq!(page(&world, &session)["title"], format!("Release code - {brand}"));

    // The citation is a real link out of the site, and the history link comes back.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url": mine}));
    assert_eq!(browser(&world, &session)["url"], mine);
    act(&mut world, &session, "browser.v1", "click", json!({"id":"msg-1-cite-0"}));
    assert_eq!(browser(&world, &session)["url"], source);
    act(&mut world, &session, "browser.v1", "back", json!({}));
    assert_eq!(page(&world, &session)["title"], format!("Release code - {brand}"));

    // Deleting lands on the home page with the conversation gone from the history.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"delete"}));
    let all = elements(&page(&world, &session));
    assert!(all.iter().any(|e| e["id"] == "composer-message"));
    assert!(!all.iter().any(|e| e["url"] == mine), "{all:?}");

    // A suggestion is a one-button form: clicking it asks that question.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url": format!("{origin}/")}));
    let suggestion = by_id(&elements(&page(&world, &session)), "suggestion-0")["text"].as_str().unwrap().to_owned();
    act(&mut world, &session, "browser.v1", "click", json!({"id":"suggestion-0"}));
    let all = elements(&page(&world, &session));
    assert!(says(&all, &suggestion), "{all:?}");
    assert!(all.iter().any(|e| e["id"] == "regenerate" && e["kind"] == "button"));

    // A seeded conversation opens from the sidebar.
    act(&mut world, &session, "browser.v1", "click", json!({"id": format!("side-{seeded}")}));
    assert_eq!(browser(&world, &session)["url"], format!("{origin}/c/{seeded}"));
}

#[test]
fn chatgpt_answers_a_typed_prompt_with_a_citation_through_the_agent_api() {
    drive("http://chatgpt.com", "ChatGPT", "conv-1");
}
#[test]
fn claude_answers_a_typed_prompt_with_a_citation_through_the_agent_api() {
    drive("http://claude.ai", "Claude", "conv-2");
}
