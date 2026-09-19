//! The Slack workspace page, laid out the way Slack's desktop web client is in 2024:
//! an aubergine frame holding a top bar with the search box, a narrow rail of
//! workspace tabs, the sidebar of channels and direct messages, the open conversation
//! with its messages grouped by author under date dividers, a composer pinned to the
//! bottom, and, on the right, the thread or the member list the query string opens.
//! The whole rendering lives here, apart from the state and the routes.
use crate::{time, Channel, Message, SlackState};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result as SimResult};
use cw_service_common as web;

/// Slack's palette and vocabulary.
pub struct Look {
    /// The aubergine frame, top bar and rail.
    pub frame: &'static str,
    pub sidebar: &'static str,
    pub sidebar_ink: &'static str,
    /// The blue of the selected sidebar row.
    pub selected: &'static str,
    pub surface: &'static str,
    pub ink: &'static str,
    pub muted: &'static str,
    pub accent: &'static str,
    pub line: &'static str,
    /// The tint behind a mention, and behind the actor's own reactions.
    pub tint: &'static str,
    /// The green of the send button and of presence.
    pub green: &'static str,
    /// The tint behind a message that mentions the actor.
    pub highlight: &'static str,
    pub brand: &'static str,
}
pub const SLACK: Look = Look {
    frame: "#350d36",
    sidebar: "#3f0e40",
    sidebar_ink: "#cfc3cf",
    selected: "#1164a3",
    surface: "#ffffff",
    ink: "#1d1c1d",
    muted: "#616061",
    accent: "#1264a3",
    line: "#dddddd",
    tint: "#e8f5fa",
    green: "#007a5a",
    highlight: "#fff8e1",
    brand: "Slack",
};
/// What the query string opens beside the conversation, and when it is.
#[derive(Clone, Debug, Default)]
pub struct View {
    /// Now, on Slack's clock (the world tick plus `crate::HISTORY`).
    pub tick: u64,
    /// `?thread=<message id>`: that message's thread in the right-hand pane.
    pub thread: Option<String>,
    /// `?members=1`: the conversation's member list in the right-hand pane.
    pub members: bool,
}
const RAIL: u32 = 64;
const SIDEBAR: u32 = 260;
/// The browser's page margin, which the aubergine frame fills.
const MARGIN: u32 = 16;
const PANE: u32 = 400;
const AVATAR: u32 = 36;
/// Messages by one author this close together share one header, as Slack groups them.
const GROUP_US: u64 = 10 * time::MINUTE_US;
/// Characters of message text per row, the wrap the page itself cannot do: a row lays
/// its pieces out on one line, so a message is cut into rows this long at word
/// boundaries. Sized for a 1280-wide window: the conversation alone, the conversation
/// beside a pane, and the pane itself.
const WIDE_CHARS: usize = 100;
const NARROW_CHARS: usize = 56;
const PANE_CHARS: usize = 42;

/// Slack's short names for the emoji the seed and the quick reactions use.
const EMOJI: &[(&str, &str)] = &[
    ("+1", "👍"),
    ("tada", "🎉"),
    ("eyes", "👀"),
    ("heart", "❤️"),
    ("rocket", "🚀"),
    ("white_check_mark", "✅"),
    ("thinking_face", "🤔"),
    ("fire", "🔥"),
    ("pray", "🙏"),
    ("joy", "😂"),
    ("raised_hands", "🙌"),
    ("wave", "👋"),
    ("100", "💯"),
    ("pushpin", "📌"),
];
/// The emoji a Slack short name stands for, or the `:name:` itself when it is unknown.
pub fn emoji(name: &str) -> String {
    EMOJI
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, e)| (*e).to_owned())
        .unwrap_or_else(|| format!(":{name}:"))
}
/// `:tada:` and friends in running text become the characters they name.
pub fn emojify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end =
            after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-'));
        match end {
            Some(end) if end > 0 && after[end..].starts_with(':') => {
                let name = &after[..end];
                match EMOJI.iter().find(|(n, _)| *n == name) {
                    Some((_, e)) => out.push_str(e),
                    None => {
                        out.push(':');
                        out.push_str(name);
                        out.push(':');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push(':');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn post(url: String, fields: &[(&str, &str)]) -> PageAction {
    PageAction {
        method: "POST".into(),
        url,
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}
fn theme(state: &SlackState, look: &Look) -> PageTheme {
    state.theme.clone().unwrap_or(PageTheme {
        accent: Some(look.accent.into()),
        // The page is the aubergine frame; every white surface names its own colour.
        background: Some(look.frame.into()),
        surface: Some(look.surface.into()),
        ink: Some(look.ink.into()),
        muted: Some(look.muted.into()),
        content_width: Some(4096),
    })
}
/// A person's initials on a rounded square in their own colour, Slack's avatar shape.
fn avatar(id: &str, name: &str, size: u32) -> PageElement {
    let mut tile = web::avatar(id, name, size);
    if let PageElement::Thumbnail { style, .. } = &mut tile {
        style.radius = Some((size / 5).max(3));
    }
    tile
}
/// A block of the frame or the sidebar colour, `width` across, filling a pinned bar
/// under the rail and the sidebar so the columns run the full height of the window.
fn block(id: &str, width: u32, colour: &str, children: Vec<PageElement>) -> PageElement {
    web::card(
        id,
        web::style()
            .width(width)
            .flex(0)
            .padding(0)
            .radius(0)
            .background(colour),
        children,
    )
}
fn glyph(id: &str, name: &str, label: &str, size: u16, colour: &str) -> PageElement {
    web::icon(id, name, label, web::style().size(size).color(colour))
}
/// The other people in a DM key, from this actor's point of view.
pub fn partners(state: &SlackState, key: &str, actor: &str) -> String {
    let others: Vec<String> = key
        .split('|')
        .filter(|who| *who != actor)
        .map(|who| state.display(who))
        .collect();
    if others.is_empty() {
        state.display(actor)
    } else {
        others.join(", ")
    }
}
/// The other people in a DM key, as user names.
fn others<'a>(key: &'a str, actor: &str) -> Vec<&'a str> {
    key.split('|').filter(|who| *who != actor).collect()
}
/// Whether a person reads as active: they have posted in the last three hours, or they
/// are the person looking.
fn active(state: &SlackState, who: &str, actor: &str, now: u64) -> bool {
    who == actor
        || state
            .channels
            .values()
            .chain(state.dms.values())
            .flat_map(|c| &c.messages)
            .filter(|m| m.author == who)
            .any(|m| now.saturating_sub(m.time) < 3 * time::HOUR_US)
}
fn presence(id: &str, on: bool, look: &Look) -> PageElement {
    web::styled(
        id,
        if on { "●" } else { "○" },
        web::style()
            .size(9)
            .one_line()
            .color(if on { "#2bac76" } else { look.sidebar_ink }),
    )
}
/// Now on Slack's clock: the tick, or the newest message the actor can see if the seed
/// runs ahead of the world.
fn now(state: &SlackState, actor: &str, tick: u64) -> u64 {
    state
        .channels
        .values()
        .chain(state.dms.values())
        .filter(|c| c.members.contains(actor))
        .flat_map(|c| &c.messages)
        .map(|m| m.time)
        .fold(tick, u64::max)
}

// ---- the top bar and the rail --------------------------------------------------------

fn topbar(state: &SlackState, actor: &str, look: &Look) -> PageElement {
    let name = if state.workspace.is_empty() {
        look.brand.to_owned()
    } else {
        state.workspace.clone()
    };
    let search = web::card(
        "search",
        web::style()
            .width(560)
            .flex(0)
            .padding(6)
            .radius(6)
            .background("#5c3a5d")
            .border("#7b5c7c"),
        vec![web::styled_row(
            "search-line",
            8,
            "center",
            web::style(),
            vec![
                glyph("search-icon", "search", "Search", 14, "#ffffff"),
                web::styled(
                    "search-text",
                    format!("Search {name}"),
                    web::style().size(13).color("#e8e0e8").one_line(),
                ),
            ],
        )],
    );
    web::styled_row(
        "topbar",
        10,
        "center",
        web::style().padding(6).background(look.frame).pin("top"),
        vec![
            web::styled("topbar-lead", "", web::style().width(80)),
            glyph("nav-back", "chevron-left", "Back", 16, look.sidebar_ink),
            glyph(
                "nav-forward",
                "chevron-right",
                "Forward",
                16,
                look.sidebar_ink,
            ),
            glyph("nav-history", "clock", "History", 16, look.sidebar_ink),
            web::rest("topbar-left"),
            search,
            web::rest("topbar-right"),
            glyph("help", "info", "Help", 16, look.sidebar_ink),
            web::styled(
                "my-status",
                state
                    .members
                    .get(actor)
                    .map(|m| m.status.clone())
                    .unwrap_or_default(),
                web::style().size(12).color(look.sidebar_ink).one_line(),
            ),
            avatar("me-avatar", &state.display(actor), 26),
        ],
    )
}
/// The rail's tabs: Home, DMs, Activity, Later and More, each an icon over its label.
fn rail(state: &SlackState, actor: &str, home: bool, dms: bool, look: &Look) -> PageElement {
    let mentions = state.mentions(actor).len();
    let tab = |id: &str, icon: &str, label: &str, on: bool, url: Option<String>| {
        let style = web::style()
            .size(20)
            .padding(6)
            .radius(8)
            .color("#ffffff")
            .align("center")
            .background(if on { "#5c3a5d" } else { look.frame });
        let mark = match url {
            Some(url) => web::icon_action(id, icon, label, style, web::visit(url)),
            None => web::icon(id, icon, label, style),
        };
        let mut items = vec![mark];
        if id == "rail-activity" && mentions > 0 {
            items.push(web::badge(
                "rail-activity-count",
                mentions.to_string(),
                web::style()
                    .background("#cd2553")
                    .color("#ffffff")
                    .size(10)
                    .padding(2)
                    .align("center"),
            ));
        }
        items.push(web::styled(
            &format!("{id}-label"),
            label,
            web::style()
                .size(10)
                .color("#ffffff")
                .align("center")
                .one_line(),
        ));
        web::column(&format!("{id}-tab"), 2, web::style(), items)
    };
    web::column(
        "rail",
        10,
        web::style()
            .width(RAIL)
            .flex(0)
            .padding(4)
            .background(look.frame),
        vec![
            tab("rail-home", "home", "Home", home, Some("/".into())),
            tab("rail-dms", "chat", "DMs", dms, Some("/dms".into())),
            tab("rail-activity", "bell", "Activity", false, None),
            tab("rail-later", "clock", "Later", false, None),
            tab("rail-more", "more", "More", false, None),
        ],
    )
}

// ---- the sidebar ---------------------------------------------------------------------

fn section(id: &str, label: &str, look: &Look) -> PageElement {
    web::pills(
        id,
        4,
        vec![
            glyph(
                &format!("{id}-chevron"),
                "chevron-down",
                "Collapse",
                10,
                look.sidebar_ink,
            ),
            web::styled(
                &format!("{id}-text"),
                label,
                web::style().size(14).medium().color(look.sidebar_ink),
            ),
        ],
    )
}
/// How a sidebar row stands: open, unread, and the count on its badge (unread mentions
/// for a channel, unread messages for a DM).
#[derive(Clone, Copy, Default)]
struct Standing {
    current: bool,
    unread: bool,
    badge: usize,
}
/// One conversation in the sidebar: its mark, its name (bold while it is unread, with
/// a count), the row highlighted while it is the open one.
fn sidebar_row(
    id: &str,
    mark: PageElement,
    text: String,
    action: PageAction,
    look: &Look,
    standing: Standing,
) -> PageElement {
    let Standing {
        current,
        unread,
        badge,
    } = standing;
    let mut children = vec![
        mark,
        web::styled(
            &format!("{id}-text"),
            text,
            match (current, unread) {
                (true, _) => web::style().size(15).color("#ffffff").one_line().flex(1),
                (false, true) => web::style()
                    .size(15)
                    .bold()
                    .color("#ffffff")
                    .one_line()
                    .flex(1),
                (false, false) => web::style()
                    .size(15)
                    .color(look.sidebar_ink)
                    .one_line()
                    .flex(1),
            },
        ),
    ];
    if badge > 0 {
        children.push(web::badge(
            &format!("{id}-unread"),
            badge.to_string(),
            web::style()
                .background("#cd2553")
                .color("#ffffff")
                .padding(2)
                .size(11),
        ));
    }
    web::card_action(
        id,
        web::style().padding(4).radius(6).background(if current {
            look.selected
        } else {
            look.sidebar
        }),
        action,
        vec![web::styled_row(
            &format!("{id}-line"),
            8,
            "center",
            web::style(),
            children,
        )],
    )
}
fn sidebar(
    state: &SlackState,
    actor: &str,
    open: Option<&str>,
    now: u64,
    look: &Look,
) -> PageElement {
    let mut items = vec![section("channels-label", "Channels", look)];
    let mentions = state.mentions(actor);
    for (id, channel) in &state.channels {
        if !channel.members.contains(actor) {
            continue;
        }
        let current = open == Some(id.as_str());
        let ink = if current { "#ffffff" } else { look.sidebar_ink };
        let mark = if channel.private {
            glyph(
                &format!("nav-{id}-lock"),
                "lock",
                "Private channel",
                14,
                ink,
            )
        } else {
            glyph(&format!("nav-{id}-hash"), "hash", "Channel", 14, ink)
        };
        items.push(sidebar_row(
            &format!("nav-{id}"),
            mark,
            id.clone(),
            web::visit(format!("/channels/{id}")),
            look,
            // A channel is bold while it is unread and badged with its unread mentions;
            // a DM is badged with every unread message.
            Standing {
                current,
                unread: state.unread(actor, id) > 0,
                badge: mentions.iter().filter(|(at, _)| *at == id).count(),
            },
        ));
    }
    items.push(web::pills(
        "add-channel",
        8,
        vec![
            glyph(
                "add-channel-icon",
                "plus",
                "Add channels",
                14,
                look.sidebar_ink,
            ),
            web::styled(
                "add-channel-text",
                "Add channels",
                web::style().size(14).color(look.sidebar_ink).one_line(),
            ),
        ],
    ));
    items.push(web::spacer("sidebar-gap", 6));
    items.push(section("dms-label", "Direct messages", look));
    let mut listed = std::collections::BTreeSet::new();
    for (key, dm) in &state.dms {
        if !dm.members.contains(actor) {
            continue;
        }
        let people = others(key, actor);
        let current = open == Some(key.as_str());
        let mark = match people.as_slice() {
            [one] => {
                listed.insert((*one).to_owned());
                web::styled_row(
                    &format!("dm-{key}-mark"),
                    3,
                    "center",
                    web::style(),
                    vec![
                        avatar(&format!("dm-{key}-avatar"), &state.display(one), 20),
                        presence(
                            &format!("dm-{key}-presence"),
                            active(state, one, actor, now),
                            look,
                        ),
                    ],
                )
            }
            _ => web::thumbnail(
                &format!("dm-{key}-avatar"),
                people.len().to_string(),
                web::style()
                    .width(20)
                    .height(20)
                    .radius(4)
                    .size(9)
                    .background("#5c3a5d")
                    .border(look.sidebar_ink)
                    .color("#ffffff")
                    .align("center"),
            ),
        };
        items.push(sidebar_row(
            &format!("dm-{key}"),
            mark,
            partners(state, key, actor),
            web::visit(format!("/channels/{key}")),
            look,
            Standing {
                current,
                unread: state.unread(actor, key) > 0,
                badge: state.unread(actor, key),
            },
        ));
    }
    // Everyone else is one click from a conversation, as Slack lists them under the
    // DMs you already have; opening one is a POST that lands on it.
    let rest: Vec<String> = state
        .people()
        .filter(|who| *who != actor && !listed.contains(*who))
        .map(str::to_owned)
        .collect();
    for who in rest {
        let mark = web::styled_row(
            &format!("start-{who}-mark"),
            3,
            "center",
            web::style(),
            vec![
                avatar(&format!("start-{who}-avatar"), &state.display(&who), 20),
                presence(
                    &format!("start-{who}-presence"),
                    active(state, &who, actor, now),
                    look,
                ),
            ],
        );
        items.push(sidebar_row(
            &format!("start-{who}"),
            mark,
            state.display(&who),
            post("/dms".into(), &[("to", &who)]),
            look,
            Standing::default(),
        ));
    }
    items.push(web::spacer("sidebar-gap-2", 6));
    items.push(section("apps-label", "Apps", look));
    items.push(web::pills(
        "add-apps",
        8,
        vec![
            glyph("add-apps-icon", "plus", "Add apps", 14, look.sidebar_ink),
            web::styled(
                "add-apps-text",
                "Add apps",
                web::style().size(14).color(look.sidebar_ink).one_line(),
            ),
        ],
    ));
    web::column(
        "sidebar",
        0,
        web::style()
            .background(look.sidebar)
            .padding(8)
            .width(SIDEBAR)
            .flex(0),
        items,
    )
}

// ---- messages ------------------------------------------------------------------------

/// Everything a message block needs to know about where it is drawn.
struct Ctx<'a> {
    state: &'a SlackState,
    id: &'a str,
    channel: &'a Channel,
    actor: &'a str,
    now: u64,
    look: &'a Look,
    /// Characters of message text per row, see `WIDE_CHARS`.
    budget: usize,
}
impl Ctx<'_> {
    fn heading(&self) -> String {
        if self.state.channels.contains_key(self.id) {
            format!("# {}", self.id)
        } else {
            partners(self.state, self.id, self.actor)
        }
    }
    fn url(&self) -> String {
        format!("/channels/{}", self.id)
    }
}
/// A line of a message as Slack sets it: plain runs, mentions on their tint, links in
/// blue and code in the monospace face, each at its own width.
fn inline_line(id: &str, line: &str, mentions: &[&str], look: &Look) -> PageElement {
    let words: Vec<&str> = line.split(' ').filter(|w| !w.is_empty()).collect();
    let mut pieces = vec![];
    let mut run: Vec<String> = vec![];
    let flush = |run: &mut Vec<String>, pieces: &mut Vec<PageElement>| {
        if !run.is_empty() {
            pieces.push(web::styled(
                &format!("{id}-p{}", pieces.len()),
                emojify(&run.join(" ")),
                web::style().size(15).color(look.ink).one_line(),
            ));
            run.clear();
        }
    };
    /// A word and the punctuation that closes it, which stays plain text.
    fn trailing(w: &str) -> (&str, &str) {
        let end = w.trim_end_matches(['.', ',', ';', ':', ')', ']', '!', '?']);
        (end, &w[end.len()..])
    }
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if word.starts_with("http://") || word.starts_with("https://") {
            let (url, tail) = trailing(word);
            flush(&mut run, &mut pieces);
            let shown = url
                .trim_start_matches("http://")
                .trim_start_matches("https://");
            pieces.push(web::inline_link(
                &format!("{id}-p{}", pieces.len()),
                shown,
                url,
                15,
                look.accent,
            ));
            if !tail.is_empty() {
                run.push(tail.into());
            }
        } else if let Some(name) = word
            .strip_prefix('@')
            .filter(|name| mentions.iter().any(|m| name.starts_with(m)))
        {
            let (name, tail) = trailing(name);
            flush(&mut run, &mut pieces);
            pieces.push(web::styled(
                &format!("{id}-p{}", pieces.len()),
                format!("@{name}"),
                web::style()
                    .size(15)
                    .color(look.accent)
                    .background(look.tint)
                    .radius(3)
                    .padding(2)
                    .one_line(),
            ));
            if !tail.is_empty() {
                run.push(tail.into());
            }
        } else if word.starts_with('`') && word.len() > 1 {
            // A code span runs to the word that closes it.
            let mut end = i;
            while end < words.len()
                && !(words[end].ends_with('`') && (end > i || words[end].len() > 1))
            {
                end += 1;
            }
            let end = end.min(words.len() - 1);
            let code = words[i..=end].join(" ");
            let code = code.trim_matches('`');
            flush(&mut run, &mut pieces);
            pieces.push(web::styled(
                &format!("{id}-p{}", pieces.len()),
                code,
                web::style()
                    .size(13)
                    .mono()
                    .color("#e01e5a")
                    .background("#f8f8f8")
                    .border("#e0e0e0")
                    .radius(3)
                    .padding(2)
                    .one_line(),
            ));
            i = end;
        } else {
            run.push(word.into());
        }
        i += 1;
    }
    flush(&mut run, &mut pieces);
    web::pills(id, 4, pieces)
}
/// A line cut into rows of at most `budget` characters at word boundaries, a code span
/// never split; a single word longer than the budget (a long URL) is a row of its own.
fn chunks(line: &str, budget: usize) -> Vec<String> {
    let words: Vec<&str> = line.split(' ').filter(|w| !w.is_empty()).collect();
    let mut tokens: Vec<String> = vec![];
    let mut i = 0;
    while i < words.len() {
        let mut token = words[i].to_owned();
        if words[i].starts_with('`') && !(words[i].len() > 1 && words[i].ends_with('`')) {
            while i + 1 < words.len() && !words[i].ends_with('`') {
                i += 1;
                token.push(' ');
                token.push_str(words[i]);
            }
        }
        tokens.push(token);
        i += 1;
    }
    let mut rows: Vec<String> = vec![];
    for token in tokens {
        match rows.last_mut() {
            Some(row) if row.chars().count() + 1 + token.chars().count() <= budget => {
                row.push(' ');
                row.push_str(&token);
            }
            _ => rows.push(token),
        }
    }
    rows
}
/// A message's text: every line cut to the budget, each row set inline.
fn body(id: &str, text: &str, mentions: &[&str], look: &Look, budget: usize) -> Vec<PageElement> {
    let mut out = vec![];
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        for row in chunks(line, budget) {
            let row_id = if out.is_empty() {
                format!("{id}-text")
            } else {
                format!("{id}-text-{}", out.len())
            };
            out.push(inline_line(&row_id, &row, mentions, look));
        }
    }
    out
}
fn reactions(ctx: &Ctx, m: &Message) -> Option<PageElement> {
    if m.reactions.is_empty() {
        return None;
    }
    let look = ctx.look;
    let mut chips: Vec<PageElement> = m
        .reactions
        .iter()
        .map(|(name, who)| {
            let mine = who.contains(ctx.actor);
            web::styled_button(
                &format!("{}-react-{name}", m.id),
                format!("{} {}", emoji(name), who.len()),
                post(
                    format!("{}/messages/{}/reactions", ctx.url(), m.id),
                    &[("reaction", name)],
                ),
                web::style()
                    .size(12)
                    .padding(8)
                    .radius(12)
                    .background(if mine { look.tint } else { "#f2f2f2" })
                    .border(if mine { look.accent } else { "#e0e0e0" })
                    .color(if mine { look.accent } else { look.ink }),
            )
        })
        .collect();
    chips.push(web::icon(
        &format!("{}-react-add", m.id),
        "emoji",
        "Add reaction",
        web::style()
            .size(14)
            .padding(5)
            .radius(12)
            .background("#f2f2f2")
            .color(look.muted),
    ));
    Some(web::pills(&format!("{}-reactions", m.id), 4, chips))
}
/// The collapsed thread under a message: the repliers' faces, "N replies" and the age
/// of the last, opening the thread pane.
fn thread_row(ctx: &Ctx, m: &Message, replies: &[&Message]) -> PageElement {
    let mut chips = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for reply in replies {
        if seen.insert(reply.author.as_str()) && seen.len() <= 3 {
            chips.push(avatar(
                &format!("{}-thread-avatar-{}", m.id, reply.author),
                &ctx.state.display(&reply.author),
                20,
            ));
        }
    }
    chips.push(web::styled_link(
        &format!("{}-replies", m.id),
        format!(
            "{} {}",
            replies.len(),
            if replies.len() == 1 {
                "reply"
            } else {
                "replies"
            }
        ),
        format!("{}?thread={}", ctx.url(), m.id),
        web::style()
            .size(13)
            .bold()
            .color(ctx.look.accent)
            .one_line(),
    ));
    if let Some(last) = replies.iter().map(|r| r.time).max() {
        chips.push(web::styled(
            &format!("{}-last-reply", m.id),
            format!("Last reply {}", time::ago(last, ctx.now)),
            web::style().size(12).color(ctx.look.muted).one_line(),
        ));
    }
    web::pills(&format!("{}-thread", m.id), 6, chips)
}
/// The actions Slack floats over the message under the pointer: quick reactions, add
/// reaction, reply in thread, pin and more.
fn toolbar(ctx: &Ctx, m: &Message) -> PageElement {
    let look = ctx.look;
    let quick = |name: &str| {
        web::styled_button(
            &format!("{}-quick-{name}", m.id),
            emoji(name),
            post(
                format!("{}/messages/{}/reactions", ctx.url(), m.id),
                &[("reaction", name)],
            ),
            web::style()
                .size(14)
                .padding(4)
                .radius(4)
                .background(look.surface)
                .color(look.ink),
        )
    };
    let tool = web::style().size(16).padding(4).radius(4).color(look.muted);
    web::styled_row(
        &format!("{}-tools", m.id),
        2,
        "center",
        web::style()
            .padding(2)
            .radius(6)
            .background(look.surface)
            .border(look.line),
        vec![
            quick("white_check_mark"),
            quick("eyes"),
            quick("+1"),
            web::icon(
                &format!("{}-add-reaction", m.id),
                "emoji",
                "Add reaction",
                tool.clone(),
            ),
            web::icon_action(
                &format!("{}-open-thread", m.id),
                "thread",
                "Reply in thread",
                tool.clone(),
                web::visit(format!("{}?thread={}", ctx.url(), m.id)),
            ),
            web::icon_action(
                &format!("{}-pin", m.id),
                "pin",
                if ctx.channel.pins.contains(&m.id) {
                    "Unpin from channel"
                } else {
                    "Pin to channel"
                },
                tool.clone(),
                post(format!("{}/messages/{}/pin", ctx.url(), m.id), &[]),
            ),
            web::icon(&format!("{}-more", m.id), "more", "More actions", tool),
        ],
    )
}
/// One message: the author's avatar, name and time on the first of a run by one
/// author, the time alone in the gutter on the rest.
fn message(
    ctx: &Ctx,
    prefix: &str,
    m: &Message,
    first: bool,
    hovered: bool,
    replies: &[&Message],
) -> PageElement {
    let look = ctx.look;
    let id = &format!("{prefix}{}", m.id);
    let first = first || hovered;
    let stamp = time::civil(m.time).clock();
    let mut inner = vec![];
    if ctx.channel.pins.contains(id) {
        inner.push(web::styled_row(
            &format!("{id}-pinned"),
            4,
            "center",
            web::style(),
            vec![
                glyph(&format!("{id}-pin-icon"), "pin", "Pinned", 11, look.muted),
                web::styled(
                    &format!("{id}-pinned-text"),
                    "Pinned to this channel",
                    web::style().size(11).color(look.muted).one_line(),
                ),
            ],
        ));
    }
    if first {
        let mut head = vec![
            web::styled(
                &format!("{id}-author"),
                ctx.state.display(&m.author),
                web::style().size(15).bold().color(look.ink).one_line(),
            ),
            web::styled(
                &format!("{id}-time"),
                stamp.clone(),
                web::style().size(12).color(look.muted).one_line(),
            ),
            web::rest(&format!("{id}-head-rest")),
        ];
        if hovered {
            head.push(toolbar(ctx, m));
        }
        inner.push(web::styled_row(
            &format!("{id}-head"),
            8,
            "center",
            web::style(),
            head,
        ));
    }
    let mentions = m.mentions();
    inner.extend(body(id, &m.text, &mentions, look, ctx.budget));
    inner.extend(reactions(ctx, m));
    if !replies.is_empty() {
        inner.push(thread_row(ctx, m, replies));
    }
    let gutter = if first {
        avatar(
            &format!("{id}-avatar"),
            &ctx.state.display(&m.author),
            AVATAR,
        )
    } else {
        web::styled(
            &format!("{id}-time"),
            stamp,
            web::style()
                .size(9)
                .color(look.muted)
                .width(AVATAR)
                .one_line()
                .align("center"),
        )
    };
    let mut style = web::style().padding(4);
    if mentions.contains(&ctx.actor) {
        style = style.background(look.highlight);
    }
    web::styled_row(
        &format!("{id}-row"),
        10,
        "start",
        style,
        vec![
            gutter,
            web::column(&format!("{id}-body"), 2, web::style().flex(1), inner),
        ],
    )
}
fn day_divider(index: u64, label: String, look: &Look) -> PageElement {
    let rule = |id: String| PageElement::Divider {
        id,
        style: web::style().flex(1).color(look.line),
    };
    web::styled_row(
        &format!("day-{index}"),
        8,
        "center",
        web::style().padding(6),
        vec![
            rule(format!("day-{index}-left")),
            web::chip(
                &format!("day-{index}-label"),
                label,
                look.surface,
                look.ink,
                web::style().border(look.line).size(12).padding(8),
            ),
            rule(format!("day-{index}-right")),
        ],
    )
}
/// The transcript: top-level messages under date dividers, runs by one author grouped,
/// the last message carrying the hover toolbar.
fn transcript(ctx: &Ctx) -> Vec<PageElement> {
    let tops: Vec<&Message> = ctx
        .channel
        .messages
        .iter()
        .filter(|m| m.parent.is_none())
        .collect();
    let mut out = vec![];
    let mut previous: Option<&Message> = None;
    for (n, m) in tops.iter().enumerate() {
        let day = time::civil(m.time).index;
        let new_day = previous.is_none_or(|p| time::civil(p.time).index != day);
        if new_day {
            out.push(day_divider(day, time::day_label(m.time, ctx.now), ctx.look));
        }
        let first = new_day
            || previous
                .is_none_or(|p| p.author != m.author || m.time.saturating_sub(p.time) > GROUP_US);
        let replies: Vec<&Message> = ctx
            .channel
            .messages
            .iter()
            .filter(|r| r.parent.as_deref() == Some(m.id.as_str()))
            .collect();
        out.push(message(ctx, "", m, first, n + 1 == tops.len(), &replies));
        previous = Some(m);
    }
    out
}

// ---- the header, the composer and the panes ------------------------------------------

/// The channel header: name and topic, the member count opening the member list.
fn header(ctx: &Ctx) -> PageElement {
    let look = ctx.look;
    let heading = ctx.heading();
    let is_channel = ctx.state.channels.contains_key(ctx.id);
    let mut line = vec![];
    if is_channel {
        line.push(web::styled(
            "channel-title",
            &heading,
            web::style().size(18).bold().color(look.ink).one_line(),
        ));
        line.push(glyph(
            "channel-menu",
            "chevron-down",
            "Channel details",
            14,
            look.muted,
        ));
    } else {
        let people = others(ctx.id, ctx.actor);
        if let [one] = people.as_slice() {
            line.push(avatar("channel-avatar", &ctx.state.display(one), 24));
            line.push(web::styled(
                "channel-title",
                &heading,
                web::style().size(18).bold().color(look.ink).one_line(),
            ));
            line.push(presence(
                "channel-presence",
                active(ctx.state, one, ctx.actor, ctx.now),
                look,
            ));
            if let Some(status) = ctx.state.members.get(*one).filter(|m| !m.status.is_empty()) {
                line.push(web::styled(
                    "channel-status",
                    &status.status,
                    web::style().size(13).color(look.muted).one_line(),
                ));
            }
        } else {
            line.push(web::styled(
                "channel-title",
                &heading,
                web::style().size(18).bold().color(look.ink).one_line(),
            ));
        }
    }
    if !ctx.channel.topic.is_empty() {
        line.push(web::styled(
            "channel-topic",
            &ctx.channel.topic,
            web::style().size(13).color(look.muted).one_line(),
        ));
    }
    line.push(web::rest("channel-head-rest"));
    line.push(web::card_action(
        "channel-members",
        web::style().border(look.line).radius(6).padding(4),
        web::visit(format!("{}?members=1", ctx.url())),
        vec![web::styled_row(
            "channel-members-line",
            4,
            "center",
            web::style(),
            vec![
                glyph("channel-members-icon", "person", "Members", 14, look.muted),
                web::styled(
                    "channel-members-count",
                    ctx.channel.members.len().to_string(),
                    web::style().size(13).color(look.muted).one_line(),
                ),
            ],
        )],
    ));
    let mut rows = vec![web::styled_row(
        "channel-head",
        8,
        "center",
        web::style(),
        line,
    )];
    if !ctx.channel.pins.is_empty() || !ctx.channel.purpose.is_empty() {
        let mut bar = vec![];
        if !ctx.channel.pins.is_empty() {
            bar.push(glyph("channel-pins-icon", "pin", "Pinned", 12, look.muted));
            bar.push(web::styled(
                "channel-pins",
                format!("{} Pinned", ctx.channel.pins.len()),
                web::style().size(12).color(look.muted).one_line(),
            ));
        }
        if !ctx.channel.purpose.is_empty() {
            bar.push(glyph(
                "channel-purpose-icon",
                "info",
                "Purpose",
                12,
                look.muted,
            ));
            bar.push(web::styled(
                "channel-purpose",
                &ctx.channel.purpose,
                web::style().size(12).color(look.muted).one_line(),
            ));
        }
        bar.push(glyph(
            "add-bookmark-icon",
            "plus",
            "Add a bookmark",
            12,
            look.muted,
        ));
        bar.push(web::styled(
            "add-bookmark",
            "Add a bookmark",
            web::style().size(12).color(look.muted).one_line(),
        ));
        rows.push(web::pills("channel-bar", 6, bar));
    }
    web::column("channel-header", 2, web::style().padding(0), rows)
}
/// What the right-hand pane holds, which decides the last column of the composer bar.
enum Pane<'a> {
    None,
    Thread(&'a Message),
    Members,
}
/// The thread's reply field, at the bottom of its pane and level with the composer.
fn thread_composer(ctx: &Ctx, parent: &Message) -> PageElement {
    let look = ctx.look;
    let id = &parent.id;
    let action = post(
        format!("{}/messages", ctx.url()),
        &[("text", &format!("${id}-reply-body")), ("parent", id)],
    );
    web::card(
        "thread-composer",
        web::style()
            .width(PANE)
            .flex(0)
            .padding(12)
            .radius(0)
            .background(look.surface),
        vec![PageElement::Form {
            id: format!("{id}-reply"),
            action: action.clone(),
            children: vec![
                PageElement::Input {
                    id: format!("{id}-reply-body"),
                    label: "Reply in thread".into(),
                    value: String::new(),
                    placeholder: String::new(),
                },
                web::styled_button(
                    &format!("{id}-reply-submit"),
                    "Reply",
                    action,
                    web::style()
                        .size(12)
                        .padding(10)
                        .radius(4)
                        .background(look.green)
                        .color("#ffffff"),
                ),
            ],
        }],
    )
}
/// The composer pinned to the bottom: a bordered box with formatting tools over the
/// field and the send button under it, the frame and sidebar colours running beneath,
/// and the thread's reply field beside it while a thread is open.
fn composer(ctx: &Ctx, pane: &Pane) -> PageElement {
    let look = ctx.look;
    let url = format!("{}/messages", ctx.url());
    let action = post(url, &[("text", "$send-text")]);
    let tool = |id: &str, icon: &str, label: &str| {
        web::icon(
            id,
            icon,
            label,
            web::style().size(14).padding(3).color(look.muted),
        )
    };
    let form = PageElement::Form {
        id: "send".into(),
        action: action.clone(),
        children: vec![
            web::pills(
                "send-tools",
                2,
                vec![
                    tool("send-bold", "bold", "Bold"),
                    tool("send-italic", "italic", "Italic"),
                    tool("send-strike", "minus", "Strikethrough"),
                    tool("send-link", "link", "Link"),
                    tool("send-list", "list-view", "Bulleted list"),
                    tool("send-code", "code", "Code"),
                ],
            ),
            PageElement::Input {
                id: "send-text".into(),
                label: format!("Message {}", ctx.heading()),
                value: String::new(),
                placeholder: String::new(),
            },
            web::styled_row(
                "send-actions",
                4,
                "center",
                web::style(),
                vec![
                    web::icon(
                        "send-attach",
                        "plus",
                        "Attach",
                        web::style()
                            .size(14)
                            .padding(4)
                            .radius(11)
                            .background("#f2f2f2")
                            .color(look.muted),
                    ),
                    tool("send-emoji", "emoji", "Emoji"),
                    tool("send-mention", "at", "Mention someone"),
                    web::rest("send-actions-rest"),
                    web::icon_action(
                        "send-submit",
                        "send",
                        "Send message",
                        web::style()
                            .size(14)
                            .padding(6)
                            .radius(4)
                            .background(look.green)
                            .color("#ffffff"),
                        action,
                    ),
                ],
            ),
        ],
    };
    let main = web::card(
        "composer-main",
        web::style()
            .background(look.surface)
            .padding(12)
            .radius(0)
            .flex(1),
        vec![
            form,
            web::styled(
                "send-hint",
                "Shift + Enter to add a new line",
                web::style().size(11).color(look.muted).align("right"),
            ),
        ],
    );
    let mut columns = vec![
        block("composer-rail", MARGIN + RAIL, look.frame, vec![]),
        block("composer-sidebar", SIDEBAR, look.sidebar, vec![]),
        main,
    ];
    match pane {
        Pane::None => {}
        Pane::Thread(parent) => columns.push(thread_composer(ctx, parent)),
        Pane::Members => columns.push(block("composer-pane", PANE, look.surface, vec![])),
    }
    columns.push(block("composer-edge", MARGIN, look.frame, vec![]));
    web::styled_row(
        "composer",
        0,
        "stretch",
        web::style().pin("bottom").background(look.frame),
        columns,
    )
}
/// The thread pane: the parent, its replies, and the reply field.
fn thread_pane(ctx: &Ctx, parent: &Message) -> PageElement {
    let ctx = &Ctx {
        budget: PANE_CHARS,
        ..*ctx
    };
    let look = ctx.look;
    let replies: Vec<&Message> = ctx
        .channel
        .messages
        .iter()
        .filter(|r| r.parent.as_deref() == Some(parent.id.as_str()))
        .collect();
    let mut items = vec![
        web::styled_row(
            "thread-head",
            8,
            "center",
            web::style(),
            vec![
                web::styled(
                    "thread-title",
                    "Thread",
                    web::style().size(15).bold().color(look.ink).one_line(),
                ),
                web::styled(
                    "thread-channel",
                    ctx.heading(),
                    web::style().size(13).color(look.muted).one_line(),
                ),
                web::rest("thread-head-rest"),
                web::icon_action(
                    "thread-close",
                    "close",
                    "Close thread",
                    web::style().size(16).padding(4).color(look.muted),
                    web::visit(ctx.url()),
                ),
            ],
        ),
        web::divider("thread-rule"),
        // The parent is also in the transcript, so its ids are the pane's own here.
        message(ctx, "thread-", parent, true, false, &[]),
    ];
    if !replies.is_empty() {
        items.push(web::styled_row(
            "thread-count",
            8,
            "center",
            web::style().padding(2),
            vec![
                web::styled(
                    "thread-count-text",
                    format!(
                        "{} {}",
                        replies.len(),
                        if replies.len() == 1 {
                            "reply"
                        } else {
                            "replies"
                        }
                    ),
                    web::style().size(12).color(look.muted).one_line(),
                ),
                PageElement::Divider {
                    id: "thread-count-rule".into(),
                    style: web::style().flex(1).color(look.line),
                },
            ],
        ));
    }
    let mut previous: Option<&Message> = None;
    for reply in &replies {
        let first = previous.is_none_or(|p| {
            p.author != reply.author || reply.time.saturating_sub(p.time) > GROUP_US
        });
        items.push(message(ctx, "", reply, first, false, &[]));
        previous = Some(reply);
    }
    web::column(
        "thread-pane",
        6,
        web::style()
            .width(PANE)
            .flex(0)
            .padding(12)
            .background(look.surface)
            .border(look.line),
        items,
    )
}
/// The member list of the open conversation: face, name, title and status.
fn members_pane(ctx: &Ctx) -> PageElement {
    let look = ctx.look;
    let mut items = vec![
        web::styled_row(
            "members-head",
            8,
            "center",
            web::style(),
            vec![
                web::styled(
                    "members-label",
                    format!("Members · {}", ctx.channel.members.len()),
                    web::style().size(15).bold().color(look.ink).one_line(),
                ),
                web::rest("members-head-rest"),
                web::icon_action(
                    "members-close",
                    "close",
                    "Close",
                    web::style().size(16).padding(4).color(look.muted),
                    web::visit(ctx.url()),
                ),
            ],
        ),
        web::divider("members-rule"),
    ];
    for who in &ctx.channel.members {
        let member = ctx.state.members.get(who);
        let mut lines = vec![web::styled_row(
            &format!("member-{who}-name"),
            6,
            "center",
            web::style(),
            vec![
                web::styled(
                    &format!("member-{who}"),
                    ctx.state.display(who),
                    web::style().size(14).bold().color(look.ink).one_line(),
                ),
                presence(
                    &format!("member-{who}-presence"),
                    active(ctx.state, who, ctx.actor, ctx.now),
                    look,
                ),
                web::rest(&format!("member-{who}-rest")),
            ],
        )];
        if let Some(m) = member {
            if !m.title.is_empty() {
                lines.push(web::styled(
                    &format!("member-{who}-title"),
                    &m.title,
                    web::style().size(12).color(look.muted).one_line(),
                ));
            }
            if !m.status.is_empty() {
                lines.push(web::styled(
                    &format!("member-{who}-status"),
                    &m.status,
                    web::style().size(12).color(look.muted).one_line(),
                ));
            }
        }
        items.push(web::styled_row(
            &format!("member-{who}-card"),
            10,
            "start",
            web::style().padding(4),
            vec![
                avatar(&format!("member-{who}-avatar"), &ctx.state.display(who), 32),
                web::column(
                    &format!("member-{who}-lines"),
                    1,
                    web::style().flex(1),
                    lines,
                ),
            ],
        ));
    }
    web::column(
        "members",
        4,
        web::style()
            .width(PANE)
            .flex(0)
            .padding(12)
            .background(look.surface)
            .border(look.line),
        items,
    )
}
/// The banner over unread messages, with the one control that marks them read.
fn unread_banner(ctx: &Ctx, unread: usize) -> PageElement {
    let action = post(format!("{}/read", ctx.url()), &[]);
    PageElement::Form {
        id: "read".into(),
        action: action.clone(),
        children: vec![web::styled_row(
            "read-line",
            8,
            "center",
            web::style().padding(6).radius(6).background(ctx.look.tint),
            vec![
                web::styled(
                    "read-count",
                    format!("{unread} new message{}", if unread == 1 { "" } else { "s" }),
                    web::style()
                        .size(13)
                        .bold()
                        .color(ctx.look.accent)
                        .one_line(),
                ),
                web::rest("read-rest"),
                web::styled_button(
                    "read-submit",
                    "Mark as read",
                    action,
                    web::style()
                        .size(12)
                        .padding(8)
                        .radius(4)
                        .background(ctx.look.surface)
                        .border(ctx.look.line)
                        .color(ctx.look.ink),
                ),
            ],
        )],
    }
}

/// The whole workspace: top bar, rail, sidebar, the open conversation, the composer,
/// and the pane the query string asked for.
pub fn workspace(
    state: &SlackState,
    actor: &str,
    open: Option<&str>,
    view: &View,
) -> SimResult<HttpResponse> {
    let look = &SLACK;
    let now = now(state, actor, view.tick);
    // The root opens the first channel the actor is in, as Slack lands on one.
    let open = open.or_else(|| {
        state
            .channels
            .iter()
            .find(|(_, c)| c.members.contains(actor))
            .map(|(id, _)| id.as_str())
    });
    let (channel, id) = match open {
        None => (None, None),
        Some(id) => match state.channel(actor, id) {
            Err(e) => return web::error(403, e),
            Ok(channel) => (Some(channel), Some(id)),
        },
    };
    // A pane beside the conversation narrows it; the rows of text are cut to fit.
    let beside = view.members
        || view
            .thread
            .as_deref()
            .is_some_and(|t| channel.is_some_and(|c| c.messages.iter().any(|m| m.id == t)));
    let ctx = channel.map(|channel| Ctx {
        state,
        id: id.unwrap_or_default(),
        channel,
        actor,
        now,
        look,
        budget: if beside { NARROW_CHARS } else { WIDE_CHARS },
    });
    let is_dm = id.is_some_and(|id| state.dms.contains_key(id));
    let workspace_name = if state.workspace.is_empty() {
        look.brand.to_owned()
    } else {
        state.workspace.clone()
    };
    let title = match &ctx {
        Some(ctx) if is_dm => format!("{} (DM) - {workspace_name} - {}", ctx.heading(), look.brand),
        Some(ctx) => format!("#{} (Channel) - {workspace_name} - {}", ctx.id, look.brand),
        None => format!("{workspace_name} - {}", look.brand),
    };
    // The header bar: the workspace tile over the rail, the workspace name over the
    // sidebar, the channel header over the conversation.
    let mark = web::thumbnail(
        "rail-mark",
        workspace_name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default(),
        web::style()
            .width(36)
            .height(36)
            .radius(8)
            .background("#ffffff")
            .color(look.frame)
            .size(16)
            .align("center"),
    );
    let sidebar_head = web::styled_row(
        "sidebar-head",
        6,
        "center",
        web::style().padding(10),
        vec![
            web::styled(
                "workspace",
                &workspace_name,
                web::style().size(16).bold().color("#ffffff").one_line(),
            ),
            glyph(
                "workspace-menu",
                "chevron-down",
                "Workspace menu",
                12,
                "#ffffff",
            ),
            web::rest("sidebar-head-rest"),
            web::icon(
                "compose",
                "compose",
                "New message",
                web::style()
                    .size(14)
                    .padding(6)
                    .radius(13)
                    .background("#ffffff")
                    .color(look.frame),
            ),
        ],
    );
    let channel_header = match &ctx {
        Some(ctx) => header(ctx),
        None => web::styled(
            "empty",
            "Pick a channel or start a direct message.",
            web::style().size(15).color(look.muted),
        ),
    };
    let header_bar = web::styled_row(
        "header",
        0,
        "stretch",
        web::style().pin("top").background(look.frame),
        vec![
            block(
                "header-rail",
                MARGIN + RAIL,
                look.frame,
                vec![web::styled_row(
                    "header-rail-line",
                    0,
                    "center",
                    web::style().padding(6).justify("end"),
                    vec![mark],
                )],
            ),
            block("header-sidebar", SIDEBAR, look.sidebar, vec![sidebar_head]),
            web::card(
                "header-main",
                web::style()
                    .background(look.surface)
                    .padding(12)
                    .radius(0)
                    .flex(1),
                vec![channel_header],
            ),
            block("header-edge", MARGIN, look.frame, vec![]),
        ],
    );
    // The conversation.
    let mut main = vec![];
    if let (Some(ctx), Some(id)) = (&ctx, id) {
        let unread = state.unread(actor, id);
        if unread > 0 {
            main.push(unread_banner(ctx, unread));
        }
        main.extend(transcript(ctx));
    }
    let mut shell = vec![
        rail(state, actor, !is_dm, is_dm, look),
        sidebar(state, actor, id, now, look),
        web::column(
            "main",
            4,
            web::style().background(look.surface).padding(12).flex(1),
            main,
        ),
    ];
    let pane = match &ctx {
        Some(ctx) => match view
            .thread
            .as_deref()
            .and_then(|t| ctx.channel.messages.iter().find(|m| m.id == t))
        {
            Some(parent) => Pane::Thread(parent),
            None if view.members => Pane::Members,
            None => Pane::None,
        },
        None => Pane::None,
    };
    if let Some(ctx) = &ctx {
        match pane {
            Pane::Thread(parent) => shell.push(thread_pane(ctx, parent)),
            Pane::Members => shell.push(members_pane(ctx)),
            Pane::None => {}
        }
    }
    let mut elements = vec![
        topbar(state, actor, look),
        header_bar,
        web::styled_row("shell", 0, "stretch", web::style(), shell),
    ];
    if let Some(ctx) = &ctx {
        elements.push(composer(ctx, &pane));
    }
    web::themed_page(&title, theme(state, look), elements)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emoji_names_become_characters() {
        assert_eq!(emoji("+1"), "👍");
        assert_eq!(emoji("nope"), ":nope:");
        assert_eq!(
            emojify("ship it :rocket: at 10:30 :tada:"),
            "ship it 🚀 at 10:30 🎉"
        );
        assert_eq!(emojify("a :unknown: b"), "a :unknown: b");
        assert_eq!(emojify("http://x.com/a:b"), "http://x.com/a:b");
    }
    #[test]
    fn lines_split_into_runs_links_mentions_and_code() {
        let look = &SLACK;
        let PageElement::Row { children, .. } = inline_line(
            "m-text",
            "@bob see http://github.com/x/y/pull/1, then run `cargo test` :eyes:",
            &["bob"],
            look,
        ) else {
            panic!()
        };
        // mention, "see", link, ", then run", code, emoji run, rest.
        assert_eq!(children.len(), 7);
        assert!(
            matches!(&children[0], PageElement::Styled { text, style, .. } if text == "@bob" && style.background.is_some())
        );
        assert!(
            matches!(&children[2], PageElement::Link { url, text, .. } if url == "http://github.com/x/y/pull/1" && text == "github.com/x/y/pull/1")
        );
        assert!(
            matches!(&children[4], PageElement::Styled { text, style, .. } if text == "cargo test" && style.mono == Some(true))
        );
        assert!(matches!(&children[5], PageElement::Styled { text, .. } if text == "👀"));
    }
}
