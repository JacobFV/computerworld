//! The six discussion sites served as HTML by the forum service and driven end to end through
//! the agent API: the browser renders each with the web engine, the semantic observation lists
//! the links, vote buttons, forms and inputs by the ids the service documents, and the main
//! flow of each site (open a thread, vote, reply, search, join) works by id.
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
fn go(world: &mut World, session: &str, url: &str) -> Vec<Value> {
    act(world, session, "browser.v1", "navigate", json!({ "url": url }));
    elements(&page(world, session))
}
fn click(world: &mut World, session: &str, id: &str) -> Vec<Value> {
    act(world, session, "browser.v1", "click", json!({ "id": id }));
    elements(&page(world, session))
}
fn fill(world: &mut World, session: &str, id: &str, value: &str) {
    act(world, session, "browser.v1", "fill", json!({ "id": id, "value": value }));
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"].as_str().unwrap().to_owned()
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
    all.iter().find(|e| e["id"] == id).unwrap_or_else(|| {
        let ids: Vec<&str> = all.iter().filter_map(|e| e["id"].as_str()).collect();
        panic!("no element {id} among {ids:?}")
    })
}
fn has(all: &[Value], id: &str) -> bool {
    all.iter().any(|e| e["id"] == id)
}
/// Whether any element's text or label carries `needle`.
fn says(all: &[Value], needle: &str) -> bool {
    all.iter().any(|e| ["text", "label", "value"].iter().any(|k| e[*k].as_str().is_some_and(|t| t.contains(needle))))
}

#[test]
fn hacker_news_is_read_voted_on_and_commented_on() {
    let (mut world, session) = world();
    let home = go(&mut world, &session, "http://news.ycombinator.com/");
    assert_eq!(page(&world, &session)["title"], "Hacker News / Top stories");
    assert_eq!(by_id(&home, "nav-submit")["kind"], "link");
    assert_eq!(by_id(&home, "nav-new")["url"], "http://news.ycombinator.com/newest");
    assert_eq!(by_id(&home, "nav-me")["url"], "http://news.ycombinator.com/u/achen");
    assert_eq!(by_id(&home, "t-9001-out")["url"], "http://theverge.com/2026/atlas-determinism");
    assert_eq!(by_id(&home, "t-9001-up")["kind"], "button");
    assert_eq!(by_id(&home, "t-9001-up")["text"], "Up");
    assert!(!has(&home, "t-9001-down"), "a link feed has no downvote");
    assert_eq!(by_id(&home, "search")["kind"], "form");
    assert_eq!(by_id(&home, "search-q")["kind"], "input");
    assert!(says(&home, "147 points by"), "the subtext line is on the page");

    // The arrow is a one-button form: the vote lands and the same list comes back.
    let voted = click(&mut world, &session, "t-9001-up");
    assert!(says(&voted, "148 points by"), "the vote moved the score");
    assert!(has(&voted, "t-9002-up"), "we are still on the front page");

    // The comments link opens the item; the comment box posts and lands back on it.
    let comments = by_id(&voted, "t-9001-comments")["url"].as_str().unwrap().to_owned();
    assert_eq!(comments, "http://news.ycombinator.com/item?id=t-9001");
    let item = click(&mut world, &session, "t-9001-comments");
    assert_eq!(url(&world, &session), comments);
    assert_eq!(by_id(&item, "compose")["kind"], "form");
    assert_eq!(by_id(&item, "compose-body")["kind"], "input");
    fill(&mut world, &session, "compose-body", "Replayed it on three machines, byte for byte.");
    let after = click(&mut world, &session, "compose-submit");
    assert!(says(&after, "Replayed it on three machines, byte for byte."));
    assert!(says(&after, "7 comments"));
}

#[test]
fn reddit_joins_a_community_and_replies_inside_a_thread() {
    let (mut world, session) = world();
    let home = go(&mut world, &session, "http://reddit.com/");
    assert_eq!(page(&world, &session)["title"], "reddit / Popular posts");
    assert_eq!(by_id(&home, "nav-boards")["url"], "http://reddit.com/r");
    assert_eq!(by_id(&home, "t-5120-open")["url"], "http://reddit.com/r/programming/comments/t-5120");
    assert_eq!(by_id(&home, "t-5120-up")["kind"], "button");
    assert_eq!(by_id(&home, "t-5120-down")["kind"], "button");
    let join = by_id(&home, "board-sysadmin-sub")["text"].as_str().unwrap().to_owned();
    let toggled = click(&mut world, &session, "board-sysadmin-sub");
    assert_ne!(by_id(&toggled, "board-sysadmin-sub")["text"].as_str().unwrap(), join, "Join toggled");

    let board = go(&mut world, &session, "http://reddit.com/r/programming");
    assert_eq!(by_id(&board, "board-head-sub")["kind"], "button");
    let thread = click(&mut world, &session, "t-5120-open");
    assert_eq!(url(&world, &session), "http://reddit.com/r/programming/comments/t-5120");
    assert_eq!(by_id(&thread, "r-6020-reply")["kind"], "form");
    fill(&mut world, &session, "r-6020-reply-body", "Nested from the agent API.");
    let after = click(&mut world, &session, "r-6020-reply-submit");
    assert!(says(&after, "Nested from the agent API."));
    let down = click(&mut world, &session, "t-5120-down");
    assert!(says(&down, "811"), "the downvote moved 812 to 811");
}

#[test]
fn stack_overflow_is_searched_and_an_answer_is_upvoted_and_commented_on() {
    let (mut world, session) = world();
    let home = go(&mut world, &session, "http://stackoverflow.com/");
    assert_eq!(page(&world, &session)["title"], "Stack Overflow / Top Questions");
    assert_eq!(by_id(&home, "nav-submit")["url"], "http://stackoverflow.com/submit");
    assert_eq!(by_id(&home, "search-q")["label"], "Search");
    // Type into the header box and press Enter: the search is a POST that reads.
    act(&mut world, &session, "browser.v1", "click", json!({"id":"search-q"}));
    act(&mut world, &session, "keyboard.v1", "type", json!({"text":"hashmap"}));
    act(&mut world, &session, "browser.v1", "key", json!({"key":"Enter"}));
    let results = elements(&page(&world, &session));
    assert!(says(&results, "results for \"hashmap\""));
    assert_eq!(by_id(&results, "row-t-4411")["kind"], "link");
    let question = click(&mut world, &session, "row-t-4411");
    assert_eq!(url(&world, &session), "http://stackoverflow.com/questions/t-4411");
    assert!(says(&question, "Accepted"));
    assert!(!has(&question, "r-9003-accept"), "only the asker sees the accept control");
    assert_eq!(by_id(&question, "thread-tag-0")["kind"], "link");
    let voted = click(&mut world, &session, "r-9003-up");
    assert!(has(&voted, "r-9003-down"));
    fill(&mut world, &session, "r-9002-comment-body", "Confirmed on the Windows runner.");
    let after = click(&mut world, &session, "r-9002-comment-submit");
    assert!(says(&after, "Confirmed on the Windows runner."));
    // Asking: the form carries title, tags and body and lands on the new question.
    let ask = go(&mut world, &session, "http://stackoverflow.com/submit");
    assert_eq!(by_id(&ask, "submit")["kind"], "form");
    fill(&mut world, &session, "submit-title", "How do I pin a hasher seed in tests?");
    fill(&mut world, &session, "submit-tags", "rust, testing");
    fill(&mut world, &session, "submit-body", "I want the same iteration order on every run.");
    let asked = click(&mut world, &session, "submit-submit");
    assert!(says(&asked, "How do I pin a hasher seed in tests?"));
    assert!(has(&asked, "compose"), "we are on the new question's page");
}

#[test]
fn quora_yelp_and_craigslist_open_a_thread_and_post_into_it() {
    let (mut world, session) = world();
    let home = go(&mut world, &session, "http://quora.com/");
    assert_eq!(by_id(&home, "row-t-3000")["url"], "http://quora.com/questions/t-3000");
    assert_eq!(by_id(&home, "nav-submit")["text"], "Add question");
    let question = click(&mut world, &session, "row-t-3000");
    assert!(has(&question, "t-3000-up") && has(&question, "r-300-up"));
    fill(&mut world, &session, "compose-body", "It means you write the FAQ on Tuesday.");
    assert!(says(&click(&mut world, &session, "compose-submit"), "It means you write the FAQ on Tuesday."));

    let home = go(&mut world, &session, "http://yelp.com/");
    assert_eq!(by_id(&home, "t-1000-open")["url"], "http://yelp.com/r/restaurants/comments/t-1000");
    assert_eq!(by_id(&home, "board-coffee-sub")["kind"], "button");
    assert_eq!(by_id(&home, "cat-restaurants")["url"], "http://yelp.com/r/restaurants");
    let biz = click(&mut world, &session, "t-1000-open");
    assert!(says(&biz, "Alder Street Kitchen"));
    fill(&mut world, &session, "compose-body", "***** 5/5. The counter seats are the move.");
    assert!(says(&click(&mut world, &session, "compose-submit"), "The counter seats are the move."));

    let home = go(&mut world, &session, "http://craigslist.org/");
    assert_eq!(by_id(&home, "nav-submit")["text"], "create a posting");
    assert_eq!(by_id(&home, "cat-0")["kind"], "link");
    assert_eq!(by_id(&home, "t-7000-comments")["url"], "http://craigslist.org/item?id=t-7000");
    let listing = click(&mut world, &session, "t-7000-title");
    assert_eq!(url(&world, &session), "http://craigslist.org/item?id=t-7000");
    assert!(says(&listing, "Solid oak dining table"));
    fill(&mut world, &session, "r-100-reply-body", "Sunday morning works too.");
    assert!(says(&click(&mut world, &session, "r-100-reply-submit"), "Sunday morning works too."));
    let cat = click(&mut world, &session, "nav-home");
    let furniture = cat.iter().find(|e| e["kind"] == "link" && e["text"] == "furniture").expect("a furniture category").clone();
    act(&mut world, &session, "browser.v1", "click", json!({"id": furniture["id"]}));
    assert!(url(&world, &session).ends_with("/questions/tagged/furniture"));
    let tagged = elements(&page(&world, &session));
    // A craigslist row is an `<li id="row-..">` with the title link inside, so the semantic tree
    // lists the link the agent would click rather than the row that holds it.
    let ids: Vec<&str> = tagged.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(has(&tagged, "t-7000-title"), "furniture listings: {ids:?} at {}", url(&world, &session));
    assert!(has(&tagged, "t-7002-title"), "every furniture listing is here: {ids:?}");
    assert!(!has(&tagged, "t-7003-title"), "a bikes listing is not on the furniture page: {ids:?}");
}
