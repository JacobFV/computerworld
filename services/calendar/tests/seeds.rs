//! The seeded Google Calendar in `worlds/company-2026/sites` must survive `initialize`, render,
//! and — the bug this file exists to catch — carry times in microseconds rather than milliseconds.
use cw_protocol::{HttpRequest, Page};
use cw_sdk::{Service, ServiceContext};
use cw_service_calendar::{civil, CalendarService, CalendarState, DAY_US, HOUR_US};
use serde_json::Value;

fn site() -> Value {
    let path = format!(
        "{}/../../worlds/company-2026/sites/google-calendar.json",
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
        instance: "calendar".into(),
    }
}
fn state() -> Value {
    let site = site();
    assert_eq!(site["id"], "google-calendar");
    assert_eq!(site["kind"], "calendar");
    CalendarService
        .initialize(site["initial_state"].clone(), &context("carol"))
        .expect("seed initialises")
}
#[test]
fn seeded_events_are_spread_across_real_days_not_clustered_in_one_minute() {
    let s: CalendarState = serde_json::from_value(state()).unwrap();
    let mut days: Vec<u64> = s.events.values().map(|e| e.start / DAY_US).collect();
    days.sort_unstable();
    days.dedup();
    assert!(
        days.len() >= 3,
        "a month view must show more than one cluster"
    );
    for e in s.events.values() {
        // A millisecond seed puts every event inside the first four seconds of the epoch.
        assert!(
            e.start >= HOUR_US || e.start == 0,
            "{}: start {} is below one hour, which is the millisecond bug",
            e.id,
            e.start
        );
        assert!(e.end > e.start && e.end - e.start >= HOUR_US / 4);
        let at = civil(e.start);
        assert!(
            (7..=20).contains(&at.hour),
            "{}: {} is not a work hour",
            e.id,
            at.clock()
        );
    }
    // Storyline 1: the launch review is the Thursday afternoon slot the mail thread refers to.
    let review = &s.events["event-1"];
    assert_eq!(civil(review.start).clock(), "14:00");
    assert_eq!(review.conference_url, "http://slack.com/archives/eng");
    assert!(review
        .description
        .contains("http://docs.google.com/documents/atlas-launch"));
    assert_eq!(review.attendees.len(), 2, "alice and bob are invited");
    // Storyline 4: DevCon is the two-day trip the Ticketmaster mail confirms.
    assert_eq!(s.events["event-3"].days().count(), 2);
    assert!(s.events["event-3"].description.contains("TM-2210"));
}
#[test]
fn the_seeded_week_renders_and_its_rsvp_buttons_really_answer() {
    let mut v = state();
    let page = |v: &mut Value, actor: &str, url: &str| {
        let r = HttpRequest::get(url);
        let response = CalendarService.handle(v, &context(actor), &r).unwrap();
        (response.status, String::from_utf8(response.body).unwrap())
    };
    let (status, body) = page(&mut v, "carol", "http://calendar/?day=0&event=event-1");
    assert_eq!(status, 200);
    // Duplicate element ids and out-of-budget styles are render-time errors, not review notes.
    serde_json::from_str::<Page>(&body)
        .expect("a page")
        .validate()
        .expect("the week grid is well formed");
    assert!(body.contains("Atlas launch review") && body.contains("Thu 17 Sep"));
    assert!(
        body.contains("DevCon Seattle 2026"),
        "the trip is in the same week"
    );
    // Bob has not answered yet; the page says so, and the button changes it.
    assert!(body.contains("Awaiting reply"));
    let rsvp = HttpRequest::json(
        "POST",
        "http://calendar/events/event-1/rsvp",
        &serde_json::json!({"response": "accepted"}),
    )
    .unwrap();
    let answered = CalendarService
        .handle(&mut v, &context("bob"), &rsvp)
        .unwrap();
    assert_eq!(answered.status, 200);
    assert_eq!(v["events"]["event-1"]["attendees"]["bob"], "accepted");
    assert!(String::from_utf8(answered.body).unwrap().contains("Going"));
    // A week with no events still renders, and the retro is where the next-week link leads.
    assert!(page(&mut v, "carol", "http://calendar/?day=7")
        .1
        .contains("Atlas retro"));
    assert!(!page(&mut v, "carol", "http://calendar/?day=14")
        .1
        .contains("Atlas"));
}
#[test]
fn a_seeded_calendar_still_creates_and_the_new_event_lands_in_its_own_day() {
    let mut v = state();
    let request = HttpRequest::json(
        "POST",
        "http://calendar/api/events",
        &serde_json::json!({
            "title": "Atlas launch",
            "start": 2 * DAY_US + 5 * HOUR_US,
            "end": 2 * DAY_US + 6 * HOUR_US,
            "attendees": ["alice", "bob"]
        }),
    )
    .unwrap();
    assert_eq!(
        CalendarService
            .handle(&mut v, &context("carol"), &request)
            .unwrap()
            .status,
        200
    );
    let s: CalendarState = serde_json::from_value(v).unwrap();
    let created = &s.events["event-5"];
    assert_eq!(
        created.start / DAY_US,
        2,
        "the new event owns Saturday, not the epoch minute"
    );
    assert_eq!(s.on_day("alice", 2).len(), 1);
}
