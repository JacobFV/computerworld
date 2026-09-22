//! Nothing on a speaker's screen looks like a control it is not.
//!
//! `cw_service_common::audit` crawls the page the way the browser would: it probes every
//! form `action` and `formaction` with the method written on it and reports anything that
//! answers 404, 405 or worse, and per page it reports every anchor with nowhere to go,
//! every button outside a form, every field with no name or no label, every fake `role`,
//! and every inert element the cascade draws with `cursor: pointer`.
//!
//! There is one skin and two page kinds — idle and playing — and all three shipped
//! speakers are swept in both.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common as web;
use cw_service_common::audit;
use cw_service_speaker::SpeakerService;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Every speaker the world ships. Each is swept idle and with a session on it.
const SPEAKERS: [&str; 3] = ["livingroom-speaker", "kitchen-speaker", "office-tv-speaker"];

fn ctx(tick: u64) -> ServiceContext {
    ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick,
        seed: 1,
        instance: "speaker".into(),
    }
}
/// The session a music player hands over, so the playing page has art and a queue.
fn session() -> Value {
    json!({
        "source": "http://spotify.com/",
        "player": {"item": "cold-reads", "queue": ["cold-reads", "warm-cache", "eviction"],
                   "index": 0, "position_ms": 64_000, "playing": true, "volume": 70},
        "tracks": {
            "cold-reads": {"title": "Cold Reads", "artist": "Cache Miss", "album": "Cold Reads", "duration_ms": 214_000},
            "warm-cache": {"title": "Warm Cache", "artist": "Cache Miss", "album": "Cold Reads", "duration_ms": 196_000},
            "eviction": {"title": "Eviction", "artist": "Cache Miss", "album": "Cold Reads", "duration_ms": 243_000}
        }
    })
}
/// The shipped seed, loaded and optionally cast to.
fn seeded(name: &str, playing: bool) -> (String, Value) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../worlds/company-2026/sites/{name}.json"));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let host = file["domains"][0].as_str().unwrap().to_owned();
    let mut state = SpeakerService
        .initialize(file["initial_state"].clone(), &ctx(0))
        .unwrap_or_else(|e| panic!("{name} seed: {e:?}"));
    if playing {
        let cast =
            HttpRequest::json("POST", format!("http://{host}/api/cast"), &session()).unwrap();
        let reply = SpeakerService.handle(&mut state, &ctx(0), &cast).unwrap();
        assert_eq!(reply.status, 200, "{name} would not take the session");
    }
    (format!("http://{host}"), state)
}

/// Every complaint the sweep has about one speaker in one state.
///
/// Stop and the volume steps really run, so each request is served by its own copy of
/// the state: a probed `POST /stop` cannot silence the page the crawl is reading.
fn sweep(name: &str, playing: bool) -> Vec<String> {
    let (origin, base) = seeded(name, playing);
    let mut call = |method: &str, path: &str| {
        let mut state = base.clone();
        let mut request = HttpRequest::get(format!("{origin}{path}"));
        request.method = method.to_owned();
        if method != "GET" {
            request.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
        }
        match SpeakerService.handle(&mut state, &ctx(3_000_000), &request) {
            Ok(reply) => {
                let body = String::from_utf8_lossy(&reply.body).into_owned();
                if reply.header("content-type") == Some(web::html::HTML_MEDIA_TYPE) {
                    web::html::validate_strict(&body)
                        .unwrap_or_else(|e| panic!("{method} {path} is not strict: {e:?}"));
                }
                (reply.status, body)
            }
            // A request a browser can make must never be a simulation error.
            Err(e) => (500, format!("{e:?}")),
        }
    };
    audit::Sweep::new(&["/"], &mut call).run()
}

#[test]
fn no_speaker_screen_offers_a_control_that_cannot_act() {
    let mut faults = Vec::new();
    for name in SPEAKERS {
        for playing in [false, true] {
            let state = if playing { "playing" } else { "idle" };
            faults.extend(
                sweep(name, playing)
                    .into_iter()
                    .map(|fault| format!("{name} {state}: {fault}")),
            );
        }
    }
    assert!(faults.is_empty(), "dead controls:\n{}", faults.join("\n"));
}

/// Every step of the slider is its own control and really sets that level, and Stop
/// really ends the session: the sweep only proves the routes answer, this proves they
/// act.
#[test]
fn the_slider_and_stop_do_what_they_say() {
    let (origin, mut state) = seeded("livingroom-speaker", true);
    let submit = |state: &mut Value, path: &str, body: &str| {
        let mut request = HttpRequest::get(format!("{origin}{path}"));
        request.method = "POST".into();
        request.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        request.body = body.as_bytes().to_vec();
        SpeakerService.handle(state, &ctx(0), &request).unwrap()
    };
    let page = SpeakerService
        .handle(&mut state, &ctx(0), &HttpRequest::get(format!("{origin}/")))
        .unwrap();
    let html = String::from_utf8(page.body).unwrap();
    let doc = cw_web::html::parse(&html);
    // Twenty steps, each one a form of its own carrying the level it sets.
    for step in 1..=20u64 {
        let pct = step * 5;
        let form = *doc
            .by_id(&format!("speaker-volume-{pct}-form"))
            .first()
            .unwrap_or_else(|| panic!("no step at {pct}%"));
        assert_eq!(doc.attr(form, "action"), Some("/volume"));
        let level = doc
            .descendants(form)
            .find(|n| doc.attr(*n, "name") == Some("level"))
            .expect("the step carries a level");
        assert_eq!(doc.attr(level, "value"), Some(pct.to_string().as_str()));
        assert_eq!(
            submit(&mut state, "/volume", &format!("level={pct}")).status,
            200
        );
        let typed: Value = state.clone();
        assert_eq!(typed["volume"], pct, "step {pct}% did not set the volume");
    }
    // A form the page never sends is still refused rather than blowing up.
    assert_eq!(submit(&mut state, "/volume", "").status, 400);
    assert_eq!(submit(&mut state, "/volume", "level=101").status, 400);
    // Stop hands the session back, and the page then says the speaker is idle.
    assert_eq!(submit(&mut state, "/stop", "").status, 200);
    assert!(state["session"].is_null(), "Stop must end the session");
}
