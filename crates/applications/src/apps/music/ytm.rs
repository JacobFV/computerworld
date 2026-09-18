//! YouTube Music for Android: the dark app with the red-and-white logo, mood chips over
//! Home, shelves of square album art and round artist avatars, a mini player whose thin
//! progress line runs along its top edge above the Home / Explore / Library bar, and a
//! full-screen player with a white play button and the Up next queue.
use super::art::{self, Surface};
use super::{clock, Music, Repeat, Shelf, Track, View};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use cw_scene::{Color, Rect};

const BG: Color = Color::rgb(3, 3, 3);
const RAISED: Color = Color(255, 255, 255, 26);
const BAR: Color = Color::rgb(33, 33, 33);
const INK: Color = Color::WHITE;
const MUTED: Color = Color::rgb(170, 170, 170);
const RED: Color = Color::rgb(255, 0, 0);
const PAD: i32 = 16;
/// YouTube Music's mood chips, in its order, when some song carries the mood.
const MOODS: &[&str] = &[
    "energize",
    "relax",
    "feel good",
    "workout",
    "commute",
    "focus",
    "party",
    "sleep",
];

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

pub fn render(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    p.scene.background = BG;
    p.box_(Rect::new(0, 0, w, h), BG, 0);
    let all = Rect::new(0, 0, w, h);
    if app.view == View::NowPlaying && app.catalog.player.is_some() {
        now_playing(app, p, env, all);
        overlays(app, p, all);
        return;
    }
    let nav = 64u32;
    let mini = if app.catalog.player.is_some() {
        64u32
    } else {
        0
    };
    let area = Rect::new(0, 0, w, h.saturating_sub(nav + mini));
    // The view scrolls over the bars; each view keeps its own place.
    let pane = p.pane(&app.view_pane(), area);
    body(
        app,
        p,
        env,
        Rect::new(area.x, pane.top(), area.width, area.height),
    );
    p.end_pane(pane, None);
    let bottom = h.saturating_sub(nav) as i32;
    if mini > 0 {
        mini_player(app, p, env, Rect::new(0, bottom - mini as i32, w, mini));
    }
    nav_bar(app, p, Rect::new(0, bottom, w, nav));
    overlays(app, p, all);
}

fn overlays(app: &Music, p: &mut Painter, all: Rect) {
    let s = Surface {
        fill: Color::rgb(40, 40, 40),
        ink: INK,
        muted: MUTED,
        line: Color(255, 255, 255, 30),
        accent: RED,
        radius: 12,
        size: 15,
    };
    if app.menu.is_some() {
        // YouTube Music's song menu is a bottom sheet.
        let rows = app.menu_items(DesktopTheme::Android).len() as i32;
        art::menu(
            p,
            app,
            DesktopTheme::Android,
            0,
            all.height as i32 - rows * 29 - 30,
            all,
            &s,
        );
    }
    if let Some(draft) = &app.draft {
        art::composer(p, draft, all, &s);
    }
}

fn top_bar(app: &Music, p: &mut Painter, area: Rect) -> i32 {
    let y = area.y + 10;
    if app.back.is_empty() {
        p.circle(PAD + 13, y + 16, 13, RED);
        p.symbol("play", PAD + 7, y + 10, 12, Color::WHITE);
        p.strong(PAD + 32, y + 5, 100, "Music", 21, INK);
    } else {
        art::icon(
            p,
            Rect::new(PAD - 6, y, 36, 36),
            "arrow-left",
            22,
            INK,
            "music:back",
            "Back",
        );
    }
    art::icon(
        p,
        Rect::new(area.width as i32 - 52, y, 36, 36),
        "search",
        22,
        INK,
        "music:search-field",
        "Search",
    );
    y + 48
}

fn chips(p: &mut Painter, area: Rect, y: i32, chips: &[(String, String, bool)]) -> i32 {
    let mut x = PAD;
    for (label, target, on) in chips {
        let w = p.measure(label, 14, true) + 24;
        if x + w as i32 > area.width as i32 - 4 {
            break;
        }
        let r = Rect::new(x, y, w, 32);
        p.button(r, if *on { INK } else { RAISED }, 8, target, label);
        p.label(
            r.x,
            r.y + 7,
            w,
            label,
            14,
            if *on { Color::BLACK } else { INK },
            true,
            Align::Center,
        );
        x += w as i32 + 8;
    }
    y + 46
}

fn heading(p: &mut Painter, area: Rect, y: i32, text: &str) -> i32 {
    p.strong(
        PAD,
        y,
        area.width.saturating_sub(PAD as u32 * 2),
        text,
        22,
        INK,
    );
    y + 36
}

/// A square tile with its title and one grey line under it.
#[allow(clippy::too_many_arguments)]
fn tile(
    p: &mut Painter,
    x: i32,
    y: i32,
    size: u32,
    key: &str,
    title: &str,
    sub: &str,
    target: &str,
    round: bool,
) {
    p.button(
        Rect::new(x, y, size, size + 44),
        Color::TRANSPARENT,
        4,
        target,
        title,
    );
    if round {
        art::avatar(p, Rect::new(x, y, size, size), key, title);
    } else {
        art::cover(p, Rect::new(x, y, size, size), key, 4);
    }
    let align = if round { Align::Center } else { Align::Left };
    p.label(x, y + size as i32 + 6, size, title, 14, INK, false, align);
    p.label(x, y + size as i32 + 25, size, sub, 12, MUTED, false, align);
}

type Card = (String, String, String, String, bool);
fn shelf(p: &mut Painter, area: Rect, y: i32, size: u32, cards: &[Card]) -> i32 {
    if cards.is_empty() {
        return y;
    }
    let mut x = PAD;
    for (key, title, sub, target, round) in cards {
        if x > area.width as i32 {
            break;
        }
        tile(p, x, y, size, key, title, sub, target, *round);
        x += size as i32 + 12;
    }
    y + size as i32 + 60
}

/// A song row: art, title, "artist • album" and the ⋮ menu. The row plays in `context`.
#[allow(clippy::too_many_arguments)]
fn song(
    app: &Music,
    p: &mut Painter,
    env: &crate::AppEnv<'_>,
    area: Rect,
    y: i32,
    t: &Track,
    context: &str,
    lead: Option<String>,
) -> i32 {
    let c = &app.catalog;
    let current = app.live(env.clock_us).is_some_and(|l| l.id == t.id);
    let r = Rect::new(0, y, area.width, 60);
    p.button(
        r,
        if current { RAISED } else { Color::TRANSPARENT },
        0,
        &format!("music:play:{context}@{}", t.id),
        &t.title,
    );
    let mut x = PAD;
    if let Some(lead) = lead {
        p.label(x, y + 20, 24, &lead, 15, MUTED, true, Align::Center);
        x += 32;
    }
    art::cover(p, Rect::new(x, y + 6, 48, 48), &t.album, 4);
    let tw = area.width.saturating_sub(x as u32 + 48 + 16 + 52);
    p.left(
        x + 60,
        y + 10,
        tw,
        &t.title,
        15,
        if current { RED } else { INK },
    );
    let album = c
        .album(&t.album)
        .map(|a| a.title.clone())
        .unwrap_or_default();
    p.left(
        x + 60,
        y + 32,
        tw,
        &format!("{} • {album}", c.artist_name(&t.artist)),
        13,
        MUTED,
    );
    art::icon(
        p,
        Rect::new(area.width as i32 - 48, y + 12, 36, 36),
        "more-vertical",
        20,
        INK,
        &format!("music:menu:{}", t.id),
        &format!("More for {}", t.title),
    );
    y + 60
}

fn body(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, area: Rect) {
    let c = &app.catalog;
    let mut y = top_bar(app, p, area);
    if !app.loaded {
        p.center(
            0,
            y + 80,
            area.width,
            app.status.notice().unwrap_or("Loading…"),
            15,
            MUTED,
        );
        return;
    }
    if let Some(notice) = app.status.notice() {
        p.left(PAD, y - 6, area.width, notice, 12, MUTED);
        y += 12;
    }
    match &app.view {
        View::Home => {
            let tags = c.tags();
            let moods: Vec<(String, String, bool)> = MOODS
                .iter()
                .filter(|m| tags.iter().any(|t| t == *m))
                .map(|m| {
                    (
                        cap(m),
                        format!("music:mood:{m}"),
                        app.mood.as_deref() == Some(*m),
                    )
                })
                .collect();
            y = chips(p, area, y, &moods);
            let fits = |t: &&Track| app.mood.as_ref().is_none_or(|m| t.tags.contains(m));
            let again: Vec<Card> = c
                .history
                .iter()
                .filter_map(|id| c.track(id))
                .filter(fits)
                .map(|t| {
                    (
                        t.album.clone(),
                        t.title.clone(),
                        c.artist_name(&t.artist),
                        format!("music:play:station:{}@{}", t.artist, t.id),
                        false,
                    )
                })
                .collect();
            if !again.is_empty() {
                y = heading(p, area, y, "Listen again");
                y = shelf(p, area, y, 128, &again);
            }
            let mut picks: Vec<&Track> = c.tracks.iter().filter(fits).collect();
            picks.sort_by_key(|t| std::cmp::Reverse(t.plays));
            y = heading(p, area, y, "Quick picks");
            let context = match &app.mood {
                Some(m) => format!("mood:{m}"),
                None => "charts".into(),
            };
            for t in picks.iter().take(4) {
                y = song(app, p, env, area, y, t, &context, None);
            }
            y += 16;
            let albums: Vec<Card> = c
                .albums
                .iter()
                .filter(|a| a.tracks.iter().filter_map(|t| c.track(t)).any(|t| fits(&t)))
                .map(|a| {
                    (
                        a.id.clone(),
                        a.title.clone(),
                        format!("{} • {}", a.kind, c.artist_name(&a.artist)),
                        format!("music:album:{}", a.id),
                        false,
                    )
                })
                .collect();
            if !albums.is_empty() {
                y = heading(p, area, y, "Recommended albums");
                shelf(p, area, y, 150, &albums);
            }
        }
        View::New | View::Radio => {
            p.strong(PAD, y, area.width, "Explore", 26, INK);
            y += 44;
            y = heading(p, area, y, "New albums & singles");
            let albums: Vec<Card> = c
                .albums
                .iter()
                .map(|a| {
                    (
                        a.id.clone(),
                        a.title.clone(),
                        format!("{} • {}", a.kind, c.artist_name(&a.artist)),
                        format!("music:album:{}", a.id),
                        false,
                    )
                })
                .collect();
            y = shelf(p, area, y, 150, &albums);
            y = heading(p, area, y, "Moods & genres");
            let tags = c.tags();
            let moods: Vec<&str> = MOODS
                .iter()
                .copied()
                .filter(|m| tags.iter().any(|t| t == m))
                .collect();
            let cell = (area.width - PAD as u32 * 2 - 10) / 2;
            for (i, m) in moods.iter().enumerate() {
                let r = Rect::new(
                    PAD + (i as u32 % 2 * (cell + 10)) as i32,
                    y + (i as u32 / 2 * 52) as i32,
                    cell,
                    44,
                );
                p.button(
                    r,
                    Color::rgb(41, 41, 41),
                    6,
                    &format!("music:mood:{m}"),
                    &cap(m),
                );
                p.box_(Rect::new(r.x, r.y, 6, 44), art::tint(m), 3);
                p.strong(r.x + 18, r.y + 13, cell - 24, &cap(m), 15, INK);
            }
            y += moods.len().div_ceil(2) as i32 * 52 + 12;
            y = heading(p, area, y, "Top songs");
            let mut top: Vec<&Track> = c.tracks.iter().collect();
            top.sort_by_key(|t| std::cmp::Reverse(t.plays));
            for (i, t) in top.iter().enumerate() {
                y = song(app, p, env, area, y, t, "charts", Some((i + 1).to_string()));
            }
        }
        View::Library(shelf_) => library(app, p, env, area, y, *shelf_),
        View::Album(id) => {
            let Some(a) = c.album(id) else { return };
            let tracks: Vec<&Track> = a.tracks.iter().filter_map(|t| c.track(t)).collect();
            let saved = a.tracks.iter().all(|t| c.library.contains(t));
            y = header(
                p,
                area,
                y,
                &a.id,
                &a.title,
                &format!("{} • {} • {}", a.kind, c.artist_name(&a.artist), a.year),
                &super::summary(&tracks),
                false,
            );
            y = actions(
                p,
                area,
                y,
                &format!("album:{id}"),
                Some((saved, format!("music:save-album:{id}"))),
            );
            for (i, t) in tracks.iter().enumerate() {
                y = song(
                    app,
                    p,
                    env,
                    area,
                    y,
                    t,
                    &format!("album:{id}"),
                    Some((i + 1).to_string()),
                );
            }
        }
        View::Playlist(id) => {
            let Some(l) = c.playlist(id) else { return };
            let tracks: Vec<&Track> = l.items.iter().filter_map(|t| c.track(t)).collect();
            y = header(
                p,
                area,
                y,
                &l.id,
                &l.title,
                &format!(
                    "Playlist • {}",
                    if l.owner.is_empty() {
                        "YouTube Music".to_owned()
                    } else {
                        cap(&l.owner)
                    }
                ),
                &super::summary(&tracks),
                false,
            );
            if tracks.is_empty() {
                p.center(0, y, area.width, "This playlist is empty", 14, MUTED);
            } else {
                y = actions(p, area, y, &format!("playlist:{id}"), None);
            }
            for t in tracks {
                y = song(app, p, env, area, y, t, &format!("playlist:{id}"), None);
            }
        }
        View::Liked => {
            let tracks = c.liked_songs();
            y = header(
                p,
                area,
                y,
                "liked",
                "Liked music",
                "Auto playlist",
                &super::summary(&tracks),
                false,
            );
            if tracks.is_empty() {
                p.center(
                    0,
                    y,
                    area.width,
                    "Songs you like will show up here",
                    14,
                    MUTED,
                );
            } else {
                y = actions(p, area, y, "liked", None);
            }
            for t in tracks {
                y = song(app, p, env, area, y, t, "liked", None);
            }
        }
        View::Artist(id) => {
            let Some(a) = c.artist(id) else { return };
            let banner = Rect::new(0, y - 8, area.width, 200);
            let tint = art::tint(id);
            p.gradient(banner, art::mix(tint, Color::BLACK, 20), BG, 16);
            p.strong(
                PAD,
                banner.y + 110,
                area.width.saturating_sub(32),
                &a.name,
                34,
                INK,
            );
            p.left(
                PAD,
                banner.y + 154,
                area.width,
                &format!("{} subscribers", short(a.followers)),
                14,
                MUTED,
            );
            y = banner.y + banner.height as i32 + 4;
            let subscribed = c.subscriptions.contains(id);
            let pills = [
                (
                    if subscribed {
                        "Subscribed"
                    } else {
                        "Subscribe"
                    }
                    .to_owned(),
                    format!("music:subscribe:{id}"),
                    if subscribed { RAISED } else { INK },
                    if subscribed { INK } else { Color::BLACK },
                ),
                (
                    "Shuffle".to_owned(),
                    format!("music:shuffle-play:artist:{id}"),
                    RAISED,
                    INK,
                ),
                (
                    "Radio".to_owned(),
                    format!("music:play:station:{id}"),
                    RAISED,
                    INK,
                ),
            ];
            let mut x = PAD;
            for (label, target, fill, ink) in pills {
                let w = p.measure(&label, 14, true) + 32;
                let r = Rect::new(x, y, w, 36);
                p.button(r, fill, 18, &target, &label);
                p.label(r.x, r.y + 9, w, &label, 14, ink, true, Align::Center);
                x += w as i32 + 8;
            }
            y += 56;
            y = heading(p, area, y, "Songs");
            for t in c.top_songs(id).into_iter().take(4) {
                y = song(app, p, env, area, y, t, &format!("artist:{id}"), None);
            }
            y += 12;
            let albums: Vec<Card> = c
                .albums
                .iter()
                .filter(|al| al.artist == *id)
                .map(|al| {
                    (
                        al.id.clone(),
                        al.title.clone(),
                        format!("{} • {}", al.kind, al.year),
                        format!("music:album:{}", al.id),
                        false,
                    )
                })
                .collect();
            if !albums.is_empty() {
                y = heading(p, area, y, "Albums");
                shelf(p, area, y, 150, &albums);
            }
        }
        View::Search => search(app, p, env, area, y),
        View::Queue | View::NowPlaying => up_next(app, p, env, area, y),
    }
}

fn short(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}K", n / 1_000),
        _ => format!("{}.{}M", n / 1_000_000, n / 100_000 % 10),
    }
}

/// Album and playlist header: big centred art, title, two grey lines.
#[allow(clippy::too_many_arguments)]
fn header(
    p: &mut Painter,
    area: Rect,
    y: i32,
    key: &str,
    title: &str,
    line: &str,
    meta: &str,
    _round: bool,
) -> i32 {
    let size = (area.width * 3 / 5).min(240);
    let x = (area.width as i32 - size as i32) / 2;
    if key == "liked" {
        p.gradient(
            Rect::new(x, y, size, size),
            Color::rgb(120, 90, 200),
            Color::rgb(60, 40, 120),
            12,
        );
        p.symbol(
            "thumb-up-fill",
            x + size as i32 / 2 - 32,
            y + size as i32 / 2 - 32,
            64,
            INK,
        );
    } else {
        art::cover(p, Rect::new(x, y, size, size), key, 6);
    }
    let mut ty = y + size as i32 + 16;
    p.label(
        PAD,
        ty,
        area.width - PAD as u32 * 2,
        title,
        24,
        INK,
        true,
        Align::Center,
    );
    ty += 34;
    p.label(
        PAD,
        ty,
        area.width - PAD as u32 * 2,
        line,
        14,
        MUTED,
        false,
        Align::Center,
    );
    ty += 22;
    p.label(
        PAD,
        ty,
        area.width - PAD as u32 * 2,
        meta,
        14,
        MUTED,
        false,
        Align::Center,
    );
    ty + 30
}

/// Save, the big white Play, and Shuffle.
fn actions(
    p: &mut Painter,
    area: Rect,
    y: i32,
    context: &str,
    save: Option<(bool, String)>,
) -> i32 {
    let cx = area.width as i32 / 2;
    if let Some((saved, target)) = save {
        let r = Rect::new(cx - 100, y + 8, 44, 44);
        p.box_(r, RAISED, 22);
        art::icon(
            p,
            r,
            if saved { "check" } else { "plus" },
            22,
            INK,
            &target,
            if saved {
                "Remove from library"
            } else {
                "Save to library"
            },
        );
    }
    let play = Rect::new(cx - 32, y, 64, 64);
    p.button(play, INK, 32, &format!("music:play:{context}"), "Play");
    p.symbol("play", play.x + 20, play.y + 18, 28, Color::BLACK);
    let r = Rect::new(cx + 56, y + 8, 44, 44);
    p.box_(r, RAISED, 22);
    art::icon(
        p,
        r,
        "shuffle",
        22,
        INK,
        &format!("music:shuffle-play:{context}"),
        "Shuffle",
    );
    y + 84
}

fn library(
    app: &Music,
    p: &mut Painter,
    env: &crate::AppEnv<'_>,
    area: Rect,
    mut y: i32,
    shelf_: Shelf,
) {
    let c = &app.catalog;
    p.strong(PAD, y, area.width, "Library", 26, INK);
    y += 44;
    let tabs: Vec<(String, String, bool)> = [
        Shelf::Playlists,
        Shelf::Songs,
        Shelf::Albums,
        Shelf::Artists,
    ]
    .into_iter()
    .map(|s| {
        (
            s.title().to_owned(),
            format!("music:library:{}", s.key()),
            shelf_ == s,
        )
    })
    .collect();
    y = chips(p, area, y, &tabs);
    match shelf_ {
        Shelf::Recent | Shelf::Playlists => {
            let r = Rect::new(PAD, y, 160, 36);
            p.button(r, RAISED, 18, "music:compose", "New playlist");
            p.symbol("plus", r.x + 12, r.y + 9, 18, INK);
            p.strong(r.x + 36, r.y + 9, 120, "New playlist", 14, INK);
            y += 52;
            let mut rows: Vec<(String, String, String, String)> = vec![(
                "liked".into(),
                "Liked music".into(),
                format!("Auto playlist • {} songs", c.liked.len()),
                "music:liked".into(),
            )];
            rows.extend(c.playlists.iter().map(|l| {
                (
                    l.id.clone(),
                    l.title.clone(),
                    format!(
                        "Playlist • {} • {} songs",
                        if l.owner.is_empty() {
                            "YouTube Music".to_owned()
                        } else {
                            cap(&l.owner)
                        },
                        l.items.len()
                    ),
                    format!("music:playlist:{}", l.id),
                )
            }));
            for (key, title, sub, target) in rows {
                p.button(
                    Rect::new(0, y, area.width, 68),
                    Color::TRANSPARENT,
                    0,
                    &target,
                    &title,
                );
                if key == "liked" {
                    p.gradient(
                        Rect::new(PAD, y + 6, 56, 56),
                        Color::rgb(120, 90, 200),
                        Color::rgb(60, 40, 120),
                        6,
                    );
                    p.symbol("thumb-up-fill", PAD + 16, y + 22, 24, INK);
                } else {
                    art::cover(p, Rect::new(PAD, y + 6, 56, 56), &key, 4);
                }
                p.left(PAD + 70, y + 14, area.width - 110, &title, 16, INK);
                p.left(PAD + 70, y + 38, area.width - 110, &sub, 13, MUTED);
                y += 68;
            }
        }
        Shelf::Songs => {
            let songs = c.library_songs();
            if songs.is_empty() {
                p.center(
                    0,
                    y + 20,
                    area.width,
                    "Songs you save will show up here",
                    14,
                    MUTED,
                );
                return;
            }
            let r = Rect::new(PAD, y, 140, 36);
            p.button(r, INK, 18, "music:shuffle-play:library", "Shuffle all");
            p.symbol("shuffle", r.x + 14, r.y + 9, 18, Color::BLACK);
            p.strong(r.x + 40, r.y + 9, 100, "Shuffle all", 14, Color::BLACK);
            y += 52;
            for t in songs {
                y = song(app, p, env, area, y, t, "library", None);
            }
        }
        Shelf::Albums => {
            let cards: Vec<Card> = c
                .library_albums()
                .into_iter()
                .map(|a| {
                    (
                        a.id.clone(),
                        a.title.clone(),
                        format!("{} • {}", a.kind, c.artist_name(&a.artist)),
                        format!("music:album:{}", a.id),
                        false,
                    )
                })
                .collect();
            grid(p, area, y, &cards, "Albums you save will show up here");
        }
        Shelf::Artists => {
            let mut artists: Vec<&super::Artist> = c.library_artists();
            for id in &c.subscriptions {
                if let Some(a) = c.artist(id) {
                    if !artists.iter().any(|x| x.id == a.id) {
                        artists.push(a);
                    }
                }
            }
            for a in artists {
                p.button(
                    Rect::new(0, y, area.width, 68),
                    Color::TRANSPARENT,
                    0,
                    &format!("music:artist:{}", a.id),
                    &a.name,
                );
                art::avatar(p, Rect::new(PAD, y + 6, 56, 56), &a.id, &a.name);
                p.left(PAD + 70, y + 14, area.width - 110, &a.name, 16, INK);
                p.left(
                    PAD + 70,
                    y + 38,
                    area.width - 110,
                    &format!("{} subscribers", short(a.followers)),
                    13,
                    MUTED,
                );
                y += 68;
            }
        }
    }
}

fn grid(p: &mut Painter, area: Rect, mut y: i32, cards: &[Card], empty: &str) {
    if cards.is_empty() {
        p.center(0, y + 20, area.width, empty, 14, MUTED);
        return;
    }
    let size = (area.width - PAD as u32 * 2 - 12) / 2;
    for chunk in cards.chunks(2) {
        for (i, (key, title, sub, target, round)) in chunk.iter().enumerate() {
            tile(
                p,
                PAD + i as i32 * (size as i32 + 12),
                y,
                size,
                key,
                title,
                sub,
                target,
                *round,
            );
        }
        y += size as i32 + 60;
    }
}

fn search(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, area: Rect, mut y: i32) {
    let c = &app.catalog;
    let field = Rect::new(PAD, y, area.width - PAD as u32 * 2 - 70, 44);
    p.button(
        field,
        Color::rgb(40, 40, 40),
        22,
        "music:search-field",
        "Search songs, albums, artists",
    );
    p.symbol("search", field.x + 14, field.y + 12, 20, MUTED);
    p.left(
        field.x + 44,
        field.y + 12,
        field.width - 56,
        if app.query.is_empty() {
            "Search songs, albums, artists"
        } else {
            &app.query
        },
        15,
        if app.query.is_empty() { MUTED } else { INK },
    );
    let go = Rect::new(field.x + field.width as i32 + 8, y + 4, 62, 36);
    if app.query.trim().is_empty() {
        p.label(
            go.x,
            go.y + 9,
            go.width,
            "Search",
            14,
            Color(255, 255, 255, 80),
            true,
            Align::Center,
        );
        p.disabled("Type something to search for");
    } else {
        p.button(go, INK, 18, "music:search", "Search");
        p.label(
            go.x,
            go.y + 9,
            go.width,
            "Search",
            14,
            Color::BLACK,
            true,
            Align::Center,
        );
    }
    y += 60;
    match &app.results {
        Some(r) if r.is_empty() => {
            p.center(
                0,
                y + 20,
                area.width,
                &format!("No results for “{}”", r.query),
                14,
                MUTED,
            );
        }
        Some(r) => {
            if let Some(a) = r.artists.first().and_then(|a| c.artist(a)) {
                y = heading(p, area, y, "Top result");
                p.button(
                    Rect::new(PAD, y, area.width - PAD as u32 * 2, 72),
                    RAISED,
                    10,
                    &format!("music:artist:{}", a.id),
                    &a.name,
                );
                art::avatar(p, Rect::new(PAD + 10, y + 8, 56, 56), &a.id, &a.name);
                p.strong(PAD + 80, y + 14, area.width - 120, &a.name, 18, INK);
                p.left(
                    PAD + 80,
                    y + 40,
                    area.width - 120,
                    &format!("Artist • {} subscribers", short(a.followers)),
                    13,
                    MUTED,
                );
                y += 88;
            }
            if !r.tracks.is_empty() {
                y = heading(p, area, y, "Songs");
                for t in r.tracks.iter().filter_map(|t| c.track(t)).take(4) {
                    y = song(
                        app,
                        p,
                        env,
                        area,
                        y,
                        t,
                        &format!("station:{}", t.artist),
                        None,
                    );
                }
            }
            let albums: Vec<Card> = r
                .albums
                .iter()
                .filter_map(|a| c.album(a))
                .map(|a| {
                    (
                        a.id.clone(),
                        a.title.clone(),
                        format!("{} • {}", a.kind, c.artist_name(&a.artist)),
                        format!("music:album:{}", a.id),
                        false,
                    )
                })
                .collect();
            if !albums.is_empty() {
                y = heading(p, area, y + 8, "Albums");
                shelf(p, area, y, 140, &albums);
            }
        }
        None => {
            y = heading(p, area, y, "Moods & genres");
            let tags = c.tags();
            let cell = (area.width - PAD as u32 * 2 - 10) / 2;
            for (i, t) in tags.iter().enumerate() {
                let r = Rect::new(
                    PAD + (i as u32 % 2 * (cell + 10)) as i32,
                    y + (i as u32 / 2 * 52) as i32,
                    cell,
                    44,
                );
                p.button(
                    r,
                    Color::rgb(41, 41, 41),
                    6,
                    &format!("music:find:{t}"),
                    &cap(t),
                );
                p.box_(Rect::new(r.x, r.y, 6, 44), art::tint(t), 3);
                p.strong(r.x + 18, r.y + 13, cell - 24, &cap(t), 15, INK);
            }
        }
    }
}

fn up_next(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, area: Rect, mut y: i32) {
    let c = &app.catalog;
    p.strong(PAD, y, area.width, "Up next", 26, INK);
    y += 40;
    let Some(player) = &c.player else {
        p.center(0, y + 20, area.width, "Nothing is playing", 14, MUTED);
        return;
    };
    if !player.context_title.is_empty() {
        p.left(
            PAD,
            y,
            area.width,
            &format!("Playing from {}", player.context_title),
            13,
            MUTED,
        );
        y += 26;
    }
    let index = app.live(env.clock_us).map_or(player.index, |l| l.index);
    for (i, id) in player.queue.iter().enumerate().skip(index) {
        let Some(t) = c.track(id) else { continue };
        let r = Rect::new(0, y, area.width, 60);
        p.button(
            r,
            if i == index {
                RAISED
            } else {
                Color::TRANSPARENT
            },
            0,
            &format!("music:jump:{i}"),
            &t.title,
        );
        art::cover(p, Rect::new(PAD, y + 6, 48, 48), &t.album, 4);
        if i == index {
            p.box_(Rect::new(PAD, y + 6, 48, 48), Color(0, 0, 0, 120), 4);
            p.symbol("volume", PAD + 14, y + 20, 20, INK);
        }
        p.left(PAD + 60, y + 10, area.width - 140, &t.title, 15, INK);
        p.left(
            PAD + 60,
            y + 32,
            area.width - 140,
            &c.artist_name(&t.artist),
            13,
            MUTED,
        );
        p.right(
            area.width as i32 - 70,
            y + 20,
            54,
            &clock(t.duration_ms),
            13,
            MUTED,
        );
        y += 60;
    }
}

fn mini_player(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, r: Rect) {
    let (Some(live), Some(t)) = (app.live(env.clock_us), app.now()) else {
        return;
    };
    p.box_(r, BAR, 0);
    p.button(r, Color::TRANSPARENT, 0, "music:expand", "Now playing");
    // The progress line along the top edge.
    let done = (u64::from(r.width) * live.position_ms)
        .checked_div(live.duration_ms)
        .unwrap_or(0) as u32;
    p.box_(Rect::new(r.x, r.y, r.width, 2), Color(255, 255, 255, 50), 0);
    p.box_(Rect::new(r.x, r.y, done.min(r.width), 2), INK, 0);
    art::cover(p, Rect::new(r.x + 12, r.y + 12, 40, 40), &t.album, 4);
    let tw = r.width.saturating_sub(64 + 100);
    p.left(r.x + 64, r.y + 13, tw, &t.title, 15, INK);
    p.left(
        r.x + 64,
        r.y + 34,
        tw,
        &app.catalog.artist_name(&t.artist),
        13,
        MUTED,
    );
    art::icon(
        p,
        Rect::new(r.x + r.width as i32 - 96, r.y + 12, 40, 40),
        if live.playing { "pause" } else { "play" },
        24,
        INK,
        "music:toggle",
        if live.playing { "Pause" } else { "Play" },
    );
    let next = Rect::new(r.x + r.width as i32 - 52, r.y + 12, 40, 40);
    if app.can_next(env.clock_us) {
        art::icon(p, next, "skip-next", 24, INK, "music:next", "Next");
    } else {
        art::icon_off(
            p,
            next,
            "skip-next",
            24,
            INK,
            "Nothing is queued after this song",
        );
    }
}

fn nav_bar(app: &Music, p: &mut Painter, r: Rect) {
    p.box_(r, Color::rgb(15, 15, 15), 0);
    p.hline(r.x, r.y, r.width, Color(255, 255, 255, 20));
    let items = [
        ("home", "Home", "music:home", app.view == View::Home),
        (
            "compass",
            "Explore",
            "music:new",
            matches!(app.view, View::New | View::Radio),
        ),
        (
            "library",
            "Library",
            "music:library:playlists",
            matches!(app.view, View::Library(_)),
        ),
    ];
    let cell = r.width / items.len() as u32;
    for (i, (symbol, label, target, on)) in items.into_iter().enumerate() {
        let x = r.x + (cell * i as u32) as i32;
        let colour = if on { INK } else { MUTED };
        p.button(
            Rect::new(x, r.y, cell, r.height),
            Color::TRANSPARENT,
            0,
            target,
            label,
        );
        p.symbol(symbol, x + (cell as i32 - 24) / 2, r.y + 10, 24, colour);
        p.label(x, r.y + 38, cell, label, 12, colour, on, Align::Center);
    }
}

/// The full-screen player: artwork, title, the like and save pills, the scrubber, and the
/// transport around a white play button; Up next sits below it.
fn now_playing(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, all: Rect) {
    let (Some(live), Some(t)) = (app.live(env.clock_us), app.now()) else {
        return;
    };
    let c = &app.catalog;
    let player = c.player.as_ref();
    let tint = art::tint(&t.album);
    p.gradient(all, art::mix(tint, Color::BLACK, 45), BG, 20);
    art::icon(
        p,
        Rect::new(8, 8, 40, 40),
        "chevron-down",
        24,
        INK,
        "music:back",
        "Close player",
    );
    let size = all.width.saturating_sub(48).min(all.height / 2 - 40);
    let art_r = Rect::new((all.width as i32 - size as i32) / 2, 60, size, size);
    art::cover(p, art_r, &t.album, 8);
    let mut y = art_r.y + size as i32 + 24;
    p.strong(24, y, all.width - 48, &t.title, 22, INK);
    p.left(
        24,
        y + 30,
        all.width - 48,
        &c.artist_name(&t.artist),
        16,
        MUTED,
    );
    y += 66;
    // The action carousel: like, save.
    let liked = c.liked.contains(&t.id);
    let saved = c.library.contains(&t.id);
    let mut x = 24;
    for (symbol, label, target, on) in [
        (
            if liked { "thumb-up-fill" } else { "thumb-up" },
            if liked { "Liked" } else { "Like" },
            format!("music:like:{}", t.id),
            liked,
        ),
        (
            if saved { "check" } else { "plus" },
            if saved { "Saved" } else { "Save" },
            format!("music:save:{}", t.id),
            saved,
        ),
        ("queue", "Up next", "music:queue".to_owned(), false),
    ] {
        let w = p.measure(label, 13, true) + 48;
        let r = Rect::new(x, y, w, 34);
        p.button(
            r,
            if on { Color(255, 255, 255, 60) } else { RAISED },
            17,
            &target,
            label,
        );
        p.symbol(symbol, r.x + 12, r.y + 8, 18, INK);
        p.strong(r.x + 36, r.y + 9, w - 40, label, 13, INK);
        x += w as i32 + 8;
    }
    y += 56;
    art::scrubber(
        p,
        Rect::new(24, y, all.width - 48, 16),
        &live,
        4,
        INK,
        Color(255, 255, 255, 60),
        Some((12, INK)),
    );
    y += 22;
    p.left(24, y, 60, &clock(live.position_ms), 12, MUTED);
    p.right(
        all.width as i32 - 84,
        y,
        60,
        &clock(live.duration_ms),
        12,
        MUTED,
    );
    y += 34;
    let cx = all.width as i32 / 2;
    let shuffle_on = player.is_some_and(|p| p.shuffle);
    let repeat = player.map_or(Repeat::Off, |p| p.repeat);
    art::icon(
        p,
        Rect::new(cx - 160, y + 8, 44, 44),
        "shuffle",
        22,
        if shuffle_on { INK } else { MUTED },
        "music:shuffle",
        "Shuffle",
    );
    art::icon(
        p,
        Rect::new(cx - 100, y + 4, 52, 52),
        "skip-previous",
        30,
        INK,
        "music:previous",
        "Previous",
    );
    let play = Rect::new(cx - 36, y - 6, 72, 72);
    p.button(
        play,
        INK,
        36,
        "music:toggle",
        if live.playing { "Pause" } else { "Play" },
    );
    p.symbol(
        if live.playing { "pause" } else { "play" },
        play.x + 20,
        play.y + 20,
        32,
        Color::BLACK,
    );
    let next = Rect::new(cx + 48, y + 4, 52, 52);
    if app.can_next(env.clock_us) {
        art::icon(p, next, "skip-next", 30, INK, "music:next", "Next");
    } else {
        art::icon_off(
            p,
            next,
            "skip-next",
            30,
            INK,
            "Nothing is queued after this song",
        );
    }
    art::icon(
        p,
        Rect::new(cx + 116, y + 8, 44, 44),
        if repeat == Repeat::One {
            "repeat-one"
        } else {
            "repeat"
        },
        22,
        if repeat == Repeat::Off { MUTED } else { INK },
        "music:repeat",
        "Repeat",
    );
    y += 90;
    // UP NEXT opens the queue; there are no lyrics in this catalogue, so that tab is grey.
    let cell = all.width / 3;
    p.button(
        Rect::new(0, y, cell, 40),
        Color::TRANSPARENT,
        0,
        "music:queue",
        "Up next",
    );
    p.label(0, y + 12, cell, "UP NEXT", 13, INK, true, Align::Center);
    p.label(
        cell as i32,
        y + 12,
        cell,
        "LYRICS",
        13,
        Color(255, 255, 255, 70),
        true,
        Align::Center,
    );
    p.disabled("No lyrics for this song");
    p.button(
        Rect::new(cell as i32 * 2, y, cell, 40),
        Color::TRANSPARENT,
        0,
        &format!("music:artist:{}", t.artist),
        "Related",
    );
    p.label(
        cell as i32 * 2,
        y + 12,
        cell,
        "RELATED",
        13,
        INK,
        true,
        Align::Center,
    );
}
