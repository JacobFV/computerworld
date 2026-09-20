//! Page rendering as HTML. Every site runs the same routes over the same state; the skin
//! decides what it looks like: X's three columns on black, Bluesky's and Mastodon's
//! variations of it, Facebook's blue bar over a grey page of cards, Instagram's icon rail
//! and centred square pictures, LinkedIn's profile card beside the feed, Pinterest's wall
//! of pins. `social.css` holds the structure every skin shares and `<skin>.css` the look;
//! the palette a seed carries goes on `<html>` as custom properties.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `brand`, `nav`,
//! `nav-home|explore|search|inbox|me` (links, with `-text` inside), `compose-box`,
//! `compose` (form), `compose-text`, `compose-submit`, `feed-title`, `feed-empty`,
//! `feed-grid`, `post-<id>` (the link that opens the thread), `<id>-row`, `-avatar`,
//! `-head`, `-name`, `-handle`, `-verified`, `-photo`, `-text`, `-quote`, `-quote-by`,
//! `-quote-text`, `-acts`, `-like`, `-repost` (buttons, with `-label` inside),
//! `-replies` (a link, and on the post a thread page is about a plain count instead),
//! `suggest-<n>` with its own `suggest-<n>-follow` button, `profile`, `profile-avatar`,
//! `name`, `handle`, `headline`, `bio`,
//! `followers`, `following-count`, `location`, `site`, `follow`, `connect`, `exp-<n>`,
//! `reply` (form), `reply-text`, `reply-submit`, `replies-title`, `search` (form),
//! `search-q`, `search-submit`, `inbox-title`, `inbox-empty`, `thread-<id>` (link),
//! `open-title`, `dm-<id>`, `dm` (form), `dm-text`, `dm-submit`.
use crate::{Account, SocialState};
use cw_protocol::{HttpResponse, Result as SimResult};
use cw_sdk::ServiceContext;
use cw_service_common as web;
use cw_service_common::html::{
    self, a, button, div, el, form, hidden, link, span, text_input, Document, Html as Node,
};

const BASE: &str = include_str!("social.css");
/// The skins a seed may name with `skin`; absent, the brand and then the mode decide.
pub const SKINS: &[&str] = &[
    "x",
    "bsky",
    "mastodon",
    "facebook",
    "instagram",
    "linkedin",
    "pinterest",
];
fn sheet(skin: &str) -> &'static str {
    match skin {
        "bsky" => include_str!("bsky.css"),
        "mastodon" => include_str!("mastodon.css"),
        "facebook" => include_str!("facebook.css"),
        "instagram" => include_str!("instagram.css"),
        "linkedin" => include_str!("linkedin.css"),
        "pinterest" => include_str!("pinterest.css"),
        _ => include_str!("x.css"),
    }
}
/// The skin of a state: the seed's `skin` when it names one, else the brand it stands in
/// for, else the look that goes with its mode.
pub fn skin_of(s: &SocialState) -> &'static str {
    if let Some(named) = SKINS.iter().find(|k| **k == s.skin) {
        return named;
    }
    let brand = s.brand.to_ascii_lowercase();
    let by_brand = [
        ("bluesky", "bsky"),
        ("bsky", "bsky"),
        ("mastodon", "mastodon"),
        ("facebook", "facebook"),
        ("instagram", "instagram"),
        ("linkedin", "linkedin"),
        ("pinterest", "pinterest"),
    ];
    if let Some((_, skin)) = by_brand.iter().find(|(needle, _)| brand.contains(needle)) {
        return skin;
    }
    match s.mode.as_str() {
        "professional" => "linkedin",
        "photos" => "instagram",
        _ => "x",
    }
}

const TINTS: [&str; 6] = [
    "#1d9bf0", "#f91880", "#00ba7c", "#7856ff", "#ff7a00", "#e0245e",
];
/// Avatar colour is a pure FNV-1a over the handle: stable across runs, machines and snapshots,
/// and no seeded stream is reachable from a render.
fn hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}
fn tint(handle: &str) -> &'static str {
    TINTS[(hash(handle) % TINTS.len() as u64) as usize]
}
fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect();
    if letters.is_empty() {
        "?".into()
    } else {
        letters.to_uppercase()
    }
}
/// Age in ticks, which is the only clock a service has.
fn ago(now: u64, tick: u64) -> String {
    match now.saturating_sub(tick) {
        0 => "now".into(),
        n => format!("{n}t"),
    }
}
fn display(s: &SocialState, handle: &str) -> String {
    match s.accounts.get(handle) {
        Some(a) if !a.domain.is_empty() => format!("@{}@{}", a.handle, a.domain),
        _ => format!("@{handle}"),
    }
}
/// A glyph drawn by the stylesheet (`.ico-home::before`), so it never reaches a link's text.
fn ico(name: &str) -> Node {
    el("i")
        .class(&format!("ico ico-{name}"))
        .attr("aria-hidden", "true")
}
fn avatar(id: &str, account: Option<&Account>, handle: &str, class: &str) -> Node {
    let name = account.map_or(handle, |a| a.name.as_str());
    span(&format!("avatar {class}"))
        .id(id)
        .style(&format!("background-color: {}", tint(handle)))
        .attr("title", name)
        .text(initials(name))
}
/// A one-button form: a control that mutates and comes back to `view`.
fn pill(id: &str, icon: &str, word: &str, count: Option<u64>, on: bool, url: &str, view: &str) -> Node {
    let label = match count {
        Some(n) => format!("{word} {n}"),
        None => word.to_owned(),
    };
    form(&format!("{id}-form"), url, "post")
        .class("pill-form")
        .child(hidden("view", view))
        .child(
            button(id, "")
                .class(if on { "pill on" } else { "pill" })
                .attr("aria-label", label.as_str())
                .attr("aria-pressed", if on { "true" } else { "false" })
                .child(ico(icon))
                .child(
                    span("pill-label")
                        .id(format!("{id}-label"))
                        .child(span("lbl").text(word))
                        .maybe(count.map(|n| span("n").text(format!(" {n}")))),
                ),
        )
}

pub(crate) struct View<'a> {
    s: &'a SocialState,
    ctx: &'a ServiceContext,
    skin: &'static str,
    /// The path the page is at: what a like or a follow comes back to.
    here: String,
    /// `home`, `explore`, `profile`, `thread`, `search` or `inbox`: a class on `<body>`.
    page: &'static str,
}
impl<'a> View<'a> {
    pub(crate) fn new(s: &'a SocialState, ctx: &'a ServiceContext, here: &str, page: &'static str) -> Self {
        Self {
            s,
            ctx,
            skin: skin_of(s),
            here: here.to_owned(),
            page,
        }
    }
    fn me(&self) -> Option<&'a Account> {
        self.s.account_of(&self.ctx.actor)
    }
    fn explore_title(&self) -> &'a str {
        if self.s.explore_title.is_empty() {
            "Explore"
        } else {
            self.s.explore_title.as_str()
        }
    }
    fn inbox_root(&self) -> &'static str {
        if self.s.professional() {
            "/messaging"
        } else {
            "/messages"
        }
    }
    /// Facebook, LinkedIn and Pinterest keep their navigation in a bar across the top; the
    /// others in a rail down the left.
    fn topbar(&self) -> bool {
        matches!(self.skin, "facebook" | "linkedin" | "pinterest")
    }
    fn brand(&self) -> Node {
        a("/")
            .id("brand")
            .class("brand")
            .child(span("logo").attr("aria-hidden", "true"))
            .child(span("wordmark").text(self.s.brand.as_str()))
    }
    fn nav(&self) -> Node {
        let mut items = vec![
            ("nav-home", "home", "Home", "/".to_owned()),
            ("nav-explore", "explore", self.explore_title(), "/explore".to_owned()),
            ("nav-search", "search", "Search", "/search".to_owned()),
            ("nav-inbox", "inbox", "Messages", self.inbox_root().to_owned()),
        ];
        if let Some(me) = self.me() {
            items.push(("nav-me", "me", "Profile", format!("/{}", me.handle)));
        }
        el("nav").id("nav").class("nav").each(items, |(id, icon, label, url)| {
            let current = url == self.here;
            a(url.as_str())
                .id(id)
                .class(if current { "nav-item on" } else { "nav-item" })
                .when(current, |n| n.attr("aria-current", "page"))
                .child(ico(icon))
                .child(span("nav-text").id(format!("{id}-text")).text(label))
        })
    }
    /// The search box the chrome carries on every page; the search page has its own.
    /// The magnifier is the submit button, because that is the thing on the page that
    /// looks like it sends the search.
    fn top_search(&self) -> Node {
        form("top-search", "/search", "post")
            .class("top-search")
            .attr("role", "search")
            .child(button("top-search-go", "").attr("aria-label", "Search").child(ico("search")))
            .child(
                text_input("top-q", "q", "")
                    .attr("aria-label", format!("Search {}", self.s.brand))
                    .attr("placeholder", "Search")
                    .attr("autocomplete", "off"),
            )
    }
    /// The signed-in account, as the chrome shows it: a link to the profile.
    fn me_card(&self, id: &str) -> Option<Node> {
        let me = self.me()?;
        Some(
            a(format!("/{}", me.handle))
                .id(id)
                .class("me-card")
                .child(avatar(&format!("{id}-avatar"), Some(me), &me.handle, "mid"))
                .child(
                    span("me-who")
                        .child(span("me-name").text(me.name.as_str()))
                        .child(span("me-handle").text(if self.s.professional() {
                            me.headline.clone()
                        } else {
                            display(self.s, &me.handle)
                        })),
                ),
        )
    }
    /// True when `handle`'s profile is not the page being read: a row that lands the reader
    /// where they already stand is a link with nothing behind it, so no list offers one.
    fn elsewhere(&self, handle: &str) -> bool {
        format!("/{handle}") != self.here
    }
    /// Accounts the actor does not follow yet, the biggest first.
    fn suggestions(&self, title: &str, take: usize) -> Option<Node> {
        let followed = self.s.follows.get(&self.ctx.actor);
        let mine = self.me().map(|m| m.handle.as_str());
        let mut rest: Vec<&Account> = self
            .s
            .accounts
            .values()
            .filter(|a| {
                Some(a.handle.as_str()) != mine
                    && self.elsewhere(&a.handle)
                    && !followed.is_some_and(|f| f.contains(&a.handle))
            })
            .collect();
        rest.sort_by(|x, y| y.followers.cmp(&x.followers).then_with(|| x.handle.cmp(&y.handle)));
        if rest.is_empty() {
            return None;
        }
        Some(
            el("section")
                .id("suggest")
                .class("panel")
                .child(el("h2").class("panel-title").text(title))
                .each(rest.into_iter().take(take).enumerate(), |(i, acct)| self.person(&format!("suggest-{i}"), acct, true)),
        )
    }
    /// One account as a row that opens its profile. `follow` adds the real Follow control
    /// beside it — a one-button form of its own, because a button cannot live inside a link
    /// and a pill that only opened the profile would be a lie about what it does.
    fn person(&self, id: &str, acct: &Account, follow: bool) -> Node {
        div("person")
            .child(
                a(format!("/{}", acct.handle))
                    .id(id)
                    .class("person-link")
                    .child(avatar(&format!("{id}-avatar"), Some(acct), &acct.handle, "mid"))
                    .child(
                        span("person-who")
                            .child(span("person-name").text(acct.name.as_str()))
                            .child(span("person-handle").text(if self.s.professional() && !acct.headline.is_empty() {
                                acct.headline.clone()
                            } else {
                                display(self.s, &acct.handle)
                            })),
                    ),
            )
            .when(follow, |n| {
                n.child(pill(
                    &format!("{id}-follow"),
                    "none",
                    if self.s.professional() { "Connect" } else { "Follow" },
                    None,
                    false,
                    &format!("/accounts/{}/follow", acct.handle),
                    &self.here,
                ))
            })
    }
    /// What is being talked about: the posts with the most reach, each a link to its thread.
    fn trends(&self, title: &str) -> Option<Node> {
        // Never offer the reader the page they are already on: a trend row that lands where
        // you stand is a link with nothing behind it.
        let top: Vec<String> = self
            .s
            .explore()
            .into_iter()
            .filter(|id| format!("/{}/status/{id}", self.s.posts[id].author) != self.here)
            .collect();
        if top.is_empty() {
            return None;
        }
        Some(
            el("section")
                .id("trends")
                .class("panel")
                .child(el("h2").class("panel-title").text(title))
                .each(top.iter().take(4).enumerate(), |(i, id)| {
                    let post = &self.s.posts[id];
                    let name = self.s.accounts.get(&post.author).map_or(post.author.as_str(), |a| a.name.as_str());
                    let mut words: String = post.text.chars().take(64).collect();
                    if words.len() < post.text.len() {
                        words.push('…');
                    }
                    a(format!("/{}/status/{id}", post.author))
                        .id(format!("trend-{i}"))
                        .class("trend")
                        .child(span("trend-kind").text(format!("{name} · Trending")))
                        .child(span("trend-text").text(words))
                        .child(span("trend-count").text(format!("{} likes", self.s.likes_of(id))))
                }),
        )
    }
    /// The accounts the actor follows, as Facebook's contacts and Instagram's stories.
    fn followed(&self) -> Vec<&'a Account> {
        self.s
            .follows
            .get(&self.ctx.actor)
            .into_iter()
            .flatten()
            .filter(|h| self.elsewhere(h))
            .filter_map(|h| self.s.accounts.get(h))
            .collect()
    }
    /// One of Facebook's shortcut rows: a link that says when it is the page being read.
    fn shortcut(&self, id: &str, icon: &str, url: &str, label: &str) -> Node {
        let current = url == self.here;
        a(url)
            .id(id)
            .class(if current { "shortcut on" } else { "shortcut" })
            .when(current, |n| n.attr("aria-current", "page"))
            .child(ico(icon))
            .child(span("").text(label))
    }
    fn left(&self) -> Option<Node> {
        match self.skin {
            "facebook" => Some(
                el("aside")
                    .id("left")
                    .class("left")
                    .maybe(self.me_card("short-me"))
                    .child(self.shortcut("short-groups", "explore", "/explore", self.explore_title()))
                    .child(self.shortcut("short-inbox", "inbox", self.inbox_root(), "Messenger"))
                    .child(self.shortcut("short-search", "search", "/search", "Find friends"))
                    .child(el("h2").class("left-title").text("Your shortcuts"))
                    .each(
                        self.followed().into_iter().filter(|a| a.actor.is_none()).take(5).enumerate(),
                        |(i, acct)| self.person(&format!("short-{i}"), acct, false),
                    ),
            ),
            "linkedin" => {
                let me = self.me()?;
                let links = self.s.connections.get(&self.ctx.actor).map_or(0, |c| c.len());
                Some(
                    el("aside").id("left").class("left").child(
                        el("section")
                            .class("card-me")
                            .child(div("card-banner"))
                            .child(
                                a(format!("/{}", me.handle))
                                    .id("card-me")
                                    .class("card-me-link")
                                    .child(avatar("card-me-avatar", Some(me), &me.handle, "big"))
                                    .child(span("card-me-name").text(me.name.as_str()))
                                    .child(span("card-me-headline").text(me.headline.as_str())),
                            )
                            .child(
                                div("card-stats")
                                    .child(div("card-stat").child(span("").text("Connections")).child(el("b").text(links.to_string())))
                                    .child(div("card-stat").child(span("").text("Followers")).child(el("b").text(me.followers.to_string()))),
                            ),
                    ),
                )
            }
            _ => None,
        }
    }
    fn right(&self) -> Option<Node> {
        if self.page == "inbox" || self.skin == "pinterest" {
            return None;
        }
        let aside = el("aside").id("side").class("side");
        Some(match self.skin {
            "facebook" => {
                let contacts = self.followed();
                aside.child(
                    el("section")
                        .id("contacts")
                        .class("panel")
                        .child(el("h2").class("panel-title").text("Contacts"))
                        .each(contacts.into_iter().take(10).enumerate(), |(i, acct)| self.person(&format!("contact-{i}"), acct, false)),
                )
            }
            "instagram" => aside
                .maybe(self.me_card("side-me"))
                .maybe(self.suggestions("Suggested for you", 5))
                .child(el("p").class("side-foot").text(self.s.tagline.as_str())),
            "linkedin" => aside
                .maybe(self.suggestions("Add to your feed", 3))
                .child(el("p").class("side-foot").text(self.s.tagline.as_str())),
            _ => aside
                .child(self.top_search())
                .maybe(self.trends(if self.skin == "x" { "What's happening" } else { "Trending now" }))
                .maybe(self.suggestions("Who to follow", 3))
                .child(el("p").class("side-foot").text(self.s.tagline.as_str())),
        })
    }
    /// The page shell: the chrome around `main`, the two stylesheets, the palette, the skin.
    fn document(&self, title: &str, main: Vec<Node>) -> SimResult<HttpResponse> {
        let t = &self.s.theme;
        let or = |v: &Option<String>, fallback: &str| v.clone().unwrap_or_else(|| fallback.to_owned());
        let root = format!(
            "--accent: {}; --ink: {}; --muted: {}; --surface: {}; --paper: {}; --content: {}px",
            or(&t.accent, "#1d9bf0"),
            or(&t.ink, "#0f1419"),
            or(&t.muted, "#536471"),
            or(&t.surface, "#ffffff"),
            or(&t.background, "#ffffff"),
            t.content_width.unwrap_or(900).clamp(320, 1400)
        );
        let main = el("main").id("main").class("main").children(main);
        let shell = div("shell");
        let body = if self.topbar() {
            vec![
                el("header").id("topbar").class("topbar").child(
                    div("topbar-in")
                        .child(self.brand())
                        .child(self.top_search())
                        .child(self.nav())
                        .maybe(self.me().map(|me| avatar("top-me", Some(me), &me.handle, "small"))),
                ),
                shell.maybe(self.left()).child(main).maybe(self.right()),
            ]
        } else {
            vec![shell
                .child(
                    el("header")
                        .id("rail")
                        .class("rail")
                        .child(self.brand())
                        .child(self.nav())
                        .maybe(self.me_card("rail-me")),
                )
                .child(main)
                .maybe(self.right())]
        };
        html::page(
            &Document::new(title)
                .lang("en")
                .stylesheet(BASE)
                .stylesheet(sheet(self.skin))
                .root_style(&root)
                .body_class(&format!("skin-{} mode-{} page-{}", self.skin, self.s.mode, self.page))
                .body(body),
        )
    }
    /// One timeline entry. The text (and the picture) is the link that opens the thread; the
    /// like and repost buttons are their own forms, so a like never costs a navigation.
    fn post(&self, id: &str) -> Node {
        self.post_at(id, false)
    }
    /// `focused` is the post a thread page is about: on its own page its text opens nothing
    /// and its reply count is a count, not a link back to where the reader already is.
    fn post_at(&self, id: &str, focused: bool) -> Node {
        let s = self.s;
        let post = &s.posts[id];
        let acct = s.accounts.get(&post.author);
        let liked = s.likes.get(id).is_some_and(|l| l.contains(&self.ctx.actor));
        let boosted = s.reposts.get(id).is_some_and(|r| r.contains(&self.ctx.actor));
        let thread = format!("/{}/status/{id}", post.author);
        let head = el("header")
            .id(format!("{id}-head"))
            .class("post-head")
            .child(avatar(&format!("{id}-avatar"), acct, &post.author, "mid"))
            .child(
                div("who")
                    .child({
                        // On the author's own profile the name is the page's own heading:
                        // a link back to it would land the reader where they already are.
                        let name = acct.map_or(post.author.clone(), |a| a.name.clone());
                        if self.here == format!("/{}", post.author) {
                            span("name").id(format!("{id}-name")).text(name)
                        } else {
                            link(&format!("{id}-name"), format!("/{}", post.author), name).class("name")
                        }
                    })
                    .when(acct.is_some_and(|a| a.verified), |n| {
                        n.child(span("verified").id(format!("{id}-verified")).attr("title", "Verified").text("✓"))
                    })
                    .child(span("handle").id(format!("{id}-handle")).text(if s.professional() {
                        acct.map_or(String::new(), |a| a.headline.clone())
                    } else {
                        format!("{} · {}", display(s, &post.author), ago(self.ctx.tick, post.tick))
                    })),
            );
        let open = if focused { div("post-link here") } else { a(thread.as_str()).id(format!("post-{id}")).class("post-link") }
            .when(s.photos(), |n| {
                // The picture itself: a tile in the author's tint, named after what it shows.
                let shows = if post.image.is_empty() { "Photo" } else { post.image.as_str() };
                n.child(
                    span(&format!("photo shape-{}", hash(id) % 4))
                        .id(format!("{id}-photo"))
                        .style(&format!("background-color: {}", tint(&post.author)))
                        .child(span("photo-label").text(shows)),
                )
            })
            .child(span("post-text").id(format!("{id}-text")).text(post.text.as_str()));
        let quote = post.quoted.as_ref().filter(|q| s.posts.contains_key(*q)).map(|q| {
            let quoted = &s.posts[q];
            div("quote")
                .id(format!("{id}-quote"))
                .child(span("quote-by").id(format!("{id}-quote-by")).text(display(s, &quoted.author)))
                .child(span("quote-text").id(format!("{id}-quote-text")).text(quoted.text.as_str()))
        });
        let acts = div("acts")
            .id(format!("{id}-acts"))
            .child(
                if focused { span("act replies here") } else { a(thread.as_str()).class("act replies") }
                    .id(format!("{id}-replies"))
                    .child(ico("reply"))
                    .child(span("n").text(s.replies_to(id).len().to_string()))
                    .child(span("lbl").text(if s.replies_to(id).len() == 1 { " reply" } else { " replies" })),
            )
            .child(pill(
                &format!("{id}-repost"),
                "repost",
                if boosted { "Reposted" } else { "Repost" },
                Some(s.reposts_of(id)),
                boosted,
                &format!("/posts/{id}/repost"),
                &self.here,
            ))
            .child(pill(
                &format!("{id}-like"),
                "like",
                if liked { "Liked" } else { "Like" },
                Some(s.likes_of(id)),
                liked,
                &format!("/posts/{id}/like"),
                &self.here,
            ));
        el("article")
            .id(format!("{id}-row"))
            .class(if post.reply_to.is_some() { "post is-reply" } else { "post" })
            .child(head)
            .child(div("post-body").id(format!("{id}-body")).child(open).maybe(quote).child(acts))
    }
    fn feed(&self, title: &str, ids: &[String]) -> Vec<Node> {
        let mut out = vec![el("h2").id("feed-title").class("feed-title").text(title)];
        if ids.is_empty() {
            out.push(el("p").id("feed-empty").class("empty").text("Nothing here yet."));
        }
        if !self.s.photos() {
            out.extend(ids.iter().map(|id| self.post(id)));
            return out;
        }
        // A photo feed is a wall of tiles. Pinterest's is a masonry, which the engine has no
        // multi-column layout for, so the pins are dealt round-robin into flex columns.
        let lanes = if self.skin == "pinterest" { 5 } else { 1 };
        let mut grid = div("feed-grid").id("feed-grid");
        if lanes == 1 {
            grid = grid.each(ids, |id| self.post(id));
        } else {
            for lane in 0..lanes {
                grid = grid.child(div("lane").each(ids.iter().skip(lane).step_by(lanes), |id| self.post(id)));
            }
        }
        out.push(grid);
        out
    }
    pub(crate) fn timeline(&self, home: bool) -> SimResult<HttpResponse> {
        let s = self.s;
        let (title, ids) = if home {
            (if s.professional() { "Feed" } else { "Home" }, s.home(&self.ctx.actor))
        } else {
            (self.explore_title(), s.explore())
        };
        let mut main = vec![];
        if !self.topbar() && !s.photos() {
            main.push(
                div("tabs")
                    .id("tabs")
                    .child(
                        link("tab-home", "/", if self.skin == "x" { "For you" } else { "Following" })
                            .class(if home { "tab on" } else { "tab" })
                            .when(home, |n| n.attr("aria-current", "page")),
                    )
                    .child(
                        link("tab-explore", "/explore", self.explore_title())
                            .class(if home { "tab" } else { "tab on" })
                            .when(!home, |n| n.attr("aria-current", "page")),
                    ),
            );
        }
        if home && self.skin == "instagram" {
            let stories = self.followed();
            if !stories.is_empty() {
                main.push(div("stories").id("stories").each(stories.into_iter().take(8).enumerate(), |(i, acct)| {
                    a(format!("/{}", acct.handle))
                        .id(format!("story-{i}"))
                        .class("story")
                        .child(span("ring").child(avatar(&format!("story-{i}-avatar"), Some(acct), &acct.handle, "big")))
                        .child(span("story-name").text(acct.handle.as_str()))
                }));
            }
        }
        if let Some(me) = self.me() {
            let prompt = if s.professional() {
                "Share an update"
            } else if s.photos() {
                "Share a photo"
            } else {
                "What is happening?"
            };
            let placeholder = match self.skin {
                "x" => "What is happening?!".to_owned(),
                "bsky" => "What's up?".to_owned(),
                "mastodon" => "What's on your mind?".to_owned(),
                "facebook" => format!("What's on your mind, {}?", me.name.split_whitespace().next().unwrap_or("")),
                "linkedin" => "Start a post".to_owned(),
                _ => "Write a caption...".to_owned(),
            };
            let go = match self.skin {
                "mastodon" => "Publish",
                "instagram" => "Share",
                "pinterest" => "Create",
                _ => "Post",
            };
            main.push(
                el("section")
                    .id("compose-box")
                    .class("compose")
                    .child(avatar("compose-avatar", Some(me), &me.handle, "mid"))
                    .child(
                        form("compose", "/posts", "post")
                            .child(el("p").id("compose-title").class("compose-title").text(prompt))
                            .child(
                                text_input("compose-text", "text", "")
                                    .attr("aria-label", "Post")
                                    .attr("placeholder", placeholder)
                                    .attr("autocomplete", "off"),
                            )
                            .child(
                                div("compose-bar")
                                    .child(span("compose-tools").attr("aria-hidden", "true").child(ico("photo")).child(ico("gif")).child(ico("poll")))
                                    .child(button("compose-submit", go).class("primary")),
                            ),
                    ),
            );
        }
        main.extend(self.feed(title, &ids));
        self.document(&format!("{} / {title}", s.brand), main)
    }
    pub(crate) fn profile(&self, handle: &str) -> SimResult<HttpResponse> {
        let s = self.s;
        let Some(acct) = s.accounts.get(handle) else {
            return web::error(404, "no such account");
        };
        let following = s.follows.get(&self.ctx.actor).is_some_and(|f| f.contains(handle));
        let connected = s.connections.get(&self.ctx.actor).is_some_and(|c| c.contains(handle));
        let mine = self.me().is_some_and(|m| m.handle == *handle);
        let ids = s.by_author(handle);
        let acts = div("profile-acts")
            .id("profile-acts")
            .when(!mine, |n| {
                n.child(pill(
                    "follow",
                    "none",
                    if following { "Following" } else { "Follow" },
                    None,
                    following,
                    &format!("/accounts/{handle}/follow"),
                    &self.here,
                ))
            })
            .when(!mine && s.professional(), |n| {
                n.child(pill(
                    "connect",
                    "none",
                    if connected { "Invitation sent" } else { "Connect" },
                    None,
                    connected,
                    &format!("/accounts/{handle}/connect"),
                    &self.here,
                ))
            });
        let card = el("section")
            .id("profile")
            .class("profile")
            .child(div("banner").style(&format!("background-color: {}", tint(handle))))
            .child(
                div("profile-row")
                    .id("profile-row")
                    .child(avatar("profile-avatar", Some(acct), handle, "huge"))
                    .child(acts),
            )
            .child(
                div("profile-body")
                    .id("profile-body")
                    .child(
                        div("profile-title")
                            .child(el("h1").id("name").class("profile-name").text(acct.name.as_str()))
                            .when(acct.verified, |n| n.child(span("verified").id("verified").attr("title", "Verified").text("✓"))),
                    )
                    .child(el("p").id("handle").class("profile-handle").text(display(s, handle)))
                    .when(!acct.headline.is_empty(), |n| n.child(el("p").id("headline").class("headline").text(acct.headline.as_str())))
                    .child(el("p").id("bio").class("bio").text(acct.bio.as_str()))
                    .child(
                        div("meta")
                            .id("meta")
                            .when(!acct.location.is_empty(), |n| {
                                n.child(span("meta-item").child(ico("place")).child(span("").id("location").text(acct.location.as_str())))
                            })
                            .when(!acct.site.is_empty(), |n| {
                                n.child(span("meta-item").child(ico("link")).child(link("site", acct.site.as_str(), acct.site.as_str())))
                            }),
                    )
                    .child(
                        div("counts")
                            .child(span("count").id("post-count").child(el("b").text(ids.len().to_string())).text(" posts"))
                            .child(span("count").id("following-count").child(el("b").text(acct.following.to_string())).text(" following"))
                            .child(span("count").id("followers").child(el("b").text(acct.followers.to_string())).text(" followers")),
                    ),
            );
        let mut main = vec![card];
        if !acct.experience.is_empty() {
            main.push(
                el("section")
                    .id("experience")
                    .class("experience")
                    .child(el("h2").id("exp-title").class("section-title").text("Experience"))
                    .each(acct.experience.iter().enumerate(), |(i, r)| {
                        div("exp")
                            .id(format!("exp-{i}"))
                            .child(
                                span("exp-logo")
                                    .id(format!("exp-{i}-logo"))
                                    .style(&format!("background-color: {}", tint(&r.company)))
                                    .text(initials(&r.company)),
                            )
                            .child(
                                div("exp-body")
                                    .id(format!("exp-{i}-body"))
                                    .child(el("p").id(format!("exp-{i}-title")).class("exp-title").text(format!("{} · {}", r.title, r.company)))
                                    .child(el("p").id(format!("exp-{i}-period")).class("exp-period").text(r.period.as_str())),
                            )
                    }),
            );
        }
        main.extend(self.feed("Posts", &ids));
        self.document(&format!("{} ({}) / {}", acct.name, display(s, handle), s.brand), main)
    }
    pub(crate) fn thread(&self, handle: &str, id: &str) -> SimResult<HttpResponse> {
        let s = self.s;
        let Some(post) = s.posts.get(id).filter(|p| p.author == handle) else {
            return web::error(404, "no such post");
        };
        // Walk up to the conversation root so a reply is never read out of context.
        let mut chain = vec![];
        let mut cursor = post.reply_to.clone();
        while let Some(parent) = cursor {
            let Some(up) = s.posts.get(&parent) else {
                break;
            };
            chain.push(up.id.clone());
            cursor = up.reply_to.clone();
        }
        chain.reverse();
        let mut main = vec![div("crumb").child(link("thread-back", "/", "←").attr("aria-label", "Back to Home")).child(el("h2").class("crumb-title").text("Post"))];
        main.extend(chain.iter().map(|up| self.post(up).class("ancestor")));
        main.push(self.post_at(id, true).class("focus"));
        main.push(
            el("section")
                .id("reply-box")
                .class("compose reply-box")
                .maybe(self.me().map(|me| avatar("reply-avatar", Some(me), &me.handle, "mid")))
                .child(
                    form("reply", format!("/posts/{id}/replies"), "post")
                        .child(
                            text_input("reply-text", "text", "")
                                .attr("aria-label", "Reply")
                                .attr("placeholder", if self.skin == "x" { "Post your reply" } else { "Write a reply" })
                                .attr("autocomplete", "off"),
                        )
                        .child(div("compose-bar").child(button("reply-submit", "Reply").class("primary"))),
                ),
        );
        let replies = s.replies_to(id);
        let count = replies.len();
        main.push(el("h2").id("replies-title").class("section-title").text(format!(
            "{count} {}",
            if count == 1 { "reply" } else { "replies" }
        )));
        main.extend(replies.iter().map(|r| self.post(r)));
        self.document(
            &format!("{} on {}", s.accounts.get(handle).map_or(handle, |a| a.name.as_str()), s.brand),
            main,
        )
    }
    pub(crate) fn search(&self, q: &str) -> SimResult<HttpResponse> {
        let ids = self.s.search(q);
        let title = if q.trim().is_empty() {
            "Search".to_owned()
        } else {
            format!("{} for \"{q}\"", if ids.len() == 1 { "1 result".to_owned() } else { format!("{} results", ids.len()) })
        };
        let mut main = vec![form("search", "/search", "post")
            .class("search-form")
            .attr("role", "search")
            .child(ico("search"))
            .child(text_input("search-q", "q", q).attr("aria-label", "Search").attr("placeholder", "Search").attr("autocomplete", "off"))
            .child(button("search-submit", "Search").class("primary"))];
        main.extend(self.feed(&title, &ids));
        self.document(&format!("{} / Search", self.s.brand), main)
    }
    pub(crate) fn inbox(&self, root: &str, open: Option<&str>) -> SimResult<HttpResponse> {
        let s = self.s;
        let threads = s.inbox(&self.ctx.actor);
        let mut list = div("threads")
            .id("threads")
            .child(el("h2").id("inbox-title").class("feed-title").text("Messages"));
        if threads.is_empty() {
            list = list.child(el("p").id("inbox-empty").class("empty").text("No conversations."));
        }
        let mine = self.me().map(|m| m.handle.clone());
        list = list.each(&threads, |c| {
            // The face of a thread is whoever else is in it.
            let other = c.members.iter().find(|m| Some(*m) != mine.as_ref()).or_else(|| c.members.iter().next());
            let face = other.map_or("", String::as_str);
            a(format!("{root}/{}", c.id))
                .id(format!("thread-{}", c.id))
                .class(if open == Some(c.id.as_str()) { "thread on" } else { "thread" })
                .when(open == Some(c.id.as_str()), |n| n.attr("aria-current", "page"))
                .child(avatar(&format!("thread-{}-avatar", c.id), s.accounts.get(face), face, "mid"))
                .child(
                    span("thread-body")
                        .id(format!("thread-{}-body", c.id))
                        .child(span("thread-title").id(format!("thread-{}-title", c.id)).text(c.title.as_str()))
                        .child(
                            span("thread-last")
                                .id(format!("thread-{}-last", c.id))
                                .text(c.messages.last().map_or(String::new(), |m| format!("{}: {}", display(s, &m.from), m.text))),
                        ),
                )
        });
        let mut panes = vec![list];
        if let Some(id) = open {
            let c = match s.conversation(&self.ctx.actor, id) {
                Ok(c) => c,
                Err(e) => return web::error(403, e),
            };
            panes.push(
                el("section")
                    .id("convo")
                    .class("convo")
                    .child(el("h2").id("open-title").class("convo-title").text(c.title.as_str()))
                    .child(div("bubbles").each(&c.messages, |m| {
                        let own = mine.as_deref() == Some(m.from.as_str());
                        div(if own { "dm own" } else { "dm" })
                            .id(format!("dm-{}", m.id))
                            .child(span("dm-from").id(format!("dm-{}-from", m.id)).text(format!("{} · {}", display(s, &m.from), ago(self.ctx.tick, m.tick))))
                            .child(span("dm-text").id(format!("dm-{}-text", m.id)).text(m.text.as_str()))
                    }))
                    .child(
                        form("dm", format!("{root}/{id}/messages"), "post")
                            .class("dm-form")
                            .child(text_input("dm-text", "text", "").attr("aria-label", "Message").attr("placeholder", "Start a new message").attr("autocomplete", "off"))
                            .child(button("dm-submit", "Send").class("primary")),
                    ),
            );
        }
        self.document(&format!("{} / Messages", s.brand), vec![div("inbox").children(panes)])
    }
}
