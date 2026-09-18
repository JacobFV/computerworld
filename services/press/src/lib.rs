//! Publications: reuters.com (`wire`), theverge.com and arstechnica.com (`magazine`), and the
//! three personal/company blogs (`blog`). `layout` picks the front page, not the data shape.
//!
//! Reading is pure; everything a reader does — commenting, liking a comment, saving an article,
//! following a topic or the publication itself, subscribing to the newsletter — is a POST that
//! really mutates state and survives a snapshot round trip.
use cw_protocol::{HttpRequest, HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Value};
pub struct PressService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(PressService)
}
/// Documented seed keys, checked by container type only — the crate that fills them owns the rest.
const OBJECTS: &[&str] = &["theme", "articles", "saved", "follows"];
const ARRAYS: &[&str] = &["sections", "subscribers"];
/// `layout` is the documented discriminant; an unlisted value is a seed typo, not a fallback.
const LAYOUTS: &[&str] = &["wire", "magazine", "blog"];
const BRAND: &str = "Press";
const TAGLINE: &str = "Today's reporting.";
/// The publication itself is a follow target alongside the topics, under a token no tag uses.
const PUBLICATION: &str = "publication";
/// Flat stand-in tints for art; a stable hash of the slug keeps a story the same colour forever.
const TINTS: &[&str] = &[
    "#dbe4f0", "#f0e2db", "#dcefe4", "#eee0ef", "#e6e6f2", "#f2ece0", "#dfeef2", "#eae4f2",
];
fn tint(seed: &str) -> &'static str {
    let hash = seed
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    TINTS[hash as usize % TINTS.len()]
}
fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}
fn article<'a>(state: &'a Value, id: &str) -> Option<&'a Value> {
    state.get("articles")?.get(id)
}
fn ids(state: &Value) -> Vec<String> {
    match state.get("articles").and_then(Value::as_object) {
        Some(m) => m.keys().cloned().collect(),
        None => vec![],
    }
}
/// Newest first, id breaking ties — every list on every layout shares this order.
fn recent(state: &Value, keep: impl Fn(&Value) -> bool) -> Vec<String> {
    let mut all: Vec<String> = ids(state)
        .into_iter()
        .filter(|id| article(state, id).is_some_and(&keep))
        .collect();
    all.sort_by_key(|id| {
        let a = article(state, id).cloned().unwrap_or(Value::Null);
        (std::cmp::Reverse(num(&a, "tick")), id.clone())
    });
    all
}
fn listed(state: &Value, map: &str, key: &str) -> Vec<String> {
    match state.get(map).and_then(|m| m.get(key)) {
        Some(v) => web::strings(&json!({ "v": v }), "v"),
        None => vec![],
    }
}
fn has(state: &Value, map: &str, key: &str, needle: &str) -> bool {
    listed(state, map, key).iter().any(|v| v == needle)
}
/// Saving and following are both per-actor toggles; one place to flip them.
fn toggle(state: &mut Value, map: &str, key: &str, value: &str) -> bool {
    let list = state
        .as_object_mut()
        .expect("state is an object")
        .entry(map)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .map(|m| m.entry(key).or_insert_with(|| json!([])))
        .and_then(Value::as_array_mut);
    let Some(list) = list else { return false };
    match list.iter().position(|v| v.as_str() == Some(value)) {
        Some(at) => {
            list.remove(at);
            false
        }
        None => {
            list.push(json!(value));
            true
        }
    }
}
fn post(url: String, fields: &[(&str, &str)]) -> PageAction {
    PageAction {
        method: "POST".into(),
        url,
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).to_owned()))
            .collect(),
    }
}
fn ink(theme: &PageTheme) -> String {
    theme.ink.clone().unwrap_or_else(|| "#08090a".into())
}
fn muted(theme: &PageTheme) -> String {
    theme.muted.clone().unwrap_or_else(|| "#606468".into())
}
fn surface(theme: &PageTheme) -> String {
    theme.surface.clone().unwrap_or_else(|| "#f5f5f7".into())
}
fn accent(theme: &PageTheme) -> String {
    theme.accent.clone().unwrap_or_else(|| "#5200ff".into())
}
/// A pill that really submits; `on` is the engaged state, which is what makes a toggle legible.
fn pill(id: &str, text: &str, on: bool, theme: &PageTheme, action: PageAction) -> PageElement {
    web::card_action(
        id,
        web::style().padding(9).radius(16).background(if on {
            accent(theme)
        } else {
            surface(theme)
        }),
        action,
        vec![web::styled(
            &format!("{id}-text"),
            text,
            web::style().size(13).medium().color(if on {
                "#ffffff".to_owned()
            } else {
                ink(theme)
            }),
        )],
    )
}
/// Art stand-in: a tinted block carrying its own headline, never claiming to be a photograph.
fn art(id: &str, label: &str, height: u32) -> PageElement {
    web::thumbnail(
        id,
        label,
        web::style()
            .height(height)
            .radius(6)
            .background(tint(id))
            .color("#20242c")
            .align("center"),
    )
}
fn section_title(state: &Value, id: &str) -> String {
    state
        .get("sections")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|s| web::text(s, "id") == id))
        .map(|s| web::text(s, "title"))
        .unwrap_or_else(|| id.to_owned())
}
fn sections(state: &Value) -> Vec<String> {
    state
        .get("sections")
        .and_then(Value::as_array)
        .map(|list| list.iter().map(|s| web::text(s, "id")).collect())
        .unwrap_or_default()
}
/// The masthead: wordmark, section nav, reading list and the publication Follow control.
fn masthead(state: &Value, actor: &str, theme: &PageTheme, layout: &str) -> PageElement {
    let brand = match web::text(state, "brand").as_str() {
        "" => BRAND.to_owned(),
        s => s.to_owned(),
    };
    let following = has(state, "follows", actor, PUBLICATION);
    let mut nav: Vec<PageElement> = vec![web::card_action(
        "masthead-home",
        web::style().width(if layout == "blog" { 220 } else { 260 }),
        web::visit("/"),
        vec![web::styled(
            "masthead-brand",
            brand,
            web::style()
                .size(if layout == "blog" { 22 } else { 26 })
                .bold()
                .color(if layout == "wire" {
                    accent(theme)
                } else {
                    ink(theme)
                }),
        )],
    )];
    for id in sections(state) {
        nav.push(web::link(
            &format!("masthead-{id}"),
            section_title(state, &id),
            format!("/{id}"),
        ));
    }
    nav.push(web::link("masthead-archive", "Archive", "/archive"));
    nav.push(web::link("masthead-saved", "Reading list", "/saved"));
    nav.push(pill(
        "masthead-follow",
        if following { "Following" } else { "Follow" },
        following,
        theme,
        post("/follow".into(), &[("return", "/")]),
    ));
    web::styled_row(
        "masthead",
        14,
        "center",
        web::style().background(surface(theme)).padding(14),
        nav,
    )
}
/// "Tom Weber · Mar 4, 2026 · 7 min read", with the parts a seed left out simply absent.
fn byline(state: &Value, id: &str) -> String {
    let a = article(state, id).cloned().unwrap_or(Value::Null);
    let date = web::text(&a, "date");
    let minutes = num(&a, "read_minutes");
    match (date.is_empty(), minutes) {
        (true, 0) => web::text(&a, "byline"),
        (true, m) => format!("{} · {m} min read", web::text(&a, "byline")),
        (false, 0) => format!("{} · {date}", web::text(&a, "byline")),
        (false, m) => format!("{} · {date} · {m} min read", web::text(&a, "byline")),
    }
}
/// The canonical path for an article: dated for the wires and magazines, `/posts/` for blogs.
fn href(state: &Value, id: &str, layout: &str) -> String {
    match layout {
        "blog" => format!("/posts/{id}"),
        _ => format!(
            "/{}/{id}",
            match web::text(article(state, id).unwrap_or(&Value::Null), "year") {
                y if y.is_empty() => "2026".to_owned(),
                y => y,
            }
        ),
    }
}
/// A headline card sized by `scale`: 0 is a dense wire line, 1 a grid card, 2 the hero.
fn headline(state: &Value, id: &str, theme: &PageTheme, layout: &str, scale: u8) -> PageElement {
    let a = article(state, id).cloned().unwrap_or(Value::Null);
    let title = web::text(&a, "title");
    let mut children = vec![];
    if scale > 0 {
        children.push(art(
            &format!("card-{id}-art"),
            &title,
            if scale > 1 { 260 } else { 150 },
        ));
    }
    children.push(web::styled_row(
        &format!("card-{id}-kicker"),
        8,
        "center",
        web::style(),
        vec![
            web::badge(
                &format!("card-{id}-section"),
                section_title(state, &web::text(&a, "section")).to_uppercase(),
                web::style()
                    .size(10)
                    .medium()
                    .color(accent(theme))
                    .padding(4),
            ),
            web::styled(
                &format!("card-{id}-date"),
                web::text(&a, "date"),
                web::style().size(11).color(muted(theme)),
            ),
        ],
    ));
    children.push(web::styled(
        &format!("card-{id}-title"),
        &title,
        web::style()
            .size(match scale {
                0 => 15,
                1 => 18,
                _ => 30,
            })
            .bold()
            .color(ink(theme)),
    ));
    if scale > 0 {
        children.push(web::styled(
            &format!("card-{id}-dek"),
            web::text(&a, "dek"),
            web::style().size(14).color(muted(theme)),
        ));
    }
    children.push(web::styled(
        &format!("card-{id}-byline"),
        byline(state, id),
        web::style().size(12).color(muted(theme)),
    ));
    web::card_action(
        &format!("card-{id}"),
        web::style().padding(if scale > 0 { 12 } else { 8 }),
        web::visit(href(state, id, layout)),
        children,
    )
}
fn newsletter(theme: &PageTheme) -> Vec<PageElement> {
    vec![
        web::divider("newsletter-divider"),
        web::styled(
            "newsletter-heading",
            "Get the newsletter",
            web::style().size(16).bold().color(ink(theme)).padding(8),
        ),
        web::form("subscribe", "/subscribe", &[("email", "Email address", "")]),
    ]
}
fn front(state: &Value, actor: &str, theme: &PageTheme, layout: &str) -> Result<HttpResponse> {
    let all = recent(state, |_| true);
    let mut elements = vec![masthead(state, actor, theme, layout)];
    match layout {
        // The wire: a dated stack, newest first, with the same five words of chrome all day.
        "wire" => {
            elements.push(web::styled(
                "front-heading",
                "Latest",
                web::style().size(18).bold().color(ink(theme)).padding(12),
            ));
            let lines = all
                .iter()
                .map(|id| headline(state, id, theme, layout, 0))
                .collect::<Vec<_>>();
            let most_read = all
                .iter()
                .take(3)
                .map(|id| {
                    web::link(
                        &format!("mostread-{id}"),
                        web::text(article(state, id).unwrap_or(&Value::Null), "title"),
                        href(state, id, layout),
                    )
                })
                .collect::<Vec<_>>();
            elements.push(web::styled_row(
                "front-layout",
                24,
                "start",
                web::style().padding(12),
                vec![
                    web::styled_row("front-lines", 2, "start", web::style().flex(3), lines),
                    web::styled_row(
                        "front-side",
                        8,
                        "start",
                        web::style()
                            .flex(1)
                            .background(surface(theme))
                            .padding(12)
                            .radius(6),
                        std::iter::once(web::styled(
                            "front-side-heading",
                            "Most read",
                            web::style().size(14).bold().color(ink(theme)),
                        ))
                        .chain(most_read)
                        .collect(),
                    ),
                ],
            ));
        }
        // The magazine: one hero, then a two-up grid.
        "magazine" => {
            if let Some(hero) = all.first() {
                elements.push(headline(state, hero, theme, layout, 2));
            }
            elements.push(web::grid(
                "front-grid",
                2,
                20,
                all.iter()
                    .skip(1)
                    .map(|id| headline(state, id, theme, layout, 1))
                    .collect(),
            ));
        }
        // The blog: a masthead line, then posts in reverse chronological order.
        _ => {
            elements.push(web::styled(
                "front-tagline",
                web::text(state, "tagline"),
                web::style().size(15).color(muted(theme)).padding(12),
            ));
            for id in &all {
                elements.push(headline(state, id, theme, layout, 1));
                elements.push(web::divider(&format!("front-rule-{id}")));
            }
        }
    }
    elements.extend(newsletter(theme));
    web::themed_page(&web::text(state, "brand"), theme.clone(), elements)
}
fn list_page(
    state: &Value,
    actor: &str,
    theme: &PageTheme,
    layout: &str,
    title: &str,
    entries: &[String],
    follow: Option<&str>,
) -> Result<HttpResponse> {
    let mut elements = vec![
        masthead(state, actor, theme, layout),
        web::styled_row(
            "list-header",
            12,
            "center",
            web::style().padding(12),
            std::iter::once(web::styled(
                "list-title",
                title,
                web::style().size(24).bold().color(ink(theme)),
            ))
            .chain(follow.map(|tag| {
                let on = has(state, "follows", actor, tag);
                pill(
                    "list-follow",
                    if on { "Following" } else { "Follow topic" },
                    on,
                    theme,
                    post(
                        format!("/tags/{tag}/follow"),
                        &[("return", &format!("/tag/{tag}"))],
                    ),
                )
            }))
            .collect(),
        ),
    ];
    if entries.is_empty() {
        elements.push(web::styled(
            "list-empty",
            "Nothing here yet.",
            web::style().size(14).color(muted(theme)).padding(12),
        ));
    }
    for id in entries {
        elements.push(headline(state, id, theme, layout, 1));
        elements.push(web::divider(&format!("list-rule-{id}")));
    }
    web::themed_page(title, theme.clone(), elements)
}
fn article_page(
    state: &Value,
    id: &str,
    actor: &str,
    theme: &PageTheme,
    layout: &str,
) -> Result<HttpResponse> {
    let Some(a) = article(state, id).cloned() else {
        return web::error(404, "article not found");
    };
    let back = href(state, id, layout);
    let saved = has(state, "saved", actor, id);
    let title = web::text(&a, "title");
    let mut body = vec![
        masthead(state, actor, theme, layout),
        web::styled_row(
            "article-kicker",
            10,
            "center",
            web::style().padding(12),
            vec![
                web::link(
                    "article-section",
                    section_title(state, &web::text(&a, "section")),
                    format!("/{}", web::text(&a, "section")),
                ),
                web::styled(
                    "article-date",
                    web::text(&a, "date"),
                    web::style().size(12).color(muted(theme)),
                ),
            ],
        ),
        web::styled(
            "article-title",
            &title,
            web::style().size(32).bold().color(ink(theme)),
        ),
        web::styled(
            "article-dek",
            web::text(&a, "dek"),
            web::style().size(17).color(muted(theme)),
        ),
        web::styled_row(
            "article-byline",
            12,
            "center",
            web::style().padding(8),
            vec![
                web::thumbnail(
                    "article-avatar",
                    web::text(&a, "byline"),
                    web::style()
                        .width(40)
                        .height(40)
                        .radius(20)
                        .background(tint(&web::text(&a, "byline")))
                        .color("#20242c")
                        .align("center"),
                ),
                web::styled(
                    "article-credit",
                    byline(state, id),
                    web::style().size(13).color(muted(theme)).flex(3),
                ),
                pill(
                    "article-save",
                    if saved { "Saved" } else { "Save" },
                    saved,
                    theme,
                    post(format!("/articles/{id}/save"), &[("return", &back)]),
                ),
            ],
        ),
        art("article-art", &title, 280),
    ];
    for (index, paragraph) in a
        .get("body")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let text = paragraph.as_str().unwrap_or_default();
        body.push(web::styled(
            &format!("article-p{index}"),
            text,
            web::style().size(16).color(ink(theme)),
        ));
        body.extend(web::links(&format!("article-p{index}"), text));
    }
    let related: Vec<PageElement> = a
        .get("links")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, l)| {
            web::link(
                &format!("article-ref-{i}"),
                web::text(l, "label"),
                web::text(l, "url"),
            )
        })
        .collect();
    if !related.is_empty() {
        body.push(web::card(
            "article-links",
            web::style()
                .background(surface(theme))
                .padding(12)
                .radius(6),
            std::iter::once(web::styled(
                "article-links-heading",
                "Read more",
                web::style().size(13).bold().color(ink(theme)),
            ))
            .chain(related)
            .collect(),
        ));
    }
    let tags: Vec<PageElement> = web::strings(&a, "tags")
        .iter()
        .map(|tag| {
            web::card_action(
                &format!("article-tag-{tag}"),
                web::style()
                    .padding(7)
                    .radius(12)
                    .background(surface(theme)),
                web::visit(format!("/tag/{tag}")),
                vec![web::styled(
                    &format!("article-tag-{tag}-text"),
                    format!("#{tag}"),
                    web::style().size(12).color(ink(theme)),
                )],
            )
        })
        .collect();
    body.push(web::styled_row(
        "article-tags",
        8,
        "center",
        web::style().padding(8),
        tags,
    ));
    body.push(web::divider("article-divider"));
    let comments = a
        .get("comments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    body.push(web::styled(
        "comments-heading",
        format!("{} comments", comments.len()),
        web::style().size(16).bold().color(ink(theme)),
    ));
    body.push(web::form(
        "comment",
        &format!("/articles/{id}/comments"),
        &[("text", "Join the discussion", "")],
    ));
    for comment in comments {
        let cid = web::text(&comment, "id");
        body.push(web::styled_row(
            &format!("comment-{cid}"),
            12,
            "start",
            web::style().padding(8),
            vec![
                web::styled_row(
                    &format!("comment-{cid}-body"),
                    4,
                    "start",
                    web::style().flex(4),
                    vec![
                        web::styled(
                            &format!("comment-{cid}-author"),
                            web::text(&comment, "author"),
                            web::style().size(13).medium().color(ink(theme)),
                        ),
                        web::styled(
                            &format!("comment-{cid}-text"),
                            web::text(&comment, "text"),
                            web::style().size(14).color(ink(theme)),
                        ),
                    ],
                ),
                pill(
                    &format!("comment-{cid}-like"),
                    &format!("▲ {}", num(&comment, "likes")),
                    false,
                    theme,
                    post(
                        format!("/articles/{id}/comments/{cid}/like"),
                        &[("return", &back)],
                    ),
                ),
            ],
        ));
    }
    body.extend(newsletter(theme));
    web::themed_page(&title, theme.clone(), body)
}
/// Brand splash; nothing on it reads as a control, so no action is promised that does not exist.
fn landing(state: &Value) -> Result<HttpResponse> {
    let pick = |key: &str, fallback: &str| match web::text(state, key) {
        s if s.is_empty() => fallback.to_owned(),
        s => s,
    };
    web::brand_page(
        &pick("brand", BRAND),
        &pick("tagline", TAGLINE),
        web::theme(state)?,
    )
}
/// Every GET route. Reading never mutates, so this takes the state by reference.
fn render(state: &Value, ctx: &ServiceContext, request: &HttpRequest) -> Result<HttpResponse> {
    let theme = web::theme(state)?;
    let layout = web::variant(state, "layout", LAYOUTS)?;
    let actor = ctx.actor.as_str();
    let path = web::path(request);
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        [""] if state.get("articles").is_none() => landing(state),
        [""] => front(state, actor, &theme, &layout),
        ["archive"] => list_page(
            state,
            actor,
            &theme,
            &layout,
            "Archive",
            &recent(state, |_| true),
            None,
        ),
        ["saved"] => list_page(
            state,
            actor,
            &theme,
            &layout,
            "Reading list",
            &recent(state, |a| has(state, "saved", actor, &web::text(a, "id"))),
            None,
        ),
        ["tag", tag] => list_page(
            state,
            actor,
            &theme,
            &layout,
            &format!("#{tag}"),
            &recent(state, |a| web::strings(a, "tags").iter().any(|t| t == tag)),
            Some(tag),
        ),
        ["posts", slug] => article_page(state, slug, actor, &theme, &layout),
        [section] if sections(state).iter().any(|s| s == section) => list_page(
            state,
            actor,
            &theme,
            &layout,
            &section_title(state, section),
            &recent(state, |a| web::text(a, "section") == *section),
            None,
        ),
        [slug] | [_, slug] if article(state, slug).is_some() => {
            article_page(state, slug, actor, &theme, &layout)
        }
        _ => web::error(404, "route not found"),
    }
}
/// Ids are dense and never recycled, so the next one is simply past the highest in use.
fn next_comment_id(comments: &[Value]) -> String {
    let highest = comments
        .iter()
        .filter_map(|c| web::text(c, "id").strip_prefix('c')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("c{}", highest + 1)
}
/// Liking a comment is the same work whichever of the two documented routes asked for it.
fn like_comment(
    state: &mut Value,
    id: &str,
    comment: &str,
    layout: &str,
) -> std::result::Result<(Value, String), String> {
    let back = href(state, id, layout);
    let Some(a) = article_mut(state, id) else {
        return Err("article not found".into());
    };
    let found = a
        .get_mut("comments")
        .and_then(Value::as_array_mut)
        .and_then(|list| list.iter_mut().find(|c| web::text(c, "id") == *comment));
    match found {
        None => Err("comment not found".into()),
        Some(c) => {
            c["likes"] = json!(num(c, "likes") + 1);
            Ok((c.clone(), back))
        }
    }
}
fn article_mut<'a>(state: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    state.get_mut("articles")?.as_object_mut()?.get_mut(id)
}
impl Service for PressService {
    fn kind(&self) -> &str {
        "press"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let state = web::shape(initial, OBJECTS, ARRAYS)?;
        web::variant(&state, "layout", LAYOUTS)?;
        web::theme(&state)?;
        Ok(state)
    }
    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        request: &HttpRequest,
    ) -> Result<HttpResponse> {
        let method = request.method.to_ascii_uppercase();
        if method == "GET" {
            return render(state, ctx, request);
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let path = web::path(request);
        let body = web::body(request)?;
        let api = path.starts_with("/api/");
        let trimmed = path.trim_matches('/');
        let route: Vec<&str> = trimmed
            .strip_prefix("api/")
            .unwrap_or(trimmed)
            .split('/')
            .collect();
        let layout = web::variant(state, "layout", LAYOUTS)?;
        let result: std::result::Result<(Value, String), String> = match route.as_slice() {
            ["articles", id, "comments"] => {
                let text = web::text(&body, "text");
                if text.trim().is_empty() {
                    Err("comment text is required".into())
                } else if let Some(a) = article_mut(state, id) {
                    let mut comments = a
                        .get("comments")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let comment = json!({
                        "id": next_comment_id(&comments),
                        "author": ctx.actor,
                        "text": text,
                        "tick": ctx.tick,
                        "likes": 0,
                    });
                    comments.push(comment.clone());
                    a["comments"] = Value::Array(comments);
                    Ok((comment, href(state, id, &layout)))
                } else {
                    Err("article not found".into())
                }
            }
            ["articles", id, "comments", comment, "like"] => {
                like_comment(state, id, comment, &layout)
            }
            // `/comments/{id}/like` is the documented short form; it needs to be told which story.
            ["comments", comment, "like"] => match web::text(&body, "article") {
                owner if owner.is_empty() => Err("article is required".into()),
                owner => like_comment(state, &owner, comment, &layout),
            },
            ["articles", id, "save"] => match article(state, id) {
                None => Err("article not found".into()),
                Some(_) => {
                    let saved = toggle(state, "saved", &ctx.actor, id);
                    Ok((json!({"saved": saved}), "/saved".into()))
                }
            },
            ["tags", tag, "follow"] => {
                let following = toggle(state, "follows", &ctx.actor, tag);
                Ok((json!({"following": following}), format!("/tag/{tag}")))
            }
            ["follow"] => {
                let following = toggle(state, "follows", &ctx.actor, PUBLICATION);
                Ok((json!({"following": following}), "/".into()))
            }
            ["subscribe"] => {
                let email = web::text(&body, "email");
                if !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
                    Err("a valid email address is required".into())
                } else {
                    match state
                        .as_object_mut()
                        .expect("state is an object")
                        .entry("subscribers")
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                    {
                        None => Err("subscribers must be an array".into()),
                        Some(list) => {
                            if !list.iter().any(|v| v.as_str() == Some(email.as_str())) {
                                list.push(json!(email));
                            }
                            Ok((json!({"subscribed": email}), "/".into()))
                        }
                    }
                }
            }
            _ => return web::error(404, "route not found"),
        };
        match result {
            Err(message) => web::domain::<Value>(Err(message)),
            Ok((value, _)) if api => HttpResponse::json(200, &value),
            // A browser control says where it came from, so a mutation lands back on that page.
            Ok((_, fallback)) => {
                let back = match web::text(&body, "return") {
                    r if r.is_empty() => fallback,
                    r => r,
                };
                render(state, ctx, &HttpRequest::get(format!("http://press{back}")))
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "alice".into(),
            source: "alice-mac".into(),
            tick: 12,
            seed: 1,
            instance: "press".into(),
        }
    }
    fn seed(layout: &str) -> Value {
        json!({
            "layout": layout, "brand": "The Testpaper", "tagline": "All the news that fits.",
            "theme": {"accent": "#5200ff", "background": "#ffffff", "surface": "#f5f5f7",
                      "ink": "#08090a", "muted": "#606468", "content_width": 880},
            "sections": [{"id": "tech", "title": "Tech"}, {"id": "reviews", "title": "Reviews"}],
            "articles": {
                "atlas-determinism": {
                    "id": "atlas-determinism", "section": "tech", "year": "2026",
                    "title": "Northstar's Atlas bets everything on determinism",
                    "dek": "Reproducibility is a feature, not a research project.",
                    "byline": "Tom Weber", "date": "Mar 5, 2026", "tick": 8, "read_minutes": 7,
                    "body": ["Most simulation software will tell you it is reproducible.",
                             "The repo is at http://github.com/northstar/atlas and it is worth reading."],
                    "links": [{"label": "Atlas on GitHub", "url": "http://github.com/northstar/atlas"}],
                    "tags": ["northstar", "determinism"],
                    "comments": [{"id": "c1", "author": "praman", "text": "Finally.",
                                  "tick": 8, "likes": 3}]
                },
                "monitor-roundup": {
                    "id": "monitor-roundup", "section": "reviews", "year": "2026",
                    "title": "The best 27-inch 4K USB-C monitors right now",
                    "dek": "One cable, one panel.", "byline": "Tom Weber", "date": "Feb 26, 2026",
                    "tick": 6, "read_minutes": 9, "body": ["We tested nine panels."],
                    "links": [], "tags": ["reviews"], "comments": []
                }
            },
            "subscribers": ["alice.chen@gmail.com"],
            "saved": {"bob": ["monitor-roundup"]},
            "follows": {}
        })
    }
    fn get(state: &mut Value, url: &str) -> HttpResponse {
        PressService
            .handle(state, &ctx(), &HttpRequest::get(url))
            .unwrap()
    }
    fn post(state: &mut Value, url: &str, body: Value) -> HttpResponse {
        let request = HttpRequest::json("POST", url, &body).unwrap();
        PressService.handle(state, &ctx(), &request).unwrap()
    }
    fn text(response: &HttpResponse) -> String {
        String::from_utf8(response.body.clone()).unwrap()
    }
    #[test]
    fn seed_shape_is_gated_at_load() {
        assert!(PressService.initialize(json!([]), &ctx()).is_err());
        assert!(PressService
            .initialize(json!({"layout": "wire", "articles": []}), &ctx())
            .is_err());
        assert!(PressService
            .initialize(json!({"layout": "tabloid"}), &ctx())
            .is_err());
        assert!(PressService
            .initialize(json!({"layout": "blog"}), &ctx())
            .is_ok());
    }
    #[test]
    fn each_layout_builds_its_own_front_page_out_of_the_same_articles() {
        for (layout, marker) in [
            ("wire", "front-side-heading"),
            ("magazine", "front-grid"),
            ("blog", "front-tagline"),
        ] {
            let mut state = PressService.initialize(seed(layout), &ctx()).unwrap();
            let page = text(&get(&mut state, "http://press.example/"));
            assert!(page.contains(marker), "{layout} front page");
            assert!(page.contains("Northstar's Atlas bets everything on determinism"));
            assert!(
                page.contains("newsletter-heading"),
                "{layout} offers the newsletter"
            );
        }
    }
    #[test]
    fn an_article_resolves_under_the_path_its_layout_publishes() {
        let mut dated = PressService.initialize(seed("magazine"), &ctx()).unwrap();
        assert_eq!(
            get(&mut dated, "http://press.example/2026/atlas-determinism").status,
            200
        );
        assert_eq!(
            get(&mut dated, "http://press.example/atlas-determinism").status,
            200
        );
        assert_eq!(
            get(&mut dated, "http://press.example/posts/atlas-determinism").status,
            200
        );
        let mut blog = PressService.initialize(seed("blog"), &ctx()).unwrap();
        assert_eq!(
            get(&mut blog, "http://press.example/posts/atlas-determinism").status,
            200
        );
        let page = text(&get(
            &mut dated,
            "http://press.example/2026/atlas-determinism",
        ));
        assert!(page.contains("Tom Weber · Mar 5, 2026 · 7 min read"));
        assert!(page.contains("article-ref-0"), "the Read more box is real");
        assert!(
            page.contains("article-p1-link-4"),
            "URLs in the prose become links"
        );
        assert!(page.contains("1 comments"));
    }
    #[test]
    fn commenting_appends_a_dense_id_and_refuses_an_empty_body() {
        let mut state = PressService.initialize(seed("magazine"), &ctx()).unwrap();
        let made = post(
            &mut state,
            "http://press.example/api/articles/atlas-determinism/comments",
            json!({"text": "The hash-order detail is the best part."}),
        );
        assert_eq!(made.status, 200);
        let comments = state["articles"]["atlas-determinism"]["comments"].clone();
        assert_eq!(comments[1]["id"], json!("c2"));
        assert_eq!(comments[1]["author"], json!("alice"));
        assert_eq!(comments[1]["tick"], json!(12));
        assert_eq!(
            post(
                &mut state,
                "http://press.example/api/articles/atlas-determinism/comments",
                json!({"text": "  "})
            )
            .status,
            400
        );
        assert_eq!(
            post(
                &mut state,
                "http://press.example/api/articles/ghost/comments",
                json!({"text": "hello"})
            )
            .status,
            400
        );
    }
    #[test]
    fn a_comment_can_be_liked_by_either_route_and_a_stray_id_is_refused() {
        let mut state = PressService.initialize(seed("magazine"), &ctx()).unwrap();
        post(
            &mut state,
            "http://press.example/api/articles/atlas-determinism/comments/c1/like",
            json!({}),
        );
        assert_eq!(
            state["articles"]["atlas-determinism"]["comments"][0]["likes"],
            json!(4)
        );
        post(
            &mut state,
            "http://press.example/api/comments/c1/like",
            json!({"article": "atlas-determinism"}),
        );
        assert_eq!(
            state["articles"]["atlas-determinism"]["comments"][0]["likes"],
            json!(5)
        );
        assert_eq!(
            post(
                &mut state,
                "http://press.example/api/comments/c1/like",
                json!({})
            )
            .status,
            400,
            "the short form has to be told which story"
        );
        assert_eq!(
            post(
                &mut state,
                "http://press.example/api/articles/atlas-determinism/comments/c9/like",
                json!({})
            )
            .status,
            400
        );
    }
    #[test]
    fn saving_is_a_per_actor_toggle_that_the_reading_list_reflects() {
        let mut state = PressService.initialize(seed("magazine"), &ctx()).unwrap();
        assert!(text(&get(&mut state, "http://press.example/saved")).contains("list-empty"));
        let on = post(
            &mut state,
            "http://press.example/api/articles/atlas-determinism/save",
            json!({}),
        );
        assert_eq!(text(&on), r#"{"saved":true}"#);
        assert_eq!(state["saved"]["alice"], json!(["atlas-determinism"]));
        let list = text(&get(&mut state, "http://press.example/saved"));
        assert!(list.contains("Northstar's Atlas bets everything on determinism"));
        assert!(
            !list.contains("27-inch"),
            "bob's reading list is not alice's"
        );
        post(
            &mut state,
            "http://press.example/api/articles/atlas-determinism/save",
            json!({}),
        );
        assert_eq!(state["saved"]["alice"], json!([]));
        assert_eq!(
            post(
                &mut state,
                "http://press.example/api/articles/ghost/save",
                json!({})
            )
            .status,
            400
        );
    }
    #[test]
    fn following_a_topic_and_the_publication_are_separate_toggles() {
        let mut state = PressService.initialize(seed("magazine"), &ctx()).unwrap();
        post(
            &mut state,
            "http://press.example/api/tags/determinism/follow",
            json!({}),
        );
        post(&mut state, "http://press.example/api/follow", json!({}));
        assert_eq!(
            state["follows"]["alice"],
            json!(["determinism", "publication"])
        );
        let tag = text(&get(&mut state, "http://press.example/tag/determinism"));
        assert!(
            tag.contains("Following"),
            "the topic control shows its engaged state"
        );
        assert!(tag.contains("Northstar's Atlas"));
        post(&mut state, "http://press.example/api/follow", json!({}));
        assert_eq!(state["follows"]["alice"], json!(["determinism"]));
    }
    #[test]
    fn the_newsletter_stores_one_copy_of_a_real_address() {
        let mut state = PressService.initialize(seed("wire"), &ctx()).unwrap();
        let ok = post(
            &mut state,
            "http://press.example/api/subscribe",
            json!({"email": "carol.nakamura@gmail.com"}),
        );
        assert_eq!(ok.status, 200);
        post(
            &mut state,
            "http://press.example/api/subscribe",
            json!({"email": "carol.nakamura@gmail.com"}),
        );
        assert_eq!(
            state["subscribers"],
            json!(["alice.chen@gmail.com", "carol.nakamura@gmail.com"]),
            "a second subscribe is not a second subscriber"
        );
        for bad in ["", "carol", "@gmail.com", "carol@"] {
            assert_eq!(
                post(
                    &mut state,
                    "http://press.example/api/subscribe",
                    json!({"email": bad})
                )
                .status,
                400,
                "{bad}"
            );
        }
    }
    #[test]
    fn a_browser_control_lands_back_on_the_page_it_was_pressed_from() {
        let mut state = PressService.initialize(seed("magazine"), &ctx()).unwrap();
        let back = post(
            &mut state,
            "http://press.example/articles/atlas-determinism/save",
            json!({"return": "/2026/atlas-determinism"}),
        );
        assert_eq!(back.status, 200);
        assert!(
            text(&back).contains("article-title"),
            "it re-renders the article"
        );
        assert!(text(&back).contains("Saved"));
    }
    #[test]
    fn reading_pages_is_pure_and_unknown_routes_are_refused() {
        let mut state = PressService.initialize(seed("magazine"), &ctx()).unwrap();
        for url in [
            "http://press.example/",
            "http://press.example/tech",
            "http://press.example/reviews",
            "http://press.example/archive",
            "http://press.example/saved",
            "http://press.example/tag/northstar",
            "http://press.example/2026/atlas-determinism",
        ] {
            let page = get(&mut state, url);
            assert_eq!(page.status, 200, "{url}");
            assert_eq!(page, get(&mut state, url), "{url} must be pure");
        }
        assert_eq!(
            get(&mut state, "http://press.example/2026/ghost").status,
            404
        );
        assert_eq!(get(&mut state, "http://press.example/opinion").status, 404);
        let mut odd = HttpRequest::get("http://press.example/");
        odd.method = "PUT".into();
        assert_eq!(
            PressService
                .handle(&mut state, &ctx(), &odd)
                .unwrap()
                .status,
            405
        );
    }
    #[test]
    fn everything_a_reader_does_survives_a_snapshot_round_trip() {
        let mut state = PressService.initialize(seed("magazine"), &ctx()).unwrap();
        post(
            &mut state,
            "http://press.example/api/articles/atlas-determinism/comments",
            json!({"text": "Bookmarking this."}),
        );
        post(
            &mut state,
            "http://press.example/api/articles/atlas-determinism/save",
            json!({}),
        );
        post(
            &mut state,
            "http://press.example/api/tags/northstar/follow",
            json!({}),
        );
        post(
            &mut state,
            "http://press.example/api/subscribe",
            json!({"email": "bmartinez@outlook.com"}),
        );
        let bytes = serde_json::to_vec(&state).unwrap();
        let mut restored: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, state);
        assert_eq!(
            text(&get(
                &mut restored,
                "http://press.example/2026/atlas-determinism"
            )),
            text(&get(
                &mut state,
                "http://press.example/2026/atlas-determinism"
            ))
        );
    }
}
