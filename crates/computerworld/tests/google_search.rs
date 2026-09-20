//! google.com served as HTML by the search service and driven end to end through the
//! agent API: the browser renders it with the web engine, the semantic observation
//! lists the box, the buttons and the links by the ids the service documents, typing
//! a query and pressing Enter lands on the results page, and the first result is a
//! link that navigates to the indexed site.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";

fn world() -> (World, String) {
    let mut world = World::new(reference_world(), 42).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: ACTOR.into(),
            machines: vec![MACHINE.into()],
            actions: vec!["browser.v1".into(), "keyboard.v1".into()],
            observations: vec!["semantic.v1".into(), "browser.v1".into()],
            action_budget: 1 << 20,
        })
        .unwrap();
    (world, session)
}
fn act(world: &mut World, session: &str, channel: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(session, vec![ActionEnvelope::new(channel, op, MACHINE, payload)])
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn browser(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE].clone()
}
/// Every element of the semantic tree, flattened.
fn elements(page: &Value) -> Vec<Value> {
    fn walk(elements: &[Value], out: &mut Vec<Value>) {
        for e in elements {
            out.push(e.clone());
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    out
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no element {id} in {all:?}"))
}

#[test]
fn google_is_searched_and_the_first_result_is_followed_through_the_agent_api() {
    let (mut world, session) = world();
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://google.com/"}));
    let home = page(&world, &session);
    assert_eq!(home["title"], "Google");
    let all = elements(&home);
    // The semantic tree lists the box, the buttons and the links by their ids.
    let q = by_id(&all, "q");
    assert_eq!(q["kind"], "input");
    assert_eq!(q["label"], "Search Google");
    assert_eq!(by_id(&all, "search-go")["kind"], "button");
    assert_eq!(by_id(&all, "search-go")["text"], "Google Search");
    assert_eq!(by_id(&all, "search-lucky")["text"], "I'm Feeling Lucky");
    let about = by_id(&all, "foot-about");
    assert_eq!(about["kind"], "link");
    assert_eq!(about["url"], "http://google.com/about");
    assert_eq!(by_id(&all, "head-0")["url"], "http://gmail.com/");
    assert_eq!(by_id(&all, "search")["kind"], "form");
    assert!(all.iter().any(|e| e["kind"] == "link" && e["id"] == "recent-0"));

    // Type a query into the focused box and press Enter: the form is a GET, so the
    // results URL is the query.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"q"}));
    act(&mut world, &session, "keyboard.v1", "type", json!({"text":"atlas"}));
    act(&mut world, &session, "browser.v1", "key", json!({"key":"Enter"}));
    let shown = browser(&world, &session);
    assert_eq!(shown["url"], "http://google.com/search?q=atlas");
    let results = page(&world, &session);
    assert_eq!(results["title"], "atlas - Google");
    let all = elements(&results);
    assert_eq!(by_id(&all, "q")["value"], "atlas");
    assert!(all.iter().any(|e| e["id"] == "tab-images" && e["kind"] == "link"));
    assert!(all.iter().any(|e| e["id"] == "stats" || e["text"].as_str().is_some_and(|t| t.starts_with("About "))));
    let first = by_id(&all, "hit-0");
    assert_eq!(first["kind"], "link");
    let target = first["url"].as_str().unwrap().to_owned();
    assert!(target.starts_with("http://"), "{target}");
    assert!(first["text"].as_str().unwrap().to_lowercase().contains("atlas"), "{first:?}");

    // The first result is a real link into the world: clicking it leaves google.com.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"hit-0"}));
    let landed = browser(&world, &session);
    assert_eq!(landed["url"], target);
    assert_ne!(page(&world, &session)["title"], "atlas - Google");
    act(&mut world, &session, "browser.v1", "back", json!({}));
    assert_eq!(browser(&world, &session)["url"], "http://google.com/search?q=atlas");

    // The Lucky button submits the same box to /lucky.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://google.com/"}));
    act(&mut world, &session, "browser.v1", "fill", json!({"id":"q","value":"atlas"}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"search-lucky"}));
    assert_eq!(browser(&world, &session)["url"], "http://google.com/lucky?q=atlas");
    let lucky = elements(&page(&world, &session));
    assert_eq!(by_id(&lucky, "lucky-go")["kind"], "link");
}
