//! The frozen plain rendering of chat.internal, and the API contract the browser and
//! world tests drive. Slack and Discord live in their own crates now, so a seed that still
//! names one of those skins is refused rather than silently drawn as plain chat.
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
        instance: "chat".into(),
    }
}
fn get(state: &mut Value, actor: &str, path: &str) -> (u16, String) {
    let r = ChatService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::get(format!("http://chat.internal{path}")),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn post(state: &mut Value, actor: &str, path: &str, body: Value) -> (u16, String) {
    let r = ChatService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::json("POST", format!("http://chat.internal{path}"), &body).unwrap(),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn seeded() -> Value {
    ChatService
        .initialize(
            json!({"next_id":2,"channels":{
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

/// chat.internal must serialise and render exactly as it always has.
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

/// The branded skins moved out; naming one here is a seed error, not a fallback.
#[test]
fn only_the_plain_skin_is_accepted() {
    for skin in ["slack", "discord", "irc"] {
        assert!(
            ChatService
                .initialize(json!({"skin":skin}), &ctx("alice"))
                .is_err(),
            "{skin} must be refused"
        );
    }
    let state = ChatService
        .initialize(json!({"skin":"plain"}), &ctx("alice"))
        .unwrap();
    assert!(state.get("skin").is_none());
}

/// The contract the browser and the world's tests drive over `/api`.
#[test]
fn api_contract_holds() {
    let mut state = seeded();
    let (status, list) = get(&mut state, "alice", "/api/channels");
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&list).unwrap(),
        json!({"eng":"eng","random":"random"})
    );
    let channel: Value =
        serde_json::from_str(&get(&mut state, "alice", "/api/channels/eng").1).unwrap();
    assert_eq!(channel["messages"][0]["author"], "bob");
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
    assert_eq!(get(&mut state, "eve", "/channels/eng").0, 403);
    // A threaded reply to a message that is not there is refused and changes nothing.
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
fn direct_messages_open_once_and_survive_a_round_trip() {
    let mut state = seeded();
    let (status, page) = post(&mut state, "alice", "/dms", json!({"to":"bob"}));
    assert_eq!(status, 200);
    assert!(page.contains("alice|bob"));
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
