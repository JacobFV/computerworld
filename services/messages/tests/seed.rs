//! The shipped Messages seed and the texting contract the phones' Messages app drives:
//! handles, conversations, receipts and tapbacks.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::validate_strict;
use cw_service_messages::MessagesService;
use cw_web::dom::Document;
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
fn class(doc: &Document, id: &str) -> String {
    attr(doc, id, "class")
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
        validate_strict(&body).unwrap_or_else(|e| panic!("{actor} {path}: {e:?}"));
    }
    let thread = dom(&get(&mut state, "bob", &format!("/conversations/{AB}")).1);
    // Bob's last text is delivered and not yet read; his bubbles are blue.
    assert_eq!(text(&thread, "sms-7-status"), "Delivered");
    assert!(class(&thread, "sms-7-bubble").contains("imessage"));
    assert!(class(&thread, "sms-1-bubble").contains("imessage"));
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
    assert!(sms.contains("Sent as Text Message"));
    let sms = dom(&sms);
    let mine = sms
        .descendants(Document::ROOT)
        .find(|n| {
            sms.attr(*n, "class")
                .is_some_and(|c| c.split(' ').any(|c| c == "sms") && c.contains("bubble"))
        })
        .expect("a green bubble");
    assert!(sms.attr(mine, "id").unwrap().ends_with("-bubble"));
    assert_eq!(attr(&sms, "send-text", "placeholder"), "Text Message");
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
    // A conversation with nothing in it yet is still a page the engine renders: the
    // transcript says so rather than standing empty, and it validates like the rest.
    let fresh = dom(&page);
    assert_eq!(text(&fresh, "bar-title"), "Ops");
    assert_eq!(text(&fresh, "empty"), "Say something.");
    assert!(
        !fresh.by_id("members").is_empty(),
        "a group names its people"
    );
    let restored: cw_service_messages::MessagesState =
        serde_json::from_value(state.clone()).unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), state);
}

/// The pages keep the ids, links and forms the Page version had.
#[test]
fn the_pages_keep_their_ids_links_and_forms() {
    let mut state = seeded();
    let inbox = dom(&get(&mut state, "alice", "/").1);
    assert_eq!(text(&inbox, "bar-title"), "Messages");
    assert_eq!(text(&inbox, "bar-me"), "Alice Chen · +14155550100");
    assert_eq!(
        attr(&inbox, &format!("row-{AB}"), "href"),
        format!("/conversations/{AB}")
    );
    assert_eq!(text(&inbox, &format!("row-{AB}-title")), "Bob Martinez");
    assert_eq!(text(&inbox, &format!("row-{AB}-time")), "tick 44");
    assert!(text(&inbox, &format!("row-{AB}-preview")).contains("spicy"));
    assert!(!inbox.by_id(&format!("row-{AB}-unread")).is_empty());
    assert!(!inbox.by_id(&format!("row-{AB}-avatar")).is_empty());
    assert_eq!(attr(&inbox, "new", "action"), "/conversations");
    assert_eq!(attr(&inbox, "new", "method"), "post");
    assert_eq!(attr(&inbox, "new-to", "name"), "to");
    assert_eq!(attr(&inbox, "new-name", "name"), "name");
    assert_eq!(inbox.tag(node(&inbox, "new-submit")), Some("button"));
    // A person with no handle sees why the list is empty, and no form.
    let eve = dom(&get(&mut state, "eve", "/").1);
    assert_eq!(text(&eve, "empty"), "This device has no number or address.");
    assert!(eve.by_id("new").is_empty());

    let thread = dom(&get(&mut state, "alice", &format!("/conversations/{AB}")).1);
    assert_eq!(text(&thread, "bar-title"), "Bob Martinez");
    assert_eq!(text(&thread, "bar-service"), "iMessage");
    assert_eq!(text(&thread, "service"), "iMessage");
    assert_eq!(attr(&thread, "back", "href"), "/");
    assert_eq!(
        attr(&thread, "send", "action"),
        format!("/conversations/{AB}/messages")
    );
    assert_eq!(attr(&thread, "send", "method"), "post");
    assert_eq!(attr(&thread, "send-text", "name"), "text");
    assert_eq!(attr(&thread, "send-text", "aria-label"), "iMessage");
    assert_eq!(thread.tag(node(&thread, "send-submit")), Some("button"));
    assert_eq!(
        attr(&thread, "read", "action"),
        format!("/conversations/{AB}/read")
    );
    assert!(!thread.by_id("read-submit").is_empty());
    assert!(
        class(&thread, "sms-1-row").contains("in") && class(&thread, "sms-2-row").contains("out")
    );
    assert!(class(&thread, "sms-1-bubble").contains("incoming"));
    assert_eq!(text(&thread, "sms-1-text"), "lunch?");
    // Tapbacks: a picker per bubble, one named button per tapback, posting to the message.
    assert_eq!(
        attr(&thread, "sms-1-tapbacks", "action"),
        format!("/conversations/{AB}/messages/sms-1/tapbacks")
    );
    for name in cw_service_messages::TAPBACKS {
        let id = format!("sms-1-tapback-{name}");
        assert_eq!(attr(&thread, &id, "name"), "tapback");
        assert_eq!(attr(&thread, &id, "value"), *name);
    }
    // The sidebar rides along, with the open conversation marked.
    assert!(class(&thread, &format!("row-{AB}")).contains("current"));
    // A browser form post (urlencoded) gives a tapback and lands back on the thread.
    let mut request = HttpRequest::get(format!(
        "http://messages.internal/conversations/{AB}/messages/sms-1/tapbacks"
    ));
    request.method = "POST".into();
    request.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    request.body = b"tapback=laughed".to_vec();
    let r = MessagesService
        .handle(&mut state, &ctx("alice", 61), &request)
        .unwrap();
    assert_eq!(r.status, 200);
    let after = dom(&String::from_utf8(r.body).unwrap());
    assert_eq!(
        attr(&after, "sms-1-has-laughed", "title"),
        "laughed · Alice Chen"
    );
    // The group names its senders and lists its members.
    let crew = dom(&get(&mut state, "carol", &format!("/conversations/{CREW}")).1);
    assert!(!crew.by_id("members").is_empty());
    assert!(crew
        .descendants(Document::ROOT)
        .any(|n| crew.attr(n, "id").is_some_and(|id| id.ends_with("-from"))));
}
