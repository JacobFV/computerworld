//! The shipped seeds for youtube.com and spotify.com, exercised the way an agent would meet them.
//!
//! `include_str!` rather than a file read: the seed is a build input, so a drift between the world
//! data and the crate that serves it fails to compile instead of failing at simulation time.
use cw_protocol::{HttpRequest, HttpResponse};
use cw_sdk::{Service, ServiceContext};
use cw_service_media::MediaService;
use serde_json::Value;
const YOUTUBE: &str = include_str!("../../../../worlds/company-2026/sites/youtube.json");
const SPOTIFY: &str = include_str!("../../../../worlds/company-2026/sites/spotify.json");
const YOUTUBE_MUSIC: &str = include_str!("../../../../worlds/company-2026/sites/youtube-music.json");
fn ctx() -> ServiceContext {
    ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 1,
        instance: "media".into(),
    }
}
/// The site file, its loaded state, and the build-only entries the search engines will index.
fn site(raw: &str) -> (Value, Value, Vec<String>) {
    let file: Value = serde_json::from_str(raw).expect("site file parses");
    let state = MediaService
        .initialize(file["initial_state"].clone(), &ctx())
        .expect("seed passes the service's own gate");
    let entries = file["search_entries"]
        .as_array()
        .expect("search entries are an array")
        .iter()
        .map(|e| e["url"].as_str().expect("entry has a url").to_owned())
        .collect();
    (file, state, entries)
}
fn get(state: &mut Value, url: &str) -> HttpResponse {
    MediaService
        .handle(state, &ctx(), &HttpRequest::get(url))
        .expect("the service answers")
}
fn keys(state: &Value, map: &str) -> Vec<String> {
    state[map]
        .as_object()
        .expect("map is an object")
        .keys()
        .cloned()
        .collect()
}
#[test]
fn every_youtube_page_a_seed_names_resolves() {
    let (_, mut state, entries) = site(YOUTUBE);
    assert_eq!(get(&mut state, "http://youtube.com/").status, 200);
    for id in keys(&state, "items") {
        assert_eq!(
            get(&mut state, &format!("http://youtube.com/watch?v={id}")).status,
            200
        );
        // youtu.be is the same catalogue reached by a bare id.
        assert_eq!(
            get(&mut state, &format!("http://youtu.be/{id}")).status,
            200,
            "{id}"
        );
    }
    for id in keys(&state, "channels") {
        assert_eq!(
            get(&mut state, &format!("http://youtube.com/channel/{id}")).status,
            200
        );
    }
    for id in keys(&state, "playlists") {
        assert_eq!(
            get(
                &mut state,
                &format!("http://youtube.com/playlist?list={id}")
            )
            .status,
            200
        );
    }
    for url in &entries {
        assert_eq!(
            get(&mut state, url).status,
            200,
            "indexed page {url} must resolve"
        );
    }
}
#[test]
fn every_spotify_page_a_seed_names_resolves() {
    let (_, mut state, entries) = site(SPOTIFY);
    assert_eq!(get(&mut state, "http://spotify.com/").status, 200);
    for id in keys(&state, "items") {
        assert_eq!(
            get(&mut state, &format!("http://spotify.com/track/{id}")).status,
            200
        );
    }
    for id in keys(&state, "channels") {
        assert_eq!(
            get(&mut state, &format!("http://spotify.com/artist/{id}")).status,
            200
        );
    }
    for id in keys(&state, "playlists") {
        assert_eq!(
            get(&mut state, &format!("http://spotify.com/playlist/{id}")).status,
            200
        );
    }
    for url in &entries {
        assert_eq!(
            get(&mut state, url).status,
            200,
            "indexed page {url} must resolve"
        );
    }
}
#[test]
fn the_seeded_controls_are_not_decoration() {
    let (_, youtube, _) = site(YOUTUBE);
    // The watch page's Save posts here and the track page's Add to playlist posts there; both
    // lists have to exist, unowned, or those buttons are lies.
    assert_eq!(youtube["playlists"]["watch-later"]["owner"], Value::Null);
    let (_, spotify, _) = site(SPOTIFY);
    assert_eq!(spotify["playlists"]["liked"]["owner"], Value::Null);
    // Every item names a channel that exists, or its card renders a raw id.
    for (state, catalogue) in [(&youtube, "youtube"), (&spotify, "spotify")] {
        for id in keys(state, "items") {
            let channel = state["items"][&id]["channel"].as_str().unwrap_or_default();
            assert!(
                state["channels"].get(channel).is_some(),
                "{catalogue}: {id} names an unknown channel {channel}"
            );
        }
        for list in keys(state, "playlists") {
            for track in state["playlists"][&list]["items"].as_array().unwrap() {
                let track = track.as_str().unwrap_or_default();
                assert!(
                    state["items"].get(track).is_some(),
                    "{catalogue}: playlist {list} names an unknown item {track}"
                );
            }
        }
    }
}
#[test]
fn the_storylines_the_content_bible_pins_are_present() {
    let (_, youtube, _) = site(YOUTUBE);
    // Storyline 7: the walkthrough's top comment is bob's.
    let comments = youtube["items"]["atlas-walkthrough"]["comments"]
        .as_array()
        .unwrap();
    assert_eq!(comments[0]["author"], "bob");
    assert_eq!(
        youtube["items"]["atlas-walkthrough"]["channel"],
        "alice-builds"
    );
    assert_eq!(
        youtube["channels"]["alice-builds"]["handle"],
        "@alicebuilds"
    );
    let (_, spotify, _) = site(SPOTIFY);
    // Storyline 7: Carol's "Ship It" is eleven tracks long.
    assert_eq!(spotify["playlists"]["ship-it"]["owner"], "carol");
    assert_eq!(
        spotify["playlists"]["ship-it"]["items"]
            .as_array()
            .unwrap()
            .len(),
        11
    );
    // Storyline 1 keeps the release code to the doc, the mail and the tracker; not here.
    assert!(!YOUTUBE.contains("ATLAS-2026"));
    assert!(!SPOTIFY.contains("ATLAS-2026"));
}
#[test]
fn watching_the_seeded_walkthrough_changes_the_world() {
    let (_, mut state, _) = site(YOUTUBE);
    let before = state["items"]["atlas-walkthrough"]["views"]
        .as_u64()
        .unwrap();
    get(&mut state, "http://youtube.com/watch?v=atlas-walkthrough");
    assert_eq!(
        state["items"]["atlas-walkthrough"]["views"]
            .as_u64()
            .unwrap(),
        before + 1
    );
    assert_eq!(state["now_playing"]["alice"]["item"], "atlas-walkthrough");
}
#[test]
fn every_youtube_music_page_a_seed_names_resolves_and_plays() {
    let (file, mut state, entries) = site(YOUTUBE_MUSIC);
    assert_eq!(file["domains"][0], "music.youtube.com");
    assert_eq!(state["mode"], "music");
    for id in keys(&state, "channels") {
        assert_eq!(
            get(
                &mut state,
                &format!("http://music.youtube.com/channel/{id}")
            )
            .status,
            200
        );
    }
    for id in keys(&state, "playlists") {
        assert_eq!(
            get(
                &mut state,
                &format!("http://music.youtube.com/playlist?list={id}")
            )
            .status,
            200
        );
    }
    for url in &entries {
        assert_eq!(
            get(&mut state, url).status,
            200,
            "indexed page {url} must resolve"
        );
    }
    // Opening the last indexed song left it playing, with a real queue behind it.
    let player = &state["now_playing"]["alice"];
    assert_eq!(player["playing"], true);
    assert!(player["queue"].as_array().is_some_and(|q| !q.is_empty()));
    // The shared catalogue: the same songs spotify.com serves.
    let (_, spotify, _) = site(SPOTIFY);
    assert_eq!(keys(&state, "items"), keys(&spotify, "items"));
}
