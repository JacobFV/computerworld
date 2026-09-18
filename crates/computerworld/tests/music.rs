//! The music players and the music sites, driven the way a person drives them: pointer
//! clicks on what is painted. Every assertion about playback reads the service back, so a
//! control that only changed pixels would fail here.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, HttpRequest, HttpResponse};
use cw_scene::Primitive;
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
/// One second of world time.
const S: u64 = 1_000_000;

fn world(profile: &str) -> (World, String) {
    let mut d = reference_world();
    d.metadata["desktop_themes"] = json!({ MACHINE: profile });
    // The same entry examples/browser/build.mjs ships: Android's player is YouTube Music.
    d.metadata["desktop_apps"] = json!([{
        "id": "music", "label": "Music", "kind": "native", "url": "http://spotify.com/",
        "urls": {"android": "http://music.youtube.com/"}, "icon": "music"
    }]);
    for c in &mut d.computers {
        if c.id == MACHINE {
            c.installed_apps.push("music".into());
        }
    }
    let mut world = World::new(d, 42).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", MACHINE))
        .unwrap();
    (world, actor)
}
fn size(profile: &str) -> (u32, u32) {
    if profile.contains("ios") || profile.contains("android") {
        (430, 932)
    } else {
        (1280, 800)
    }
}
fn act(world: &mut World, actor: &str, family: &str, op: &str, payload: Value) {
    let result = world
        .step(
            actor,
            vec![ActionEnvelope::new(family, op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
}
/// Click the centre of the painted control whose interaction is `target`.
fn click(world: &mut World, actor: &str, (w, h): (u32, u32), target: &str) {
    let scene = world.scene(actor, w, h).unwrap();
    let suffix = format!(":content:{target}");
    let node = scene
        .nodes
        .iter()
        .find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i == target || i.ends_with(&suffix))
        })
        .unwrap_or_else(|| panic!("{target} is not painted"));
    let b = node.transform.bounds(node.bounds);
    let (x, y) = (b.x + (b.width / 2) as i32, b.y + (b.height / 2) as i32);
    // The control really is what a click there lands on.
    let hit = scene.hit_test(x, y).map(|n| n.interaction.clone());
    assert!(
        hit.flatten()
            .is_some_and(|i| i == target || i.ends_with(&suffix)),
        "{target} is painted but covered"
    );
    act(
        world,
        actor,
        "pointer.v1",
        "click",
        json!({"x": x, "y": y, "width": w, "height": h}),
    );
}
/// Scroll the page until `target` is on screen and not under the pinned player bar.
fn reveal(world: &mut World, actor: &str, (w, h): (u32, u32), target: &str) {
    let suffix = format!(":content:{target}");
    for y in (0..3000).step_by(100) {
        act(world, actor, "browser.v1", "scroll", json!({ "y": y }));
        let scene = world.scene(actor, w, h).unwrap();
        let visible = scene.nodes.iter().any(|n| {
            if !n
                .interaction
                .as_deref()
                .is_some_and(|i| i.ends_with(&suffix))
            {
                return false;
            }
            let b = n.transform.bounds(n.bounds);
            let (x, y) = (b.x + (b.width / 2) as i32, b.y + (b.height / 2) as i32);
            scene
                .hit_test(x, y)
                .and_then(|hit| hit.interaction.as_deref())
                .is_some_and(|i| i.ends_with(&suffix))
        });
        if visible {
            return;
        }
    }
    panic!("{target} never scrolled into view");
}
fn painted(world: &World, actor: &str, (w, h): (u32, u32), target: &str) -> bool {
    let suffix = format!(":content:{target}");
    world.scene(actor, w, h).unwrap().nodes.iter().any(|n| {
        n.interaction
            .as_deref()
            .is_some_and(|i| i == target || i.ends_with(&suffix))
    })
}
fn texts(world: &World, actor: &str, (w, h): (u32, u32)) -> Vec<String> {
    world
        .scene(actor, w, h)
        .unwrap()
        .nodes
        .iter()
        .filter_map(|n| match &n.primitive {
            Primitive::UiText { text, .. } | Primitive::UiTextBold { text, .. } => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}
fn http(world: &mut World, actor: &str, method: &str, url: &str, body: Value) -> HttpResponse {
    let request = HttpRequest::json(method, url, &body).unwrap();
    let result = world
        .step(
            actor,
            vec![ActionEnvelope::new(
                "http.v1",
                "request",
                MACHINE,
                serde_json::to_value(request).unwrap(),
            )],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{:?}", result.outcomes[0]);
    serde_json::from_value(result.outcomes[0].value.clone()).unwrap()
}
/// What the service says this listener's session is, read over the network.
fn catalog(world: &mut World, actor: &str, host: &str) -> Value {
    let reply = http(
        world,
        actor,
        "GET",
        &format!("http://{host}/api/catalog"),
        json!({}),
    );
    assert_eq!(reply.status, 200);
    serde_json::from_slice(&reply.body).unwrap()
}
fn app_state(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    let desktop = serde_json::to_value(&session.machines[MACHINE].desktop).unwrap();
    desktop["windows"]
        .as_object()
        .unwrap()
        .values()
        .find(|w| w["state"]["app"] == "music")
        .expect("the music window is open")["state"]
        .clone()
}
fn launch(world: &mut World, actor: &str) {
    act(
        world,
        actor,
        "application.v1",
        "launch",
        json!({"kind": "music"}),
    );
}
/// Seconds shown for a `m:ss` label, if `text` is one.
fn seconds(text: &str) -> Option<u64> {
    let (m, s) = text.trim_start_matches('-').split_once(':')?;
    Some(m.parse::<u64>().ok()? * 60 + s.parse::<u64>().ok()?)
}

#[test]
fn apple_music_plays_an_album_and_the_world_clock_moves_it() {
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    launch(&mut world, &actor);
    assert_eq!(app_state(&world, &actor)["base"], "http://spotify.com/");
    click(&mut world, &actor, screen, "music:library:albums");
    click(&mut world, &actor, screen, "music:album:cold-reads");
    assert!(texts(&world, &actor, screen)
        .iter()
        .any(|t| t == "Cold Reads"));
    click(&mut world, &actor, screen, "music:play:album:cold-reads");
    let session = catalog(&mut world, &actor, "spotify.com");
    assert_eq!(session["player"]["item"], "cold-reads");
    assert_eq!(session["player"]["context"], "album:cold-reads");
    assert_eq!(session["player"]["playing"], true);
    // Forty seconds of world time later the LCD's elapsed time has moved with it.
    world.runtime_mut().advance(40 * S).unwrap();
    click(&mut world, &actor, screen, "music:queue");
    let elapsed: Vec<u64> = texts(&world, &actor, screen)
        .iter()
        .filter(|t| !t.starts_with('-'))
        .filter_map(|t| seconds(t))
        .filter(|s| (35..=50).contains(s))
        .collect();
    assert!(
        !elapsed.is_empty(),
        "the LCD did not advance with the clock"
    );
    // The scrubber seeks where it is clicked: half way through a 2:38 song.
    click(&mut world, &actor, screen, "music:seek:500");
    let session = catalog(&mut world, &actor, "spotify.com");
    let at = session["player"]["position_ms"].as_u64().unwrap();
    assert!((75_000..=85_000).contains(&at), "seeked to {at}");
    click(&mut world, &actor, screen, "music:next");
    click(&mut world, &actor, screen, "music:shuffle");
    click(&mut world, &actor, screen, "music:repeat");
    let session = catalog(&mut world, &actor, "spotify.com");
    assert_eq!(session["player"]["item"], "warm-cache");
    assert_eq!(session["player"]["shuffle"], true);
    assert_eq!(session["player"]["repeat"], "all");
    assert_eq!(
        session["player"]["queue"][0], "warm-cache",
        "shuffle keeps the song"
    );
    click(&mut world, &actor, screen, "music:toggle");
    assert_eq!(
        catalog(&mut world, &actor, "spotify.com")["player"]["playing"],
        false
    );
}

#[test]
fn apple_music_on_ios_searches_without_a_keyboard_in_the_way_until_asked() {
    let profile = "virtual-ios-18";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    launch(&mut world, &actor);
    let keys = |world: &World| painted(world, &actor, screen, "shell:type:a");
    assert!(
        !keys(&world),
        "a player with no field focused shows no keyboard"
    );
    click(&mut world, &actor, screen, "music:search-field");
    assert!(keys(&world), "the search field brings up the keyboard");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text": "tiling"}),
    );
    click(&mut world, &actor, screen, "music:search");
    assert!(!keys(&world));
    click(&mut world, &actor, screen, "music:play:track@tiling");
    assert!(
        painted(&world, &actor, screen, "music:expand"),
        "the mini player appears"
    );
    click(&mut world, &actor, screen, "music:expand");
    click(&mut world, &actor, screen, "music:like:tiling");
    let session = catalog(&mut world, &actor, "spotify.com");
    assert_eq!(session["player"]["item"], "tiling");
    assert!(
        !session["liked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "tiling"),
        "alice had liked Tiling; the star un-liked it"
    );
}

#[test]
fn youtube_music_on_android_reads_music_youtube_com_and_saves_to_a_playlist() {
    let profile = "virtual-android-12";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    launch(&mut world, &actor);
    assert_eq!(
        app_state(&world, &actor)["base"],
        "http://music.youtube.com/"
    );
    click(&mut world, &actor, screen, "music:mood:relax");
    assert_eq!(app_state(&world, &actor)["mood"], "relax");
    click(&mut world, &actor, screen, "music:new");
    click(&mut world, &actor, screen, "music:album:cold-reads");
    click(&mut world, &actor, screen, "music:play:album:cold-reads");
    click(&mut world, &actor, screen, "music:menu:warm-cache");
    click(&mut world, &actor, screen, "music:menu-playlists");
    click(&mut world, &actor, screen, "music:add:deep-focus");
    let session = catalog(&mut world, &actor, "music.youtube.com");
    assert_eq!(session["player"]["context"], "album:cold-reads");
    let deep = session["playlists"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "deep-focus")
        .unwrap()
        .clone();
    assert!(deep["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t == "warm-cache"));
    // The full-screen player's controls are real too.
    click(&mut world, &actor, screen, "music:expand");
    click(&mut world, &actor, screen, "music:next");
    assert_eq!(
        catalog(&mut world, &actor, "music.youtube.com")["player"]["item"],
        "warm-cache"
    );
    // spotify.com's session is untouched: the two services are separate worlds.
    assert!(catalog(&mut world, &actor, "spotify.com")["player"].is_null());
}

#[test]
fn media_player_and_rhythmbox_drive_the_same_service() {
    for (profile, steps) in [
        (
            "virtual-windows-11",
            vec![
                "music:library:songs",
                "music:play:charts@eviction",
                "music:repeat",
            ],
        ),
        (
            "virtual-ubuntu-24",
            vec![
                "music:filter-artist:cache-miss",
                "music:play:charts@eviction",
                "music:repeat",
            ],
        ),
    ] {
        let screen = size(profile);
        let (mut world, actor) = crate::world(profile);
        launch(&mut world, &actor);
        for target in steps {
            click(&mut world, &actor, screen, target);
        }
        let session = catalog(&mut world, &actor, "spotify.com");
        assert_eq!(session["player"]["item"], "eviction", "{profile}");
        assert_eq!(session["player"]["repeat"], "all", "{profile}");
        assert!(texts(&world, &actor, screen)
            .iter()
            .any(|t| t.contains("Eviction")));
    }
}

#[test]
fn youtube_music_on_the_web_plays_likes_searches_and_builds_a_playlist() {
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind": "browser"}),
    );
    let go = |world: &mut World, url: &str| {
        act(
            world,
            &actor,
            "browser.v1",
            "navigate",
            json!({ "url": url }),
        );
    };
    go(&mut world, "http://music.youtube.com/browse/cold-reads");
    assert!(
        !painted(&world, &actor, screen, "player-toggle"),
        "nothing is playing yet"
    );
    click(&mut world, &actor, screen, "album-play");
    assert!(
        painted(&world, &actor, screen, "player-toggle"),
        "the player bar appears"
    );
    click(&mut world, &actor, screen, "player-next");
    click(&mut world, &actor, screen, "bar-like");
    let session = catalog(&mut world, &actor, "music.youtube.com");
    assert_eq!(session["player"]["item"], "warm-cache");
    assert!(session["liked"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t == "warm-cache"));
    // The progress line is a seek control.
    click(&mut world, &actor, screen, "player-seek-40");
    let at = catalog(&mut world, &actor, "music.youtube.com")["player"]["position_ms"]
        .as_u64()
        .unwrap();
    assert!((80_000..=95_000).contains(&at), "seeked to {at}");
    // Search through the page's own form.
    go(&mut world, "http://music.youtube.com/search");
    click(&mut world, &actor, screen, "search-q");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text": "tessellate"}),
    );
    click(&mut world, &actor, screen, "search-submit");
    assert!(painted(&world, &actor, screen, "results-top"));
    // And a playlist, from the library's form.
    go(&mut world, "http://music.youtube.com/library/playlists");
    reveal(&mut world, &actor, screen, "playlist-title");
    click(&mut world, &actor, screen, "playlist-title");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text": "Road Trip"}),
    );
    click(&mut world, &actor, screen, "playlist-submit");
    let session = catalog(&mut world, &actor, "music.youtube.com");
    assert!(session["playlists"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == "road-trip"));
}

#[test]
fn spotify_on_the_web_keeps_playing_through_its_own_player_bar() {
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind": "browser"}),
    );
    act(
        &mut world,
        &actor,
        "browser.v1",
        "navigate",
        json!({"url": "http://spotify.com/album/cold-reads"}),
    );
    click(&mut world, &actor, screen, "album-play");
    click(&mut world, &actor, screen, "player-shuffle");
    click(&mut world, &actor, screen, "player-toggle");
    let session = catalog(&mut world, &actor, "spotify.com");
    assert_eq!(session["player"]["context"], "album:cold-reads");
    assert_eq!(session["player"]["shuffle"], true);
    assert_eq!(session["player"]["playing"], false);
}

/// The machine's own settings: the system volume a phone's slider sets.
fn system_volume(world: &World, actor: &str) -> u64 {
    let session = world.interfaces().session(actor).unwrap();
    let desktop = serde_json::to_value(&session.machines[MACHINE].desktop).unwrap();
    desktop["settings"]["volume"].as_u64().unwrap()
}
/// What a speaker on the network says it is playing.
fn speaker(world: &mut World, actor: &str, host: &str) -> Value {
    let reply = http(
        world,
        actor,
        "GET",
        &format!("http://{host}/api/status"),
        json!({}),
    );
    assert_eq!(reply.status, 200);
    serde_json::from_slice(&reply.body).unwrap()
}
/// The colour of the first text node that begins with `words` (a long lyric line wraps,
/// so the first piece of it is what is looked for).
fn ink_of(world: &World, actor: &str, (w, h): (u32, u32), words: &str) -> Option<cw_scene::Color> {
    world
        .scene(actor, w, h)
        .unwrap()
        .nodes
        .iter()
        .find_map(|n| match &n.primitive {
            Primitive::UiText { text, color, .. } | Primitive::UiTextBold { text, color, .. }
                if text.starts_with(words) =>
            {
                Some(*color)
            }
            _ => None,
        })
}
/// The page the browser shows, as JSON.
fn page_text(world: &World, actor: &str) -> String {
    let session = world.interfaces().session(actor).unwrap();
    serde_json::to_string(session.machines[MACHINE].browser.page().unwrap()).unwrap()
}
/// A browser step that changes nothing but lets the world's step-end work (a page's
/// refresh) run.
fn idle_step(world: &mut World, actor: &str) {
    act(
        world,
        actor,
        "pointer.v1",
        "move",
        json!({"x": 5, "y": 5, "width": 1280, "height": 800}),
    );
}

#[test]
fn airplay_hands_the_session_to_a_speaker_that_is_really_on_the_network() {
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    launch(&mut world, &actor);
    click(&mut world, &actor, screen, "music:library:albums");
    click(&mut world, &actor, screen, "music:album:cold-reads");
    click(&mut world, &actor, screen, "music:play:album:cold-reads");
    // Opening AirPlay asks each of the account's speakers whether it is there.
    click(&mut world, &actor, screen, "music:output");
    let state = app_state(&world, &actor);
    assert_eq!(state["reachable"]["livingroom"], true);
    assert_eq!(state["reachable"]["office-tv"], true);
    assert_eq!(
        state["reachable"]["bedroom"], false,
        "unplugged: no such host"
    );
    // Only AirPlay speakers are offered, and only those that answered can be picked.
    assert!(
        !painted(&world, &actor, screen, "music:output:kitchen"),
        "Cast and DLNA only"
    );
    assert!(!painted(&world, &actor, screen, "music:output:bedroom"));
    click(&mut world, &actor, screen, "music:output:livingroom");
    let there = speaker(&mut world, &actor, "livingroom.speaker.internal");
    assert_eq!(there["session"]["item"], "cold-reads");
    assert_eq!(there["session"]["controller"], "alice");
    assert_eq!(there["session"]["playing"], true);
    let session = catalog(&mut world, &actor, "spotify.com");
    assert_eq!(session["player"]["device"], "livingroom");
    assert_eq!(session["player"]["device_name"], "Living Room");
    // The session keeps moving there: the next song is the speaker's next song, and the
    // speaker's own clock carries the position.
    click(&mut world, &actor, screen, "music:next");
    assert_eq!(
        speaker(&mut world, &actor, "livingroom.speaker.internal")["session"]["item"],
        "warm-cache"
    );
    world.runtime_mut().advance(30 * S).unwrap();
    let later = speaker(&mut world, &actor, "livingroom.speaker.internal");
    let at = later["session"]["position_ms"].as_u64().unwrap();
    assert!((29_000..=32_000).contains(&at), "the speaker is {at} ms in");
    // The volume slider now sets the session's volume, which the speaker is sent.
    click(&mut world, &actor, screen, "music:volume:45");
    assert_eq!(
        speaker(&mut world, &actor, "livingroom.speaker.internal")["session"]["volume"],
        45
    );
    // Back to this Mac: the speaker lets the session go.
    click(&mut world, &actor, screen, "music:output");
    click(&mut world, &actor, screen, "music:output:");
    assert!(speaker(&mut world, &actor, "livingroom.speaker.internal")["session"].is_null());
    assert_eq!(
        catalog(&mut world, &actor, "spotify.com")["player"]["device"],
        ""
    );
}

#[test]
fn volume_is_the_players_own_on_a_mac_and_the_phones_on_an_iphone() {
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    launch(&mut world, &actor);
    click(&mut world, &actor, screen, "music:library:albums");
    click(&mut world, &actor, screen, "music:album:cold-reads");
    click(&mut world, &actor, screen, "music:play:album:cold-reads");
    let before = system_volume(&world, &actor);
    click(&mut world, &actor, screen, "music:volume:30");
    let session = catalog(&mut world, &actor, "spotify.com");
    assert_eq!(session["player"]["volume"], 30);
    click(&mut world, &actor, screen, "music:mute");
    assert_eq!(
        catalog(&mut world, &actor, "spotify.com")["player"]["muted"],
        true
    );
    assert_eq!(
        system_volume(&world, &actor),
        before,
        "Music's slider is its own"
    );

    let profile = "virtual-ios-18";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    launch(&mut world, &actor);
    click(&mut world, &actor, screen, "music:new");
    click(&mut world, &actor, screen, "music:album:cold-reads");
    click(&mut world, &actor, screen, "music:play:album:cold-reads");
    click(&mut world, &actor, screen, "music:expand");
    // On the iPhone the Now Playing slider is the phone's volume, as on a real one.
    click(&mut world, &actor, screen, "shell:set:volume:40");
    assert_eq!(system_volume(&world, &actor), 40);
    assert_eq!(
        catalog(&mut world, &actor, "spotify.com")["player"]["volume"],
        100
    );
}

#[test]
fn lyrics_follow_the_world_clock_in_the_players_and_on_the_sites() {
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    launch(&mut world, &actor);
    click(&mut world, &actor, screen, "music:search-field");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text": "puget"}),
    );
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "key",
        json!({"key": "Enter"}),
    );
    click(&mut world, &actor, screen, "music:play:track@puget");
    click(&mut world, &actor, screen, "music:lyrics");
    let lit = cw_scene::Color::rgb(29, 29, 31);
    let chorus = "Oh, Puget, carry me slow";
    let next = "Past every light I used to";
    // Forty-four seconds in, the chorus's first line is the one being sung.
    world.runtime_mut().advance(44 * S).unwrap();
    idle_step(&mut world, &actor);
    assert_eq!(ink_of(&world, &actor, screen, chorus), Some(lit));
    assert_ne!(ink_of(&world, &actor, screen, next), Some(lit));
    world.runtime_mut().advance(4 * S).unwrap();
    idle_step(&mut world, &actor);
    assert_eq!(ink_of(&world, &actor, screen, next), Some(lit));
    // A line is a seek to where it is sung.
    click(&mut world, &actor, screen, "music:lyric:6");
    let at = catalog(&mut world, &actor, "spotify.com")["player"]["position_ms"]
        .as_u64()
        .unwrap();
    assert!((42_000..43_000).contains(&at), "seeked to {at}");

    // spotify.com's lyrics view reads the same session and follows the same clock.
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind": "browser"}),
    );
    act(
        &mut world,
        &actor,
        "browser.v1",
        "navigate",
        json!({"url": "http://spotify.com/lyrics"}),
    );
    let page = page_text(&world, &actor);
    assert!(page.contains("Grey water folding under the bow"));

    // YouTube Music on Android: the LYRICS tab is live, not greyed.
    let profile = "virtual-android-12";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    http(
        &mut world,
        &actor,
        "POST",
        "http://music.youtube.com/api/player",
        json!({"action": "play", "item": "ferry-terminal", "context": "album:puget"}),
    );
    launch(&mut world, &actor);
    click(&mut world, &actor, screen, "music:expand");
    click(&mut world, &actor, screen, "music:lyrics");
    world.runtime_mut().advance(13 * S).unwrap();
    act(
        &mut world,
        &actor,
        "pointer.v1",
        "move",
        json!({"x": 5, "y": 5, "width": screen.0, "height": screen.1}),
    );
    assert_eq!(
        ink_of(&world, &actor, screen, "Ticket in my pocket,"),
        Some(cw_scene::Color::WHITE)
    );
}

#[test]
fn a_music_sites_player_bar_keeps_up_with_the_world_clock() {
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind": "browser"}),
    );
    act(
        &mut world,
        &actor,
        "browser.v1",
        "navigate",
        json!({"url": "http://spotify.com/album/cold-reads"}),
    );
    click(&mut world, &actor, screen, "album-play");
    let elapsed = |world: &World| -> u64 {
        let text = page_text(world, &actor);
        let at = text
            .find("\"bar-elapsed\"")
            .expect("the bar shows the time");
        let tail = &text[at..];
        let start = tail.find("\"text\":\"").unwrap() + 8;
        seconds(&tail[start..start + tail[start..].find('"').unwrap()]).unwrap()
    };
    assert!(elapsed(&world) <= 1);
    // Forty world seconds later, with no click at all, the bar has moved on.
    world.runtime_mut().advance(40 * S).unwrap();
    idle_step(&mut world, &actor);
    let now = elapsed(&world);
    assert!((39..=42).contains(&now), "the bar shows {now} s");
    // It is the page refreshing itself, not a visit: the song was not played again.
    let plays = catalog(&mut world, &actor, "spotify.com")["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "cold-reads")
        .unwrap()["plays"]
        .as_u64()
        .unwrap();
    world.runtime_mut().advance(5 * S).unwrap();
    idle_step(&mut world, &actor);
    let again = catalog(&mut world, &actor, "spotify.com")["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "cold-reads")
        .unwrap()["plays"]
        .as_u64()
        .unwrap();
    assert_eq!(plays, again);
    // Paused, the page stops asking.
    click(&mut world, &actor, screen, "player-toggle");
    let paused = elapsed(&world);
    world.runtime_mut().advance(30 * S).unwrap();
    idle_step(&mut world, &actor);
    assert_eq!(elapsed(&world), paused);
}

#[test]
fn shelves_scroll_sideways_by_wheel_bar_and_swipe() {
    // Apple Music on the Mac: "Stations for You" holds every artist on one shelf.
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    launch(&mut world, &actor);
    let shelf = |world: &World| -> cw_scene::ScrollArea {
        world
            .scene(&actor, screen.0, screen.1)
            .unwrap()
            .scrolls
            .into_iter()
            .find(|a| a.horizontal && a.target.ends_with("-albums"))
            .expect("the new releases shelf is a sideways pane")
    };
    click(&mut world, &actor, screen, "music:new");
    let first = shelf(&world);
    assert_eq!(first.offset, 0);
    assert!(first.max_offset() > 0, "more albums than fit");
    let albums = |world: &World| -> std::collections::BTreeSet<String> {
        world
            .scene(&actor, screen.0, screen.1)
            .unwrap()
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
            .filter(|i| i.contains(":content:music:album:"))
            .collect()
    };
    let before = albums(&world);
    let (x, y) = (
        first.bounds.x + first.bounds.width as i32 / 2,
        first.bounds.y + 40,
    );
    act(
        &mut world,
        &actor,
        "pointer.v1",
        "wheel",
        json!({"x": x, "y": y, "width": screen.0, "height": screen.1, "delta_x": 5000, "delta_y": 0}),
    );
    assert_eq!(shelf(&world).offset, first.max_offset());
    assert!(
        albums(&world).difference(&before).count() > 0,
        "albums that were off the shelf's edge are on it now"
    );
    // Shift and the wheel go back the other way; a plain turn scrolls the page instead.
    act(
        &mut world,
        &actor,
        "pointer.v1",
        "wheel",
        json!({"x": x, "y": y, "width": screen.0, "height": screen.1, "delta_y": -5000, "modifiers": ["shift"]}),
    );
    assert_eq!(shelf(&world).offset, 0);

    // On the iPhone a sideways swipe moves it under the finger.
    let profile = "virtual-ios-18";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    launch(&mut world, &actor);
    let area = world
        .scene(&actor, screen.0, screen.1)
        .unwrap()
        .scrolls
        .into_iter()
        .find(|a| a.horizontal)
        .expect("a sideways shelf");
    let y = area.bounds.y + 60;
    let x0 = area.bounds.x + area.bounds.width as i32 - 30;
    act(
        &mut world,
        &actor,
        "pointer.v1",
        "down",
        json!({"x": x0, "y": y, "width": screen.0, "height": screen.1}),
    );
    act(
        &mut world,
        &actor,
        "pointer.v1",
        "up",
        json!({"x": x0 - 200, "y": y, "width": screen.0, "height": screen.1}),
    );
    let moved = world
        .scene(&actor, screen.0, screen.1)
        .unwrap()
        .scrolls
        .into_iter()
        .find(|a| a.target == area.target)
        .unwrap();
    assert!(moved.offset > 0, "the swipe moved the shelf");

    // spotify.com's shelves are rows that scroll sideways too.
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind": "browser"}),
    );
    act(
        &mut world,
        &actor,
        "browser.v1",
        "navigate",
        json!({"url": "http://spotify.com/"}),
    );
    let shelf_area = |world: &World| -> Option<cw_scene::ScrollArea> {
        world
            .scene(&actor, screen.0, screen.1)
            .unwrap()
            .scrolls
            .into_iter()
            .find(|a| a.horizontal && a.max_offset() > 0)
    };
    let mut found = None;
    for y in (0..1600).step_by(200) {
        act(
            &mut world,
            &actor,
            "browser.v1",
            "scroll",
            json!({ "y": y }),
        );
        if let Some(area) = shelf_area(&world) {
            found = Some(area);
            break;
        }
    }
    let area = found.expect("a shelf on spotify.com scrolls sideways");
    let row = area
        .target
        .rsplit(":content:pane:row:")
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(area.offset, 0);
    // The wheel's sideways turn over it moves that shelf, not the page.
    let page_before = world.interfaces().session(&actor).unwrap().machines[MACHINE]
        .browser
        .tab()
        .scroll_y;
    act(
        &mut world,
        &actor,
        "pointer.v1",
        "wheel",
        json!({"x": area.bounds.x + area.bounds.width as i32 / 2,
               "y": area.bounds.y + area.bounds.height as i32 / 2,
               "width": screen.0, "height": screen.1, "delta_x": 5000, "delta_y": 0}),
    );
    let moved = shelf_area(&world).unwrap();
    assert_eq!(moved.offset, moved.max_offset());
    assert_eq!(
        world.interfaces().session(&actor).unwrap().machines[MACHINE]
            .browser
            .tab()
            .scroll_y,
        page_before,
        "the page stayed where it was"
    );
    // And the browser can be told directly which shelf to scroll.
    act(
        &mut world,
        &actor,
        "browser.v1",
        "scroll",
        json!({"row": row, "x": 0}),
    );
    assert_eq!(shelf_area(&world).unwrap().offset, 0);
}

#[test]
fn covers_and_icons_are_real_on_the_music_sites() {
    let profile = "virtual-macos-golden-gate";
    let screen = size(profile);
    let (mut world, actor) = crate::world(profile);
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind": "browser"}),
    );
    act(
        &mut world,
        &actor,
        "browser.v1",
        "navigate",
        json!({"url": "http://music.youtube.com/browse/cold-reads"}),
    );
    click(&mut world, &actor, screen, "album-play");
    let scene = world.scene(&actor, screen.0, screen.1).unwrap();
    // Covers are pictures the site drew, not flat tiles.
    assert!(scene
        .nodes
        .iter()
        .any(|n| matches!(&n.primitive, Primitive::Image { width, .. } if *width == 220)));
    // YouTube Music's like is a thumb, a real button that says what it will do.
    let liked = |world: &mut World| {
        catalog(world, &actor, "music.youtube.com")["liked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "cold-reads")
    };
    let like_label = |world: &World| {
        world
            .scene(&actor, screen.0, screen.1)
            .unwrap()
            .nodes
            .iter()
            .find(|n| {
                n.interaction
                    .as_deref()
                    .is_some_and(|i| i.ends_with("bar-like"))
            })
            .and_then(|n| n.semantic.clone())
            .unwrap()
            .label
    };
    let thumbs = |world: &World, asset: &str| {
        world
            .scene(&actor, screen.0, screen.1)
            .unwrap()
            .nodes
            .iter()
            .any(|n| matches!(&n.primitive, Primitive::Symbol { asset: a, .. } if a == asset))
    };
    let was = liked(&mut world);
    assert_eq!(like_label(&world), if was { "Remove like" } else { "Like" });
    assert!(thumbs(
        &world,
        if was {
            "symbol/thumb-up-fill"
        } else {
            "symbol/thumb-up"
        }
    ));
    click(&mut world, &actor, screen, "bar-like");
    assert_eq!(liked(&mut world), !was);
    assert!(thumbs(
        &world,
        if was {
            "symbol/thumb-up"
        } else {
            "symbol/thumb-up-fill"
        }
    ));
}
