//! Every control the chat.internal pages draw, checked against what the service answers.
//!
//! The sweep starts at the home page, follows every same-origin link it finds, probes
//! every form `action` and `formaction` with the method written on it, and complains
//! about anything that answers 404, 405 or an error, about every dead control
//! [`audit::page`] finds on a page (an anchor with nowhere to go, a button in no form, a
//! field with no name or no label, an inert element drawn with `cursor: pointer`), and
//! about any link that leads back to the page it is on.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_chat::{ChatService, ChatState};
use cw_service_common::audit;
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

/// Every page kind at once: a busy channel with reactions and a threaded reply, an empty
/// one, and a direct message.
fn seeded() -> Value {
    let mut state = ChatService
        .initialize(
            json!({"next_id":3,"channels":{
                "eng":{"title":"eng","members":["alice","bob","carol","admin"],"messages":[
                    {"id":"chat-1","author":"bob","text":"BFS path test fails on Windows only.",
                     "time":30,"reactions":{"eyes":["alice"]}},
                    {"id":"chat-2","author":"alice","text":"Priya answered it on Stack Overflow.",
                     "time":31,"reactions":{"tada":["carol"],"+1":["bob","carol"]}},
                    {"id":"chat-3","author":"carol","text":"on it","time":32,
                     "reactions":{},"parent":"chat-1"}]},
                "random":{"title":"random","members":["alice","bob","carol"],"messages":[]}}}),
            &ctx("alice"),
        )
        .unwrap();
    let opened = ChatService
        .handle(
            &mut state,
            &ctx("alice"),
            &HttpRequest::json("POST", "http://chat.internal/api/dms", &json!({"to":"bob"}))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(opened.status, 200);
    ChatService
        .handle(
            &mut state,
            &ctx("alice"),
            &HttpRequest::json(
                "POST",
                "http://chat.internal/api/channels/alice|bob/messages",
                &json!({"text":"which monitor did you get?"}),
            )
            .unwrap(),
        )
        .unwrap();
    state
}

/// The sidebar row for the conversation already open points at the page it is on, the
/// way a selected channel does in any chat client. Every other control must go somewhere.
fn open_rows(state: &Value) -> Vec<String> {
    let s: ChatState = serde_json::from_value(state.clone()).unwrap();
    s.channels
        .keys()
        .cloned()
        .chain(s.dms.keys().map(|id| format!("dm-{id}")))
        .collect()
}

fn sweep(seed: &Value, actors: &[&str]) {
    let rows = open_rows(seed);
    let allow: Vec<&str> = rows.iter().map(String::as_str).collect();
    for actor in actors {
        let mut state = seed.clone();
        let mut call = |method: &str, path: &str| -> (u16, String) {
            let url = format!("http://chat.internal{path}");
            let response = if method == "POST" {
                let mut request = HttpRequest::get(url);
                request.method = "POST".into();
                request.headers.insert(
                    "content-type".into(),
                    "application/x-www-form-urlencoded".into(),
                );
                // A POST probe really runs, so it runs against a scratch copy of the
                // state and leaves the pages the crawl is reading alone.
                let mut scratch = state.clone();
                ChatService.handle(&mut scratch, &ctx(actor), &request)
            } else {
                ChatService.handle(&mut state, &ctx(actor), &HttpRequest::get(url))
            }
            .unwrap();
            (response.status, String::from_utf8(response.body).unwrap())
        };
        let faults = audit::Sweep::new(&["/", "/dms"], &mut call)
            .allow_self(&allow)
            .run();
        assert!(faults.is_empty(), "{actor}:\n{}", faults.join("\n"));
    }
}

#[test]
fn every_control_on_every_chat_page_can_act() {
    // A member of everything, a member of some of it, and a stranger with no rooms.
    sweep(&seeded(), &["alice", "carol", "admin", "eve"]);
}

/// The company world ships a single empty channel; that page has controls too.
#[test]
fn the_shipped_seed_has_no_dead_controls_either() {
    let raw = std::fs::read_to_string("../../worlds/company-2026/world.json").expect("world");
    let world: Value = serde_json::from_str(&raw).expect("world file parses");
    let site = world["services"]
        .as_array()
        .expect("services")
        .iter()
        .find(|s| s["kind"] == "chat")
        .expect("a chat service in the company world");
    let state = ChatService
        .initialize(site["initial_state"].clone(), &ctx("alice"))
        .unwrap();
    sweep(&state, &["alice", "admin"]);
}

fn dom(body: &str) -> cw_web::dom::Document {
    cw_service_common::html::validate_strict(body).unwrap_or_else(|e| panic!("{e:?}"));
    cw_web::html::parse(body)
}
fn node(doc: &cw_web::dom::Document, id: &str) -> cw_web::dom::NodeId {
    *doc.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"))
}
fn attr(doc: &cw_web::dom::Document, id: &str, name: &str) -> String {
    doc.attr(node(doc, id), name).unwrap_or_default().to_owned()
}
fn get(state: &mut Value, actor: &str, path: &str) -> String {
    let r = ChatService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::get(format!("http://chat.internal{path}")),
        )
        .unwrap();
    assert_eq!(r.status, 200, "{actor} {path}");
    String::from_utf8(r.body).unwrap()
}

/// A reaction already on a message is drawn as a pill next to the button that adds one,
/// so it must be a control: it used to be an inert `<span>` that swallowed the click.
/// The quote on a threaded reply must lead to the message it quotes.
#[test]
fn a_reaction_pill_joins_that_reaction_and_a_quote_leads_to_its_parent() {
    let mut state = seeded();
    let page = dom(&get(&mut state, "alice", "/channels/eng"));
    assert_eq!(page.tag(node(&page, "chat-2-reacted-tada")), Some("button"));
    assert_eq!(attr(&page, "chat-2-reacted-tada", "name"), "reaction");
    assert_eq!(attr(&page, "chat-2-reacted-tada", "value"), "tada");
    assert_eq!(
        attr(&page, "chat-2-reacted-tada", "aria-label"),
        "tada · carol"
    );
    assert_eq!(
        attr(&page, "chat-2-reactions", "action"),
        "/channels/eng/messages/chat-2/reactions"
    );
    assert_eq!(attr(&page, "chat-2-reactions", "method"), "post");
    // Pressing the pill the way the browser does: bob joins the reaction it shows.
    let mut request =
        HttpRequest::get("http://chat.internal/channels/eng/messages/chat-2/reactions");
    request.method = "POST".into();
    request.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    request.body = b"reaction=tada".to_vec();
    let r = ChatService
        .handle(&mut state, &ctx("bob"), &request)
        .unwrap();
    assert_eq!(r.status, 200);
    let after = dom(&String::from_utf8(r.body).unwrap());
    assert_eq!(
        attr(&after, "chat-2-reacted-tada", "aria-label"),
        "tada · bob, carol"
    );
    // The reply's quote goes to the parent, which is on the page with that very id.
    assert_eq!(attr(&page, "chat-3-parent", "href"), "#chat-1");
    assert_eq!(page.tag(node(&page, "chat-3-parent")), Some("a"));
    assert!(
        !page.by_id("chat-1").is_empty(),
        "the quoted message is here"
    );
}
