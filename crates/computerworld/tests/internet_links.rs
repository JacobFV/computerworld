//! The simulated web has to hang together: every declared domain answers, every link a
//! page paints leads somewhere real, and every search result resolves. A dead link in a
//! world an agent is being trained on is a silently wrong lesson.
//!
//! Sites serve one of two things: the `Page` JSON the browser converts, or the HTML the
//! migrated services emit. The crawl reads both. It has to: a body that is not a `Page`
//! used to be skipped in silence, so as services moved to HTML they quietly fell out of
//! this test, and a link one of them painted to a route that no longer existed had
//! nothing checking it.
use computerworld::{reference_world, World};
use cw_protocol::{
    ActionEnvelope, EnvironmentConfig, HttpRequest, HttpResponse, Page, PageElement,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Machine every request is made from, and the actor making them.
const FROM: &str = "alice-mac";
const ACTOR: &str = "alice";

fn world() -> (World, String) {
    let mut world = World::new(reference_world(), 42).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: ACTOR.into(),
            machines: vec![FROM.into()],
            actions: vec!["http.v1".into(), "browser.v1".into()],
            observations: vec!["semantic.v1".into()],
            action_budget: 1 << 20,
        })
        .unwrap();
    (world, session)
}
fn get(world: &mut World, session: &str, url: &str) -> Option<HttpResponse> {
    let request = HttpRequest::json("GET", url, &json!({})).ok()?;
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new(
                "http.v1",
                "request",
                FROM,
                serde_json::to_value(request).ok()?,
            )],
        )
        .ok()?;
    if !result.outcomes[0].success {
        return None;
    }
    serde_json::from_value(result.outcomes[0].value.clone()).ok()
}
/// Absolute URLs a page navigates to: links, and the GET actions on buttons, forms,
/// cards and thumbnails. The layout containers hold children, so this recurses into them.
fn outbound(page: &Page) -> Vec<String> {
    fn walk(elements: &[PageElement], out: &mut Vec<String>) {
        for element in elements {
            match element {
                PageElement::Link { url, .. } => out.push(url.clone()),
                PageElement::Button { action, .. } | PageElement::Form { action, .. } => {
                    if action.method.eq_ignore_ascii_case("GET") {
                        out.push(action.url.clone());
                    }
                }
                PageElement::Card {
                    action: Some(action),
                    ..
                }
                | PageElement::Thumbnail {
                    action: Some(action),
                    ..
                } if action.method.eq_ignore_ascii_case("GET") => out.push(action.url.clone()),
                _ => {}
            }
            if let Some(children) = children_of(element) {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&page.elements, &mut out);
    out.retain(|url| url.starts_with("http://") || url.starts_with("https://"));
    out
}
/// The same, for a page served as HTML: every `href`, every GET `action`, every GET
/// `formaction`, resolved against the URL it was found on. A `POST` is left alone —
/// following one would fire the world's write routes — and so is a `#fragment`, which
/// never leaves the page.
fn outbound_html(base: &str, body: &str) -> Vec<String> {
    use cw_web::dom::Document as Dom;
    let Ok(base) = url::Url::parse(base) else {
        return Vec::new();
    };
    let doc = cw_web::html::parse(body);
    let mut out = Vec::new();
    for node in doc.descendants(Dom::ROOT) {
        if !doc.is_element(node) {
            continue;
        }
        let Some(target) = doc
            .attr(node, "href")
            .or_else(|| doc.attr(node, "action"))
            .or_else(|| doc.attr(node, "formaction"))
        else {
            continue;
        };
        if target.trim().is_empty() || target.starts_with('#') {
            continue;
        }
        // What method would this send? A form's own `method`; a submit button's
        // `formmethod`, and failing that the method of the form it sits in — a button
        // that posts somewhere else still posts, and following it would write.
        if doc.is(node, "form") || doc.is(node, "button") || doc.is(node, "input") {
            let method = if doc.is(node, "form") {
                doc.attr(node, "method").unwrap_or("get").to_owned()
            } else {
                match doc.attr(node, "formmethod") {
                    Some(method) => method.to_owned(),
                    None => doc
                        .ancestors(node)
                        .find(|a| doc.is(*a, "form"))
                        .and_then(|form| doc.attr(form, "method"))
                        .unwrap_or("get")
                        .to_owned(),
                }
            };
            if !method.eq_ignore_ascii_case("get") {
                continue;
            }
        }
        if let Ok(link) = base.join(target) {
            let link = link.to_string();
            if link.starts_with("http://") || link.starts_with("https://") {
                out.push(link);
            }
        }
    }
    out
}
/// Whether a response body is a page the engine would render as HTML.
fn is_html(body: &str) -> bool {
    let head = body.trim_start().to_ascii_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html")
}
fn children_of(element: &PageElement) -> Option<&Vec<PageElement>> {
    match element {
        PageElement::Group { children, .. }
        | PageElement::Form { children, .. }
        | PageElement::Row { children, .. }
        | PageElement::Grid { children, .. }
        | PageElement::Card { children, .. } => Some(children),
        _ => None,
    }
}
/// A page the browser would accept. `validate` is what the browser runs before it will
/// show anything, so a page that only parses is still a site an actor cannot open.
fn page_of(response: &HttpResponse) -> Option<Page> {
    let page: Page = serde_json::from_slice(&response.body).ok()?;
    page.validate().ok()?;
    Some(page)
}
/// The same, but reporting why a page was rejected.
fn checked_page(response: &HttpResponse) -> Result<Option<Page>, String> {
    let Ok(page) = serde_json::from_slice::<Page>(&response.body) else {
        return Ok(None);
    };
    page.validate().map_err(|e| e.message)?;
    Ok(Some(page))
}
/// Every domain the world declares, with the service that answers it.
fn domains(world: &World) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for service in &world.definition().services {
        for domain in &service.domains {
            out.insert(domain.clone(), service.id.clone());
        }
    }
    out
}

#[test]
fn every_declared_domain_answers_from_a_real_machine() {
    let (mut world, session) = world();
    let mut dead = Vec::new();
    for (domain, service) in domains(&world) {
        let url = format!("http://{domain}/");
        match get(&mut world, &session, &url) {
            // A service may legitimately refuse this actor, but it must answer.
            Some(response) if response.status < 500 => {}
            Some(response) => dead.push(format!("{url} ({service}) -> {}", response.status)),
            None => dead.push(format!("{url} ({service}) -> no response")),
        }
        // And over https, which every site serves unless its definition opts out.
        let secure = url.replacen("http://", "https://", 1);
        match get(&mut world, &session, &secure) {
            Some(response) if response.status < 500 => {}
            Some(response) => dead.push(format!("{secure} ({service}) -> {}", response.status)),
            None => dead.push(format!("{secure} ({service}) -> no response")),
        }
    }
    assert!(dead.is_empty(), "domains that do not answer:\n{dead:#?}");
}

/// Element ids of a page, in order: the structure an alias must share with its
/// canonical name, ignoring text that legitimately moves with simulation time.
fn shape(page: &Page) -> (String, Vec<String>) {
    fn walk(elements: &[PageElement], out: &mut Vec<String>) {
        for element in elements {
            out.push(element.id().to_owned());
            if let Some(children) = children_of(element) {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&page.elements, &mut out);
    // Sorted, because a feed ranked by age legitimately reorders between two requests.
    out.sort();
    (page.title.clone(), out)
}

#[test]
fn the_alternate_domains_reach_the_same_site_as_the_canonical_one() {
    let (mut world, session) = world();
    // Alternates are the whole point of the CNAMEs: twitter.com must be x.com. The
    // comparison is on structure, not bytes, because resolving an alias really costs an
    // extra hop, so a page that prints an age renders it a few ticks later.
    let mut by_service: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (domain, service) in domains(&world) {
        by_service.entry(service).or_default().push(domain);
    }
    for (service, domains) in by_service {
        if domains.len() < 2 {
            continue;
        }
        let checkpoint = world.snapshot();
        let shapes: Vec<_> = domains
            .iter()
            .map(|d| {
                world.restore(&checkpoint).unwrap();
                let response = get(&mut world, &session, &format!("http://{d}/"))
                    .unwrap_or_else(|| panic!("{d} did not answer"));
                (response.status, page_of(&response).map(|p| shape(&p)))
            })
            .collect();
        world.restore(&checkpoint).unwrap();
        assert!(
            shapes.windows(2).all(|w| w[0] == w[1]),
            "{service}: alternate domains {domains:?} do not reach the same site"
        );
    }
}

#[test]
fn every_link_a_page_paints_leads_somewhere_real() {
    let (mut world, session) = world();
    // Both schemes: an address bar defaults to https, a typed link is often http.
    let roots: Vec<String> = domains(&world)
        .keys()
        .flat_map(|d| [format!("http://{d}/"), format!("https://{d}/")])
        .collect();
    let mut queue: VecDeque<(String, usize)> = roots.iter().cloned().map(|u| (u, 0)).collect();
    let mut seen: BTreeSet<String> = roots.iter().cloned().collect();
    let mut broken = Vec::new();
    let mut checked = 0;
    while let Some((url, depth)) = queue.pop_front() {
        let Some(response) = get(&mut world, &session, &url) else {
            broken.push(format!("{url} -> unroutable"));
            continue;
        };
        checked += 1;
        if response.status >= 400 {
            broken.push(format!("{url} -> {}", response.status));
            continue;
        }
        if depth >= 2 {
            continue;
        }
        let body = String::from_utf8_lossy(&response.body).into_owned();
        let links = if is_html(&body) {
            outbound_html(&url, &body)
        } else {
            match checked_page(&response) {
                Ok(Some(page)) => outbound(&page),
                Ok(None) => continue,
                Err(why) => {
                    broken.push(format!("{url} -> serves a page the browser rejects: {why}"));
                    continue;
                }
            }
        };
        for link in links {
            if seen.insert(link.clone()) {
                queue.push_back((link, depth + 1));
            }
        }
    }
    assert!(checked > 0, "no pages were reachable at all");
    assert!(
        broken.is_empty(),
        "{} broken links:\n{broken:#?}",
        broken.len()
    );
}

#[test]
fn every_search_result_resolves_to_the_page_it_promises() {
    let (mut world, session) = world();
    // Each engine indexes the sites' own `search_entries` at boot, so a stale entry is a
    // real risk: a result that 404s teaches an agent the wrong thing about the web.
    let engines: Vec<String> = world
        .definition()
        .services
        .iter()
        .filter(|s| s.kind == "search")
        .map(|s| s.id.clone())
        .collect();
    assert!(!engines.is_empty(), "no search engine is installed");
    let mut broken = Vec::new();
    let mut checked = 0;
    for engine in &engines {
        let documents = world.runtime().service_state(engine).unwrap()["documents"].clone();
        let entries: Vec<&Value> = match &documents {
            Value::Array(a) => a.iter().collect(),
            Value::Object(o) => o.values().collect(),
            _ => vec![],
        };
        for entry in entries {
            let Some(url) = entry.get("url").and_then(Value::as_str) else {
                broken.push(format!("{engine}: indexed document with no url"));
                continue;
            };
            match get(&mut world, &session, url) {
                Some(r) if r.status < 400 => checked += 1,
                Some(r) => broken.push(format!("{engine}: {url} -> {}", r.status)),
                None => broken.push(format!("{engine}: {url} -> unroutable")),
            }
        }
    }
    assert!(checked > 0, "the search index is empty");
    assert!(
        broken.is_empty(),
        "{} search results do not resolve:\n{broken:#?}",
        broken.len()
    );
}

#[test]
fn the_world_still_reaches_its_own_intranet() {
    // The company's internal services are kept, not replaced; existing tasks depend on them.
    let (mut world, session) = world();
    for url in [
        "http://intranet.internal/",
        "http://mail.internal/",
        "http://docs.internal/",
        "http://chat.internal/",
        "http://messages.internal/",
        "http://calendar.internal/",
        "http://git.internal/",
        "http://issues.internal/",
        "http://guide.example/",
    ] {
        let response = get(&mut world, &session, url).unwrap_or_else(|| panic!("{url} unroutable"));
        assert!(response.status < 400, "{url} -> {}", response.status);
    }
}
