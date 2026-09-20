//! The shipped Discord seed and the server's behaviour: categories, roles, voice and the
//! short channel URLs the scenes and seeded prose use.
use cw_protocol::HttpRequest;
use cw_service_common::html::validate_strict;
use cw_web::dom::{Document, NodeId};
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
/// A page fetched, checked by the strict validator and parsed.
struct Page {
    doc: Document,
    html: String,
}
fn parsed(path: &str, html: String) -> Page {
    assert!(html.starts_with("<!DOCTYPE html>"), "{path}: not HTML");
    validate_strict(&html).unwrap_or_else(|e| panic!("{path}: {e:?}"));
    Page {
        doc: cw_web::html::parse(&html),
        html,
    }
}
fn page(state: &mut Value, actor: &str, path: &str) -> Page {
    let (status, body) = get(state, actor, path);
    assert_eq!(status, 200, "{path}");
    parsed(path, body)
}
impl Page {
    fn find(&self, id: &str) -> Option<NodeId> {
        self.doc.by_id(id).first().copied()
    }
    fn has(&self, id: &str) -> bool {
        self.find(id).is_some()
    }
    fn node(&self, id: &str) -> NodeId {
        self.find(id).unwrap_or_else(|| panic!("no #{id}"))
    }
    fn text(&self, id: &str) -> String {
        self.doc.text_content(self.node(id))
    }
    fn attr(&self, id: &str, name: &str) -> Option<&str> {
        self.doc.attr(self.node(id), name)
    }
    fn class(&self, id: &str, class: &str) -> bool {
        self.doc.has_class(self.node(id), class)
    }
    fn title(&self) -> String {
        let title = self
            .doc
            .descendants(Document::ROOT)
            .find(|n| self.doc.is(*n, "title"))
            .expect("title");
        self.doc.text_content(title)
    }
    /// The form an element belongs to: its action and method.
    fn form_of(&self, id: &str) -> (String, String) {
        let node = self.node(id);
        let form = std::iter::once(node)
            .chain(self.doc.ancestors(node))
            .find(|n| self.doc.is(*n, "form"))
            .unwrap_or_else(|| panic!("#{id} is in no form"));
        (
            self.doc.attr(form, "action").unwrap_or_default().to_owned(),
            self.doc.attr(form, "method").unwrap_or_default().to_owned(),
        )
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
        page(&mut state, "alice", path);
    }
    // Every channel of the seed, with the member list open and closed, and as someone
    // who cannot see the moderators' channel.
    let channels: Vec<String> = state["server"]["channels"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    for id in &channels {
        page(&mut state, "alice", &format!("/channels/atlas/{id}"));
        page(&mut state, "alice", &format!("/channels/atlas/{id}?members=0"));
    }
    page(&mut state, "bob", "/");
    page(&mut state, "eve", "/");
    for entry in site["search_entries"].as_array().unwrap() {
        let url = entry["url"].as_str().unwrap();
        let r = DiscordService
            .handle(&mut state, &ctx("alice"), &HttpRequest::get(url))
            .unwrap();
        assert_eq!(r.status, 200, "search entry {url}");
        assert_eq!(
            r.header("content-type"),
            Some("text/html; charset=utf-8"),
            "{url}"
        );
        parsed(url, String::from_utf8(r.body).unwrap());
    }
    // The short and the full URL are one page.
    assert_eq!(
        get(&mut state, "alice", "/channels/atlas-help").1,
        get(&mut state, "alice", "/channels/atlas/atlas-help").1
    );
    let page = get(&mut state, "alice", "/channels/atlas/atlas-help").1;
    assert!(page.contains("INFORMATION") && page.contains("SUPPORT") && page.contains("COMMUNITY"));
    assert!(page.contains("#5865f2") && page.contains("chat-33-react-eyes"));
    assert!(page.contains("class=\"discord\""));
    assert!(page.contains("voice-Lounge") && page.contains("voice-Lounge-praman"));
}

#[test]
fn the_channel_looks_like_discord() {
    let mut state = seeded();
    let p = page(&mut state, "alice", "/channels/atlas/atlas-help");
    assert_eq!(p.title(), "#atlas-help · Atlas Community");
    // The shell: rail, sidebar, header, the inner scrolling transcript, composer, members.
    for id in ["app", "rail", "sidebar", "header", "shell", "main", "composer", "members"] {
        assert!(p.has(id), "{id}");
    }
    assert_eq!(p.attr("rail-home", "href"), Some("/channels/@me"));
    assert_eq!(p.attr("rail-server", "href"), Some("/channels/atlas"));
    assert_eq!(p.text("rail-server"), "AC");
    assert!(p.has("rail-add") && p.has("rail-explore") && p.has("rail-rule"));
    assert_eq!(p.text("server-name"), "Atlas Community");
    // The header: hash, name, topic and the toolbar with its search box.
    assert_eq!(p.text("channel-title"), "atlas-help");
    assert!(p.has("channel-hash") && p.has("search-text") && p.has("channel-topic"));
    assert!(p.has("channel-threads") && p.has("channel-pins") && p.has("channel-inbox"));
    assert!(!p.has("channel-private"));
    // Messages are stamped from the civil clock and grouped under date dividers.
    assert_eq!(p.text("day-5-label"), "September 15, 2026");
    assert_eq!(p.text("day-7-label"), "September 17, 2026");
    assert_eq!(p.text("chat-33-time"), "09/15/2026 3:12 PM");
    assert_eq!(p.text("chat-45-time"), "Today at 8:31 AM");
    assert!(p.has("chat-45-avatar") && p.has("chat-45-author"));
    // Every message carries the hover toolbar; the sheet shows it under the pointer,
    // and on the newest message always.
    assert!(p.has("chat-45-tools") && p.has("chat-44-tools") && p.has("chat-45-reply"));
    assert!(p.class("chat-45-row", "newest") && !p.class("chat-44-row", "newest"));
    // A reply shows Discord's reply line: the quoted author's face, name and text.
    assert_eq!(p.text("chat-34-quote-author"), "mkowalski");
    assert!(p.has("chat-34-quote-avatar") && p.has("chat-34-quote-spine"));
    assert!(p.text("chat-34-quote-text").starts_with("My replay diverges"));
    // Names take their highest role's colour.
    assert_eq!(p.attr("chat-34-author", "style"), Some("color: #3ba55d"));
    // Reactions are pill chips with the emoji itself, each a submit button of the
    // message's reactions form; the actor's own is outlined blurple.
    assert_eq!(p.text("chat-34-react-+1"), "👍 2");
    assert!(p.class("chat-34-react-+1", "mine"));
    assert_eq!(p.attr("chat-34-react-+1", "name"), Some("reaction"));
    assert_eq!(p.attr("chat-34-react-+1", "value"), Some("+1"));
    assert_eq!(
        p.form_of("chat-34-react-+1"),
        (
            "/channels/atlas/atlas-help/messages/chat-34/reactions".to_owned(),
            "post".to_owned()
        )
    );
    assert_eq!(p.attr("chat-34-reactions", "action"), Some("/channels/atlas/atlas-help/messages/chat-34/reactions"));
    assert_eq!(p.text("chat-33-react-eyes"), "👀 1");
    assert!(!p.class("chat-33-react-eyes", "mine") && p.has("chat-33-react-add"));
    // Emoji short names in text are the characters they name; text wraps on the page,
    // so a message is one `-text` block whatever its length; no reply form per message.
    assert!(p.text("chat-45-text").contains("📌"));
    assert!(p.has("chat-45-text-p0") && !p.has("chat-45-text-1"));
    assert!(!p.html.contains(":pushpin:") && !p.html.contains("chat-45-reply-text"));
    // The composer posts `text` to the channel; the user panel sits under the sidebar.
    assert_eq!(
        p.form_of("send-text"),
        ("/channels/atlas/atlas-help/messages".to_owned(), "post".to_owned())
    );
    assert_eq!(p.attr("send", "action"), Some("/channels/atlas/atlas-help/messages"));
    assert_eq!(p.attr("send-text", "name"), Some("text"));
    assert_eq!(p.attr("send-text", "aria-label"), Some("Message #atlas-help"));
    assert_eq!(p.doc.tag(p.node("send-submit")), Some("button"));
    assert_eq!(p.form_of("send-submit").0, "/channels/atlas/atlas-help/messages");
    assert!(p.has("send-attach") && p.has("send-gif") && p.has("send-emoji"));
    assert_eq!(p.text("me-name"), "alice.chen");
    assert!(p.has("me-mic") && p.has("me-settings") && p.has("me-avatar"));
    assert!(p.class("me-presence", "on"));
    // The member list: hoisted roles, then online and offline, with presence.
    assert_eq!(p.text("group-Admin"), "ADMIN — 1");
    assert_eq!(p.text("group-Mod"), "MOD — 2");
    assert_eq!(p.text("group-online"), "ONLINE — 2");
    assert_eq!(p.text("group-offline"), "OFFLINE — 2");
    assert!(!p.has("group-Member"));
    assert!(p.class("member-carol-presence", "off") && p.class("member-carol", "away"));
    assert!(p.class("member-bob-presence", "on") && p.has("member-bob-avatar"));
    assert_eq!(p.attr("member-admin-name", "style"), Some("color: #f23f43"));
    assert_eq!(p.attr("member-carol-name", "style"), None);
    // The sidebar: categories, the open channel highlighted, voice occupants with faces.
    assert_eq!(p.text("category-1-text"), "SUPPORT");
    assert!(p.class("nav-atlas-help", "current") && !p.class("nav-general", "current"));
    assert_eq!(p.attr("nav-general", "href"), Some("/channels/atlas/general"));
    assert!(p.has("nav-general-hash") && p.has("nav-mod-log"));
    assert!(p.has("voice-Lounge-jlee-avatar") && p.has("voice-Lounge-icon"));
    assert_eq!(p.text("voice-Lounge-praman-name"), "priya");
    // A voice channel is a one-button form: joining, or leaving while inside.
    assert_eq!(p.doc.tag(p.node("voice-Lounge")), Some("button"));
    assert_eq!(p.form_of("voice-Lounge"), ("/voice/Lounge/join".to_owned(), "post".to_owned()));
    assert_eq!(p.form_of("voice-Office Hours").0, "/voice/Office%20Hours/join");
    // `?members=0` closes the member list and the header button reopens it.
    assert_eq!(
        p.attr("channel-members", "href"),
        Some("/channels/atlas/atlas-help?members=0")
    );
    let closed = page(&mut state, "alice", "/channels/atlas/atlas-help?members=0");
    assert!(!closed.has("members") && closed.has("composer"));
    assert_eq!(closed.attr("channel-members", "href"), Some("/channels/atlas/atlas-help"));
    // A role-gated channel shows its lock in the header.
    assert!(page(&mut state, "alice", "/channels/atlas/mod-log").has("channel-private"));
    // priya's second message two minutes on shares her header: no face, no name.
    let general = page(&mut state, "alice", "/channels/atlas/general");
    assert!(general.has("chat-58-avatar") && general.has("chat-58-author"));
    assert!(!general.has("chat-57-avatar") && !general.has("chat-57-author"));
    assert!(general.has("chat-57-time"));
    assert!(general.text("chat-57-text-p0").ends_with("🙏"));
    // Channel mentions in text are links to the channel; URLs are links out.
    let welcome = page(&mut state, "alice", "/channels/atlas/welcome");
    let links: Vec<(String, String)> = welcome
        .doc
        .descendants(Document::ROOT)
        .filter(|n| welcome.doc.is(*n, "a"))
        .filter(|n| welcome.doc.attr(*n, "id").is_some_and(|id| id.contains("-text-p")))
        .map(|n| {
            (
                welcome.doc.text_content(n),
                welcome.doc.attr(n, "href").unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert!(
        links.iter().any(|(text, href)| text.starts_with('#') && href.starts_with("/channels/atlas/")),
        "{links:?}"
    );
    // The root is the server with no channel open.
    let home = page(&mut state, "alice", "/");
    assert_eq!(home.title(), "Atlas Community");
    assert_eq!(home.text("empty"), "Pick a channel.");
    assert!(!home.has("composer") && home.has("members") && home.has("nav-welcome"));
    // Clicking a reaction chip toggles the actor's own reaction.
    let (status, after) = post(
        &mut state,
        "alice",
        "/channels/atlas-help/messages/chat-33/reactions",
        json!({"reaction":"eyes"}),
    );
    assert_eq!(status, 200);
    let after = parsed("reaction", after);
    assert_eq!(after.text("chat-33-react-eyes"), "👀 2");
    assert!(after.class("chat-33-react-eyes", "mine"));
}

/// The browser posts forms url-encoded; the composer and the voice rows work that way.
#[test]
fn browser_forms_post_urlencoded() {
    let mut state = seeded();
    let mut request = HttpRequest::get("http://discord.com/channels/atlas/general/messages");
    request.method = "POST".into();
    request.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    request.body = b"text=hello+from+the+form+%3Cb%3E".to_vec();
    let r = DiscordService
        .handle(&mut state, &ctx("bob"), &request)
        .unwrap();
    assert_eq!(r.status, 200);
    let p = parsed("send", String::from_utf8(r.body).unwrap());
    assert_eq!(p.title(), "#general · Atlas Community");
    assert!(p.html.contains("hello from the form &lt;b&gt;"));
    let mut join = HttpRequest::get("http://discord.com/voice/Office%20Hours/join");
    join.method = "POST".into();
    let r = DiscordService.handle(&mut state, &ctx("bob"), &join).unwrap();
    assert_eq!(r.status, 200);
    let p = parsed("join", String::from_utf8(r.body).unwrap());
    assert!(p.has("voice-Office Hours-bob") && p.class("voice-Office Hours", "current"));
    assert_eq!(p.form_of("voice-Office Hours").0, "/voice/Office%20Hours/leave");
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
    let (status, body) = post(&mut state, "bob", "/voice/Lounge/leave", json!({}));
    assert_eq!(status, 200);
    assert!(!body.contains("voice-Lounge-bob"));
    // The page a leave lands on is a real page too.
    let left = parsed("/voice/Lounge/leave", body);
    assert!(!left.has("voice-Lounge-bob") && left.has("voice-Lounge"));
}
