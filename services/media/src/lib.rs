//! Streaming platforms: youtube.com (`mode: video`) and spotify.com (`mode: audio`) share
//! one catalogue shape, so channels/artists and videos/tracks are the same two maps.
//!
//! Watching is a mutation on purpose: `GET /watch` counts a view and parks `now_playing`, so the
//! world visibly changes when an agent watches something. Audio keeps the play count behind the
//! explicit Play control, which is where a listener expects it.
use cw_protocol::{HttpRequest, HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Map, Value};
pub struct MediaService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(MediaService)
}
/// Documented seed keys, checked by container type only — the crate that fills them owns the rest.
const OBJECTS: &[&str] = &[
    "theme",
    "channels",
    "items",
    "playlists",
    "subscriptions",
    "likes",
    "now_playing",
];
const ARRAYS: &[&str] = &[];
/// `mode` is the documented discriminant; an unlisted value is a seed typo, not a fallback.
const MODES: &[&str] = &["video", "audio"];
const BRAND: &str = "Media";
const TAGLINE: &str = "Watch and listen.";
/// Flat stand-in tints. Artwork is never photography here, so a stable hash of the id keeps every
/// tile the same colour across renders, checkpoints and platforms.
const TINTS: &[&str] = &[
    "#2b3a55", "#4a2d43", "#1f4037", "#4b3621", "#2f2f4f", "#523a28", "#264653", "#3d2c4b",
];
fn tint(seed: &str) -> &'static str {
    let hash = seed
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    TINTS[hash as usize % TINTS.len()]
}
fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}
/// Thousands separators, because "18342 views" reads like a debug print, not a product.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
fn clock(seconds: u64) -> String {
    match (seconds / 3600, (seconds / 60) % 60, seconds % 60) {
        (0, m, s) => format!("{m}:{s:02}"),
        (h, m, s) => format!("{h}:{m:02}:{s:02}"),
    }
}
fn record<'a>(state: &'a Value, map: &str, id: &str) -> Option<&'a Value> {
    state.get(map)?.get(id)
}
/// Every map is keyed by id, so sorted keys are a stable render order for free.
fn keys(state: &Value, map: &str) -> Vec<String> {
    match state.get(map).and_then(Value::as_object) {
        Some(m) => m.keys().cloned().collect(),
        None => vec![],
    }
}
/// Newest first, id breaking ties — the home grid and every channel page share this order.
fn by_recency(state: &Value, filter: impl Fn(&Value) -> bool) -> Vec<String> {
    let mut ids: Vec<String> = keys(state, "items")
        .into_iter()
        .filter(|id| record(state, "items", id).is_some_and(&filter))
        .collect();
    ids.sort_by_key(|id| {
        let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
        (std::cmp::Reverse(num(&item, "published_tick")), id.clone())
    });
    ids
}
fn strings_at(state: &Value, map: &str, key: &str) -> Vec<String> {
    match state.get(map).and_then(|m| m.get(key)) {
        Some(v) => web::strings(&json!({ "v": v }), "v"),
        None => vec![],
    }
}
fn has(state: &Value, map: &str, key: &str, needle: &str) -> bool {
    strings_at(state, map, key).iter().any(|v| v == needle)
}
/// Per-actor membership lists (likes, subscriptions) are all toggles; one place to flip them.
fn toggle(state: &mut Value, map: &str, key: &str, value: &str) -> bool {
    let list = state
        .as_object_mut()
        .expect("state is an object")
        .entry(map)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .map(|m| m.entry(key).or_insert_with(|| json!([])))
        .and_then(Value::as_array_mut);
    let Some(list) = list else { return false };
    match list.iter().position(|v| v.as_str() == Some(value)) {
        Some(at) => {
            list.remove(at);
            false
        }
        None => {
            list.push(json!(value));
            true
        }
    }
}
fn post(url: String, fields: &[(&str, &str)]) -> PageAction {
    PageAction {
        method: "POST".into(),
        url,
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).to_owned()))
            .collect(),
    }
}
/// A pill that really submits; `on` is the engaged state, which is what makes a toggle legible.
fn pill(id: &str, text: &str, on: bool, theme: &PageTheme, action: PageAction) -> PageElement {
    let accent = theme.accent.clone().unwrap_or_else(|| "#ff0000".into());
    let surface = theme.surface.clone().unwrap_or_else(|| "#212121".into());
    let ink = theme.ink.clone().unwrap_or_else(|| "#f1f1f1".into());
    let style =
        web::style()
            .padding(10)
            .radius(18)
            .background(if on { accent.clone() } else { surface });
    web::card_action(
        id,
        style,
        action,
        vec![web::styled(
            &format!("{id}-text"),
            text,
            web::style()
                .size(13)
                .medium()
                .color(if on { "#ffffff".to_owned() } else { ink }),
        )],
    )
}
fn muted(theme: &PageTheme) -> String {
    theme.muted.clone().unwrap_or_else(|| "#aaaaaa".into())
}
fn ink(theme: &PageTheme) -> String {
    theme.ink.clone().unwrap_or_else(|| "#f1f1f1".into())
}
fn surface(theme: &PageTheme) -> String {
    theme.surface.clone().unwrap_or_else(|| "#212121".into())
}
/// Artwork stand-in: a tinted block carrying its own label, never claiming to be a photograph.
/// A flat-colour stand-in for a still. A `width` of 0 fills the column it sits in.
fn art(id: &str, label: &str, width: u32, height: u32, radius: u32) -> PageElement {
    let style = web::style();
    let style = if width == 0 {
        style
    } else {
        style.width(width)
    };
    web::thumbnail(
        id,
        label,
        style
            .height(height)
            .radius(radius)
            .background(tint(id))
            .color("#e8eaed")
            .align("center"),
    )
}
fn chrome(state: &Value, mode: &str, theme: &PageTheme) -> PageElement {
    let brand = match web::text(state, "brand").as_str() {
        "" => BRAND.to_owned(),
        s => s.to_owned(),
    };
    let accent = theme.accent.clone().unwrap_or_else(|| "#ff0000".into());
    let (search_url, field, label, library) = match mode {
        "audio" => ("/search", "q", "Search songs and artists", "/playlists"),
        _ => ("/results", "search_query", "Search", "/playlists"),
    };
    web::styled_row(
        "chrome",
        16,
        "center",
        web::style().background(surface(theme)).padding(12),
        vec![
            web::card_action(
                "chrome-home",
                web::style().width(150),
                web::visit("/"),
                vec![web::styled(
                    "chrome-brand",
                    brand,
                    web::style().size(22).bold().color(accent),
                )],
            ),
            web::styled_row(
                "chrome-search",
                0,
                "center",
                web::style().flex(3),
                vec![web::form("search", search_url, &[(field, label, "")])],
            ),
            web::link("chrome-library", "Library", library),
        ],
    )
}
/// Optional seed flags — "LIVE", "4K", "NEW". LIVE is the loud one, as it is on the real thing.
fn flags(item: &Value, prefix: &str, theme: &PageTheme) -> Vec<PageElement> {
    web::strings(item, "badges")
        .iter()
        .map(|flag| {
            let background = if flag.eq_ignore_ascii_case("live") {
                theme.accent.clone().unwrap_or_else(|| "#ff0000".into())
            } else {
                "#3f3f3f".to_owned()
            };
            web::badge(
                &format!("{prefix}-flag-{}", slug(flag)),
                flag,
                web::style()
                    .size(11)
                    .padding(4)
                    .radius(4)
                    .color("#ffffff")
                    .background(background),
            )
        })
        .collect()
}
/// One video tile: artwork, duration, title, channel, view count — the unit the home grid repeats.
fn video_card(state: &Value, id: &str, theme: &PageTheme) -> PageElement {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let channel = web::text(&item, "channel");
    let channel_name = record(state, "channels", &channel)
        .map(|c| web::text(c, "name"))
        .unwrap_or(channel);
    web::card_action(
        &format!("tile-{id}"),
        web::style().padding(4),
        web::visit(format!("/watch?v={id}")),
        vec![
            art(
                &format!("tile-{id}-art"),
                &web::text(&item, "title"),
                0,
                150,
                8,
            ),
            web::styled_row(
                &format!("tile-{id}-meta"),
                8,
                "center",
                web::style(),
                std::iter::once(web::badge(
                    &format!("tile-{id}-duration"),
                    clock(num(&item, "duration_s")),
                    web::style()
                        .background("#000000")
                        .color("#ffffff")
                        .size(11)
                        .padding(4)
                        .radius(4),
                ))
                .chain(flags(&item, &format!("tile-{id}"), theme))
                .chain(std::iter::once(web::styled(
                    &format!("tile-{id}-published"),
                    web::text(&item, "published"),
                    web::style().size(11).color(muted(theme)),
                )))
                .collect(),
            ),
            web::styled(
                &format!("tile-{id}-title"),
                web::text(&item, "title"),
                web::style().size(15).medium().color(ink(theme)),
            ),
            web::styled(
                &format!("tile-{id}-channel"),
                channel_name,
                web::style().size(13).color(muted(theme)),
            ),
            web::styled(
                &format!("tile-{id}-views"),
                format!("{} views", grouped(num(&item, "views"))),
                web::style().size(12).color(muted(theme)),
            ),
        ],
    )
}
/// Compact row used by "Up next" and by every list that is not the home grid.
fn item_row(state: &Value, id: &str, prefix: &str, theme: &PageTheme, to: String) -> PageElement {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let channel = web::text(&item, "channel");
    let channel_name = record(state, "channels", &channel)
        .map(|c| web::text(c, "name"))
        .unwrap_or(channel);
    web::card_action(
        &format!("{prefix}-{id}"),
        web::style().padding(6),
        web::visit(to),
        vec![web::styled_row(
            &format!("{prefix}-{id}-row"),
            12,
            "start",
            web::style(),
            vec![
                art(
                    &format!("{prefix}-{id}-art"),
                    &clock(num(&item, "duration_s")),
                    120,
                    68,
                    6,
                ),
                web::styled_row(
                    &format!("{prefix}-{id}-text"),
                    2,
                    "start",
                    web::style().flex(3),
                    vec![
                        web::styled(
                            &format!("{prefix}-{id}-title"),
                            web::text(&item, "title"),
                            web::style().size(13).medium().color(ink(theme)),
                        ),
                        web::styled(
                            &format!("{prefix}-{id}-channel"),
                            format!("{channel_name} · {} views", grouped(num(&item, "views"))),
                            web::style().size(11).color(muted(theme)),
                        ),
                    ],
                ),
            ],
        )],
    )
}
fn comment_block(
    state: &Value,
    item_id: &str,
    actor: &str,
    theme: &PageTheme,
    back: &str,
) -> Vec<PageElement> {
    let item = record(state, "items", item_id)
        .cloned()
        .unwrap_or(Value::Null);
    let comments = item
        .get("comments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = vec![
        web::styled(
            "comments-heading",
            format!("{} Comments", comments.len()),
            web::style().size(16).bold().color(ink(theme)),
        ),
        web::styled_row(
            "comment-compose",
            12,
            "center",
            web::style(),
            vec![
                art("comment-avatar", actor, 40, 40, 20),
                web::form(
                    "comment",
                    &format!("/items/{item_id}/comments"),
                    &[("text", "Add a comment", "")],
                ),
            ],
        ),
    ];
    for comment in comments {
        let cid = web::text(&comment, "id");
        let author = web::text(&comment, "author");
        out.push(web::styled_row(
            &format!("comment-{cid}"),
            12,
            "start",
            web::style().padding(8),
            vec![
                art(&format!("comment-{cid}-avatar"), &author, 36, 36, 18),
                web::styled_row(
                    &format!("comment-{cid}-body"),
                    4,
                    "start",
                    web::style().flex(4),
                    vec![
                        web::styled(
                            &format!("comment-{cid}-author"),
                            format!("@{author}"),
                            web::style().size(13).medium().color(ink(theme)),
                        ),
                        web::styled(
                            &format!("comment-{cid}-text"),
                            web::text(&comment, "text"),
                            web::style().size(13).color(ink(theme)),
                        ),
                    ],
                ),
                pill(
                    &format!("comment-{cid}-like"),
                    &format!("♥ {}", num(&comment, "likes")),
                    false,
                    theme,
                    post(
                        format!("/items/{item_id}/comments/{cid}/like"),
                        &[("return", back)],
                    ),
                ),
            ],
        ));
    }
    out
}
fn home(state: &Value, theme: &PageTheme) -> Result<HttpResponse> {
    let ids = by_recency(state, |_| true);
    let mut tags: Vec<String> = ids
        .iter()
        .filter_map(|id| record(state, "items", id))
        .flat_map(|item| web::strings(item, "tags"))
        .collect();
    tags.sort();
    tags.dedup();
    let chips = tags
        .iter()
        .map(|tag| {
            web::card_action(
                &format!("chip-{tag}"),
                web::style()
                    .padding(8)
                    .radius(14)
                    .background(surface(theme)),
                web::visit(format!("/results?search_query={tag}")),
                vec![web::styled(
                    &format!("chip-{tag}-text"),
                    tag,
                    web::style().size(13).color(ink(theme)),
                )],
            )
        })
        .collect();
    let tiles = ids.iter().map(|id| video_card(state, id, theme)).collect();
    web::themed_page(
        &web::text(state, "brand"),
        theme.clone(),
        vec![
            chrome(state, "video", theme),
            web::styled_row("chips", 8, "center", web::style().padding(12), chips),
            web::grid("home-grid", 3, 20, tiles),
        ],
    )
}
/// The watch page. `list` keeps a playlist queue in the sidebar so playback has somewhere to go.
fn watch(
    state: &Value,
    id: &str,
    list: Option<&str>,
    actor: &str,
    theme: &PageTheme,
) -> Result<HttpResponse> {
    let Some(item) = record(state, "items", id).cloned() else {
        return web::error(404, "video not found");
    };
    let back = match list {
        Some(l) => format!("/watch?v={id}&list={l}"),
        None => format!("/watch?v={id}"),
    };
    let channel_id = web::text(&item, "channel");
    let channel = record(state, "channels", &channel_id)
        .cloned()
        .unwrap_or(Value::Null);
    let subscribed = has(state, "subscriptions", actor, &channel_id);
    let liked = has(state, "likes", actor, id);
    let description = web::text(&item, "description");
    let mut left = vec![
        art("player", &web::text(&item, "title"), 0, 360, 10),
        web::styled_row(
            "watch-heading",
            10,
            "center",
            web::style(),
            std::iter::once(web::styled(
                "watch-title",
                web::text(&item, "title"),
                web::style().size(20).bold().color(ink(theme)).flex(3),
            ))
            .chain(flags(&item, "watch", theme))
            .collect(),
        ),
        web::styled_row(
            "watch-actions",
            12,
            "center",
            web::style(),
            vec![
                art("watch-avatar", &web::text(&channel, "name"), 48, 48, 24),
                web::styled_row(
                    "watch-channel",
                    2,
                    "start",
                    web::style().flex(3),
                    vec![
                        web::link(
                            "watch-channel-name",
                            web::text(&channel, "name"),
                            format!("/channel/{channel_id}"),
                        ),
                        web::styled(
                            "watch-subs",
                            format!("{} subscribers", grouped(num(&channel, "subscribers"))),
                            web::style().size(12).color(muted(theme)),
                        ),
                    ],
                ),
                pill(
                    "watch-subscribe",
                    if subscribed {
                        "Subscribed"
                    } else {
                        "Subscribe"
                    },
                    subscribed,
                    theme,
                    post(
                        format!("/channels/{channel_id}/subscribe"),
                        &[("return", &back)],
                    ),
                ),
                pill(
                    "watch-like",
                    &format!("♥ {}", grouped(num(&item, "likes"))),
                    liked,
                    theme,
                    post(format!("/items/{id}/like"), &[("return", &back)]),
                ),
                pill(
                    "watch-later",
                    "Save",
                    false,
                    theme,
                    post(
                        "/playlists/watch-later/items".into(),
                        &[("item", id), ("return", &back)],
                    ),
                ),
            ],
        ),
        web::card(
            "watch-description",
            web::style()
                .background(surface(theme))
                .padding(12)
                .radius(8),
            std::iter::once(web::styled(
                "watch-stats",
                format!(
                    "{} views · {}",
                    grouped(num(&item, "views")),
                    web::text(&item, "published")
                ),
                web::style().size(13).medium().color(ink(theme)),
            ))
            .chain(std::iter::once(web::paragraph("watch-body", &description)))
            .chain(web::links("watch", &description))
            .collect(),
        ),
        web::divider("watch-divider"),
    ];
    left.extend(comment_block(state, id, actor, theme, &back));
    let queue: Vec<String> = match list.and_then(|l| record(state, "playlists", l).cloned()) {
        Some(playlist) => web::strings(&playlist, "items"),
        None => web::strings(&item, "related"),
    };
    let mut side = vec![web::styled(
        "queue-heading",
        match list {
            Some(l) => record(state, "playlists", l)
                .map(|p| web::text(p, "title"))
                .unwrap_or_else(|| "Up next".into()),
            None => "Up next".into(),
        },
        web::style().size(15).bold().color(ink(theme)),
    )];
    for next in queue.iter().filter(|n| *n != id) {
        let to = match list {
            Some(l) => format!("/watch?v={next}&list={l}"),
            None => format!("/watch?v={next}"),
        };
        side.push(item_row(state, next, "queue", theme, to));
    }
    web::themed_page(
        &web::text(&item, "title"),
        theme.clone(),
        vec![
            chrome(state, "video", theme),
            web::styled_row(
                "watch-layout",
                24,
                "start",
                web::style().padding(16),
                vec![
                    web::column("watch-main", 12, web::style().flex(3), left),
                    web::column("watch-side", 8, web::style().flex(2), side),
                ],
            ),
        ],
    )
}
fn channel_page(state: &Value, id: &str, actor: &str, theme: &PageTheme) -> Result<HttpResponse> {
    let Some(channel) = record(state, "channels", id).cloned() else {
        return web::error(404, "channel not found");
    };
    let name = web::text(&channel, "name");
    let subscribed = has(state, "subscriptions", actor, id);
    let owned = by_recency(state, |item| web::text(item, "channel") == id);
    let video = web::variant(state, "mode", MODES)? == "video";
    let tiles = owned
        .iter()
        .map(|item| {
            if video {
                video_card(state, item, theme)
            } else {
                item_row(state, item, "channel", theme, format!("/track/{item}"))
            }
        })
        .collect();
    web::themed_page(
        &name,
        theme.clone(),
        vec![
            chrome(state, if video { "video" } else { "audio" }, theme),
            art("channel-banner", &name, 0, 120, 10),
            web::styled_row(
                "channel-header",
                16,
                "center",
                web::style().padding(16),
                vec![
                    art("channel-avatar", &name, 80, 80, 40),
                    web::styled_row(
                        "channel-identity",
                        4,
                        "start",
                        web::style().flex(3),
                        vec![
                            web::styled(
                                "channel-name",
                                &name,
                                web::style().size(22).bold().color(ink(theme)),
                            ),
                            web::styled(
                                "channel-handle",
                                format!(
                                    "{} · {} subscribers",
                                    web::text(&channel, "handle"),
                                    grouped(num(&channel, "subscribers"))
                                ),
                                web::style().size(13).color(muted(theme)),
                            ),
                            web::styled(
                                "channel-about",
                                web::text(&channel, "about"),
                                web::style().size(13).color(muted(theme)),
                            ),
                        ],
                    ),
                    pill(
                        "channel-subscribe",
                        if subscribed {
                            "Subscribed"
                        } else {
                            "Subscribe"
                        },
                        subscribed,
                        theme,
                        post(
                            format!("/channels/{id}/subscribe"),
                            &[("return", &format!("/channel/{id}"))],
                        ),
                    ),
                ],
            ),
            web::divider("channel-divider"),
            if video {
                web::grid("channel-grid", 3, 20, tiles)
            } else {
                web::styled_row("channel-list", 8, "start", web::style().padding(12), tiles)
            },
        ],
    )
}
/// Title, description, tags and channel name all match, so a search is worth typing.
fn matches(state: &Value, id: &str, needle: &str) -> bool {
    let Some(item) = record(state, "items", id) else {
        return false;
    };
    let channel = record(state, "channels", &web::text(item, "channel"))
        .map(|c| web::text(c, "name"))
        .unwrap_or_default();
    let hay = format!(
        "{} {} {} {} {}",
        web::text(item, "title"),
        web::text(item, "description"),
        web::text(item, "album"),
        web::strings(item, "tags").join(" "),
        channel
    )
    .to_lowercase();
    needle
        .split_whitespace()
        .all(|word| hay.contains(&word.to_lowercase()))
}
fn results(state: &Value, query: &str, theme: &PageTheme, video: bool) -> Result<HttpResponse> {
    let hits = by_recency(state, |_| true)
        .into_iter()
        .filter(|id| matches(state, id, query))
        .collect::<Vec<_>>();
    let mut elements = vec![
        chrome(state, if video { "video" } else { "audio" }, theme),
        web::styled(
            "results-heading",
            format!("{} results for \"{query}\"", hits.len()),
            web::style().size(16).bold().color(ink(theme)).padding(12),
        ),
    ];
    for id in &hits {
        let to = if video {
            format!("/watch?v={id}")
        } else {
            format!("/track/{id}")
        };
        elements.push(item_row(state, id, "result", theme, to));
    }
    for channel in keys(state, "channels") {
        let name = record(state, "channels", &channel)
            .map(|c| web::text(c, "name"))
            .unwrap_or_default();
        if !name.to_lowercase().contains(&query.to_lowercase()) {
            continue;
        }
        elements.push(web::card_action(
            &format!("result-channel-{channel}"),
            web::style().padding(8),
            web::visit(format!("/channel/{channel}")),
            vec![web::styled_row(
                &format!("result-channel-{channel}-row"),
                12,
                "center",
                web::style(),
                vec![
                    art(&format!("result-channel-{channel}-art"), &name, 56, 56, 28),
                    web::styled(
                        &format!("result-channel-{channel}-name"),
                        name,
                        web::style().size(14).medium().color(ink(theme)),
                    ),
                ],
            )],
        ));
    }
    web::themed_page(&format!("{query} - search"), theme.clone(), elements)
}
fn playlist_page(
    state: &Value,
    id: &str,
    actor: &str,
    theme: &PageTheme,
    video: bool,
) -> Result<HttpResponse> {
    let Some(playlist) = record(state, "playlists", id).cloned() else {
        return web::error(404, "playlist not found");
    };
    let tracks = web::strings(&playlist, "items");
    let title = web::text(&playlist, "title");
    let owner = match web::text(&playlist, "owner") {
        o if o.is_empty() => "everyone".to_owned(),
        o => o,
    };
    let mut elements = vec![
        chrome(state, if video { "video" } else { "audio" }, theme),
        web::styled_row(
            "playlist-header",
            20,
            "center",
            web::style().padding(16),
            vec![
                art("playlist-art", &title, 180, 180, 8),
                web::styled_row(
                    "playlist-identity",
                    6,
                    "start",
                    web::style().flex(3),
                    vec![
                        web::styled(
                            "playlist-title",
                            &title,
                            web::style().size(28).bold().color(ink(theme)),
                        ),
                        web::styled(
                            "playlist-meta",
                            format!("{owner} · {} tracks", tracks.len()),
                            web::style().size(13).color(muted(theme)),
                        ),
                        match tracks.first() {
                            Some(first) if video => web::card_action(
                                "playlist-play",
                                web::style().padding(10).radius(18).background(
                                    theme.accent.clone().unwrap_or_else(|| "#ff0000".into()),
                                ),
                                web::visit(format!("/watch?v={first}&list={id}")),
                                vec![web::styled(
                                    "playlist-play-text",
                                    "Play all",
                                    web::style().size(13).medium().color("#ffffff"),
                                )],
                            ),
                            Some(first) => pill(
                                "playlist-play",
                                "Play",
                                true,
                                theme,
                                post(
                                    format!("/items/{first}/play"),
                                    &[("list", id), ("return", &format!("/playlist/{id}"))],
                                ),
                            ),
                            None => web::styled(
                                "playlist-play",
                                "Nothing added yet",
                                web::style().size(13).color(muted(theme)),
                            ),
                        },
                    ],
                ),
            ],
        ),
        web::divider("playlist-divider"),
    ];
    for (index, track) in tracks.iter().enumerate() {
        let to = if video {
            format!("/watch?v={track}&list={id}")
        } else {
            format!("/track/{track}")
        };
        elements.push(web::styled_row(
            &format!("playlist-line-{index}"),
            12,
            "center",
            web::style(),
            vec![
                web::styled(
                    &format!("playlist-index-{index}"),
                    format!("{}", index + 1),
                    web::style().size(13).color(muted(theme)).width(28),
                ),
                item_row(state, track, "playlist", theme, to),
            ],
        ));
    }
    elements.push(web::divider("playlist-foot"));
    elements.push(track_bar(state, actor, theme));
    web::themed_page(&title, theme.clone(), elements)
}
/// Library: every playlist plus the form that creates one, which is what makes them buildable.
fn library(state: &Value, theme: &PageTheme, video: bool) -> Result<HttpResponse> {
    let cards = keys(state, "playlists")
        .iter()
        .map(|id| {
            let playlist = record(state, "playlists", id)
                .cloned()
                .unwrap_or(Value::Null);
            let title = web::text(&playlist, "title");
            web::card_action(
                &format!("library-{id}"),
                web::style().padding(8),
                web::visit(if video {
                    format!("/playlist?list={id}")
                } else {
                    format!("/playlist/{id}")
                }),
                vec![
                    art(&format!("library-{id}-art"), &title, 0, 140, 8),
                    web::styled(
                        &format!("library-{id}-title"),
                        title,
                        web::style().size(14).medium().color(ink(theme)),
                    ),
                    web::styled(
                        &format!("library-{id}-count"),
                        format!("{} tracks", web::strings(&playlist, "items").len()),
                        web::style().size(12).color(muted(theme)),
                    ),
                ],
            )
        })
        .collect();
    web::themed_page(
        "Library",
        theme.clone(),
        vec![
            chrome(state, if video { "video" } else { "audio" }, theme),
            web::styled(
                "library-heading",
                "Your playlists",
                web::style().size(20).bold().color(ink(theme)).padding(12),
            ),
            web::grid("library-grid", 3, 16, cards),
            web::divider("library-divider"),
            web::form("playlist", "/playlists", &[("title", "New playlist", "")]),
        ],
    )
}
/// The persistent now-playing bar. Inert text plus one real link to whatever is parked there.
fn track_bar(state: &Value, actor: &str, theme: &PageTheme) -> PageElement {
    let Some(playing) = record(state, "now_playing", actor).cloned() else {
        return web::styled(
            "bar",
            "Nothing playing",
            web::style().size(12).color(muted(theme)).padding(12),
        );
    };
    let id = web::text(&playing, "item");
    let item = record(state, "items", &id).cloned().unwrap_or(Value::Null);
    web::styled_row(
        "bar",
        12,
        "center",
        web::style().background(surface(theme)).padding(12),
        vec![
            art("bar-art", &web::text(&item, "title"), 48, 48, 4),
            web::link(
                "bar-title",
                web::text(&item, "title"),
                format!("/track/{id}"),
            ),
            web::styled(
                "bar-meta",
                format!(
                    "{} · {} plays",
                    web::text(&item, "album"),
                    grouped(num(&item, "plays"))
                ),
                web::style().size(12).color(muted(theme)),
            ),
        ],
    )
}
fn browse(state: &Value, actor: &str, theme: &PageTheme) -> Result<HttpResponse> {
    let artists = keys(state, "channels")
        .iter()
        .map(|id| {
            let artist = record(state, "channels", id)
                .cloned()
                .unwrap_or(Value::Null);
            let name = web::text(&artist, "name");
            web::card_action(
                &format!("artist-{id}"),
                web::style()
                    .padding(12)
                    .background(surface(theme))
                    .radius(8),
                web::visit(format!("/artist/{id}")),
                vec![
                    art(&format!("artist-{id}-art"), &name, 140, 140, 70),
                    web::styled(
                        &format!("artist-{id}-name"),
                        name,
                        web::style().size(15).medium().color(ink(theme)),
                    ),
                    web::styled(
                        &format!("artist-{id}-meta"),
                        format!("{} followers", grouped(num(&artist, "subscribers"))),
                        web::style().size(12).color(muted(theme)),
                    ),
                ],
            )
        })
        .collect();
    let mut side = vec![web::styled(
        "side-heading",
        "Your library",
        web::style().size(14).bold().color(ink(theme)),
    )];
    for id in keys(state, "playlists") {
        let playlist = record(state, "playlists", &id)
            .cloned()
            .unwrap_or(Value::Null);
        side.push(web::link(
            &format!("side-{id}"),
            web::text(&playlist, "title"),
            format!("/playlist/{id}"),
        ));
    }
    side.push(web::form(
        "playlist",
        "/playlists",
        &[("title", "New playlist", "")],
    ));
    let tracks = by_recency(state, |_| true)
        .iter()
        .map(|id| track_line(state, id, actor, theme))
        .collect();
    web::themed_page(
        &web::text(state, "brand"),
        theme.clone(),
        vec![
            chrome(state, "audio", theme),
            web::styled_row(
                "browse-layout",
                24,
                "start",
                web::style().padding(16),
                vec![
                    web::styled_row(
                        "browse-side",
                        8,
                        "start",
                        web::style()
                            .flex(1)
                            .background(surface(theme))
                            .padding(12)
                            .radius(8),
                        side,
                    ),
                    web::styled_row(
                        "browse-main",
                        16,
                        "start",
                        web::style().flex(3),
                        vec![
                            web::styled(
                                "browse-heading",
                                "Popular artists",
                                web::style().size(20).bold().color(ink(theme)),
                            ),
                            web::grid("browse-artists", 3, 16, artists),
                            web::styled(
                                "browse-tracks-heading",
                                "Tracks",
                                web::style().size(20).bold().color(ink(theme)),
                            ),
                            web::styled_row("browse-tracks", 4, "start", web::style(), tracks),
                        ],
                    ),
                ],
            ),
            track_bar(state, actor, theme),
        ],
    )
}
/// One line of the track table: open, play and like are three separate, real controls.
fn track_line(state: &Value, id: &str, actor: &str, theme: &PageTheme) -> PageElement {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let artist = record(state, "channels", &web::text(&item, "channel"))
        .map(|c| web::text(c, "name"))
        .unwrap_or_default();
    web::styled_row(
        &format!("line-{id}"),
        12,
        "center",
        web::style().padding(6),
        vec![
            web::card_action(
                &format!("line-{id}-open"),
                web::style().flex(3),
                web::visit(format!("/track/{id}")),
                vec![
                    web::styled(
                        &format!("line-{id}-title"),
                        web::text(&item, "title"),
                        web::style().size(14).medium().color(ink(theme)).one_line(),
                    ),
                    web::styled(
                        &format!("line-{id}-artist"),
                        format!("{artist} · {}", web::text(&item, "album")),
                        web::style().size(12).color(muted(theme)).one_line(),
                    ),
                ],
            ),
            pill(
                &format!("line-{id}-play"),
                "Play",
                false,
                theme,
                post(format!("/items/{id}/play"), &[("return", "/")]),
            ),
            pill(
                &format!("line-{id}-like"),
                "♥",
                has(state, "likes", actor, id),
                theme,
                post(format!("/items/{id}/like"), &[("return", "/")]),
            ),
            web::badge(
                &format!("line-{id}-duration"),
                clock(num(&item, "duration_s")),
                web::style().size(11).color(muted(theme)).padding(4),
            ),
        ],
    )
}
fn track_page(state: &Value, id: &str, actor: &str, theme: &PageTheme) -> Result<HttpResponse> {
    let Some(item) = record(state, "items", id).cloned() else {
        return web::error(404, "track not found");
    };
    let artist_id = web::text(&item, "channel");
    let artist = record(state, "channels", &artist_id)
        .cloned()
        .unwrap_or(Value::Null);
    let back = format!("/track/{id}");
    let title = web::text(&item, "title");
    web::themed_page(
        &title,
        theme.clone(),
        vec![
            chrome(state, "audio", theme),
            web::styled_row(
                "track-header",
                24,
                "center",
                web::style().padding(16),
                vec![
                    art("track-art", &title, 200, 200, 6),
                    web::styled_row(
                        "track-identity",
                        8,
                        "start",
                        web::style().flex(3),
                        vec![
                            web::styled(
                                "track-title",
                                &title,
                                web::style().size(30).bold().color(ink(theme)),
                            ),
                            web::link(
                                "track-artist",
                                web::text(&artist, "name"),
                                format!("/artist/{artist_id}"),
                            ),
                            web::styled(
                                "track-meta",
                                format!(
                                    "{} · {} · {} plays",
                                    web::text(&item, "album"),
                                    clock(num(&item, "duration_s")),
                                    grouped(num(&item, "plays"))
                                ),
                                web::style().size(13).color(muted(theme)),
                            ),
                            web::styled_row(
                                "track-actions",
                                12,
                                "center",
                                web::style(),
                                vec![
                                    pill(
                                        "track-play",
                                        "Play",
                                        true,
                                        theme,
                                        post(format!("/items/{id}/play"), &[("return", &back)]),
                                    ),
                                    pill(
                                        "track-like",
                                        &format!("♥ {}", grouped(num(&item, "likes"))),
                                        has(state, "likes", actor, id),
                                        theme,
                                        post(format!("/items/{id}/like"), &[("return", &back)]),
                                    ),
                                    pill(
                                        "track-save",
                                        "Add to playlist",
                                        false,
                                        theme,
                                        post(
                                            "/playlists/liked/items".into(),
                                            &[("item", id), ("return", &back)],
                                        ),
                                    ),
                                ],
                            ),
                        ],
                    ),
                ],
            ),
            track_bar(state, actor, theme),
        ],
    )
}
/// Brand splash; nothing on it reads as a control, so no action is promised that does not exist.
fn landing(state: &Value) -> Result<HttpResponse> {
    let pick = |key: &str, fallback: &str| match web::text(state, key) {
        s if s.is_empty() => fallback.to_owned(),
        s => s,
    };
    web::brand_page(
        &pick("brand", BRAND),
        &pick("tagline", TAGLINE),
        web::theme(state)?,
    )
}
fn item_mut<'a>(state: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    state.get_mut("items")?.as_object_mut()?.get_mut(id)
}
fn bump(state: &mut Value, id: &str, key: &str, delta: i64) {
    if let Some(item) = item_mut(state, id) {
        let now = num(item, key) as i64 + delta;
        item[key] = json!(now.max(0));
    }
}
/// Ids are dense and never recycled, so the next one is simply past the highest in use.
fn next_comment_id(comments: &[Value]) -> String {
    let highest = comments
        .iter()
        .filter_map(|c| web::text(c, "id").strip_prefix('c')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("c{}", highest + 1)
}
fn slug(title: &str) -> String {
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
/// Every GET route. `count` is false when a mutation re-renders its own page, so liking a video
/// does not also count as watching it again.
fn render(
    state: &mut Value,
    ctx: &ServiceContext,
    request: &HttpRequest,
    count: bool,
) -> Result<HttpResponse> {
    let theme = web::theme(state)?;
    let video = web::variant(state, "mode", MODES)? == "video";
    let path = web::path(request);
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        [""] if state.get("items").is_none() => landing(state),
        [""] if video => home(state, &theme),
        [""] => browse(state, &ctx.actor, &theme),
        ["watch"] => {
            let Some(id) = web::query(request, "v") else {
                return web::error(404, "video not found");
            };
            let list = web::query(request, "list");
            if count {
                play(state, &id, list.as_deref(), ctx, true);
            }
            watch(state, &id, list.as_deref(), &ctx.actor, &theme)
        }
        ["results"] => results(
            state,
            &web::query(request, "search_query").unwrap_or_default(),
            &theme,
            video,
        ),
        ["search"] => results(
            state,
            &web::query(request, "q").unwrap_or_default(),
            &theme,
            video,
        ),
        ["channel", id] | ["artist", id] => channel_page(state, id, &ctx.actor, &theme),
        ["track", id] => track_page(state, id, &ctx.actor, &theme),
        ["playlists"] => library(state, &theme, video),
        ["playlist"] => match web::query(request, "list") {
            Some(id) => playlist_page(state, &id, &ctx.actor, &theme, video),
            None => library(state, &theme, video),
        },
        ["playlist", id] => playlist_page(state, id, &ctx.actor, &theme, video),
        // youtu.be hands us the bare video id and nothing else.
        [id] if video && record(state, "items", id).is_some() => {
            if count {
                play(state, id, None, ctx, true);
            }
            watch(state, id, None, &ctx.actor, &theme)
        }
        _ => web::error(404, "route not found"),
    }
}

impl Service for MediaService {
    fn kind(&self) -> &str {
        "media"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let state = web::shape(initial, OBJECTS, ARRAYS)?;
        web::variant(&state, "mode", MODES)?;
        web::theme(&state)?;
        Ok(state)
    }
    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        request: &HttpRequest,
    ) -> Result<HttpResponse> {
        let theme = web::theme(state)?;
        let video = web::variant(state, "mode", MODES)? == "video";
        let path = web::path(request);
        let method = request.method.to_ascii_uppercase();
        if method == "GET" {
            return render(state, ctx, request, true);
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let body = web::body(request)?;
        let api = path.starts_with("/api/");
        let route: Vec<&str> = path
            .trim_matches('/')
            .strip_prefix("api/")
            .unwrap_or(path.trim_matches('/'))
            .split('/')
            .collect();
        let result: std::result::Result<(Value, String), String> = match route.as_slice() {
            ["items", id, "view"] => {
                if record(state, "items", id).is_none() {
                    Err("item not found".into())
                } else {
                    play(state, id, None, ctx, true);
                    Ok((
                        json!({"views": num(&state["items"][id], "views")}),
                        format!("/watch?v={id}"),
                    ))
                }
            }
            ["items", id, "play"] => {
                if record(state, "items", id).is_none() {
                    Err("item not found".into())
                } else {
                    let list = web::text(&body, "list");
                    play(
                        state,
                        id,
                        (!list.is_empty()).then_some(list.as_str()),
                        ctx,
                        false,
                    );
                    Ok((
                        json!({"plays": num(&state["items"][id], "plays")}),
                        format!("/track/{id}"),
                    ))
                }
            }
            ["items", id, "like"] => match record(state, "items", id) {
                None => Err("item not found".into()),
                Some(_) => {
                    let liked = toggle(state, "likes", &ctx.actor, id);
                    bump(state, id, "likes", if liked { 1 } else { -1 });
                    Ok((json!({"liked": liked}), format!("/watch?v={id}")))
                }
            },
            ["items", id, "comments"] => {
                let text = web::text(&body, "text");
                if text.trim().is_empty() {
                    Err("comment text is required".into())
                } else if let Some(item) = item_mut(state, id) {
                    let comments = item
                        .get("comments")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let comment = json!({
                        "id": next_comment_id(&comments),
                        "author": ctx.actor,
                        "text": text,
                        "tick": ctx.tick,
                        "likes": 0,
                    });
                    let mut comments = comments;
                    comments.push(comment.clone());
                    item["comments"] = Value::Array(comments);
                    Ok((comment, format!("/watch?v={id}")))
                } else {
                    Err("item not found".into())
                }
            }
            ["items", id, "comments", comment, "like"] => match item_mut(state, id) {
                None => Err("item not found".into()),
                Some(item) => {
                    let found = item
                        .get_mut("comments")
                        .and_then(Value::as_array_mut)
                        .and_then(|list| list.iter_mut().find(|c| web::text(c, "id") == *comment));
                    match found {
                        None => Err("comment not found".into()),
                        Some(c) => {
                            c["likes"] = json!(num(c, "likes") + 1);
                            Ok((c.clone(), format!("/watch?v={id}")))
                        }
                    }
                }
            },
            ["channels", id, "subscribe"] => match record(state, "channels", id) {
                None => Err("channel not found".into()),
                Some(_) => {
                    let on = toggle(state, "subscriptions", &ctx.actor, id);
                    let delta = if on { 1 } else { -1 };
                    if let Some(channel) = state.get_mut("channels").and_then(|c| c.get_mut(id)) {
                        let subs = num(channel, "subscribers") as i64 + delta;
                        channel["subscribers"] = json!(subs.max(0));
                    }
                    Ok((json!({"subscribed": on}), format!("/channel/{id}")))
                }
            },
            ["playlists"] => {
                let title = web::text(&body, "title");
                let id = slug(&title);
                if id.is_empty() {
                    Err("playlist title is required".into())
                } else if record(state, "playlists", &id).is_some() {
                    Err("conflict: playlist already exists".into())
                } else {
                    let playlist =
                        json!({"id": id, "title": title, "owner": ctx.actor, "items": []});
                    state
                        .as_object_mut()
                        .expect("state is an object")
                        .entry("playlists")
                        .or_insert_with(|| json!({}))[&id] = playlist.clone();
                    Ok((playlist, format!("/playlist/{id}")))
                }
            }
            ["playlists", id, "items"] => {
                let item = web::text(&body, "item");
                match record(state, "playlists", id).cloned() {
                    None => Err("playlist not found".into()),
                    Some(playlist) => {
                        let owner = web::text(&playlist, "owner");
                        if !owner.is_empty() && owner != ctx.actor {
                            Err(format!("playlist {id} is not writable by {}", ctx.actor))
                        } else if record(state, "items", &item).is_none() {
                            Err("item not found".into())
                        } else {
                            let mut tracks = web::strings(&playlist, "items");
                            if !tracks.contains(&item) {
                                tracks.push(item);
                            }
                            state["playlists"][id]["items"] = json!(tracks);
                            Ok((state["playlists"][id].clone(), format!("/playlist/{id}")))
                        }
                    }
                }
            }
            // The chrome search box is a form, so searching arrives as a POST.
            ["results"] | ["search"] => {
                let query = match web::text(&body, "search_query") {
                    q if q.is_empty() => web::text(&body, "q"),
                    q => q,
                };
                return results(state, &query, &theme, video);
            }
            _ => return web::error(404, "route not found"),
        };
        match result {
            Err(message) => web::domain::<Value>(Err(message)),
            Ok((value, _)) if api => HttpResponse::json(200, &value),
            // A browser control says where it came from, so a mutation lands back on that page.
            Ok((_, fallback)) => {
                let back = match web::text(&body, "return") {
                    r if r.is_empty() => fallback,
                    r => r,
                };
                render(
                    state,
                    ctx,
                    &HttpRequest::get(format!("http://media{back}")),
                    false,
                )
            }
        }
    }
}
/// Playback bookkeeping shared by the watch page and the explicit play control.
fn play(state: &mut Value, id: &str, list: Option<&str>, ctx: &ServiceContext, view: bool) {
    if record(state, "items", id).is_none() {
        return;
    }
    bump(state, id, if view { "views" } else { "plays" }, 1);
    let mut playing = Map::new();
    playing.insert("item".into(), json!(id));
    playing.insert("tick".into(), json!(ctx.tick));
    if let Some(list) = list {
        playing.insert("list".into(), json!(list));
    }
    state
        .as_object_mut()
        .expect("state is an object")
        .entry("now_playing")
        .or_insert_with(|| json!({}))[&ctx.actor] = Value::Object(playing);
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "alice".into(),
            source: "alice-mac".into(),
            tick: 12,
            seed: 1,
            instance: "media".into(),
        }
    }
    fn video_seed() -> Value {
        json!({
            "mode": "video", "brand": "Testtube",
            "theme": {"accent": "#ff0000", "background": "#0f0f0f", "surface": "#212121",
                      "ink": "#f1f1f1", "muted": "#aaaaaa"},
            "channels": {
                "alice-builds": {"id": "alice-builds", "name": "Alice Builds",
                                 "handle": "@alicebuilds", "owner": "alice", "subscribers": 12400,
                                 "about": "Deterministic systems, slowly."},
                "carol-ships": {"id": "carol-ships", "name": "Carol Ships It",
                                "handle": "@carolships", "owner": "carol", "subscribers": 3120}
            },
            "items": {
                "atlas-walkthrough": {
                    "id": "atlas-walkthrough", "channel": "alice-builds",
                    "title": "Atlas 1.0 walkthrough", "description": "Repo: http://github.com/northstar/atlas",
                    "duration_s": 742, "published": "Mar 4, 2026", "published_tick": 7,
                    "views": 18342, "likes": 903, "tags": ["atlas", "determinism"],
                    "badges": ["4K"],
                    "comments": [{"id": "c1", "author": "bob", "text": "Clicked at 6:20.",
                                  "tick": 7, "likes": 41}],
                    "related": ["carol-postmortem"]
                },
                "carol-postmortem": {
                    "id": "carol-postmortem", "channel": "carol-ships",
                    "title": "Release postmortem", "description": "Twelve minutes.",
                    "duration_s": 1264, "published": "Feb 20, 2026", "published_tick": 5,
                    "views": 6031, "likes": 288, "tags": ["release"], "badges": ["LIVE"],
                    "comments": [], "related": ["atlas-walkthrough"]
                }
            },
            "playlists": {
                "watch-later": {"id": "watch-later", "title": "Watch later", "owner": null, "items": []},
                "kit": {"id": "kit", "title": "Kit", "owner": "carol", "items": ["carol-postmortem"]}
            },
            "subscriptions": {"bob": ["alice-builds"]},
            "likes": {"bob": ["atlas-walkthrough"]},
            "now_playing": {}
        })
    }
    fn audio_seed() -> Value {
        json!({
            "mode": "audio", "brand": "Testify",
            "theme": {"accent": "#1db954", "background": "#121212", "surface": "#181818",
                      "ink": "#ffffff", "muted": "#b3b3b3"},
            "channels": {"low-latency": {"id": "low-latency", "name": "Low Latency",
                                         "subscribers": 911403, "about": "Drum and bass."}},
            "items": {
                "backpressure": {"id": "backpressure", "channel": "low-latency",
                                 "title": "Backpressure", "album": "Backpressure",
                                 "duration_s": 256, "published_tick": 6, "plays": 1204338,
                                 "likes": 62114, "tags": ["electronic"]},
                "p99": {"id": "p99", "channel": "low-latency", "title": "P99",
                        "album": "Backpressure", "duration_s": 223, "published_tick": 8,
                        "plays": 489201, "likes": 19844, "tags": ["focus"]}
            },
            "playlists": {
                "liked": {"id": "liked", "title": "Liked Songs", "owner": null, "items": []},
                "ship-it": {"id": "ship-it", "title": "Ship It", "owner": "carol",
                            "items": ["backpressure", "p99"]}
            },
            "subscriptions": {}, "likes": {}, "now_playing": {}
        })
    }
    fn get(state: &mut Value, url: &str) -> HttpResponse {
        MediaService
            .handle(state, &ctx(), &HttpRequest::get(url))
            .unwrap()
    }
    fn post(state: &mut Value, url: &str, body: Value) -> HttpResponse {
        let request = HttpRequest::json("POST", url, &body).unwrap();
        MediaService.handle(state, &ctx(), &request).unwrap()
    }
    fn text(response: &HttpResponse) -> String {
        String::from_utf8(response.body.clone()).unwrap()
    }
    #[test]
    fn seed_shape_is_gated_at_load() {
        assert!(MediaService.initialize(json!([]), &ctx()).is_err());
        assert!(MediaService
            .initialize(json!({"mode": "video", "items": []}), &ctx())
            .is_err());
        assert!(MediaService
            .initialize(json!({"mode": "cinema"}), &ctx())
            .is_err());
        assert!(MediaService
            .initialize(json!({"mode": "video"}), &ctx())
            .is_ok());
    }
    #[test]
    fn home_grid_carries_a_card_per_video_with_its_duration_and_flags() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let page = text(&get(&mut state, "http://youtube.com/"));
        assert!(page.contains("tile-atlas-walkthrough"));
        assert!(page.contains("tile-carol-postmortem"));
        assert!(page.contains("12:22"), "duration is formatted as a clock");
        assert!(page.contains("18,342 views"), "counts are grouped");
        assert!(page.contains("tile-atlas-walkthrough-flag-4k"));
        assert!(page.contains("tile-carol-postmortem-flag-live"));
    }
    #[test]
    fn watching_counts_a_view_and_parks_now_playing() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        assert_eq!(
            get(&mut state, "http://youtube.com/watch?v=atlas-walkthrough").status,
            200
        );
        assert_eq!(state["items"]["atlas-walkthrough"]["views"], json!(18343));
        assert_eq!(
            state["now_playing"]["alice"]["item"],
            json!("atlas-walkthrough")
        );
        assert_eq!(state["now_playing"]["alice"]["tick"], json!(12));
        // youtu.be hands over the bare id, and it counts exactly the same.
        assert_eq!(
            get(&mut state, "http://youtu.be/atlas-walkthrough").status,
            200
        );
        assert_eq!(state["items"]["atlas-walkthrough"]["views"], json!(18344));
        assert_eq!(get(&mut state, "http://youtu.be/no-such-video").status, 404);
    }
    #[test]
    fn liking_is_a_toggle_that_moves_the_counter_both_ways() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let on = post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/like",
            json!({}),
        );
        assert_eq!(text(&on), r#"{"liked":true}"#);
        assert_eq!(state["items"]["atlas-walkthrough"]["likes"], json!(904));
        assert_eq!(state["likes"]["alice"], json!(["atlas-walkthrough"]));
        post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/like",
            json!({}),
        );
        assert_eq!(state["items"]["atlas-walkthrough"]["likes"], json!(903));
        assert_eq!(state["likes"]["alice"], json!([]));
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/items/nope/like",
                json!({})
            )
            .status,
            400
        );
    }
    #[test]
    fn commenting_appends_a_dense_id_and_refuses_an_empty_body() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let made = post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/comments",
            json!({"text": "The replay bit is the good part."}),
        );
        assert_eq!(made.status, 200);
        let comments = state["items"]["atlas-walkthrough"]["comments"].clone();
        assert_eq!(comments[1]["id"], json!("c2"));
        assert_eq!(comments[1]["author"], json!("alice"));
        assert_eq!(comments[1]["tick"], json!(12));
        let liked = post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/comments/c2/like",
            json!({}),
        );
        assert_eq!(liked.status, 200);
        assert_eq!(
            state["items"]["atlas-walkthrough"]["comments"][1]["likes"],
            json!(1)
        );
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/items/atlas-walkthrough/comments",
                json!({"text": "   "})
            )
            .status,
            400
        );
    }
    #[test]
    fn subscribing_toggles_the_membership_and_the_channel_count() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        post(
            &mut state,
            "http://youtube.com/api/channels/alice-builds/subscribe",
            json!({}),
        );
        assert_eq!(
            state["channels"]["alice-builds"]["subscribers"],
            json!(12401)
        );
        assert_eq!(state["subscriptions"]["alice"], json!(["alice-builds"]));
        post(
            &mut state,
            "http://youtube.com/api/channels/alice-builds/subscribe",
            json!({}),
        );
        assert_eq!(
            state["channels"]["alice-builds"]["subscribers"],
            json!(12400)
        );
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/channels/ghost/subscribe",
                json!({})
            )
            .status,
            400
        );
    }
    #[test]
    fn a_playlist_can_be_built_and_played_but_not_by_a_stranger() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        // The watch page's Save control targets the shared list, which has no owner.
        post(
            &mut state,
            "http://youtube.com/api/playlists/watch-later/items",
            json!({"item": "atlas-walkthrough"}),
        );
        assert_eq!(
            state["playlists"]["watch-later"]["items"],
            json!(["atlas-walkthrough"])
        );
        let made = post(
            &mut state,
            "http://youtube.com/api/playlists",
            json!({"title": "Launch Kit"}),
        );
        assert_eq!(made.status, 200);
        assert_eq!(state["playlists"]["launch-kit"]["owner"], json!("alice"));
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/playlists",
                json!({"title": "Launch Kit"})
            )
            .status,
            409
        );
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/playlists/kit/items",
                json!({"item": "atlas-walkthrough"})
            )
            .status,
            403,
            "carol's playlist is not alice's to edit"
        );
        assert_eq!(
            post(
                &mut state,
                "http://youtube.com/api/playlists/watch-later/items",
                json!({"item": "ghost"})
            )
            .status,
            400
        );
        let page = text(&get(
            &mut state,
            "http://youtube.com/playlist?list=watch-later",
        ));
        assert!(
            page.contains("playlist-play"),
            "a built playlist offers Play all"
        );
    }
    #[test]
    fn a_browser_control_lands_back_on_the_page_it_was_pressed_from() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let back = post(
            &mut state,
            "http://youtube.com/items/atlas-walkthrough/like",
            json!({"return": "/watch?v=atlas-walkthrough"}),
        );
        assert_eq!(back.status, 200);
        assert!(
            text(&back).contains("watch-title"),
            "it re-renders the watch page"
        );
        assert_eq!(
            state["items"]["atlas-walkthrough"]["views"],
            json!(18342),
            "a re-render after a mutation must not count a second view"
        );
    }
    #[test]
    fn search_finds_a_video_by_tag_and_a_channel_by_name() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        let hits = text(&get(
            &mut state,
            "http://youtube.com/results?search_query=determinism",
        ));
        assert!(hits.contains("1 results"));
        assert!(hits.contains("result-atlas-walkthrough"));
        let channels = text(&get(
            &mut state,
            "http://youtube.com/results?search_query=Carol",
        ));
        assert!(channels.contains("result-channel-carol-ships"));
        // The chrome search box is a form, so the same query also arrives as a POST.
        let posted = post(
            &mut state,
            "http://youtube.com/results",
            json!({"search_query": "atlas"}),
        );
        assert!(text(&posted).contains("result-atlas-walkthrough"));
    }
    #[test]
    fn audio_mode_plays_a_track_and_keeps_the_now_playing_bar() {
        let mut state = MediaService.initialize(audio_seed(), &ctx()).unwrap();
        let browse = text(&get(&mut state, "http://spotify.com/"));
        assert!(browse.contains("artist-low-latency"));
        assert!(browse.contains("Nothing playing"));
        let played = post(
            &mut state,
            "http://spotify.com/api/items/backpressure/play",
            json!({"list": "ship-it"}),
        );
        assert_eq!(text(&played), r#"{"plays":1204339}"#);
        assert_eq!(state["now_playing"]["alice"]["list"], json!("ship-it"));
        let bar = text(&get(&mut state, "http://spotify.com/"));
        assert!(
            bar.contains("bar-title"),
            "the bar now points at the parked track"
        );
        assert_eq!(get(&mut state, "http://spotify.com/track/p99").status, 200);
        assert_eq!(
            get(&mut state, "http://spotify.com/artist/low-latency").status,
            200
        );
        assert_eq!(
            get(&mut state, "http://spotify.com/playlist/ship-it").status,
            200
        );
        assert_eq!(
            get(&mut state, "http://spotify.com/track/ghost").status,
            404
        );
    }
    #[test]
    fn reading_pages_is_pure_and_unknown_routes_are_refused() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        for url in [
            "http://youtube.com/",
            "http://youtube.com/channel/alice-builds",
            "http://youtube.com/playlists",
            "http://youtube.com/results?search_query=atlas",
        ] {
            let page = get(&mut state, url);
            assert_eq!(page.status, 200, "{url}");
            assert_eq!(page, get(&mut state, url), "{url} must be pure");
        }
        assert_eq!(
            get(&mut state, "http://youtube.com/settings/billing").status,
            404
        );
        assert_eq!(
            get(&mut state, "http://youtube.com/channel/ghost").status,
            404
        );
        let mut head = HttpRequest::get("http://youtube.com/");
        head.method = "DELETE".into();
        assert_eq!(
            MediaService
                .handle(&mut state, &ctx(), &head)
                .unwrap()
                .status,
            405
        );
    }
    #[test]
    fn everything_a_viewer_does_survives_a_snapshot_round_trip() {
        let mut state = MediaService.initialize(video_seed(), &ctx()).unwrap();
        get(&mut state, "http://youtube.com/watch?v=atlas-walkthrough");
        post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/like",
            json!({}),
        );
        post(
            &mut state,
            "http://youtube.com/api/channels/alice-builds/subscribe",
            json!({}),
        );
        post(
            &mut state,
            "http://youtube.com/api/items/atlas-walkthrough/comments",
            json!({"text": "Saved for later."}),
        );
        post(
            &mut state,
            "http://youtube.com/api/playlists/watch-later/items",
            json!({"item": "atlas-walkthrough"}),
        );
        let bytes = serde_json::to_vec(&state).unwrap();
        let mut restored: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, state);
        assert_eq!(
            text(&get(
                &mut restored,
                "http://youtube.com/playlist?list=watch-later"
            )),
            text(&get(
                &mut state,
                "http://youtube.com/playlist?list=watch-later"
            ))
        );
    }
}
