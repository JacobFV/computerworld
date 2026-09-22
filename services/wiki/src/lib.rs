//! Encyclopedia: wikipedia.org. Articles carry their own revision log, so an edit made in the
//! simulation is visible in History exactly like a seeded one, and Talk carries the argument
//! about it.
//!
//! Search is the same token scorer the `search` engines use, run over the local corpus only, so
//! `!w` from DuckDuckGo lands on a results page that resolves rather than on a stub.
use cw_protocol::{HttpRequest, HttpResponse, PageTheme, Result as SimResult, SimError};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
mod view;
use view::{
    article_page, category_page, history_page, portal, random_page, results_page, section_page,
    talk_page,
};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WikiState {
    pub brand: String,
    pub tagline: String,
    pub theme: PageTheme,
    /// One of [`SKINS`]; empty is read off the brand, so a seed written before skins keeps
    /// the look of the site it stands in for.
    pub skin: String,
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
    /// The skin this instance wears: the seeded one, or the one its brand implies.
    pub fn skin_name(&self) -> &'static str {
        let brand = self.brand.to_ascii_lowercase();
        match self.skin.as_str() {
            "imdb" => "imdb",
            "archive" => "archive",
            "" if brand.contains("imdb") => "imdb",
            "" if brand.contains("archive") => "archive",
            _ => "vector",
        }
    }
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
/// The looks one set of routes can wear: wikipedia.org, imdb.com, archive.org.
pub const SKINS: &[&str] = &["vector", "imdb", "archive"];
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
        if !s.skin.is_empty() && !SKINS.contains(&s.skin.as_str()) {
            return Err(SimError::invalid(format!(
                "unknown skin {}; expected one of {}",
                s.skin,
                SKINS.join(", ")
            )));
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
                    let n: u64 = web::query(r, "n").and_then(|n| n.parse().ok()).unwrap_or(0);
                    match s.random(&c.actor, n) {
                        // The page a pick landed on carries the next step of the walk, so
                        // pressing Random again reaches a different article rather than this
                        // one over and over.
                        Some(article) => random_page(&s, &article.id, n.wrapping_add(1)),
                        None => web::error(404, "no articles"),
                    }
                }
                ["wiki", "Special:History", title] if !api => history_page(&s, title),
                ["wiki", title] if !api => {
                    match (title.strip_prefix("Talk:"), title.strip_prefix("Category:")) {
                        (Some(title), _) => talk_page(&s, title),
                        (_, Some(name)) => category_page(&s, name),
                        _ => match web::query(r, "section") {
                            Some(sid) => section_page(&s, title, &sid),
                            None => article_page(&s, title),
                        },
                    }
                }
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
    /// A parsed HTML page and the queries the tests make of it.
    struct Html {
        doc: cw_web::dom::Document,
    }
    impl Html {
        fn node(&self, id: &str) -> cw_web::dom::NodeId {
            *self
                .doc
                .by_id(id)
                .first()
                .unwrap_or_else(|| panic!("no element #{id}"))
        }
        fn has(&self, id: &str) -> bool {
            !self.doc.by_id(id).is_empty()
        }
        fn text(&self, id: &str) -> String {
            self.doc.text_content(self.node(id))
        }
        fn attr(&self, id: &str, name: &str) -> String {
            self.doc
                .attr(self.node(id), name)
                .unwrap_or_default()
                .to_owned()
        }
        fn tag(&self, id: &str) -> String {
            self.doc.tag(self.node(id)).unwrap_or_default().to_owned()
        }
        fn all_text(&self) -> String {
            self.doc.text_content(cw_web::dom::Document::ROOT)
        }
        fn title(&self) -> String {
            let root = cw_web::dom::Document::ROOT;
            let node = self
                .doc
                .descendants(root)
                .find(|n| self.doc.is(*n, "title"))
                .unwrap();
            self.doc.text_content(node)
        }
    }
    /// A page the engine would refuse is not a page: HTML media type, strict CSS, unique ids.
    fn rendered(response: &HttpResponse) -> Html {
        assert_eq!(response.status, 200);
        assert_eq!(
            response.header("content-type"),
            Some(web::html::HTML_MEDIA_TYPE)
        );
        let html = std::str::from_utf8(&response.body).unwrap();
        web::html::validate_strict(html).unwrap_or_else(|e| panic!("strict validation: {e:?}"));
        Html {
            doc: cw_web::html::parse(html),
        }
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
            assert_eq!(page.text("article-title"), s.articles[target].title);
            assert!(page.text("article-redirect").contains("Redirected from"));
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
        assert_eq!(page.text("section-body"), "Rewritten by the test.");
        assert_eq!(page.text("edit-body"), "Rewritten by the test.");
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
        assert_eq!(
            history.text(&format!("rev-{next}-comment")),
            "tighten wording"
        );
        assert_eq!(history.text(&format!("rev-{next}-mark")), "current");
        assert_eq!(
            history.attr(&format!("rev-{next}-comment"), "href"),
            format!("/wiki/{ARTICLE}?section=history")
        );
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
        let last = seeded().articles[ARTICLE].talk.len();
        assert_eq!(page.text(&format!("talk-{last}-text")), "Sources, please.");
        assert_eq!(page.text(&format!("talk-{last}-author")), "bmartinez");
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
        assert_eq!(page.title(), "Wikipedia — The free encyclopedia");
        assert_eq!(page.text("news-empty"), "Nothing filed today.");
        assert!(page.has("search") && !page.has("featured") && !page.has("nav-random"));
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
    const SKINNED: [(&str, &str, &str); 3] = [
        (
            "vector",
            "wikipedia.org",
            include_str!("../../../worlds/company-2026/sites/wikipedia.json"),
        ),
        (
            "imdb",
            "imdb.com",
            include_str!("../../../worlds/company-2026/sites/imdb.json"),
        ),
        (
            "archive",
            "archive.org",
            include_str!("../../../worlds/company-2026/sites/archive.json"),
        ),
    ];
    /// Every page of every site: strict HTML and CSS, in the skin its seed names.
    #[test]
    fn every_page_of_every_skin_validates_strictly() {
        for (skin, host, seed) in SKINNED {
            let site: Value = serde_json::from_str(seed).unwrap();
            let mut state = WikiService
                .initialize(site["initial_state"].clone(), &ctx())
                .unwrap();
            let s: WikiState = serde_json::from_value(state.clone()).unwrap();
            assert_eq!(s.skin_name(), skin, "{host}");
            let before = state.clone();
            let mut urls = vec![
                format!("http://{host}/"),
                format!("http://{host}/search?q=the"),
                format!("http://{host}/search?q=zzzznothing"),
                format!("http://{host}/wiki/Special:Random"),
            ];
            for alias in s.redirects.keys() {
                urls.push(format!("http://{host}/wiki/{alias}"));
            }
            for (id, article) in &s.articles {
                urls.push(format!("http://{host}/wiki/{id}"));
                urls.push(format!("http://{host}/wiki/Talk:{id}"));
                urls.push(format!("http://{host}/wiki/Special:History/{id}"));
                for section in &article.sections {
                    urls.push(format!("http://{host}/wiki/{id}?section={}", section.id));
                }
            }
            for url in urls {
                let page = rendered(&get(&mut state, &url));
                assert!(
                    std::str::from_utf8(&get(&mut state, &url).body)
                        .unwrap()
                        .contains(&format!("skin-{skin}")),
                    "{url}"
                );
                // The chrome every page wears: the search form posts `q` to /search.
                assert_eq!(page.tag("search"), "form", "{url}");
                assert_eq!(page.attr("search", "action"), "/search");
                assert_eq!(page.attr("search", "method"), "post");
                assert_eq!(page.attr("search-q", "name"), "q");
                assert_eq!(page.tag("search-submit"), "button");
                assert_eq!(page.attr("masthead-logo", "href"), "/");
                assert!(
                    page.has("masthead-brand") && page.has("masthead-tagline") && page.has("foot")
                );
            }
            for entry in site["search_entries"].as_array().unwrap() {
                let url = entry["url"].as_str().unwrap();
                assert_eq!(
                    get(&mut state, url).status,
                    200,
                    "indexed {url} does not resolve"
                );
            }
            assert_eq!(before, state, "rendering must not mutate seed state");
        }
        assert!(WikiService
            .initialize(json!({"skin": "monobook"}), &ctx())
            .is_err());
        let explicit = WikiService
            .initialize(json!({"skin": "imdb"}), &ctx())
            .unwrap();
        let s: WikiState = serde_json::from_value(explicit).unwrap();
        assert_eq!(s.skin_name(), "imdb");
    }
    /// The ids the `Page` version exposed are on the elements that play the same roles.
    #[test]
    fn ids_forms_and_links_are_the_agent_api() {
        let mut state = raw();
        let s = seeded();
        let portal = rendered(&get(&mut state, "http://wikipedia.org/"));
        assert_eq!(
            portal.text("featured-label"),
            "From today's featured article"
        );
        assert_eq!(
            portal.attr("featured-title", "href"),
            format!("/wiki/{ARTICLE}")
        );
        assert!(
            portal.has("featured-summary") && portal.has("news-label") && portal.has("all-label")
        );
        assert_eq!(portal.attr("news-0", "href"), s.in_the_news[0].url);
        for (i, article) in s.articles.values().enumerate() {
            assert_eq!(portal.tag(&format!("all-{i}")), "a");
            assert_eq!(
                portal.attr(&format!("all-{i}"), "href"),
                format!("/wiki/{}", article.id)
            );
            assert_eq!(portal.text(&format!("all-{i}-title")), article.title);
            assert!(portal.has(&format!("all-{i}-summary")));
        }
        assert_eq!(portal.attr("portal-random", "href"), "/wiki/Special:Random");

        let article = &s.articles[ARTICLE];
        let page = rendered(&get(
            &mut state,
            &format!("http://wikipedia.org/wiki/{ARTICLE}"),
        ));
        assert_eq!(page.title(), format!("{} — Wikipedia", article.title));
        assert_eq!(page.text("article-title"), article.title);
        assert_eq!(page.text("article-summary"), article.summary);
        for (i, (name, url)) in [
            ("Article", format!("/wiki/{ARTICLE}")),
            ("Talk", format!("/wiki/Talk:{ARTICLE}")),
            ("History", format!("/wiki/Special:History/{ARTICLE}")),
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(page.text(&format!("tab-{i}")), *name);
            assert_eq!(page.attr(&format!("tab-{i}"), "href"), *url);
        }
        assert_eq!(page.attr("tab-0", "aria-current"), "page");
        assert_eq!(page.text("toc-label"), "Contents");
        for (i, section) in article.sections.iter().enumerate() {
            let target = format!("/wiki/{ARTICLE}?section={}", section.id);
            assert_eq!(page.attr(&format!("toc-{i}"), "href"), target);
            assert!(page.text(&format!("toc-{i}")).ends_with(&section.heading));
            assert_eq!(page.text(&format!("sec-{i}-heading")), section.heading);
            assert_eq!(page.text(&format!("sec-{i}-body")), section.body);
            assert_eq!(page.attr(&format!("sec-{i}-edit"), "href"), target);
            assert!(page.has(&format!("sec-{i}-head")));
        }
        for (i, reference) in article.references.iter().enumerate() {
            assert_eq!(page.attr(&format!("ref-{i}"), "href"), reference.url);
            assert_eq!(page.text(&format!("ref-{i}")), reference.label);
        }
        for (i, target) in article.see_also.iter().enumerate() {
            assert_eq!(
                page.attr(&format!("see-{i}"), "href"),
                format!("/wiki/{target}")
            );
        }
        for (i, category) in article.categories.iter().enumerate() {
            assert_eq!(page.text(&format!("cat-{i}")), *category);
        }
        assert_eq!(page.text("info-title"), article.title);
        assert!(page.has("infobox") && page.has("info-image") && page.has("article-latest"));
        assert!(page.has("article-body") && page.has("article-column"));
        for (i, (key, value)) in article.infobox.iter().enumerate() {
            assert_eq!(page.text(&format!("info-{i}-key")), *key);
            assert_eq!(page.text(&format!("info-{i}-value")), *value);
        }

        let section = rendered(&get(
            &mut state,
            &format!("http://wikipedia.org/wiki/{ARTICLE}?section=history"),
        ));
        assert_eq!(
            section.attr("section-back", "href"),
            format!("/wiki/{ARTICLE}")
        );
        assert!(section.has("section-heading") && section.has("edit-label"));
        assert_eq!(section.tag("edit"), "form");
        assert_eq!(
            section.attr("edit", "action"),
            format!("/articles/{ARTICLE}/sections/history")
        );
        assert_eq!(section.attr("edit", "method"), "post");
        assert_eq!(section.tag("edit-body"), "textarea");
        assert_eq!(section.attr("edit-body", "name"), "body");
        assert_eq!(
            section.text("edit-body"),
            article.section("history").unwrap().body
        );
        assert_eq!(section.attr("edit-comment", "name"), "comment");
        assert_eq!(section.tag("edit-submit"), "button");

        let talk = rendered(&get(
            &mut state,
            &format!("http://wikipedia.org/wiki/Talk:{ARTICLE}"),
        ));
        assert_eq!(talk.text("talk-title"), format!("Talk: {}", article.title));
        for (i, post) in article.talk.iter().enumerate() {
            assert_eq!(talk.text(&format!("talk-{i}-author")), post.author);
            assert_eq!(talk.text(&format!("talk-{i}-text")), post.text);
            assert_eq!(
                talk.text(&format!("talk-{i}-tick")),
                format!("tick {}", post.tick)
            );
            assert!(talk.has(&format!("talk-{i}")) && talk.has(&format!("talk-{i}-avatar")));
        }
        assert_eq!(
            talk.attr("reply", "action"),
            format!("/articles/{ARTICLE}/talk")
        );
        assert_eq!(talk.attr("reply", "method"), "post");
        assert_eq!(talk.attr("reply-text", "name"), "text");
        assert_eq!(talk.tag("reply-submit"), "button");
        assert_eq!(talk.attr("tab-1", "aria-current"), "page");

        let history = rendered(&get(
            &mut state,
            &format!("http://wikipedia.org/wiki/Special:History/{ARTICLE}"),
        ));
        assert!(history.has("hist-title"));
        for revision in &article.revisions {
            let rev = revision.rev;
            assert_eq!(history.text(&format!("rev-{rev}-id")), format!("rev {rev}"));
            assert_eq!(history.text(&format!("rev-{rev}-author")), revision.author);
            assert_eq!(
                history.text(&format!("rev-{rev}-comment")),
                revision.comment
            );
            assert!(history.has(&format!("rev-{rev}")) && history.has(&format!("rev-{rev}-tick")));
        }

        let results = rendered(&get(&mut state, "http://wikipedia.org/search?q=simulation"));
        assert_eq!(
            results.text("results-title"),
            "Search results for simulation"
        );
        let hits = s.search("simulation");
        assert!(results
            .text("results-count")
            .starts_with(&hits.len().to_string()));
        for (i, hit) in hits.iter().enumerate() {
            assert_eq!(
                results.attr(&format!("hit-{i}"), "href"),
                format!("/wiki/{}", hit.id)
            );
            assert_eq!(results.text(&format!("hit-{i}-title")), hit.title);
            assert_eq!(results.text(&format!("hit-{i}-snippet")), hit.snippet);
        }
        let none = rendered(&get(
            &mut state,
            "http://wikipedia.org/search?q=zzzznothing",
        ));
        assert!(none.has("results-empty"));
        // A form post from the browser arrives form-encoded and lands on the same page.
        let mut request = HttpRequest::get("http://wikipedia.org/search");
        request.method = "POST".into();
        request.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        request.body = b"q=simulation".to_vec();
        let posted = WikiService.handle(&mut state, &ctx(), &request).unwrap();
        assert_eq!(rendered(&posted).all_text(), results.all_text());
    }
    /// IMDb's title page: rating, credits linked to people, a cast grid; the Archive's item
    /// page: the theatre and the collection navigation.
    #[test]
    fn the_imdb_and_archive_skins_add_their_own_furniture() {
        let site: Value = serde_json::from_str(SKINNED[1].2).unwrap();
        let mut state = WikiService
            .initialize(site["initial_state"].clone(), &ctx())
            .unwrap();
        let page = rendered(&get(&mut state, "http://imdb.com/wiki/Northbound_Signal"));
        assert!(page.text("rating").contains("8.1"));
        assert_eq!(page.attr("cast-0", "href"), "/wiki/Tobias_Renard");
        assert_eq!(
            page.attr("info-1-value-link-0", "href"),
            "/wiki/Ilse_Marchetti"
        );
        assert_eq!(page.text("info-1-value"), "Ilse Marchetti");
        assert_eq!(page.attr("nav-top", "href"), "/wiki/Top_rated");
        let home = rendered(&get(&mut state, "http://imdb.com/"));
        assert_eq!(home.text("featured-label"), "Featured today");
        assert_eq!(home.attr("nav-random", "href"), "/wiki/Special:Random");
        let site: Value = serde_json::from_str(SKINNED[2].2).unwrap();
        let mut state = WikiService
            .initialize(site["initial_state"].clone(), &ctx())
            .unwrap();
        let item = rendered(&get(&mut state, "http://archive.org/wiki/Cavern_Runner_98"));
        assert_eq!(item.text("info-image"), "Cavern Runner 98");
        assert_eq!(
            item.attr("info-0-value-link-0", "href"),
            "/wiki/Software_Library"
        );
        assert_eq!(item.attr("nav-col-0", "href"), "/wiki/Live_Music_Archive");
        assert_eq!(item.attr("nav-site-0", "href"), "/wiki/About");
    }
}
