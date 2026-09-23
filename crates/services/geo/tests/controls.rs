//! No dead controls on maps.google.com, openstreetmap.org or weather.com. Every skin and
//! every page kind is crawled from a handful of seeds: every link, button and form on each
//! page is followed against the service's own router, and every per-page lie the auditor
//! knows about — an anchor with nowhere to go, a dangling `#fragment`, a button in no form,
//! a field with no name or no label, an inert element drawn with `cursor: pointer` or one
//! that lights up under it — fails here rather than in front of an agent.
//!
//! A tab, chip or travel mode that leads back to the page it sits on is allowed only because
//! it says so with `aria-current="page"`; the sweep reads that marker off the page itself, so
//! nothing here has to be taken on trust.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::audit;
use cw_service_geo::GeoService;
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
fn load(raw: &str) -> Value {
    let doc: Value = serde_json::from_str(raw).expect("site file must be JSON");
    assert_eq!(doc["kind"], "geo");
    GeoService
        .initialize(doc["initial_state"].clone(), &ctx("alice"))
        .expect("seed must load")
}
/// Crawls one shipped site as one person. A `POST` probe is a real write, so it runs
/// against a scratch copy of the state and the crawl keeps reading the untouched one.
fn sweep(raw: &str, host: &str, actor: &str, seeds: &[&str]) -> Vec<String> {
    let mut state = load(raw);
    let mut call = |method: &str, path: &str| {
        let mut r = HttpRequest::get(format!("http://{host}{path}"));
        let response = if method.eq_ignore_ascii_case("POST") {
            r.method = "POST".into();
            r.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
            let mut scratch = state.clone();
            GeoService.handle(&mut scratch, &ctx(actor), &r)
        } else {
            GeoService.handle(&mut state, &ctx(actor), &r)
        };
        let response = response.unwrap_or_else(|e| panic!("{method} {path}: {e}"));
        (
            response.status,
            String::from_utf8_lossy(&response.body).into_owned(),
        )
    };
    // No `allow_self` list: the pages earn that exemption themselves, by marking the tab,
    // chip or travel mode that leads back to where you are with `aria-current="page"`.
    audit::Sweep::new(seeds, &mut call).limit(5000).run()
}
fn report(site: &str, actor: &str, faults: &[String]) -> String {
    if faults.is_empty() {
        return String::new();
    }
    format!("{site} as {actor}:\n  {}\n", faults.join("\n  "))
}

#[test]
fn google_maps_has_no_dead_control_on_any_page() {
    let seeds = [
        "/",
        "/maps",
        "/maps/saved",
        "/maps/notes",
        "/search?q=coffee",
        "/search?q=nothing-matches-this",
        "/maps/dir?from=northstar-hq&to=devcon-center&mode=driving",
    ];
    let mut out = String::new();
    for actor in ["alice", "dave"] {
        out.push_str(&report(
            "maps.google.com",
            actor,
            &sweep(*GOOGLE_MAPS, "maps.google.com", actor, &seeds),
        ));
    }
    assert!(out.is_empty(), "{out}");
}

#[test]
fn openstreetmap_has_no_dead_control_on_any_page() {
    let seeds = [
        "/",
        "/maps/saved",
        "/maps/notes",
        "/search?q=market",
        "/maps/dir?from=northstar-hq&to=devcon-center&mode=cycling",
    ];
    let mut out = String::new();
    for actor in ["bob", "dave"] {
        out.push_str(&report(
            "openstreetmap.org",
            actor,
            &sweep(*OSM, "openstreetmap.org", actor, &seeds),
        ));
    }
    assert!(out.is_empty(), "{out}");
}

#[test]
fn weather_has_no_dead_control_on_any_page() {
    let seeds = [
        "/",
        "/weather/today/l/seattle",
        "/weather/tenday/l/portland",
        "/search?q=seattle",
        "/search?q=nothing-matches-this",
        "/maps/saved",
        "/maps/notes",
        "/maps/place/northstar-hq",
    ];
    let mut out = String::new();
    for actor in ["alice", "dave"] {
        out.push_str(&report(
            "weather.com",
            actor,
            &sweep(*WEATHER, "weather.com", actor, &seeds),
        ));
    }
    assert!(out.is_empty(), "{out}");
}

/// Classes these stylesheets draw as something to press: a round map control, a category
/// chip, a forecast pill, an action, a travel-mode tab, a nav item, a result card. Neither
/// a pointer cursor nor a hover change is needed to make a white disc with a `+` in it read
/// as a button, so the auditor cannot catch these; they are named here instead. The one
/// exception is `off`, the greyed-out state a zoom control wears at the end of its range,
/// which is a disabled button and is drawn as one on purpose.
const PRESSABLE: &[&str] = &[
    "ctl", "chip", "pill", "act", "go", "mode", "nav", "place", "city", "notecard", "tile",
];

/// The complaints, and how many elements wore one of the classes at all — a page that wears
/// none of them proves nothing, so the test checks it found some to judge.
fn looks_pressable_but_is_not(html: &str) -> (Vec<String>, usize) {
    use cw_web::dom::Document;
    use cw_web::paint::semantics::is_interactive;
    let doc = cw_web::html::parse(html);
    let (mut out, mut seen) = (Vec::new(), 0usize);
    for node in doc.descendants(Document::ROOT) {
        if !doc.is_element(node) {
            continue;
        }
        let classes: Vec<&str> = doc
            .attr(node, "class")
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        if classes.contains(&"off") {
            continue;
        }
        let Some(class) = classes.iter().find(|c| PRESSABLE.contains(c)) else {
            continue;
        };
        seen += 1;
        if is_interactive(&doc, node)
            || doc
                .ancestors(node)
                .any(|a| doc.is_element(a) && is_interactive(&doc, a))
        {
            continue;
        }
        out.push(format!(
            "<{} id={:?} class={class:?}> is drawn as pressable but is not a control",
            doc.tag(node).unwrap_or("?"),
            doc.attr(node, "id").unwrap_or_default()
        ));
    }
    (out, seen)
}
fn render(raw: &str, host: &str, actor: &str, path: &str) -> String {
    let mut state = load(raw);
    let r = GeoService
        .handle(
            &mut state,
            &ctx(actor),
            &HttpRequest::get(format!("http://{host}{path}")),
        )
        .unwrap();
    assert_eq!(r.status, 200, "{path}");
    String::from_utf8(r.body).unwrap()
}

#[test]
fn nothing_that_is_drawn_as_pressable_is_inert() {
    let pages = [
        (
            *GOOGLE_MAPS,
            "maps.google.com",
            "alice",
            [
                "/",
                "/maps/saved",
                "/maps/notes",
                "/search?q=cafe",
                "/maps/place/harborline-coffee",
                "/maps/place/harborline-coffee?zoom=0",
                "/maps/dir?from=northstar-hq&to=devcon-center&mode=driving",
            ]
            .as_slice(),
        ),
        (
            *OSM,
            "openstreetmap.org",
            "bob",
            [
                "/",
                "/maps/saved",
                "/maps/notes",
                "/search?q=market",
                "/maps/place/bayfront-depot",
                "/maps/place/bayfront-depot?zoom=6",
                "/maps/dir?from=northstar-hq&to=devcon-center&mode=cycling",
            ]
            .as_slice(),
        ),
        (
            *WEATHER,
            "weather.com",
            "alice",
            [
                "/",
                "/weather/today/l/seattle",
                "/weather/tenday/l/seattle",
                "/search?q=seattle",
                "/maps/saved",
                "/maps/place/northstar-hq",
            ]
            .as_slice(),
        ),
    ];
    let mut out = String::new();
    for (raw, host, actor, paths) in pages {
        for path in paths {
            let (faults, seen) = looks_pressable_but_is_not(&render(raw, host, actor, path));
            assert!(
                seen >= 3,
                "{host}{path} wears none of the pressable classes; the check is vacuous"
            );
            for fault in faults {
                out.push_str(&format!("{host}{path}: {fault}\n"));
            }
        }
    }
    assert!(out.is_empty(), "{out}");
}
