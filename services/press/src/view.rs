//! Pages as HTML. Every publication serves the same routes out of the same article
//! map; `layout` decides what the front page is made of and `skin` which stylesheet
//! dresses it, which is the whole difference between nytimes.com and a personal blog.
//! The sheets are real files next to this one: `base.css` is shared, and one
//! `<skin>.css` per publication is appended to it. The seeded palette rides on
//! `<html style>` as custom properties so the sheets stay static.
//!
//! Element ids are the agent API and are the ones the `Page` version used:
//! `masthead`, `masthead-home`, `masthead-brand`, `masthead-<section>`,
//! `masthead-archive`, `masthead-saved`, `masthead-follow` (a submit button inside
//! `masthead-follow-form`), `card-<id>` (one link, with `-art`, `-kicker`, `-section`,
//! `-date`, `-title`, `-dek`, `-byline` inside), `front-heading`, `front-layout`,
//! `front-lines`, `front-side`, `front-side-heading`, `mostread-<id>`, `front-grid`,
//! `front-tagline`, `list-header`, `list-title`, `list-follow`, `list-empty`,
//! `article-*` (`-kicker`, `-section`, `-date`, `-title`, `-dek`, `-byline`, `-avatar`,
//! `-credit`, `-save`, `-art`, `-p<n>`, `-p<n>-link-<word>`, `-links`, `-ref-<n>`,
//! `-tags`, `-tag-<tag>`), `comments-heading`, `comment` (the form), `comment-text`,
//! `comment-submit`, `comment-<cid>` with `-body`, `-author`, `-text`, `-like`,
//! `newsletter-heading`, `subscribe`, `subscribe-email`, `subscribe-submit`.
//!
//! The rewrite kept every one of those and added the ids the markup needs of its own:
//! every toggle is a one-button `POST` form, so `masthead-follow`, `list-follow`,
//! `article-save` and `comment-<cid>-like` each sit inside a `<name>-form` carrying the
//! `return` field, and each keeps its `<name>-text` span; the containers that used to be
//! anonymous rows are `masthead-nav`, `front`, `front-lead`, `front-posts`,
//! `front-topics` (with `front-topics-heading` and one `topic-<tag>` chip each), `list`,
//! `list-stories`, `article`, `article-body`, `article-links-heading`, `comments`,
//! `newsletter` and `foot` (with `foot-<section>`, `foot-archive`, `foot-saved`);
//! `masthead-follow-form` and `subscribe` carry a hidden `return` of the page they are on,
//! since both sit in the furniture of every page and neither may move the reader;
//! `article-tag-<tag>-text` labels a chip. The `Page` dividers (`article-divider`,
//! `newsletter-divider`, `front-rule-<id>`, `list-rule-<id>`) are gone: a rule is a
//! border in the stylesheet, not an element an agent can address.
//! The splash a seed without articles serves is `brand` and `tagline`, and nothing else.
use crate::{article, byline, has, num, recent, section_title, sections, BRAND, LAYOUTS, PUBLICATION, SKINS, TAGLINE};
use cw_protocol::{HttpResponse, Result};
use cw_service_common as web;
use cw_service_common::html::{
    button, div, el, form, hidden, link, span, text, text_input, Document, Html as Node,
};
use serde_json::Value;

const BASE: &str = include_str!("base.css");
fn sheet(skin: &str) -> &'static str {
    match skin {
        "nyt" => include_str!("nyt.css"),
        "bbc" => include_str!("bbc.css"),
        "cnn" => include_str!("cnn.css"),
        "reuters" => include_str!("reuters.css"),
        "verge" => include_str!("verge.css"),
        "ars" => include_str!("ars.css"),
        "gnews" => include_str!("gnews.css"),
        "medium" => include_str!("medium.css"),
        "substack" => include_str!("substack.css"),
        "blog" => include_str!("blog.css"),
        _ => "",
    }
}
/// The skin a seed without a `skin` key gets: the publication its brand names, the
/// personal-blog sheet for any other blog, and the plain base sheet otherwise.
fn default_skin(brand: &str, layout: &str) -> &'static str {
    let brand = brand.to_ascii_lowercase();
    for (needle, skin) in [
        ("new york times", "nyt"),
        ("bbc", "bbc"),
        ("cnn", "cnn"),
        ("reuters", "reuters"),
        ("verge", "verge"),
        ("ars technica", "ars"),
        ("google news", "gnews"),
        ("medium", "medium"),
        ("substack", "substack"),
    ] {
        if brand.contains(needle) {
            return skin;
        }
    }
    if layout == "blog" {
        "blog"
    } else {
        "plain"
    }
}
/// Two-stop stand-in art: a stable hash of the slug keeps a story the same colours forever.
const ART: &[(&str, &str)] = &[
    ("#3b5b7a", "#9fb6c9"),
    ("#7a4a3b", "#d9b9a3"),
    ("#3f6b57", "#a9cdb8"),
    ("#5c4a7a", "#c3b3da"),
    ("#2f4f5f", "#8fb9c6"),
    ("#7a6a3b", "#dccf9f"),
    ("#6b3f52", "#d3a9bb"),
    ("#44506b", "#aab4cf"),
];
fn art_tint(seed: &str) -> (&'static str, &'static str) {
    let hash = seed.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    ART[(hash >> 11) as usize % ART.len()]
}
fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase()
}

/// Everything a render reads once: the state, who is reading, the layout, the skin, the palette.
pub(crate) struct Chrome<'a> {
    state: &'a Value,
    actor: &'a str,
    /// The path being rendered. The masthead Follow and the newsletter sit on every page,
    /// so their `return` is wherever the reader pressed them, not the front page.
    here: &'a str,
    layout: String,
    skin: String,
    brand: String,
    root: String,
}
impl<'a> Chrome<'a> {
    pub(crate) fn read(state: &'a Value, actor: &'a str, here: &'a str) -> Result<Self> {
        let theme = web::theme(state)?;
        let layout = web::variant(state, "layout", LAYOUTS)?;
        let brand = match web::text(state, "brand") {
            b if b.is_empty() => BRAND.to_owned(),
            b => b,
        };
        let skin = match state.get("skin").and_then(Value::as_str).unwrap_or("") {
            "" => default_skin(&brand, &layout).to_owned(),
            _ => web::variant(state, "skin", SKINS)?,
        };
        let or = |value: &Option<String>, fallback: &str| value.clone().unwrap_or_else(|| fallback.to_owned());
        let root = format!(
            "--accent: {}; --ink: {}; --muted: {}; --surface: {}; --paper: {}; --content: {}px",
            or(&theme.accent, "#5200ff"),
            or(&theme.ink, "#08090a"),
            or(&theme.muted, "#606468"),
            or(&theme.surface, "#f5f5f7"),
            or(&theme.background, "#ffffff"),
            theme.content_width.unwrap_or(900).clamp(320, 1400)
        );
        Ok(Self { state, actor, here, layout, skin, brand, root })
    }
    fn document(&self, title: &str, page_class: &str, main: Node) -> Result<HttpResponse> {
        let doc = Document::new(title)
            .lang("en")
            .stylesheet(BASE)
            .stylesheet(sheet(&self.skin))
            .root_style(&self.root)
            .body_class(&format!("skin-{} layout-{} {page_class}", self.skin, self.layout))
            .body([self.masthead(), main, self.footer()]);
        web::html::page(&doc)
    }
    fn path(&self, id: &str) -> String {
        crate::href(self.state, id, &self.layout)
    }
    /// The wordmark, a span per word so a sheet can set the BBC's blocks or Ars's
    /// circle; the BBC's blocks and Google's colours further split the first word into letters.
    fn wordmark(&self) -> Node {
        let mut mark = span("wordmark").id("masthead-brand");
        for (i, word) in self.brand.split_whitespace().enumerate() {
            if i > 0 {
                mark = mark.child(text(" "));
            }
            let mut w = span(&format!("w w{}", i.min(3)));
            if matches!(self.skin.as_str(), "bbc" | "gnews") && i == 0 {
                w = w.each(word.chars().enumerate(), |(n, c)| span(&format!("ch c{}", n % 6)).text(c.to_string()));
            } else {
                w = w.text(word);
            }
            mark = mark.child(w);
        }
        mark
    }
    /// A toggle that really submits: a one-button POST form carrying where to come back to.
    fn pill(&self, id: &str, label: &str, on: bool, action: &str, back: &str) -> Node {
        form(&format!("{id}-form"), action, "post").class("pill-form").child(hidden("return", back)).child(
            el("button")
                .id(id)
                .attr("type", "submit")
                .class(if on { "pill on" } else { "pill" })
                .child(span("").id(format!("{id}-text")).text(label)),
        )
    }
    /// The masthead: wordmark, section nav, reading list and the publication Follow control.
    fn masthead(&self) -> Node {
        let state = self.state;
        let following = has(state, "follows", self.actor, PUBLICATION);
        let today = recent(state, |_| true)
            .first()
            .map(|id| web::text(article(state, id).unwrap_or(&Value::Null), "date"))
            .unwrap_or_default();
        let nav = el("nav")
            .id("masthead-nav")
            .class("sections")
            .attr("aria-label", "Sections")
            .each(sections(state), |id| link(&format!("masthead-{id}"), format!("/{id}"), section_title(state, &id)))
            .child(link("masthead-archive", "/archive", "Archive"));
        el("header")
            .id("masthead")
            .class("masthead")
            .child(
                div("util")
                    .child(span("today").text(today))
                    .child(span("edition").text(web::text(state, "tagline"))),
            )
            .child(
                div("bar")
                    .child(
                        el("a")
                            .id("masthead-home")
                            .class("brand")
                            .attr("href", "/")
                            .child(span("logo").attr("aria-hidden", "true"))
                            .child(self.wordmark()),
                    )
                    .child(nav)
                    .child(
                        div("tools")
                            .child(link("masthead-saved", "/saved", "Reading list").class("saved"))
                            .child(self.pill(
                                "masthead-follow",
                                if following { "Following" } else { "Follow" },
                                following,
                                "/follow",
                                self.here,
                            )),
                    ),
            )
    }
    fn footer(&self) -> Node {
        let state = self.state;
        el("footer")
            .id("foot")
            .class("foot")
            .child(
                div("inner")
                    .child(span("foot-brand").text(self.brand.as_str()))
                    .child(
                        el("nav")
                            .class("foot-links")
                            .attr("aria-label", "Footer")
                            .each(sections(state), |id| {
                                link(&format!("foot-{id}"), format!("/{id}"), section_title(state, &id))
                            })
                            .child(link("foot-archive", "/archive", "Archive"))
                            .child(link("foot-saved", "/saved", "Reading list")),
                    )
                    .child(span("legal").text(format!("© 2026 {}", self.brand))),
            )
    }
    /// A headline card sized by `scale`: 0 is a dense wire line, 1 a grid card, 2 the hero.
    /// The whole card is one link; the sheet decides which parts a scale shows.
    fn card(&self, id: &str, scale: u8) -> Node {
        let state = self.state;
        let a = article(state, id).cloned().unwrap_or(Value::Null);
        let (from, to) = art_tint(id);
        let cid = format!("card-{id}");
        el("a")
            .id(cid.as_str())
            .class(&format!("card s{scale}"))
            .attr("href", self.path(id))
            .style(&format!("--a: {from}; --b: {to}"))
            .child(span("art").id(format!("{cid}-art")).attr("aria-hidden", "true"))
            .child(
                span("text")
                    .child(
                        span("kicker")
                            .id(format!("{cid}-kicker"))
                            .child(
                                span("section")
                                    .id(format!("{cid}-section"))
                                    .text(section_title(state, &web::text(&a, "section"))),
                            )
                            .child(text(" "))
                            .child(span("date").id(format!("{cid}-date")).text(web::text(&a, "date"))),
                    )
                    .child(span("title").id(format!("{cid}-title")).text(web::text(&a, "title")))
                    .child(span("dek").id(format!("{cid}-dek")).text(web::text(&a, "dek")))
                    .child(span("byline").id(format!("{cid}-byline")).text(byline(state, id))),
            )
    }
    fn newsletter(&self) -> Node {
        let lead = match web::text(self.state, "tagline") {
            t if t.is_empty() => TAGLINE.to_owned(),
            t => t,
        };
        el("section")
            .id("newsletter")
            .class("newsletter")
            .child(el("h2").id("newsletter-heading").text("Get the newsletter"))
            .child(el("p").class("lead").text(lead))
            .child(
                form("subscribe", "/subscribe", "post")
                    .child(hidden("return", self.here))
                    .child(
                        text_input("subscribe-email", "email", "")
                            .attr("aria-label", "Email address")
                            .attr("placeholder", "Email address")
                            .attr("autocomplete", "off"),
                    )
                    .child(button("subscribe-submit", "Subscribe")),
            )
    }
    /// The rail beside a front page: the most read stories, then the topics in use.
    fn side(&self, all: &[String]) -> Node {
        let state = self.state;
        let mut tags: Vec<String> = Vec::new();
        for id in all {
            for tag in web::strings(article(state, id).unwrap_or(&Value::Null), "tags") {
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
        tags.truncate(10);
        el("aside")
            .id("front-side")
            .class("side")
            .child(el("h2").id("front-side-heading").text("Most read"))
            .child(el("ol").class("ranked").each(all.iter().take(5), |id| {
                el("li").child(link(
                    &format!("mostread-{id}"),
                    self.path(id),
                    web::text(article(state, id).unwrap_or(&Value::Null), "title"),
                ))
            }))
            .when(!tags.is_empty(), |side| {
                side.child(el("h2").id("front-topics-heading").class("topics-heading").text("Topics")).child(
                    div("topics")
                        .id("front-topics")
                        .each(tags.iter(), |tag| link(&format!("topic-{tag}"), format!("/tag/{tag}"), format!("#{tag}")).class("chip")),
                )
            })
    }
    pub(crate) fn front(&self) -> Result<HttpResponse> {
        let all = recent(self.state, |_| true);
        let main = el("main").id("front").class("front");
        let main = match self.layout.as_str() {
            // The wire: a dated stack, newest first, beside what is being read most.
            "wire" => main.child(el("h1").id("front-heading").class("heading").text("Latest")).child(
                div("layout")
                    .id("front-layout")
                    .child(
                        el("section")
                            .id("front-lines")
                            .class("stories lines")
                            .each(all.iter().enumerate(), |(i, id)| self.card(id, 0).class(&format!("n{}", i.min(9)))),
                    )
                    .child(self.side(&all)),
            ),
            // The magazine: one hero, then a grid, beside the same rail.
            "magazine" => main.child(
                div("layout")
                    .id("front-layout")
                    .child(div("lead").id("front-lead").maybe(all.first().map(|id| self.card(id, 2))))
                    .child(
                        el("section")
                            .id("front-grid")
                            .class("stories grid")
                            .each(all.iter().skip(1).enumerate(), |(i, id)| self.card(id, 1).class(&format!("n{}", i.min(9)))),
                    )
                    .child(self.side(&all)),
            ),
            // The blog: a line about the author, then posts in reverse chronological order.
            _ => main
                .child(el("p").id("front-tagline").class("tagline").text(web::text(self.state, "tagline")))
                .child(
                    div("layout")
                        .id("front-layout")
                        .child(
                            el("section")
                                .id("front-posts")
                                .class("stories posts")
                                .each(all.iter().enumerate(), |(i, id)| self.card(id, 1).class(&format!("n{}", i.min(9)))),
                        )
                        .child(self.side(&all)),
                ),
        };
        self.document(&self.brand, "page-front", main.child(self.newsletter()))
    }
    pub(crate) fn list(&self, title: &str, entries: &[String], follow: Option<&str>) -> Result<HttpResponse> {
        let main = el("main")
            .id("list")
            .class("list")
            .child(
                div("list-header")
                    .id("list-header")
                    .child(el("h1").id("list-title").text(title))
                    .maybe(follow.map(|tag| {
                        let on = has(self.state, "follows", self.actor, tag);
                        self.pill(
                            "list-follow",
                            if on { "Following" } else { "Follow topic" },
                            on,
                            &format!("/tags/{tag}/follow"),
                            &format!("/tag/{tag}"),
                        )
                    })),
            )
            .when(entries.is_empty(), |m| m.child(el("p").id("list-empty").class("empty").text("Nothing here yet.")))
            .child(el("section").id("list-stories").class("stories listing").each(entries, |id| self.card(id, 1)))
            .child(self.newsletter());
        self.document(&format!("{title} - {}", self.brand), "page-list", main)
    }
    pub(crate) fn article(&self, id: &str) -> Result<HttpResponse> {
        let state = self.state;
        let Some(a) = article(state, id).cloned() else {
            return web::error(404, "article not found");
        };
        let back = self.path(id);
        let saved = has(state, "saved", self.actor, id);
        let title = web::text(&a, "title");
        let author = web::text(&a, "byline");
        let (from, to) = art_tint(id);
        let (avatar, _) = art_tint(&author);
        let paragraphs: Vec<String> = web::strings(&a, "body");
        let related: Vec<(String, String)> = a
            .get("links")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|l| (web::text(l, "label"), web::text(l, "url")))
            .collect();
        let comments = a.get("comments").and_then(Value::as_array).cloned().unwrap_or_default();
        let story = el("article")
            .class("story")
            .child(
                div("kicker")
                    .id("article-kicker")
                    .child(link(
                        "article-section",
                        format!("/{}", web::text(&a, "section")),
                        section_title(state, &web::text(&a, "section")),
                    ))
                    .child(span("date").id("article-date").text(web::text(&a, "date"))),
            )
            .child(el("h1").id("article-title").text(title.as_str()))
            .child(el("p").id("article-dek").class("dek").text(web::text(&a, "dek")))
            .child(
                div("credit")
                    .id("article-byline")
                    .child(
                        span("avatar")
                            .id("article-avatar")
                            .attr("aria-hidden", "true")
                            .style(&format!("background-color: {avatar}"))
                            .text(initials(&author)),
                    )
                    .child(span("who").id("article-credit").text(byline(state, id)))
                    .child(self.pill(
                        "article-save",
                        if saved { "Saved" } else { "Save" },
                        saved,
                        &format!("/articles/{id}/save"),
                        &back,
                    )),
            )
            .child(
                el("figure")
                    .id("article-art")
                    .class("art")
                    .style(&format!("--a: {from}; --b: {to}"))
                    .child(span("plate").attr("aria-hidden", "true"))
                    .child(el("figcaption").text(format!("Illustration: {}", self.brand))),
            )
            .child(div("body").id("article-body").each(paragraphs.iter().enumerate(), |(i, p)| prose(i, p)))
            .when(!related.is_empty(), |s| {
                s.child(
                    el("aside")
                        .id("article-links")
                        .class("readmore")
                        .child(el("h2").id("article-links-heading").text("Read more"))
                        .child(el("ul").each(related.iter().enumerate(), |(i, (label, url))| {
                            el("li").child(link(&format!("article-ref-{i}"), url.as_str(), label.as_str()))
                        })),
                )
            })
            .child(div("tags").id("article-tags").each(web::strings(&a, "tags"), |tag| {
                el("a")
                    .id(format!("article-tag-{tag}"))
                    .class("chip")
                    .attr("href", format!("/tag/{tag}"))
                    .child(span("").id(format!("article-tag-{tag}-text")).text(format!("#{tag}")))
            }));
        let discussion = el("section")
            .id("comments")
            .class("comments")
            .child(el("h2").id("comments-heading").text(match comments.len() {
                0 => "No comments yet".to_owned(),
                1 => "1 comment".to_owned(),
                n => format!("{n} comments"),
            }))
            .child(
                form("comment", format!("/articles/{id}/comments"), "post")
                    .child(
                        text_input("comment-text", "text", "")
                            .attr("aria-label", "Join the discussion")
                            .attr("placeholder", "Join the discussion")
                            .attr("autocomplete", "off"),
                    )
                    .child(button("comment-submit", "Post")),
            )
            .each(comments.iter(), |comment| {
                let cid = web::text(comment, "id");
                let who = web::text(comment, "author");
                let (tint, _) = art_tint(&who);
                div("comment")
                    .id(format!("comment-{cid}"))
                    .child(
                        span("avatar")
                            .attr("aria-hidden", "true")
                            .style(&format!("background-color: {tint}"))
                            .text(initials(&who)),
                    )
                    .child(
                        div("said")
                            .id(format!("comment-{cid}-body"))
                            .child(span("author").id(format!("comment-{cid}-author")).text(who.as_str()))
                            .child(el("p").id(format!("comment-{cid}-text")).text(web::text(comment, "text"))),
                    )
                    .child(self.pill(
                        &format!("comment-{cid}-like"),
                        &format!("▲ {}", num(comment, "likes")),
                        false,
                        &format!("/articles/{id}/comments/{cid}/like"),
                        &back,
                    ))
            });
        let main = el("main").id("article").class("article").child(story).child(discussion).child(self.newsletter());
        self.document(&format!("{title} - {}", self.brand), "page-article", main)
    }
    /// Brand splash; nothing on it reads as a control, so no action is promised that does not exist.
    pub(crate) fn landing(&self) -> Result<HttpResponse> {
        let tagline = match web::text(self.state, "tagline") {
            t if t.is_empty() => TAGLINE.to_owned(),
            t => t,
        };
        let doc = Document::new(self.brand.as_str())
            .lang("en")
            .stylesheet(BASE)
            .stylesheet(sheet(&self.skin))
            .root_style(&self.root)
            .body_class(&format!("skin-{} layout-{} page-landing", self.skin, self.layout))
            .body([el("main")
                .class("landing")
                .child(el("h1").id("brand").text(self.brand.as_str()))
                .child(el("p").id("tagline").text(tagline))]);
        web::html::page(&doc)
    }
}
/// A paragraph with the HTTP URLs in it as links, `article-p<n>-link-<word index>`, the ids
/// the `Page` version gave the links it listed under the paragraph.
fn prose(index: usize, paragraph: &str) -> Node {
    let mut p = el("p").id(format!("article-p{index}"));
    let mut plain = String::new();
    for (i, word) in paragraph.split_whitespace().enumerate() {
        if i > 0 {
            plain.push(' ');
        }
        let url = word.trim_end_matches(['.', ',', ';', ')', ']']);
        let is_link = url
            .split_once("://")
            .is_some_and(|(scheme, rest)| matches!(scheme, "http" | "https") && !rest.is_empty());
        if is_link {
            p = p.child(text(std::mem::take(&mut plain)));
            p = p.child(link(&format!("article-p{index}-link-{i}"), url, url));
            plain.push_str(&word[url.len()..]);
        } else {
            plain.push_str(word);
        }
    }
    if plain.is_empty() {
        p
    } else {
        p.child(text(plain))
    }
}
