//! calendar.google.com served as HTML by the calendar service and driven end to end through
//! the agent API: the browser renders the week grid with the web engine, the semantic
//! observation lists the chips, the navigation and the forms by the ids the service documents,
//! an event chip opens the side panel, an RSVP button answers, the new-event form creates an
//! event that lands in the grid, and its owner deletes it again.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";
const HOUR_US: u64 = 3_600_000_000;
const DAY_US: u64 = 24 * HOUR_US;

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
fn act(world: &mut World, session: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(session, vec![ActionEnvelope::new("browser.v1", op, MACHINE, payload)])
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"].as_str().unwrap().to_owned()
}
/// Every element of the semantic tree, flattened.
fn elements(world: &World, session: &str) -> (Value, Vec<Value>) {
    fn walk(elements: &[Value], out: &mut Vec<Value>) {
        for e in elements {
            out.push(e.clone());
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let page = world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone();
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    (page, out)
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter().find(|e| e["id"] == id).unwrap_or_else(|| panic!("no element {id} in {all:?}"))
}
fn says(all: &[Value], text: &str) -> bool {
    all.iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains(text)))
}

#[test]
fn google_calendar_is_browsed_answered_and_edited_through_the_agent_api() {
    let (mut world, session) = world();
    act(&mut world, &session, "navigate", json!({"url":"http://calendar.google.com/"}));
    let (page, all) = elements(&world, &session);
    assert_eq!(page["title"], "Google Calendar");
    // Navigation, the grid's chips and the new-event form are all there under their ids.
    for (id, href) in [("today", "/?day=0"), ("prev", "/?day=0"), ("next", "/?day=7"), ("create", "/?day=0"), ("view-month", "/?view=month&day=0")] {
        let e = by_id(&all, id);
        assert_eq!(e["kind"], "link", "{id}");
        assert_eq!(e["url"], format!("http://calendar.google.com{href}"), "{id}");
    }
    assert_eq!(by_id(&all, "event")["kind"], "form");
    let title = by_id(&all, "event-title");
    assert_eq!((title["kind"].as_str(), title["label"].as_str()), (Some("input"), Some("Title")));
    assert_eq!(by_id(&all, "event-start")["value"], HOUR_US.to_string());
    assert_eq!(by_id(&all, "event-submit")["kind"], "button");
    let chip = by_id(&all, "day-0-event-1");
    assert_eq!(chip["kind"], "link");
    assert!(chip["text"].as_str().unwrap().contains("Atlas launch review"), "{chip:?}");
    assert!(chip["text"].as_str().unwrap().contains("14:00"), "{chip:?}");

    // The chip opens the event in the side panel; the page is a permalink.
    act(&mut world, &session, "click", json!({"id":"day-0-event-1"}));
    assert_eq!(url(&world, &session), "http://calendar.google.com/?day=0&event=event-1");
    let (_, all) = elements(&world, &session);
    assert!(says(&all, "Thu 17 Sep · 14:00 – 15:00"));
    assert_eq!(by_id(&all, "detail-join")["url"], "http://slack.com/archives/eng");
    assert!(all.iter().any(|e| e["kind"] == "link" && e["url"] == "http://docs.google.com/documents/atlas-launch"));
    assert_eq!(by_id(&all, "rsvp")["kind"], "form");
    assert_eq!(by_id(&all, "rsvp-maybe")["text"], "Maybe");
    assert!(!all.iter().any(|e| e["id"] == "edit"), "carol owns the review, not alice");

    // Alice had accepted; Maybe is one click, and the guest list says so on the page it returns.
    act(&mut world, &session, "click", json!({"id":"rsvp-maybe"}));
    assert_eq!(url(&world, &session), "http://calendar.google.com/events/event-1/rsvp");
    let (_, all) = elements(&world, &session);
    assert!(says(&all, "Maybe"));
    assert_eq!(by_id(&all, "detail-back")["url"], "http://calendar.google.com/?day=0");

    // Next week and back again are plain links.
    act(&mut world, &session, "click", json!({"id":"next"}));
    assert_eq!(url(&world, &session), "http://calendar.google.com/?day=7");
    let (_, all) = elements(&world, &session);
    assert!(all.iter().any(|e| e["id"] == "day-8-event-4"), "the retro is on Friday the 25th");
    act(&mut world, &session, "click", json!({"id":"prev"}));
    assert_eq!(url(&world, &session), "http://calendar.google.com/?day=0");

    // The new-event form posts the same fields the Page form did.
    let (start, end) = (2 * DAY_US + HOUR_US, 2 * DAY_US + 2 * HOUR_US);
    act(&mut world, &session, "fill", json!({"id":"event-title","value":"Pairing on the renderer"}));
    act(&mut world, &session, "fill", json!({"id":"event-start","value":start.to_string()}));
    act(&mut world, &session, "fill", json!({"id":"event-end","value":end.to_string()}));
    act(&mut world, &session, "fill", json!({"id":"event-attendees","value":"bob"}));
    act(&mut world, &session, "click", json!({"id":"event-submit"}));
    assert_eq!(url(&world, &session), "http://calendar.google.com/events");
    let (_, all) = elements(&world, &session);
    let created = all
        .iter()
        .find(|e| e["kind"] == "link" && e["text"].as_str().is_some_and(|t| t.contains("Pairing on the renderer")))
        .expect("the new event is a chip in the grid");
    let chip_id = created["id"].as_str().unwrap().to_owned();
    assert!(chip_id.starts_with("day-2-event-"), "{chip_id}");
    assert_eq!(by_id(&all, "edit-title")["value"], "Pairing on the renderer");

    // Its owner renames it through the edit form, then deletes it.
    act(&mut world, &session, "fill", json!({"id":"edit-title","value":"Pairing (moved)"}));
    act(&mut world, &session, "submit", json!({"id":"edit"}));
    let (_, all) = elements(&world, &session);
    assert!(by_id(&all, &chip_id)["text"].as_str().unwrap().contains("Pairing (moved)"));
    act(&mut world, &session, "click", json!({"id":"delete"}));
    let (_, all) = elements(&world, &session);
    assert!(!all.iter().any(|e| e["id"] == chip_id.as_str()));
    assert_eq!(by_id(&all, "event")["kind"], "form");

    // The month view is the same calendar in a seven-column grid.
    act(&mut world, &session, "navigate", json!({"url":"http://calendar.google.com/?view=month&day=0"}));
    let (_, all) = elements(&world, &session);
    assert_eq!(by_id(&all, "day-0-event-1")["url"], "http://calendar.google.com/?view=month&day=0&event=event-1");
    assert_eq!(by_id(&all, "next")["url"], "http://calendar.google.com/?view=month&day=14");
}
