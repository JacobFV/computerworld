//! Dead-control auditing, for a service's own tests.
//!
//! The rule the sites are held to is that anything which looks like a control is one,
//! and anything that cannot act does not look like one. A decorative `<button>`, an
//! anchor with no `href`, a link to a route that 404s, an emoji picker that swallows
//! the click: each is a lie to the person looking at the page and to the agent reading
//! the semantic tree, which lists exactly the elements
//! [`cw_web::paint::semantics::is_interactive`] accepts.
//!
//! [`page`] audits one rendered page on its own: it finds every control, checks that
//! each has somewhere to go, and — using the same cascade the engine paints with —
//! catches the subtler lie of an inert element drawn with `cursor: pointer`.
//! [`Sweep`] walks a whole service: it starts from a few seed paths, follows every
//! same-origin link, and asserts the service answers every link target, form action
//! and `formaction` with something other than a 404 or a 405.
//!
//! Both return a list of complaints; a test asserts the list is empty and prints it
//! when it is not.

use cw_web::dom::{Document as Dom, NodeId};
use cw_web::paint::semantics::is_interactive;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// One thing on a page an agent can act on, with the request acting on it would make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    /// The `id` the agent API addresses it by, empty when it has none.
    pub id: String,
    pub tag: String,
    /// `GET` or `POST` for something that navigates, empty for a field or a fragment.
    pub method: String,
    /// The `href`, `action` or `formaction` it would request, as written on the page.
    pub target: String,
    /// The accessible name: `aria-label`, else the text inside, else `title`.
    pub label: String,
}

/// What the tag is called in a complaint: `<a id="x">` or `<button>`.
fn named(doc: &Dom, node: NodeId) -> String {
    let tag = doc.tag(node).unwrap_or("?");
    match doc.attr(node, "id") {
        Some(id) if !id.is_empty() => format!("<{tag} id={id:?}>"),
        _ => format!("<{tag}>"),
    }
}

fn label_of(doc: &Dom, node: NodeId) -> String {
    let text = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    for attr in ["aria-label", "title", "alt", "value"] {
        if let Some(v) = doc.attr(node, attr).filter(|v| !v.trim().is_empty()) {
            return text(v);
        }
    }
    text(&doc.text_content(node))
}

fn ancestor_form(doc: &Dom, node: NodeId) -> Option<NodeId> {
    doc.ancestors(node).find(|a| doc.is(*a, "form"))
}

fn input_kind(doc: &Dom, node: NodeId) -> String {
    doc.attr(node, "type").unwrap_or("text").to_ascii_lowercase()
}

/// The method a form or a submit button uses, upper-cased, `GET` when unwritten.
fn method_of(doc: &Dom, node: NodeId) -> String {
    let raw = doc
        .attr(node, "formmethod")
        .or_else(|| doc.attr(node, "method"))
        .unwrap_or("get");
    raw.to_ascii_uppercase()
}

/// Every control on the page, in document order.
pub fn controls(doc: &Dom) -> Vec<Control> {
    let mut out = Vec::new();
    for node in doc.descendants(Dom::ROOT) {
        if !doc.is_element(node) {
            continue;
        }
        let tag = doc.tag(node).unwrap_or("").to_owned();
        let interactive = is_interactive(doc, node);
        if !interactive {
            continue;
        }
        let (method, target) = match tag.as_str() {
            "a" | "area" => ("GET".to_owned(), doc.attr(node, "href").unwrap_or_default().to_owned()),
            "form" => (method_of(doc, node), doc.attr(node, "action").unwrap_or_default().to_owned()),
            "button" | "input" => {
                let submits = tag == "button"
                    && !matches!(doc.attr(node, "type"), Some("button") | Some("reset"))
                    || tag == "input" && matches!(input_kind(doc, node).as_str(), "submit" | "image");
                match (submits, doc.attr(node, "formaction")) {
                    // `formmethod` wins, else the form's own method: a button that
                    // posts elsewhere still posts.
                    (true, Some(action)) => {
                        let method = match doc.has_attr(node, "formmethod") {
                            true => method_of(doc, node),
                            false => ancestor_form(doc, node)
                                .map(|form| method_of(doc, form))
                                .unwrap_or_else(|| "GET".to_owned()),
                        };
                        (method, action.to_owned())
                    }
                    (true, None) => match ancestor_form(doc, node) {
                        Some(form) => (
                            method_of(doc, form),
                            doc.attr(form, "action").unwrap_or_default().to_owned(),
                        ),
                        None => (String::new(), String::new()),
                    },
                    (false, _) => (String::new(), String::new()),
                }
            }
            _ => (String::new(), String::new()),
        };
        out.push(Control {
            id: doc.attr(node, "id").unwrap_or_default().to_owned(),
            tag,
            method,
            target,
            label: label_of(doc, node),
        });
    }
    out
}

/// Roles that make a plain element clickable in the semantic tree. An element carrying
/// one of these had better be able to act.
const CLICKABLE_ROLES: &[&str] = &["button", "link", "checkbox", "radio", "tab", "menuitem", "switch", "option"];

/// The affordances an inert element must not have.
///
/// `cursor: pointer` says "press me"; so does lighting up under the pointer. Neither can
/// be seen by looking at the markup, so both are read off the engine's own cascade: once at
/// rest, and once with every element hovered, which is every `:hover` rule at once.
///
/// An element is only judged when nothing around it could be answering the click: it is not
/// interactive itself, no ancestor is (so a `<span>` inside a `<button>` is fine), and — for
/// the hover test — it contains no control either, so a row that lights up to reveal the real
/// buttons inside it is not accused of anything.
fn affordance_lies(doc: &Dom) -> Vec<String> {
    use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
    use cw_web::style::computed::Cursor;
    use cw_web::Strictness;
    let mut sheets = Vec::new();
    for node in doc.descendants(Dom::ROOT) {
        if doc.is(node, "style") {
            match parse_stylesheet(&doc.text_content(node), Origin::Author, Strictness::Lenient) {
                Ok(sheet) => sheets.push(sheet),
                Err(_) => return Vec::new(),
            }
        }
    }
    let media = Media::default();
    let Ok(resting) = cw_web::style::cascade(doc, &sheets, &media, &MatchContext::new(), Strictness::Lenient) else {
        return Vec::new();
    };
    // Every element hovered at once: the union of what the sheet promises under the pointer.
    let mut hovered_ctx = MatchContext::new();
    hovered_ctx.hovered = doc.descendants(Dom::ROOT).filter(|n| doc.is_element(*n)).collect();
    let hovered = cw_web::style::cascade(doc, &sheets, &media, &hovered_ctx, Strictness::Lenient).ok();
    let mut out = Vec::new();
    for node in doc.descendants(Dom::ROOT) {
        if !doc.is_element(node) || is_interactive(doc, node) {
            continue;
        }
        if doc.ancestors(node).any(|a| doc.is_element(a) && is_interactive(doc, a)) {
            continue;
        }
        let at_rest = resting.get(node);
        if at_rest.map(|s| s.cursor) == Some(Cursor::Pointer) {
            out.push(format!(
                "{} is drawn with cursor: pointer but nothing happens when it is clicked",
                named(doc, node)
            ));
            continue;
        }
        // A row that reveals the controls inside it is honest; one with nothing inside it is not.
        if doc.descendants(node).any(|d| doc.is_element(d) && is_interactive(doc, d)) {
            continue;
        }
        let under_pointer = hovered.as_ref().and_then(|set| set.get(node));
        if let (Some(rest), Some(over)) = (at_rest, under_pointer) {
            if rest != over {
                out.push(format!(
                    "{} changes under the pointer but nothing happens when it is clicked",
                    named(doc, node)
                ));
                continue;
            }
        }
    }
    out
}

/// The quietest lie of the three, and the one [`page`] does not look for: no pointer
/// cursor, no hover, just the same box the page's own controls are drawn in — a tile
/// beside tiles that are links, a pill beside pills that are buttons. Nothing in the
/// markup says so and nothing in the cascade changes; only the painted box gives it away,
/// which is why it has to be compared against the page's own vocabulary rather than
/// against a fixed idea of what a control looks like.
///
/// Separate from [`page`] and opt-in, because a page may dress an element like its
/// controls on purpose: the day you are on in a month grid, the tab you are in. Say so
/// with `aria-current`, which this skips, rather than by leaving the check off.
pub fn clothes(html: &str) -> Vec<String> {
    use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
    use cw_web::Strictness;
    let doc = cw_web::html::parse(html);
    let mut sheets = Vec::new();
    for node in doc.descendants(Dom::ROOT) {
        if doc.is(node, "style") {
            match parse_stylesheet(&doc.text_content(node), Origin::Author, Strictness::Lenient) {
                Ok(sheet) => sheets.push(sheet),
                Err(_) => return Vec::new(),
            }
        }
    }
    let Ok(styles) = cw_web::style::cascade(&doc, &sheets, &Media::default(), &MatchContext::new(), Strictness::Lenient)
    else {
        return Vec::new();
    };
    let worn: Vec<&cw_web::style::computed::ComputedStyle> = doc
        .descendants(Dom::ROOT)
        .filter(|n| doc.is_element(*n) && is_interactive(&doc, *n) && !doc.is(*n, "form"))
        .filter_map(|n| styles.get(n))
        .filter(|s| dressed_as_a_control(s))
        .collect();
    let mut out = Vec::new();
    for node in doc.descendants(Dom::ROOT) {
        if !doc.is_element(node) || is_interactive(&doc, node) || doc.has_attr(node, "aria-current") {
            continue;
        }
        if doc.ancestors(node).any(|a| doc.is_element(a) && is_interactive(&doc, a)) {
            continue;
        }
        if doc.descendants(node).any(|d| doc.is_element(d) && is_interactive(&doc, d)) {
            continue;
        }
        let Some(style) = styles.get(node).filter(|s| dressed_as_a_control(s)) else {
            continue;
        };
        if worn.iter().any(|control| same_clothes(style, control)) {
            out.push(format!(
                "{} is drawn in the same box as this page's controls but is not one",
                named(&doc, node)
            ));
        }
    }
    out
}

/// Whether an element is painted as a box at all: a fill or a drawn border. Plain text
/// wears nothing, so it can never be mistaken for a control.
fn dressed_as_a_control(style: &cw_web::style::computed::ComputedStyle) -> bool {
    use cw_web::style::computed::BorderStyle;
    let border = &style.border.top;
    style.background_color.3 > 0 || (border.width > cw_web::geom::Au::ZERO && border.style != BorderStyle::None)
}

/// Whether two elements are painted in the same box: same fill, same drawn border, same
/// corners. Only what is actually painted counts — an undrawn border still carries a
/// colour, and `border-color` defaults to `currentColor`, so comparing the whole border
/// would say a blue link and black prose wear different boxes when neither has one.
fn same_clothes(
    a: &cw_web::style::computed::ComputedStyle,
    b: &cw_web::style::computed::ComputedStyle,
) -> bool {
    use cw_web::style::computed::{BorderSide, BorderStyle, ComputedStyle};
    /// A colour as four channels, so the comparison needs no type from the scene crate.
    type Paint = (u8, u8, u8, u8);
    /// One edge of the box: nothing at all, or a width, a style and a colour.
    type Edge = (i32, BorderStyle, Option<Paint>);
    /// The whole painted box: its fill, and its four edges.
    type Painted = (Option<Paint>, [Edge; 4]);
    fn drawn(side: &BorderSide) -> Edge {
        match side.width > cw_web::geom::Au::ZERO && side.style != BorderStyle::None {
            true => (
                side.width.0,
                side.style,
                Some((side.color.0, side.color.1, side.color.2, side.color.3)),
            ),
            false => (0, BorderStyle::None, None),
        }
    }
    fn box_of(s: &ComputedStyle) -> Painted {
        let fill = match s.background_color.3 {
            0 => None,
            _ => Some((
                s.background_color.0,
                s.background_color.1,
                s.background_color.2,
                s.background_color.3,
            )),
        };
        (
            fill,
            [
                drawn(&s.border.top),
                drawn(&s.border.right),
                drawn(&s.border.bottom),
                drawn(&s.border.left),
            ],
        )
    }
    box_of(a) == box_of(b) && a.border_radius == b.border_radius
}

/// What is wrong with one rendered page, without making any further request: controls
/// with nowhere to go, fields outside a form, fake roles, dangling fragments and
/// labels, and inert elements drawn as if they were pressable.
pub fn page(html: &str) -> Vec<String> {
    let doc = cw_web::html::parse(html);
    let ids: BTreeSet<String> = doc
        .descendants(Dom::ROOT)
        .filter(|n| doc.is_element(*n))
        .filter_map(|n| doc.attr(n, "id").map(str::to_owned))
        .collect();
    let mut out = Vec::new();
    for node in doc.descendants(Dom::ROOT) {
        if !doc.is_element(node) {
            continue;
        }
        let tag = doc.tag(node).unwrap_or("");
        let name = named(&doc, node);
        if doc.has_attr(node, "onclick") {
            out.push(format!("{name} has an onclick, and this world runs no page script"));
        }
        if let Some(role) = doc.attr(node, "role").filter(|r| CLICKABLE_ROLES.contains(r)) {
            let real = matches!(tag, "a" | "area") && doc.has_attr(node, "href")
                || matches!(tag, "button" | "input" | "select" | "textarea" | "summary" | "option");
            if !real {
                out.push(format!("{name} claims role={role:?} but is not a control"));
            }
        }
        if let Some(index) = doc.attr(node, "tabindex").and_then(|t| t.trim().parse::<i32>().ok()) {
            let focusable = matches!(tag, "a" | "area" | "button" | "input" | "select" | "textarea" | "summary");
            if index >= 0 && !focusable {
                out.push(format!("{name} is focusable by tabindex but is not a control"));
            }
        }
        match tag {
            "a" | "area" => {
                let href = doc.attr(node, "href").unwrap_or_default().trim().to_owned();
                if href.is_empty() {
                    // Without an href it is not a link at all; with an empty one it reloads.
                    if doc.has_attr(node, "href") {
                        out.push(format!("{name} has an empty href, so it only reloads the page"));
                    }
                } else if href == "#" {
                    out.push(format!("{name} links to \"#\", which goes nowhere"));
                } else if let Some(fragment) = href.strip_prefix('#') {
                    if !ids.contains(fragment) {
                        out.push(format!("{name} links to #{fragment}, which is not on the page"));
                    }
                }
                if label_of(&doc, node).is_empty() {
                    out.push(format!("{name} is a link with no readable label"));
                }
            }
            "button" => {
                let kind = doc.attr(node, "type").unwrap_or("submit").to_ascii_lowercase();
                if kind == "button" || kind == "reset" {
                    out.push(format!("{name} is type={kind:?}, which needs a script this world does not run"));
                } else if ancestor_form(&doc, node).is_none() && !doc.has_attr(node, "formaction") {
                    out.push(format!("{name} is a button in no form, so pressing it does nothing"));
                }
                if label_of(&doc, node).is_empty() {
                    out.push(format!("{name} is a button with no readable label"));
                }
            }
            "input" | "select" | "textarea" => {
                let kind = if tag == "input" { input_kind(&doc, node) } else { String::new() };
                if kind == "hidden" {
                    continue;
                }
                if ancestor_form(&doc, node).is_none() {
                    out.push(format!("{name} is a field in no form, so what is typed into it goes nowhere"));
                } else if kind != "submit" && kind != "image" && kind != "reset" && !doc.has_attr(node, "name") {
                    out.push(format!("{name} is a field with no name, so its value is never submitted"));
                }
                let labelled = doc.has_attr(node, "aria-label")
                    || doc.attr(node, "id").is_some_and(|id| {
                        doc.descendants(Dom::ROOT).any(|n| doc.is(n, "label") && doc.attr(n, "for") == Some(id))
                    });
                if !labelled && kind != "submit" && kind != "image" && kind != "reset" {
                    out.push(format!("{name} is a field with no label an agent can read"));
                }
            }
            "label" => {
                if let Some(target) = doc.attr(node, "for") {
                    if !ids.contains(target) {
                        out.push(format!("{name} labels #{target}, which is not on the page"));
                    }
                }
            }
            "form" => {
                if doc.attr(node, "action").unwrap_or_default().trim().is_empty() {
                    out.push(format!("{name} has no action, so submitting it reloads the same page"));
                }
                let submits = doc.descendants(node).any(|n| {
                    doc.is(n, "button") && !matches!(doc.attr(n, "type"), Some("button") | Some("reset"))
                        || doc.is(n, "input") && matches!(input_kind(&doc, n).as_str(), "submit" | "image")
                });
                if !submits {
                    out.push(format!("{name} has no submit control, so nothing can send it"));
                }
            }
            _ => {}
        }
    }
    out.extend(affordance_lies(&doc));
    out
}

/// A crawl of one service: follow every same-origin link from the seeds, probe every
/// form action, and report everything that answers with a 404, a 405 or an error, plus
/// every per-page fault [`page`] finds.
///
/// `call` makes one request and returns `(status, body)`. A `POST` probe is a real
/// request, so a caller whose service writes state should hand the probe a scratch copy
/// of it; see [`Sweep::probe_posts`] to turn the probes off instead.
pub struct Sweep<'a> {
    call: &'a mut dyn FnMut(&str, &str) -> (u16, String),
    seeds: Vec<String>,
    allow_self: BTreeSet<String>,
    skip: BTreeSet<String>,
    probe_posts: bool,
    clothes: bool,
    limit: usize,
}

impl<'a> Sweep<'a> {
    /// A sweep from the given paths, following links up to 200 pages.
    pub fn new(seeds: &[&str], call: &'a mut dyn FnMut(&str, &str) -> (u16, String)) -> Self {
        Self {
            call,
            seeds: seeds.iter().map(|s| (*s).to_owned()).collect(),
            allow_self: BTreeSet::new(),
            skip: BTreeSet::new(),
            probe_posts: true,
            clothes: false,
            limit: 200,
        }
    }
    /// Ids whose link may lead back to the page it is on, because the real product's
    /// does too — the current tab in a tab strip, a canonical self link.
    ///
    /// A link that carries `aria-current` needs no entry here: saying "you are here" is
    /// the honest form of a link back to the page it sits on, and it is what the semantic
    /// tree passes on to the agent.
    pub fn allow_self(mut self, ids: &[&str]) -> Self {
        self.allow_self.extend(ids.iter().map(|s| (*s).to_owned()));
        self
    }
    /// Paths the crawl must not follow: an unbounded space, or a route whose side
    /// effect the test does not want.
    pub fn skip(mut self, paths: &[&str]) -> Self {
        self.skip.extend(paths.iter().map(|s| (*s).to_owned()));
        self
    }
    /// Whether to send the `POST` a form or a `formaction` names. On by default.
    pub fn probe_posts(mut self, probe: bool) -> Self {
        self.probe_posts = probe;
        self
    }
    /// Whether to run [`clothes`] on every page too: off by default, because a page may
    /// dress an element like its controls deliberately and has to say so with
    /// `aria-current` first.
    pub fn clothes(mut self, check: bool) -> Self {
        self.clothes = check;
        self
    }
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Runs the crawl and returns every complaint, deduplicated, in a stable order.
    pub fn run(self) -> Vec<String> {
        let mut faults: BTreeSet<String> = BTreeSet::new();
        let mut queue: VecDeque<String> = self.seeds.iter().cloned().collect();
        let mut seen: BTreeSet<String> = self.seeds.iter().cloned().collect();
        let mut probed: BTreeMap<(String, String), u16> = BTreeMap::new();
        let mut visited = 0usize;
        while let Some(path) = queue.pop_front() {
            if visited >= self.limit {
                break;
            }
            visited += 1;
            let (status, body) = (self.call)("GET", &path);
            if status == 404 || status == 405 || status >= 500 {
                faults.insert(format!("GET {path} answers {status}"));
                continue;
            }
            if status >= 300 || !looks_like_html(&body) {
                continue;
            }
            let mut found = page(&body);
            if self.clothes {
                found.extend(clothes(&body));
            }
            for fault in found {
                faults.insert(format!("{path}: {fault}"));
            }
            let doc = cw_web::html::parse(&body);
            // Links that say they are the page being read: `aria-current` is the marker an
            // agent sees, so a link wearing it is not pretending to lead anywhere else.
            let current: BTreeSet<String> = doc
                .descendants(Dom::ROOT)
                .filter(|n| doc.is_element(*n) && doc.has_attr(*n, "aria-current"))
                .filter_map(|n| doc.attr(n, "id"))
                .map(str::to_owned)
                .collect();
            for control in controls(&doc) {
                let Some(target) = resolve(&path, &control.target) else {
                    continue;
                };
                if self.skip.contains(&target) {
                    continue;
                }
                let key = (control.method.clone(), target.clone());
                let status = match probed.get(&key) {
                    Some(status) => *status,
                    None => {
                        if control.method == "POST" && !self.probe_posts {
                            continue;
                        }
                        let (status, _) = (self.call)(&control.method, &target);
                        probed.insert(key, status);
                        status
                    }
                };
                let which = match control.id.is_empty() {
                    true => format!("<{}>", control.tag),
                    false => format!("#{}", control.id),
                };
                if status == 404 || status == 405 || status >= 500 {
                    faults.insert(format!(
                        "{path}: {which} sends {} {target}, which answers {status}",
                        control.method
                    ));
                    continue;
                }
                // A form's action is a template — its fields decide the request — so only a
                // plain link that leads back where you already are is a dead control.
                if control.tag == "a"
                    && target == path
                    && !self.allow_self.contains(&control.id)
                    && !current.contains(&control.id)
                {
                    faults.insert(format!("{path}: {which} links to the page it is on"));
                }
                if control.method == "GET" && seen.insert(target.clone()) {
                    queue.push_back(target);
                }
            }
        }
        faults.into_iter().collect()
    }
}

fn looks_like_html(body: &str) -> bool {
    let head = body.trim_start().to_ascii_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html")
}

/// A same-origin request path, or `None` for a fragment, an empty target, or an
/// absolute URL — a link off the site is the world's problem, not the service's.
fn resolve(from: &str, target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() || target.starts_with('#') || target.contains("://") || target.starts_with("//") {
        return None;
    }
    if target.starts_with('/') {
        return Some(target.to_owned());
    }
    let base = from.split(['?', '#']).next().unwrap_or("/");
    let dir = match base.rfind('/') {
        Some(cut) => &base[..=cut],
        None => "/",
    };
    Some(format!("{dir}{target}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(body: &str) -> String {
        format!("<!DOCTYPE html><html><head><title>t</title></head><body>{body}</body></html>")
    }

    #[test]
    fn a_dead_control_is_named_and_a_live_one_is_not() {
        let faults = page(&wrap(
            r##"<a id="ghost">Save</a><a id="hash" href="#">Follow</a>
               <a id="gone" href="#nowhere">Talk</a><button id="loose">Attach</button>
               <span id="fake" role="button">Emoji</span><div id="click" onclick="x()">Pin</div>
               <input id="stray" name="q"><form id="f" action="/go" method="get">
               <input id="q" name="q" aria-label="Query"><button id="go">Go</button></form>"##,
        ));
        let joined = faults.join("\n");
        for expected in [
            "<a id=\"hash\"> links to \"#\"",
            "<a id=\"gone\"> links to #nowhere",
            "<button id=\"loose\"> is a button in no form",
            "<span id=\"fake\"> claims role=\"button\"",
            "<div id=\"click\"> has an onclick",
            "<input id=\"stray\"> is a field in no form",
        ] {
            assert!(joined.contains(expected), "missing {expected} in:\n{joined}");
        }
        // The live form and its field and button are not complained about.
        assert!(!joined.contains("id=\"q\""), "{joined}");
        assert!(!joined.contains("id=\"go\""), "{joined}");
        assert!(!joined.contains("id=\"f\""), "{joined}");
        // An anchor with no href at all is prose, not a control, and is left alone.
        assert!(!joined.contains("id=\"ghost\""), "{joined}");
    }

    #[test]
    fn an_inert_element_drawn_as_pressable_is_a_lie() {
        let html = "<!DOCTYPE html><html><head><title>t</title><style>.tile { cursor: pointer }</style></head>\
                    <body><div id=\"tile\" class=\"tile\">Add server</div>\
                    <a id=\"real\" class=\"tile\" href=\"/x\">Explore</a></body></html>";
        let faults = page(html);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("id=\"tile\""), "{faults:?}");
    }

    /// The subtler version: no pointer cursor, but the thing lights up when you reach for it.
    #[test]
    fn an_inert_element_that_lights_up_under_the_pointer_is_the_same_lie() {
        let html = "<!DOCTYPE html><html><head><title>t</title><style>\
                    .member:hover { background-color: #eee }\
                    .msg:hover .picker { opacity: 1 }\
                    a:hover { text-decoration: underline }\
                    </style></head><body>\
                    <div id=\"member\" class=\"member\">carol</div>\
                    <div id=\"msg\" class=\"msg\">hi<span id=\"picker\" class=\"picker\">\
                    <form id=\"f\" action=\"/react\" method=\"post\"><button id=\"b\">+</button></form></span></div>\
                    <a id=\"real\" href=\"/x\">Explore</a></body></html>";
        let faults = page(html);
        // The member row promises something it cannot do.
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("id=\"member\"") && faults[0].contains("under the pointer"), "{faults:?}");
        // The revealer wrapping real buttons, and the link, are both honest.
        let joined = faults.join("\n");
        assert!(!joined.contains("picker") && !joined.contains("real"), "{joined}");
    }

    /// The lie with no cursor and no hover: a tile beside tiles that are links.
    #[test]
    fn an_inert_element_in_a_control_s_clothes_is_caught_only_when_asked() {
        let html = "<!DOCTYPE html><html><head><title>t</title><style>\
                    .tile, .bang { background-color: #f1f3f4; border-radius: 10px; padding: 10px }\
                    </style></head><body>\
                    <a id=\"trend\" class=\"tile\" href=\"/s?q=atlas\">atlas</a>\
                    <div id=\"bang\" class=\"bang\">!gh - GitHub</div>\
                    <span id=\"tab\" class=\"tile\" aria-current=\"page\">All</span>\
                    <p id=\"prose\">just words</p></body></html>";
        // `page` does not look for this: nothing about the box changes under the pointer.
        assert!(page(html).is_empty(), "{:?}", page(html));
        let faults = clothes(html);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("id=\"bang\""), "{faults:?}");
        // The link wears it honestly, the tab says it is where you are, prose wears nothing.
        let joined = faults.join("\n");
        assert!(!joined.contains("trend") && !joined.contains("\"tab\"") && !joined.contains("prose"), "{joined}");
    }

    #[test]
    fn a_form_needs_an_action_and_a_way_to_send_it() {
        let faults = page(&wrap(
            r#"<form id="a" method="post"><input id="i" name="x" aria-label="X"><button id="b">Go</button></form>
               <form id="c" action="/c" method="post"><input id="j" name="y" aria-label="Y"></form>"#,
        ));
        let joined = faults.join("\n");
        assert!(joined.contains("<form id=\"a\"> has no action"), "{joined}");
        assert!(joined.contains("<form id=\"c\"> has no submit control"), "{joined}");
    }

    #[test]
    fn the_sweep_follows_links_and_names_the_route_that_is_missing() {
        let pages = |path: &str| match path {
            "/" => (
                200,
                wrap(r#"<a id="ok" href="/next">Next</a><a id="bad" href="/ghost">Ghost</a>
                        <a id="self" href="/">Home</a>"#),
            ),
            "/next" => (200, wrap("<p>end</p>")),
            _ => (404, String::new()),
        };
        let mut call = |_method: &str, path: &str| pages(path);
        let faults = Sweep::new(&["/"], &mut call).run();
        assert_eq!(
            faults,
            ["/: #bad sends GET /ghost, which answers 404", "/: #self links to the page it is on"]
        );
        let mut call = |_method: &str, path: &str| pages(path);
        let allowed = Sweep::new(&["/"], &mut call).allow_self(&["self"]).skip(&["/ghost"]).run();
        assert_eq!(allowed, Vec::<String>::new());
    }

    #[test]
    fn relative_targets_resolve_against_the_page() {
        assert_eq!(resolve("/a/b", "c").as_deref(), Some("/a/c"));
        assert_eq!(resolve("/a/b?x=1", "c").as_deref(), Some("/a/c"));
        assert_eq!(resolve("/a/b", "/c").as_deref(), Some("/c"));
        assert_eq!(resolve("/", "#x"), None);
        assert_eq!(resolve("/", "http://other.test/x"), None);
    }
}
