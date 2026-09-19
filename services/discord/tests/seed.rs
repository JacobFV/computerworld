//! The shipped Discord seed and the server's behaviour: categories, roles, voice and the
//! short channel URLs the scenes and seeded prose use.
use cw_protocol::{HttpRequest, Page, PageElement};
use cw_sdk::{Service, ServiceContext};
use cw_service_discord::DiscordService;
use serde_json::{json, Value};

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 60,
        seed: 1,
        instance: "discord".into(),
    }
}
fn site() -> Value {
    let raw =
        std::fs::read_to_string("../../worlds/company-2026/sites/discord.json").expect("site");
    serde_json::from_str(&raw).expect("site file parses")
}
fn get(state: &mut Value, actor: &str, path: &str) -> (u16, String) {
    let r = DiscordService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::get(format!("http://discord.com{path}")),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn post(state: &mut Value, actor: &str, path: &str, body: Value) -> (u16, String) {
    let r = DiscordService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::json("POST", format!("http://discord.com{path}"), &body).unwrap(),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn page(state: &mut Value, actor: &str, path: &str) -> Page {
    let (status, body) = get(state, actor, path);
    assert_eq!(status, 200, "{path}");
    let page: Page = serde_json::from_str(&body).unwrap();
    page.validate().unwrap_or_else(|e| panic!("{path}: {e}"));
    page
}
/// Every element on the page, containers walked.
fn elements(page: &Page) -> Vec<&PageElement> {
    fn walk<'a>(items: &'a [PageElement], out: &mut Vec<&'a PageElement>) {
        for e in items {
            out.push(e);
            match e {
                PageElement::Group { children, .. }
                | PageElement::Form { children, .. }
                | PageElement::Row { children, .. }
                | PageElement::Grid { children, .. }
                | PageElement::Card { children, .. } => walk(children, out),
                _ => {}
            }
        }
    }
    let mut out = vec![];
    walk(&page.elements, &mut out);
    out
}
fn find<'a>(page: &'a Page, id: &str) -> Option<&'a PageElement> {
    elements(page).into_iter().find(|e| e.id() == id)
}
fn text(page: &Page, id: &str) -> String {
    match find(page, id) {
        Some(PageElement::Styled { text, .. } | PageElement::Button { text, .. }) => text.clone(),
        other => panic!("{id}: {other:?}"),
    }
}
fn seeded() -> Value {
    let site = site();
    assert_eq!(site["kind"], "discord");
    DiscordService
        .initialize(site["initial_state"].clone(), &ctx("alice"))
        .unwrap()
}

#[test]
fn the_seed_renders_its_server_and_every_search_entry() {
    let site = site();
    let mut state = seeded();
    for path in [
        "/",
        "/channels/atlas/welcome",
        "/channels/atlas/atlas-help",
        "/channels/atlas/showcase",
        "/channels/welcome",
        "/channels/atlas-help",
        "/channels/showcase",
        "/channels/@me",
    ] {
        let (status, body) = get(&mut state, "alice", path);
        assert_eq!(status, 200, "{path}");
        serde_json::from_str::<Page>(&body)
            .unwrap()
            .validate()
            .unwrap_or_else(|e| panic!("{path}: {e}"));
    }
    for entry in site["search_entries"].as_array().unwrap() {
        let url = entry["url"].as_str().unwrap();
        let r = DiscordService
            .handle(&mut state, &ctx("alice"), &HttpRequest::get(url))
            .unwrap();
        assert_eq!(r.status, 200, "search entry {url}");
    }
    // The short and the full URL are one page.
    assert_eq!(
        get(&mut state, "alice", "/channels/atlas-help").1,
        get(&mut state, "alice", "/channels/atlas/atlas-help").1
    );
    let page = get(&mut state, "alice", "/channels/atlas/atlas-help").1;
    assert!(page.contains("INFORMATION") && page.contains("SUPPORT") && page.contains("COMMUNITY"));
    assert!(page.contains("#5865f2") && page.contains("chat-33-react-eyes"));
    assert!(page.contains("voice-Lounge") && page.contains("voice-Lounge-praman"));
}

#[test]
fn the_channel_looks_like_discord() {
    let mut state = seeded();
    let p = page(&mut state, "alice", "/channels/atlas/atlas-help");
    // The header: hash, name, topic and the toolbar with its search box.
    assert_eq!(text(&p, "channel-title"), "atlas-help");
    assert!(find(&p, "channel-hash").is_some() && find(&p, "search-text").is_some());
    assert!(
        matches!(find(&p, "header"), Some(PageElement::Row { style, .. }) if style.pin.as_deref() == Some("top"))
    );
    // Messages are stamped from the civil clock and grouped under date dividers.
    assert_eq!(text(&p, "day-5-label"), "September 15, 2026");
    assert_eq!(text(&p, "day-7-label"), "September 17, 2026");
    assert_eq!(text(&p, "chat-33-time"), "09/15/2026 3:12 PM");
    assert_eq!(text(&p, "chat-45-time"), "Today at 8:31 AM");
    assert!(find(&p, "chat-45-avatar").is_some() && find(&p, "chat-45-author").is_some());
    // The last message carries the hover toolbar; the others do not.
    assert!(find(&p, "chat-45-tools").is_some() && find(&p, "chat-44-tools").is_none());
    // A reply shows Discord's reply line: the quoted author's face, name and text.
    assert_eq!(text(&p, "chat-34-quote-author"), "mkowalski");
    assert!(find(&p, "chat-34-quote-avatar").is_some());
    assert!(text(&p, "chat-34-quote-text").starts_with("My replay diverges"));
    assert!(
        matches!(find(&p, "chat-34-author"), Some(PageElement::Styled { style, .. }) if style.color.as_deref() == Some("#3ba55d"))
    );
    // Reactions are pill chips with the emoji itself; the actor's own is outlined blurple.
    assert!(
        matches!(find(&p, "chat-34-react-+1"), Some(PageElement::Button { text, style: Some(s), action, .. }) if text == "👍 2" && s.border.as_deref() == Some("#5865f2") && action.url == "/channels/atlas/atlas-help/messages/chat-34/reactions")
    );
    assert!(
        matches!(find(&p, "chat-33-react-eyes"), Some(PageElement::Button { text, style: Some(s), .. }) if text == "👀 1" && s.border.as_deref() == Some("#2b2d31"))
    );
    // Emoji short names in text are the characters they name; no reply form per message.
    assert!(text(&p, "chat-45-text-1-p0").contains("📌"));
    let body = serde_json::to_string(&p).unwrap();
    assert!(!body.contains(":pushpin:") && !body.contains("chat-45-reply-text"));
    // The composer is pinned to the bottom with the user panel under the sidebar.
    assert!(
        matches!(find(&p, "composer"), Some(PageElement::Row { style, .. }) if style.pin.as_deref() == Some("bottom"))
    );
    assert!(
        matches!(find(&p, "send-text"), Some(PageElement::Input { label, .. }) if label == "Message #atlas-help")
    );
    assert_eq!(text(&p, "me-name"), "alice.chen");
    assert!(find(&p, "me-mic").is_some() && find(&p, "me-settings").is_some());
    // The member list: hoisted roles, then online and offline, with presence.
    assert_eq!(text(&p, "group-Admin"), "ADMIN — 1");
    assert_eq!(text(&p, "group-Mod"), "MOD — 2");
    assert_eq!(text(&p, "group-online"), "ONLINE — 2");
    assert_eq!(text(&p, "group-offline"), "OFFLINE — 2");
    assert!(find(&p, "group-Member").is_none());
    assert_eq!(text(&p, "member-carol-presence"), "○");
    assert_eq!(text(&p, "member-bob-presence"), "●");
    // The sidebar: the open channel highlighted, voice occupants with faces.
    assert!(
        matches!(find(&p, "nav-atlas-help"), Some(PageElement::Card { style, .. }) if style.background.as_deref() == Some("#404249"))
    );
    assert!(
        matches!(find(&p, "nav-general"), Some(PageElement::Card { style, .. }) if style.background.as_deref() == Some("#2b2d31"))
    );
    assert!(find(&p, "voice-Lounge-jlee-avatar").is_some());
    // `?members=0` closes the member list and the header button reopens it.
    let closed = page(&mut state, "alice", "/channels/atlas/atlas-help?members=0");
    assert!(find(&closed, "members").is_none() && find(&closed, "composer-members").is_none());
    assert!(
        matches!(find(&closed, "channel-members"), Some(PageElement::Icon { action: Some(a), .. }) if a.url == "/channels/atlas/atlas-help")
    );
    // priya's second message two minutes on shares her header: no face, no name.
    let general = page(&mut state, "alice", "/channels/atlas/general");
    assert!(
        find(&general, "chat-58-avatar").is_some() && find(&general, "chat-58-author").is_some()
    );
    assert!(
        find(&general, "chat-57-avatar").is_none() && find(&general, "chat-57-author").is_none()
    );
    assert!(text(&general, "chat-57-text-p0").ends_with("🙏"));
    // Clicking a reaction chip toggles the actor's own reaction.
    let (status, after) = post(
        &mut state,
        "alice",
        "/channels/atlas-help/messages/chat-33/reactions",
        json!({"reaction":"eyes"}),
    );
    assert_eq!(status, 200);
    let after: Page = serde_json::from_str(&after).unwrap();
    assert!(
        matches!(find(&after, "chat-33-react-eyes"), Some(PageElement::Button { text, style: Some(s), .. }) if text == "👀 2" && s.border.as_deref() == Some("#5865f2"))
    );
}

#[test]
fn roles_gate_channels_and_membership_gates_the_server() {
    let mut state = seeded();
    // #mod-log is Admin and Mod only; alice moderates, bob does not.
    assert_eq!(get(&mut state, "alice", "/channels/atlas/mod-log").0, 200);
    assert_eq!(get(&mut state, "bob", "/channels/atlas/mod-log").0, 403);
    assert!(!get(&mut state, "bob", "/").1.contains("nav-mod-log"));
    assert_eq!(get(&mut state, "eve", "/channels/atlas/welcome").0, 403);
    assert_eq!(get(&mut state, "eve", "/api/server").0, 403);
    let listed: Value = serde_json::from_str(&get(&mut state, "bob", "/api/channels").1).unwrap();
    assert!(listed.get("mod-log").is_none() && listed.get("atlas-help").is_some());
    let server: Value = serde_json::from_str(&get(&mut state, "alice", "/api/server").1).unwrap();
    assert_eq!(server["name"], "Atlas Community");
    assert_eq!(server["members"]["alice"]["nick"], "alice.chen");
}

#[test]
fn replies_reactions_and_voice_are_real_routes() {
    let mut state = seeded();
    let (status, sent) = post(
        &mut state,
        "jlee",
        "/api/channels/atlas/atlas-help/messages",
        json!({"text":"That did it, thanks!","reply_to":"chat-45"}),
    );
    assert_eq!(status, 200);
    let sent: Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(sent["reply_to"], "chat-45");
    assert_eq!(
        post(
            &mut state,
            "jlee",
            "/api/channels/atlas-help/messages",
            json!({"text":"orphan","reply_to":"chat-1"})
        )
        .0,
        400
    );
    assert_eq!(
        post(
            &mut state,
            "bob",
            "/channels/atlas-help/messages/chat-33/reactions",
            json!({"reaction":"eyes"})
        )
        .0,
        200
    );
    let channel: Value =
        serde_json::from_str(&get(&mut state, "alice", "/api/channels/atlas-help").1).unwrap();
    assert_eq!(
        channel["messages"][0]["reactions"]["eyes"],
        json!(["bob", "praman"])
    );
    // A message sent now is stamped on Discord's clock, a week past the world's.
    assert_eq!(
        channel["messages"].as_array().unwrap().last().unwrap()["time"],
        60 + 7 * 24 * 60 * 60 * 1_000_000u64
    );
    assert_eq!(
        post(&mut state, "bob", "/api/voice/Office Hours/join", json!({})).0,
        200
    );
    assert_eq!(
        post(&mut state, "bob", "/api/voice/Lounge/join", json!({})).0,
        200
    );
    let server: Value = serde_json::from_str(&get(&mut state, "alice", "/api/server").1).unwrap();
    assert_eq!(
        server["voice"]["Lounge"]["occupants"],
        json!(["bob", "jlee", "praman"])
    );
    assert_eq!(server["voice"]["Office Hours"]["occupants"], json!([]));
    assert_eq!(
        post(&mut state, "eve", "/api/voice/Lounge/join", json!({})).0,
        403
    );
    let (status, page) = post(&mut state, "bob", "/voice/Lounge/leave", json!({}));
    assert_eq!(status, 200);
    assert!(!page.contains("voice-Lounge-bob"));
}
