//! Icon coverage: a missing icon fails silently. `AssetImage` with an id the
//! table does not know paints nothing at all, so a shell that names an icon it
//! never bundled shows an empty dock slot or home-screen tile instead of an
//! error. These tests render every advertised id and demand visible artwork.
use cw_render::{Renderer, ASSET_IDS};
use cw_scene::{Color, Node, Rect, Scene};
use std::collections::BTreeSet;

/// The shell namespaces, and every application a shell may place on a home
/// screen, dock or launcher tile. Both lists are checked against `ASSET_IDS`.
const PLATFORMS: [&str; 5] = ["macos", "windows", "ubuntu", "ios", "android"];
const APPS: [&str; 21] = [
    "files",
    "browser",
    "terminal",
    "docs",
    "mail",
    "calendar",
    "chat",
    "settings",
    "camera",
    "photos",
    "phone",
    "store",
    "launcher",
    "trash",
    "notes",
    "contacts",
    "clock",
    "calculator",
    "music",
    "maps",
    "weather",
];
/// Spellings kept resolvable for existing call sites, and what they resolve to.
const ALIASES: [(&str, &str); 6] = [
    ("editor", "docs"),
    ("messages", "chat"),
    ("notepad", "notes"),
    ("addressbook", "contacts"),
    ("clocks", "clock"),
    ("calc", "calculator"),
];

const TILE: u32 = 128;
/// Nothing in the icon set is magenta, so leftover backdrop is unpainted.
const BACKDROP: Color = Color(255, 0, 255, 255);

/// Paints `asset` over the backdrop at tile size, as a dock or home screen does.
fn tile(asset: &str) -> cw_render::Frame {
    let mut scene = Scene::new(TILE, TILE);
    scene.background = BACKDROP;
    scene
        .nodes
        .push(Node::asset(1, Rect::new(0, 0, TILE, TILE), asset));
    Renderer::new().render(&scene)
}

/// Painted pixels (backdrop left showing does not count) and distinct colours.
fn ink(frame: &cw_render::Frame) -> (usize, usize) {
    let mut painted = 0;
    let mut colors = BTreeSet::new();
    for px in frame.rgba.chunks_exact(4) {
        if px != [BACKDROP.0, BACKDROP.1, BACKDROP.2, BACKDROP.3] {
            painted += 1;
            colors.insert([px[0], px[1], px[2]]);
        }
    }
    (painted, colors.len())
}

#[test]
fn every_advertised_icon_paints_artwork() {
    let area = (TILE * TILE) as usize;
    for id in ASSET_IDS.iter().filter(|id| id.starts_with("icon/")) {
        let frame = tile(id);
        assert_eq!((frame.width, frame.height), (TILE, TILE), "{id}");
        let (painted, colors) = ink(&frame);
        // A silhouette-only Windows icon covers the least; a blank one covers none.
        assert!(
            painted * 20 >= area,
            "{id} painted only {painted} of {area} pixels"
        );
        assert!(colors >= 8, "{id} painted {colors} distinct colours");
    }
}

#[test]
fn every_platform_advertises_every_application() {
    let advertised: BTreeSet<&str> = ASSET_IDS.iter().copied().collect();
    for platform in PLATFORMS {
        for app in APPS {
            let id = format!("icon/{platform}/{app}");
            assert!(advertised.contains(id.as_str()), "{id} is not advertised");
        }
    }
    // And nothing is advertised that the two lists above do not cover, so a new
    // icon cannot be added without being added to every shell.
    for id in ASSET_IDS.iter().filter(|id| id.starts_with("icon/")) {
        let (platform, app) = id
            .strip_prefix("icon/")
            .and_then(|rest| rest.split_once('/'))
            .unwrap_or_else(|| panic!("{id} is not icon/<platform>/<app>"));
        assert!(PLATFORMS.contains(&platform), "{id} has no shell");
        assert!(APPS.contains(&app), "{id} is not a known application");
    }
    assert_eq!(
        ASSET_IDS
            .iter()
            .filter(|id| id.starts_with("icon/"))
            .count(),
        PLATFORMS.len() * APPS.len()
    );
}

#[test]
fn aliases_and_shorthands_resolve_to_the_same_artwork() {
    for platform in PLATFORMS {
        for (alias, canonical) in ALIASES {
            assert_eq!(
                tile(&format!("icon/{platform}/{alias}")).rgba,
                tile(&format!("icon/{platform}/{canonical}")).rgba,
                "icon/{platform}/{alias}"
            );
        }
    }
    for app in APPS {
        // Unqualified and platform-neutral ids fall back to the macOS artwork.
        let macos = tile(&format!("icon/macos/{app}")).rgba;
        assert_eq!(tile(&format!("icon/{app}")).rgba, macos, "icon/{app}");
        assert_eq!(
            tile(&format!("icon/common/{app}")).rgba,
            macos,
            "icon/common/{app}"
        );
    }
}

#[test]
fn unknown_icon_ids_paint_nothing_and_never_touch_the_host() {
    let blank = tile("icon/macos/there-is-no-such-app");
    assert_eq!(ink(&blank).0, 0);
    for id in [
        "icon/plan9/mail",
        "https://example.org/icon.png",
        "/usr/share/icons/Yaru/256x256/apps/mail-app.png",
        "../assets/icons/macos-mail.png",
    ] {
        assert_eq!(tile(id).rgba, blank.rgba, "{id}");
    }
}

#[test]
fn icon_rendering_is_reproducible() {
    for id in ASSET_IDS.iter().filter(|id| id.starts_with("icon/")) {
        assert_eq!(tile(id).rgba, tile(id).rgba, "{id}");
    }
}
