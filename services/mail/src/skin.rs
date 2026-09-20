//! Gmail, Outlook and mail.com layouts over the one mailbox, served as HTML. `plain` never
//! reaches this module, so the original page bytes cannot move; everything here is
//! presentation plus real routes. The markup is shared and the skin is a class on `<body>`
//! with its own stylesheet (`gmail.css`, `outlook.css`, `mailcom.css`) next to this file.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `search` (the
//! form, with `search-q` and `search-submit`), `search-clear`, `compose`, `folder-<key>`,
//! `row-<id>` (the whole row is one link, with `-sender`, `-subject`, `-snippet`, `-time`,
//! `-star`, `-count` and `-avatar` inside), `new` (the compose form: `new-to`, `new-cc`,
//! `new-subject`, `new-body`, `new-submit`), `read-<id>` with `-star`, `-read`, `-archive`
//! (three buttons of one form), `-permalink`, `-link-<n>`, the `read-<id>-label` form, and
//! `reply` (`reply-to`, `reply-subject`, `reply-body`, `reply-submit`).
use crate::{stamp, MailState, Message, Nav};
use cw_protocol::{HttpResponse, Result};
use cw_service_common::html::{
    button, div, el, empty, form, fragment, hidden, href, label, link, span, text_input, Document, Html,
};

/// One product's surface. Nothing here reaches a record: names, colours and the sheet.
struct Look {
    skin: &'static str,
    brand: &'static str,
    title: &'static str,
    css: &'static str,
    /// accent, background, surface, ink, muted: what a seed's `theme` may override.
    palette: [&'static str; 5],
    compose: &'static str,
    new_message: &'static str,
    send: &'static str,
    folders: &'static [(&'static str, &'static str)],
}
const GMAIL: Look = Look {
    skin: "gmail",
    brand: "Gmail",
    title: "Gmail",
    css: include_str!("gmail.css"),
    palette: ["#0b57d0", "#f6f8fc", "#ffffff", "#202124", "#5f6368"],
    compose: "Compose",
    new_message: "New Message",
    send: "Send",
    folders: &[("inbox", "Inbox"), ("starred", "Starred"), ("sent", "Sent"), ("archive", "All Mail")],
};
const OUTLOOK: Look = Look {
    skin: "outlook",
    brand: "Outlook",
    title: "Mail - Outlook",
    css: include_str!("outlook.css"),
    palette: ["#0f6cbd", "#f5f5f5", "#ffffff", "#242424", "#616161"],
    compose: "New mail",
    new_message: "New message",
    send: "Send",
    folders: &[("inbox", "Inbox"), ("starred", "Favourites"), ("sent", "Sent Items"), ("archive", "Archive")],
};
const MAILCOM: Look = Look {
    skin: "mailcom",
    brand: "mail.com",
    title: "mail.com - Inbox",
    css: include_str!("mailcom.css"),
    palette: ["#0a4ea3", "#eef1f5", "#ffffff", "#1c2a3a", "#64748b"],
    compose: "Compose E-mail",
    new_message: "Compose E-mail",
    send: "Send",
    folders: &[("inbox", "Inbox"), ("starred", "Favorites"), ("sent", "Sent"), ("archive", "Archive")],
};

/// Six avatar fills (`.av0`..`.av5` in the sheets), picked by name so one person keeps one
/// colour across every render.
fn avatar(id: &str, name: &str) -> Html {
    let tint = name.bytes().map(usize::from).sum::<usize>() % 6;
    let initials: String = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    span(&format!("avatar av{tint}")).id(id).attr("aria-hidden", "true").text(initials)
}
fn at(folder: &str, thread: Option<&str>, compose: bool) -> String {
    let mut params = vec![("folder", folder)];
    if let Some(thread) = thread {
        params.push(("thread", thread));
    }
    if compose {
        params.push(("compose", "1"));
    }
    href("/", &params)
}
fn snippet(body: &str, width: usize) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(width) {
        Some((cut, _)) => format!("{}…", &flat[..cut]),
        None => flat,
    }
}
/// A message body as paragraphs. Addresses written in the prose become real links in place
/// (this is how sites connect), numbered by their word position as `<prefix>-link-<n>`.
fn prose(prefix: &str, body: &str) -> Html {
    let mut out = fragment([]);
    let mut word = 0usize;
    for line in body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut p = el("p");
        for (i, w) in line.split_whitespace().enumerate() {
            if i > 0 {
                p = p.text(" ");
            }
            let url = w.trim_end_matches(['.', ',', ';', ')', ']']);
            let is_link = url.starts_with("http://") || url.starts_with("https://");
            if is_link {
                p = p.child(link(&format!("{prefix}-link-{word}"), url, url)).text(&w[url.len()..]);
            } else {
                p = p.text(w);
            }
            word += 1;
        }
        out = out.child(p);
    }
    out
}
/// A labelled one-line field of a form: `<div class=field><label for>..</label><input></div>`.
fn field(form_id: &str, name: &str, text: &str, value: &str) -> Html {
    let id = format!("{form_id}-{name}");
    div("field")
        .child(label(&id, text))
        .child(text_input(&id, name, value).attr("autocomplete", "off"))
}
fn area(form_id: &str, name: &str, text: &str) -> Html {
    let id = format!("{form_id}-{name}");
    div("field body")
        .child(label(&id, text))
        .child(el("textarea").id(id.as_str()).attr("name", name).attr("rows", "8"))
}

struct View<'a> {
    s: &'a MailState,
    actor: &'a str,
    nav: &'a Nav,
    look: &'a Look,
    folder: String,
}
impl View<'_> {
    fn brand(&self) -> &str {
        if self.s.brand.is_empty() {
            self.look.brand
        } else {
            &self.s.brand
        }
    }
    fn header(&self) -> Html {
        let query = self.s.queries.get(self.actor).cloned().unwrap_or_default();
        let search = form("search", "/search", "post")
            .attr("role", "search")
            .child(span("mag").attr("aria-hidden", "true"))
            .child(
                text_input("search-q", "q", &query)
                    .attr("aria-label", "Search mail")
                    .attr("placeholder", if self.look.skin == "outlook" { "Search" } else { "Search mail" })
                    .attr("autocomplete", "off"),
            )
            .child(button("search-submit", "Search"));
        let clear = if query.is_empty() {
            empty()
        } else {
            form("search-clear-form", "/search", "post")
                .child(hidden("q", ""))
                .child(button("search-clear", "Clear search"))
        };
        let name = self.s.display(self.actor);
        el("header")
            .id("bar")
            .class("bar")
            .child(span("menu").attr("aria-hidden", "true").each(0..9, |_| el("i")))
            .child(
                el("a")
                    .id("wordmark")
                    .class("brand")
                    .attr("href", "/")
                    .attr("aria-label", self.brand())
                    .child(span("logo").attr("aria-hidden", "true").child(el("i")))
                    .child(span("name").text(self.brand())),
            )
            .child(div("search-box").id("search-box").child(search).child(clear))
            .child(
                div("account")
                    .id("account")
                    .child(
                        span("unread")
                            .id("account-folder")
                            .text(format!("{} unread", self.s.unread(self.actor, &self.folder))),
                    )
                    .child(span("address").id("account-address").text(self.s.address(self.actor)))
                    .child(avatar("account-avatar", &name)),
            )
    }
    fn sidebar(&self) -> Html {
        let mut labels: Vec<&str> = self
            .s
            .messages
            .values()
            .filter_map(|m| m.mailboxes.get(self.actor))
            .flat_map(|b| b.labels.iter().map(String::as_str))
            .collect();
        labels.sort_unstable();
        labels.dedup();
        el("nav")
            .id("sidebar")
            .class("rail")
            .attr("aria-label", "Folders")
            .child(
                el("a")
                    .id("compose")
                    .class("compose")
                    .attr("href", at(&self.folder, None, true))
                    .child(span("pen").attr("aria-hidden", "true"))
                    .child(span("").id("compose-label").text(self.look.compose)),
            )
            .child(div("folders").id("sidebar-items").each(self.look.folders, |(key, text)| {
                let count = self.s.unread(self.actor, key);
                let on = *key == self.folder;
                el("a")
                    .id(format!("folder-{key}"))
                    .class(if on { "folder on" } else { "folder" })
                    .when(count > 0, |a| a.class("fresh"))
                    .attr("href", at(key, None, false))
                    .child(span(&format!("ico ico-{key}")).attr("aria-hidden", "true"))
                    .child(span("label").id(format!("folder-{key}-label")).text(*text))
                    .child(
                        span("count")
                            .id(format!("folder-{key}-count"))
                            .text(if count > 0 { count.to_string() } else { String::new() }),
                    )
            }))
            .when(!labels.is_empty(), |nav| {
                nav.child(
                    div("tags")
                        .id("sidebar-labels")
                        .child(div("tags-title").text(if self.look.skin == "outlook" { "Categories" } else { "Labels" }))
                        .each(labels.iter().enumerate(), |(i, name)| {
                            div("tag")
                                .child(span(&format!("dot av{}", i % 6)).attr("aria-hidden", "true"))
                                .child(span("label").text(*name))
                        }),
                )
            })
    }
    fn list(&self) -> Html {
        let title = self
            .look
            .folders
            .iter()
            .find(|(key, _)| *key == self.folder)
            .map_or("Mail", |(_, text)| *text);
        let conversations = self.s.conversations(self.actor, &self.folder);
        let n = conversations.len();
        let count = match (self.look.skin, n) {
            ("gmail", 0) => "0 of 0".to_owned(),
            ("gmail", n) => format!("1–{n} of {n}"),
            (_, 1) => "1 conversation".to_owned(),
            (_, n) => format!("{n} conversations"),
        };
        let head = div("list-head")
            .id("list-head")
            .child(span("tools").attr("aria-hidden", "true").child(span("box")).child(span("reload")).child(span("more")))
            .child(el("h1").id("list-title").text(title))
            .child(span("count").id("list-count").text(count));
        let tabs = if self.look.skin == "mailcom" {
            div("cols")
                .attr("aria-hidden", "true")
                .child(span("c-from").text("From"))
                .child(span("c-subject").text("Subject"))
                .child(span("c-date").text("Date"))
        } else {
            let names: &[&str] = if self.look.skin == "gmail" { &["Primary", "Promotions", "Social"] } else { &["Focused", "Other"] };
            div("tabs").attr("aria-hidden", "true").each(names.iter().enumerate(), |(i, name)| {
                span(if i == 0 { "tab on" } else { "tab" }).child(span(&format!("tab-ico t{i}"))).text(*name)
            })
        };
        let rows = div("rows").id("list-rows").each(conversations, |(m, count)| {
            let box_ = &m.mailboxes[self.actor];
            let open = self.nav.thread.as_deref() == Some(m.thread());
            let id = &m.id;
            let mut class = String::from("row");
            class.push_str(if box_.read { " read" } else { " unread" });
            if open {
                class.push_str(" open");
            }
            el("a")
                .id(format!("row-{id}"))
                .class(&class)
                .attr("href", at(&self.folder, Some(m.thread()), false))
                .child(span("check").attr("aria-hidden", "true"))
                .child(
                    span(if box_.starred { "star on" } else { "star" })
                        .id(format!("row-{id}-star"))
                        .attr("aria-label", if box_.starred { "Starred" } else { "Not starred" })
                        .text(if box_.starred { "★" } else { "☆" }),
                )
                .child(avatar(&format!("row-{id}-avatar"), &self.s.display(&m.sender)))
                .child(
                    span("who")
                        .child(span("sender").id(format!("row-{id}-sender")).text(self.s.display(&m.sender)))
                        .child(
                            span("n")
                                .id(format!("row-{id}-count"))
                                .text(if count > 1 { count.to_string() } else { String::new() }),
                        ),
                )
                .child(
                    span("line")
                        .each(&box_.labels, |name| span("chip").text(name.as_str()))
                        .child(span("subject").id(format!("row-{id}-subject")).text(m.subject.as_str()))
                        .child(span("dash").attr("aria-hidden", "true").text(" - "))
                        .child(span("snippet").id(format!("row-{id}-snippet")).text(snippet(&m.body, 110))),
                )
                .child(span("time").id(format!("row-{id}-time")).text(stamp(m.time)))
        });
        el("section")
            .id("list")
            .class("list")
            .attr("aria-label", title)
            .child(head)
            .when(self.folder == "inbox" || self.look.skin == "mailcom", |l| l.child(tabs))
            .child(rows)
            .when(n == 0, |l| l.child(el("p").id("list-empty").class("none").text("Nothing here.")))
    }
    fn reading(&self) -> Html {
        let pane = el("section").id("reading").class("reading");
        if self.nav.compose {
            return pane.class("composing").child(self.compose());
        }
        match self.nav.thread.as_deref().map(|t| (t, self.s.thread(self.actor, t))) {
            Some((thread, messages)) if !messages.is_empty() => pane.child(self.conversation(thread, &messages)),
            _ => pane.class("vacant").child(
                div("vacant-note")
                    .child(span("envelope").attr("aria-hidden", "true").child(el("i")))
                    .child(el("p").id("reading-empty").text("Select a conversation"))
                    .child(
                        el("p")
                            .id("reading-hint")
                            .text(format!("Pick a message on the left, or start a new one with {}.", self.look.compose)),
                    ),
            ),
        }
    }
    fn compose(&self) -> Html {
        div("window")
            .id("reading-body")
            .child(
                div("window-head")
                    .child(el("h2").id("compose-title").text(self.look.new_message))
                    .child(link("compose-close", at(&self.folder, None, false), "×").attr("aria-label", "Discard and close")),
            )
            .child(el("p").id("compose-from").class("from").text(format!("From {}", self.s.address(self.actor))))
            .child(
                form("new", "/send", "post")
                    .child(field("new", "to", "To", ""))
                    .child(field("new", "cc", "Cc", ""))
                    .child(field("new", "subject", "Subject", ""))
                    .child(area("new", "body", "Message"))
                    .child(div("send-row").child(button("new-submit", self.look.send).class("primary"))),
            )
    }
    fn conversation(&self, thread: &str, messages: &[&Message]) -> Html {
        let s = self.s;
        let last = messages[messages.len() - 1];
        let mut labels: Vec<&str> = messages
            .iter()
            .flat_map(|m| m.mailboxes[self.actor].labels.iter().map(String::as_str))
            .collect();
        labels.sort_unstable();
        labels.dedup();
        let head = div("thread-head")
            .id("thread-head")
            .child(link("thread-back", at(&self.folder, None, false), "←").class("back").attr("aria-label", "Back to the list"))
            .child(
                el("h2")
                    .id("thread-subject")
                    .text(last.subject.as_str())
                    .each(labels, |name| span("chip").text(name)),
            )
            .child(span("size").id("thread-size").text(format!("{} in thread", messages.len())));
        let cards = fragment([]).each(messages, |m| {
            let box_ = &m.mailboxes[self.actor];
            let id = &m.id;
            let name = s.display(&m.sender);
            let route = format!("/messages/{id}");
            let to = m.to.iter().chain(&m.cc).map(|r| s.address(r)).collect::<Vec<_>>().join(", ");
            el("article")
                .id(format!("read-{id}"))
                .class("msg")
                .child(
                    div("msg-head")
                        .id(format!("read-{id}-head"))
                        .child(avatar(&format!("read-{id}-avatar"), &name))
                        .child(
                            div("msg-who")
                                .child(span("name").id(format!("read-{id}-name")).text(name.as_str()))
                                .child(span("route").id(format!("read-{id}-line")).text(format!("{} → {}", s.address(&m.sender), to))),
                        )
                        .child(span("time").id(format!("read-{id}-time")).text(stamp(m.time))),
                )
                .when(!box_.labels.is_empty(), |a| {
                    a.child(div("msg-labels").id(format!("read-{id}-labels")).each(box_.labels.iter().enumerate(), |(i, name)| {
                        span("chip").id(format!("read-{id}-label-{i}")).text(name.as_str())
                    }))
                })
                .child(div("msg-body").id(format!("read-{id}-body")).child(prose(&format!("read-{id}"), &m.body)))
                .child(
                    form(&format!("read-{id}-actions"), route.as_str(), "post")
                        .class("msg-actions")
                        .child(hidden("folder", &self.folder))
                        .child(
                            button(&format!("read-{id}-star"), if box_.starred { "Unstar" } else { "Star" })
                                .attr("name", "star")
                                .attr("value", "toggle"),
                        )
                        .child(
                            button(&format!("read-{id}-read"), if box_.read { "Mark unread" } else { "Mark read" })
                                .attr("name", "read")
                                .attr("value", if box_.read { "false" } else { "true" }),
                        )
                        .child(button(&format!("read-{id}-archive"), "Archive").attr("name", "archive").attr("value", "true"))
                        .child(link(&format!("read-{id}-permalink"), format!("/threads/{thread}"), "Permalink")),
                )
                .child(
                    form(&format!("read-{id}-label"), route.as_str(), "post")
                        .class("msg-label")
                        .child(
                            text_input(&format!("read-{id}-label-label"), "label", "")
                                .attr("aria-label", "Add label")
                                .attr("placeholder", "Add label")
                                .attr("autocomplete", "off"),
                        )
                        .child(button(&format!("read-{id}-label-submit"), "Label")),
                )
        });
        let reply_to = if last.sender == self.actor {
            last.to.first().cloned().unwrap_or_else(|| self.actor.to_owned())
        } else {
            last.sender.clone()
        };
        let subject = if last.subject.to_lowercase().starts_with("re:") {
            last.subject.clone()
        } else {
            format!("Re: {}", last.subject)
        };
        let reply = form("reply", "/send", "post")
            .class("reply")
            .child(div("reply-title").child(span("arrow").attr("aria-hidden", "true").text("↩")).text("Reply"))
            .child(field("reply", "to", "To", &s.address(&reply_to)))
            .child(field("reply", "subject", "Subject", &subject))
            .child(area("reply", "body", "Message"))
            .child(div("send-row").child(button("reply-submit", self.look.send).class("primary")));
        div("thread").id("reading-body").child(head).child(cards).child(reply)
    }
}
pub(crate) fn mailbox(s: &MailState, actor: &str, nav: &Nav) -> Result<HttpResponse> {
    let look = match s.skin.as_str() {
        "outlook" => &OUTLOOK,
        "mailcom" => &MAILCOM,
        _ => &GMAIL,
    };
    let view = View { s, actor, nav, look, folder: nav.folder().to_owned() };
    let theme = s.theme.clone().unwrap_or_default();
    let [accent, paper, surface, ink, muted] = look.palette;
    let or = |value: &Option<String>, fallback: &str| value.clone().unwrap_or_else(|| fallback.to_owned());
    let mode = if nav.compose {
        "view-compose"
    } else if nav.thread.as_deref().is_some_and(|t| !s.thread(actor, t).is_empty()) {
        "view-thread"
    } else {
        "view-list"
    };
    let apps = if look.skin == "outlook" {
        div("apps").attr("aria-hidden", "true").each(["mail on", "cal", "people", "todo"], |k| span(&format!("app {k}")).child(el("i")))
    } else {
        empty()
    };
    let document = Document::new(look.title)
        .lang("en")
        .stylesheet(look.css)
        .root_style(&format!(
            "--accent: {}; --paper: {}; --surface: {}; --ink: {}; --muted: {}",
            or(&theme.accent, accent),
            or(&theme.background, paper),
            or(&theme.surface, surface),
            or(&theme.ink, ink),
            or(&theme.muted, muted),
        ))
        .body_class(&format!("skin-{} {mode}", look.skin))
        .body([
            view.header(),
            div("panes").id("panes").child(apps).child(view.sidebar()).child(view.list()).child(view.reading()),
        ]);
    cw_service_common::html::page(&document)
}
