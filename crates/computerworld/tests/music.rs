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
    let (mut world, actor) = world(profile);
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
    let (mut world, actor) = world(profile);
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
    let (mut world, actor) = world(profile);
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
        let (mut world, actor) = world(profile);
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
    let (mut world, actor) = world(profile);
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
    let (mut world, actor) = world(profile);
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
