//! A typed HTML template layer for services that answer with `text/html`.
//!
//! Pages are built from [`Html`] nodes that read like the markup they emit:
//!
//! ```
//! use cw_service_common::html::{el, link, text, Document};
//! let page = Document::new("Atlas")
//!     .lang("en")
//!     .stylesheet("h1 { color: #202124 }")
//!     .body([
//!         el("h1").id("title").text("Atlas"),
//!         el("p").class("lead").child(text("Deterministic simulation.")),
//!         link("docs", "/docs/", "Read the docs"),
//!     ]);
//! let html = page.render();
//! assert!(html.contains("<a id=\"docs\" href=\"/docs/\">Read the docs</a>"));
//! ```
//!
//! Text is escaped for element content, attribute values for attributes, and the
//! raw-text elements (`style`, `script`) are emitted verbatim with `</` defused so
//! a stylesheet can never close its own tag. Void elements (`input`, `img`, `br`,
//! `meta`, `link`, ...) take no closing tag and refuse children.
//!
//! A site's stylesheet is a real `.css` file next to its source, pulled in with
//! `include_str!` and handed to [`Document::stylesheet`], which emits it as one
//! `<style>` in `<head>`: one request per page, nothing to fetch, and the strict
//! validator sees the whole page in one string. Colours that vary per instance (a
//! seeded theme) go on the root element as custom properties through
//! [`Document::root_style`] so the sheet itself stays static.
//!
//! Element ids are the agent API: the browser's `click`, `fill` and `submit` name
//! elements by `id`, so every link, input and button a person could use carries one,
//! and [`validate_strict`] (feature `validate`) refuses a page that repeats an id or
//! uses HTML or CSS the `cw-web` engine does not render.
use cw_protocol::HttpResponse;
use std::collections::BTreeMap;
use std::fmt;

/// The media type every HTML response carries.
pub const HTML_MEDIA_TYPE: &str = "text/html; charset=utf-8";

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];
const RAW_TEXT: &[&str] = &["script", "style"];

/// One node of a page: an element, escaped text, trusted markup, or a fragment of
/// siblings that renders with no wrapper of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Html {
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
        children: Vec<Html>,
    },
    Text(String),
    Raw(String),
    Fragment(Vec<Html>),
}

/// An element with no attributes and no children yet.
pub fn el(tag: &str) -> Html {
    debug_assert!(
        !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "invalid tag name {tag:?}"
    );
    Html::Element {
        tag: tag.to_ascii_lowercase(),
        attrs: Vec::new(),
        children: Vec::new(),
    }
}
/// Escaped text.
pub fn text(s: impl Into<String>) -> Html {
    Html::Text(s.into())
}
/// Markup emitted verbatim. Only for strings the service itself wrote, never for
/// anything that came from a request or a seed.
pub fn raw(s: impl Into<String>) -> Html {
    Html::Raw(s.into())
}
/// Siblings with no element around them.
pub fn fragment(children: impl IntoIterator<Item = Html>) -> Html {
    Html::Fragment(children.into_iter().collect())
}
/// Nothing at all; the `else` branch of a conditional child.
pub fn empty() -> Html {
    Html::Fragment(Vec::new())
}
pub fn div(class: &str) -> Html {
    el("div").class(class)
}
pub fn span(class: &str) -> Html {
    el("span").class(class)
}
/// `<a id href>text</a>`: the link the agent clicks by `id`.
pub fn link(id: &str, href: impl Into<String>, label: impl Into<String>) -> Html {
    el("a").id(id).attr("href", href).text(label)
}
/// `<a href>` with children of its own.
pub fn a(href: impl Into<String>) -> Html {
    el("a").attr("href", href)
}
/// `<form action method>`; `method` is `get` or `post`.
pub fn form(id: &str, action: impl Into<String>, method: &str) -> Html {
    let method = method.to_ascii_lowercase();
    debug_assert!(method == "get" || method == "post", "form method {method}");
    el("form")
        .id(id)
        .attr("action", action)
        .attr("method", method)
}
/// `<input type=text id name value>`; `value` is the current text.
pub fn text_input(id: &str, name: &str, value: &str) -> Html {
    el("input")
        .id(id)
        .attr("type", "text")
        .attr("name", name)
        .attr("value", value)
}
/// `<input type=hidden name value>`: a field the form carries without showing.
pub fn hidden(name: &str, value: &str) -> Html {
    el("input")
        .attr("type", "hidden")
        .attr("name", name)
        .attr("value", value)
}
/// `<button id type=submit>label</button>`.
pub fn button(id: &str, label: impl Into<String>) -> Html {
    el("button").id(id).attr("type", "submit").text(label)
}
/// `<label for>text</label>`: the accessible name of a control.
pub fn label(for_id: &str, label: impl Into<String>) -> Html {
    el("label").attr("for", for_id).text(label)
}
/// A path with a query string, each value form-encoded: the resolved `href` of a
/// link that re-runs a search. Empty values are kept so `?q=` reads as a field.
pub fn href(path: &str, params: &[(&str, &str)]) -> String {
    if params.is_empty() {
        return path.to_owned();
    }
    let mut out = String::from(path);
    out.push(if path.contains('?') { '&' } else { '?' });
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params.iter().copied())
        .finish();
    out.push_str(&query);
    out
}

impl Html {
    /// Sets the `id` attribute.
    pub fn id(self, id: impl Into<String>) -> Html {
        self.attr("id", id)
    }
    /// Appends to the `class` attribute; an empty class adds nothing.
    pub fn class(mut self, class: &str) -> Html {
        if class.is_empty() {
            return self;
        }
        if let Html::Element { attrs, .. } = &mut self {
            match attrs.iter_mut().find(|(k, _)| k == "class") {
                Some((_, v)) => {
                    v.push(' ');
                    v.push_str(class);
                }
                None => attrs.push(("class".into(), class.into())),
            }
        }
        self
    }
    /// Sets an attribute, replacing an earlier value of the same name.
    pub fn attr(mut self, name: &str, value: impl Into<String>) -> Html {
        if let Html::Element { attrs, .. } = &mut self {
            let value = value.into();
            match attrs.iter_mut().find(|(k, _)| k == name) {
                Some((_, v)) => *v = value,
                None => attrs.push((name.into(), value)),
            }
        }
        self
    }
    /// A boolean attribute (`hidden`, `disabled`, `checked`).
    pub fn flag(self, name: &str) -> Html {
        self.attr(name, "")
    }
    /// An inline `style` attribute; for values that vary per render (a width from a
    /// seed), never for what belongs in the stylesheet.
    pub fn style(self, css: &str) -> Html {
        self.attr("style", css)
    }
    /// Appends a child.
    pub fn child(mut self, child: Html) -> Html {
        match &mut self {
            Html::Element { tag, children, .. } => {
                debug_assert!(!VOID.contains(&tag.as_str()), "<{tag}> takes no children");
                children.push(child);
            }
            Html::Fragment(children) => children.push(child),
            Html::Text(_) | Html::Raw(_) => panic!("text takes no children"),
        }
        self
    }
    /// Appends every child.
    pub fn children(mut self, children: impl IntoIterator<Item = Html>) -> Html {
        for c in children {
            self = self.child(c);
        }
        self
    }
    /// Appends escaped text.
    pub fn text(self, s: impl Into<String>) -> Html {
        self.child(text(s))
    }
    /// Appends `build(self)`'s result when `cond` holds; the node is unchanged otherwise.
    pub fn when(self, cond: bool, build: impl FnOnce(Html) -> Html) -> Html {
        if cond {
            build(self)
        } else {
            self
        }
    }
    /// Appends a child if there is one.
    pub fn maybe(self, child: Option<Html>) -> Html {
        match child {
            Some(c) => self.child(c),
            None => self,
        }
    }
    /// Appends one child per item.
    pub fn each<T>(
        mut self,
        items: impl IntoIterator<Item = T>,
        mut build: impl FnMut(T) -> Html,
    ) -> Html {
        for item in items {
            self = self.child(build(item));
        }
        self
    }
    /// The element's tag, if it is one.
    pub fn tag(&self) -> Option<&str> {
        match self {
            Html::Element { tag, .. } => Some(tag),
            _ => None,
        }
    }
    /// Serialises the node.
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }
    fn write(&self, out: &mut String) {
        match self {
            Html::Text(s) => escape_text(s, out),
            Html::Raw(s) => out.push_str(s),
            Html::Fragment(children) => {
                for c in children {
                    c.write(out);
                }
            }
            Html::Element {
                tag,
                attrs,
                children,
            } => {
                out.push('<');
                out.push_str(tag);
                for (k, v) in attrs {
                    out.push(' ');
                    out.push_str(k);
                    if !v.is_empty() {
                        out.push_str("=\"");
                        escape_attr(v, out);
                        out.push('"');
                    }
                }
                out.push('>');
                if VOID.contains(&tag.as_str()) {
                    return;
                }
                if RAW_TEXT.contains(&tag.as_str()) {
                    for c in children {
                        match c {
                            Html::Text(s) | Html::Raw(s) => out.push_str(&s.replace("</", "<\\/")),
                            other => other.write(out),
                        }
                    }
                } else {
                    for c in children {
                        c.write(out);
                    }
                }
                out.push_str("</");
                out.push_str(tag);
                out.push('>');
            }
        }
    }
}
impl fmt::Display for Html {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}
impl From<&str> for Html {
    fn from(s: &str) -> Html {
        text(s)
    }
}
impl From<String> for Html {
    fn from(s: String) -> Html {
        text(s)
    }
}

/// Escapes text for element content: `&`, `<` and `>`.
pub fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}
/// Escapes a double-quoted attribute value: `&`, `<`, `>` and `"`.
pub fn escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}
/// [`escape_text`] into a new string.
pub fn escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape_text(s, &mut out);
    out
}

/// A whole page: `<!DOCTYPE html>`, `<html lang>`, a `<head>` with the title and one
/// `<style>`, and the body.
#[derive(Clone, Debug, Default)]
pub struct Document {
    title: String,
    lang: Option<String>,
    css: Vec<String>,
    head: Vec<Html>,
    root_style: Option<String>,
    body_class: Option<String>,
    body: Vec<Html>,
}
impl Document {
    pub fn new(title: impl Into<String>) -> Document {
        Document {
            title: title.into(),
            ..Document::default()
        }
    }
    /// `<html lang>`: the document language the browser reports.
    pub fn lang(mut self, lang: &str) -> Document {
        self.lang = Some(lang.to_owned());
        self
    }
    /// The page's stylesheet; several calls concatenate into the one `<style>`.
    pub fn stylesheet(mut self, css: &str) -> Document {
        self.css.push(css.to_owned());
        self
    }
    /// Extra `<head>` children (`<meta>`, a second `<title>`-adjacent element).
    pub fn head(mut self, node: Html) -> Document {
        self.head.push(node);
        self
    }
    /// A `style` attribute on `<html>`: where per-instance custom properties go
    /// (`--accent: #1a73e8; --ink: #202124`), so the stylesheet stays a static file.
    pub fn root_style(mut self, css: &str) -> Document {
        self.root_style = Some(css.to_owned());
        self
    }
    /// A class on `<body>`: the skin selector a stylesheet keys its variants on.
    pub fn body_class(mut self, class: &str) -> Document {
        self.body_class = Some(class.to_owned());
        self
    }
    /// The body's children, appended.
    pub fn body(mut self, children: impl IntoIterator<Item = Html>) -> Document {
        self.body.extend(children);
        self
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    /// The whole document as a string.
    pub fn render(&self) -> String {
        let mut html = el("html");
        if let Some(lang) = &self.lang {
            html = html.attr("lang", lang.as_str());
        }
        if let Some(style) = &self.root_style {
            html = html.attr("style", style.as_str());
        }
        let mut head = el("head")
            .child(el("meta").attr("charset", "utf-8"))
            .child(el("title").text(self.title.as_str()));
        if !self.css.is_empty() {
            head = head.child(el("style").child(raw(self.css.join("\n"))));
        }
        head = head.children(self.head.iter().cloned());
        let mut body = el("body");
        if let Some(class) = &self.body_class {
            body = body.class(class);
        }
        body = body.children(self.body.iter().cloned());
        let mut out = String::from("<!DOCTYPE html>\n");
        html.child(head).child(body).write(&mut out);
        out.push('\n');
        out
    }
    /// A 200 response carrying the document.
    pub fn response(&self) -> HttpResponse {
        HtmlResponse::new(200, self.render()).into()
    }
}

/// A `text/html; charset=utf-8` response. Page responses carry only their content
/// type today, so this does too; `header` adds what a page needs (a `refresh`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HtmlResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}
impl HtmlResponse {
    pub fn new(status: u16, body: impl Into<String>) -> HtmlResponse {
        HtmlResponse {
            status,
            headers: BTreeMap::from([("content-type".into(), HTML_MEDIA_TYPE.into())]),
            body: body.into(),
        }
    }
    pub fn ok(document: &Document) -> HtmlResponse {
        HtmlResponse::new(200, document.render())
    }
    pub fn header(mut self, name: &str, value: impl Into<String>) -> HtmlResponse {
        self.headers.insert(name.to_ascii_lowercase(), value.into());
        self
    }
}
impl From<HtmlResponse> for HttpResponse {
    fn from(r: HtmlResponse) -> HttpResponse {
        HttpResponse {
            status: r.status,
            headers: r.headers,
            body: r.body.into_bytes(),
        }
    }
}
/// `Ok(document.response())`: what a route handler returns.
pub fn page(document: &Document) -> cw_protocol::Result<HttpResponse> {
    Ok(document.response())
}

/// Runs a page through the engine's strict pipeline: the HTML is parsed, every
/// `<style>` is parsed with `Strictness::Strict`, and the cascade runs strictly, so
/// an unknown property, value, selector or at-rule fails the test that calls this.
/// Ids must be unique (the agent API addresses elements by id) and stylesheets
/// must be inline, since a linked sheet cannot be checked from the page alone.
#[cfg(any(test, feature = "validate"))]
pub fn validate_strict(html: &str) -> Result<(), cw_web::Unsupported> {
    use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
    use cw_web::dom::Document as Dom;
    use cw_web::{Strictness, Unsupported, UnsupportedKind};
    let doc = cw_web::html::parse(html);
    let mut seen = std::collections::BTreeSet::new();
    let mut sheets = Vec::new();
    for node in doc.descendants(Dom::ROOT) {
        if !doc.is_element(node) {
            continue;
        }
        if let Some(id) = doc.attr(node, "id") {
            if id.is_empty() {
                return Err(Unsupported {
                    kind: UnsupportedKind::Attribute,
                    name: "id".into(),
                    detail: format!("<{}> has an empty id", doc.tag(node).unwrap_or("?")),
                });
            }
            if !seen.insert(id.to_owned()) {
                return Err(Unsupported {
                    kind: UnsupportedKind::Attribute,
                    name: "id".into(),
                    detail: format!("id {id:?} appears more than once"),
                });
            }
        }
        if doc.is(node, "style") {
            sheets.push(parse_stylesheet(
                &doc.text_content(node),
                Origin::Author,
                Strictness::Strict,
            )?);
        }
        if doc.is(node, "link")
            && doc.attr(node, "rel").is_some_and(|rel| {
                rel.split_ascii_whitespace()
                    .any(|r| r.eq_ignore_ascii_case("stylesheet"))
            })
        {
            return Err(Unsupported {
                kind: UnsupportedKind::Feature,
                name: "link".into(),
                detail: "a linked stylesheet cannot be validated from the page; inline it with Document::stylesheet".into(),
            });
        }
    }
    cw_web::style::cascade(
        &doc,
        &sheets,
        &Media::default(),
        &MatchContext::new(),
        Strictness::Strict,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_attributes_and_raw_text_are_escaped_each_their_own_way() {
        let node = el("p")
            .id("x")
            .attr("title", "a \"quoted\" <b> & c")
            .text("1 < 2 & \"q\"");
        assert_eq!(
            node.render(),
            "<p id=\"x\" title=\"a &quot;quoted&quot; &lt;b&gt; &amp; c\">1 &lt; 2 &amp; \"q\"</p>"
        );
        assert_eq!(
            el("style")
                .text("a::after { content: \"</style>\" }")
                .render(),
            "<style>a::after { content: \"<\\/style>\" }</style>"
        );
        assert_eq!(el("br").render(), "<br>");
        assert_eq!(
            el("input").attr("name", "q").flag("disabled").render(),
            "<input name=\"q\" disabled>"
        );
        assert_eq!(
            div("a").class("b").class("").render(),
            "<div class=\"a b\"></div>"
        );
        assert_eq!(
            fragment([text("a"), el("i").text("b")]).render(),
            "a<i>b</i>"
        );
    }
    #[test]
    fn conditional_and_iterated_children() {
        let items = ["x", "y"];
        let list = el("ul")
            .each(items, |i| el("li").text(i))
            .when(false, |n| n.child(el("li").text("never")))
            .maybe(None)
            .maybe(Some(el("li").text("z")));
        assert_eq!(list.render(), "<ul><li>x</li><li>y</li><li>z</li></ul>");
    }
    #[test]
    fn hrefs_are_form_encoded() {
        assert_eq!(
            href("/search", &[("q", "a b&c"), ("v", "news")]),
            "/search?q=a+b%26c&v=news"
        );
        assert_eq!(href("/search?v=news", &[("q", "")]), "/search?v=news&q=");
        assert_eq!(href("/about", &[]), "/about");
    }
    #[test]
    fn documents_carry_title_language_one_stylesheet_and_the_html_media_type() {
        let doc = Document::new("T & U")
            .lang("en")
            .stylesheet("a{color:red}")
            .stylesheet("b{color:blue}")
            .root_style("--accent: #123456")
            .body_class("skin-google")
            .body([form("search", "/search", "get")
                .child(text_input("q", "q", "atlas"))
                .child(button("go", "Go"))]);
        let html = doc.render();
        assert!(html.starts_with("<!DOCTYPE html>\n<html lang=\"en\" style=\"--accent: #123456\"><head><meta charset=\"utf-8\"><title>T &amp; U</title><style>a{color:red}\nb{color:blue}</style></head><body class=\"skin-google\">"));
        assert!(html.contains("<form id=\"search\" action=\"/search\" method=\"get\"><input id=\"q\" type=\"text\" name=\"q\" value=\"atlas\"><button id=\"go\" type=\"submit\">Go</button></form>"));
        let response: HttpResponse = HtmlResponse::ok(&doc).header("Refresh", "5; url=/").into();
        assert_eq!(response.status, 200);
        assert_eq!(response.header("content-type"), Some(HTML_MEDIA_TYPE));
        assert_eq!(response.header("refresh"), Some("5; url=/"));
        assert_eq!(response.body, html.into_bytes());
    }
    #[test]
    fn the_strict_validator_refuses_what_the_engine_does_not_render() {
        let good = Document::new("ok")
            .stylesheet(".a { display: flex; gap: 8px; border-radius: 24px }")
            .body([div("a").id("one").text("x")]);
        validate_strict(&good.render()).unwrap();
        let bad_property = Document::new("bad")
            .stylesheet(".a { scroll-snap-type: x mandatory }")
            .body([div("a")]);
        assert!(validate_strict(&bad_property.render()).is_err());
        let bad_at_rule = Document::new("bad").stylesheet("@container (min-width: 1px) { a {} }");
        assert!(validate_strict(&bad_at_rule.render()).is_err());
        let duplicate = Document::new("bad").body([div("").id("x"), div("").id("x")]);
        assert!(validate_strict(&duplicate.render())
            .unwrap_err()
            .detail
            .contains("more than once"));
        let linked =
            Document::new("bad").head(el("link").attr("rel", "stylesheet").attr("href", "/s.css"));
        assert!(validate_strict(&linked.render()).is_err());
    }
}
