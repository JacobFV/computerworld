//! A network speaker (or a TV with speakers): the far end of AirPlay, Cast and DLNA.
//!
//! A music player hands its session to a speaker by sending it — the queue, where in it
//! the listener is, whether it is playing, the repeat mode and the volume — and from then
//! on the speaker is where the music is: its own page says what it is playing, and it
//! carries the position forward on the world clock with the very same calculation the
//! music service uses (`cw_service_media::Player::settle`), so the two never disagree
//! about which song is on. The player keeps the speaker current by sending the session
//! again whenever it changes, and takes it back with `/api/stop`.
//!
//! - `GET /` — the speaker's page, as HTML: its name, what it is playing, how far in,
//!   its volume and the rest of the queue. `speaker.css` gives it the full-bleed "now
//!   playing" screen a Sonos or a Chromecast puts on a display.
//! - `GET /art/<key>?size=&radius=` — the album's generated artwork as a page image, the
//!   same picture `cw_service_media` serves for the same key.
//! - `POST /volume`, `POST /stop` — the browser's half of `/api/volume` and `/api/stop`:
//!   the page's own controls post here and get the page back.
//! - `GET /api/status` — the same as JSON, the position settled to the request's tick.
//! - `POST /api/cast` — `{source, controller?, player, tracks}`: play this session here.
//!   `player` is the music service's player (`queue`, `index`, `position_ms`, `playing`,
//!   `repeat`, `tick`, `volume`, `muted`); `tracks` maps each queued id to its `title`,
//!   `artist`, `album` and `duration_ms`.
//! - `POST /api/stop` — stop and forget the session.
//! - `POST /api/volume` — `{level}`: the speaker's own volume, 0 to 100.
//!
//! Seed: `{name, model, protocols: ["airplay"|"cast"|"dlna"...], volume}`.
mod view;

use cw_protocol::{HttpRequest, HttpResponse, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use cw_service_media::Player;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub struct SpeakerService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(SpeakerService)
}
/// Ways a speaker can be sent sound.
pub const PROTOCOLS: &[&str] = &["airplay", "cast", "dlna"];
/// A queue this long is enough for any session a player hands over.
const QUEUE_LIMIT: usize = 500;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// The music service the session belongs to (`http://spotify.com/`).
    pub source: String,
    /// Who handed it over.
    pub controller: String,
    pub player: Player,
    pub tracks: BTreeMap<String, Track>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Speaker {
    pub name: String,
    pub model: String,
    pub protocols: Vec<String>,
    pub volume: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<Session>,
}
impl Speaker {
    /// The session as of `now`: the handed-over position carried forward on the clock.
    pub(crate) fn settled(&self, now: u64) -> Option<(Session, Player)> {
        let session = self.session.clone()?;
        let player = session.player.settle(now, |id| {
            session.tracks.get(id).map_or(0, |t| t.duration_ms)
        });
        Some((session, player))
    }
    fn status(&self, now: u64) -> Value {
        let playing = self.settled(now).map(|(session, p)| {
            let id = p.current().unwrap_or_default().to_owned();
            let track = session.tracks.get(&id).cloned().unwrap_or_default();
            json!({
                "source": session.source,
                "controller": session.controller,
                "item": id,
                "title": track.title,
                "artist": track.artist,
                "album": track.album,
                "position_ms": p.position_ms,
                "duration_ms": track.duration_ms,
                "playing": p.playing,
                "index": p.index,
                "queue": p.queue,
                "repeat": p.repeat,
                "volume": p.volume,
                "muted": p.muted,
                "tick": now,
            })
        });
        json!({
            "name": self.name,
            "model": self.model,
            "protocols": self.protocols,
            "volume": self.volume,
            "session": playing,
        })
    }
}

pub(crate) fn clock(ms: u64) -> String {
    let s = ms / 1_000;
    format!("{}:{:02}", s / 60, s % 60)
}

impl Service for SpeakerService {
    fn kind(&self) -> &str {
        "speaker"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let mut speaker: Speaker = web::load(&initial)?;
        if speaker.name.trim().is_empty() {
            speaker.name = "Speaker".into();
        }
        if let Some(bad) = speaker
            .protocols
            .iter()
            .find(|p| !PROTOCOLS.contains(&p.as_str()))
        {
            return Err(cw_protocol::SimError::invalid(format!(
                "unknown speaker protocol {bad}"
            )));
        }
        speaker.volume = speaker.volume.min(100);
        Ok(serde_json::to_value(speaker)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        request: &HttpRequest,
    ) -> Result<HttpResponse> {
        let mut speaker: Speaker = web::load(state)?;
        let path = web::path(request);
        let method = request.method.to_ascii_uppercase();
        let now = ctx.tick;
        // `/art/<key>` is the only route with a variable in it.
        if method == "GET" {
            if let Some(key) = path.trim_start_matches('/').strip_prefix("art/") {
                return artwork(key, request);
            }
        }
        match (method.as_str(), path.trim_end_matches('/')) {
            ("GET", "") => web::html::page(&view::page(&speaker, now)),
            ("GET", "/api/status") => HttpResponse::json(200, &speaker.status(now)),
            ("POST", "/api/cast") => {
                let body = web::body(request)?;
                let mut session: Session = match serde_json::from_value(body.clone()) {
                    Ok(s) => s,
                    Err(e) => return web::error(400, format!("not a session: {e}")),
                };
                if session.source.is_empty() {
                    return web::error(400, "a session names the service it came from");
                }
                if session.player.queue.is_empty() {
                    return web::error(400, "there is nothing to play");
                }
                session.player.queue.truncate(QUEUE_LIMIT);
                session.player.source.truncate(QUEUE_LIMIT);
                session
                    .tracks
                    .retain(|id, _| session.player.queue.contains(id));
                session.controller = ctx.actor.clone();
                // The position was true when the player sent it; from here the speaker's
                // own clock carries it.
                session.player.tick = now;
                session.player = session.player.normalize();
                speaker.session = Some(session);
                web::save(state, &speaker)?;
                HttpResponse::json(200, &speaker.status(now))
            }
            ("POST", "/api/stop") | ("POST", "/stop") => {
                speaker.session = None;
                web::save(state, &speaker)?;
                if path.trim_end_matches('/') == "/stop" {
                    return web::html::page(&view::page(&speaker, now));
                }
                HttpResponse::json(200, &speaker.status(now))
            }
            ("POST", "/api/volume") | ("POST", "/volume") => {
                let body = web::body(request)?;
                // A request that names no level is the caller's mistake, not the
                // simulation's: answer it the way a bad level is answered.
                let Ok(level) = web::number(&body, "level") else {
                    return web::error(400, "volume is a level from 0 to 100");
                };
                if level > 100 {
                    return web::error(400, "volume is a level from 0 to 100");
                }
                speaker.volume = level as u8;
                web::save(state, &speaker)?;
                if path.trim_end_matches('/') == "/volume" {
                    return web::html::page(&view::page(&speaker, now));
                }
                HttpResponse::json(200, &speaker.status(now))
            }
            ("GET" | "POST", _) => web::error(404, "route not found"),
            _ => web::error(405, "method not allowed"),
        }
    }
}

/// Largest cover the page may ask for.
const ART_LIMIT: u32 = 512;
/// `GET /art/<key>?size=&radius=`: the album's generated artwork as a page image. The
/// key is the album's slug, so the picture is the very one the music service that cast
/// here shows for the same album.
fn artwork(key: &str, request: &HttpRequest) -> Result<HttpResponse> {
    #[derive(Serialize)]
    struct Asset<'a> {
        width: u32,
        height: u32,
        rgba: &'a [u8],
    }
    let number = |k: &str| web::query(request, k).and_then(|v| v.parse::<u32>().ok());
    let size = number("size").unwrap_or(320).clamp(8, ART_LIMIT);
    let radius = number("radius").unwrap_or(0).min(size / 2);
    let rgba = cw_artwork::rasterize(&cw_artwork::artwork(key), size, radius);
    Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::from([("content-type".into(), cw_protocol::RGBA_MEDIA_TYPE.into())]),
        body: serde_json::to_vec(&Asset {
            width: size,
            height: size,
            rgba: &rgba,
        })?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const S: u64 = 1_000_000;
    fn ctx(tick: u64) -> ServiceContext {
        ServiceContext {
            actor: "alice".into(),
            source: "alice-mac".into(),
            tick,
            seed: 1,
            instance: "livingroom".into(),
        }
    }
    fn post(state: &mut Value, tick: u64, path: &str, body: Value) -> HttpResponse {
        let request = HttpRequest::json("POST", format!("http://livingroom{path}"), &body).unwrap();
        SpeakerService.handle(state, &ctx(tick), &request).unwrap()
    }
    fn get(state: &mut Value, tick: u64, path: &str) -> Value {
        let r = SpeakerService
            .handle(
                state,
                &ctx(tick),
                &HttpRequest::get(format!("http://livingroom{path}")),
            )
            .unwrap();
        serde_json::from_slice(&r.body).unwrap()
    }
    fn seed() -> Value {
        SpeakerService
            .initialize(
                json!({"name": "Living Room", "protocols": ["airplay", "cast"], "volume": 40}),
                &ctx(0),
            )
            .unwrap()
    }
    /// The page: HTML, strict-clean, parsed by the engine.
    fn page(state: &mut Value, tick: u64) -> cw_web::dom::Document {
        let reply = SpeakerService
            .handle(state, &ctx(tick), &HttpRequest::get("http://livingroom/"))
            .unwrap();
        assert_eq!(reply.status, 200);
        assert_eq!(
            reply.header("content-type"),
            Some(web::html::HTML_MEDIA_TYPE)
        );
        let html = String::from_utf8(reply.body).unwrap();
        web::html::validate_strict(&html).unwrap_or_else(|e| panic!("{e:?}"));
        cw_web::html::parse(&html)
    }
    /// A page posted to, which answers with the page again.
    fn submit(state: &mut Value, tick: u64, path: &str, fields: &[(&str, &str)]) -> HttpResponse {
        let body: String = fields
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let mut request = HttpRequest::get(format!("http://livingroom{path}"));
        request.method = "POST".into();
        request.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        request.body = body.into_bytes();
        SpeakerService.handle(state, &ctx(tick), &request).unwrap()
    }
    fn text_of(dom: &cw_web::dom::Document, id: &str) -> String {
        let node = *dom
            .by_id(id)
            .first()
            .unwrap_or_else(|| panic!("no #{id} on the page"));
        dom.text_content(node)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn session() -> Value {
        json!({
            "source": "http://spotify.com/",
            "player": {"item": "a", "queue": ["a", "b"], "index": 0, "position_ms": 5000,
                       "playing": true, "volume": 70},
            "tracks": {"a": {"title": "A", "artist": "X", "duration_ms": 10000},
                       "b": {"title": "B", "artist": "X", "duration_ms": 20000},
                       "zzz": {"title": "not queued"}}
        })
    }

    #[test]
    fn a_handed_over_session_plays_on_the_speakers_own_clock() {
        let mut state = seed();
        assert!(get(&mut state, 0, "/api/status")["session"].is_null());
        assert_eq!(post(&mut state, 3 * S, "/api/cast", session()).status, 200);
        // Four seconds later it is 9 s into A; ten seconds later it has moved on to B.
        let now = get(&mut state, 7 * S, "/api/status");
        assert_eq!(now["session"]["item"], "a");
        assert_eq!(now["session"]["position_ms"], 9_000);
        let later = get(&mut state, 13 * S, "/api/status");
        assert_eq!(later["session"]["item"], "b");
        assert_eq!(later["session"]["position_ms"], 5_000);
        assert_eq!(later["session"]["controller"], "alice");
        // Only what was queued is kept.
        let speaker: Speaker = serde_json::from_value(state.clone()).unwrap();
        assert!(!speaker.session.unwrap().tracks.contains_key("zzz"));
        // Its page says so too: the state, the track, the artist and the clock.
        let dom = page(&mut state, 13 * S);
        assert_eq!(text_of(&dom, "speaker-state"), "Playing");
        assert_eq!(text_of(&dom, "speaker-now"), "B");
        assert_eq!(text_of(&dom, "speaker-artist"), "X");
        assert_eq!(text_of(&dom, "speaker-position"), "0:05 / 0:20");
        assert_eq!(text_of(&dom, "speaker-name"), "Living Room");
        assert_eq!(
            text_of(&dom, "speaker-source"),
            "From http://spotify.com/ (alice)"
        );
        // The art is the album's own key, the one the music service rasterises.
        let art = *dom.by_id("speaker-art").first().expect("the art is shown");
        assert_eq!(dom.attr(art, "src"), Some("/art/b?size=320&radius=10"));
    }

    #[test]
    fn the_pages_own_controls_set_the_volume_and_stop_the_session() {
        let mut state = seed();
        // Idle: the screen says so, and the volume slider is still the speaker's.
        let dom = page(&mut state, 0);
        assert_eq!(text_of(&dom, "speaker-state"), "Idle");
        assert_eq!(text_of(&dom, "speaker-now"), "Nothing playing");
        assert_eq!(text_of(&dom, "speaker-artist"), "Ready for AirPlay, Cast");
        assert_eq!(text_of(&dom, "speaker-volume"), "Volume 40%");
        assert!(dom.by_id("speaker-art").is_empty(), "nothing to show");
        // Every fifth percent is its own control, and it really posts a level.
        let form = *dom.by_id("speaker-volume-55-form").first().unwrap();
        assert_eq!(dom.attr(form, "action"), Some("/volume"));
        assert_eq!(dom.attr(form, "method"), Some("post"));
        let level = dom
            .descendants(form)
            .find(|n| dom.attr(*n, "name") == Some("level"))
            .expect("the control carries a level");
        assert_eq!(dom.attr(level, "value"), Some("55"));
        assert_eq!(
            submit(&mut state, 0, "/volume", &[("level", "55")]).status,
            200
        );
        assert_eq!(
            text_of(&page(&mut state, 0), "speaker-volume"),
            "Volume 55%"
        );
        assert_eq!(get(&mut state, 0, "/api/status")["volume"], 55);
        // A level the slider could never send is still refused.
        assert_eq!(
            submit(&mut state, 0, "/volume", &[("level", "140")]).status,
            400
        );
        // Playing: the queue beyond the current song is "Up next", and Stop ends it.
        post(&mut state, 0, "/api/cast", session());
        let dom = page(&mut state, 0);
        assert_eq!(text_of(&dom, "speaker-now"), "A");
        assert_eq!(text_of(&dom, "speaker-queue-1"), "2BX0:20");
        let stop = *dom.by_id("speaker-stop-form").first().unwrap();
        assert_eq!(dom.attr(stop, "action"), Some("/stop"));
        assert_eq!(submit(&mut state, 0, "/stop", &[]).status, 200);
        assert!(get(&mut state, 0, "/api/status")["session"].is_null());
        assert_eq!(text_of(&page(&mut state, 0), "speaker-state"), "Idle");
    }

    #[test]
    fn every_shipped_speaker_serves_a_strict_page_idle_and_playing() {
        for name in ["livingroom-speaker", "kitchen-speaker", "office-tv-speaker"] {
            let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../worlds/company-2026/sites/{name}.json"));
            let file: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut state = SpeakerService
                .initialize(file["initial_state"].clone(), &ctx(0))
                .unwrap_or_else(|e| panic!("{name} seed: {e}"));
            // Idle, then with a session on: both states of every shipped speaker.
            let idle = page(&mut state, 0);
            assert_eq!(
                text_of(&idle, "speaker-name"),
                file["initial_state"]["name"].as_str().unwrap()
            );
            assert!(!idle.by_id("speaker-protocols").is_empty(), "{name}");
            post(&mut state, 0, "/api/cast", session());
            let on = page(&mut state, 3 * S);
            assert_eq!(text_of(&on, "speaker-state"), "Playing", "{name}");
        }
    }

    #[test]
    fn stop_volume_and_bad_requests() {
        let mut state = seed();
        post(&mut state, 0, "/api/cast", session());
        assert_eq!(
            post(&mut state, 0, "/api/volume", json!({"level": 55})).status,
            200
        );
        assert_eq!(get(&mut state, 0, "/api/status")["volume"], 55);
        assert_eq!(
            post(&mut state, 0, "/api/volume", json!({"level": 101})).status,
            400
        );
        assert_eq!(post(&mut state, 0, "/api/stop", json!({})).status, 200);
        assert!(get(&mut state, 0, "/api/status")["session"].is_null());
        assert_eq!(
            post(
                &mut state,
                0,
                "/api/cast",
                json!({"source": "x", "player": {}})
            )
            .status,
            400
        );
        assert!(SpeakerService
            .initialize(json!({"protocols": ["bluetooth"]}), &ctx(0))
            .is_err());
    }
}
