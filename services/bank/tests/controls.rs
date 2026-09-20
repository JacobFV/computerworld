//! No dead controls on northwind.example or paypal.com. Both skins and every page kind are
//! crawled from a handful of seeds: every link, button and form is followed against the
//! service's own router, every `POST` action is probed against a scratch copy of the state,
//! and every per-page lie the auditor knows about — an anchor with nowhere to go, a dangling
//! `#fragment`, a button in no form, a field with no name or no label, an inert element drawn
//! with `cursor: pointer` or one that lights up under it — fails here rather than in front of
//! an agent. A tab or chip that leads back to the page it sits on is allowed only because it
//! says so with `aria-current="page"`, which the sweep reads off the page itself.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_bank::{BankService, BankState};
use cw_service_common::audit;
use serde_json::Value;

const NORTHWIND: &str = include_str!("../../../worlds/company-2026/sites/northwind.json");
const PAYPAL: &str = include_str!("../../../worlds/company-2026/sites/paypal.json");

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 20,
        seed: 1,
        instance: "bank".into(),
    }
}
fn load(raw: &str) -> Value {
    let doc: Value = serde_json::from_str(raw).expect("site file must be JSON");
    assert_eq!(doc["kind"], "bank");
    BankService
        .initialize(doc["initial_state"].clone(), &ctx("alice"))
        .expect("seed must load")
}
/// Crawls one shipped site as one person. A `POST` probe really moves money, so it runs
/// against a scratch copy of the state and the crawl keeps reading the untouched one.
fn sweep(raw: &str, host: &str, actor: &str, seeds: &[&str]) -> Vec<String> {
    let mut state = load(raw);
    let mut call = |method: &str, path: &str| {
        let mut r = HttpRequest::get(format!("http://{host}{path}"));
        let response = if method.eq_ignore_ascii_case("POST") {
            r.method = "POST".into();
            r.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
            let mut scratch = state.clone();
            BankService.handle(&mut scratch, &ctx(actor), &r)
        } else {
            BankService.handle(&mut state, &ctx(actor), &r)
        };
        let response = response.unwrap_or_else(|e| panic!("{method} {path}: {e}"));
        (
            response.status,
            String::from_utf8_lossy(&response.body).into_owned(),
        )
    };
    // No `allow_self` list: the pages earn that exemption themselves, by marking the tab or
    // chip that leads back to where you are with `aria-current="page"`.
    audit::Sweep::new(seeds, &mut call).limit(5000).run()
}
/// The pages one person's own money reaches: the overview, the transfer desk, and each of
/// their accounts plain, filtered, and as a statement over the whole ledger and one month.
fn seeds_for(raw: &str, actor: &str) -> Vec<String> {
    let s: BankState = serde_json::from_value(load(raw)).unwrap();
    let mut seeds = vec!["/".to_owned(), "/transfers".to_owned()];
    for a in s.accounts.values().filter(|a| a.owner == actor) {
        seeds.push(format!("/accounts/{}", a.id));
        seeds.push(format!("/accounts/{}?category=&q=nothing-matches", a.id));
        seeds.push(format!("/statements/{}/all", a.id));
        seeds.push(format!("/statements/{}/2026-03", a.id));
    }
    seeds
}
fn report(site: &str, actor: &str, faults: &[String]) -> String {
    if faults.is_empty() {
        return String::new();
    }
    format!("{site} as {actor}:\n  {}\n", faults.join("\n  "))
}

#[test]
fn northwind_has_no_dead_control_on_any_page() {
    let mut out = String::new();
    for actor in ["alice", "dave"] {
        let seeds = seeds_for(NORTHWIND, actor);
        let paths: Vec<&str> = seeds.iter().map(String::as_str).collect();
        let faults = sweep(NORTHWIND, "northwind.example", actor, &paths);
        out.push_str(&report("northwind.example", actor, &faults));
    }
    assert!(out.is_empty(), "{out}");
}

#[test]
fn paypal_has_no_dead_control_on_any_page() {
    let mut out = String::new();
    for actor in ["alice", "bob", "carol", "dave"] {
        let seeds = seeds_for(PAYPAL, actor);
        let paths: Vec<&str> = seeds.iter().map(String::as_str).collect();
        let faults = sweep(PAYPAL, "paypal.com", actor, &paths);
        out.push_str(&report("paypal.com", actor, &faults));
    }
    assert!(out.is_empty(), "{out}");
}

/// Classes these stylesheets draw as something to press: a masthead tab, an account tile,
/// a ledger row, a button, a filter chip, a round quick action, a breadcrumb. A bordered
/// pill needs neither a pointer cursor nor a hover change to read as a button, so the
/// auditor cannot catch one; anything wearing these is named here instead, and has to be a
/// control or sit inside one.
const PRESSABLE: &[&str] = &["tab", "tile", "tx", "btn", "chip", "round", "crumb"];

/// The complaints, and how many elements wore one of the classes at all — a page that wears
/// none of them proves nothing, so the test checks it found some to judge.
fn looks_pressable_but_is_not(html: &str) -> (Vec<String>, usize) {
    use cw_web::dom::Document;
    use cw_web::paint::semantics::is_interactive;
    let doc = cw_web::html::parse(html);
    let (mut out, mut seen) = (Vec::new(), 0usize);
    for node in doc.descendants(Document::ROOT) {
        if !doc.is_element(node) {
            continue;
        }
        let classes: Vec<&str> = doc.attr(node, "class").unwrap_or_default().split_whitespace().collect();
        let Some(class) = classes.iter().find(|c| PRESSABLE.contains(c)) else {
            continue;
        };
        seen += 1;
        if is_interactive(&doc, node) || doc.ancestors(node).any(|a| doc.is_element(a) && is_interactive(&doc, a)) {
            continue;
        }
        out.push(format!(
            "<{} id={:?} class={class:?}> is drawn as pressable but is not a control",
            doc.tag(node).unwrap_or("?"),
            doc.attr(node, "id").unwrap_or_default()
        ));
    }
    (out, seen)
}
fn render(raw: &str, host: &str, actor: &str, path: &str) -> String {
    let mut state = load(raw);
    let r = BankService
        .handle(&mut state, &ctx(actor), &HttpRequest::get(format!("http://{host}{path}")))
        .unwrap();
    assert_eq!(r.status, 200, "{path}");
    String::from_utf8(r.body).unwrap()
}

#[test]
fn nothing_that_is_drawn_as_pressable_is_inert() {
    let pages = [
        (NORTHWIND, "northwind.example", "alice", "chk-4417", "cc-3310"),
        (PAYPAL, "paypal.com", "alice", "pp-alice", "pp-alice"),
    ];
    let mut out = String::new();
    for (raw, host, actor, account, card) in pages {
        let s: BankState = serde_json::from_value(load(raw)).unwrap();
        let tx = s
            .activity(actor, card)
            .first()
            .map(|t| format!("/accounts/{card}/transactions/{}", t.id))
            .expect("the seed gives this actor some activity to read");
        let paths = [
            "/".to_owned(),
            "/transfers".to_owned(),
            format!("/accounts/{account}"),
            format!("/accounts/{account}?category=&q=nothing-matches"),
            format!("/statements/{account}/all"),
            format!("/statements/{account}/2026-03"),
            tx,
        ];
        for path in paths {
            let (faults, seen) = looks_pressable_but_is_not(&render(raw, host, actor, &path));
            assert!(seen >= 3, "{host}{path} wears none of the pressable classes; the check is vacuous");
            for fault in faults {
                out.push_str(&format!("{host}{path}: {fault}\n"));
            }
        }
    }
    assert!(out.is_empty(), "{out}");
}
