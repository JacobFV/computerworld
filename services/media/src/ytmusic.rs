//! music.youtube.com (`music` mode), laid out like YouTube Music on the web: near-black
//! page, a top bar with the logo and a wide search box, a sidebar with Home, Explore and
//! Library above your playlists, mood chips and shelves of square album art and round
//! artist avatars, an Up next queue beside the big artwork, and a player bar pinned to the
//! bottom whose red progress line is a real seek control.
use super::catalog;
use super::kit::{self, bold, cover, text, Skin};
use super::*;

const SKIN: Skin = Skin {
    page: "#030303",
    panel: "#030303",
    raised: "#ffffff1a",
    ink: "#ffffff",
    muted: "#aaaaaa",
    accent: "#ff0000",
    played: "#ff0000",
    track: "#ffffff33",
    art_radius: 4,
};
const BAR: &str = "#212121";

/// A `list` query value to the context it plays, and back. YouTube Music's own prefixes:
/// `LM` is Liked Music, `OLAK` an album, `RD` a radio mix.
fn context_of(list: &str) -> String {
    if list == "LM" {
        "liked".into()
    } else if list == "LIB" {
        "library".into()
    } else if let Some(album) = list.strip_prefix("OLAK-") {
        format!("album:{album}")
    } else if let Some(artist) = list.strip_prefix("RD-") {
        format!("station:{artist}")
    } else if let Some(tag) = list.strip_prefix("MOOD-") {
        format!("mood:{tag}")
    } else {
        format!("playlist:{list}")
    }
}
fn list_of(context: &str) -> String {
    let (kind, id) = context.split_once(':').unwrap_or((context, ""));
    match kind {
        "liked" => "LM".into(),
        "library" => "LIB".into(),
        "album" => format!("OLAK-{id}"),
        "station" | "artist" => format!("RD-{id}"),
        "mood" => format!("MOOD-{}", encode(id)),
        "playlist" => id.to_owned(),
        _ => String::new(),
    }
}
fn watch_url(id: &str, context: &str) -> String {
    match list_of(context) {
        l if l.is_empty() => format!("/watch?v={id}"),
        l => format!("/watch?v={id}&list={l}"),
    }
}

pub fn route(
    state: &mut Value,
    ctx: &ServiceContext,
    request: &HttpRequest,
    parts: &[&str],
    theme: &PageTheme,
    count: bool,
) -> Result<HttpResponse> {
    let back = kit::here(request);
    // Opening a song is how YouTube Music plays it: the watch page starts it, within the
    // list it was opened from or, with none, a radio mix of its artist.
    if let (["watch"], true) = (parts, count) {
        if let Some(v) = web::query(request, "v") {
            let Some(item) = record(state, "items", &v).cloned() else {
                return web::error(404, "song not found");
            };
            let context = match web::query(request, "list") {
                Some(list) => context_of(&list),
                None => format!("station:{}", web::text(&item, "channel")),
            };
            let current = catalog::player(state, &ctx.actor, ctx.tick);
            let already = current
                .as_ref()
                .is_some_and(|p| p.current() == Some(v.as_str()) && p.context == context);
            if !already {
                catalog::command(
                    state,
                    ctx,
                    &json!({"action": "play", "item": v, "context": context}),
                )
                .or_else(|_| catalog::command(state, ctx, &json!({"action": "play", "item": v})))
                .map_err(cw_protocol::SimError::invalid)?;
            }
        }
    }
    let s: &Value = state;
    let (title, section, main) = match parts {
        [""] => {
            let mood = web::query(request, "mood").unwrap_or_default();
            ("YouTube Music".to_owned(), "home", home(s, ctx, &mood))
        }
        ["explore"] => ("Explore".into(), "explore", explore(s, ctx, &back, "")),
        ["explore", part @ ("new_releases" | "charts" | "moods_and_genres")] => {
            ("Explore".into(), "explore", explore(s, ctx, &back, part))
        }
        ["library"] => (
            "Library".into(),
            "library",
            library(s, ctx, "playlists", &back),
        ),
        ["library", tab @ ("playlists" | "songs" | "albums" | "artists")] => {
            ("Library".into(), "library", library(s, ctx, tab, &back))
        }
        ["browse", id] => match catalog::album(s, id) {
            Some(album) => (album.title.clone(), "", album_page(s, ctx, &album, &back)),
            None => return web::error(404, "album not found"),
        },
        ["channel", id] => match record(s, "channels", id) {
            Some(_) => (
                catalog::artist_name(s, id),
                "",
                artist_page(s, ctx, id, &back),
            ),
            None => return web::error(404, "artist not found"),
        },
        ["playlist"] => {
            let list = web::query(request, "list").unwrap_or_default();
            if list != "LM" && record(s, "playlists", &list).is_none() {
                return web::error(404, "playlist not found");
            }
            let title = if list == "LM" {
                "Liked Music".to_owned()
            } else {
                record(s, "playlists", &list)
                    .map(|p| web::text(p, "title"))
                    .unwrap_or_default()
            };
            (title, "", playlist_page(s, ctx, &list, &back))
        }
        ["watch"] => {
            let v = web::query(request, "v").unwrap_or_default();
            let Some(item) = record(s, "items", &v) else {
                return web::error(404, "song not found");
            };
            let tab = web::query(request, "tab").unwrap_or_default();
            (web::text(item, "title"), "", watch(s, ctx, &v, &tab, &back))
        }
        ["search"] => {
            let q = web::query(request, "q").unwrap_or_default();
            ("Search".into(), "", search(s, ctx, &q, &back))
        }
        _ => return web::error(404, "route not found"),
    };
    let mut theme = theme.clone();
    theme.background = Some(SKIN.page.into());
    theme.content_width = None;
    web::themed_page(
        &if title == "YouTube Music" {
            title.clone()
        } else {
            format!("{title} - YouTube Music")
        },
        theme,
        vec![
            top_bar(&ctx.actor),
            web::styled_row(
                "frame",
                24,
                "start",
                web::style().padding(8),
                vec![
                    sidebar(s, &ctx.actor, section),
                    web::column("main", 20, web::style().flex(4), main),
                ],
            ),
            player_bar(s, ctx, &back),
        ],
    )
}
fn top_bar(actor: &str) -> PageElement {
    web::styled_row(
        "topbar",
        16,
        "center",
        web::style().padding(10).background(SKIN.page),
        vec![
            web::card_action(
                "topbar-logo",
                web::style().width(120).padding(2).background(SKIN.page),
                web::visit("/"),
                vec![web::styled_row(
                    "topbar-logo-row",
                    6,
                    "center",
                    web::style(),
                    vec![
                        web::thumbnail(
                            "topbar-logo-mark",
                            "▶",
                            web::style()
                                .width(26)
                                .height(26)
                                .radius(13)
                                .background("#ff0000")
                                .color("#ffffff")
                                .size(10),
                        ),
                        bold("topbar-logo-name", "Music", 19, SKIN.ink),
                    ],
                )],
            ),
            web::card_action(
                "topbar-search",
                web::style()
                    .flex(4)
                    .padding(11)
                    .radius(8)
                    .background("#ffffff14")
                    .border("#ffffff1a"),
                web::visit("/search"),
                vec![kit::line(
                    "topbar-search-hint",
                    "Search songs, albums, artists, podcasts",
                    14,
                    SKIN.muted,
                )],
            ),
            web::spacer("topbar-space", 0),
            web::thumbnail(
                "topbar-avatar",
                actor
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default(),
                web::style()
                    .width(32)
                    .height(32)
                    .radius(16)
                    .background("#7e57c2")
                    .color("#ffffff")
                    .size(13),
            ),
        ],
    )
}
fn nav(id: &str, glyph: &str, label: &str, to: &str, on: bool) -> PageElement {
    web::card_action(
        id,
        web::style()
            .padding(10)
            .radius(8)
            .background(if on { SKIN.raised } else { "#00000000" }),
        web::visit(to),
        vec![web::styled_row(
            &format!("{id}-row"),
            16,
            "center",
            web::style(),
            vec![
                web::styled(
                    &format!("{id}-glyph"),
                    glyph,
                    web::style().size(18).color(SKIN.ink).width(22),
                ),
                bold(&format!("{id}-label"), label, 15, SKIN.ink),
            ],
        )],
    )
}
fn owner(p: &Value) -> String {
    match web::text(p, "owner") {
        o if o.is_empty() => "YouTube Music".into(),
        o => {
            let mut c = o.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        }
    }
}
fn sidebar(state: &Value, actor: &str, section: &str) -> PageElement {
    let mut rows = vec![
        nav("nav-home", "⌂", "Home", "/", section == "home"),
        nav(
            "nav-explore",
            "◎",
            "Explore",
            "/explore",
            section == "explore",
        ),
        nav(
            "nav-library",
            "♬",
            "Library",
            "/library",
            section == "library",
        ),
        web::divider("nav-divider"),
        web::card_action(
            "nav-new",
            web::style().padding(10).radius(20).background(SKIN.raised),
            web::visit("/library/playlists"),
            vec![kit::line_bold(
                "nav-new-text",
                "+  New playlist",
                14,
                SKIN.ink,
            )],
        ),
    ];
    let mut entry = |id: String, title: String, meta: String, to: String| {
        rows.push(web::card_action(
            &format!("nav-list-{id}"),
            web::style().padding(8).radius(8).background("#00000000"),
            web::visit(to),
            vec![
                text(&format!("nav-list-{id}-title"), title, 14, SKIN.ink),
                text(&format!("nav-list-{id}-meta"), meta, 12, SKIN.muted),
            ],
        ));
    };
    entry(
        "liked".into(),
        "Liked Music".into(),
        format!(
            "♪ Auto playlist • {}",
            kit::count(catalog::liked(state, actor).len(), "song", "songs")
        ),
        "/playlist?list=LM".into(),
    );
    for id in keys(state, "playlists") {
        let p = record(state, "playlists", &id)
            .cloned()
            .unwrap_or(Value::Null);
        entry(
            id.clone(),
            web::text(&p, "title"),
            owner(&p),
            format!("/playlist?list={id}"),
        );
    }
    web::column("nav", 4, web::style().width(220), rows)
}
fn heading(id: &str, title: &str) -> PageElement {
    web::styled(id, title, web::style().size(24).bold().color(SKIN.ink))
}
fn shelf(id: &str, title: &str, columns: u32, tiles: Vec<PageElement>) -> Vec<PageElement> {
    if tiles.is_empty() {
        return vec![];
    }
    vec![
        heading(&format!("{id}-heading"), title),
        web::grid(id, columns, 16, tiles),
    ]
}
/// Square (or, for an artist, round) art over a title and one line, the unit every
/// YouTube Music shelf repeats.
fn tile(id: &str, of: &str, title: &str, meta: &str, to: String, round: bool) -> PageElement {
    web::card_action(
        id,
        web::style().padding(0).radius(4).background("#00000000"),
        web::visit(to),
        vec![
            cover(
                &format!("{id}-art"),
                of,
                if round { "" } else { title },
                (if round { 150 } else { 0 }, 150),
                if round { 64 } else { 4 },
            ),
            web::styled(
                &format!("{id}-title"),
                title,
                web::style()
                    .size(15)
                    .medium()
                    .color(SKIN.ink)
                    .align(if round { "center" } else { "left" }),
            ),
            web::styled(
                &format!("{id}-meta"),
                meta,
                web::style().size(13).color(SKIN.muted).align(if round {
                    "center"
                } else {
                    "left"
                }),
            ),
        ],
    )
}
fn album_tile(prefix: &str, a: &catalog::Album) -> PageElement {
    tile(
        &format!("{prefix}-{}", a.id),
        &a.id,
        &a.title,
        &format!("{} • {}", a.kind(), a.artist_name),
        format!("/browse/{}", a.id),
        false,
    )
}
fn artist_tile(state: &Value, prefix: &str, id: &str) -> PageElement {
    let subs = record(state, "channels", id).map_or(0, |c| num(c, "subscribers"));
    tile(
        &format!("{prefix}-{id}"),
        id,
        &catalog::artist_name(state, id),
        &format!("{} subscribers", short(subs)),
        format!("/channel/{id}"),
        true,
    )
}
/// "184K", "1.2M": YouTube never prints a subscriber count in full.
fn short(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}K", n / 1_000),
        _ => {
            let tenths = n / 100_000;
            if tenths.is_multiple_of(10) {
                format!("{}M", tenths / 10)
            } else {
                format!("{}.{}M", tenths / 10, tenths % 10)
            }
        }
    }
}
/// A compact song entry: small art, title, and "artist • album" — the Quick picks unit.
fn song_entry(state: &Value, prefix: &str, id: &str, to: String) -> PageElement {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let album = catalog::album(state, &catalog::album_id(&item))
        .map(|a| a.title)
        .unwrap_or_default();
    web::card_action(
        &format!("{prefix}-{id}"),
        web::style().padding(4).radius(4).background("#00000000"),
        web::visit(to),
        vec![web::styled_row(
            &format!("{prefix}-{id}-row"),
            12,
            "center",
            web::style(),
            vec![
                cover(
                    &format!("{prefix}-{id}-art"),
                    &catalog::album_id(&item),
                    "",
                    (48, 48),
                    4,
                ),
                web::column(
                    &format!("{prefix}-{id}-text"),
                    0,
                    web::style().flex(1),
                    vec![
                        text(
                            &format!("{prefix}-{id}-title"),
                            web::text(&item, "title"),
                            15,
                            SKIN.ink,
                        ),
                        text(
                            &format!("{prefix}-{id}-meta"),
                            format!(
                                "{} • {album}",
                                catalog::artist_name(state, &web::text(&item, "channel"))
                            ),
                            13,
                            SKIN.muted,
                        ),
                    ],
                ),
            ],
        )],
    )
}
/// YouTube Music's mood chips, in its order, limited to the moods some song here carries.
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
fn moods(state: &Value) -> Vec<String> {
    let tags: Vec<String> = keys(state, "items")
        .iter()
        .filter_map(|id| record(state, "items", id))
        .flat_map(|i| web::strings(i, "tags"))
        .collect();
    MOODS
        .iter()
        .filter(|m| tags.iter().any(|t| t.eq_ignore_ascii_case(m)))
        .map(|m| (*m).to_owned())
        .collect()
}
fn label(tag: &str) -> String {
    let mut c = tag.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}
fn chip(id: &str, text_: &str, on: bool, to: String) -> PageElement {
    web::card_action(
        id,
        web::style()
            .padding(8)
            .radius(8)
            .background(if on { SKIN.ink } else { SKIN.raised }),
        web::visit(to),
        vec![web::styled(
            &format!("{id}-text"),
            text_,
            web::style()
                .size(14)
                .medium()
                .one_line()
                .color(if on { "#000000" } else { SKIN.ink }),
        )],
    )
}
fn home(state: &Value, ctx: &ServiceContext, mood: &str) -> Vec<PageElement> {
    let chips = moods(state)
        .iter()
        .map(|t| {
            let on = t.eq_ignore_ascii_case(mood);
            chip(
                &format!("mood-{}", slug(t)),
                &label(t),
                on,
                if on {
                    "/".into()
                } else {
                    format!("/?mood={}", encode(t))
                },
            )
        })
        .collect();
    let mut out = vec![kit::pills("moods", 8, chips)];
    let fits = |id: &String| {
        mood.is_empty()
            || record(state, "items", id).is_some_and(|i| {
                web::strings(i, "tags")
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(mood))
            })
    };
    let history: Vec<String> = strings_at(state, "history", &ctx.actor)
        .into_iter()
        .filter(|id| record(state, "items", id).is_some())
        .filter(fits)
        .take(6)
        .collect();
    let again: Vec<PageElement> = history
        .iter()
        .map(|id| {
            let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
            tile(
                &format!("again-{id}"),
                &catalog::album_id(&item),
                &web::text(&item, "title"),
                &format!(
                    "Song • {}",
                    catalog::artist_name(state, &web::text(&item, "channel"))
                ),
                format!("/watch?v={id}"),
                false,
            )
        })
        .collect();
    out.extend(shelf("home-again", "Listen again", 6, again));
    let context = if mood.is_empty() {
        "charts".to_owned()
    } else {
        format!("mood:{mood}")
    };
    let picks: Vec<PageElement> = catalog::charts(state)
        .into_iter()
        .filter(fits)
        .take(12)
        .map(|id| song_entry(state, "pick", &id, watch_url(&id, &context)))
        .collect();
    out.extend(shelf("home-picks", "Quick picks", 3, picks));
    let albums: Vec<PageElement> = catalog::albums(state)
        .iter()
        .filter(|a| a.tracks.iter().any(fits))
        .map(|a| album_tile("recommended", a))
        .collect();
    out.extend(shelf("home-albums", "Recommended albums", 5, albums));
    if mood.is_empty() {
        let lists: Vec<PageElement> = keys(state, "playlists")
            .iter()
            .filter_map(|id| record(state, "playlists", id).map(|p| (id, p)))
            .map(|(id, p)| {
                tile(
                    &format!("mix-{id}"),
                    id,
                    &web::text(p, "title"),
                    &format!("Playlist • {}", owner(p)),
                    format!("/playlist?list={id}"),
                    false,
                )
            })
            .collect();
        out.extend(shelf("home-mixes", "Mixed for you", 5, lists));
        let artists: Vec<PageElement> = keys(state, "channels")
            .iter()
            .map(|id| artist_tile(state, "artist", id))
            .collect();
        out.extend(shelf("home-artists", "Artists you might like", 5, artists));
    }
    out
}
fn explore(state: &Value, ctx: &ServiceContext, back: &str, part: &str) -> Vec<PageElement> {
    let mut out = vec![web::styled_row(
        "explore-buttons",
        12,
        "center",
        web::style(),
        [
            ("new_releases", "✚  New releases", "/explore/new_releases"),
            ("charts", "↑  Charts", "/explore/charts"),
            (
                "moods_and_genres",
                "☺  Moods & genres",
                "/explore/moods_and_genres",
            ),
        ]
        .iter()
        .map(|(id, text_, to)| {
            // The section on show is the one whose button is lit; pressing it again
            // goes back to the whole Explore page.
            let on = part == *id;
            web::card_action(
                &format!("explore-{id}"),
                web::style()
                    .padding(16)
                    .radius(8)
                    .background(if on { SKIN.ink } else { SKIN.raised })
                    .flex(1),
                web::visit(if on { "/explore" } else { *to }),
                vec![kit::line_bold(
                    &format!("explore-{id}-text"),
                    *text_,
                    16,
                    if on { "#000000" } else { SKIN.ink },
                )],
            )
        })
        .collect(),
    )];
    let show = |section: &str| part.is_empty() || part == section;
    if show("new_releases") {
        let albums: Vec<PageElement> = catalog::albums(state)
            .iter()
            .take(if part.is_empty() { 5 } else { 50 })
            .map(|a| album_tile("release", a))
            .collect();
        out.extend(shelf("explore-releases", "New albums & singles", 5, albums));
    }
    if show("moods_and_genres") {
        out.extend(explore_moods(state));
    }
    if show("charts") {
        out.extend(explore_charts(
            state,
            ctx,
            back,
            if part.is_empty() { 10 } else { 50 },
        ));
    }
    out
}
fn explore_moods(state: &Value) -> Vec<PageElement> {
    let mut out = vec![];
    out.push(heading("explore-moods-heading", "Moods & genres"));
    out.push(web::grid(
        "explore-moods",
        4,
        12,
        moods(state)
            .iter()
            .map(|t| {
                web::card_action(
                    &format!("genre-{}", slug(t)),
                    web::style().padding(0).radius(6).background("#292929"),
                    web::visit(format!("/?mood={}", encode(t))),
                    vec![web::styled_row(
                        &format!("genre-{}-row", slug(t)),
                        12,
                        "center",
                        web::style(),
                        vec![
                            web::thumbnail(
                                &format!("genre-{}-edge", slug(t)),
                                "",
                                web::style()
                                    .width(6)
                                    .height(44)
                                    .radius(0)
                                    .background(tint(&format!("mood{t}"))),
                            ),
                            bold(&format!("genre-{}-name", slug(t)), label(t), 14, SKIN.ink),
                        ],
                    )],
                )
            })
            .collect(),
    ));
    out
}
fn explore_charts(
    state: &Value,
    ctx: &ServiceContext,
    back: &str,
    limit: usize,
) -> Vec<PageElement> {
    let mut out = vec![heading("explore-charts-heading", "Top songs")];
    let playing =
        catalog::player(state, &ctx.actor, ctx.tick).and_then(|p| p.current().map(str::to_owned));
    for (rank, id) in catalog::charts(state).iter().take(limit).enumerate() {
        out.push(kit::track_row(
            state,
            &ctx.actor,
            "chart",
            id,
            rank + 1,
            "charts",
            playing.as_deref(),
            &SKIN,
            back,
            record(state, "items", id).map(|i| format!("{} plays", short(num(i, "plays")))),
            true,
        ));
    }
    out
}
fn library(state: &Value, ctx: &ServiceContext, tab: &str, back: &str) -> Vec<PageElement> {
    let actor = &ctx.actor;
    let mut out = vec![kit::pills(
        "library-chips",
        8,
        ["playlists", "songs", "albums", "artists"]
            .iter()
            .map(|t| {
                chip(
                    &format!("library-chip-{t}"),
                    &label(t),
                    *t == tab,
                    format!("/library/{t}"),
                )
            })
            .collect(),
    )];
    match tab {
        "songs" => {
            let songs = catalog::library_songs(state, actor);
            if songs.is_empty() {
                out.push(text(
                    "library-empty",
                    "Songs you save will show up here",
                    15,
                    SKIN.muted,
                ));
            } else {
                out.push(web::card_action(
                    "library-shuffle",
                    web::style()
                        .padding(10)
                        .radius(20)
                        .background(SKIN.ink)
                        .width(150),
                    kit::fields(&[
                        ("action", "play"),
                        ("context", "library"),
                        ("shuffle", "true"),
                        ("return", back),
                    ]),
                    vec![kit::line_bold(
                        "library-shuffle-text",
                        "⇄  Shuffle all",
                        14,
                        "#000000",
                    )],
                ));
                let playing = catalog::player(state, actor, ctx.tick)
                    .and_then(|p| p.current().map(str::to_owned));
                for (i, id) in songs.iter().enumerate() {
                    let album = record(state, "items", id)
                        .and_then(|item| catalog::album(state, &catalog::album_id(item)))
                        .map(|a| a.title);
                    out.push(kit::track_row(
                        state,
                        actor,
                        "song",
                        id,
                        i + 1,
                        "library",
                        playing.as_deref(),
                        &SKIN,
                        back,
                        album,
                        true,
                    ));
                }
            }
        }
        "albums" => {
            let library = strings_at(state, "library", actor);
            let tiles: Vec<PageElement> = catalog::albums(state)
                .iter()
                .filter(|a| a.tracks.iter().any(|t| library.contains(t)))
                .map(|a| album_tile("library-album", a))
                .collect();
            if tiles.is_empty() {
                out.push(text(
                    "library-empty",
                    "Albums you save will show up here",
                    15,
                    SKIN.muted,
                ));
            } else {
                out.push(web::grid("library-albums", 5, 16, tiles));
            }
        }
        "artists" => {
            let mut artists = strings_at(state, "subscriptions", actor);
            for id in catalog::library_songs(state, actor) {
                if let Some(a) = record(state, "items", &id).map(|i| web::text(i, "channel")) {
                    if !artists.contains(&a) {
                        artists.push(a);
                    }
                }
            }
            let tiles: Vec<PageElement> = artists
                .iter()
                .filter(|a| record(state, "channels", a).is_some())
                .map(|a| artist_tile(state, "library-artist", a))
                .collect();
            if tiles.is_empty() {
                out.push(text(
                    "library-empty",
                    "Artists you subscribe to will show up here",
                    15,
                    SKIN.muted,
                ));
            } else {
                out.push(web::grid("library-artists", 5, 16, tiles));
            }
        }
        _ => {
            let mut tiles = vec![tile(
                "library-liked",
                "liked",
                "Liked Music",
                "♪ Auto playlist",
                "/playlist?list=LM".into(),
                false,
            )];
            for id in keys(state, "playlists") {
                let p = record(state, "playlists", &id)
                    .cloned()
                    .unwrap_or(Value::Null);
                tiles.push(tile(
                    &format!("library-{id}"),
                    &id,
                    &web::text(&p, "title"),
                    &format!(
                        "{} • {}",
                        owner(&p),
                        kit::count(web::strings(&p, "items").len(), "track", "tracks")
                    ),
                    format!("/playlist?list={id}"),
                    false,
                ));
            }
            out.push(web::grid("library-playlists", 5, 16, tiles));
            out.push(web::form(
                "playlist",
                "/playlists",
                &[("title", "New playlist", "")],
            ));
        }
    }
    out
}
fn current(state: &Value, ctx: &ServiceContext) -> Option<catalog_player::Player> {
    catalog::player(state, &ctx.actor, ctx.tick)
}
use super::player as catalog_player;
fn round(id: &str, glyph: &str, fg: &str, bg: &str, action: PageAction) -> PageElement {
    kit::control(id, glyph, 18, fg, Some(bg), action)
}
#[allow(clippy::too_many_arguments)]
fn header(
    id: &str,
    of: &str,
    title: &str,
    lines: Vec<String>,
    buttons: Vec<PageElement>,
    round_art: bool,
) -> PageElement {
    let mut identity = vec![web::styled(
        &format!("{id}-title"),
        title,
        web::style().size(36).bold().color(SKIN.ink),
    )];
    for (i, line) in lines.into_iter().enumerate() {
        identity.push(text(&format!("{id}-line-{i}"), line, 14, SKIN.muted));
    }
    identity.push(kit::pills(&format!("{id}-buttons"), 12, buttons));
    web::styled_row(
        &format!("{id}-header"),
        32,
        "center",
        web::style().padding(8),
        vec![
            cover(
                &format!("{id}-art"),
                of,
                title,
                (220, 220),
                if round_art { 64 } else { 6 },
            ),
            web::column(&format!("{id}-identity"), 6, web::style().flex(3), identity),
        ],
    )
}
fn play_buttons(
    id: &str,
    context: &str,
    back: &str,
    p: Option<&catalog_player::Player>,
) -> Vec<PageElement> {
    let playing_this = p.is_some_and(|p| p.context == context && p.playing);
    vec![
        round(
            &format!("{id}-play"),
            if playing_this { "❚❚" } else { "▶" },
            "#000000",
            SKIN.ink,
            if playing_this {
                kit::fields(&[("action", "toggle"), ("return", back)])
            } else {
                kit::fields(&[("action", "play"), ("context", context), ("return", back)])
            },
        ),
        web::card_action(
            &format!("{id}-shuffle"),
            web::style()
                .padding(10)
                .radius(20)
                .border("#ffffff33")
                .background("#00000000"),
            kit::fields(&[
                ("action", "play"),
                ("context", context),
                ("shuffle", "true"),
                ("return", back),
            ]),
            vec![kit::line_bold(
                &format!("{id}-shuffle-text"),
                "⇄  Shuffle",
                14,
                SKIN.ink,
            )],
        ),
    ]
}
fn rows(
    state: &Value,
    ctx: &ServiceContext,
    prefix: &str,
    ids: &[String],
    context: &str,
    back: &str,
    art: bool,
) -> Vec<PageElement> {
    let playing = current(state, ctx).and_then(|p| p.current().map(str::to_owned));
    ids.iter()
        .enumerate()
        .map(|(i, id)| {
            kit::track_row(
                state,
                &ctx.actor,
                prefix,
                id,
                i + 1,
                context,
                playing.as_deref(),
                &SKIN,
                back,
                record(state, "items", id).map(|i| format!("{} plays", short(num(i, "plays")))),
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
    let library = strings_at(state, "library", &ctx.actor);
    let saved = album.tracks.iter().all(|t| library.contains(t));
    let p = current(state, ctx);
    let mut buttons = play_buttons("album", &context, back, p.as_ref());
    buttons.push(web::card_action(
        "album-save",
        web::style()
            .padding(10)
            .radius(20)
            .border("#ffffff33")
            .background("#00000000"),
        post(format!("/library/albums/{}", album.id), &[("return", back)]),
        vec![kit::line_bold(
            "album-save-text",
            if saved {
                "✓  Saved to library"
            } else {
                "+  Save to library"
            },
            14,
            SKIN.ink,
        )],
    ));
    let mut out = vec![header(
        "album",
        &album.id,
        &album.title,
        vec![
            format!("{} • {} • {}", album.kind(), album.artist_name, album.year),
            format!(
                "{} • {}",
                kit::count(album.tracks.len(), "song", "songs"),
                kit::running(state, &album.tracks)
            ),
        ],
        buttons,
        false,
    )];
    out.extend(rows(
        state,
        ctx,
        "track",
        &album.tracks,
        &context,
        back,
        false,
    ));
    out
}
fn playlist_page(state: &Value, ctx: &ServiceContext, list: &str, back: &str) -> Vec<PageElement> {
    let context = context_of(list);
    let items = catalog::context_tracks(state, &ctx.actor, &context);
    let (title, who, editable) = if list == "LM" {
        ("Liked Music".to_owned(), "Auto playlist".to_owned(), false)
    } else {
        let p = record(state, "playlists", list)
            .cloned()
            .unwrap_or(Value::Null);
        let o = web::text(&p, "owner");
        (
            web::text(&p, "title"),
            owner(&p),
            o.is_empty() || o == ctx.actor,
        )
    };
    let p = current(state, ctx);
    let mut out = vec![header(
        "playlist",
        if list == "LM" { "liked" } else { list },
        &title,
        vec![
            format!("Playlist • {who}"),
            format!(
                "{} • {}",
                kit::count(items.len(), "song", "songs"),
                kit::running(state, &items)
            ),
        ],
        if items.is_empty() {
            vec![]
        } else {
            play_buttons("playlist", &context, back, p.as_ref())
        },
        false,
    )];
    if items.is_empty() {
        out.push(text(
            "playlist-empty",
            "This playlist is empty. Add songs with Save to playlist on any song.",
            15,
            SKIN.muted,
        ));
    }
    for (i, row) in rows(state, ctx, "track", &items, &context, back, true)
        .into_iter()
        .enumerate()
    {
        if editable {
            let id = &items[i];
            out.push(web::styled_row(
                &format!("playlist-entry-{i}"),
                4,
                "center",
                web::style(),
                vec![
                    web::column(
                        &format!("playlist-entry-{i}-row"),
                        0,
                        web::style().flex(1),
                        vec![row],
                    ),
                    kit::control(
                        &format!("playlist-remove-{i}"),
                        "✕",
                        13,
                        SKIN.muted,
                        None,
                        post(
                            format!("/playlists/{list}/remove"),
                            &[("item", id), ("return", back)],
                        ),
                    ),
                ],
            ));
        } else {
            out.push(row);
        }
    }
    out
}
fn artist_page(state: &Value, ctx: &ServiceContext, id: &str, back: &str) -> Vec<PageElement> {
    let artist = record(state, "channels", id)
        .cloned()
        .unwrap_or(Value::Null);
    let name = web::text(&artist, "name");
    let subscribed = has(state, "subscriptions", &ctx.actor, id);
    let subs = num(&artist, "subscribers");
    let mut out = vec![web::card(
        "artist-banner",
        web::style().padding(28).radius(0).background(tint(id)),
        vec![
            web::styled(
                "artist-name",
                &name,
                web::style().size(48).bold().color(SKIN.ink),
            ),
            text(
                "artist-subscribers",
                format!("{} subscribers", short(subs)),
                14,
                SKIN.ink,
            ),
            kit::pills(
                "artist-buttons",
                12,
                vec![
                    web::card_action(
                        "artist-shuffle",
                        web::style().padding(10).radius(20).background(SKIN.ink),
                        kit::fields(&[
                            ("action", "play"),
                            ("context", &format!("artist:{id}")),
                            ("shuffle", "true"),
                            ("return", back),
                        ]),
                        vec![kit::line_bold(
                            "artist-shuffle-text",
                            "⇄  Shuffle",
                            14,
                            "#000000",
                        )],
                    ),
                    web::card_action(
                        "artist-radio",
                        web::style()
                            .padding(10)
                            .radius(20)
                            .border("#ffffff66")
                            .background("#00000000"),
                        kit::fields(&[
                            ("action", "play"),
                            ("context", &format!("station:{id}")),
                            ("return", back),
                        ]),
                        vec![kit::line_bold(
                            "artist-radio-text",
                            "◉  Radio",
                            14,
                            SKIN.ink,
                        )],
                    ),
                    web::card_action(
                        "artist-subscribe",
                        web::style()
                            .padding(10)
                            .radius(20)
                            .background(if subscribed { "#ffffff33" } else { SKIN.ink }),
                        post(format!("/channels/{id}/subscribe"), &[("return", back)]),
                        vec![kit::line_bold(
                            "artist-subscribe-text",
                            if subscribed {
                                format!("Subscribed {}", short(subs))
                            } else {
                                format!("Subscribe {}", short(subs))
                            },
                            14,
                            if subscribed { SKIN.ink } else { "#000000" },
                        )],
                    ),
                ],
            ),
        ],
    )];
    out.push(heading("artist-top-heading", "Top songs"));
    let top: Vec<String> = catalog::top_tracks(state, id).into_iter().take(5).collect();
    out.extend(rows(
        state,
        ctx,
        "top",
        &top,
        &format!("artist:{id}"),
        back,
        true,
    ));
    let albums: Vec<PageElement> = catalog::albums(state)
        .iter()
        .filter(|a| a.artist == id)
        .map(|a| album_tile("disc", a))
        .collect();
    out.extend(shelf("artist-albums", "Albums", 5, albums));
    let about = web::text(&artist, "about");
    if !about.is_empty() {
        out.push(heading("artist-about-heading", "About"));
        out.push(web::styled(
            "artist-about",
            about,
            web::style().size(14).color(SKIN.muted),
        ));
    }
    out
}
fn watch(state: &Value, ctx: &ServiceContext, v: &str, tab: &str, back: &str) -> Vec<PageElement> {
    let item = record(state, "items", v).cloned().unwrap_or(Value::Null);
    let p = current(state, ctx);
    // Tabs: the one on show is white over a white underline; LYRICS is greyed out because
    // no song in this catalogue has lyrics, which is exactly what YouTube Music does.
    let tab_cell = |id: &str, label: &str, on: bool, to: Option<String>| {
        let children = vec![
            web::styled(
                &format!("{id}-text"),
                label,
                web::style()
                    .size(13)
                    .bold()
                    .align("center")
                    .color(match (&to, on) {
                        (None, _) => "#ffffff4d",
                        (_, true) => SKIN.ink,
                        _ => SKIN.muted,
                    }),
            ),
            web::thumbnail(
                &format!("{id}-line"),
                "",
                web::style().height(2).radius(0).background(if on {
                    SKIN.ink
                } else {
                    "#ffffff1a"
                }),
            ),
        ];
        let style = web::style().padding(4).radius(0).background("#00000000");
        match to {
            Some(to) => web::card_action(id, style, web::visit(to), children),
            None => web::card(id, style, children),
        }
    };
    let tabs = web::grid(
        "tabs",
        3,
        0,
        vec![
            tab_cell(
                "tab-next",
                "UP NEXT",
                tab != "related",
                Some(format!("/watch?v={v}")),
            ),
            tab_cell("tab-lyrics", "LYRICS", false, None),
            tab_cell(
                "tab-related",
                "RELATED",
                tab == "related",
                Some(format!("/watch?v={v}&tab=related")),
            ),
        ],
    );
    let mut side = vec![tabs];
    if tab == "related" {
        for id in catalog::station(state, &web::text(&item, "channel"))
            .iter()
            .filter(|id| *id != v)
            .take(10)
        {
            side.push(song_entry(state, "related", id, format!("/watch?v={id}")));
        }
    } else if let Some(p) = &p {
        let from = catalog::context_title(state, &p.context);
        if !from.is_empty() {
            side.push(text(
                "queue-from",
                format!("Playing from {from}"),
                13,
                SKIN.muted,
            ));
        }
        for (i, id) in p.queue.iter().enumerate() {
            let row_item = record(state, "items", id).cloned().unwrap_or(Value::Null);
            let now = i == p.index;
            side.push(web::card_action(
                &format!("queue-{i}"),
                web::style().padding(6).radius(4).background(if now {
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
                            &catalog::album_id(&row_item),
                            if now { "▶" } else { "" },
                            (40, 40),
                            2,
                        ),
                        web::column(
                            &format!("queue-{i}-text"),
                            0,
                            web::style().flex(1),
                            vec![
                                text(
                                    &format!("queue-{i}-title"),
                                    web::text(&row_item, "title"),
                                    14,
                                    SKIN.ink,
                                ),
                                text(
                                    &format!("queue-{i}-artist"),
                                    catalog::artist_name(state, &web::text(&row_item, "channel")),
                                    12,
                                    SKIN.muted,
                                ),
                            ],
                        ),
                        text(
                            &format!("queue-{i}-time"),
                            clock(num(&row_item, "duration_s")),
                            12,
                            SKIN.muted,
                        ),
                    ],
                )],
            ));
        }
    }
    let album = catalog::album(state, &catalog::album_id(&item));
    vec![web::styled_row(
        "watch",
        32,
        "start",
        web::style(),
        vec![
            web::column(
                "watch-player",
                12,
                web::style().flex(3),
                vec![
                    cover(
                        "watch-art",
                        &catalog::album_id(&item),
                        &web::text(&item, "title"),
                        (0, 360),
                        4,
                    ),
                    bold("watch-title", web::text(&item, "title"), 20, SKIN.ink),
                    text(
                        "watch-meta",
                        format!(
                            "{} • {} • {}",
                            catalog::artist_name(state, &web::text(&item, "channel")),
                            album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
                            album.as_ref().map(|a| a.year.clone()).unwrap_or_default()
                        ),
                        14,
                        SKIN.muted,
                    ),
                ],
            ),
            web::column("watch-side", 6, web::style().flex(2), side),
        ],
    )]
}
fn search(state: &Value, ctx: &ServiceContext, q: &str, back: &str) -> Vec<PageElement> {
    let mut out = vec![web::form(
        "search",
        "/search",
        &[("q", "Search songs, albums, artists, podcasts", q)],
    )];
    if q.trim().is_empty() {
        out.push(heading("search-moods-heading", "Moods & genres"));
        out.push(kit::pills(
            "search-moods",
            8,
            moods(state)
                .iter()
                .map(|t| {
                    chip(
                        &format!("search-mood-{}", slug(t)),
                        &label(t),
                        false,
                        format!("/?mood={}", encode(t)),
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
        out.push(heading("results-none", "No results"));
        out.push(text(
            "results-hint",
            "Try different keywords, or check your spelling",
            14,
            SKIN.muted,
        ));
        return out;
    }
    if let Some(top) = artists.first() {
        out.push(heading("results-top-heading", "Top result"));
        out.push(web::card_action(
            "results-top",
            web::style().padding(16).radius(8).background(SKIN.raised),
            web::visit(format!("/channel/{top}")),
            vec![web::styled_row(
                "results-top-row",
                16,
                "center",
                web::style(),
                vec![
                    cover("results-top-art", top, "", (96, 96), 48),
                    web::column(
                        "results-top-text",
                        2,
                        web::style().flex(1),
                        vec![
                            bold(
                                "results-top-name",
                                catalog::artist_name(state, top),
                                22,
                                SKIN.ink,
                            ),
                            text(
                                "results-top-meta",
                                format!(
                                    "Artist • {} subscribers",
                                    short(
                                        record(state, "channels", top)
                                            .map_or(0, |c| num(c, "subscribers"))
                                    )
                                ),
                                14,
                                SKIN.muted,
                            ),
                        ],
                    ),
                ],
            )],
        ));
    }
    if !songs.is_empty() {
        out.push(heading("results-songs-heading", "Songs"));
        let playing = current(state, ctx).and_then(|p| p.current().map(str::to_owned));
        for (i, id) in songs.iter().take(8).enumerate() {
            let artist = record(state, "items", id)
                .map(|i| web::text(i, "channel"))
                .unwrap_or_default();
            out.push(kit::track_row(
                state,
                &ctx.actor,
                "result",
                id,
                i + 1,
                &format!("station:{artist}"),
                playing.as_deref(),
                &SKIN,
                back,
                record(state, "items", id).map(|i| format!("{} plays", short(num(i, "plays")))),
                true,
            ));
        }
    }
    let tiles: Vec<PageElement> = albums
        .iter()
        .filter_map(|a| catalog::album(state, a))
        .map(|a| album_tile("result-album", &a))
        .collect();
    out.extend(shelf("results-albums", "Albums", 5, tiles));
    let tiles: Vec<PageElement> = artists
        .iter()
        .map(|a| artist_tile(state, "result-artist", a))
        .collect();
    out.extend(shelf("results-artists", "Artists", 5, tiles));
    let tiles: Vec<PageElement> = playlists
        .iter()
        .filter_map(|id| record(state, "playlists", id).map(|p| (id, p)))
        .map(|(id, p)| {
            tile(
                &format!("result-playlist-{id}"),
                id,
                &web::text(p, "title"),
                &format!("Playlist • {}", owner(p)),
                format!("/playlist?list={id}"),
                false,
            )
        })
        .collect();
    out.extend(shelf("results-playlists", "Playlists", 5, tiles));
    out
}
fn player_bar(state: &Value, ctx: &ServiceContext, back: &str) -> PageElement {
    let style = web::style().background(BAR).padding(0).pin("bottom");
    let Some(now) = kit::loaded(state, &ctx.actor, ctx.tick) else {
        return web::styled_row(
            "bar",
            12,
            "center",
            style.padding(16),
            vec![text(
                "bar-idle",
                "Pick a song to start listening",
                13,
                SKIN.muted,
            )],
        );
    };
    let p = &now.player;
    let liked = has(state, "likes", &ctx.actor, &now.id);
    let album = now.album.as_ref();
    let mut left = kit::transport(p, &SKIN, back, ("#00000000", SKIN.ink));
    // YouTube Music keeps previous, play and next on the left; shuffle and repeat live on
    // the right, so split the five controls the way it does.
    let repeat = left.pop().expect("five controls");
    let shuffle = left.remove(0);
    left.push(text(
        "bar-time",
        format!(
            "{} / {}",
            kit::clock_ms(p.position_ms),
            kit::clock_ms(now.length_ms)
        ),
        12,
        SKIN.muted,
    ));
    let row = web::styled_row(
        "bar-row",
        16,
        "center",
        web::style().padding(8),
        vec![
            web::styled_row("bar-left", 4, "center", web::style().flex(2), left),
            web::styled_row(
                "bar-now",
                12,
                "center",
                web::style().flex(4),
                vec![
                    cover(
                        "bar-art",
                        &album.map(|a| a.id.clone()).unwrap_or_default(),
                        "",
                        (40, 40),
                        2,
                    ),
                    web::column(
                        "bar-text",
                        0,
                        web::style().flex(1),
                        vec![
                            web::card_action(
                                "bar-title",
                                web::style().padding(0).background("#00000000"),
                                web::visit(watch_url(&now.id, &p.context)),
                                vec![text("bar-title-text", &now.title, 14, SKIN.ink)],
                            ),
                            web::card_action(
                                "bar-meta",
                                web::style().padding(0).background("#00000000"),
                                web::visit(format!("/channel/{}", now.artist_id)),
                                vec![text(
                                    "bar-meta-text",
                                    match album {
                                        Some(a) => {
                                            format!("{} • {} • {}", now.artist, a.title, a.year)
                                        }
                                        None => now.artist.clone(),
                                    },
                                    12,
                                    SKIN.muted,
                                )],
                            ),
                        ],
                    ),
                    web::card_action(
                        "bar-like",
                        web::style()
                            .width(64)
                            .padding(8)
                            .radius(16)
                            .background(if liked { SKIN.raised } else { "#00000000" }),
                        post(format!("/items/{}/like", now.id), &[("return", back)]),
                        vec![kit::line_bold(
                            "bar-like-text",
                            if liked { "Liked" } else { "Like" },
                            12,
                            SKIN.ink,
                        )],
                    ),
                ],
            ),
            web::styled_row(
                "bar-right",
                4,
                "center",
                web::style().flex(2),
                vec![web::spacer("bar-right-space", 0), repeat, shuffle],
            ),
        ],
    );
    web::column(
        "bar",
        0,
        style,
        vec![
            web::styled_row(
                "bar-progress",
                0,
                "center",
                web::style(),
                kit::scrubber(p, now.length_ms, &SKIN, back),
            ),
            row,
        ],
    )
}
