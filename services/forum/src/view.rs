//! The discussion sites as HTML. Six skins over three modes: `hackernews` and
//! `craigslist` are link feeds, `reddit` and `yelp` are boards with a subscribe rail,
//! `stackoverflow` and `quora` are Q&A. The routes, forms and ids are the same in every
//! skin; the skin decides the chrome around them and the furniture of a list row, and
//! each has its own stylesheet next to this file.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `nav-home`,
//! `nav-new`, `nav-search`, `nav-submit`, `nav-boards`, `nav-me`; `row-<thread>` with
//! `<thread>-title`, `-out`, `-host`, `-rank`, `-by`, `-comments`, `-open`, `-board`,
//! `-count`, `-tag-<n>`, `-st-votes|answers|views`; the vote controls `<target>-up`,
//! `<target>-down` (submit buttons of one-button forms) and `<target>-score`; the forms
//! `search`, `submit`, `compose`, `<reply>-reply`, `<reply>-comment` with inputs
//! `<form>-<field>` and buttons `<form>-submit`; `<reply>-accept`; `board-<id>-link`,
//! `board-<id>-sub`, `board-head-sub`; `thread-*`, `member-*`, `list-title`.
use crate::{ForumState, Reply, Thread};
use cw_protocol::{HttpResponse, Result};
use cw_sdk::ServiceContext;
use cw_service_common::html::{
    self, button, div, el, empty, form, fragment, hidden, href, link, span, text_input, Document, Html as Node,
};

/// The skins, in the order `skin` documents them; an absent `skin` follows the brand and then the mode.
pub const SKINS: &[&str] = &["hackernews", "reddit", "stackoverflow", "quora", "yelp", "craigslist"];
/// Threads per list page; longer lists get `page-prev`, `page-<n>`, `page-next`.
pub const PAGE_SIZE: usize = 30;

const TINTS: [&str; 6] = ["#ff4500", "#0079d3", "#46a35e", "#7856ff", "#f48024", "#d93a49"];
/// Avatar colour is a pure FNV-1a over the handle: stable across runs, machines and snapshots,
/// and no seeded stream is reachable from a render.
fn tint(handle: &str) -> &'static str {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in handle.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    TINTS[(h % TINTS.len() as u64) as usize]
}
fn initials(name: &str) -> String {
    let letters: String = name.split_whitespace().filter_map(|w| w.chars().next()).filter(|c| c.is_alphanumeric()).take(2).collect();
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
        n => format!("{n}t ago"),
    }
}
/// The host of an outbound link, which is the grey suffix on a link-feed title.
fn host(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("").trim_start_matches("www.").to_owned()
}
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        return format!("{n} {word}");
    }
    let stem = word.strip_suffix('y').filter(|s| !s.ends_with(['a', 'e', 'i', 'o', 'u']));
    match stem {
        Some(stem) => format!("{n} {stem}ies"),
        None if word.ends_with('s') => format!("{n} {word}es"),
        None => format!("{n} {word}s"),
    }
}
/// The bare word a stats column puts under a number: `1 answer`, `2 answers`.
fn word(n: u64, w: &str) -> &str {
    if n == 1 {
        w.strip_suffix('s').unwrap_or(w)
    } else {
        w
    }
}
/// `38200` as `38.2k`, the way a stats column abbreviates.
fn compact(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}.{}k", n / 1_000, (n % 1_000) / 100),
        _ => format!("{}.{}m", n / 1_000_000, (n % 1_000_000) / 100_000),
    }
}
fn capital(word: &str) -> String {
    let mut c = word.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
/// A path segment, percent-encoded: a Craigslist category is `for sale`.
fn segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
/// Byte ranges of the http(s) words in `body` with their whitespace-split index, which is
/// the `<prefix>-link-<n>` id the link has always had.
fn url_spans(body: &str) -> Vec<(usize, usize, usize)> {
    body.split_whitespace()
        .enumerate()
        .filter_map(|(i, word)| {
            let url = word.trim_end_matches(['.', ',', ';', ')', ']']);
            let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?;
            if rest.is_empty() {
                return None;
            }
            let start = word.as_ptr() as usize - body.as_ptr() as usize;
            Some((start, start + url.len(), i))
        })
        .collect()
}
/// A Yelp review opens with its rating as `****. 4/5. `; the stars and the text after them.
fn review_stars(body: &str) -> (Option<u32>, &str) {
    let head: &str = body.split(' ').next().unwrap_or("");
    if head.len() == 5 && head.bytes().all(|b| b == b'*' || b == b'.') {
        let rest = body[5..].trim_start();
        let rest = match rest.split_once(". ") {
            Some((score, tail)) if score.len() == 3 && score.as_bytes()[1] == b'/' => tail,
            _ => rest,
        };
        (Some(head.bytes().filter(|b| *b == b'*').count() as u32), rest)
    } else {
        (None, body)
    }
}
/// What a Yelp business body carries in its header lines.
#[derive(Default)]
struct Business {
    address: String,
    hours: String,
    price: String,
    /// Rating in tenths of a star.
    rating: Option<u32>,
    about: String,
}
fn business(t: &Thread) -> Business {
    let mut b = Business::default();
    let (head, about) = t.body.split_once("\n\n").unwrap_or(("", t.body.as_str()));
    b.about = about.to_owned();
    for (i, line) in head.lines().enumerate() {
        if let Some(h) = line.strip_prefix("Hours:") {
            b.hours = h.trim().to_owned();
        } else if let Some(p) = line.strip_prefix("Price:") {
            let mut parts = p.split("Rating:");
            b.price = parts.next().unwrap_or("").trim().to_owned();
            if let Some(r) = parts.next() {
                let number = r.trim().split(' ').next().unwrap_or("");
                let (whole, frac) = number.split_once('.').unwrap_or((number, "0"));
                if let (Ok(w), Ok(f)) = (whole.parse::<u32>(), frac[..frac.len().min(1)].parse::<u32>()) {
                    b.rating = Some((w * 10 + f).min(50));
                }
            }
        } else if i == 1 {
            b.address = line.trim().to_owned();
        }
    }
    if b.rating.is_none() {
        let stars: Vec<u32> = t.replies.iter().filter_map(|r| review_stars(&r.body).0).collect();
        if !stars.is_empty() {
            b.rating = Some(stars.iter().sum::<u32>() * 10 / stars.len() as u32);
        }
    }
    b
}
/// Five squares, filled to the rating: Yelp's stars, drawn by the stylesheet.
fn stars(id: &str, tenths: u32) -> Node {
    let label = format!("{}.{} star rating", tenths / 10, tenths % 10);
    span("stars").id(id).attr("role", "img").attr("aria-label", label).each(0..5u32, |i| {
        let fill = tenths.saturating_sub(i * 10);
        el("i").class(if fill >= 8 {
            "full"
        } else if fill >= 3 {
            "half"
        } else {
            "none"
        })
    })
}
/// A Craigslist title is `what - $price (neighbourhood)`.
pub(crate) fn listing_parts(title: &str) -> (&str, &str, &str) {
    let (rest, hood) = match title.rfind(" (") {
        Some(i) if title.ends_with(')') => (&title[..i], &title[i + 2..title.len() - 1]),
        _ => (title, ""),
    };
    match rest.rfind(" - $") {
        Some(i) => (&rest[..i], &rest[i + 3..], hood),
        None => (rest.trim_end_matches([' ', '-']), "", hood),
    }
}

/// One render: the state, who is looking, the skin, and the page the controls come back to.
pub(crate) struct View<'a> {
    s: &'a ForumState,
    ctx: &'a ServiceContext,
    skin: &'static str,
    /// The page without its list-page number, which is what the pager counts from.
    base: String,
    here: String,
    query: String,
}
impl<'a> View<'a> {
    pub(crate) fn new(s: &'a ForumState, ctx: &'a ServiceContext, base: &str, here: &str) -> Self {
        Self { s, ctx, skin: s.skin(), base: base.to_owned(), here: here.to_owned(), query: String::new() }
    }
    fn is(&self, skin: &str) -> bool {
        self.skin == skin
    }
    fn who(&self, handle: &str) -> String {
        if self.is("reddit") {
            format!("u/{handle}")
        } else {
            self.s.members.get(handle).map_or(handle.to_owned(), |m| m.name.clone())
        }
    }
    fn board_name(&self, board: &str) -> String {
        if self.is("reddit") {
            format!("r/{board}")
        } else {
            self.s.boards.get(board).map_or(board.to_owned(), |b| b.title.clone())
        }
    }
    fn avatar(&self, id: &str, handle: &str) -> Node {
        let name = self.s.members.get(handle).map_or(handle, |m| m.name.as_str());
        span("avatar").id(id).attr("aria-hidden", "true").style(&format!("background-color: {}", tint(handle))).text(initials(name))
    }
    fn author(&self, id: &str, handle: &str) -> Node {
        link(id, format!("/u/{handle}"), self.who(handle)).class("user")
    }

    // ------------------------------------------------------------------ the shell
    fn document(&self, title: &str, page: &str, main: Vec<Node>, side: Vec<Node>) -> Document {
        let skin_css = match self.skin {
            "hackernews" => include_str!("hackernews.css"),
            "reddit" => include_str!("reddit.css"),
            "stackoverflow" => include_str!("stackoverflow.css"),
            "quora" => include_str!("quora.css"),
            "yelp" => include_str!("yelp.css"),
            _ => include_str!("craigslist.css"),
        };
        let theme = &self.s.theme;
        let mut vars = Vec::new();
        for (name, value) in [
            ("--accent", &theme.accent),
            ("--ink", &theme.ink),
            ("--muted", &theme.muted),
            ("--surface", &theme.surface),
            ("--paper", &theme.background),
        ] {
            if let Some(v) = value {
                vars.push(format!("{name}: {v}"));
            }
        }
        if let Some(w) = theme.content_width {
            vars.push(format!("--content: {}px", w.clamp(320, 1400)));
        }
        let main = el("main").id("content").class("content").children(main);
        let side = if side.is_empty() { empty() } else { el("aside").id("sidebar").class("sidebar").children(side) };
        let body = match self.skin {
            "hackernews" => vec![div("hnmain").child(self.hn_top()).child(main).child(self.hn_foot())],
            "reddit" => vec![self.reddit_top(), div("wrap").child(main).child(side), self.plain_foot()],
            "stackoverflow" => vec![
                self.so_top(),
                div("wrap").child(self.so_left()).child(div("mainbar").child(main).child(side)),
                self.plain_foot(),
            ],
            "quora" => vec![self.quora_top(), div("wrap").child(self.quora_left()).child(main).child(side), self.plain_foot()],
            "yelp" => vec![self.yelp_top(), div("wrap").child(main).child(side), self.plain_foot()],
            _ => vec![div("wrap").child(self.cl_left()).child(main).child(side), self.plain_foot()],
        };
        let mut doc = Document::new(title).lang("en").stylesheet(include_str!("base.css")).stylesheet(skin_css).body_class(&format!("skin-{} page-{page}", self.skin)).body(body);
        if !vars.is_empty() {
            doc = doc.root_style(&vars.join("; "));
        }
        doc
    }
    fn submit_label(&self) -> &'static str {
        match self.skin {
            "hackernews" => "submit",
            "reddit" => "Create Post",
            "stackoverflow" => "Ask Question",
            "quora" => "Add question",
            "yelp" => "Add a Business",
            _ => "create a posting",
        }
    }
    fn nav_link(&self, id: &str, url: &str, label: &str) -> Node {
        link(id, url, label).class(if url == self.here { "on" } else { "" })
    }
    /// The search box every skin carries once per page. A POST that reads, as it always was.
    fn search_form(&self, placeholder: &str, go: &str) -> Node {
        form("search", "/search", "post")
            .class("search")
            .attr("role", "search")
            .child(span("mag").attr("aria-hidden", "true"))
            .child(text_input("search-q", "q", &self.query).attr("aria-label", "Search").attr("placeholder", placeholder).attr("autocomplete", "off"))
            .child(button("search-submit", go).class("go"))
    }
    fn me(&self) -> Option<Node> {
        let m = self.s.member_of(&self.ctx.actor)?;
        Some(
            el("a")
                .id("nav-me")
                .class(if format!("/u/{}", m.handle) == self.here { "me on" } else { "me" })
                .attr("href", format!("/u/{}", m.handle))
                .attr("aria-label", "Profile")
                .child(self.avatar("nav-me-avatar", &m.handle))
                .child(span("me-name").id("nav-me-text").text(match self.skin {
                    "reddit" => format!("u/{}", m.handle),
                    "hackernews" | "craigslist" => m.handle.clone(),
                    _ => m.name.clone(),
                }))
                .child(span("me-rep").id("nav-me-rep").text(compact(m.reputation))),
        )
    }
    fn plain_foot(&self) -> Node {
        el("footer").id("footer").class("foot").child(span("foot-brand").text(self.s.brand.as_str())).child(span("foot-note").text(match self.skin {
            "reddit" => "Reddit, Inc. © 2026. All rights reserved.",
            "stackoverflow" => "Site design / logo © 2026 Stack Exchange Inc; user contributions licensed under CC BY-SA.",
            "quora" => "About · Careers · Terms · Privacy · Acceptable Use",
            "yelp" => "Copyright © 2004–2026 Yelp Inc. Yelp, the Yelp logo and related marks are registered trademarks of Yelp.",
            _ => "© 2026 craigslist · help · safety · privacy · terms · about",
        }))
    }

    fn hn_top(&self) -> Node {
        el("header")
            .id("masthead")
            .class("pagetop")
            .child(el("a").class("ylogo").attr("href", "/").attr("aria-label", "Home").attr("tabindex", "-1").text("Y"))
            .child(
                el("nav")
                    .id("nav")
                    .class("links")
                    .child(el("b").id("brand").class("hnname").child(self.nav_link("nav-home", "/", &self.s.brand)))
                    .child(self.nav_link("nav-new", "/newest", "new"))
                    .child(span("sep").text("|"))
                    .child(link("nav-ask", href("/search", &[("q", "Ask HN")]), "ask"))
                    .child(span("sep").text("|"))
                    .child(link("nav-show", href("/search", &[("q", "Show HN")]), "show"))
                    .child(span("sep").text("|"))
                    .child(self.nav_link("nav-search", "/search", "search"))
                    .child(span("sep").text("|"))
                    .child(self.nav_link("nav-submit", "/submit", self.submit_label())),
            )
            .maybe(self.me())
    }
    fn hn_foot(&self) -> Node {
        el("footer")
            .id("footer")
            .class("foot")
            .child(div("rule"))
            .child(el("p").id("tagline").class("yclinks").text(self.s.tagline.as_str()))
            .child(self.search_form("", "Search"))
    }
    fn reddit_top(&self) -> Node {
        el("header")
            .id("masthead")
            .class("top")
            .child(
                el("a").class("logo").attr("href", "/").attr("aria-label", "Front page").attr("tabindex", "-1")
                    .child(span("snoo").child(el("i")).child(el("b")))
                    .child(span("word").id("brand").text(self.s.brand.as_str())),
            )
            .child(self.search_form(&format!("Search {}", capital(&self.s.brand)), "Search"))
            .child(
                el("nav")
                    .id("nav")
                    .class("links")
                    .child(self.nav_link("nav-home", "/", "Home"))
                    .child(self.nav_link("nav-boards", "/r", "Communities"))
                    .child(self.nav_link("nav-new", "/newest", "New"))
                    .child(self.nav_link("nav-search", "/search", "Search"))
                    .child(self.nav_link("nav-submit", "/submit", self.submit_label()).class("create")),
            )
            .maybe(self.me())
    }
    fn so_top(&self) -> Node {
        el("header").id("masthead").class("top").child(
            div("bar")
                .child(
                    el("a").class("logo").attr("href", "/").attr("aria-label", "Home").attr("tabindex", "-1")
                        .child(span("glyph").child(el("i")).child(el("i")).child(el("i")).child(el("b")))
                        .child(span("word").id("brand").text(self.s.brand.as_str())),
                )
                .child(span("products").id("tagline").attr("title", self.s.tagline.as_str()).text("Products"))
                .child(self.search_form("Search…", "Search"))
                .maybe(self.me()),
        )
    }
    fn so_left(&self) -> Node {
        el("nav")
            .id("nav")
            .class("leftnav")
            .child(self.nav_link("nav-home", "/", "Home"))
            .child(span("group").text("PUBLIC"))
            .child(self.nav_link("nav-questions", "/questions", "Questions").class("sub globe"))
            .child(self.nav_link("nav-new", "/newest", "Newest").class("sub"))
            .child(self.nav_link("nav-search", "/search", "Search").class("sub"))
            .child(self.nav_link("nav-submit", "/submit", self.submit_label()).class("sub"))
            .child(span("group").text("COLLECTIVES"))
            .child(span("note").text("Explore Collectives"))
            .child(span("group").text("TEAMS"))
            .child(span("note").text("Ask questions, find answers and collaborate at work."))
    }
    fn quora_top(&self) -> Node {
        el("header").id("masthead").class("top").child(
            div("bar")
                .child(el("a").class("logo").id("brand").attr("href", "/").attr("tabindex", "-1").text(self.s.brand.as_str()))
                .child(
                    el("nav")
                        .id("nav")
                        .class("links")
                        .child(self.nav_link("nav-home", "/", "Home"))
                        .child(self.nav_link("nav-new", "/newest", "Following"))
                        .child(self.nav_link("nav-questions", "/questions", "Answer"))
                        .child(self.nav_link("nav-search", "/search", "Search")),
                )
                .child(self.search_form(&format!("Search {}", self.s.brand), "Search"))
                .maybe(self.me())
                .child(self.nav_link("nav-submit", "/submit", self.submit_label()).class("add")),
        )
    }
    /// Tags by how many threads carry them, most used first.
    fn tag_counts(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for t in self.s.threads.values() {
            for tag in &t.tags {
                *counts.entry(tag.clone()).or_default() += 1;
            }
        }
        let mut tags: Vec<(String, usize)> = counts.into_iter().collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        tags
    }
    fn tag_url(tag: &str) -> String {
        format!("/questions/tagged/{}", segment(tag))
    }
    fn quora_left(&self) -> Node {
        el("nav").id("topics").class("leftnav").attr("aria-label", "Topics").each(self.tag_counts().into_iter().take(12).enumerate(), |(i, (tag, _))| {
            el("a")
                .id(format!("topic-{i}"))
                .attr("href", Self::tag_url(&tag))
                .class(if Self::tag_url(&tag) == self.here { "on" } else { "" })
                .child(span("sq").style(&format!("background-color: {}", tint(&tag))))
                .child(span("name").text(capital(&tag.replace('-', " "))))
        })
    }
    fn yelp_top(&self) -> Node {
        el("header")
            .id("masthead")
            .class("top")
            .child(
                div("bar")
                    .child(
                        el("a").class("logo").attr("href", "/").attr("aria-label", "Home").attr("tabindex", "-1")
                            .child(span("word").id("brand").text(self.s.brand.to_lowercase()))
                            .child(span("burst").each(0..5, |_| el("i"))),
                    )
                    .child(self.search_form("tacos, cheap dinner, Max's", "Search"))
                    .child(self.nav_link("nav-submit", "/submit", self.submit_label()).class("biz"))
                    .maybe(self.me()),
            )
            .child(
                el("nav")
                    .id("nav")
                    .class("cats")
                    .child(self.nav_link("nav-home", "/", "Home"))
                    .each(self.s.boards.values(), |b| self.nav_link(&format!("cat-{}", b.id), &format!("/r/{}", b.id), &b.title))
                    .child(self.nav_link("nav-boards", "/r", "All Categories"))
                    .child(self.nav_link("nav-new", "/newest", "New"))
                    .child(self.nav_link("nav-search", "/search", "Search")),
            )
            .child(el("p").id("tagline").class("tagline").text(self.s.tagline.as_str()))
    }
    fn cl_left(&self) -> Node {
        el("nav")
            .id("nav")
            .class("leftbar")
            .child(el("a").id("nav-home").class("logo").attr("href", "/").child(span("").id("brand").text(self.s.brand.as_str())))
            .child(self.nav_link("nav-submit", "/submit", self.submit_label()).class("post"))
            .maybe(self.s.member_of(&self.ctx.actor).map(|m| link("nav-me", format!("/u/{}", m.handle), "my account").class("acct")))
            .child(self.search_form(&format!("search {}", self.s.brand), "search"))
            .child(
                div("cal")
                    .child(span("cal-title").text("event calendar"))
                    .child(div("days").each(["M", "T", "W", "T", "F", "S", "S"], |d| el("b").text(d)).each(1..=28, |d| el("i").text(d.to_string()))),
            )
            .child(self.nav_link("nav-new", "/newest", "newest listings"))
            .child(self.nav_link("nav-search", "/search", "search listings"))
            .child(span("small").text("help, faq, abuse, legal"))
            .child(span("small").text("avoid scams & fraud"))
            .child(span("small").text("personal safety tips"))
    }

    // ------------------------------------------------------------------ controls
    /// A one-button form that mutates and comes back to this page.
    fn post_button(&self, id: &str, url: &str, extra: &[(&str, &str)], label: &str, on: bool, class: &str) -> Node {
        form(&format!("{id}-form"), url, "post")
            .class(class)
            .child(hidden("view", &self.here))
            .each(extra.iter(), |(k, v)| hidden(k, v))
            .child(button(id, "").class(if on { "on" } else { "" }).attr("aria-label", label).attr("title", label).child(span("label").id(format!("{id}-label")).text(label)))
    }
    /// The up/score/down column. A link feed has no downvote, so it gets one arrow and no empty
    /// slot pretending otherwise; its score sits in the subtext line instead of the column.
    fn votes(&self, id: &str, kind: &str) -> Node {
        let mine = self.s.my_vote(&self.ctx.actor, id);
        let url = format!("/{kind}/{id}/vote");
        div("votes")
            .id(format!("{id}-votes"))
            .child(self.post_button(&format!("{id}-up"), &url, &[("dir", "1")], "Up", mine > 0, "vote up"))
            .when(!self.s.linkfeed(), |v| {
                v.child(span(if mine != 0 { "score mine" } else { "score" }).id(format!("{id}-score")).text(self.s.score_of(id).to_string()))
                    .child(self.post_button(&format!("{id}-down"), &url, &[("dir", "-1")], "Down", mine < 0, "vote down"))
            })
    }
    /// A titled form: visible labels, `<form>-<field>` inputs named by the field, `<form>-submit`.
    /// `long` fields are textareas.
    fn fields_form(&self, id: &str, url: &str, fields: &[(&str, &str, bool)], fixed: &[(&str, &str)], go: &str) -> Node {
        form(id, url, "post")
            .class("fields")
            .each(fixed.iter(), |(k, v)| hidden(k, v))
            .each(fields.iter(), |(key, label, long)| {
                let fid = format!("{id}-{key}");
                let control = if *long {
                    el("textarea").id(fid.as_str()).attr("name", *key).attr("rows", "6")
                } else {
                    text_input(&fid, key, "")
                };
                div("field").child(html::label(&fid, *label)).child(control)
            })
            .child(button(&format!("{id}-submit"), go).class("primary"))
    }
    /// A one-line form hanging off a reply.
    fn inline_form(&self, id: &str, url: &str, label: &str, fixed: &[(&str, &str)], go: &str) -> Node {
        form(id, url, "post")
            .class("inline")
            .each(fixed.iter(), |(k, v)| hidden(k, v))
            .child(text_input(&format!("{id}-body"), "body", "").attr("aria-label", label).attr("placeholder", label).attr("autocomplete", "off"))
            .child(button(&format!("{id}-submit"), go))
    }
    /// Body text: paragraphs, line breaks, code (Q&A), and http words as the inline links
    /// `<prefix>-link-<n>`, where `n` is the word's index in the body.
    fn prose(&self, id: &str, prefix: &str, body: &str, skip: usize) -> Node {
        let spans = url_spans(body);
        let base = body.as_ptr() as usize;
        let at = |part: &str| part.as_ptr() as usize - base;
        let code = self.s.qa();
        let run = |node: Node, part: &str| -> Node {
            let (start, end) = (at(part), at(part) + part.len());
            let mut node = node;
            let mut cursor = start;
            for (a, b, i) in spans.iter().filter(|(a, b, _)| *a >= start && *b <= end) {
                node = node.text(&body[cursor..*a]).child(link(&format!("{prefix}-link-{i}"), &body[*a..*b], &body[*a..*b]));
                cursor = *b;
            }
            node.text(&body[cursor..end])
        };
        let mut out = div("text").id(id);
        for para in body[skip..].split("\n\n").filter(|p| !p.trim().is_empty()) {
            let lines: Vec<&str> = para.lines().collect();
            let looks_like_code = code
                && (lines.iter().any(|l| l.starts_with("    ") || l.starts_with('\t'))
                    || lines.iter().filter(|l| l.trim_end().ends_with(['{', '}', ';'])).count() * 2 >= lines.len().max(1)
                    || (lines.len() == 1 && (para.starts_with("error") || para.starts_with("$ "))));
            if looks_like_code {
                out = out.child(el("pre").child(el("code").text(para)));
                continue;
            }
            let mut p = el("p");
            for (n, line) in lines.iter().enumerate() {
                if n > 0 {
                    p = p.child(el("br"));
                }
                if code && line.matches('`').count() >= 2 && line.matches('`').count() % 2 == 0 {
                    for (k, part) in line.split('`').enumerate() {
                        p = if k % 2 == 1 { p.child(el("code").text(part)) } else { run(p, part) };
                    }
                } else {
                    p = run(p, line);
                }
            }
            out = out.child(p);
        }
        out
    }
    fn tags(&self, id: &str, item: &str, tags: &[String]) -> Node {
        if tags.is_empty() {
            return empty();
        }
        div("tags").id(id).each(tags.iter().enumerate(), |(i, tag)| link(&format!("{item}-{i}"), Self::tag_url(tag), tag.as_str()).class("tag"))
    }

    // ------------------------------------------------------------------ list rows
    fn entry(&self, id: &str, rank: usize) -> Node {
        match self.skin {
            "hackernews" => self.entry_hn(id, rank),
            "craigslist" => self.entry_cl(id, rank),
            "stackoverflow" => self.entry_so(id),
            "quora" => self.entry_quora(id),
            "yelp" => self.entry_yelp(id, rank),
            _ => self.entry_reddit(id),
        }
    }
    /// The title of a link-feed row: the outbound link when there is one, else the item page.
    fn feed_title(&self, t: &Thread, label: &str) -> Node {
        match t.url.as_deref() {
            Some(u) => fragment([
                link(&format!("{}-out", t.id), u, label).class("titlelink"),
                span("host").id(format!("{}-host", t.id)).text(format!("({})", host(u))),
            ]),
            None => link(&format!("{}-title", t.id), self.s.permalink(t), label).class("titlelink"),
        }
    }
    fn entry_hn(&self, id: &str, rank: usize) -> Node {
        let t = &self.s.threads[id];
        let mine = self.s.my_vote(&self.ctx.actor, id) != 0;
        fragment([
            el("tr")
                .id(format!("row-{id}"))
                .class("athing")
                .child(el("td").class("rank").id(format!("{id}-rank")).text(format!("{rank}.")))
                .child(el("td").class("votelinks").child(self.votes(id, "threads")))
                .child(el("td").class("title").child(self.feed_title(t, &t.title))),
            el("tr").class("sub").child(el("td").attr("colspan", "2")).child(
                el("td")
                    .class("subtext")
                    .child(
                        span("by")
                            .id(format!("{id}-by"))
                            .child(span(if mine { "score mine" } else { "score" }).id(format!("{id}-score")).text(self.s.score_of(id).to_string()))
                            .text(" points by ")
                            .child(self.author(&format!("{id}-author"), &t.author))
                            .text(format!(" {}", ago(self.ctx.tick, t.tick))),
                    )
                    .child(span("sep").text(" | "))
                    .child(link(&format!("{id}-comments"), self.s.permalink(t), plural(self.s.conversation_size(t), "comment"))),
            ),
            el("tr").class("spacer").child(el("td").attr("colspan", "3")),
        ])
    }
    fn entry_cl(&self, id: &str, rank: usize) -> Node {
        let t = &self.s.threads[id];
        let (what, price, hood) = listing_parts(&t.title);
        let mine = self.s.my_vote(&self.ctx.actor, id) != 0;
        el("li")
            .id(format!("row-{id}"))
            .class("result")
            .child(self.votes(id, "threads"))
            .child(span("rank").id(format!("{id}-rank")).text(format!("{rank}.")))
            .child(span("date").id(format!("{id}-date")).text(ago(self.ctx.tick, t.tick)))
            .child(self.feed_title(t, what))
            .when(!price.is_empty(), |n| n.child(span("price").id(format!("{id}-price")).text(price)))
            .when(!hood.is_empty(), |n| n.child(span("hood").id(format!("{id}-hood")).text(format!("({hood})"))))
            .child(
                span("by")
                    .id(format!("{id}-by"))
                    .child(span(if mine { "score mine" } else { "score" }).id(format!("{id}-score")).text(self.s.score_of(id).to_string()))
                    .text(" points by ")
                    .child(self.author(&format!("{id}-author"), &t.author)),
            )
            .child(link(&format!("{id}-comments"), self.s.permalink(t), plural(self.s.conversation_size(t), self.s.reply_word())).class("replies"))
    }
    fn stat(&self, id: String, n: String, label: &str, class: &str) -> Node {
        div(&format!("stat {class}")).id(id.as_str()).child(span("n").id(format!("{id}-n")).text(n)).child(span("l").id(format!("{id}-l")).text(label))
    }
    fn entry_so(&self, id: &str) -> Node {
        let t = &self.s.threads[id];
        let answers = match (t.replies.is_empty(), t.accepted.is_some()) {
            (true, _) => "s-answers",
            (false, true) => "s-answers has accepted",
            (false, false) => "s-answers has",
        };
        div("qrow")
            .id(format!("{id}-row"))
            .child(
                div("stats")
                    .id(format!("{id}-stats"))
                    .child(self.stat(format!("{id}-st-votes"), self.s.score_of(id).to_string(), word(self.s.score_of(id).unsigned_abs(), "votes"), "s-votes"))
                    .child(self.stat(format!("{id}-st-answers"), t.replies.len().to_string(), word(t.replies.len() as u64, "answers"), answers))
                    .child(self.stat(format!("{id}-st-views"), compact(t.views), word(t.views, "views"), "s-views")),
            )
            .child(
                div("summary")
                    .id(format!("{id}-body"))
                    .child(div("h3").child(el("a").id(format!("row-{id}")).attr("href", self.s.permalink(t)).child(span("").id(format!("{id}-title")).text(t.title.as_str()))))
                    .child(el("p").class("excerpt").id(format!("{id}-excerpt")).text(t.body.chars().take(200).collect::<String>().replace('\n', " ")))
                    .child(
                        div("meta")
                            .child(self.tags(&format!("{id}-tags"), &format!("{id}-tag"), &t.tags))
                            .child(
                                span("usercard")
                                    .id(format!("{id}-asked"))
                                    .child(self.avatar(&format!("{id}-avatar"), &t.author))
                                    .child(self.author(&format!("{id}-author"), &t.author))
                                    .child(span("rep").text(compact(self.s.members.get(&t.author).map_or(0, |m| m.reputation))))
                                    .child(span("when").text(format!("asked {}", ago(self.ctx.tick, t.tick)))),
                            ),
                    ),
            )
    }
    fn entry_quora(&self, id: &str) -> Node {
        let t = &self.s.threads[id];
        let top = self.s.children(t, None).into_iter().next().and_then(|rid| t.replies.iter().find(|r| r.id == rid));
        el("article")
            .class("qcard")
            .id(format!("{id}-row"))
            .maybe(top.map(|r| {
                let m = self.s.members.get(&r.author);
                div("answerer")
                    .child(self.avatar(&format!("{id}-avatar"), &r.author))
                    .child(
                        div("")
                            .child(self.author(&format!("{id}-author"), &r.author))
                            .child(span("cred").text(m.map_or(String::new(), |m| m.flair.clone())))
                            .child(span("when").text(format!("Answered {}", ago(self.ctx.tick, r.tick)))),
                    )
            }))
            .child(div("h3").child(el("a").id(format!("row-{id}")).attr("href", self.s.permalink(t)).child(span("").id(format!("{id}-title")).text(t.title.as_str()))))
            .child(el("p").class("excerpt").id(format!("{id}-excerpt")).text(top.map_or(t.body.as_str(), |r| r.body.as_str()).chars().take(280).collect::<String>().replace('\n', " ")))
            .child(self.tags(&format!("{id}-tags"), &format!("{id}-tag"), &t.tags))
            .child(
                div("stats")
                    .id(format!("{id}-stats"))
                    .child(self.stat(format!("{id}-st-votes"), compact(self.s.score_of(id).max(0) as u64), word(self.s.score_of(id).max(0) as u64, "upvotes"), "s-votes"))
                    .child(self.stat(format!("{id}-st-answers"), t.replies.len().to_string(), word(t.replies.len() as u64, "answers"), if t.accepted.is_some() { "s-answers accepted" } else { "s-answers" }))
                    .child(self.stat(format!("{id}-st-views"), compact(t.views), word(t.views, "views"), "s-views"))
                    .child(span("asked").id(format!("{id}-asked")).text(format!("asked by {} {}", self.who(&t.author), ago(self.ctx.tick, t.tick)))),
            )
    }
    fn entry_reddit(&self, id: &str) -> Node {
        let t = &self.s.threads[id];
        let board = self.s.boards.get(&t.board);
        el("article")
            .class("post")
            .id(format!("row-{id}"))
            .child(self.votes(id, "threads"))
            .child(
                div("pbody")
                    .id(format!("{id}-body"))
                    .child(
                        el("p")
                            .class("sub")
                            .child(span("avatar sm").id(format!("{id}-icon")).style(&format!("background-color: {}", tint(&t.board))).text(initials(board.map_or(t.board.as_str(), |b| b.title.as_str()))))
                            .child(link(&format!("{id}-board"), format!("/r/{}", t.board), format!("r/{}", t.board)).class("board"))
                            .child(span("dot").text("•"))
                            .child(span("").id(format!("{id}-sub")).text("Posted by ").child(self.author(&format!("{id}-author"), &t.author)).text(format!(" {}", ago(self.ctx.tick, t.tick)))),
                    )
                    .child(div("h3").child(el("a").id(format!("{id}-open")).attr("href", self.s.permalink(t)).child(span("").id(format!("{id}-title")).text(t.title.as_str()))))
                    .maybe(t.url.as_deref().map(|u| link(&format!("{id}-out"), u, host(u)).class("out")))
                    .when(!t.body.is_empty(), |n| n.child(el("p").class("excerpt").id(format!("{id}-excerpt")).text(t.body.chars().take(320).collect::<String>())))
                    .child(
                        div("actions")
                            .id(format!("{id}-meta"))
                            .child(link(&format!("{id}-count"), self.s.permalink(t), plural(self.s.conversation_size(t), "comment")).class("act comments"))
                            .child(span("act").text("Share"))
                            .child(span("act").text("Save")),
                    ),
            )
    }
    fn entry_yelp(&self, id: &str, rank: usize) -> Node {
        let t = &self.s.threads[id];
        let b = business(t);
        let quote = self.s.children(t, None).into_iter().next().and_then(|rid| t.replies.iter().find(|r| r.id == rid)).map(|r| review_stars(&r.body).1.to_owned());
        el("article")
            .class("biz")
            .id(format!("row-{id}"))
            .child(span("photo").id(format!("{id}-icon")).style(&format!("background-color: {}", tint(&t.title))).text(initials(&t.title)))
            .child(
                div("info")
                    .id(format!("{id}-body"))
                    .child(div("h3").child(span("rank").id(format!("{id}-rank")).text(format!("{rank}. "))).child(el("a").id(format!("{id}-open")).attr("href", self.s.permalink(t)).child(span("").id(format!("{id}-title")).text(t.title.as_str()))))
                    .child(
                        div("rating")
                            .maybe(b.rating.map(|r| stars(&format!("{id}-stars"), r)))
                            .maybe(b.rating.map(|r| span("num").text(format!("{}.{}", r / 10, r % 10))))
                            .child(link(&format!("{id}-count"), self.s.permalink(t), format!("({})", plural(self.s.conversation_size(t), self.s.reply_word()))).class("count")),
                    )
                    .child(
                        el("p")
                            .class("sub")
                            .id(format!("{id}-sub"))
                            .child(link(&format!("{id}-board"), format!("/r/{}", t.board), self.board_name(&t.board)).class("cat"))
                            .when(!b.price.is_empty(), |n| n.child(span("price").text(format!(" · {}", b.price))))
                            .when(!b.hours.is_empty(), |n| n.child(span("hours").text(format!(" · {}", b.hours)))),
                    )
                    .child(self.tags(&format!("{id}-tags"), &format!("{id}-tag"), &t.tags))
                    .maybe(quote.map(|q| el("p").class("quote").id(format!("{id}-excerpt")).text(format!("“{}”", q.chars().take(180).collect::<String>())))),
            )
            .child(self.votes(id, "threads"))
    }
    fn pager(&self, page: usize, pages: usize) -> Node {
        let base = self.base.clone();
        let to = |n: usize| if n == 1 { base.clone() } else { href(&base, &[("p", &n.to_string())]) };
        el("nav")
            .id("pages")
            .class("pages")
            .attr("aria-label", "Pages")
            .when(page > 1, |n| n.child(link("page-prev", to(page - 1), "Prev").class("word")))
            .each(1..=pages, |n| link(&format!("page-{n}"), to(n), n.to_string()).class(if n == page { "on" } else { "" }))
            .when(page < pages, |n| n.child(link("page-next", to(page + 1), if self.is("hackernews") { "More" } else { "Next" }).class("word")))
    }
    fn listing(&self, title: &str, ids: &[String], page: usize) -> Vec<Node> {
        let pages = ids.len().div_ceil(PAGE_SIZE).max(1);
        let page = page.clamp(1, pages);
        let offset = (page - 1) * PAGE_SIZE;
        let shown = &ids[offset.min(ids.len())..(offset + PAGE_SIZE).min(ids.len())];
        let rows = shown.iter().enumerate().map(|(i, id)| self.entry(id, offset + i + 1));
        let list = match self.skin {
            "hackernews" => el("table").class("itemlist").id("list").child(el("tbody").children(rows)),
            "craigslist" => el("ul").class("results").id("list").children(rows),
            _ => div("list").id("list").children(rows),
        };
        let head = div("listhead")
            .child(el("h1").id("list-title").text(title))
            .when(self.s.qa(), |h| h.child(link("list-ask", "/submit", self.submit_label()).class("primary")));
        vec![
            head,
            if ids.is_empty() { el("p").id("list-empty").class("empty").text("Nothing here yet.") } else { empty() },
            list,
            if pages > 1 { self.pager(page, pages) } else { empty() },
        ]
    }

    // ------------------------------------------------------------------ sidebars
    /// The subscribe rail. Every board on it is a real page and a real toggle.
    fn rail(&self) -> Option<Node> {
        if !self.s.subreddits() || self.s.boards.is_empty() {
            return None;
        }
        let subs = self.s.subscriptions.get(&self.ctx.actor);
        Some(
            el("section")
                .class("card rail")
                .child(el("h2").id("rail-title").text(if self.is("yelp") { "Your categories" } else { "Your communities" }))
                .child(el("ul").id("rail").each(self.s.boards.values().enumerate(), |(i, b)| {
                    let on = subs.is_some_and(|x| x.contains(&b.id));
                    el("li")
                        .id(format!("board-{}", b.id))
                        .child(span("n").text((i + 1).to_string()))
                        .child(span("avatar sm").style(&format!("background-color: {}", tint(&b.id))).text(initials(&b.title)))
                        .child(
                            span("names")
                                .child(link(&format!("board-{}-link", b.id), format!("/r/{}", b.id), self.board_name(&b.id)))
                                .child(span("members").id(format!("board-{}-members", b.id)).text(format!("{} members", b.members))),
                        )
                        .child(self.post_button(&format!("board-{}-sub", b.id), &format!("/boards/{}/subscribe", b.id), &[], if on { "Joined" } else { "Join" }, on, "join"))
                })),
        )
    }
    fn side(&self, thread: Option<&Thread>, board: Option<&str>) -> Vec<Node> {
        let mut side = Vec::new();
        match self.skin {
            "reddit" => {
                if let Some(b) = board.and_then(|b| self.s.boards.get(b)) {
                    side.push(
                        el("section")
                            .class("card about")
                            .child(el("h2").text("About Community"))
                            .child(el("p").id("about-desc").text(b.description.as_str()))
                            .child(div("counts").child(el("b").text(compact(b.members))).child(span("").text("Members")))
                            .child(link("about-create", "/submit", self.submit_label()).class("primary")),
                    );
                } else {
                    side.push(
                        el("section")
                            .class("card home")
                            .child(div("banner"))
                            .child(el("h2").text("Home"))
                            .child(el("p").id("tagline").text(self.s.tagline.as_str()))
                            .child(link("home-create", "/submit", self.submit_label()).class("primary"))
                            .child(link("home-boards", "/r", "Browse Communities").class("secondary")),
                    );
                }
                side.extend(self.rail());
            }
            "yelp" => {
                if let Some(t) = thread {
                    let b = business(t);
                    side.push(
                        el("section")
                            .class("card bizinfo")
                            .child(el("h2").text("Location & Hours"))
                            .child(div("map").child(span("pin")))
                            .when(!b.address.is_empty(), |n| n.child(el("p").id("biz-address").class("addr").text(b.address.as_str())))
                            .when(!b.hours.is_empty(), |n| n.child(el("p").id("biz-hours").class("hours").text(b.hours.as_str())))
                            .when(!b.price.is_empty(), |n| n.child(el("p").id("biz-price").class("pricerange").text(format!("Price range {}", b.price)))),
                    );
                }
                side.extend(self.rail());
            }
            "stackoverflow" => {
                let mut hot: Vec<&Thread> = self.s.threads.values().collect();
                hot.sort_by(|a, b| b.views.cmp(&a.views).then_with(|| a.id.cmp(&b.id)));
                side.push(
                    el("section")
                        .class("card yellow")
                        .child(el("h2").text("The Overflow Blog"))
                        .child(el("p").id("side-tagline").child(span("pencil")).text(self.s.tagline.as_str()))
                        .child(el("h2").text("Featured on Meta"))
                        .child(el("p").child(span("bubble")).text(format!("{} questions from {} members", self.s.threads.len(), self.s.members.len())))
                        .child(el("p").child(span("bubble")).text("Policy: answers must show what was tried")),
                );
                side.push(el("section").class("card tagbox").child(el("h2").text("Watched Tags")).child(div("tags").each(
                    self.tag_counts().into_iter().take(10).enumerate(),
                    |(i, (tag, _))| link(&format!("side-tag-{i}"), Self::tag_url(&tag), tag).class("tag"),
                )));
                side.push(el("section").class("hot").child(el("h2").text("Hot Network Questions")).child(el("ul").each(
                    hot.into_iter().filter(|t| thread.is_none_or(|x| x.id != t.id)).take(6).enumerate(),
                    |(i, t)| el("li").child(span("fav").style(&format!("background-color: {}", tint(&t.id)))).child(link(&format!("hot-{i}"), self.s.permalink(t), t.title.as_str())),
                )));
            }
            "quora" => {
                let mut writers: Vec<_> = self.s.members.values().collect();
                writers.sort_by(|a, b| b.reputation.cmp(&a.reputation).then_with(|| a.handle.cmp(&b.handle)));
                side.push(el("section").class("card writers").child(el("h2").text("Top writers")).child(el("ul").each(writers.into_iter().take(6).enumerate(), |(i, m)| {
                    el("li")
                        .child(self.avatar(&format!("writer-{i}-avatar"), &m.handle))
                        .child(span("names").child(link(&format!("writer-{i}"), format!("/u/{}", m.handle), m.name.as_str())).child(span("cred").text(m.flair.as_str())))
                })));
                side.push(el("p").id("tagline").class("tagline").text(self.s.tagline.as_str()));
            }
            _ => {}
        }
        side
    }

    // ------------------------------------------------------------------ pages
    fn page(&self, title: &str, class: &str, main: Vec<Node>, side: Vec<Node>) -> Result<HttpResponse> {
        html::page(&self.document(title, class, main, side))
    }
    /// Craigslist's front page: the category columns, from the tags the listings carry.
    fn cl_categories(&self) -> Node {
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for t in self.s.threads.values() {
            let Some(first) = t.tags.first() else { continue };
            let at = match groups.iter().position(|(g, _)| g == first) {
                Some(i) => i,
                None => {
                    groups.push((first.clone(), Vec::new()));
                    groups.len() - 1
                }
            };
            for tag in t.tags.iter().skip(1) {
                if !groups[at].1.contains(tag) {
                    groups[at].1.push(tag.clone());
                }
            }
        }
        groups.sort();
        for g in &mut groups {
            g.1.sort();
        }
        el("section")
            .id("cats")
            .class("cats")
            .child(el("h2").id("tagline").class("city").text(self.s.tagline.split(':').next().unwrap_or("").trim().to_owned()))
            .child(div("cols").each(groups.iter().enumerate(), |(i, (group, subs))| {
                div("cat")
                    .child(div("h3").child(link(&format!("cat-{i}"), Self::tag_url(group), group.as_str())))
                    .child(el("ul").each(subs.iter().enumerate(), |(j, tag)| el("li").child(link(&format!("cat-{i}-{j}"), Self::tag_url(tag), tag.as_str()))))
            }))
    }
    pub(crate) fn front(&self, page: usize) -> Result<HttpResponse> {
        let ids = self.s.front(&self.ctx.actor, self.ctx.tick);
        let title = match self.skin {
            "stackoverflow" => "Top Questions",
            "quora" => "Top questions",
            "hackernews" => "Top stories",
            "craigslist" => "Top listings",
            "yelp" => "Popular businesses",
            _ => "Popular posts",
        };
        let mut main = Vec::new();
        if self.is("craigslist") {
            main.push(self.cl_categories());
        }
        if self.is("quora") {
            main.push(
                div("card askbox")
                    .maybe(self.s.member_of(&self.ctx.actor).map(|m| self.avatar("ask-avatar", &m.handle)))
                    .child(link("ask-box", "/submit", "What do you want to ask or share?").class("ask")),
            );
        }
        if self.is("reddit") {
            main.push(
                div("card createbar")
                    .maybe(self.s.member_of(&self.ctx.actor).map(|m| self.avatar("create-avatar", &m.handle)))
                    .child(link("create-box", "/submit", "Create Post").class("ask")),
            );
        }
        main.extend(self.listing(title, &ids, page));
        self.page(&format!("{} / {title}", self.s.brand), "front", main, self.side(None, None))
    }
    pub(crate) fn simple_list(&self, title: &str, ids: Vec<String>, page: usize) -> Result<HttpResponse> {
        self.page(&format!("{} / {title}", self.s.brand), "list", self.listing(title, &ids, page), self.side(None, None))
    }
    pub(crate) fn boards(&self) -> Result<HttpResponse> {
        let title = if self.is("yelp") { "Categories" } else { "Communities" };
        let main = vec![
            div("listhead").child(el("h1").id("list-title").text(title)),
            el("ul").class("boards card").each(self.s.boards.values(), |b| {
                el("li")
                    .child(span("avatar").style(&format!("background-color: {}", tint(&b.id))).text(initials(&b.title)))
                    .child(el("p").id(format!("about-{}", b.id)).child(link(&format!("about-{}-link", b.id), format!("/r/{}", b.id), self.board_name(&b.id))).text(format!(" — {}", b.description)))
            }),
        ];
        self.page(&format!("{} / {title}", self.s.brand), "boards", main, self.side(None, None))
    }
    pub(crate) fn board(&self, board: &str, page: usize) -> Result<HttpResponse> {
        let Some(b) = self.s.boards.get(board) else {
            return cw_service_common::error(404, "no such board");
        };
        let on = self.s.subscriptions.get(&self.ctx.actor).is_some_and(|x| x.contains(board));
        let head = el("section")
            .id("board-head")
            .class("boardhead")
            .child(div("banner").style(&format!("background-color: {}", tint(board))))
            .child(
                div("row")
                    .child(span("avatar lg").id("board-head-icon").style(&format!("background-color: {}", tint(board))).text(initials(&b.title)))
                    .child(
                        div("names")
                            .id("board-head-body")
                            .child(el("h1").id("board-head-title").text(if self.is("reddit") { format!("r/{board}") } else { b.title.clone() }))
                            .child(el("p").id("board-head-desc").text(b.description.as_str()))
                            .child(span("members").id("board-head-members").text(format!("{} members", b.members))),
                    )
                    .child(self.post_button("board-head-sub", &format!("/boards/{board}/subscribe"), &[], if on { "Joined" } else { "Join" }, on, "join")),
            );
        let ids = self.s.in_board(board, self.ctx.tick);
        let mut main = vec![head];
        main.extend(self.listing(&b.title, &ids, page));
        self.page(&format!("{} / {}", self.board_name(board), self.s.brand), "board", main, self.side(None, Some(board)))
    }

    /// One reply, with everything under it nested inside, which is what draws the thread lines.
    fn reply(&self, t: &Thread, r: &Reply) -> Node {
        let rid = r.id.as_str();
        let accepted = t.accepted.as_deref() == Some(rid);
        let member = self.s.members.get(&r.author);
        let (rating, shown) = if self.is("yelp") { review_stars(&r.body) } else { (None, r.body.as_str()) };
        let skip = r.body.len() - shown.len();
        let head = div("rhead")
            .id(format!("{rid}-head"))
            .child(self.avatar(&format!("{rid}-avatar"), &r.author))
            .child(
                span("by")
                    .id(format!("{rid}-by"))
                    .child(self.author(&format!("{rid}-author"), &r.author))
                    .text(format!(" · {}", ago(self.ctx.tick, r.tick))),
            )
            .maybe(member.filter(|m| !m.flair.is_empty()).map(|m| span("flair").id(format!("{rid}-flair")).text(m.flair.as_str())))
            .maybe(member.filter(|m| self.s.qa() && m.reputation > 0).map(|m| span("rep").id(format!("{rid}-rep")).text(compact(m.reputation))))
            .when(accepted, |h| h.child(span("accepted").id(format!("{rid}-accepted")).text("Accepted")));
        let mut col = div("rcol").id(format!("{rid}-col")).child(head).maybe(rating.map(|n| stars(&format!("{rid}-stars"), n * 10)));
        // A link keeps the index it has in the whole body whether or not a rating led it.
        col = col.child(self.prose(&format!("{rid}-body"), &format!("{rid}-src"), &r.body, skip));
        // Q&A: comments hang off the answer and the asker gets the accept control. Everywhere
        // else a reply box hangs off every comment, which is what makes the tree a tree.
        if self.s.qa() {
            col = col.when(!r.comments.is_empty(), |c| {
                c.child(el("ul").class("comments").each(r.comments.iter(), |c| {
                    el("li").id(format!("{}-text", c.id)).text(format!("{} — ", c.body)).child(self.author(&format!("{}-author", c.id), &c.author)).child(span("when").text(format!(" {}", ago(self.ctx.tick, c.tick))))
                }))
            });
            let mut controls = div("controls").id(format!("{rid}-controls"));
            if self.s.member_of(&self.ctx.actor).is_some_and(|m| m.handle == t.author) {
                controls = controls.child(self.post_button(
                    &format!("{rid}-accept"),
                    &format!("/threads/{}/accept", t.id),
                    &[("reply", rid)],
                    if accepted { "Unaccept" } else { "Accept" },
                    accepted,
                    "accept",
                ));
            }
            col = col.child(controls.child(self.inline_form(&format!("{rid}-comment"), &format!("/replies/{rid}/comments"), "Add a comment", &[("view", &self.here)], "Add comment")));
        } else {
            col = col.child(self.inline_form(&format!("{rid}-reply"), &format!("/threads/{}/replies", t.id), "Reply", &[("parent", rid), ("view", &self.here)], "Reply"));
        }
        let children = self.s.children(t, Some(rid));
        div(if accepted { "reply accepted" } else { "reply" })
            .id(format!("reply-{rid}"))
            .child(div("rrow").id(format!("{rid}-row")).child(self.votes(rid, "replies")).when(self.s.linkfeed(), |n| n.child(span("score").id(format!("{rid}-score")).text(self.s.score_of(rid).to_string()))).child(col))
            .when(!children.is_empty(), |n| {
                n.child(div("children").each(children.iter(), |c| match t.replies.iter().find(|x| x.id == *c) {
                    Some(child) => self.reply(t, child),
                    None => empty(),
                }))
            })
    }
    pub(crate) fn thread(&self, t: &Thread) -> Result<HttpResponse> {
        let id = t.id.as_str();
        let s = self.s;
        let count = if s.qa() { t.replies.len() } else { s.conversation_size(t) };
        let byline = el("p").id("thread-by").class("byline");
        let byline = match self.skin {
            "reddit" => byline
                .child(link("thread-board", format!("/r/{}", t.board), format!("r/{}", t.board)).class("board"))
                .text(" · Posted by ")
                .child(self.author("thread-author", &t.author))
                .text(format!(" {}", ago(self.ctx.tick, t.tick))),
            "yelp" => byline
                .child(link("thread-board", format!("/r/{}", t.board), self.board_name(&t.board)).class("board"))
                .text(" · listed by ")
                .child(self.author("thread-author", &t.author))
                .text(format!(" {}", ago(self.ctx.tick, t.tick))),
            "stackoverflow" | "quora" => byline
                .child(span("k").text("Asked "))
                .text(format!("{} by ", ago(self.ctx.tick, t.tick)))
                .child(self.author("thread-author", &t.author))
                .text(" · ")
                .child(span("k").text("Viewed "))
                .text(format!("{} times", compact(t.views))),
            _ => byline
                .text(format!("{} points by ", s.score_of(id)))
                .child(self.author("thread-author", &t.author))
                .text(format!(" {}", ago(self.ctx.tick, t.tick)))
                .child(span("sep").text(" | "))
                .child(span("n").text(plural(count, s.reply_word()))),
        };
        let yelp = self.is("yelp").then(|| business(t));
        let title = el("h1").id("thread-title").text(t.title.as_str());
        let out = t.url.as_deref().map(|u| link("thread-out", u, u).class("out"));
        let mut col = div("tcol").id("thread-col");
        let lead_outside = s.qa() || self.is("yelp");
        if !lead_outside {
            col = col.child(byline.clone()).child(title.clone()).maybe(out.clone());
        }
        // A Yelp body leads with the address, hours and price, which the sidebar already carries;
        // the prose starts at the paragraph after them so the page does not say it twice.
        let skip = match self.is("yelp") {
            true => t.body.split_once("\n\n").map_or(0, |(head, _)| head.len() + 2),
            false => 0,
        };
        col = col
            .when(t.body[skip..].trim() != "", |c| c.child(self.prose("thread-body", "thread-src", &t.body, skip)))
            .child(self.tags("thread-tags", "thread-tag", &t.tags))
            .when(s.qa(), |c| {
                let m = s.members.get(&t.author);
                c.child(
                    div("usercard")
                        .child(span("when").text(format!("asked {}", ago(self.ctx.tick, t.tick))))
                        .child(self.avatar("thread-avatar", &t.author))
                        .child(span("who").text(self.who(&t.author)))
                        .child(span("rep").text(compact(m.map_or(0, |m| m.reputation)))),
                )
            });
        let article = el("article").id("thread").class("thread").child(div("trow").id("thread-row").child(self.votes(id, "threads")).child(col));
        let compose = el("section").class("compose").child(
            self.fields_form(
                "compose",
                &format!("/threads/{id}/replies"),
                &[(
                    "body",
                    match self.skin {
                        "stackoverflow" | "quora" => "Your Answer",
                        "yelp" => "Write a review",
                        "craigslist" => "Reply to this posting",
                        _ => "Add a comment",
                    },
                    true,
                )],
                &[("view", &self.here)],
                match self.skin {
                    "stackoverflow" => "Post Your Answer",
                    "quora" => "Post",
                    "yelp" => "Post Review",
                    "hackernews" => "add comment",
                    "craigslist" => "reply",
                    _ => "Comment",
                },
            ),
        );
        let replies_title = el("h2").id("replies-title").text(if self.is("yelp") { format!("Recommended Reviews · {}", plural(count, s.reply_word())) } else if s.qa() { plural(count, &capital(s.reply_word())) } else { plural(count, s.reply_word()) });
        let replies = div("replies").id("replies").each(s.children(t, None).iter(), |rid| match t.replies.iter().find(|r| r.id == *rid) {
            Some(r) => self.reply(t, r),
            None => empty(),
        });
        let mut main = Vec::new();
        if lead_outside {
            main.push(
                div("qhead")
                    .child(title)
                    .maybe(yelp.as_ref().and_then(|b| b.rating).map(|r| div("rating").child(stars("thread-stars", r)).child(span("num").text(format!("{}.{}", r / 10, r % 10))).child(span("count").text(format!("({})", plural(count, s.reply_word()))))))
                    .child(byline)
                    .maybe(out),
            );
        }
        main.push(article);
        if s.qa() {
            main.extend([replies_title, replies, compose]);
        } else {
            main.extend([compose, replies_title, replies]);
        }
        self.page(&format!("{} / {}", t.title, s.brand), "thread", main, self.side(Some(t), Some(&t.board)))
    }
    pub(crate) fn submit(&self) -> Result<HttpResponse> {
        let s = self.s;
        let title = match self.skin {
            "stackoverflow" => "Ask a public question",
            "quora" => "Add question",
            "hackernews" => "Submit",
            "craigslist" => "create a posting",
            "yelp" => "Add a business",
            _ => "Create a post",
        };
        let mut main = vec![div("listhead").child(el("h1").id("submit-head").text(title))];
        if s.member_of(&self.ctx.actor).is_none() {
            main.push(el("p").id("submit-anon").class("empty").text("Sign in on this machine as a member of this site to post."));
            return self.page(&format!("{} / {title}", s.brand), "submit", main, self.side(None, None));
        }
        let mut fields: Vec<(&str, &str, bool)> = vec![("title", "Title", false)];
        if s.subreddits() {
            fields.push(("board", if self.is("yelp") { "Category" } else { "Community" }, false));
        }
        if s.linkfeed() {
            fields.push(("url", if self.is("hackernews") { "url" } else { "Link" }, false));
        }
        if s.qa() {
            fields.push(("tags", "Tags (comma separated)", false));
        }
        fields.push((
            "body",
            match s.mode.as_str() {
                "qa" => "What did you try?",
                _ => "Text (optional)",
            },
            true,
        ));
        main.push(div("card submitcard").child(self.fields_form("submit", "/threads", &fields, &[], match self.skin {
            "stackoverflow" => "Post your question",
            "hackernews" => "submit",
            "craigslist" => "publish",
            "quora" => "Add question",
            _ => "Post",
        })));
        if self.is("hackernews") {
            main.push(el("p").class("hint").text("Leave url blank to submit a question for discussion. If there is no url, text will appear at the top of the thread. If there is a url, text is optional."));
        }
        self.page(&format!("{} / {title}", s.brand), "submit", main, self.side(None, None))
    }
    pub(crate) fn search(mut self, q: &str, page: usize) -> Result<HttpResponse> {
        self.query = q.to_owned();
        let ids = self.s.search(q, self.ctx.tick);
        let title = if q.trim().is_empty() { "Search".to_owned() } else { format!("{} results for \"{q}\"", ids.len()) };
        if !q.trim().is_empty() {
            self.base = href("/search", &[("q", q)]);
            self.here = if page > 1 { href(&self.base, &[("p", &page.to_string())]) } else { self.base.clone() };
        }
        let main = self.listing(&title, &ids, page);
        self.page(&format!("{} / Search", self.s.brand), "search", main, self.side(None, None))
    }
    pub(crate) fn member(&self, handle: &str, page: usize) -> Result<HttpResponse> {
        let Some(m) = self.s.members.get(handle) else {
            return cw_service_common::error(404, "no such member");
        };
        let ids = self.s.by_author(handle);
        let card = el("section")
            .id("member")
            .class("card member")
            .child(self.avatar("member-avatar", handle).class("xl"))
            .child(
                div("facts")
                    .id("member-body")
                    .child(el("h1").id("member-name").text(m.name.as_str()))
                    .child(el("p").id("member-handle").class("handle").text(self.who(handle)))
                    .when(!m.flair.is_empty(), |n| n.child(el("p").id("member-flair").class("flair").text(m.flair.as_str())))
                    .when(!m.bio.is_empty(), |n| n.child(el("p").id("member-bio").class("bio").text(m.bio.as_str())))
                    .child(
                        div("badges")
                            .id("member-meta")
                            .child(span("badge rep").id("member-rep").text(format!("{} {}", m.reputation, if self.is("hackernews") || self.is("reddit") { "karma" } else { "reputation" })))
                            .child(span("badge").id("member-posts").text(plural(ids.len(), self.s.item_word()))),
                    )
                    .when(!m.site.is_empty(), |n| n.child(link("member-site", m.site.as_str(), m.site.as_str()))),
            );
        let mut main = vec![card];
        main.extend(self.listing(if self.is("yelp") { "Businesses" } else { "Posts" }, &ids, page));
        self.page(&format!("{} / {}", m.name, self.s.brand), "member", main, self.side(None, None))
    }
}
