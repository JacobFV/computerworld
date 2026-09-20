//! Page rendering as HTML. The three engines serve identical routes; the skin decides
//! what they look like, which is the whole difference between a Google results page
//! and a Bing one. The markup follows `research/google-ceiling/mock.html` and the
//! stylesheet is `search.css` next to this file; the palette a seed carries goes on
//! `<html>` as custom properties so the sheet stays one static file.
//!
//! Element ids are the agent API and are the same the `Page` version used: `search`
//! (the form), `q`, `search-go`, `search-lucky`, `tab-<vertical>`, `hit-<n>` (the whole
//! result is one link, with `hit-<n>-site`, `-title` and `-snippet` inside), `shot-<n>`,
//! `reel-<n>`, `news-<n>`, `recent-<n>`, `recent-clear`, `trend-<n>`, `bang-<tag>`,
//! `foot-about`, `foot-<n>`, `jump`, `lucky-go`, `about-home`, `fact-<id>`, `mark`.
use crate::{documents, recent, Document, Hit, PAGE_SIZE, VERTICALS};
use cw_protocol::{HttpResponse, Result};
use cw_service_common as web;
use cw_service_common::html::{
    button, div, el, empty, form, fragment, href, hidden, link, span, text_input, Document as Html, Html as Node,
};
use serde_json::Value;

const CSS: &str = include_str!("search.css");

/// Palette and wordmark resolved once per render, so every helper reads the same colours.
pub(crate) struct Chrome {
    brand: String,
    skin: String,
    accent: String,
    ink: String,
    muted: String,
    surface: String,
    paper: String,
    link: String,
    content: u32,
}
impl Chrome {
    pub(crate) fn read(state: &Value) -> Result<Self> {
        let theme = web::theme(state)?;
        let skin = web::variant(state, "skin", crate::SKINS)?;
        let or = |value: &Option<String>, fallback: &str| value.clone().unwrap_or_else(|| fallback.to_owned());
        let accent = or(&theme.accent, "#1a73e8");
        // The classic result title is its own blue, older than any of these brands' accents.
        let link = match skin.as_str() {
            "ddg" => "#3969ef".to_owned(),
            "plain" => accent.clone(),
            _ => "#1a0dab".to_owned(),
        };
        Ok(Self {
            brand: match web::text(state, "brand") {
                b if b.is_empty() => "Search".to_owned(),
                b => b,
            },
            skin,
            accent,
            ink: or(&theme.ink, "#202124"),
            muted: or(&theme.muted, "#5f6368"),
            surface: or(&theme.surface, "#f1f3f4"),
            paper: or(&theme.background, "#ffffff"),
            link,
            content: theme.content_width.unwrap_or(652).clamp(320, 1200),
        })
    }
    /// The page shell: title, language, the one stylesheet, the palette and the skin.
    fn document(&self, title: &str, page_class: &str, body: Vec<Node>) -> Html {
        Html::new(title)
            .lang("en")
            .stylesheet(CSS)
            .root_style(&format!(
                "--accent: {}; --ink: {}; --muted: {}; --surface: {}; --paper: {}; --link: {}; --content: {}px",
                self.accent, self.ink, self.muted, self.surface, self.paper, self.link, self.content
            ))
            .body_class(&format!("skin-{} {page_class}", self.skin))
            .body(body)
    }
    /// The wordmark: six letters in four colours for Google, a dot and the name for
    /// DuckDuckGo, the name in the accent otherwise. A link home except on the home page.
    fn mark(&self, home: bool) -> Node {
        let mut mark = if home { el("div") } else { el("a").attr("href", "/") };
        mark = mark.id("mark").class("logo").attr("aria-label", self.brand.as_str());
        match self.skin.as_str() {
            "google" => mark.each(self.brand.chars().enumerate(), |(i, letter)| {
                span(&format!("c{}", i % 6)).text(letter.to_string())
            }),
            "ddg" => mark
                .child(span("dot"))
                .child(span("name").id("mark-text").text(self.brand.as_str())),
            _ => mark.child(span("name").id("mark-text").text(self.brand.as_str())),
        }
    }
    /// The query box: a GET form so the results URL is the query, as it should be. The
    /// vertical rides along as a hidden field; the home page adds the two buttons.
    fn box_(&self, query: &str, vertical: &str, buttons: bool) -> Node {
        let (go, lucky) = self.buttons();
        let label = format!("Search {}", self.brand);
        let pill = div("box")
            .attr("role", "search")
            .child(span("mag").attr("aria-hidden", "true"))
            .child(
                text_input("q", "q", query)
                    .attr("aria-label", label.as_str())
                    .attr("placeholder", "Search the web")
                    .attr("autocomplete", "off"),
            )
            .child(span("mic").attr("aria-hidden", "true").child(el("i")))
            .child(span("lens").attr("aria-hidden", "true"))
            .when(!buttons, |pill| pill.child(button("search-go", go).class("go")));
        form("search", "/search", "get")
            .when(vertical != VERTICALS[0], |f| f.child(hidden("v", vertical)))
            .child(pill)
            .when(buttons, |f| {
                f.child(
                    div("buttons")
                        .id("search-buttons")
                        .child(button("search-go", go))
                        .child(button("search-lucky", lucky).attr("formaction", "/lucky")),
                )
            })
    }
    fn buttons(&self) -> (&'static str, &'static str) {
        match self.skin.as_str() {
            "google" => ("Google Search", "I'm Feeling Lucky"),
            "bing" => ("Search", "Surprise me"),
            "ddg" => ("Search", "I'm Feeling Ducky"),
            _ => ("Search", "First hit"),
        }
    }
    /// The results header: the small mark beside the box.
    fn bar(&self, query: &str, vertical: &str) -> Node {
        el("header").id("head").class("bar").child(self.mark(false)).child(self.box_(query, vertical, false))
    }
    /// Vertical tabs: real links that re-run the same query against a filtered index.
    fn tabs(&self, verticals: &[String], query: &str, current: &str) -> Node {
        el("nav").id("tabs").class("tabs").each(verticals, |vertical| {
            link(&format!("tab-{vertical}"), href("/search", &[("q", query), ("v", vertical)]), label(vertical))
                .class(if vertical == current { "tab on" } else { "tab" })
        })
    }
    /// The home page's top-right links, from the seed's `header` list, and its `cta`
    /// (Google's blue Sign in) when the seed names one.
    fn top(&self, state: &Value) -> Node {
        let entries: Vec<(String, String)> = state
            .get("header")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|e| (web::text(e, "text"), web::text(e, "url")))
            .filter(|(t, u)| !t.is_empty() && !u.is_empty())
            .collect();
        let cta = state.get("cta").map(|e| (web::text(e, "text"), web::text(e, "url"))).filter(|(t, u)| !t.is_empty() && !u.is_empty());
        if entries.is_empty() && cta.is_none() {
            return empty();
        }
        el("header")
            .id("top")
            .class("top")
            .each(entries.iter().enumerate(), |(i, (t, u))| link(&format!("head-{i}"), u.as_str(), t.as_str()).class("txt"))
            .when(self.skin == "google", |h| {
                h.child(
                    el("a")
                        .id("head-apps")
                        .class("apps")
                        .attr("href", "/about")
                        .attr("aria-label", format!("{} apps", self.brand))
                        .child(span("dots").each(0..9, |_| el("i"))),
                )
            })
            .maybe(cta.map(|(t, u)| link("head-cta", u, t).class("signin")))
    }
    /// Seeded outbound links plus the engine's own about page; nothing here is decorative.
    fn footer(&self, state: &Value) -> Node {
        let country = match web::text(state, "country") {
            c if c.is_empty() => "United States".to_owned(),
            c => c,
        };
        let mut left = div("").child(link("foot-about", "/about", format!("About {}", self.brand)));
        let mut right = div("");
        for (i, entry) in state.get("footer").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let url = web::text(entry, "url");
            if url.is_empty() {
                continue;
            }
            let a = link(&format!("foot-{i}"), url, web::text(entry, "text"));
            if i % 2 == 0 {
                left = left.child(a);
            } else {
                right = right.child(a);
            }
        }
        el("footer")
            .id("foot")
            .class("band")
            .child(div("country").id("foot-country").text(country))
            .child(div("links").child(left).child(right))
    }
}
fn label(vertical: &str) -> String {
    let mut chars = vertical.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}
/// `docs.google.com › documents › atlas-launch`, the line above a result title.
fn crumb(document: &Document) -> String {
    let Ok(parsed) = url::Url::parse(&document.url) else {
        return document.site.clone();
    };
    let host = parsed.host_str().unwrap_or(&document.site).to_owned();
    let trail: Vec<&str> = parsed.path().split('/').filter(|s| !s.is_empty()).collect();
    if trail.is_empty() {
        host
    } else {
        format!("{host} › {}", trail.join(" › "))
    }
}
/// A flat tint for a synthetic picture, chosen from its label, stable across renders.
fn tint(label: &str) -> &'static str {
    const PALETTE: [&str; 8] = ["#5b6dcd", "#2f8f6f", "#c2603a", "#8a4fb8", "#2e86ab", "#b8536b", "#5f7d2e", "#9a6b1f"];
    let mut h = 0xcbf29ce484222325u64;
    for b in label.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    PALETTE[(h % 8) as usize]
}
fn verticals(state: &Value) -> Vec<String> {
    match web::strings(state, "verticals") {
        v if v.is_empty() => VERTICALS.iter().map(|v| (*v).to_owned()).collect(),
        v => v,
    }
}

pub(crate) fn home(state: &Value, actor: &str) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    let tagline = web::text(state, "tagline");
    let history = recent(state, actor);
    let trending = web::strings(state, "trending");
    let bangs: Vec<(String, String)> = state
        .get("bangs")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(tag, entry)| (tag.clone(), web::text(entry, "title")))
        .collect();
    let hero = el("main")
        .class("hero")
        .child(chrome.mark(true))
        .when(!tagline.is_empty(), |m| m.child(el("p").id("tagline").class("tagline").text(tagline.as_str())))
        .child(chrome.box_("", VERTICALS[0], true))
        .when(!history.is_empty(), |m| {
            m.child(
                el("section")
                    .id("recent")
                    .class("list")
                    .child(el("h2").id("recent-title").text("Recent searches"))
                    .child(el("ul").each(history.iter().enumerate(), |(i, q)| {
                        el("li").child(link(&format!("recent-{i}"), href("/search", &[("q", q)]), q.as_str()))
                    }))
                    .child(
                        form("recent-clear-form", "/history/clear", "post")
                            .class("clear")
                            .child(button("recent-clear", "Clear recent searches")),
                    ),
            )
        })
        .when(!trending.is_empty(), |m| {
            m.child(
                el("section")
                    .id("trending")
                    .class("list")
                    .child(el("h2").id("trend-title").text("Trending searches"))
                    .child(div("tiles").id("trend").each(trending.iter().enumerate(), |(i, q)| {
                        link(&format!("trend-{i}"), href("/search", &[("q", q)]), q.as_str())
                    })),
            )
        })
        .when(!bangs.is_empty(), |m| {
            // Inert by design: a bang is typed into the box, so a link here would be a fake control.
            m.child(
                el("section")
                    .id("bang-list")
                    .class("list")
                    .child(el("h2").id("bang-title").text("Bang shortcuts jump straight to another site"))
                    .child(div("tiles").id("bangs").each(bangs.iter(), |(tag, title)| {
                        div("bang").id(format!("bang-{tag}")).text(format!("!{tag} — {title}"))
                    })),
            )
        });
    let doc = chrome.document(&chrome.brand, "home", vec![chrome.top(state), hero, chrome.footer(state)]);
    web::html::page(&doc)
}

pub(crate) fn results(
    state: &Value,
    query: &str,
    vertical: &str,
    hits: &[Hit],
    page: usize,
    note: Option<String>,
) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    let pages = hits.len().div_ceil(PAGE_SIZE).max(1);
    let page = page.clamp(1, pages);
    let shown = &hits[((page - 1) * PAGE_SIZE).min(hits.len())..(page * PAGE_SIZE).min(hits.len())];
    let stats = match chrome.skin.as_str() {
        "google" => format!("About {} results", hits.len()),
        "ddg" => format!("{} results for {query}", hits.len()),
        _ => format!("{} results", hits.len()),
    };
    let offset = (page - 1) * PAGE_SIZE;
    let listing = match vertical {
        "images" => div("shots").id("images").each(shown.iter().enumerate(), |(i, hit)| tile(offset + i, hit)),
        "videos" => fragment(shown.iter().enumerate().map(|(i, hit)| reel(offset + i, hit))),
        "news" => fragment(shown.iter().enumerate().map(|(i, hit)| story(offset + i, hit))),
        _ => fragment(shown.iter().enumerate().map(|(i, hit)| snippet(offset + i, hit))),
    };
    let main = el("main")
        .class("results")
        .maybe(note.map(|n| el("p").id("note").class("note").text(n)))
        .child(el("p").id("stats").class("stats").text(stats))
        .when(hits.is_empty(), |m| {
            m.child(el("p").id("empty").class("empty").text(format!("Your search - {query} - did not match any documents.")))
                .child(el("p").id("empty-hint").class("hint").text("Try different keywords, or fewer of them."))
        })
        .child(listing)
        .when(pages > 1, |m| m.child(pagination(query, vertical, page, pages)));
    let doc = chrome.document(
        &format!("{query} - {}", chrome.brand),
        "serp",
        vec![chrome.bar(query, vertical), chrome.tabs(&verticals(state), query, vertical), main, chrome.footer(state)],
    );
    web::html::page(&doc)
}
/// Numbered page links with Previous and Next: `page-<n>`, `page-prev`, `page-next`.
fn pagination(query: &str, vertical: &str, page: usize, pages: usize) -> Node {
    let to = |n: usize| href("/search", &[("q", query), ("v", vertical), ("p", &n.to_string())]);
    el("nav")
        .id("pages")
        .class("pages")
        .attr("aria-label", "Pages")
        .when(page > 1, |n| n.child(link("page-prev", to(page - 1), "Previous").class("word")))
        .each(1..=pages, |n| {
            link(&format!("page-{n}"), to(n), n.to_string()).class(if n == page { "on" } else { "" })
        })
        .when(page < pages, |n| n.child(link("page-next", to(page + 1), "Next").class("word")))
}
/// The classic web result: breadcrumb, blue title, two lines of description, all one link.
fn snippet(i: usize, hit: &Hit) -> Node {
    let id = format!("hit-{i}");
    el("a")
        .id(id.as_str())
        .class("hit")
        .attr("href", hit.document.url.as_str())
        .child(span("crumb").id(format!("{id}-site")).text(crumb(&hit.document)))
        .child(span("title").id(format!("{id}-title")).text(hit.document.title.as_str()))
        .child(span("snip").id(format!("{id}-snippet")).text(hit.document.snippet.as_str()))
}
/// Images are tiles; the artwork is a flat tint of the title, honestly synthetic.
fn tile(i: usize, hit: &Hit) -> Node {
    let id = format!("shot-{i}");
    el("a")
        .id(id.as_str())
        .class("shot")
        .attr("href", hit.document.url.as_str())
        .child(
            span("art")
                .id(format!("{id}-art"))
                .style(&format!("background-color: {}", tint(&hit.document.title)))
                .text(hit.document.site.as_str()),
        )
        .child(span("cap").id(format!("{id}-title")).text(hit.document.title.as_str()))
}
fn reel(i: usize, hit: &Hit) -> Node {
    let id = format!("reel-{i}");
    el("a")
        .id(id.as_str())
        .class("reel")
        .attr("href", hit.document.url.as_str())
        .child(span("art").id(format!("{id}-art")).text(hit.document.site.as_str()))
        .child(
            span("text")
                .child(span("title").id(format!("{id}-title")).text(hit.document.title.as_str()))
                .child(span("crumb").id(format!("{id}-site")).text(crumb(&hit.document)))
                .child(span("snip").id(format!("{id}-snippet")).text(hit.document.snippet.as_str())),
        )
}
fn story(i: usize, hit: &Hit) -> Node {
    let id = format!("news-{i}");
    el("a")
        .id(id.as_str())
        .class("story")
        .attr("href", hit.document.url.as_str())
        .child(span("art").id(format!("{id}-art")).text(hit.document.site.as_str()))
        .child(
            span("text")
                .child(span("source").id(format!("{id}-source")).text(hit.document.site.as_str()))
                .child(span("title").id(format!("{id}-title")).text(hit.document.title.as_str()))
                .child(span("snip").id(format!("{id}-snippet")).text(hit.document.snippet.as_str())),
        )
}
/// A bang is a jump, so the page is the jump and nothing else.
pub(crate) fn jump(state: &Value, query: &str, tag: &str, title: &str, url: &str) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    let main = el("main")
        .class("results")
        .child(
            el("a")
                .id("jump")
                .class("jump")
                .attr("href", url)
                .child(span("tag").id("jump-tag").text(format!("!{tag} → {title}")))
                .child(span("url").id("jump-url").text(url)),
        )
        .child(el("p").id("jump-hint").class("hint").text("Bang shortcuts skip the results page and go straight to the site."));
    let doc = chrome.document(
        &format!("!{tag} - {}", chrome.brand),
        "serp",
        vec![chrome.bar(query, VERTICALS[0]), main, chrome.footer(state)],
    );
    web::html::page(&doc)
}
pub(crate) fn lucky(state: &Value, query: &str, top: Option<&Hit>) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    let hero = el("main").class("hero short").child(chrome.mark(false));
    let hero = match top {
        Some(hit) => hero
            .child(el("p").id("lucky-lead").class("lead").text(format!("Top result for {query}")))
            .child(el("h1").id("lucky-title").class("lucky-title").text(hit.document.title.as_str()))
            .child(link("lucky-go", hit.document.url.as_str(), format!("Go to {}", crumb(&hit.document))).class("btn primary")),
        None => hero.child(el("p").id("lucky-empty").class("lead").text(format!("Nothing in the index matches {query}."))),
    };
    let doc = chrome.document(&format!("Lucky - {}", chrome.brand), "lucky", vec![hero, chrome.footer(state)]);
    web::html::page(&doc)
}
pub(crate) fn about(state: &Value) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    let indexed = documents(state);
    let about = match web::text(state, "about") {
        a if a.is_empty() => format!(
            "{} ranks a fixed index of pages by where a query matches — title, keywords, then \
             description — and breaks ties by URL, so the same search always returns the same \
             order.",
            chrome.brand
        ),
        a => a,
    };
    let sites = {
        let mut seen: Vec<&str> = indexed.iter().map(|d| d.site.as_str()).collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    };
    let facts = [
        ("pages", "Pages indexed".to_owned(), indexed.len().to_string()),
        ("sites", "Sites covered".to_owned(), sites.to_string()),
        ("tabs", "Verticals".to_owned(), verticals(state).iter().map(|v| label(v)).collect::<Vec<_>>().join(", ")),
        (
            "history",
            "Recent searches".to_owned(),
            if state.get("retain_history").and_then(Value::as_bool).unwrap_or(true) {
                "kept per signed-in person".to_owned()
            } else {
                "never stored".to_owned()
            },
        ),
    ];
    let hero = el("main")
        .class("hero short")
        .child(chrome.mark(false))
        .child(el("p").id("about-lead").class("prose").text(about))
        .child(div("facts").id("facts").each(facts.iter(), |(id, name, value)| {
            div("fact")
                .id(format!("fact-{id}"))
                .child(span("name").id(format!("fact-{id}-name")).text(name.as_str()))
                .child(span("value").id(format!("fact-{id}-value")).text(value.as_str()))
        }))
        .child(link("about-home", "/", format!("Back to {}", chrome.brand)).class("btn primary"));
    let doc = chrome.document(&format!("About {}", chrome.brand), "about", vec![hero, chrome.footer(state)]);
    web::html::page(&doc)
}
