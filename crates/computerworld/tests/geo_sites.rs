//! maps.google.com, openstreetmap.org and weather.com served as HTML by the geo service
//! and driven through the agent API: the browser renders each with the web engine, the
//! semantic observation lists the search box, the forms and the links by the ids the
//! service documents, and the main flow of each site works by id: search a place and open
//! it, ask for directions and switch the travel mode, save a place, leave a map note,
//! read a forecast, switch units and acknowledge an alert.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";

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
fn act(world: &mut World, session: &str, channel: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new(channel, op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn navigate(world: &mut World, session: &str, url: &str) {
    act(
        world,
        session,
        "browser.v1",
        "navigate",
        json!({ "url": url }),
    );
}
fn click(world: &mut World, session: &str, id: &str) {
    act(world, session, "browser.v1", "click", json!({ "id": id }));
}
fn fill(world: &mut World, session: &str, id: &str, value: &str) {
    act(
        world,
        session,
        "browser.v1",
        "fill",
        json!({ "id": id, "value": value }),
    );
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"]
        .as_str()
        .unwrap()
        .to_owned()
}
/// Every element of the semantic tree, flattened.
fn elements(page: &Value) -> Vec<Value> {
    fn walk(elements: &[Value], out: &mut Vec<Value>) {
        for e in elements {
            out.push(e.clone());
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    out
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no element {id} in {all:?}"))
}
fn has_id(all: &[Value], id: &str) -> bool {
    all.iter().any(|e| e["id"] == id)
}
/// Whether any element's text or label carries `needle`.
fn says(all: &[Value], needle: &str) -> bool {
    all.iter().any(|e| {
        ["text", "label", "value"]
            .iter()
            .any(|k| e[*k].as_str().is_some_and(|t| t.contains(needle)))
    })
}

#[test]
fn google_maps_finds_a_place_gets_directions_and_saves_it() {
    let (mut world, session) = world();
    navigate(&mut world, &session, "http://maps.google.com/");
    let home = page(&world, &session);
    assert_eq!(home["title"], "Google Maps");
    let all = elements(&home);
    assert_eq!(by_id(&all, "hdr-search")["kind"], "form");
    let q = by_id(&all, "hdr-q");
    assert_eq!(q["kind"], "input");
    assert_eq!(q["label"], "Search places");
    assert_eq!(by_id(&all, "hdr-search-go")["kind"], "button");
    assert_eq!(
        by_id(&all, "nav-saved")["url"],
        "http://maps.google.com/maps/saved"
    );
    assert_eq!(
        by_id(&all, "nav-notes")["url"],
        "http://maps.google.com/maps/notes"
    );
    assert_eq!(by_id(&all, "dir-form")["kind"], "form");
    for id in ["dir-from", "dir-to", "dir-mode"] {
        assert_eq!(by_id(&all, id)["kind"], "input", "{id}");
    }
    assert_eq!(by_id(&all, "dir-mode")["value"], "driving");
    assert_eq!(by_id(&all, "dir-form-go")["text"], "Directions");
    // alice's saved places and the nearby list are cards that are one link each.
    assert_eq!(
        by_id(&all, "home-saved-0")["url"],
        "http://maps.google.com/maps/place/northstar-hq"
    );
    assert_eq!(by_id(&all, "home-place-0")["kind"], "link");

    // Search from the floating box: a GET, so the URL is the query.
    fill(&mut world, &session, "hdr-q", "coffee");
    click(&mut world, &session, "hdr-search-go");
    assert_eq!(
        url(&world, &session),
        "http://maps.google.com/search?q=coffee"
    );
    let all = elements(&page(&world, &session));
    assert_eq!(by_id(&all, "hdr-q")["value"], "coffee");
    let first = by_id(&all, "r-0");
    assert_eq!(first["kind"], "link");
    assert_eq!(
        first["url"],
        "http://maps.google.com/maps/place/harborline-coffee"
    );
    assert!(
        first["text"]
            .as_str()
            .unwrap()
            .contains("Harborline Coffee"),
        "{first:?}"
    );

    // Open the place and ask for directions from it.
    click(&mut world, &session, "r-0");
    assert_eq!(
        url(&world, &session),
        "http://maps.google.com/maps/place/harborline-coffee"
    );
    let place = page(&world, &session);
    assert_eq!(place["title"], "Harborline Coffee");
    let all = elements(&place);
    assert!(says(&all, "128 Bayfront Ave, Seattle WA"));
    assert_eq!(by_id(&all, "place-dir-from")["value"], "harborline-coffee");
    // alice already keeps this cafe in Your places, so the button offers to remove it.
    assert_eq!(
        by_id(&all, "save-form-go")["text"],
        "Remove from Your places"
    );
    fill(&mut world, &session, "place-dir-to", "devcon-center");
    click(&mut world, &session, "place-dir-go");
    assert_eq!(
        url(&world, &session),
        "http://maps.google.com/maps/dir?from=harborline-coffee&to=devcon-center&mode=driving"
    );
    let all = elements(&page(&world, &session));
    assert!(says(&all, "Harborline Coffee to Cascade Convention Center"));
    assert!(says(&all, "Arrive at Cascade Convention Center"));
    let walk = by_id(&all, "mode-walking");
    assert_eq!(walk["kind"], "link");
    click(&mut world, &session, "mode-walking");
    assert_eq!(
        url(&world, &session),
        "http://maps.google.com/maps/dir?from=harborline-coffee&to=devcon-center&mode=walking"
    );
    assert!(says(&elements(&page(&world, &session)), "walking"));
    click(&mut world, &session, "dir-to-place");
    assert_eq!(
        url(&world, &session),
        "http://maps.google.com/maps/place/devcon-center"
    );

    // Saving is a one-button POST form; the page that comes back shows the new state.
    click(&mut world, &session, "save-form-go");
    let all = elements(&page(&world, &session));
    assert_eq!(
        by_id(&all, "save-form-go")["text"],
        "Remove from Your places"
    );
    navigate(&mut world, &session, "http://maps.google.com/maps/saved");
    let all = elements(&page(&world, &session));
    assert!(says(&all, "Cascade Convention Center"));
}

#[test]
fn openstreetmap_reads_the_notes_and_adds_one() {
    let (mut world, session) = world();
    navigate(&mut world, &session, "http://openstreetmap.org/");
    let home = page(&world, &session);
    assert_eq!(home["title"], "OpenStreetMap");
    let all = elements(&home);
    assert_eq!(by_id(&all, "hdr-q")["kind"], "input");
    assert_eq!(by_id(&all, "nav-notes")["kind"], "link");
    click(&mut world, &session, "nav-notes");
    assert_eq!(url(&world, &session), "http://openstreetmap.org/maps/notes");
    let all = elements(&page(&world, &session));
    let note = by_id(&all, "n-0");
    assert_eq!(
        note["url"],
        "http://openstreetmap.org/maps/place/bayfront-depot"
    );
    assert!(
        note["text"]
            .as_str()
            .unwrap()
            .contains("southbound platform entrance"),
        "{note:?}"
    );
    click(&mut world, &session, "n-0");
    let all = elements(&page(&world, &session));
    assert_eq!(by_id(&all, "note-place")["value"], "bayfront-depot");
    fill(
        &mut world,
        &session,
        "note-text",
        "Bike racks moved to the east entrance.",
    );
    click(&mut world, &session, "note-form-go");
    let all = elements(&page(&world, &session));
    assert!(says(&all, "Bike racks moved to the east entrance."));
    // Directions read metric here.
    navigate(
        &mut world,
        &session,
        "http://openstreetmap.org/maps/dir?from=northstar-hq&to=devcon-center&mode=cycling",
    );
    let all = elements(&page(&world, &session));
    assert!(says(&all, "25 min") && says(&all, "5.4 km"));
}

#[test]
fn weather_reads_the_forecast_switches_units_and_acknowledges_the_alert() {
    let (mut world, session) = world();
    navigate(&mut world, &session, "http://weather.com/");
    let home = page(&world, &session);
    assert_eq!(home["title"], "Seattle, WA weather");
    let all = elements(&home);
    assert_eq!(by_id(&all, "hdr-q")["label"], "Search a city");
    assert_eq!(
        by_id(&all, "nav-tenday")["url"],
        "http://weather.com/weather/tenday/l/seattle"
    );
    assert_eq!(by_id(&all, "units-value")["value"], "f");
    assert_eq!(by_id(&all, "loc-city")["value"], "seattle");
    assert!(says(&all, "57°") && says(&all, "80% rain"));
    assert!(says(&all, "Rain likely through Monday evening"));
    assert_eq!(by_id(&all, "alert-0-ack-go")["text"], "Got it");

    click(&mut world, &session, "wx-ten");
    assert_eq!(
        url(&world, &session),
        "http://weather.com/weather/tenday/l/seattle"
    );
    let all = elements(&page(&world, &session));
    assert!(says(&all, "10 day forecast") && says(&all, "Partly cloudy"));

    // Celsius: (57 - 32) * 5 / 9 is 13.
    fill(&mut world, &session, "units-value", "c");
    click(&mut world, &session, "units-form-go");
    let all = elements(&page(&world, &session));
    assert_eq!(by_id(&all, "units-value")["value"], "c");
    assert!(says(&all, "13°"), "{all:?}");

    click(&mut world, &session, "alert-0-ack-go");
    let all = elements(&page(&world, &session));
    assert!(!has_id(&all, "alert-0-ack-go"));
    assert!(!says(&all, "Rain likely through Monday evening"));

    // Search a city and open it.
    fill(&mut world, &session, "hdr-q", "portland");
    click(&mut world, &session, "hdr-search-go");
    assert_eq!(
        url(&world, &session),
        "http://weather.com/search?q=portland"
    );
    let all = elements(&page(&world, &session));
    assert_eq!(
        by_id(&all, "r-0")["url"],
        "http://weather.com/weather/today/l/portland"
    );
    click(&mut world, &session, "r-0");
    assert_eq!(page(&world, &session)["title"], "Portland, OR weather");
}
