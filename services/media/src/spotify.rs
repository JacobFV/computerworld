//! The `audio` sites: spotify.com and soundcloud.com. One markup, laid out like Spotify's
//! web player: a top bar with Home and search, a "Your Library" panel beside the main
//! panel, and the player bar pinned to the bottom edge with shuffle, previous, play, next,
//! repeat and a scrubber. `spotify.css` and `soundcloud.css` make it one or the other.
use super::catalog;
use super::kit::{self, Row};
use super::view::{self, act, cover, div, el, field_form, href, icon, press, span, Html};
use super::*;

pub fn route(state: &mut Value, ctx: &ServiceContext, request: &HttpRequest, parts: &[&str]) -> Result<HttpResponse> {
    let back = kit::here(request);
    let s: &Value = state;
    let (title, page, main) = match parts {
        [""] => (web::text(s, "brand"), "home", home(s, ctx)),
        ["search"] => {
            let q = web::query(request, "q").unwrap_or_default();
            (
                if q.is_empty() { "Search".to_owned() } else { format!("{q} - search") },
                "search",
                search(s, &ctx.actor, &q, &back),
            )
        }
        ["album", id] => match catalog::album(s, id) {
            Some(album) => (album.title.clone(), "album", album_page(s, ctx, &album, &back)),
            None => return web::error(404, "album not found"),
        },
        ["artist", id] | ["channel", id] => match record(s, "channels", id) {
            Some(_) => (catalog::artist_name(s, id), "artist", artist_page(s, ctx, id, &back)),
            None => return web::error(404, "artist not found"),
        },
        ["track", id] => match record(s, "items", id) {
            Some(item) => (web::text(item, "title"), "track", track_page(s, ctx, id, &back)),
            None => return web::error(404, "track not found"),
        },
        ["playlist", id] => match record(s, "playlists", id) {
            Some(p) => (web::text(p, "title"), "playlist", playlist_page(s, ctx, id, &back)),
            None => return web::error(404, "playlist not found"),
        },
        ["playlist"] => match web::query(request, "list") {
            Some(id) if record(s, "playlists", &id).is_some() => (id.clone(), "playlist", playlist_page(s, ctx, &id, &back)),
            _ => ("Your Library".into(), "library", library(s, &ctx.actor, "all")),
        },
        ["collection", "tracks"] => ("Liked Songs".into(), "playlist", liked_page(s, ctx, &back)),
        ["collection", filter @ ("playlists" | "albums" | "artists")] => {
            ("Your Library".into(), "library", library(s, &ctx.actor, filter))
        }
        ["collection"] | ["playlists"] => ("Your Library".into(), "library", library(s, &ctx.actor, "all")),
        ["queue"] => ("Queue".into(), "queue", queue(s, ctx, &back)),
        ["lyrics"] => ("Lyrics".into(), "lyrics", lyrics_page(s, ctx, &back)),
        _ => return web::error(404, "route not found"),
    };
    let doc = view::document(
        s,
        "audio",
        &title,
        &format!("page-{page}"),
        vec![
            top_bar(s, &ctx.actor),
            div("frame").id("frame").child(sidebar(s, &ctx.actor)).child(el("main").id("main").class("panel").children(main)),
            player_bar(s, ctx, &back),
        ],
    );
    Ok(kit::live(doc.response(), s, ctx, &back))
}
fn top_bar(state: &Value, actor: &str) -> Html {
    el("header")
        .id("topbar")
        .class("topbar")
        .child(
            el("a")
                .id("topbar-logo")
                .class("logo")
                .attr("href", "/")
                .child(span("mark").id("topbar-logo-mark").attr("aria-hidden", "true").each(0..3, |_| el("i")))
                .child(span("name").id("topbar-logo-name").text(web::text(state, "brand"))),
        )
        .child(
            div("middle")
                .child(el("a").id("topbar-home").class("round").attr("href", "/").attr("aria-label", "Home").child(icon("home")))
                .child(
                    el("a")
                        .id("topbar-search")
                        .class("searchpill")
                        .attr("href", "/search")
                        .child(icon("search"))
                        .child(span("hint").id("topbar-search-hint").text("What do you want to play?")),
                ),
        )
        .child(span("avatar").id("topbar-avatar").attr("aria-label", actor).text(view::initial(actor)))
}
/// The purple heart tile that stands for Liked Songs wherever a cover would.
fn liked_art(id: &str, class: &str) -> Html {
    span(&format!("likedart {class}")).id(id).attr("role", "img").attr("aria-label", "Liked Songs").child(icon("like-fill"))
}
/// One entry of the library panel: art, title and the "Playlist • owner" line.
fn library_entry(id: &str, art: Html, title: &str, meta: &str, to: String) -> Html {
    el("a")
        .id(format!("side-{id}"))
        .class("entry")
        .attr("href", to)
        .child(
            span("row")
                .id(format!("side-{id}-row"))
                .child(art)
                .child(
                    span("names")
                        .id(format!("side-{id}-text"))
                        .child(span("title").id(format!("side-{id}-title")).text(title))
                        .child(span("meta").id(format!("side-{id}-meta")).text(meta)),
                ),
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
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
fn sidebar(state: &Value, actor: &str) -> Html {
    let liked = catalog::liked(state, actor);
    let mut side = el("aside")
        .id("side")
        .class("panel side")
        .child(
            div("head")
                .id("side-head")
                .child(el("h2").id("side-heading").child(icon("library")).text("Your Library"))
                .child(
                    el("a")
                        .id("side-create")
                        .class("pill")
                        .attr("href", "/collection")
                        .child(span("label").id("side-create-text").child(icon("plus")).child(span("").id("side-create-text-label").text("Create"))),
                ),
        )
        .child(div("chips").id("side-chips").each(["playlists", "artists", "albums"], |f| {
            chip(&format!("side-chip-{f}"), &titled_name(f), false, format!("/collection/{f}"))
        }))
        .child(library_entry(
            "liked",
            liked_art("side-liked-art", "small"),
            "Liked Songs",
            &format!("Playlist • {}", kit::count(liked.len(), "song", "songs")),
            "/collection/tracks".into(),
        ));
    for id in keys(state, "playlists") {
        let p = record(state, "playlists", &id).cloned().unwrap_or(Value::Null);
        side = side.child(library_entry(
            &format!("playlist-{id}"),
            cover(&format!("side-playlist-{id}-art"), &id, "", 48, 4),
            &web::text(&p, "title"),
            &format!("Playlist • {}", owner_name(state, &p)),
            format!("/playlist/{id}"),
        ));
    }
    for album in saved_albums(state, actor) {
        side = side.child(library_entry(
            &format!("album-{}", album.id),
            cover(&format!("side-album-{}-art", album.id), &album.id, "", 48, 4),
            &album.title,
            &format!("Album • {}", album.artist_name),
            format!("/album/{}", album.id),
        ));
    }
    for artist in strings_at(state, "subscriptions", actor) {
        if record(state, "channels", &artist).is_none() {
            continue;
        }
        side = side.child(library_entry(
            &format!("artist-{artist}"),
            cover(&format!("side-artist-{artist}-art"), &artist, "", 48, 24),
            &catalog::artist_name(state, &artist),
            "Artist",
            format!("/artist/{artist}"),
        ));
    }
    side
}
fn chip(id: &str, label: &str, on: bool, to: String) -> Html {
    el("a")
        .id(id)
        .class(if on { "chip on" } else { "chip" })
        .attr("href", to)
        .child(span("").id(format!("{id}-text")).text(label))
}
/// Albums with every track in the listener's library.
fn saved_albums(state: &Value, actor: &str) -> Vec<catalog::Album> {
    let library = strings_at(state, "library", actor);
    catalog::albums(state)
        .into_iter()
        .filter(|a| a.tracks.iter().all(|t| library.contains(t)))
        .collect()
}
fn heading(id: &str, text: &str) -> Html {
    el("h2").id(id).class("heading").text(text)
}
/// A card in a shelf: square art (round for an artist), a title and one muted line.
fn tile(id: &str, of: &str, title: &str, meta: &str, to: String, round: bool) -> Html {
    let art = if of == "liked" {
        liked_art(&format!("{id}-art"), "large")
    } else {
        cover(&format!("{id}-art"), of, if round { "" } else { title }, 132, if round { 66 } else { 6 })
    };
    el("a")
        .id(id)
        .class(if round { "tile round" } else { "tile" })
        .attr("href", to)
        .child(span("art").child(art).child(span("playdot").attr("aria-hidden", "true").child(icon("play"))))
        .child(span("title").id(format!("{id}-title")).text(title))
        .child(span("meta").id(format!("{id}-meta")).text(meta))
}
/// A titled shelf of tiles on one line that scrolls sideways, as Spotify's do.
fn shelf(id: &str, title: &str, tiles: Vec<Html>) -> Vec<Html> {
    if tiles.is_empty() {
        return vec![];
    }
    vec![heading(&format!("{id}-heading"), title), div("shelf").id(id).children(tiles)]
}
fn album_tile(prefix: &str, album: &catalog::Album) -> Html {
    tile(
        &format!("{prefix}-{}", album.id),
        &album.id,
        &album.title,
        &format!("{} • {}", album.year, album.artist_name),
        format!("/album/{}", album.id),
        false,
    )
}
fn home(state: &Value, ctx: &ServiceContext) -> Vec<Html> {
    let actor = &ctx.actor;
    let greeting = match kit::hour(ctx.tick) {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };
    // Shortcuts: Liked Songs, then playlists, then the newest albums, eight in all.
    let mut shortcuts = vec![shortcut("short-liked", "liked", "Liked Songs", "/collection/tracks".into())];
    for id in keys(state, "playlists") {
        let title = record(state, "playlists", &id).map(|p| web::text(p, "title")).unwrap_or_default();
        shortcuts.push(shortcut(&format!("short-playlist-{id}"), &id, &title, format!("/playlist/{id}")));
    }
    for album in catalog::albums(state) {
        shortcuts.push(shortcut(&format!("short-album-{}", album.id), &album.id, &album.title, format!("/album/{}", album.id)));
    }
    shortcuts.truncate(8);
    let mut out = vec![
        div("chips").id("home-filters").each([("all", "All", "/"), ("music", "Music", "/search"), ("library", "Library", "/collection")], |(k, label, to)| {
            chip(&format!("home-filter-{k}"), label, k == "all", to.to_owned())
        }),
        heading("home-greeting", greeting),
        div("shortcuts").id("home-shortcuts").children(shortcuts),
    ];
    let history = strings_at(state, "history", actor);
    let mut seen = vec![];
    let recent: Vec<Html> = history
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
    let albums: Vec<Html> = catalog::albums(state).iter().take(10).map(|a| album_tile("popular", a)).collect();
    out.extend(shelf("home-albums", "Popular albums and singles", albums));
    let mut artists = keys(state, "channels");
    artists.sort_by_key(|id| std::cmp::Reverse(record(state, "channels", id).map_or(0, |c| num(c, "subscribers"))));
    let artists: Vec<Html> = artists
        .iter()
        .map(|id| tile(&format!("artist-{id}"), id, &catalog::artist_name(state, id), "Artist", format!("/artist/{id}"), true))
        .collect();
    out.extend(shelf("home-artists", "Popular artists", artists));
    let lists: Vec<Html> = keys(state, "playlists")
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
fn shortcut(id: &str, of: &str, title: &str, to: String) -> Html {
    el("a").id(id).class("shortcut").attr("href", to).child(
        span("row")
            .id(format!("{id}-row"))
            .child(if of == "liked" { liked_art(&format!("{id}-art"), "medium") } else { cover(&format!("{id}-art"), of, "", 56, 4) })
            .child(span("title").id(format!("{id}-title")).text(title)),
    )
}
/// The big header every collection page opens with: art, kind, title and a meta line,
/// over a wash of the artwork's own colour.
fn hero(id: &str, of: &str, kind: &str, title: &str, meta: String, round: bool) -> Html {
    let art = if of == "liked" {
        liked_art(&format!("{id}-art"), "huge")
    } else {
        cover(&format!("{id}-art"), of, title, 180, if round { 64 } else { 6 })
    };
    div("hero")
        .id(format!("{id}-hero"))
        .style(&format!("--wash: {}", kit::shade(&kit::wash(of), 60)))
        .child(art)
        .child(
            div("identity")
                .id(format!("{id}-identity"))
                .child(span("kind").id(format!("{id}-kind")).text(kind))
                .child(el("h1").id(format!("{id}-title")).text(title))
                .child(span("meta").id(format!("{id}-meta")).text(meta)),
        )
}
fn big_play(id: &str, context: &str, back: &str, playing_this: bool) -> Html {
    let (glyph, label) = if playing_this { ("pause", "Pause") } else { ("play", "Play") };
    let control = press("ctl bigplay", label).child(icon(glyph));
    if playing_this {
        kit::command(id, &[("action", "toggle"), ("return", back)], control)
    } else {
        kit::command(id, &[("action", "play"), ("context", context), ("return", back)], control)
    }
}
fn shuffle_play(id: &str, context: &str, back: &str) -> Html {
    kit::control(
        id,
        "shuffle",
        "Shuffle play",
        "large",
        &[("action", "play"), ("context", context), ("shuffle", "true"), ("return", back)],
    )
}
fn actions(children: Vec<Html>) -> Html {
    div("actions").id("actions").children(children)
}
fn column_head(extra: &str, art: bool) -> Html {
    div(if art { "trow head with-art" } else { "trow head" })
        .id("tracks-head")
        .child(
            span(if art { "cells with-art" } else { "cells" })
                .child(span("num").id("tracks-head-number").text("#"))
                .when(art, |c| c.child(span("")))
                .child(span("names").id("tracks-head-title").text("Title"))
                .child(span("extra").id("tracks-head-extra").text(extra)),
        )
        .child(span("dur").id("tracks-head-time").attr("aria-label", "Time").child(icon("clock")))
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
) -> Html {
    let (playing, _, _) = current(state, ctx);
    let row = Row { prefix: "track", context, playing: playing.as_deref(), back, art };
    div("tracks").id("tracks").each(ids.iter().enumerate(), |(i, id)| kit::track_row(state, &ctx.actor, &row, id, i + 1, extra(id)))
}
fn album_page(state: &Value, ctx: &ServiceContext, album: &catalog::Album, back: &str) -> Vec<Html> {
    let context = format!("album:{}", album.id);
    let (_, playing_context, playing) = current(state, ctx);
    let saved = saved_albums(state, &ctx.actor).iter().any(|a| a.id == album.id);
    vec![
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
            big_play("album-play", &context, back, playing && playing_context == context),
            shuffle_play("album-shuffle", &context, back),
            act(
                "album-save",
                &format!("/library/albums/{}", album.id),
                &[("return", back)],
                press(
                    if saved { "ctl large on" } else { "ctl large" },
                    if saved { "Remove from Your Library" } else { "Save to Your Library" },
                )
                .child(icon(if saved { "check" } else { "plus" })),
            ),
            el("a")
                .id("album-artist")
                .class("textlink")
                .attr("href", format!("/artist/{}", album.artist))
                .child(span("").id("album-artist-name").text(album.artist_name.as_str())),
        ]),
        column_head("", false),
        tracks(state, ctx, &album.tracks, &context, back, |_| None, false),
        el("p").id("album-copyright").class("fine").text(format!("℗ {} {}", album.year, album.artist_name)),
    ]
}
fn playlist_page(state: &Value, ctx: &ServiceContext, id: &str, back: &str) -> Vec<Html> {
    let p = record(state, "playlists", id).cloned().unwrap_or(Value::Null);
    let items: Vec<String> = catalog::context_tracks(state, &ctx.actor, &format!("playlist:{id}"));
    let context = format!("playlist:{id}");
    let (_, playing_context, playing) = current(state, ctx);
    let title = web::text(&p, "title");
    let mut out = vec![hero(
        "playlist",
        id,
        "Playlist",
        &title,
        format!("{} • {}, {}", owner_name(state, &p), kit::count(items.len(), "song", "songs"), kit::running(state, &items)),
        false,
    )];
    if items.is_empty() {
        out.push(el("p").id("playlist-empty").class("lead").text("Let's find something for your playlist"));
    } else {
        out.push(actions(vec![
            big_play("playlist-play", &context, back, playing && playing_context == context),
            shuffle_play("playlist-shuffle", &context, back),
        ]));
        out.push(column_head("Album", true));
        out.push(tracks(
            state,
            ctx,
            &items,
            &context,
            back,
            |t| record(state, "items", t).and_then(|i| catalog::album(state, &catalog::album_id(i))).map(|a| a.title),
            true,
        ));
    }
    out
}
fn liked_page(state: &Value, ctx: &ServiceContext, back: &str) -> Vec<Html> {
    let items = catalog::liked(state, &ctx.actor);
    let mut out = vec![hero(
        "liked",
        "liked",
        "Playlist",
        "Liked Songs",
        format!("{} • {}", titled_name(&ctx.actor), kit::count(items.len(), "song", "songs")),
        false,
    )];
    if items.is_empty() {
        out.push(
            el("p")
                .id("liked-empty")
                .class("muted")
                .text("Songs you like will appear here. Save songs by tapping the heart icon."),
        );
    } else {
        out.push(actions(vec![big_play("liked-play", "liked", back, false)]));
        out.push(column_head("Album", true));
        out.push(tracks(
            state,
            ctx,
            &items,
            "liked",
            back,
            |t| record(state, "items", t).and_then(|i| catalog::album(state, &catalog::album_id(i))).map(|a| a.title),
            true,
        ));
    }
    out
}
fn artist_page(state: &Value, ctx: &ServiceContext, id: &str, back: &str) -> Vec<Html> {
    let artist = record(state, "channels", id).cloned().unwrap_or(Value::Null);
    let name = web::text(&artist, "name");
    let following = has(state, "subscriptions", &ctx.actor, id);
    let context = format!("artist:{id}");
    let (_, playing_context, playing) = current(state, ctx);
    let top: Vec<String> = catalog::top_tracks(state, id).into_iter().take(5).collect();
    let mut out = vec![
        div("banner")
            .id("artist-banner")
            .style(&format!("--wash: {}", kit::shade(&kit::wash(id), 70)))
            .child(view::still("artist-banner-art", id, "", 512))
            .child(
                div("identity")
                    .child(
                        span("verified")
                            .id("artist-verified")
                            .child(icon("check"))
                            .child(span("").id("artist-verified-label").text("Verified Artist")),
                    )
                    .child(el("h1").id("artist-name").text(name.as_str()))
                    .child(
                        span("listeners")
                            .id("artist-listeners")
                            .text(format!("{} monthly listeners", grouped(num(&artist, "subscribers")))),
                    ),
            ),
        actions(vec![
            big_play("artist-play", &context, back, playing && playing_context == context),
            act(
                "artist-follow",
                &format!("/channels/{id}/subscribe"),
                &[("return", back)],
                press(if following { "outline on" } else { "outline" }, "")
                    .child(span("").id("artist-follow-text").text(if following { "Following" } else { "Follow" })),
            ),
            kit::command(
                "artist-radio",
                &[("action", "play"), ("context", &format!("station:{id}")), ("return", back)],
                press("textlink", "").child(span("").id("artist-radio-text").text("Go to artist radio")),
            ),
        ]),
        heading("artist-popular", "Popular"),
    ];
    let plays = |t: &str| record(state, "items", t).map(|i| grouped(num(i, "plays")));
    out.push(tracks(state, ctx, &top, &context, back, plays, true));
    let discography: Vec<Html> = catalog::albums(state).iter().filter(|a| a.artist == id).map(|a| album_tile("disco", a)).collect();
    out.extend(shelf("artist-discography", "Discography", discography));
    let about = web::text(&artist, "about");
    if !about.is_empty() {
        out.push(heading("artist-about-heading", "About"));
        out.push(div("about").id("artist-about").child(el("p").id("artist-about-text").text(about)));
    }
    out
}
fn track_page(state: &Value, ctx: &ServiceContext, id: &str, back: &str) -> Vec<Html> {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let title = web::text(&item, "title");
    let artist = web::text(&item, "channel");
    let album = catalog::album(state, &catalog::album_id(&item));
    let liked = has(state, "likes", &ctx.actor, id);
    let saved = has(state, "library", &ctx.actor, id);
    let context = album.as_ref().map(|a| format!("album:{}", a.id)).unwrap_or_else(|| "track".into());
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
                kit::command(
                    "track-play",
                    &[("action", "play"), ("item", id), ("context", &context), ("return", back)],
                    press("ctl bigplay", "Play").child(icon("play")),
                )
            },
            kit::like("track-like", id, liked, back),
            act(
                "track-save",
                &format!("/library/items/{id}"),
                &[("return", back)],
                press("outline", "").child(
                    span("label")
                        .id("track-save-text")
                        .child(icon(if saved { "check" } else { "plus" }))
                        .child(span("").id("track-save-text-label").text(if saved { "In your library" } else { "Save to library" })),
                ),
            ),
            act(
                "track-queue",
                &format!("/items/{id}/queue"),
                &[("return", back)],
                press("outline", "").child(span("").id("track-queue-text").text("Add to queue")),
            ),
            el("a")
                .id("track-artist")
                .class("textlink")
                .attr("href", format!("/artist/{artist}"))
                .text(catalog::artist_name(state, &artist)),
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
        out.push(div("chips").id("track-add").each(writable.iter(), |(p, title)| {
            let inside = record(state, "playlists", p).is_some_and(|l| web::strings(l, "items").iter().any(|i| i == id));
            let label = |glyph: &str| {
                span("label")
                    .id(format!("track-add-{p}-text"))
                    .child(icon(glyph))
                    .child(span("").id(format!("track-add-{p}-text-label")).text(title.as_str()))
            };
            if inside {
                span("chip done").id(format!("track-add-{p}")).child(label("check"))
            } else {
                act(
                    &format!("track-add-{p}"),
                    &format!("/playlists/{p}/items"),
                    &[("item", id), ("return", back)],
                    press("chip", "").child(label("plus")),
                )
            }
        }));
    }
    if let Some(album) = album {
        out.extend(shelf("track-album", &format!("From {}", album.title), vec![album_tile("from", &album)]));
    }
    out
}
fn search(state: &Value, actor: &str, q: &str, back: &str) -> Vec<Html> {
    let mut out = vec![field_form("search", "/search", "get", "q", "What do you want to play?", q, "Search").class("searchform")];
    if q.trim().is_empty() {
        out.push(heading("browse-heading", "Browse all"));
        let mut tags: Vec<String> = keys(state, "items")
            .iter()
            .filter_map(|id| record(state, "items", id))
            .flat_map(|i| web::strings(i, "tags"))
            .collect();
        tags.sort();
        tags.dedup();
        out.push(div("genres").id("browse-grid").each(tags.iter(), |t| {
            el("a")
                .id(format!("genre-{}", slug(t)))
                .class("genre")
                .attr("href", href("/search", &[("q", t)]))
                .style(&format!("background-color: {}", tint(&format!("genre{t}"))))
                .child(span("").id(format!("genre-{}-name", slug(t))).text(titled_name(t)))
                .child(span("corner").attr("aria-hidden", "true"))
        }));
        return out;
    }
    let hits = catalog::search(state, q);
    let list = |k: &str| web::strings(&hits, k);
    let (songs, albums, artists, playlists) = (list("tracks"), list("albums"), list("artists"), list("playlists"));
    if songs.is_empty() && albums.is_empty() && artists.is_empty() && playlists.is_empty() {
        out.push(heading("results-heading", &format!("No results found for \"{q}\"")));
        out.push(
            el("p")
                .id("results-hint")
                .class("muted")
                .text("Please make sure your words are spelled correctly, or use fewer or different keywords."),
        );
        return out;
    }
    if !songs.is_empty() {
        out.push(heading("results-songs", "Songs"));
        let row = Row { prefix: "result", context: "track", playing: None, back, art: true };
        out.push(div("tracks").id("results-tracks").each(songs.iter().take(8).enumerate(), |(i, id)| {
            kit::track_row(state, actor, &row, id, i + 1, None)
        }));
    }
    let artist_tiles: Vec<Html> = artists
        .iter()
        .map(|id| tile(&format!("result-artist-{id}"), id, &catalog::artist_name(state, id), "Artist", format!("/artist/{id}"), true))
        .collect();
    out.extend(shelf("results-artists", "Artists", artist_tiles));
    let album_tiles: Vec<Html> = albums.iter().filter_map(|id| catalog::album(state, id)).map(|a| album_tile("result-album", &a)).collect();
    out.extend(shelf("results-albums", "Albums", album_tiles));
    let list_tiles: Vec<Html> = playlists
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
fn library(state: &Value, actor: &str, filter: &str) -> Vec<Html> {
    let mut out = vec![
        heading("library-heading", "Your Library"),
        div("chips").id("library-chips").each(
            [
                ("all", "All", "/collection"),
                ("playlists", "Playlists", "/collection/playlists"),
                ("albums", "Albums", "/collection/albums"),
                ("artists", "Artists", "/collection/artists"),
            ],
            |(k, label, to)| chip(&format!("library-chip-{k}"), label, k == filter, to.to_owned()),
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
            let p = record(state, "playlists", &id).cloned().unwrap_or(Value::Null);
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
        out.push(el("p").id("library-empty").class("muted").text("Nothing here yet."));
    } else {
        out.push(div("tilegrid").id("library-grid").children(tiles));
    }
    out.push(heading("playlist-heading", "Create a playlist"));
    out.push(field_form("playlist", "/playlists", "post", "title", "New playlist", "", "Create").class("createform"));
    out
}
fn queue(state: &Value, ctx: &ServiceContext, back: &str) -> Vec<Html> {
    let mut out = vec![heading("queue-heading", "Queue")];
    let Some(p) = catalog::player(state, &ctx.actor, ctx.tick) else {
        out.push(el("p").id("queue-empty").class("lead").text("Add to your queue"));
        out.push(el("p").id("queue-hint").class("muted").text("Tap \"Add to queue\" on a song to find it here."));
        return out;
    };
    out.push(el("h3").id("queue-now").class("subheading").text("Now playing"));
    let row = |i: usize, id: &str| {
        let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
        let current = i == p.index;
        kit::command(
            &format!("queue-{i}"),
            &[("action", "jump"), ("index", &i.to_string()), ("return", back)],
            press(if current { "qrow current" } else { "qrow" }, "").child(
                span("row")
                    .id(format!("queue-{i}-row"))
                    .child(cover(&format!("queue-{i}-art"), &catalog::album_id(&item), "", 48, 4))
                    .child(
                        span("names")
                            .id(format!("queue-{i}-text"))
                            .child(span("title").id(format!("queue-{i}-title")).text(web::text(&item, "title")))
                            .child(
                                span("artist")
                                    .id(format!("queue-{i}-artist"))
                                    .text(catalog::artist_name(state, &web::text(&item, "channel"))),
                            ),
                    )
                    .child(span("dur").id(format!("queue-{i}-time")).text(clock(num(&item, "duration_s")))),
            ),
        )
    };
    if let Some(id) = p.current() {
        out.push(row(p.index, id));
    }
    let title = catalog::context_title(state, &p.context);
    out.push(
        el("h3")
            .id("queue-next")
            .class("subheading")
            .text(if title.is_empty() { "Next up".to_owned() } else { format!("Next from: {title}") }),
    );
    for (i, id) in p.queue.iter().enumerate().skip(p.index + 1) {
        out.push(row(i, id));
    }
    out
}
/// The lyrics view: the song's lines over its artwork's colour, the one being sung lit,
/// following the player as the world clock moves it.
fn lyrics_page(state: &Value, ctx: &ServiceContext, back: &str) -> Vec<Html> {
    let Some(now) = kit::loaded(state, &ctx.actor, ctx.tick) else {
        return vec![
            heading("lyrics-heading", "Lyrics"),
            el("p").id("lyrics-idle").class("muted").text("Play a song to see its lyrics."),
        ];
    };
    let album = now.album.as_ref().map(|a| a.id.clone()).unwrap_or_default();
    let item = record(state, "items", &now.id).cloned().unwrap_or(Value::Null);
    let lines = catalog::lyrics(&item);
    let mut sheet = div("lyrics")
        .id("lyrics")
        .style(&format!("--wash: {}", kit::shade(&kit::wash(&album), 85)))
        .child(el("p").id("lyrics-title").class("over").text(format!("{} · {}", now.title, now.artist)));
    if lines.is_empty() {
        sheet = sheet.child(el("p").id("lyrics-none").class("lyric now").text(
            if web::strings(&item, "tags").iter().any(|t| t == "instrumental") {
                "This song is instrumental."
            } else {
                "Looks like we don't have lyrics for this song."
            },
        ));
    } else {
        sheet = sheet.children(kit::lyric_lines("lyric", &lines, catalog::sung(&lines, now.player.position_ms), back));
    }
    vec![sheet]
}
fn player_bar(state: &Value, ctx: &ServiceContext, back: &str) -> Html {
    let bar = el("footer").id("bar").class("bar");
    let Some(now) = kit::loaded(state, &ctx.actor, ctx.tick) else {
        return bar.class("idle").child(span("muted").id("bar-idle").text("Nothing playing"));
    };
    let liked = has(state, "likes", &ctx.actor, &now.id);
    let album = now.album.as_ref().map(|a| a.id.clone()).unwrap_or_default();
    let p = &now.player;
    bar.child(
        div("now")
            .id("bar-now")
            .child(cover("bar-art", &album, "", 56, 4))
            .child(
                div("names")
                    .id("bar-text")
                    .child(
                        el("a")
                            .id("bar-title")
                            .class("title")
                            .attr("href", format!("/track/{}", now.id))
                            .child(span("").id("bar-title-text").text(now.title.as_str())),
                    )
                    .child(
                        el("a")
                            .id("bar-meta")
                            .class("artist")
                            .attr("href", format!("/artist/{}", now.artist_id))
                            .child(span("").id("bar-meta-text").text(now.artist.as_str())),
                    ),
            )
            .child(kit::like("bar-like", &now.id, liked, back)),
    )
    .child(
        div("center")
            .id("bar-center")
            .child(div("controls").id("bar-controls").children(kit::transport(p, back)))
            .child(
                div("scrubline")
                    .id("bar-scrub")
                    .child(span("time").id("bar-elapsed").text(kit::clock_ms(p.position_ms)))
                    .child(kit::scrubber(p, now.length_ms, back))
                    .child(span("time").id("bar-length").text(kit::clock_ms(now.length_ms))),
            ),
    )
    .child(
        div("right")
            .id("bar-right")
            .child(
                el("a")
                    .id("bar-lyrics")
                    .class(if back.starts_with("/lyrics") { "ctl on" } else { "ctl" })
                    .attr("href", "/lyrics")
                    .attr("aria-label", "Lyrics")
                    .attr("title", "Lyrics")
                    .child(icon("mic")),
            )
            .child(
                el("a")
                    .id("bar-queue")
                    .class(if back.starts_with("/queue") { "ctl on" } else { "ctl" })
                    .attr("href", "/queue")
                    .attr("aria-label", "Queue")
                    .attr("title", "Queue")
                    .child(icon("queue")),
            )
            .child(kit::volume(p, back)),
    )
}
