//! spotify.com in `audio` mode, laid out like Spotify's web player: a black frame holding a
//! "Your Library" panel and a main panel, a top bar with Home and search, and the player bar
//! pinned to the bottom edge with shuffle, previous, play, next, repeat and a scrubber.
use super::catalog;
use super::kit::{self, bold, cover, text, Skin};
use super::*;

const SKIN: Skin = Skin {
    page: "#000000",
    panel: "#121212",
    raised: "#ffffff1a",
    ink: "#ffffff",
    muted: "#b3b3b3",
    accent: "#1ed760",
    played: "#ffffff",
    track: "#4d4d4d",
    art_radius: 4,
};
/// Spotify's own lighter green for filled buttons.
const GREEN: &str = "#1ed760";

pub fn route(
    state: &mut Value,
    ctx: &ServiceContext,
    request: &HttpRequest,
    parts: &[&str],
    theme: &PageTheme,
) -> Result<HttpResponse> {
    let back = kit::here(request);
    let s: &Value = state;
    let (title, main) = match parts {
        [""] => (web::text(s, "brand"), home(s, ctx)),
        ["search"] => {
            let q = web::query(request, "q").unwrap_or_default();
            (
                if q.is_empty() {
                    "Search".to_owned()
                } else {
                    format!("{q} - search")
                },
                search(s, &ctx.actor, &q, &back),
            )
        }
        ["album", id] => match catalog::album(s, id) {
            Some(album) => (album.title.clone(), album_page(s, ctx, &album, &back)),
            None => return web::error(404, "album not found"),
        },
        ["artist", id] | ["channel", id] => match record(s, "channels", id) {
            Some(_) => (catalog::artist_name(s, id), artist_page(s, ctx, id, &back)),
            None => return web::error(404, "artist not found"),
        },
        ["track", id] => match record(s, "items", id) {
            Some(item) => (web::text(item, "title"), track_page(s, ctx, id, &back)),
            None => return web::error(404, "track not found"),
        },
        ["playlist", id] => match record(s, "playlists", id) {
            Some(p) => (web::text(p, "title"), playlist_page(s, ctx, id, &back)),
            None => return web::error(404, "playlist not found"),
        },
        ["playlist"] => match web::query(request, "list") {
            Some(id) if record(s, "playlists", &id).is_some() => {
                (id.clone(), playlist_page(s, ctx, &id, &back))
            }
            _ => ("Your Library".into(), library(s, &ctx.actor, "all")),
        },
        ["collection", "tracks"] => ("Liked Songs".into(), liked_page(s, ctx, &back)),
        ["collection", filter @ ("playlists" | "albums" | "artists")] => {
            ("Your Library".into(), library(s, &ctx.actor, filter))
        }
        ["collection"] | ["playlists"] => ("Your Library".into(), library(s, &ctx.actor, "all")),
        ["queue"] => ("Queue".into(), queue(s, ctx, &back)),
        _ => return web::error(404, "route not found"),
    };
    let mut theme = theme.clone();
    theme.background = Some(SKIN.page.into());
    theme.content_width = None;
    web::themed_page(
        &title,
        theme,
        vec![
            top_bar(s, &ctx.actor),
            web::styled_row(
                "frame",
                8,
                "start",
                web::style().padding(8),
                vec![sidebar(s, &ctx.actor), panel(main)],
            ),
            player_bar(s, ctx, &back),
        ],
    )
}
fn panel(children: Vec<PageElement>) -> PageElement {
    web::column(
        "main",
        16,
        web::style()
            .flex(3)
            .background(SKIN.panel)
            .radius(8)
            .padding(16),
        children,
    )
}
fn top_bar(state: &Value, actor: &str) -> PageElement {
    let initial = actor
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    web::styled_row(
        "topbar",
        12,
        "center",
        web::style().padding(8).background(SKIN.page),
        vec![
            web::card_action(
                "topbar-logo",
                web::style().width(132).padding(4).background(SKIN.page),
                web::visit("/"),
                vec![web::styled_row(
                    "topbar-logo-row",
                    8,
                    "center",
                    web::style(),
                    vec![
                        web::thumbnail(
                            "topbar-logo-mark",
                            "",
                            web::style()
                                .width(32)
                                .height(32)
                                .radius(16)
                                .background(GREEN),
                        ),
                        bold("topbar-logo-name", web::text(state, "brand"), 18, SKIN.ink),
                    ],
                )],
            ),
            web::spacer("topbar-left", 0),
            web::card_action(
                "topbar-home",
                web::style()
                    .width(48)
                    .padding(12)
                    .radius(24)
                    .background("#1f1f1f"),
                web::visit("/"),
                vec![web::styled(
                    "topbar-home-glyph",
                    "⌂",
                    web::style().size(18).color(SKIN.ink).align("center"),
                )],
            ),
            web::card_action(
                "topbar-search",
                web::style()
                    .flex(4)
                    .padding(14)
                    .radius(24)
                    .background("#1f1f1f"),
                web::visit("/search"),
                vec![kit::line(
                    "topbar-search-hint",
                    "What do you want to play?",
                    15,
                    SKIN.muted,
                )],
            ),
            web::spacer("topbar-right", 0),
            web::thumbnail(
                "topbar-avatar",
                initial,
                web::style()
                    .width(32)
                    .height(32)
                    .radius(16)
                    .background("#f573a0")
                    .color("#000000")
                    .size(13),
            ),
        ],
    )
}
/// One entry of the library panel: art, title and the "Playlist • owner" line.
fn library_entry(
    id: &str,
    of: &str,
    title: &str,
    meta: &str,
    to: String,
    round: bool,
) -> PageElement {
    web::card_action(
        &format!("side-{id}"),
        web::style().padding(6).radius(6).background("#00000000"),
        web::visit(to),
        vec![web::styled_row(
            &format!("side-{id}-row"),
            12,
            "center",
            web::style(),
            vec![
                cover(
                    &format!("side-{id}-art"),
                    of,
                    "",
                    (48, 48),
                    if round { 24 } else { 4 },
                ),
                web::column(
                    &format!("side-{id}-text"),
                    0,
                    web::style().flex(1),
                    vec![
                        text(&format!("side-{id}-title"), title, 15, SKIN.ink),
                        text(&format!("side-{id}-meta"), meta, 13, SKIN.muted),
                    ],
                ),
            ],
        )],
    )
}
fn owner_name(state: &Value, playlist: &Value) -> String {
    match web::text(playlist, "owner") {
        o if o.is_empty() => web::text(state, "brand"),
        o => titled_name(&o),
    }
}
fn titled_name(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}
fn sidebar(state: &Value, actor: &str) -> PageElement {
    let mut rows = vec![
        web::styled_row(
            "side-head",
            8,
            "center",
            web::style(),
            vec![
                bold("side-heading", "Your Library", 16, SKIN.ink),
                web::card_action(
                    "side-create",
                    web::style().padding(8).radius(16).background("#1f1f1f"),
                    web::visit("/collection"),
                    vec![kit::line_bold("side-create-text", "+ Create", 13, SKIN.ink)],
                ),
            ],
        ),
        kit::pills(
            "side-chips",
            8,
            ["playlists", "artists", "albums"]
                .iter()
                .map(|f| {
                    chip(
                        &format!("side-chip-{f}"),
                        &titled_name(f),
                        false,
                        format!("/collection/{f}"),
                    )
                })
                .collect(),
        ),
    ];
    let liked = catalog::liked(state, actor);
    rows.push(web::card_action(
        "side-liked",
        web::style().padding(6).radius(6).background("#00000000"),
        web::visit("/collection/tracks"),
        vec![web::styled_row(
            "side-liked-row",
            12,
            "center",
            web::style(),
            vec![
                web::thumbnail(
                    "side-liked-art",
                    "♥",
                    web::style()
                        .width(48)
                        .height(48)
                        .radius(4)
                        .background("#5038a0")
                        .color("#ffffff")
                        .size(18),
                ),
                web::column(
                    "side-liked-text",
                    0,
                    web::style().flex(1),
                    vec![
                        text("side-liked-title", "Liked Songs", 15, SKIN.ink),
                        text(
                            "side-liked-meta",
                            format!("Playlist • {}", kit::count(liked.len(), "song", "songs")),
                            13,
                            SKIN.muted,
                        ),
                    ],
                ),
            ],
        )],
    ));
    for id in keys(state, "playlists") {
        let p = record(state, "playlists", &id)
            .cloned()
            .unwrap_or(Value::Null);
        rows.push(library_entry(
            &format!("playlist-{id}"),
            &id,
            &web::text(&p, "title"),
            &format!("Playlist • {}", owner_name(state, &p)),
            format!("/playlist/{id}"),
            false,
        ));
    }
    for album in saved_albums(state, actor) {
        rows.push(library_entry(
            &format!("album-{}", album.id),
            &album.id,
            &album.title,
            &format!("Album • {}", album.artist_name),
            format!("/album/{}", album.id),
            false,
        ));
    }
    for artist in strings_at(state, "subscriptions", actor) {
        if record(state, "channels", &artist).is_none() {
            continue;
        }
        rows.push(library_entry(
            &format!("artist-{artist}"),
            &artist,
            &catalog::artist_name(state, &artist),
            "Artist",
            format!("/artist/{artist}"),
            true,
        ));
    }
    web::column(
        "side",
        4,
        web::style()
            .width(300)
            .background(SKIN.panel)
            .radius(8)
            .padding(12),
        rows,
    )
}
fn chip(id: &str, label: &str, on: bool, to: String) -> PageElement {
    web::card_action(
        id,
        web::style()
            .padding(8)
            .radius(16)
            .background(if on { SKIN.ink } else { "#2a2a2a" }),
        web::visit(to),
        vec![web::styled(
            &format!("{id}-text"),
            label,
            web::style()
                .size(13)
                .one_line()
                .color(if on { "#000000" } else { SKIN.ink }),
        )],
    )
}
/// Albums with every track in the listener's library.
fn saved_albums(state: &Value, actor: &str) -> Vec<catalog::Album> {
    let library = strings_at(state, "library", actor);
    catalog::albums(state)
        .into_iter()
        .filter(|a| a.tracks.iter().all(|t| library.contains(t)))
        .collect()
}
fn heading(id: &str, text: &str) -> PageElement {
    web::styled(id, text, web::style().size(22).bold().color(SKIN.ink))
}
/// A card in a shelf: square art, a title and one muted line.
fn tile(id: &str, of: &str, title: &str, meta: &str, to: String, round: bool) -> PageElement {
    web::card_action(
        id,
        web::style().padding(10).radius(6).background("#00000000"),
        web::visit(to),
        vec![
            cover(
                &format!("{id}-art"),
                of,
                if round { "" } else { title },
                (if round { 132 } else { 0 }, 132),
                if round { 64 } else { 6 },
            ),
            text(&format!("{id}-title"), title, 15, SKIN.ink),
            text(&format!("{id}-meta"), meta, 13, SKIN.muted),
        ],
    )
}
fn shelf(id: &str, title: &str, tiles: Vec<PageElement>) -> Vec<PageElement> {
    if tiles.is_empty() {
        return vec![];
    }
    vec![
        heading(&format!("{id}-heading"), title),
        web::grid(id, 5, 8, tiles),
    ]
}
fn album_tile(prefix: &str, album: &catalog::Album) -> PageElement {
    tile(
        &format!("{prefix}-{}", album.id),
        &album.id,
        &album.title,
        &format!("{} • {}", album.year, album.artist_name),
        format!("/album/{}", album.id),
        false,
    )
}
fn home(state: &Value, ctx: &ServiceContext) -> Vec<PageElement> {
    let actor = &ctx.actor;
    let greeting = match kit::hour(ctx.tick) {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };
    // Shortcuts: Liked Songs, then playlists, then the newest albums, eight in all.
    let mut shortcuts = vec![shortcut(
        "short-liked",
        "liked",
        "Liked Songs",
        "/collection/tracks".into(),
    )];
    for id in keys(state, "playlists") {
        let title = record(state, "playlists", &id)
            .map(|p| web::text(p, "title"))
            .unwrap_or_default();
        shortcuts.push(shortcut(
            &format!("short-playlist-{id}"),
            &id,
            &title,
            format!("/playlist/{id}"),
        ));
    }
    for album in catalog::albums(state) {
        shortcuts.push(shortcut(
            &format!("short-album-{}", album.id),
            &album.id,
            &album.title,
            format!("/album/{}", album.id),
        ));
    }
    shortcuts.truncate(8);
    let mut out = vec![
        heading("home-greeting", greeting),
        web::grid("home-shortcuts", 4, 8, shortcuts),
    ];
    let history = strings_at(state, "history", actor);
    let mut seen = vec![];
    let recent: Vec<PageElement> = history
        .iter()
        .filter_map(|id| record(state, "items", id))
        .map(catalog::album_id)
        .filter(|a| {
            let fresh = !seen.contains(a);
            seen.push(a.clone());
            fresh
        })
        .filter_map(|a| catalog::album(state, &a))
        .take(5)
        .map(|a| album_tile("recent", &a))
        .collect();
    out.extend(shelf("home-recent", "Recently played", recent));
    let albums: Vec<PageElement> = catalog::albums(state)
        .iter()
        .take(10)
        .map(|a| album_tile("popular", a))
        .collect();
    out.extend(shelf("home-albums", "Popular albums and singles", albums));
    let mut artists = keys(state, "channels");
    artists.sort_by_key(|id| {
        std::cmp::Reverse(record(state, "channels", id).map_or(0, |c| num(c, "subscribers")))
    });
    let artists: Vec<PageElement> = artists
        .iter()
        .map(|id| {
            tile(
                &format!("artist-{id}"),
                id,
                &catalog::artist_name(state, id),
                "Artist",
                format!("/artist/{id}"),
                true,
            )
        })
        .collect();
    out.extend(shelf("home-artists", "Popular artists", artists));
    let lists: Vec<PageElement> = keys(state, "playlists")
        .iter()
        .filter_map(|id| record(state, "playlists", id).map(|p| (id, p)))
        .map(|(id, p)| {
            tile(
                &format!("list-{id}"),
                id,
                &web::text(p, "title"),
                &format!("By {}", owner_name(state, p)),
                format!("/playlist/{id}"),
                false,
            )
        })
        .collect();
    out.extend(shelf("home-playlists", "Made for you", lists));
    out
}
fn shortcut(id: &str, of: &str, title: &str, to: String) -> PageElement {
    web::card_action(
        id,
        web::style().padding(0).radius(4).background(SKIN.raised),
        web::visit(to),
        vec![web::styled_row(
            &format!("{id}-row"),
            12,
            "center",
            web::style(),
            vec![
                if of == "liked" {
                    web::thumbnail(
                        &format!("{id}-art"),
                        "♥",
                        web::style()
                            .width(56)
                            .height(56)
                            .radius(4)
                            .background("#5038a0")
                            .color("#ffffff")
                            .size(16),
                    )
                } else {
                    cover(&format!("{id}-art"), of, "", (56, 56), 4)
                },
                web::styled(
                    &format!("{id}-title"),
                    title,
                    web::style().size(14).bold().color(SKIN.ink),
                ),
            ],
        )],
    )
}
/// The big header every collection page opens with: art, kind, title and a meta line.
fn hero(id: &str, of: &str, kind: &str, title: &str, meta: String, round: bool) -> PageElement {
    web::styled_row(
        &format!("{id}-hero"),
        20,
        "end",
        web::style()
            .padding(16)
            .radius(8)
            .background(kit::shade(tint(of), 60)),
        vec![
            cover(
                &format!("{id}-art"),
                of,
                title,
                (180, 180),
                if round { 64 } else { 6 },
            ),
            web::column(
                &format!("{id}-identity"),
                6,
                web::style().flex(3),
                vec![
                    text(&format!("{id}-kind"), kind, 13, SKIN.ink),
                    web::styled(
                        &format!("{id}-title"),
                        title,
                        web::style().size(40).bold().color(SKIN.ink),
                    ),
                    text(&format!("{id}-meta"), meta, 13, SKIN.ink),
                ],
            ),
        ],
    )
}
fn big_play(id: &str, context: &str, back: &str, playing_this: bool) -> PageElement {
    kit::control(
        id,
        if playing_this { "❚❚" } else { "▶" },
        20,
        "#000000",
        Some(GREEN),
        if playing_this {
            kit::fields(&[("action", "toggle"), ("return", back)])
        } else {
            kit::fields(&[("action", "play"), ("context", context), ("return", back)])
        },
    )
}
fn actions(children: Vec<PageElement>) -> PageElement {
    kit::pills("actions", 16, children)
}
fn column_head(extra: &str) -> PageElement {
    web::styled_row(
        "tracks-head",
        14,
        "center",
        web::style().padding(6),
        vec![
            web::styled(
                "tracks-head-number",
                "#",
                web::style()
                    .size(13)
                    .color(SKIN.muted)
                    .width(28)
                    .align("right"),
            ),
            text("tracks-head-title", "Title", 13, SKIN.muted),
            text("tracks-head-extra", extra, 13, SKIN.muted),
            web::styled(
                "tracks-head-time",
                "Time",
                web::style()
                    .size(13)
                    .color(SKIN.muted)
                    .width(96)
                    .align("right"),
            ),
        ],
    )
}
fn current(state: &Value, ctx: &ServiceContext) -> (Option<String>, String, bool) {
    match catalog::player(state, &ctx.actor, ctx.tick) {
        Some(p) => (p.current().map(str::to_owned), p.context.clone(), p.playing),
        None => (None, String::new(), false),
    }
}
fn tracks(
    state: &Value,
    ctx: &ServiceContext,
    ids: &[String],
    context: &str,
    back: &str,
    extra: impl Fn(&str) -> Option<String>,
    art: bool,
) -> Vec<PageElement> {
    let (playing, _, _) = current(state, ctx);
    ids.iter()
        .enumerate()
        .map(|(i, id)| {
            kit::track_row(
                state,
                &ctx.actor,
                "track",
                id,
                i + 1,
                context,
                playing.as_deref(),
                &SKIN,
                back,
                extra(id),
                art,
            )
        })
        .collect()
}
fn album_page(
    state: &Value,
    ctx: &ServiceContext,
    album: &catalog::Album,
    back: &str,
) -> Vec<PageElement> {
    let context = format!("album:{}", album.id);
    let (_, playing_context, playing) = current(state, ctx);
    let saved = saved_albums(state, &ctx.actor)
        .iter()
        .any(|a| a.id == album.id);
    let mut out = vec![
        hero(
            "album",
            &album.id,
            album.kind(),
            &album.title,
            format!(
                "{} • {} • {}, {}",
                album.artist_name,
                album.year,
                kit::count(album.tracks.len(), "song", "songs"),
                kit::running(state, &album.tracks)
            ),
            false,
        ),
        actions(vec![
            big_play(
                "album-play",
                &context,
                back,
                playing && playing_context == context,
            ),
            kit::control(
                "album-shuffle",
                "⇄",
                18,
                SKIN.muted,
                None,
                kit::fields(&[
                    ("action", "play"),
                    ("context", &context),
                    ("shuffle", "true"),
                    ("return", back),
                ]),
            ),
            kit::control(
                "album-save",
                if saved { "✓" } else { "+" },
                18,
                if saved { GREEN } else { SKIN.muted },
                None,
                post(format!("/library/albums/{}", album.id), &[("return", back)]),
            ),
            web::card_action(
                "album-artist",
                web::style().padding(6).radius(16).background("#00000000"),
                web::visit(format!("/artist/{}", album.artist)),
                vec![kit::line(
                    "album-artist-name",
                    &album.artist_name,
                    13,
                    SKIN.ink,
                )],
            ),
        ]),
        column_head(""),
    ];
    out.extend(tracks(
        state,
        ctx,
        &album.tracks,
        &context,
        back,
        |_| None,
        false,
    ));
    out.push(text(
        "album-copyright",
        format!("℗ {} {}", album.year, album.artist_name),
        11,
        SKIN.muted,
    ));
    out
}
fn playlist_page(state: &Value, ctx: &ServiceContext, id: &str, back: &str) -> Vec<PageElement> {
    let p = record(state, "playlists", id)
        .cloned()
        .unwrap_or(Value::Null);
    let items: Vec<String> = catalog::context_tracks(state, &ctx.actor, &format!("playlist:{id}"));
    let context = format!("playlist:{id}");
    let (_, playing_context, playing) = current(state, ctx);
    let title = web::text(&p, "title");
    let mut out = vec![hero(
        "playlist",
        id,
        "Playlist",
        &title,
        format!(
            "{} • {}, {}",
            owner_name(state, &p),
            kit::count(items.len(), "song", "songs"),
            kit::running(state, &items)
        ),
        false,
    )];
    if items.is_empty() {
        out.push(text(
            "playlist-empty",
            "Let's find something for your playlist",
            16,
            SKIN.ink,
        ));
    } else {
        out.push(actions(vec![
            big_play(
                "playlist-play",
                &context,
                back,
                playing && playing_context == context,
            ),
            kit::control(
                "playlist-shuffle",
                "⇄",
                18,
                SKIN.muted,
                None,
                kit::fields(&[
                    ("action", "play"),
                    ("context", &context),
                    ("shuffle", "true"),
                    ("return", back),
                ]),
            ),
        ]));
        out.push(column_head("Album"));
        out.extend(tracks(
            state,
            ctx,
            &items,
            &context,
            back,
            |t| {
                record(state, "items", t)
                    .and_then(|i| catalog::album(state, &catalog::album_id(i)))
                    .map(|a| a.title)
            },
            true,
        ));
    }
    out
}
fn liked_page(state: &Value, ctx: &ServiceContext, back: &str) -> Vec<PageElement> {
    let items = catalog::liked(state, &ctx.actor);
    let mut out = vec![hero(
        "liked",
        "liked",
        "Playlist",
        "Liked Songs",
        format!(
            "{} • {}",
            titled_name(&ctx.actor),
            kit::count(items.len(), "song", "songs")
        ),
        false,
    )];
    if items.is_empty() {
        out.push(text(
            "liked-empty",
            "Songs you like will appear here. Save songs by tapping the heart icon.",
            14,
            SKIN.muted,
        ));
    } else {
        out.push(actions(vec![big_play("liked-play", "liked", back, false)]));
        out.push(column_head("Album"));
        out.extend(tracks(state, ctx, &items, "liked", back, |_| None, true));
    }
    out
}
fn artist_page(state: &Value, ctx: &ServiceContext, id: &str, back: &str) -> Vec<PageElement> {
    let artist = record(state, "channels", id)
        .cloned()
        .unwrap_or(Value::Null);
    let name = web::text(&artist, "name");
    let following = has(state, "subscriptions", &ctx.actor, id);
    let context = format!("artist:{id}");
    let (_, playing_context, playing) = current(state, ctx);
    let top: Vec<String> = catalog::top_tracks(state, id).into_iter().take(5).collect();
    let mut out = vec![
        web::card(
            "artist-banner",
            web::style()
                .padding(24)
                .radius(8)
                .background(kit::shade(tint(id), 70)),
            vec![
                text("artist-verified", "✓ Verified Artist", 13, SKIN.ink),
                web::styled(
                    "artist-name",
                    &name,
                    web::style().size(48).bold().color(SKIN.ink),
                ),
                text(
                    "artist-listeners",
                    format!("{} monthly listeners", grouped(num(&artist, "subscribers"))),
                    14,
                    SKIN.ink,
                ),
            ],
        ),
        actions(vec![
            big_play(
                "artist-play",
                &context,
                back,
                playing && playing_context == context,
            ),
            web::card_action(
                "artist-follow",
                web::style()
                    .padding(8)
                    .radius(16)
                    .border(if following { SKIN.ink } else { "#727272" })
                    .background("#00000000"),
                post(format!("/channels/{id}/subscribe"), &[("return", back)]),
                vec![kit::line_bold(
                    "artist-follow-text",
                    if following { "Following" } else { "Follow" },
                    13,
                    SKIN.ink,
                )],
            ),
            web::card_action(
                "artist-radio",
                web::style().padding(8).radius(16).background("#00000000"),
                kit::fields(&[
                    ("action", "play"),
                    ("context", &format!("station:{id}")),
                    ("return", back),
                ]),
                vec![kit::line(
                    "artist-radio-text",
                    "Go to artist radio",
                    13,
                    SKIN.muted,
                )],
            ),
        ]),
        heading("artist-popular", "Popular"),
    ];
    let plays = |t: &str| record(state, "items", t).map(|i| grouped(num(i, "plays")));
    out.extend(tracks(state, ctx, &top, &context, back, plays, true));
    let discography: Vec<PageElement> = catalog::albums(state)
        .iter()
        .filter(|a| a.artist == id)
        .map(|a| album_tile("disco", a))
        .collect();
    out.extend(shelf("artist-discography", "Discography", discography));
    let about = web::text(&artist, "about");
    if !about.is_empty() {
        out.push(heading("artist-about-heading", "About"));
        out.push(web::card(
            "artist-about",
            web::style().padding(16).radius(8).background("#242424"),
            vec![web::styled(
                "artist-about-text",
                about,
                web::style().size(14).color(SKIN.muted),
            )],
        ));
    }
    out
}
fn track_page(state: &Value, ctx: &ServiceContext, id: &str, back: &str) -> Vec<PageElement> {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let title = web::text(&item, "title");
    let artist = web::text(&item, "channel");
    let album = catalog::album(state, &catalog::album_id(&item));
    let liked = has(state, "likes", &ctx.actor, id);
    let saved = has(state, "library", &ctx.actor, id);
    let context = album
        .as_ref()
        .map(|a| format!("album:{}", a.id))
        .unwrap_or_else(|| "track".into());
    let (playing_id, _, playing) = current(state, ctx);
    let mut out = vec![
        hero(
            "track",
            &catalog::album_id(&item),
            "Song",
            &title,
            format!(
                "{} • {} • {} • {}",
                catalog::artist_name(state, &artist),
                album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
                clock(num(&item, "duration_s")),
                grouped(num(&item, "plays"))
            ),
            false,
        ),
        actions(vec![
            if playing && playing_id.as_deref() == Some(id) {
                big_play("track-play", &context, back, true)
            } else {
                kit::control(
                    "track-play",
                    "▶",
                    20,
                    "#000000",
                    Some(GREEN),
                    kit::fields(&[
                        ("action", "play"),
                        ("item", id),
                        ("context", &context),
                        ("return", back),
                    ]),
                )
            },
            kit::control(
                "track-like",
                if liked { "♥" } else { "♡" },
                20,
                if liked { GREEN } else { SKIN.muted },
                None,
                post(format!("/items/{id}/like"), &[("return", back)]),
            ),
            web::card_action(
                "track-save",
                web::style()
                    .padding(8)
                    .radius(16)
                    .border("#727272")
                    .background("#00000000"),
                post(format!("/library/items/{id}"), &[("return", back)]),
                vec![kit::line_bold(
                    "track-save-text",
                    if saved {
                        "✓ In your library"
                    } else {
                        "+ Save to library"
                    },
                    13,
                    SKIN.ink,
                )],
            ),
            web::card_action(
                "track-queue",
                web::style()
                    .padding(8)
                    .radius(16)
                    .border("#727272")
                    .background("#00000000"),
                post(format!("/items/{id}/queue"), &[("return", back)]),
                vec![kit::line_bold(
                    "track-queue-text",
                    "Add to queue",
                    13,
                    SKIN.ink,
                )],
            ),
        ]),
    ];
    let writable: Vec<(String, String)> = keys(state, "playlists")
        .into_iter()
        .filter_map(|p| {
            let list = record(state, "playlists", &p)?;
            let owner = web::text(list, "owner");
            (owner.is_empty() || owner == ctx.actor).then(|| (p.clone(), web::text(list, "title")))
        })
        .collect();
    if !writable.is_empty() {
        out.push(heading("track-add-heading", "Add to playlist"));
        out.push(kit::pills(
            "track-add",
            8,
            writable
                .iter()
                .map(|(p, title)| {
                    let inside = record(state, "playlists", p)
                        .is_some_and(|l| web::strings(l, "items").iter().any(|i| i == id));
                    if inside {
                        web::card(
                            &format!("track-add-{p}"),
                            web::style().padding(8).radius(16).background("#2a2a2a"),
                            vec![kit::line(
                                &format!("track-add-{p}-text"),
                                format!("✓ {title}"),
                                13,
                                SKIN.muted,
                            )],
                        )
                    } else {
                        web::card_action(
                            &format!("track-add-{p}"),
                            web::style().padding(8).radius(16).background("#2a2a2a"),
                            post(
                                format!("/playlists/{p}/items"),
                                &[("item", id), ("return", back)],
                            ),
                            vec![kit::line(
                                &format!("track-add-{p}-text"),
                                format!("+ {title}"),
                                13,
                                SKIN.ink,
                            )],
                        )
                    }
                })
                .collect(),
        ));
    }
    if let Some(album) = album {
        out.extend(shelf(
            "track-album",
            &format!("From {}", album.title),
            vec![album_tile("from", &album)],
        ));
    }
    out
}
fn search(state: &Value, actor: &str, q: &str, back: &str) -> Vec<PageElement> {
    let mut out = vec![web::form(
        "search",
        "/search",
        &[("q", "What do you want to play?", q)],
    )];
    if q.trim().is_empty() {
        out.push(heading("browse-heading", "Browse all"));
        let mut tags: Vec<String> = keys(state, "items")
            .iter()
            .filter_map(|id| record(state, "items", id))
            .flat_map(|i| web::strings(i, "tags"))
            .collect();
        tags.sort();
        tags.dedup();
        out.push(web::grid(
            "browse-grid",
            4,
            12,
            tags.iter()
                .map(|t| {
                    web::card_action(
                        &format!("genre-{}", slug(t)),
                        web::style()
                            .padding(14)
                            .radius(8)
                            .height(96)
                            .background(tint(&format!("genre{t}"))),
                        web::visit(format!("/search?q={}", encode(t))),
                        vec![web::styled(
                            &format!("genre-{}-name", slug(t)),
                            titled_name(t),
                            web::style().size(20).bold().color(SKIN.ink),
                        )],
                    )
                })
                .collect(),
        ));
        return out;
    }
    let hits = catalog::search(state, q);
    let list = |k: &str| web::strings(&hits, k);
    let (songs, albums, artists, playlists) = (
        list("tracks"),
        list("albums"),
        list("artists"),
        list("playlists"),
    );
    if songs.is_empty() && albums.is_empty() && artists.is_empty() && playlists.is_empty() {
        out.push(heading(
            "results-heading",
            &format!("No results found for \"{q}\""),
        ));
        out.push(text("results-hint", "Please make sure your words are spelled correctly, or use fewer or different keywords.", 14, SKIN.muted));
        return out;
    }
    if !songs.is_empty() {
        out.push(heading("results-songs", "Songs"));
        for (i, id) in songs.iter().take(8).enumerate() {
            out.push(kit::track_row(
                state,
                actor,
                "result",
                id,
                i + 1,
                "track",
                None,
                &SKIN,
                back,
                None,
                true,
            ));
        }
    }
    let artist_tiles: Vec<PageElement> = artists
        .iter()
        .map(|id| {
            tile(
                &format!("result-artist-{id}"),
                id,
                &catalog::artist_name(state, id),
                "Artist",
                format!("/artist/{id}"),
                true,
            )
        })
        .collect();
    out.extend(shelf("results-artists", "Artists", artist_tiles));
    let album_tiles: Vec<PageElement> = albums
        .iter()
        .filter_map(|id| catalog::album(state, id))
        .map(|a| album_tile("result-album", &a))
        .collect();
    out.extend(shelf("results-albums", "Albums", album_tiles));
    let list_tiles: Vec<PageElement> = playlists
        .iter()
        .filter_map(|id| record(state, "playlists", id).map(|p| (id, p)))
        .map(|(id, p)| {
            tile(
                &format!("result-playlist-{id}"),
                id,
                &web::text(p, "title"),
                &format!("By {}", owner_name(state, p)),
                format!("/playlist/{id}"),
                false,
            )
        })
        .collect();
    out.extend(shelf("results-playlists", "Playlists", list_tiles));
    out
}
fn library(state: &Value, actor: &str, filter: &str) -> Vec<PageElement> {
    let mut out = vec![
        heading("library-heading", "Your Library"),
        kit::pills(
            "library-chips",
            8,
            [
                ("all", "All", "/collection"),
                ("playlists", "Playlists", "/collection/playlists"),
                ("albums", "Albums", "/collection/albums"),
                ("artists", "Artists", "/collection/artists"),
            ]
            .iter()
            .map(|(k, label, to)| {
                chip(
                    &format!("library-chip-{k}"),
                    label,
                    *k == filter,
                    (*to).to_owned(),
                )
            })
            .collect(),
        ),
    ];
    let mut tiles = vec![];
    if matches!(filter, "all" | "playlists") {
        tiles.push(tile(
            "library-liked",
            "liked",
            "Liked Songs",
            &kit::count(catalog::liked(state, actor).len(), "song", "songs"),
            "/collection/tracks".into(),
            false,
        ));
        for id in keys(state, "playlists") {
            let p = record(state, "playlists", &id)
                .cloned()
                .unwrap_or(Value::Null);
            tiles.push(tile(
                &format!("library-{id}"),
                &id,
                &web::text(&p, "title"),
                &format!("Playlist • {}", owner_name(state, &p)),
                format!("/playlist/{id}"),
                false,
            ));
        }
    }
    if matches!(filter, "all" | "albums") {
        for a in saved_albums(state, actor) {
            tiles.push(album_tile("library-album", &a));
        }
    }
    if matches!(filter, "all" | "artists") {
        for id in strings_at(state, "subscriptions", actor) {
            if record(state, "channels", &id).is_some() {
                tiles.push(tile(
                    &format!("library-artist-{id}"),
                    &id,
                    &catalog::artist_name(state, &id),
                    "Artist",
                    format!("/artist/{id}"),
                    true,
                ));
            }
        }
    }
    if tiles.is_empty() {
        out.push(text("library-empty", "Nothing here yet.", 14, SKIN.muted));
    } else {
        out.push(web::grid("library-grid", 5, 8, tiles));
    }
    out.push(web::form(
        "playlist",
        "/playlists",
        &[("title", "New playlist", "")],
    ));
    out
}
fn queue(state: &Value, ctx: &ServiceContext, back: &str) -> Vec<PageElement> {
    let mut out = vec![heading("queue-heading", "Queue")];
    let Some(p) = catalog::player(state, &ctx.actor, ctx.tick) else {
        out.push(text("queue-empty", "Add to your queue", 16, SKIN.ink));
        out.push(text(
            "queue-hint",
            "Tap \"Add to queue\" on a song to find it here.",
            14,
            SKIN.muted,
        ));
        return out;
    };
    out.push(bold("queue-now", "Now playing", 16, SKIN.ink));
    let row = |i: usize, id: &str| {
        let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
        let current = i == p.index;
        web::card_action(
            &format!("queue-{i}"),
            web::style().padding(8).radius(4).background(if current {
                SKIN.raised
            } else {
                "#00000000"
            }),
            kit::fields(&[
                ("action", "jump"),
                ("index", &i.to_string()),
                ("return", back),
            ]),
            vec![web::styled_row(
                &format!("queue-{i}-row"),
                12,
                "center",
                web::style(),
                vec![
                    cover(
                        &format!("queue-{i}-art"),
                        &catalog::album_id(&item),
                        "",
                        (48, 48),
                        4,
                    ),
                    web::column(
                        &format!("queue-{i}-text"),
                        0,
                        web::style().flex(1),
                        vec![
                            text(
                                &format!("queue-{i}-title"),
                                web::text(&item, "title"),
                                15,
                                if current { GREEN } else { SKIN.ink },
                            ),
                            text(
                                &format!("queue-{i}-artist"),
                                catalog::artist_name(state, &web::text(&item, "channel")),
                                13,
                                SKIN.muted,
                            ),
                        ],
                    ),
                    text(
                        &format!("queue-{i}-time"),
                        clock(num(&item, "duration_s")),
                        13,
                        SKIN.muted,
                    ),
                ],
            )],
        )
    };
    if let Some(id) = p.current() {
        out.push(row(p.index, id));
    }
    let title = catalog::context_title(state, &p.context);
    out.push(bold(
        "queue-next",
        if title.is_empty() {
            "Next up".to_owned()
        } else {
            format!("Next from: {title}")
        },
        16,
        SKIN.ink,
    ));
    for (i, id) in p.queue.iter().enumerate().skip(p.index + 1) {
        out.push(row(i, id));
    }
    out
}
fn player_bar(state: &Value, ctx: &ServiceContext, back: &str) -> PageElement {
    let style = web::style().background(SKIN.page).padding(10).pin("bottom");
    let Some(now) = kit::loaded(state, &ctx.actor, ctx.tick) else {
        return web::styled_row(
            "bar",
            12,
            "center",
            style,
            vec![text("bar-idle", "Nothing playing", 13, SKIN.muted)],
        );
    };
    let liked = has(state, "likes", &ctx.actor, &now.id);
    let album = now.album.as_ref().map(|a| a.id.clone()).unwrap_or_default();
    let p = &now.player;
    let mut middle = vec![web::spacer("bar-controls-left", 0)];
    middle.extend(kit::transport(p, &SKIN, back, (SKIN.ink, "#000000")));
    middle.push(web::spacer("bar-controls-right", 0));
    let mut scrub = vec![web::styled(
        "bar-elapsed",
        kit::clock_ms(p.position_ms),
        web::style()
            .size(11)
            .color(SKIN.muted)
            .width(40)
            .align("right"),
    )];
    scrub.extend(kit::scrubber(p, now.length_ms, &SKIN, back));
    scrub.push(web::styled(
        "bar-length",
        kit::clock_ms(now.length_ms),
        web::style().size(11).color(SKIN.muted).width(40),
    ));
    web::styled_row(
        "bar",
        16,
        "center",
        style,
        vec![
            web::styled_row(
                "bar-now",
                12,
                "center",
                web::style().flex(2),
                vec![
                    cover("bar-art", &album, "", (56, 56), 4),
                    web::column(
                        "bar-text",
                        0,
                        web::style().flex(1),
                        vec![
                            web::card_action(
                                "bar-title",
                                web::style().padding(0).background("#00000000"),
                                web::visit(format!("/track/{}", now.id)),
                                vec![text("bar-title-text", &now.title, 14, SKIN.ink)],
                            ),
                            web::card_action(
                                "bar-meta",
                                web::style().padding(0).background("#00000000"),
                                web::visit(format!("/artist/{}", now.artist_id)),
                                vec![text("bar-meta-text", &now.artist, 12, SKIN.muted)],
                            ),
                        ],
                    ),
                    kit::control(
                        "bar-like",
                        if liked { "♥" } else { "♡" },
                        16,
                        if liked { GREEN } else { SKIN.muted },
                        None,
                        post(format!("/items/{}/like", now.id), &[("return", back)]),
                    ),
                ],
            ),
            web::column(
                "bar-center",
                0,
                web::style().flex(3),
                vec![
                    web::styled_row("bar-controls", 8, "center", web::style(), middle),
                    web::styled_row("bar-scrub", 0, "center", web::style(), scrub),
                ],
            ),
            web::styled_row(
                "bar-right",
                8,
                "center",
                web::style().flex(1),
                vec![
                    web::spacer("bar-right-space", 0),
                    web::card_action(
                        "bar-queue",
                        web::style().padding(8).radius(16).background("#00000000"),
                        web::visit("/queue"),
                        vec![kit::line("bar-queue-text", "☰ Queue", 13, SKIN.muted)],
                    ),
                ],
            ),
        ],
    )
}
