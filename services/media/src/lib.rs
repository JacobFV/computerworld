//! Streaming platforms: youtube.com (`mode: video`), spotify.com (`mode: audio`) and
//! music.youtube.com (`mode: music`) share one catalogue shape, so channels/artists and
//! videos/tracks are the same two maps.
//!
//! Watching is a mutation on purpose: `GET /watch` counts a view and parks `now_playing`, so the
//! world visibly changes when an agent watches something. The two music modes play through a
//! real player (`player`): a queue built from an album, playlist, artist, station, mood or
//! the library, shuffle and repeat, and a position the world clock moves. Their pages
//! (`spotify`, `ytmusic`) pin a player bar to the bottom of the viewport, and the native
//! music players read the same session as JSON:
//!
//! - `GET /api/catalog` — artists, albums, tracks, playlists, and this listener's likes,
//!   library, history, subscriptions and player as of the request's tick;
//! - `GET /api/search?q=` — matching track, album, artist and playlist ids;
//! - `POST /api/player` — `{action: play|toggle|pause|resume|next|previous|seek|shuffle|
//!   repeat|jump|queue, ...}`, answered with the player;
//! - `POST /api/library/items/<id>`, `/api/library/albums/<id>` — save or unsave;
//! - `POST /api/items/<id>/queue`, `/api/playlists/<id>/remove` — Play Next and removal.
use cw_protocol::{HttpRequest, HttpResponse, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Map, Value};
mod catalog;
mod kit;
mod player;
mod spotify;
mod video;
mod view;
mod ytmusic;
pub use player::{Player, Repeat};
pub struct MediaService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(MediaService)
}
/// Documented seed keys, checked by container type only — the crate that fills them owns the rest.
const OBJECTS: &[&str] = &[
    "theme",
    "channels",
    "items",
    "playlists",
    "subscriptions",
    "likes",
    "now_playing",
    "albums",
    "library",
    "history",
    "devices",
];
const ARRAYS: &[&str] = &["sections"];
/// `mode` is the documented discriminant; an unlisted value is a seed typo, not a fallback.
/// `video` is a video site (youtube.com), `audio` a Spotify-style player (spotify.com) and
/// `music` a YouTube Music-style one (music.youtube.com). The two music modes share one
/// catalogue model and one player; only the pages differ.
const MODES: &[&str] = &["video", "audio", "music"];
const BRAND: &str = "Media";
const TAGLINE: &str = "Watch and listen.";
/// Flat stand-in tints. Artwork is never photography here, so a stable hash of the id keeps every
/// tile the same colour across renders, checkpoints and platforms.
const TINTS: &[&str] = &[
    "#2b3a55", "#4a2d43", "#1f4037", "#4b3621", "#2f2f4f", "#523a28", "#264653", "#3d2c4b",
];
fn tint(seed: &str) -> &'static str {
    let hash = seed
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    TINTS[hash as usize % TINTS.len()]
}
fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}
/// Thousands separators, because "18342 views" reads like a debug print, not a product.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
fn clock(seconds: u64) -> String {
    match (seconds / 3600, (seconds / 60) % 60, seconds % 60) {
        (0, m, s) => format!("{m}:{s:02}"),
        (h, m, s) => format!("{h}:{m:02}:{s:02}"),
    }
}
fn record<'a>(state: &'a Value, map: &str, id: &str) -> Option<&'a Value> {
    state.get(map)?.get(id)
}
/// Every map is keyed by id, so sorted keys are a stable render order for free.
fn keys(state: &Value, map: &str) -> Vec<String> {
    match state.get(map).and_then(Value::as_object) {
        Some(m) => m.keys().cloned().collect(),
        None => vec![],
    }
}
/// Newest first, id breaking ties — the home grid and every channel page share this order.
fn by_recency(state: &Value, filter: impl Fn(&Value) -> bool) -> Vec<String> {
    let mut ids: Vec<String> = keys(state, "items")
        .into_iter()
        .filter(|id| record(state, "items", id).is_some_and(&filter))
        .collect();
    ids.sort_by_key(|id| {
        let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
        (std::cmp::Reverse(num(&item, "published_tick")), id.clone())
    });
    ids
}
fn strings_at(state: &Value, map: &str, key: &str) -> Vec<String> {
    match state.get(map).and_then(|m| m.get(key)) {
        Some(v) => web::strings(&json!({ "v": v }), "v"),
        None => vec![],
    }
}
fn has(state: &Value, map: &str, key: &str, needle: &str) -> bool {
    strings_at(state, map, key).iter().any(|v| v == needle)
}
/// Per-actor membership lists (likes, subscriptions) are all toggles; one place to flip them.
fn toggle(state: &mut Value, map: &str, key: &str, value: &str) -> bool {
    let list = state
        .as_object_mut()
        .expect("state is an object")
        .entry(map)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .map(|m| m.entry(key).or_insert_with(|| json!([])))
        .and_then(Value::as_array_mut);
    let Some(list) = list else { return false };
    match list.iter().position(|v| v.as_str() == Some(value)) {
        Some(at) => {
            list.remove(at);
            false
        }
        None => {
            list.push(json!(value));
            true
        }
    }
}
/// Title, description, tags and channel name all match, so a search is worth typing.
fn matches(state: &Value, id: &str, needle: &str) -> bool {
    let Some(item) = record(state, "items", id) else {
        return false;
    };
    let hay = format!(
        "{} {} {} {} {}",
        web::text(item, "title"),
        web::text(item, "description"),
        web::text(item, "album"),
        web::strings(item, "tags").join(" "),
        record(state, "channels", &web::text(item, "channel")).map(|c| web::text(c, "name")).unwrap_or_default()
    )
    .to_lowercase();
    needle.split_whitespace().all(|word| hay.contains(&word.to_lowercase()))
}
fn item_mut<'a>(state: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    state.get_mut("items")?.as_object_mut()?.get_mut(id)
}
fn bump(state: &mut Value, id: &str, key: &str, delta: i64) {
    if let Some(item) = item_mut(state, id) {
        let now = num(item, key) as i64 + delta;
        item[key] = json!(now.max(0));
    }
}
/// Ids are dense and never recycled, so the next one is simply past the highest in use.
fn next_comment_id(comments: &[Value]) -> String {
    let highest = comments
        .iter()
        .filter_map(|c| web::text(c, "id").strip_prefix('c')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("c{}", highest + 1)
}
/// Percent-encode a query value so it survives a round trip through a URL.
fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
fn slug(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    slug.split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
/// Every GET route. `count` is false when a mutation re-renders its own page, so liking a video
/// does not also count as watching it again.
fn render(
    state: &mut Value,
    ctx: &ServiceContext,
    request: &HttpRequest,
    count: bool,
) -> Result<HttpResponse> {
    let mode = web::variant(state, "mode", MODES)?;
    let path = web::path(request);
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        ["api", "catalog"] => {
            return HttpResponse::json(200, &catalog::snapshot(state, &ctx.actor, ctx.tick))
        }
        ["api", "search"] => {
            return HttpResponse::json(
                200,
                &catalog::search(state, &web::query(request, "q").unwrap_or_default()),
            )
        }
        ["art", key] => return kit::artwork(key, request),
        [""] if state.get("items").is_none() => return video::landing(state),
        _ => {}
    }
    match mode.as_str() {
        "audio" => return spotify::route(state, ctx, request, &parts),
        "music" => return ytmusic::route(state, ctx, request, &parts, count),
        _ => {}
    }
    let actor = ctx.actor.as_str();
    match parts.as_slice() {
        [""] => video::home(state, actor),
        ["watch"] => {
            let Some(id) = web::query(request, "v") else {
                return web::error(404, "video not found");
            };
            let list = web::query(request, "list");
            if count {
                play(state, &id, list.as_deref(), ctx, true);
            }
            video::watch(state, &id, list.as_deref(), actor)
        }
        ["results"] => video::results(
            state,
            &web::query(request, "search_query").unwrap_or_default(),
            actor,
        ),
        ["search"] => video::results(state, &web::query(request, "q").unwrap_or_default(), actor),
        ["channel", id] | ["artist", id] => video::channel_page(state, id, actor),
        ["track", id] => video::track_page(state, id, actor),
        ["playlists"] => video::library(state, actor),
        ["playlist"] => match web::query(request, "list") {
            Some(id) => video::playlist_page(state, &id, actor),
            None => video::library(state, actor),
        },
        ["playlist", id] => video::playlist_page(state, id, actor),
        // youtu.be hands us the bare video id and nothing else.
        [id] if record(state, "items", id).is_some() => {
            if count {
                play(state, id, None, ctx, true);
            }
            video::watch(state, id, None, actor)
        }
        _ => web::error(404, "route not found"),
    }
}

impl Service for MediaService {
    fn kind(&self) -> &str {
        "media"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let state = web::shape(initial, OBJECTS, ARRAYS)?;
        web::variant(&state, "mode", MODES)?;
        web::theme(&state)?;
        Ok(state)
    }
    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        request: &HttpRequest,
    ) -> Result<HttpResponse> {
        let mode = web::variant(state, "mode", MODES)?;
        let video = mode == "video";
        // Where a control lands when the form does not say where it came from. The three
        // modes lay their pages out differently, so one mutation has a different home on
        // each; a fallback that is not a route on this skin is a 404 for whoever pressed it.
        let library = match mode.as_str() {
            "audio" => "/collection",
            "music" => "/library",
            _ => "/playlists",
        };
        let playlist_at = |id: &str| match mode.as_str() {
            "music" => format!("/playlist?list={id}"),
            _ => format!("/playlist/{id}"),
        };
        let item_at = |id: &str| match mode.as_str() {
            "audio" => format!("/track/{id}"),
            _ => format!("/watch?v={id}"),
        };
        let path = web::path(request);
        let method = request.method.to_ascii_uppercase();
        if method == "GET" {
            // A page refreshing itself is not a visit: it plays nothing and counts nothing.
            let visit = request.header(cw_protocol::REFRESH_HEADER).is_none();
            return render(state, ctx, request, visit);
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let body = web::body(request)?;
        let api = path.starts_with("/api/");
        let route: Vec<&str> = path
            .trim_matches('/')
            .strip_prefix("api/")
            .unwrap_or(path.trim_matches('/'))
            .split('/')
            .collect();
        let result: std::result::Result<(Value, String), String> = match route.as_slice() {
            ["items", id, "view"] => {
                if record(state, "items", id).is_none() {
                    Err("item not found".into())
                } else {
                    play(state, id, None, ctx, true);
                    Ok((json!({"views": num(&state["items"][id], "views")}), item_at(id)))
                }
            }
            // Both music modes play through the listener's player; its reply is the player.
            ["player"] if !video => catalog::command(state, ctx, &body)
                .map(|p| (catalog::player_json(state, &p), "/".to_owned())),
            ["items", id, "play"] if !video => {
                if record(state, "items", id).is_none() {
                    Err("item not found".into())
                } else {
                    let list = web::text(&body, "list");
                    let context = if list.is_empty() {
                        "track".to_owned()
                    } else {
                        format!("playlist:{list}")
                    };
                    catalog::command(
                        state,
                        ctx,
                        &json!({"action": "play", "item": id, "context": context}),
                    )
                    .map(|_| (json!({"plays": num(&state["items"][id], "plays")}), item_at(id)))
                }
            }
            ["items", id, "queue"] if !video => {
                if record(state, "items", id).is_none() {
                    Err("item not found".into())
                } else {
                    let next = web::text(&body, "next") == "true";
                    catalog::queue_or_play(state, ctx, id, next)
                        .map(|p| (catalog::player_json(state, &p), "/".to_owned()))
                }
            }
            ["library", "items", id] if !video => match record(state, "items", id) {
                None => Err("item not found".into()),
                Some(_) => {
                    let saved = catalog::save(state, &ctx.actor, &[id.to_string()]);
                    Ok((json!({"saved": saved}), library.to_owned()))
                }
            },
            ["library", "albums", id] if !video => match catalog::album(state, id) {
                None => Err("album not found".into()),
                Some(album) => {
                    let saved = catalog::save(state, &ctx.actor, &album.tracks);
                    Ok((json!({"saved": saved}), library.to_owned()))
                }
            },
            ["playlists", id, "remove"] => {
                let item = web::text(&body, "item");
                catalog::remove_from_playlist(state, &ctx.actor, id, &item)
                    .map(|p| (p, playlist_at(id)))
            }
            ["items", id, "play"] => {
                if record(state, "items", id).is_none() {
                    Err("item not found".into())
                } else {
                    let list = web::text(&body, "list");
                    play(
                        state,
                        id,
                        (!list.is_empty()).then_some(list.as_str()),
                        ctx,
                        false,
                    );
                    Ok((json!({"plays": num(&state["items"][id], "plays")}), item_at(id)))
                }
            }
            ["items", id, "like"] => match record(state, "items", id) {
                None => Err("item not found".into()),
                Some(_) => {
                    let liked = toggle(state, "likes", &ctx.actor, id);
                    bump(state, id, "likes", if liked { 1 } else { -1 });
                    Ok((json!({"liked": liked}), if video { item_at(id) } else { "/".to_owned() }))
                }
            },
            ["items", id, "comments"] => {
                let text = web::text(&body, "text");
                if text.trim().is_empty() {
                    Err("comment text is required".into())
                } else if let Some(item) = item_mut(state, id) {
                    let comments = item
                        .get("comments")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let comment = json!({
                        "id": next_comment_id(&comments),
                        "author": ctx.actor,
                        "text": text,
                        "tick": ctx.tick,
                        "likes": 0,
                    });
                    let mut comments = comments;
                    comments.push(comment.clone());
                    item["comments"] = Value::Array(comments);
                    Ok((comment, item_at(id)))
                } else {
                    Err("item not found".into())
                }
            }
            ["items", id, "comments", comment, "like"] => match item_mut(state, id) {
                None => Err("item not found".into()),
                Some(item) => {
                    let found = item
                        .get_mut("comments")
                        .and_then(Value::as_array_mut)
                        .and_then(|list| list.iter_mut().find(|c| web::text(c, "id") == *comment));
                    match found {
                        None => Err("comment not found".into()),
                        Some(c) => {
                            c["likes"] = json!(num(c, "likes") + 1);
                            Ok((c.clone(), item_at(id)))
                        }
                    }
                }
            },
            ["channels", id, "subscribe"] => match record(state, "channels", id) {
                None => Err("channel not found".into()),
                Some(_) => {
                    let on = toggle(state, "subscriptions", &ctx.actor, id);
                    let delta = if on { 1 } else { -1 };
                    if let Some(channel) = state.get_mut("channels").and_then(|c| c.get_mut(id)) {
                        let subs = num(channel, "subscribers") as i64 + delta;
                        channel["subscribers"] = json!(subs.max(0));
                    }
                    Ok((json!({"subscribed": on}), format!("/channel/{id}")))
                }
            },
            ["playlists"] => {
                let title = web::text(&body, "title");
                let id = slug(&title);
                if id.is_empty() {
                    Err("playlist title is required".into())
                } else if record(state, "playlists", &id).is_some() {
                    Err("conflict: playlist already exists".into())
                } else {
                    let playlist =
                        json!({"id": id, "title": title, "owner": ctx.actor, "items": []});
                    state
                        .as_object_mut()
                        .expect("state is an object")
                        .entry("playlists")
                        .or_insert_with(|| json!({}))[&id] = playlist.clone();
                    let at = playlist_at(&id);
                    Ok((playlist, at))
                }
            }
            ["playlists", id, "items"] => {
                let item = web::text(&body, "item");
                match record(state, "playlists", id).cloned() {
                    None => Err("playlist not found".into()),
                    Some(playlist) => {
                        let owner = web::text(&playlist, "owner");
                        if !owner.is_empty() && owner != ctx.actor {
                            Err(format!("playlist {id} is not writable by {}", ctx.actor))
                        } else if record(state, "items", &item).is_none() {
                            Err("item not found".into())
                        } else {
                            let mut tracks = web::strings(&playlist, "items");
                            if !tracks.contains(&item) {
                                tracks.push(item);
                            }
                            state["playlists"][id]["items"] = json!(tracks);
                            Ok((state["playlists"][id].clone(), playlist_at(id)))
                        }
                    }
                }
            }
            // The chrome search box is a form, so searching arrives as a POST.
            ["results"] | ["search"] => {
                let query = match web::text(&body, "search_query") {
                    q if q.is_empty() => web::text(&body, "q"),
                    q => q,
                };
                if !video {
                    return render(
                        state,
                        ctx,
                        &HttpRequest::get(format!("http://media/search?q={}", encode(&query))),
                        false,
                    );
                }
                return video::results(state, &query, &ctx.actor);
            }
            _ => return web::error(404, "route not found"),
        };
        match result {
            Err(message) => web::domain::<Value>(Err(message)),
            Ok((value, _)) if api => HttpResponse::json(200, &value),
            // A browser control says where it came from, so a mutation lands back on that page.
            Ok((_, fallback)) => {
                let back = match web::text(&body, "return") {
                    r if r.is_empty() => fallback,
                    r => r,
                };
                render(
                    state,
                    ctx,
                    &HttpRequest::get(format!("http://media{back}")),
                    false,
                )
            }
        }
    }
}
/// Playback bookkeeping shared by the watch page and the explicit play control.
fn play(state: &mut Value, id: &str, list: Option<&str>, ctx: &ServiceContext, view: bool) {
    if record(state, "items", id).is_none() {
        return;
    }
    bump(state, id, if view { "views" } else { "plays" }, 1);
    let mut playing = Map::new();
    playing.insert("item".into(), json!(id));
    playing.insert("tick".into(), json!(ctx.tick));
    if let Some(list) = list {
        playing.insert("list".into(), json!(list));
    }
    state
        .as_object_mut()
        .expect("state is an object")
        .entry("now_playing")
        .or_insert_with(|| json!({}))[&ctx.actor] = Value::Object(playing);
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "alice".into(),
            source: "alice-mac".into(),
            tick: 12,
            seed: 1,
            instance: "media".into(),
        }
    }
    fn video_seed() -> Value {
        json!({
            "mode": "video", "brand": "Testtube",
            "theme": {"accent": "#ff0000", "background": "#0f0f0f", "surface": "#212121",
                      "ink": "#f1f1f1", "muted": "#aaaaaa"},
            "channels": {
                "alice-builds": {"id": "alice-builds", "name": "Alice Builds",
                                 "handle": "@alicebuilds", "owner": "alice", "subscribers": 12400,
                                 "about": "Deterministic systems, slowly."},
                "carol-ships": {"id": "carol-ships", "name": "Carol Ships It",
                                "handle": "@carolships", "owner": "carol", "subscribers": 3120}
            },
            "items": {
                "atlas-walkthrough": {
                    "id": "atlas-walkthrough", "channel": "alice-builds",
                    "title": "Atlas 1.0 walkthrough", "description": "Repo: http://github.com/northstar/atlas",
                    "duration_s": 742, "published": "Mar 4, 2026", "published_tick": 7,
                    "views": 18342, "likes": 903, "tags": ["atlas", "determinism"],
                    "badges": ["4K"],
                    "comments": [{"id": "c1", "author": "bob", "text": "Clicked at 6:20.",
                                  "tick": 7, "likes": 41}],
                    "related": ["carol-postmortem"]
                },
                "carol-postmortem": {
                    "id": "carol-postmortem", "channel": "carol-ships",
                    "title": "Release postmortem", "description": "Twelve minutes.",
                    "duration_s": 1264, "published": "Feb 20, 2026", "published_tick": 5,
                    "views": 6031, "likes": 288, "tags": ["release"], "badges": ["LIVE"],
                    "comments": [], "related": ["atlas-walkthrough"]
                }
            },
            "playlists": {
                "watch-later": {"id": "watch-later", "title": "Watch later", "owner": null, "items": []},
                "kit": {"id": "kit", "title": "Kit", "owner": "carol", "items": ["carol-postmortem"]}
            },
            "subscriptions": {"bob": ["alice-builds"]},
            "likes": {"bob": ["atlas-walkthrough"]},
            "now_playing": {}
        })
    }
    fn audio_seed() -> Value {
        json!({
            "mode": "audio", "brand": "Testify",
            "theme": {"accent": "#1db954", "background": "#121212", "surface": "#181818",
                      "ink": "#ffffff", "muted": "#b3b3b3"},
            "channels": {"low-latency": {"id": "low-latency", "name": "Low Latency",
                                         "subscribers": 911403, "about": "Drum and bass."}},
            "items": {
                "backpressure": {"id": "backpressure", "channel": "low-latency",
                                 "title": "Backpressure", "album": "Backpressure",
                                 "duration_s": 256, "published_tick": 6, "plays": 1204338,
                                 "likes": 62114, "tags": ["electronic"]},
                "p99": {"id": "p99", "channel": "low-latency", "title": "P99",
                        "album": "Backpressure", "duration_s": 223, "published_tick": 8,
                        "plays": 489201, "likes": 19844, "tags": ["focus"]}
            },
            "playlists": {
                "liked": {"id": "liked", "title": "Liked Songs", "owner": null, "items": []},
                "ship-it": {"id": "ship-it", "title": "Ship It", "owner": "carol",
                            "items": ["backpressure", "p99"]}
            },
            "subscriptions": {}, "likes": {}, "now_playing": {}
        })
    }
    fn get(state: &mut Value, url: &str) -> HttpResponse {
        MediaService
            .handle(state, &ctx(), &HttpRequest::get(url))
            .unwrap()
    }
    fn post(state: &mut Value, url: &str, body: Value) -> HttpResponse {
        let request = HttpRequest::json("POST", url, &body).unwrap();
        MediaService.handle(state, &ctx(), &request).unwrap()
    }
    fn text(response: &HttpResponse) -> String {
        String::from_utf8(response.body.clone()).unwrap()
    }
    #[test]
    fn seed_shape_is_gated_at_load() {
        assert!(MediaService.initialize(json!([]), &ctx()).is_err());
        assert!(MediaService
            .initialize(json!({"mode": "video", "items": []}), &ctx())
            .is_err());
        assert!(MediaService
            .initialize(json!({"mode": "cinema"}), &ctx())
            .is_err());
        assert!(MediaService
            .initialize(json!({"mode": "video"}), &ctx())
            .is_ok());
    }
    #[test]
    fn home_grid_carries_a_card_per_video_with_its_duration_and_flags() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let page = text(&get(&mut state, "http://youtube.com/"));
        assert!(page.contains("tile-atlas-walkthrough"));
        assert!(page.contains("tile-carol-postmortem"));
        assert!(page.contains("12:22"), "duration is formatted as a clock");
        assert!(page.contains("18,342 views"), "counts are grouped");
        assert!(page.contains("tile-atlas-walkthrough-flag-4k"));
        assert!(page.contains("tile-carol-postmortem-flag-live"));
    }
    #[test]
    fn watching_counts_a_view_and_parks_now_playing() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        assert_eq!(
            get(&mut state, "http://youtube.com/watch?v=atlas-walkthrough").status,
            200
        );
        assert_eq!(state["items"]["atlas-walkthrough"]["views"], json!(18343));
        assert_eq!(
            state["now_playing"]["alice"]["item"],
            json!("atlas-walkthrough")
        );
        assert_eq!(state["now_playing"]["alice"]["tick"], json!(12));
        // youtu.be hands over the bare id, and it counts exactly the same.
        assert_eq!(
            get(&mut state, "http://youtu.be/atlas-walkthrough").status,
            200
        );
        assert_eq!(state["items"]["atlas-walkthrough"]["views"], json!(18344));
        assert_eq!(get(&mut state, "http://youtu.be/no-such-video").status, 404);
    }
    #[test]
    fn liking_is_a_toggle_that_moves_the_counter_both_ways() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let on = post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/like",
            json!({}),
        );
        assert_eq!(text(&on), r#"{"liked":true}"#);
        assert_eq!(state["items"]["atlas-walkthrough"]["likes"], json!(904));
        assert_eq!(state["likes"]["alice"], json!(["atlas-walkthrough"]));
        post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/like",
            json!({}),
        );
        assert_eq!(state["items"]["atlas-walkthrough"]["likes"], json!(903));
        assert_eq!(state["likes"]["alice"], json!([]));
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/items/nope/like",
                json!({})
            )
            .status,
            400
        );
    }
    #[test]
    fn commenting_appends_a_dense_id_and_refuses_an_empty_body() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let made = post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/comments",
            json!({"text": "The replay bit is the good part."}),
        );
        assert_eq!(made.status, 200);
        let comments = state["items"]["atlas-walkthrough"]["comments"].clone();
        assert_eq!(comments[1]["id"], json!("c2"));
        assert_eq!(comments[1]["author"], json!("alice"));
        assert_eq!(comments[1]["tick"], json!(12));
        let liked = post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/comments/c2/like",
            json!({}),
        );
        assert_eq!(liked.status, 200);
        assert_eq!(
            state["items"]["atlas-walkthrough"]["comments"][1]["likes"],
            json!(1)
        );
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/items/atlas-walkthrough/comments",
                json!({"text": "   "})
            )
            .status,
            400
        );
    }
    #[test]
    fn subscribing_toggles_the_membership_and_the_channel_count() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        post(
            &mut state,
            "http://youtube.com/api/channels/alice-builds/subscribe",
            json!({}),
        );
        assert_eq!(
            state["channels"]["alice-builds"]["subscribers"],
            json!(12401)
        );
        assert_eq!(state["subscriptions"]["alice"], json!(["alice-builds"]));
        post(
            &mut state,
            "http://youtube.com/api/channels/alice-builds/subscribe",
            json!({}),
        );
        assert_eq!(
            state["channels"]["alice-builds"]["subscribers"],
            json!(12400)
        );
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/channels/ghost/subscribe",
                json!({})
            )
            .status,
            400
        );
    }
    #[test]
    fn a_playlist_can_be_built_and_played_but_not_by_a_stranger() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        // The watch page's Save control targets the shared list, which has no owner.
        post(
            &mut state,
            "http://youtube.com/api/playlists/watch-later/items",
            json!({"item": "atlas-walkthrough"}),
        );
        assert_eq!(
            state["playlists"]["watch-later"]["items"],
            json!(["atlas-walkthrough"])
        );
        let made = post(
            &mut state,
            "http://youtube.com/api/playlists",
            json!({"title": "Launch Kit"}),
        );
        assert_eq!(made.status, 200);
        assert_eq!(state["playlists"]["launch-kit"]["owner"], json!("alice"));
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/playlists",
                json!({"title": "Launch Kit"})
            )
            .status,
            409
        );
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/playlists/kit/items",
                json!({"item": "atlas-walkthrough"})
            )
            .status,
            403,
            "carol's playlist is not alice's to edit"
        );
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/playlists/watch-later/items",
                json!({"item": "ghost"})
            )
            .status,
            400
        );
        let page = text(&get(
            &mut state,
            "http://youtube.com/playlist?list=watch-later",
        ));
        assert!(
            page.contains("playlist-play"),
            "a built playlist offers Play all"
        );
    }
    #[test]
    fn a_browser_control_lands_back_on_the_page_it_was_pressed_from() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let back = post(
            &mut state,
            "http://youtube.com/items/atlas-walkthrough/like",
            json!({"return": "/watch?v=atlas-walkthrough"}),
        );
        assert_eq!(back.status, 200);
        assert!(
            text(&back).contains("watch-title"),
            "it re-renders the watch page"
        );
        assert_eq!(
            state["items"]["atlas-walkthrough"]["views"],
            json!(18342),
            "a re-render after a mutation must not count a second view"
        );
    }
    #[test]
    fn search_finds_a_video_by_tag_and_a_channel_by_name() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let hits = text(&get(
            &mut state,
            "http://youtube.com/results?search_query=determinism",
        ));
        assert!(hits.contains("1 result for"));
        assert!(hits.contains("result-atlas-walkthrough"));
        let channels = text(&get(
            &mut state,
            "http://youtube.com/results?search_query=Carol",
        ));
        assert!(channels.contains("result-channel-carol-ships"));
        // The chrome search box is a form, so the same query also arrives as a POST.
        let posted = post(
            &mut state,
            "http://youtube.com/results",
            json!({"search_query": "atlas"}),
        );
        assert!(text(&posted).contains("result-atlas-walkthrough"));
    }
    #[test]
    fn audio_mode_plays_a_track_and_keeps_the_now_playing_bar() {
        let mut state = MediaService.initialize(audio_seed(), &ctx()).unwrap();
        let browse = text(&get(&mut state, "http://spotify.com/"));
        assert!(browse.contains("artist-low-latency"));
        assert!(browse.contains("Nothing playing"));
        let played = post(
            &mut state,
            "http://spotify.com/api/items/backpressure/play",
            json!({"list": "ship-it"}),
        );
        assert_eq!(text(&played), r#"{"plays":1204339}"#);
        assert_eq!(state["now_playing"]["alice"]["list"], json!("ship-it"));
        let bar = text(&get(&mut state, "http://spotify.com/"));
        assert!(
            bar.contains("bar-title"),
            "the bar now points at the parked track"
        );
        assert_eq!(get(&mut state, "http://spotify.com/track/p99").status, 200);
        assert_eq!(
            get(&mut state, "http://spotify.com/artist/low-latency").status,
            200
        );
        assert_eq!(
            get(&mut state, "http://spotify.com/playlist/ship-it").status,
            200
        );
        assert_eq!(
            get(&mut state, "http://spotify.com/track/ghost").status,
            404
        );
    }
    #[test]
    fn reading_pages_is_pure_and_unknown_routes_are_refused() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        for url in [
            "http://youtube.com/",
            "http://youtube.com/channel/alice-builds",
            "http://youtube.com/playlists",
            "http://youtube.com/results?search_query=atlas",
        ] {
            let page = get(&mut state, url);
            assert_eq!(page.status, 200, "{url}");
            assert_eq!(page, get(&mut state, url), "{url} must be pure");
        }
        assert_eq!(
            get(&mut state, "http://youtube.com/settings/billing").status,
            404
        );
        assert_eq!(
            get(&mut state, "http://youtube.com/channel/ghost").status,
            404
        );
        let mut head = HttpRequest::get("http://youtube.com/");
        head.method = "DELETE".into();
        assert_eq!(
            MediaService
                .handle(&mut state, &ctx(), &head)
                .unwrap()
                .status,
            405
        );
    }
    #[test]
    fn everything_a_viewer_does_survives_a_snapshot_round_trip() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        get(&mut state, "http://youtube.com/watch?v=atlas-walkthrough");
        post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/like",
            json!({}),
        );
        post(
            &mut state,
            "http://youtube.com/api/channels/alice-builds/subscribe",
            json!({}),
        );
        post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/comments",
            json!({"text": "Saved for later."}),
        );
        post(
            &mut state,
            "http://youtube.com/api/playlists/watch-later/items",
            json!({"item": "atlas-walkthrough"}),
        );
        let bytes = serde_json::to_vec(&state).unwrap();
        let mut restored: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, state);
        assert_eq!(
            text(&get(
                &mut restored,
                "http://youtube.com/playlist?list=watch-later"
            )),
            text(&get(
                &mut state,
                "http://youtube.com/playlist?list=watch-later"
            ))
        );
    }

    /// The music catalogue both music sites share, small enough to reason about: an EP of
    /// three songs (10 s, 20 s and 30 s) and a single.
    fn music_seed(mode: &str) -> Value {
        json!({
            "mode": mode, "brand": if mode == "music" { "YouTube Music" } else { "Spotify" },
            "theme": {"accent": "#ff0000", "background": "#030303", "surface": "#212121",
                      "ink": "#ffffff", "muted": "#aaaaaa"},
            "channels": {
                "cache-miss": {"id": "cache-miss", "name": "Cache Miss", "subscribers": 1320455,
                               "about": "Lo-fi beats to wait on a cold read to."},
                "tessellate": {"id": "tessellate", "name": "Tessellate", "subscribers": 29118}
            },
            "items": {
                "cold-reads": {"id": "cold-reads", "channel": "cache-miss", "title": "Cold Reads",
                               "album": "Cold Reads", "duration_s": 10, "published": "Apr 10, 2026",
                               "published_tick": 18, "plays": 100, "likes": 5, "tags": ["lo-fi", "focus"]},
                "warm-cache": {"id": "warm-cache", "channel": "cache-miss", "title": "Warm Cache",
                               "album": "Cold Reads", "duration_s": 20, "published": "Apr 10, 2026",
                               "published_tick": 19, "plays": 90, "likes": 4, "tags": ["lo-fi", "sleep"]},
                "eviction": {"id": "eviction", "channel": "cache-miss", "title": "Eviction",
                             "album": "Cold Reads", "duration_s": 30, "published": "Apr 10, 2026",
                             "published_tick": 20, "plays": 80, "likes": 3, "tags": ["relax"]},
                "tiling": {"id": "tiling", "channel": "tessellate", "title": "Tiling", "album": "Tiling",
                           "duration_s": 294, "published": "Feb 6, 2026", "published_tick": 11,
                           "plays": 60, "likes": 2, "tags": ["focus", "sleep"]}
            },
            "albums": {"cold-reads": {"genre": "Lo-Fi"}},
            "playlists": {
                "deep-focus": {"id": "deep-focus", "title": "Deep Focus", "owner": "alice", "items": ["tiling"]},
                "ship-it": {"id": "ship-it", "title": "Ship It", "owner": "carol", "items": ["eviction"]}
            },
            "subscriptions": {}, "likes": {"alice": ["tiling"]}, "now_playing": {},
            "library": {"alice": ["tiling"]}, "history": {"alice": ["tiling"]}
        })
    }
    fn at(tick: u64) -> ServiceContext {
        ServiceContext { tick, ..ctx() }
    }
    fn api(state: &mut Value, tick: u64, url: &str, body: Value) -> Value {
        let request = HttpRequest::json("POST", url, &body).unwrap();
        let reply = MediaService.handle(state, &at(tick), &request).unwrap();
        assert_eq!(reply.status, 200, "{url} {body}: {}", text(&reply));
        serde_json::from_slice(&reply.body).unwrap()
    }
    fn refused(state: &mut Value, tick: u64, url: &str, body: Value) -> u16 {
        let request = HttpRequest::json("POST", url, &body).unwrap();
        MediaService
            .handle(state, &at(tick), &request)
            .unwrap()
            .status
    }
    fn get_at(state: &mut Value, tick: u64, url: &str) -> HttpResponse {
        MediaService
            .handle(state, &at(tick), &HttpRequest::get(url))
            .unwrap()
    }
    fn json_at(state: &mut Value, tick: u64, url: &str) -> Value {
        serde_json::from_slice(&get_at(state, tick, url).body).unwrap()
    }
    const S: u64 = 1_000_000;
    const PLAYER: &str = "http://spotify.com/api/player";

    #[test]
    fn albums_are_gathered_from_their_tracks_in_running_order() {
        let state = MediaService
            .initialize(music_seed("audio"), &ctx())
            .unwrap();
        let albums = catalog::albums(&state);
        assert_eq!(albums[0].id, "cold-reads", "newest release first");
        assert_eq!(albums[0].tracks, ["cold-reads", "warm-cache", "eviction"]);
        assert_eq!(albums[0].genre, "Lo-Fi", "seed metadata wins");
        assert_eq!(albums[0].year, "2026");
        assert_eq!(albums[1].genre, "Focus", "otherwise the first tag");
        assert_eq!(albums[1].kind(), "Single");
    }

    #[test]
    fn the_player_runs_on_the_world_clock_through_the_queue() {
        let mut state = MediaService
            .initialize(music_seed("audio"), &ctx())
            .unwrap();
        let p = api(
            &mut state,
            5 * S,
            PLAYER,
            json!({"action": "play", "context": "album:cold-reads"}),
        );
        assert_eq!(p["item"], json!("cold-reads"));
        assert_eq!(p["playing"], json!(true));
        assert_eq!(state["items"]["cold-reads"]["plays"], json!(101));
        assert_eq!(state["history"]["alice"][0], json!("cold-reads"));
        // Twelve seconds later the first 10 s song is over and the second is 2 s in.
        let later = json_at(&mut state, 17 * S, "http://spotify.com/api/catalog");
        assert_eq!(later["player"]["item"], json!("warm-cache"));
        assert_eq!(later["player"]["position_ms"], json!(2000));
        // Reading is pure: nothing was written by looking.
        assert_eq!(state["now_playing"]["alice"]["index"], json!(0));
        // Seek, then pause: the clock stops moving it.
        let p = api(
            &mut state,
            17 * S,
            PLAYER,
            json!({"action": "seek", "position_ms": "15000"}),
        );
        assert_eq!(p["item"], json!("warm-cache"));
        assert_eq!(p["position_ms"], json!(15000));
        api(&mut state, 18 * S, PLAYER, json!({"action": "toggle"}));
        let paused = json_at(&mut state, 90 * S, "http://spotify.com/api/catalog");
        assert_eq!(paused["player"]["position_ms"], json!(16000));
        assert_eq!(paused["player"]["playing"], json!(false));
        // Next; nothing after the last song; previous; jump.
        assert_eq!(
            api(&mut state, 91 * S, PLAYER, json!({"action": "next"}))["item"],
            json!("eviction")
        );
        assert_eq!(
            refused(&mut state, 92 * S, PLAYER, json!({"action": "next"})),
            400
        );
        assert_eq!(
            api(&mut state, 92 * S, PLAYER, json!({"action": "previous"}))["item"],
            json!("warm-cache")
        );
        let p = api(
            &mut state,
            93 * S,
            PLAYER,
            json!({"action": "jump", "index": 0}),
        );
        assert_eq!(p["item"], json!("cold-reads"));
        assert_eq!(p["playing"], json!(true));
        // Repeat cycles off → all → one → off.
        for mode in ["all", "one", "off"] {
            assert_eq!(
                api(&mut state, 94 * S, PLAYER, json!({"action": "repeat"}))["repeat"],
                json!(mode)
            );
        }
        assert_eq!(
            refused(&mut state, 95 * S, PLAYER, json!({"action": "dance"})),
            400
        );
    }

    #[test]
    fn shuffle_is_seeded_keeps_the_song_and_restores_the_order() {
        let mut a = MediaService
            .initialize(music_seed("audio"), &ctx())
            .unwrap();
        let mut b = a.clone();
        let play = json!({"action": "play", "context": "album:cold-reads", "item": "warm-cache"});
        api(&mut a, S, PLAYER, play.clone());
        api(&mut b, S, PLAYER, play);
        let one = api(&mut a, 2 * S, PLAYER, json!({"action": "shuffle"}));
        let two = api(&mut b, 2 * S, PLAYER, json!({"action": "shuffle"}));
        assert_eq!(one, two, "the same world deals the same order");
        assert_eq!(one["queue"][0], json!("warm-cache"));
        assert_eq!(one["shuffle"], json!(true));
        let off = api(&mut a, 3 * S, PLAYER, json!({"action": "shuffle"}));
        assert_eq!(
            off["queue"],
            json!(["cold-reads", "warm-cache", "eviction"])
        );
        assert_eq!(off["item"], json!("warm-cache"));
    }

    #[test]
    fn library_queue_and_playlists_are_the_listeners_own() {
        let mut state = MediaService
            .initialize(music_seed("music"), &ctx())
            .unwrap();
        let host = "http://music.youtube.com/api";
        assert_eq!(
            api(
                &mut state,
                1,
                &format!("{host}/library/items/eviction"),
                json!({})
            )["saved"],
            json!(true)
        );
        assert_eq!(
            api(
                &mut state,
                1,
                &format!("{host}/library/albums/cold-reads"),
                json!({})
            )["saved"],
            json!(true)
        );
        assert_eq!(
            catalog::library_songs(&state, "alice"),
            ["cold-reads", "eviction", "tiling", "warm-cache"]
        );
        assert_eq!(
            api(
                &mut state,
                1,
                &format!("{host}/library/albums/cold-reads"),
                json!({})
            )["saved"],
            json!(false)
        );
        // "Play next" with nothing loaded plays; with something loaded it queues.
        let p = api(
            &mut state,
            2,
            &format!("{host}/items/tiling/queue"),
            json!({"next": "true"}),
        );
        assert_eq!(p["item"], json!("tiling"));
        let p = api(
            &mut state,
            3,
            &format!("{host}/items/eviction/queue"),
            json!({"next": "true"}),
        );
        assert_eq!(p["queue"], json!(["tiling", "eviction"]));
        assert_eq!(
            refused(
                &mut state,
                4,
                &format!("{host}/playlists/ship-it/remove"),
                json!({"item": "eviction"})
            ),
            403,
            "carol's playlist is not alice's"
        );
        api(
            &mut state,
            4,
            &format!("{host}/playlists/deep-focus/remove"),
            json!({"item": "tiling"}),
        );
        assert_eq!(state["playlists"]["deep-focus"]["items"], json!([]));
    }

    #[test]
    fn the_catalogue_and_search_answer_in_json() {
        let mut state = MediaService
            .initialize(music_seed("music"), &ctx())
            .unwrap();
        let catalog = json_at(&mut state, 1, "http://music.youtube.com/api/catalog");
        assert_eq!(catalog["albums"][0]["tracks"].as_array().unwrap().len(), 3);
        assert_eq!(catalog["playlists"][0]["editable"], json!(true));
        assert_eq!(catalog["playlists"][1]["editable"], json!(false));
        assert_eq!(catalog["liked"], json!(["tiling"]));
        assert!(catalog["player"].is_null());
        let hits = json_at(&mut state, 1, "http://music.youtube.com/api/search?q=cache");
        assert_eq!(hits["artists"], json!(["cache-miss"]));
        assert_eq!(hits["tracks"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn every_music_page_is_a_valid_page_and_reading_it_is_pure() {
        for (mode, host, routes) in [
            (
                "music",
                "music.youtube.com",
                vec![
                    "/",
                    "/?mood=relax",
                    "/explore",
                    "/explore/charts",
                    "/explore/new_releases",
                    "/explore/moods_and_genres",
                    "/library",
                    "/library/songs",
                    "/library/albums",
                    "/library/artists",
                    "/browse/cold-reads",
                    "/channel/cache-miss",
                    "/playlist?list=deep-focus",
                    "/playlist?list=LM",
                    "/search",
                    "/search?q=cold",
                    "/search?q=zzz",
                ],
            ),
            (
                "audio",
                "spotify.com",
                vec![
                    "/",
                    "/search",
                    "/search?q=cold",
                    "/album/cold-reads",
                    "/artist/cache-miss",
                    "/track/tiling",
                    "/playlist/deep-focus",
                    "/collection",
                    "/collection/tracks",
                    "/collection/albums",
                    "/queue",
                    "/playlists",
                ],
            ),
        ] {
            let mut state = MediaService.initialize(music_seed(mode), &ctx()).unwrap();
            // With something playing, every page carries the pinned player bar.
            api(
                &mut state,
                1,
                &format!("http://{host}/api/player"),
                json!({"action": "play", "context": "album:cold-reads"}),
            );
            for route in routes {
                let url = format!("http://{host}{route}");
                let page = get(&mut state, &url);
                assert_eq!(page.status, 200, "{url}");
                let body = text(&page);
                web::html::validate_strict(&body).unwrap_or_else(|e| panic!("{url}: {e:?}"));
                let dom = cw_web::html::parse(&body);
                assert!(
                    !dom.by_id("player-toggle").is_empty(),
                    "{url} has no play control"
                );
                let bar = *dom
                    .by_id("bar")
                    .first()
                    .unwrap_or_else(|| panic!("{url} has no player bar"));
                assert!(
                    dom.is(bar, "footer") && dom.has_class(bar, "bar"),
                    "{url}: the player bar is the page's pinned footer"
                );
                assert_eq!(page, get(&mut state, &url), "{url} must be pure");
            }
            assert_eq!(
                get(&mut state, &format!("http://{host}/browse/ghost")).status,
                404
            );
        }
    }

    #[test]
    fn opening_a_song_on_youtube_music_plays_it_in_its_list() {
        let mut state = MediaService
            .initialize(music_seed("music"), &ctx())
            .unwrap();
        let page = get_at(
            &mut state,
            S,
            "http://music.youtube.com/watch?v=warm-cache&list=OLAK-cold-reads",
        );
        assert_eq!(page.status, 200);
        assert!(text(&page).contains("Playing from Cold Reads"));
        assert_eq!(
            state["now_playing"]["alice"]["context"],
            json!("album:cold-reads")
        );
        assert_eq!(state["items"]["warm-cache"]["plays"], json!(91));
        // Opening it again does not restart it or count another play.
        get_at(
            &mut state,
            5 * S,
            "http://music.youtube.com/watch?v=warm-cache&list=OLAK-cold-reads",
        );
        assert_eq!(state["items"]["warm-cache"]["plays"], json!(91));
        // With no list, a song starts its artist's radio.
        get_at(&mut state, 6 * S, "http://music.youtube.com/watch?v=tiling");
        assert_eq!(
            state["now_playing"]["alice"]["context"],
            json!("station:tessellate")
        );
    }

    #[test]
    fn a_player_bar_control_lands_back_on_its_page() {
        let mut state = MediaService
            .initialize(music_seed("music"), &ctx())
            .unwrap();
        api(
            &mut state,
            1,
            "http://music.youtube.com/api/player",
            json!({"action": "play", "context": "album:cold-reads"}),
        );
        let back = post(
            &mut state,
            "http://music.youtube.com/player",
            json!({"action": "next", "return": "/browse/cold-reads"}),
        );
        assert_eq!(back.status, 200);
        assert!(
            text(&back).contains("album-header"),
            "it re-renders the album page"
        );
        assert_eq!(state["now_playing"]["alice"]["item"], json!("warm-cache"));
        let liked = post(
            &mut state,
            "http://music.youtube.com/items/warm-cache/like",
            json!({"return": "/"}),
        );
        assert_eq!(liked.status, 200);
        assert_eq!(state["likes"]["alice"], json!(["tiling", "warm-cache"]));
        let searched = post(
            &mut state,
            "http://music.youtube.com/search",
            json!({"q": "tiling"}),
        );
        assert!(text(&searched).contains("result-tiling"));
    }

    fn seed_with_extras(mode: &str) -> Value {
        let mut seed = music_seed(mode);
        seed["items"]["warm-cache"]["lyrics"] = json!([
            [2000, "first line"],
            [6000, "second line"],
            [12000, "third line"]
        ]);
        seed["devices"] = json!({"livingroom": {"name": "Living Room", "kind": "speaker",
            "url": "http://livingroom.speaker.internal/", "protocols": ["airplay", "cast"]}});
        MediaService.initialize(seed, &ctx()).unwrap()
    }

    #[test]
    fn volume_mute_and_output_are_the_listeners_and_outlast_a_new_play() {
        let mut state = seed_with_extras("audio");
        api(
            &mut state,
            0,
            PLAYER,
            json!({"action": "play", "context": "album:cold-reads"}),
        );
        let p = api(
            &mut state,
            0,
            PLAYER,
            json!({"action": "volume", "level": 35}),
        );
        assert_eq!(
            (p["volume"].clone(), p["muted"].clone()),
            (json!(35), json!(false))
        );
        let p = api(&mut state, 0, PLAYER, json!({"action": "mute"}));
        assert_eq!(p["muted"], json!(true));
        // Moving the slider unmutes, as it does in every player.
        let p = api(
            &mut state,
            0,
            PLAYER,
            json!({"action": "volume", "level": 60}),
        );
        assert_eq!(
            (p["volume"].clone(), p["muted"].clone()),
            (json!(60), json!(false))
        );
        assert_eq!(
            refused(
                &mut state,
                0,
                PLAYER,
                json!({"action": "volume", "level": 101})
            ),
            400
        );
        let p = api(
            &mut state,
            0,
            PLAYER,
            json!({"action": "output", "device": "livingroom"}),
        );
        assert_eq!(p["device"], json!("livingroom"));
        assert_eq!(p["device_name"], json!("Living Room"));
        assert_eq!(
            refused(
                &mut state,
                0,
                PLAYER,
                json!({"action": "output", "device": "attic"})
            ),
            400
        );
        // A new play keeps the volume and where the sound goes.
        let p = api(
            &mut state,
            S,
            PLAYER,
            json!({"action": "play", "context": "album:tiling"}),
        );
        assert_eq!(
            (p["volume"].clone(), p["device"].clone()),
            (json!(60), json!("livingroom"))
        );
        let catalog = json_at(&mut state, S, "http://spotify.com/api/catalog");
        assert_eq!(
            catalog["devices"][0]["protocols"],
            json!(["airplay", "cast"])
        );
        // Back to this device.
        let p = api(
            &mut state,
            S,
            PLAYER,
            json!({"action": "output", "device": ""}),
        );
        assert_eq!(p["device"], json!(""));
    }

    #[test]
    fn lyrics_are_served_with_the_catalogue_and_follow_the_clock_on_both_sites() {
        for (mode, host) in [("audio", "spotify.com"), ("music", "music.youtube.com")] {
            let mut state = seed_with_extras(mode);
            let catalog = json_at(&mut state, 0, &format!("http://{host}/api/catalog"));
            let warm = catalog["tracks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == "warm-cache")
                .unwrap()
                .clone();
            assert_eq!(warm["lyrics"][1], json!([6000, "second line"]));
            api(
                &mut state,
                0,
                &format!("http://{host}/api/player"),
                json!({"action": "play", "context": "album:cold-reads", "item": "warm-cache"}),
            );
            let page = |state: &mut Value, tick: u64| {
                let url = if mode == "audio" {
                    format!("http://{host}/lyrics")
                } else {
                    format!("http://{host}/watch?v=warm-cache&list=OLAK-cold-reads&tab=lyrics")
                };
                let reply = get_at(state, tick, &url);
                assert_eq!(reply.status, 200);
                let html = String::from_utf8(reply.body).unwrap();
                web::html::validate_strict(&html).unwrap_or_else(|e| panic!("{mode}: {e:?}"));
                cw_web::html::parse(&html)
            };
            // The lit line is the one the clock has reached: `lyric now` on its button.
            let lit = |dom: &cw_web::dom::Document| -> String {
                (0..3)
                    .find_map(|i| {
                        let node = *dom.by_id(&format!("lyric-{i}")).first()?;
                        dom.has_class(node, "now")
                            .then(|| dom.text_content(node).trim().to_owned())
                    })
                    .unwrap_or_else(|| "none".to_owned())
            };
            assert_eq!(lit(&page(&mut state, S)), "none", "{mode}");
            assert_eq!(lit(&page(&mut state, 3 * S)), "first line", "{mode}");
            assert_eq!(lit(&page(&mut state, 7 * S)), "second line", "{mode}");
            // A line is a seek control to where it is sung: its form posts that position.
            let dom = page(&mut state, 7 * S);
            let form = *dom.by_id("lyric-2-form").first().unwrap();
            assert_eq!(dom.attr(form, "action"), Some("/player"), "{mode}");
            assert_eq!(dom.attr(form, "method"), Some("post"), "{mode}");
            let seek = dom
                .descendants(form)
                .find(|n| dom.attr(*n, "name") == Some("position_ms"))
                .unwrap_or_else(|| panic!("{mode}: the line carries no seek"));
            assert_eq!(dom.attr(seek, "value"), Some("12000"), "{mode}");
        }
    }

    #[test]
    fn covers_are_served_as_page_images_the_size_the_page_asks_for() {
        let mut state = seed_with_extras("audio");
        let reply = get_at(
            &mut state,
            0,
            "http://spotify.com/art/cold-reads?size=40&radius=20",
        );
        assert_eq!(reply.status, 200);
        assert_eq!(
            reply.header("content-type"),
            Some(cw_protocol::RGBA_MEDIA_TYPE)
        );
        let image: Value = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(
            (image["width"].clone(), image["height"].clone()),
            (json!(40), json!(40))
        );
        let rgba = image["rgba"].as_array().unwrap();
        assert_eq!(rgba.len(), 40 * 40 * 4);
        // Round: the corner is transparent.
        assert_eq!(rgba[3], json!(0));
        // The same key is the same picture every time; another key is another picture.
        let again = get_at(
            &mut state,
            5,
            "http://spotify.com/art/cold-reads?size=40&radius=20",
        );
        assert_eq!(again.body, reply.body);
        let other = get_at(
            &mut state,
            5,
            "http://spotify.com/art/tiling?size=40&radius=20",
        );
        assert_ne!(other.body, reply.body);
        // Pages use them, with the album named as the picture's alternative text.
        let page = text(&get_at(
            &mut state,
            0,
            "http://spotify.com/album/cold-reads",
        ));
        assert!(page.contains("/art/cold-reads?size=180"));
    }
}
