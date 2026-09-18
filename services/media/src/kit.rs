//! Building blocks the two music sites share: artwork, transport controls, the scrubber
//! and a track row. Each site passes its own `Skin`, so one control reads as Spotify's on
//! spotify.com and as YouTube Music's on music.youtube.com while doing the same thing.
use super::catalog::{self, Album};
use super::player::{Player, Repeat};
use super::*;

/// Segments in a scrubber. Each is a real seek to its own share of the track.
pub const SEGMENTS: u64 = 80;

#[derive(Clone, Copy)]
pub struct Skin {
    pub page: &'static str,
    pub panel: &'static str,
    pub raised: &'static str,
    pub ink: &'static str,
    pub muted: &'static str,
    pub accent: &'static str,
    /// Colour of the scrubber's played part.
    pub played: &'static str,
    pub track: &'static str,
    pub art_radius: u32,
}

/// Where the request came from, so a control can bring the listener back to it.
pub fn here(request: &HttpRequest) -> String {
    let path = web::path(request);
    match request.url.split_once('?') {
        Some((_, query)) if !query.is_empty() => format!("{path}?{query}"),
        _ => path,
    }
}
/// Artwork: a flat tint chosen by what it depicts, so an album is the same colour on its
/// page, in a grid and in the player bar.
pub fn cover(id: &str, of: &str, label: &str, size: (u32, u32), radius: u32) -> PageElement {
    let style = web::style();
    let style = if size.0 == 0 {
        style
    } else {
        style.width(size.0)
    };
    web::thumbnail(
        id,
        label,
        style
            .height(size.1)
            .radius(radius)
            .background(tint(of))
            .color("#e8eaed")
            .size(if size.1 >= 120 { 14 } else { 11 })
            .align("center"),
    )
}
pub fn text(id: &str, s: impl Into<String>, size: u16, color: &str) -> PageElement {
    web::styled(id, s, web::style().size(size).color(color))
}
pub fn bold(id: &str, s: impl Into<String>, size: u16, color: &str) -> PageElement {
    web::styled(id, s, web::style().size(size).bold().color(color))
}
/// Text that never wraps: a pill's label, which sizes the pill.
pub fn line(id: &str, s: impl Into<String>, size: u16, color: &str) -> PageElement {
    web::styled(id, s, web::style().size(size).color(color).one_line())
}
pub fn line_bold(id: &str, s: impl Into<String>, size: u16, color: &str) -> PageElement {
    web::styled(
        id,
        s,
        web::style().size(size).bold().color(color).one_line(),
    )
}
/// Soaks up a row's spare width so the pills before it keep their own size, as chips and
/// button rows do on every one of these sites instead of stretching edge to edge.
pub fn rest(id: &str) -> PageElement {
    web::styled(id, "", web::style().flex(64))
}
/// `children` laid out at their natural widths, left-aligned.
pub fn pills(id: &str, gap: u32, mut children: Vec<PageElement>) -> PageElement {
    children.push(rest(&format!("{id}-rest")));
    web::styled_row(id, gap, "center", web::style(), children)
}
pub fn fields(pairs: &[(&str, &str)]) -> PageAction {
    post("/player".into(), pairs)
}
/// A pill or round button that posts a player command.
pub fn control(
    id: &str,
    glyph: &str,
    size: u16,
    fg: &str,
    bg: Option<&str>,
    action: PageAction,
) -> PageElement {
    let pad = if bg.is_some() { 10 } else { 6 };
    // Icon buttons are as wide as their glyph, not as wide as the row they sit in.
    let glyphs = glyph.chars().count().max(1) as u32;
    let style = web::style()
        .padding(pad)
        .radius(24)
        .width(pad * 2 + (u32::from(size) * 13 / 10).max(u32::from(size) * glyphs * 3 / 4 + 6));
    let style = match bg {
        Some(bg) => style.background(bg),
        None => style.background("#00000000"),
    };
    web::card_action(
        id,
        style,
        action,
        vec![web::styled(
            &format!("{id}-glyph"),
            glyph,
            web::style().size(size).bold().color(fg).align("center"),
        )],
    )
}
/// A glyph shown in a control's place when it cannot act, and says so by being faint.
pub fn inert(id: &str, glyph: &str, size: u16, skin: &Skin) -> PageElement {
    web::card(
        id,
        web::style()
            .padding(6)
            .radius(24)
            .width(
                12 + (u32::from(size) * 13 / 10)
                    .max(u32::from(size) * glyph.chars().count().max(1) as u32 * 3 / 4 + 6),
            )
            .background("#00000000"),
        vec![web::styled(
            &format!("{id}-glyph"),
            glyph,
            web::style()
                .size(size)
                .bold()
                .color(format!("{}66", skin.muted))
                .align("center"),
        )],
    )
}
/// `hex` darkened to `pct` percent of itself: the dim wash behind a header's artwork.
pub fn shade(hex: &str, pct: u32) -> String {
    let channel = |i: usize| {
        u32::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) * pct / 100
    };
    format!("#{:02x}{:02x}{:02x}", channel(1), channel(3), channel(5))
}
pub fn clock_ms(ms: u64) -> String {
    clock(ms / 1_000)
}
/// The five transport controls. `back` is where each one returns the listener.
pub fn transport(p: &Player, skin: &Skin, back: &str, big: (&str, &str)) -> Vec<PageElement> {
    let on = |b: bool| if b { skin.accent } else { skin.muted };
    let at_end = p.index + 1 >= p.queue.len() && p.repeat != Repeat::All;
    vec![
        control(
            "player-shuffle",
            "⇄",
            16,
            on(p.shuffle),
            None,
            fields(&[("action", "shuffle"), ("return", back)]),
        ),
        control(
            "player-previous",
            "▮◀",
            14,
            skin.ink,
            None,
            fields(&[("action", "previous"), ("return", back)]),
        ),
        control(
            "player-toggle",
            if p.playing { "❚❚" } else { "▶" },
            14,
            big.1,
            Some(big.0),
            fields(&[("action", "toggle"), ("return", back)]),
        ),
        if at_end {
            inert("player-next", "▶▮", 14, skin)
        } else {
            control(
                "player-next",
                "▶▮",
                14,
                skin.ink,
                None,
                fields(&[("action", "next"), ("return", back)]),
            )
        },
        control(
            "player-repeat",
            match p.repeat {
                Repeat::One => "↻1",
                _ => "↻",
            },
            16,
            on(p.repeat != Repeat::Off),
            None,
            fields(&[("action", "repeat"), ("return", back)]),
        ),
    ]
}
/// The scrubber: every segment seeks to where it sits, the played ones in `skin.played`.
pub fn scrubber(p: &Player, length_ms: u64, skin: &Skin, back: &str) -> Vec<PageElement> {
    let played = (p.position_ms * SEGMENTS)
        .checked_div(length_ms)
        .unwrap_or(0);
    (0..SEGMENTS)
        .map(|i| {
            let at = length_ms * i / SEGMENTS;
            web::thumbnail_action(
                &format!("player-seek-{i}"),
                format!("Seek to {}", clock_ms(at)),
                web::style().height(4).radius(0).flex(1).background(
                    if i < played || (i == 0 && p.position_ms > 0) {
                        skin.played
                    } else {
                        skin.track
                    },
                ),
                fields(&[
                    ("action", "seek"),
                    ("position_ms", &at.to_string()),
                    ("return", back),
                ]),
            )
        })
        .collect()
}
/// Pure numbers for a bar: what is loaded, where it is and how long it is.
pub struct Loaded {
    pub player: Player,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub album: Option<Album>,
    pub length_ms: u64,
}
pub fn loaded(state: &Value, actor: &str, now: u64) -> Option<Loaded> {
    let player = catalog::player(state, actor, now)?;
    let id = player.current()?.to_owned();
    let item = record(state, "items", &id)?.clone();
    let artist_id = web::text(&item, "channel");
    Some(Loaded {
        title: web::text(&item, "title"),
        artist: catalog::artist_name(state, &artist_id),
        album: catalog::album(state, &catalog::album_id(&item)),
        length_ms: catalog::duration_ms(state, &id),
        artist_id,
        id,
        player,
    })
}
/// One row of a track list: number (or a playing mark), title and artist, extra columns,
/// then like and duration. The whole row plays the track within `context`.
#[allow(clippy::too_many_arguments)]
pub fn track_row(
    state: &Value,
    actor: &str,
    prefix: &str,
    id: &str,
    number: usize,
    context: &str,
    playing: Option<&str>,
    skin: &Skin,
    back: &str,
    extra: Option<String>,
    art: bool,
) -> PageElement {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let artist = catalog::artist_name(state, &web::text(&item, "channel"));
    let current = playing == Some(id);
    let liked = has(state, "likes", actor, id);
    let title_colour = if current { skin.accent } else { skin.ink };
    let mut cells = vec![web::styled(
        &format!("{prefix}-{id}-number"),
        if current {
            "♪".to_owned()
        } else {
            number.to_string()
        },
        web::style()
            .size(14)
            .color(if current { skin.accent } else { skin.muted })
            .width(28)
            .align("right"),
    )];
    if art {
        cells.push(cover(
            &format!("{prefix}-{id}-art"),
            &catalog::album_id(&item),
            "",
            (40, 40),
            skin.art_radius.min(4),
        ));
    }
    cells.push(web::column(
        &format!("{prefix}-{id}-text"),
        0,
        web::style().flex(3),
        vec![
            web::styled(
                &format!("{prefix}-{id}-title"),
                web::text(&item, "title"),
                web::style().size(15).color(title_colour),
            ),
            text(&format!("{prefix}-{id}-artist"), artist, 13, skin.muted),
        ],
    ));
    if let Some(extra) = extra {
        cells.push(web::styled(
            &format!("{prefix}-{id}-extra"),
            extra,
            web::style().size(13).color(skin.muted).flex(2),
        ));
    }
    let mut row = vec![web::card_action(
        &format!("{prefix}-{id}"),
        web::style()
            .padding(6)
            .radius(4)
            .flex(1)
            .background(if current { skin.raised } else { "#00000000" }),
        fields(&[
            ("action", "play"),
            ("item", id),
            ("context", context),
            ("return", back),
        ]),
        vec![web::styled_row(
            &format!("{prefix}-{id}-cells"),
            14,
            "center",
            web::style(),
            cells,
        )],
    )];
    row.push(control(
        &format!("{prefix}-{id}-like"),
        if liked { "♥" } else { "♡" },
        15,
        if liked { skin.accent } else { skin.muted },
        None,
        post(format!("/items/{id}/like"), &[("return", back)]),
    ));
    row.push(web::styled(
        &format!("{prefix}-{id}-duration"),
        clock(num(&item, "duration_s")),
        web::style()
            .size(13)
            .color(skin.muted)
            .width(44)
            .align("right"),
    ));
    web::styled_row(
        &format!("{prefix}-line-{id}"),
        8,
        "center",
        web::style(),
        row,
    )
}
/// Total running time as players print it: "11 min 25 sec" or "1 hr 4 min".
pub fn running(state: &Value, ids: &[String]) -> String {
    let seconds: u64 = ids
        .iter()
        .map(|id| catalog::duration_ms(state, id) / 1_000)
        .sum();
    match (seconds / 3600, (seconds / 60) % 60, seconds % 60) {
        (0, m, s) => format!("{m} min {s} sec"),
        (h, m, _) => format!("{h} hr {m} min"),
    }
}
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
/// Hour of the day on the world clock, which starts at 09:00.
pub fn hour(tick: u64) -> u64 {
    (9 + tick / 3_600_000_000) % 24
}
