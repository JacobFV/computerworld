//! The shipped Messages seed and the texting contract the phones' Messages app drives:
//! handles, conversations, receipts and tapbacks.
use cw_protocol::{HttpRequest, Page};
use cw_sdk::{Service, ServiceContext};
use cw_service_messages::MessagesService;
use serde_json::{json, Value};

fn ctx(actor: &str, tick: u64) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: format!("{actor}-phone"),
        tick,
        seed: 1,
        instance: "messages".into(),
    }
}
fn site() -> Value {
    let raw =
        std::fs::read_to_string("../../worlds/company-2026/sites/messages.json").expect("site");
    serde_json::from_str(&raw).expect("site file parses")
}
fn get(state: &mut Value, actor: &str, path: &str) -> (u16, String) {
    let r = MessagesService
        .handle(
            state,
            &ctx(actor, 60),
            &HttpRequest::get(format!("http://messages.internal{path}")),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn post(state: &mut Value, actor: &str, tick: u64, path: &str, body: Value) -> (u16, String) {
    let r = MessagesService
        .handle(
            state,
            &ctx(actor, tick),
            &HttpRequest::json("POST", format!("http://messages.internal{path}"), &body).unwrap(),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn seeded() -> Value {
    let site = site();
    assert_eq!(site["kind"], "messages");
    assert_eq!(site["domains"][0], "messages.internal");
    MessagesService
        .initialize(site["initial_state"].clone(), &ctx("alice", 0))
        .unwrap()
}
const AB: &str = "+14155550100|+14155550101";
const CREW: &str = "+14155550100|+14155550101|+14155550102";

#[test]
fn the_seed_renders_for_each_of_its_people() {
    let mut state = seeded();
    for (actor, path) in [
        ("alice", "/"),
        ("bob", "/"),
        ("carol", "/"),
        ("alice", &format!("/conversations/{AB}")),
        ("bob", &format!("/conversations/{AB}")),
        ("carol", &format!("/conversations/{CREW}")),
        ("alice", "/conversations/+14155550100|+14155550199"),
        ("eve", "/"),
    ] {
        let (status, body) = get(&mut state, actor, path);
        assert_eq!(status, 200, "{actor} {path}");
        serde_json::from_str::<Page>(&body)
            .unwrap()
            .validate()
            .unwrap_or_else(|e| panic!("{actor} {path}: {e}"));
    }
    let thread = get(&mut state, "bob", &format!("/conversations/{AB}")).1;
    // Bob's last text is delivered and not yet read; his bubbles are blue.
    assert!(thread.contains("sms-7-status") && thread.contains("Delivered"));
    assert!(thread.contains("#0b84fe"));
    let alice = get(&mut state, "alice", &format!("/conversations/{AB}")).1;
    assert!(
        alice.contains("Read tick 33"),
        "alice's last message was read"
    );
    let sms = get(
        &mut state,
        "alice",
        "/conversations/+14155550100|+14155550199",
    )
    .1;
    assert!(sms.contains("#34c759") && sms.contains("Sent as Text Message"));
    // Carol is not in alice and bob's thread.
    assert_eq!(
        get(&mut state, "carol", &format!("/conversations/{AB}")).0,
        403
    );
    assert_eq!(
        get(&mut state, "carol", &format!("/api/conversations/{AB}")).0,
        403
    );
}

#[test]
fn texting_delivers_reads_and_tapbacks() {
    let mut state = seeded();
    let inbox: Value =
        serde_json::from_str(&get(&mut state, "alice", "/api/conversations").1).unwrap();
    assert_eq!(inbox[0]["id"], CREW, "newest activity first");
    assert_eq!(inbox[1]["id"], AB);
    assert_eq!(inbox[1]["unread"], 1);
    assert_eq!(inbox[1]["title"], "Bob Martinez");
    // Opening reads: alice's read receipt lands on bob's last text.
    let (status, thread) = post(
        &mut state,
        "alice",
        61,
        &format!("/api/conversations/{AB}/read"),
        json!({}),
    );
    assert_eq!(status, 200);
    let thread: Value = serde_json::from_str(&thread).unwrap();
    assert_eq!(thread["me"], "+14155550100");
    assert_eq!(thread["unread"], 0);
    assert_eq!(thread["messages"][6]["read"]["+14155550100"], 61);
    let (status, sent) = post(
        &mut state,
        "alice",
        62,
        &format!("/api/conversations/{AB}/messages"),
        json!({"text":"spicy it is"}),
    );
    assert_eq!(status, 200);
    let sent: Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(sent["from"], "+14155550100");
    assert_eq!(sent["service"], "imessage");
    assert_eq!(sent["delivered"], json!(["+14155550101"]));
    let id = sent["id"].as_str().unwrap().to_owned();
    assert_eq!(
        post(
            &mut state,
            "bob",
            63,
            &format!("/api/conversations/{AB}/messages/{id}/tapbacks"),
            json!({"tapback":"loved"})
        )
        .0,
        200
    );
    assert_eq!(
        post(
            &mut state,
            "bob",
            63,
            &format!("/api/conversations/{AB}/messages/{id}/tapbacks"),
            json!({"tapback":"wow"})
        )
        .0,
        400
    );
    let bob: Value =
        serde_json::from_str(&get(&mut state, "bob", &format!("/api/conversations/{AB}")).1)
            .unwrap();
    assert_eq!(
        bob["messages"][7]["tapbacks"]["loved"],
        json!(["+14155550101"])
    );
    assert_eq!(bob["unread"], 1);
    // The contacts directory is what the phones' Contacts app reads.
    let contacts: Value =
        serde_json::from_str(&get(&mut state, "alice", "/api/contacts").1).unwrap();
    assert!(contacts
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["user"] == "carol" && c["handles"][0] == "+14155550102"));
}

#[test]
fn conversations_open_once_by_handle_or_name_and_strangers_get_sms() {
    let mut state = seeded();
    let (status, body) = post(
        &mut state,
        "bob",
        70,
        "/api/conversations",
        json!({"to":"carol"}),
    );
    assert_eq!(status, 200);
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["conversation"], "+14155550101|+14155550102");
    assert_eq!(
        post(
            &mut state,
            "carol",
            70,
            "/api/conversations",
            json!({"to":"+14155550101"})
        )
        .0,
        200
    );
    assert_eq!(state["conversations"].as_object().unwrap().len(), 4);
    assert_eq!(
        post(
            &mut state,
            "bob",
            70,
            "/api/conversations",
            json!({"to":"nobody"})
        )
        .0,
        403
    );
    assert_eq!(
        post(
            &mut state,
            "eve",
            70,
            "/api/conversations",
            json!({"to":"bob"})
        )
        .0,
        403
    );
    // A number nobody has in iMessage gets a green text with no receipts.
    let (status, sent) = post(
        &mut state,
        "bob",
        71,
        "/api/conversations/+14155550101|+14155550199/messages",
        json!({"text":"hi"}),
    );
    assert_eq!(status, 403, "{sent}");
    post(
        &mut state,
        "bob",
        71,
        "/api/conversations",
        json!({"to":"dentist"}),
    );
    let (status, sent) = post(
        &mut state,
        "bob",
        71,
        "/api/conversations/+14155550101|+14155550199/messages",
        json!({"text":"hi"}),
    );
    assert_eq!(status, 200);
    let sent: Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(sent["service"], "sms");
    assert!(sent.get("delivered").is_none());
    // A named group.
    let (status, page) = post(
        &mut state,
        "alice",
        72,
        "/conversations",
        json!({"to":"bob, admin","name":"Ops"}),
    );
    assert_eq!(status, 200);
    assert!(page.contains("Ops"));
    let restored: cw_service_messages::MessagesState =
        serde_json::from_value(state.clone()).unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), state);
}
