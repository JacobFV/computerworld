//! Every page of every skin, through the strict validator.
//!
//! The service backs eight shipped sites and three modes, and each site picks its own
//! stylesheet; a rule one skin needs and the engine will not render only shows up when
//! that skin's pages are parsed and cascaded. So this walks each site's seed as it is
//! shipped, fetches every page kind its mode has, and runs `validate_strict` over the
//! HTML: the ids are unique, the markup parses, and every declaration in the sheet is
//! one `cw-web` renders in strict mode.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::{validate_strict, HTML_MEDIA_TYPE};
use cw_service_media::MediaService;
use serde_json::{json, Value};
use std::path::PathBuf;

/// The eight sites this service backs, in the order the world file lists them.
const SITES: [&str; 8] = [
    "youtube",
    "netflix",
    "twitch",
    "vimeo",
    "tiktok",
    "spotify",
    "soundcloud",
    "youtube-music",
];

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 1,
        instance: "media".into(),
    }
}
fn site(name: &str) -> (String, Value) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../worlds/internet/sites/{name}.json"));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let host = file["domains"][0].as_str().unwrap().to_owned();
    let state = MediaService
        .initialize(file["initial_state"].clone(), &ctx())
        .unwrap_or_else(|e| panic!("{name} seed: {e}"));
    (host, state)
}
fn first(state: &Value, map: &str) -> String {
    state[map]
        .as_object()
        .and_then(|m| m.keys().next().cloned())
        .unwrap_or_else(|| panic!("{map} is empty"))
}
/// Fetch a page and hold it to the contract: HTML, and strict-clean.
fn check(state: &mut Value, site: &str, host: &str, path: &str) -> cw_web::dom::Document {
    let mut request = HttpRequest::get(format!("http://{host}{path}"));
    // A refresh, so walking the catalogue neither counts views nor starts playback.
    request
        .headers
        .insert(cw_protocol::REFRESH_HEADER.into(), "1".into());
    let response = MediaService.handle(state, &ctx(), &request).unwrap();
    assert_eq!(response.status, 200, "{site} GET {path}");
    assert_eq!(
        response.header("content-type"),
        Some(HTML_MEDIA_TYPE),
        "{site} GET {path}"
    );
    let html = String::from_utf8(response.body).unwrap();
    validate_strict(&html).unwrap_or_else(|e| panic!("{site} GET {path}: {e:?}"));
    cw_web::html::parse(&html)
}

#[test]
fn every_page_of_every_skin_is_strict_html() {
    for name in SITES {
        let (host, mut state) = site(name);
        let mode = state["mode"].as_str().unwrap().to_owned();
        let item = first(&state, "items");
        let channel = first(&state, "channels");
        let playlist = first(&state, "playlists");
        // Something playing, so the pinned bar and the live controls are on every page.
        let play = HttpRequest::json(
            "POST",
            format!("http://{host}/api/player"),
            &json!({"action": "play", "item": item}),
        )
        .unwrap();
        if mode != "video" {
            MediaService.handle(&mut state, &ctx(), &play).unwrap();
        }
        let mut paths = vec!["/".to_owned()];
        match mode.as_str() {
            "video" => paths.extend([
                format!("/watch?v={item}"),
                format!("/channel/{channel}"),
                format!("/playlist?list={playlist}"),
                "/playlists".to_owned(),
                "/results?search_query=a".to_owned(),
                "/results?search_query=zzzznothing".to_owned(),
            ]),
            "audio" => paths.extend([
                format!("/track/{item}"),
                format!("/artist/{channel}"),
                format!("/playlist/{playlist}"),
                "/collection".to_owned(),
                "/collection/tracks".to_owned(),
                "/collection/albums".to_owned(),
                "/collection/artists".to_owned(),
                "/queue".to_owned(),
                "/lyrics".to_owned(),
                "/search".to_owned(),
                "/search?q=a".to_owned(),
                "/search?q=zzzznothing".to_owned(),
            ]),
            _ => paths.extend([
                format!("/watch?v={item}"),
                format!("/watch?v={item}&tab=lyrics"),
                format!("/channel/{channel}"),
                format!("/playlist?list={playlist}"),
                "/playlist?list=LM".to_owned(),
                "/explore".to_owned(),
                "/explore/new_releases".to_owned(),
                "/explore/charts".to_owned(),
                "/explore/moods_and_genres".to_owned(),
                "/library".to_owned(),
                "/library/songs".to_owned(),
                "/library/albums".to_owned(),
                "/library/artists".to_owned(),
                "/search".to_owned(),
                "/search?q=a".to_owned(),
            ]),
        }
        for path in &paths {
            let dom = check(&mut state, name, &host, path);
            // The skin is on the body, so a stylesheet rule can key on it.
            let body = dom
                .body()
                .unwrap_or_else(|| panic!("{name} {path}: no body"));
            let classes: Vec<&str> = dom.classes(body).collect();
            assert!(
                classes.iter().any(|c| c.starts_with("skin-")),
                "{name} {path}: body carries no skin class, only {classes:?}"
            );
        }
    }
}
