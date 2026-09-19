//! The shipped Slack seed and the workspace's behaviour: every channel renders, every
//! search entry resolves, threads, pins, unread counts and mentions all hold, and the
//! page is laid out the way Slack is.
use cw_protocol::{HttpRequest, Page, PageElement};
use cw_sdk::{Service, ServiceContext};
use cw_service_slack::SlackService;
use serde_json::{json, Value};

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 60,
        seed: 1,
        instance: "slack".into(),
    }
}
fn site() -> Value {
    let raw = std::fs::read_to_string("../../worlds/company-2026/sites/slack.json").expect("site");
    serde_json::from_str(&raw).expect("site file parses")
}
fn get(state: &mut Value, actor: &str, path: &str) -> (u16, String) {
    let r = SlackService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::get(format!("http://slack.com{path}")),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn post(state: &mut Value, actor: &str, path: &str, body: Value) -> (u16, String) {
    let r = SlackService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::json("POST", format!("http://slack.com{path}"), &body).unwrap(),
        )
        .unwrap();
    (r.status, String::from_utf8(r.body).unwrap())
}
fn seeded() -> Value {
    let site = site();
    assert_eq!(site["kind"], "slack");
    assert!(site["initial_state"].get("skin").is_none());
    SlackService
        .initialize(site["initial_state"].clone(), &ctx("alice"))
        .unwrap()
}
fn page(body: &str) -> Page {
    let page: Page = serde_json::from_str(body).unwrap();
    page.validate().unwrap();
    page
}
/// Every element of a page, depth first.
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

#[test]
fn the_seed_renders_every_channel_and_search_entry() {
    let site = site();
    let mut state = seeded();
    for path in [
        "/",
        "/channels",
        "/dms",
        "/channels/eng",
        "/channels/random",
        "/channels/incidents",
        "/channels/general",
        "/channels/atlas-release",
        "/archives/eng",
        "/channels/eng?thread=chat-1",
        "/channels/eng?members=1",
        "/channels/alice|bob",
        "/channels/alice|bob|carol",
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
        let r = SlackService
            .handle(&mut state, &ctx("alice"), &HttpRequest::get(url))
            .unwrap();
        assert_eq!(r.status, 200, "search entry {url}");
    }
    let all = state.to_string();
    // 3 the flaky test, 5 the monitor, 7 the soundtrack, 10 the outage, 1 the launch.
    assert!(all.contains("stackoverflow.com/questions/t-4411"));
    assert!(all.contains("amazon.com/dp/b0monitor27"));
    assert!(all.contains("open.spotify.com/playlist/ship-it"));
    assert!(all.contains("status.northstar.example"));
    assert!(all.contains("docs.google.com/documents/atlas-launch"));
    // The release code is a search task, and Slack is not one of its three homes.
    assert!(!all.contains("ATLAS-2026"));
    // The private channel is only there for its members.
    assert_eq!(get(&mut state, "admin", "/channels/atlas-release").0, 403);
}

#[test]
fn the_workspace_is_laid_out_like_slack() {
    let mut state = seeded();
    let body = get(&mut state, "bob", "/channels/eng").1;
    let p = page(&body);
    assert_eq!(p.title, "#eng (Channel) - Northstar - Slack");
    // The top bar and the header are pinned to the top, the composer to the bottom.
    let pins: Vec<(&str, Option<&str>)> = p
        .elements
        .iter()
        .map(|e| match e {
            PageElement::Row { id, style, .. } => (id.as_str(), style.pin.as_deref()),
            _ => (e.id(), None),
        })
        .collect();
    assert_eq!(
        pins,
        vec![
            ("topbar", Some("top")),
            ("header", Some("top")),
            ("shell", None),
            ("composer", Some("bottom")),
        ]
    );
    assert!(body.contains("Search Northstar"));
    let Some(PageElement::Input { label, .. }) = find(&p, "send-text") else {
        panic!("composer field");
    };
    assert_eq!(label, "Message # eng");
    assert!(
        matches!(find(&p, "send-submit"), Some(PageElement::Icon { name, .. }) if name == "send")
    );
    assert!(find(&p, "send-bold").is_some() && find(&p, "send-hint").is_some());
    // The header: name, topic, member count opening the member list, pins.
    assert!(
        matches!(find(&p, "channel-title"), Some(PageElement::Styled { text, .. }) if text == "# eng")
    );
    assert!(
        matches!(find(&p, "channel-members"), Some(PageElement::Card { action: Some(a), .. }) if a.url == "/channels/eng?members=1")
    );
    assert!(
        matches!(find(&p, "channel-pins"), Some(PageElement::Styled { text, .. }) if text == "2 Pinned")
    );
    assert!(find(&p, "chat-5-pinned").is_some() && find(&p, "channel-topic").is_some());
    // The sidebar: hash and lock marks, the open channel, DM rows with presence.
    assert!(find(&p, "nav-eng-hash").is_some() && find(&p, "nav-atlas-release-lock").is_some());
    assert!(
        matches!(find(&p, "nav-eng"), Some(PageElement::Card { style, .. }) if style.background.as_deref() == Some("#1164a3"))
    );
    assert!(
        find(&p, "dm-alice|bob-presence").is_some() && find(&p, "dm-alice|bob|carol").is_some()
    );
    assert!(find(&p, "rail-home").is_some() && find(&p, "rail-activity").is_some());
    // Messages: date dividers, grouped runs, clock times, emoji, mentions and code.
    for day in ["Today", "Yesterday", "Tuesday", "Monday"] {
        assert!(body.contains(&format!("\"text\":\"{day}\"")), "{day}");
    }
    assert!(find(&p, "chat-5-author").is_some() && find(&p, "chat-5-avatar").is_some());
    assert!(find(&p, "chat-6-author").is_none() && find(&p, "chat-6-avatar").is_none());
    assert!(
        matches!(find(&p, "chat-12-time"), Some(PageElement::Styled { text, .. }) if text == "9:41 AM")
    );
    assert!(body.contains("✅") && !body.contains(":white_check_mark:"));
    assert!(elements(&p).iter().any(|e| matches!(e, PageElement::Styled { text, style, .. } if text == "HashMap" && style.mono == Some(true))));
    assert!(elements(&p).iter().any(|e| matches!(e, PageElement::Styled { text, style, .. } if text == "@admin" && style.background.is_some())));
    assert!(elements(&p).iter().any(|e| matches!(e, PageElement::Link { id, url, .. } if id.starts_with("chat-12-text") && url == "http://github.com/northstar/atlas/pull/15")));
    // Long lines are cut into rows at word boundaries, narrower beside a pane.
    assert!(find(&p, "chat-2-text").is_some() && find(&p, "chat-2-text-1").is_none());
    let beside = page(&get(&mut state, "bob", "/channels/eng?thread=chat-1").1);
    assert!(find(&beside, "chat-2-text-1").is_some() && find(&beside, "thread-pane").is_some());
    // Unread channels are bold; only mentions and DMs carry a count.
    assert!(find(&p, "nav-eng-unread").is_none() && find(&p, "nav-atlas-release-unread").is_some());
    assert!(
        matches!(find(&p, "dm-alice|bob-unread"), Some(PageElement::Badge { text, .. }) if text == "4")
    );
    // Reactions are chips that react; the actor's own are outlined in blue.
    assert!(
        matches!(find(&p, "chat-2-react-eyes"), Some(PageElement::Button { text, style: Some(s), action, .. }) if text == "👀 2" && s.border.as_deref() == Some("#1264a3") && action.url == "/channels/eng/messages/chat-2/reactions")
    );
    assert!(
        matches!(find(&p, "chat-14-react-eyes"), Some(PageElement::Button { style: Some(s), .. }) if s.border.as_deref() == Some("#e0e0e0"))
    );
    // Threads collapse to a link row; the toolbar floats over the last message only.
    assert!(
        matches!(find(&p, "chat-1-replies"), Some(PageElement::Link { text, url, .. }) if text == "3 replies" && url == "/channels/eng?thread=chat-1")
    );
    assert!(
        find(&p, "chat-1-thread-avatar-bob").is_some() && find(&p, "chat-1-last-reply").is_some()
    );
    assert!(find(&p, "chat-1-reply-body").is_none() && find(&p, "chat-3-text").is_none());
    assert!(find(&p, "chat-15-quick-+1").is_some() && find(&p, "chat-15-open-thread").is_some());
    assert!(find(&p, "chat-14-quick-+1").is_none() && find(&p, "chat-14-open-thread").is_none());
    assert!(find(&p, "chat-15-pin").is_some() && find(&p, "chat-15-add-reaction").is_some());
    // Bob is mentioned nowhere in #eng, but is in the release channel.
    assert!(
        matches!(find(&p, "chat-15-row"), Some(PageElement::Row { style, .. }) if style.background.is_none())
    );
    let release = page(&get(&mut state, "bob", "/channels/atlas-release").1);
    assert!(
        matches!(find(&release, "chat-35-row"), Some(PageElement::Row { style, .. }) if style.background.is_some())
    );
    // Slack permalinks in seeded prose are /archives/<channel>.
    assert_eq!(get(&mut state, "bob", "/archives/eng").1, body);
}

#[test]
fn threads_open_in_a_pane_and_replies_land_back_in_it() {
    let mut state = seeded();
    let p = page(&get(&mut state, "carol", "/channels/eng?thread=chat-1").1);
    assert!(find(&p, "thread-pane").is_some() && find(&p, "thread-close").is_some());
    assert!(find(&p, "chat-3-text").is_some() && find(&p, "chat-17-text").is_some());
    assert!(find(&p, "thread-chat-1-text").is_some() && find(&p, "chat-1-text").is_some());
    assert!(
        matches!(find(&p, "thread-count-text"), Some(PageElement::Styled { text, .. }) if text == "3 replies")
    );
    let Some(PageElement::Form { action, .. }) = find(&p, "chat-1-reply") else {
        panic!("reply form");
    };
    assert_eq!(action.fields["parent"], "chat-1");
    assert_eq!(action.fields["text"], "$chat-1-reply-body");
    assert!(find(&p, "chat-1-reply-body").is_some());
    // An unknown thread is the plain conversation.
    assert!(find(
        &page(&get(&mut state, "carol", "/channels/eng?thread=chat-999").1),
        "thread-pane"
    )
    .is_none());
    let (status, threaded) = post(
        &mut state,
        "carol",
        "/channels/eng/messages",
        json!({"text":"Accepted answer is the hash order.","parent":"chat-1"}),
    );
    assert_eq!(status, 200);
    let p = page(&threaded);
    assert!(find(&p, "thread-pane").is_some() && threaded.contains("Accepted answer"));
    assert!(
        matches!(find(&p, "chat-1-replies"), Some(PageElement::Link { text, .. }) if text == "4 replies")
    );
    let before = state.clone();
    assert_eq!(
        post(
            &mut state,
            "carol",
            "/channels/eng/messages",
            json!({"text":"orphan","parent":"chat-999"})
        )
        .0,
        400
    );
    assert_eq!(before, state);
    assert_eq!(get(&mut state, "eve", "/channels/eng").0, 403);
    assert_eq!(get(&mut state, "eve", "/api/channels/eng").0, 403);
}

#[test]
fn the_member_list_and_the_dm_view() {
    let mut state = seeded();
    let members = get(&mut state, "alice", "/channels/eng?members=1").1;
    assert!(members.contains("Software Engineer") && members.contains("member-carol-status"));
    assert!(find(&page(&members), "members-close").is_some());
    assert!(!get(&mut state, "alice", "/channels/eng")
        .1
        .contains("member-carol-status"));
    let dm = get(&mut state, "alice", "/channels/alice|bob").1;
    let p = page(&dm);
    assert_eq!(p.title, "bob (DM) - Northstar - Slack");
    assert!(find(&p, "channel-presence").is_some() && find(&p, "channel-avatar").is_some());
    assert!(
        matches!(find(&p, "channel-status"), Some(PageElement::Styled { text, .. }) if text.contains("Windows CI"))
    );
    assert!(find(&p, "chat-40-author").is_some() && find(&p, "chat-41-author").is_some());
    assert!(dm.contains("🎉"));
    // A sent message is stamped on Slack's clock and lands in the conversation.
    let (status, sent) = post(
        &mut state,
        "alice",
        "/channels/alice|bob/messages",
        json!({"text":"see you at standup"}),
    );
    assert_eq!(status, 200);
    assert!(sent.contains("see you at standup"));
    let last = state["dms"]["alice|bob"]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["time"], 60 + cw_service_slack::HISTORY);
}

#[test]
fn unread_counts_and_mentions_are_per_person() {
    let mut state = seeded();
    // Nobody has opened anything: everything seeded is unread for everyone.
    let unread: Value = serde_json::from_str(&get(&mut state, "bob", "/api/unread").1).unwrap();
    assert_eq!(unread["eng"], 17);
    let banner = page(&get(&mut state, "bob", "/channels/eng").1);
    assert!(
        matches!(find(&banner, "read-count"), Some(PageElement::Styled { text, .. }) if text == "17 new messages")
    );
    assert!(find(&banner, "read-submit").is_some());
    assert_eq!(
        post(&mut state, "bob", "/api/channels/eng/read", json!({})).0,
        200
    );
    let unread: Value = serde_json::from_str(&get(&mut state, "bob", "/api/unread").1).unwrap();
    assert!(unread.get("eng").is_none());
    assert!(find(
        &page(&get(&mut state, "bob", "/channels/eng").1),
        "read-submit"
    )
    .is_none());
    let (status, sent) = post(
        &mut state,
        "alice",
        "/api/channels/eng/messages",
        json!({"text":"@bob can you re-run the Windows job?"}),
    );
    assert_eq!(status, 200);
    let sent: Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(sent["author"], "alice");
    // Bob is also @-mentioned in the seeded release plan he has not read.
    let mentions: Value = serde_json::from_str(&get(&mut state, "bob", "/api/mentions").1).unwrap();
    assert_eq!(mentions.as_array().unwrap().len(), 2);
    assert_eq!(mentions[1]["channel"], "eng");
    let sidebar = get(&mut state, "bob", "/").1;
    assert!(
        matches!(find(&page(&sidebar), "nav-eng-unread"), Some(PageElement::Badge { text, .. }) if text == "1")
    );
    assert!(
        matches!(find(&page(&sidebar), "rail-activity-count"), Some(PageElement::Badge { text, .. }) if text == "2")
    );
    // Carol was not mentioned and has her own count.
    let mentions: Value =
        serde_json::from_str(&get(&mut state, "carol", "/api/mentions").1).unwrap();
    assert!(mentions.as_array().unwrap().is_empty());
    // The API channel view carries the unread count for the caller.
    let eng: Value = serde_json::from_str(&get(&mut state, "bob", "/api/channels/eng").1).unwrap();
    assert_eq!(eng["unread"], 1);
    assert_eq!(eng["topic"], "CI, reviews and the Atlas launch");
}

#[test]
fn pins_status_and_group_dms() {
    let mut state = seeded();
    let (status, _) = post(
        &mut state,
        "alice",
        "/api/channels/eng/messages/chat-7/pin",
        json!({}),
    );
    assert_eq!(status, 200);
    let pins: Value =
        serde_json::from_str(&get(&mut state, "bob", "/api/channels/eng/pins").1).unwrap();
    assert_eq!(pins.as_array().unwrap().len(), 3);
    assert_eq!(
        post(
            &mut state,
            "alice",
            "/api/status",
            json!({"status":"🍜 lunch"})
        )
        .0,
        200
    );
    assert!(get(&mut state, "alice", "/").1.contains("🍜 lunch"));
    let (status, body) = post(
        &mut state,
        "alice",
        "/api/dms",
        json!({"to":["bob","carol"]}),
    );
    assert_eq!(status, 200);
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["conversation"], "alice|bob|carol");
    assert_eq!(
        post(&mut state, "carol", "/api/dms", json!({"to":"alice,bob"})).0,
        200
    );
    assert_eq!(state["dms"].as_object().unwrap().len(), 2);
    assert_eq!(get(&mut state, "admin", "/channels/alice|bob|carol").0, 403);
    assert!(get(&mut state, "bob", "/channels/alice|bob|carol")
        .1
        .contains("alice, carol"));
    // A DM with someone new is one click in the sidebar, and lands on the conversation.
    let (status, opened) = post(&mut state, "admin", "/dms", json!({"to":"bob"}));
    assert_eq!(status, 200);
    assert_eq!(page(&opened).title, "bob (DM) - Northstar - Slack");
}
