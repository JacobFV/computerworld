//! The three webmails served as HTML by the mail service and driven end to end through the
//! agent API: Gmail as alice, Outlook as bob, mail.com as carol. Each is rendered by the web
//! engine; the semantic observation lists the folders, the rows, the search box and the forms
//! by the ids the service documents; opening a row shows the conversation, the reply form
//! sends real mail, the metadata buttons mutate the mailbox, and Compose delivers to the
//! other person's inbox.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

struct Desk {
    world: World,
    session: String,
    machine: &'static str,
}
impl Desk {
    fn new(actor: &str, machine: &'static str) -> Desk {
        let mut definition = reference_world();
        // mail.com wears its own skin in `sites/mail-com.json`; a world file generated before
        // that still says `plain`, which is the Page rendering this test is not about.
        for service in &mut definition.services {
            if service.id == "mail-com" && service.initial_state["skin"] == "plain" {
                service.initial_state["skin"] = json!("mailcom");
            }
        }
        let mut world = World::new(definition, 42).unwrap();
        let session = world
            .environment(EnvironmentConfig {
                actor: actor.into(),
                machines: vec![machine.into()],
                actions: vec!["browser.v1".into(), "keyboard.v1".into()],
                observations: vec!["semantic.v1".into(), "browser.v1".into()],
                action_budget: 1 << 20,
            })
            .unwrap();
        Desk {
            world,
            session,
            machine,
        }
    }
    fn act(&mut self, channel: &str, op: &str, payload: Value) -> Value {
        let result = self
            .world
            .step(
                &self.session,
                vec![ActionEnvelope::new(channel, op, self.machine, payload)],
            )
            .unwrap();
        assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
        result.outcomes[0].value.clone()
    }
    fn go(&mut self, url: &str) {
        self.act("browser.v1", "navigate", json!({"url": url}));
    }
    fn click(&mut self, id: &str) {
        self.act("browser.v1", "click", json!({"id": id}));
    }
    fn fill(&mut self, id: &str, value: &str) {
        self.act("browser.v1", "fill", json!({"id": id, "value": value}));
    }
    fn url(&self) -> String {
        self.world.observe(&self.session).unwrap().channels["browser.v1"][self.machine]["url"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    fn title(&self) -> String {
        self.world.observe(&self.session).unwrap().channels["semantic.v1"][self.machine]["title"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    /// Every element of the semantic tree, flattened.
    fn elements(&self) -> Vec<Value> {
        fn walk(elements: &[Value], out: &mut Vec<Value>) {
            for e in elements {
                out.push(e.clone());
                if let Some(children) = e["children"].as_array() {
                    walk(children, out);
                }
            }
        }
        let page = self.world.observe(&self.session).unwrap().channels["semantic.v1"][self.machine]
            .clone();
        let mut out = Vec::new();
        walk(page["elements"].as_array().unwrap(), &mut out);
        out
    }
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn has(all: &[Value], id: &str) -> bool {
    all.iter().any(|e| e["id"] == id)
}
fn says(all: &[Value], text: &str) -> bool {
    all.iter()
        .any(|e| e["text"].as_str().is_some_and(|t| t.contains(text)))
}

#[test]
fn gmail_is_read_replied_to_starred_and_searched_through_the_agent_api() {
    let mut desk = Desk::new("alice", "alice-mac");
    desk.go("http://mail.google.com/");
    assert_eq!(desk.title(), "Gmail");
    let all = desk.elements();
    // The semantic tree lists the rail, the search box and the rows by their ids.
    assert_eq!(by_id(&all, "compose")["kind"], "link");
    assert_eq!(
        by_id(&all, "compose")["url"],
        "http://mail.google.com/?folder=inbox&compose=1"
    );
    assert_eq!(
        by_id(&all, "folder-sent")["url"],
        "http://mail.google.com/?folder=sent"
    );
    assert_eq!(by_id(&all, "search")["kind"], "form");
    assert_eq!(by_id(&all, "search-q")["kind"], "input");
    assert_eq!(by_id(&all, "search-q")["label"], "Search mail");
    assert_eq!(by_id(&all, "search-submit")["kind"], "button");
    let row = by_id(&all, "row-mail-3");
    assert_eq!(row["kind"], "link");
    assert!(
        row["text"]
            .as_str()
            .unwrap()
            .contains("Atlas launch checklist"),
        "{row:?}"
    );

    // Open the checklist conversation: three messages, the doc link is a real link.
    desk.click("row-mail-3");
    assert_eq!(
        desk.url(),
        "http://mail.google.com/?folder=inbox&thread=mail-1"
    );
    let all = desk.elements();
    assert!(says(&all, "3 in thread"));
    let doc = all
        .iter()
        .find(|e| {
            e["kind"] == "link" && e["url"] == "http://docs.google.com/documents/atlas-launch"
        })
        .expect("the doc link in the prose");
    assert!(doc["id"].as_str().unwrap().starts_with("read-mail-1-link-"));
    assert_eq!(by_id(&all, "reply-to")["value"], "bob@northstar.example");
    assert_eq!(
        by_id(&all, "reply-subject")["value"],
        "Re: Atlas launch checklist"
    );

    // Reply through the form: the conversation grows and the message is really in the store.
    desk.fill("reply-body", "Ticked my boxes. ATLAS-REPLY-77");
    desk.click("reply-submit");
    let all = desk.elements();
    assert!(
        says(&all, "4 in thread"),
        "the reply joined the conversation"
    );
    assert!(says(&all, "ATLAS-REPLY-77"));

    // The star button is one of three submitters of the message's form.
    desk.go("http://mail.google.com/?folder=inbox&thread=mail-1");
    let before = by_id(&desk.elements(), "read-mail-3-star")["text"].clone();
    assert_eq!(before, "Star");
    desk.click("read-mail-3-star");
    assert_eq!(
        by_id(&desk.elements(), "read-mail-3-star")["text"],
        "Unstar"
    );
    desk.go("http://mail.google.com/?folder=starred");
    assert!(has(&desk.elements(), "row-mail-3"));

    // Search narrows the list and stays until cleared.
    desk.go("http://mail.google.com/");
    desk.fill("search-q", "Amazon");
    desk.click("search-submit");
    let all = desk.elements();
    assert!(!has(&all, "row-mail-3"));
    assert!(all
        .iter()
        .any(|e| e["kind"] == "link" && e["text"].as_str().is_some_and(|t| t.contains("Amazon"))));
    desk.click("search-clear");
    assert!(has(&desk.elements(), "row-mail-3"));

    // Compose a new message to Bob; it lands in Sent and opens as its own conversation.
    desk.click("compose");
    assert_eq!(desk.url(), "http://mail.google.com/?folder=inbox&compose=1");
    desk.fill("new-to", "bob@northstar.example");
    desk.fill("new-subject", "Lunch on Thursday?");
    desk.fill("new-body", "Noodles.\nMy treat.");
    desk.click("new-submit");
    let all = desk.elements();
    assert!(says(&all, "Lunch on Thursday?") && says(&all, "1 in thread"));
    desk.go("http://mail.google.com/?folder=sent");
    assert!(desk.elements().iter().any(|e| e["kind"] == "link"
        && e["text"]
            .as_str()
            .is_some_and(|t| t.contains("Lunch on Thursday?"))));
}

#[test]
fn outlook_shows_three_panes_and_archives_through_the_agent_api() {
    let mut desk = Desk::new("bob", "bob-windows");
    desk.go("http://outlook.com/");
    assert_eq!(desk.title(), "Mail - Outlook");
    let all = desk.elements();
    assert_eq!(by_id(&all, "compose")["kind"], "link");
    assert!(by_id(&all, "compose")["text"]
        .as_str()
        .unwrap()
        .contains("New mail"));
    assert!(by_id(&all, "folder-sent")["text"]
        .as_str()
        .unwrap()
        .contains("Sent Items"));
    assert!(
        says(&all, "Select a conversation"),
        "the reading pane is there and empty"
    );
    let first = all
        .iter()
        .find(|e| {
            e["kind"] == "link"
                && e["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("row-mail-"))
        })
        .expect("a message row")
        .clone();
    let row = first["id"].as_str().unwrap().to_owned();
    let message = row.trim_start_matches("row-").to_owned();
    desk.click(&row);
    assert!(desk.url().contains("thread="), "{}", desk.url());
    let all = desk.elements();
    assert!(has(&all, "reply") && has(&all, &format!("read-{message}-archive")));
    // Label it, then archive it: it leaves the inbox and is in Archive.
    desk.fill(&format!("read-{message}-label-label"), "Keep");
    desk.click(&format!("read-{message}-label-submit"));
    assert!(says(&desk.elements(), "Keep"));
    desk.click(&format!("read-{message}-archive"));
    desk.go("http://outlook.com/?folder=inbox");
    assert!(!has(&desk.elements(), &row));
    desk.go("http://outlook.com/?folder=archive");
    assert!(has(&desk.elements(), &row));
}

#[test]
fn mail_com_wears_its_own_skin_and_sends_through_the_agent_api() {
    let mut desk = Desk::new("carol", "carol-ubuntu");
    desk.go("http://mail.com/");
    assert_eq!(desk.title(), "mail.com - Inbox");
    let all = desk.elements();
    assert!(by_id(&all, "compose")["text"]
        .as_str()
        .unwrap()
        .contains("Compose E-mail"));
    assert!(says(&all, "carol.nakamura@mail.com"));
    desk.click("row-mail-2");
    let all = desk.elements();
    assert!(says(&all, "2 in thread"));
    desk.fill("reply-body", "See you at check-in. DEVCON-ACK");
    desk.click("reply-submit");
    let all = desk.elements();
    assert!(says(&all, "3 in thread") && says(&all, "DEVCON-ACK"));
    desk.click("folder-sent");
    assert_eq!(desk.url(), "http://mail.com/?folder=sent");
    assert!(desk.elements().iter().any(|e| e["kind"] == "link"
        && e["text"]
            .as_str()
            .is_some_and(|t| t.contains("DevCon Seattle"))));
}
