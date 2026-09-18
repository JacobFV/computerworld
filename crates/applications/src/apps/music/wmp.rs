//! Media Player for Windows 11: a Mica navigation pane (search, Home, Music library, Play
//! queue, Playlists) beside a rounded content page, the library's Songs / Albums / Artists
//! pivot with its accent underline, and the player bar along the bottom with the seek bar
//! over shuffle, previous, the accent-filled play button, next and repeat.
use super::art::{self, Surface};
use super::{clock, summary, Music, Repeat, Shelf, Track, View};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use cw_scene::{Color, Rect};

const MICA: Color = Color::rgb(243, 243, 243);
const PAGE: Color = Color::rgb(249, 249, 249);
const INK: Color = Color::rgb(26, 26, 26);
const MUTED: Color = Color::rgb(96, 96, 96);
const LINE: Color = Color(0, 0, 0, 18);
const ACCENT: Color = Color::rgb(0, 95, 184);
const HOVER: Color = Color(0, 0, 0, 10);

pub fn render(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    p.scene.background = MICA;
    p.box_(Rect::new(0, 0, w, h), MICA, 0);
    let bar = 96u32;
    let nav: u32 = if w >= 640 { 230 } else { 0 };
    if nav > 0 {
        pane(app, p, nav, h.saturating_sub(bar));
    }
    let page = Rect::new(nav as i32, 0, w - nav, h.saturating_sub(bar));
    p.border(
        Rect::new(page.x, page.y, page.width + 8, page.height + 8),
        PAGE,
        8,
        LINE,
    );
    let mark = p.scene.nodes.len();
    body(
        app,
        p,
        env,
        Rect::new(
            page.x + 28,
            page.y + 20,
            page.width.saturating_sub(56),
            page.height.saturating_sub(20),
        ),
    );
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(n.clip.and_then(|c| c.intersection(page)).unwrap_or(page));
    }
    player_bar(
        app,
        p,
        env,
        Rect::new(0, h.saturating_sub(bar) as i32, w, bar),
    );
    let s = Surface {
        fill: Color::rgb(252, 252, 252),
        ink: INK,
        muted: MUTED,
        line: LINE,
        accent: ACCENT,
        radius: 8,
        size: 13,
    };
    let all = Rect::new(0, 0, w, h);
    if app.menu.is_some() {
        art::menu(
            p,
            app,
            DesktopTheme::Windows,
            nav as i32 + 260,
            120,
            all,
            &s,
        );
    }
    if let Some(draft) = &app.draft {
        art::composer(p, draft, all, &s);
    }
}

fn pane(app: &Music, p: &mut Painter, nav: u32, h: u32) {
    // Search box.
    let field = Rect::new(12, 12, nav - 24, 32);
    p.button(
        field,
        Color::rgb(251, 251, 251),
        4,
        "music:search-field",
        "Search",
    );
    p.border(field, Color::TRANSPARENT, 4, LINE);
    if app.focus == super::Focus::Search {
        p.box_(Rect::new(field.x, field.y + 30, field.width, 2), ACCENT, 1);
    }
    p.left(
        field.x + 10,
        field.y + 8,
        field.width - 36,
        if app.query.is_empty() {
            "Search"
        } else {
            &app.query
        },
        13,
        if app.query.is_empty() { MUTED } else { INK },
    );
    p.symbol(
        "search",
        field.x + field.width as i32 - 24,
        field.y + 9,
        14,
        MUTED,
    );
    let mut y = 56;
    let mut item =
        |p: &mut Painter, symbol: &str, label: &str, target: &str, on: bool, indent: i32| {
            if y + 36 > h as i32 {
                return;
            }
            let r = Rect::new(6, y, nav - 12, 36);
            p.button(
                r,
                if on { HOVER } else { Color::TRANSPARENT },
                4,
                target,
                label,
            );
            if on {
                p.box_(Rect::new(r.x, y + 10, 3, 16), ACCENT, 2);
            }
            p.symbol(symbol, 18 + indent, y + 10, 16, INK);
            p.left(46 + indent, y + 9, nav - 60 - indent as u32, label, 14, INK);
            y += 38;
        };
    item(
        p,
        "home",
        "Home",
        "music:home",
        matches!(app.view, View::Home | View::New | View::Radio),
        0,
    );
    item(
        p,
        "music",
        "Music library",
        "music:library:songs",
        matches!(
            app.view,
            View::Library(Shelf::Songs | Shelf::Albums | Shelf::Artists | Shelf::Recent)
                | View::Album(_)
                | View::Artist(_)
        ),
        0,
    );
    item(
        p,
        "queue",
        "Play queue",
        "music:queue",
        matches!(app.view, View::Queue | View::NowPlaying),
        0,
    );
    item(
        p,
        "list-view",
        "Playlists",
        "music:library:playlists",
        app.view == View::Library(Shelf::Playlists),
        0,
    );
    item(
        p,
        "heart",
        "Liked songs",
        "music:liked",
        app.view == View::Liked,
        16,
    );
    for list in &app.catalog.playlists {
        let on = app.view == View::Playlist(list.id.clone());
        item(
            p,
            "list-view",
            &list.title,
            &format!("music:playlist:{}", list.id),
            on,
            16,
        );
    }
}

fn title(p: &mut Painter, app: &Music, area: Rect, text: &str) -> i32 {
    let mut x = area.x;
    if !app.back.is_empty() {
        art::icon(
            p,
            Rect::new(x - 6, area.y + 4, 32, 32),
            "arrow-left",
            16,
            INK,
            "music:back",
            "Back",
        );
        x += 34;
    }
    p.strong(x, area.y, area.width, text, 28, INK);
    let mut y = area.y + 50;
    if let Some(notice) = app.status.notice() {
        p.left(area.x, y - 8, area.width, notice, 12, MUTED);
        y += 14;
    }
    y
}

/// The pivot under a page title; the selected one carries the accent underline.
fn pivot(p: &mut Painter, x: i32, y: i32, items: &[(&str, String, bool)]) -> i32 {
    let mut cx = x;
    for (label, target, on) in items {
        let w = p.measure(label, 14, *on) + 20;
        p.button(
            Rect::new(cx, y, w, 34),
            Color::TRANSPARENT,
            4,
            target,
            label,
        );
        p.label(
            cx,
            y + 7,
            w,
            label,
            14,
            if *on { INK } else { MUTED },
            *on,
            Align::Center,
        );
        if *on {
            p.box_(Rect::new(cx + w as i32 / 2 - 8, y + 30, 16, 3), ACCENT, 2);
        }
        cx += w as i32 + 8;
    }
    y + 48
}

fn button(
    p: &mut Painter,
    x: i32,
    y: i32,
    symbol: &str,
    label: &str,
    target: &str,
    primary: bool,
) -> i32 {
    let w = p.measure(label, 13, false) + 44;
    let r = Rect::new(x, y, w, 32);
    p.button(
        r,
        if primary {
            ACCENT
        } else {
            Color::rgb(251, 251, 251)
        },
        4,
        target,
        label,
    );
    if !primary {
        p.border(r, Color::TRANSPARENT, 4, LINE);
    }
    let ink = if primary { Color::WHITE } else { INK };
    p.symbol(symbol, r.x + 12, r.y + 8, 16, ink);
    p.left(r.x + 34, r.y + 8, w - 36, label, 13, ink);
    x + w as i32 + 8
}

type Tile = (String, String, String, String, bool);
fn tiles(p: &mut Painter, area: Rect, mut y: i32, cards: &[Tile]) -> i32 {
    let size = 150u32;
    let per = ((area.width + 16) / (size + 16)).max(1);
    for chunk in cards.chunks(per as usize) {
        if y > area.y + area.height as i32 {
            break;
        }
        for (i, (key, title, sub, target, round)) in chunk.iter().enumerate() {
            let x = area.x + (i as u32 * (size + 16)) as i32;
            p.button(
                Rect::new(x - 6, y - 6, size + 12, size + 58),
                Color::TRANSPARENT,
                6,
                target,
                title,
            );
            if *round {
                art::avatar(p, Rect::new(x, y, size, size), key, title);
            } else {
                art::cover(p, Rect::new(x, y, size, size), key, 6);
            }
            p.left(x, y + size as i32 + 8, size, title, 13, INK);
            p.left(x, y + size as i32 + 26, size, sub, 12, MUTED);
        }
        y += size as i32 + 64;
    }
    y
}

/// The Songs table: title, artist, album, year, genre and time, zebra striped.
fn table(
    app: &Music,
    p: &mut Painter,
    env: &crate::AppEnv<'_>,
    area: Rect,
    mut y: i32,
    tracks: &[&Track],
    context: &str,
) -> i32 {
    let c = &app.catalog;
    let live = app.live(env.clock_us);
    let w = area.width;
    let cols = [
        (0, "Title"),
        (w * 34 / 100, "Artist"),
        (w * 53 / 100, "Album"),
        (w * 71 / 100, "Genre"),
    ];
    for (dx, label) in cols {
        p.left(area.x + 40 + dx as i32, y, 120, label, 12, MUTED);
    }
    p.right(area.x + w as i32 - 92, y, 50, "Time", 12, MUTED);
    y += 24;
    for (i, t) in tracks.iter().enumerate() {
        if y + 40 > area.y + area.height as i32 {
            break;
        }
        let r = Rect::new(area.x, y, w, 40);
        let current = live.as_ref().is_some_and(|l| l.id == t.id);
        art::row(
            p,
            r,
            if i % 2 == 0 {
                Color(0, 0, 0, 8)
            } else {
                Color::TRANSPARENT
            },
            4,
            &format!("music:play:{context}@{}", t.id),
            &t.title,
        );
        let ink = if current { ACCENT } else { INK };
        if current {
            p.symbol("volume", r.x + 12, y + 12, 16, ACCENT);
        } else {
            p.symbol("music", r.x + 12, y + 12, 16, MUTED);
        }
        let album = c.album(&t.album);
        let cells = [
            (cols[0].0, t.title.clone()),
            (cols[1].0, c.artist_name(&t.artist)),
            (
                cols[2].0,
                album.map(|a| a.title.clone()).unwrap_or_default(),
            ),
            (
                cols[3].0,
                album.map(|a| a.genre.clone()).unwrap_or_default(),
            ),
        ];
        for (k, (dx, text)) in cells.iter().enumerate() {
            let next = cells.get(k + 1).map_or(w - 100, |n| n.0);
            p.left(
                area.x + 40 + *dx as i32,
                y + 11,
                (next - dx).saturating_sub(12),
                text,
                13,
                if k == 0 { ink } else { MUTED },
            );
        }
        p.right(
            area.x + w as i32 - 92,
            y + 11,
            50,
            &clock(t.duration_ms),
            13,
            MUTED,
        );
        art::icon(
            p,
            Rect::new(area.x + w as i32 - 36, y + 4, 32, 32),
            "more",
            16,
            INK,
            &format!("music:menu:{}", t.id),
            &format!("More options for {}", t.title),
        );
        y += 40;
    }
    y
}

fn body(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, area: Rect) {
    let c = &app.catalog;
    if !app.loaded {
        p.center(
            area.x,
            area.y + 120,
            area.width,
            app.status.notice().unwrap_or("Loading…"),
            14,
            MUTED,
        );
        return;
    }
    let album_tile = |a: &super::Album| {
        (
            a.id.clone(),
            a.title.clone(),
            c.artist_name(&a.artist),
            format!("music:album:{}", a.id),
            false,
        )
    };
    match &app.view {
        View::Home | View::New | View::Radio => {
            let mut y = title(p, app, area, "Home");
            let mut seen = vec![];
            let recent: Vec<Tile> = c
                .history
                .iter()
                .filter_map(|id| c.track(id))
                .filter_map(|t| c.album(&t.album))
                .filter(|a| {
                    let fresh = !seen.contains(&a.id);
                    seen.push(a.id.clone());
                    fresh
                })
                .map(album_tile)
                .collect();
            p.strong(area.x, y, area.width, "Recent media", 18, INK);
            y += 34;
            if recent.is_empty() {
                p.left(
                    area.x,
                    y,
                    area.width,
                    "Media you play appears here.",
                    13,
                    MUTED,
                );
                y += 30;
            } else {
                y = tiles(
                    p,
                    area,
                    y,
                    &recent[..recent.len().min(((area.width + 16) / 166).max(1) as usize)],
                );
            }
            p.strong(area.x, y, area.width, "Playlists", 18, INK);
            y += 34;
            let lists: Vec<Tile> = c
                .playlists
                .iter()
                .map(|l| {
                    (
                        l.id.clone(),
                        l.title.clone(),
                        format!("{} songs", l.items.len()),
                        format!("music:playlist:{}", l.id),
                        false,
                    )
                })
                .collect();
            tiles(p, area, y, &lists);
        }
        View::Library(shelf) if *shelf != Shelf::Playlists => {
            let mut y = title(p, app, area, "Music library");
            let items: Vec<(&str, String, bool)> = [Shelf::Songs, Shelf::Albums, Shelf::Artists]
                .into_iter()
                .map(|s| {
                    (
                        s.title(),
                        format!("music:library:{}", s.key()),
                        *shelf == s || (*shelf == Shelf::Recent && s == Shelf::Songs),
                    )
                })
                .collect();
            y = pivot(p, area.x - 10, y, &items);
            match shelf {
                Shelf::Albums => {
                    let cards: Vec<Tile> = c.albums.iter().map(album_tile).collect();
                    tiles(p, area, y, &cards);
                }
                Shelf::Artists => {
                    let cards: Vec<Tile> = c
                        .artists
                        .iter()
                        .map(|a| {
                            (
                                a.id.clone(),
                                a.name.clone(),
                                format!(
                                    "{} songs",
                                    c.tracks.iter().filter(|t| t.artist == a.id).count()
                                ),
                                format!("music:artist:{}", a.id),
                                true,
                            )
                        })
                        .collect();
                    tiles(p, area, y, &cards);
                }
                _ => {
                    let mut all: Vec<&Track> = c.tracks.iter().collect();
                    all.sort_by_key(|t| t.title.to_lowercase());
                    button(
                        p,
                        area.x,
                        y,
                        "shuffle",
                        "Shuffle and play",
                        "music:shuffle-play:charts",
                        true,
                    );
                    table(app, p, env, area, y + 48, &all, "charts");
                }
            }
        }
        View::Library(_) => {
            let y = title(p, app, area, "Playlists");
            button(p, area.x, y, "plus", "New playlist", "music:compose", true);
            let mut cards: Vec<Tile> = vec![(
                "liked".into(),
                "Liked songs".into(),
                format!("{} songs", c.liked.len()),
                "music:liked".into(),
                false,
            )];
            cards.extend(c.playlists.iter().map(|l| {
                (
                    l.id.clone(),
                    l.title.clone(),
                    format!("{} songs", l.items.len()),
                    format!("music:playlist:{}", l.id),
                    false,
                )
            }));
            tiles(p, area, y + 52, &cards);
        }
        View::Album(id) => {
            let Some(a) = c.album(id) else { return };
            let tracks: Vec<&Track> = a.tracks.iter().filter_map(|t| c.track(t)).collect();
            let saved = a.tracks.iter().all(|t| c.library.contains(t));
            let y = header(
                p,
                app,
                area,
                &a.id,
                "Album",
                &a.title,
                &c.artist_name(&a.artist),
                Some(format!("music:artist:{}", a.artist)),
                &format!("{} • {} • {}", a.year, a.genre, summary(&tracks)),
            );
            let mut x = button(
                p,
                area.x,
                y,
                "play",
                "Play all",
                &format!("music:play:album:{id}"),
                true,
            );
            x = button(
                p,
                x,
                y,
                "shuffle",
                "Shuffle",
                &format!("music:shuffle-play:album:{id}"),
                false,
            );
            button(
                p,
                x,
                y,
                if saved { "check" } else { "plus" },
                if saved {
                    "In library"
                } else {
                    "Add to library"
                },
                &format!("music:save-album:{id}"),
                false,
            );
            table(app, p, env, area, y + 50, &tracks, &format!("album:{id}"));
        }
        View::Artist(id) => {
            let Some(a) = c.artist(id) else { return };
            let songs = c.top_songs(id);
            let y = header(
                p,
                app,
                area,
                id,
                "Artist",
                &a.name,
                &format!("{} followers", a.followers),
                None,
                &summary(&songs),
            );
            let x = button(
                p,
                area.x,
                y,
                "shuffle",
                "Shuffle and play",
                &format!("music:shuffle-play:artist:{id}"),
                true,
            );
            button(
                p,
                x,
                y,
                "radio",
                "Artist radio",
                &format!("music:play:station:{id}"),
                false,
            );
            table(app, p, env, area, y + 50, &songs, &format!("artist:{id}"));
        }
        View::Playlist(id) => {
            let Some(l) = c.playlist(id) else { return };
            let tracks: Vec<&Track> = l.items.iter().filter_map(|t| c.track(t)).collect();
            let y = header(
                p,
                app,
                area,
                id,
                "Playlist",
                &l.title,
                &if l.owner.is_empty() {
                    "Everyone".to_owned()
                } else {
                    l.owner.clone()
                },
                None,
                &summary(&tracks),
            );
            if tracks.is_empty() {
                p.left(
                    area.x,
                    y,
                    area.width,
                    "Add songs to this playlist from any song's More options.",
                    13,
                    MUTED,
                );
                return;
            }
            let x = button(
                p,
                area.x,
                y,
                "play",
                "Play all",
                &format!("music:play:playlist:{id}"),
                true,
            );
            button(
                p,
                x,
                y,
                "shuffle",
                "Shuffle",
                &format!("music:shuffle-play:playlist:{id}"),
                false,
            );
            table(
                app,
                p,
                env,
                area,
                y + 50,
                &tracks,
                &format!("playlist:{id}"),
            );
        }
        View::Liked => {
            let tracks = c.liked_songs();
            let y = title(p, app, area, "Liked songs");
            if tracks.is_empty() {
                p.left(
                    area.x,
                    y,
                    area.width,
                    "Songs you like appear here.",
                    13,
                    MUTED,
                );
                return;
            }
            button(p, area.x, y, "play", "Play all", "music:play:liked", true);
            table(app, p, env, area, y + 50, &tracks, "liked");
        }
        View::Search => {
            let y = title(
                p,
                app,
                area,
                &if app.query.is_empty() {
                    "Search".to_owned()
                } else {
                    format!("Results for “{}”", app.query)
                },
            );
            if app.query.trim().is_empty() {
                p.left(
                    area.x,
                    y,
                    area.width,
                    "Type in the search box to find songs, albums and artists.",
                    13,
                    MUTED,
                );
                return;
            }
            button(p, area.x, y, "search", "Search", "music:search", true);
            let mut y = y + 48;
            match &app.results {
                Some(r) if r.is_empty() => {
                    p.left(area.x, y, area.width, "No results.", 13, MUTED);
                }
                Some(r) => {
                    let tracks: Vec<&Track> = r.tracks.iter().filter_map(|t| c.track(t)).collect();
                    if !tracks.is_empty() {
                        p.strong(area.x, y, area.width, "Songs", 18, INK);
                        y = table(
                            app,
                            p,
                            env,
                            area,
                            y + 30,
                            &tracks[..tracks.len().min(5)],
                            "track",
                        ) + 16;
                    }
                    let mut cards: Vec<Tile> = r
                        .albums
                        .iter()
                        .filter_map(|a| c.album(a))
                        .map(album_tile)
                        .collect();
                    cards.extend(r.artists.iter().filter_map(|a| c.artist(a)).map(|a| {
                        (
                            a.id.clone(),
                            a.name.clone(),
                            "Artist".into(),
                            format!("music:artist:{}", a.id),
                            true,
                        )
                    }));
                    if !cards.is_empty() {
                        p.strong(area.x, y, area.width, "Albums and artists", 18, INK);
                        tiles(p, area, y + 34, &cards);
                    }
                }
                None => {}
            }
        }
        View::Queue | View::NowPlaying => {
            let y = title(p, app, area, "Play queue");
            let Some(player) = &c.player else {
                p.left(area.x, y, area.width, "Nothing is queued.", 13, MUTED);
                return;
            };
            let index = app.live(env.clock_us).map_or(player.index, |l| l.index);
            let mut y = y;
            for (i, id) in player.queue.iter().enumerate() {
                let Some(t) = c.track(id) else { continue };
                if y + 40 > area.y + area.height as i32 {
                    break;
                }
                let r = Rect::new(area.x, y, area.width, 40);
                p.button(
                    r,
                    if i == index {
                        Color(ACCENT.0, ACCENT.1, ACCENT.2, 24)
                    } else if i % 2 == 0 {
                        Color(0, 0, 0, 8)
                    } else {
                        Color::TRANSPARENT
                    },
                    4,
                    &format!("music:jump:{i}"),
                    &t.title,
                );
                p.symbol(
                    if i == index { "volume" } else { "music" },
                    r.x + 12,
                    y + 12,
                    16,
                    if i == index { ACCENT } else { MUTED },
                );
                p.left(
                    r.x + 40,
                    y + 11,
                    area.width * 45 / 100,
                    &t.title,
                    13,
                    if i == index { ACCENT } else { INK },
                );
                p.left(
                    r.x + 40 + (area.width * 48 / 100) as i32,
                    y + 11,
                    area.width * 35 / 100,
                    &c.artist_name(&t.artist),
                    13,
                    MUTED,
                );
                p.right(
                    r.x + area.width as i32 - 60,
                    y + 11,
                    50,
                    &clock(t.duration_ms),
                    13,
                    MUTED,
                );
                y += 40;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn header(
    p: &mut Painter,
    app: &Music,
    area: Rect,
    key: &str,
    kind: &str,
    name: &str,
    by: &str,
    by_target: Option<String>,
    meta: &str,
) -> i32 {
    let y = title(p, app, area, "") - if app.back.is_empty() { 50 } else { 10 };
    let size = 160;
    if kind == "Artist" {
        art::avatar(p, Rect::new(area.x, y, size, size), key, name);
    } else {
        art::cover(p, Rect::new(area.x, y, size, size), key, 6);
    }
    let x = area.x + size as i32 + 24;
    let w = area.width.saturating_sub(size + 24);
    p.left(x, y + 16, w, kind, 12, MUTED);
    p.strong(x, y + 36, w, name, 28, INK);
    match by_target {
        Some(target) => {
            let bw = p.measure(by, 14, false).min(w);
            p.button(
                Rect::new(x, y + 78, bw, 22),
                Color::TRANSPARENT,
                2,
                &target,
                by,
            );
            p.left(x, y + 78, w, by, 14, ACCENT);
        }
        None => {
            p.left(x, y + 78, w, by, 14, MUTED);
        }
    }
    p.left(x, y + 104, w, meta, 12, MUTED);
    y + size as i32 + 20
}

fn player_bar(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, r: Rect) {
    p.box_(r, Color::rgb(238, 238, 238), 0);
    p.hline(r.x, r.y, r.width, LINE);
    let live = app.live(env.clock_us);
    let player = app.catalog.player.as_ref();
    // Seek bar across the top of the bar, the times at either end.
    let seek = Rect::new(r.x + 70, r.y + 8, r.width.saturating_sub(140), 16);
    match &live {
        Some(l) => {
            p.left(r.x + 16, r.y + 8, 50, &clock(l.position_ms), 12, INK);
            p.right(
                r.x + r.width as i32 - 66,
                r.y + 8,
                50,
                &clock(l.duration_ms),
                12,
                INK,
            );
            art::scrubber(
                p,
                seek,
                l,
                4,
                ACCENT,
                Color(0, 0, 0, 60),
                Some((14, ACCENT)),
            );
        }
        None => {
            p.box_(
                Rect::new(seek.x, seek.y + 6, seek.width, 4),
                Color(0, 0, 0, 30),
                2,
            );
        }
    }
    let y = r.y + 32;
    if let Some(t) = app.now() {
        art::cover(p, Rect::new(r.x + 16, y + 4, 52, 52), &t.album, 4);
        let tw = r.width / 3 - 90;
        p.strong(r.x + 80, y + 10, tw, &t.title, 14, INK);
        p.left(
            r.x + 80,
            y + 32,
            tw,
            &app.catalog.artist_name(&t.artist),
            12,
            MUTED,
        );
    }
    let cx = r.x + r.width as i32 / 2;
    let on = |b: bool| if b { ACCENT } else { INK };
    let controls: [(&str, &str, &str, i32, bool); 5] = [
        (
            "shuffle",
            "music:shuffle",
            "Shuffle",
            -118,
            player.is_some_and(|p| p.shuffle),
        ),
        ("skip-previous", "music:previous", "Previous", -70, false),
        ("", "music:toggle", "Play", 0, false),
        ("skip-next", "music:next", "Next", 50, false),
        (
            if player.is_some_and(|p| p.repeat == Repeat::One) {
                "repeat-one"
            } else {
                "repeat"
            },
            "music:repeat",
            "Repeat",
            98,
            player.is_some_and(|p| p.repeat != Repeat::Off),
        ),
    ];
    for (symbol, target, label, dx, lit) in controls {
        if target == "music:toggle" {
            let play = Rect::new(cx - 22, y + 8, 44, 44);
            match &live {
                Some(l) => {
                    p.button(
                        play,
                        ACCENT,
                        22,
                        target,
                        if l.playing { "Pause" } else { "Play" },
                    );
                    p.symbol(
                        if l.playing { "pause" } else { "play" },
                        play.x + 12,
                        play.y + 12,
                        20,
                        Color::WHITE,
                    );
                }
                None => {
                    p.box_(play, Color(ACCENT.0, ACCENT.1, ACCENT.2, 90), 22);
                    p.symbol("play", play.x + 12, play.y + 12, 20, Color::WHITE);
                    p.disabled("Nothing is playing");
                }
            }
            continue;
        }
        let rr = Rect::new(cx + dx - 16, y + 14, 32, 32);
        if live.is_none() {
            art::icon_off(p, rr, symbol, 16, INK, "Nothing is playing");
        } else if target == "music:next" && !app.can_next(env.clock_us) {
            art::icon_off(p, rr, symbol, 16, INK, "Nothing is queued after this song");
        } else {
            art::icon(p, rr, symbol, 16, on(lit), target, label);
        }
    }
    let q = Rect::new(r.x + r.width as i32 - 52, y + 14, 32, 32);
    art::icon(
        p,
        q,
        "queue",
        16,
        if matches!(app.view, View::Queue) {
            ACCENT
        } else {
            INK
        },
        "music:queue",
        "Play queue",
    );
}
