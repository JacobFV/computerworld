//! Building blocks the music sites share: artwork, transport controls, the scrubber, the
//! volume slider, lyric lines and a track row. The markup is the same on spotify.com,
//! soundcloud.com and music.youtube.com; each skin's stylesheet makes one control read
//! as that product's while doing the same thing.
use super::catalog::{self, Album};
use super::player::{Player, Repeat};
use super::view::{act, cover, div, icon, press, span, Html};
use super::*;

/// Segments in a scrubber. Each is a real seek to its own share of the track.
pub const SEGMENTS: u64 = 80;

/// Where the request came from, so a control can bring the listener back to it.
pub fn here(request: &HttpRequest) -> String {
    let path = web::path(request);
    match request.url.split_once('?') {
        Some((_, query)) if !query.is_empty() => format!("{path}?{query}"),
        _ => path,
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
/// The fields of a player command, posted to `/player`.
pub fn command(id: &str, fields: &[(&str, &str)], control: Html) -> Html {
    act(id, "/player", fields, control)
}
/// A round icon button that posts a player command.
pub fn control(id: &str, glyph: &str, label: &str, class: &str, fields: &[(&str, &str)]) -> Html {
    command(
        id,
        fields,
        press(&format!("ctl {class}"), label).child(icon(glyph)),
    )
}
/// A control that cannot act now: its glyph, faint, named with the reason.
pub fn inert(id: &str, glyph: &str, label: &str) -> Html {
    span("ctl inert")
        .id(id)
        .attr("role", "img")
        .attr("aria-label", label)
        .attr("title", label)
        .child(icon(glyph))
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
/// The five transport controls, in Spotify's order: shuffle, previous, play or pause,
/// next, repeat. `back` is where each one returns the listener.
pub fn transport(p: &Player, back: &str) -> Vec<Html> {
    let lit = |b: bool| if b { "on" } else { "" };
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
            lit(p.shuffle),
            &[("action", "shuffle"), ("return", back)],
        ),
        control(
            "player-previous",
            "previous",
            "Previous",
            "",
            &[("action", "previous"), ("return", back)],
        ),
        control(
            "player-toggle",
            if p.playing { "pause" } else { "play" },
            if p.playing { "Pause" } else { "Play" },
            "big",
            &[("action", "toggle"), ("return", back)],
        ),
        if at_end {
            inert(
                "player-next",
                "next",
                "Next (nothing is queued after this song)",
            )
        } else {
            control(
                "player-next",
                "next",
                "Next",
                "",
                &[("action", "next"), ("return", back)],
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
            lit(p.repeat != Repeat::Off),
            &[("action", "repeat"), ("return", back)],
        ),
    ]
}
/// The like control for `item`, lit when the listener likes it. The skin draws the
/// glyph: Spotify's heart, YouTube Music's thumb.
pub fn like(id: &str, item: &str, liked: bool, back: &str) -> Html {
    act(
        id,
        &format!("/items/{item}/like"),
        &[("return", back)],
        press(
            if liked { "ctl like on" } else { "ctl like" },
            if liked { "Remove like" } else { "Like" },
        )
        .child(icon(if liked { "like-fill" } else { "like" })),
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
pub fn volume(p: &Player, back: &str) -> Html {
    let level = u32::from(p.audible());
    div("volume")
        .id("bar-volume")
        .child(control(
            "player-mute",
            if level == 0 { "volume-mute" } else { "volume" },
            if p.muted { "Unmute" } else { "Mute" },
            "",
            &[("action", "mute"), ("return", back)],
        ))
        .child(div("slider").each(1..=VOLUME_STEPS, |i| {
            let pct = i * 100 / VOLUME_STEPS;
            command(
                &format!("player-volume-{pct}"),
                &[
                    ("action", "volume"),
                    ("level", &pct.to_string()),
                    ("return", back),
                ],
                press(
                    if pct <= level { "seg on" } else { "seg" },
                    &format!("Volume {pct}%"),
                ),
            )
        }))
}
/// A song's lyrics as a column of lines, each a control that seeks to where it is sung;
/// `at` is the line being sung now (`now`), those already sung are `sung` and those to
/// come `ahead`.
pub fn lyric_lines(
    prefix: &str,
    lines: &[(u64, String)],
    at: Option<usize>,
    back: &str,
) -> Vec<Html> {
    lines
        .iter()
        .enumerate()
        .map(|(i, (ms, line))| {
            let class = match at {
                Some(a) if a == i => "lyric now",
                Some(a) if i < a => "lyric sung",
                _ => "lyric ahead",
            };
            command(
                &format!("{prefix}-{i}"),
                &[
                    ("action", "seek"),
                    ("position_ms", &ms.to_string()),
                    ("return", back),
                ],
                press(class, "").child(
                    span("")
                        .id(format!("{prefix}-{i}-text"))
                        .text(line.as_str()),
                ),
            )
        })
        .collect()
}
/// The scrubber: every segment seeks to where it sits, the played ones lit.
pub fn scrubber(p: &Player, length_ms: u64, back: &str) -> Html {
    let played = (p.position_ms * SEGMENTS)
        .checked_div(length_ms)
        .unwrap_or(0);
    div("scrub").each(0..SEGMENTS, |i| {
        let at = length_ms * i / SEGMENTS;
        let on = i < played || (i == 0 && p.position_ms > 0);
        command(
            &format!("player-seek-{i}"),
            &[
                ("action", "seek"),
                ("position_ms", &at.to_string()),
                ("return", back),
            ],
            press(
                if on { "seg on" } else { "seg" },
                &format!("Seek to {}", clock_ms(at)),
            ),
        )
    })
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
/// What a track row is drawn from besides the track itself.
pub struct Row<'a> {
    pub prefix: &'a str,
    /// The context the row plays within (`album:<id>`, `playlist:<id>`, `charts`...).
    pub context: &'a str,
    /// The item playing now, whose row is lit.
    pub playing: Option<&'a str>,
    pub back: &'a str,
    /// Whether the row shows its album's cover.
    pub art: bool,
}
/// One row of a track list: number (or a playing mark), cover, title and artist, an
/// extra column, then like and duration. The whole row plays the track within its
/// context; like is its own control.
pub fn track_row(
    state: &Value,
    actor: &str,
    row: &Row,
    id: &str,
    number: usize,
    extra: Option<String>,
) -> Html {
    let Row {
        prefix,
        context,
        playing,
        back,
        art,
    } = *row;
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let artist = catalog::artist_name(state, &web::text(&item, "channel"));
    let current = playing == Some(id);
    let liked = has(state, "likes", actor, id);
    let number_cell = if current {
        span("num playing")
            .id(format!("{prefix}-{id}-number"))
            .attr("role", "img")
            .attr("aria-label", "Now playing")
            .child(icon("bars"))
    } else {
        span("num")
            .id(format!("{prefix}-{id}-number"))
            .text(number.to_string())
    };
    let cells = span(if art { "cells with-art" } else { "cells" })
        .id(format!("{prefix}-{id}-cells"))
        .child(number_cell)
        .when(art, |c| {
            c.child(cover(
                &format!("{prefix}-{id}-art"),
                &catalog::album_id(&item),
                "",
                40,
                4,
            ))
        })
        .child(
            span("names")
                .id(format!("{prefix}-{id}-text"))
                .child(
                    span("title")
                        .id(format!("{prefix}-{id}-title"))
                        .text(web::text(&item, "title")),
                )
                .child(
                    span("artist")
                        .id(format!("{prefix}-{id}-artist"))
                        .text(artist),
                ),
        )
        .child(
            span("extra")
                .id(format!("{prefix}-{id}-extra"))
                .text(extra.unwrap_or_default()),
        );
    div(if current { "trow current" } else { "trow" })
        .id(format!("{prefix}-line-{id}"))
        .child(command(
            &format!("{prefix}-{id}"),
            &[
                ("action", "play"),
                ("item", id),
                ("context", context),
                ("return", back),
            ],
            press("rowplay", "").child(cells),
        ))
        .child(like(&format!("{prefix}-{id}-like"), id, liked, back))
        .child(
            span("dur")
                .id(format!("{prefix}-{id}-duration"))
                .text(clock(num(&item, "duration_s"))),
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
