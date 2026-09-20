//! The speaker's own page: what a network speaker shows when you open its address.
//!
//! One skin, `speaker.css`: the full-bleed "now playing" screen a Sonos or a Chromecast
//! puts on a display — the room's name and the protocols it speaks across the top, the
//! album art large in the middle over a wash of the artwork's own colour, the title and
//! artist under it, a scrubber that carries the world clock, the speaker's own volume as
//! a slider, and the rest of the handed-over queue as "Up next". Idle, the art is a
//! CSS-drawn grille and the screen says the speaker is ready.
//!
//! The controls are the browser's half of the JSON API the players use: the volume
//! segments post to `/volume` and Stop to `/stop`, which run exactly the handlers
//! `/api/volume` and `/api/stop` run and then re-render this page.
use super::{clock, Session, Speaker};
use crate::Track;
use cw_service_common::html::{button, div, el, form, hidden, span, Document, Html};
use cw_service_media::Player;

const CSS: &str = include_str!("speaker.css");
/// Steps in the volume slider: every fifth percent is its own control.
const STEPS: u64 = 20;

/// A protocol's name as the badge prints it.
fn protocol(name: &str) -> &'static str {
    match name {
        "airplay" => "AirPlay",
        "cast" => "Cast",
        _ => "DLNA",
    }
}
/// The artwork key a music service would use for this album: the slug of its title, the
/// very same key `cw_service_media` rasterises, so the speaker shows the same picture the
/// player that cast to it does.
pub fn art_key(album: &str, fallback: &str) -> String {
    let title = if album.trim().is_empty() {
        fallback
    } else {
        album
    };
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    slug.split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
/// `#rrggbb` of the colour the artwork is built around: the wash behind the screen.
fn tint(key: &str) -> String {
    let c = cw_artwork::artwork(key).tint;
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}
/// A control that posts: a one-button form, the button carrying the id.
fn act(id: &str, url: &str, fields: &[(&str, &str)], control: Html) -> Html {
    form(&format!("{id}-form"), url, "post")
        .class("act")
        .each(fields.iter(), |(k, v)| hidden(k, v))
        .child(control.id(id).attr("type", "submit"))
}
/// The speaker's own volume as a row of segments, each a real set to its own level.
fn volume(level: u8) -> Html {
    div("slider").id("speaker-slider").each(1..=STEPS, |step| {
        let pct = step * 100 / STEPS;
        act(
            &format!("speaker-volume-{pct}"),
            "/volume",
            &[("level", &pct.to_string())],
            el("button")
                .class(if pct <= u64::from(level) { "seg on" } else { "seg" })
                .attr("aria-label", format!("Volume {pct}%"))
                .attr("title", format!("Volume {pct}%")),
        )
    })
}
/// How far in, as a bar that cannot be dragged: the speaker is told where it is, it is
/// not asked, so this is a readout rather than a control.
fn scrubber(position_ms: u64, duration_ms: u64) -> Html {
    let played = (position_ms * 100).checked_div(duration_ms.max(1)).unwrap_or(0).min(100);
    div("scrub")
        .id("speaker-scrub")
        .child(span("fill").style(&format!("width: {played}%")))
}
/// The protocol badges: what this speaker will answer to.
fn badges(speaker: &Speaker) -> Html {
    div("chips").id("speaker-protocols").each(
        speaker.protocols.iter(),
        |p| span("chip").id(format!("speaker-proto-{p}")).text(protocol(p)),
    )
}
/// The header every state shows: the grille mark, the room's name, the model and the
/// protocols.
fn header(speaker: &Speaker) -> Html {
    el("header")
        .id("speaker-head")
        .class("head")
        .child(
            div("who")
                .child(span("mark").attr("aria-hidden", "true").child(el("i")).child(el("i")))
                .child(
                    div("names")
                        .child(el("h1").id("speaker-name").text(speaker.name.as_str()))
                        .child(el("p").id("speaker-model").class("model").text(format!(
                            "{} · {}",
                            if speaker.model.is_empty() {
                                "Network speaker"
                            } else {
                                &speaker.model
                            },
                            speaker
                                .protocols
                                .iter()
                                .map(|p| protocol(p))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))),
                ),
        )
        .child(badges(speaker))
}
/// The volume panel, which is the speaker's own whether or not anything is playing.
fn volume_panel(speaker: &Speaker) -> Html {
    div("volume")
        .child(
            div("vhead")
                .child(span("ic ic-volume").attr("aria-hidden", "true").child(el("i")))
                .child(
                    el("p")
                        .id("speaker-volume")
                        .class("level")
                        .text(format!("Volume {}%", speaker.volume)),
                ),
        )
        .child(volume(speaker.volume))
}
/// What is on: the art, the state, the track, where it came from, the scrubber.
fn stage(session: &Session, p: &Player, track: &Track, id: &str) -> Html {
    let key = art_key(&track.album, &track.title);
    let artist = if track.artist.is_empty() {
        "Unknown artist".to_owned()
    } else {
        track.artist.clone()
    };
    let album = if track.album.is_empty() {
        String::new()
    } else {
        format!(" · {}", track.album)
    };
    el("main")
        .id("speaker-stage")
        .class("stage")
        .child(
            div("art")
                .child(
                    el("img")
                        .id("speaker-art")
                        .class("cover")
                        .attr("src", format!("/art/{key}?size=320&radius=10"))
                        .attr("alt", format!("{}, {artist}", track.title))
                        .attr("width", "320")
                        .attr("height", "320"),
                )
                .child(span("shadow").attr("aria-hidden", "true")),
        )
        .child(
            div("meta")
                .child(
                    span(if p.playing { "state on" } else { "state" })
                        .id("speaker-state")
                        .child(span("dot").attr("aria-hidden", "true"))
                        .child(span("word").text(if p.playing { "Playing" } else { "Paused" })),
                )
                .child(el("h2").id("speaker-now").class("track").text(if track.title.is_empty() {
                    id.to_owned()
                } else {
                    track.title.clone()
                }))
                .child(
                    el("p")
                        .id("speaker-artist")
                        .class("artist")
                        .text(format!("{artist}{album}")),
                )
                .child(
                    div("line")
                        .child(scrubber(p.position_ms, track.duration_ms))
                        .child(span("time").id("speaker-position").text(format!(
                            "{} / {}",
                            clock(p.position_ms),
                            clock(track.duration_ms)
                        ))),
                )
                .child(
                    el("p")
                        .id("speaker-source")
                        .class("source")
                        .text(format!("From {} ({})", session.source, session.controller)),
                )
                .child(
                    div("actions")
                        .child(act("speaker-stop", "/stop", &[], button("", "Stop").class("stop"))),
                ),
        )
}
/// The rest of the handed-over queue, in the order it will be heard.
fn up_next(session: &Session, p: &Player) -> Html {
    let rest: Vec<(usize, &String)> = p
        .queue
        .iter()
        .enumerate()
        .skip(p.index + 1)
        .take(8)
        .collect();
    if rest.is_empty() {
        return el("section")
            .id("speaker-queue")
            .class("queue empty")
            .child(el("h3").class("qhead").text("Up next"))
            .child(el("p").id("speaker-queue-end").class("fine").text("Last in the queue"));
    }
    el("section")
        .id("speaker-queue")
        .class("queue")
        .child(el("h3").class("qhead").text("Up next"))
        .child(el("ol").class("rows").each(rest, |(i, id)| {
            let track = session.tracks.get(id).cloned().unwrap_or_default();
            el("li")
                .id(format!("speaker-queue-{i}"))
                .class("row")
                .child(span("num").text((i + 1).to_string()))
                .child(
                    div("qnames")
                        .child(span("qtitle").text(if track.title.is_empty() {
                            id.clone()
                        } else {
                            track.title.clone()
                        }))
                        .child(span("qartist").text(track.artist.clone())),
                )
                .child(span("qtime").text(clock(track.duration_ms)))
        }))
}
/// Nothing playing: the grille, the room's name, and what it is waiting for.
fn idle(speaker: &Speaker) -> Html {
    let ready = if speaker.protocols.is_empty() {
        "Waiting for a player".to_owned()
    } else {
        format!(
            "Ready for {}",
            speaker
                .protocols
                .iter()
                .map(|p| protocol(p))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    el("main")
        .id("speaker-stage")
        .class("stage idle")
        .child(
            div("art").child(
                span("grille")
                    .attr("aria-hidden", "true")
                    .child(el("i"))
                    .child(el("i"))
                    .child(el("i")),
            ),
        )
        .child(
            div("meta")
                .child(
                    span("state")
                        .id("speaker-state")
                        .child(span("dot").attr("aria-hidden", "true"))
                        .child(span("word").text("Idle")),
                )
                .child(el("h2").id("speaker-now").class("track").text("Nothing playing"))
                .child(el("p").id("speaker-artist").class("artist").text(ready)),
        )
}

/// The whole page: the wash on `<html>`, the header, the stage and the volume.
pub fn page(speaker: &Speaker, now: u64) -> Document {
    let playing = speaker.settled(now).map(|(session, p)| {
        let id = p.current().unwrap_or_default().to_owned();
        let track = session.tracks.get(&id).cloned().unwrap_or_default();
        (session, p, track, id)
    });
    let wash = playing
        .as_ref()
        .map(|(_, _, track, _)| tint(&art_key(&track.album, &track.title)))
        .unwrap_or_else(|| "#2a2f3a".to_owned());
    let (stage_, queue) = match &playing {
        Some((session, p, track, id)) => (stage(session, p, track, id), up_next(session, p)),
        None => (idle(speaker), Html::Fragment(Vec::new())),
    };
    Document::new(&speaker.name)
        .lang("en")
        .stylesheet(CSS)
        .root_style(&format!("--wash: {wash}"))
        .body_class(if playing.is_some() {
            "speaker on"
        } else {
            "speaker off"
        })
        .body([
            div("room")
                .child(header(speaker))
                .child(stage_)
                .child(div("panel").child(volume_panel(speaker)).child(queue)),
        ])
}
