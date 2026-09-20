//! Page rendering as HTML. One set of routes and one set of element ids, three skins:
//! `vector` (wikipedia.org: the left panel, the tab row, serif headings on hairlines, the
//! floated infobox), `imdb` (the black bar with the yellow block, a dark title hero with
//! poster, rating and credits, a cast grid) and `archive` (archive.org: the black top
//! navigation with media types, tiles on the home page, a theatre over an item page).
//! Each skin has its own stylesheet next to this file; the seed's palette goes on
//! `<html>` as custom properties.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `search`
//! (form), `search-q`, `search-submit`, `masthead*`, `featured*`, `news*`, `all-<n>`,
//! `portal-random`, `tabs`, `tab-<n>`, `article-*`, `toc`, `toc-<n>`, `sec-<n>-*`,
//! `ref-<n>`, `see-<n>`, `cat-<n>`, `infobox`, `info-*`, `section-*`, `edit`,
//! `edit-body`, `edit-comment`, `edit-submit`, `talk-*`, `reply`, `reply-text`,
//! `reply-submit`, `hist-*`, `rev-<n>*`, `results-*`, `hit-<n>*`, `foot`.
use crate::{snippet, Article, WikiState, BRAND, TAGLINE};
use cw_protocol::{HttpResponse, Result as SimResult};
use cw_service_common as web;
use cw_service_common::html::{
    button, div, el, empty, form, fragment, label, link, span, text, text_input, Document,
    Html as Node,
};

const VECTOR: &str = include_str!("vector.css");
const IMDB: &str = include_str!("imdb.css");
const ARCHIVE: &str = include_str!("archive.css");

pub(crate) fn article_url(id: &str) -> String {
    format!("/wiki/{id}")
}
fn brand_of(s: &WikiState) -> &str {
    match s.brand.as_str() {
        "" => BRAND,
        b => b,
    }
}
fn tagline_of(s: &WikiState) -> &str {
    match s.tagline.as_str() {
        "" => TAGLINE,
        t => t,
    }
}
/// A flat tint for a synthetic picture, chosen from its label, stable across renders.
fn tint(label: &str) -> &'static str {
    const PALETTE: [&str; 8] = [
        "#44546a", "#2f6f5f", "#8a4b32", "#5d3f7a", "#25607c", "#7c3a4b", "#4d6426", "#6e5a2a",
    ];
    PALETTE[((crate::fnv1a64(label) >> 23) % 8) as usize]
}
fn initials(label: &str) -> String {
    label
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}
/// A synthetic picture: a tinted box with the label inside, honestly not a photograph.
fn art(id: Option<String>, class: &str, title: &str) -> Node {
    let node = span(class)
        .style(&format!("background-color: {}", tint(title)))
        .child(span("art-label").text(title));
    match id {
        Some(id) => node.id(id),
        None => node,
    }
}
fn info<'a>(article: &'a Article, key: &str) -> Option<&'a str> {
    article
        .infobox
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}
/// `8.1` out of `8.1/10`, from the infobox row whose key ends in "rating".
fn rating(article: &Article) -> Option<&str> {
    article
        .infobox
        .iter()
        .find(|(k, _)| k.to_ascii_lowercase().ends_with("rating"))
        .and_then(|(_, v)| v.split('/').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}
/// The Archive's media type of an item or a collection, read off its infobox and categories.
fn media_type(article: &Article) -> &'static str {
    let mut hay = article.categories.join(" ").to_ascii_lowercase();
    for key in ["Type", "Collection"] {
        hay.push(' ');
        hay.push_str(&info(article, key).unwrap_or("").to_ascii_lowercase());
    }
    let has = |words: &[&str]| words.iter().any(|w| hay.contains(w));
    if has(&["software"]) {
        "software"
    } else if has(&["film", "video", "prelinger"]) {
        "video"
    } else if has(&["audio", "radio", "music"]) {
        "audio"
    } else if has(&["book", "library", "texts"]) {
        "texts"
    } else if has(&["web", "wayback"]) {
        "web"
    } else {
        "data"
    }
}

struct Tab {
    name: &'static str,
    url: String,
    current: bool,
}
/// Article, Talk, History: one row of real routes.
fn tabs(id: &str, current: &str) -> Vec<Tab> {
    [
        ("Article", article_url(id)),
        ("Talk", format!("/wiki/Talk:{id}")),
        ("History", format!("/wiki/Special:History/{id}")),
    ]
    .into_iter()
    .map(|(name, url)| Tab {
        name,
        url,
        current: name == current,
    })
    .collect()
}
fn tab_nav(tabs: &[Tab]) -> Node {
    el("nav")
        .id("tabs")
        .class("tabs")
        .each(tabs.iter().enumerate(), |(i, tab)| {
            let a = link(&format!("tab-{i}"), tab.url.as_str(), tab.name);
            if tab.current {
                a.class("on").attr("aria-current", "page")
            } else {
                a
            }
        })
}
fn search_form(s: &WikiState) -> Node {
    let text = format!("Search {}", brand_of(s));
    div("search").id("masthead-search").child(
        form("search", "/search", "post")
            .attr("role", "search")
            .child(
                text_input("search-q", "q", "")
                    .attr("aria-label", text.as_str())
                    .attr("placeholder", text.as_str())
                    .attr("autocomplete", "off"),
            )
            .child(
                button("search-submit", "Search")
                    .attr("aria-label", "Search")
                    .child(span("glass").attr("aria-hidden", "true")),
            ),
    )
}
/// Articles of one category, as header or sidebar navigation.
fn category_links(s: &WikiState, category: &str, prefix: &str) -> Vec<Node> {
    s.articles
        .values()
        .filter(|a| a.categories.iter().any(|c| c == category))
        .enumerate()
        .map(|(i, a)| {
            link(&format!("{prefix}-{i}"), article_url(&a.id), a.title.as_str())
                .class(&format!("mt mt-{}", media_type(a)))
        })
        .collect()
}
/// Header, navigation, search box and footer: every page of a site wears them.
fn shell(
    s: &WikiState,
    title: &str,
    page_class: &str,
    tabs: Option<Vec<Tab>>,
    main: Vec<Node>,
) -> SimResult<HttpResponse> {
    let skin = s.skin_name();
    let brand = brand_of(s);
    let words = div("words")
        .id("masthead-words")
        .child(span("brand").id("masthead-brand").text(brand))
        .child(span("tagline").id("masthead-tagline").text(tagline_of(s)));
    let random = s.articles.len() > 1;
    let foot = el("footer").class("foot").child(
        el("p")
            .id("foot")
            .text("A simulated encyclopedia. Text is available under a free licence."),
    );
    let body = match skin {
        "imdb" => {
            let top = s.canonical("Top_rated").map(|(id, _)| id);
            vec![
                el("header").id("masthead").class("bar").child(
                    div("inner")
                        .child(
                            el("a")
                                .id("masthead-logo")
                                .class("logo")
                                .attr("href", "/")
                                .attr("aria-label", format!("{brand} home"))
                                .text(brand),
                        )
                        .child(link("nav-home", "/", "Menu").class("menu"))
                        .child(search_form(s))
                        .child(
                            el("nav")
                                .class("links")
                                .maybe(top.map(|id| link("nav-top", article_url(&id), "Top rated")))
                                .when(random, |n| {
                                    n.child(link("nav-random", "/wiki/Special:Random", "Random title"))
                                }),
                        ),
                ),
                match tabs {
                    Some(t) => div("subnav").child(div("inner").child(tab_nav(&t))),
                    None => empty(),
                },
                el("main").id("content").class("content").children(main),
                foot.child(div("inner").child(words)),
            ]
        }
        "archive" => vec![
            el("header")
                .id("masthead")
                .class("bar")
                .child(
                    el("a")
                        .id("masthead-logo")
                        .class("logo")
                        .attr("href", "/")
                        .attr("aria-label", format!("{brand} home"))
                        .child(
                            span("temple")
                                .attr("aria-hidden", "true")
                                .child(el("b"))
                                .child(span("cols").each(0..4, |_| el("i")))
                                .child(el("u")),
                        ),
                )
                .child(words)
                .child(
                    el("nav")
                        .class("media")
                        .children(category_links(s, "Collections", "nav-col")),
                )
                .child(search_form(s)),
            div("subnav")
                .child(el("nav").class("site").children(category_links(s, "Site", "nav-site")).when(
                    random,
                    |n| n.child(link("nav-random", "/wiki/Special:Random", "Random item")),
                ))
                .maybe(tabs.map(|t| tab_nav(&t))),
            el("main").id("content").class("content").children(main),
            foot,
        ],
        _ => vec![
            div("panel")
                .id("mw-panel")
                .child(
                    el("a")
                        .id("masthead-logo")
                        .class("globe")
                        .attr("href", "/")
                        .attr("aria-label", format!("{brand} home"))
                        .text(brand.chars().next().unwrap_or('W').to_string()),
                )
                .child(words)
                .child(
                    el("nav")
                        .class("navbox")
                        .child(el("h3").text("Navigation"))
                        .child(
                            el("ul")
                                .child(el("li").child(link("nav-main", "/", "Main page")))
                                .when(random, |ul| {
                                    ul.child(el("li").child(link(
                                        "nav-random",
                                        "/wiki/Special:Random",
                                        "Random article",
                                    )))
                                }),
                        ),
                )
                .when(!s.articles.is_empty(), |panel| {
                    panel.child(
                        el("nav")
                            .class("navbox")
                            .child(el("h3").text("Articles"))
                            .child(el("ul").each(s.articles.values().take(12).enumerate(), |(i, a)| {
                                el("li").child(link(
                                    &format!("nav-art-{i}"),
                                    article_url(&a.id),
                                    a.title.as_str(),
                                ))
                            })),
                    )
                }),
            el("header")
                .id("masthead")
                .class("head")
                .child(div("personal").text("Not logged in"))
                .child(
                    div("row")
                        .child(match tabs {
                            Some(t) => tab_nav(&t),
                            None => el("nav").class("tabs").child(span("on").text(
                                if page_class == "portal" {
                                    "Main Page"
                                } else {
                                    "Special page"
                                },
                            )),
                        })
                        .child(search_form(s)),
                ),
            el("main").id("content").class("content").children(main),
            foot,
        ],
    };
    let theme = &s.theme;
    let pick = |v: &Option<String>, d: &str| v.clone().unwrap_or_else(|| d.to_owned());
    let (css, accent, surface, ink, muted) = match skin {
        "imdb" => (IMDB, "#f5c518", "#f5f2ea", "#121212", "#5a5a5a"),
        "archive" => (ARCHIVE, "#428bca", "#f1f1f1", "#2c2c2c", "#666666"),
        _ => (VECTOR, "#3366cc", "#f8f9fa", "#202122", "#54595d"),
    };
    let doc = Document::new(title)
        .lang("en")
        .stylesheet(css)
        .root_style(&format!(
            "--accent: {}; --paper: {}; --surface: {}; --ink: {}; --muted: {}; --content: {}px",
            pick(&theme.accent, accent),
            pick(&theme.background, "#ffffff"),
            pick(&theme.surface, surface),
            pick(&theme.ink, ink),
            pick(&theme.muted, muted),
            theme.content_width.unwrap_or(960).clamp(320, 1600),
        ))
        .body_class(&format!("skin-{skin} {page_class}"))
        .body(body);
    web::html::page(&doc)
}

/// One card of a listing: the whole card is the link, as the `Page` card with an action was.
fn card(s: &WikiState, id: &str, article: &Article, summary_key: &str) -> Node {
    let skin = s.skin_name();
    el("a")
        .id(id)
        .class(&format!("card mt-{}", media_type(article)))
        .attr("href", article_url(&article.id))
        .child(art(None, "art", &article.title))
        .child(
            span("text")
                .when(skin == "imdb", |t| {
                    t.maybe(rating(article).map(|r| span("stars").child(span("star").text("★")).text(format!(" {r}"))))
                })
                .child(span("title").id(format!("{id}-title")).text(article.title.as_str()))
                .child(
                    span("summary")
                        .id(format!("{id}-{summary_key}"))
                        .text(snippet(&article.summary)),
                )
                .when(skin == "archive", |t| {
                    t.child(span("kind").child(el("i")).text(media_type(article)))
                }),
        )
}

/// Portal: featured article, "In the news", and the whole (small) corpus, because a reader who
/// cannot list the articles cannot tell an empty encyclopedia from a broken one.
pub(crate) fn portal(s: &WikiState) -> SimResult<HttpResponse> {
    let skin = s.skin_name();
    let brand = brand_of(s);
    let featured = s.article(&s.featured).or_else(|| s.articles.values().next());
    let (welcome, featured_label, news_label, all_label, random_label) = match skin {
        "imdb" => (
            format!("What to watch on {brand}"),
            "Featured today",
            "Top news",
            "Fan favorites",
            "Surprise me",
        ),
        "archive" => (
            format!("{brand} is a non-profit library of millions of free books, movies, software, music, websites, and more."),
            "Featured collection",
            "From the blog",
            "Top collections and items at the Archive",
            "Random item",
        ),
        _ => (
            format!("Welcome to {brand},"),
            "From today's featured article",
            "In the news",
            "All articles",
            "Random article",
        ),
    };
    let hero = div("welcome")
        .when(skin == "archive", |h| {
            h.child(
                span("temple big")
                    .attr("aria-hidden", "true")
                    .child(el("b"))
                    .child(span("cols").each(0..4, |_| el("i")))
                    .child(el("u")),
            )
        })
        .child(el("h1").id("welcome").text(welcome))
        .child(el("p").id("welcome-sub").text(match skin {
            "vector" => format!(
                "the free encyclopedia that anyone can edit. {} articles in English",
                s.articles.len()
            ),
            _ => tagline_of(s).to_owned(),
        }));
    let featured = featured.map(|article| {
        el("section")
            .id("featured")
            .class("box featured")
            .child(el("h2").id("featured-label").text(featured_label))
            .child(
                div("box-body")
                    .child(art(None, "art", &article.title))
                    .child(
                        div("box-text")
                            .child(
                                link("featured-title", article_url(&article.id), article.title.as_str())
                                    .class("headline"),
                            )
                            .child(el("p").id("featured-summary").text(snippet(&article.summary))),
                    ),
            )
    });
    let news = el("section")
        .id("news")
        .class("box news")
        .child(el("h2").id("news-label").text(news_label))
        .child(
            el("ul")
                .class("box-body")
                .each(s.in_the_news.iter().enumerate(), |(i, item)| {
                    el("li").child(link(&format!("news-{i}"), item.url.as_str(), item.label.as_str()))
                })
                .when(s.in_the_news.is_empty(), |ul| {
                    ul.child(el("li").id("news-empty").class("small").text("Nothing filed today."))
                }),
        );
    let main = vec![
        hero,
        div("columns").id("portal").maybe(featured).child(news),
        el("section")
            .class("box everything")
            .child(el("h2").id("all-label").text(all_label))
            .child(
                div("cards")
                    .id("all")
                    .each(s.articles.values().enumerate(), |(i, article)| {
                        card(s, &format!("all-{i}"), article, "summary")
                    }),
            ),
        el("p")
            .class("more")
            .child(link("portal-random", "/wiki/Special:Random", random_label).class("btn")),
    ];
    shell(s, &format!("{brand} — {TAGLINE}"), "portal", None, main)
}

/// An infobox value with the names that are articles of this site linked.
fn linked(s: &WikiState, own: &str, id: &str, value: &str) -> Node {
    let mut out = span("value").id(id);
    for (j, part) in value.split(", ").enumerate() {
        if j > 0 {
            out = out.text(", ");
        }
        out = match s.canonical(part) {
            Some((target, _)) if target != own => {
                out.child(link(&format!("{id}-link-{j}"), article_url(&target), part))
            }
            _ => out.text(part),
        };
    }
    out
}
fn infobox(s: &WikiState, article: &Article, with_image: bool) -> Node {
    el("aside")
        .id("infobox")
        .class("infobox")
        .child(div("info-title").id("info-title").text(article.title.as_str()))
        .when(with_image, |b| {
            b.child(
                el("figure")
                    .class("thumb")
                    .child(art(Some("info-image".into()), "art", &article.title))
                    .child(el("figcaption").text(article.title.as_str())),
            )
        })
        .each(article.infobox.iter().enumerate(), |(i, (key, value))| {
            div("info-row")
                .id(format!("info-{i}"))
                .child(span("key").id(format!("info-{i}-key")).text(key.as_str()))
                .child(linked(s, &article.id, &format!("info-{i}-value"), value))
        })
}
fn toc(article: &Article) -> Node {
    if article.sections.len() < 2 {
        return empty();
    }
    el("nav")
        .id("toc")
        .class("toc")
        .attr("aria-label", "Contents")
        .child(el("h2").id("toc-label").text("Contents"))
        .child(el("ul").each(article.sections.iter().enumerate(), |(i, section)| {
            el("li").child(
                el("a")
                    .id(format!("toc-{i}"))
                    .attr("href", format!("{}?section={}", article_url(&article.id), section.id))
                    .child(span("num").text(format!("{}", i + 1)))
                    .child(text(format!(" {}", section.heading))),
            )
        }))
}
fn sections(article: &Article) -> Node {
    fragment(article.sections.iter().enumerate().map(|(i, section)| {
        el("section")
            .class("sec")
            .child(
                div("sec-head")
                    .id(format!("sec-{i}-head"))
                    .child(el("h2").id(format!("sec-{i}-heading")).text(section.heading.as_str()))
                    .child(
                        span("editlink")
                            .text("[")
                            .child(link(
                                &format!("sec-{i}-edit"),
                                format!("{}?section={}", article_url(&article.id), section.id),
                                "edit",
                            ))
                            .text("]"),
                    ),
            )
            .child(el("p").id(format!("sec-{i}-body")).class("prose").text(section.body.as_str()))
    }))
}
fn references(article: &Article, heading: &str) -> Node {
    if article.references.is_empty() {
        return empty();
    }
    el("section")
        .class("sec refs")
        .child(div("sec-head").child(el("h2").id("refs-label").text(heading)))
        .child(el("ol").each(article.references.iter().enumerate(), |(i, reference)| {
            el("li").child(span("caret").text("^ ")).child(link(
                &format!("ref-{i}"),
                reference.url.as_str(),
                reference.label.as_str(),
            ))
        }))
}
fn see_also(s: &WikiState, article: &Article, heading: &str) -> Node {
    if article.see_also.is_empty() {
        return empty();
    }
    el("section")
        .class("sec see")
        .child(div("sec-head").child(el("h2").id("see-label").text(heading)))
        .child(el("ul").each(article.see_also.iter().enumerate(), |(i, target)| {
            let title = s
                .article(target)
                .map(|a| a.title.clone())
                .unwrap_or_else(|| target.replace('_', " "));
            el("li").child(
                el("a")
                    .id(format!("see-{i}"))
                    .attr("href", article_url(target))
                    .child(art(None, "art", &title))
                    .child(span("name").text(target.replace('_', " "))),
            )
        }))
}
fn categories(article: &Article) -> Node {
    if article.categories.is_empty() {
        return empty();
    }
    div("cats")
        .id("cats")
        .child(span("cats-label").id("cats-label").text("Categories:"))
        .each(article.categories.iter().enumerate(), |(i, category)| {
            span("cat").id(format!("cat-{i}")).text(category.as_str())
        })
}
fn latest(article: &Article) -> Node {
    match article.latest() {
        Some(latest) => el("p").id("article-latest").class("small latest").text(format!(
            "Revision {} · last edited by {} at tick {}",
            latest.rev, latest.author, latest.tick
        )),
        None => empty(),
    }
}
/// The names of an infobox row as a cast grid: a round tinted avatar and the name, linked
/// when the person has a page.
fn cast(s: &WikiState, article: &Article) -> Node {
    let Some(names) = info(article, "Starring") else {
        return empty();
    };
    el("section")
        .class("sec cast")
        .child(div("sec-head").child(el("h2").id("cast-label").text("Top cast")))
        .child(div("cast-grid").each(names.split(", ").enumerate(), |(j, name)| {
            let inner = [
                span("avatar")
                    .style(&format!("background-color: {}", tint(name)))
                    .text(initials(name)),
                span("name").text(name),
            ];
            match s.canonical(name) {
                Some((target, _)) => el("a")
                    .id(format!("cast-{j}"))
                    .class("member")
                    .attr("href", article_url(&target))
                    .children(inner),
                None => div("member").id(format!("cast-{j}")).children(inner),
            }
        }))
}

/// The full article: lead, contents, sections, references, see also, categories.
pub(crate) fn article_page(s: &WikiState, requested: &str) -> SimResult<HttpResponse> {
    let Some((id, from)) = s.canonical(requested) else {
        return missing();
    };
    let article = &s.articles[&id];
    let brand = brand_of(s);
    let title = el("h1").id("article-title").text(article.title.as_str());
    let redirect = from.as_ref().map(|alias| {
        el("p")
            .id("article-redirect")
            .class("small redirect")
            .text(format!("(Redirected from {})", alias.replace('_', " ")))
    });
    let summary = el("p").id("article-summary").class("prose lead").text(article.summary.as_str());
    let main = match s.skin_name() {
        "imdb" => {
            let meta: Vec<&str> = ["Year", "Years", "Runtime", "Episodes", "Occupation"]
                .iter()
                .filter_map(|k| info(article, k))
                .collect();
            vec![
                div("hero").child(
                    div("inner")
                        .id("article-body")
                        .child(
                            div("hero-head")
                                .child(
                                    div("hero-title")
                                        .child(title)
                                        .maybe(redirect)
                                        .child(el("p").class("meta").text(meta.join(" · "))),
                                )
                                .maybe(rating(article).map(|r| {
                                    div("rating")
                                        .id("rating")
                                        .child(span("rating-label").text(format!("{} RATING", brand.to_uppercase())))
                                        .child(
                                            span("rating-value")
                                                .child(span("star").text("★"))
                                                .child(el("b").text(format!(" {r}")))
                                                .child(span("of").text("/10")),
                                        )
                                })),
                        )
                        .child(
                            div("hero-body")
                                .child(art(Some("info-image".into()), "poster", &article.title))
                                .child(
                                    div("hero-text")
                                        .id("article-column")
                                        .child(categories(article))
                                        .child(summary)
                                        .child(infobox(s, article, false)),
                                ),
                        ),
                ),
                div("inner below")
                    .child(toc(article))
                    .child(cast(s, article))
                    .child(sections(article))
                    .child(see_also(s, article, "More like this"))
                    .child(references(article, "Related links"))
                    .child(latest(article)),
            ]
        }
        "archive" => vec![
            div(&format!("theatre mt-{}", media_type(article)))
                .child(art(Some("info-image".into()), "stage", &article.title)),
            div("inner item")
                .id("article-body")
                .child(
                    div("item-main")
                        .id("article-column")
                        .child(
                            div("item-head")
                                .child(span(&format!("mt-icon mt-{}", media_type(article))).child(el("i")))
                                .child(title),
                        )
                        .maybe(redirect)
                        .maybe(
                            ["Publisher", "Author", "Producer"]
                                .iter()
                                .find_map(|k| info(article, k))
                                .map(|by| el("p").class("by").text(format!("by {by}"))),
                        )
                        .child(summary)
                        .child(sections(article))
                        .child(references(article, "References"))
                        .child(see_also(s, article, "In collections"))
                        .child(latest(article)),
                )
                .child(
                    div("item-side")
                        .child(infobox(s, article, false))
                        .child(toc(article))
                        .child(categories(article)),
                ),
        ],
        _ => vec![
            title,
            el("p").class("siteSub").text(format!("From {brand}, the free encyclopedia")),
            div("article-body")
                .id("article-body")
                .maybe(redirect)
                .child(infobox(s, article, true))
                .child(
                    div("article-column")
                        .id("article-column")
                        .child(summary)
                        .child(toc(article))
                        .child(sections(article))
                        .child(references(article, "References"))
                        .child(see_also(s, article, "See also")),
                )
                .child(categories(article))
                .child(latest(article)),
        ],
    };
    shell(
        s,
        &format!("{} — {brand}", article.title),
        "article",
        Some(tabs(&id, "Article")),
        main,
    )
}

/// One section, with the form that rewrites it. Section editing is the mutation this site is for.
pub(crate) fn section_page(s: &WikiState, requested: &str, sid: &str) -> SimResult<HttpResponse> {
    let Some((id, _)) = s.canonical(requested) else {
        return missing();
    };
    let article = &s.articles[&id];
    let Some(section) = article.section(sid) else {
        return web::error(404, "unknown section");
    };
    let history: Vec<_> = article
        .revisions
        .iter()
        .filter(|r| r.section.as_deref() == Some(sid))
        .collect();
    let page = div("inner plain")
        .child(
            el("p").class("back").child(link(
                "section-back",
                article_url(&id),
                format!("← {}", article.title),
            )),
        )
        .child(el("h1").id("section-heading").text(section.heading.as_str()))
        .child(el("p").id("section-body").class("prose").text(section.body.as_str()))
        .child(el("h2").id("edit-label").text("Edit this section"))
        .child(
            form("edit", format!("/articles/{id}/sections/{sid}"), "post")
                .class("editor")
                .child(label("edit-body", "Section text"))
                .child(
                    el("textarea")
                        .id("edit-body")
                        .attr("name", "body")
                        .attr("rows", "10")
                        .text(section.body.as_str()),
                )
                .child(label("edit-comment", "Edit summary"))
                .child(text_input("edit-comment", "comment", "").attr(
                    "placeholder",
                    "Briefly describe your changes",
                ))
                .child(
                    el("p")
                        .class("small")
                        .text("By publishing changes, you agree to release your contribution under a free licence."),
                )
                .child(button("edit-submit", "Publish changes").class("btn primary")),
        )
        .when(!history.is_empty(), |page| {
            page.child(el("h2").id("section-hist-label").text("Revisions to this section"))
                .child(el("ul").class("revs").each(history.iter(), |revision| {
                    el("li")
                        .id(format!("section-rev-{}", revision.rev))
                        .class("small")
                        .text(format!(
                            "{} · {} · tick {} · {}",
                            revision.rev, revision.author, revision.tick, revision.comment
                        ))
                }))
        });
    shell(
        s,
        &format!("{}: {} — {}", article.title, section.heading, brand_of(s)),
        "section",
        Some(tabs(&id, "Article")),
        vec![page],
    )
}

pub(crate) fn talk_page(s: &WikiState, requested: &str) -> SimResult<HttpResponse> {
    let Some((id, _)) = s.canonical(requested) else {
        return missing();
    };
    let article = &s.articles[&id];
    let page = div("inner plain")
        .child(el("h1").id("talk-title").text(format!("Talk: {}", article.title)))
        .each(article.talk.iter().enumerate(), |(i, post)| {
            el("article")
                .id(format!("talk-{i}"))
                .class("post")
                .child(
                    div("post-head")
                        .id(format!("talk-{i}-head"))
                        .child(
                            span("avatar")
                                .id(format!("talk-{i}-avatar"))
                                .style(&format!("background-color: {}", tint(&post.author)))
                                .text(post.author.chars().next().unwrap_or('?').to_uppercase().to_string()),
                        )
                        .child(span("author").id(format!("talk-{i}-author")).text(post.author.as_str()))
                        .child(span("tick").id(format!("talk-{i}-tick")).text(format!("tick {}", post.tick))),
                )
                .child(el("p").id(format!("talk-{i}-text")).class("prose").text(post.text.as_str()))
        })
        .when(article.talk.is_empty(), |page| {
            page.child(
                el("p")
                    .id("talk-empty")
                    .class("small")
                    .text("No discussion on this article yet."),
            )
        })
        .child(
            form("reply", format!("/articles/{id}/talk"), "post")
                .class("editor")
                .child(label("reply-text", "Add a topic"))
                .child(el("textarea").id("reply-text").attr("name", "text").attr("rows", "4"))
                .child(button("reply-submit", "Add topic").class("btn primary")),
        );
    shell(
        s,
        &format!("Talk: {} — {}", article.title, brand_of(s)),
        "talk",
        Some(tabs(&id, "Talk")),
        vec![page],
    )
}

pub(crate) fn history_page(s: &WikiState, requested: &str) -> SimResult<HttpResponse> {
    let Some((id, _)) = s.canonical(requested) else {
        return missing();
    };
    let article = &s.articles[&id];
    let current = article.latest().map(|r| r.rev);
    let page = div("inner plain")
        .child(
            el("h1")
                .id("hist-title")
                .text(format!("Revision history of {}", article.title)),
        )
        .child(el("ul").class("history").each(article.revisions.iter().rev(), |revision| {
            let target = revision
                .section
                .as_ref()
                .map(|sid| format!("{}?section={sid}", article_url(&id)))
                .unwrap_or_else(|| article_url(&id));
            let rev = revision.rev;
            el("li")
                .id(format!("rev-{rev}"))
                .child(span("rev-id").id(format!("rev-{rev}-id")).text(format!("rev {rev}")))
                .child(span("tick").id(format!("rev-{rev}-tick")).text(format!("tick {}", revision.tick)))
                .child(span("author").id(format!("rev-{rev}-author")).text(revision.author.as_str()))
                .child(
                    span("comment")
                        .text("(")
                        .child(link(&format!("rev-{rev}-comment"), target, revision.comment.as_str()))
                        .text(")"),
                )
                .child(
                    span(if current == Some(rev) { "mark current" } else { "mark" })
                        .id(format!("rev-{rev}-mark"))
                        .text(if current == Some(rev) { "current" } else { "superseded" }),
                )
        }))
        .when(article.revisions.is_empty(), |page| {
            page.child(el("p").id("hist-empty").class("small").text("No revisions recorded."))
        });
    shell(
        s,
        &format!("Revision history of {} — {}", article.title, brand_of(s)),
        "history",
        Some(tabs(&id, "History")),
        vec![page],
    )
}

pub(crate) fn results_page(s: &WikiState, query: &str) -> SimResult<HttpResponse> {
    // An exact title hit is what a reader asked for; the results list is the consolation prize.
    if let Some(article) = s.article(query) {
        return article_page(s, &article.id);
    }
    let hits = s.search(query);
    let page = div("inner plain")
        .child(el("h1").id("results-title").text(format!("Search results for {query}")))
        .child(
            el("p")
                .id("results-count")
                .class("small")
                .text(format!("{} article(s) matched.", hits.len())),
        )
        .child(div("cards hits").each(hits.iter().enumerate(), |(i, hit)| {
            card(s, &format!("hit-{i}"), &s.articles[&hit.id], "snippet")
        }))
        .when(hits.is_empty(), |page| {
            page.child(
                el("p")
                    .id("results-empty")
                    .class("small")
                    .text("No article matched. Try a different wording."),
            )
        });
    shell(s, &format!("{query} — search results"), "results", None, vec![page])
}
/// A red link in real life; here it is an honest 404 rather than a page pretending to be one.
pub(crate) fn missing() -> SimResult<HttpResponse> {
    web::error(404, "article not found")
}
