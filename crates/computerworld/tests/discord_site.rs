//! discord.com served as HTML by the Discord service and driven end to end through the
//! agent API: the browser renders the server with the web engine, the semantic
//! observation lists the channels, the composer, the reaction chips and the voice rows
//! by the ids the service documents, and clicking, filling and submitting them changes
//! the server the way the Page version did.
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
fn has(all: &[Value], id: &str) -> bool {
    all.iter().any(|e| e["id"] == id)
}
fn says(all: &[Value], needle: &str) -> bool {
    all.iter()
        .any(|e| e["text"].as_str().is_some_and(|t| t.contains(needle)))
}

#[test]
fn discord_is_read_posted_to_reacted_on_and_joined_through_the_agent_api() {
    let (mut world, session) = world();
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://discord.com/"}));
    let home = page(&world, &session);
    assert_eq!(home["title"], "Atlas Community");
    let all = elements(&home);
    // The sidebar lists the channels alice can see as links, the voice rows as buttons.
    let help = by_id(&all, "nav-atlas-help");
    assert_eq!(help["kind"], "link");
    assert_eq!(help["url"], "http://discord.com/channels/atlas/atlas-help");
    assert_eq!(by_id(&all, "nav-mod-log")["kind"], "link");
    assert_eq!(by_id(&all, "voice-Lounge")["kind"], "button");
    assert_eq!(by_id(&all, "rail-home")["url"], "http://discord.com/channels/@me");
    assert!(!has(&all, "send-text"), "no composer until a channel is open");

    // Open a channel by its sidebar row.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"nav-atlas-help"}));
    assert_eq!(browser(&world, &session)["url"], "http://discord.com/channels/atlas/atlas-help");
    let channel = page(&world, &session);
    assert_eq!(channel["title"], "#atlas-help · Atlas Community");
    let all = elements(&channel);
    let field = by_id(&all, "send-text");
    assert_eq!(field["kind"], "input");
    assert_eq!(field["label"], "Message #atlas-help");
    assert_eq!(by_id(&all, "send")["kind"], "form");
    assert_eq!(by_id(&all, "send-submit")["kind"], "button");
    assert!(says(&all, "My replay diverges after about 200 ticks"));
    let link = all
        .iter()
        .find(|e| e["kind"] == "link" && e["id"].as_str().is_some_and(|id| id.starts_with("chat-34-text-p")))
        .expect("the issue link in priya's answer");
    assert_eq!(link["url"], "http://github.com/northstar/atlas/issues/14");

    // Type a message and press Enter: the composer posts it and lands on the channel.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"send-text"}));
    act(&mut world, &session, "keyboard.v1", "type", json!({"text":"Diffing state_hash() per tick found it, thanks"}));
    act(&mut world, &session, "browser.v1", "key", json!({"key":"Enter"}));
    let all = elements(&page(&world, &session));
    assert!(says(&all, "Diffing state_hash() per tick found it, thanks"));
    assert_eq!(by_id(&all, "send-text")["value"], "");

    // Fill and click the send button: the same form by its button.
    act(&mut world, &session, "browser.v1", "fill", json!({"id":"send-text","value":"Second message"}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"send-submit"}));
    let all = elements(&page(&world, &session));
    assert!(says(&all, "Second message"));

    // A reaction chip toggles alice's reaction.
    let chip = by_id(&all, "chat-33-react-eyes");
    assert_eq!(chip["kind"], "button");
    assert_eq!(chip["text"], "👀 1");
    act(&mut world, &session, "browser.v1", "click", json!({"id":"chat-33-react-eyes"}));
    let all = elements(&page(&world, &session));
    assert_eq!(by_id(&all, "chat-33-react-eyes")["text"], "👀 2");

    // The member-list toggle is a link that closes the list.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://discord.com/channels/atlas/atlas-help"}));
    let all = elements(&page(&world, &session));
    assert!(says(&all, "MOD — 2"));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"channel-members"}));
    assert_eq!(
        browser(&world, &session)["url"],
        "http://discord.com/channels/atlas/atlas-help?members=0"
    );
    assert!(!says(&elements(&page(&world, &session)), "MOD — 2"));

    // Joining a voice channel seats alice under it; the same row then leaves.
    let named = |all: &[Value]| {
        all.iter()
            .filter(|e| e["text"].as_str().is_some_and(|t| t.contains("alice.chen")))
            .count()
    };
    let outside = named(&elements(&page(&world, &session)));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"voice-Lounge"}));
    let inside = named(&elements(&page(&world, &session)));
    assert!(inside > outside, "alice is listed under Lounge: {outside} -> {inside}");
    act(&mut world, &session, "browser.v1", "click", json!({"id":"voice-Lounge"}));
    assert_eq!(named(&elements(&page(&world, &session))), outside);
    // A voice channel whose name has a space is joined the same way.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"voice-Office Hours"}));
    assert_eq!(named(&elements(&page(&world, &session))), inside);
    // A channel mention in text is a link into the server.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://discord.com/channels/atlas/welcome"}));
    let all = elements(&page(&world, &session));
    let mention = all
        .iter()
        .find(|e| e["kind"] == "link" && e["text"] == "#rules")
        .expect("#rules mention");
    let id = mention["id"].as_str().unwrap().to_owned();
    act(&mut world, &session, "browser.v1", "click", json!({"id": id}));
    assert_eq!(browser(&world, &session)["url"], "http://discord.com/channels/atlas/rules");
}
