//! Slack and Discord behaviour, the frozen plain rendering, and the API the desktop
//! Messages application in `cw-applications` talks to.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_chat::{ChatService, ChatState};
use serde_json::{json, Value};

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: format!("{actor}-pc"),
        tick: 12,
        seed: 2,
        instance: "slack".into(),
    }
}
fn get(state: &mut Value, actor: &str, path: &str) -> (u16, String) {
    let r = ChatService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::get(format!("http://slack.com{path}")),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn post(state: &mut Value, actor: &str, path: &str, body: Value) -> (u16, String) {
    let r = ChatService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::json("POST", format!("http://slack.com{path}"), &body).unwrap(),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn workspace(skin: &str) -> Value {
    ChatService
        .initialize(
            json!({"skin":skin,"workspace":"Northstar","next_id":2,"channels":{
                "eng":{"title":"eng","members":["alice","bob","carol","admin"],"messages":[
                    {"id":"chat-1","author":"bob","text":"BFS path test fails on Windows only.",
                     "time":30,"reactions":{}},
                    {"id":"chat-2","author":"alice","text":"Priya answered it on Stack Overflow.",
                     "time":31,"reactions":{"tada":["carol"]}}]},
                "random":{"title":"random","members":["alice","bob","carol"],"messages":[]}}}),
            &ctx("alice"),
        )
        .unwrap()
}

/// `skin: "plain"` is chat.internal, and it must serialise and render exactly as it always has.
#[test]
fn plain_state_and_pages_are_byte_identical() {
    const STATE: &str = r#"{"channels":{"general":{"members":["admin","alice","bob","carol"],"messages":[],"title":"General"}},"next_id":0}"#;
    const HOME: &str = r#"{"version":1,"title":"Chat","elements":[{"kind":"heading","id":"title","text":"Chat","level":1},{"kind":"link","id":"general","text":"General","url":"/channels/general"}]}"#;
    let mut state = ChatService
        .initialize(
            json!({"next_id":0,"channels":{"general":{"title":"General",
                "members":["alice","bob","carol","admin"],"messages":[]}}}),
            &ctx("alice"),
        )
        .unwrap();
    assert_eq!(serde_json::to_string(&state).unwrap(), STATE);
    assert_eq!(get(&mut state, "alice", "/").1, HOME);
    let channel = get(&mut state, "alice", "/channels/general").1;
    assert!(channel.contains(r#""id":"send""#) && !channel.contains("theme"));
    assert!(!channel.contains("sidebar") && !channel.contains("rail"));
}

/// The contract `crates/applications/src/apps/chat.rs` drives; its own tests assert on it too.
#[test]
fn desktop_messages_api_contract_holds() {
    let mut state = workspace("slack");
    let (status, list) = get(&mut state, "alice", "/api/channels");
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&list).unwrap(),
        json!({"eng":"eng","random":"random"})
    );
    let channel: Value =
        serde_json::from_str(&get(&mut state, "alice", "/api/channels/eng").1).unwrap();
    assert_eq!(channel["messages"][0]["author"], "bob");
    assert_eq!(channel["title"], "eng");
    let sent: Value = serde_json::from_str(
        &post(
            &mut state,
            "alice",
            "/api/channels/eng/messages",
            json!({"text":"Linking the accepted answer."}),
        )
        .1,
    )
    .unwrap();
    assert_eq!(sent["author"], "alice");
    assert_eq!(sent["time"], 12);
    let id = sent["id"].as_str().unwrap().to_owned();
    assert_eq!(
        post(
            &mut state,
            "carol",
            &format!("/api/channels/eng/messages/{id}/reactions"),
            json!({"reaction":"+1"})
        )
        .0,
        200
    );
    let after: Value =
        serde_json::from_str(&get(&mut state, "bob", "/api/channels/eng/messages").1).unwrap();
    assert_eq!(after["messages"][2]["reactions"]["+1"], json!(["carol"]));
    // A non-member gets nothing, over the API or in the page.
    assert_eq!(get(&mut state, "eve", "/api/channels/eng").0, 403);
}

#[test]
fn slack_workspace_renders_threads_and_reactions() {
    let mut state = workspace("slack");
    let page = get(&mut state, "alice", "/channels/eng").1;
    assert!(page.contains("Northstar") && page.contains("# eng"));
    assert!(page.contains("chat-2-has-tada") && page.contains("chat-1-react-+1"));
    // Slack permalinks in seeded prose are /archives/<channel>.
    assert_eq!(get(&mut state, "alice", "/archives/eng").1, page);
    let (status, threaded) = post(
        &mut state,
        "carol",
        "/channels/eng/messages",
        json!({"text":"Accepted answer is the hash order.","parent":"chat-1"}),
    );
    assert_eq!(status, 200);
    assert!(threaded.contains("in-thread") && threaded.contains("Accepted answer"));
    // A reply to a message that is not there is refused and changes nothing.
    let before = state.clone();
    assert_eq!(
        post(
            &mut state,
            "carol",
            "/channels/eng/messages",
            json!({"text":"orphan","parent":"chat-999"})
        )
        .0,
        400
    );
    assert_eq!(before, state);
}

#[test]
fn discord_wears_its_own_palette() {
    let mut state = workspace("discord");
    let page = get(&mut state, "alice", "/channels/eng").1;
    assert!(page.contains("Text channels") && page.contains("#5865f2"));
    assert!(!page.contains("Channels\""));
    assert!(ChatService
        .initialize(json!({"skin":"irc"}), &ctx("alice"))
        .is_err());
}

#[test]
fn direct_messages_open_once_and_survive_a_round_trip() {
    let mut state = workspace("slack");
    let (status, page) = post(&mut state, "alice", "/dms", json!({"to":"bob"}));
    assert_eq!(status, 200);
    assert!(page.contains("alice|bob"));
    // Opening again is idempotent: the pair has exactly one conversation.
    post(&mut state, "bob", "/dms", json!({"to":"alice"}));
    assert_eq!(state["dms"].as_object().unwrap().len(), 1);
    assert_eq!(
        post(&mut state, "alice", "/dms", json!({"to":"nobody"})).0,
        403
    );
    post(
        &mut state,
        "alice",
        "/channels/alice|bob/messages",
        json!({"text":"which monitor did you get?"}),
    );
    let seen = get(&mut state, "bob", "/channels/alice|bob").1;
    assert!(seen.contains("which monitor did you get?"));
    assert_eq!(get(&mut state, "carol", "/channels/alice|bob").0, 403);
    let restored: ChatState = serde_json::from_value(state.clone()).unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), state);
}
