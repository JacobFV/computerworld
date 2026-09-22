//! wikipedia.org, imdb.com and archive.org served as HTML by the wiki service and driven
//! through the agent API: the browser renders each skin with the web engine, the semantic
//! observation lists the search box, the tabs, the cards and the forms by the ids the
//! service documents, a search is typed and submitted, a section is edited and the edit
//! shows in History, and a topic is added on Talk.
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
        .step(
            session,
            vec![ActionEnvelope::new(channel, op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn browse(world: &mut World, session: &str, op: &str, payload: Value) {
    act(world, session, "browser.v1", op, payload);
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"]
        .as_str()
        .unwrap()
        .to_owned()
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
fn says(all: &[Value], needle: &str) -> bool {
    all.iter().any(|e| {
        ["text", "value", "label"]
            .iter()
            .any(|k| e[*k].as_str().is_some_and(|t| t.contains(needle)))
    })
}

#[test]
fn wikipedia_is_searched_read_edited_and_discussed_through_the_agent_api() {
    let (mut world, session) = world();
    browse(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://wikipedia.org/"}),
    );
    let home = page(&world, &session);
    assert_eq!(home["title"], "Wikipedia — The free encyclopedia");
    let all = elements(&home);
    assert_eq!(by_id(&all, "search")["kind"], "form");
    assert_eq!(by_id(&all, "search-q")["kind"], "input");
    assert_eq!(by_id(&all, "search-q")["label"], "Search Wikipedia");
    assert_eq!(by_id(&all, "search-submit")["kind"], "button");
    let featured = by_id(&all, "featured-title");
    assert_eq!(featured["kind"], "link");
    assert_eq!(
        featured["url"],
        "http://wikipedia.org/wiki/Deterministic_simulation"
    );
    assert_eq!(by_id(&all, "all-0")["kind"], "link");
    assert_eq!(
        by_id(&all, "portal-random")["url"],
        "http://wikipedia.org/wiki/Special:Random"
    );
    assert_eq!(
        by_id(&all, "news-0")["url"],
        "http://theverge.com/2026/atlas-determinism"
    );

    // Type into the box and press Enter: the form posts `q` to /search.
    browse(&mut world, &session, "click", json!({"id":"search-q"}));
    act(
        &mut world,
        &session,
        "keyboard.v1",
        "type",
        json!({"text":"simulation"}),
    );
    browse(&mut world, &session, "key", json!({"key":"Enter"}));
    assert_eq!(url(&world, &session), "http://wikipedia.org/search");
    let results = page(&world, &session);
    assert_eq!(results["title"], "simulation — search results");
    let all = elements(&results);
    let first = by_id(&all, "hit-0");
    assert_eq!(first["kind"], "link");
    let target = first["url"].as_str().unwrap().to_owned();
    assert!(target.starts_with("http://wikipedia.org/wiki/"), "{target}");

    // The first hit is the article; its tabs, contents and edit links are all links.
    browse(&mut world, &session, "click", json!({"id":"hit-0"}));
    assert_eq!(url(&world, &session), target);
    browse(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://wikipedia.org/wiki/Deterministic_simulation"}),
    );
    let article = page(&world, &session);
    assert_eq!(article["title"], "Deterministic simulation — Wikipedia");
    let all = elements(&article);
    assert_eq!(
        by_id(&all, "tab-1")["url"],
        "http://wikipedia.org/wiki/Talk:Deterministic_simulation"
    );
    assert_eq!(
        by_id(&all, "tab-2")["url"],
        "http://wikipedia.org/wiki/Special:History/Deterministic_simulation"
    );
    assert_eq!(by_id(&all, "toc-1")["kind"], "link");
    assert_eq!(by_id(&all, "ref-0")["kind"], "link");
    assert_eq!(by_id(&all, "see-0")["kind"], "link");
    assert_eq!(
        by_id(&all, "sec-1-edit")["url"],
        "http://wikipedia.org/wiki/Deterministic_simulation?section=history"
    );

    // Edit the History section: fill the textarea and the summary, publish.
    browse(&mut world, &session, "click", json!({"id":"sec-1-edit"}));
    let all = elements(&page(&world, &session));
    assert_eq!(by_id(&all, "edit")["kind"], "form");
    assert_eq!(by_id(&all, "edit-comment")["kind"], "input");
    assert_eq!(by_id(&all, "edit-submit")["kind"], "button");
    browse(
        &mut world,
        &session,
        "fill",
        json!({"id":"edit-body","value":"Rewritten through the browser."}),
    );
    browse(
        &mut world,
        &session,
        "fill",
        json!({"id":"edit-comment","value":"agent copyedit"}),
    );
    browse(&mut world, &session, "click", json!({"id":"edit-submit"}));
    assert_eq!(
        url(&world, &session),
        "http://wikipedia.org/articles/Deterministic_simulation/sections/history"
    );
    let all = elements(&page(&world, &session));
    assert!(says(&all, "Rewritten through the browser."), "{all:?}");

    // History shows the edit, signed with alice's wiki username and marked current.
    browse(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://wikipedia.org/wiki/Special:History/Deterministic_simulation"}),
    );
    let all = elements(&page(&world, &session));
    let edit = all
        .iter()
        .find(|e| e["kind"] == "link" && e["text"] == "agent copyedit")
        .unwrap_or_else(|| panic!("the edit is not in History: {all:?}"));
    assert_eq!(
        edit["url"],
        "http://wikipedia.org/wiki/Deterministic_simulation?section=history"
    );
    assert!(says(&all, "alicechen"));

    // Talk: add a topic.
    browse(&mut world, &session, "click", json!({"id":"tab-1"}));
    assert_eq!(
        url(&world, &session),
        "http://wikipedia.org/wiki/Talk:Deterministic_simulation"
    );
    browse(
        &mut world,
        &session,
        "fill",
        json!({"id":"reply-text","value":"Sources for the rewrite, please."}),
    );
    browse(&mut world, &session, "click", json!({"id":"reply-submit"}));
    let all = elements(&page(&world, &session));
    assert!(says(&all, "Sources for the rewrite, please."), "{all:?}");
}

#[test]
fn imdb_goes_from_the_featured_title_to_its_cast_and_the_chart() {
    let (mut world, session) = world();
    browse(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://imdb.com/"}),
    );
    let home = page(&world, &session);
    let all = elements(&home);
    assert_eq!(by_id(&all, "search-q")["label"], "Search IMDb");
    assert_eq!(by_id(&all, "masthead-logo")["url"], "http://imdb.com/");
    assert_eq!(
        by_id(&all, "nav-top")["url"],
        "http://imdb.com/wiki/Top_rated"
    );
    assert_eq!(
        by_id(&all, "featured-title")["url"],
        "http://imdb.com/wiki/Northbound_Signal"
    );

    browse(
        &mut world,
        &session,
        "click",
        json!({"id":"featured-title"}),
    );
    assert_eq!(
        url(&world, &session),
        "http://imdb.com/wiki/Northbound_Signal"
    );
    let title = page(&world, &session);
    assert_eq!(title["title"], "Northbound Signal — IMDb");
    let all = elements(&title);
    assert!(says(&all, "8.1"), "the rating is on the page");
    assert_eq!(
        by_id(&all, "cast-0")["url"],
        "http://imdb.com/wiki/Tobias_Renard"
    );
    assert_eq!(
        by_id(&all, "info-1-value-link-0")["url"],
        "http://imdb.com/wiki/Ilse_Marchetti"
    );

    browse(&mut world, &session, "click", json!({"id":"cast-0"}));
    assert_eq!(url(&world, &session), "http://imdb.com/wiki/Tobias_Renard");
    assert_eq!(page(&world, &session)["title"], "Tobias Renard — IMDb");
    browse(&mut world, &session, "click", json!({"id":"nav-top"}));
    assert_eq!(page(&world, &session)["title"], "Top rated — IMDb");

    // The search box finds a title by a word of its name.
    browse(
        &mut world,
        &session,
        "fill",
        json!({"id":"search-q","value":"clockwork"}),
    );
    browse(&mut world, &session, "click", json!({"id":"search-submit"}));
    let all = elements(&page(&world, &session));
    assert_eq!(
        by_id(&all, "hit-0")["url"],
        "http://imdb.com/wiki/Saltwater_Clockwork"
    );
}

#[test]
fn the_archive_goes_from_a_collection_to_an_item_and_searches() {
    let (mut world, session) = world();
    browse(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://archive.org/"}),
    );
    let home = page(&world, &session);
    let all = elements(&home);
    assert_eq!(by_id(&all, "search-q")["label"], "Search Internet Archive");
    assert_eq!(
        by_id(&all, "featured-title")["url"],
        "http://archive.org/wiki/Wayback_Machine"
    );
    let software = all
        .iter()
        .find(|e| {
            e["kind"] == "link"
                && e["text"] == "Software Library"
                && e["id"].as_str().is_some_and(|i| i.starts_with("nav-col-"))
        })
        .unwrap_or_else(|| panic!("no Software Library in the top navigation: {all:?}"))["id"]
        .as_str()
        .unwrap()
        .to_owned();

    browse(&mut world, &session, "click", json!({"id": software}));
    assert_eq!(
        url(&world, &session),
        "http://archive.org/wiki/Software_Library"
    );
    let all = elements(&page(&world, &session));
    assert_eq!(
        by_id(&all, "see-0")["url"],
        "http://archive.org/wiki/Cavern_Runner_98"
    );
    browse(&mut world, &session, "click", json!({"id":"see-0"}));
    let item = page(&world, &session);
    assert_eq!(item["title"], "Cavern Runner 98 — Internet Archive");
    let all = elements(&item);
    assert!(says(&all, "Lodestone Software"));
    assert_eq!(
        by_id(&all, "info-0-value-link-0")["url"],
        "http://archive.org/wiki/Software_Library"
    );

    browse(&mut world, &session, "click", json!({"id":"search-q"}));
    act(
        &mut world,
        &session,
        "keyboard.v1",
        "type",
        json!({"text":"transistor"}),
    );
    browse(&mut world, &session, "key", json!({"key":"Enter"}));
    let all = elements(&page(&world, &session));
    assert_eq!(
        by_id(&all, "hit-0")["url"],
        "http://archive.org/wiki/Our_Friend_the_Transistor"
    );
}
