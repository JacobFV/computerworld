//! The browser's view of Messages, laid out the way Messages for Mac is: the list of
//! conversations in a grey sidebar, the open conversation beside it with blue iMessage
//! bubbles, green SMS bubbles and grey incoming ones, tapbacks on the bubble's corner,
//! the delivery or read status under the last thing the actor sent, and the composer
//! pinned under the transcript. The stylesheet is `messages.css`.
use crate::{Conversation, Message, MessagesState, TAPBACKS};
use cw_protocol::{HttpResponse, Result as SimResult};
use cw_service_common as web;
use web::html::{self, button, div, el, form, span, text_input, Document, Html};

const CSS: &str = include_str!("messages.css");

/// The glyph a tapback shows as on the bubble and in the picker.
pub fn tapback_glyph(name: &str) -> &'static str {
    match name {
        "loved" => "♥",
        "liked" => "👍",
        "disliked" => "👎",
        "laughed" => "HA",
        "emphasized" => "!!",
        "questioned" => "?",
        _ => "·",
    }
}
fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    if letters.is_empty() {
        "?".into()
    } else {
        letters
    }
}
fn avatar(id: &str, name: &str, class: &str) -> Html {
    span("avatar").class(class).id(id).text(initials(name))
}

/// The sidebar every page carries: the title, the new-conversation form and the list.
/// `open` is the conversation shown beside it, if any.
fn sidebar(state: &MessagesState, actor: &str, me: &str, open: Option<&str>) -> Html {
    let inbox = state.inbox(actor);
    // The inbox page's bar is the sidebar's head; a thread's bar is over the transcript.
    let (title_id, me_id) = if open.is_some() {
        ("side-title", "side-me")
    } else {
        ("bar-title", "bar-me")
    };
    let head = div("side-head")
        .when(open.is_none(), |n| n.id("bar"))
        .child(el("h1").id(title_id).text("Messages"))
        .child(span("me").id(me_id).text(if me.is_empty() {
            String::new()
        } else {
            format!("{} · {me}", state.display(me))
        }));
    // A new conversation: to a number, an address or a contact; a comma makes a group.
    let new = (!me.is_empty()).then(|| {
        form("new", "/conversations", "post")
            .class("new")
            .child(
                text_input("new-to", "to", "")
                    .attr("aria-label", "To (number, address or name; comma for a group)")
                    .attr("placeholder", "To: name, number or address"),
            )
            .child(
                text_input("new-name", "name", "")
                    .attr("aria-label", "Group name")
                    .attr("placeholder", "Group name (optional)"),
            )
            .child(button("new-submit", "✎").attr("aria-label", "New message").attr("title", "New message"))
    });
    let mut list = el("nav").class("list").attr("aria-label", "Conversations");
    if inbox.is_empty() {
        list = list.child(el("p").id("empty").class("empty").text(if me.is_empty() {
            "This device has no number or address."
        } else {
            "No messages yet."
        }));
    }
    for (id, c) in inbox {
        let title = state.title(c, me);
        let last = c.messages.last();
        let unread = MessagesState::unread(c, me);
        let preview = last
            .map(|m| {
                if c.participants.len() > 2 && m.from != me {
                    format!("{}: {}", state.display(&m.from), m.text)
                } else {
                    m.text.clone()
                }
            })
            .unwrap_or_default();
        let row = el("a")
            .id(format!("row-{id}"))
            .class("row")
            .when(open == Some(id), |n| n.class("current"))
            .when(unread > 0, |n| n.class("unread"))
            .attr("href", format!("/conversations/{id}"))
            .child(if unread > 0 {
                span("dot").id(format!("row-{id}-unread")).attr("title", format!("{unread} unread")).text("●")
            } else {
                span("dot")
            })
            .child(avatar(&format!("row-{id}-avatar"), &title, if c.participants.len() > 2 { "group" } else { "" }))
            .child(
                span("row-text")
                    .id(format!("row-{id}-text"))
                    .child(
                        span("row-head")
                            .id(format!("row-{id}-head"))
                            .child(span("row-title").id(format!("row-{id}-title")).text(title.as_str()))
                            .child(
                                span("row-time")
                                    .id(format!("row-{id}-time"))
                                    .text(last.map(|m| format!("tick {}", m.time)).unwrap_or_default()),
                            ),
                    )
                    .child(span("row-preview").id(format!("row-{id}-preview")).text(preview)),
            );
        list = list.child(row);
    }
    el("aside").id("sidebar").class("sidebar").child(head).maybe(new).child(list)
}

fn document(title: &str, thread: bool, children: Vec<Html>) -> SimResult<HttpResponse> {
    let doc = Document::new(title)
        .lang("en")
        .stylesheet(CSS)
        .body_class(if thread { "app on-thread" } else { "app on-inbox" })
        .body([div("phone").id("phone").children(children)]);
    html::page(&doc)
}

/// The conversation list, with nothing open beside it.
pub fn inbox(state: &MessagesState, actor: &str) -> SimResult<HttpResponse> {
    let me = state.handle_of(actor).unwrap_or_default().to_owned();
    let blank = el("main").class("pane blank").child(
        el("p")
            .id("no-conversation")
            .class("blank-text")
            .text("No Conversation Selected"),
    );
    document("Messages", false, vec![sidebar(state, actor, &me, None), blank])
}

/// The tapbacks a bubble carries, and the picker that gives or takes one back.
fn tapbacks(id: &str, conversation: &str, m: &Message, state: &MessagesState, me: &str) -> (Html, Html) {
    let given = div("tapbacks").each(&m.tapbacks, |(name, who)| {
        let names: Vec<String> = who.iter().map(|h| state.display(h)).collect();
        let label = format!("{name} · {}", names.join(", "));
        span("tapback")
            .id(format!("{id}-has-{name}"))
            .class(name)
            .when(who.contains(me), |n| n.class("mine"))
            .attr("title", label.as_str())
            .attr("aria-label", label.as_str())
            .text(tapback_glyph(name))
            .when(who.len() > 1, |n| n.child(span("n").text(who.len().to_string())))
    });
    let picker = form(
        &format!("{id}-tapbacks"),
        format!("/conversations/{conversation}/messages/{}/tapbacks", m.id),
        "post",
    )
    .class("picker")
    .each(TAPBACKS, |name| {
        button(&format!("{id}-tapback-{name}"), tapback_glyph(name))
            .class(name)
            .attr("name", "tapback")
            .attr("value", *name)
            .attr("aria-label", *name)
            .attr("title", *name)
    });
    (given, picker)
}
fn bubble(
    conversation: &str,
    state: &MessagesState,
    m: &Message,
    me: &str,
    group: bool,
    last_outgoing: bool,
    tail: bool,
) -> Html {
    let id = m.id.as_str();
    let mine = m.from == me;
    let tone = match (mine, m.service.as_str()) {
        (true, "sms") => "sms",
        (true, _) => "imessage",
        (false, _) => "incoming",
    };
    let (given, picker) = tapbacks(id, conversation, m, state, me);
    let mut stack = div("stack").id(format!("{id}-stack"));
    if !mine && group {
        stack = stack.child(span("from").id(format!("{id}-from")).text(state.display(&m.from)));
    }
    stack = stack.child(
        div("bubble-line").child(
            div("bubble")
                .id(format!("{id}-bubble"))
                .class(tone)
                .when(tail, |n| n.class("tail"))
                .when(!m.tapbacks.is_empty(), |n| n.class("tapped"))
                .child(span("text").id(format!("{id}-text")).text(m.text.as_str()))
                .child(given),
        ).child(picker),
    );
    if mine && last_outgoing {
        let status = if m.service == "sms" {
            "Sent as Text Message".to_owned()
        } else if let Some((who, at)) = m.read.iter().max_by_key(|(_, at)| **at) {
            if group {
                format!("Read by {} at tick {at}", state.display(who))
            } else {
                format!("Read tick {at}")
            }
        } else if m.delivered.is_empty() {
            "Sending…".to_owned()
        } else {
            "Delivered".to_owned()
        };
        stack = stack.child(span("status").id(format!("{id}-status")).text(status));
    }
    let mut row = div("msg")
        .id(format!("{id}-row"))
        .class(if mine { "out" } else { "in" })
        .when(!tail, |n| n.class("run"));
    if !mine && group {
        row = row.child(if tail {
            avatar(&format!("{id}-avatar"), &state.display(&m.from), "small")
        } else {
            span("avatar small ghost")
        });
    }
    row.child(stack)
}

/// One conversation: the sidebar, the transcript as bubbles, then the composer.
pub fn thread(state: &MessagesState, actor: &str, id: &str) -> SimResult<HttpResponse> {
    let (me, c): (String, &Conversation) = match state.conversation(actor, id) {
        Ok(found) => found,
        Err(e) => return web::error(403, e),
    };
    let title = state.title(c, &me);
    let group = c.participants.len() > 2;
    let service = state.service_for(&c.participants, &me);
    let imessage = service == "imessage";
    let mut transcript = div("transcript").id("transcript");
    if group {
        let names: Vec<String> = c
            .participants
            .iter()
            .map(|h| format!("{} ({h})", state.display(h)))
            .collect();
        transcript = transcript.child(el("p").id("members").class("note").text(names.join(" · ")));
    }
    transcript = transcript.child(
        el("p")
            .id("service")
            .class("note")
            .text(if imessage { "iMessage" } else { "Text Message · SMS" }),
    );
    let last_outgoing = c.messages.iter().rev().find(|m| m.from == me).map(|m| m.id.as_str());
    for (n, m) in c.messages.iter().enumerate() {
        // The last bubble of a run by one sender carries the tail.
        let tail = c.messages.get(n + 1).is_none_or(|next| next.from != m.from);
        transcript = transcript.child(bubble(id, state, m, &me, group, last_outgoing == Some(m.id.as_str()), tail));
    }
    if c.messages.is_empty() {
        transcript = transcript.child(el("p").id("empty").class("note").text("Say something."));
    }
    let placeholder = if imessage { "iMessage" } else { "Text Message" };
    let composer = form("send", format!("/conversations/{id}/messages"), "post")
        .class("composer")
        .class(service)
        .child(span("plus").attr("title", "Apps").text("+"))
        .child(
            div("field")
                .child(
                    text_input("send-text", "text", "")
                        .attr("aria-label", placeholder)
                        .attr("placeholder", placeholder)
                        .attr("autocomplete", "off"),
                )
                .child(button("send-submit", "↑").attr("aria-label", "Send").attr("title", "Send")),
        );
    // Reading is a real route: the receipt the other side sees comes from here.
    let unread = MessagesState::unread(c, &me);
    let read = form("read", format!("/conversations/{id}/read"), "post")
        .class("read")
        .child(button("read-submit", "Mark as read").when(unread > 0, |n| n.class("pending")));
    let bar = el("header")
        .id("bar")
        .class("bar")
        .child(
            el("a")
                .id("back")
                .class("back")
                .attr("href", "/")
                .child(span("chev").text("‹"))
                .child(span("back-text").id("back-text").text("Messages")),
        )
        .child(
            div("who")
                .id("bar-title-stack")
                .child(avatar("bar-avatar", &title, if group { "group" } else { "" }))
                .child(span("name").id("bar-title").text(title.as_str())),
        )
        .child(
            div("bar-end")
                .child(span("service").class(service).id("bar-service").text(if imessage { "iMessage" } else { "SMS" }))
                .child(read),
        );
    let pane = el("main").class("pane").child(bar).child(transcript).child(composer);
    document(
        &format!("{title} · Messages"),
        true,
        vec![sidebar(state, actor, &me, Some(id)), pane],
    )
}
