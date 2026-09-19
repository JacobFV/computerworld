//! The Discord server page, laid out the way Discord's desktop web app is in 2024: dark
//! throughout, a rail of round server icons on the left, the channel sidebar with its
//! collapsible categories, `#` text channels and voice channels showing whoever is in
//! them, the user panel pinned under the sidebar, the open channel in the middle with
//! its messages grouped by author under date dividers, replies drawn as Discord's reply
//! line, reactions as pill chips, a composer pinned to the bottom, and the member list
//! grouped by role on the right. The whole rendering lives here, apart from the state
//! and the routes.
use crate::{time, DiscordState, Message, TextChannel};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result as SimResult};
use cw_service_common as web;

/// Discord's palette and vocabulary.
pub struct Look {
    /// The darkest column: the server rail, the search box, code.
    pub rail: &'static str,
    /// The channel sidebar and the member list.
    pub sidebar: &'static str,
    /// The chat.
    pub surface: &'static str,
    /// The user panel under the sidebar.
    pub panel: &'static str,
    /// The composer's field.
    pub field: &'static str,
    /// The sidebar row under the pointer, and the open channel's row.
    pub hover: &'static str,
    pub ink: &'static str,
    /// Sidebar text, and the secondary text in the chat.
    pub muted: &'static str,
    /// The header's icons and the category labels.
    pub dim: &'static str,
    /// Blurple.
    pub accent: &'static str,
    /// The tint behind the actor's own reactions.
    pub tint: &'static str,
    pub link: &'static str,
    pub line: &'static str,
    pub green: &'static str,
    pub brand: &'static str,
}
pub const DISCORD: Look = Look {
    rail: "#1e1f22",
    sidebar: "#2b2d31",
    surface: "#313338",
    panel: "#232428",
    field: "#383a40",
    hover: "#404249",
    ink: "#f2f3f5",
    muted: "#949ba4",
    dim: "#80848e",
    accent: "#5865f2",
    tint: "#3c4270",
    link: "#00a8fc",
    line: "#3f4147",
    green: "#23a559",
    brand: "Discord",
};
/// What the query string opens beside the channel, and when it is.
#[derive(Clone, Debug)]
pub struct View {
    /// Now, on Discord's clock (the world tick plus `crate::HISTORY`).
    pub tick: u64,
    /// The member list on the right, open unless `?members=0` closes it.
    pub members: bool,
}
impl Default for View {
    fn default() -> Self {
        Self {
            tick: 0,
            members: true,
        }
    }
}
const RAIL: u32 = 72;
const SIDEBAR: u32 = 240;
const MEMBERS: u32 = 240;
/// The browser's page margin, which the rail colour fills.
const MARGIN: u32 = 16;
const AVATAR: u32 = 40;
/// Messages by one author this close together share one header, as Discord groups them.
const GROUP_US: u64 = 7 * time::MINUTE_US;
/// Characters of message text per row, the wrap the page itself cannot do: a row lays
/// its pieces out on one line, so a message is cut into rows this long at word
/// boundaries. Sized for a 1280-wide window, with and without the member list.
const WIDE_CHARS: usize = 96;
const NARROW_CHARS: usize = 66;
/// Whoever has posted this recently reads as online.
const ONLINE_US: u64 = 3 * time::HOUR_US;

/// Discord's short names for the emoji the seed and the reactions use.
const EMOJI: &[(&str, &str)] = &[
    ("+1", "👍"),
    ("thumbsup", "👍"),
    ("tada", "🎉"),
    ("eyes", "👀"),
    ("heart", "❤️"),
    ("rocket", "🚀"),
    ("white_check_mark", "✅"),
    ("thinking", "🤔"),
    ("thinking_face", "🤔"),
    ("fire", "🔥"),
    ("pray", "🙏"),
    ("joy", "😂"),
    ("raised_hands", "🙌"),
    ("wave", "👋"),
    ("100", "💯"),
    ("pushpin", "📌"),
    ("sob", "😭"),
    ("skull", "💀"),
];
/// The emoji a short name stands for, or the `:name:` itself when it is unknown.
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
fn theme(state: &DiscordState, look: &Look) -> PageTheme {
    state.theme.clone().unwrap_or(PageTheme {
        accent: Some(look.accent.into()),
        // The page is the rail's dark; every lighter surface names its own colour.
        background: Some(look.rail.into()),
        surface: Some(look.surface.into()),
        ink: Some(look.ink.into()),
        muted: Some(look.muted.into()),
        content_width: Some(4096),
    })
}
/// A block of one colour, `width` across, filling a pinned bar under the rail, the
/// sidebar or the member list so the columns run the full height of the window.
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
/// The colour a member's name takes: their highest role's, or plain ink.
fn role_color<'a>(state: &'a DiscordState, who: &str, look: &'a Look) -> &'a str {
    state
        .top_role(who)
        .map(|(_, r)| r.color.as_str())
        .filter(|c| !c.is_empty())
        .unwrap_or(look.ink)
}
/// Whether a person reads as online: they have posted lately, they are in a voice
/// channel, or they are the person looking.
fn online(state: &DiscordState, who: &str, actor: &str, now: u64) -> bool {
    who == actor
        || state
            .server
            .voice
            .values()
            .any(|v| v.occupants.contains(who))
        || state
            .server
            .channels
            .values()
            .flat_map(|c| &c.messages)
            .filter(|m| m.author == who)
            .any(|m| now.saturating_sub(m.time) < ONLINE_US)
}
/// The presence dot beside an avatar: green while online, hollow grey while not.
fn presence(id: &str, on: bool, look: &Look) -> PageElement {
    web::styled(
        id,
        if on { "●" } else { "○" },
        web::style()
            .size(9)
            .one_line()
            .color(if on { look.green } else { look.dim }),
    )
}
/// Now on Discord's clock: the tick, or the newest message if the seed runs ahead of
/// the world.
fn now(state: &DiscordState, tick: u64) -> u64 {
    state
        .server
        .channels
        .values()
        .flat_map(|c| &c.messages)
        .map(|m| m.time)
        .fold(tick, u64::max)
}
fn server_name(state: &DiscordState, look: &Look) -> String {
    if state.server.name.is_empty() {
        look.brand.to_owned()
    } else {
        state.server.name.clone()
    }
}
fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

// ---- the rail --------------------------------------------------------------------------

/// The Discord home button, at the top of the rail.
fn home(look: &Look) -> PageElement {
    web::icon_action(
        "rail-home",
        "chat",
        "Direct Messages",
        web::style()
            .size(22)
            .padding(13)
            .radius(24)
            .background(look.accent)
            .color("#ffffff"),
        web::visit("/channels/@me"),
    )
}
/// The server rail: this server's icon, rounded to a square as Discord marks the open
/// one, then the add-server and explore buttons.
fn rail(state: &DiscordState, look: &Look) -> PageElement {
    let name = server_name(state, look);
    let round = |id: &str, icon: &str, label: &str| {
        web::icon(
            id,
            icon,
            label,
            web::style()
                .size(22)
                .padding(13)
                .radius(24)
                .background(look.sidebar)
                .color(look.green),
        )
    };
    web::column(
        "rail",
        8,
        web::style()
            .width(RAIL)
            .flex(0)
            .padding(12)
            .background(look.rail),
        vec![
            PageElement::Divider {
                id: "rail-rule".into(),
                style: web::style().color(look.line),
            },
            web::thumbnail_action(
                "rail-server",
                initials(&name),
                web::style()
                    .width(48)
                    .height(48)
                    .radius(16)
                    .background(look.accent)
                    .color("#ffffff")
                    .size(16)
                    .align("center"),
                web::visit(format!("/channels/{}", state.server.id)),
            ),
            round("rail-add", "plus", "Add a Server"),
            round("rail-explore", "compass", "Explore Discoverable Servers"),
        ],
    )
}

// ---- the sidebar -----------------------------------------------------------------------

/// A category label: the collapse chevron and the name in small capitals.
fn category(id: &str, label: &str, look: &Look) -> PageElement {
    web::pills(
        id,
        3,
        vec![
            glyph(
                &format!("{id}-chevron"),
                "chevron-down",
                "Collapse category",
                9,
                look.dim,
            ),
            web::styled(
                &format!("{id}-text"),
                label.to_uppercase(),
                web::style().size(11).bold().color(look.dim).one_line(),
            ),
        ],
    )
}
/// One text channel in the sidebar: `#` and its name, the row highlighted while it is
/// the open one.
fn channel_row(state: &DiscordState, id: &str, open: Option<&str>, look: &Look) -> PageElement {
    let current = open == Some(id);
    let ink = if current { look.ink } else { look.muted };
    web::card_action(
        &format!("nav-{id}"),
        web::style().padding(5).radius(4).background(if current {
            look.hover
        } else {
            look.sidebar
        }),
        web::visit(format!("/channels/{}/{id}", state.server.id)),
        vec![web::styled_row(
            &format!("nav-{id}-line"),
            6,
            "center",
            web::style(),
            vec![
                glyph(
                    &format!("nav-{id}-hash"),
                    "hash",
                    "Text channel",
                    16,
                    look.dim,
                ),
                web::styled(
                    &format!("nav-{id}-text"),
                    id,
                    web::style().size(15).medium().color(ink).one_line().flex(1),
                ),
            ],
        )],
    )
}
/// A voice channel: the speaker, its name, and whoever is in it under it with their
/// faces. Clicking the row joins it, or leaves it while the actor is inside.
fn voice_rows(state: &DiscordState, id: &str, actor: &str, look: &Look) -> Vec<PageElement> {
    let voice = &state.server.voice[id];
    let inside = voice.occupants.contains(actor);
    let action = post(
        if inside {
            format!("/voice/{id}/leave")
        } else {
            format!("/voice/{id}/join")
        },
        &[],
    );
    let mut out = vec![web::card_action(
        &format!("voice-{id}"),
        web::style().padding(5).radius(4).background(if inside {
            look.hover
        } else {
            look.sidebar
        }),
        action,
        vec![web::styled_row(
            &format!("voice-{id}-line"),
            6,
            "center",
            web::style(),
            vec![
                glyph(
                    &format!("voice-{id}-icon"),
                    "volume",
                    "Voice channel",
                    16,
                    look.dim,
                ),
                web::styled(
                    &format!("voice-{id}-name"),
                    id,
                    web::style()
                        .size(15)
                        .medium()
                        .color(if inside { look.ink } else { look.muted })
                        .one_line()
                        .flex(1),
                ),
            ],
        )],
    )];
    for who in &voice.occupants {
        out.push(web::styled_row(
            &format!("voice-{id}-{who}"),
            8,
            "center",
            web::style().padding(2),
            vec![
                web::styled(
                    &format!("voice-{id}-{who}-indent"),
                    "",
                    web::style().width(22),
                ),
                web::avatar(&format!("voice-{id}-{who}-avatar"), &state.display(who), 24),
                web::styled(
                    &format!("voice-{id}-{who}-name"),
                    state.display(who),
                    web::style().size(13).color(look.muted).one_line().flex(1),
                ),
            ],
        ));
    }
    out
}
fn sidebar(state: &DiscordState, actor: &str, open: Option<&str>, look: &Look) -> PageElement {
    let mut items = vec![];
    let mut listed = std::collections::BTreeSet::new();
    for (index, cat) in state.server.categories.iter().enumerate() {
        if index > 0 {
            items.push(web::spacer(&format!("category-{index}-gap"), 10));
        }
        items.push(category(&format!("category-{index}"), &cat.name, look));
        for id in &cat.channels {
            listed.insert(id.as_str());
            if state.server.voice.contains_key(id) {
                items.extend(voice_rows(state, id, actor, look));
            } else if state.channel(actor, id).is_ok() {
                items.push(channel_row(state, id, open, look));
            }
        }
    }
    let loose: Vec<&str> = state
        .visible(actor)
        .into_iter()
        .filter(|id| !listed.contains(id))
        .collect();
    if !loose.is_empty() {
        items.push(web::spacer("category-other-gap", 10));
        items.push(category("category-other", "Text Channels", look));
        for id in loose {
            items.push(channel_row(state, id, open, look));
        }
    }
    web::column(
        "sidebar",
        1,
        web::style()
            .background(look.sidebar)
            .padding(8)
            .width(SIDEBAR)
            .flex(0),
        items,
    )
}
/// The user panel pinned under the sidebar: the actor's face and name, their presence,
/// and the mute, deafen and settings buttons.
fn user_panel(state: &DiscordState, actor: &str, look: &Look) -> PageElement {
    let name = state.display(actor);
    let tool = |id: &str, icon: &str, label: &str| {
        web::icon(
            id,
            icon,
            label,
            web::style().size(16).padding(4).radius(4).color(look.muted),
        )
    };
    web::styled_row(
        "user-panel",
        6,
        "center",
        web::style().padding(8).background(look.panel),
        vec![
            web::avatar("me-avatar", &name, 32),
            web::column(
                "me-lines",
                0,
                web::style().flex(1),
                vec![
                    web::styled(
                        "me-name",
                        &name,
                        web::style().size(13).bold().color(look.ink).one_line(),
                    ),
                    web::pills(
                        "me-status",
                        4,
                        vec![
                            presence("me-presence", true, look),
                            web::styled(
                                "me-status-text",
                                "Online",
                                web::style().size(11).color(look.muted).one_line(),
                            ),
                        ],
                    ),
                ],
            ),
            tool("me-mic", "mic", "Mute"),
            tool("me-headset", "headphones", "Deafen"),
            tool("me-settings", "gear", "User Settings"),
        ],
    )
}

// ---- messages --------------------------------------------------------------------------

/// Everything a message block needs to know about where it is drawn.
struct Ctx<'a> {
    state: &'a DiscordState,
    id: &'a str,
    channel: &'a TextChannel,
    actor: &'a str,
    now: u64,
    look: &'a Look,
    /// Characters of message text per row, see `WIDE_CHARS`.
    budget: usize,
}
impl Ctx<'_> {
    fn url(&self) -> String {
        format!("/channels/{}/{}", self.state.server.id, self.id)
    }
}
/// A line of a message as Discord sets it: plain runs with their emoji, `#channel` and
/// `@name` mentions on blurple, links in blue and code in the monospace face, each at
/// its own width.
fn inline_line(ctx: &Ctx, id: &str, line: &str) -> PageElement {
    let look = ctx.look;
    let words: Vec<&str> = line.split(' ').filter(|w| !w.is_empty()).collect();
    let mut pieces = vec![];
    let mut run: Vec<String> = vec![];
    let flush = |run: &mut Vec<String>, pieces: &mut Vec<PageElement>| {
        if !run.is_empty() {
            pieces.push(web::styled(
                &format!("{id}-p{}", pieces.len()),
                emojify(&run.join(" ")),
                web::style().size(15).color("#dbdee1").one_line(),
            ));
            run.clear();
        }
    };
    /// A word and the punctuation that closes it, which stays plain text.
    fn trailing(w: &str) -> (&str, &str) {
        let end = w.trim_end_matches(['.', ',', ';', ':', ')', ']', '!', '?']);
        (end, &w[end.len()..])
    }
    let mention = |id: &str, text: String, action: Option<PageAction>| {
        let style = web::style()
            .size(15)
            .medium()
            .color("#c9cdfb")
            .background("#3c4270")
            .radius(3)
            .padding(2)
            .one_line();
        match action {
            Some(action) => web::styled_link(id, text, action.url, style),
            None => web::styled(id, text, style),
        }
    };
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if word.starts_with("http://") || word.starts_with("https://") {
            let (url, tail) = trailing(word);
            flush(&mut run, &mut pieces);
            pieces.push(web::inline_link(
                &format!("{id}-p{}", pieces.len()),
                url,
                url,
                15,
                look.link,
            ));
            if !tail.is_empty() {
                run.push(tail.into());
            }
        } else if let Some(name) = word
            .strip_prefix('#')
            .filter(|name| ctx.state.server.channels.contains_key(trailing(name).0))
        {
            let (name, tail) = trailing(name);
            flush(&mut run, &mut pieces);
            let url = format!("/channels/{}/{name}", ctx.state.server.id);
            pieces.push(mention(
                &format!("{id}-p{}", pieces.len()),
                format!("#{name}"),
                Some(web::visit(url)),
            ));
            if !tail.is_empty() {
                run.push(tail.into());
            }
        } else if let Some(name) = word.strip_prefix('@').filter(|name| {
            let (name, _) = trailing(name);
            ctx.state
                .server
                .members
                .iter()
                .any(|(who, m)| who == name || m.nick == name)
        }) {
            let (name, tail) = trailing(name);
            flush(&mut run, &mut pieces);
            pieces.push(mention(
                &format!("{id}-p{}", pieces.len()),
                format!("@{name}"),
                None,
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
                    .color(look.ink)
                    .background(look.rail)
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
    web::pills(id, 3, pieces)
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
fn body(ctx: &Ctx, id: &str, text: &str) -> Vec<PageElement> {
    let mut out = vec![];
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        for row in chunks(line, ctx.budget) {
            let row_id = if out.is_empty() {
                format!("{id}-text")
            } else {
                format!("{id}-text-{}", out.len())
            };
            out.push(inline_line(ctx, &row_id, &row));
        }
    }
    out
}
/// The reactions under a message as Discord's pills: the emoji and its count, the
/// actor's own outlined in blurple; each one toggles the actor's reaction.
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
                    .size(13)
                    .padding(6)
                    .radius(8)
                    .background(if mine { look.tint } else { look.sidebar })
                    .border(if mine { look.accent } else { look.sidebar })
                    .color(if mine { "#c9cdfb" } else { "#dbdee1" }),
            )
        })
        .collect();
    chips.push(web::icon(
        &format!("{}-react-add", m.id),
        "emoji",
        "Add Reaction",
        web::style()
            .size(14)
            .padding(6)
            .radius(8)
            .background(look.sidebar)
            .color(look.muted),
    ));
    Some(web::pills(&format!("{}-reactions", m.id), 4, chips))
}
/// Discord's reply line over a message: the spine from the avatar column, the quoted
/// author's small face and coloured name, and the start of what they said.
fn reply_line(ctx: &Ctx, m: &Message, quoted: &Message) -> PageElement {
    let look = ctx.look;
    let id = &m.id;
    let text: String = emojify(&quoted.text)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let text = if text.chars().count() > 72 {
        let cut: String = text.chars().take(70).collect();
        format!("{}...", cut.trim_end())
    } else {
        text
    };
    web::styled_row(
        &format!("{id}-quote"),
        6,
        "center",
        web::style(),
        vec![
            web::styled(
                &format!("{id}-quote-indent"),
                "",
                web::style().width(AVATAR / 2),
            ),
            PageElement::Divider {
                id: format!("{id}-quote-spine"),
                style: web::style().width(AVATAR / 2 + 2).color(look.line),
            },
            web::avatar(
                &format!("{id}-quote-avatar"),
                &ctx.state.display(&quoted.author),
                16,
            ),
            web::styled(
                &format!("{id}-quote-author"),
                ctx.state.display(&quoted.author),
                web::style()
                    .size(13)
                    .medium()
                    .color(role_color(ctx.state, &quoted.author, look))
                    .one_line(),
            ),
            web::styled(
                &format!("{id}-quote-text"),
                text,
                web::style().size(13).color(look.muted).one_line().flex(1),
            ),
        ],
    )
}
/// The actions Discord floats over the message under the pointer: add reaction, reply,
/// create thread, and more.
fn toolbar(ctx: &Ctx, m: &Message) -> PageElement {
    let look = ctx.look;
    let tool = web::style().size(16).padding(5).radius(4).color(look.muted);
    web::styled_row(
        &format!("{}-tools", m.id),
        0,
        "center",
        web::style()
            .padding(1)
            .radius(6)
            .background(look.surface)
            .border(look.line),
        vec![
            web::icon(
                &format!("{}-add-reaction", m.id),
                "emoji",
                "Add Reaction",
                tool.clone(),
            ),
            web::icon(&format!("{}-reply", m.id), "reply", "Reply", tool.clone()),
            web::icon(
                &format!("{}-thread", m.id),
                "thread",
                "Create Thread",
                tool.clone(),
            ),
            web::icon(&format!("{}-more", m.id), "more", "More", tool),
        ],
    )
}
/// One message: the author's avatar, name and time on the first of a run by one
/// author, the gutter empty on the rest; the reply line over a reply; the hover
/// toolbar on the message under the pointer.
fn message(ctx: &Ctx, m: &Message, first: bool, hovered: bool) -> PageElement {
    let look = ctx.look;
    let id = &m.id;
    let quoted = m
        .reply_to
        .as_deref()
        .and_then(|r| ctx.channel.messages.iter().find(|q| q.id == r));
    let first = first || quoted.is_some();
    let stamp = time::stamp(m.time, ctx.now);
    let mut inner = vec![];
    if first {
        let mut head = vec![
            web::styled(
                &format!("{id}-author"),
                ctx.state.display(&m.author),
                web::style()
                    .size(15)
                    .medium()
                    .color(role_color(ctx.state, &m.author, look))
                    .one_line(),
            ),
            web::styled(
                &format!("{id}-time"),
                stamp,
                web::style().size(11).color(look.muted).one_line(),
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
    } else if hovered {
        inner.push(web::styled_row(
            &format!("{id}-head"),
            8,
            "center",
            web::style(),
            vec![web::rest(&format!("{id}-head-rest")), toolbar(ctx, m)],
        ));
    }
    inner.extend(body(ctx, id, &m.text));
    inner.extend(reactions(ctx, m));
    let gutter = if first {
        web::avatar(
            &format!("{id}-avatar"),
            &ctx.state.display(&m.author),
            AVATAR,
        )
    } else {
        web::styled(
            &format!("{id}-time"),
            if hovered {
                time::civil(m.time).clock()
            } else {
                String::new()
            },
            web::style()
                .size(9)
                .color(look.muted)
                .width(AVATAR)
                .one_line()
                .align("center"),
        )
    };
    let mut rows = vec![];
    if let Some(quoted) = quoted {
        rows.push(reply_line(ctx, m, quoted));
    }
    rows.push(web::styled_row(
        &format!("{id}-line"),
        12,
        "start",
        web::style(),
        vec![
            gutter,
            web::column(&format!("{id}-body"), 2, web::style().flex(1), inner),
        ],
    ));
    let mut style = web::style().padding(2).radius(4);
    if hovered {
        style = style.background("#2e3035");
    }
    web::column(&format!("{id}-row"), 2, style, rows)
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
            web::styled(
                &format!("day-{index}-label"),
                label,
                web::style().size(11).bold().color(look.muted).one_line(),
            ),
            rule(format!("day-{index}-right")),
        ],
    )
}
/// The transcript: messages under date dividers, runs by one author grouped, the last
/// message carrying the hover toolbar.
fn transcript(ctx: &Ctx) -> Vec<PageElement> {
    let messages = &ctx.channel.messages;
    let mut out = vec![];
    let mut previous: Option<&Message> = None;
    for (n, m) in messages.iter().enumerate() {
        let day = time::civil(m.time).index;
        let new_day = previous.is_none_or(|p| time::civil(p.time).index != day);
        if new_day {
            out.push(day_divider(day, time::day_label(m.time), ctx.look));
        }
        let first = new_day
            || previous
                .is_none_or(|p| p.author != m.author || m.time.saturating_sub(p.time) > GROUP_US);
        out.push(message(ctx, m, first, n + 1 == messages.len()));
        previous = Some(m);
    }
    out
}

// ---- the header, the composer and the member list ----------------------------------------

/// The channel header: `#`, the name, the topic, and the toolbar with the search box.
fn header(ctx: &Ctx, members_open: bool) -> PageElement {
    let look = ctx.look;
    let tool = |id: &str, icon: &str, label: &str, on: bool| {
        web::icon(
            id,
            icon,
            label,
            web::style()
                .size(20)
                .padding(2)
                .color(if on { look.ink } else { look.dim }),
        )
    };
    let members_url = if members_open {
        format!("{}?members=0", ctx.url())
    } else {
        ctx.url()
    };
    let mut line = vec![
        glyph("channel-hash", "hash", "Text channel", 22, look.dim),
        web::styled(
            "channel-title",
            ctx.id,
            web::style().size(15).bold().color(look.ink).one_line(),
        ),
    ];
    if !ctx.channel.roles.is_empty() {
        line.push(glyph(
            "channel-private",
            "lock",
            "Private channel",
            14,
            look.dim,
        ));
    }
    if !ctx.channel.topic.is_empty() {
        line.push(web::styled(
            "channel-topic-rule",
            "",
            web::style().width(1).height(22).background(look.line),
        ));
        line.push(web::styled(
            "channel-topic",
            emojify(&ctx.channel.topic),
            web::style().size(13).color(look.muted).one_line(),
        ));
    }
    line.push(web::rest("channel-head-rest"));
    line.push(tool("channel-threads", "thread", "Threads", false));
    line.push(tool(
        "channel-notifications",
        "bell",
        "Notification Settings",
        false,
    ));
    line.push(tool("channel-pins", "pin", "Pinned Messages", false));
    line.push(web::icon_action(
        "channel-members",
        "person",
        if members_open {
            "Hide Member List"
        } else {
            "Show Member List"
        },
        web::style()
            .size(20)
            .padding(2)
            .color(if members_open { look.ink } else { look.dim }),
        web::visit(members_url),
    ));
    line.push(web::card(
        "search",
        web::style()
            .width(150)
            .flex(0)
            .padding(4)
            .radius(4)
            .background(look.rail),
        vec![web::styled_row(
            "search-line",
            6,
            "center",
            web::style(),
            vec![
                web::styled(
                    "search-text",
                    "Search",
                    web::style().size(13).color(look.dim).one_line().flex(1),
                ),
                glyph("search-icon", "search", "Search", 14, look.dim),
            ],
        )],
    ));
    line.push(tool("channel-inbox", "inbox", "Inbox", false));
    line.push(tool("channel-help", "info", "Help", false));
    web::styled_row("channel-head", 8, "center", web::style(), line)
}
/// The composer pinned to the bottom: the field with its attach button and the gift,
/// GIF, sticker and emoji buttons, the rail and sidebar colours running beneath it with
/// the user panel, and the member list's colour beside it while the list is open.
fn composer(ctx: &Ctx, members_open: bool) -> PageElement {
    let look = ctx.look;
    let action = post(format!("{}/messages", ctx.url()), &[("text", "$send-text")]);
    let tool = |id: &str, icon: &str, label: &str| {
        web::icon(
            id,
            icon,
            label,
            web::style().size(20).padding(2).color(look.muted),
        )
    };
    let form = PageElement::Form {
        id: "send".into(),
        action: action.clone(),
        children: vec![
            PageElement::Input {
                id: "send-text".into(),
                label: format!("Message #{}", ctx.id),
                value: String::new(),
                placeholder: String::new(),
            },
            web::styled_row(
                "send-actions",
                8,
                "center",
                web::style(),
                vec![
                    web::icon(
                        "send-attach",
                        "plus",
                        "Upload a File or Send Invites",
                        web::style()
                            .size(12)
                            .padding(4)
                            .radius(10)
                            .background(look.muted)
                            .color(look.field),
                    ),
                    web::rest("send-actions-rest"),
                    tool("send-gift", "star", "Send a gift"),
                    web::styled(
                        "send-gif",
                        "GIF",
                        web::style()
                            .size(10)
                            .bold()
                            .padding(3)
                            .radius(4)
                            .border(look.muted)
                            .color(look.muted)
                            .one_line(),
                    ),
                    tool("send-sticker", "shapes", "Sticker"),
                    tool("send-emoji", "emoji", "Emoji"),
                    web::icon_action(
                        "send-submit",
                        "send",
                        "Send Message",
                        web::style()
                            .size(14)
                            .padding(5)
                            .radius(4)
                            .background(look.accent)
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
            .padding(8)
            .radius(0)
            .flex(1),
        vec![web::card(
            "composer-field",
            web::style().background(look.field).padding(4).radius(8),
            vec![form],
        )],
    );
    let mut columns = vec![
        block("composer-rail", MARGIN + RAIL, look.rail, vec![]),
        block(
            "composer-sidebar",
            SIDEBAR,
            look.panel,
            vec![user_panel(ctx.state, ctx.actor, look)],
        ),
        main,
    ];
    if members_open {
        columns.push(block("composer-members", MEMBERS, look.sidebar, vec![]));
    }
    columns.push(block("composer-edge", MARGIN, look.rail, vec![]));
    web::styled_row(
        "composer",
        0,
        "stretch",
        web::style().pin("bottom").background(look.rail),
        columns,
    )
}
/// The member list: the hoisted roles' online members under the role's name, everyone
/// else online under "Online", and everyone offline, dimmed, under "Offline".
fn members(state: &DiscordState, actor: &str, now: u64, look: &Look) -> PageElement {
    let mut roles: Vec<(&String, &crate::Role)> = state.server.roles.iter().collect();
    roles.sort_by_key(|(name, r)| (std::cmp::Reverse(r.position), (*name).clone()));
    // Discord lists a role's members apart only when the role is hoisted; here the
    // lowest role is everyone's base role and is not.
    let floor = roles.last().map(|(_, r)| r.position).unwrap_or_default();
    let mut items = vec![];
    let mut placed = std::collections::BTreeSet::new();
    let group = |items: &mut Vec<PageElement>, key: &str, label: &str, who: Vec<&String>| {
        if who.is_empty() {
            return;
        }
        if !items.is_empty() {
            items.push(web::spacer(&format!("group-{key}-gap"), 10));
        }
        items.push(web::styled(
            &format!("group-{key}"),
            format!("{} — {}", label.to_uppercase(), who.len()),
            web::style()
                .size(11)
                .bold()
                .color(look.dim)
                .padding(4)
                .one_line(),
        ));
        for member in who {
            let on = online(state, member, actor, now);
            let colour = role_color(state, member, look);
            items.push(web::styled_row(
                &format!("member-{member}"),
                8,
                "center",
                web::style().padding(4).radius(4),
                vec![
                    web::avatar(
                        &format!("member-{member}-avatar"),
                        &state.display(member),
                        32,
                    ),
                    web::styled(
                        &format!("member-{member}-name"),
                        state.display(member),
                        web::style()
                            .size(15)
                            .medium()
                            .color(if on { colour } else { look.dim })
                            .one_line()
                            .flex(1),
                    ),
                    presence(&format!("member-{member}-presence"), on, look),
                ],
            ));
        }
    };
    for (name, role) in &roles {
        if role.position <= floor {
            continue;
        }
        let who: Vec<&String> = state
            .server
            .members
            .keys()
            .filter(|m| state.top_role(m).is_some_and(|(top, _)| top == *name))
            .filter(|m| online(state, m, actor, now))
            .collect();
        for m in &who {
            placed.insert((*m).clone());
        }
        group(&mut items, name, name, who);
    }
    let rest = |on: bool| -> Vec<&String> {
        state
            .server
            .members
            .keys()
            .filter(|m| !placed.contains(*m) && online(state, m, actor, now) == on)
            .collect()
    };
    group(&mut items, "online", "Online", rest(true));
    group(&mut items, "offline", "Offline", rest(false));
    web::column(
        "members",
        1,
        web::style()
            .background(look.sidebar)
            .padding(8)
            .width(MEMBERS)
            .flex(0),
        items,
    )
}

/// The whole server: the rail, the sidebar with the user panel under it, the open
/// channel with its composer, and the member list while it is open.
pub fn server(
    state: &DiscordState,
    actor: &str,
    open: Option<&str>,
    view: &View,
) -> SimResult<HttpResponse> {
    let look = &DISCORD;
    let now = now(state, view.tick);
    let member = state.server.members.contains_key(actor);
    let (channel, id) = match open {
        None => (None, None),
        Some(id) => match state.channel(actor, id) {
            Err(e) => return web::error(403, e),
            Ok(channel) => (Some(channel), Some(id)),
        },
    };
    let members_open = member && view.members;
    let ctx = channel.map(|channel| Ctx {
        state,
        id: id.unwrap_or_default(),
        channel,
        actor,
        now,
        look,
        budget: if members_open {
            NARROW_CHARS
        } else {
            WIDE_CHARS
        },
    });
    let name = server_name(state, look);
    let title = match id {
        Some(id) => format!("#{id} · {name}"),
        None => name.clone(),
    };
    // The header bar: the home button over the rail, the server name over the sidebar,
    // the channel header over the chat and the member list.
    let sidebar_head = web::styled_row(
        "sidebar-head",
        6,
        "center",
        web::style().padding(12),
        vec![
            web::styled(
                "server-name",
                &name,
                web::style()
                    .size(15)
                    .bold()
                    .color(look.ink)
                    .one_line()
                    .flex(1),
            ),
            glyph("server-menu", "chevron-down", "Server menu", 14, look.ink),
        ],
    );
    let channel_header = match &ctx {
        Some(ctx) => header(ctx, members_open),
        None => web::styled(
            "empty",
            if member {
                "Pick a channel."
            } else {
                "You are not in this server."
            },
            web::style().size(15).color(look.muted).one_line(),
        ),
    };
    let header_bar = web::styled_row(
        "header",
        0,
        "stretch",
        web::style().pin("top").background(look.rail),
        vec![
            block(
                "header-rail",
                MARGIN + RAIL,
                look.rail,
                vec![web::styled_row(
                    "header-rail-line",
                    0,
                    "center",
                    web::style().padding(6).justify("end"),
                    vec![home(look)],
                )],
            ),
            block("header-sidebar", SIDEBAR, look.sidebar, vec![sidebar_head]),
            web::card(
                "header-main",
                web::style()
                    .background(look.surface)
                    .padding(8)
                    .radius(0)
                    .flex(1),
                vec![channel_header],
            ),
            block("header-edge", MARGIN, look.rail, vec![]),
        ],
    );
    let mut shell = vec![
        rail(state, look),
        sidebar(state, actor, id, look),
        web::column(
            "main",
            2,
            web::style().background(look.surface).padding(8).flex(1),
            ctx.as_ref().map(transcript).unwrap_or_default(),
        ),
    ];
    if members_open {
        shell.push(members(state, actor, now, look));
    }
    let mut elements = vec![
        header_bar,
        web::styled_row("shell", 0, "stretch", web::style(), shell),
    ];
    if let Some(ctx) = &ctx {
        elements.push(composer(ctx, members_open));
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
        let mut state = DiscordState::default();
        state.server.id = "atlas".into();
        state
            .server
            .channels
            .insert("rules".into(), TextChannel::default());
        state.server.members.insert(
            "bob".into(),
            crate::Member {
                nick: "bmartinez".into(),
                roles: Default::default(),
            },
        );
        let channel = TextChannel::default();
        let ctx = Ctx {
            state: &state,
            id: "rules",
            channel: &channel,
            actor: "alice",
            now: 0,
            look: &DISCORD,
            budget: WIDE_CHARS,
        };
        let PageElement::Row { children, .. } = inline_line(
            &ctx,
            "m-text",
            "@bmartinez read #rules, then http://github.com/x/y and run `cargo test` :eyes:",
        ) else {
            panic!()
        };
        // mention, "read", channel, ", then", link, "and run", code, emoji run, rest.
        assert_eq!(children.len(), 9);
        assert!(
            matches!(&children[0], PageElement::Styled { text, style, .. } if text == "@bmartinez" && style.background.is_some())
        );
        assert!(
            matches!(&children[2], PageElement::Link { url, text, .. } if url == "/channels/atlas/rules" && text == "#rules")
        );
        assert!(
            matches!(&children[4], PageElement::Link { url, .. } if url == "http://github.com/x/y")
        );
        assert!(
            matches!(&children[6], PageElement::Styled { text, style, .. } if text == "cargo test" && style.mono == Some(true))
        );
        assert!(matches!(&children[7], PageElement::Styled { text, .. } if text == "👀"));
    }
}
