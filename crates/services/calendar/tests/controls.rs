//! Every control the calendar draws must be able to act.
//!
//! The sweep starts from the pages a reader lands on, follows every same-origin link,
//! probes every form action and `formaction`, and asserts the service answers each with
//! something other than a 404 or a 405 — and that no link leads back to the page it is
//! on. It runs over the seeded world calendar for three readers with different rights
//! (owner, invitee, stranger) and over synthetic states that reach the page kinds the
//! seed does not: a crowded month cell, an event with no guests, an empty calendar.
//! The `plain` skin still answers with `Page` JSON, so its actions are collected from
//! the JSON and probed the same way.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_calendar::{CalendarService, CalendarState, DAY_US, HOUR_US};
use serde_json::Value;

fn context(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "pc".into(),
        tick: HOUR_US,
        seed: 1,
        instance: "calendar".into(),
    }
}
/// The seeded Google Calendar the world ships.
fn seeded() -> Value {
    let path = format!(
        "{}/../../../worlds/internet/sites/google-calendar.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let site: Value = serde_json::from_str(&std::fs::read_to_string(path).expect("site file"))
        .expect("valid JSON");
    CalendarService
        .initialize(site["initial_state"].clone(), &context("carol"))
        .expect("seed initialises")
}
/// One request against a scratch copy of the state: a `POST` probe is a real mutation,
/// and must not disturb the pages the crawl has left to read.
fn call(state: &Value, actor: &str, method: &str, path: &str) -> (u16, String) {
    let mut scratch = state.clone();
    let mut r = HttpRequest::get(format!("http://calendar{path}"));
    r.method = method.to_owned();
    if method != "GET" {
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
    }
    let response = CalendarService
        .handle(&mut scratch, &context(actor), &r)
        .unwrap_or_else(|e| panic!("{method} {path}: {e:?}"));
    (response.status, String::from_utf8(response.body).unwrap())
}
/// Ids whose link may lead back to the page it is on, because the product's does too.
/// Google Calendar's Today button is live on today as well, and takes the reader to the
/// same week it already shows; everything else here must go somewhere.
const SELF_LINKS: &[&str] = &["today"];

fn sweep(state: &Value, actor: &str, seeds: &[&str], limit: usize) -> Vec<String> {
    let mut caller = |method: &str, path: &str| call(state, actor, method, path);
    cw_service_common::audit::Sweep::new(seeds, &mut caller)
        .allow_self(SELF_LINKS)
        // On: the mini-month's day cells are the one place this service dresses an inert
        // element like its controls, and the day on screen now says so with `aria-current`
        // rather than by being drawn as a day you could have clicked.
        .clothes(true)
        .limit(limit)
        .run()
        .into_iter()
        .map(|fault| format!("[{actor}] {fault}"))
        .collect()
}

#[test]
fn the_seeded_calendar_has_no_dead_controls_for_any_reader() {
    let state = seeded();
    let mut faults = Vec::new();
    for actor in ["carol", "alice", "bob", "dave"] {
        faults.extend(sweep(
            &state,
            actor,
            &["/", "/?view=month&day=0", "/?day=12"],
            90,
        ));
    }
    assert!(faults.is_empty(), "dead controls:\n{}", faults.join("\n"));
}

#[test]
fn the_page_kinds_the_seed_does_not_reach_have_no_dead_controls_either() {
    // A crowded month cell (the "n more" link), an event with no guests, an event with
    // neither location nor conference link, and a calendar with nothing in it at all.
    let mut s = CalendarState {
        skin: cw_service_common::Skin("gcal".into()),
        ..Default::default()
    };
    for n in 0..5 {
        s.create(
            "carol",
            &format!("Slot {n}"),
            DAY_US + n * HOUR_US,
            DAY_US + n * HOUR_US + HOUR_US / 2,
            vec!["alice".into()],
            0,
        )
        .unwrap();
    }
    s.create("carol", "Solo", 2 * DAY_US, 2 * DAY_US + HOUR_US, vec![], 0)
        .unwrap();
    s.create(
        "alice",
        "Trip",
        3 * DAY_US,
        5 * DAY_US,
        vec!["carol".into()],
        0,
    )
    .unwrap();
    let crowded = serde_json::to_value(&s).unwrap();
    let mut faults = Vec::new();
    for actor in ["carol", "alice"] {
        faults.extend(sweep(
            &crowded,
            actor,
            &["/?day=1", "/?view=month&day=1", "/?day=3"],
            60,
        ));
    }
    let empty = serde_json::to_value(CalendarState {
        skin: cw_service_common::Skin("gcal".into()),
        ..Default::default()
    })
    .unwrap();
    faults.extend(sweep(&empty, "alice", &["/", "/?view=month&day=0"], 40));
    assert!(faults.is_empty(), "dead controls:\n{}", faults.join("\n"));
}

/// Every page kind the gcal skin has, with the reader each one needs: the week and the
/// month, the first of them and a later one, an event open in the panel and the same
/// event as a permalink, a hidden calendar, a day of all-day bars, a crowded month cell,
/// and a calendar with nothing in it for a reader who is invited to nothing.
fn page_kinds() -> Vec<(Value, &'static str, &'static str)> {
    let seed = seeded();
    let mut crowded = CalendarState {
        skin: cw_service_common::Skin("gcal".into()),
        ..Default::default()
    };
    for n in 0..5 {
        crowded
            .create(
                "carol",
                &format!("Slot {n}"),
                DAY_US + n * HOUR_US,
                DAY_US + n * HOUR_US + HOUR_US / 2,
                vec!["alice".into()],
                0,
            )
            .unwrap();
    }
    crowded
        .create("carol", "Solo", 2 * DAY_US, 2 * DAY_US + HOUR_US, vec![], 0)
        .unwrap();
    let crowded = serde_json::to_value(&crowded).unwrap();
    let empty = serde_json::to_value(CalendarState {
        skin: cw_service_common::Skin("gcal".into()),
        ..Default::default()
    })
    .unwrap();
    let mut out = Vec::new();
    for (actor, path) in [
        ("carol", "/"),
        ("carol", "/?day=7"),
        ("carol", "/?day=4"),
        ("carol", "/?day=0&event=event-1"),
        ("carol", "/events/event-1"),
        ("carol", "/?day=0&hide=carol"),
        ("alice", "/"),
        ("alice", "/?day=0&event=event-1"),
        ("alice", "/events/event-1"),
        ("alice", "/?view=month&day=0"),
        ("alice", "/?view=month&day=14"),
        ("alice", "/?view=month&day=0&event=event-1"),
        ("bob", "/?day=12"),
        ("dave", "/"),
        ("dave", "/?view=month&day=0"),
    ] {
        out.push((seed.clone(), actor, path));
    }
    for (actor, path) in [("carol", "/?view=month&day=1"), ("alice", "/?day=1")] {
        out.push((crowded.clone(), actor, path));
    }
    for path in ["/", "/?view=month&day=0"] {
        out.push((empty.clone(), "alice", path));
    }
    out
}

#[test]
fn every_link_form_and_button_on_every_page_kind_reaches_a_route_that_answers() {
    // Not a crawl: an explicit list of page kinds, so a page kind that stops being
    // rendered fails the test rather than quietly dropping out of the sweep.
    let mut faults: Vec<String> = Vec::new();
    let mut covered: std::collections::BTreeSet<String> = Default::default();
    for (state, actor, path) in page_kinds() {
        let (status, body) = call(&state, actor, "GET", path);
        assert_eq!(status, 200, "[{actor}] GET {path}");
        for fault in cw_service_common::audit::page(&body)
            .into_iter()
            .chain(cw_service_common::audit::clothes(&body))
        {
            faults.push(format!("[{actor}] {path}: {fault}"));
        }
        let doc = cw_web::html::parse(&body);
        for control in cw_service_common::audit::controls(&doc) {
            covered.insert(control.id.clone());
            let target = control.target.trim();
            // A fragment stays on the page and an absolute URL is the world's business.
            if control.method.is_empty()
                || target.is_empty()
                || target.starts_with('#')
                || target.contains("://")
            {
                continue;
            }
            let (status, _) = call(&state, actor, &control.method, target);
            if status == 404 || status == 405 || status >= 500 {
                faults.push(format!(
                    "[{actor}] {path}: #{} sends {} {target}, which answers {status}",
                    control.id, control.method
                ));
            }
            if control.method == "GET"
                && target == path
                && !SELF_LINKS.contains(&control.id.as_str())
            {
                faults.push(format!(
                    "[{actor}] {path}: #{} links to the page it is on",
                    control.id
                ));
            }
        }
    }
    assert!(faults.is_empty(), "dead controls:\n{}", faults.join("\n"));
    // The buttons and forms are covered, not only the links.
    for id in [
        "today",
        "next",
        "prev",
        "view-week",
        "view-month",
        "create",
        "mini-20",
        "calendar-carol",
        "day-0-event-1",
        "day-1-event-1",
        "day-0-head",
        "day-1-more",
        "detail-permalink",
        "detail-back",
        "detail-join",
        "event",
        "event-title",
        "event-start",
        "event-end",
        "event-attendees",
        "event-submit",
        "rsvp",
        "rsvp-yes",
        "rsvp-no",
        "rsvp-maybe",
        "edit",
        "edit-title",
        "edit-start",
        "edit-end",
        "edit-submit",
        "delete-form",
        "delete",
    ] {
        assert!(covered.contains(id), "no page kind drew #{id}");
    }
    // A written address inside a description is a real link out into the world.
    assert!(
        covered.iter().any(|id| id.starts_with("detail-link-")),
        "no page kind drew a description link"
    );
}

#[test]
fn every_action_the_plain_skin_posts_is_a_route_the_service_answers() {
    // `plain` is frozen `Page` JSON, so its controls are read out of the JSON rather
    // than the DOM: every form and every button carries the action it would send.
    let mut s = CalendarState::default();
    s.create(
        "carol",
        "Atlas launch review",
        HOUR_US,
        2 * HOUR_US,
        vec!["alice".into()],
        0,
    )
    .unwrap();
    s.create("alice", "Pairing", DAY_US, DAY_US + HOUR_US, vec![], 0)
        .unwrap();
    let state = serde_json::to_value(&s).unwrap();
    for actor in ["carol", "alice"] {
        let (status, body) = call(&state, actor, "GET", "/");
        assert_eq!(status, 200, "{actor}");
        let page: Value = serde_json::from_str(&body).expect("plain answers Page JSON");
        let mut actions = Vec::new();
        collect(&page["elements"], &mut actions);
        assert!(!actions.is_empty(), "{actor} sees no controls at all");
        for (method, url) in actions {
            let (status, body) = call(&state, actor, &method, &url);
            assert!(
                status != 404 && status != 405 && status < 500,
                "[{actor}] {method} {url} answers {status}: {body}"
            );
        }
    }
}
/// Every `PageAction` in a `Page`, depth first.
fn collect(elements: &Value, out: &mut Vec<(String, String)>) {
    for e in elements.as_array().into_iter().flatten() {
        if let Some(action) = e.get("action") {
            let method = action["method"].as_str().unwrap_or("GET").to_owned();
            let url = action["url"].as_str().unwrap_or_default().to_owned();
            if !url.is_empty() {
                out.push((method, url));
            }
        }
        collect(&e["children"], out);
    }
}
