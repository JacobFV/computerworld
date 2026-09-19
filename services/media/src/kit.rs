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
    /// The like control's glyph, off and on: Spotify's heart, YouTube Music's thumb.
    pub like: (&'static str, &'static str),
}

/// Where the request came from, so a control can bring the listener back to it.
pub fn here(request: &HttpRequest) -> String {
    let path = web::path(request);
    match request.url.split_once('?') {
        Some((_, query)) if !query.is_empty() => format!("{path}?{query}"),
        _ => path,
    }
}
/// Artwork: `cw_artwork`'s composition for `of`, served rasterised by this site at
/// `/art/<of>` at the size it is shown, so an album has the same cover on its page, in a
/// grid, in the player bar and in the native players. `label` is its accessible name.
pub fn cover(id: &str, of: &str, label: &str, size: (u32, u32), radius: u32) -> PageElement {
    let side = if size.0 == 0 {
        size.1
    } else {
        size.0.min(size.1)
    };
    // A radius near half the side is a round avatar: make it exactly round.
    let radius = if radius * 2 + 8 >= side {
        side / 2
    } else {
        radius
    };
    PageElement::Image {
        id: id.into(),
        source: format!("/art/{}?size={side}&radius={radius}", encode(of)),
        alt: if label.is_empty() {
            "Cover art".into()
        } else {
            label.into()
        },
        width: side,
        height: side,
        style: None,
        action: None,
    }
}
/// The colour a header behind `of`'s artwork is washed with.
pub fn wash(of: &str) -> String {
    let c = cw_artwork::artwork(of).tint;
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}
/// Largest cover a page may ask for.
const ART_LIMIT: u32 = 512;
/// `GET /art/<key>?size=&radius=`: the cover for `key` as a page image.
pub fn artwork(key: &str, request: &HttpRequest) -> Result<HttpResponse> {
    #[derive(serde::Serialize)]
    struct Asset<'a> {
        width: u32,
        height: u32,
        rgba: &'a [u8],
    }
    let number = |k: &str| web::query(request, k).and_then(|v| v.parse::<u32>().ok());
    let size = number("size").unwrap_or(160).clamp(8, ART_LIMIT);
    let radius = number("radius").unwrap_or(0).min(size / 2);
    let rgba = cw_artwork::rasterize(&cw_artwork::artwork(key), size, radius);
    Ok(HttpResponse {
        status: 200,
        headers: std::collections::BTreeMap::from([(
            "content-type".into(),
            cw_protocol::RGBA_MEDIA_TYPE.into(),
        )]),
        body: serde_json::to_vec(&Asset {
            width: size,
            height: size,
            rgba: &rgba,
        })?,
    })
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
/// The shared spacer and chip-row helpers, kept under their old names here.
pub use web::pills;
#[allow(unused_imports)]
pub use web::rest;
/// An icon then its words, as a pill's content: "+ Create", "Shuffle".
pub fn labelled(id: &str, icon: &str, words: &str, size: u16, color: &str) -> PageElement {
    let glyph = u32::from(size) + 4;
    web::styled_row(
        id,
        6,
        "center",
        web::style(),
        vec![
            web::icon(
                &format!("{id}-icon"),
                icon,
                words,
                web::style().size(size + 4).color(color).width(glyph),
            ),
            line_bold(&format!("{id}-label"), words, size, color),
        ],
    )
}
pub fn fields(pairs: &[(&str, &str)]) -> PageAction {
    post("/player".into(), pairs)
}
/// A round icon button that posts a player command (or visits a page): `icon` names a
/// glyph from the page model's icon set and `label` is what it is called.
pub fn control(
    id: &str,
    icon: &str,
    label: &str,
    size: u16,
    fg: &str,
    bg: Option<&str>,
    action: PageAction,
) -> PageElement {
    let pad = if bg.is_some() { 10 } else { 6 };
    // Icon buttons are as wide as their glyph, not as wide as the row they sit in.
    let side = u32::from(size) + pad * 2;
    let style = web::style()
        .size(size)
        .color(fg)
        .padding(pad)
        .radius(side / 2)
        .width(side)
        .background(bg.unwrap_or("#00000000"));
    web::icon_action(id, icon, label, style, action)
}
/// A control that cannot act now: its glyph, faint, named with the reason.
pub fn inert(id: &str, icon: &str, label: &str, size: u16, skin: &Skin) -> PageElement {
    let side = u32::from(size) + 12;
    web::icon(
        id,
        icon,
        label,
        web::style()
            .size(size)
            .color(format!("{}66", skin.muted))
            .padding(6)
            .width(side),
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
            "shuffle",
            if p.shuffle {
                "Disable shuffle"
            } else {
                "Enable shuffle"
            },
            18,
            on(p.shuffle),
            None,
            fields(&[("action", "shuffle"), ("return", back)]),
        ),
        control(
            "player-previous",
            "skip-previous",
            "Previous",
            18,
            skin.ink,
            None,
            fields(&[("action", "previous"), ("return", back)]),
        ),
        control(
            "player-toggle",
            if p.playing { "pause" } else { "play" },
            if p.playing { "Pause" } else { "Play" },
            18,
            big.1,
            Some(big.0),
            fields(&[("action", "toggle"), ("return", back)]),
        ),
        if at_end {
            inert(
                "player-next",
                "skip-next",
                "Next (nothing is queued after this song)",
                18,
                skin,
            )
        } else {
            control(
                "player-next",
                "skip-next",
                "Next",
                18,
                skin.ink,
                None,
                fields(&[("action", "next"), ("return", back)]),
            )
        },
        control(
            "player-repeat",
            match p.repeat {
                Repeat::One => "repeat-one",
                _ => "repeat",
            },
            match p.repeat {
                Repeat::Off => "Enable repeat",
                Repeat::All => "Enable repeat one",
                Repeat::One => "Disable repeat",
            },
            18,
            on(p.repeat != Repeat::Off),
            None,
            fields(&[("action", "repeat"), ("return", back)]),
        ),
    ]
}
/// The like control for `id` in `skin`'s glyph, lit when the listener likes it.
pub fn like(id: &str, item: &str, liked: bool, size: u16, skin: &Skin, back: &str) -> PageElement {
    control(
        id,
        if liked { skin.like.1 } else { skin.like.0 },
        if liked { "Remove like" } else { "Like" },
        size,
        if liked { skin.accent } else { skin.muted },
        None,
        post(format!("/items/{item}/like"), &[("return", back)]),
    )
}
/// While the listener's music plays, a page asks the browser to fetch it again every
/// second of world time (`refresh: 1; url=<this page>`), so its player bar's position,
/// scrubber and lit lyric line move with the clock rather than only on a click.
pub fn live(
    mut response: HttpResponse,
    state: &Value,
    ctx: &ServiceContext,
    back: &str,
) -> HttpResponse {
    if catalog::player(state, &ctx.actor, ctx.tick).is_some_and(|p| p.playing) {
        response
            .headers
            .insert("refresh".into(), format!("1; url={back}"));
    }
    response
}
/// Steps in a volume slider. Each is a real volume command to its own level.
pub const VOLUME_STEPS: u32 = 10;
/// The player's volume: a speaker button that mutes and unmutes, and a slider whose steps
/// each set the volume to where they sit.
pub fn volume(p: &Player, skin: &Skin, back: &str) -> PageElement {
    let level = u32::from(p.audible());
    let mut cells = vec![control(
        "player-mute",
        if level == 0 { "volume-mute" } else { "volume" },
        if p.muted { "Unmute" } else { "Mute" },
        18,
        skin.muted,
        None,
        fields(&[("action", "mute"), ("return", back)]),
    )];
    for i in 1..=VOLUME_STEPS {
        let pct = i * 100 / VOLUME_STEPS;
        cells.push(web::thumbnail_action(
            &format!("player-volume-{pct}"),
            format!("Volume {pct}%"),
            web::style()
                .height(4)
                .radius(0)
                .flex(1)
                .background(if pct <= level { skin.ink } else { skin.track }),
            fields(&[
                ("action", "volume"),
                ("level", &pct.to_string()),
                ("return", back),
            ]),
        ));
    }
    web::styled_row("bar-volume", 1, "center", web::style().width(150), cells)
}
/// A song's lyrics as a column of lines, each a control that seeks to where it is sung;
/// `at` is the line being sung now, lit, with those already sung dimmer than those to come.
pub fn lyric_lines(
    prefix: &str,
    lines: &[(u64, String)],
    at: Option<usize>,
    size: u16,
    colours: (&str, &str, &str),
    back: &str,
) -> Vec<PageElement> {
    let (now, sung, ahead) = colours;
    lines
        .iter()
        .enumerate()
        .map(|(i, (ms, line))| {
            let colour = match at {
                Some(a) if a == i => now,
                Some(a) if i < a => sung,
                _ => ahead,
            };
            web::card_action(
                &format!("{prefix}-{i}"),
                web::style().padding(4).radius(4).background("#00000000"),
                fields(&[
                    ("action", "seek"),
                    ("position_ms", &ms.to_string()),
                    ("return", back),
                ]),
                vec![web::styled(
                    &format!("{prefix}-{i}-text"),
                    line,
                    web::style().size(size).bold().color(colour),
                )],
            )
        })
        .collect()
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
    let mut cells = vec![if current {
        web::icon(
            &format!("{prefix}-{id}-number"),
            "volume",
            "Now playing",
            web::style()
                .size(16)
                .color(skin.accent)
                .width(28)
                .align("right"),
        )
    } else {
        web::styled(
            &format!("{prefix}-{id}-number"),
            number.to_string(),
            web::style()
                .size(14)
                .color(skin.muted)
                .width(28)
                .align("right"),
        )
    }];
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
    row.push(like(
        &format!("{prefix}-{id}-like"),
        id,
        liked,
        16,
        skin,
        back,
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
