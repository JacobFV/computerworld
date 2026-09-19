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
//! - `GET /` — the speaker's page: its name, what it is playing, how far in, its volume.
//! - `GET /api/status` — the same as JSON, the position settled to the request's tick.
//! - `POST /api/cast` — `{source, controller?, player, tracks}`: play this session here.
//!   `player` is the music service's player (`queue`, `index`, `position_ms`, `playing`,
//!   `repeat`, `tick`, `volume`, `muted`); `tracks` maps each queued id to its `title`,
//!   `artist`, `album` and `duration_ms`.
//! - `POST /api/stop` — stop and forget the session.
//! - `POST /api/volume` — `{level}`: the speaker's own volume, 0 to 100.
//!
//! Seed: `{name, model, protocols: ["airplay"|"cast"|"dlna"...], volume}`.
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
    fn settled(&self, now: u64) -> Option<(Session, Player)> {
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

fn clock(ms: u64) -> String {
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
        match (method.as_str(), path.trim_end_matches('/')) {
            ("GET", "") => page(&speaker, now),
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
            ("POST", "/api/stop") => {
                speaker.session = None;
                web::save(state, &speaker)?;
                HttpResponse::json(200, &speaker.status(now))
            }
            ("POST", "/api/volume") => {
                let body = web::body(request)?;
                let level = web::number(&body, "level")?;
                if level > 100 {
                    return web::error(400, "volume is a level from 0 to 100");
                }
                speaker.volume = level as u8;
                web::save(state, &speaker)?;
                HttpResponse::json(200, &speaker.status(now))
            }
            ("GET" | "POST", _) => web::error(404, "route not found"),
            _ => web::error(405, "method not allowed"),
        }
    }
}

fn page(speaker: &Speaker, now: u64) -> Result<HttpResponse> {
    let mut elements = vec![
        web::heading("speaker-name", &speaker.name),
        web::paragraph(
            "speaker-model",
            format!(
                "{} · {}",
                if speaker.model.is_empty() {
                    "Network speaker"
                } else {
                    &speaker.model
                },
                speaker
                    .protocols
                    .iter()
                    .map(|p| match p.as_str() {
                        "airplay" => "AirPlay",
                        "cast" => "Cast",
                        _ => "DLNA",
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        web::paragraph("speaker-volume", format!("Volume {}%", speaker.volume)),
    ];
    match speaker.settled(now) {
        Some((session, p)) => {
            let id = p.current().unwrap_or_default();
            let track = session.tracks.get(id).cloned().unwrap_or_default();
            elements.push(web::paragraph(
                "speaker-now",
                format!(
                    "{} {} — {}",
                    if p.playing { "Playing" } else { "Paused" },
                    track.title,
                    track.artist
                ),
            ));
            elements.push(web::paragraph(
                "speaker-position",
                format!("{} / {}", clock(p.position_ms), clock(track.duration_ms)),
            ));
            elements.push(web::paragraph(
                "speaker-source",
                format!("From {} ({})", session.source, session.controller),
            ));
        }
        None => elements.push(web::paragraph("speaker-now", "Idle")),
    }
    web::page(&speaker.name, elements)
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
        // Its page says so too.
        let page = SpeakerService
            .handle(
                &mut state,
                &ctx(13 * S),
                &HttpRequest::get("http://livingroom/"),
            )
            .unwrap();
        let page: cw_protocol::Page = serde_json::from_slice(&page.body).unwrap();
        page.validate().unwrap();
        let text = serde_json::to_string(&page).unwrap();
        assert!(text.contains("Playing B — X") && text.contains("0:05 / 0:20"));
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
