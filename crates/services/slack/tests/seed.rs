//! The shipped Slack seed and the workspace's behaviour: every channel renders, every
//! search entry resolves, threads, pins, unread counts and mentions all hold, and the
//! page is laid out the way Slack is.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_common::html::validate_strict;
use cw_service_slack::SlackService;
use cw_web::dom::Document as Dom;
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
    let raw = std::fs::read_to_string("../../../worlds/internet/sites/slack.json").expect("site");
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
    if r.status == 200 && !path.starts_with("/api/") {
        assert_eq!(r.header("content-type"), Some("text/html; charset=utf-8"));
    }
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
/// A POST the way the browser sends a form: url-encoded fields.
fn post_form(state: &mut Value, actor: &str, path: &str, fields: &[(&str, &str)]) -> (u16, String) {
    let mut r = HttpRequest::get(format!("http://slack.com{path}"));
    r.method = "POST".into();
    r.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    r.body = cw_service_common::html::href("", fields)
        .trim_start_matches('?')
        .as_bytes()
        .to_vec();
    let r = SlackService.handle(state, &ctx(actor), &r).unwrap();
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
/// The page parsed by the engine, after the strict validator has passed it.
fn page(body: &str) -> Dom {
    validate_strict(body).unwrap_or_else(|e| panic!("strict: {e:?}"));
    cw_web::html::parse(body)
}
fn has(page: &Dom, id: &str) -> bool {
    !page.by_id(id).is_empty()
}
fn text_of(page: &Dom, id: &str) -> String {
    let node = *page.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"));
    page.text_content(node)
}
fn attr_of(page: &Dom, id: &str, name: &str) -> String {
    let node = *page.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"));
    page.attr(node, name).unwrap_or_default().to_owned()
}
fn tag_of(page: &Dom, id: &str) -> String {
    let node = *page.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"));
    page.tag(node).unwrap_or_default().to_owned()
}
fn has_class(page: &Dom, id: &str, class: &str) -> bool {
    let node = *page.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"));
    page.has_class(node, class)
}
/// The form an element belongs to.
fn form_of(page: &Dom, id: &str) -> cw_web::dom::NodeId {
    let node = *page.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"));
    std::iter::once(node)
        .chain(page.ancestors(node))
        .find(|n| page.is(*n, "form"))
        .unwrap_or_else(|| panic!("#{id} is in no form"))
}
/// The `name=value` of every hidden field of a form.
fn hidden_fields(page: &Dom, form: cw_web::dom::NodeId) -> Vec<(String, String)> {
    page.descendants(form)
        .filter(|n| page.is(*n, "input") && page.attr(*n, "type") == Some("hidden"))
        .map(|n| {
            (
                page.attr(n, "name").unwrap_or_default().to_owned(),
                page.attr(n, "value").unwrap_or_default().to_owned(),
            )
        })
        .collect()
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
        validate_strict(&body).unwrap_or_else(|e| panic!("{path}: {e:?}"));
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
    let title = p
        .descendants(Dom::ROOT)
        .find(|n| p.is(*n, "title"))
        .unwrap();
    assert_eq!(p.text_content(title), "#eng (Channel) - Northstar - Slack");
    // An app shell: the top bar over the frame, the frame holding the rail and the
    // panel, the panel the sidebar and the conversation, whose list scrolls alone
    // between the header and the composer.
    let body_children: Vec<String> = p
        .element_children(p.body().unwrap())
        .map(|n| p.attr(n, "id").unwrap_or_default().to_owned())
        .collect();
    assert_eq!(body_children, ["topbar", "app"]);
    let main_children: Vec<String> = p
        .element_children(p.by_id("main")[0])
        .map(|n| p.attr(n, "id").unwrap_or_default().to_owned())
        .collect();
    assert_eq!(main_children, ["channel-header", "messages", "composer"]);
    assert!(has_class(&p, "messages", "scroller"));
    // The top bar's search box is a real GET form on /search, labelled and submittable.
    assert_eq!(text_of(&p, "search-text"), "Search Northstar");
    assert_eq!(tag_of(&p, "search"), "form");
    assert_eq!(attr_of(&p, "search", "action"), "/search");
    assert_eq!(attr_of(&p, "search", "method"), "get");
    assert_eq!(attr_of(&p, "search-text", "for"), "search-q");
    assert_eq!(attr_of(&p, "search-q", "name"), "q");
    assert_eq!(form_of(&p, "search-go"), p.by_id("search")[0]);
    // The composer: one POST form with the field and the send button.
    assert_eq!(tag_of(&p, "send"), "form");
    assert_eq!(attr_of(&p, "send", "action"), "/channels/eng/messages");
    assert_eq!(attr_of(&p, "send", "method"), "post");
    assert_eq!(attr_of(&p, "send-text", "name"), "text");
    assert_eq!(attr_of(&p, "send-text", "aria-label"), "Message # eng");
    assert_eq!(tag_of(&p, "send-submit"), "button");
    assert_eq!(attr_of(&p, "send-submit", "aria-label"), "Send message");
    assert_eq!(form_of(&p, "send-submit"), p.by_id("send")[0]);
    // The formatting strip, the attach, emoji and mention buttons and the
    // "Shift + Enter" hint all described a composer this world cannot run: gone.
    assert!(!has(&p, "send-bold") && !has(&p, "send-hint") && !has(&p, "send-attach"));
    // The header: name, topic, member count opening the member list, pins.
    assert_eq!(text_of(&p, "channel-title"), "# eng");
    assert_eq!(tag_of(&p, "channel-members"), "a");
    assert_eq!(
        attr_of(&p, "channel-members", "href"),
        "/channels/eng?members=1"
    );
    assert_eq!(text_of(&p, "channel-members-count"), "4");
    assert_eq!(text_of(&p, "channel-pins"), "2 Pinned");
    assert!(has(&p, "chat-5-pinned") && has(&p, "channel-topic"));
    // The sidebar: hash and lock marks, the open channel, DM rows with presence.
    assert!(has(&p, "nav-eng-hash") && has(&p, "nav-atlas-release-lock"));
    assert_eq!(attr_of(&p, "nav-eng", "href"), "/channels/eng");
    assert!(has_class(&p, "nav-eng", "current") && !has_class(&p, "nav-random", "current"));
    assert!(has(&p, "dm-alice|bob-presence"));
    assert_eq!(
        attr_of(&p, "dm-alice|bob|carol", "href"),
        "/channels/alice|bob|carol"
    );
    assert_eq!(attr_of(&p, "rail-home", "href"), "/");
    assert_eq!(attr_of(&p, "rail-dms", "href"), "/dms");
    assert!(has(&p, "rail-mark"));
    assert_eq!(attr_of(&p, "rail-activity", "href"), "/activity");
    // Later and More named nothing here, and the add-channel, add-apps, compose and
    // workspace-menu rows had nowhere to go.
    assert!(!has(&p, "rail-later") && !has(&p, "rail-more"));
    assert!(!has(&p, "add-channel") && !has(&p, "add-apps") && !has(&p, "compose"));
    // Messages: date dividers, grouped runs, clock times, emoji, mentions and code.
    let days: Vec<String> = p
        .descendants(Dom::ROOT)
        .filter(|n| p.has_class(*n, "day-label"))
        .map(|n| p.text_content(n))
        .collect();
    for day in ["Today", "Yesterday", "Tuesday", "Monday"] {
        assert!(days.iter().any(|d| d == day), "{day} in {days:?}");
    }
    assert!(has(&p, "chat-5-author") && has(&p, "chat-5-avatar"));
    assert!(!has(&p, "chat-6-author") && !has(&p, "chat-6-avatar"));
    assert_eq!(text_of(&p, "chat-12-time"), "9:41 AM");
    assert!(text_of(&p, "chat-6-text").ends_with("deterministic. ✅"));
    let code: Vec<String> = p
        .descendants(Dom::ROOT)
        .filter(|n| p.is(*n, "code"))
        .map(|n| p.text_content(n))
        .collect();
    assert!(code.iter().any(|c| c == "HashMap"), "{code:?}");
    assert!(p
        .descendants(Dom::ROOT)
        .any(|n| p.has_class(n, "mention") && p.text_content(n) == "@admin"));
    assert!(p.descendants(Dom::ROOT).any(|n| p.is(n, "a")
        && p.attr(n, "id")
            .is_some_and(|id| id.starts_with("chat-12-text"))
        && p.attr(n, "href") == Some("http://github.com/northstar/atlas/pull/15")));
    // A message is one block of text the page wraps, beside a pane or not.
    assert!(has(&p, "chat-2-text") && !has(&p, "chat-2-text-1"));
    assert!(text_of(&p, "chat-2-text").starts_with("That smells like hash-map iteration order."));
    let beside = page(&get(&mut state, "bob", "/channels/eng?thread=chat-1").1);
    assert!(has(&beside, "chat-2-text") && has(&beside, "thread-pane"));
    // Unread channels are bold; only mentions and DMs carry a count.
    assert!(has_class(&p, "nav-eng", "current") && has_class(&p, "nav-random", "unread"));
    assert!(!has(&p, "nav-eng-unread") && has(&p, "nav-atlas-release-unread"));
    assert_eq!(text_of(&p, "dm-alice|bob-unread"), "4");
    // Reactions are chips that react; the actor's own are outlined in blue.
    assert_eq!(tag_of(&p, "chat-2-react-eyes"), "button");
    assert_eq!(text_of(&p, "chat-2-react-eyes"), "👀 2");
    assert!(has_class(&p, "chat-2-react-eyes", "mine"));
    assert_eq!(attr_of(&p, "chat-2-react-eyes", "name"), "reaction");
    assert_eq!(attr_of(&p, "chat-2-react-eyes", "value"), "eyes");
    let chips = form_of(&p, "chat-2-react-eyes");
    assert_eq!(
        p.attr(chips, "action"),
        Some("/channels/eng/messages/chat-2/reactions")
    );
    assert_eq!(p.attr(chips, "method"), Some("post"));
    assert!(!has_class(&p, "chat-14-react-eyes", "mine"));
    // Threads collapse to a link row.
    assert_eq!(text_of(&p, "chat-1-replies"), "3 replies");
    assert_eq!(
        attr_of(&p, "chat-1-replies", "href"),
        "/channels/eng?thread=chat-1"
    );
    assert!(has(&p, "chat-1-thread-avatar-bob") && has(&p, "chat-1-last-reply"));
    assert!(!has(&p, "chat-1-reply-body") && !has(&p, "chat-3-text"));
    // Every message carries the toolbar the pointer brings up; the last one's shows.
    for id in ["chat-14", "chat-15"] {
        assert_eq!(attr_of(&p, &format!("{id}-quick-+1"), "value"), "+1");
        let tools = form_of(&p, &format!("{id}-quick-+1"));
        assert_eq!(
            p.attr(tools, "action").unwrap(),
            format!("/channels/eng/messages/{id}/reactions")
        );
        assert_eq!(
            attr_of(&p, &format!("{id}-open-thread"), "href"),
            format!("/channels/eng?thread={id}")
        );
        assert_eq!(
            attr_of(&p, &format!("{id}-pin"), "formaction"),
            format!("/channels/eng/messages/{id}/pin")
        );
        // The emoji picker and the overflow menu are gone; the quick reactions,
        // the thread link and the pin are what is left, and all three act.
        assert!(!has(&p, &format!("{id}-add-reaction")) && !has(&p, &format!("{id}-more")));
    }
    assert!(has_class(&p, "chat-15-row", "last") && !has_class(&p, "chat-14-row", "last"));
    // Bob is mentioned nowhere in #eng, but is in the release channel.
    assert!(!has_class(&p, "chat-15-row", "mentioned"));
    let release = page(&get(&mut state, "bob", "/channels/atlas-release").1);
    assert!(has_class(&release, "chat-35-row", "mentioned"));
    // Slack permalinks in seeded prose are /archives/<channel>.
    assert_eq!(get(&mut state, "bob", "/archives/eng").1, body);
}

#[test]
fn threads_open_in_a_pane_and_replies_land_back_in_it() {
    let mut state = seeded();
    let p = page(&get(&mut state, "carol", "/channels/eng?thread=chat-1").1);
    assert!(has(&p, "thread-pane"));
    assert_eq!(attr_of(&p, "thread-close", "href"), "/channels/eng");
    assert!(has(&p, "chat-3-text") && has(&p, "chat-17-text"));
    assert!(has(&p, "thread-chat-1-text") && has(&p, "chat-1-text"));
    assert_eq!(text_of(&p, "thread-count-text"), "3 replies");
    assert_eq!(tag_of(&p, "chat-1-reply"), "form");
    assert_eq!(
        attr_of(&p, "chat-1-reply", "action"),
        "/channels/eng/messages"
    );
    assert_eq!(attr_of(&p, "chat-1-reply", "method"), "post");
    assert_eq!(
        hidden_fields(&p, p.by_id("chat-1-reply")[0]),
        [("parent".to_owned(), "chat-1".to_owned())]
    );
    assert_eq!(attr_of(&p, "chat-1-reply-body", "name"), "text");
    assert_eq!(
        form_of(&p, "chat-1-reply-submit"),
        p.by_id("chat-1-reply")[0]
    );
    // An unknown thread is the plain conversation.
    assert!(!has(
        &page(&get(&mut state, "carol", "/channels/eng?thread=chat-999").1),
        "thread-pane"
    ));
    // The reply arrives the way the browser submits the form.
    let (status, threaded) = post_form(
        &mut state,
        "carol",
        "/channels/eng/messages",
        &[
            ("parent", "chat-1"),
            ("text", "Accepted answer is the hash order."),
        ],
    );
    assert_eq!(status, 200);
    let p = page(&threaded);
    assert!(has(&p, "thread-pane") && threaded.contains("Accepted answer"));
    assert_eq!(text_of(&p, "chat-1-replies"), "4 replies");
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
    let p = page(&members);
    assert_eq!(text_of(&p, "member-alice-title"), "Software Engineer");
    assert!(has(&p, "member-carol-status"));
    assert_eq!(text_of(&p, "members-label"), "Members · 4");
    assert_eq!(attr_of(&p, "members-close", "href"), "/channels/eng");
    assert!(!get(&mut state, "alice", "/channels/eng")
        .1
        .contains("member-carol-status"));
    let dm = get(&mut state, "alice", "/channels/alice|bob").1;
    let p = page(&dm);
    let title = p
        .descendants(Dom::ROOT)
        .find(|n| p.is(*n, "title"))
        .unwrap();
    assert_eq!(p.text_content(title), "bob (DM) - Northstar - Slack");
    assert!(has(&p, "channel-presence") && has(&p, "channel-avatar"));
    assert!(text_of(&p, "channel-status").contains("Windows CI"));
    assert!(has(&p, "chat-40-author") && has(&p, "chat-41-author"));
    assert!(dm.contains("🎉"));
    // A sent message is stamped on Slack's clock and lands in the conversation.
    let (status, sent) = post_form(
        &mut state,
        "alice",
        "/channels/alice|bob/messages",
        &[("text", "see you at standup")],
    );
    assert_eq!(status, 200);
    assert!(page(&sent)
        .descendants(Dom::ROOT)
        .any(|n| page(&sent).has_class(n, "text")
            && page(&sent).text_content(n) == "see you at standup"));
    let last = state["dms"]["alice|bob"]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["time"], 60 + cw_service_slack::HISTORY);
    // Text is escaped on its way into the page.
    post_form(
        &mut state,
        "alice",
        "/channels/alice|bob/messages",
        &[("text", "<script>alert(1)</script> & co")],
    );
    let escaped = get(&mut state, "bob", "/channels/alice|bob").1;
    assert!(escaped.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; co"));
    page(&escaped);
}

#[test]
fn unread_counts_and_mentions_are_per_person() {
    let mut state = seeded();
    // Nobody has opened anything: everything seeded is unread for everyone.
    let unread: Value = serde_json::from_str(&get(&mut state, "bob", "/api/unread").1).unwrap();
    assert_eq!(unread["eng"], 17);
    let banner = page(&get(&mut state, "bob", "/channels/eng").1);
    assert_eq!(text_of(&banner, "read-count"), "17 new messages");
    assert_eq!(attr_of(&banner, "read", "action"), "/channels/eng/read");
    assert_eq!(attr_of(&banner, "read", "method"), "post");
    assert_eq!(form_of(&banner, "read-submit"), banner.by_id("read")[0]);
    // The button's form posts no fields and lands back on the channel, read.
    let (status, read) = post_form(&mut state, "bob", "/channels/eng/read", &[]);
    assert_eq!(status, 200);
    assert!(!has(&page(&read), "read-submit"));
    let unread: Value = serde_json::from_str(&get(&mut state, "bob", "/api/unread").1).unwrap();
    assert!(unread.get("eng").is_none());
    assert!(!has(
        &page(&get(&mut state, "bob", "/channels/eng").1),
        "read-submit"
    ));
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
    assert_eq!(text_of(&page(&sidebar), "nav-eng-unread"), "1");
    assert_eq!(text_of(&page(&sidebar), "rail-activity-count"), "2");
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
    assert_eq!(
        text_of(&page(&get(&mut state, "alice", "/").1), "my-status"),
        "🍜 lunch"
    );
    // The browser posts the status to `/status` and lands on a page of its own.
    let (status, landed) = post_form(&mut state, "alice", "/status", &[("status", "🚀 shipping")]);
    assert_eq!(status, 200);
    assert_eq!(text_of(&page(&landed), "my-status"), "🚀 shipping");
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
    let home = page(&get(&mut state, "admin", "/").1);
    assert_eq!(tag_of(&home, "start-bob"), "button");
    let start = form_of(&home, "start-bob");
    assert_eq!(home.attr(start, "action"), Some("/dms"));
    assert_eq!(home.attr(start, "method"), Some("post"));
    assert_eq!(
        hidden_fields(&home, start),
        [("to".to_owned(), "bob".to_owned())]
    );
    let (status, opened) = post_form(&mut state, "admin", "/dms", &[("to", "bob")]);
    assert_eq!(status, 200);
    let opened = page(&opened);
    let title = opened
        .descendants(Dom::ROOT)
        .find(|n| opened.is(*n, "title"))
        .unwrap();
    assert_eq!(opened.text_content(title), "bob (DM) - Northstar - Slack");
    // Pinning from the toolbar and reacting from a chip are form posts too.
    let (status, pinned) = post_form(
        &mut state,
        "alice",
        "/channels/eng/messages/chat-8/pin",
        &[],
    );
    assert_eq!(status, 200);
    assert!(has(&page(&pinned), "chat-8-pinned"));
    assert!(state["channels"]["eng"]["pins"]
        .as_array()
        .unwrap()
        .contains(&json!("chat-8")));
    let (status, reacted) = post_form(
        &mut state,
        "alice",
        "/channels/eng/messages/chat-8/reactions",
        &[("reaction", "tada")],
    );
    assert_eq!(status, 200);
    assert!(has_class(&page(&reacted), "chat-8-react-tada", "mine"));
}

/// The two pages the top bar and the rail lead to: what the search box finds, and the
/// unread mentions the Activity tab counts.
#[test]
fn search_and_activity_answer_the_controls_that_lead_to_them() {
    let mut state = seeded();
    // The search box carries its query back into the field it was typed in.
    let found = page(&get(&mut state, "bob", "/search?q=windows").1);
    assert_eq!(attr_of(&found, "search-q", "value"), "windows");
    assert!(text_of(&found, "search-summary").ends_with("for \u{201c}windows\u{201d}"));
    // Every hit links to the conversation it is in, and says where it is from.
    assert_eq!(text_of(&found, "hit-0-where"), "#eng");
    assert_eq!(attr_of(&found, "hit-0", "href"), "/channels/eng");
    assert!(text_of(&found, "hit-0-text")
        .to_lowercase()
        .contains("windows"));
    // Nothing matching says so, in prose, rather than showing an empty list.
    let none = page(&get(&mut state, "bob", "/search?q=zzzznothing").1);
    assert_eq!(
        text_of(&none, "search-empty"),
        "No message here says \u{201c}zzzznothing\u{201d}."
    );
    assert!(!has(&none, "hit-0"));
    // An empty query is the box itself, not a claim about results.
    let empty = page(&get(&mut state, "bob", "/search").1);
    assert_eq!(
        text_of(&empty, "search-summary"),
        "Type in the box above to search this workspace"
    );
    // Search only reaches the conversations the caller is in: admin is not in the
    // private release channel, so what was said there is not among their hits.
    let private = "Windows laptop for the install run";
    assert!(get(&mut state, "alice", "/search?q=windows")
        .1
        .contains(private));
    assert!(!get(&mut state, "admin", "/search?q=windows")
        .1
        .contains(private));
    // Activity: alice's two unread mentions, counted the way the rail badges them,
    // newest first; bob's single one reads as one, not "1 mentions".
    let activity = page(&get(&mut state, "alice", "/activity").1);
    assert_eq!(
        text_of(&activity, "activity-summary"),
        "2 mentions waiting for you"
    );
    assert_eq!(text_of(&activity, "rail-activity-count"), "2");
    assert_eq!(text_of(&activity, "mention-0-where"), "#atlas-release");
    assert_eq!(
        attr_of(&activity, "mention-0", "href"),
        "/channels/atlas-release"
    );
    assert!(has(&activity, "mention-1") && !has(&activity, "mention-2"));
    assert_eq!(
        text_of(
            &page(&get(&mut state, "bob", "/activity").1),
            "activity-summary"
        ),
        "1 mention waiting for you"
    );
    // Carol is mentioned nowhere, and the page says that rather than lying with a list.
    let quiet = page(&get(&mut state, "carol", "/activity").1);
    assert_eq!(text_of(&quiet, "activity-summary"), "Nothing new");
    assert!(has(&quiet, "activity-empty") && !has(&quiet, "mention-0"));
}
