//! The publications the press service backs, served as HTML and driven end to end
//! through the agent API: the semantic observation lists the masthead, the story cards,
//! the forms and their buttons by the ids the service documents; a card opens its
//! article, the Save and Follow pills post and show their engaged state, a comment typed
//! into the box and submitted appears on the page, a tag chip opens its listing, and the
//! newsletter box takes an address.
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
fn has_text(all: &[Value], needle: &str) -> bool {
    all.iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains(needle)))
}

/// Front page, first card, article: the reading flow every publication shares. Returns
/// the article's URL with the browser left on it.
fn read_first_story(world: &mut World, session: &str, origin: &str, brand: &str) -> String {
    act(world, session, "browser.v1", "navigate", json!({"url": format!("{origin}/")}));
    let front = page(world, session);
    assert_eq!(front["title"], brand, "{origin}");
    let all = elements(&front);
    assert_eq!(by_id(&all, "masthead-home")["kind"], "link");
    assert_eq!(by_id(&all, "masthead-home")["url"], format!("{origin}/"));
    assert_eq!(by_id(&all, "masthead-archive")["url"], format!("{origin}/archive"));
    assert_eq!(by_id(&all, "masthead-saved")["url"], format!("{origin}/saved"));
    assert_eq!(by_id(&all, "masthead-follow")["kind"], "button");
    assert_eq!(by_id(&all, "subscribe")["kind"], "form");
    assert_eq!(by_id(&all, "subscribe-email")["kind"], "input");
    assert_eq!(by_id(&all, "subscribe-email")["label"], "Email address");
    assert_eq!(by_id(&all, "subscribe-submit")["kind"], "button");
    let card = all
        .iter()
        .find(|e| e["kind"] == "link" && e["id"].as_str().is_some_and(|id| id.starts_with("card-")))
        .unwrap_or_else(|| panic!("{origin} has no story card"))
        .clone();
    let target = card["url"].as_str().unwrap().to_owned();
    assert!(target.starts_with(origin), "{target}");
    act(world, session, "browser.v1", "click", json!({"id": card["id"]}));
    assert_eq!(browser(world, session)["url"], target.as_str());
    let story = elements(&page(world, session));
    assert_eq!(by_id(&story, "article-section")["kind"], "link");
    assert_eq!(by_id(&story, "comment")["kind"], "form");
    assert_eq!(by_id(&story, "comment-text")["label"], "Join the discussion");
    assert_eq!(by_id(&story, "article-save")["kind"], "button");
    target
}

#[test]
fn the_verge_is_read_saved_commented_on_and_followed_through_the_agent_api() {
    let (mut world, session) = world();
    let story = read_first_story(&mut world, &session, "http://theverge.com", "The Verge");
    assert_eq!(story, "http://theverge.com/2026/atlas-determinism");

    // Comment: click the box, type, press Enter; the form posts and the page shows it.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"comment-text"}));
    act(&mut world, &session, "keyboard.v1", "type", json!({"text":"Replay or it did not happen."}));
    act(&mut world, &session, "browser.v1", "key", json!({"key":"Enter"}));
    let all = elements(&page(&world, &session));
    assert!(has_text(&all, "Replay or it did not happen."), "the comment is on the page");
    assert!(has_text(&all, "7 comments"));

    // Like the first comment: a one-button form that lands back on the story.
    let before = by_id(&all, "comment-c1-like")["text"].as_str().unwrap().to_owned();
    act(&mut world, &session, "browser.v1", "click", json!({"id":"comment-c1-like"}));
    let all = elements(&page(&world, &session));
    assert_ne!(by_id(&all, "comment-c1-like")["text"].as_str().unwrap(), before);
    assert!(by_id(&all, "article-title")["text"].as_str().unwrap().contains("Atlas"));

    // Save is a toggle, and the reading list reflects it.
    let was = by_id(&all, "article-save")["text"].as_str().unwrap().to_owned();
    act(&mut world, &session, "browser.v1", "click", json!({"id":"article-save"}));
    let all = elements(&page(&world, &session));
    let now = by_id(&all, "article-save")["text"].as_str().unwrap().to_owned();
    assert_ne!(was, now);
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://theverge.com/saved"}));
    let list = elements(&page(&world, &session));
    assert_eq!(
        list.iter().any(|e| e["id"] == "card-atlas-determinism"),
        now == "Saved",
        "the reading list agrees with the pill"
    );

    // A tag chip opens its listing, where the topic can be followed.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url": story}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"article-tag-determinism"}));
    assert_eq!(browser(&world, &session)["url"], "http://theverge.com/tag/determinism");
    let tagged = elements(&page(&world, &session));
    assert!(tagged.iter().any(|e| e["id"] == "card-atlas-determinism" && e["kind"] == "link"));
    let was = by_id(&tagged, "list-follow")["text"].as_str().unwrap().to_owned();
    act(&mut world, &session, "browser.v1", "click", json!({"id":"list-follow"}));
    let tagged = elements(&page(&world, &session));
    assert_ne!(by_id(&tagged, "list-follow")["text"].as_str().unwrap(), was);

    // The newsletter box: fill and submit.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://theverge.com/"}));
    act(&mut world, &session, "browser.v1", "fill", json!({"id":"subscribe-email","value":"alice.chen@gmail.com"}));
    act(&mut world, &session, "browser.v1", "click", json!({"id":"subscribe-submit"}));
    let front = elements(&page(&world, &session));
    assert!(front.iter().any(|e| e["id"] == "front-grid" || e["id"] == "masthead-home"));
    // Follow the publication from the masthead.
    let was = by_id(&front, "masthead-follow")["text"].as_str().unwrap().to_owned();
    act(&mut world, &session, "browser.v1", "click", json!({"id":"masthead-follow"}));
    let front = elements(&page(&world, &session));
    assert_ne!(by_id(&front, "masthead-follow")["text"].as_str().unwrap(), was);
}

#[test]
fn every_publication_opens_its_first_story_and_its_sections_through_the_agent_api() {
    let (mut world, session) = world();
    for (origin, brand, section) in [
        ("http://nytimes.com", "The New York Times", "world"),
        ("http://bbc.com", "BBC News", "uk"),
        ("http://cnn.com", "CNN", "us"),
        ("http://reuters.com", "Reuters", "technology"),
        ("http://theverge.com", "The Verge", "tech"),
        ("http://arstechnica.com", "Ars Technica", "it"),
        ("http://news.google.com", "Google News", "top"),
        ("http://medium.com", "Medium", "programming"),
        ("http://substack.com", "Substack", "the-ledger"),
        ("http://alicechen.dev", "Alice Chen", "engineering"),
        ("http://bmartinez.net", "Bob Martinez", "debugging"),
        ("http://eng.northstar.example", "Northstar Engineering", "engineering"),
    ] {
        let story = read_first_story(&mut world, &session, origin, brand);
        // The kicker goes to the story's section; the masthead goes to any of them.
        act(&mut world, &session, "browser.v1", "click", json!({"id":"article-section"}));
        let listing = elements(&page(&world, &session));
        assert!(listing.iter().any(|e| e["kind"] == "link" && e["url"] == story.as_str()), "{origin}: the section lists the story");
        act(&mut world, &session, "browser.v1", "click", json!({"id": format!("masthead-{section}")}));
        assert_eq!(browser(&world, &session)["url"], format!("{origin}/{section}"));
        act(&mut world, &session, "browser.v1", "click", json!({"id":"masthead-home"}));
        assert_eq!(browser(&world, &session)["url"], format!("{origin}/"));
    }
}
