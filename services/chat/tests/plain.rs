//! The frozen plain rendering of chat.internal, and the API contract the browser and
//! world tests drive. Slack and Discord live in their own crates now, so a seed that still
//! names one of those skins is refused rather than silently drawn as plain chat.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_chat::{ChatService, ChatState};
use cw_service_common::html::validate_strict;
use cw_web::dom::Document;
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

fn dom(body: &str) -> Document {
    validate_strict(body).unwrap_or_else(|e| panic!("{e:?}"));
    cw_web::html::parse(body)
}
fn node(doc: &Document, id: &str) -> cw_web::dom::NodeId {
    *doc.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"))
}
fn text(doc: &Document, id: &str) -> String {
    doc.text_content(node(doc, id)).trim().to_owned()
}
fn attr(doc: &Document, id: &str, name: &str) -> String {
    doc.attr(node(doc, id), name).unwrap_or_default().to_owned()
}

/// chat.internal must serialise exactly as it always has, and its pages keep the ids,
/// links and forms the plain rendering had, now as HTML.
#[test]
fn plain_state_is_byte_identical_and_pages_keep_their_ids() {
    const STATE: &str = r#"{"channels":{"general":{"members":["admin","alice","bob","carol"],"messages":[],"title":"General"}},"next_id":0}"#;
    let mut state = ChatService
        .initialize(
            json!({"next_id":0,"channels":{"general":{"title":"General",
                "members":["alice","bob","carol","admin"],"messages":[]}}}),
            &ctx("alice"),
        )
        .unwrap();
    assert_eq!(serde_json::to_string(&state).unwrap(), STATE);
    let home_body = get(&mut state, "alice", "/").1;
    assert!(home_body.starts_with("<!DOCTYPE html>"));
    let home = dom(&home_body);
    assert_eq!(text(&home, "title"), "Chat");
    assert_eq!(home.tag(node(&home, "title")), Some("h1"));
    assert_eq!(attr(&home, "general", "href"), "/channels/general");
    assert!(text(&home, "general").contains("General"));
    assert!(home.by_id("send").is_empty() && home.by_id("channel").is_empty());
    let channel = dom(&get(&mut state, "alice", "/channels/general").1);
    assert_eq!(text(&channel, "channel"), "General");
    assert_eq!(
        attr(&channel, "send", "action"),
        "/channels/general/messages"
    );
    assert_eq!(attr(&channel, "send", "method"), "post");
    assert_eq!(attr(&channel, "send-text", "name"), "text");
    assert_eq!(channel.tag(node(&channel, "send-submit")), Some("button"));
    // Rendering changes nothing.
    assert_eq!(serde_json::to_string(&state).unwrap(), STATE);
}

/// Messages, reactions, replies and DMs on the page, every page strictly valid.
#[test]
fn messages_reactions_and_dms_render_with_their_forms() {
    let mut state = seeded();
    let page = dom(&get(&mut state, "alice", "/channels/eng").1);
    assert_eq!(attr(&page, "eng", "href"), "/channels/eng");
    assert_eq!(attr(&page, "random", "href"), "/channels/random");
    let first = text(&page, "chat-1");
    assert!(first.contains("bob") && first.contains("BFS path test fails on Windows only."));
    let second = text(&page, "chat-2");
    assert!(second.contains("tada") && second.contains('1'));
    assert_eq!(
        attr(&page, "chat-1-react", "action"),
        "/channels/eng/messages/chat-1/reactions"
    );
    assert_eq!(attr(&page, "chat-1-react", "method"), "post");
    assert_eq!(attr(&page, "chat-1-react-reaction", "name"), "reaction");
    assert_eq!(page.tag(node(&page, "chat-1-react-submit")), Some("button"));
    // A browser form post reacts and lands back on the channel.
    let mut request =
        HttpRequest::get("http://chat.internal/channels/eng/messages/chat-1/reactions");
    request.method = "POST".into();
    request.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    request.body = b"reaction=eyes".to_vec();
    let r = ChatService
        .handle(&mut state, &ctx("carol"), &request)
        .unwrap();
    assert_eq!(r.status, 200);
    let after = dom(&String::from_utf8(r.body).unwrap());
    assert!(text(&after, "chat-1").contains("eyes"));
    // A threaded reply quotes its parent.
    let (status, replied) = post(
        &mut state,
        "carol",
        "/channels/eng/messages",
        json!({"text":"on it","parent":"chat-1"}),
    );
    assert_eq!(status, 200);
    let replied = dom(&replied);
    assert!(
        text(&replied, "chat-3").contains("BFS path test")
            && text(&replied, "chat-3").contains("on it")
    );
    // DMs are listed under their key and opened from the sidebar's form.
    assert_eq!(attr(&page, "dm", "action"), "/dms");
    assert_eq!(attr(&page, "dm-to", "name"), "to");
    let opened = dom(&post(&mut state, "alice", "/dms", json!({"to":"bob"})).1);
    assert_eq!(attr(&opened, "dm-alice|bob", "href"), "/channels/alice|bob");
    assert_eq!(text(&opened, "channel"), "alice and bob");
    assert_eq!(
        attr(&opened, "send", "action"),
        "/channels/alice|bob/messages"
    );
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
