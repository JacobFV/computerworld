//! Rhythmbox on Ubuntu (GNOME, Yaru): a header bar with previous, play and next, the song
//! line and its seek slider, and repeat and shuffle toggles; a source list (Music, Radio,
//! Play Queue, playlists); the Artist and Album browser over the song table; and a status
//! line counting what is listed.
use super::art::{self, Surface};
use super::{clock, summary, Music, Repeat, Track, View};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use cw_scene::{Color, Rect};

const WINDOW: Color = Color::rgb(250, 250, 250);
const HEADER: Color = Color::rgb(235, 235, 235);
const SIDE: Color = Color::rgb(242, 242, 242);
const INK: Color = Color::rgb(51, 51, 51);
const MUTED: Color = Color::rgb(120, 120, 120);
const LINE: Color = Color(0, 0, 0, 24);
const ORANGE: Color = Color::rgb(233, 84, 32);
const ZEBRA: Color = Color(0, 0, 0, 9);
const BUTTON: Color = Color(0, 0, 0, 18);

pub fn render(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    p.scene.background = WINDOW;
    p.box_(Rect::new(0, 0, w, h), WINDOW, 0);
    let head = 58u32;
    header(app, p, env, Rect::new(0, 0, w, head));
    let side: u32 = if w >= 600 { 180 } else { 0 };
    let status = 26u32;
    let body_h = h.saturating_sub(head + status + 1);
    if side > 0 {
        sources(app, p, Rect::new(0, head as i32 + 1, side, body_h));
    }
    let area = Rect::new(
        side as i32 + 1,
        head as i32 + 1,
        w.saturating_sub(side + 1),
        body_h,
    );
    let mark = p.scene.nodes.len();
    let listed = main(app, p, env, area);
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(n.clip.and_then(|c| c.intersection(area)).unwrap_or(area));
    }
    let bar = Rect::new(0, (h - status) as i32, w, status);
    p.box_(bar, SIDE, 0);
    p.hline(0, bar.y, w, LINE);
    let text = match app.status.notice() {
        Some(notice) => notice.to_owned(),
        None => listed,
    };
    p.center(0, bar.y + 5, w, &text, 12, MUTED);
    let s = Surface {
        fill: Color::rgb(255, 255, 255),
        ink: INK,
        muted: MUTED,
        line: LINE,
        accent: ORANGE,
        radius: 8,
        size: 13,
    };
    let all = Rect::new(0, 0, w, h);
    if app.menu.is_some() {
        art::menu(
            p,
            app,
            DesktopTheme::Ubuntu,
            side as i32 + 200,
            head as i32 + 60,
            all,
            &s,
        );
    }
    if let Some(draft) = &app.draft {
        art::composer(p, draft, all, &s);
    }
}

fn flat(p: &mut Painter, r: Rect, symbol: &str, target: &str, label: &str, on: bool) {
    if on {
        p.box_(r, Color(0, 0, 0, 40), 6);
    }
    art::icon(p, r, symbol, 16, INK, target, label);
}

fn header(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, r: Rect) {
    p.box_(r, HEADER, 0);
    p.hline(0, r.height as i32, r.width, LINE);
    let live = app.live(env.clock_us);
    let player = app.catalog.player.as_ref();
    let cy = r.y + (r.height as i32 - 34) / 2;
    // Previous / play / next are one linked button group, as GTK draws them.
    let group = Rect::new(10, cy, 114, 34);
    p.box_(group, BUTTON, 6);
    let buttons = [
        ("skip-previous", "music:previous", "Previous"),
        (
            if live.as_ref().is_some_and(|l| l.playing) {
                "pause"
            } else {
                "play"
            },
            "music:toggle",
            "Play",
        ),
        ("skip-next", "music:next", "Next"),
    ];
    for (i, (symbol, target, label)) in buttons.into_iter().enumerate() {
        let b = Rect::new(group.x + i as i32 * 38, cy, 38, 34);
        if live.is_none() {
            art::icon_off(p, b, symbol, 16, INK, "Nothing is playing");
        } else if target == "music:next" && !app.can_next(env.clock_us) {
            art::icon_off(p, b, symbol, 16, INK, "Nothing is queued after this song");
        } else {
            art::icon(p, b, symbol, 16, INK, target, label);
        }
        if i > 0 {
            p.vline(b.x, cy + 6, 22, LINE);
        }
    }
    let x = group.x + group.width as i32 + 16;
    let right = 96;
    let w = (r.width as i32 - x - right).max(80) as u32;
    match (&live, app.now()) {
        (Some(l), Some(t)) => {
            let album = app
                .catalog
                .album(&t.album)
                .map(|a| a.title.clone())
                .unwrap_or_default();
            let line = format!(
                "{} by {} from {}",
                t.title,
                app.catalog.artist_name(&t.artist),
                album
            );
            p.label(x, r.y + 8, w - 110, &line, 13, INK, true, Align::Left);
            p.right(
                x + w as i32 - 110,
                r.y + 8,
                106,
                &format!("{} of {}", clock(l.position_ms), clock(l.duration_ms)),
                12,
                MUTED,
            );
            art::scrubber(
                p,
                Rect::new(x, r.y + 30, w, 18),
                l,
                4,
                ORANGE,
                Color(0, 0, 0, 40),
                Some((14, Color::WHITE)),
            );
        }
        _ => {
            p.label(x, r.y + 20, w, "Not playing", 13, MUTED, false, Align::Left);
        }
    }
    let rx = r.width as i32 - right + 8;
    let repeat = player.map_or(Repeat::Off, |p| p.repeat);
    if player.is_some() {
        flat(
            p,
            Rect::new(rx, cy, 38, 34),
            if repeat == Repeat::One {
                "repeat-one"
            } else {
                "repeat"
            },
            "music:repeat",
            "Repeat",
            repeat != Repeat::Off,
        );
        flat(
            p,
            Rect::new(rx + 42, cy, 38, 34),
            "shuffle",
            "music:shuffle",
            "Shuffle",
            player.is_some_and(|p| p.shuffle),
        );
    } else {
        art::icon_off(
            p,
            Rect::new(rx, cy, 38, 34),
            "repeat",
            16,
            INK,
            "Nothing is playing",
        );
        art::icon_off(
            p,
            Rect::new(rx + 42, cy, 38, 34),
            "shuffle",
            16,
            INK,
            "Nothing is playing",
        );
    }
}

fn sources(app: &Music, p: &mut Painter, r: Rect) {
    p.box_(r, SIDE, 0);
    p.vline(r.width as i32, r.y, r.height, LINE);
    let mut y = r.y + 10;
    let bottom = r.y + r.height as i32 - 34;
    let group = |p: &mut Painter, y: &mut i32, label: &str| {
        p.strong(12, *y, r.width - 24, label, 12, MUTED);
        *y += 22;
    };
    let item = |p: &mut Painter, y: &mut i32, symbol: &str, label: &str, target: &str, on: bool| {
        if *y + 28 > bottom {
            return;
        }
        let rr = Rect::new(4, *y, r.width - 8, 28);
        p.button(
            rr,
            if on { ORANGE } else { Color::TRANSPARENT },
            6,
            target,
            label,
        );
        let ink = if on { Color::WHITE } else { INK };
        p.symbol(symbol, 14, *y + 6, 16, ink);
        p.left(38, *y + 6, r.width - 48, label, 13, ink);
        *y += 30;
    };
    let music = matches!(
        app.view,
        View::Home | View::New | View::Library(_) | View::Album(_) | View::Artist(_) | View::Search
    );
    group(p, &mut y, "Library");
    item(p, &mut y, "music", "Music", "music:library:songs", music);
    item(
        p,
        &mut y,
        "radio",
        "Radio",
        "music:radio",
        app.view == View::Radio,
    );
    y += 8;
    group(p, &mut y, "Playlists");
    item(
        p,
        &mut y,
        "queue",
        "Play Queue",
        "music:queue",
        matches!(app.view, View::Queue | View::NowPlaying),
    );
    item(
        p,
        &mut y,
        "star",
        "My Top Rated",
        "music:liked",
        app.view == View::Liked,
    );
    for list in &app.catalog.playlists {
        let on = app.view == View::Playlist(list.id.clone());
        item(
            p,
            &mut y,
            "list-view",
            &list.title,
            &format!("music:playlist:{}", list.id),
            on,
        );
    }
    // The "+" under the source list makes a new playlist.
    art::icon(
        p,
        Rect::new(8, r.y + r.height as i32 - 32, 28, 28),
        "plus",
        16,
        INK,
        "music:compose",
        "New Playlist",
    );
}

/// The Artist | Album browser. Returns where the song table starts.
fn browser(app: &Music, p: &mut Painter, area: Rect, y: i32) -> i32 {
    let c = &app.catalog;
    let height = 132u32;
    let half = area.width / 2;
    let artists: Vec<&super::Artist> = {
        let mut a: Vec<&super::Artist> = c.artists.iter().collect();
        a.sort_by_key(|a| a.name.to_lowercase());
        a
    };
    let albums: Vec<&super::Album> = c
        .albums
        .iter()
        .filter(|a| app.filter_artist.as_ref().is_none_or(|f| a.artist == *f))
        .collect();
    let pane =
        |p: &mut Painter, x: i32, width: u32, title: &str, rows: Vec<(String, String, bool)>| {
            p.box_(Rect::new(x, y, width, height), Color::WHITE, 0);
            p.box_(Rect::new(x, y, width, 22), SIDE, 0);
            p.strong(x + 8, y + 3, width - 16, title, 12, INK);
            p.hline(x, y + 22, width, LINE);
            let mut ry = y + 23;
            for (label, target, on) in rows {
                if ry + 20 > y + height as i32 {
                    break;
                }
                p.button(
                    Rect::new(x, ry, width, 20),
                    if on { ORANGE } else { Color::TRANSPARENT },
                    0,
                    &target,
                    &label,
                );
                p.left(
                    x + 8,
                    ry + 2,
                    width - 16,
                    &label,
                    12,
                    if on { Color::WHITE } else { INK },
                );
                ry += 20;
            }
            p.vline(x + width as i32 - 1, y, height, LINE);
        };
    let mut rows = vec![(
        format!(
            "All {} {} ({})",
            artists.len(),
            if artists.len() == 1 {
                "artist"
            } else {
                "artists"
            },
            c.tracks.len()
        ),
        "music:filter-artist:all".to_owned(),
        app.filter_artist.is_none(),
    )];
    rows.extend(artists.iter().map(|a| {
        let n = c.tracks.iter().filter(|t| t.artist == a.id).count();
        (
            format!("{} ({n})", a.name),
            format!("music:filter-artist:{}", a.id),
            app.filter_artist.as_deref() == Some(a.id.as_str()),
        )
    }));
    pane(p, area.x, half, "Artist", rows);
    let total: usize = albums.iter().map(|a| a.tracks.len()).sum();
    let mut rows = vec![(
        format!(
            "All {} {} ({total})",
            albums.len(),
            if albums.len() == 1 { "album" } else { "albums" }
        ),
        "music:filter-album:all".to_owned(),
        app.filter_album.is_none(),
    )];
    rows.extend(albums.iter().map(|a| {
        (
            format!("{} ({})", a.title, a.tracks.len()),
            format!("music:filter-album:{}", a.id),
            app.filter_album.as_deref() == Some(a.id.as_str()),
        )
    }));
    pane(p, area.x + half as i32, area.width - half, "Album", rows);
    p.hline(area.x, y + height as i32, area.width, LINE);
    y + height as i32 + 1
}

/// The song table. Returns the status line's summary of what it lists.
#[allow(clippy::too_many_arguments)]
fn table(
    app: &Music,
    p: &mut Painter,
    env: &crate::AppEnv<'_>,
    area: Rect,
    mut y: i32,
    tracks: &[&Track],
    context: &str,
    jump: bool,
) -> String {
    let c = &app.catalog;
    let live = app.live(env.clock_us);
    let w = area.width;
    let cols: [(u32, &str); 6] = [
        (0, ""),
        (28, "Track"),
        (80, "Title"),
        (w * 40 / 100, "Genre"),
        (w * 55 / 100, "Artist"),
        (w * 75 / 100, "Album"),
    ];
    p.box_(Rect::new(area.x, y, w, 24), Color::WHITE, 0);
    for (dx, label) in cols {
        p.left(area.x + dx as i32 + 6, y + 4, 120, label, 12, INK);
    }
    p.right(area.x + w as i32 - 78, y + 4, 50, "Time", 12, INK);
    p.hline(area.x, y + 24, w, LINE);
    y += 25;
    for (i, t) in tracks.iter().enumerate() {
        if y + 24 > area.y + area.height as i32 {
            break;
        }
        let current = live
            .as_ref()
            .is_some_and(|l| l.id == t.id && (!jump || l.index == i));
        let target = if jump {
            format!("music:jump:{i}")
        } else {
            format!("music:play:{context}@{}", t.id)
        };
        art::row(
            p,
            Rect::new(area.x, y, w, 24),
            if i % 2 == 1 {
                ZEBRA
            } else {
                Color::TRANSPARENT
            },
            0,
            &target,
            &t.title,
        );
        if current {
            p.symbol(
                if live.as_ref().is_some_and(|l| l.playing) {
                    "volume"
                } else {
                    "pause"
                },
                area.x + 8,
                y + 4,
                14,
                INK,
            );
        }
        let album = c.album(&t.album);
        let number = album
            .and_then(|a| a.tracks.iter().position(|x| *x == t.id))
            .map(|n| (n + 1).to_string())
            .unwrap_or_default();
        let cells = [
            number,
            t.title.clone(),
            album.map(|a| a.genre.clone()).unwrap_or_default(),
            c.artist_name(&t.artist),
            album.map(|a| a.title.clone()).unwrap_or_default(),
        ];
        for (k, text) in cells.iter().enumerate() {
            let (dx, _) = cols[k + 1];
            let next = cols.get(k + 2).map_or(w - 90, |n| n.0);
            p.label(
                area.x + dx as i32 + 6,
                y + 4,
                (next - dx).saturating_sub(12),
                text,
                12,
                INK,
                current,
                Align::Left,
            );
        }
        p.right(
            area.x + w as i32 - 78,
            y + 4,
            50,
            &clock(t.duration_ms),
            12,
            INK,
        );
        // The row's context menu; a right click in Rhythmbox, a "…" here.
        art::icon(
            p,
            Rect::new(area.x + w as i32 - 24, y, 24, 24),
            "more",
            14,
            MUTED,
            &format!("music:menu:{}", t.id),
            &format!("More for {}", t.title),
        );
        y += 24;
    }
    summary(tracks)
}

fn search_field(app: &Music, p: &mut Painter, area: Rect, y: i32) -> i32 {
    let field = Rect::new(area.x + 8, y + 6, area.width.saturating_sub(100), 30);
    p.button(
        field,
        Color::WHITE,
        6,
        "music:search-field",
        "Search all fields",
    );
    p.border(
        field,
        Color::TRANSPARENT,
        6,
        if app.focus == super::Focus::Search {
            ORANGE
        } else {
            LINE
        },
    );
    p.symbol("search", field.x + 8, field.y + 8, 14, MUTED);
    p.left(
        field.x + 30,
        field.y + 7,
        field.width - 40,
        if app.query.is_empty() {
            "Search all fields"
        } else {
            &app.query
        },
        13,
        if app.query.is_empty() { MUTED } else { INK },
    );
    let go = Rect::new(field.x + field.width as i32 + 6, field.y, 80, 30);
    if app.query.trim().is_empty() {
        p.box_(go, Color(0, 0, 0, 8), 6);
        p.label(
            go.x,
            go.y + 7,
            go.width,
            "Search",
            13,
            MUTED,
            false,
            Align::Center,
        );
        p.disabled("Type something to search for");
    } else {
        p.button(go, BUTTON, 6, "music:search", "Search");
        p.label(
            go.x,
            go.y + 7,
            go.width,
            "Search",
            13,
            INK,
            false,
            Align::Center,
        );
    }
    y + 42
}

fn main(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, area: Rect) -> String {
    let c = &app.catalog;
    if !app.loaded {
        p.center(
            area.x,
            area.y + 100,
            area.width,
            app.status.notice().unwrap_or("Loading…"),
            13,
            MUTED,
        );
        return String::new();
    }
    match &app.view {
        View::Radio => {
            let mut y = area.y;
            p.box_(Rect::new(area.x, y, area.width, 24), Color::WHITE, 0);
            p.left(area.x + 30, y + 4, 200, "Title", 12, INK);
            p.left(area.x + area.width as i32 / 2, y + 4, 200, "Genre", 12, INK);
            p.hline(area.x, y + 24, area.width, LINE);
            y += 25;
            let live_context = c
                .player
                .as_ref()
                .map(|p| p.context.clone())
                .unwrap_or_default();
            for (i, a) in c.artists.iter().enumerate() {
                let context = format!("station:{}", a.id);
                art::row(
                    p,
                    Rect::new(area.x, y, area.width, 24),
                    if i % 2 == 1 {
                        ZEBRA
                    } else {
                        Color::TRANSPARENT
                    },
                    0,
                    &format!("music:play:{context}"),
                    &format!("{} Radio", a.name),
                );
                if live_context == context {
                    p.symbol("volume", area.x + 8, y + 4, 14, INK);
                }
                p.left(
                    area.x + 30,
                    y + 4,
                    area.width / 2 - 40,
                    &format!("{} Radio", a.name),
                    12,
                    INK,
                );
                let genre = c
                    .albums
                    .iter()
                    .find(|al| al.artist == a.id)
                    .map(|al| al.genre.clone())
                    .unwrap_or_default();
                p.left(
                    area.x + area.width as i32 / 2,
                    y + 4,
                    area.width / 2 - 10,
                    &genre,
                    12,
                    INK,
                );
                y += 24;
            }
            format!("{} stations", c.artists.len())
        }
        View::Queue | View::NowPlaying => {
            let Some(player) = &c.player else {
                p.center(
                    area.x,
                    area.y + 60,
                    area.width,
                    "The play queue is empty.",
                    13,
                    MUTED,
                );
                return "0 songs".into();
            };
            let tracks: Vec<&Track> = player.queue.iter().filter_map(|id| c.track(id)).collect();
            table(app, p, env, area, area.y, &tracks, "", true)
        }
        View::Playlist(id) => {
            let Some(l) = c.playlist(id) else {
                return String::new();
            };
            let tracks: Vec<&Track> = l.items.iter().filter_map(|t| c.track(t)).collect();
            if tracks.is_empty() {
                p.center(
                    area.x,
                    area.y + 60,
                    area.width,
                    "This playlist is empty. Add songs from a song's menu.",
                    13,
                    MUTED,
                );
            }
            table(
                app,
                p,
                env,
                area,
                area.y,
                &tracks,
                &format!("playlist:{id}"),
                false,
            )
        }
        View::Liked => {
            let tracks = c.liked_songs();
            table(app, p, env, area, area.y, &tracks, "liked", false)
        }
        View::Search => {
            let y = search_field(app, p, area, area.y);
            let tracks: Vec<&Track> = match &app.results {
                Some(r) => r.tracks.iter().filter_map(|t| c.track(t)).collect(),
                None => vec![],
            };
            if app.results.as_ref().is_some_and(|r| r.is_empty()) {
                p.center(area.x, y + 60, area.width, "No matches.", 13, MUTED);
            }
            table(app, p, env, area, y, &tracks, "track", false)
        }
        // Music: the whole collection through the browser. Opening an album or artist from
        // a menu narrows the browser to it, which is how Rhythmbox shows one.
        view => {
            let (artist, album) = match view {
                View::Album(id) => (c.album(id).map(|a| a.artist.clone()), Some(id.clone())),
                View::Artist(id) => (Some(id.clone()), None),
                _ => (app.filter_artist.clone(), app.filter_album.clone()),
            };
            let mut y = search_field(app, p, area, area.y);
            let mut shown = app.clone();
            shown.filter_artist = artist.clone();
            shown.filter_album = album.clone();
            y = browser(&shown, p, area, y);
            let mut tracks: Vec<&Track> = c
                .tracks
                .iter()
                .filter(|t| artist.as_ref().is_none_or(|a| t.artist == *a))
                .filter(|t| album.as_ref().is_none_or(|a| t.album == *a))
                .collect();
            tracks.sort_by_key(|t| {
                let album = c.album(&t.album);
                (
                    c.artist_name(&t.artist).to_lowercase(),
                    album.map(|a| a.title.to_lowercase()).unwrap_or_default(),
                    album
                        .and_then(|a| a.tracks.iter().position(|x| *x == t.id))
                        .unwrap_or(0),
                )
            });
            table(app, p, env, area, y, &tracks, "charts", false)
        }
    }
}
