//! Encyclopedia: wikipedia.org. Articles carry their own revision log, so an edit made in the
//! simulation is visible in History exactly like a seeded one, and Talk carries the argument
//! about it.
//!
//! Search is the same token scorer the `search` engines use, run over the local corpus only, so
//! `!w` from DuckDuckGo lands on a results page that resolves rather than on a stub.
use cw_protocol::{
    HttpRequest, HttpResponse, PageElement, PageTheme, Result as SimResult, SimError, Style,
};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WikiState {
    pub brand: String,
    pub tagline: String,
    pub theme: PageTheme,
    /// Portal headliner; empty falls back to the first article by id.
    pub featured: String,
    pub in_the_news: Vec<Reference>,
    /// OS actor → wiki username, so an edit made in the simulation signs like a seeded one.
    pub accounts: BTreeMap<String, String>,
    pub articles: BTreeMap<String, Article>,
    pub redirects: BTreeMap<String, String>,
    /// Revision ids are wiki-wide, the way a MediaWiki `oldid` is.
    pub next_rev: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Article {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub infobox: Vec<(String, String)>,
    pub sections: Vec<Section>,
    pub references: Vec<Reference>,
    pub see_also: Vec<String>,
    pub categories: Vec<String>,
    pub revisions: Vec<Revision>,
    pub talk: Vec<TalkPost>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Section {
    pub id: String,
    pub heading: String,
    pub body: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Reference {
    pub label: String,
    pub url: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Revision {
    pub rev: u64,
    pub author: String,
    pub tick: u64,
    pub comment: String,
    /// Which section changed; `null` is the article-level entry a creation leaves.
    pub section: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TalkPost {
    pub author: String,
    pub text: String,
    pub tick: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: i64,
}
/// A redirect chain is data, not a loop: four hops is more than any real one needs.
const HOPS: usize = 4;
const SNIPPET: usize = 180;
/// Same shape as the web scorer: a title match is worth much more than a body mention.
const W_TITLE: i64 = 8;
const W_CATEGORY: i64 = 4;
const W_BODY: i64 = 1;
/// FNV-1a, inline and dependency-free: "Random article" must not read a clock or the world seed.
pub fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
/// Whitespace tokens with punctuation trimmed off the ends only, so `ATLAS-2026` stays one token.
fn tokens(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}
fn snippet(text: &str) -> String {
    match text.char_indices().nth(SNIPPET) {
        Some((at, _)) => format!("{}…", text[..at].trim_end()),
        None => text.to_owned(),
    }
}
/// Titles arrive inside a URL path, where `%XX` is the only escaping a browser applies to one.
fn unescape(raw: &str) -> String {
    let hex = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match (
            bytes[i],
            bytes.get(i + 1).copied().and_then(hex),
            bytes.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(high), Some(low)) => {
                out.push((high << 4) | low);
                i += 3;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
impl Article {
    /// Everything a body-text search should reach, headings included.
    fn prose(&self) -> String {
        let mut text = self.summary.clone();
        for section in &self.sections {
            text.push(' ');
            text.push_str(&section.heading);
            text.push(' ');
            text.push_str(&section.body);
        }
        text
    }
    fn section(&self, id: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.id == id)
    }
    /// The revision that currently stands, i.e. the last one recorded.
    pub fn latest(&self) -> Option<&Revision> {
        self.revisions.last()
    }
}
impl WikiState {
    fn user(&self, actor: &str) -> String {
        self.accounts
            .get(actor)
            .cloned()
            .unwrap_or_else(|| actor.to_owned())
    }
    /// Canonical id for a requested title plus the alias it arrived under, following `redirects`.
    pub fn canonical(&self, title: &str) -> Option<(String, Option<String>)> {
        let mut key = title.trim().replace(' ', "_");
        let mut from = None;
        for _ in 0..HOPS {
            if self.articles.contains_key(&key) {
                return Some((key, from));
            }
            let next = self.redirects.get(&key)?.clone();
            from = Some(key);
            key = next;
        }
        None
    }
    pub fn article(&self, title: &str) -> Option<&Article> {
        let (id, _) = self.canonical(title)?;
        self.articles.get(&id)
    }
    /// Redirect titles pointing here; they are searchable names for the same page.
    fn aliases(&self, id: &str) -> String {
        self.redirects
            .iter()
            .filter(|(_, target)| target.as_str() == id)
            .map(|(alias, _)| alias.replace('_', " "))
            .collect::<Vec<_>>()
            .join(" ")
    }
    /// Ranked hits over titles, redirect aliases, categories and prose. `(-score, id)` is total.
    pub fn search(&self, query: &str) -> Vec<Hit> {
        let tokens = tokens(query);
        if tokens.is_empty() {
            return vec![];
        }
        let mut hits: Vec<Hit> = self
            .articles
            .values()
            .filter_map(|article| {
                let title = format!("{} {}", article.title, self.aliases(&article.id))
                    .replace('_', " ")
                    .to_ascii_lowercase();
                let categories = article.categories.join(" ").to_ascii_lowercase();
                let prose = article.prose().to_ascii_lowercase();
                let matched: i64 = tokens
                    .iter()
                    .map(|t| {
                        W_TITLE * i64::from(title.contains(t.as_str()))
                            + W_CATEGORY * i64::from(categories.contains(t.as_str()))
                            + W_BODY * i64::from(prose.contains(t.as_str()))
                    })
                    .sum();
                (matched > 0).then(|| Hit {
                    id: article.id.clone(),
                    title: article.title.clone(),
                    snippet: snippet(&article.summary),
                    score: matched,
                })
            })
            .collect();
        hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        hits
    }
    /// Deterministic per actor, and `n` walks the corpus instead of rerolling a die.
    pub fn random(&self, actor: &str, n: u64) -> Option<&Article> {
        let ids: Vec<&String> = self.articles.keys().collect();
        let len = ids.len() as u64;
        if len == 0 {
            return None;
        }
        self.articles
            .get(ids[(fnv1a64(actor).wrapping_add(n) % len) as usize])
    }
    /// Replace a section body and push the revision that did it. Rewriting a section with the
    /// same text is refused: History is a log of changes, not of button presses.
    pub fn edit(
        &mut self,
        actor: &str,
        title: &str,
        section: &str,
        body: &str,
        comment: &str,
        tick: u64,
    ) -> Result<Revision, String> {
        let body = body.trim();
        if body.is_empty() {
            return Err("body required".into());
        }
        let (id, _) = self.canonical(title).ok_or("article unavailable")?;
        let rev = self.next_rev.max(1);
        let author = self.user(actor);
        let comment = comment.trim().to_owned();
        let article = self.articles.get_mut(&id).ok_or("article unavailable")?;
        let target = article
            .sections
            .iter_mut()
            .find(|s| s.id == section)
            .ok_or("unknown section")?;
        if target.body == body {
            return Err("edit changes nothing".into());
        }
        let heading = target.heading.clone();
        target.body = body.to_owned();
        let revision = Revision {
            rev,
            author,
            tick,
            comment: match comment.as_str() {
                "" => format!("edited {heading}"),
                c => c.to_owned(),
            },
            section: Some(section.to_owned()),
        };
        article.revisions.push(revision.clone());
        self.next_rev = rev + 1;
        Ok(revision)
    }
    pub fn discuss(
        &mut self,
        actor: &str,
        title: &str,
        text: &str,
        tick: u64,
    ) -> Result<TalkPost, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("text required".into());
        }
        let (id, _) = self.canonical(title).ok_or("article unavailable")?;
        let post = TalkPost {
            author: self.user(actor),
            text: text.to_owned(),
            tick,
        };
        let article = self.articles.get_mut(&id).ok_or("article unavailable")?;
        article.talk.push(post.clone());
        Ok(post)
    }
}
pub struct WikiService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(WikiService)
}
const BRAND: &str = "Wikipedia";
const TAGLINE: &str = "The free encyclopedia";
/// Hairlines are not in `PageTheme`; the encyclopedia's rule colour is part of its look.
const RULE: &str = "#a2a9b1";
struct Palette {
    accent: String,
    background: String,
    surface: String,
    ink: String,
    muted: String,
}
impl Palette {
    fn of(theme: &PageTheme) -> Self {
        let pick = |v: &Option<String>, d: &str| v.clone().unwrap_or_else(|| d.to_owned());
        Self {
            accent: pick(&theme.accent, "#3366cc"),
            background: pick(&theme.background, "#ffffff"),
            surface: pick(&theme.surface, "#f8f9fa"),
            ink: pick(&theme.ink, "#202122"),
            muted: pick(&theme.muted, "#54595d"),
        }
    }
    fn body(&self, id: &str, text: &str) -> PageElement {
        web::styled(id, text, web::style().size(15).color(self.ink.clone()))
    }
    fn small(&self, id: &str, text: impl Into<String>) -> PageElement {
        web::styled(id, text, web::style().size(12).color(self.muted.clone()))
    }
    fn title(&self, id: &str, text: &str, size: u16) -> PageElement {
        web::styled(
            id,
            text,
            web::style().size(size).bold().color(self.ink.clone()),
        )
    }
    fn panel(&self) -> Style {
        web::style()
            .background(self.surface.clone())
            .border(RULE)
            .radius(2)
            .padding(12)
    }
}
fn article_url(id: &str) -> String {
    format!("/wiki/{id}")
}
/// Masthead, search box and footer — every page of the encyclopedia wears them.
fn chrome(s: &WikiState, title: &str, main: Vec<PageElement>) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let brand = match s.brand.as_str() {
        "" => BRAND,
        b => b,
    };
    let label = format!("Search {brand}");
    let mut elements = vec![
        web::styled_row(
            "masthead",
            12,
            "center",
            web::style().padding(12).background(p.surface.clone()),
            vec![
                web::thumbnail(
                    "masthead-logo",
                    "W",
                    web::style()
                        .width(44)
                        .height(44)
                        .radius(22)
                        .background(p.background.clone())
                        .border(RULE)
                        .color(p.ink.clone())
                        .size(20)
                        .flex(0),
                ),
                web::card(
                    "masthead-words",
                    web::style().flex(1),
                    vec![
                        web::styled(
                            "masthead-brand",
                            brand,
                            web::style().size(24).color(p.ink.clone()),
                        ),
                        p.small(
                            "masthead-tagline",
                            match s.tagline.as_str() {
                                "" => TAGLINE,
                                t => t,
                            },
                        ),
                    ],
                ),
                web::card(
                    "masthead-search",
                    web::style().width(320).flex(0),
                    vec![web::form("search", "/search", &[("q", label.as_str(), "")])],
                ),
            ],
        ),
        web::divider("masthead-rule"),
    ];
    elements.extend(main);
    elements.push(web::divider("foot-rule"));
    elements.push(web::styled(
        "foot",
        "A simulated encyclopedia. Text is available under a free licence.",
        web::style().size(11).color(p.muted.clone()).align("center"),
    ));
    web::themed_page(title, s.theme.clone(), elements)
}
/// Portal: featured article, "In the news", and the whole (small) corpus, because a reader who
/// cannot list the articles cannot tell an empty encyclopedia from a broken one.
fn portal(s: &WikiState) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let featured = s
        .article(&s.featured)
        .or_else(|| s.articles.values().next());
    let mut columns = vec![];
    if let Some(article) = featured {
        columns.push(web::card(
            "featured",
            p.panel(),
            vec![
                p.title("featured-label", "From today's featured article", 14),
                web::divider("featured-rule"),
                web::link(
                    "featured-title",
                    article.title.clone(),
                    article_url(&article.id),
                ),
                p.body("featured-summary", &snippet(&article.summary)),
            ],
        ));
    }
    let mut news = vec![
        p.title("news-label", "In the news", 14),
        web::divider("news-rule"),
    ];
    for (i, item) in s.in_the_news.iter().enumerate() {
        news.push(web::link(
            &format!("news-{i}"),
            item.label.clone(),
            item.url.clone(),
        ));
    }
    if s.in_the_news.is_empty() {
        news.push(p.small("news-empty", "Nothing filed today."));
    }
    columns.push(web::card("news", p.panel(), news));
    let mut main = vec![
        web::spacer("portal-lead", 8),
        web::grid("portal", 2, 16, columns),
        web::spacer("portal-gap", 16),
        p.title("all-label", "All articles", 16),
        web::divider("all-rule"),
    ];
    main.push(web::grid(
        "all",
        2,
        12,
        s.articles
            .values()
            .enumerate()
            .map(|(i, article)| {
                web::card_action(
                    &format!("all-{i}"),
                    web::style().padding(8).radius(2),
                    web::visit(article_url(&article.id)),
                    vec![
                        web::styled(
                            &format!("all-{i}-title"),
                            article.title.clone(),
                            web::style().size(15).color(p.accent.clone()),
                        ),
                        p.small(&format!("all-{i}-summary"), snippet(&article.summary)),
                    ],
                )
            })
            .collect(),
    ));
    main.push(web::spacer("portal-tail", 8));
    main.push(web::link(
        "portal-random",
        "Random article",
        "/wiki/Special:Random",
    ));
    chrome(s, &format!("{} — {}", brand_of(s), TAGLINE), main)
}
fn brand_of(s: &WikiState) -> String {
    match s.brand.as_str() {
        "" => BRAND.to_owned(),
        b => b.to_owned(),
    }
}
/// Article, Talk, History — one row of real routes, the current one shown as a badge.
fn tabs(p: &Palette, id: &str, current: &str) -> PageElement {
    let mut children = vec![];
    for (i, (name, url)) in [
        ("Article", article_url(id)),
        ("Talk", format!("/wiki/Talk:{id}")),
        ("History", format!("/wiki/Special:History/{id}")),
    ]
    .into_iter()
    .enumerate()
    {
        children.push(if name == current {
            web::badge(
                &format!("tab-{i}"),
                name,
                web::style()
                    .size(12)
                    .bold()
                    .background(p.surface.clone())
                    .border(RULE)
                    .color(p.ink.clone())
                    .radius(2)
                    .padding(6)
                    .flex(0),
            )
        } else {
            web::link(&format!("tab-{i}"), name, url)
        });
    }
    web::styled_row("tabs", 14, "center", web::style().flex(0), children)
}
fn infobox(p: &Palette, article: &Article) -> PageElement {
    let mut rows = vec![
        web::styled(
            "info-title",
            article.title.clone(),
            web::style()
                .size(14)
                .bold()
                .align("center")
                .color(p.ink.clone()),
        ),
        web::thumbnail(
            "info-image",
            article.title.clone(),
            web::style()
                .height(120)
                .background(p.background.clone())
                .border(RULE)
                .color(p.muted.clone())
                .size(12),
        ),
    ];
    for (i, (key, value)) in article.infobox.iter().enumerate() {
        rows.push(web::row(
            &format!("info-{i}"),
            8,
            "start",
            vec![
                web::styled(
                    &format!("info-{i}-key"),
                    key.clone(),
                    web::style().size(12).bold().color(p.ink.clone()).flex(2),
                ),
                web::styled(
                    &format!("info-{i}-value"),
                    value.clone(),
                    web::style().size(12).color(p.muted.clone()).flex(3),
                ),
            ],
        ));
    }
    web::card("infobox", p.panel().width(280).flex(0), rows)
}
/// The full article: lead, contents, sections, references, see also, categories.
fn article_page(s: &WikiState, requested: &str) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let Some((id, from)) = s.canonical(requested) else {
        return missing();
    };
    let article = &s.articles[&id];
    let mut column = vec![p.title("article-title", &article.title, 28)];
    if let Some(alias) = &from {
        column.push(p.small(
            "article-redirect",
            format!("(Redirected from {})", alias.replace('_', " ")),
        ));
    }
    column.push(web::divider("article-rule"));
    column.push(p.body("article-summary", &article.summary));
    if article.sections.len() > 1 {
        let mut toc = vec![
            p.title("toc-label", "Contents", 13),
            web::divider("toc-rule"),
        ];
        for (i, section) in article.sections.iter().enumerate() {
            toc.push(web::link(
                &format!("toc-{i}"),
                format!("{}. {}", i + 1, section.heading),
                format!("{}?section={}", article_url(&id), section.id),
            ));
        }
        column.push(web::card("toc", p.panel().width(280).flex(0), toc));
    }
    for (i, section) in article.sections.iter().enumerate() {
        column.push(web::row(
            &format!("sec-{i}-head"),
            10,
            "center",
            vec![
                p.title(&format!("sec-{i}-heading"), &section.heading, 20),
                web::link(
                    &format!("sec-{i}-edit"),
                    "edit",
                    format!("{}?section={}", article_url(&id), section.id),
                ),
            ],
        ));
        column.push(web::divider(&format!("sec-{i}-rule")));
        column.push(p.body(&format!("sec-{i}-body"), &section.body));
    }
    if !article.references.is_empty() {
        column.push(p.title("refs-label", "References", 20));
        column.push(web::divider("refs-rule"));
        for (i, reference) in article.references.iter().enumerate() {
            column.push(web::link(
                &format!("ref-{i}"),
                format!("{}. {}", i + 1, reference.label),
                reference.url.clone(),
            ));
        }
    }
    if !article.see_also.is_empty() {
        column.push(p.title("see-label", "See also", 20));
        column.push(web::divider("see-rule"));
        for (i, target) in article.see_also.iter().enumerate() {
            column.push(web::link(
                &format!("see-{i}"),
                target.replace('_', " "),
                article_url(target),
            ));
        }
    }
    if !article.categories.is_empty() {
        column.push(web::spacer("cats-gap", 12));
        column.push(web::styled_row(
            "cats",
            8,
            "center",
            Style::default(),
            std::iter::once(p.small("cats-label", "Categories:"))
                .chain(article.categories.iter().enumerate().map(|(i, category)| {
                    web::badge(
                        &format!("cat-{i}"),
                        category.clone(),
                        web::style()
                            .size(11)
                            .background(p.surface.clone())
                            .border(RULE)
                            .color(p.accent.clone())
                            .radius(2)
                            .padding(4)
                            .flex(0),
                    )
                }))
                .collect(),
        ));
    }
    if let Some(latest) = article.latest() {
        column.push(p.small(
            "article-latest",
            format!(
                "Revision {} · last edited by {} at tick {}",
                latest.rev, latest.author, latest.tick
            ),
        ));
    }
    let main = vec![
        tabs(&p, &id, "Article"),
        web::spacer("article-lead", 8),
        web::row(
            "article-body",
            20,
            "start",
            vec![
                web::card("article-column", web::style().flex(1), column),
                infobox(&p, article),
            ],
        ),
    ];
    chrome(s, &format!("{} — {}", article.title, brand_of(s)), main)
}
/// One section, with the form that rewrites it. Section editing is the mutation this site is for.
fn section_page(s: &WikiState, requested: &str, sid: &str) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let Some((id, _)) = s.canonical(requested) else {
        return missing();
    };
    let article = &s.articles[&id];
    let Some(section) = article.section(sid) else {
        return web::error(404, "unknown section");
    };
    let mut main = vec![
        tabs(&p, &id, "Article"),
        web::spacer("section-lead", 8),
        web::link(
            "section-back",
            format!("← {}", article.title),
            article_url(&id),
        ),
        p.title("section-heading", &section.heading, 24),
        web::divider("section-rule"),
        p.body("section-body", &section.body),
        web::spacer("section-gap", 12),
        p.title("edit-label", "Edit this section", 16),
        web::form(
            "edit",
            &format!("/articles/{id}/sections/{sid}"),
            &[
                ("body", "Section text", section.body.as_str()),
                ("comment", "Edit summary", ""),
            ],
        ),
    ];
    let history: Vec<_> = article
        .revisions
        .iter()
        .filter(|r| r.section.as_deref() == Some(sid))
        .collect();
    if !history.is_empty() {
        main.push(p.title("section-hist-label", "Revisions to this section", 14));
        for revision in history {
            main.push(p.small(
                &format!("section-rev-{}", revision.rev),
                format!(
                    "{} · {} · tick {} · {}",
                    revision.rev, revision.author, revision.tick, revision.comment
                ),
            ));
        }
    }
    chrome(
        s,
        &format!("{}: {} — {}", article.title, section.heading, brand_of(s)),
        main,
    )
}
fn talk_page(s: &WikiState, requested: &str) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let Some((id, _)) = s.canonical(requested) else {
        return missing();
    };
    let article = &s.articles[&id];
    let mut main = vec![
        tabs(&p, &id, "Talk"),
        web::spacer("talk-lead", 8),
        p.title("talk-title", &format!("Talk: {}", article.title), 26),
        web::divider("talk-rule"),
    ];
    for (i, post) in article.talk.iter().enumerate() {
        main.push(web::card(
            &format!("talk-{i}"),
            p.panel(),
            vec![
                web::row(
                    &format!("talk-{i}-head"),
                    8,
                    "center",
                    vec![
                        web::thumbnail(
                            &format!("talk-{i}-avatar"),
                            post.author.chars().next().unwrap_or('?').to_string(),
                            web::style()
                                .width(24)
                                .height(24)
                                .radius(12)
                                .background(p.accent.clone())
                                .color(p.background.clone())
                                .size(12)
                                .flex(0),
                        ),
                        web::styled(
                            &format!("talk-{i}-author"),
                            post.author.clone(),
                            web::style().size(13).bold().color(p.ink.clone()),
                        ),
                        web::badge(
                            &format!("talk-{i}-tick"),
                            format!("tick {}", post.tick),
                            web::style()
                                .size(11)
                                .color(p.muted.clone())
                                .border(RULE)
                                .radius(2)
                                .padding(4)
                                .flex(0),
                        ),
                    ],
                ),
                p.body(&format!("talk-{i}-text"), &post.text),
            ],
        ));
    }
    if article.talk.is_empty() {
        main.push(p.small("talk-empty", "No discussion on this article yet."));
    }
    main.push(web::spacer("talk-gap", 12));
    main.push(web::form(
        "reply",
        &format!("/articles/{id}/talk"),
        &[("text", "Add a topic", "")],
    ));
    chrome(
        s,
        &format!("Talk: {} — {}", article.title, brand_of(s)),
        main,
    )
}
fn history_page(s: &WikiState, requested: &str) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    let Some((id, _)) = s.canonical(requested) else {
        return missing();
    };
    let article = &s.articles[&id];
    let current = article.latest().map(|r| r.rev);
    let mut main = vec![
        tabs(&p, &id, "History"),
        web::spacer("hist-lead", 8),
        p.title(
            "hist-title",
            &format!("Revision history of {}", article.title),
            26,
        ),
        web::divider("hist-rule"),
    ];
    for revision in article.revisions.iter().rev() {
        let target = revision
            .section
            .as_ref()
            .map(|sid| format!("{}?section={sid}", article_url(&id)))
            .unwrap_or_else(|| article_url(&id));
        main.push(web::row(
            &format!("rev-{}", revision.rev),
            10,
            "center",
            vec![
                web::badge(
                    &format!("rev-{}-id", revision.rev),
                    format!("rev {}", revision.rev),
                    web::style()
                        .size(11)
                        .background(p.surface.clone())
                        .border(RULE)
                        .color(p.ink.clone())
                        .radius(2)
                        .padding(4)
                        .flex(0),
                ),
                web::styled(
                    &format!("rev-{}-author", revision.rev),
                    revision.author.clone(),
                    web::style().size(13).bold().color(p.ink.clone()).flex(2),
                ),
                web::styled(
                    &format!("rev-{}-tick", revision.rev),
                    format!("tick {}", revision.tick),
                    web::style().size(12).color(p.muted.clone()).flex(1),
                ),
                web::link(
                    &format!("rev-{}-comment", revision.rev),
                    revision.comment.clone(),
                    target,
                ),
                web::badge(
                    &format!("rev-{}-mark", revision.rev),
                    if current == Some(revision.rev) {
                        "current"
                    } else {
                        "superseded"
                    },
                    web::style()
                        .size(11)
                        .color(p.muted.clone())
                        .radius(2)
                        .padding(4)
                        .flex(0),
                ),
            ],
        ));
    }
    if article.revisions.is_empty() {
        main.push(p.small("hist-empty", "No revisions recorded."));
    }
    chrome(
        s,
        &format!("Revision history of {} — {}", article.title, brand_of(s)),
        main,
    )
}
fn results_page(s: &WikiState, query: &str) -> SimResult<HttpResponse> {
    let p = Palette::of(&s.theme);
    // An exact title hit is what a reader asked for; the results list is the consolation prize.
    if let Some(article) = s.article(query) {
        return article_page(s, &article.id);
    }
    let hits = s.search(query);
    let mut main = vec![
        p.title("results-title", &format!("Search results for {query}"), 24),
        p.small(
            "results-count",
            format!("{} article(s) matched.", hits.len()),
        ),
        web::divider("results-rule"),
    ];
    for (i, hit) in hits.iter().enumerate() {
        main.push(web::card_action(
            &format!("hit-{i}"),
            web::style().padding(8).radius(2),
            web::visit(article_url(&hit.id)),
            vec![
                web::styled(
                    &format!("hit-{i}-title"),
                    hit.title.clone(),
                    web::style().size(16).color(p.accent.clone()),
                ),
                p.small(&format!("hit-{i}-snippet"), hit.snippet.clone()),
            ],
        ));
    }
    if hits.is_empty() {
        main.push(p.small(
            "results-empty",
            "No article matched. Try a different wording.",
        ));
    }
    chrome(s, &format!("{query} — search results"), main)
}
/// A red link in real life; here it is an honest 404 rather than a page pretending to be one.
fn missing() -> SimResult<HttpResponse> {
    web::error(404, "article not found")
}
/// Where a successful mutation leaves the reader when the caller was a page form, not the API.
enum View {
    Section(String, String),
    Talk(String),
}
/// Documented seed keys, checked by container type before the typed load reports anything finer.
const OBJECTS: &[&str] = &["theme", "articles", "redirects", "accounts"];
const ARRAYS: &[&str] = &["in_the_news"];
impl Service for WikiService {
    fn kind(&self) -> &str {
        "wiki"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> SimResult<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        let mut s: WikiState = web::load(&gated)?;
        if s.brand.is_empty() {
            s.brand = BRAND.into();
        }
        if s.tagline.is_empty() {
            s.tagline = TAGLINE.into();
        }
        let keys: Vec<String> = s.articles.keys().cloned().collect();
        for key in &keys {
            let article = s.articles.get_mut(key).expect("key came from the map");
            if article.id.is_empty() {
                article.id = key.clone();
            } else if &article.id != key {
                return Err(SimError::invalid(format!(
                    "article {key} carries id {}",
                    article.id
                )));
            }
            if article.title.is_empty() {
                article.title = key.replace('_', " ");
            }
            let mut ids = std::collections::BTreeSet::new();
            for section in &article.sections {
                if section.id.is_empty() || !ids.insert(section.id.clone()) {
                    return Err(SimError::invalid(format!(
                        "article {key} has an empty or duplicate section id"
                    )));
                }
            }
            for reference in &article.references {
                // A reference is a promise that the source exists; a relative one promises nothing.
                if !reference.url.starts_with("http://") && !reference.url.starts_with("https://") {
                    return Err(SimError::invalid(format!(
                        "article {key} cites a non-absolute url {}",
                        reference.url
                    )));
                }
            }
        }
        for (alias, target) in &s.redirects {
            if !s.articles.contains_key(target) {
                return Err(SimError::invalid(format!(
                    "redirect {alias} points at missing article {target}"
                )));
            }
        }
        for key in &keys {
            for target in &s.articles[key].see_also {
                if s.canonical(target).is_none() {
                    return Err(SimError::invalid(format!(
                        "article {key} sees also missing article {target}"
                    )));
                }
            }
        }
        if !s.featured.is_empty() && s.canonical(&s.featured).is_none() {
            return Err(SimError::invalid(format!(
                "featured article {} does not exist",
                s.featured
            )));
        }
        let highest = s
            .articles
            .values()
            .flat_map(|a| a.revisions.iter().map(|r| r.rev))
            .max()
            .unwrap_or(0);
        s.next_rev = s.next_rev.max(highest + 1);
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<HttpResponse> {
        let mut s: WikiState = web::load(state)?;
        let full = web::path(r);
        let path = full.strip_prefix("/api").unwrap_or(&full);
        let api = full.starts_with("/api/") || full == "/api";
        let parts: Vec<String> = path.trim_matches('/').split('/').map(unescape).collect();
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            let query = web::query(r, "q")
                .or_else(|| web::query(r, "search"))
                .unwrap_or_default();
            return match parts.as_slice() {
                [""] if !api => portal(&s),
                ["wiki", "Special:Random"] if !api => {
                    let n = web::query(r, "n").and_then(|n| n.parse().ok()).unwrap_or(0);
                    match s.random(&c.actor, n) {
                        Some(article) => article_page(&s, &article.id),
                        None => web::error(404, "no articles"),
                    }
                }
                ["wiki", "Special:History", title] if !api => history_page(&s, title),
                ["wiki", title] if !api => match title.strip_prefix("Talk:") {
                    Some(title) => talk_page(&s, title),
                    None => match web::query(r, "section") {
                        Some(sid) => section_page(&s, title, &sid),
                        None => article_page(&s, title),
                    },
                },
                ["search"] | ["w", "index.php"] if !api => match query.trim() {
                    "" => portal(&s),
                    q => results_page(&s, q),
                },
                ["articles"] => HttpResponse::json(200, &s.articles.keys().collect::<Vec<_>>()),
                ["articles", title] => match s.article(title) {
                    Some(article) => HttpResponse::json(200, article),
                    None => web::error(404, "article not found"),
                },
                ["search"] => HttpResponse::json(
                    200,
                    &json!({"query": query, "results": s.search(query.trim())}),
                ),
                ["random"] => match s.random(&c.actor, 0) {
                    Some(article) => HttpResponse::json(200, &json!({"id": article.id})),
                    None => web::error(404, "no articles"),
                },
                _ => web::error(404, "route not found"),
            };
        }
        let b = web::body(r)?;
        // The search box is a POST form, but its result is the same page a GET query reaches.
        if method == "POST" && parts.as_slice() == ["search"] && !api {
            return match web::text(&b, "q").trim() {
                "" => portal(&s),
                q => results_page(&s, q),
            };
        }
        let (result, view) = match (method.as_str(), parts.as_slice()) {
            ("POST", ["articles", title, "sections", sid]) => (
                s.edit(
                    &c.actor,
                    title,
                    sid,
                    &web::text(&b, "body"),
                    &web::text(&b, "comment"),
                    c.tick,
                )
                .map(|v| json!(v)),
                View::Section((*title).to_owned(), (*sid).to_owned()),
            ),
            ("POST", ["articles", title, "talk"]) => (
                s.discuss(&c.actor, title, &web::text(&b, "text"), c.tick)
                    .map(|v| json!(v)),
                View::Talk((*title).to_owned()),
            ),
            _ => return web::error(405, "unsupported route or method"),
        };
        if result.is_err() {
            return web::domain(result);
        }
        web::save(state, &s)?;
        if api {
            return web::domain(result);
        }
        match view {
            View::Section(title, sid) => section_page(&s, &title, &sid),
            View::Talk(title) => talk_page(&s, &title),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const SITE: &str = include_str!("../../../worlds/company-2026/sites/wikipedia.json");
    const ARTICLE: &str = "Deterministic_simulation";
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "bob".into(),
            source: "bob-linux".into(),
            tick: 11,
            seed: 1,
            instance: "wiki".into(),
        }
    }
    fn site() -> Value {
        serde_json::from_str::<Value>(SITE).unwrap()
    }
    fn raw() -> Value {
        WikiService
            .initialize(site()["initial_state"].clone(), &ctx())
            .unwrap()
    }
    fn seeded() -> WikiState {
        serde_json::from_value(raw()).unwrap()
    }
    /// A page the browser would reject is not a page; ids must be unique and colours well formed.
    fn rendered(response: &HttpResponse) -> cw_protocol::Page {
        assert_eq!(response.status, 200);
        let page: cw_protocol::Page = serde_json::from_slice(&response.body).unwrap();
        page.validate().unwrap();
        page
    }
    fn get(state: &mut Value, url: &str) -> HttpResponse {
        WikiService
            .handle(state, &ctx(), &HttpRequest::get(url))
            .unwrap()
    }
    fn post(state: &mut Value, url: &str, body: &str) -> HttpResponse {
        let mut request = HttpRequest::get(url);
        request.method = "POST".into();
        request.body = body.as_bytes().to_vec();
        WikiService.handle(state, &ctx(), &request).unwrap()
    }
    #[test]
    fn every_seeded_page_renders_and_validates() {
        let mut state = raw();
        let s = seeded();
        let before = state.clone();
        rendered(&get(&mut state, "http://wikipedia.org/"));
        for id in s.articles.keys() {
            rendered(&get(&mut state, &format!("http://wikipedia.org/wiki/{id}")));
            rendered(&get(
                &mut state,
                &format!("http://wikipedia.org/wiki/Talk:{id}"),
            ));
            rendered(&get(
                &mut state,
                &format!("http://wikipedia.org/wiki/Special:History/{id}"),
            ));
            for section in &s.articles[id].sections {
                rendered(&get(
                    &mut state,
                    &format!("http://wikipedia.org/wiki/{id}?section={}", section.id),
                ));
            }
        }
        rendered(&get(
            &mut state,
            "http://wikipedia.org/search?q=determinism",
        ));
        assert_eq!(before, state, "rendering must not mutate seed state");
    }
    #[test]
    fn redirects_resolve_and_unknown_titles_are_refused() {
        let mut state = raw();
        let s = seeded();
        assert!(!s.redirects.is_empty(), "the seed ships no redirects");
        for (alias, target) in &s.redirects {
            let page = rendered(&get(
                &mut state,
                &format!("http://wikipedia.org/wiki/{alias}"),
            ));
            let body = serde_json::to_string(&page).unwrap();
            assert!(body.contains(&s.articles[target].title));
            assert!(body.contains("Redirected from"));
        }
        assert_eq!(
            get(&mut state, "http://wikipedia.org/wiki/Nonexistent_thing").status,
            404
        );
        assert_eq!(get(&mut state, "http://wikipedia.org/nope").status, 404);
    }
    #[test]
    fn editing_a_section_pushes_a_revision_that_history_shows() {
        let mut state = raw();
        let before = state.clone();
        let url = format!("http://wikipedia.org/articles/{ARTICLE}/sections/history");
        let page = rendered(&post(
            &mut state,
            &url,
            r#"{"body":"Rewritten by the test.","comment":"tighten wording"}"#,
        ));
        assert!(serde_json::to_string(&page)
            .unwrap()
            .contains("Rewritten by the test."));
        assert_ne!(before, state, "an edit must move real state");
        let s: WikiState = serde_json::from_value(state.clone()).unwrap();
        let article = &s.articles[ARTICLE];
        let latest = article.latest().unwrap();
        // The seed maps actor `bob` to his wiki username; an in-sim edit signs like a seeded one.
        assert_eq!(latest.author, "bmartinez");
        assert_eq!(latest.tick, 11);
        assert_eq!(latest.section.as_deref(), Some("history"));
        let next = seeded().next_rev;
        assert_eq!(latest.rev, next, "revision ids are wiki-wide and monotonic");
        assert_eq!(s.next_rev, next + 1);
        let history = rendered(&get(
            &mut state,
            &format!("http://wikipedia.org/wiki/Special:History/{ARTICLE}"),
        ));
        let body = serde_json::to_string(&history).unwrap();
        assert!(body.contains("tighten wording") && body.contains("current"));
        // Refusals: no such section, empty body, and a rewrite that changes nothing.
        assert_eq!(
            post(
                &mut state,
                &format!("http://wikipedia.org/api/articles/{ARTICLE}/sections/nope"),
                r#"{"body":"x"}"#
            )
            .status,
            400
        );
        assert_eq!(post(&mut state, &url, r#"{"body":"  "}"#).status, 400);
        assert_eq!(
            post(&mut state, &url, r#"{"body":"Rewritten by the test."}"#).status,
            400,
            "History logs changes, not button presses"
        );
        let restored: WikiState = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(s, restored, "state must survive a snapshot round trip");
    }
    #[test]
    fn talk_accepts_a_topic_and_refuses_an_empty_one() {
        let mut state = raw();
        let url = format!("http://wikipedia.org/articles/{ARTICLE}/talk");
        let page = rendered(&post(&mut state, &url, r#"{"text":"Sources, please."}"#));
        let body = serde_json::to_string(&page).unwrap();
        assert!(body.contains("Sources, please.") && body.contains("bmartinez"));
        assert_eq!(post(&mut state, &url, r#"{"text":" "}"#).status, 400);
        assert_eq!(
            post(
                &mut state,
                "http://wikipedia.org/articles/Missing/talk",
                r#"{"text":"hello"}"#
            )
            .status,
            403
        );
        let s: WikiState = serde_json::from_value(state).unwrap();
        assert_eq!(s.articles[ARTICLE].talk.last().unwrap().tick, 11);
    }
    #[test]
    fn search_reaches_titles_redirects_and_prose() {
        let s = seeded();
        let top = |q: &str| {
            s.search(q)
                .first()
                .map(|h| h.id.clone())
                .unwrap_or_default()
        };
        assert_eq!(top("deterministic simulation"), ARTICLE);
        assert_eq!(top("determinism computing"), ARTICLE, "redirects are names");
        assert_eq!(top("Atlas"), "Atlas_(software)");
        assert!(s.search("zzzznothing").is_empty());
        assert!(s.search("   ").is_empty());
        // Ranking is a total order, so two runs over the same corpus never disagree.
        assert_eq!(
            s.search("simulation")
                .iter()
                .map(|h| h.id.clone())
                .collect::<Vec<_>>(),
            s.search("simulation")
                .iter()
                .map(|h| h.id.clone())
                .collect::<Vec<_>>()
        );
        let mut state = raw();
        // The `!w` bang from DuckDuckGo and the MediaWiki URL both land on the same results.
        let one = get(&mut state, "http://wikipedia.org/search?q=criticism");
        let two = get(
            &mut state,
            "http://wikipedia.org/w/index.php?search=criticism",
        );
        assert_eq!(one, two);
        rendered(&one);
        let posted = post(
            &mut state,
            "http://wikipedia.org/search",
            r#"{"q":"criticism"}"#,
        );
        assert_eq!(posted, one, "the form and the URL are the same query");
    }
    #[test]
    fn random_article_is_deterministic_per_actor() {
        let s = seeded();
        let pick = |actor: &str, n: u64| s.random(actor, n).unwrap().id.clone();
        assert_eq!(pick("alice", 0), pick("alice", 0));
        let walk: Vec<_> = (0..s.articles.len() as u64)
            .map(|n| pick("alice", n))
            .collect();
        let mut sorted = walk.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), s.articles.len(), "?n= must walk the corpus");
    }
    /// Storyline 8: the edit war is in the seed, and the article cites the story that started it.
    #[test]
    fn the_edit_war_is_seeded_on_the_determinism_article() {
        let s = seeded();
        let article = &s.articles[ARTICLE];
        assert_eq!(article.revisions.len(), 5);
        assert_eq!(article.revisions[2].author, "bmartinez");
        assert!(article.revisions[2]
            .comment
            .to_lowercase()
            .contains("criticism"));
        assert!(article.revisions[3].comment.to_lowercase().contains("rv"));
        assert!(
            !article.sections.iter().any(|s| s.id == "criticism"),
            "revision 4 reverted the Criticism section, so it must be gone"
        );
        assert!(article.talk.len() >= 3, "the argument lives on Talk");
        assert!(article
            .references
            .iter()
            .any(|r| r.url.contains("theverge.com/2026/atlas-determinism")));
        // The release code is storyline 1's to hold; the encyclopedia must not leak it.
        assert!(!SITE.contains("ATLAS-2026"));
    }
    #[test]
    fn seeded_search_entries_resolve_to_real_pages() {
        let site = site();
        let mut state = raw();
        let entries = site["search_entries"].as_array().unwrap();
        assert!(
            entries.len() >= seeded().articles.len(),
            "every article must be indexed"
        );
        for entry in entries {
            let url = entry["url"].as_str().unwrap();
            assert!(!entry["title"].as_str().unwrap_or("").trim().is_empty());
            assert_eq!(
                get(&mut state, url).status,
                200,
                "indexed {url} does not resolve"
            );
        }
    }
    /// A site can be bound to a node before its content lands; an empty encyclopedia is a page.
    #[test]
    fn an_empty_encyclopedia_still_serves_a_themed_portal() {
        let mut state = WikiService.initialize(json!({}), &ctx()).unwrap();
        let page = rendered(&get(&mut state, "http://wikipedia.org/"));
        assert!(page.theme.is_some());
        assert_eq!(page.title, "Wikipedia — The free encyclopedia");
        assert_eq!(
            get(&mut state, "http://wikipedia.org/wiki/Anything").status,
            404
        );
        assert_eq!(
            get(&mut state, "http://wikipedia.org/wiki/Special:Random").status,
            404
        );
    }
    #[test]
    fn seed_shape_is_gated_at_load() {
        assert!(WikiService.initialize(json!([]), &ctx()).is_err());
        assert!(WikiService
            .initialize(json!({"articles": []}), &ctx())
            .is_err());
        assert!(
            WikiService
                .initialize(json!({"redirects": {"A": "Missing"}}), &ctx())
                .is_err(),
            "a redirect into nothing is a broken link at load"
        );
        assert!(
            WikiService
                .initialize(
                    json!({"articles": {"A": {"references": [{"label": "x", "url": "/rel"}]}}}),
                    &ctx()
                )
                .is_err(),
            "a relative citation promises nothing"
        );
        assert!(
            WikiService
                .initialize(json!({"articles": {"A": {"see_also": ["B"]}}}), &ctx())
                .is_err(),
            "see-also must resolve inside the encyclopedia"
        );
        assert!(WikiService
            .initialize(json!({"articles": {"A": {"id": "B"}}}), &ctx())
            .is_err());
        assert!(WikiService.initialize(Value::Null, &ctx()).is_ok());
    }
}
