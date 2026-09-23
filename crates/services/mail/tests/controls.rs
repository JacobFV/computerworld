//! Every control on every mail page leads somewhere.
//!
//! The service is crawled skin by skin from the seeded mailboxes in `worlds/internet/sites`,
//! plus the states a reader can put a mailbox into: searching, labelled, starred and empty. Each
//! page is parsed and strictly validated, then every link, every form and every button that
//! submits one is collected and answered by the router. A link whose target 404s, a form posting
//! to a route the service does not have, a form with nothing to submit it or a button outside any
//! form is a dead control, and this is where one is found instead of by an agent.
use cw_protocol::{HttpRequest, HttpResponse};
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::{href, validate_strict};
use cw_service_mail::{MailService, FOLDERS};
use cw_web::dom::{Document, NodeId};
use serde_json::Value;
use std::collections::BTreeSet;

const HOST: &str = "http://mail";

fn site(name: &str) -> Value {
    let path = format!(
        "{}/../../../worlds/internet/sites/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("site file")).expect("valid JSON")
}
fn context(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "pc".into(),
        tick: 7,
        seed: 1,
        instance: "mail".into(),
    }
}
/// One request against a copy of the state: probing a control never changes the mailbox the
/// crawl is reading, so the pages stay the ones the seed describes.
fn request(
    state: &Value,
    actor: &str,
    method: &str,
    url: &str,
    fields: &[(String, String)],
) -> HttpResponse {
    let pairs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let mut r = if method == "post" {
        HttpRequest::get(format!("{HOST}{url}"))
    } else {
        HttpRequest::get(format!(
            "{HOST}{}{}",
            url,
            href("", &pairs).replacen('?', if url.contains('?') { "&" } else { "?" }, 1)
        ))
    };
    if method == "post" {
        r.method = "POST".into();
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        r.body = href("", &pairs).trim_start_matches('?').as_bytes().to_vec();
    }
    MailService
        .handle(&mut state.clone(), &context(actor), &r)
        .unwrap_or_else(|e| panic!("the router refused {method} {url}: {e:?}"))
}
/// A page's kind, with the message ids and label values folded away: `/?folder=inbox&thread=mail-9`
/// and `/?folder=inbox&thread=mail-3` are the same page to crawl, though both are still fetched.
fn shape(url: &str) -> String {
    let mut out = String::new();
    let mut rest = url;
    while let Some(cut) = rest.find("mail-") {
        out.push_str(&rest[..cut + 5]);
        rest = &rest[cut + 5..];
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        out.push('N');
        rest = &rest[digits..];
    }
    out.push_str(rest);
    match out.find("label=") {
        Some(cut) => {
            let tail = &out[cut + 6..];
            let end = tail.find('&').map_or(out.len(), |i| cut + 6 + i);
            format!("{}label=X{}", &out[..cut], &out[end..])
        }
        None => out,
    }
}
fn attr(doc: &Document, node: NodeId, name: &str) -> String {
    doc.attr(node, name).unwrap_or_default().trim().to_owned()
}
/// A same-origin target this service is supposed to answer; anything absolute belongs to
/// another site in the world and is somebody else's route table.
fn internal(target: &str) -> bool {
    target.starts_with('/') && !target.starts_with("//")
}

struct Crawl<'a> {
    state: &'a Value,
    actor: &'a str,
    name: String,
    queue: Vec<String>,
    /// Page kinds already queued or walked; a second conversation is still fetched, not walked.
    kinds: BTreeSet<String>,
    probed: BTreeSet<String>,
    pages: usize,
    controls: usize,
}
impl<'a> Crawl<'a> {
    fn new(state: &'a Value, actor: &'a str, name: String) -> Crawl<'a> {
        let mut queue: Vec<String> = vec!["/".into(), "/?folder=inbox&compose=1".into()];
        queue.extend(FOLDERS.iter().map(|f| format!("/?folder={f}")));
        let kinds = queue.iter().map(|u| shape(u)).collect();
        Crawl {
            state,
            actor,
            name,
            queue,
            kinds,
            probed: BTreeSet::new(),
            pages: 0,
            controls: 0,
        }
    }
    /// Ask for one control's target once. Every page a link points at must render; a form may
    /// answer with a domain error (an empty compose is a 400), but never with "no such route".
    fn probe(&mut self, method: &str, url: &str, fields: &[(String, String)], what: &str) -> u16 {
        let names: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
        let key = format!("{method} {url} {}", names.join(","));
        self.controls += 1;
        if !self.probed.insert(key) {
            return 200;
        }
        let status = request(self.state, self.actor, method, url, fields).status;
        assert!(
            status != 404 && status != 405,
            "{}: {what} {method} {url} answered {status}",
            self.name
        );
        if method == "get" {
            assert_eq!(status, 200, "{}: {what} {method} {url}", self.name);
        }
        status
    }
    /// Every field a browser would send for this form, and every control that submits it.
    fn submit(&mut self, doc: &Document, form: NodeId) {
        let action = attr(doc, form, "action");
        let form_id = attr(doc, form, "id");
        assert!(
            !action.is_empty(),
            "{}: form #{form_id} posts nowhere",
            self.name
        );
        let method = match attr(doc, form, "method").to_lowercase().as_str() {
            "post" => "post",
            _ => "get",
        };
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut submitters: Vec<NodeId> = Vec::new();
        for node in doc.descendants(form) {
            match doc.tag(node) {
                Some("input") => {
                    let kind = attr(doc, node, "type");
                    let name = attr(doc, node, "name");
                    if matches!(kind.as_str(), "submit" | "button" | "image") {
                        submitters.push(node);
                    } else {
                        assert!(
                            !name.is_empty(),
                            "{}: an input of #{form_id} has no name",
                            self.name
                        );
                        fields.push((name, attr(doc, node, "value")));
                    }
                }
                Some("textarea") => {
                    let name = attr(doc, node, "name");
                    assert!(
                        !name.is_empty(),
                        "{}: a textarea of #{form_id} has no name",
                        self.name
                    );
                    fields.push((name, doc.text_content(node)));
                }
                Some("button") => submitters.push(node),
                _ => {}
            }
        }
        assert!(
            !submitters.is_empty(),
            "{}: form #{form_id} has nothing to submit it",
            self.name
        );
        for node in submitters {
            let mut sent = fields.clone();
            let name = attr(doc, node, "name");
            if !name.is_empty() {
                sent.push((name, attr(doc, node, "value")));
            }
            let target = match attr(doc, node, "formaction") {
                t if t.is_empty() => action.clone(),
                t => t,
            };
            assert!(
                internal(&target),
                "{}: #{form_id} submits off-site to {target}",
                self.name
            );
            let id = attr(doc, node, "id");
            self.probe(
                method,
                &target,
                &sent,
                &format!("button #{id} of form #{form_id}"),
            );
        }
    }
    fn page(&mut self, url: &str) {
        let response = request(self.state, self.actor, "get", url, &[]);
        assert_eq!(response.status, 200, "{}: {url}", self.name);
        let html = String::from_utf8(response.body).expect("utf-8");
        validate_strict(&html).unwrap_or_else(|e| panic!("{} {url}: {e:?}", self.name));
        // The shared audit over the same page: a control with nowhere to go, an `onclick`
        // in a world with no script, a role or a tabindex on something inert, a field with
        // no name or label, a fragment that is not here — and, from the engine's own
        // cascade, an inert element with a pointer, a hover, or the painted box this
        // page's controls wear.
        if let Some(fault) = cw_service_common::audit::page(&html)
            .into_iter()
            .chain(cw_service_common::audit::clothes(&html))
            .next()
        {
            panic!("{} {url}: {fault}", self.name);
        }
        let doc = cw_web::html::parse(&html);
        self.pages += 1;
        let text = doc.body().map(|b| doc.text_content(b)).unwrap_or_default();
        for slop in [
            "Lorem ipsum",
            "TODO",
            "FIXME",
            "1 items",
            "undefined",
            "NaN",
        ] {
            assert!(!text.contains(slop), "{}: {url} says {slop:?}", self.name);
        }
        for node in doc.descendants(Document::ROOT) {
            let id = attr(&doc, node, "id");
            match doc.tag(node) {
                Some("a") => {
                    let target = attr(&doc, node, "href");
                    assert!(
                        !target.is_empty() && target != "#",
                        "{}: {url} has a link #{id} to nowhere",
                        self.name
                    );
                    if !internal(&target) {
                        continue;
                    }
                    self.probe("get", &target, &[], &format!("link #{id}"));
                    if self.kinds.insert(shape(&target)) {
                        self.queue.push(target);
                    }
                }
                Some("form") => self.submit(&doc, node),
                Some("button") => assert!(
                    doc.ancestors(node).any(|a| doc.is(a, "form"))
                        || !attr(&doc, node, "formaction").is_empty(),
                    "{}: {url} draws a button #{id} that submits nothing",
                    self.name
                ),
                _ => {}
            }
        }
    }
    fn run(&mut self) {
        while let Some(url) = self.queue.pop() {
            self.page(&url);
        }
    }
}

/// The mailbox as the seed leaves it, and the four states a reader can put it in: a search that
/// matches nothing, a labelled and starred and read message, and a mailbox with no mail at all.
fn variants(name: &str, actor: &str) -> Vec<(String, Value)> {
    let initial = site(name)["initial_state"].clone();
    let base = MailService
        .initialize(initial.clone(), &context(actor))
        .expect("seed initialises");
    let post = |state: &Value, url: &str, fields: &[(&str, &str)]| {
        let mut next = state.clone();
        let pairs: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let sent: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let mut r = HttpRequest::get(format!("{HOST}{url}"));
        r.method = "POST".into();
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        r.body = href("", &sent).trim_start_matches('?').as_bytes().to_vec();
        let status = MailService
            .handle(&mut next, &context(actor), &r)
            .unwrap()
            .status;
        assert_eq!(status, 200, "{name}: setting up {url}");
        next
    };
    // The newest message of the reader's inbox: the one the first row is drawn from.
    let mine = base["messages"]
        .as_object()
        .expect("messages")
        .iter()
        .filter(|(_, m)| m["mailboxes"].get(actor).is_some())
        .map(|(id, _)| id.clone())
        .next_back()
        .expect("the seed gives this reader mail");
    let mut empty = initial;
    empty["messages"] = serde_json::json!({});
    empty["next_id"] = serde_json::json!(0);
    vec![
        ("seeded".to_owned(), base.clone()),
        (
            "searching".to_owned(),
            post(&base, "/search", &[("q", "no such mail")]),
        ),
        (
            "labelled".to_owned(),
            post(
                &post(
                    &base,
                    &format!("/messages/{mine}"),
                    &[("label", "Crawl"), ("read", "true")],
                ),
                &format!("/messages/{mine}"),
                &[("star", "toggle")],
            ),
        ),
        (
            "empty".to_owned(),
            MailService
                .initialize(empty, &context(actor))
                .expect("an empty seed initialises"),
        ),
    ]
}

#[test]
fn every_link_form_and_button_of_every_page_of_every_skin_is_answered() {
    let mut pages = 0;
    let mut controls = 0;
    for (name, actor) in [
        ("google-mail", "alice"),
        ("outlook-mail", "bob"),
        ("mail-com", "carol"),
    ] {
        let mut kinds = BTreeSet::new();
        for (variant, state) in variants(name, actor) {
            let mut crawl = Crawl::new(&state, actor, format!("{name}/{variant}"));
            crawl.run();
            assert!(
                crawl.pages >= 6,
                "{name}/{variant}: only {} pages walked",
                crawl.pages
            );
            pages += crawl.pages;
            controls += crawl.controls;
            kinds.append(&mut crawl.kinds);
        }
        // Every kind of page this service serves was reached by following its own controls.
        for kind in [
            "/",
            "/?folder=inbox",
            "/?folder=archive",
            "/?folder=inbox&compose=1",
            "/?folder=inbox&thread=mail-N",
            "/threads/mail-N",
            "/?folder=all&label=X",
        ] {
            assert!(
                kinds.contains(kind),
                "{name}: the crawl never reached {kind}"
            );
        }
    }
    println!("{pages} pages walked, {controls} controls followed");
    assert!(
        pages >= 100 && controls >= 1500,
        "{pages} pages, {controls} controls"
    );
}

/// The named controls a reader looks for are the ones that act, in every skin: the star in a row
/// is a button of its own form, the rail's labels are links, and the toolbar's one icon refreshes.
#[test]
fn the_list_controls_post_and_navigate_rather_than_decorate() {
    for (name, actor) in [
        ("google-mail", "alice"),
        ("outlook-mail", "bob"),
        ("mail-com", "carol"),
    ] {
        let (_, labelled) = variants(name, actor).swap_remove(2);
        let label_href = "/?folder=all&label=Crawl";
        let response = request(&labelled, actor, "get", "/", &[]);
        let doc = cw_web::html::parse(&String::from_utf8(response.body).unwrap());
        let tag = doc
            .descendants(Document::ROOT)
            .find(|n| {
                attr(&doc, *n, "id").starts_with("label-") && !attr(&doc, *n, "href").is_empty()
            })
            .unwrap_or_else(|| panic!("{name}: the rail has no label link"));
        assert_eq!(doc.tag(tag), Some("a"), "{name}: a label is a link");
        assert!(
            doc.descendants(Document::ROOT)
                .any(|n| attr(&doc, n, "href") == label_href),
            "{name}: the new label is not in the rail"
        );
        let refresh = *doc
            .by_id("list-refresh")
            .first()
            .expect("a refresh control");
        assert_eq!(
            (doc.tag(refresh), attr(&doc, refresh, "href").as_str()),
            (Some("a"), "/?folder=inbox"),
            "{name}"
        );
        // The label view is reachable and holds the message that was labelled.
        let filtered = request(&labelled, actor, "get", label_href, &[]);
        assert_eq!(filtered.status, 200);
        let doc = cw_web::html::parse(&String::from_utf8(filtered.body).unwrap());
        assert!(
            doc.body()
                .map(|b| doc.text_content(b))
                .unwrap_or_default()
                .contains("Crawl"),
            "{name}"
        );
        let row = doc
            .descendants(Document::ROOT)
            .find(|n| doc.tag(*n) == Some("button") && attr(&doc, *n, "id").ends_with("-star"))
            .unwrap_or_else(|| panic!("{name}: no star button in the label view"));
        assert_eq!(attr(&doc, row, "name"), "star");
        assert_eq!(attr(&doc, row, "value"), "toggle");
        let form = doc
            .ancestors(row)
            .find(|a| doc.is(*a, "form"))
            .expect("the star's form");
        assert_eq!(attr(&doc, form, "method"), "post");
        assert!(
            attr(&doc, form, "action").starts_with("/messages/"),
            "{name}"
        );
        // Pressing it comes back to the label view it was pressed in, not to some other folder.
        let id = attr(&doc, row, "id");
        let pressed = request(
            &labelled,
            actor,
            "post",
            &attr(&doc, form, "action"),
            &[
                ("folder".into(), "all".into()),
                ("filter".into(), "Crawl".into()),
                ("thread".into(), String::new()),
                ("star".into(), "toggle".into()),
            ],
        );
        assert_eq!(pressed.status, 200);
        let after = cw_web::html::parse(&String::from_utf8(pressed.body).unwrap());
        assert_eq!(
            attr(
                &after,
                *after.by_id("list-refresh").first().unwrap(),
                "href"
            ),
            label_href,
            "{name}"
        );
        let again = *after
            .by_id(&id)
            .first()
            .unwrap_or_else(|| panic!("{name}: the row is gone"));
        assert!(
            after.has_class(again, "on") != doc.has_class(row, "on"),
            "{name}: the star flipped"
        );
    }
}

/// `mail.internal` still serves the original `Page`, and its two forms still post to routes the
/// service has. Nothing here may grow a control: this page's bytes are frozen.
#[test]
fn the_plain_mailbox_posts_only_to_routes_it_has() {
    let mut initial = site("google-mail")["initial_state"].clone();
    initial["skin"] = serde_json::json!("plain");
    let state = MailService.initialize(initial, &context("alice")).unwrap();
    let response = request(&state, "alice", "get", "/", &[]);
    assert_eq!(response.status, 200);
    let page: Value = serde_json::from_slice(&response.body).expect("the plain page is a Page");
    let mut forms = 0;
    for element in page["elements"].as_array().expect("elements") {
        if element["kind"] != "form" {
            continue;
        }
        forms += 1;
        let url = element["action"]["url"]
            .as_str()
            .expect("an action url")
            .to_owned();
        assert!(internal(&url), "the plain page posts off-site to {url}");
        let fields: Vec<(String, String)> = element["action"]["fields"]
            .as_object()
            .expect("fields")
            .keys()
            .map(|k| (k.clone(), String::new()))
            .collect();
        let status = request(&state, "alice", "post", &url, &fields).status;
        assert!(
            status != 404 && status != 405,
            "plain: {url} answered {status}"
        );
    }
    assert!(
        forms >= 2,
        "the plain page has the compose form and one per message"
    );
}
