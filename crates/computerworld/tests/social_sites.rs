//! The seven social sites served as HTML by the social service and driven end to end through
//! the agent API: the browser renders each with the web engine, the semantic observation
//! lists the navigation, the composer, the posts and their buttons by the ids the service
//! documents, and posting, liking, replying, following, searching and messaging all work by
//! clicking and filling those ids.
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
    assert!(
        result.outcomes[0].success,
        "{op} {payload_hint}: {:?}",
        result.outcomes[0],
        payload_hint = op
    );
    result.outcomes[0].value.clone()
}
fn go(world: &mut World, session: &str, url: &str) {
    act(
        world,
        session,
        "browser.v1",
        "navigate",
        json!({ "url": url }),
    );
}
fn click(world: &mut World, session: &str, id: &str) {
    act(world, session, "browser.v1", "click", json!({ "id": id }));
}
fn fill(world: &mut World, session: &str, id: &str, value: &str) {
    act(
        world,
        session,
        "browser.v1",
        "fill",
        json!({ "id": id, "value": value }),
    );
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"]
        .as_str()
        .unwrap()
        .to_owned()
}
/// The page title and every element of the semantic tree, flattened.
fn page(world: &World, session: &str) -> (String, Vec<Value>) {
    fn walk(elements: &[Value], out: &mut Vec<Value>) {
        for e in elements {
            out.push(e.clone());
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let page = world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone();
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    (page["title"].as_str().unwrap_or("").to_owned(), out)
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn has_text(all: &[Value], needle: &str) -> bool {
    all.iter()
        .any(|e| e["text"].as_str().is_some_and(|t| t.contains(needle)))
}
/// `Like 12` -> ("Like", 12).
fn count(label: &str) -> (String, u64) {
    let (word, n) = label
        .rsplit_once(' ')
        .unwrap_or_else(|| panic!("label {label:?}"));
    (word.to_owned(), n.parse().unwrap())
}

/// What every site does the same way: the chrome, a post, a like, a thread, a reply, a
/// follow, and a search. Returns the id of the post it used.
fn drive(
    world: &mut World,
    session: &str,
    domain: &str,
    brand: &str,
    home: &str,
    inbox: &str,
) -> String {
    let root = format!("http://{domain}");
    go(world, session, &format!("{root}/"));
    let (title, all) = page(world, session);
    assert_eq!(title, format!("{brand} / {home}"));
    for (id, path) in [
        ("brand", "/"),
        ("nav-home", "/"),
        ("nav-explore", "/explore"),
        ("nav-search", "/search"),
        ("nav-inbox", inbox),
    ] {
        let e = by_id(&all, id);
        assert_eq!(e["kind"], "link", "{domain} {id}");
        assert_eq!(e["url"], format!("{root}{path}"), "{domain} {id}");
    }
    assert_eq!(by_id(&all, "compose")["kind"], "form");
    assert_eq!(by_id(&all, "compose-text")["kind"], "input");
    assert_eq!(by_id(&all, "compose-text")["label"], "Post");
    assert_eq!(by_id(&all, "compose-submit")["kind"], "button");

    // Post: fill the composer and press its button; the new post's thread comes back.
    let words = format!("Hello from the agent API on {brand}");
    fill(world, session, "compose-text", &words);
    click(world, session, "compose-submit");
    let (_, all) = page(world, session);
    assert!(has_text(&all, &words), "{domain}: the new post is shown");
    assert_eq!(by_id(&all, "reply")["kind"], "form");

    // Like: the first post on Home that is somebody else's (so its author can be followed
    // further down), toggled by its button.
    go(world, session, &format!("{root}/"));
    let (_, all) = page(world, session);
    let open = all
        .iter()
        .find(|e| {
            let Some(post) = e["id"].as_str().and_then(|i| i.strip_prefix("post-")) else {
                return false;
            };
            e["kind"] == "link"
                && all
                    .iter()
                    .any(|n| n["id"] == format!("{post}-name") && n["text"] != "Alice Chen")
        })
        .unwrap_or_else(|| panic!("{domain}: no post on home"))
        .clone();
    let post = open["id"]
        .as_str()
        .unwrap()
        .trim_start_matches("post-")
        .to_owned();
    let thread = open["url"].as_str().unwrap().to_owned();
    assert!(
        thread.starts_with(&root) && thread.ends_with(&format!("/status/{post}")),
        "{thread}"
    );
    let like = format!("{post}-like");
    assert_eq!(by_id(&all, &like)["kind"], "button");
    let (word, before) = count(by_id(&all, &like)["text"].as_str().unwrap());
    click(world, session, &like);
    let (title, all) = page(world, session);
    assert_eq!(
        title,
        format!("{brand} / {home}"),
        "{domain}: a like comes back to the page it was on"
    );
    let (after_word, after) = count(by_id(&all, &like)["text"].as_str().unwrap());
    if word == "Like" {
        assert_eq!(
            (after_word.as_str(), after),
            ("Liked", before + 1),
            "{domain}"
        );
    } else {
        assert_eq!(
            (after_word.as_str(), after),
            ("Like", before - 1),
            "{domain}"
        );
    }
    let (_, reposts) = count(
        by_id(&all, &format!("{post}-repost"))["text"]
            .as_str()
            .unwrap(),
    );
    click(world, session, &format!("{post}-repost"));
    let (_, all) = page(world, session);
    let (_, reposted) = count(
        by_id(&all, &format!("{post}-repost"))["text"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(reposted.abs_diff(reposts), 1, "{domain}");

    // Thread: the post's text is the link; reply there.
    click(world, session, &format!("post-{post}"));
    assert_eq!(url(world, session), thread);
    let (_, all) = page(world, session);
    assert_eq!(by_id(&all, "reply-text")["label"], "Reply");
    fill(world, session, "reply-text", "Replying through the form");
    click(world, session, "reply-submit");
    let (_, all) = page(world, session);
    assert!(has_text(&all, "Replying through the form"), "{domain}");
    assert!(
        has_text(&all, "1 replies") || all.iter().any(|e| e["id"] == "replies-title"),
        "{domain}"
    );

    // Profile: the author's name is a link; follow toggles.
    let author = by_id(&all, &format!("{post}-name")).clone();
    assert_eq!(author["kind"], "link");
    click(world, session, &format!("{post}-name"));
    assert_eq!(url(world, session), author["url"].as_str().unwrap());
    let (_, all) = page(world, session);
    assert_eq!(by_id(&all, "name")["text"], author["text"]);
    let was = by_id(&all, "follow")["text"].as_str().unwrap().to_owned();
    click(world, session, "follow");
    let (_, all) = page(world, session);
    let now = by_id(&all, "follow")["text"].as_str().unwrap().to_owned();
    assert_eq!(
        now,
        if was == "Follow" {
            "Following"
        } else {
            "Follow"
        },
        "{domain}"
    );

    // Search: the form posts `q` and the results are posts again.
    click(world, session, "nav-search");
    assert_eq!(url(world, session), format!("{root}/search"));
    fill(world, session, "search-q", "agent API");
    click(world, session, "search-submit");
    let (_, all) = page(world, session);
    assert_eq!(by_id(&all, "search-q")["value"], "agent API");
    assert!(has_text(&all, "1 result for \"agent API\""), "{domain}");
    assert!(has_text(&all, &words), "{domain}");
    post
}

#[test]
fn x_is_posted_to_liked_replied_to_and_messaged_through_the_agent_api() {
    let (mut world, session) = world();
    drive(&mut world, &session, "x.com", "X", "Home", "/messages");
    // The alias reaches the same site, and the seeded conversation with The Verge is there.
    go(&mut world, &session, "http://twitter.com/messages");
    let (title, all) = page(&world, &session);
    assert_eq!(title, "X / Messages");
    assert_eq!(by_id(&all, "thread-c-verge")["kind"], "link");
    click(&mut world, &session, "thread-c-verge");
    assert_eq!(url(&world, &session), "http://twitter.com/messages/c-verge");
    let (_, all) = page(&world, &session);
    assert_eq!(by_id(&all, "open-title")["text"], "Comment for a story?");
    assert_eq!(by_id(&all, "dm-text")["label"], "Message");
    fill(&mut world, &session, "dm-text", "Thanks, Tom. Reads well.");
    click(&mut world, &session, "dm-submit");
    let (_, all) = page(&world, &session);
    assert!(has_text(&all, "Thanks, Tom. Reads well."));
    // Typing and Enter submit the composer too.
    go(&mut world, &session, "http://x.com/");
    click(&mut world, &session, "compose-text");
    act(
        &mut world,
        &session,
        "keyboard.v1",
        "type",
        json!({"text": "Typed, not filled"}),
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "key",
        json!({"key": "Enter"}),
    );
    let (_, all) = page(&world, &session);
    assert!(has_text(&all, "Typed, not filled"));
    // The side column is real navigation: a trend opens its thread.
    go(&mut world, &session, "http://x.com/");
    let (_, all) = page(&world, &session);
    let trend = by_id(&all, "trend-0")["url"].as_str().unwrap().to_owned();
    click(&mut world, &session, "trend-0");
    assert_eq!(url(&world, &session), trend);
}

#[test]
fn bluesky_and_mastodon_run_the_same_flow_in_their_own_skins() {
    let (mut world, session) = world();
    drive(
        &mut world,
        &session,
        "bsky.app",
        "Bluesky",
        "Home",
        "/messages",
    );
    go(&mut world, &session, "http://bsky.app/explore");
    assert_eq!(page(&world, &session).0, "Bluesky / Discover");
    drive(
        &mut world,
        &session,
        "mastodon.social",
        "mastodon.social",
        "Home",
        "/messages",
    );
    go(&mut world, &session, "http://mastodon.social/local");
    let (title, all) = page(&world, &session);
    assert_eq!(title, "mastodon.social / Local timeline");
    // A federated handle is shown whole.
    assert!(has_text(&all, "@mastodon.social"));
}

#[test]
fn facebook_is_driven_from_its_top_bar_shortcuts_and_contacts() {
    let (mut world, session) = world();
    drive(
        &mut world,
        &session,
        "facebook.com",
        "Facebook",
        "Home",
        "/messages",
    );
    go(&mut world, &session, "http://facebook.com/");
    let (_, all) = page(&world, &session);
    assert_eq!(
        by_id(&all, "short-me")["url"],
        "http://facebook.com/alice.chen"
    );
    assert_eq!(by_id(&all, "top-q")["kind"], "input");
    let contact = by_id(&all, "contact-0")["url"].as_str().unwrap().to_owned();
    click(&mut world, &session, "contact-0");
    assert_eq!(url(&world, &session), contact);
    click(&mut world, &session, "nav-inbox");
    click(&mut world, &session, "thread-c-birthday");
    fill(&mut world, &session, "dm-text", "Saturday works.");
    click(&mut world, &session, "dm-submit");
    assert!(has_text(&page(&world, &session).1, "Saturday works."));
}

#[test]
fn instagram_and_pinterest_are_walls_of_pictures_with_the_same_controls() {
    let (mut world, session) = world();
    drive(
        &mut world,
        &session,
        "instagram.com",
        "Instagram",
        "Home",
        "/messages",
    );
    go(&mut world, &session, "http://instagram.com/");
    let (_, all) = page(&world, &session);
    let story = by_id(&all, "story-0")["url"].as_str().unwrap().to_owned();
    click(&mut world, &session, "story-0");
    assert_eq!(url(&world, &session), story);
    go(&mut world, &session, "http://instagram.com/explore");
    let (title, all) = page(&world, &session);
    assert_eq!(title, "Instagram / Explore");
    assert!(
        all.iter()
            .filter(|e| e["id"].as_str().is_some_and(|i| i.starts_with("post-")))
            .count()
            >= 9
    );

    let pin = drive(
        &mut world,
        &session,
        "pinterest.com",
        "Pinterest",
        "Home",
        "/messages",
    );
    go(&mut world, &session, "http://pin.it/explore");
    let (title, all) = page(&world, &session);
    assert_eq!(title, "Pinterest / Today");
    assert_eq!(by_id(&all, &format!("post-{pin}"))["kind"], "link");
}

#[test]
fn linkedin_connects_and_answers_the_recruiter() {
    let (mut world, session) = world();
    drive(
        &mut world,
        &session,
        "linkedin.com",
        "LinkedIn",
        "Feed",
        "/messaging",
    );
    go(&mut world, &session, "http://linkedin.com/");
    let (_, all) = page(&world, &session);
    assert_eq!(
        by_id(&all, "card-me")["url"],
        "http://linkedin.com/alice-chen"
    );
    go(&mut world, &session, "http://linkedin.com/nadia-fischer");
    let (_, all) = page(&world, &session);
    // Plain paragraphs are listed as text runs (the browser numbers them itself), so the
    // headline is found by its words; headings, links, inputs and buttons keep their ids.
    assert!(has_text(&all, "Technical Recruiting at Meridian Labs"));
    assert_eq!(by_id(&all, "name")["text"], "Nadia Fischer");
    assert_eq!(by_id(&all, "connect")["text"], "Connect");
    click(&mut world, &session, "connect");
    let (_, all) = page(&world, &session);
    assert_eq!(by_id(&all, "connect")["text"], "Invitation sent");
    go(&mut world, &session, "http://linkedin.com/bob-martinez");
    let (_, all) = page(&world, &session);
    assert!(has_text(&all, "Experience"));
    assert!(all
        .iter()
        .any(|e| e["id"] == "exp-0-title"
            || e["text"].as_str().is_some_and(|t| t.contains("Northstar"))));
}
