//! The shipped seeds, exercised through the crate that serves them. A seed that does not load, or
//! a storyline fact that drifts out of the data, fails here rather than in the world build.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_geo::{GeoService, GeoState};
use serde_json::Value;
const GOOGLE_MAPS: &str = include_str!("../../../worlds/company-2026/sites/google-maps.json");
const OSM: &str = include_str!("../../../worlds/company-2026/sites/osm.json");
const WEATHER: &str = include_str!("../../../worlds/company-2026/sites/weather.json");
fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 1,
        instance: "geo".into(),
    }
}
fn site(raw: &str) -> Value {
    serde_json::from_str::<Value>(raw).expect("site file must be JSON")
}
fn load(raw: &str) -> Value {
    let doc = site(raw);
    assert_eq!(doc["kind"], "geo");
    GeoService
        .initialize(doc["initial_state"].clone(), &ctx("alice"))
        .expect("seed must load")
}
fn get(state: &mut Value, actor: &str, url: &str) -> (u16, String) {
    let r = GeoService
        .handle(state, &ctx(actor), &HttpRequest::get(url))
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
#[test]
fn the_three_geo_sites_share_one_places_dataset() {
    let places = |raw: &str| site(raw)["initial_state"]["places"].clone();
    assert_eq!(places(GOOGLE_MAPS), places(OSM));
    assert_eq!(places(GOOGLE_MAPS), places(WEATHER));
    assert!(places(GOOGLE_MAPS).as_object().unwrap().len() >= 12);
}
#[test]
fn storyline_4_has_the_saved_fourteen_minute_drive_to_devcon() {
    let mut state = load(GOOGLE_MAPS);
    let seeded: GeoState = serde_json::from_value(state.clone()).unwrap();
    let cached = seeded.routes["northstar-hq|devcon-center|driving"].clone();
    assert_eq!((cached.metres, cached.minutes), (5_428, 14));
    // The seeded cache must be exactly what a cold lookup computes, or the cache is a lie.
    let mut cold = state.clone();
    cold["routes"] = serde_json::json!({});
    let (status, _) = get(
        &mut cold,
        "carol",
        "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=driving",
    );
    assert_eq!(status, 200);
    let recomputed: GeoState = serde_json::from_value(cold).unwrap();
    assert_eq!(
        recomputed.routes["northstar-hq|devcon-center|driving"],
        cached
    );
    let (status, body) = get(
        &mut state,
        "carol",
        "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=driving",
    );
    assert_eq!(status, 200);
    assert!(body.contains("14 min") && body.contains("3.4 mi"), "{body}");
}
#[test]
fn every_maps_page_the_seed_advertises_resolves() {
    let mut state = load(GOOGLE_MAPS);
    for url in [
        "http://maps.google.com/",
        "http://maps.google.com/maps",
        "http://maps.google.com/maps/saved",
        "http://maps.google.com/maps/notes",
        "http://maps.google.com/maps/place/northstar-hq",
        "http://maps.google.com/maps/place/devcon-center",
        "http://maps.google.com/maps/place/harborline-coffee",
        "http://maps.google.com/maps/place/marrow-point-park",
        "http://maps.google.com/maps/place/puget-field",
        "http://maps.google.com/search?q=convention%20center",
    ] {
        assert_eq!(get(&mut state, "alice", url).0, 200, "{url}");
    }
    let (_, body) = get(&mut state, "carol", "http://maps.google.com/maps/saved");
    assert!(body.contains("Cascade Convention Center"), "{body}");
}
#[test]
fn openstreetmap_reads_the_same_places_in_metric_and_carries_its_notes() {
    let mut state = load(OSM);
    let (status, body) = get(
        &mut state,
        "bob",
        "http://openstreetmap.org/maps/dir?from=northstar-hq&to=devcon-center&mode=cycling",
    );
    assert_eq!(status, 200);
    // 5 428 m at 220 m/min is 25 min, and the same distance reads metric here.
    assert!(body.contains("25 min") && body.contains("5.4 km"), "{body}");
    let (status, body) = get(&mut state, "bob", "http://openstreetmap.org/maps/notes");
    assert_eq!(status, 200);
    assert!(body.contains("southbound platform entrance"), "{body}");
    let s: GeoState = serde_json::from_value(state).unwrap();
    assert_eq!(s.notes.len(), 3);
    assert_eq!(s.next_note, 4, "a new note must not reuse a seeded id");
}
#[test]
fn storyline_4_has_rain_in_seattle_on_the_devcon_travel_day() {
    let mut state = load(WEATHER);
    let (status, body) = get(
        &mut state,
        "carol",
        "http://weather.com/weather/today/l/seattle",
    );
    assert_eq!(status, 200);
    assert!(
        body.contains("Seattle, WA") && body.contains("Rain"),
        "{body}"
    );
    assert!(body.contains("80% rain"), "{body}");
    let s: GeoState = serde_json::from_value(state.clone()).unwrap();
    assert_eq!(s.forecasts["seattle"].days.len(), 10);
    assert_eq!(s.forecasts["seattle"].place, "northstar-hq");
    for city in ["seattle", "portland", "san-jose"] {
        for shape in ["today", "tenday"] {
            let url = format!("http://weather.com/weather/{shape}/l/{city}");
            assert_eq!(get(&mut state, "alice", &url).0, 200, "{url}");
        }
    }
    assert_eq!(
        get(&mut state, "alice", "http://weather.com/search?q=seattle").0,
        200
    );
}
