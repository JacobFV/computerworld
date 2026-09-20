//! Publications: reuters.com (`wire`), theverge.com and arstechnica.com (`magazine`), and the
//! three personal/company blogs (`blog`). `layout` picks the front page, not the data shape.
//!
//! Reading is pure; everything a reader does — commenting, liking a comment, saving an article,
//! following a topic or the publication itself, subscribing to the newsletter — is a POST that
//! really mutates state and survives a snapshot round trip.
use cw_protocol::{HttpRequest, HttpResponse, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Value};
mod view;
pub struct PressService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(PressService)
}
/// Documented seed keys, checked by container type only — the crate that fills them owns the rest.
const OBJECTS: &[&str] = &["theme", "articles", "saved", "follows"];
const ARRAYS: &[&str] = &["sections", "subscribers"];
/// `layout` is the documented discriminant; an unlisted value is a seed typo, not a fallback.
const LAYOUTS: &[&str] = &["wire", "magazine", "blog"];
/// `skin` picks the stylesheet: the publication a site stands in for. Optional; a seed without
/// one gets the skin its brand names, and the plain sheet of its layout otherwise.
const SKINS: &[&str] = &[
    "plain", "blog", "nyt", "bbc", "cnn", "reuters", "verge", "ars", "gnews", "medium", "substack",
];
const BRAND: &str = "Press";
const TAGLINE: &str = "Today's reporting.";
/// The publication itself is a follow target alongside the topics, under a token no tag uses.
const PUBLICATION: &str = "publication";
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
/// Every GET route. Reading never mutates, so this takes the state by reference.
fn render(state: &Value, ctx: &ServiceContext, request: &HttpRequest) -> Result<HttpResponse> {
    let path = web::path(request);
    let chrome = view::Chrome::read(state, &ctx.actor, &path)?;
    let actor = ctx.actor.as_str();
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        [""] if state.get("articles").is_none() => chrome.landing(),
        [""] => chrome.front(),
        ["archive"] => chrome.list("Archive", &recent(state, |_| true), None),
        ["saved"] => chrome.list(
            "Reading list",
            &recent(state, |a| has(state, "saved", actor, &web::text(a, "id"))),
            None,
        ),
        ["tag", tag] => chrome.list(
            &format!("#{tag}"),
            &recent(state, |a| web::strings(a, "tags").iter().any(|t| t == tag)),
            Some(tag),
        ),
        ["posts", slug] => chrome.article(slug),
        [section] if sections(state).iter().any(|s| s == section) => chrome.list(
            &section_title(state, section),
            &recent(state, |a| web::text(a, "section") == *section),
            None,
        ),
        [slug] | [_, slug] if article(state, slug).is_some() => chrome.article(slug),
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
        web::variant(&state, "skin", SKINS)?;
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
    use cw_service_common::html::validate_strict;
    use cw_web::dom::Document as Dom;
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
    /// A parsed HTML page, validated strictly on the way in: every page any test fetches
    /// has to be markup and CSS the engine renders, with unique ids.
    struct Page(Dom);
    fn page(state: &mut Value, url: &str) -> Page {
        let response = get(state, url);
        assert_eq!(response.status, 200, "{url}");
        assert_eq!(response.header("content-type"), Some(cw_service_common::html::HTML_MEDIA_TYPE));
        parsed(&response)
    }
    fn parsed(response: &HttpResponse) -> Page {
        let html = text(response);
        validate_strict(&html).unwrap_or_else(|e| panic!("strict validation: {e:?}"));
        Page(cw_web::html::parse(&html))
    }
    impl Page {
        fn has(&self, id: &str) -> bool {
            !self.0.by_id(id).is_empty()
        }
        fn node(&self, id: &str) -> cw_web::dom::NodeId {
            *self.0.by_id(id).first().unwrap_or_else(|| panic!("no element #{id}"))
        }
        fn text(&self, id: &str) -> String {
            self.0.text_content(self.node(id))
        }
        fn attr(&self, id: &str, name: &str) -> String {
            self.0.attr(self.node(id), name).unwrap_or_else(|| panic!("#{id} has no {name}")).to_owned()
        }
        fn tag(&self, id: &str) -> String {
            self.0.tag(self.node(id)).unwrap().to_owned()
        }
        /// The `name=value` pairs of the inputs inside a form, hidden ones included.
        fn fields(&self, form: &str) -> Vec<(String, String)> {
            let form = self.node(form);
            self.0
                .descendants(form)
                .filter(|n| self.0.is(*n, "input"))
                .map(|n| (self.0.attr(n, "name").unwrap_or("").to_owned(), self.0.attr(n, "value").unwrap_or("").to_owned()))
                .collect()
        }
        fn body(&self) -> String {
            self.0.text_content(self.0.body().unwrap())
        }
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
            let front = page(&mut state, "http://press.example/");
            assert!(front.has(marker), "{layout} front page");
            assert_eq!(front.text("card-atlas-determinism-title"), "Northstar's Atlas bets everything on determinism");
            assert_eq!(front.tag("card-atlas-determinism"), "a");
            assert_eq!(
                front.attr("card-atlas-determinism", "href"),
                if layout == "blog" { "/posts/atlas-determinism" } else { "/2026/atlas-determinism" }
            );
            assert_eq!(front.text("card-atlas-determinism-section"), "Tech");
            assert_eq!(front.text("masthead-brand"), "The Testpaper");
            assert_eq!(front.attr("masthead-home", "href"), "/");
            assert_eq!(front.attr("masthead-tech", "href"), "/tech");
            assert_eq!(front.attr("masthead-archive", "href"), "/archive");
            assert_eq!(front.attr("masthead-saved", "href"), "/saved");
            // Follow is a one-button POST form that comes back to the front page.
            assert_eq!(front.tag("masthead-follow"), "button");
            assert_eq!(front.text("masthead-follow-text"), "Follow");
            assert_eq!(front.attr("masthead-follow-form", "action"), "/follow");
            assert_eq!(front.attr("masthead-follow-form", "method"), "post");
            assert_eq!(front.fields("masthead-follow-form"), [("return".to_owned(), "/".to_owned())]);
            // The newsletter form posts `email` to /subscribe.
            assert_eq!(front.text("newsletter-heading"), "Get the newsletter");
            assert_eq!(front.attr("subscribe", "action"), "/subscribe");
            assert_eq!(front.attr("subscribe", "method"), "post");
            assert_eq!(front.attr("subscribe-email", "name"), "email");
            assert_eq!(front.attr("subscribe-email", "aria-label"), "Email address");
            assert_eq!(front.tag("subscribe-submit"), "button");
            if layout == "wire" {
                assert_eq!(front.attr("mostread-atlas-determinism", "href"), "/2026/atlas-determinism");
                assert_eq!(front.text("front-heading"), "Latest");
            }
        }
    }
    #[test]
    fn every_skin_serves_every_page_as_strict_html() {
        for skin in SKINS {
            for layout in LAYOUTS {
                let mut initial = seed(layout);
                initial["skin"] = json!(skin);
                let mut state = PressService.initialize(initial, &ctx()).unwrap();
                for url in [
                    "http://press.example/",
                    "http://press.example/tech",
                    "http://press.example/archive",
                    "http://press.example/saved",
                    "http://press.example/tag/northstar",
                    "http://press.example/tag/nothing-tagged",
                    "http://press.example/2026/atlas-determinism",
                    "http://press.example/posts/monitor-roundup",
                ] {
                    let shown = page(&mut state, url);
                    assert!(shown.has("masthead") && shown.has("subscribe"), "{skin} {layout} {url}");
                    assert_eq!(
                        shown.0.attr(shown.0.body().unwrap(), "class").unwrap().split(' ').next(),
                        Some(format!("skin-{skin}").as_str())
                    );
                }
            }
        }
        let mut bad = seed("wire");
        bad["skin"] = json!("tabloid");
        assert!(PressService.initialize(bad, &ctx()).is_err());
        // Without a `skin` key the brand picks it, and a blog falls back to the blog sheet.
        let mut named = seed("magazine");
        named["brand"] = json!("The New York Times");
        let mut state = PressService.initialize(named, &ctx()).unwrap();
        let front = page(&mut state, "http://press.example/");
        assert!(front.0.has_class(front.0.body().unwrap(), "skin-nyt"));
        let mut state = PressService.initialize(seed("blog"), &ctx()).unwrap();
        let front = page(&mut state, "http://press.example/");
        assert!(front.0.has_class(front.0.body().unwrap(), "skin-blog"));
        // A seed with no articles is the brand splash — on every skin, since the splash wears
        // the same sheets and is the one page that has neither masthead nor newsletter.
        for skin in SKINS {
            for layout in LAYOUTS {
                let mut state = PressService
                    .initialize(json!({"layout": layout, "skin": skin, "brand": "Soon"}), &ctx())
                    .unwrap();
                let splash = page(&mut state, "http://press.example/");
                assert_eq!(splash.text("brand"), "Soon", "{skin} {layout}");
                assert_eq!(splash.text("tagline"), TAGLINE);
                assert!(!splash.has("masthead"), "the splash promises no control it cannot honour");
                assert_eq!(
                    splash.0.attr(splash.0.body().unwrap(), "class").unwrap().split(' ').next(),
                    Some(format!("skin-{skin}").as_str())
                );
            }
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
        let story = page(&mut dated, "http://press.example/2026/atlas-determinism");
        assert_eq!(story.text("article-credit"), "Tom Weber · Mar 5, 2026 · 7 min read");
        assert_eq!(story.text("article-title"), "Northstar's Atlas bets everything on determinism");
        assert_eq!(story.attr("article-section", "href"), "/tech");
        assert_eq!(story.text("article-date"), "Mar 5, 2026");
        assert_eq!(story.text("article-avatar"), "TW");
        assert_eq!(story.text("article-p0"), "Most simulation software will tell you it is reproducible.");
        // The Read more box is real, and URLs in the prose are links in place.
        assert_eq!(story.attr("article-ref-0", "href"), "http://github.com/northstar/atlas");
        assert_eq!(story.text("article-ref-0"), "Atlas on GitHub");
        assert_eq!(story.tag("article-p1-link-4"), "a");
        assert_eq!(story.attr("article-p1-link-4", "href"), "http://github.com/northstar/atlas");
        assert_eq!(
            story.text("article-p1"),
            "The repo is at http://github.com/northstar/atlas and it is worth reading."
        );
        assert_eq!(story.attr("article-tag-determinism", "href"), "/tag/determinism");
        assert_eq!(story.text("article-tag-determinism-text"), "#determinism");
        assert_eq!(story.text("comments-heading"), "1 comment");
        // Save, comment and like are POST forms with the fields the Page actions carried.
        assert_eq!(story.attr("article-save-form", "action"), "/articles/atlas-determinism/save");
        assert_eq!(story.fields("article-save-form"), [("return".to_owned(), "/2026/atlas-determinism".to_owned())]);
        assert_eq!(story.text("article-save"), "Save");
        assert_eq!(story.attr("comment", "action"), "/articles/atlas-determinism/comments");
        assert_eq!(story.attr("comment", "method"), "post");
        assert_eq!(story.fields("comment"), [("text".to_owned(), String::new())]);
        assert_eq!(story.attr("comment-text", "aria-label"), "Join the discussion");
        assert_eq!(story.tag("comment-submit"), "button");
        assert_eq!(story.text("comment-c1-author"), "praman");
        assert_eq!(story.text("comment-c1-text"), "Finally.");
        assert_eq!(story.text("comment-c1-like"), "▲ 3");
        assert_eq!(
            story.attr("comment-c1-like-form", "action"),
            "/articles/atlas-determinism/comments/c1/like"
        );
        assert_eq!(story.fields("comment-c1-like-form"), [("return".to_owned(), "/2026/atlas-determinism".to_owned())]);
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
        assert!(page(&mut state, "http://press.example/saved").has("list-empty"));
        let on = post(
            &mut state,
            "http://press.example/api/articles/atlas-determinism/save",
            json!({}),
        );
        assert_eq!(text(&on), r#"{"saved":true}"#);
        assert_eq!(state["saved"]["alice"], json!(["atlas-determinism"]));
        let list = page(&mut state, "http://press.example/saved");
        assert_eq!(list.text("list-title"), "Reading list");
        assert!(list.has("card-atlas-determinism") && !list.has("list-empty"));
        assert!(
            !list.has("card-monitor-roundup"),
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
        let tag = page(&mut state, "http://press.example/tag/determinism");
        assert_eq!(tag.text("list-follow"), "Following", "the topic control shows its engaged state");
        assert_eq!(tag.attr("list-follow-form", "action"), "/tags/determinism/follow");
        assert_eq!(tag.fields("list-follow-form"), [("return".to_owned(), "/tag/determinism".to_owned())]);
        assert_eq!(tag.text("masthead-follow"), "Following");
        assert!(tag.has("card-atlas-determinism") && !tag.has("card-monitor-roundup"));
        assert!(tag.body().contains("Northstar's Atlas"));
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
        let back = parsed(&back);
        assert!(back.has("article-title"), "it re-renders the article");
        assert_eq!(back.text("article-save"), "Saved");
        // The browser posts forms urlencoded; the same route takes them.
        let mut request = HttpRequest::get("http://press.example/articles/atlas-determinism/comments");
        request.method = "POST".into();
        request.headers.insert("content-type".into(), "application/x-www-form-urlencoded".into());
        request.body = b"text=Read+it+twice.".to_vec();
        let shown = parsed(&PressService.handle(&mut state, &ctx(), &request).unwrap());
        assert_eq!(shown.text("comment-c2-text"), "Read it twice.");
        assert_eq!(shown.text("comments-heading"), "2 comments");
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
