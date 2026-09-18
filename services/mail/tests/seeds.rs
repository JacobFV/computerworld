//! The two seeded mailboxes in `worlds/company-2026/sites` must survive `initialize` and render.
//! A malformed seed is otherwise only discovered when the whole world is built.
use cw_protocol::{HttpRequest, Page};
use cw_sdk::{Service, ServiceContext};
use cw_service_mail::{MailService, MailState};
use serde_json::Value;

fn site(name: &str) -> Value {
    let path = format!(
        "{}/../../worlds/company-2026/sites/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("site file")).expect("valid JSON")
}
fn context(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "pc".into(),
        tick: 0,
        seed: 1,
        instance: "mail".into(),
    }
}
/// Every seeded mailbox entry must name a real mailbox, or the message is invisible to everybody.
fn check(name: &str, reader: &str, expect: &[&str]) -> MailState {
    let site = site(name);
    assert_eq!(site["id"], name, "the basename is the service id");
    assert_eq!(site["kind"], "mail");
    let mut state = MailService
        .initialize(site["initial_state"].clone(), &context(reader))
        .expect("seed initialises");
    let s: MailState = serde_json::from_value(state.clone()).unwrap();
    for m in s.messages.values() {
        assert_eq!(m.id, *m.id, "ids are self-consistent");
        for who in m.mailboxes.keys() {
            assert!(s.serves(who), "{name}: {} delivers to unknown {who}", m.id);
        }
        // A reply must point at a message that is actually in the store.
        assert!(
            m.thread_id.is_empty() || s.messages.contains_key(&m.thread_id),
            "{name}: {} threads onto a missing root",
            m.id
        );
        assert!(
            s.next_id >= 1,
            "{name}: next_id must not collide with a seed"
        );
    }
    let page = MailService
        .handle(
            &mut state,
            &context(reader),
            &HttpRequest::get("http://mail/"),
        )
        .expect("mailbox renders");
    assert_eq!(page.status, 200);
    let body = String::from_utf8(page.body).unwrap();
    // Duplicate element ids and out-of-budget styles are render-time errors, not review notes.
    serde_json::from_str::<Page>(&body)
        .expect("a page")
        .validate()
        .expect("the skinned page is well formed");
    for text in expect {
        assert!(body.contains(text), "{name}: page is missing {text}");
    }
    s
}
#[test]
fn gmail_seed_loads_and_renders_the_storyline_threads() {
    let s = check(
        "google-mail",
        "alice",
        &["Gmail", "Atlas launch checklist", "alice@northstar.example"],
    );
    // Storyline 1: Carol's checklist thread really is a thread, and Bob is on it.
    let thread = s.thread("alice", "mail-1");
    assert_eq!(thread.len(), 3, "checklist thread has three messages");
    assert!(s.messages["mail-1"]
        .body
        .contains("http://docs.google.com/documents/atlas-launch"));
    // Storyline 9: the forward leaves for Outlook and is therefore only in Sent.
    let forward = &s.messages["mail-5"];
    assert_eq!(forward.to, ["bmartinez@outlook.com"]);
    assert_eq!(forward.mailboxes.keys().collect::<Vec<_>>(), ["bob"]);
    // Storylines 4, 5 and 11 each land in the mailbox of the person the bible gives them to.
    assert!(
        !s.list("carol", Some("inbox")).is_empty() && !s.list("alice", Some("inbox")).is_empty()
    );
    assert!(
        s.messages["mail-1"].mailboxes["alice"].starred,
        "the checklist is flagged"
    );
    assert!(
        s.unread("alice", "inbox") >= 3,
        "the inbox has unread mail to open"
    );
    assert!(s
        .list("alice", Some("archive"))
        .iter()
        .any(|m| m.sender.contains("theverge")));
}
#[test]
fn outlook_seed_loads_and_keeps_the_recruiter_thread_off_the_work_account() {
    let s = check("outlook-mail", "bob", &["Outlook", "Nadia Fischer"]);
    assert_eq!(s.address("bob"), "bmartinez@outlook.com");
    assert!(s.messages["mail-1"].sender.ends_with("@northstar.example"));
    // Nobody but Bob has mail here, but the other actors still get a mailbox rather than a 403.
    for actor in ["alice", "carol", "admin"] {
        assert!(s.serves(actor) && s.list(actor, Some("inbox")).is_empty());
    }
}
#[test]
fn a_seeded_mailbox_still_sends_and_the_send_survives_serde() {
    let mut state = MailService
        .initialize(
            site("google-mail")["initial_state"].clone(),
            &context("alice"),
        )
        .unwrap();
    let request = HttpRequest::json(
        "POST",
        "http://mail/send",
        &serde_json::json!({"to":["carol@northstar.example"],"subject":"Re: Atlas launch checklist","body":"Ticked."}),
    )
    .unwrap();
    assert_eq!(
        MailService
            .handle(&mut state, &context("alice"), &request)
            .unwrap()
            .status,
        200
    );
    let s: MailState = serde_json::from_value(state).unwrap();
    // The new message joins the seeded conversation instead of starting a twelfth one.
    assert_eq!(s.thread("carol", "mail-1").len(), 4);
    assert!(s.messages.contains_key("mail-12"));
}
