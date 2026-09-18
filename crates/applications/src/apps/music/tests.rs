use super::*;
use serde_json::json;

/// A catalogue reply shaped exactly like the media service's `GET /api/catalog`.
fn catalog(player: serde_json::Value) -> String {
    json!({
        "brand": "Spotify",
        "artists": [
            {"id": "cache-miss", "name": "Cache Miss", "followers": 1320455, "about": "Lo-fi."},
            {"id": "tessellate", "name": "Tessellate", "followers": 29118, "about": "Piano."}
        ],
        "albums": [
            {"id": "cold-reads", "title": "Cold Reads", "artist": "cache-miss", "year": "2026",
             "genre": "Lo-Fi", "kind": "EP", "tracks": ["cold-reads", "warm-cache", "eviction"]},
            {"id": "tiling", "title": "Tiling", "artist": "tessellate", "year": "2026",
             "genre": "Classical", "kind": "Single", "tracks": ["tiling"]}
        ],
        "tracks": [
            {"id": "cold-reads", "title": "Cold Reads", "artist": "cache-miss", "album": "cold-reads",
             "duration_ms": 10000, "plays": 9, "tags": ["focus", "lo-fi"]},
            {"id": "warm-cache", "title": "Warm Cache", "artist": "cache-miss", "album": "cold-reads",
             "duration_ms": 20000, "plays": 8, "tags": ["sleep"]},
            {"id": "eviction", "title": "Eviction", "artist": "cache-miss", "album": "cold-reads",
             "duration_ms": 30000, "plays": 7, "tags": ["relax"]},
            {"id": "tiling", "title": "Tiling", "artist": "tessellate", "album": "tiling",
             "duration_ms": 294000, "plays": 6, "tags": ["focus"]}
        ],
        "playlists": [
            {"id": "deep-focus", "title": "Deep Focus", "owner": "alice", "items": ["tiling"], "editable": true},
            {"id": "ship-it", "title": "Ship It", "owner": "carol", "items": ["eviction"], "editable": false}
        ],
        "liked": ["tiling"],
        "library": ["tiling", "cold-reads"],
        "history": ["cold-reads"],
        "subscriptions": ["tessellate"],
        "player": player
    })
    .to_string()
}
fn playing(tick: u64, playing: bool) -> serde_json::Value {
    json!({"item": "cold-reads", "index": 0, "queue": ["cold-reads", "warm-cache", "eviction"],
           "context": "album:cold-reads", "context_title": "Cold Reads", "position_ms": 0,
           "duration_ms": 10000, "playing": playing, "shuffle": false, "repeat": "off", "tick": tick})
}
fn app_with(player: serde_json::Value) -> Music {
    let (mut app, effects) = Music::launch("http://spotify.com/", 1, 0);
    assert!(matches!(
        effects.as_slice(),
        [AppEffect::Http { url, tag, method, .. }]
            if url == "http://spotify.com/api/catalog" && tag == "catalog" && method == "GET"
    ));
    app.http(1, "catalog", 200, &catalog(player)).unwrap();
    app
}
fn request(effects: &[AppEffect]) -> (String, String, serde_json::Value) {
    let [AppEffect::Http {
        method, url, body, ..
    }] = effects
    else {
        panic!("expected one request, got {effects:?}");
    };
    (
        method.clone(),
        url.clone(),
        serde_json::from_str(body).unwrap_or(serde_json::Value::Null),
    )
}
const S: u64 = 1_000_000;

#[test]
fn the_catalogue_is_the_services_reply_and_nothing_else() {
    let app = app_with(serde_json::Value::Null);
    assert_eq!(app.status, Status::Idle);
    assert!(app.loaded);
    assert_eq!(app.catalog.tracks.len(), 4);
    assert_eq!(app.catalog.library_albums().len(), 2);
    assert_eq!(
        app.catalog
            .library_songs()
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["cold-reads", "tiling"]
    );
    assert_eq!(app.catalog.top_songs("cache-miss")[0].id, "cold-reads");
    assert!(app.now().is_none());
    assert!(app.live(0).is_none());
}

#[test]
fn transport_commands_post_to_the_player_and_then_re_read() {
    let mut app = app_with(playing(0, true));
    let (method, url, body) = request(
        &app.click(1, "music:play:album:cold-reads@warm-cache", 0)
            .unwrap(),
    );
    assert_eq!(
        (method.as_str(), url.as_str()),
        ("POST", "http://spotify.com/api/player")
    );
    assert_eq!(
        body,
        json!({"action": "play", "context": "album:cold-reads", "item": "warm-cache"})
    );
    let again = app
        .http(1, "done", 200, r#"{"item":"warm-cache"}"#)
        .unwrap();
    assert!(matches!(again.as_slice(), [AppEffect::Http { tag, .. }] if tag == "catalog"));
    for (target, action) in [
        ("music:toggle", "toggle"),
        ("music:shuffle", "shuffle"),
        ("music:repeat", "repeat"),
        ("music:previous", "previous"),
        ("music:next", "next"),
    ] {
        let (_, _, body) = request(&app.click(1, target, 0).unwrap());
        assert_eq!(body["action"], action, "{target}");
    }
    let (_, _, body) = request(&app.click(1, "music:shuffle-play:library", 0).unwrap());
    assert_eq!(
        body,
        json!({"action": "play", "context": "library", "shuffle": true})
    );
    assert!(app.click(1, "music:play:album:ghost", 0).is_err());
    assert!(app
        .click(1, "music:play:album:cold-reads@ghost", 0)
        .is_err());
}

#[test]
fn the_world_clock_moves_the_position_across_tracks_like_the_service() {
    let app = app_with(playing(5 * S, true));
    let live = app.live(9 * S).unwrap();
    assert_eq!(
        (live.index, live.position_ms, live.playing),
        (0, 4_000, true)
    );
    // 10 s into the queue is the start of the second track; 12 s is 2 s into it.
    let live = app.live(17 * S).unwrap();
    assert_eq!((live.id.as_str(), live.position_ms), ("warm-cache", 2_000));
    // Past the whole queue with repeat off: stopped at the last track's start.
    let live = app.live(200 * S).unwrap();
    assert_eq!((live.index, live.position_ms, live.playing), (2, 0, false));
    // Paused, the clock changes nothing.
    let paused = app_with(playing(0, false));
    assert_eq!(paused.live(50 * S).unwrap().position_ms, 0);
    // Repeat one keeps the track and wraps within it.
    let mut one = playing(0, true);
    one["repeat"] = json!("one");
    let one = app_with(one);
    let live = one.live(34 * S).unwrap();
    assert_eq!((live.index, live.position_ms), (0, 4_000));
}

#[test]
fn seeking_names_a_share_of_the_track_and_next_knows_the_end() {
    let mut app = app_with(playing(0, true));
    let (_, _, body) = request(&app.click(1, "music:seek:500", 0).unwrap());
    assert_eq!(body, json!({"action": "seek", "position_ms": 5000}));
    assert!(app.click(1, "music:seek:1000", 0).is_err());
    assert!(app.can_next(0));
    // On the last track with repeat off there is nowhere for Next to go.
    assert!(!app.can_next(35 * S));
    assert!(app.click(1, "music:next", 35 * S).is_err());
    let mut all = playing(0, true);
    all["repeat"] = json!("all");
    assert!(app_with(all).can_next(35 * S));
    let (_, _, body) = request(&app.click(1, "music:jump:2", 0).unwrap());
    assert_eq!(body, json!({"action": "jump", "index": 2}));
    assert!(app.click(1, "music:jump:9", 0).is_err());
}

#[test]
fn nothing_loaded_means_no_transport() {
    let mut app = app_with(serde_json::Value::Null);
    for target in [
        "music:toggle",
        "music:next",
        "music:previous",
        "music:seek:10",
        "music:expand",
    ] {
        assert!(app.click(1, target, 0).is_err(), "{target}");
    }
}

#[test]
fn the_song_menu_adds_to_playlists_it_may_edit_and_nowhere_else() {
    let mut app = app_with(serde_json::Value::Null);
    app.click(1, "music:menu:eviction", 0).unwrap();
    let items = app.menu_items(DesktopTheme::Macos);
    assert!(items.iter().any(|i| i.label == "Add to Library"));
    assert!(items.iter().any(|i| i.label == "Favorite"));
    app.click(1, "music:menu-playlists", 0).unwrap();
    let lists = app.menu_items(DesktopTheme::Macos);
    let ship = lists.iter().find(|i| i.label == "Ship It").unwrap();
    assert!(
        ship.target.is_none(),
        "carol's playlist is not alice's to change"
    );
    assert!(app.click(1, "music:add:ship-it", 0).is_err());
    let (_, url, body) = request(&app.click(1, "music:add:deep-focus", 0).unwrap());
    assert_eq!(url, "http://spotify.com/api/playlists/deep-focus/items");
    assert_eq!(body, json!({"item": "eviction"}));
    assert!(app.menu.is_none());
    // Removing is offered only inside a playlist the listener may edit.
    app.click(1, "music:playlist:deep-focus", 0).unwrap();
    app.click(1, "music:menu:tiling", 0).unwrap();
    assert!(app
        .menu_items(DesktopTheme::Macos)
        .iter()
        .any(|i| i.target.as_deref() == Some("music:remove:deep-focus@tiling")));
    let (_, url, _) = request(&app.click(1, "music:remove:deep-focus@tiling", 0).unwrap());
    assert_eq!(url, "http://spotify.com/api/playlists/deep-focus/remove");
    assert!(app.click(1, "music:remove:ship-it@eviction", 0).is_err());
    // YouTube Music words the same menu its own way.
    app.click(1, "music:menu:eviction", 0).unwrap();
    let yt = app.menu_items(DesktopTheme::Android);
    assert_eq!(yt[0].label, "Start radio");
    assert!(yt.iter().any(|i| i.label == "Save to library"));
}

#[test]
fn library_likes_and_follows_are_real_requests() {
    let mut app = app_with(serde_json::Value::Null);
    let (_, url, _) = request(&app.click(1, "music:save:eviction", 0).unwrap());
    assert_eq!(url, "http://spotify.com/api/library/items/eviction");
    let (_, url, _) = request(&app.click(1, "music:save-album:cold-reads", 0).unwrap());
    assert_eq!(url, "http://spotify.com/api/library/albums/cold-reads");
    let (_, url, _) = request(&app.click(1, "music:like:tiling", 0).unwrap());
    assert_eq!(url, "http://spotify.com/api/items/tiling/like");
    let (_, url, _) = request(&app.click(1, "music:subscribe:cache-miss", 0).unwrap());
    assert_eq!(url, "http://spotify.com/api/channels/cache-miss/subscribe");
    let (_, url, body) = request(&app.click(1, "music:play-next:tiling", 0).unwrap());
    assert_eq!(
        (url.as_str(), body["next"].as_str()),
        ("http://spotify.com/api/items/tiling/queue", Some("true"))
    );
}

#[test]
fn searching_asks_the_service_and_navigation_keeps_a_back_stack() {
    let mut app = app_with(serde_json::Value::Null);
    assert!(app.click(1, "music:search", 0).is_err());
    app.click(1, "music:search-field", 0).unwrap();
    assert!(app.takes_text());
    app.text("cold reads").unwrap();
    let (method, url, _) = request(&app.click(1, "music:search", 0).unwrap());
    assert_eq!(
        (method.as_str(), url.as_str()),
        ("GET", "http://spotify.com/api/search?q=cold%20reads")
    );
    assert!(!app.takes_text());
    app.http(1, "search", 200, r#"{"query":"cold reads","tracks":["cold-reads"],"albums":["cold-reads"],"artists":[],"playlists":[]}"#)
        .unwrap();
    assert_eq!(app.results.as_ref().unwrap().tracks, ["cold-reads"]);
    assert_eq!(app.view, View::Search);
    app.click(1, "music:album:cold-reads", 0).unwrap();
    app.click(1, "music:artist:cache-miss", 0).unwrap();
    app.click(1, "music:back", 0).unwrap();
    assert_eq!(app.view, View::Album("cold-reads".into()));
    app.click(1, "music:home", 0).unwrap();
    assert!(app.back.is_empty(), "a tab starts a fresh history");
    assert!(app.click(1, "music:back", 0).is_err());
    let (_, url, _) = request(&app.click(1, "music:find:focus", 0).unwrap());
    assert!(url.ends_with("/api/search?q=focus"));
}

#[test]
fn a_playlist_is_named_then_created_over_http() {
    let mut app = app_with(serde_json::Value::Null);
    app.click(1, "music:compose", 0).unwrap();
    assert!(
        app.click(1, "music:create", 0).is_err(),
        "an untitled playlist is refused"
    );
    app.text("Night Drive").unwrap();
    // Clicking the field again keeps what was typed.
    app.click(1, "music:compose", 0).unwrap();
    let (_, url, body) = request(&app.click(1, "music:create", 0).unwrap());
    assert_eq!(url, "http://spotify.com/api/playlists");
    assert_eq!(body, json!({"title": "Night Drive"}));
    app.http(1, "done", 200, "{}").unwrap();
    assert!(app.draft.is_none());
}

#[test]
fn an_unreachable_service_and_a_refusal_are_different_states() {
    let mut app = app_with(serde_json::Value::Null);
    app.offline("catalog", "network unreachable");
    assert_eq!(app.status, Status::Offline("network unreachable".into()));
    app.http(
        1,
        "done",
        400,
        r#"{"error":"nothing is queued after this track"}"#,
    )
    .unwrap();
    assert_eq!(
        app.status,
        Status::Offline("nothing is queued after this track".into())
    );
    app.http(1, "catalog", 403, r#"{"error":"catalogue unavailable"}"#)
        .unwrap();
    assert_eq!(app.status, Status::Denied("catalogue unavailable".into()));
}

#[test]
fn the_music_app_is_named_for_each_platform() {
    let app = app_with(serde_json::Value::Null);
    assert_eq!(app.title(DesktopTheme::Android), "YouTube Music");
    assert_eq!(app.title(DesktopTheme::Windows), "Media Player");
    assert_eq!(app.title(DesktopTheme::Ubuntu), "Rhythmbox");
    assert_eq!(app.title(DesktopTheme::Macos), "Music");
}

/// Every control any face paints, in every view, is one the model accepts.
#[test]
fn every_painted_control_is_one_the_model_accepts() {
    let base = app_with(playing(0, true));
    let mut states = vec![];
    for target in [
        "music:home",
        "music:new",
        "music:radio",
        "music:library:recent",
        "music:library:songs",
        "music:library:albums",
        "music:library:artists",
        "music:library:playlists",
        "music:album:cold-reads",
        "music:artist:cache-miss",
        "music:playlist:deep-focus",
        "music:liked",
        "music:queue",
        "music:expand",
        "music:search-field",
    ] {
        let mut app = base.clone();
        app.click(1, target, 0).unwrap();
        states.push((target.to_owned(), app));
    }
    let mut searched = base.clone();
    searched.click(1, "music:search-field", 0).unwrap();
    searched.text("cache").unwrap();
    searched
        .http(1, "search", 200, r#"{"query":"cache","tracks":["cold-reads"],"albums":["cold-reads"],"artists":["cache-miss"],"playlists":["deep-focus"]}"#)
        .unwrap();
    states.push(("search results".into(), searched));
    let mut menu = base.clone();
    menu.click(1, "music:playlist:deep-focus", 0).unwrap();
    menu.click(1, "music:menu:tiling", 0).unwrap();
    states.push(("menu".into(), menu.clone()));
    menu.click(1, "music:menu-playlists", 0).unwrap();
    states.push(("playlist menu".into(), menu));
    let mut composing = base.clone();
    composing.click(1, "music:compose", 0).unwrap();
    composing.text("Mix").unwrap();
    states.push(("composer".into(), composing));
    states.push(("nothing playing".into(), app_with(serde_json::Value::Null)));
    for theme in [
        DesktopTheme::Macos,
        DesktopTheme::Windows,
        DesktopTheme::Ubuntu,
        DesktopTheme::Ios,
        DesktopTheme::Android,
    ] {
        let (width, height) = if theme.mobile() {
            (430, 800)
        } else {
            (1000, 640)
        };
        for (name, state) in &states {
            let mut scene = Painter::themed(theme, width, height, 0);
            state.render(
                &mut scene,
                &crate::AppEnv {
                    theme,
                    width,
                    height,
                    clock_us: 3 * S,
                    settings: &crate::SystemSettings::DEFAULT,
                    clipboard: None,
                    share_to: None,
                    editor: None,
                    pointer: None,
                    files: Default::default(),
                },
            );
            let targets: Vec<_> = scene
                .scene
                .nodes
                .iter()
                .filter_map(|n| n.interaction.clone())
                .collect();
            assert!(
                !targets.is_empty(),
                "{name} on {theme:?} paints no controls"
            );
            // A pane's scroll bar is the window's, not the player's: the platform
            // drags it (see `desktop_scene::scroll`).
            for target in targets.into_iter().filter(|t| !t.starts_with("pane:")) {
                let mut app = state.clone();
                assert!(
                    app.click(1, &target, 3 * S).is_ok(),
                    "unhandled control {target} in {name} on {theme:?}"
                );
            }
        }
    }
}
