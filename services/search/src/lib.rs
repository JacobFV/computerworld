//! Web search: google.com, bing.com and duckduckgo.com are three instances of one engine over
//! one index, differing by skin, ranking weights, bangs and whether history is retained.
//!
//! The index is static and generated — `scripts/build-search-index.mjs` splices every other
//! site's `search_entries` into `documents` — because `Service::initialize` has no network
//! handle to crawl with. Every engine therefore has to work with an index of any size, empty
//! included, and a seeded entry is a promise that the link resolves, gated at load below.
mod view;
use cw_protocol::{HttpRequest, HttpResponse, Result, SimError};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub struct SearchService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(SearchService)
}
/// Documented seed keys, checked by container type only — the seed owns the rest.
const OBJECTS: &[&str] = &["theme", "rank_weights", "bangs", "history"];
const ARRAYS: &[&str] = &["verticals", "documents", "footer", "trending"];
/// `skin` is the documented discriminant; an unlisted value is a seed typo, not a fallback.
const SKINS: &[&str] = &["plain", "google", "bing", "ddg"];
const VERTICALS: &[&str] = &["all", "images", "news", "videos"];
const HISTORY: usize = 10;
/// One indexed page. `site` and `vertical` are filled in by the index build, so both default.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Document {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub site: String,
    #[serde(default = "everything")]
    pub vertical: String,
    #[serde(default)]
    pub authority: i64,
    #[serde(default)]
    pub keywords: Vec<String>,
}
fn everything() -> String {
    VERTICALS[0].into()
}
#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    #[serde(flatten)]
    pub document: Document,
    pub score: i64,
}
/// Per-engine ranking weights. Two engines with the same index and different weights disagree,
/// which is the entire point of shipping three of them.
struct Weights {
    title: i64,
    keywords: i64,
    snippet: i64,
    authority: i64,
}
impl Weights {
    fn read(state: &Value) -> Self {
        let at = |key: &str, fallback: i64| {
            state
                .get("rank_weights")
                .and_then(|w| w.get(key))
                .and_then(Value::as_i64)
                .unwrap_or(fallback)
        };
        Self {
            title: at("title", 8),
            keywords: at("keywords", 4),
            snippet: at("snippet", 1),
            authority: at("authority", 2),
        }
    }
}
/// Whitespace tokens with punctuation trimmed off the ends only: `ATLAS-2026` stays one token,
/// so a release code is still an exact search rather than a match on every page saying "atlas".
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
/// `None` for a document no token touched. Authority lifts a page that already matched; added
/// unconditionally it would make every document a hit for every query.
fn score(document: &Document, tokens: &[String], weights: &Weights) -> Option<i64> {
    let title = document.title.to_ascii_lowercase();
    let snippet = document.snippet.to_ascii_lowercase();
    let keywords: Vec<String> = document
        .keywords
        .iter()
        .map(|k| k.to_ascii_lowercase())
        .collect();
    let matched: i64 = tokens
        .iter()
        .map(|token| {
            weights.title * i64::from(title.contains(token.as_str()))
                + weights.keywords * i64::from(keywords.iter().any(|k| k.contains(token.as_str())))
                + weights.snippet * i64::from(snippet.contains(token.as_str()))
        })
        .sum();
    (matched > 0).then(|| matched + weights.authority * document.authority)
}
pub fn documents(state: &Value) -> Vec<Document> {
    state
        .get("documents")
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(|d| serde_json::from_value(d.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}
fn verticals(state: &Value) -> Vec<String> {
    match web::strings(state, "verticals") {
        v if v.is_empty() => VERTICALS.iter().map(|v| (*v).to_owned()).collect(),
        v => v,
    }
}
/// Ranked hits. Sorting by `(-score, url)` is a total order, so two runs never disagree.
pub fn rank(state: &Value, query: &str, vertical: &str) -> Vec<Hit> {
    let tokens = tokens(query);
    if tokens.is_empty() {
        return vec![];
    }
    let weights = Weights::read(state);
    let mut hits: Vec<Hit> = documents(state)
        .into_iter()
        .filter(|d| vertical == VERTICALS[0] || d.vertical == vertical)
        .filter_map(|d| score(&d, &tokens, &weights).map(|score| Hit { document: d, score }))
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.document.url.cmp(&b.document.url))
    });
    hits
}
/// A `!tag` shortcut. Unknown tags are reported rather than swallowed, so a typo is visible.
pub(crate) enum Bang {
    Jump {
        tag: String,
        title: String,
        url: String,
    },
    Unknown {
        tag: String,
        rest: String,
    },
}
fn bang(state: &Value, query: &str) -> Option<Bang> {
    let prefix = state
        .get("bang_prefix")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())?;
    let (head, rest) = match query.split_once(char::is_whitespace) {
        Some((head, rest)) => (head, rest.trim()),
        None => (query, ""),
    };
    let tag = head.strip_prefix(prefix)?.to_ascii_lowercase();
    if tag.is_empty() {
        return None;
    }
    let Some(entry) = state.get("bangs").and_then(|b| b.get(&tag)) else {
        return Some(Bang::Unknown {
            tag,
            rest: rest.to_owned(),
        });
    };
    let template = match entry {
        Value::String(t) => t.clone(),
        other => web::text(other, "template"),
    };
    let title = match entry {
        Value::String(_) => tag.clone(),
        other => web::text(other, "title"),
    };
    Some(Bang::Jump {
        url: template.replace("{}", &enc(rest)),
        title: if title.is_empty() { tag.clone() } else { title },
        tag,
    })
}
pub(crate) fn enc(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
fn retains(state: &Value) -> bool {
    state
        .get("retain_history")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}
pub(crate) fn recent(state: &Value, actor: &str) -> Vec<String> {
    state
        .get("history")
        .and_then(|h| h.get(actor))
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
/// Most-recent-first, deduplicated, bounded. DuckDuckGo opts out entirely.
fn remember(state: &mut Value, actor: &str, query: &str) {
    if !retains(state) {
        return;
    }
    let Some(root) = state.as_object_mut() else {
        return;
    };
    let Some(history) = root
        .entry("history")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        return;
    };
    let Some(list) = history
        .entry(actor)
        .or_insert_with(|| json!([]))
        .as_array_mut()
    else {
        return;
    };
    list.retain(|seen| seen.as_str() != Some(query));
    list.insert(0, json!(query));
    list.truncate(HISTORY);
}
fn forget(state: &mut Value, actor: &str) -> usize {
    let cleared = recent(state, actor).len();
    if let Some(history) = state.get_mut("history").and_then(Value::as_object_mut) {
        history.remove(actor);
    }
    cleared
}
/// GET carries the query in the URL, the form POSTs it; both reach the same results page.
fn asked(request: &HttpRequest) -> Result<(String, String)> {
    let form = if request.method.eq_ignore_ascii_case("POST") {
        web::body(request)?
    } else {
        json!({
            "q": web::query(request, "q").unwrap_or_default(),
            "v": web::query(request, "v").unwrap_or_default(),
        })
    };
    let vertical = match web::text(&form, "v") {
        v if v.is_empty() => VERTICALS[0].to_owned(),
        v => v,
    };
    Ok((web::text(&form, "q").trim().to_owned(), vertical))
}
fn search(state: &mut Value, ctx: &ServiceContext, request: &HttpRequest) -> Result<HttpResponse> {
    let (query, vertical) = asked(request)?;
    if !verticals(state).contains(&vertical) {
        return web::error(400, format!("unknown vertical {vertical}"));
    }
    if query.is_empty() {
        return view::home(state, &ctx.actor);
    }
    remember(state, &ctx.actor, &query);
    match bang(state, &query) {
        Some(Bang::Jump { tag, title, url }) => view::jump(state, &query, &tag, &title, &url),
        Some(Bang::Unknown { tag, rest }) => {
            let hits = rank(state, &rest, &vertical);
            let note = format!("No !{tag} shortcut is configured; showing web results instead.");
            view::results(state, &query, &vertical, &hits, Some(note))
        }
        None => {
            let hits = rank(state, &query, &vertical);
            view::results(state, &query, &vertical, &hits, None)
        }
    }
}
impl Service for SearchService {
    fn kind(&self) -> &str {
        "search"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let state = web::shape(initial, OBJECTS, ARRAYS)?;
        web::variant(&state, "skin", SKINS)?;
        web::theme(&state)?;
        let verticals = verticals(&state);
        for raw in state
            .get("documents")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let document: Document = serde_json::from_value(raw.clone())?;
            // A result card is a promise that the link resolves. The index is generated, so a
            // relative URL or a missing title is a build bug and belongs at load, not at render.
            if !(document.url.starts_with("http://") || document.url.starts_with("https://")) {
                return Err(SimError::invalid(format!(
                    "indexed url {} must be absolute",
                    document.url
                )));
            }
            if document.title.trim().is_empty() {
                return Err(SimError::invalid(format!(
                    "indexed url {} has no title",
                    document.url
                )));
            }
            if !verticals.contains(&document.vertical) {
                return Err(SimError::invalid(format!(
                    "unknown vertical {} for {}",
                    document.vertical, document.url
                )));
            }
        }
        for (tag, entry) in state
            .get("bangs")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let template = match entry {
                Value::String(t) => t.clone(),
                other => web::text(other, "template"),
            };
            if !(template.starts_with("http://") || template.starts_with("https://")) {
                return Err(SimError::invalid(format!(
                    "bang !{tag} must expand to an absolute url"
                )));
            }
        }
        Ok(state)
    }
    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        request: &HttpRequest,
    ) -> Result<HttpResponse> {
        let method = request.method.to_ascii_uppercase();
        match (method.as_str(), web::path(request).as_str()) {
            ("GET", "/") => view::home(state, &ctx.actor),
            ("GET", "/about") => view::about(state),
            ("GET" | "POST", "/search") => search(state, ctx, request),
            ("GET", "/lucky") => {
                let (query, vertical) = asked(request)?;
                let top = rank(state, &query, &vertical).into_iter().next();
                view::lucky(state, &query, top.as_ref())
            }
            ("GET", "/api/search") => {
                // Read-only on purpose: an agent polling the API must not rewrite someone's
                // recent searches, and the link-integrity test wants a pure function.
                let (query, vertical) = asked(request)?;
                if query.is_empty() {
                    return web::error(400, "missing q");
                }
                if !verticals(state).contains(&vertical) {
                    return web::error(400, format!("unknown vertical {vertical}"));
                }
                let hits = rank(state, &query, &vertical);
                HttpResponse::json(
                    200,
                    &json!({"query": query, "vertical": vertical, "count": hits.len(), "results": hits}),
                )
            }
            ("POST", "/api/history/clear") => {
                let cleared = forget(state, &ctx.actor);
                HttpResponse::json(200, &json!({"cleared": cleared}))
            }
            // The page-facing twin of the API route: a button must land somewhere readable.
            ("POST", "/history/clear") => {
                forget(state, &ctx.actor);
                view::home(state, &ctx.actor)
            }
            ("GET", _) => web::error(404, "route not found"),
            _ => web::error(405, "method not allowed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The real world and the three sites files this crate owns: a seeded result card is a claim
    /// that the link resolves, so the tests read the shipped index rather than a fixture of it.
    const WORLD: &str = include_str!("../../../worlds/company-2026/world.json");
    const SITES: [(&str, &str); 3] = [
        (
            "google-search",
            include_str!("../../../worlds/company-2026/sites/google-search.json"),
        ),
        (
            "bing-search",
            include_str!("../../../worlds/company-2026/sites/bing-search.json"),
        ),
        (
            "ddg-search",
            include_str!("../../../worlds/company-2026/sites/ddg-search.json"),
        ),
    ];
    fn ctx(actor: &str) -> ServiceContext {
        ServiceContext {
            actor: actor.into(),
            source: "alice-mac".into(),
            tick: 11,
            seed: 3,
            instance: "google-search".into(),
        }
    }
    fn start(initial: Value) -> Value {
        SearchService.initialize(initial, &ctx("alice")).unwrap()
    }
    /// The engine as `world.json` carries it, index and all.
    fn engine(id: &str) -> Value {
        let world: Value = serde_json::from_str(WORLD).unwrap();
        let service = world["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == json!(id))
            .unwrap_or_else(|| panic!("{id} is not in world.json"));
        start(service["initial_state"].clone())
    }
    fn site(id: &str) -> Value {
        let raw = SITES.iter().find(|(name, _)| *name == id).unwrap().1;
        serde_json::from_str::<Value>(raw).unwrap()["initial_state"].clone()
    }
    /// What `scripts/build-search-index.mjs` does, over an index the test controls: one shared
    /// document set, each engine's own `authority_overrides` applied.
    fn seeded(id: &str, documents: &[Value]) -> Value {
        let file: Value =
            serde_json::from_str(SITES.iter().find(|(n, _)| *n == id).unwrap().1).unwrap();
        let overrides = file["authority_overrides"].clone();
        let mut state = site(id);
        state["documents"] = documents
            .iter()
            .map(|d| {
                let mut d = d.clone();
                if let Some(authority) = overrides.get(web::text(&d, "site")) {
                    d["authority"] = authority.clone();
                }
                d
            })
            .collect();
        start(state)
    }
    fn hit(url: &str, title: &str, from: &str, vertical: &str, keywords: &[&str]) -> Value {
        json!({"url": url, "title": title, "site": from, "vertical": vertical,
               "snippet": "", "authority": 0, "keywords": keywords})
    }
    fn call(state: &mut Value, actor: &str, method: &str, url: &str) -> HttpResponse {
        let request = if method == "GET" {
            HttpRequest::get(format!("http://engine.test{url}"))
        } else {
            HttpRequest::json(method, format!("http://engine.test{url}"), &json!({})).unwrap()
        };
        SearchService
            .handle(state, &ctx(actor), &request)
            .unwrap_or_else(|e| panic!("{method} {url}: {e}"))
    }
    fn get(state: &mut Value, actor: &str, url: &str) -> Value {
        let response = call(state, actor, "GET", url);
        assert_eq!(response.status, 200, "GET {url}");
        serde_json::from_slice(&response.body).unwrap()
    }
    fn look(state: &mut Value, query: &str) -> Vec<String> {
        get(state, "alice", &format!("/api/search?q={}", enc(query)))["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| web::text(r, "url"))
            .collect()
    }
    /// Every `url` a page offers, however deeply nested — links, cards, buttons, thumbnails.
    fn links(page: &Value) -> Vec<String> {
        match page {
            Value::Object(map) => map
                .iter()
                .flat_map(|(key, value)| match (key.as_str(), value) {
                    ("url", Value::String(url)) => vec![url.clone()],
                    _ => links(value),
                })
                .collect(),
            Value::Array(items) => items.iter().flat_map(links).collect(),
            _ => vec![],
        }
    }

    /// The contract the whole index rests on: a seeded document is an absolute, titled URL, and
    /// the engines carry the same set because one script splices it into all three.
    #[test]
    fn every_seeded_document_is_a_real_link() {
        let mut shared: Option<Vec<String>> = None;
        for (id, _) in SITES {
            let state = engine(id);
            let index = documents(&state);
            assert!(!index.is_empty(), "{id} has an empty index");
            for document in &index {
                let parsed = url::Url::parse(&document.url)
                    .unwrap_or_else(|e| panic!("{id}: {} is not a url: {e}", document.url));
                assert!(
                    parsed.scheme() == "http" || parsed.scheme() == "https",
                    "{id}: {} is not absolute",
                    document.url
                );
                assert!(parsed.has_host(), "{id}: {} has no host", document.url);
                assert!(!document.title.trim().is_empty(), "{}", document.url);
                assert!(!document.site.trim().is_empty(), "{}", document.url);
            }
            let urls: Vec<String> = index.iter().map(|d| d.url.clone()).collect();
            match &shared {
                None => shared = Some(urls),
                Some(first) => assert_eq!(first, &urls, "{id} indexes a different document set"),
            }
        }
    }
    /// A hit hands back the seeded URL byte for byte — no rewriting, no tracking wrapper — and
    /// the rendered card navigates to that same URL. Every engine indexes the same set, so the
    /// whole index is swept once and the other two are spot-checked: ranking is quadratic in the
    /// index and the index grows with every site that lands.
    #[test]
    fn a_hit_returns_the_documents_url_unchanged() {
        for (id, _) in SITES {
            let mut state = engine(id);
            let index = documents(&state);
            let sweep = if id == "google-search" {
                index.len()
            } else {
                8
            };
            for document in index.iter().take(sweep) {
                // A document nothing can be typed to reach is not in the index in any useful
                // sense, so the query is its title, or its keywords when the title is symbols.
                let query = match tokens(&document.title).is_empty() {
                    false => document.title.clone(),
                    true => document.keywords.join(" "),
                };
                assert!(
                    !tokens(&query).is_empty(),
                    "{id}: {} has no searchable text",
                    document.url
                );
                assert!(
                    look(&mut state, &query).contains(&document.url),
                    "{id}: searching {query:?} lost {}",
                    document.url
                );
                let page = get(&mut state, "alice", &format!("/search?q={}", enc(&query)));
                assert!(
                    links(&page).contains(&document.url),
                    "{id}: no card links to {}",
                    document.url
                );
            }
        }
    }
    /// Three engines, one index, three orders — otherwise shipping three of them is decoration.
    #[test]
    fn the_engines_disagree_about_the_same_index() {
        let index = [
            hit("http://outlook.com/mail", "Mail", "outlook.com", "all", &[]),
            hit(
                "http://wikipedia.org/wiki/mail",
                "Mail",
                "wikipedia.org",
                "all",
                &[],
            ),
            hit(
                "http://x.com/alice/atlas",
                "Atlas post",
                "x.com",
                "all",
                &[],
            ),
            hit(
                "http://mastodon.social/@alice/atlas",
                "Atlas post",
                "mastodon.social",
                "all",
                &[],
            ),
        ];
        let mut google = seeded("google-search", &index);
        let mut bing = seeded("bing-search", &index);
        let mut ddg = seeded("ddg-search", &index);
        // Bing weights the publisher hardest, so the work tool beats the encyclopedia.
        assert_eq!(look(&mut bing, "mail")[0], "http://outlook.com/mail");
        assert_eq!(
            look(&mut google, "mail")[0],
            "http://wikipedia.org/wiki/mail"
        );
        // DuckDuckGo's overrides favour the open network over the big platform.
        assert_eq!(
            look(&mut google, "atlas post")[0],
            "http://x.com/alice/atlas"
        );
        assert_eq!(
            look(&mut ddg, "atlas post")[0],
            "http://mastodon.social/@alice/atlas"
        );
    }
    /// Scoring is a pure total order: same state, same query, same answer, and zero-score
    /// documents never appear.
    #[test]
    fn ranking_is_total_and_drops_misses() {
        let state = seeded(
            "google-search",
            &[
                hit(
                    "http://a.example/1",
                    "Atlas launch",
                    "northstar.example",
                    "all",
                    &["atlas"],
                ),
                hit(
                    "http://a.example/2",
                    "Atlas launch",
                    "northstar.example",
                    "all",
                    &["atlas"],
                ),
                hit(
                    "http://b.example/3",
                    "Kettle repair",
                    "bmartinez.net",
                    "all",
                    &[],
                ),
            ],
        );
        let ranked: Vec<String> = rank(&state, "atlas", "all")
            .iter()
            .map(|h| h.document.url.clone())
            .collect();
        assert_eq!(ranked, ["http://a.example/1", "http://a.example/2"]);
        assert_eq!(ranked, {
            let again: Vec<String> = rank(&state, "ATLAS", "all")
                .iter()
                .map(|h| h.document.url.clone())
                .collect();
            again
        });
        assert!(rank(&state, "", "all").is_empty());
        assert!(rank(&state, "nothingmatchesthis", "all").is_empty());
    }
    /// Vertical tabs filter the index and are real links; an invented vertical is a 400, not an
    /// empty page that looks like "no results".
    #[test]
    fn verticals_filter_and_reject_typos() {
        let index = [
            hit(
                "http://theverge.com/atlas",
                "Atlas ships",
                "theverge.com",
                "news",
                &[],
            ),
            hit(
                "http://youtube.com/watch",
                "Atlas walkthrough",
                "youtube.com",
                "videos",
                &[],
            ),
            hit(
                "http://northstar.example/atlas",
                "Atlas",
                "northstar.example",
                "all",
                &[],
            ),
        ];
        let mut state = seeded("google-search", &index);
        assert_eq!(look(&mut state, "atlas").len(), 3);
        let news = get(&mut state, "alice", "/api/search?q=atlas&v=news");
        assert_eq!(news["count"], 1);
        assert_eq!(news["results"][0]["url"], "http://theverge.com/atlas");
        let videos = get(&mut state, "alice", "/api/search?q=atlas&v=videos");
        assert_eq!(videos["results"][0]["url"], "http://youtube.com/watch");
        let page = get(&mut state, "alice", "/search?q=atlas");
        for vertical in VERTICALS {
            assert!(
                links(&page)
                    .iter()
                    .any(|u| u == &format!("/search?q=atlas&v={vertical}")),
                "no tab re-queries {vertical}"
            );
        }
        for url in [
            "/search?q=atlas&v=chocolate",
            "/api/search?q=atlas&v=chocolate",
        ] {
            assert_eq!(call(&mut state, "alice", "GET", url).status, 400);
        }
        assert_eq!(call(&mut state, "alice", "GET", "/api/search").status, 400);
    }
    /// The form is a POST and the tabs are GETs; both land on the same results page.
    #[test]
    fn the_form_and_the_link_reach_the_same_page() {
        let index = [hit(
            "http://northstar.example/atlas",
            "Atlas",
            "northstar.example",
            "all",
            &[],
        )];
        let mut state = seeded("google-search", &index);
        let posted = SearchService
            .handle(
                &mut state,
                &ctx("alice"),
                &HttpRequest::json("POST", "http://google.com/search", &json!({"q": "atlas"}))
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(posted.status, 200);
        let got = call(&mut state, "alice", "GET", "/search?q=atlas");
        assert_eq!(posted.body, got.body);
    }
    /// Recent searches: most-recent-first, deduplicated, bounded, per actor, and clearable.
    #[test]
    fn history_dedupes_bounds_and_clears() {
        let mut state = seeded("google-search", &[]);
        state["history"] = json!({});
        for n in 0..12 {
            get(&mut state, "alice", &format!("/search?q=q{n}"));
        }
        assert_eq!(recent(&state, "alice").len(), HISTORY);
        assert_eq!(recent(&state, "alice")[0], "q11");
        assert_eq!(recent(&state, "alice")[9], "q2");
        get(&mut state, "alice", "/search?q=q5");
        assert_eq!(recent(&state, "alice")[0], "q5");
        assert_eq!(
            recent(&state, "alice")
                .iter()
                .filter(|q| *q == "q5")
                .count(),
            1
        );
        get(&mut state, "bob", "/search?q=kettle");
        assert_eq!(recent(&state, "bob"), ["kettle"]);
        // The home page offers the history back as real links.
        let home = get(&mut state, "alice", "/");
        assert!(links(&home).contains(&"/search?q=q5".to_string()));
        let cleared = call(&mut state, "alice", "POST", "/api/history/clear");
        assert_eq!(cleared.status, 200);
        assert_eq!(
            serde_json::from_slice::<Value>(&cleared.body).unwrap()["cleared"],
            HISTORY
        );
        assert!(recent(&state, "alice").is_empty());
        assert_eq!(recent(&state, "bob"), ["kettle"]);
        call(&mut state, "bob", "POST", "/history/clear");
        assert!(recent(&state, "bob").is_empty());
    }
    /// DuckDuckGo's whole pitch: nothing typed into the box is written down. The API never
    /// writes history for anyone, so polling it cannot rewrite someone's recent searches.
    #[test]
    fn duckduckgo_writes_nothing_down() {
        let mut ddg = seeded("ddg-search", &[]);
        get(&mut ddg, "alice", "/search?q=kettle");
        assert!(recent(&ddg, "alice").is_empty());
        assert_eq!(ddg["history"], json!({}));
        let mut google = seeded("google-search", &[]);
        google["history"] = json!({});
        get(&mut google, "alice", "/api/search?q=kettle");
        assert!(recent(&google, "alice").is_empty());
    }
    /// `!gh atlas` jumps; `!zz atlas` says so and searches anyway; an engine without a prefix
    /// treats the bang as ordinary text.
    #[test]
    fn bangs_jump_and_unknown_tags_fall_back() {
        let mut ddg = seeded(
            "ddg-search",
            &[hit(
                "http://gh.example/atlas",
                "!gh atlas",
                "github.com",
                "all",
                &[],
            )],
        );
        let page = get(
            &mut ddg,
            "alice",
            &format!("/search?q={}", enc("!gh atlas ranking")),
        );
        assert_eq!(page["title"], "!gh - DuckDuckGo");
        assert!(links(&page).contains(&"http://github.com/search?q=atlas+ranking".to_string()));
        let unknown = get(
            &mut ddg,
            "alice",
            &format!("/search?q={}", enc("!zz atlas")),
        );
        assert_eq!(unknown["title"], "!zz atlas - DuckDuckGo");
        assert!(serde_json::to_string(&unknown)
            .unwrap()
            .contains("No !zz shortcut"));
        // Google has no bang prefix, so the text is just text.
        let mut google = seeded(
            "google-search",
            &[hit(
                "http://gh.example/atlas",
                "!gh atlas",
                "github.com",
                "all",
                &[],
            )],
        );
        let plain = get(
            &mut google,
            "alice",
            &format!("/search?q={}", enc("!gh atlas")),
        );
        assert!(links(&plain).contains(&"http://gh.example/atlas".to_string()));
    }
    /// Lucky is one button and the button goes to the top hit; with nothing indexed it says so
    /// rather than offering a control that goes nowhere.
    #[test]
    fn lucky_goes_to_the_top_hit() {
        let index = [
            hit(
                "http://wikipedia.org/wiki/atlas",
                "Atlas",
                "wikipedia.org",
                "all",
                &[],
            ),
            hit(
                "http://bmartinez.net/atlas",
                "Atlas",
                "bmartinez.net",
                "all",
                &[],
            ),
        ];
        let mut state = seeded("google-search", &index);
        let top = rank(&state, "atlas", "all")[0].document.url.clone();
        let page = get(&mut state, "alice", "/lucky?q=atlas");
        assert!(links(&page).contains(&top));
        let mut empty = seeded("google-search", &[]);
        let nothing = get(&mut empty, "alice", "/lucky?q=atlas");
        assert!(serde_json::to_string(&nothing)
            .unwrap()
            .contains("Nothing in the index"));
    }
    /// The index is generated, so an engine has to be born before the pages that fill it exist.
    #[test]
    fn an_empty_index_still_serves_every_route() {
        for (id, _) in SITES {
            let mut state = seeded(id, &[]);
            for url in ["/", "/about", "/search?q=atlas", "/lucky?q=atlas"] {
                get(&mut state, "alice", url);
            }
            assert_eq!(get(&mut state, "alice", "/api/search?q=atlas")["count"], 0);
        }
    }
    /// Seed mistakes are build bugs and belong at load, where the message names the file.
    #[test]
    fn initialize_refuses_a_broken_seed() {
        let good = |documents: Value| json!({"skin": "google", "documents": documents});
        assert!(SearchService.initialize(json!([]), &ctx("alice")).is_err());
        assert!(SearchService
            .initialize(json!({"skin": "askjeeves"}), &ctx("alice"))
            .is_err());
        assert!(SearchService
            .initialize(json!({"documents": {}}), &ctx("alice"))
            .is_err());
        for bad in [
            json!([{"url": "/relative", "title": "Atlas"}]),
            json!([{"url": "http://a.example/", "title": "  "}]),
            json!([{"url": "http://a.example/", "title": "Atlas", "vertical": "podcasts"}]),
        ] {
            assert!(
                SearchService
                    .initialize(good(bad.clone()), &ctx("alice"))
                    .is_err(),
                "{bad} was accepted"
            );
        }
        assert!(SearchService
            .initialize(
                json!({"skin": "ddg", "bang_prefix": "!", "bangs": {"gh": "/search?q={}"}}),
                &ctx("alice")
            )
            .is_err());
    }
    /// State is a plain `Value`, so a checkpoint must be a byte-for-byte round trip — including
    /// the history a search just wrote.
    #[test]
    fn state_survives_a_snapshot_round_trip() {
        let mut state = engine("google-search");
        get(&mut state, "alice", "/search?q=atlas");
        let bytes = serde_json::to_vec(&state).unwrap();
        let restored: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(state, restored);
        assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
        let mut reloaded = start(restored);
        assert_eq!(state, reloaded);
        // And the page a restored engine renders is the page the live one rendered.
        let mut live = state.clone();
        assert_eq!(
            call(&mut live, "alice", "GET", "/search?q=atlas").body,
            call(&mut reloaded, "alice", "GET", "/search?q=atlas").body
        );
    }
    /// Unknown routes and methods are refused rather than silently answered with the home page.
    #[test]
    fn unknown_routes_are_refused() {
        let mut state = seeded("google-search", &[]);
        assert_eq!(call(&mut state, "alice", "GET", "/images").status, 404);
        assert_eq!(call(&mut state, "alice", "DELETE", "/").status, 405);
    }
}
