//! Apple Music, as macOS Sequoia and iOS 18 draw it.
//!
//! macOS: a vibrant sidebar (search field; Home, New, Radio; Library; Playlists) with red
//! symbols, and a toolbar holding shuffle, back, play, forward and repeat either side of the
//! "LCD" — artwork, title, artist and a scrubber — with the Playing Next button at its end.
//! iOS: large titles, a floating mini player above a five-tab bar (Home, New, Radio,
//! Library, Search), and a full-screen Now Playing sheet.
use super::art::{self, Surface};
use super::{clock, summary, Live, Music, Repeat, Shelf, Track, View};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use cw_scene::{Color, Rect};

pub const RED: Color = Color::rgb(250, 45, 72);
const INK: Color = Color::rgb(29, 29, 31);
const GREY: Color = Color::rgb(134, 134, 139);
const HAIR: Color = Color(0, 0, 0, 22);
const SIDEBAR: Color = Color::rgb(238, 238, 240);
const SELECT: Color = Color(0, 0, 0, 20);
const FILL: Color = Color(118, 118, 128, 30);

/// Sizes that differ between the Mac and the phone.
struct M {
    mobile: bool,
    pad: i32,
    title: u16,
    heading: u16,
    body: u16,
    small: u16,
    row: u32,
    card: u32,
}
const MAC: M = M {
    mobile: false,
    pad: 24,
    title: 26,
    heading: 17,
    body: 13,
    small: 11,
    row: 32,
    card: 150,
};
const PHONE: M = M {
    mobile: true,
    pad: 16,
    title: 30,
    heading: 20,
    body: 16,
    small: 13,
    row: 52,
    card: 160,
};

fn surface(m: &M) -> Surface {
    Surface {
        fill: Color(250, 250, 250, 250),
        ink: INK,
        muted: GREY,
        line: HAIR,
        accent: RED,
        radius: if m.mobile { 14 } else { 8 },
        size: if m.mobile { 15 } else { 13 },
    }
}

pub fn mac(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    p.scene.background = Color::WHITE;
    let side: u32 = if w >= 640 { 200 } else { 0 };
    if side > 0 {
        sidebar(app, p, side, h);
    }
    let bar = 56;
    toolbar(app, p, env, side as i32, w - side, bar);
    let area = Rect::new(
        side as i32,
        bar as i32 + 1,
        w - side,
        h.saturating_sub(bar + 1),
    );
    // The view scrolls under the toolbar; each view keeps its own place.
    let pane = p.pane(&app.view_pane(), area);
    body(
        app,
        p,
        env,
        Rect::new(area.x, pane.top(), area.width, area.height),
        &MAC,
    );
    p.end_pane(pane, None);
    overlays(app, p, env, Rect::new(0, 0, w, h), &MAC);
}

pub fn ios(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    p.scene.background = Color::WHITE;
    if app.view == View::NowPlaying && app.catalog.player.is_some() {
        now_playing(app, p, env, Rect::new(0, 0, w, h));
        overlays(app, p, env, Rect::new(0, 0, w, h), &PHONE);
        return;
    }
    let tabs = 58u32;
    let mini = if app.catalog.player.is_some() {
        64u32
    } else {
        0
    };
    let area = Rect::new(0, 0, w, h.saturating_sub(tabs + mini));
    let pane = p.pane(&app.view_pane(), area);
    body(
        app,
        p,
        env,
        Rect::new(area.x, pane.top(), area.width, area.height),
        &PHONE,
    );
    p.end_pane(pane, None);
    let bottom = h.saturating_sub(tabs) as i32;
    if mini > 0 {
        mini_player(
            app,
            p,
            env,
            Rect::new(8, bottom - mini as i32 + 4, w - 16, mini - 10),
        );
    }
    tab_bar(app, p, Rect::new(0, bottom, w, tabs));
    overlays(app, p, env, Rect::new(0, 0, w, h), &PHONE);
}

fn overlays(app: &Music, p: &mut Painter, _env: &crate::AppEnv<'_>, all: Rect, m: &M) {
    let s = surface(m);
    if app.menu.is_some() {
        let (x, y) = if m.mobile {
            (all.x + 24, all.y + all.height as i32 / 3)
        } else {
            (all.x + all.width as i32 / 2 - 60, all.y + 90)
        };
        art::menu(
            p,
            app,
            if m.mobile {
                DesktopTheme::Ios
            } else {
                DesktopTheme::Macos
            },
            x,
            y,
            all,
            &s,
        );
    }
    if let Some(draft) = &app.draft {
        art::composer(p, draft, all, &s);
    }
}

fn sidebar(app: &Music, p: &mut Painter, side: u32, h: u32) {
    p.box_(Rect::new(0, 0, side, h), SIDEBAR, 0);
    p.vline(side as i32, 0, h, HAIR);
    // The search field takes you to Search and focuses it.
    let field = Rect::new(10, 10, side - 20, 26);
    p.button(field, FILL, 7, "music:search-field", "Search");
    p.symbol("search", field.x + 7, field.y + 6, 14, GREY);
    let focused = app.focus == super::Focus::Search;
    p.left(
        field.x + 26,
        field.y + 5,
        field.width - 32,
        if app.query.is_empty() {
            "Search"
        } else {
            &app.query
        },
        13,
        if app.query.is_empty() { GREY } else { INK },
    );
    if focused {
        p.border(
            field,
            Color::TRANSPARENT,
            7,
            Color(RED.0, RED.1, RED.2, 140),
        );
    }
    let mut y = 46;
    let item = |p: &mut Painter, y: &mut i32, symbol: &str, label: &str, target: &str, on: bool| {
        if *y + 26 > h as i32 {
            return;
        }
        let r = Rect::new(8, *y, side - 16, 26);
        p.button(
            r,
            if on { SELECT } else { Color::TRANSPARENT },
            6,
            target,
            label,
        );
        p.symbol(symbol, 16, *y + 5, 16, RED);
        p.left(40, *y + 5, side - 52, label, 13, INK);
        *y += 28;
    };
    let head = |p: &mut Painter, y: &mut i32, label: &str| {
        *y += 8;
        if *y + 18 < h as i32 {
            p.strong(16, *y, side - 32, label, 11, GREY);
        }
        *y += 20;
    };
    item(
        p,
        &mut y,
        "home",
        "Home",
        "music:home",
        app.view == View::Home,
    );
    item(p, &mut y, "grid", "New", "music:new", app.view == View::New);
    item(
        p,
        &mut y,
        "radio",
        "Radio",
        "music:radio",
        app.view == View::Radio,
    );
    head(p, &mut y, "Library");
    for (shelf, symbol) in [
        (Shelf::Recent, "clock"),
        (Shelf::Artists, "person"),
        (Shelf::Albums, "grid-view"),
        (Shelf::Songs, "music"),
    ] {
        item(
            p,
            &mut y,
            symbol,
            shelf.title(),
            &format!("music:library:{}", shelf.key()),
            app.view == View::Library(shelf),
        );
    }
    head(p, &mut y, "Playlists");
    item(
        p,
        &mut y,
        "grid",
        "All Playlists",
        "music:library:playlists",
        app.view == View::Library(Shelf::Playlists),
    );
    item(
        p,
        &mut y,
        "star",
        "Favorite Songs",
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
    if y + 26 <= h as i32 {
        let r = Rect::new(8, y + 4, side - 16, 26);
        p.button(r, Color::TRANSPARENT, 6, "music:compose", "New Playlist");
        p.symbol("plus", 16, y + 9, 16, GREY);
        p.left(40, y + 9, side - 52, "New Playlist", 13, GREY);
    }
}

/// The Mac toolbar: transport, the LCD and Playing Next.
fn toolbar(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, x: i32, w: u32, bar: u32) {
    p.box_(Rect::new(x, 0, w, bar), Color::rgb(250, 250, 250), 0);
    p.hline(x, bar as i32, w, HAIR);
    let live = app.live(env.clock_us);
    let player = app.catalog.player.as_ref();
    let cy = bar as i32 / 2;
    let mut cx = x + 14;
    let loaded = live.is_some();
    let buttons: [(&str, &str, u32, &str, bool); 5] = [
        (
            "shuffle",
            "music:shuffle",
            15,
            "Shuffle",
            player.is_some_and(|p| p.shuffle),
        ),
        ("backward", "music:previous", 20, "Previous", false),
        (
            if live.as_ref().is_some_and(|l| l.playing) {
                "pause"
            } else {
                "play"
            },
            "music:toggle",
            22,
            "Play",
            false,
        ),
        ("forward", "music:next", 20, "Next", false),
        (
            if player.is_some_and(|p| p.repeat == Repeat::One) {
                "repeat-one"
            } else {
                "repeat"
            },
            "music:repeat",
            15,
            "Repeat",
            player.is_some_and(|p| p.repeat != Repeat::Off),
        ),
    ];
    for (symbol, target, size, label, on) in buttons {
        let r = Rect::new(cx, cy - 15, 30, 30);
        let colour = if on { RED } else { Color::rgb(60, 60, 64) };
        if !loaded {
            art::icon_off(p, r, symbol, size, colour, "Nothing is playing");
        } else if target == "music:next" && !app.can_next(env.clock_us) {
            art::icon_off(
                p,
                r,
                symbol,
                size,
                colour,
                "Nothing is queued after this song",
            );
        } else {
            art::icon(p, r, symbol, size, colour, target, label);
        }
        cx += 34;
    }
    // The LCD.
    let queue_w = 40;
    let lcd_w = (w as i32 - (cx - x) - queue_w - 36).clamp(160, 520) as u32;
    let lcd = Rect::new(
        (cx + 12).max(x + (w as i32 - lcd_w as i32) / 2),
        6,
        lcd_w,
        bar - 12,
    );
    p.border(lcd, Color::rgb(244, 244, 245), 6, HAIR);
    match (&live, app.now()) {
        (Some(live), Some(track)) => {
            let art_r = Rect::new(lcd.x, lcd.y, lcd.height, lcd.height);
            art::cover(p, art_r, &track.album, 5);
            let tx = art_r.x + art_r.width as i32 + 8;
            let tw = lcd.width.saturating_sub(art_r.width + 16);
            p.label(
                tx,
                lcd.y + 3,
                tw,
                &track.title,
                12,
                INK,
                true,
                Align::Center,
            );
            let album = app
                .catalog
                .album(&track.album)
                .map(|a| a.title.clone())
                .unwrap_or_default();
            p.label(
                tx,
                lcd.y + 18,
                tw,
                &format!("{} — {}", app.catalog.artist_name(&track.artist), album),
                11,
                GREY,
                false,
                Align::Center,
            );
            p.left(tx, lcd.y + 29, 40, &clock(live.position_ms), 9, GREY);
            p.right(
                tx + tw as i32 - 40,
                lcd.y + 29,
                40,
                &format!(
                    "-{}",
                    clock(live.duration_ms.saturating_sub(live.position_ms))
                ),
                9,
                GREY,
            );
            art::scrubber(
                p,
                Rect::new(
                    tx + 34,
                    lcd.y + lcd.height as i32 - 10,
                    tw.saturating_sub(68),
                    10,
                ),
                live,
                3,
                Color::rgb(90, 90, 94),
                Color(0, 0, 0, 30),
                None,
            );
        }
        _ => {
            p.symbol(
                "fruit",
                lcd.x + (lcd.width as i32 - 18) / 2,
                lcd.y + (lcd.height as i32 - 18) / 2,
                18,
                Color::rgb(170, 170, 174),
            );
        }
    }
    let q = Rect::new(x + w as i32 - queue_w - 8, cy - 15, 30, 30);
    let on = app.view == View::Queue;
    if player.is_some() {
        if on {
            p.box_(q, SELECT, 6);
        }
        art::icon(
            p,
            q,
            "queue",
            16,
            if on { RED } else { Color::rgb(60, 60, 64) },
            "music:queue",
            "Playing Next",
        );
    } else {
        art::icon_off(
            p,
            q,
            "queue",
            16,
            Color::rgb(60, 60, 64),
            "Nothing is playing",
        );
    }
}

/// The phone's floating mini player: art, title, play/pause and forward.
fn mini_player(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, r: Rect) {
    let (Some(live), Some(track)) = (app.live(env.clock_us), app.now()) else {
        return;
    };
    p.drop_shadow(r, 14, 14, 40, 4);
    p.glass(r, 14, 24, Color(250, 250, 250, 235), Some(HAIR));
    // The card opens Now Playing; its two buttons sit above it and win their own taps.
    p.button(r, Color::TRANSPARENT, 14, "music:expand", "Now Playing");
    let art_r = Rect::new(r.x + 8, r.y + 8, r.height - 16, r.height - 16);
    art::cover(p, art_r, &track.album, 6);
    p.left(
        art_r.x + art_r.width as i32 + 10,
        r.y + (r.height as i32 - 20) / 2,
        r.width.saturating_sub(art_r.width + 110),
        &track.title,
        15,
        INK,
    );
    let play = Rect::new(
        r.x + r.width as i32 - 84,
        r.y + (r.height as i32 - 36) / 2,
        36,
        36,
    );
    art::icon(
        p,
        play,
        if live.playing { "pause" } else { "play" },
        22,
        INK,
        "music:toggle",
        if live.playing { "Pause" } else { "Play" },
    );
    let next = Rect::new(r.x + r.width as i32 - 44, play.y, 36, 36);
    if app.can_next(env.clock_us) {
        art::icon(p, next, "forward", 24, INK, "music:next", "Next");
    } else {
        art::icon_off(
            p,
            next,
            "forward",
            24,
            INK,
            "Nothing is queued after this song",
        );
    }
}

fn tab_bar(app: &Music, p: &mut Painter, r: Rect) {
    p.glass(r, 0, 20, Color(249, 249, 249, 240), None);
    p.hline(r.x, r.y, r.width, HAIR);
    let tabs = [
        ("home", "Home", "music:home", app.view == View::Home),
        ("grid", "New", "music:new", app.view == View::New),
        ("radio", "Radio", "music:radio", app.view == View::Radio),
        (
            "library",
            "Library",
            "music:library",
            matches!(app.view, View::Library(_)),
        ),
        (
            "search",
            "Search",
            "music:search-field",
            app.view == View::Search,
        ),
    ];
    let cell = r.width / tabs.len() as u32;
    for (i, (symbol, label, target, on)) in tabs.into_iter().enumerate() {
        let x = r.x + (cell * i as u32) as i32;
        let colour = if on { RED } else { Color::rgb(153, 153, 153) };
        p.button(
            Rect::new(x, r.y, cell, r.height),
            Color::TRANSPARENT,
            0,
            target,
            label,
        );
        p.symbol(symbol, x + (cell as i32 - 24) / 2, r.y + 7, 24, colour);
        p.label(x, r.y + 33, cell, label, 10, colour, true, Align::Center);
    }
}

/// Large title with, on the phone, the back chevron above it when there is somewhere to go.
fn title(app: &Music, p: &mut Painter, area: Rect, m: &M, text: &str) -> i32 {
    let mut y = area.y + if m.mobile { 8 } else { 18 };
    if m.mobile && !app.back.is_empty() {
        p.button(
            Rect::new(area.x + 4, y, 90, 30),
            Color::TRANSPARENT,
            8,
            "music:back",
            "Back",
        );
        p.symbol("chevron-left", area.x + 8, y + 4, 22, RED);
        p.left(area.x + 30, y + 5, 60, "Back", 17, RED);
        y += 34;
    } else if !m.mobile && !app.back.is_empty() {
        art::icon(
            p,
            Rect::new(area.x + m.pad - 8, y + 2, 28, 28),
            "chevron-left",
            18,
            GREY,
            "music:back",
            "Back",
        );
        p.strong(
            area.x + m.pad + 24,
            y,
            area.width.saturating_sub(m.pad as u32 * 2 + 24),
            text,
            m.title,
            INK,
        );
        return y + i32::from(m.title) + 20;
    }
    if !text.is_empty() {
        p.strong(
            area.x + m.pad,
            y,
            area.width.saturating_sub(m.pad as u32 * 2),
            text,
            m.title,
            INK,
        );
        y += i32::from(m.title) + 18;
    }
    if let Some(notice) = app.status.notice() {
        p.left(
            area.x + m.pad,
            y - 10,
            area.width.saturating_sub(m.pad as u32 * 2),
            notice,
            m.small,
            GREY,
        );
        y += 12;
    }
    y
}

fn heading(p: &mut Painter, area: Rect, m: &M, y: i32, text: &str) -> i32 {
    p.strong(
        area.x + m.pad,
        y,
        area.width.saturating_sub(m.pad as u32 * 2),
        text,
        m.heading,
        INK,
    );
    y + i32::from(m.heading) + 12
}

/// A card: artwork over a title and a grey line. Returns the card's height.
#[allow(clippy::too_many_arguments)]
fn card(
    p: &mut Painter,
    x: i32,
    y: i32,
    size: u32,
    key: &str,
    title: &str,
    sub: &str,
    target: &str,
    round: bool,
    m: &M,
) -> u32 {
    let r = Rect::new(x, y, size, size);
    p.button(
        Rect::new(x, y, size, size + 40),
        Color::TRANSPARENT,
        8,
        target,
        title,
    );
    if round {
        art::avatar(p, r, key, title);
    } else {
        art::cover(p, r, key, if m.mobile { 8 } else { 6 });
    }
    let align = if round { Align::Center } else { Align::Left };
    p.label(
        x,
        y + size as i32 + 6,
        size,
        title,
        m.small + 1,
        INK,
        false,
        align,
    );
    p.label(
        x,
        y + size as i32 + 23,
        size,
        sub,
        m.small,
        GREY,
        false,
        align,
    );
    size + 46
}

/// A shelf of cards, as many as fit across; on the phone it scrolls sideways, so the next
/// card peeks in at the edge.
#[allow(clippy::type_complexity)]
fn shelf(
    p: &mut Painter,
    area: Rect,
    m: &M,
    y: i32,
    cards: &[(String, String, String, String, bool)],
) -> i32 {
    if cards.is_empty() {
        return y;
    }
    let gap = if m.mobile { 12 } else { 20 };
    let inner = area.width.saturating_sub(m.pad as u32 * 2);
    let per = ((inner + gap) / (m.card + gap)).max(if m.mobile { 2 } else { 1 });
    let size = if m.mobile {
        m.card
    } else {
        (inner - gap * (per - 1)) / per
    };
    let mut x = area.x + m.pad;
    let mut height = 0;
    for (key, title, sub, target, round) in cards.iter().take(per as usize + usize::from(m.mobile))
    {
        height = card(p, x, y, size, key, title, sub, target, *round, m);
        x += (size + gap) as i32;
    }
    y + height as i32 + 14
}
/// Rows of cards wrapped into a grid, until the area runs out.
#[allow(clippy::type_complexity)]
fn grid(
    p: &mut Painter,
    area: Rect,
    m: &M,
    mut y: i32,
    cards: &[(String, String, String, String, bool)],
) -> i32 {
    let gap = if m.mobile { 12 } else { 20 };
    let inner = area.width.saturating_sub(m.pad as u32 * 2);
    let per = ((inner + gap) / (m.card + gap)).max(2);
    let size = (inner - gap * (per - 1)) / per;
    for chunk in cards.chunks(per as usize) {
        let mut x = area.x + m.pad;
        for (key, title, sub, target, round) in chunk {
            card(p, x, y, size, key, title, sub, target, *round, m);
            x += (size + gap) as i32;
        }
        y += (size + 46 + 14) as i32;
    }
    y
}

fn album_card(app: &Music, a: &super::Album) -> (String, String, String, String, bool) {
    (
        a.id.clone(),
        a.title.clone(),
        app.catalog.artist_name(&a.artist),
        format!("music:album:{}", a.id),
        false,
    )
}

/// Track rows: number (or a red speaker on the one playing), title, artist, time and the
/// "…" menu. The row itself plays the song within `context`.
#[allow(clippy::too_many_arguments)]
fn songs(
    app: &Music,
    p: &mut Painter,
    env: &crate::AppEnv<'_>,
    area: Rect,
    m: &M,
    mut y: i32,
    tracks: &[&Track],
    context: &str,
    numbered: bool,
    artwork: bool,
) -> i32 {
    let live = app.live(env.clock_us);
    let x = area.x + m.pad;
    let width = area.width.saturating_sub(m.pad as u32 * 2);
    for (i, t) in tracks.iter().enumerate() {
        let r = Rect::new(x, y, width, m.row);
        let current = live.as_ref().is_some_and(|l| l.id == t.id);
        let zebra = !m.mobile && i % 2 == 1;
        art::row(
            p,
            r,
            if zebra {
                Color(0, 0, 0, 7)
            } else {
                Color::TRANSPARENT
            },
            6,
            &format!("music:play:{context}@{}", t.id),
            &t.title,
        );
        if m.mobile && i > 0 {
            p.hline(
                x + if artwork { 60 } else { 28 },
                y,
                width.saturating_sub(28),
                HAIR,
            );
        }
        let mut tx = x + 8;
        if artwork {
            let s = m.row - 10;
            art::cover(p, Rect::new(tx, y + 5, s, s), &t.album, 4);
            tx += s as i32 + 12;
        } else if numbered {
            if current {
                p.symbol("volume", tx, y + (m.row as i32 - 14) / 2, 14, RED);
            } else {
                p.left(
                    tx,
                    y + (m.row as i32 - i32::from(m.body) * 3 / 2) / 2,
                    24,
                    &(i + 1).to_string(),
                    m.body,
                    GREY,
                );
            }
            tx += 30;
        }
        let right = 110;
        let tw = (x + width as i32 - right - tx).max(40) as u32;
        let artist = app.catalog.artist_name(&t.artist);
        let colour = if current { RED } else { INK };
        if m.mobile && artwork {
            p.left(tx, y + 8, tw, &t.title, m.body, colour);
            p.left(tx, y + 28, tw, &artist, m.small, GREY);
        } else if artwork || !numbered {
            p.left(
                tx,
                y + (m.row as i32 - i32::from(m.body) * 3 / 2) / 2,
                tw / 2,
                &t.title,
                m.body,
                colour,
            );
            p.left(
                tx + tw as i32 / 2 + 8,
                y + (m.row as i32 - i32::from(m.body) * 3 / 2) / 2,
                tw / 2 - 8,
                &artist,
                m.body,
                GREY,
            );
        } else {
            p.left(
                tx,
                y + (m.row as i32 - i32::from(m.body) * 3 / 2) / 2,
                tw,
                &t.title,
                m.body,
                colour,
            );
        }
        if app.catalog.liked.contains(&t.id) {
            p.symbol(
                "star",
                x + width as i32 - right + 6,
                y + (m.row as i32 - 12) / 2,
                12,
                RED,
            );
        }
        if !m.mobile {
            p.right(
                x + width as i32 - 88,
                y + (m.row as i32 - i32::from(m.body) * 3 / 2) / 2,
                50,
                &clock(t.duration_ms),
                m.body,
                GREY,
            );
        }
        art::icon(
            p,
            Rect::new(x + width as i32 - 34, y + (m.row as i32 - 28) / 2, 28, 28),
            "more",
            16,
            if m.mobile { INK } else { RED },
            &format!("music:menu:{}", t.id),
            &format!("More for {}", t.title),
        );
        y += m.row as i32;
    }
    y
}

/// Apple Music's pair of buttons under a collection: Play and Shuffle.
fn play_shuffle(p: &mut Painter, x: i32, y: i32, width: u32, context: &str, m: &M) -> i32 {
    let w = if m.mobile { (width - 12) / 2 } else { 110 };
    let h = if m.mobile { 44 } else { 28 };
    let (fill, ink) = if m.mobile {
        (FILL, RED)
    } else {
        (RED, Color::WHITE)
    };
    for (i, (symbol, label, target)) in [
        ("play", "Play", format!("music:play:{context}")),
        (
            "shuffle",
            "Shuffle",
            format!("music:shuffle-play:{context}"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let r = Rect::new(x + (i as u32 * (w + 12)) as i32, y, w, h);
        p.button(r, fill, if m.mobile { 10 } else { 6 }, &target, label);
        let tw = p.measure(label, m.body, true) + 20;
        let sx = r.x + (w as i32 - tw as i32) / 2;
        p.symbol(symbol, sx, r.y + (h as i32 - 14) / 2, 14, ink);
        p.strong(
            sx + 20,
            r.y + (h as i32 - i32::from(m.body) * 3 / 2) / 2,
            tw,
            label,
            m.body,
            ink,
        );
    }
    y + h as i32 + 16
}

fn body(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, area: Rect, m: &M) {
    if !app.loaded {
        let text = app.status.notice().unwrap_or("Loading…");
        p.center(
            area.x,
            area.y + area.height as i32 / 3,
            area.width,
            text,
            14,
            GREY,
        );
        return;
    }
    let c = &app.catalog;
    match &app.view {
        View::Home => {
            let mut y = title(app, p, area, m, "Home");
            let mut picks: Vec<_> = vec![(
                "favorites".to_owned(),
                "Favorite Songs".to_owned(),
                "Apple Music".to_owned(),
                "music:liked".to_owned(),
                false,
            )];
            picks.extend(c.playlists.iter().map(|l| {
                (
                    l.id.clone(),
                    l.title.clone(),
                    format!("Playlist · {}", owner(&l.owner)),
                    format!("music:playlist:{}", l.id),
                    false,
                )
            }));
            y = heading(p, area, m, y, "Top Picks for You");
            y = shelf(p, area, m, y, &picks);
            let mut seen = vec![];
            let recent: Vec<_> = c
                .history
                .iter()
                .filter_map(|id| c.track(id))
                .filter_map(|t| c.album(&t.album))
                .filter(|a| {
                    let fresh = !seen.contains(&a.id);
                    seen.push(a.id.clone());
                    fresh
                })
                .map(|a| album_card(app, a))
                .collect();
            if !recent.is_empty() {
                y = heading(p, area, m, y, "Recently Played");
                y = shelf(p, area, m, y, &recent);
            }
            let stations: Vec<_> = c
                .artists
                .iter()
                .map(|a| {
                    (
                        a.id.clone(),
                        format!("{} Station", a.name),
                        "Apple Music".to_owned(),
                        format!("music:play:station:{}", a.id),
                        false,
                    )
                })
                .collect();
            y = heading(p, area, m, y, "Stations for You");
            shelf(p, area, m, y, &stations);
        }
        View::New => {
            let mut y = title(app, p, area, m, "New");
            y = heading(p, area, m, y, "New Releases");
            let albums: Vec<_> = c.albums.iter().map(|a| album_card(app, a)).collect();
            y = shelf(p, area, m, y, &albums);
            y = heading(p, area, m, y, "Latest Songs");
            let latest: Vec<&Track> = c.tracks.iter().take(12).collect();
            songs(app, p, env, area, m, y, &latest, "track", false, true);
        }
        View::Radio => {
            let mut y = title(app, p, area, m, "Radio");
            y = heading(p, area, m, y, "Stations");
            let stations: Vec<_> = c
                .artists
                .iter()
                .map(|a| {
                    (
                        format!("station-{}", a.id),
                        format!("{} Station", a.name),
                        "Based on your listening".to_owned(),
                        format!("music:play:station:{}", a.id),
                        false,
                    )
                })
                .collect();
            grid(p, area, m, y, &stations);
        }
        View::Library(shelf_) => library(app, p, env, area, m, *shelf_),
        View::Album(id) => {
            let Some(a) = c.album(id) else { return };
            let tracks: Vec<&Track> = a.tracks.iter().filter_map(|t| c.track(t)).collect();
            let context = format!("album:{id}");
            let saved = a.tracks.iter().all(|t| c.library.contains(t));
            let y = collection(
                p,
                app,
                area,
                m,
                &a.id,
                &a.title,
                &c.artist_name(&a.artist),
                Some(&format!("music:artist:{}", a.artist)),
                &format!("{} · {}", a.genre.to_uppercase(), a.year),
                &context,
            );
            // Add to Library, or the tick that says it is there.
            let add = Rect::new(
                area.x + area.width as i32 - m.pad - 34,
                if m.mobile { area.y + 8 } else { area.y + 24 },
                30,
                30,
            );
            art::icon(
                p,
                add,
                if saved { "check" } else { "plus" },
                18,
                RED,
                &format!("music:save-album:{id}"),
                if saved {
                    "Remove from Library"
                } else {
                    "Add to Library"
                },
            );
            let y = songs(app, p, env, area, m, y, &tracks, &context, true, false);
            p.left(
                area.x + m.pad,
                y + 12,
                area.width,
                &format!("{} · {}", a.year, summary(&tracks)),
                m.small,
                GREY,
            );
        }
        View::Playlist(id) => {
            let Some(l) = c.playlist(id) else { return };
            let tracks: Vec<&Track> = l.items.iter().filter_map(|t| c.track(t)).collect();
            let context = format!("playlist:{id}");
            let y = collection(
                p,
                app,
                area,
                m,
                &l.id,
                &l.title,
                &owner(&l.owner),
                None,
                &summary(&tracks),
                &context,
            );
            if tracks.is_empty() {
                p.left(
                    area.x + m.pad,
                    y,
                    area.width,
                    "Add songs with “Add to Playlist” in any song's menu.",
                    m.body,
                    GREY,
                );
            }
            songs(app, p, env, area, m, y, &tracks, &context, false, true);
        }
        View::Liked => {
            let tracks = c.liked_songs();
            let y = collection(
                p,
                app,
                area,
                m,
                "favorites",
                "Favorite Songs",
                "Apple Music",
                None,
                &summary(&tracks),
                "liked",
            );
            if tracks.is_empty() {
                p.left(
                    area.x + m.pad,
                    y,
                    area.width,
                    "Favorite a song and it appears here.",
                    m.body,
                    GREY,
                );
            }
            songs(app, p, env, area, m, y, &tracks, "liked", false, true);
        }
        View::Artist(id) => {
            let Some(a) = c.artist(id) else { return };
            let mut y = title(app, p, area, m, "");
            let banner = Rect::new(area.x, y - 8, area.width, if m.mobile { 220 } else { 150 });
            let tint = art::tint(id);
            p.gradient(
                banner,
                art::mix(tint, Color::WHITE, 10),
                art::mix(tint, Color::BLACK, 45),
                16,
            );
            p.strong(
                area.x + m.pad,
                banner.y + banner.height as i32 - 50,
                area.width.saturating_sub(120),
                &a.name,
                if m.mobile { 30 } else { 34 },
                Color::WHITE,
            );
            let play = Rect::new(
                area.x + area.width as i32 - m.pad - 44,
                banner.y + banner.height as i32 - 54,
                44,
                44,
            );
            p.button(
                play,
                RED,
                22,
                &format!("music:play:artist:{id}"),
                &format!("Play {}", a.name),
            );
            p.symbol("play", play.x + 13, play.y + 12, 20, Color::WHITE);
            y = banner.y + banner.height as i32 + 16;
            y = heading(p, area, m, y, "Top Songs");
            let top: Vec<&Track> = c.top_songs(id).into_iter().take(4).collect();
            y = songs(
                app,
                p,
                env,
                area,
                m,
                y,
                &top,
                &format!("artist:{id}"),
                false,
                true,
            ) + 12;
            let albums: Vec<_> = c
                .albums
                .iter()
                .filter(|al| al.artist == *id)
                .map(|al| {
                    (
                        al.id.clone(),
                        al.title.clone(),
                        al.year.clone(),
                        format!("music:album:{}", al.id),
                        false,
                    )
                })
                .collect();
            y = heading(p, area, m, y, "Albums");
            shelf(p, area, m, y, &albums);
        }
        View::Search => search(app, p, env, area, m),
        View::Queue | View::NowPlaying => queue(app, p, env, area, m),
    }
}

/// The header every album and playlist page opens with. Returns where the songs start.
#[allow(clippy::too_many_arguments)]
fn collection(
    p: &mut Painter,
    app: &Music,
    area: Rect,
    m: &M,
    key: &str,
    name: &str,
    by: &str,
    by_target: Option<&str>,
    meta: &str,
    context: &str,
) -> i32 {
    let y = title(app, p, area, m, "");
    let width = area.width.saturating_sub(m.pad as u32 * 2);
    if m.mobile {
        let size = (area.width * 3 / 5).min(260);
        let x = area.x + (area.width as i32 - size as i32) / 2;
        art::cover(p, Rect::new(x, y, size, size), key, 10);
        let mut ty = y + size as i32 + 14;
        p.label(
            area.x + m.pad,
            ty,
            width,
            name,
            22,
            INK,
            true,
            Align::Center,
        );
        ty += 30;
        if let Some(target) = by_target {
            let tw = p.measure(by, 20, false).min(width);
            p.button(
                Rect::new(area.x + (area.width as i32 - tw as i32) / 2, ty, tw, 26),
                Color::TRANSPARENT,
                6,
                target,
                by,
            );
        }
        p.label(area.x + m.pad, ty, width, by, 20, RED, false, Align::Center);
        ty += 28;
        p.label(
            area.x + m.pad,
            ty,
            width,
            meta,
            m.small,
            GREY,
            false,
            Align::Center,
        );
        ty += 26;
        play_shuffle(p, area.x + m.pad, ty, width, context, m)
    } else {
        let size = 180u32.min(area.height / 2);
        let x = area.x + m.pad;
        art::cover(p, Rect::new(x, y, size, size), key, 8);
        let tx = x + size as i32 + 24;
        let tw = width.saturating_sub(size + 24 + 44);
        let mut ty = y + size as i32 - 150;
        p.strong(tx, ty.max(y), tw, name, 26, INK);
        ty = ty.max(y) + 34;
        if let Some(target) = by_target {
            let bw = p.measure(by, 20, false).min(tw);
            p.button(Rect::new(tx, ty, bw, 26), Color::TRANSPARENT, 4, target, by);
        }
        p.left(tx, ty, tw, by, 20, RED);
        ty += 30;
        p.strong(tx, ty, tw, meta, m.small, GREY);
        play_shuffle(p, tx, y + size as i32 - 30, tw, context, m);
        y + size as i32 + 20
    }
}

fn owner(owner: &str) -> String {
    let mut c = owner.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Apple Music".into(),
    }
}

fn library(
    app: &Music,
    p: &mut Painter,
    env: &crate::AppEnv<'_>,
    area: Rect,
    m: &M,
    shelf_: Shelf,
) {
    let c = &app.catalog;
    if m.mobile && shelf_ == Shelf::Recent {
        let mut y = title(app, p, area, m, "Library");
        for (shelf, symbol) in [
            (Shelf::Playlists, "list-view"),
            (Shelf::Artists, "person"),
            (Shelf::Albums, "grid-view"),
            (Shelf::Songs, "music"),
        ] {
            let r = Rect::new(area.x, y, area.width, 46);
            p.button(
                r,
                Color::TRANSPARENT,
                0,
                &format!("music:library:{}", shelf.key()),
                shelf.title(),
            );
            p.symbol(symbol, area.x + m.pad, y + 12, 22, RED);
            p.left(
                area.x + m.pad + 36,
                y + 12,
                area.width,
                shelf.title(),
                20,
                INK,
            );
            p.symbol(
                "chevron-right",
                area.x + area.width as i32 - m.pad - 16,
                y + 15,
                16,
                Color::rgb(196, 196, 199),
            );
            p.hline(area.x + m.pad + 36, y + 45, area.width, HAIR);
            y += 46;
        }
        y += 16;
        y = heading(p, area, m, y, "Recently Added");
        let cards: Vec<_> = c
            .recently_added()
            .into_iter()
            .map(|a| album_card(app, a))
            .collect();
        grid(p, area, m, y, &cards);
        return;
    }
    let y = title(app, p, area, m, shelf_.title());
    match shelf_ {
        Shelf::Recent => {
            let cards: Vec<_> = c
                .recently_added()
                .into_iter()
                .map(|a| album_card(app, a))
                .collect();
            empty_or(
                p,
                area,
                m,
                y,
                cards.is_empty(),
                "Songs you add to your library appear here.",
            );
            grid(p, area, m, y, &cards);
        }
        Shelf::Albums => {
            let cards: Vec<_> = c
                .library_albums()
                .into_iter()
                .map(|a| album_card(app, a))
                .collect();
            empty_or(
                p,
                area,
                m,
                y,
                cards.is_empty(),
                "Albums you add to your library appear here.",
            );
            grid(p, area, m, y, &cards);
        }
        Shelf::Artists => {
            let artists = c.library_artists();
            empty_or(
                p,
                area,
                m,
                y,
                artists.is_empty(),
                "Artists in your library appear here.",
            );
            let mut y = y;
            for a in artists {
                let r = Rect::new(
                    area.x + m.pad,
                    y,
                    area.width.saturating_sub(m.pad as u32 * 2),
                    52,
                );
                p.button(
                    r,
                    Color::TRANSPARENT,
                    6,
                    &format!("music:artist:{}", a.id),
                    &a.name,
                );
                art::avatar(p, Rect::new(r.x + 4, y + 6, 40, 40), &a.id, &a.name);
                p.left(
                    r.x + 56,
                    y + 16,
                    r.width.saturating_sub(60),
                    &a.name,
                    m.body,
                    INK,
                );
                p.hline(r.x + 56, y + 51, r.width.saturating_sub(56), HAIR);
                y += 52;
            }
        }
        Shelf::Songs => {
            let songs_ = c.library_songs();
            empty_or(
                p,
                area,
                m,
                y,
                songs_.is_empty(),
                "Songs you add to your library appear here.",
            );
            let y = if songs_.is_empty() {
                y
            } else {
                play_shuffle(
                    p,
                    area.x + m.pad,
                    y,
                    area.width.saturating_sub(m.pad as u32 * 2),
                    "library",
                    m,
                )
            };
            songs(app, p, env, area, m, y, &songs_, "library", false, true);
        }
        Shelf::Playlists => {
            let mut cards = vec![(
                "favorites".to_owned(),
                "Favorite Songs".to_owned(),
                "Apple Music".to_owned(),
                "music:liked".to_owned(),
                false,
            )];
            cards.extend(c.playlists.iter().map(|l| {
                (
                    l.id.clone(),
                    l.title.clone(),
                    owner(&l.owner),
                    format!("music:playlist:{}", l.id),
                    false,
                )
            }));
            let r = Rect::new(
                area.x + area.width as i32 - m.pad - 130,
                area.y + if m.mobile { 44 } else { 20 },
                130,
                28,
            );
            p.button(r, Color::TRANSPARENT, 6, "music:compose", "New Playlist");
            p.symbol("plus", r.x + 6, r.y + 6, 16, RED);
            p.left(r.x + 26, r.y + 6, 104, "New Playlist", m.body, RED);
            grid(p, area, m, y, &cards);
        }
    }
}

fn empty_or(p: &mut Painter, area: Rect, m: &M, y: i32, empty: bool, text: &str) {
    if empty {
        p.left(
            area.x + m.pad,
            y,
            area.width.saturating_sub(m.pad as u32 * 2),
            text,
            m.body,
            GREY,
        );
    }
}

fn search(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, area: Rect, m: &M) {
    let c = &app.catalog;
    let mut y = title(app, p, area, m, "Search");
    // The field: on the phone it heads the page; on the Mac it is the sidebar's, shown here
    // too when the sidebar is hidden.
    if m.mobile || area.x == 0 {
        let field = Rect::new(
            area.x + m.pad,
            y,
            area.width.saturating_sub(m.pad as u32 * 2 + 60),
            36,
        );
        p.button(field, FILL, 10, "music:search-field", "Search");
        p.symbol("search", field.x + 10, field.y + 10, 16, GREY);
        p.left(
            field.x + 34,
            field.y + 9,
            field.width.saturating_sub(40),
            if app.query.is_empty() {
                "Artists, Songs, Lyrics, and More"
            } else {
                &app.query
            },
            15,
            if app.query.is_empty() { GREY } else { INK },
        );
        let go = Rect::new(field.x + field.width as i32 + 6, y, 54, 36);
        if app.query.trim().is_empty() {
            p.left(
                go.x + 4,
                go.y + 9,
                50,
                "Search",
                15,
                Color(RED.0, RED.1, RED.2, 90),
            );
            p.disabled("Type something to search for");
        } else {
            p.button(go, Color::TRANSPARENT, 8, "music:search", "Search");
            p.left(go.x + 4, go.y + 9, 50, "Search", 15, RED);
        }
        y += 50;
    }
    match &app.results {
        Some(r) if r.query == app.query.trim() || !app.query.is_empty() => {
            if r.is_empty() {
                p.left(
                    area.x + m.pad,
                    y,
                    area.width,
                    &format!("No results for “{}”", r.query),
                    m.body,
                    GREY,
                );
                return;
            }
            if !r.tracks.is_empty() {
                y = heading(p, area, m, y, "Songs");
                let tracks: Vec<&Track> =
                    r.tracks.iter().filter_map(|t| c.track(t)).take(5).collect();
                y = songs(app, p, env, area, m, y, &tracks, "track", false, true) + 12;
            }
            let mut cards: Vec<_> = r
                .artists
                .iter()
                .filter_map(|a| c.artist(a))
                .map(|a| {
                    (
                        a.id.clone(),
                        a.name.clone(),
                        "Artist".to_owned(),
                        format!("music:artist:{}", a.id),
                        true,
                    )
                })
                .collect();
            cards.extend(r.albums.iter().filter_map(|a| c.album(a)).map(|a| {
                (
                    a.id.clone(),
                    a.title.clone(),
                    format!("Album · {}", c.artist_name(&a.artist)),
                    format!("music:album:{}", a.id),
                    false,
                )
            }));
            cards.extend(r.playlists.iter().filter_map(|l| c.playlist(l)).map(|l| {
                (
                    l.id.clone(),
                    l.title.clone(),
                    "Playlist".to_owned(),
                    format!("music:playlist:{}", l.id),
                    false,
                )
            }));
            if !cards.is_empty() {
                y = heading(p, area, m, y, "Top Results");
                shelf(p, area, m, y, &cards);
            }
        }
        _ => {
            y = heading(p, area, m, y, "Browse Categories");
            let tags = c.tags();
            let gap = 12;
            let inner = area.width.saturating_sub(m.pad as u32 * 2);
            let per = if m.mobile { 2 } else { (inner / 180).max(2) };
            let w = (inner - gap * (per - 1)) / per;
            for (i, tag) in tags.iter().enumerate() {
                let col = i as u32 % per;
                let row = i as u32 / per;
                let r = Rect::new(
                    area.x + m.pad + (col * (w + gap)) as i32,
                    y + (row * (70 + gap)) as i32,
                    w,
                    70,
                );
                let tint = art::tint(tag);
                p.button(
                    r,
                    art::mix(tint, Color::BLACK, 10),
                    10,
                    &format!("music:find:{tag}"),
                    tag,
                );
                let name: String = {
                    let mut ch = tag.chars();
                    ch.next()
                        .map(|f| f.to_uppercase().chain(ch).collect())
                        .unwrap_or_default()
                };
                p.strong(r.x + 12, r.y + 44, w - 20, &name, 15, Color::WHITE);
            }
        }
    }
}

fn queue(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, area: Rect, m: &M) {
    let c = &app.catalog;
    let mut y = title(app, p, area, m, "Playing Next");
    let Some(player) = &c.player else {
        p.left(
            area.x + m.pad,
            y,
            area.width,
            "Nothing is playing.",
            m.body,
            GREY,
        );
        return;
    };
    let live = app.live(env.clock_us);
    let index = live.as_ref().map_or(player.index, |l| l.index);
    // Shuffle and Repeat live here on the phone, as they do in iOS's Playing Next.
    let pills = [
        ("shuffle", "Shuffle", "music:shuffle", player.shuffle),
        (
            if player.repeat == Repeat::One {
                "repeat-one"
            } else {
                "repeat"
            },
            "Repeat",
            "music:repeat",
            player.repeat != Repeat::Off,
        ),
    ];
    let pw = 120;
    for (i, (symbol, label, target, on)) in pills.into_iter().enumerate() {
        let r = Rect::new(area.x + m.pad + i as i32 * (pw + 10), y, pw as u32, 32);
        p.button(r, if on { RED } else { FILL }, 8, target, label);
        let ink = if on { Color::WHITE } else { RED };
        p.symbol(symbol, r.x + 12, r.y + 8, 16, ink);
        p.strong(r.x + 34, r.y + 8, 80, label, 13, ink);
    }
    y += 48;
    if !player.context_title.is_empty() {
        p.left(
            area.x + m.pad,
            y,
            area.width,
            &format!("Playing from {}", player.context_title),
            m.small,
            GREY,
        );
        y += 22;
    }
    for (i, id) in player.queue.iter().enumerate() {
        if i < index {
            continue;
        }
        let Some(t) = c.track(id) else { continue };
        let r = Rect::new(
            area.x + m.pad,
            y,
            area.width.saturating_sub(m.pad as u32 * 2),
            m.row,
        );
        let now = i == index;
        p.button(
            r,
            if now {
                Color(RED.0, RED.1, RED.2, 24)
            } else {
                Color::TRANSPARENT
            },
            6,
            &format!("music:jump:{i}"),
            &t.title,
        );
        let s = m.row - 10;
        art::cover(p, Rect::new(r.x + 4, y + 5, s, s), &t.album, 4);
        p.left(
            r.x + s as i32 + 16,
            y + (m.row as i32 - 16) / 2 - if m.mobile { 8 } else { 0 },
            r.width.saturating_sub(s + 80),
            &t.title,
            m.body,
            if now { RED } else { INK },
        );
        if m.mobile {
            p.left(
                r.x + s as i32 + 16,
                y + 28,
                r.width.saturating_sub(s + 80),
                &c.artist_name(&t.artist),
                m.small,
                GREY,
            );
        }
        p.right(
            r.x + r.width as i32 - 60,
            y + (m.row as i32 - 16) / 2,
            52,
            &clock(t.duration_ms),
            m.small,
            GREY,
        );
        y += m.row as i32;
    }
}

/// iOS's full-screen Now Playing: the artwork's colour washes the whole sheet.
fn now_playing(app: &Music, p: &mut Painter, env: &crate::AppEnv<'_>, all: Rect) {
    let (Some(live), Some(track)) = (app.live(env.clock_us), app.now()) else {
        return;
    };
    let player = app.catalog.player.as_ref();
    let tint = art::tint(&track.album);
    p.gradient(
        all,
        art::mix(tint, Color::BLACK, 10),
        art::mix(tint, Color::BLACK, 70),
        24,
    );
    // The grabber collapses the sheet.
    let grab = Rect::new(all.x + (all.width as i32 - 80) / 2, all.y + 6, 80, 24);
    p.button(
        grab,
        Color::TRANSPARENT,
        12,
        "music:back",
        "Close Now Playing",
    );
    p.box_(
        Rect::new(grab.x + 22, grab.y + 8, 36, 5),
        Color(255, 255, 255, 110),
        3,
    );
    let size = all.width.saturating_sub(64).min(all.height / 2);
    let art_r = Rect::new(
        all.x + (all.width as i32 - size as i32) / 2,
        all.y + 44,
        size,
        size,
    );
    p.drop_shadow(art_r, 10, 24, 90, 12);
    art::cover(p, art_r, &track.album, 10);
    let x = all.x + 32;
    let w = all.width.saturating_sub(64);
    let mut y = art_r.y + size as i32 + 28;
    p.strong(x, y, w - 60, &track.title, 20, Color::WHITE);
    p.left(
        x,
        y + 28,
        w - 60,
        &app.catalog.artist_name(&track.artist),
        18,
        Color(255, 255, 255, 170),
    );
    let liked = app.catalog.liked.contains(&track.id);
    art::icon(
        p,
        Rect::new(x + w as i32 - 36, y + 6, 36, 36),
        if liked { "star" } else { "star-outline" },
        22,
        Color::WHITE,
        &format!("music:like:{}", track.id),
        if liked { "Undo Favorite" } else { "Favorite" },
    );
    y += 70;
    art::scrubber(
        p,
        Rect::new(x, y, w, 16),
        &live,
        6,
        Color(255, 255, 255, 220),
        Color(255, 255, 255, 70),
        None,
    );
    y += 22;
    p.left(
        x,
        y,
        60,
        &clock(live.position_ms),
        12,
        Color(255, 255, 255, 150),
    );
    p.right(
        x + w as i32 - 60,
        y,
        60,
        &format!(
            "-{}",
            clock(live.duration_ms.saturating_sub(live.position_ms))
        ),
        12,
        Color(255, 255, 255, 150),
    );
    y += 40;
    let cx = all.x + all.width as i32 / 2;
    art::icon(
        p,
        Rect::new(cx - 110, y - 6, 56, 56),
        "backward",
        34,
        Color::WHITE,
        "music:previous",
        "Previous",
    );
    art::icon(
        p,
        Rect::new(cx - 30, y - 10, 64, 64),
        if live.playing { "pause" } else { "play" },
        44,
        Color::WHITE,
        "music:toggle",
        if live.playing { "Pause" } else { "Play" },
    );
    let next = Rect::new(cx + 54, y - 6, 56, 56);
    if app.can_next(env.clock_us) {
        art::icon(p, next, "forward", 34, Color::WHITE, "music:next", "Next");
    } else {
        art::icon_off(
            p,
            next,
            "forward",
            34,
            Color::WHITE,
            "Nothing is queued after this song",
        );
    }
    y += 84;
    let row = [
        (
            "shuffle",
            "music:shuffle",
            "Shuffle",
            player.is_some_and(|p| p.shuffle),
        ),
        ("queue", "music:queue", "Playing Next", false),
        (
            if player.is_some_and(|p| p.repeat == Repeat::One) {
                "repeat-one"
            } else {
                "repeat"
            },
            "music:repeat",
            "Repeat",
            player.is_some_and(|p| p.repeat != Repeat::Off),
        ),
    ];
    let cell = w / 3;
    for (i, (symbol, target, label, on)) in row.into_iter().enumerate() {
        let r = Rect::new(
            x + (i as u32 * cell) as i32 + (cell as i32 - 44) / 2,
            y,
            44,
            44,
        );
        if on {
            p.box_(r, Color(255, 255, 255, 60), 10);
        }
        art::icon(
            p,
            r,
            symbol,
            22,
            Color(255, 255, 255, if on { 255 } else { 190 }),
            target,
            label,
        );
    }
    let _ = Live::clone;
}
