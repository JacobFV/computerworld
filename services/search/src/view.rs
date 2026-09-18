//! Page rendering. The three engines serve identical routes; the skin decides what they look
//! like, which is the whole difference between a Google results page and a Bing one.
use crate::{documents, enc, recent, Document, Hit, VERTICALS};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result, Style};
use cw_service_common as web;
use serde_json::Value;
use std::collections::BTreeMap;
/// Four colours over six letters: the mark the page is recognised by before a word is read.
const GOOGLE: [&str; 6] = [
    "#4285f4", "#ea4335", "#fbbc05", "#4285f4", "#34a853", "#ea4335",
];
const DUCK: &str = "#de5833";
/// Palette and wordmark resolved once per render, so every helper reads the same colours.
pub(crate) struct Chrome {
    brand: String,
    skin: String,
    theme: PageTheme,
    accent: String,
    ink: String,
    muted: String,
    surface: String,
    paper: String,
    link: String,
}
impl Chrome {
    pub(crate) fn read(state: &Value) -> Result<Self> {
        let theme = web::theme(state)?;
        let skin = web::variant(state, "skin", crate::SKINS)?;
        let or = |value: &Option<String>, fallback: &str| {
            value.clone().unwrap_or_else(|| fallback.to_owned())
        };
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
            link: link.clone(),
            theme,
        })
    }
    /// A block that stacks its children without drawing a panel of its own.
    fn flat(&self, over: &str) -> Style {
        web::style()
            .background(over.to_owned())
            .padding(0)
            .radius(0)
    }
    fn blank(&self, id: &str) -> PageElement {
        web::styled(id, "", web::style().flex(1))
    }
    /// The wordmark, sized to its own fixed width so a row can centre it or sit beside it.
    fn mark(&self, size: u16) -> PageElement {
        let cell = u32::from(size) * 2 / 3;
        match self.skin.as_str() {
            "google" => web::styled_row(
                "mark",
                0,
                "center",
                web::style().width(cell * self.brand.chars().count() as u32),
                self.brand
                    .chars()
                    .enumerate()
                    .map(|(i, letter)| {
                        web::styled(
                            &format!("mark-{i}"),
                            letter.to_string(),
                            web::style()
                                .size(size)
                                .bold()
                                .color(GOOGLE[i % GOOGLE.len()].to_owned())
                                .align("center")
                                .width(cell),
                        )
                    })
                    .collect(),
            ),
            "ddg" => {
                let dot = u32::from(size);
                web::styled_row(
                    "mark",
                    8,
                    "center",
                    web::style().width(dot + 8 + self.wide(size)),
                    vec![
                        web::thumbnail(
                            "mark-dot",
                            "",
                            web::style()
                                .background(DUCK)
                                .width(dot)
                                .height(dot)
                                .radius(dot / 2),
                        ),
                        web::styled(
                            "mark-text",
                            self.brand.clone(),
                            web::style().size(size).bold().color(self.ink.clone()),
                        ),
                    ],
                )
            }
            _ => web::styled_row(
                "mark",
                0,
                "center",
                web::style().width(self.wide(size)),
                vec![web::styled(
                    "mark-text",
                    self.brand.clone(),
                    web::style()
                        .size(size)
                        .bold()
                        .color(self.accent.clone())
                        .align("center"),
                )],
            ),
        }
    }
    /// Rough advance width of the brand at a size; only used to centre, never to clip.
    fn wide(&self, size: u16) -> u32 {
        self.brand.chars().count() as u32 * u32::from(size) * 3 / 5
    }
    /// The query box. Its action is a GET so the results URL is the query, as it should be.
    fn box_(&self, query: &str, vertical: &str) -> PageElement {
        let target = if vertical == VERTICALS[0] {
            "/search".to_owned()
        } else {
            format!("/search?v={}", enc(vertical))
        };
        let submit = PageAction {
            method: "GET".into(),
            url: target,
            fields: BTreeMap::from([("q".to_string(), "$q".to_string())]),
        };
        let (go, lucky) = match self.skin.as_str() {
            "google" => ("Google Search", "I'm Feeling Lucky"),
            "bing" => ("Search", "Surprise me"),
            "ddg" => ("Search", "I'm Feeling Ducky"),
            _ => ("Search", "First hit"),
        };
        PageElement::Form {
            id: "search".into(),
            action: submit.clone(),
            children: vec![
                PageElement::Input {
                    id: "q".into(),
                    label: format!("Search {}", self.brand),
                    value: query.into(),
                    placeholder: "Search the web".into(),
                },
                web::row(
                    "search-buttons",
                    12,
                    "center",
                    vec![
                        PageElement::Button {
                            id: "search-go".into(),
                            text: go.into(),
                            action: submit,
                        },
                        PageElement::Button {
                            id: "search-lucky".into(),
                            text: lucky.into(),
                            action: PageAction {
                                method: "GET".into(),
                                url: "/lucky".into(),
                                fields: BTreeMap::from([("q".to_string(), "$q".to_string())]),
                            },
                        },
                    ],
                ),
            ],
        }
    }
    /// Vertical tabs: real links that re-run the same query against a filtered index.
    fn tabs(&self, verticals: &[String], query: &str, current: &str) -> PageElement {
        web::row(
            "tabs",
            8,
            "center",
            verticals
                .iter()
                .map(|vertical| {
                    let on = vertical == current;
                    web::card_action(
                        &format!("tab-{vertical}"),
                        web::style()
                            .background(if on {
                                self.surface.clone()
                            } else {
                                self.paper.clone()
                            })
                            .padding(8)
                            .radius(8)
                            .width(92),
                        web::visit(format!("/search?q={}&v={}", enc(query), enc(vertical))),
                        vec![web::styled(
                            &format!("tab-{vertical}-label"),
                            label(vertical),
                            web::style()
                                .size(13)
                                .color(if on {
                                    self.accent.clone()
                                } else {
                                    self.muted.clone()
                                })
                                .align("center"),
                        )],
                    )
                })
                .collect(),
        )
    }
    /// Seeded outbound links plus the engine's own about page; nothing here is decorative.
    fn footer(&self, state: &Value) -> Vec<PageElement> {
        let mut feet = vec![web::card_action(
            "foot-about",
            self.flat(&self.paper).padding(6),
            web::visit("/about"),
            vec![web::styled(
                "foot-about-label",
                format!("About {}", self.brand),
                web::style()
                    .size(12)
                    .color(self.muted.clone())
                    .align("center"),
            )],
        )];
        for (i, entry) in state
            .get("footer")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let url = web::text(entry, "url");
            if url.is_empty() {
                continue;
            }
            feet.push(web::card_action(
                &format!("foot-{i}"),
                self.flat(&self.paper).padding(6),
                web::visit(url),
                vec![web::styled(
                    &format!("foot-{i}-label"),
                    web::text(entry, "text"),
                    web::style()
                        .size(12)
                        .color(self.muted.clone())
                        .align("center"),
                )],
            ));
        }
        vec![
            web::divider("foot-rule"),
            web::row("foot", 8, "center", feet),
        ]
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
pub(crate) fn home(state: &Value, actor: &str) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    let mut elements = vec![
        web::spacer("lead", 56),
        web::row(
            "hero",
            0,
            "center",
            vec![
                chrome.blank("hero-lead"),
                chrome.mark(46),
                chrome.blank("hero-tail"),
            ],
        ),
    ];
    let tagline = web::text(state, "tagline");
    if !tagline.is_empty() {
        elements.push(web::styled(
            "tagline",
            tagline,
            web::style()
                .size(14)
                .color(chrome.muted.clone())
                .align("center"),
        ));
    }
    elements.push(web::spacer("hero-gap", 12));
    elements.push(web::row(
        "bar",
        12,
        "center",
        vec![
            chrome.blank("bar-lead"),
            web::card(
                "bar-box",
                chrome.flat(&chrome.paper).flex(8),
                vec![chrome.box_("", VERTICALS[0])],
            ),
            chrome.blank("bar-tail"),
        ],
    ));
    let history = recent(state, actor);
    if !history.is_empty() {
        elements.push(web::styled(
            "recent-title",
            "Recent searches",
            web::style().size(12).medium().color(chrome.muted.clone()),
        ));
        for (i, query) in history.iter().enumerate() {
            elements.push(web::card_action(
                &format!("recent-{i}"),
                chrome.flat(&chrome.paper).padding(6),
                web::visit(format!("/search?q={}", enc(query))),
                vec![web::styled(
                    &format!("recent-{i}-label"),
                    query.clone(),
                    web::style().size(13).color(chrome.link.clone()).one_line(),
                )],
            ));
        }
        elements.push(PageElement::Button {
            id: "recent-clear".into(),
            text: "Clear recent searches".into(),
            action: PageAction {
                method: "POST".into(),
                url: "/history/clear".into(),
                fields: BTreeMap::new(),
            },
        });
    }
    let trending = web::strings(state, "trending");
    if !trending.is_empty() {
        elements.push(web::styled(
            "trend-title",
            "Trending searches",
            web::style().size(12).medium().color(chrome.muted.clone()),
        ));
        elements.push(web::grid(
            "trend",
            2,
            10,
            trending
                .iter()
                .enumerate()
                .map(|(i, query)| {
                    web::card_action(
                        &format!("trend-{i}"),
                        web::style()
                            .background(chrome.surface.clone())
                            .padding(10)
                            .radius(10),
                        web::visit(format!("/search?q={}", enc(query))),
                        vec![web::styled(
                            &format!("trend-{i}-label"),
                            query.clone(),
                            web::style().size(13).color(chrome.ink.clone()).one_line(),
                        )],
                    )
                })
                .collect(),
        ));
    }
    let bangs = state.get("bangs").and_then(Value::as_object);
    if let Some(bangs) = bangs.filter(|b| !b.is_empty()) {
        elements.push(web::styled(
            "bang-title",
            "Bang shortcuts jump straight to another site",
            web::style().size(12).medium().color(chrome.muted.clone()),
        ));
        // Inert by design: a bang is typed into the box, so a card here would be a fake control.
        elements.push(web::grid(
            "bangs",
            2,
            10,
            bangs
                .iter()
                .map(|(tag, entry)| {
                    web::card(
                        &format!("bang-{tag}"),
                        web::style()
                            .background(chrome.surface.clone())
                            .padding(10)
                            .radius(10),
                        vec![web::styled(
                            &format!("bang-{tag}-label"),
                            format!("!{tag} — {}", web::text(entry, "title")),
                            web::style().size(13).color(chrome.ink.clone()).one_line(),
                        )],
                    )
                })
                .collect(),
        ));
    }
    elements.extend(chrome.footer(state));
    web::themed_page(&chrome.brand, chrome.theme.clone(), elements)
}
pub(crate) fn results(
    state: &Value,
    query: &str,
    vertical: &str,
    hits: &[Hit],
    note: Option<String>,
) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    let verticals = match web::strings(state, "verticals") {
        v if v.is_empty() => VERTICALS.iter().map(|v| (*v).to_owned()).collect(),
        v => v,
    };
    let mut elements = vec![
        web::row(
            "head",
            16,
            "center",
            vec![
                chrome.mark(22),
                web::card(
                    "head-box",
                    chrome.flat(&chrome.paper).flex(6),
                    vec![chrome.box_(query, vertical)],
                ),
            ],
        ),
        chrome.tabs(&verticals, query, vertical),
        web::divider("head-rule"),
    ];
    if let Some(note) = note {
        elements.push(web::styled(
            "note",
            note,
            web::style().size(12).color(chrome.accent.clone()),
        ));
    }
    elements.push(web::styled(
        "stats",
        match chrome.skin.as_str() {
            "google" => format!("About {} results", hits.len()),
            "ddg" => format!("{} results for {query}", hits.len()),
            _ => format!("{} results", hits.len()),
        },
        web::style().size(12).color(chrome.muted.clone()),
    ));
    if hits.is_empty() {
        elements.push(web::styled(
            "empty",
            format!("Your search - {query} - did not match any documents."),
            web::style().size(14).color(chrome.ink.clone()),
        ));
        elements.push(web::styled(
            "empty-hint",
            "Try different keywords, or fewer of them.",
            web::style().size(13).color(chrome.muted.clone()),
        ));
    }
    match vertical {
        "images" => elements.push(web::grid(
            "images",
            3,
            12,
            hits.iter()
                .enumerate()
                .map(|(i, hit)| tile(i, hit))
                .collect(),
        )),
        "videos" => elements.extend(
            hits.iter()
                .enumerate()
                .map(|(i, hit)| reel(&chrome, i, hit)),
        ),
        "news" => elements.extend(
            hits.iter()
                .enumerate()
                .map(|(i, hit)| story(&chrome, i, hit)),
        ),
        _ => elements.extend(
            hits.iter()
                .enumerate()
                .map(|(i, hit)| snippet(&chrome, i, hit)),
        ),
    }
    elements.extend(chrome.footer(state));
    web::themed_page(
        &format!("{query} - {}", chrome.brand),
        chrome.theme.clone(),
        elements,
    )
}
/// The classic web result: breadcrumb, blue title, two lines of description, all one link.
fn snippet(chrome: &Chrome, i: usize, hit: &Hit) -> PageElement {
    let id = format!("hit-{i}");
    web::card_action(
        &id,
        chrome.flat(&chrome.paper).padding(6),
        web::visit(hit.document.url.clone()),
        vec![
            web::styled(
                &format!("{id}-site"),
                crumb(&hit.document),
                web::style().size(12).color(chrome.muted.clone()).one_line(),
            ),
            web::styled(
                &format!("{id}-title"),
                hit.document.title.clone(),
                web::style().size(18).medium().color(chrome.link.clone()),
            ),
            web::styled(
                &format!("{id}-snippet"),
                hit.document.snippet.clone(),
                web::style().size(13).color(chrome.ink.clone()),
            ),
        ],
    )
}
/// Images are tiles; the artwork is a flat tint of the title, honestly synthetic.
fn tile(i: usize, hit: &Hit) -> PageElement {
    web::thumbnail_action(
        &format!("shot-{i}"),
        hit.document.title.clone(),
        web::style().height(128).radius(10),
        web::visit(hit.document.url.clone()),
    )
}
fn reel(chrome: &Chrome, i: usize, hit: &Hit) -> PageElement {
    let id = format!("reel-{i}");
    web::card_action(
        &id,
        chrome.flat(&chrome.paper).padding(6),
        web::visit(hit.document.url.clone()),
        vec![web::row(
            &format!("{id}-row"),
            12,
            "start",
            vec![
                web::thumbnail(
                    &format!("{id}-art"),
                    hit.document.site.clone(),
                    web::style().width(150).height(86).radius(8),
                ),
                web::card(
                    &format!("{id}-text"),
                    chrome.flat(&chrome.paper).flex(3),
                    vec![
                        web::styled(
                            &format!("{id}-title"),
                            hit.document.title.clone(),
                            web::style().size(16).medium().color(chrome.link.clone()),
                        ),
                        web::styled(
                            &format!("{id}-site"),
                            crumb(&hit.document),
                            web::style().size(12).color(chrome.muted.clone()).one_line(),
                        ),
                        web::styled(
                            &format!("{id}-snippet"),
                            hit.document.snippet.clone(),
                            web::style().size(13).color(chrome.ink.clone()),
                        ),
                    ],
                ),
            ],
        )],
    )
}
fn story(chrome: &Chrome, i: usize, hit: &Hit) -> PageElement {
    let id = format!("news-{i}");
    web::card_action(
        &id,
        web::style()
            .background(chrome.surface.clone())
            .padding(12)
            .radius(12),
        web::visit(hit.document.url.clone()),
        vec![web::row(
            &format!("{id}-row"),
            12,
            "start",
            vec![
                web::thumbnail(
                    &format!("{id}-art"),
                    hit.document.site.clone(),
                    web::style().width(96).height(72).radius(8),
                ),
                web::card(
                    &format!("{id}-text"),
                    chrome.flat(&chrome.surface).flex(3),
                    vec![
                        web::badge(
                            &format!("{id}-source"),
                            hit.document.site.clone(),
                            web::style()
                                .background(chrome.accent.clone())
                                .color("#ffffff")
                                .size(10)
                                .padding(4)
                                .radius(4),
                        ),
                        web::styled(
                            &format!("{id}-title"),
                            hit.document.title.clone(),
                            web::style().size(16).medium().color(chrome.ink.clone()),
                        ),
                        web::styled(
                            &format!("{id}-snippet"),
                            hit.document.snippet.clone(),
                            web::style().size(13).color(chrome.muted.clone()),
                        ),
                    ],
                ),
            ],
        )],
    )
}
/// A bang is a jump, so the page is the jump and nothing else.
pub(crate) fn jump(
    state: &Value,
    query: &str,
    tag: &str,
    title: &str,
    url: &str,
) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    web::themed_page(
        &format!("!{tag} - {}", chrome.brand),
        chrome.theme.clone(),
        vec![
            web::row(
                "head",
                16,
                "center",
                vec![
                    chrome.mark(22),
                    web::card(
                        "head-box",
                        chrome.flat(&chrome.paper).flex(6),
                        vec![chrome.box_(query, VERTICALS[0])],
                    ),
                ],
            ),
            web::divider("head-rule"),
            web::card_action(
                "jump",
                web::style()
                    .background(chrome.surface.clone())
                    .padding(14)
                    .radius(12),
                web::visit(url.to_owned()),
                vec![
                    web::styled(
                        "jump-tag",
                        format!("!{tag} → {title}"),
                        web::style().size(12).color(chrome.muted.clone()),
                    ),
                    web::styled(
                        "jump-url",
                        url.to_owned(),
                        web::style().size(17).medium().color(chrome.link.clone()),
                    ),
                ],
            ),
            web::styled(
                "jump-hint",
                "Bang shortcuts skip the results page and go straight to the site.",
                web::style().size(12).color(chrome.muted.clone()),
            ),
        ],
    )
}
pub(crate) fn lucky(state: &Value, query: &str, top: Option<&Hit>) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    let mut elements = vec![
        web::spacer("lead", 32),
        web::row(
            "hero",
            0,
            "center",
            vec![
                chrome.blank("hero-lead"),
                chrome.mark(34),
                chrome.blank("hero-tail"),
            ],
        ),
    ];
    match top {
        Some(hit) => {
            elements.push(web::styled(
                "lucky-lead",
                format!("Top result for {query}"),
                web::style()
                    .size(13)
                    .color(chrome.muted.clone())
                    .align("center"),
            ));
            elements.push(web::styled(
                "lucky-title",
                hit.document.title.clone(),
                web::style()
                    .size(20)
                    .medium()
                    .color(chrome.link.clone())
                    .align("center"),
            ));
            elements.push(PageElement::Button {
                id: "lucky-go".into(),
                text: format!("Go to {}", crumb(&hit.document)),
                action: web::visit(hit.document.url.clone()),
            });
        }
        None => elements.push(web::styled(
            "lucky-empty",
            format!("Nothing in the index matches {query}."),
            web::style()
                .size(14)
                .color(chrome.ink.clone())
                .align("center"),
        )),
    }
    elements.extend(chrome.footer(state));
    web::themed_page(
        &format!("Lucky - {}", chrome.brand),
        chrome.theme.clone(),
        elements,
    )
}
pub(crate) fn about(state: &Value) -> Result<HttpResponse> {
    let chrome = Chrome::read(state)?;
    let indexed = documents(state);
    let verticals = match web::strings(state, "verticals") {
        v if v.is_empty() => VERTICALS.iter().map(|v| (*v).to_owned()).collect(),
        v => v,
    };
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
        (
            "pages",
            "Pages indexed".to_owned(),
            indexed.len().to_string(),
        ),
        ("sites", "Sites covered".to_owned(), sites.to_string()),
        (
            "tabs",
            "Verticals".to_owned(),
            verticals
                .iter()
                .map(|v| label(v))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        (
            "history",
            "Recent searches".to_owned(),
            if state
                .get("retain_history")
                .and_then(Value::as_bool)
                .unwrap_or(true)
            {
                "kept per signed-in person".to_owned()
            } else {
                "never stored".to_owned()
            },
        ),
    ];
    let mut elements = vec![
        web::spacer("lead", 24),
        web::row(
            "hero",
            0,
            "center",
            vec![
                chrome.blank("hero-lead"),
                chrome.mark(34),
                chrome.blank("hero-tail"),
            ],
        ),
        web::spacer("hero-gap", 8),
        web::styled(
            "about-lead",
            about,
            web::style().size(14).color(chrome.ink.clone()),
        ),
        web::grid(
            "facts",
            2,
            12,
            facts
                .iter()
                .map(|(id, name, value)| {
                    web::card(
                        &format!("fact-{id}"),
                        web::style()
                            .background(chrome.surface.clone())
                            .padding(12)
                            .radius(12),
                        vec![
                            web::styled(
                                &format!("fact-{id}-name"),
                                name.clone(),
                                web::style().size(11).color(chrome.muted.clone()),
                            ),
                            web::styled(
                                &format!("fact-{id}-value"),
                                value.clone(),
                                web::style().size(15).medium().color(chrome.ink.clone()),
                            ),
                        ],
                    )
                })
                .collect(),
        ),
        web::card_action(
            "about-home",
            web::style()
                .background(chrome.accent.clone())
                .padding(12)
                .radius(10),
            web::visit("/"),
            vec![web::styled(
                "about-home-label",
                format!("Back to {}", chrome.brand),
                web::style()
                    .size(13)
                    .medium()
                    .color("#ffffff")
                    .align("center"),
            )],
        ),
    ];
    elements.extend(chrome.footer(state));
    web::themed_page(
        &format!("About {}", chrome.brand),
        chrome.theme.clone(),
        elements,
    )
}
