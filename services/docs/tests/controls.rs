//! No dead controls: every page of every skin is walked, every control it draws is collected,
//! and the service is asked for the route behind it.
//!
//! "Control" here is not only `<a href>`. It is every form action, every `formaction` on a
//! button or input, and every one-button form (`star-form` around `star`) — the shapes
//! `docs/html-migration.md` says a `PageElement::Button` becomes. Each one is issued against a
//! clone of the state with the fields its own form declares, and the answer must be something
//! other than 404 (no such route) or 405 (no such method): a control whose only possible reply
//! is "no such thing" is decoration wearing a button's clothes.
//!
//! The other half of the standard is the reverse: nothing that cannot act may look as though it
//! can. So the page's own stylesheet is read back, and every rule that paints a hover state or
//! a pointer cursor must land on something that really is a link, a button or a field.
//!
//! The pages come from `worlds/company-2026/sites/*.json` — the shipped seeds — read by two
//! actors with different grants, plus synthetic states for the page kinds no seed reaches: an
//! empty workspace, an empty deck, and a sheet and a deck their reader may not write.
use cw_protocol::{HttpRequest, HttpResponse};
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::{validate_strict, HTML_MEDIA_TYPE};
use cw_service_docs::DocsService;
use cw_web::css::{
    matches_list, parse_stylesheet, ComplexSelector, CompoundSelector, MatchContext, Media, Origin,
    PseudoClass, SelectorList, SimpleSelector,
};
use cw_web::dom::{Document, NodeId};
use cw_web::Strictness;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const GOOGLE_DOCS: &str = include_str!("../../../worlds/company-2026/sites/google-docs.json");
const NOTION: &str = include_str!("../../../worlds/company-2026/sites/notion.json");

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 7,
        instance: "docs".into(),
    }
}
fn boot(initial: Value) -> Value {
    DocsService
        .initialize(initial, &ctx("alice"))
        .expect("the seed must load")
}
fn seeded(site: &str) -> Value {
    let site: Value = serde_json::from_str(site).unwrap();
    assert_eq!(site["kind"], "docs");
    boot(site["initial_state"].clone())
}
fn get(state: &mut Value, actor: &str, path: &str) -> HttpResponse {
    DocsService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::get(format!("http://docs{path}")),
        )
        .unwrap_or_else(|e| panic!("GET {path} as {actor}: {e:?}"))
}

/// One control the page drew, with enough about it to ask the service for its route.
#[derive(Debug)]
struct Control {
    /// `a`, `form`, `button` or `input`, for the report when the assertion fails.
    tag: String,
    id: String,
    method: String,
    target: String,
    /// The fields the owning form declares, `name` to a value the service will accept.
    fields: Vec<(String, String)>,
}

/// Every skin and every page kind the service can draw, as `(what, state, actor, paths)`.
fn every_page() -> Vec<(String, Value, &'static str, Vec<String>)> {
    // A workspace with nothing in it: the empty gallery, the empty Favorites, the empty tabs.
    let empty = |skin: &str| json!({"skin": skin, "documents": {}, "next_id": 0});
    // A sheet and a deck bob may read and not write, and a deck with no slides yet.
    let readable = |skin: &str| {
        json!({"skin": skin, "documents": {
            "locked-sheet": {"id":"locked-sheet","title":"Locked sheet","owner":"carol","doc_type":"sheet","readers":["bob"],"revision":3,"cells":{"A1":"Metric","B1":"Q3","A2":"Latency p95","B2":"184 ms"},"comments":[{"author":"carol","text":"Numbers are final","time":4}]},
            "locked-deck": {"id":"locked-deck","title":"Locked deck","owner":"carol","doc_type":"slides","readers":["bob"],"revision":2,"slides":[{"title":"Where we are","body":"Ship date locked."}]},
            "locked-doc": {"id":"locked-doc","title":"Locked doc","owner":"carol","readers":["bob"],"revision":2,"body":"Locked doc\n\nSee http://github.com/northstar/atlas\n- [x] Freeze the branch\n"},
            "empty-deck": {"id":"empty-deck","title":"Empty deck","owner":"bob","doc_type":"slides","revision":1},
            "empty-sheet": {"id":"empty-sheet","title":"Empty sheet","owner":"bob","doc_type":"sheet","revision":1},
            "empty-doc": {"id":"empty-doc","title":"Empty doc","owner":"bob","revision":1}}})
    };
    let mut out = Vec::new();
    for (what, initial, actors) in [
        ("google-docs", seeded(GOOGLE_DOCS), ["alice", "bob"]),
        ("notion", seeded(NOTION), ["alice", "bob"]),
        // docs.internal wears `plain`, whose pages are `Page` JSON; its controls count too.
        ("plain", seeded(GOOGLE_DOCS), ["alice", "bob"]),
        ("gdocs empty", boot(empty("gdocs")), ["alice", "bob"]),
        ("notion empty", boot(empty("notion")), ["alice", "bob"]),
        ("plain empty", boot(empty("plain")), ["alice", "bob"]),
        ("gdocs grants", boot(readable("gdocs")), ["bob", "carol"]),
        ("notion grants", boot(readable("notion")), ["bob", "carol"]),
        ("plain grants", boot(readable("plain")), ["bob", "carol"]),
    ] {
        let mut state = initial;
        if what.starts_with("plain") {
            state["skin"] = json!("plain");
        }
        let mut paths: Vec<String> = [
            "/",
            "/?type=doc",
            "/?type=sheet",
            "/?type=slides",
            "/starred",
        ]
        .iter()
        .map(|p| (*p).to_owned())
        .collect();
        for id in state["documents"].as_object().unwrap().keys() {
            paths.push(format!("/documents/{id}"));
        }
        for actor in actors {
            out.push((
                format!("{what} as {actor}"),
                state.clone(),
                actor,
                paths.clone(),
            ));
        }
    }
    out
}

/// The value the probe sends for a field, so a form is refused on its merits or not at all:
/// its own `value`, else something the service will parse.
fn field_value(doc: &Document, node: NodeId, name: &str) -> String {
    let current = if doc.is(node, "textarea") {
        doc.text_content(node)
    } else {
        doc.attr(node, "value").unwrap_or_default().to_owned()
    };
    if !current.trim().is_empty() {
        return current;
    }
    match name {
        "revision" | "index" => "1".into(),
        "cell" => "A1".into(),
        "type" => "doc".into(),
        _ => "probe".into(),
    }
}
fn form_fields(doc: &Document, form: NodeId) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for n in doc.descendants(form) {
        if !matches!(doc.tag(n), Some("input" | "textarea" | "select")) {
            continue;
        }
        if let Some(name) = doc.attr(n, "name") {
            let value = if doc.is(n, "select") {
                doc.descendants(n)
                    .find(|o| doc.is(*o, "option"))
                    .and_then(|o| doc.attr(o, "value"))
                    .unwrap_or("doc")
                    .to_owned()
            } else {
                field_value(doc, n, name)
            };
            out.push((name.to_owned(), value));
        }
    }
    out
}

/// Every control an HTML page draws, and a check that each one is wired to something.
fn controls_of_html(what: &str, html: &str) -> Vec<Control> {
    validate_strict(html).unwrap_or_else(|e| panic!("{what}: {e:?}"));
    // The shared audit over the same page: a control with nowhere to go, an `onclick` in
    // a world with no script, a role or a tabindex on something inert, a field with no
    // name or label, a fragment that is not here — and, from the engine's own cascade, an
    // inert element with a pointer cursor, a hover state, or the painted box this page's
    // own controls wear.
    if let Some(fault) = cw_service_common::audit::page(html)
        .into_iter()
        .chain(cw_service_common::audit::clothes(html))
        .next()
    {
        panic!("{what}: {fault}");
    }
    let doc = cw_web::html::parse(html);
    let id_of = |n: NodeId| doc.attr(n, "id").unwrap_or("").to_owned();
    let owning_form = |n: NodeId| doc.ancestors(n).find(|a| doc.is(*a, "form"));
    let mut out = Vec::new();
    for node in doc.descendants(Document::ROOT) {
        let tag = doc.tag(node).unwrap_or("").to_owned();
        match tag.as_str() {
            "a" => {
                let href = doc.attr(node, "href").unwrap_or("");
                assert!(
                    !href.is_empty() && href != "#" && !href.starts_with("javascript:"),
                    "{what}: <a id={:?}> goes nowhere (href {href:?})",
                    id_of(node)
                );
                out.push(Control {
                    tag,
                    id: id_of(node),
                    method: "GET".into(),
                    target: href.to_owned(),
                    fields: vec![],
                });
            }
            "form" => {
                let action = doc.attr(node, "action").unwrap_or("");
                assert!(
                    !action.is_empty(),
                    "{what}: form {:?} posts nowhere",
                    id_of(node)
                );
                let method = doc
                    .attr(node, "method")
                    .unwrap_or("get")
                    .to_ascii_uppercase();
                out.push(Control {
                    tag,
                    id: id_of(node),
                    method,
                    target: action.to_owned(),
                    fields: form_fields(&doc, node),
                });
            }
            "button" | "input" | "select" | "textarea" => {
                // A control outside a form can only be wired up by script, and this world runs
                // none: it would be a button whose click goes nowhere.
                let form = owning_form(node).unwrap_or_else(|| {
                    panic!("{what}: <{tag} id={:?}> is outside every form", id_of(node))
                });
                if let Some(action) = doc.attr(node, "formaction") {
                    let method = doc
                        .attr(node, "formmethod")
                        .or_else(|| doc.attr(form, "method"))
                        .unwrap_or("get")
                        .to_ascii_uppercase();
                    assert!(
                        !action.is_empty(),
                        "{what}: {tag} {:?} has an empty formaction",
                        id_of(node)
                    );
                    out.push(Control {
                        tag,
                        id: id_of(node),
                        method,
                        target: action.to_owned(),
                        fields: form_fields(&doc, form),
                    });
                }
            }
            _ => {}
        }
    }
    hover_and_cursor_land_on_real_controls(what, &doc);
    out
}

/// Every control the `plain` skin's `Page` JSON draws: a link's `url`, a form's action, and a
/// button's action, which the migration note says may differ from its form's.
fn controls_of_page(what: &str, page: &Value) -> Vec<Control> {
    fn walk(what: &str, elements: &[Value], out: &mut Vec<Control>) {
        for e in elements {
            let id = e["id"].as_str().unwrap_or_default().to_owned();
            let kind = e["kind"].as_str().unwrap_or_default();
            if kind == "link" {
                let url = e["url"].as_str().unwrap_or_default();
                assert!(
                    !url.is_empty() && url != "#",
                    "{what}: link {id:?} goes nowhere"
                );
                out.push(Control {
                    tag: "link".into(),
                    id,
                    method: "GET".into(),
                    target: url.to_owned(),
                    fields: vec![],
                });
            } else if let Some(action) = e.get("action").filter(|a| !a.is_null()) {
                let url = action["url"].as_str().unwrap_or_default();
                assert!(!url.is_empty(), "{what}: {kind} {id:?} acts on nothing");
                // `$field` references name an input on the page; the probe fills them in.
                let fields = action["fields"]
                    .as_object()
                    .map(|f| {
                        f.iter()
                            .map(|(k, _)| {
                                (
                                    k.clone(),
                                    if k == "revision" {
                                        "1".to_owned()
                                    } else {
                                        "probe".to_owned()
                                    },
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(Control {
                    tag: kind.to_owned(),
                    id,
                    method: action["method"]
                        .as_str()
                        .unwrap_or("GET")
                        .to_ascii_uppercase(),
                    target: url.to_owned(),
                    fields,
                });
            }
            if let Some(children) = e["children"].as_array() {
                walk(what, children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(
        what,
        page["elements"].as_array().unwrap_or(&vec![]),
        &mut out,
    );
    out
}

/// Does the selector list mention `:hover` anywhere, including inside `:is()` and friends?
fn mentions_hover(list: &SelectorList) -> bool {
    fn compound(c: &CompoundSelector) -> bool {
        c.simple.iter().any(|s| match s {
            SimpleSelector::PseudoClass(PseudoClass::Hover) => true,
            SimpleSelector::PseudoClass(
                PseudoClass::Not(l) | PseudoClass::Is(l) | PseudoClass::Where(l),
            ) => mentions_hover(l),
            _ => false,
        })
    }
    fn complex(s: &ComplexSelector) -> bool {
        s.compounds.iter().any(compound)
    }
    list.0.iter().any(complex)
}

/// The visual half of the standard: a hover state or a pointer cursor is a promise that the
/// thing under the pointer can be pressed, so it must land on something that can be.
fn hover_and_cursor_land_on_real_controls(what: &str, doc: &Document) {
    let mut sheets = Vec::new();
    for node in doc.descendants(Document::ROOT) {
        if doc.is(node, "style") {
            sheets.push(
                parse_stylesheet(&doc.text_content(node), Origin::Author, Strictness::Strict)
                    .unwrap_or_else(|e| panic!("{what}: {e:?}")),
            );
        }
    }
    // Every element is "hovered", so a `:hover` rule matches exactly the elements it dresses.
    let elements: Vec<NodeId> = doc
        .descendants(Document::ROOT)
        .filter(|n| doc.is_element(*n))
        .collect();
    let mut ctx = MatchContext::new();
    ctx.hovered = elements.iter().copied().collect::<BTreeSet<_>>();
    let media = Media::with_size(1280, 800);
    let supported = |_: &str, _: &[cw_web::css::ComponentValue]| true;
    let pressable = |n: NodeId| {
        let real = |x: NodeId| match doc.tag(x) {
            Some("a") => doc.attr(x, "href").is_some_and(|h| !h.is_empty()),
            Some("button" | "input" | "select" | "textarea" | "label" | "summary") => true,
            _ => false,
        };
        real(n) || doc.ancestors(n).any(real) || doc.descendants(n).any(real)
    };
    for sheet in &sheets {
        for rule in sheet.effective_style_rules(&media, &supported) {
            let cursor = rule.declarations.iter().any(|d| {
                d.name == "cursor" && d.value.iter().any(|v| v.as_ident() == Some("pointer"))
            });
            if !cursor && !mentions_hover(rule.selectors) {
                continue;
            }
            for &node in &elements {
                if !matches_list(doc, node, rule.selectors, &ctx) {
                    continue;
                }
                assert!(
                    pressable(node),
                    "{what}: <{} id={:?} class={:?}> is painted as pressable by `{}` but is not a link, a button or a field",
                    doc.tag(node).unwrap_or(""),
                    doc.attr(node, "id").unwrap_or(""),
                    doc.attr(node, "class").unwrap_or(""),
                    rule.selectors.0.first().map(|s| format!("{s}")).unwrap_or_default(),
                );
            }
        }
    }
}

#[test]
fn every_control_of_every_page_of_every_skin_is_answered_by_a_route_this_crate_serves() {
    let mut pages = 0usize;
    let mut checked = 0usize;
    for (what, state, actor, paths) in every_page() {
        for path in paths {
            let mut state = state.clone();
            let response = get(&mut state, actor, &path);
            let what = format!("{what} {path}");
            if response.status == 403 && path.starts_with("/documents/") {
                continue; // not this actor's document; refusing is the right answer.
            }
            assert_eq!(response.status, 200, "{what}");
            let controls = if response.header("content-type") == Some(HTML_MEDIA_TYPE) {
                controls_of_html(&what, &String::from_utf8(response.body).unwrap())
            } else {
                controls_of_page(&what, &serde_json::from_slice(&response.body).unwrap())
            };
            pages += 1;
            for c in controls {
                if !c.target.starts_with('/') {
                    // A fragment stays on this page; `audit::page` has already checked
                    // that the id it names is really here. Anything else leaving the site
                    // is an outbound link written in someone's prose.
                    if c.target.starts_with('#') {
                        continue;
                    }
                    assert!(
                        c.target.starts_with("http://") || c.target.starts_with("https://"),
                        "{what}: {} {:?} points at {:?}",
                        c.tag,
                        c.id,
                        c.target
                    );
                    continue;
                }
                let mut request = HttpRequest::get(format!("http://docs{}", c.target));
                request.method = c.method.clone();
                if c.method != "GET" {
                    request.headers.insert(
                        "content-type".into(),
                        "application/x-www-form-urlencoded".into(),
                    );
                    request.body = c
                        .fields
                        .iter()
                        .map(|(k, v)| format!("{}={}", urlencoded(k), urlencoded(v)))
                        .collect::<Vec<_>>()
                        .join("&")
                        .into_bytes();
                }
                let mut probe = state.clone();
                let answer = DocsService
                    .handle(&mut probe, &ctx(actor), &request)
                    .unwrap_or_else(|e| panic!("{what}: {} {:?} promises {} {} and the service refuses to answer: {e:?}", c.tag, c.id, c.method, c.target));
                assert!(
                    answer.status != 404 && answer.status != 405,
                    "{what}: {} {:?} promises {} {}, which answers {}",
                    c.tag,
                    c.id,
                    c.method,
                    c.target,
                    answer.status
                );
                checked += 1;
            }
        }
    }
    // A guard on the guard: a walk that stopped finding pages would pass silently.
    assert!(
        pages >= 60 && checked >= 300,
        "{pages} pages, {checked} controls"
    );
}

fn urlencoded(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_owned(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// The two editors that could only ever answer 403 for a reader are not drawn for one, and the
/// page says so instead. The ones a writer gets still carry the ids the migration froze.
#[test]
fn a_reader_is_told_why_there_is_no_editor_rather_than_shown_one() {
    let state = boot(json!({"skin":"gdocs","documents":{
        "s": {"id":"s","title":"Numbers","owner":"carol","doc_type":"sheet","readers":["bob"],"writers":["alice"],"revision":1,"cells":{"A1":"Metric"}},
        "d": {"id":"d","title":"Review","owner":"carol","doc_type":"slides","readers":["bob"],"writers":["alice"],"revision":1,"slides":[{"title":"One","body":"Two"}]},
        "p": {"id":"p","title":"Plan","owner":"carol","readers":["bob"],"writers":["alice"],"revision":1,"body":"Plan\n\nShip it.\n"}}}));
    for (id, note, form) in [
        ("s", "sheet-readonly", "cell"),
        ("d", "deck-readonly", "slide"),
        ("p", "doc-readonly", "edit"),
    ] {
        for (actor, writes) in [("alice", true), ("bob", false)] {
            let html =
                String::from_utf8(get(&mut state.clone(), actor, &format!("/documents/{id}")).body)
                    .unwrap();
            let doc = cw_web::html::parse(&html);
            assert_eq!(
                !doc.by_id(form).is_empty(),
                writes,
                "{actor} and the {form} form on /documents/{id}"
            );
            assert_eq!(
                doc.by_id(note).is_empty(),
                writes,
                "{actor} and #{note} on /documents/{id}"
            );
        }
    }
    // Notion says the same thing in its own words.
    let mut notion = state.clone();
    notion["skin"] = json!("notion");
    let html = String::from_utf8(get(&mut notion, "bob", "/documents/p").body).unwrap();
    let doc = cw_web::html::parse(&html);
    assert!(doc.by_id("edit").is_empty());
    assert!(doc
        .text_content(doc.by_id("document-readonly")[0])
        .starts_with("You can read this page"));
}
