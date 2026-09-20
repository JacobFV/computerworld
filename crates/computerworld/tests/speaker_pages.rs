//! A network speaker's own page, served as HTML and driven through the agent API.
//!
//! A speaker is the far end of AirPlay and Cast: a music service hands it a session over
//! the network, and from then on the speaker's address is where the music is. This
//! drives both halves — `http.v1` casts the session the way a player would, then
//! `browser.v1` opens `http://livingroom.speaker.internal/` and reads and works the
//! screen: what is playing, how far in, the rest of the queue, the volume slider, and
//! Stop, which hands the session back.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, HttpRequest, HttpResponse};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";
const LIVINGROOM: &str = "http://livingroom.speaker.internal/";

fn world() -> (World, String) {
    let mut world = World::new(reference_world(), 42).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: ACTOR.into(),
            machines: vec![MACHINE.into()],
            actions: vec!["browser.v1".into(), "http.v1".into()],
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
fn http(world: &mut World, session: &str, method: &str, url: &str, body: Value) -> HttpResponse {
    let request = HttpRequest::json(method, url, &body).unwrap();
    let value = act(
        world,
        session,
        "http.v1",
        "request",
        serde_json::to_value(request).unwrap(),
    );
    serde_json::from_value(value).unwrap()
}
fn status(world: &mut World, session: &str) -> Value {
    let reply = http(
        world,
        session,
        "GET",
        &format!("{LIVINGROOM}api/status"),
        json!({}),
    );
    assert_eq!(reply.status, 200);
    serde_json::from_slice(&reply.body).unwrap()
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
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
    all.iter().find(|e| e["id"] == id).unwrap_or_else(|| {
        let ids: Vec<&str> = all.iter().filter_map(|e| e["id"].as_str()).collect();
        panic!("no element {id} in the semantic tree; it lists {ids:?}")
    })
}
/// The whole page as one string: what the agent can read.
fn words(page: &Value) -> String {
    fn walk(elements: &[Value], out: &mut String) {
        for e in elements {
            if let Some(t) = e["text"].as_str() {
                out.push_str(t);
                out.push(' ');
            }
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let mut out = String::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    out
}
/// The session a music service hands over when a listener picks this speaker.
fn session_body() -> Value {
    json!({
        "source": "http://spotify.com/",
        "player": {"item": "cold-start", "queue": ["cold-start", "green-build", "rollback"],
                   "index": 0, "position_ms": 30_000, "playing": true, "volume": 70},
        "tracks": {
            "cold-start": {"title": "Cold Start", "artist": "Midnight Compiler",
                           "album": "Cold Start", "duration_ms": 214_000},
            "green-build": {"title": "Green Build", "artist": "Midnight Compiler",
                            "album": "Cold Start", "duration_ms": 187_000},
            "rollback": {"title": "Rollback", "artist": "Midnight Compiler",
                         "album": "Cold Start", "duration_ms": 243_000}
        }
    })
}

#[test]
fn a_speakers_page_shows_the_session_and_its_controls_really_work() {
    let (mut world, session) = world();

    // Idle first: the screen says what the speaker is and what it is waiting for.
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url": LIVINGROOM}),
    );
    let idle = page(&world, &session);
    assert_eq!(idle["title"], "Living Room");
    let text = words(&idle);
    assert!(text.contains("Living Room"), "{text}");
    assert!(text.contains("Nothing playing"), "{text}");
    assert!(text.contains("Ready for AirPlay, Cast"), "{text}");
    assert!(text.contains("Volume 35%"), "{text}");

    // The volume slider is twenty controls, and pressing one really sets the level.
    let all = elements(&idle);
    let step = by_id(&all, "speaker-volume-60");
    assert_eq!(step["kind"], "button");
    assert_eq!(
        step["text"].as_str().or_else(|| step["label"].as_str()),
        Some("Volume 60%"),
        "each step names the level it sets: {step:?}"
    );
    assert_eq!(step["action"]["method"], "POST");
    assert_eq!(
        step["action"]["url"], "http://livingroom.speaker.internal/volume",
        "the page's control posts to the speaker's own route"
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id": "speaker-volume-60"}),
    );
    assert!(words(&page(&world, &session)).contains("Volume 60%"));
    assert_eq!(status(&mut world, &session)["volume"], 60);

    // Cast a session the way a music service does, then look at the speaker again.
    let cast = http(
        &mut world,
        &session,
        "POST",
        &format!("{LIVINGROOM}api/cast"),
        session_body(),
    );
    assert_eq!(cast.status, 200);
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url": LIVINGROOM}),
    );
    let playing = page(&world, &session);
    let text = words(&playing);
    assert!(text.contains("Playing"), "{text}");
    assert!(text.contains("Cold Start"), "{text}");
    assert!(text.contains("Midnight Compiler"), "{text}");
    assert!(text.contains("0:30 / 3:34"), "how far in: {text}");
    assert!(text.contains("From http://spotify.com/ (alice)"), "{text}");
    // The rest of the handed-over queue is "Up next", in the order it will be heard.
    assert!(text.contains("Green Build") && text.contains("Rollback"), "{text}");
    let at = text.find("Green Build").unwrap();
    assert!(at < text.find("Rollback").unwrap(), "in queue order");

    // Stop hands the session back: the screen goes idle and so does the JSON.
    let all = elements(&playing);
    assert_eq!(by_id(&all, "speaker-stop")["kind"], "button");
    assert_eq!(
        by_id(&all, "speaker-stop")["action"]["url"],
        "http://livingroom.speaker.internal/stop"
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id": "speaker-stop"}),
    );
    assert!(words(&page(&world, &session)).contains("Nothing playing"));
    assert!(status(&mut world, &session)["session"].is_null());
}
