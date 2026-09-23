//! The shipped seeds, exercised through the crate that serves them. A seed that does not load, or
//! a storyline fact that drifts out of the data, fails here rather than in the world build.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::{validate_strict, HTML_MEDIA_TYPE};
use cw_service_geo::{GeoService, GeoState};
use serde_json::Value;
static GOOGLE_MAPS: std::sync::LazyLock<&'static str> =
    std::sync::LazyLock::new(|| cw_service_common::reference::reference_site_json("google-maps"));
static OSM: std::sync::LazyLock<&'static str> =
    std::sync::LazyLock::new(|| cw_service_common::reference::reference_site_json("osm"));
static WEATHER: std::sync::LazyLock<&'static str> =
    std::sync::LazyLock::new(|| cw_service_common::reference::reference_site_json("weather"));
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
/// A page of a shipped site, parsed: it is HTML and passes the engine's strict validator.
struct Dom(cw_web::dom::Document);
fn page(state: &mut Value, actor: &str, url: &str) -> Dom {
    let r = GeoService
        .handle(state, &ctx(actor), &HttpRequest::get(url))
        .unwrap();
    assert_eq!(r.status, 200, "{url}");
    assert_eq!(
        r.headers.get("content-type").map(String::as_str),
        Some(HTML_MEDIA_TYPE),
        "{url}"
    );
    let html = String::from_utf8(r.body).unwrap();
    validate_strict(&html).unwrap_or_else(|e| panic!("{url}: {e:?}"));
    Dom(cw_web::html::parse(&html))
}
impl Dom {
    fn has(&self, id: &str) -> bool {
        !self.0.by_id(id).is_empty()
    }
    fn node(&self, id: &str) -> cw_web::dom::NodeId {
        *self
            .0
            .by_id(id)
            .first()
            .unwrap_or_else(|| panic!("no element #{id}"))
    }
    fn text(&self, id: &str) -> String {
        self.0.text_content(self.node(id))
    }
    fn attr(&self, id: &str, name: &str) -> String {
        self.0
            .attr(self.node(id), name)
            .unwrap_or_default()
            .to_owned()
    }
    fn body_class(&self) -> String {
        let d = &self.0;
        d.descendants(cw_web::dom::Document::ROOT)
            .find(|n| d.is(*n, "body"))
            .and_then(|n| d.attr(n, "class"))
            .unwrap_or_default()
            .to_owned()
    }
}
#[test]
fn the_three_geo_sites_share_one_places_dataset() {
    let places = |raw: &str| site(raw)["initial_state"]["places"].clone();
    assert_eq!(places(*GOOGLE_MAPS), places(*OSM));
    assert_eq!(places(*GOOGLE_MAPS), places(*WEATHER));
    assert!(places(*GOOGLE_MAPS).as_object().unwrap().len() >= 12);
}
#[test]
fn storyline_4_has_the_saved_fourteen_minute_drive_to_devcon() {
    let mut state = load(*GOOGLE_MAPS);
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
    let dir = page(
        &mut state,
        "carol",
        "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=driving",
    );
    assert_eq!(dir.text("dir-min"), "14 min");
    assert_eq!(dir.text("dir-dist"), "3.4 mi");
    assert_eq!(dir.body_class(), "skin-gmaps page-dir");
}
#[test]
fn every_maps_page_the_seed_advertises_resolves() {
    let mut state = load(*GOOGLE_MAPS);
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
        let dom = page(&mut state, "alice", url);
        assert!(
            dom.has("hdr-search") && dom.has("hdr-q") && dom.has("nav-saved"),
            "{url}"
        );
        assert!(dom.body_class().starts_with("skin-gmaps "), "{url}");
    }
    let home = page(&mut state, "alice", "http://maps.google.com/");
    assert_eq!(home.text("wordmark"), "Google Maps");
    assert_eq!(home.attr("chip-0", "href"), "/search?q=airport");
    assert_eq!(
        home.attr("home-tile", "src"),
        "/map.rgba?w=768&h=480&center=northstar-hq&zoom=0"
    );
    let saved = page(&mut state, "carol", "http://maps.google.com/maps/saved");
    let names: Vec<String> = (0..4)
        .map(|i| format!("saved-{i}-name"))
        .filter(|id| saved.has(id))
        .map(|id| saved.text(&id))
        .collect();
    assert!(
        names.iter().any(|n| n == "Cascade Convention Center"),
        "{names:?}"
    );
}
#[test]
fn openstreetmap_reads_the_same_places_in_metric_and_carries_its_notes() {
    let mut state = load(*OSM);
    let dir = page(
        &mut state,
        "bob",
        "http://openstreetmap.org/maps/dir?from=northstar-hq&to=devcon-center&mode=cycling",
    );
    // 5 428 m at 220 m/min is 25 min, and the same distance reads metric here.
    assert_eq!(dir.text("dir-min"), "25 min");
    assert_eq!(dir.text("dir-dist"), "5.4 km");
    assert_eq!(dir.body_class(), "skin-osm page-dir");
    assert_eq!(
        dir.attr("dir-tile", "src"),
        "/map.rgba?w=640&h=480&route=northstar-hq%7Cdevcon-center%7Ccycling&sel=devcon-center"
    );
    let notes = page(&mut state, "bob", "http://openstreetmap.org/maps/notes");
    assert!(notes
        .text("n-0-text")
        .contains("southbound platform entrance"));
    assert_eq!(notes.attr("n-0", "href"), "/maps/place/bayfront-depot");
    for url in [
        "http://openstreetmap.org/",
        "http://openstreetmap.org/maps/saved",
        "http://openstreetmap.org/maps/place/bayfront-depot",
        "http://openstreetmap.org/search?q=market",
    ] {
        assert!(
            page(&mut state, "bob", url)
                .body_class()
                .starts_with("skin-osm "),
            "{url}"
        );
    }
    let depot = page(
        &mut state,
        "bob",
        "http://openstreetmap.org/maps/place/bayfront-depot",
    );
    assert!(depot
        .text("note-0-text")
        .contains("southbound platform entrance"));
    let s: GeoState = serde_json::from_value(state).unwrap();
    assert_eq!(s.notes.len(), 3);
    assert_eq!(s.next_note, 4, "a new note must not reuse a seeded id");
}
#[test]
fn storyline_4_has_rain_in_seattle_on_the_devcon_travel_day() {
    let mut state = load(*WEATHER);
    let today = page(
        &mut state,
        "carol",
        "http://weather.com/weather/today/l/seattle",
    );
    assert_eq!(today.text("title"), "Seattle, WA");
    assert_eq!(today.text("now-meta"), "Rain · 84% humidity · 9 mph wind");
    assert_eq!(today.text("d-0-precip"), "80% rain");
    assert_eq!(today.body_class(), "skin-weather page-today");
    assert_eq!(today.attr("wx-place", "href"), "/maps/place/northstar-hq");
    assert!(today.has("alert-0-ack-go"));
    let s: GeoState = serde_json::from_value(state.clone()).unwrap();
    assert_eq!(s.forecasts["seattle"].days.len(), 10);
    assert_eq!(s.forecasts["seattle"].place, "northstar-hq");
    for city in ["seattle", "portland", "san-jose"] {
        for shape in ["today", "tenday"] {
            let url = format!("http://weather.com/weather/{shape}/l/{city}");
            page(&mut state, "alice", &url);
        }
    }
    // The pages weather shares with maps render in its own skin, without a map to fetch.
    for url in [
        "http://weather.com/",
        "http://weather.com/search?q=seattle",
        "http://weather.com/maps/saved",
        "http://weather.com/maps/notes",
        "http://weather.com/maps/place/northstar-hq",
    ] {
        let dom = page(&mut state, "alice", url);
        assert!(dom.body_class().starts_with("skin-weather "), "{url}");
        assert!(!dom.has("place-tile") && !dom.has("map-tile"), "{url}");
    }
}
