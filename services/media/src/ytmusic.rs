//! music.youtube.com (`music` mode), laid out like YouTube Music on the web: near-black
//! page, a top bar with the logo and a wide search box, a sidebar with Home, Explore and
//! Library above your playlists, mood chips and shelves of square album art and round
//! artist avatars, an Up next queue beside the big artwork, and a player bar pinned to the
//! bottom whose red progress line is a real seek control.
use super::catalog;
use super::kit::{self, Row};
use super::player as catalog_player;
use super::view::{self, act, cover, div, el, field_form, here_aware, href, icon, press, short, span, Html};
use super::*;

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
            let already = current.as_ref().is_some_and(|p| p.current() == Some(v.as_str()) && p.context == context);
            if !already {
                catalog::command(state, ctx, &json!({"action": "play", "item": v, "context": context}))
                    .or_else(|_| catalog::command(state, ctx, &json!({"action": "play", "item": v})))
                    .map_err(cw_protocol::SimError::invalid)?;
            }
        }
    }
    let s: &Value = state;
    let (title, section, main) = match parts {
        [""] => {
            let mood = web::query(request, "mood").unwrap_or_default();
            ("YouTube Music".to_owned(), "home", home(s, ctx, &mood, &back))
        }
        ["explore"] => ("Explore".into(), "explore", explore(s, ctx, &back, "")),
        ["explore", part @ ("new_releases" | "charts" | "moods_and_genres")] => {
            ("Explore".into(), "explore", explore(s, ctx, &back, part))
        }
        ["library"] => ("Library".into(), "library", library(s, ctx, "playlists", &back)),
        ["library", tab @ ("playlists" | "songs" | "albums" | "artists")] => {
            ("Library".into(), "library", library(s, ctx, tab, &back))
        }
        ["browse", id] => match catalog::album(s, id) {
            Some(album) => (album.title.clone(), "", album_page(s, ctx, &album, &back)),
            None => return web::error(404, "album not found"),
        },
        ["channel", id] => match record(s, "channels", id) {
            Some(_) => (catalog::artist_name(s, id), "", artist_page(s, ctx, id, &back)),
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
                record(s, "playlists", &list).map(|p| web::text(p, "title")).unwrap_or_default()
            };
            (title, "", playlist_page(s, ctx, &list, &back))
        }
        ["watch"] => {
            let mut v = web::query(request, "v").unwrap_or_default();
            // A watch page keeping itself current follows the player on to the next song,
            // as YouTube Music's does.
            if request.header(cw_protocol::REFRESH_HEADER).is_some() {
                if let Some(now) = catalog::player(s, &ctx.actor, ctx.tick).and_then(|p| p.current().map(str::to_owned)) {
                    v = now;
                }
            }
            let Some(item) = record(s, "items", &v) else {
                return web::error(404, "song not found");
            };
            let tab = web::query(request, "tab").unwrap_or_default();
            (web::text(item, "title"), "watch", watch(s, ctx, &v, &tab, &back))
        }
        ["search"] => {
            let q = web::query(request, "q").unwrap_or_default();
            ("Search".into(), "", search(s, ctx, &q, &back))
        }
        _ => return web::error(404, "route not found"),
    };
    let doc = view::document(
        s,
        "music",
        &if title == "YouTube Music" { title.clone() } else { format!("{title} - YouTube Music") },
        &format!("page-{}", if section.is_empty() { "detail" } else { section }),
        vec![
            top_bar(&ctx.actor, &back),
            div("frame").id("frame").child(sidebar(s, &ctx.actor, section, &back)).child(el("main").id("main").children(main)),
            player_bar(s, ctx, &back),
        ],
    );
    Ok(kit::live(doc.response(), s, ctx, &back))
}
fn top_bar(actor: &str, here: &str) -> Html {
    el("header")
        .id("topbar")
        .class("topbar")
        .child(
            div("start").child(span("burger").attr("aria-hidden", "true").each(0..3, |_| el("i"))).child(
                here_aware(el("a").id("topbar-logo").class("logo").attr("href", "/").attr("aria-label", "YouTube Music"), "/", here)
                    .child(span("mark").id("topbar-logo-mark").attr("aria-hidden", "true").child(icon("play")))
                    .child(span("name").id("topbar-logo-name").text("Music")),
            ),
        )
        .child(
            here_aware(el("a").id("topbar-search").class("searchpill").attr("href", "/search"), "/search", here)
                .child(icon("search"))
                .child(span("hint").id("topbar-search-hint").text("Search songs, albums, artists, podcasts")),
        )
        .child(span("avatar").id("topbar-avatar").attr("aria-label", actor).text(view::initial(actor)))
}
fn nav(id: &str, glyph: &str, label: &str, to: &str, on: bool, here: &str) -> Html {
    here_aware(el("a").id(id).class(if on { "nav on" } else { "nav" }).attr("href", to), to, here).child(
        span("row")
            .id(format!("{id}-row"))
            .child(icon(glyph))
            .child(span("label").id(format!("{id}-label")).text(label)),
    )
}
fn owner(p: &Value) -> String {
    match web::text(p, "owner") {
        o if o.is_empty() => "YouTube Music".into(),
        o => label(&o),
    }
}
fn sidebar(state: &Value, actor: &str, section: &str, here: &str) -> Html {
    let entry = |id: &str, title: String, meta: String, to: String| {
        here_aware(el("a").id(format!("nav-list-{id}")).class("list").attr("href", to.as_str()), &to, here)
            .child(span("title").id(format!("nav-list-{id}-title")).text(title))
            .child(span("meta").id(format!("nav-list-{id}-meta")).text(meta))
    };
    el("nav")
        .id("nav")
        .class("side")
        .child(nav("nav-home", "home", "Home", "/", section == "home", here))
        .child(nav("nav-explore", "compass", "Explore", "/explore", section == "explore", here))
        .child(nav("nav-library", "library", "Library", "/library", section == "library", here))
        .child(el("hr").id("nav-divider"))
        .child(
            here_aware(el("a").id("nav-new").class("pill").attr("href", "/library/playlists"), "/library/playlists", here)
                .child(span("label").id("nav-new-text").child(icon("plus")).child(span("").id("nav-new-text-label").text("New playlist"))),
        )
        .child(entry(
            "liked",
            "Liked Music".into(),
            format!("♪ Auto playlist • {}", kit::count(catalog::liked(state, actor).len(), "song", "songs")),
            "/playlist?list=LM".into(),
        ))
        .each(keys(state, "playlists"), |id| {
            let p = record(state, "playlists", &id).cloned().unwrap_or(Value::Null);
            entry(&id, web::text(&p, "title"), owner(&p), href("/playlist", &[("list", &id)]))
        })
}
fn heading(id: &str, title: &str) -> Html {
    el("h2").id(id).class("heading").text(title)
}
/// A titled shelf of tiles on one line that scrolls sideways, as YouTube Music's do.
fn shelf(id: &str, title: &str, class: &str, tiles: Vec<Html>) -> Vec<Html> {
    if tiles.is_empty() {
        return vec![];
    }
    vec![heading(&format!("{id}-heading"), title), div(&format!("shelf {class}")).id(id).children(tiles)]
}
/// Square (or, for an artist, round) art over a title and one line, the unit every
/// YouTube Music shelf repeats.
fn tile(id: &str, of: &str, title: &str, meta: &str, to: String, round: bool) -> Html {
    el("a")
        .id(id)
        .class(if round { "tile round" } else { "tile" })
        .attr("href", to)
        .child(if of == "liked" {
            span("likedart large").id(format!("{id}-art")).attr("role", "img").attr("aria-label", "Liked Music").child(icon("like-fill"))
        } else {
            cover(&format!("{id}-art"), of, if round { "" } else { title }, 150, if round { 75 } else { 4 })
        })
        .child(span("title").id(format!("{id}-title")).text(title))
        .child(span("meta").id(format!("{id}-meta")).text(meta))
}
fn album_tile(prefix: &str, a: &catalog::Album) -> Html {
    tile(
        &format!("{prefix}-{}", a.id),
        &a.id,
        &a.title,
        &format!("{} • {}", a.kind(), a.artist_name),
        format!("/browse/{}", a.id),
        false,
    )
}
fn artist_tile(state: &Value, prefix: &str, id: &str) -> Html {
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
/// A compact song entry: small art, title, and "artist • album" — the Quick picks unit.
fn song_entry(state: &Value, prefix: &str, id: &str, to: String) -> Html {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let album = catalog::album(state, &catalog::album_id(&item)).map(|a| a.title).unwrap_or_default();
    el("a").id(format!("{prefix}-{id}")).class("song").attr("href", to).child(
        span("row")
            .id(format!("{prefix}-{id}-row"))
            .child(cover(&format!("{prefix}-{id}-art"), &catalog::album_id(&item), "", 48, 4))
            .child(
                span("names")
                    .id(format!("{prefix}-{id}-text"))
                    .child(span("title").id(format!("{prefix}-{id}-title")).text(web::text(&item, "title")))
                    .child(span("meta").id(format!("{prefix}-{id}-meta")).text(format!(
                        "{} • {album}",
                        catalog::artist_name(state, &web::text(&item, "channel"))
                    ))),
            ),
    )
}
/// YouTube Music's mood chips, in its order, limited to the moods some song here carries.
const MOODS: &[&str] = &["energize", "relax", "feel good", "workout", "commute", "focus", "party", "sleep"];
fn moods(state: &Value) -> Vec<String> {
    let tags: Vec<String> = keys(state, "items")
        .iter()
        .filter_map(|id| record(state, "items", id))
        .flat_map(|i| web::strings(i, "tags"))
        .collect();
    MOODS.iter().filter(|m| tags.iter().any(|t| t.eq_ignore_ascii_case(m))).map(|m| (*m).to_owned()).collect()
}
fn label(tag: &str) -> String {
    let mut c = tag.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
fn chip(id: &str, text_: &str, on: bool, to: String, here: &str) -> Html {
    here_aware(
        el("a").id(id).class(if on { "chip on" } else { "chip" }).attr("href", to.as_str()).child(span("").id(format!("{id}-text")).text(text_)),
        &to,
        here,
    )
}
fn home(state: &Value, ctx: &ServiceContext, mood: &str, here: &str) -> Vec<Html> {
    let mut out = vec![div("chips").id("moods").each(moods(state).iter(), |t| {
        let on = t.eq_ignore_ascii_case(mood);
        chip(&format!("mood-{}", slug(t)), &label(t), on, if on { "/".into() } else { href("/", &[("mood", t)]) }, here)
    })];
    let fits = |id: &String| {
        mood.is_empty()
            || record(state, "items", id).is_some_and(|i| web::strings(i, "tags").iter().any(|t| t.eq_ignore_ascii_case(mood)))
    };
    let history: Vec<String> = strings_at(state, "history", &ctx.actor)
        .into_iter()
        .filter(|id| record(state, "items", id).is_some())
        .filter(fits)
        .take(6)
        .collect();
    let again: Vec<Html> = history
        .iter()
        .map(|id| {
            let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
            tile(
                &format!("again-{id}"),
                &catalog::album_id(&item),
                &web::text(&item, "title"),
                &format!("Song • {}", catalog::artist_name(state, &web::text(&item, "channel"))),
                format!("/watch?v={id}"),
                false,
            )
        })
        .collect();
    out.extend(shelf("home-again", "Listen again", "", again));
    let context = if mood.is_empty() { "charts".to_owned() } else { format!("mood:{mood}") };
    let picks: Vec<Html> = catalog::charts(state)
        .into_iter()
        .filter(fits)
        .take(12)
        .map(|id| song_entry(state, "pick", &id, watch_url(&id, &context)))
        .collect();
    out.extend(shelf("home-picks", "Quick picks", "picks", picks));
    let albums: Vec<Html> = catalog::albums(state)
        .iter()
        .filter(|a| a.tracks.iter().any(fits))
        .map(|a| album_tile("recommended", a))
        .collect();
    out.extend(shelf("home-albums", "Recommended albums", "", albums));
    if mood.is_empty() {
        let lists: Vec<Html> = keys(state, "playlists")
            .iter()
            .filter_map(|id| record(state, "playlists", id).map(|p| (id, p)))
            .map(|(id, p)| {
                tile(
                    &format!("mix-{id}"),
                    id,
                    &web::text(p, "title"),
                    &format!("Playlist • {}", owner(p)),
                    href("/playlist", &[("list", id)]),
                    false,
                )
            })
            .collect();
        out.extend(shelf("home-mixes", "Mixed for you", "", lists));
        let artists: Vec<Html> = keys(state, "channels").iter().map(|id| artist_tile(state, "artist", id)).collect();
        out.extend(shelf("home-artists", "Artists you might like", "", artists));
    }
    out
}
fn explore(state: &Value, ctx: &ServiceContext, back: &str, part: &str) -> Vec<Html> {
    let mut out = vec![div("explore-buttons").id("explore-buttons").each(
        [
            ("new_releases", "release", "New releases", "/explore/new_releases"),
            ("charts", "chart", "Charts", "/explore/charts"),
            ("moods_and_genres", "mood", "Moods & genres", "/explore/moods_and_genres"),
        ],
        |(id, glyph, text_, to)| {
            // The section on show is the one whose button is lit; pressing it again
            // goes back to the whole Explore page.
            let on = part == id;
            el("a")
                .id(format!("explore-{id}"))
                .class(if on { "bigbutton on" } else { "bigbutton" })
                .attr("href", if on { "/explore" } else { to })
                .child(icon(glyph))
                .child(span("").id(format!("explore-{id}-text")).text(text_))
        },
    )];
    let show = |section: &str| part.is_empty() || part == section;
    if show("new_releases") {
        let albums: Vec<Html> = catalog::albums(state)
            .iter()
            .take(if part.is_empty() { 5 } else { 50 })
            .map(|a| album_tile("release", a))
            .collect();
        out.extend(shelf("explore-releases", "New albums & singles", if part.is_empty() { "" } else { "wrap" }, albums));
    }
    if show("moods_and_genres") {
        out.push(heading("explore-moods-heading", "Moods & genres"));
        out.push(div("genres").id("explore-moods").each(moods(state).iter(), |t| {
            el("a")
                .id(format!("genre-{}", slug(t)))
                .class("genre")
                .attr("href", href("/", &[("mood", t)]))
                .child(
                    span("row")
                        .id(format!("genre-{}-row", slug(t)))
                        .child(
                            span("edge")
                                .id(format!("genre-{}-edge", slug(t)))
                                .style(&format!("background-color: {}", tint(&format!("mood{t}")))),
                        )
                        .child(span("name").id(format!("genre-{}-name", slug(t))).text(label(t))),
                )
        }));
    }
    if show("charts") {
        out.push(heading("explore-charts-heading", "Top songs"));
        let playing = catalog::player(state, &ctx.actor, ctx.tick).and_then(|p| p.current().map(str::to_owned));
        let row = Row { prefix: "chart", context: "charts", playing: playing.as_deref(), back, art: true };
        out.push(div("tracks").id("explore-chart-tracks").each(
            catalog::charts(state).iter().take(if part.is_empty() { 10 } else { 50 }).enumerate(),
            |(rank, id)| {
                kit::track_row(
                    state,
                    &ctx.actor,
                    &row,
                    id,
                    rank + 1,
                    record(state, "items", id).map(|i| format!("{} plays", short(num(i, "plays")))),
                )
            },
        ));
    }
    out
}
fn library(state: &Value, ctx: &ServiceContext, tab: &str, back: &str) -> Vec<Html> {
    let actor = &ctx.actor;
    let mut out = vec![
        heading("library-heading", "Library"),
        div("chips").id("library-chips").each(["playlists", "songs", "albums", "artists"], |t| {
            chip(&format!("library-chip-{t}"), &label(t), t == tab, format!("/library/{t}"), back)
        }),
    ];
    let nothing = |text_: &str| el("p").id("library-empty").class("muted").text(text_);
    match tab {
        "songs" => {
            let songs = catalog::library_songs(state, actor);
            if songs.is_empty() {
                out.push(nothing("Songs you save will show up here"));
            } else {
                out.push(kit::command(
                    "library-shuffle",
                    &[("action", "play"), ("context", "library"), ("shuffle", "true"), ("return", back)],
                    press("solid", "").child(
                        span("label")
                            .id("library-shuffle-text")
                            .child(icon("shuffle"))
                            .child(span("").id("library-shuffle-text-label").text("Shuffle all")),
                    ),
                ));
                let playing = catalog::player(state, actor, ctx.tick).and_then(|p| p.current().map(str::to_owned));
                let row = Row { prefix: "song", context: "library", playing: playing.as_deref(), back, art: true };
                out.push(div("tracks").id("library-songs").each(songs.iter().enumerate(), |(i, id)| {
                    let album = record(state, "items", id)
                        .and_then(|item| catalog::album(state, &catalog::album_id(item)))
                        .map(|a| a.title);
                    kit::track_row(state, actor, &row, id, i + 1, album)
                }));
            }
        }
        "albums" => {
            let library = strings_at(state, "library", actor);
            let tiles: Vec<Html> = catalog::albums(state)
                .iter()
                .filter(|a| a.tracks.iter().any(|t| library.contains(t)))
                .map(|a| album_tile("library-album", a))
                .collect();
            if tiles.is_empty() {
                out.push(nothing("Albums you save will show up here"));
            } else {
                out.push(div("tilegrid").id("library-albums").children(tiles));
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
            let tiles: Vec<Html> = artists
                .iter()
                .filter(|a| record(state, "channels", a).is_some())
                .map(|a| artist_tile(state, "library-artist", a))
                .collect();
            if tiles.is_empty() {
                out.push(nothing("Artists you subscribe to will show up here"));
            } else {
                out.push(div("tilegrid").id("library-artists").children(tiles));
            }
        }
        _ => {
            let mut tiles = vec![tile("library-liked", "liked", "Liked Music", "Auto playlist", "/playlist?list=LM".into(), false)];
            for id in keys(state, "playlists") {
                let p = record(state, "playlists", &id).cloned().unwrap_or(Value::Null);
                tiles.push(tile(
                    &format!("library-{id}"),
                    &id,
                    &web::text(&p, "title"),
                    &format!("{} • {}", owner(&p), kit::count(web::strings(&p, "items").len(), "track", "tracks")),
                    href("/playlist", &[("list", &id)]),
                    false,
                ));
            }
            out.push(div("tilegrid").id("library-playlists").children(tiles));
            out.push(heading("playlist-heading", "New playlist"));
            out.push(field_form("playlist", "/playlists", "post", "title", "New playlist", "", "Create").class("createform"));
        }
    }
    out
}
fn current(state: &Value, ctx: &ServiceContext) -> Option<catalog_player::Player> {
    catalog::player(state, &ctx.actor, ctx.tick)
}
/// The header of an album or playlist page: big art, title, two lines, its buttons.
fn header(id: &str, of: &str, title: &str, lines: Vec<String>, buttons: Vec<Html>) -> Html {
    div("header")
        .id(format!("{id}-header"))
        .child(if of == "liked" {
            span("likedart huge").id(format!("{id}-art")).attr("role", "img").attr("aria-label", "Liked Music").child(icon("like-fill"))
        } else {
            cover(&format!("{id}-art"), of, title, 220, 6)
        })
        .child(
            div("identity")
                .id(format!("{id}-identity"))
                .child(el("h1").id(format!("{id}-title")).text(title))
                .each(lines.into_iter().enumerate(), |(i, line)| span("line").id(format!("{id}-line-{i}")).text(line))
                .child(div("buttons").id(format!("{id}-buttons")).children(buttons)),
        )
}
fn play_buttons(id: &str, context: &str, back: &str, p: Option<&catalog_player::Player>) -> Vec<Html> {
    let playing_this = p.is_some_and(|p| p.context == context && p.playing);
    let (glyph, name) = if playing_this { ("pause", "Pause") } else { ("play", "Play") };
    let play = press("ctl bigplay", name).child(icon(glyph));
    vec![
        if playing_this {
            kit::command(&format!("{id}-play"), &[("action", "toggle"), ("return", back)], play)
        } else {
            kit::command(&format!("{id}-play"), &[("action", "play"), ("context", context), ("return", back)], play)
        },
        kit::command(
            &format!("{id}-shuffle"),
            &[("action", "play"), ("context", context), ("shuffle", "true"), ("return", back)],
            press("outline", "").child(
                span("label")
                    .id(format!("{id}-shuffle-row"))
                    .child(icon("shuffle"))
                    .child(span("").id(format!("{id}-shuffle-text")).text("Shuffle")),
            ),
        ),
    ]
}
fn rows(state: &Value, ctx: &ServiceContext, prefix: &str, ids: &[String], context: &str, back: &str, art: bool) -> Vec<Html> {
    let playing = current(state, ctx).and_then(|p| p.current().map(str::to_owned));
    let row = Row { prefix, context, playing: playing.as_deref(), back, art };
    ids.iter()
        .enumerate()
        .map(|(i, id)| {
            kit::track_row(
                state,
                &ctx.actor,
                &row,
                id,
                i + 1,
                record(state, "items", id).map(|i| format!("{} plays", short(num(i, "plays")))),
            )
        })
        .collect()
}
fn album_page(state: &Value, ctx: &ServiceContext, album: &catalog::Album, back: &str) -> Vec<Html> {
    let context = format!("album:{}", album.id);
    let library = strings_at(state, "library", &ctx.actor);
    let saved = album.tracks.iter().all(|t| library.contains(t));
    let p = current(state, ctx);
    let mut buttons = play_buttons("album", &context, back, p.as_ref());
    buttons.push(act(
        "album-save",
        &format!("/library/albums/{}", album.id),
        &[("return", back)],
        press("outline", "").child(
            span("label")
                .id("album-save-text")
                .child(icon(if saved { "check" } else { "plus" }))
                .child(span("").id("album-save-text-label").text(if saved { "Saved to library" } else { "Save to library" })),
        ),
    ));
    vec![
        header(
            "album",
            &album.id,
            &album.title,
            vec![
                format!("{} • {} • {}", album.kind(), album.artist_name, album.year),
                format!("{} • {}", kit::count(album.tracks.len(), "song", "songs"), kit::running(state, &album.tracks)),
            ],
            buttons,
        ),
        div("tracks").id("tracks").children(rows(state, ctx, "track", &album.tracks, &context, back, false)),
    ]
}
fn playlist_page(state: &Value, ctx: &ServiceContext, list: &str, back: &str) -> Vec<Html> {
    let context = context_of(list);
    let items = catalog::context_tracks(state, &ctx.actor, &context);
    let (title, who, editable) = if list == "LM" {
        ("Liked Music".to_owned(), "Auto playlist".to_owned(), false)
    } else {
        let p = record(state, "playlists", list).cloned().unwrap_or(Value::Null);
        let o = web::text(&p, "owner");
        (web::text(&p, "title"), owner(&p), o.is_empty() || o == ctx.actor)
    };
    let p = current(state, ctx);
    let mut out = vec![header(
        "playlist",
        if list == "LM" { "liked" } else { list },
        &title,
        vec![
            format!("Playlist • {who}"),
            format!("{} • {}", kit::count(items.len(), "song", "songs"), kit::running(state, &items)),
        ],
        if items.is_empty() { vec![] } else { play_buttons("playlist", &context, back, p.as_ref()) },
    )];
    if items.is_empty() {
        out.push(
            el("p")
                .id("playlist-empty")
                .class("muted")
                .text("This playlist is empty. Add songs with Save to playlist on any song."),
        );
    }
    let mut table = div("tracks").id("tracks");
    for (i, row) in rows(state, ctx, "track", &items, &context, back, true).into_iter().enumerate() {
        if editable {
            table = table.child(
                div("entry")
                    .id(format!("playlist-entry-{i}"))
                    .child(div("grow").id(format!("playlist-entry-{i}-row")).child(row))
                    .child(act(
                        &format!("playlist-remove-{i}"),
                        &format!("/playlists/{list}/remove"),
                        &[("item", &items[i]), ("return", back)],
                        press("ctl", "Remove from playlist").child(icon("close")),
                    )),
            );
        } else {
            table = table.child(row);
        }
    }
    out.push(table);
    out
}
fn artist_page(state: &Value, ctx: &ServiceContext, id: &str, back: &str) -> Vec<Html> {
    let artist = record(state, "channels", id).cloned().unwrap_or(Value::Null);
    let name = web::text(&artist, "name");
    let subscribed = has(state, "subscriptions", &ctx.actor, id);
    let subs = num(&artist, "subscribers");
    let mut out = vec![div("banner")
        .id("artist-banner")
        .style(&format!("--wash: {}", kit::shade(&kit::wash(id), 80)))
        .child(view::still("artist-banner-art", id, "", 512))
        .child(
            div("identity")
                .child(el("h1").id("artist-name").text(name.as_str()))
                .child(span("line").id("artist-subscribers").text(format!("{} subscribers", short(subs))))
                .child(
                    div("buttons")
                        .id("artist-buttons")
                        .child(kit::command(
                            "artist-shuffle",
                            &[("action", "play"), ("context", &format!("artist:{id}")), ("shuffle", "true"), ("return", back)],
                            press("solid", "").child(
                                span("label")
                                    .id("artist-shuffle-text")
                                    .child(icon("shuffle"))
                                    .child(span("").id("artist-shuffle-text-label").text("Shuffle")),
                            ),
                        ))
                        .child(kit::command(
                            "artist-radio",
                            &[("action", "play"), ("context", &format!("station:{id}")), ("return", back)],
                            press("outline", "").child(span("label").child(icon("radio")).child(span("").id("artist-radio-text").text("Radio"))),
                        ))
                        .child(act(
                            "artist-subscribe",
                            &format!("/channels/{id}/subscribe"),
                            &[("return", back)],
                            press(if subscribed { "subscribe on" } else { "subscribe" }, "").child(span("").id("artist-subscribe-text").text(
                                if subscribed { format!("Subscribed {}", short(subs)) } else { format!("Subscribe {}", short(subs)) },
                            )),
                        )),
                ),
        )];
    out.push(heading("artist-top-heading", "Top songs"));
    let top: Vec<String> = catalog::top_tracks(state, id).into_iter().take(5).collect();
    out.push(div("tracks").id("artist-top").children(rows(state, ctx, "top", &top, &format!("artist:{id}"), back, true)));
    let albums: Vec<Html> = catalog::albums(state).iter().filter(|a| a.artist == id).map(|a| album_tile("disc", a)).collect();
    out.extend(shelf("artist-albums", "Albums", "", albums));
    let about = web::text(&artist, "about");
    if !about.is_empty() {
        out.push(heading("artist-about-heading", "About"));
        out.push(el("p").id("artist-about").class("about").text(about));
    }
    out
}
fn watch(state: &Value, ctx: &ServiceContext, v: &str, tab: &str, back: &str) -> Vec<Html> {
    let item = record(state, "items", v).cloned().unwrap_or(Value::Null);
    let p = current(state, ctx);
    // Tabs: the one on show is white over a white underline; LYRICS is greyed out when
    // the song has no lyrics, which is exactly what YouTube Music does.
    let tab_cell = |id: &str, label: &str, on: bool, to: Option<String>| {
        let inner = [span("").id(format!("{id}-text")).text(label), span("line").id(format!("{id}-line"))];
        match to {
            Some(to) => here_aware(el("a").id(id).class(if on { "tab on" } else { "tab" }).attr("href", to.as_str()).children(inner), &to, back),
            None => span("tab off").id(id).children(inner),
        }
    };
    // A tab keeps the list the song is playing from, so switching tabs never restarts it.
    let here = match &p {
        Some(p) if p.current() == Some(v) => watch_url(v, &p.context),
        _ => format!("/watch?v={v}"),
    };
    let lines = catalog::lyrics(&item);
    let mut side = div("upnext").id("watch-side").child(
        div("tabs")
            .id("tabs")
            .child(tab_cell("tab-next", "UP NEXT", tab != "related" && tab != "lyrics", Some(here.clone())))
            .child(tab_cell("tab-lyrics", "LYRICS", tab == "lyrics", (!lines.is_empty()).then(|| format!("{here}&tab=lyrics"))))
            .child(tab_cell("tab-related", "RELATED", tab == "related", Some(format!("{here}&tab=related")))),
    );
    if tab == "lyrics" && !lines.is_empty() {
        // Time-synced when this is the song playing: the sung line is lit.
        let at = p.as_ref().filter(|p| p.current() == Some(v)).and_then(|p| catalog::sung(&lines, p.position_ms));
        side = side
            .child(div("lyrics").id("lyrics").children(kit::lyric_lines("lyric", &lines, at, back)))
            .child(el("p").id("lyrics-source").class("fine").text("Lyrics synced to the music"));
    } else if tab == "related" {
        for id in catalog::station(state, &web::text(&item, "channel")).iter().filter(|id| *id != v).take(10) {
            side = side.child(song_entry(state, "related", id, format!("/watch?v={id}")));
        }
    } else if let Some(p) = &p {
        let from = catalog::context_title(state, &p.context);
        if !from.is_empty() {
            side = side.child(el("p").id("queue-from").class("from").text(format!("Playing from {from}")));
        }
        for (i, id) in p.queue.iter().enumerate() {
            let row_item = record(state, "items", id).cloned().unwrap_or(Value::Null);
            let now = i == p.index;
            side = side.child(kit::command(
                &format!("queue-{i}"),
                &[("action", "jump"), ("index", &i.to_string()), ("return", back)],
                press(if now { "qrow current" } else { "qrow" }, "").child(
                    span("row")
                        .id(format!("queue-{i}-row"))
                        .child(cover(&format!("queue-{i}-art"), &catalog::album_id(&row_item), if now { "Playing" } else { "" }, 40, 2))
                        .child(
                            span("names")
                                .id(format!("queue-{i}-text"))
                                .child(span("title").id(format!("queue-{i}-title")).text(web::text(&row_item, "title")))
                                .child(
                                    span("artist")
                                        .id(format!("queue-{i}-artist"))
                                        .text(catalog::artist_name(state, &web::text(&row_item, "channel"))),
                                ),
                        )
                        .child(span("dur").id(format!("queue-{i}-time")).text(clock(num(&row_item, "duration_s")))),
                ),
            ));
        }
    }
    let album = catalog::album(state, &catalog::album_id(&item));
    vec![div("watch")
        .id("watch")
        .child(
            div("stage")
                .id("watch-player")
                .child(cover("watch-art", &catalog::album_id(&item), &web::text(&item, "title"), 360, 4))
                .child(el("h1").id("watch-title").text(web::text(&item, "title")))
                .child(el("p").id("watch-meta").class("muted").text(format!(
                    "{} • {} • {}",
                    catalog::artist_name(state, &web::text(&item, "channel")),
                    album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
                    album.as_ref().map(|a| a.year.clone()).unwrap_or_default()
                ))),
        )
        .child(side)]
}
fn search(state: &Value, ctx: &ServiceContext, q: &str, back: &str) -> Vec<Html> {
    let mut out =
        vec![field_form("search", "/search", "get", "q", "Search songs, albums, artists, podcasts", q, "Search").class("searchform")];
    if q.trim().is_empty() {
        out.push(heading("search-moods-heading", "Moods & genres"));
        out.push(div("chips").id("search-moods").each(moods(state).iter(), |t| {
            chip(&format!("search-mood-{}", slug(t)), &label(t), false, href("/", &[("mood", t)]), back)
        }));
        return out;
    }
    let hits = catalog::search(state, q);
    let list = |k: &str| web::strings(&hits, k);
    let (songs, albums, artists, playlists) = (list("tracks"), list("albums"), list("artists"), list("playlists"));
    if songs.is_empty() && albums.is_empty() && artists.is_empty() && playlists.is_empty() {
        out.push(heading("results-none", "No results"));
        out.push(el("p").id("results-hint").class("muted").text("Try different keywords, or check your spelling"));
        return out;
    }
    if let Some(top) = artists.first() {
        out.push(heading("results-top-heading", "Top result"));
        out.push(
            el("a").id("results-top").class("topresult").attr("href", format!("/channel/{top}")).child(
                span("row")
                    .id("results-top-row")
                    .child(cover("results-top-art", top, "", 96, 48))
                    .child(
                        span("names")
                            .id("results-top-text")
                            .child(span("title").id("results-top-name").text(catalog::artist_name(state, top)))
                            .child(span("meta").id("results-top-meta").text(format!(
                                "Artist • {} subscribers",
                                short(record(state, "channels", top).map_or(0, |c| num(c, "subscribers")))
                            ))),
                    ),
            ),
        );
    }
    if !songs.is_empty() {
        out.push(heading("results-songs-heading", "Songs"));
        let playing = current(state, ctx).and_then(|p| p.current().map(str::to_owned));
        let mut table = div("tracks").id("results-songs");
        for (i, id) in songs.iter().take(8).enumerate() {
            let artist = record(state, "items", id).map(|i| web::text(i, "channel")).unwrap_or_default();
            let context = format!("station:{artist}");
            let row = Row { prefix: "result", context: &context, playing: playing.as_deref(), back, art: true };
            table = table.child(kit::track_row(
                state,
                &ctx.actor,
                &row,
                id,
                i + 1,
                record(state, "items", id).map(|i| format!("{} plays", short(num(i, "plays")))),
            ));
        }
        out.push(table);
    }
    let tiles: Vec<Html> = albums.iter().filter_map(|a| catalog::album(state, a)).map(|a| album_tile("result-album", &a)).collect();
    out.extend(shelf("results-albums", "Albums", "", tiles));
    let tiles: Vec<Html> = artists.iter().map(|a| artist_tile(state, "result-artist", a)).collect();
    out.extend(shelf("results-artists", "Artists", "", tiles));
    let tiles: Vec<Html> = playlists
        .iter()
        .filter_map(|id| record(state, "playlists", id).map(|p| (id, p)))
        .map(|(id, p)| {
            tile(
                &format!("result-playlist-{id}"),
                id,
                &web::text(p, "title"),
                &format!("Playlist • {}", owner(p)),
                href("/playlist", &[("list", id)]),
                false,
            )
        })
        .collect();
    out.extend(shelf("results-playlists", "Playlists", "", tiles));
    out
}
fn player_bar(state: &Value, ctx: &ServiceContext, back: &str) -> Html {
    let bar = el("footer").id("bar").class("bar");
    let Some(now) = kit::loaded(state, &ctx.actor, ctx.tick) else {
        return bar.class("idle").child(span("muted").id("bar-idle").text("Pick a song to start listening"));
    };
    let p = &now.player;
    let liked = has(state, "likes", &ctx.actor, &now.id);
    let album = now.album.as_ref();
    // YouTube Music keeps previous, play and next on the left; shuffle and repeat live on
    // the right, so split the five controls the way it does.
    let mut left = kit::transport(p, back);
    let repeat = left.pop().expect("five controls");
    let shuffle = left.remove(0);
    left.push(span("time").id("bar-time").text(format!("{} / {}", kit::clock_ms(p.position_ms), kit::clock_ms(now.length_ms))));
    bar.child(div("progress").id("bar-progress").child(kit::scrubber(p, now.length_ms, back))).child(
        div("row")
            .id("bar-row")
            .child(div("left").id("bar-left").children(left))
            .child(
                div("now")
                    .id("bar-now")
                    .child(cover("bar-art", &album.map(|a| a.id.clone()).unwrap_or_default(), "", 40, 2))
                    .child(
                        div("names")
                            .id("bar-text")
                            .child(
                                here_aware(
                                    el("a").id("bar-title").class("title").attr("href", watch_url(&now.id, &p.context)),
                                    &watch_url(&now.id, &p.context),
                                    back,
                                )
                                    .child(span("").id("bar-title-text").text(now.title.as_str())),
                            )
                            .child(
                                here_aware(
                                    el("a").id("bar-meta").class("artist").attr("href", format!("/channel/{}", now.artist_id)),
                                    &format!("/channel/{}", now.artist_id),
                                    back,
                                )
                                    .child(span("").id("bar-meta-text").text(match album {
                                        Some(a) => format!("{} • {} • {}", now.artist, a.title, a.year),
                                        None => now.artist.clone(),
                                    })),
                            ),
                    )
                    .child(kit::like("bar-like", &now.id, liked, back)),
            )
            .child(div("right").id("bar-right").child(kit::volume(p, back)).child(repeat).child(shuffle)),
    )
}
