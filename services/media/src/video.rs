//! The `video` sites: youtube.com, netflix.com, twitch.tv, vimeo.com and tiktok.com. One
//! markup serves all five (a masthead with the search box, a guide of channels, tiles
//! with a fixed-aspect still, the watch page with its player box, channel row,
//! description, comments and the up-next list) and the skin's stylesheet arranges it as
//! that product: YouTube's grid and guide, Netflix's billboard over rows of posters,
//! Twitch's followed channels and chat column, Vimeo's staff picks, TikTok's tall tiles.
use super::view::{self, act, cover, div, el, field_form, href, icon, link, press, short, span, still, Html};
use super::*;

/// What each product calls the things the service has one name for.
struct Words {
    subscribe: (&'static str, &'static str),
    subscribers: &'static str,
    subscriptions: &'static str,
    up_next: &'static str,
    comments: &'static str,
    comment_hint: &'static str,
    save: &'static str,
    library: &'static str,
    views: &'static str,
    home: &'static str,
    later: &'static str,
}
fn words(skin: &str) -> Words {
    match skin {
        "netflix" => Words {
            subscribe: ("Remind me", "Reminding"),
            subscribers: "members",
            subscriptions: "Following",
            up_next: "More like this",
            comments: "Reviews",
            comment_hint: "Write a review",
            save: "My List",
            library: "Categories",
            views: "views",
            home: "Home",
            later: "My List",
        },
        "twitch" => Words {
            subscribe: ("Follow", "Following"),
            subscribers: "followers",
            subscriptions: "Followed Channels",
            up_next: "Recommended",
            comments: "Stream Chat",
            comment_hint: "Send a message",
            save: "Watch later",
            library: "Collections",
            views: "viewers",
            home: "Browse",
            later: "Watch later",
        },
        "vimeo" => Words {
            subscribe: ("Follow", "Following"),
            subscribers: "followers",
            subscriptions: "Following",
            up_next: "More from Vimeo",
            comments: "Comments",
            comment_hint: "Add a comment",
            save: "Watch later",
            library: "Showcases",
            views: "views",
            home: "Home",
            later: "Watch later",
        },
        "tiktok" => Words {
            subscribe: ("Follow", "Following"),
            subscribers: "Followers",
            subscriptions: "Following accounts",
            up_next: "You may like",
            comments: "Comments",
            comment_hint: "Add comment...",
            save: "Favorites",
            library: "Collections",
            views: "views",
            home: "For You",
            later: "Favorites",
        },
        _ => Words {
            subscribe: ("Subscribe", "Subscribed"),
            subscribers: "subscribers",
            subscriptions: "Subscriptions",
            up_next: "Up next",
            comments: "Comments",
            comment_hint: "Add a comment",
            save: "Save",
            library: "Library",
            views: "views",
            home: "Home",
            later: "Watch later",
        },
    }
}
fn channel_name(state: &Value, channel: &str) -> String {
    record(state, "channels", channel).map(|c| web::text(c, "name")).unwrap_or_else(|| channel.to_owned())
}
fn is_live(item: &Value) -> bool {
    web::strings(item, "badges").iter().any(|b| b.eq_ignore_ascii_case("live"))
}
/// The whole page: masthead, guide and the page's own content.
fn page(state: &Value, actor: &str, title: &str, class: &str, query: &str, main: Vec<Html>) -> Result<HttpResponse> {
    let doc = view::document(
        state,
        "video",
        title,
        &format!("page-{class}"),
        vec![
            chrome(state, actor, query),
            div("shell").child(guide(state, actor, class)).child(el("main").id("page").class(&format!("pg-{class}")).children(main)),
        ],
    );
    web::html::page(&doc)
}
/// The masthead: the brand, the search box and the way to the library.
fn chrome(state: &Value, actor: &str, query: &str) -> Html {
    let brand = match web::text(state, "brand").as_str() {
        "" => BRAND.to_owned(),
        s => s.to_owned(),
    };
    let skin = view::skin(state, "video");
    el("header")
        .id("chrome")
        .class("masthead")
        .child(
            div("start").child(span("burger").attr("aria-hidden", "true").each(0..3, |_| el("i"))).child(
                el("a")
                    .id("chrome-home")
                    .class("brand")
                    .attr("href", "/")
                    .child(span("mark").attr("aria-hidden", "true").child(el("i")))
                    .child(span("word").id("chrome-brand").text(brand)),
            ),
        )
        .child(
            el("nav")
                .class("sections")
                .child(link("chrome-nav-home", "/", words(skin).home))
                .child(link("chrome-nav-library", "/playlists", words(skin).library))
                .child(link("chrome-nav-later", "/playlist?list=watch-later", words(skin).later)),
        )
        .child(
            div("middle").id("chrome-search").child(
                form_search(query),
            ),
        )
        .child(
            div("end")
                .child(link("chrome-library", "/playlists", "Library"))
                .child(span("avatar").id("chrome-avatar").attr("aria-label", actor).text(view::initial(actor))),
        )
}
/// The search box is a GET form, so a results page has its query in its URL.
fn form_search(query: &str) -> Html {
    view::form("search", "/results", "get")
        .class("searchbox")
        .attr("role", "search")
        .child(
            view::text_input("search-search_query", "search_query", query)
                .attr("aria-label", "Search")
                .attr("placeholder", "Search")
                .attr("autocomplete", "off"),
        )
        .child(el("button").id("search-submit").attr("type", "submit").attr("aria-label", "Search").child(icon("search")))
}
/// The guide: home and library, the channels this viewer follows, then the rest.
fn guide(state: &Value, actor: &str, class: &str) -> Html {
    let skin = view::skin(state, "video");
    let w = words(skin);
    let followed = strings_at(state, "subscriptions", actor);
    let entry = |prefix: &str, id: &str| {
        let channel = record(state, "channels", id).cloned().unwrap_or(Value::Null);
        let live = by_recency(state, |item| web::text(item, "channel") == id && is_live(item));
        el("a")
            .id(format!("{prefix}-{id}"))
            .class("channel")
            .attr("href", format!("/channel/{id}"))
            .child(cover(&format!("{prefix}-{id}-art"), id, "", 48, 24))
            .child(
                span("names")
                    .child(span("name").text(web::text(&channel, "name")))
                    .child(span("sub").text(format!("{} {}", short(num(&channel, "subscribers")), w.subscribers))),
            )
            .when(!live.is_empty(), |a| a.child(span("livedot").attr("aria-label", "Live")))
    };
    let others: Vec<String> = keys(state, "channels").into_iter().filter(|c| !followed.contains(c)).take(8).collect();
    el("nav")
        .id("guide")
        .class("guide")
        .child(el("a").id("guide-home").class(if class == "home" { "item on" } else { "item" }).attr("href", "/").child(icon("home")).child(span("").text(w.home)))
        .child(el("a").id("guide-library").class(if class == "library" { "item on" } else { "item" }).attr("href", "/playlists").child(icon("library")).child(span("").text(w.library)))
        .child(
            el("a")
                .id("guide-later")
                .class("item")
                .attr("href", "/playlist?list=watch-later")
                .child(icon("clock"))
                .child(span("").text(w.later)),
        )
        .when(!followed.is_empty(), |g| {
            g.child(el("h3").id("guide-subs-title").text(w.subscriptions))
                .each(followed.iter().filter(|c| record(state, "channels", c).is_some()), |c| entry("guide-sub", c))
        })
        .when(!others.is_empty(), |g| {
            g.child(el("h3").id("guide-more-title").text(if skin == "twitch" { "Recommended Channels" } else { "Explore" }))
                .each(others.iter(), |c| entry("guide-channel", c))
        })
}
/// Optional seed flags — "LIVE", "4K", "NEW". LIVE is the loud one, as it is on the real thing.
fn flags(item: &Value, prefix: &str) -> Html {
    span("flags").each(web::strings(item, "badges").iter(), |flag| {
        span(if flag.eq_ignore_ascii_case("live") { "flag live" } else { "flag" })
            .id(format!("{prefix}-flag-{}", slug(flag)))
            .text(flag.as_str())
    })
}
/// One video tile: still, duration, flags, channel avatar, title, channel and counts —
/// the unit every grid and row repeats. `prefix` keeps a video shown twice on one page
/// (a section and the grid) unique; `to` is where it plays.
fn video_tile(state: &Value, id: &str, prefix: &str, to: String) -> Html {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let channel = web::text(&item, "channel");
    let tile = format!("{prefix}-{id}");
    let w = words(view::skin(state, "video"));
    el("a")
        .id(tile.as_str())
        .class(if is_live(&item) { "tile live" } else { "tile" })
        .attr("href", to)
        .child(
            span("thumb")
                .child(still(&format!("{tile}-art"), id, "", 256))
                .child(flags(&item, &tile))
                .child(span("time").id(format!("{tile}-duration")).text(clock(num(&item, "duration_s")))),
        )
        .child(
            span("details")
                .id(format!("{tile}-meta"))
                .child(cover(&format!("{tile}-avatar"), &channel, "", 48, 24))
                .child(
                    span("text")
                        .child(span("title").id(format!("{tile}-title")).text(web::text(&item, "title")))
                        .child(span("channel").id(format!("{tile}-channel")).text(channel_name(state, &channel)))
                        .child(
                            span("stats")
                                .child(span("").id(format!("{tile}-views")).text(format!("{} {}", grouped(num(&item, "views")), w.views)))
                                .child(span("dot").text(" · "))
                                .child(span("").id(format!("{tile}-published")).text(web::text(&item, "published"))),
                        ),
                ),
        )
}
fn video_card(state: &Value, id: &str) -> Html {
    video_tile(state, id, "tile", format!("/watch?v={id}"))
}
/// Compact row used by "Up next", search results and playlists: the still to the left.
fn item_row(state: &Value, id: &str, prefix: &str, to: String, long: bool) -> Html {
    let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
    let channel = web::text(&item, "channel");
    let p = format!("{prefix}-{id}");
    el("a").id(p.as_str()).class(if long { "rowcard long" } else { "rowcard" }).attr("href", to).child(
        span("row")
            .id(format!("{p}-row"))
            .child(
                span("thumb")
                    .child(still(&format!("{p}-art"), id, "", 256))
                    .child(flags(&item, &p))
                    .child(span("time").text(clock(num(&item, "duration_s")))),
            )
            .child(
                span("text")
                    .id(format!("{p}-text"))
                    .child(span("title").id(format!("{p}-title")).text(web::text(&item, "title")))
                    .child(span("channel").id(format!("{p}-channel")).text(format!(
                        "{} · {} views",
                        channel_name(state, &channel),
                        grouped(num(&item, "views"))
                    )))
                    .when(long, |t| t.child(span("snip").text(web::text(&item, "description")))),
            ),
    )
}
fn comment_block(state: &Value, item_id: &str, actor: &str, back: &str, w: &Words) -> Html {
    let item = record(state, "items", item_id).cloned().unwrap_or(Value::Null);
    let comments = item.get("comments").and_then(Value::as_array).cloned().unwrap_or_default();
    el("section")
        .id("comments")
        .class("comments")
        .child(el("h2").id("comments-heading").text(format!("{} {}", comments.len(), w.comments)))
        .child(
            div("compose")
                .id("comment-compose")
                .child(cover("comment-avatar", actor, actor, 40, 20))
                .child(field_form("comment", &format!("/items/{item_id}/comments"), "post", "text", w.comment_hint, "", "Comment")),
        )
        .child(div("thread").id("comment-thread").each(comments.iter(), |comment| {
            let cid = web::text(comment, "id");
            let author = web::text(comment, "author");
            div("comment")
                .id(format!("comment-{cid}"))
                .child(cover(&format!("comment-{cid}-avatar"), &author, "", 40, 20))
                .child(
                    div("body")
                        .id(format!("comment-{cid}-body"))
                        .child(span("author").id(format!("comment-{cid}-author")).text(format!("@{author}")))
                        .child(span("said").id(format!("comment-{cid}-text")).text(web::text(comment, "text"))),
                )
                .child(act(
                    &format!("comment-{cid}-like"),
                    &format!("/items/{item_id}/comments/{cid}/like"),
                    &[("return", back)],
                    press("pill small", "").text(format!("♥ {}", num(comment, "likes"))),
                ))
        }))
}
/// The playlists a viewer may see on the home page: shared ones and their own.
fn rows_for(state: &Value, actor: &str) -> Vec<String> {
    keys(state, "playlists")
        .into_iter()
        .filter(|id| {
            record(state, "playlists", id).is_some_and(|p| {
                let owner = web::text(p, "owner");
                (owner.is_empty() || owner == actor) && !web::strings(p, "items").is_empty()
            })
        })
        .collect()
}
pub fn home(state: &Value, actor: &str) -> Result<HttpResponse> {
    let skin = view::skin(state, "video");
    let ids = by_recency(state, |_| true);
    let mut tags: Vec<String> =
        ids.iter().filter_map(|id| record(state, "items", id)).flat_map(|item| web::strings(item, "tags")).collect();
    tags.sort();
    tags.dedup();
    let mut main = vec![];
    // The billboard: what the site leads with. YouTube and TikTok lead with the grid.
    if matches!(skin, "netflix" | "twitch" | "vimeo") {
        let featured = ids
            .iter()
            .find(|id| record(state, "items", id).is_some_and(|i| skin == "twitch" && is_live(i)))
            .or_else(|| {
                ids.iter().max_by_key(|id| record(state, "items", id).map_or(0, |i| num(i, "views")))
            });
        if let Some(id) = featured {
            let item = record(state, "items", id).cloned().unwrap_or(Value::Null);
            let channel = web::text(&item, "channel");
            main.push(
                el("section")
                    .id("hero")
                    .class("billboard")
                    .child(still("hero-art", id, "", 512))
                    .child(span("fade").attr("aria-hidden", "true"))
                    .child(
                        div("copy")
                            .child(flags(&item, "hero"))
                            .child(el("h1").id("hero-title").text(web::text(&item, "title")))
                            .child(el("p").id("hero-text").text(web::text(&item, "description")))
                            .child(
                                div("buttons")
                                    .child(
                                        el("a")
                                            .id("hero-play")
                                            .class("cta")
                                            .attr("href", format!("/watch?v={id}"))
                                            .child(icon("play"))
                                            .child(span("").text(if skin == "twitch" { "Watch now" } else { "Play" })),
                                    )
                                    .child(
                                        el("a")
                                            .id("hero-info")
                                            .class("cta quiet")
                                            .attr("href", format!("/channel/{channel}"))
                                            .child(icon("info"))
                                            .child(span("").text("More Info")),
                                    ),
                            ),
                    ),
            );
        }
    }
    main.push(div("chips").id("chips").child(span("chip on").id("chip-all").text("All")).each(tags.iter(), |tag| {
        el("a")
            .id(format!("chip-{tag}"))
            .class("chip")
            .attr("href", href("/results", &[("search_query", tag)]))
            .child(span("").id(format!("chip-{tag}-text")).text(tag.as_str()))
    }));
    main.extend(sections(state));
    // Sites built of rows (Netflix's categories, Twitch's directories, Vimeo's showcases,
    // TikTok's feeds) show each shared playlist as a row that scrolls sideways.
    if skin != "youtube" {
        for list in rows_for(state, actor) {
            let playlist = record(state, "playlists", &list).cloned().unwrap_or(Value::Null);
            main.push(
                el("section")
                    .class("shelf rows")
                    .child(
                        el("h2").id(format!("row-{list}-title")).child(link(
                            &format!("row-{list}-link"),
                            href("/playlist", &[("list", &list)]),
                            web::text(&playlist, "title"),
                        )),
                    )
                    .child(div("rail").id(format!("row-{list}")).each(
                        web::strings(&playlist, "items").iter().filter(|id| record(state, "items", id).is_some()),
                        |id| video_tile(state, id, &format!("row-{list}"), format!("/watch?v={id}&list={list}")),
                    )),
            );
        }
    }
    main.push(el("h2").id("home-grid-title").class("gridtitle").text(match skin {
        "netflix" => "Everything on Netflix",
        "twitch" => "Live channels and recent broadcasts",
        "vimeo" => "Recently uploaded",
        "tiktok" => "Explore",
        _ => "Recommended",
    }));
    main.push(div("grid").id("home-grid").each(ids.iter(), |id| video_card(state, id)));
    page(state, actor, &web::text(state, "brand"), "home", "", main)
}
/// Optional seed `sections`: `[{"title", "items": [ids]}]`, each a titled shelf of tiles
/// above the grid. Unknown ids are skipped and a seed without the key renders none.
fn sections(state: &Value) -> Vec<Html> {
    let mut out = vec![];
    let listed = state.get("sections").and_then(Value::as_array);
    for (index, section) in listed.into_iter().flatten().enumerate() {
        let tiles: Vec<Html> = web::strings(section, "items")
            .iter()
            .filter(|id| record(state, "items", id).is_some())
            .map(|id| video_tile(state, id, &format!("section-{index}"), format!("/watch?v={id}")))
            .collect();
        if tiles.is_empty() {
            continue;
        }
        out.push(
            el("section")
                .class("shelf")
                .child(el("h2").id(format!("section-{index}-title")).text(web::text(section, "title")))
                .child(div("grid").id(format!("section-{index}-grid")).children(tiles)),
        );
    }
    out
}
/// The watch page. `list` keeps a playlist queue in the sidebar so playback has somewhere to go.
pub fn watch(state: &Value, id: &str, list: Option<&str>, actor: &str) -> Result<HttpResponse> {
    let Some(item) = record(state, "items", id).cloned() else {
        return web::error(404, "video not found");
    };
    let w = words(view::skin(state, "video"));
    let back = match list {
        Some(l) => format!("/watch?v={id}&list={l}"),
        None => format!("/watch?v={id}"),
    };
    let channel_id = web::text(&item, "channel");
    let channel = record(state, "channels", &channel_id).cloned().unwrap_or(Value::Null);
    let subscribed = has(state, "subscriptions", actor, &channel_id);
    let liked = has(state, "likes", actor, id);
    let description = web::text(&item, "description");
    let title = web::text(&item, "title");
    let length = num(&item, "duration_s");
    let stage = div("stage")
        .id("player")
        .attr("role", "img")
        .attr("aria-label", title.as_str())
        .child(still("player-art", id, "", 512))
        .child(span("bigplay").attr("aria-hidden", "true").child(icon("play")))
        .child(flags(&item, "player"))
        .child(
            div("controls")
                .attr("aria-hidden", "true")
                .child(span("progress").child(el("i")))
                .child(
                    div("buttons")
                        .child(icon("play"))
                        .child(icon("next"))
                        .child(icon("volume"))
                        .child(span("clock").text(format!("0:00 / {}", clock(length))))
                        .child(span("grow"))
                        .child(icon("gear"))
                        .child(icon("full")),
                ),
        );
    let info = div("info")
        .id("watch-info")
        .child(div("heading").id("watch-heading").child(el("h1").id("watch-title").text(title.as_str())).child(flags(&item, "watch")))
        .child(
            div("owner")
                .id("watch-actions")
                .child(
                    el("a")
                        .id("watch-avatar-link")
                        .class("avatarlink")
                        .attr("href", format!("/channel/{channel_id}"))
                        .attr("aria-label", web::text(&channel, "name"))
                        .child(cover("watch-avatar", &channel_id, "", 48, 24)),
                )
                .child(
                    div("names")
                        .id("watch-channel")
                        .child(link("watch-channel-name", format!("/channel/{channel_id}"), web::text(&channel, "name")))
                        .child(span("subs").id("watch-subs").text(format!("{} {}", grouped(num(&channel, "subscribers")), w.subscribers))),
                )
                .child(act(
                    "watch-subscribe",
                    &format!("/channels/{channel_id}/subscribe"),
                    &[("return", &back)],
                    press(if subscribed { "pill subscribe on" } else { "pill subscribe" }, "")
                        .child(span("").id("watch-subscribe-text").text(if subscribed { w.subscribe.1 } else { w.subscribe.0 })),
                ))
                .child(span("grow"))
                .child(act(
                    "watch-like",
                    &format!("/items/{id}/like"),
                    &[("return", &back)],
                    press(if liked { "pill like on" } else { "pill like" }, if liked { "Remove like" } else { "Like" })
                        .child(icon(if liked { "like-fill" } else { "like" }))
                        .child(span("").id("watch-like-text").text(grouped(num(&item, "likes")))),
                ))
                .child(act(
                    "watch-later",
                    "/playlists/watch-later/items",
                    &[("item", id), ("return", &back)],
                    press("pill", "").child(icon("plus")).child(span("").id("watch-later-text").text(w.save)),
                )),
        )
        .child(
            div("description")
                .id("watch-description")
                .child(span("stats").id("watch-stats").text(format!("{} {} · {}", grouped(num(&item, "views")), w.views, web::text(&item, "published"))))
                .child(el("p").id("watch-body").text(description.as_str()))
                .children(view::links("watch", &description))
                .child(div("tags").each(web::strings(&item, "tags").iter().enumerate(), |(i, tag)| {
                    link(&format!("watch-tag-{i}"), href("/results", &[("search_query", tag)]), if tag.starts_with('#') { tag.clone() } else { format!("#{tag}") })
                })),
        )
        .child(el("hr").id("watch-divider"));
    let queue: Vec<String> = match list.and_then(|l| record(state, "playlists", l).cloned()) {
        Some(playlist) => web::strings(&playlist, "items"),
        None => web::strings(&item, "related"),
    };
    let side = el("aside")
        .id("watch-side")
        .class("upnext")
        .child(el("h2").id("queue-heading").text(match list {
            Some(l) => record(state, "playlists", l).map(|p| web::text(p, "title")).unwrap_or_else(|| w.up_next.into()),
            None => w.up_next.into(),
        }))
        .each(queue.iter().filter(|n| *n != id && record(state, "items", n).is_some()), |next| {
            let to = match list {
                Some(l) => format!("/watch?v={next}&list={l}"),
                None => format!("/watch?v={next}"),
            };
            item_row(state, next, "queue", to, false)
        });
    page(
        state,
        actor,
        &title,
        "watch",
        "",
        vec![div("watch-layout")
            .id("watch-layout")
            .child(div("primary").id("watch-main").child(stage).child(info))
            .child(comment_block(state, id, actor, &back, &w))
            .child(side)],
    )
}
pub fn channel_page(state: &Value, id: &str, actor: &str) -> Result<HttpResponse> {
    let Some(channel) = record(state, "channels", id).cloned() else {
        return web::error(404, "channel not found");
    };
    let w = words(view::skin(state, "video"));
    let name = web::text(&channel, "name");
    let subscribed = has(state, "subscriptions", actor, id);
    let owned = by_recency(state, |item| web::text(item, "channel") == id);
    let lists: Vec<String> = keys(state, "playlists")
        .into_iter()
        .filter(|l| {
            record(state, "playlists", l)
                .is_some_and(|p| web::strings(p, "items").iter().any(|i| owned.contains(i)))
        })
        .collect();
    let main = vec![
        div("banner").id("channel-banner").child(still("channel-banner-art", &format!("{id}-banner"), "", 512)),
        div("identity")
            .id("channel-header")
            .child(cover("channel-avatar", id, &name, 160, 80))
            .child(
                div("names")
                    .id("channel-identity")
                    .child(el("h1").id("channel-name").text(name.as_str()))
                    .child(span("handle").id("channel-handle").text(format!(
                        "{} · {} {} · {} videos",
                        web::text(&channel, "handle"),
                        grouped(num(&channel, "subscribers")),
                        w.subscribers,
                        owned.len()
                    )))
                    .child(el("p").id("channel-about").text(web::text(&channel, "about")))
                    .children(view::links("channel-about", &web::text(&channel, "about")))
                    .child(act(
                        "channel-subscribe",
                        &format!("/channels/{id}/subscribe"),
                        &[("return", &format!("/channel/{id}"))],
                        press(if subscribed { "pill subscribe on" } else { "pill subscribe" }, "")
                            .child(span("").id("channel-subscribe-text").text(if subscribed { w.subscribe.1 } else { w.subscribe.0 })),
                    )),
            ),
        el("nav")
            .id("channel-tabs")
            .class("tabs")
            .child(link("channel-tab-videos", format!("/channel/{id}"), "Videos").class("tab on"))
            .each(lists.iter().take(4), |l| {
                link(
                    &format!("channel-tab-{l}"),
                    href("/playlist", &[("list", l)]),
                    record(state, "playlists", l).map(|p| web::text(p, "title")).unwrap_or_default(),
                )
                .class("tab")
            }),
        el("hr").id("channel-divider"),
        div("grid").id("channel-grid").each(owned.iter(), |item| video_card(state, item)),
    ];
    page(state, actor, &name, "channel", "", main)
}
pub fn results(state: &Value, query: &str, actor: &str) -> Result<HttpResponse> {
    let w = words(view::skin(state, "video"));
    let hits: Vec<String> = by_recency(state, |_| true).into_iter().filter(|id| matches(state, id, query)).collect();
    let mut main = vec![el("h1").id("results-heading").text(format!("{} results for \"{query}\"", hits.len()))];
    // The channel rows, built once. The real results page leads with the best channel
    // match and puts the others below the videos rather than ahead of them, so a search
    // for a common letter does not bury the videos under a wall of avatars.
    let channel_rows: Vec<Html> = keys(state, "channels")
        .into_iter()
        .filter(|channel| {
            record(state, "channels", channel)
                .map(|c| web::text(c, "name"))
                .unwrap_or_default()
                .to_lowercase()
                .contains(&query.to_lowercase())
        })
        .map(|channel| {
        let c = record(state, "channels", &channel).cloned().unwrap_or(Value::Null);
        let name = web::text(&c, "name");
            el("a").id(format!("result-channel-{channel}")).class("rowcard channelcard").attr("href", format!("/channel/{channel}")).child(
                span("row")
                    .id(format!("result-channel-{channel}-row"))
                    .child(span("thumb round").child(cover(&format!("result-channel-{channel}-art"), &channel, "", 136, 68)))
                    .child(
                        span("text")
                            .child(span("title").id(format!("result-channel-{channel}-name")).text(name))
                            .child(span("channel").text(format!(
                                "{} · {} {}",
                                web::text(&c, "handle"),
                                grouped(num(&c, "subscribers")),
                                w.subscribers
                            )))
                            .child(span("snip").text(web::text(&c, "about"))),
                    ),
            )
        })
        .collect();
    let mut channel_rows = channel_rows.into_iter();
    main.extend(channel_rows.by_ref().take(1));
    for id in &hits {
        main.push(item_row(state, id, "result", format!("/watch?v={id}"), true));
    }
    main.extend(channel_rows);
    if hits.is_empty() {
        main.push(el("p").id("results-empty").class("muted").text("Try different keywords, or fewer of them."));
    }
    page(state, actor, &format!("{query} - search"), "results", query, main)
}
pub fn playlist_page(state: &Value, id: &str, actor: &str) -> Result<HttpResponse> {
    let Some(playlist) = record(state, "playlists", id).cloned() else {
        return web::error(404, "playlist not found");
    };
    let tracks: Vec<String> = web::strings(&playlist, "items").into_iter().filter(|t| record(state, "items", t).is_some()).collect();
    let title = web::text(&playlist, "title");
    let owner = match web::text(&playlist, "owner") {
        o if o.is_empty() => "everyone".to_owned(),
        o => o,
    };
    let header = div("listhead")
        .id("playlist-header")
        .child(match tracks.first() {
            Some(first) => still("playlist-art", first, &title, 512),
            None => still("playlist-art", id, &title, 512),
        })
        .child(
            div("identity")
                .id("playlist-identity")
                .child(el("h1").id("playlist-title").text(title.as_str()))
                .child(span("meta").id("playlist-meta").text(format!("{owner} · {} videos", tracks.len())))
                .child(match tracks.first() {
                    Some(first) => el("a")
                        .id("playlist-play")
                        .class("pill solid")
                        .attr("href", format!("/watch?v={first}&list={id}"))
                        .child(icon("play"))
                        .child(span("").id("playlist-play-text").text("Play all")),
                    None => span("muted").id("playlist-play").text("Nothing added yet"),
                }),
        );
    let lines = div("lines").id("playlist-lines").each(tracks.iter().enumerate(), |(index, track)| {
        div("line")
            .id(format!("playlist-line-{index}"))
            .child(span("index").id(format!("playlist-index-{index}")).text((index + 1).to_string()))
            .child(item_row(state, track, "playlist", format!("/watch?v={track}&list={id}"), false))
    });
    page(
        state,
        actor,
        &title,
        "playlist",
        "",
        vec![div("listpage").child(header).child(el("hr").id("playlist-divider")).child(lines), el("hr").id("playlist-foot"), track_bar(state, actor)],
    )
}
/// Library: every playlist plus the form that creates one, which is what makes them buildable.
pub fn library(state: &Value, actor: &str) -> Result<HttpResponse> {
    let w = words(view::skin(state, "video"));
    let cards = div("grid").id("library-grid").each(keys(state, "playlists").iter(), |id| {
        let playlist = record(state, "playlists", id).cloned().unwrap_or(Value::Null);
        let items = web::strings(&playlist, "items");
        el("a")
            .id(format!("library-{id}"))
            .class("tile stack")
            .attr("href", href("/playlist", &[("list", id)]))
            .child(
                span("thumb")
                    .child(still(&format!("library-{id}-art"), items.first().unwrap_or(id), "", 256))
                    .child(span("time").text(format!("{} videos", items.len()))),
            )
            .child(
                span("details").child(
                    span("text")
                        .child(span("title").id(format!("library-{id}-title")).text(web::text(&playlist, "title")))
                        .child(span("channel").id(format!("library-{id}-count")).text(format!("{} tracks", items.len()))),
                ),
            )
    });
    page(
        state,
        actor,
        w.library,
        "library",
        "",
        vec![
            el("h1").id("library-heading").text("Your playlists"),
            cards,
            el("hr").id("library-divider"),
            el("h2").id("playlist-heading").text("New playlist"),
            field_form("playlist", "/playlists", "post", "title", "New playlist", "", "Create").class("createform"),
        ],
    )
}
/// What is parked in `now_playing`: inert text plus one real link to it.
fn track_bar(state: &Value, actor: &str) -> Html {
    let Some(playing) = record(state, "now_playing", actor).cloned() else {
        return div("nowbar idle").id("bar").text("Nothing playing");
    };
    let id = web::text(&playing, "item");
    let item = record(state, "items", &id).cloned().unwrap_or(Value::Null);
    div("nowbar")
        .id("bar")
        .child(cover("bar-art", &id, "", 48, 4))
        .child(link("bar-title", format!("/track/{id}"), web::text(&item, "title")))
        .child(span("muted").id("bar-meta").text(format!(
            "{} · {} views",
            channel_name(state, &web::text(&item, "channel")),
            grouped(num(&item, "views"))
        )))
}
/// `/track/<id>` on a video site: the item as a plain detail page with its controls.
pub fn track_page(state: &Value, id: &str, actor: &str) -> Result<HttpResponse> {
    let Some(item) = record(state, "items", id).cloned() else {
        return web::error(404, "track not found");
    };
    let channel = web::text(&item, "channel");
    let back = format!("/track/{id}");
    let title = web::text(&item, "title");
    let liked = has(state, "likes", actor, id);
    let main = vec![
        div("listhead")
            .id("track-header")
            .child(still("track-art", id, &title, 512))
            .child(
                div("identity")
                    .id("track-identity")
                    .child(el("h1").id("track-title").text(title.as_str()))
                    .child(link("track-artist", format!("/channel/{channel}"), channel_name(state, &channel)))
                    .child(span("meta").id("track-meta").text(format!(
                        "{} · {} · {} plays",
                        web::text(&item, "album"),
                        clock(num(&item, "duration_s")),
                        grouped(num(&item, "plays"))
                    )))
                    .child(
                        div("buttons")
                            .id("track-actions")
                            .child(act(
                                "track-play",
                                &format!("/items/{id}/play"),
                                &[("return", &back)],
                                press("pill solid", "").child(icon("play")).child(span("").text("Play")),
                            ))
                            .child(act(
                                "track-like",
                                &format!("/items/{id}/like"),
                                &[("return", &back)],
                                press(if liked { "pill like on" } else { "pill like" }, "").text(format!("♥ {}", grouped(num(&item, "likes")))),
                            ))
                            .child(act(
                                "track-save",
                                "/playlists/liked/items",
                                &[("item", id), ("return", &back)],
                                press("pill", "").text("Add to playlist"),
                            )),
                    ),
            ),
        track_bar(state, actor),
    ];
    page(state, actor, &title, "playlist", "", main)
}
/// Brand splash for a seed with no catalogue; nothing on it reads as a control.
pub fn landing(state: &Value) -> Result<HttpResponse> {
    let pick = |key: &str, fallback: &str| match web::text(state, key) {
        s if s.is_empty() => fallback.to_owned(),
        s => s,
    };
    let brand = pick("brand", BRAND);
    let doc = view::document(
        state,
        "video",
        &brand,
        "page-landing",
        vec![el("main").class("landing").child(el("h1").id("brand").text(brand.as_str())).child(el("p").id("tagline").text(pick("tagline", TAGLINE)))],
    );
    web::html::page(&doc)
}
