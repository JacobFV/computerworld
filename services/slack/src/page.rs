//! The Slack workspace page, served as HTML and laid out the way Slack's desktop web
//! client is: an aubergine frame holding a top bar with the search box, a narrow rail
//! of workspace tabs, the sidebar of channels and direct messages, the open conversation
//! with its messages grouped by author under date dividers and scrolling on its own, a
//! composer pinned under it, and, on the right, the thread or the member list the query
//! string opens. The same frame carries the two pages its own controls lead to: what
//! the top bar's search box finds, and the Activity tab's unread mentions. The look is
//! `slack.css`; this file is the markup, apart from the state and the routes.
//!
//! Everything drawn here that looks like a control is one. What Slack's client does
//! with a script — the emoji picker, the formatting strip, attach, the workspace and
//! channel menus, Later and More — is not drawn at all rather than drawn dead, and the
//! marks that are left (carets, hashes, locks, presence dots) carry no name that
//! promises an action. `tests/controls.rs` crawls every page kind to keep it so.
use crate::{time, Channel, Message, SlackState};
use cw_protocol::{HttpResponse, Result as SimResult};
use cw_service_common as web;
use web::html::{self, button, div, el, form, hidden, span, text_input, Document, Html};

const CSS: &str = include_str!("slack.css");
const BRAND: &str = "Slack";

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
/// Messages by one author this close together share one header, as Slack groups them.
const GROUP_US: u64 = 10 * time::MINUTE_US;

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


/// A person's initials on a rounded square in their own colour, Slack's avatar shape.
/// `size` is one of the sheet's avatar sizes (`s20`, `s24`, `s26`, `s32`, `s36`).
fn avatar(id: &str, name: &str, size: &str) -> Html {
    let initials: String = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
        .filter(|w| !w.is_empty())
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    span("avatar")
        .class(size)
        .id(id)
        .style(&format!("background: {}", web::avatar_tint(name)))
        .text(initials)
}
/// A symbol that is not a control: a glyph with its accessible name. The name says
/// what it *is* ("Private channel", "Pinned"), never what pressing it would do.
fn glyph(id: &str, class: &str, label: &str, mark: &str) -> Html {
    span("ico")
        .class(class)
        .id(id)
        .attr("title", label)
        .attr("aria-label", label)
        .text(mark)
}
/// Pure ornament: the caret beside a heading that is always open, the workspace mark.
/// No accessible name, because there is nothing to press and nothing to read.
fn ornament(id: &str, class: &str, mark: &str) -> Html {
    span("ico")
        .class(class)
        .id(id)
        .attr("aria-hidden", "true")
        .text(mark)
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
/// The presence dot: filled green while active, a hollow ring while away.
fn presence(id: &str, on: bool) -> Html {
    let label = if on { "Active" } else { "Away" };
    span("presence")
        .class(if on { "on" } else { "" })
        .id(id)
        .attr("title", label)
        .attr("aria-label", label)
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
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---- the top bar and the rail --------------------------------------------------------

/// The top bar's search box: a real GET form on `/search`, the magnifier its submit
/// button and `#search-text` the label of its field. Slack's back, forward, history
/// and help buttons are not drawn: none of them has anywhere to go here.
fn search_form(workspace: &str, query: &str) -> Html {
    let label = format!("Search {workspace}");
    form("search", "/search", "get").class("search").children([
        el("button")
            .id("search-go")
            .attr("type", "submit")
            .class("lens")
            .attr("title", label.as_str())
            .attr("aria-label", label.as_str()),
        el("label")
            .id("search-text")
            .class("search-text")
            .attr("for", "search-q")
            .text(label.as_str()),
        text_input("search-q", "q", query)
            .attr("aria-label", label.as_str())
            .attr("placeholder", label)
            .attr("autocomplete", "off"),
    ])
}
fn topbar(state: &SlackState, actor: &str, workspace: &str, query: &str) -> Html {
    let status = state
        .members
        .get(actor)
        .map(|m| m.status.clone())
        .unwrap_or_default();
    el("header").id("topbar").class("topbar").children([
        div("topbar-nav"),
        search_form(workspace, query),
        div("topbar-me").children([
            span("my-status").id("my-status").text(status),
            avatar("me-avatar", &state.display(actor), "s26"),
        ]),
    ])
}
/// The rail: the workspace tile, then Home, DMs and Activity, each an icon over its
/// label. Slack's Later and More tabs are not drawn: neither names anything here.
fn rail(state: &SlackState, actor: &str, workspace: &str, home: bool, dms: bool, activity: bool) -> Html {
    let mentions = state.mentions(actor).len();
    let tab = |id: &str, icon: &str, label: &str, on: bool, url: &str| {
        el("a")
            .attr("href", url)
            .id(id)
            .class("tab")
            .class(if on { "on" } else { "" })
            .attr("title", label)
            .children([
                span("tab-icon").class(icon),
                span("tab-label").id(format!("{id}-label")).text(label),
            ])
    };
    let initial = workspace
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    el("nav").id("rail").class("rail").children([
        span("rail-mark").id("rail-mark").text(initial),
        tab("rail-home", "i-home", "Home", home, "/"),
        tab("rail-dms", "i-dms", "DMs", dms, "/dms"),
        tab("rail-activity", "i-bell", "Activity", activity, "/activity").when(mentions > 0, |t| {
            t.child(
                span("count")
                    .id("rail-activity-count")
                    .text(mentions.to_string()),
            )
        }),
    ])
}

// ---- the sidebar ---------------------------------------------------------------------

/// A sidebar heading. Nothing collapses a section here, so its caret is only a mark.
fn section(id: &str, label: &str) -> Html {
    div("section").id(id).children([
        ornament(&format!("{id}-chevron"), "caret", "▾"),
        span("section-text").id(format!("{id}-text")).text(label),
    ])
}
/// How a sidebar row stands: open, unread, and the count on its badge (unread mentions
/// for a channel, unread messages for a DM).
#[derive(Clone, Copy, Default)]
struct Standing {
    current: bool,
    unread: bool,
    badge: usize,
}
/// The inside of a sidebar row: its mark, its name (bold while unread) and its count.
fn row_inner(id: &str, mark: Html, text: String, standing: Standing) -> Vec<Html> {
    let mut children = vec![mark, span("row-text").id(format!("{id}-text")).text(text)];
    if standing.badge > 0 {
        children.push(
            span("count")
                .id(format!("{id}-unread"))
                .text(standing.badge.to_string()),
        );
    }
    children
}
fn row_class(standing: Standing) -> &'static str {
    match (standing.current, standing.unread) {
        (true, _) => "row current",
        (false, true) => "row unread",
        (false, false) => "row",
    }
}
/// One conversation in the sidebar, a link to it.
fn sidebar_row(id: &str, mark: Html, text: String, url: String, standing: Standing) -> Html {
    el("a")
        .id(id)
        .class(row_class(standing))
        .attr("href", url)
        .children(row_inner(id, mark, text, standing))
}
fn sidebar(state: &SlackState, actor: &str, open: Option<&str>, now: u64, workspace: &str) -> Html {
    let mut items = vec![section("channels-label", "Channels")];
    let mentions = state.mentions(actor);
    for (id, channel) in &state.channels {
        if !channel.members.contains(actor) {
            continue;
        }
        let mark = if channel.private {
            glyph(&format!("nav-{id}-lock"), "mark lock", "Private channel", "")
        } else {
            glyph(&format!("nav-{id}-hash"), "mark", "Channel", "#")
        };
        items.push(sidebar_row(
            &format!("nav-{id}"),
            mark,
            id.clone(),
            format!("/channels/{id}"),
            // A channel is bold while it is unread and badged with its unread mentions;
            // a DM is badged with every unread message.
            Standing {
                current: open == Some(id.as_str()),
                unread: state.unread(actor, id) > 0,
                badge: mentions.iter().filter(|(at, _)| *at == id).count(),
            },
        ));
    }
    items.push(section("dms-label", "Direct messages"));
    let mut listed = std::collections::BTreeSet::new();
    for (key, dm) in &state.dms {
        if !dm.members.contains(actor) {
            continue;
        }
        let people = others(key, actor);
        let mark = match people.as_slice() {
            [one] => {
                listed.insert((*one).to_owned());
                span("face").id(format!("dm-{key}-mark")).children([
                    avatar(&format!("dm-{key}-avatar"), &state.display(one), "s20"),
                    presence(
                        &format!("dm-{key}-presence"),
                        active(state, one, actor, now),
                    ),
                ])
            }
            _ => span("avatar s20 group")
                .id(format!("dm-{key}-avatar"))
                .text(people.len().to_string()),
        };
        items.push(sidebar_row(
            &format!("dm-{key}"),
            mark,
            partners(state, key, actor),
            format!("/channels/{key}"),
            Standing {
                current: open == Some(key.as_str()),
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
        let id = format!("start-{who}");
        let mark = span("face").id(format!("{id}-mark")).children([
            avatar(&format!("{id}-avatar"), &state.display(&who), "s20"),
            presence(&format!("{id}-presence"), active(state, &who, actor, now)),
        ]);
        items.push(
            form(&format!("{id}-form"), "/dms", "post")
                .class("start")
                .child(hidden("to", &who))
                .child(
                    el("button")
                        .id(id.as_str())
                        .attr("type", "submit")
                        .class("row")
                        .children(row_inner(&id, mark, state.display(&who), Standing::default())),
                ),
        );
    }
    el("aside").id("sidebar").class("sidebar").children([
        div("sidebar-head").id("sidebar-head").children([
            span("workspace").id("workspace").text(workspace),
            ornament("workspace-menu", "caret light", "▾"),
        ]),
        el("nav").id("sidebar-list").class("sidebar-list").children(items),
    ])
}

// ---- messages ------------------------------------------------------------------------

/// Everything a message block needs to know about where it is drawn.
struct Ctx<'a> {
    state: &'a SlackState,
    id: &'a str,
    channel: &'a Channel,
    actor: &'a str,
    now: u64,
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
/// A word and the punctuation that closes it, which stays plain text.
fn trailing(w: &str) -> (&str, &str) {
    let end = w.trim_end_matches(['.', ',', ';', ':', ')', ']', '!', '?']);
    (end, &w[end.len()..])
}
/// A line of a message as Slack sets it: plain runs, mentions on their tint, links in
/// blue and code in the monospace face. `piece` numbers the links and marks of the
/// whole message so their ids (`<id>-p<n>`) stay unique across its lines.
fn inline_line(id: &str, line: &str, mentions: &[&str], piece: &mut usize) -> Html {
    let words: Vec<&str> = line.split(' ').filter(|w| !w.is_empty()).collect();
    let mut out = div("line");
    let mut run = String::new();
    fn flush(run: &mut String, out: Html) -> Html {
        if run.is_empty() {
            return out;
        }
        let text = emojify(run);
        run.clear();
        out.text(text)
    }
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if i > 0 {
            run.push(' ');
        }
        if word.starts_with("http://") || word.starts_with("https://") {
            let (url, tail) = trailing(word);
            out = flush(&mut run, out);
            let shown = url
                .trim_start_matches("http://")
                .trim_start_matches("https://");
            out = out.child(html::link(&format!("{id}-p{piece}"), url, shown));
            *piece += 1;
            run.push_str(tail);
        } else if let Some(name) = word
            .strip_prefix('@')
            .filter(|name| mentions.iter().any(|m| name.starts_with(m)))
        {
            let (name, tail) = trailing(name);
            out = flush(&mut run, out);
            out = out.child(
                span("mention")
                    .id(format!("{id}-p{piece}"))
                    .text(format!("@{name}")),
            );
            *piece += 1;
            run.push_str(tail);
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
            let (code, tail) = trailing(&code);
            out = flush(&mut run, out);
            out = out.child(
                el("code")
                    .id(format!("{id}-p{piece}"))
                    .text(code.trim_matches('`')),
            );
            *piece += 1;
            run.push_str(tail);
            i = end;
        } else {
            run.push_str(word);
        }
        i += 1;
    }
    flush(&mut run, out)
}
/// A message's text: one block per line, wrapped by the page.
fn body(id: &str, text: &str, mentions: &[&str]) -> Html {
    let id = format!("{id}-text");
    let mut piece = 0;
    let mut out = div("text").id(id.as_str());
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        out = out.child(inline_line(&id, line, mentions, &mut piece));
    }
    out
}
/// The reaction chips under a message: each reacts when pressed, the actor's own
/// outlined in blue.
fn reactions(ctx: &Ctx, id: &str, m: &Message) -> Option<Html> {
    if m.reactions.is_empty() {
        return None;
    }
    let action = format!("{}/messages/{}/reactions", ctx.url(), m.id);
    Some(
        form(&format!("{id}-reactions"), action, "post")
            .class("reactions")
            .each(&m.reactions, |(name, who)| {
                button(
                    &format!("{id}-react-{name}"),
                    format!("{} {}", emoji(name), who.len()),
                )
                .class("chip")
                .class(if who.contains(ctx.actor) { "mine" } else { "" })
                .attr("name", "reaction")
                .attr("value", name.as_str())
            }),
    )
}
/// The collapsed thread under a message: the repliers' faces, "N replies" and the age
/// of the last, opening the thread pane.
fn thread_row(ctx: &Ctx, m: &Message, replies: &[&Message]) -> Html {
    let mut row = div("thread-row").id(format!("{}-thread", m.id));
    let mut seen = std::collections::BTreeSet::new();
    for reply in replies {
        if seen.insert(reply.author.as_str()) && seen.len() <= 3 {
            row = row.child(avatar(
                &format!("{}-thread-avatar-{}", m.id, reply.author),
                &ctx.state.display(&reply.author),
                "s24",
            ));
        }
    }
    row = row.child(html::link(
        &format!("{}-replies", m.id),
        format!("{}?thread={}", ctx.url(), m.id),
        plural(replies.len(), "reply", "replies"),
    ));
    if let Some(last) = replies.iter().map(|r| r.time).max() {
        row = row.child(
            span("last-reply")
                .id(format!("{}-last-reply", m.id))
                .text(format!("Last reply {}", time::ago(last, ctx.now))),
        );
    }
    row
}
/// The actions Slack floats over the message under the pointer: the quick reactions,
/// reply in thread and pin. The emoji picker and the overflow menu are not here: both
/// want a client script, and this world runs none.
fn toolbar(ctx: &Ctx, m: &Message) -> Html {
    let base = format!("{}/messages/{}", ctx.url(), m.id);
    let quick = |name: &str| {
        button(&format!("{}-quick-{name}", m.id), emoji(name))
            .attr("name", "reaction")
            .attr("value", name)
    };
    let pin_label = if ctx.channel.pins.contains(&m.id) {
        "Unpin from channel"
    } else {
        "Pin to channel"
    };
    form(&format!("{}-tools", m.id), format!("{base}/reactions"), "post")
        .class("tools")
        .children([
            quick("white_check_mark"),
            quick("eyes"),
            quick("+1"),
            el("a")
                .id(format!("{}-open-thread", m.id))
                .class("tool bubble")
                .attr("href", format!("{}?thread={}", ctx.url(), m.id))
                .attr("title", "Reply in thread")
                .attr("aria-label", "Reply in thread"),
            // The `formmethod` is the form's own, spelled out: a reader that takes a
            // bare `formaction` for a link would ask for this route with GET, which
            // is not a route at all.
            button(&format!("{}-pin", m.id), "📌")
                .class("tool")
                .attr("formaction", format!("{base}/pin"))
                .attr("formmethod", "post")
                .attr("title", pin_label)
                .attr("aria-label", pin_label),
        ])
}
/// Where a message is drawn, which decides what surrounds its text.
#[derive(Clone, Copy)]
struct Placement {
    /// The first of a run by one author: avatar, name and time; the rest carry the
    /// time alone in the gutter.
    first: bool,
    /// The last of the transcript keeps its toolbar showing; the others show theirs
    /// under the pointer.
    last: bool,
    /// The transcript carries toolbars and thread rows; the thread pane does not.
    transcript: bool,
}
fn message(ctx: &Ctx, prefix: &str, m: &Message, at: Placement, replies: &[&Message]) -> Html {
    let id = &format!("{prefix}{}", m.id);
    let stamp = time::civil(m.time).clock();
    let mentions = m.mentions();
    let mut inner = div("msg-body").id(format!("{id}-body"));
    if ctx.channel.pins.contains(&m.id) {
        inner = inner.child(div("pinned").id(format!("{id}-pinned")).children([
            glyph(&format!("{id}-pin-icon"), "pin", "Pinned", "📌"),
            span("").id(format!("{id}-pinned-text")).text("Pinned to this channel"),
        ]));
    }
    if at.first {
        inner = inner.child(div("msg-head").id(format!("{id}-head")).children([
            span("author")
                .id(format!("{id}-author"))
                .text(ctx.state.display(&m.author)),
            span("time").id(format!("{id}-time")).text(stamp.clone()),
        ]));
    }
    inner = inner
        .child(body(id, &m.text, &mentions))
        .maybe(reactions(ctx, id, m));
    if at.transcript && !replies.is_empty() {
        inner = inner.child(thread_row(ctx, m, replies));
    }
    let gutter = if at.first {
        div("gutter").child(avatar(
            &format!("{id}-avatar"),
            &ctx.state.display(&m.author),
            "s36",
        ))
    } else {
        div("gutter").child(span("time").id(format!("{id}-time")).text(stamp))
    };
    div("msg")
        .id(format!("{id}-row"))
        .class(if at.first { "first" } else { "" })
        .class(if at.last { "last" } else { "" })
        .class(if ctx.channel.pins.contains(&m.id) { "is-pinned" } else { "" })
        .class(if mentions.contains(&ctx.actor) { "mentioned" } else { "" })
        .child(gutter)
        .child(inner)
        .when(at.transcript, |row| row.child(toolbar(ctx, m)))
}
fn day_divider(index: u64, label: String) -> Html {
    div("day")
        .id(format!("day-{index}"))
        .child(span("day-label").id(format!("day-{index}-label")).text(label))
}
/// The transcript: top-level messages under date dividers, runs by one author grouped.
fn transcript(ctx: &Ctx) -> Vec<Html> {
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
            out.push(day_divider(day, time::day_label(m.time, ctx.now)));
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
        let at = Placement {
            first,
            last: n + 1 == tops.len(),
            transcript: true,
        };
        out.push(message(ctx, "", m, at, &replies));
        previous = Some(m);
    }
    out
}

// ---- the header, the composer and the panes ------------------------------------------

/// The channel header: name and topic, the member count opening the member list (and
/// closing it again while it is open), and the bar of pins and purpose under it.
fn header(ctx: &Ctx, members_open: bool) -> Html {
    let heading = ctx.heading();
    let is_channel = ctx.state.channels.contains_key(ctx.id);
    let title = el("h1").id("channel-title").class("channel-title").text(heading);
    let mut line = div("channel-head").id("channel-head");
    if is_channel {
        line = line
            .child(title)
            .child(ornament("channel-menu", "caret dark", "▾"));
    } else if let [one] = others(ctx.id, ctx.actor).as_slice() {
        line = line
            .child(avatar("channel-avatar", &ctx.state.display(one), "s24"))
            .child(title)
            .child(presence(
                "channel-presence",
                active(ctx.state, one, ctx.actor, ctx.now),
            ));
        if let Some(member) = ctx.state.members.get(*one).filter(|m| !m.status.is_empty()) {
            line = line.child(
                span("channel-status")
                    .id("channel-status")
                    .text(member.status.as_str()),
            );
        }
    } else {
        line = line.child(title);
    }
    if !ctx.channel.topic.is_empty() {
        line = line.child(
            span("channel-topic")
                .id("channel-topic")
                .text(ctx.channel.topic.as_str()),
        );
    }
    let (members_url, members_label) = match members_open {
        true => (ctx.url(), "Close the member list"),
        false => (format!("{}?members=1", ctx.url()), "View all members"),
    };
    line = line.child(span("grow")).child(
        el("a")
            .id("channel-members")
            .class("channel-members")
            .attr("href", members_url)
            .attr("title", members_label)
            .attr("aria-label", members_label)
            .children([
                glyph("channel-members-icon", "person", "Members", ""),
                span("")
                    .id("channel-members-count")
                    .text(ctx.channel.members.len().to_string()),
            ]),
    );
    let mut head = el("header")
        .id("channel-header")
        .class("channel-header")
        .child(line);
    if !ctx.channel.pins.is_empty() || !ctx.channel.purpose.is_empty() {
        let mut bar = div("channel-bar").id("channel-bar");
        if !ctx.channel.pins.is_empty() {
            bar = bar.child(span("bar-item").children([
                glyph("channel-pins-icon", "pin", "Pinned", "📌"),
                span("")
                    .id("channel-pins")
                    .text(format!("{} Pinned", ctx.channel.pins.len())),
            ]));
        }
        if !ctx.channel.purpose.is_empty() {
            bar = bar.child(span("bar-item").children([
                glyph("channel-purpose-icon", "info", "Purpose", "i"),
                span("")
                    .id("channel-purpose")
                    .text(ctx.channel.purpose.as_str()),
            ]));
        }
        head = head.child(bar);
    }
    head
}
/// What the right-hand pane holds.
enum Pane<'a> {
    None,
    Thread(&'a Message),
    Members,
}
/// The thread's reply field, at the bottom of its pane and level with the composer.
fn thread_composer(ctx: &Ctx, parent: &Message) -> Html {
    let id = &parent.id;
    div("composer").id("thread-composer").child(
        form(&format!("{id}-reply"), format!("{}/messages", ctx.url()), "post")
            .class("composer-box")
            .children([
                hidden("parent", id),
                text_input(&format!("{id}-reply-body"), "text", "")
                    .attr("aria-label", "Reply in thread")
                    .attr("placeholder", "Reply…")
                    .attr("autocomplete", "off"),
                div("send-actions").children([
                    span("grow"),
                    button(&format!("{id}-reply-submit"), "Reply").class("send"),
                ]),
            ]),
    )
}
/// The composer under the transcript: a bordered box with the field and the send
/// button. Slack's formatting strip, attach, emoji and mention buttons are not drawn:
/// every one of them wants a client script, and this world runs none.
fn composer(ctx: &Ctx) -> Html {
    let heading = ctx.heading();
    div("composer").id("composer").child(
        form("send", format!("{}/messages", ctx.url()), "post")
            .class("composer-box")
            .children([
                text_input("send-text", "text", "")
                    .attr("aria-label", format!("Message {heading}"))
                    .attr("placeholder", format!("Message {}", heading.replace("# ", "#")))
                    .attr("autocomplete", "off"),
                div("send-actions").id("send-actions").children([
                    span("grow"),
                    button("send-submit", "➤")
                        .class("send")
                        .attr("title", "Send message")
                        .attr("aria-label", "Send message"),
                ]),
            ]),
    )
}
fn pane_head(id: &str, title: Html, close_id: &str, close_label: &str, url: String) -> Html {
    div("pane-head").id(id).children([
        title,
        span("grow"),
        el("a")
            .id(close_id)
            .class("close")
            .attr("href", url)
            .attr("title", close_label)
            .attr("aria-label", close_label)
            .text("✕"),
    ])
}
/// The thread pane: the parent, its replies, and the reply field.
fn thread_pane(ctx: &Ctx, parent: &Message) -> Html {
    let replies: Vec<&Message> = ctx
        .channel
        .messages
        .iter()
        .filter(|r| r.parent.as_deref() == Some(parent.id.as_str()))
        .collect();
    let pane = Placement {
        first: true,
        last: false,
        transcript: false,
    };
    // The parent is also in the transcript, so its ids are the pane's own here.
    let mut list = div("scroller")
        .id("thread-messages")
        .child(message(ctx, "thread-", parent, pane, &[]));
    if !replies.is_empty() {
        list = list.child(
            div("thread-count").id("thread-count").child(
                span("")
                    .id("thread-count-text")
                    .text(plural(replies.len(), "reply", "replies")),
            ),
        );
    }
    let mut previous: Option<&Message> = None;
    for reply in &replies {
        let first = previous.is_none_or(|p| {
            p.author != reply.author || reply.time.saturating_sub(p.time) > GROUP_US
        });
        list = list.child(message(ctx, "", reply, Placement { first, ..pane }, &[]));
        previous = Some(reply);
    }
    el("aside").id("thread-pane").class("pane").children([
        pane_head(
            "thread-head",
            fragment_title("thread-title", "Thread", "thread-channel", &ctx.heading()),
            "thread-close",
            "Close thread",
            ctx.url(),
        ),
        list,
        thread_composer(ctx, parent),
    ])
}
fn fragment_title(id: &str, title: &str, sub_id: &str, sub: &str) -> Html {
    html::fragment([
        span("pane-title").id(id).text(title),
        span("pane-sub").id(sub_id).text(sub),
    ])
}
/// The member list of the open conversation: face, name, title and status.
fn members_pane(ctx: &Ctx) -> Html {
    let mut list = div("scroller").id("members-list");
    for who in &ctx.channel.members {
        let member = ctx.state.members.get(who);
        let mut lines = div("member-lines").id(format!("member-{who}-lines")).child(
            div("member-name").id(format!("member-{who}-name")).children([
                span("author")
                    .id(format!("member-{who}"))
                    .text(ctx.state.display(who)),
                presence(
                    &format!("member-{who}-presence"),
                    active(ctx.state, who, ctx.actor, ctx.now),
                ),
            ]),
        );
        if let Some(m) = member {
            if !m.title.is_empty() {
                lines = lines.child(
                    div("member-title")
                        .id(format!("member-{who}-title"))
                        .text(m.title.as_str()),
                );
            }
            if !m.status.is_empty() {
                lines = lines.child(
                    div("member-status")
                        .id(format!("member-{who}-status"))
                        .text(m.status.as_str()),
                );
            }
        }
        list = list.child(div("member").id(format!("member-{who}-card")).children([
            avatar(&format!("member-{who}-avatar"), &ctx.state.display(who), "s36"),
            lines,
        ]));
    }
    el("aside").id("members").class("pane").children([
        pane_head(
            "members-head",
            span("pane-title")
                .id("members-label")
                .text(format!("Members · {}", ctx.channel.members.len())),
            "members-close",
            "Close",
            ctx.url(),
        ),
        list,
    ])
}
/// The banner over unread messages, with the one control that marks them read.
fn unread_banner(ctx: &Ctx, unread: usize) -> Html {
    form("read", format!("{}/read", ctx.url()), "post")
        .class("unread-banner")
        .children([
            span("")
                .id("read-count")
                .text(plural(unread, "new message", "new messages")),
            span("grow"),
            button("read-submit", "Mark as read"),
        ])
}
/// A seeded theme overrides the sheet's palette through custom properties on `<html>`.
fn root_style(state: &SlackState) -> Option<String> {
    let theme = state.theme.as_ref()?;
    let pairs = [
        ("--accent", &theme.accent),
        ("--frame", &theme.background),
        ("--surface", &theme.surface),
        ("--ink", &theme.ink),
        ("--muted", &theme.muted),
    ];
    let css: Vec<String> = pairs
        .iter()
        .filter_map(|(name, value)| {
            let value = value.as_deref()?;
            // Only a plain colour goes into the attribute.
            value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '#')
                .then(|| format!("{name}: {value}"))
        })
        .collect();
    (!css.is_empty()).then(|| css.join("; "))
}

/// The whole workspace: top bar, rail, sidebar, the open conversation, the composer,
/// and the pane the query string asked for.
pub fn workspace(
    state: &SlackState,
    actor: &str,
    open: Option<&str>,
    view: &View,
) -> SimResult<HttpResponse> {
    let now = now(state, actor, view.tick);
    // The root opens the first channel the actor is in, as Slack lands on one.
    let open = open.or_else(|| {
        state
            .channels
            .iter()
            .find(|(_, c)| c.members.contains(actor))
            .map(|(id, _)| id.as_str())
    });
    let ctx = match open {
        None => None,
        Some(id) => match state.channel(actor, id) {
            Err(e) => return web::error(403, e),
            Ok(channel) => Some(Ctx {
                state,
                id,
                channel,
                actor,
                now,
            }),
        },
    };
    let id = ctx.as_ref().map(|ctx| ctx.id);
    let is_dm = id.is_some_and(|id| state.dms.contains_key(id));
    let workspace_name = if state.workspace.is_empty() {
        BRAND.to_owned()
    } else {
        state.workspace.clone()
    };
    let title = match &ctx {
        Some(ctx) if is_dm => format!("{} (DM) - {workspace_name} - {BRAND}", ctx.heading()),
        Some(ctx) => format!("#{} (Channel) - {workspace_name} - {BRAND}", ctx.id),
        None => format!("{workspace_name} - {BRAND}"),
    };
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
    // The conversation: its header, the transcript scrolling on its own, the composer.
    let mut main = el("main").id("main").class("main");
    match &ctx {
        Some(ctx) => {
            let unread = state.unread(actor, ctx.id);
            main = main.child(header(ctx, matches!(&pane, Pane::Members))).child(
                div("scroller")
                    .id("messages")
                    .when(unread > 0, |list| list.child(unread_banner(ctx, unread)))
                    .children(transcript(ctx)),
            );
            main = main.child(composer(ctx));
        }
        None => {
            main = main.child(
                el("p")
                    .id("empty")
                    .class("empty")
                    .text("Pick a channel or start a direct message."),
            );
        }
    }
    let mut panel = div("panel").id("shell").children([
        sidebar(state, actor, id, now, &workspace_name),
        main,
    ]);
    if let Some(ctx) = &ctx {
        match pane {
            Pane::Thread(parent) => panel = panel.child(thread_pane(ctx, parent)),
            Pane::Members => panel = panel.child(members_pane(ctx)),
            Pane::None => {}
        }
    }
    Frame {
        state,
        actor,
        workspace: &workspace_name,
        query: "",
        now,
        open: id,
        tab: if is_dm { Tab::Dms } else { Tab::Home },
    }
    .render(title, panel)
}

/// Which rail tab is lit.
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Home,
    Dms,
    Activity,
}
/// What every page of the workspace shares: the top bar with its search box, the rail
/// and the sidebar, around whatever fills the panel.
struct Frame<'a> {
    state: &'a SlackState,
    actor: &'a str,
    workspace: &'a str,
    /// What the search box holds, on the page a search led to.
    query: &'a str,
    now: u64,
    open: Option<&'a str>,
    tab: Tab,
}
impl Frame<'_> {
    /// `panel` is the whole panel (sidebar included) for the conversation view, which
    /// hangs a pane off it; [`Frame::main`] builds the plain one.
    fn render(&self, title: String, panel: Html) -> SimResult<HttpResponse> {
        let mut doc = Document::new(title).lang("en").stylesheet(CSS).body_class("slack");
        if let Some(style) = root_style(self.state) {
            doc = doc.root_style(&style);
        }
        let doc = doc.body([
            topbar(self.state, self.actor, self.workspace, self.query),
            div("app").id("app").children([
                rail(
                    self.state,
                    self.actor,
                    self.workspace,
                    self.tab == Tab::Home,
                    self.tab == Tab::Dms,
                    self.tab == Tab::Activity,
                ),
                panel,
            ]),
        ]);
        html::page(&doc)
    }
    /// The sidebar and one main column.
    fn main(&self, main: Html) -> Html {
        div("panel").id("shell").children([
            sidebar(self.state, self.actor, self.open, self.now, self.workspace),
            main,
        ])
    }
}
/// The header of a page that is not a conversation: a heading and a line under it.
fn plain_header(id: &str, title: &str, sub_id: &str, sub: String) -> Html {
    el("header").id(id).class("channel-header").child(
        div("channel-head").children([
            el("h1").id(format!("{id}-title")).class("channel-title").text(title),
            span("channel-topic").id(sub_id).text(sub),
        ]),
    )
}
/// One hit of a search or one unread mention: who said it, where and when, linking to
/// the conversation it is in.
fn hit(state: &SlackState, actor: &str, id: &str, at: &str, m: &Message, now: u64) -> Html {
    let where_ = match state.channels.contains_key(at) {
        true => format!("#{at}"),
        false => partners(state, at, actor),
    };
    el("a")
        .id(id)
        .class("hit")
        .attr("href", format!("/channels/{at}"))
        .children([
            div("hit-head").children([
                span("hit-where").id(format!("{id}-where")).text(where_),
                span("hit-author")
                    .id(format!("{id}-author"))
                    .text(state.display(&m.author)),
                span("hit-time")
                    .id(format!("{id}-time"))
                    .text(time::ago(m.time, now)),
            ]),
            div("hit-text").id(format!("{id}-text")).text(emojify(&m.text)),
        ])
}
/// What the top bar's search box finds: every message of a conversation the actor is
/// in whose text holds the query, newest first.
pub fn search(
    state: &SlackState,
    actor: &str,
    query: &str,
    view: &View,
) -> SimResult<HttpResponse> {
    let now = now(state, actor, view.tick);
    let workspace = match state.workspace.is_empty() {
        true => BRAND.to_owned(),
        false => state.workspace.clone(),
    };
    let needle = query.trim().to_lowercase();
    let mut hits: Vec<(&str, &Message)> = Vec::new();
    if !needle.is_empty() {
        for (at, conversation) in state.channels.iter().chain(&state.dms) {
            if !conversation.members.contains(actor) {
                continue;
            }
            for m in &conversation.messages {
                if m.text.to_lowercase().contains(&needle) {
                    hits.push((at.as_str(), m));
                }
            }
        }
        hits.sort_by(|a, b| b.1.time.cmp(&a.1.time).then(a.1.id.cmp(&b.1.id)));
        hits.truncate(50);
    }
    let summary = match (needle.is_empty(), hits.len()) {
        (true, _) => "Type in the box above to search this workspace".to_owned(),
        (false, n) => format!("{} for “{}”", plural(n, "result", "results"), query.trim()),
    };
    let mut list = div("scroller").id("search-results");
    for (n, (at, m)) in hits.iter().enumerate() {
        list = list.child(hit(state, actor, &format!("hit-{n}"), at, m, now));
    }
    if hits.is_empty() && !needle.is_empty() {
        list = list.child(
            el("p")
                .id("search-empty")
                .class("empty")
                .text(format!("No message here says “{}”.", query.trim())),
        );
    }
    let main = el("main")
        .id("main")
        .class("main")
        .child(plain_header("search-header", "Search", "search-summary", summary))
        .child(list);
    let frame = Frame {
        state,
        actor,
        workspace: &workspace,
        query,
        now,
        open: None,
        tab: Tab::Home,
    };
    let panel = frame.main(main);
    frame.render(format!("Search - {workspace} - {BRAND}"), panel)
}
/// The Activity tab: every @-mention of the actor they have not read yet, newest
/// first, each one a link into its conversation.
pub fn activity(state: &SlackState, actor: &str, view: &View) -> SimResult<HttpResponse> {
    let now = now(state, actor, view.tick);
    let workspace = match state.workspace.is_empty() {
        true => BRAND.to_owned(),
        false => state.workspace.clone(),
    };
    let mut mentions = state.mentions(actor);
    mentions.sort_by(|a, b| b.1.time.cmp(&a.1.time).then(a.1.id.cmp(&b.1.id)));
    let summary = match mentions.len() {
        0 => "Nothing new".to_owned(),
        n => format!("{} waiting for you", plural(n, "mention", "mentions")),
    };
    let mut list = div("scroller").id("activity-list");
    for (n, (at, m)) in mentions.iter().enumerate() {
        list = list.child(hit(state, actor, &format!("mention-{n}"), at, m, now));
    }
    if mentions.is_empty() {
        list = list.child(
            el("p")
                .id("activity-empty")
                .class("empty")
                .text("Nobody has mentioned you since you last read your channels."),
        );
    }
    let main = el("main")
        .id("main")
        .class("main")
        .child(plain_header("activity-header", "Activity", "activity-summary", summary))
        .child(list);
    let frame = Frame {
        state,
        actor,
        workspace: &workspace,
        query: "",
        now,
        open: None,
        tab: Tab::Activity,
    };
    let panel = frame.main(main);
    frame.render(format!("Activity - {workspace} - {BRAND}"), panel)
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
        let mut piece = 0;
        let line = inline_line(
            "m-text",
            "@bob see http://github.com/x/y/pull/1, then run `cargo test` :eyes:",
            &["bob"],
            &mut piece,
        );
        assert_eq!(piece, 3);
        assert_eq!(
            line.render(),
            "<div class=\"line\"><span class=\"mention\" id=\"m-text-p0\">@bob</span> see \
             <a id=\"m-text-p1\" href=\"http://github.com/x/y/pull/1\">github.com/x/y/pull/1</a>, then run \
             <code id=\"m-text-p2\">cargo test</code> 👀</div>"
        );
    }
}
