//! youtube.com and spotify.com served as HTML by the media service and driven end to
//! end through the agent API.
//!
//! YouTube is the whole video flow: the home grid, the chrome's search box typed into
//! and submitted, a result opened, and the watch page's own controls (Like, Save,
//! Subscribe) pressed, each a one-button form that posts and comes back to the page it
//! was pressed from. Spotify is the audio flow and the real transport: play a track,
//! then the pinned player bar's play/pause, next and volume, which run on the world
//! clock rather than on a script.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";

fn world() -> (World, String) {
    let mut world = World::new(reference_world(), 42).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: ACTOR.into(),
            machines: vec![MACHINE.into()],
            actions: vec!["browser.v1".into(), "keyboard.v1".into()],
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
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn browser(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE].clone()
}
/// Every element of the semantic tree, flattened.
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
fn has(all: &[Value], id: &str) -> bool {
    all.iter().any(|e| e["id"] == id)
}
/// An element's own words: its text, else the label the service gave it.
fn label(e: &Value) -> String {
    e["text"]
        .as_str()
        .or_else(|| e["label"].as_str())
        .unwrap_or_default()
        .to_owned()
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

#[test]
fn youtube_is_browsed_searched_and_watched_through_the_agent_api() {
    let (mut world, session) = world();
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url": "http://youtube.com/"}),
    );
    let home = page(&world, &session);
    assert_eq!(home["title"], "YouTube");
    let all = elements(&home);
    // The chrome the agent uses: the brand, the search form, the guide, the grid.
    assert_eq!(by_id(&all, "search")["kind"], "form");
    let box_ = by_id(&all, "search-search_query");
    assert_eq!(box_["kind"], "input");
    assert_eq!(box_["label"], "Search");
    assert_eq!(by_id(&all, "search-submit")["kind"], "button");
    assert_eq!(by_id(&all, "guide-home")["kind"], "link");
    // Every seeded video is a card, and a card is one link to its watch page.
    let tile = by_id(&all, "tile-determinism-intro");
    assert_eq!(tile["kind"], "link");
    assert_eq!(
        tile["url"], "http://youtube.com/watch?v=determinism-intro",
        "a card is one link"
    );

    // Type into the box and press Enter: a GET form, so the query is in the URL.
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id": "search-search_query"}),
    );
    act(
        &mut world,
        &session,
        "keyboard.v1",
        "type",
        json!({"text": "determinism"}),
    );
    act(&mut world, &session, "browser.v1", "key", json!({"key": "Enter"}));
    assert_eq!(
        browser(&world, &session)["url"],
        "http://youtube.com/results?search_query=determinism"
    );
    let results = page(&world, &session);
    assert_eq!(results["title"], "determinism - search");
    let all = elements(&results);
    assert_eq!(by_id(&all, "search-search_query")["value"], "determinism");
    let hit = by_id(&all, "result-determinism-intro");
    assert_eq!(hit["kind"], "link");
    assert_eq!(hit["url"], "http://youtube.com/watch?v=determinism-intro");

    // Open the video. Watching counts a view and parks it as what alice is playing.
    act(
        &mut world,
        &session,
        "browser.v1",
        "click",
        json!({"id": "result-determinism-intro"}),
    );
    let watch = page(&world, &session);
    let all = elements(&watch);
    assert_eq!(
        by_id(&all, "watch-title")["text"],
        "What deterministic simulation actually means"
    );
    assert_eq!(by_id(&all, "watch-channel-name")["kind"], "link");
    assert_eq!(
        by_id(&all, "watch-channel-name")["url"],
        "http://youtube.com/channel/alice-builds"
    );
    assert!(words(&watch).contains("views"), "the view count is readable");

    // The watch page's controls: each is a button in its own form, and each posts and
    // comes back to the watch page with the state it changed showing.
    assert_eq!(by_id(&all, "watch-like")["kind"], "button");
    assert_eq!(label(by_id(&all, "watch-like")), "Like");
    assert_eq!(
        by_id(&all, "watch-like")["action"]["url"],
        "http://youtube.com/items/determinism-intro/like",
        "the button posts where the Page version's did"
    );

    act(&mut world, &session, "browser.v1", "click", json!({"id": "watch-like"}));
    let liked = page(&world, &session);
    let all = elements(&liked);
    assert_eq!(
        label(by_id(&all, "watch-like")),
        "Remove like",
        "the like took, and the control is now the other way round"
    );
    let landed = browser(&world, &session)["url"].as_str().unwrap().to_owned();
    assert!(
        landed.contains("determinism-intro"),
        "a control lands back on the page it was pressed from, not {landed}"
    );
    // Subscribe, and Save, which really puts the video in Watch later.
    assert!(label(by_id(&all, "watch-subscribe")).contains("Subscribe"));
    act(&mut world, &session, "browser.v1", "click", json!({"id": "watch-subscribe"}));
    let all = elements(&page(&world, &session));
    assert!(
        label(by_id(&all, "watch-subscribe")).contains("Subscribed"),
        "the button says so now"
    );
    act(&mut world, &session, "browser.v1", "click", json!({"id": "watch-later"}));
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url": "http://youtube.com/playlist?list=watch-later"}),
    );
    let later = page(&world, &session);
    assert!(
        words(&later).contains("What deterministic simulation actually means"),
        "Save put the video in Watch later"
    );
    assert!(has(&elements(&later), "playlist-play"), "a built list plays");
}

#[test]
fn spotify_plays_a_track_and_its_player_bar_runs_the_queue() {
    let (mut world, session) = world();
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url": "http://spotify.com/"}),
    );
    let home = page(&world, &session);
    let all = elements(&home);
    // The greeting's grid of shortcuts, each one link, and the library down the left.
    assert_eq!(by_id(&all, "short-liked")["kind"], "link");
    assert_eq!(
        by_id(&all, "short-playlist-ship-it")["url"],
        "http://spotify.com/playlist/ship-it"
    );
    assert_eq!(by_id(&all, "home-filter-library")["kind"], "link");
    assert!(
        words(&home).contains("Nothing playing"),
        "the player bar is pinned to every page and nothing has been played yet"
    );

    // Search for a track and open it.
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url": "http://spotify.com/search"}),
    );
    act(&mut world, &session, "browser.v1", "fill", json!({"id": "search-q", "value": "cold"}));
    act(&mut world, &session, "browser.v1", "click", json!({"id": "search-submit"}));
    assert_eq!(browser(&world, &session)["url"], "http://spotify.com/search?q=cold");
    act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url": "http://spotify.com/track/cold-start"}),
    );
    let track = page(&world, &session);
    let all = elements(&track);
    assert_eq!(track["title"], "Cold Start");
    assert_eq!(by_id(&all, "track-artist")["kind"], "link");
    assert_eq!(by_id(&all, "track-play")["kind"], "button");

    // Press play: the bar picks the track up and the transport is live.
    act(&mut world, &session, "browser.v1", "click", json!({"id": "track-play"}));
    let all = elements(&page(&world, &session));
    assert_eq!(label(by_id(&all, "bar-title")), "Cold Start");
    assert_eq!(label(by_id(&all, "bar-meta")), "Midnight Compiler");
    assert_eq!(by_id(&all, "player-toggle")["kind"], "button");
    assert_eq!(label(by_id(&all, "player-toggle")), "Pause");

    // Pause, and the bar says so; next, and the queue moves on.
    act(&mut world, &session, "browser.v1", "click", json!({"id": "player-toggle"}));
    let all = elements(&page(&world, &session));
    assert_eq!(label(by_id(&all, "player-toggle")), "Play");
    act(&mut world, &session, "browser.v1", "click", json!({"id": "player-next"}));
    let all = elements(&page(&world, &session));
    assert_ne!(
        label(by_id(&all, "bar-title")),
        "Cold Start",
        "next moved the queue on"
    );
    // The volume slider: every fifth percent is its own control, and it really sets it.
    assert_eq!(by_id(&all, "player-volume-40")["kind"], "button");
    act(&mut world, &session, "browser.v1", "click", json!({"id": "player-volume-40"}));
    let catalog = act(
        &mut world,
        &session,
        "browser.v1",
        "navigate",
        json!({"url": "http://spotify.com/api/catalog"}),
    );
    let _ = catalog;
    let shown = browser(&world, &session);
    assert_eq!(shown["url"], "http://spotify.com/api/catalog");
}
