//! The Discord server page, served as HTML and laid out the way Discord's desktop web
//! app is: dark throughout, a full-height flex shell holding the rail of round server
//! icons, the channel sidebar with its categories, `#` text channels and voice channels
//! showing whoever is in them over the user panel, the open channel in the middle (a
//! header, an inner scrolling transcript grouped by author under date dividers, replies
//! drawn as Discord's reply line, reactions as pill chips, the composer at the bottom)
//! and the member list grouped by role on the right, plus the page the header's search
//! box leads to. The look is `discord.css`; the whole rendering lives here, apart from
//! the state and the routes.
//!
//! Everything drawn here that looks like a control is one. What Discord's client does
//! with a script — the emoji picker, attach, gift, GIF and sticker, mute and deafen,
//! add-a-server, threads, notifications, pins, the inbox — is not drawn at all rather
//! than drawn dead, and the marks that are left (chevrons, hashes, locks, presence
//! dots) carry no name that promises an action. `tests/controls.rs` crawls every page
//! kind to keep it so.
use crate::{time, DiscordState, Message, TextChannel};
use cw_protocol::{HttpResponse, Result as SimResult};
use cw_service_common as web;
use cw_service_common::html::{
    self, a, button, div, el, form, hidden, span, text, text_input, Document, Html,
};

const CSS: &str = include_str!("discord.css");
const BRAND: &str = "Discord";

/// What the query string opens beside the channel, and when it is.
#[derive(Clone, Debug)]
pub struct View {
    /// Now, on Discord's clock (the world tick plus `crate::HISTORY`).
    pub tick: u64,
    /// The member list on the right, open unless `?members=0` closes it.
    pub members: bool,
    /// `?reply_to=<message id>`: the composer answers that message, as Discord's
    /// reply bar does once you have picked one.
    pub reply_to: Option<String>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            tick: 0,
            members: true,
            reply_to: None,
        }
    }
}
/// Messages by one author this close together share one header, as Discord groups them.
const GROUP_US: u64 = 7 * time::MINUTE_US;
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


/// A seeded colour is only trusted into a `style` attribute when it is a hex colour.
fn hex(colour: &str) -> Option<&str> {
    let digits = colour.strip_prefix('#')?;
    (matches!(digits.len(), 3 | 4 | 6 | 8) && digits.chars().all(|c| c.is_ascii_hexdigit()))
        .then_some(colour)
}
/// The seeded palette as custom properties on `<html>`; the sheet's own values stand
/// wherever the seed names none.
fn root_style(state: &DiscordState) -> String {
    let Some(theme) = &state.theme else {
        return String::new();
    };
    [
        ("--accent", &theme.accent),
        ("--rail", &theme.background),
        ("--surface", &theme.surface),
        ("--ink", &theme.ink),
        ("--muted", &theme.muted),
    ]
    .into_iter()
    .filter_map(|(name, value)| Some(format!("{name}: {}", hex(value.as_deref()?)?)))
    .collect::<Vec<_>>()
    .join("; ")
}
/// A picture that says something: drawn by the sheet from three strokes (`i.a`, `i.b`,
/// `i.c`), with an accessible name and a tooltip. It is never a control, so its name
/// says what it *is* ("Private channel"), never what pressing it would do.
fn icon(id: &str, class: &str, label: &str) -> Html {
    span(class)
        .id(id)
        .attr("role", "img")
        .attr("aria-label", label)
        .attr("title", label)
        .children([el("i").class("a"), el("i").class("b"), el("i").class("c")])
}
/// Pure ornament: a chevron beside a heading, a hash beside a channel name. It has no
/// accessible name because there is nothing to press and nothing to read.
fn mark(id: &str, class: &str, glyph: &str) -> Html {
    span(class).id(id).attr("aria-hidden", "true").text(glyph)
}
fn initials(name: &str) -> String {
    name.split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
        .filter(|w| !w.is_empty())
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}
/// A round face: the person's initials on their own tint.
fn avatar(id: &str, name: &str, class: &str) -> Html {
    span("avatar")
        .class(class)
        .id(id)
        .attr("aria-hidden", "true")
        .style(&format!("background-color: {}", web::avatar_tint(name)))
        .text(initials(name))
}
/// The colour a member's name takes: their highest role's, or the sheet's plain ink.
fn role_style(state: &DiscordState, who: &str) -> Option<String> {
    state
        .top_role(who)
        .and_then(|(_, r)| hex(&r.color))
        .map(|c| format!("color: {c}"))
}
fn coloured(node: Html, style: Option<String>) -> Html {
    match style {
        Some(style) => node.style(&style),
        None => node,
    }
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
/// The presence dot on an avatar's corner: green while online, hollow grey while not.
fn presence(id: &str, on: bool) -> Html {
    let label = if on { "Online" } else { "Offline" };
    span("presence")
        .class(if on { "on" } else { "off" })
        .id(id)
        .attr("role", "img")
        .attr("aria-label", label)
        .attr("title", label)
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
fn server_name(state: &DiscordState) -> String {
    if state.server.name.is_empty() {
        BRAND.to_owned()
    } else {
        state.server.name.clone()
    }
}
/// A path segment as a URL writes it; a voice channel's name can carry a space.
fn segment(id: &str) -> String {
    id.replace('%', "%25").replace(' ', "%20")
}

// ---- the rail --------------------------------------------------------------------------

/// The server rail: the home button and this server's icon with the pill that marks
/// the open one. Discord's add-server and explore tiles are not here: this world holds
/// one server and there is nowhere for them to lead.
fn rail(state: &DiscordState) -> Html {
    let name = server_name(state);
    el("nav").id("rail").class("rail").attr("aria-label", "Servers").children([
        a("/channels/@me")
            .id("rail-home")
            .class("server home")
            .attr("aria-label", "Direct Messages")
            .attr("title", "Direct Messages")
            .child(span("home-mark").attr("aria-hidden", "true").children([el("i").class("a"), el("i").class("b")])),
        el("hr").id("rail-rule").class("rail-rule"),
        div("server-slot open").child(
            a(format!("/channels/{}", segment(&state.server.id)))
                .id("rail-server")
                .class("server open")
                .attr("aria-label", name.as_str())
                .attr("title", name.as_str())
                .text(initials(&name)),
        ),
    ])
}

// ---- the sidebar -----------------------------------------------------------------------

/// A category label: the chevron that marks it open and the name in small capitals.
/// Nothing collapses a category here, so the chevron is only a mark.
fn category(id: &str, label: &str) -> Html {
    div("category").id(id).children([
        mark(&format!("{id}-chevron"), "chevron", "\u{25BE}"),
        span("category-name").id(format!("{id}-text")).text(label.to_uppercase()),
    ])
}
/// One text channel in the sidebar: `#` and its name, the row highlighted while it is
/// the open one.
fn channel_row(state: &DiscordState, id: &str, open: Option<&str>) -> Html {
    let current = open == Some(id);
    a(format!("/channels/{}/{}", segment(&state.server.id), segment(id)))
        .id(format!("nav-{id}"))
        .class("channel")
        .class(if current { "current" } else { "" })
        .when(current, |n| n.attr("aria-current", "page"))
        .children([
            span("hash").id(format!("nav-{id}-hash")).attr("aria-hidden", "true").text("#"),
            span("channel-name").id(format!("nav-{id}-text")).text(id),
        ])
}
/// A voice channel: the speaker, its name, and whoever is in it under it with their
/// faces. Clicking the row joins it, or leaves it while the actor is inside.
fn voice_rows(state: &DiscordState, id: &str, actor: &str) -> Html {
    let voice = &state.server.voice[id];
    let inside = voice.occupants.contains(actor);
    let action = format!("/voice/{}/{}", segment(id), if inside { "leave" } else { "join" });
    let label = format!("{} voice channel {id}", if inside { "Leave" } else { "Join" });
    html::fragment([
        form(&format!("voice-{id}-form"), action, "post").class("voice-form").child(
            el("button")
                .id(format!("voice-{id}"))
                .attr("type", "submit")
                .class("channel voice")
                .class(if inside { "current" } else { "" })
                .attr("title", label.as_str())
                .children([
                    span("speaker").id(format!("voice-{id}-icon")).attr("aria-hidden", "true").children([el("i").class("a"), el("i").class("b"), el("i").class("c")]),
                    span("channel-name").id(format!("voice-{id}-name")).text(id),
                ]),
        ),
        html::fragment(voice.occupants.iter().map(|who| {
            let name = state.display(who);
            div("occupant").id(format!("voice-{id}-{who}")).children([
                avatar(&format!("voice-{id}-{who}-avatar"), &name, "s24"),
                span("occupant-name").id(format!("voice-{id}-{who}-name")).text(name.as_str()),
            ])
        })),
    ])
}
fn sidebar(state: &DiscordState, actor: &str, open: Option<&str>) -> Html {
    let mut list = el("nav").id("sidebar").class("channels").attr("aria-label", "Channels");
    let mut listed = std::collections::BTreeSet::new();
    for (index, cat) in state.server.categories.iter().enumerate() {
        list = list.child(category(&format!("category-{index}"), &cat.name));
        for id in &cat.channels {
            listed.insert(id.as_str());
            if state.server.voice.contains_key(id) {
                list = list.child(voice_rows(state, id, actor));
            } else if state.channel(actor, id).is_ok() {
                list = list.child(channel_row(state, id, open));
            }
        }
    }
    let loose: Vec<&str> = state
        .visible(actor)
        .into_iter()
        .filter(|id| !listed.contains(id))
        .collect();
    if !loose.is_empty() {
        list = list.child(category("category-other", "Text Channels"));
        for id in loose {
            list = list.child(channel_row(state, id, open));
        }
    }
    list
}
/// The user panel under the sidebar: the actor's face and name and their presence.
/// Discord's mute, deafen and settings buttons want a client this world does not run,
/// so they are not drawn.
fn user_panel(state: &DiscordState, actor: &str) -> Html {
    let name = state.display(actor);
    div("user-panel").id("user-panel").children([
        span("face").children([avatar("me-avatar", &name, "s32"), presence("me-presence", true)]),
        div("me-lines").id("me-lines").children([
            span("me-name").id("me-name").text(name.as_str()),
            span("me-status").id("me-status").child(span("").id("me-status-text").text("Online")),
        ]),
    ])
}

// ---- messages --------------------------------------------------------------------------

/// Everything a message block needs to know about where it is drawn.
struct Ctx<'a> {
    state: &'a DiscordState,
    id: &'a str,
    channel: &'a TextChannel,
    actor: &'a str,
    now: u64,
}
impl Ctx<'_> {
    fn url(&self) -> String {
        format!("/channels/{}/{}", segment(&self.state.server.id), segment(self.id))
    }
}
/// A word and the punctuation that closes it, which stays plain text.
fn trailing(w: &str) -> (&str, &str) {
    let end = w.trim_end_matches(['.', ',', ';', ':', ')', ']', '!', '?']);
    (end, &w[end.len()..])
}
/// The pieces of a message's text, numbered `{id}-p{n}` through the whole message so
/// every link, mention and run can be found.
struct Pieces<'a> {
    id: &'a str,
    count: usize,
    out: Vec<Html>,
    run: Vec<String>,
    /// The next run starts with closing punctuation and hugs the piece before it.
    hug: bool,
    fresh: bool,
}
impl Pieces<'_> {
    fn next_id(&mut self) -> String {
        let id = format!("{}-p{}", self.id, self.count);
        self.count += 1;
        id
    }
    fn gap(&mut self, hug: bool) {
        if !self.fresh && !hug {
            self.out.push(text(" "));
        }
        self.fresh = false;
    }
    fn flush(&mut self) {
        if self.run.is_empty() {
            return;
        }
        let hug = std::mem::take(&mut self.hug);
        self.gap(hug);
        let id = self.next_id();
        let run = emojify(&self.run.join(" "));
        self.out.push(span("run").id(id).text(run));
        self.run.clear();
    }
    fn piece(&mut self, build: impl FnOnce(String) -> Html, tail: &str) {
        self.flush();
        self.gap(false);
        let id = self.next_id();
        self.out.push(build(id));
        if !tail.is_empty() {
            self.hug = true;
            self.run.push(tail.into());
        }
    }
    fn line_break(&mut self) {
        self.flush();
        self.out.push(el("br"));
        self.fresh = true;
    }
}
/// A message's text as Discord sets it: plain runs with their emoji, `#channel` and
/// `@name` mentions on blurple, links in blue and code in the monospace face, wrapped
/// by the page at whatever width the chat has.
fn body(ctx: &Ctx, id: &str, message: &str) -> Html {
    let text_id = format!("{id}-text");
    let mut pieces = Pieces {
        id: &text_id,
        count: 0,
        out: vec![],
        run: vec![],
        hug: false,
        fresh: true,
    };
    for (n, line) in message.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        if n > 0 {
            pieces.line_break();
        }
        let words: Vec<&str> = line.split(' ').filter(|w| !w.is_empty()).collect();
        let mut i = 0;
        while i < words.len() {
            let word = words[i];
            if word.starts_with("http://") || word.starts_with("https://") {
                let (url, tail) = trailing(word);
                pieces.piece(|pid| a(url).id(pid).class("link").text(url), tail);
            } else if let Some(name) = word
                .strip_prefix('#')
                .filter(|name| ctx.state.server.channels.contains_key(trailing(name).0))
            {
                let (name, tail) = trailing(name);
                let url = format!("/channels/{}/{}", segment(&ctx.state.server.id), segment(name));
                pieces.piece(
                    |pid| a(url).id(pid).class("mention").text(format!("#{name}")),
                    tail,
                );
            } else if let Some(name) = word.strip_prefix('@').filter(|name| {
                let (name, _) = trailing(name);
                ctx.state
                    .server
                    .members
                    .iter()
                    .any(|(who, m)| who == name || m.nick == name)
            }) {
                let (name, tail) = trailing(name);
                pieces.piece(
                    |pid| span("mention").id(pid).text(format!("@{name}")),
                    tail,
                );
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
                let code = code.trim_matches('`').to_owned();
                pieces.piece(|pid| el("code").id(pid).text(code), "");
                i = end;
            } else {
                pieces.run.push(word.into());
            }
            i += 1;
        }
    }
    pieces.flush();
    div("text").id(text_id.as_str()).children(pieces.out)
}
/// The reactions under a message as Discord's pills: the emoji and its count, the
/// actor's own outlined in blurple; each one toggles the actor's reaction.
fn reactions(ctx: &Ctx, m: &Message) -> Option<Html> {
    if m.reactions.is_empty() {
        return None;
    }
    let action = format!("{}/messages/{}/reactions", ctx.url(), segment(&m.id));
    Some(
        form(&format!("{}-reactions", m.id), action, "post")
            .class("reactions")
            .each(&m.reactions, |(name, who)| {
                let mine = who.contains(ctx.actor);
                el("button")
                    .id(format!("{}-react-{name}", m.id))
                    .attr("type", "submit")
                    .attr("name", "reaction")
                    .attr("value", name.as_str())
                    .class("reaction")
                    .class(if mine { "mine" } else { "" })
                    .attr("aria-pressed", if mine { "true" } else { "false" })
                    .text(format!("{} {}", emoji(name), who.len()))
            }),
    )
}
/// Discord's reply line over a message: the spine from the avatar column, the quoted
/// author's small face and coloured name, and the start of what they said.
fn reply_line(ctx: &Ctx, m: &Message, quoted: &Message) -> Html {
    let id = &m.id;
    let line: String = emojify(&quoted.text)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let line = if line.chars().count() > 72 {
        let cut: String = line.chars().take(70).collect();
        format!("{}...", cut.trim_end())
    } else {
        line
    };
    let name = ctx.state.display(&quoted.author);
    div("quote").id(format!("{id}-quote")).children([
        span("spine").id(format!("{id}-quote-spine")).attr("aria-hidden", "true"),
        avatar(&format!("{id}-quote-avatar"), &name, "s16"),
        coloured(
            span("quote-author").id(format!("{id}-quote-author")).text(name.as_str()),
            role_style(ctx.state, &quoted.author),
        ),
        span("quote-text").id(format!("{id}-quote-text")).text(line),
    ])
}
/// The quick reactions Discord offers over the message under the pointer, and the
/// reply control. Each quick reaction is a submit button of the message's own
/// reactions form; reply opens the composer on this message, and closes it again
/// while it is already open. The emoji picker and the thread button are not here:
/// both want a client script, and this world runs none.
///
/// Every message carries the bar; the sheet shows it on hover, and on the newest
/// message always.
fn toolbar(ctx: &Ctx, m: &Message, replying: bool) -> Html {
    let action = format!("{}/messages/{}/reactions", ctx.url(), segment(&m.id));
    let quick = |name: &str| {
        let label = format!("React with {}", emoji(name));
        el("button")
            .id(format!("{}-quick-{name}", m.id))
            .attr("type", "submit")
            .attr("name", "reaction")
            .attr("value", name)
            .class("tool quick")
            .attr("title", label.as_str())
            .attr("aria-label", label)
            .text(emoji(name))
    };
    let (url, label) = match replying {
        true => (ctx.url(), "Cancel reply"),
        false => (
            html::href(&ctx.url(), &[("reply_to", m.id.as_str())]),
            "Reply",
        ),
    };
    form(&format!("{}-tools", m.id), action, "post")
        .class("tools")
        .children([
            quick("+1"),
            quick("eyes"),
            quick("tada"),
            a(url)
                .id(format!("{}-reply", m.id))
                .class("tool")
                .attr("title", label)
                .attr("aria-label", label)
                .text("\u{21A9}"),
        ])
}
/// One message: the author's avatar, name and time on the first of a run by one
/// author, the clock alone in the gutter on the rest; the reply line over a reply.
fn message(ctx: &Ctx, m: &Message, first: bool, newest: bool, replying: bool) -> Html {
    let id = &m.id;
    let quoted = m
        .reply_to
        .as_deref()
        .and_then(|r| ctx.channel.messages.iter().find(|q| q.id == r));
    let first = first || quoted.is_some();
    let name = ctx.state.display(&m.author);
    let mut row = el("article")
        .id(format!("{id}-row"))
        .class("message")
        .class(if first { "first" } else { "" })
        .class(if newest { "newest" } else { "" });
    if let Some(quoted) = quoted {
        row = row.child(reply_line(ctx, m, quoted));
    }
    row = if first {
        row.child(span("gutter face40").child(avatar(&format!("{id}-avatar"), &name, "s40"))).child(
            div("head").id(format!("{id}-head")).children([
                coloured(
                    span("author").id(format!("{id}-author")).text(name.as_str()),
                    role_style(ctx.state, &m.author),
                ),
                text(" "),
                el("time").id(format!("{id}-time")).class("stamp").text(time::stamp(m.time, ctx.now)),
            ]),
        )
    } else {
        row.child(
            el("time")
                .id(format!("{id}-time"))
                .class("gutter clock")
                .text(time::civil(m.time).clock()),
        )
    };
    row.child(body(ctx, id, &m.text))
        .maybe(reactions(ctx, m))
        .child(toolbar(ctx, m, replying))
}
fn day_divider(index: u64, label: String) -> Html {
    div("day")
        .id(format!("day-{index}"))
        .attr("role", "separator")
        .child(span("day-label").id(format!("day-{index}-label")).text(label))
}
/// The transcript: messages under date dividers, runs by one author grouped.
fn transcript(ctx: &Ctx, replying: Option<&str>) -> Html {
    let messages = &ctx.channel.messages;
    // The beginning of the channel, as Discord heads every channel's history.
    let mut out = div("transcript").id("transcript").child(
        div("start").id("channel-start").children([
            span("start-mark").attr("aria-hidden", "true").text("#"),
            el("h2")
                .id("channel-start-title")
                .class("start-title")
                .text(format!("Welcome to #{}!", ctx.id)),
            el("p")
                .id("channel-start-text")
                .class("start-text")
                .text(format!("This is the start of the #{} channel.", ctx.id))
                .when(!ctx.channel.topic.is_empty(), |n| {
                    n.text(format!(" {}", emojify(&ctx.channel.topic)))
                }),
        ]),
    );
    let mut previous: Option<&Message> = None;
    for (n, m) in messages.iter().enumerate() {
        let day = time::civil(m.time).index;
        let new_day = previous.is_none_or(|p| time::civil(p.time).index != day);
        if new_day {
            out = out.child(day_divider(day, time::day_label(m.time)));
        }
        let first = new_day
            || previous
                .is_none_or(|p| p.author != m.author || m.time.saturating_sub(p.time) > GROUP_US);
        out = out.child(message(
            ctx,
            m,
            first,
            n + 1 == messages.len(),
            replying == Some(m.id.as_str()),
        ));
        previous = Some(m);
    }
    out
}

// ---- the header, the composer and the member list ----------------------------------------

/// The search box in the channel header: a real GET form on `/search`, with the
/// magnifier as its submit button. `#search-text` labels the field.
fn search_form(query: &str) -> Html {
    form("search", "/search", "get").class("search").children([
        el("label")
            .id("search-text")
            .class("search-label")
            .attr("for", "search-q")
            .text("Search"),
        text_input("search-q", "q", query)
            .attr("aria-label", "Search")
            .attr("placeholder", "Search")
            .attr("autocomplete", "off"),
        el("button")
            .id("search-go")
            .attr("type", "submit")
            .class("search-icon")
            .attr("title", "Search")
            .attr("aria-label", "Search"),
    ])
}
/// The right end of the header: the member-list toggle, when a channel is open, and
/// the search box. Discord's threads, notifications, pins, inbox and help buttons are
/// not drawn: none of them has anywhere to go here.
fn head_tools(members: Option<Html>, query: &str) -> Html {
    div("head-tools").maybe(members).child(search_form(query))
}
/// The channel header: `#`, the name, the topic, and the toolbar with the search box.
fn header(ctx: &Ctx, members_open: bool) -> Html {
    let members_url = if members_open {
        format!("{}?members=0", ctx.url())
    } else {
        ctx.url()
    };
    let members_label = if members_open {
        "Hide Member List"
    } else {
        "Show Member List"
    };
    let mut head = div("channel-head").id("channel-head").children([
        span("hash big").id("channel-hash").attr("aria-hidden", "true").text("#"),
        el("h1").id("channel-title").class("channel-title").text(ctx.id),
    ]);
    if !ctx.channel.roles.is_empty() {
        head = head.child(icon("channel-private", "lock", "Private channel"));
    }
    if !ctx.channel.topic.is_empty() {
        head = head
            .child(span("topic-rule").id("channel-topic-rule").attr("aria-hidden", "true"))
            .child(span("topic").id("channel-topic").text(emojify(&ctx.channel.topic)));
    }
    head.child(head_tools(
        Some(
            a(members_url)
                .id("channel-members")
                .class("tool people")
                .class(if members_open { "on" } else { "" })
                .attr("aria-label", members_label)
                .attr("title", members_label)
                .children([el("i").class("a"), el("i").class("b")]),
        ),
        "",
    ))
}
/// The composer at the bottom of the chat: the field and the send button, over the
/// reply bar while the composer is answering a message. Discord's attach, gift, GIF,
/// sticker and emoji buttons are not drawn: each opens a picker this world cannot run.
fn composer(ctx: &Ctx, reply_to: Option<&Message>) -> Html {
    let label = format!("Message #{}", ctx.id);
    div("composer")
        .id("composer")
        .maybe(reply_to.map(|parent| {
            div("reply-bar").id("reply-bar").children([
                span("reply-bar-text").id("reply-bar-text").text(format!(
                    "Replying to {}",
                    ctx.state.display(&parent.author)
                )),
                a(ctx.url())
                    .id("reply-cancel")
                    .class("reply-cancel")
                    .attr("title", "Cancel reply")
                    .attr("aria-label", "Cancel reply")
                    .text("\u{2715}"),
            ])
        }))
        .child(
            form("send", format!("{}/messages", ctx.url()), "post")
                .class("field")
                .maybe(reply_to.map(|parent| hidden("reply_to", &parent.id)))
                .children([
                    text_input("send-text", "text", "")
                        .attr("aria-label", label.as_str())
                        .attr("placeholder", label.as_str())
                        .attr("autocomplete", "off"),
                    div("send-actions").id("send-actions").child(
                        button("send-submit", "\u{27A4}")
                            .class("send")
                            .attr("aria-label", "Send Message")
                            .attr("title", "Send Message"),
                    ),
                ]),
        )
}
/// The member list: the hoisted roles' online members under the role's name, everyone
/// else online under "Online", and everyone offline, dimmed, under "Offline".
fn members(state: &DiscordState, actor: &str, now: u64) -> Html {
    let mut roles: Vec<(&String, &crate::Role)> = state.server.roles.iter().collect();
    roles.sort_by_key(|(name, r)| (std::cmp::Reverse(r.position), (*name).clone()));
    // Discord lists a role's members apart only when the role is hoisted; here the
    // lowest role is everyone's base role and is not.
    let floor = roles.last().map(|(_, r)| r.position).unwrap_or_default();
    let mut list = el("aside").id("members").class("members").attr("aria-label", "Members");
    let mut placed = std::collections::BTreeSet::new();
    let group = |list: Html, key: &str, label: &str, who: Vec<&String>| -> Html {
        if who.is_empty() {
            return list;
        }
        list.child(
            el("h2")
                .id(format!("group-{key}"))
                .class("group")
                .text(format!("{} \u{2014} {}", label.to_uppercase(), who.len())),
        )
        .each(who, |member| {
            let on = online(state, member, actor, now);
            let name = state.display(member);
            div("member").class(if on { "" } else { "away" }).id(format!("member-{member}")).children([
                span("face").children([
                    avatar(&format!("member-{member}-avatar"), &name, "s32"),
                    presence(&format!("member-{member}-presence"), on),
                ]),
                coloured(
                    span("member-name").id(format!("member-{member}-name")).text(name.as_str()),
                    role_style(state, member).filter(|_| on),
                ),
            ])
        })
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
        list = group(list, name, name, who);
    }
    let rest = |on: bool| -> Vec<&String> {
        state
            .server
            .members
            .keys()
            .filter(|m| !placed.contains(*m) && online(state, m, actor, now) == on)
            .collect()
    };
    list = group(list, "online", "Online", rest(true));
    group(list, "offline", "Offline", rest(false))
}

/// The whole server: the rail, the sidebar with the user panel under it, the open
/// channel with its composer, and the member list while it is open.
pub fn server(
    state: &DiscordState,
    actor: &str,
    open: Option<&str>,
    view: &View,
) -> SimResult<HttpResponse> {
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
    });
    let name = server_name(state);
    let title = match id {
        Some(id) => format!("#{id} \u{b7} {name}"),
        None => name.clone(),
    };
    let side = side(state, actor, id);
    let channel_header = match &ctx {
        Some(ctx) => header(ctx, members_open),
        None => el("p").id("empty").class("empty").text(if member {
            "Pick a channel."
        } else {
            "You are not in this server."
        }),
    };
    // The composer answers a message only while `?reply_to=` names one in this channel.
    let replying = ctx.as_ref().and_then(|ctx| {
        view.reply_to
            .as_deref()
            .and_then(|r| ctx.channel.messages.iter().find(|m| m.id == r))
    });
    let mut chat = div("chat").child(
        el("main")
            .id("main")
            .class("scroller")
            .maybe(ctx.as_ref().map(|ctx| transcript(ctx, replying.map(|m| m.id.as_str())))),
    );
    if let Some(ctx) = &ctx {
        chat = chat.child(composer(ctx, replying));
    }
    let mut shell = div("shell").id("shell").child(chat);
    if members_open {
        shell = shell.child(members(state, actor, now));
    }
    document(state, title, side, channel_header, shell)
}

/// The sidebar column: the server name, the channels and the user panel.
fn side(state: &DiscordState, actor: &str, open: Option<&str>) -> Html {
    let name = server_name(state);
    div("side").children([
        el("header").id("sidebar-head").class("side-head").children([
            span("server-name").id("server-name").text(name.as_str()),
            mark("server-menu", "chevron", "\u{2304}"),
        ]),
        sidebar(state, actor, open),
        user_panel(state, actor),
    ])
}
/// The page every view shares: the rail, the sidebar, the header band and the shell.
fn document(
    state: &DiscordState,
    title: String,
    side: Html,
    head: Html,
    shell: Html,
) -> SimResult<HttpResponse> {
    let content = div("content").children([
        el("header").id("header").class("top").child(head),
        shell,
    ]);
    let mut doc = Document::new(title)
        .lang("en")
        .stylesheet(CSS)
        .body_class("discord")
        .body([div("app").id("app").children([rail(state), side, content])]);
    let root = root_style(state);
    if !root.is_empty() {
        doc = doc.root_style(&root);
    }
    html::page(&doc)
}
/// What the header's search box finds: every message of a channel the actor can see
/// whose text holds the query, newest first, each one a link into its channel.
pub fn search(
    state: &DiscordState,
    actor: &str,
    query: &str,
    view: &View,
) -> SimResult<HttpResponse> {
    let now = now(state, view.tick);
    let needle = query.trim().to_lowercase();
    let mut hits: Vec<(&str, &Message)> = Vec::new();
    if !needle.is_empty() {
        for id in state.visible(actor) {
            let Ok(channel) = state.channel(actor, id) else {
                continue;
            };
            for m in &channel.messages {
                if m.text.to_lowercase().contains(&needle) {
                    hits.push((id, m));
                }
            }
        }
        hits.sort_by(|a, b| b.1.time.cmp(&a.1.time).then(a.1.id.cmp(&b.1.id)));
        hits.truncate(50);
    }
    let summary = match (needle.is_empty(), hits.len()) {
        (true, _) => "Type in the box above to search this server".to_owned(),
        (false, 1) => format!("1 result for \u{201c}{}\u{201d}", query.trim()),
        (false, n) => format!("{n} results for \u{201c}{}\u{201d}", query.trim()),
    };
    let head = div("channel-head").id("channel-head").children([
        el("h1").id("search-title").class("channel-title").text("Search"),
        span("topic-rule").attr("aria-hidden", "true"),
        span("topic").id("search-summary").text(summary),
        head_tools(None, query),
    ]);
    let mut results = div("results").id("search-results");
    for (n, (channel, m)) in hits.iter().enumerate() {
        let id = format!("hit-{n}");
        results = results.child(
            a(format!(
                "/channels/{}/{}",
                segment(&state.server.id),
                segment(channel)
            ))
            .id(id.as_str())
            .class("hit")
            .children([
                div("hit-head").children([
                    span("hit-where").id(format!("{id}-where")).text(format!("#{channel}")),
                    span("hit-author")
                        .id(format!("{id}-author"))
                        .text(state.display(&m.author)),
                    span("hit-time")
                        .id(format!("{id}-time"))
                        .text(time::stamp(m.time, now)),
                ]),
                div("hit-text").id(format!("{id}-text")).text(emojify(&m.text)),
            ]),
        );
    }
    if hits.is_empty() && !needle.is_empty() {
        results = results.child(
            el("p")
                .id("search-empty")
                .class("empty")
                .text(format!("No message here says \u{201c}{}\u{201d}.", query.trim())),
        );
    }
    let shell = div("shell")
        .id("shell")
        .child(div("chat").child(el("main").id("main").class("scroller").child(results)));
    let title = format!("Search \u{b7} {}", server_name(state));
    document(state, title, side(state, actor, None), head, shell)
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
    fn text_splits_into_runs_links_mentions_and_code() {
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
        };
        let html = body(
            &ctx,
            "m",
            "@bmartinez read #rules, then http://github.com/x/y and run `cargo test` :eyes:\nsecond <line>",
        )
        .render();
        assert_eq!(
            html,
            "<div class=\"text\" id=\"m-text\">\
             <span class=\"mention\" id=\"m-text-p0\">@bmartinez</span> \
             <span class=\"run\" id=\"m-text-p1\">read</span> \
             <a href=\"/channels/atlas/rules\" id=\"m-text-p2\" class=\"mention\">#rules</a>\
             <span class=\"run\" id=\"m-text-p3\">, then</span> \
             <a href=\"http://github.com/x/y\" id=\"m-text-p4\" class=\"link\">http://github.com/x/y</a> \
             <span class=\"run\" id=\"m-text-p5\">and run</span> \
             <code id=\"m-text-p6\">cargo test</code> \
             <span class=\"run\" id=\"m-text-p7\">👀</span><br>\
             <span class=\"run\" id=\"m-text-p8\">second &lt;line&gt;</span></div>"
        );
    }
}
