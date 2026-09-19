//! The browser's view of Messages: a phone-shaped column, blue iMessage bubbles and
//! green SMS bubbles, the delivery or read status under the last thing the actor sent.
use crate::{Conversation, Message, MessagesState, TAPBACKS};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result as SimResult};
use cw_service_common as web;

pub const IMESSAGE: &str = "#0b84fe";
pub const SMS: &str = "#34c759";
pub const INCOMING: &str = "#e9e9eb";
pub const INK: &str = "#1d1d1f";
pub const MUTED: &str = "#8e8e93";
pub const SURFACE: &str = "#ffffff";
pub const SCREEN: &str = "#f2f2f7";
/// The width of a phone, which is what Messages is shaped like even on a desktop browser.
pub const PHONE: u32 = 390;

fn button(id: String, text: impl Into<String>, action: PageAction) -> PageElement {
    PageElement::Button {
        id,
        text: text.into(),
        action,
        style: None,
    }
}
fn theme() -> PageTheme {
    PageTheme {
        accent: Some(IMESSAGE.into()),
        background: Some(SCREEN.into()),
        surface: Some(SURFACE.into()),
        ink: Some(INK.into()),
        muted: Some(MUTED.into()),
        content_width: Some(PHONE + 32),
    }
}
fn avatar(id: &str, name: &str) -> PageElement {
    let initials: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    web::thumbnail(
        id,
        if initials.is_empty() { "?" } else { &initials },
        web::style()
            .width(40)
            .height(40)
            .radius(20)
            .background("#a2a2a8")
            .color("#ffffff")
            .size(15)
            .align("center"),
    )
}
/// A phone-shaped column with the navigation bar on top.
fn phone(title: &str, bar: Vec<PageElement>, body: Vec<PageElement>) -> SimResult<HttpResponse> {
    let mut items = vec![web::styled_row(
        "bar",
        8,
        "center",
        web::style().padding(10).background(SURFACE),
        bar,
    )];
    items.push(web::divider("bar-rule"));
    items.extend(body);
    web::themed_page(
        title,
        theme(),
        vec![web::card(
            "phone",
            web::style()
                .width(PHONE)
                .background(SURFACE)
                .radius(24)
                .border("#d1d1d6")
                .padding(0),
            items,
        )],
    )
}

/// The conversation list: who, the latest text, and a blue dot where something is unread.
pub fn inbox(state: &MessagesState, actor: &str) -> SimResult<HttpResponse> {
    let me = state.handle_of(actor).unwrap_or_default().to_owned();
    let mut body = vec![];
    let inbox = state.inbox(actor);
    if inbox.is_empty() {
        body.push(web::styled(
            "empty",
            if me.is_empty() {
                "This device has no number or address."
            } else {
                "No messages yet."
            },
            web::style()
                .size(15)
                .color(MUTED)
                .padding(16)
                .align("center"),
        ));
    }
    for (id, c) in inbox {
        let title = state.title(c, &me);
        let last = c.messages.last();
        let unread = MessagesState::unread(c, &me);
        let mut lead = vec![];
        if unread > 0 {
            lead.push(web::badge(
                &format!("row-{id}-unread"),
                "●",
                web::style().color(IMESSAGE).size(10),
            ));
        }
        lead.push(avatar(&format!("row-{id}-avatar"), &title));
        let mut lines = vec![web::styled_row(
            &format!("row-{id}-head"),
            8,
            "center",
            web::style(),
            vec![
                web::styled(
                    &format!("row-{id}-title"),
                    &title,
                    web::style().size(16).bold().color(INK).flex(1).one_line(),
                ),
                web::styled(
                    &format!("row-{id}-time"),
                    last.map(|m| format!("tick {}", m.time)).unwrap_or_default(),
                    web::style().size(12).color(MUTED),
                ),
            ],
        )];
        lines.push(web::styled(
            &format!("row-{id}-preview"),
            last.map(|m| {
                if c.participants.len() > 2 && m.from != me {
                    format!("{}: {}", state.display(&m.from), m.text)
                } else {
                    m.text.clone()
                }
            })
            .unwrap_or_default(),
            web::style().size(14).color(MUTED),
        ));
        lead.push(web::card(
            &format!("row-{id}-text"),
            web::style().flex(1),
            lines,
        ));
        body.push(web::card_action(
            &format!("row-{id}"),
            web::style().padding(12),
            web::visit(format!("/conversations/{id}")),
            vec![web::styled_row(
                &format!("row-{id}-line"),
                10,
                "center",
                web::style(),
                lead,
            )],
        ));
        body.push(web::divider(&format!("row-{id}-rule")));
    }
    // A new conversation: to a number, an address or a contact; a comma makes a group.
    if !me.is_empty() {
        body.push(web::form(
            "new",
            "/conversations",
            &[
                ("to", "To (number, address or name; comma for a group)", ""),
                ("name", "Group name", ""),
            ],
        ));
    }
    let bar = vec![
        web::styled(
            "bar-title",
            "Messages",
            web::style().size(17).bold().color(INK).flex(1),
        ),
        web::styled(
            "bar-me",
            if me.is_empty() {
                String::new()
            } else {
                format!("{} · {me}", state.display(&me))
            },
            web::style().size(12).color(MUTED),
        ),
    ];
    phone("Messages", bar, body)
}

fn tapback_chips(id: &str, conversation: &str, m: &Message, state: &MessagesState) -> PageElement {
    let mut chips: Vec<PageElement> = m
        .tapbacks
        .iter()
        .map(|(name, who)| {
            let names: Vec<String> = who.iter().map(|h| state.display(h)).collect();
            web::badge(
                &format!("{id}-has-{name}"),
                format!("{name} · {}", names.join(", ")),
                web::style()
                    .background(SURFACE)
                    .border(MUTED)
                    .color(INK)
                    .radius(10)
                    .padding(3)
                    .size(11),
            )
        })
        .collect();
    for name in TAPBACKS {
        chips.push(button(
            format!("{id}-tapback-{name}"),
            *name,
            PageAction {
                method: "POST".into(),
                url: format!("/conversations/{conversation}/messages/{}/tapbacks", m.id),
                fields: [("tapback".to_string(), (*name).to_string())]
                    .into_iter()
                    .collect(),
            },
        ));
    }
    web::styled_row(&format!("{id}-tapbacks"), 4, "center", web::style(), chips)
}
fn bubble(
    conversation: &str,
    state: &MessagesState,
    m: &Message,
    me: &str,
    group: bool,
    last_outgoing: bool,
) -> Vec<PageElement> {
    let id = m.id.clone();
    let mine = m.from == me;
    let (fill, ink) = match (mine, m.service.as_str()) {
        (true, "sms") => (SMS, "#ffffff"),
        (true, _) => (IMESSAGE, "#ffffff"),
        (false, _) => (INCOMING, INK),
    };
    let mut stack = vec![];
    if !mine && group {
        stack.push(web::styled(
            &format!("{id}-from"),
            state.display(&m.from),
            web::style().size(11).color(MUTED),
        ));
    }
    stack.push(web::card(
        &format!("{id}-bubble"),
        web::style()
            .background(fill)
            .radius(18)
            .padding(10)
            .width(260),
        vec![web::styled(
            &format!("{id}-text"),
            &m.text,
            web::style().size(15).color(ink),
        )],
    ));
    stack.push(tapback_chips(&id, conversation, m, state));
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
        stack.push(web::styled(
            &format!("{id}-status"),
            status,
            web::style().size(11).color(MUTED).align("right"),
        ));
    }
    let column = web::card(&format!("{id}-stack"), web::style().flex(0), stack);
    // An outgoing bubble sits on the right: an empty flex card takes the space before it.
    let row = if mine {
        vec![
            web::card(&format!("{id}-push"), web::style().flex(1), vec![]),
            column,
        ]
    } else {
        vec![column]
    };
    vec![web::styled_row(
        &format!("{id}-row"),
        6,
        if mine { "end" } else { "start" },
        web::style().padding(4),
        row,
    )]
}

/// One conversation: the transcript as bubbles, then the composer.
pub fn thread(state: &MessagesState, actor: &str, id: &str) -> SimResult<HttpResponse> {
    let (me, c): (String, &Conversation) = match state.conversation(actor, id) {
        Ok(found) => found,
        Err(e) => return web::error(403, e),
    };
    let title = state.title(c, &me);
    let group = c.participants.len() > 2;
    let service = state.service_for(&c.participants, &me);
    let mut body = vec![];
    if group {
        let names: Vec<String> = c
            .participants
            .iter()
            .map(|h| format!("{} ({h})", state.display(h)))
            .collect();
        body.push(web::styled(
            "members",
            names.join(" · "),
            web::style()
                .size(12)
                .color(MUTED)
                .padding(8)
                .align("center"),
        ));
    }
    body.push(web::styled(
        "service",
        if service == "imessage" {
            "iMessage"
        } else {
            "Text Message · SMS"
        },
        web::style().size(11).color(MUTED).align("center"),
    ));
    let last_outgoing = c
        .messages
        .iter()
        .rev()
        .find(|m| m.from == me)
        .map(|m| m.id.clone());
    for m in &c.messages {
        body.extend(bubble(
            id,
            state,
            m,
            &me,
            group,
            last_outgoing.as_deref() == Some(m.id.as_str()),
        ));
    }
    if c.messages.is_empty() {
        body.push(web::styled(
            "empty",
            "Say something.",
            web::style()
                .size(14)
                .color(MUTED)
                .align("center")
                .padding(16),
        ));
    }
    body.push(web::divider("composer-rule"));
    body.push(web::form(
        "send",
        &format!("/conversations/{id}/messages"),
        &[(
            "text",
            if service == "imessage" {
                "iMessage"
            } else {
                "Text Message"
            },
            "",
        )],
    ));
    // Reading is a real route: the receipt the other side sees comes from here.
    body.push(PageElement::Form {
        id: "read".into(),
        action: PageAction {
            method: "POST".into(),
            url: format!("/conversations/{id}/read"),
            fields: Default::default(),
        },
        children: vec![button(
            "read-submit".into(),
            "Mark as read",
            PageAction {
                method: "POST".into(),
                url: format!("/conversations/{id}/read"),
                fields: Default::default(),
            },
        )],
    });
    let bar = vec![
        web::card_action(
            "back",
            web::style().padding(4),
            web::visit("/"),
            vec![web::styled(
                "back-text",
                "‹ Messages",
                web::style().size(15).color(IMESSAGE),
            )],
        ),
        web::card(
            "bar-title-stack",
            web::style().flex(1),
            vec![
                avatar("bar-avatar", &title),
                web::styled(
                    "bar-title",
                    &title,
                    web::style().size(13).bold().color(INK).align("center"),
                ),
            ],
        ),
        web::styled(
            "bar-service",
            if service == "imessage" {
                "iMessage"
            } else {
                "SMS"
            },
            web::style()
                .size(11)
                .color(if service == "imessage" { IMESSAGE } else { SMS }),
        ),
    ];
    phone(&format!("{title} · Messages"), bar, body)
}
