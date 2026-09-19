//! Slack and Discord layouts for the `chat` kind. The plain skin never reaches this module,
//! so chat.internal keeps rendering exactly as it always has.
use crate::{Channel, ChatState};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result as SimResult};
use cw_service_common as web;

/// The two brandings differ only in palette and vocabulary; the layout is one workspace shell.
pub struct Look {
    pub rail: &'static str,
    pub sidebar: &'static str,
    pub sidebar_ink: &'static str,
    pub surface: &'static str,
    pub ink: &'static str,
    pub muted: &'static str,
    pub accent: &'static str,
    pub channels_label: &'static str,
    pub brand: &'static str,
}
pub const SLACK: Look = Look {
    rail: "#350d36",
    sidebar: "#3f0e40",
    sidebar_ink: "#d1c4d3",
    surface: "#ffffff",
    ink: "#1d1c1d",
    muted: "#616061",
    accent: "#1264a3",
    channels_label: "Channels",
    brand: "Slack",
};
pub const DISCORD: Look = Look {
    rail: "#1e1f22",
    sidebar: "#2b2d31",
    sidebar_ink: "#b5bac1",
    surface: "#313338",
    ink: "#f2f3f5",
    muted: "#949ba4",
    accent: "#5865f2",
    channels_label: "Text channels",
    brand: "Discord",
};
pub fn look(skin: &str) -> Look {
    if skin == "discord" {
        DISCORD
    } else {
        SLACK
    }
}
fn theme(state: &ChatState, look: &Look) -> PageTheme {
    state.theme.clone().unwrap_or(PageTheme {
        accent: Some(look.accent.into()),
        background: Some(look.surface.into()),
        // Cards and forms sit on the page, in the page's ink. The sidebar and the rail set
        // their own colour; naming it here put Slack's dark text on the sidebar's purple.
        surface: Some(look.surface.into()),
        ink: Some(look.ink.into()),
        muted: Some(look.muted.into()),
        content_width: Some(1100),
    })
}
fn avatar(id: &str, who: &str, look: &Look) -> PageElement {
    web::thumbnail(
        id,
        who,
        web::style()
            .width(36)
            .height(36)
            .radius(8)
            .background(look.accent)
            .color("#ffffff")
            .size(12)
            .align("center"),
    )
}
/// The other side of a DM key, from this actor's point of view.
pub fn partner(key: &str, actor: &str) -> String {
    key.split('|')
        .find(|who| *who != actor)
        .unwrap_or(actor)
        .to_owned()
}
fn sidebar_link(id: &str, text: String, url: String, look: &Look, current: bool) -> PageElement {
    web::card_action(
        id,
        web::style().padding(6).radius(4).background(if current {
            look.accent
        } else {
            look.sidebar
        }),
        web::visit(url),
        vec![web::styled(
            &format!("{id}-text"),
            text,
            web::style()
                .size(14)
                .color(if current { "#ffffff" } else { look.sidebar_ink }),
        )],
    )
}
fn sidebar(state: &ChatState, actor: &str, open: Option<&str>, look: &Look) -> PageElement {
    let mut items = vec![web::styled(
        "workspace",
        if state.workspace.is_empty() {
            look.brand.to_owned()
        } else {
            state.workspace.clone()
        },
        web::style().size(17).bold().color("#ffffff"),
    )];
    items.push(web::divider("sidebar-rule"));
    items.push(web::styled(
        "channels-label",
        look.channels_label,
        web::style().size(12).medium().color(look.sidebar_ink),
    ));
    for (id, channel) in &state.channels {
        if channel.members.contains(actor) {
            items.push(sidebar_link(
                &format!("nav-{id}"),
                format!("# {id}"),
                format!("/channels/{id}"),
                look,
                open == Some(id.as_str()),
            ));
        }
    }
    items.push(web::spacer("sidebar-gap", 8));
    items.push(web::styled(
        "dms-label",
        "Direct messages",
        web::style().size(12).medium().color(look.sidebar_ink),
    ));
    for (key, dm) in &state.dms {
        if dm.members.contains(actor) {
            items.push(sidebar_link(
                &format!("dm-{key}"),
                partner(key, actor),
                format!("/channels/{key}"),
                look,
                open == Some(key.as_str()),
            ));
        }
    }
    let others: Vec<String> = state
        .people()
        .filter(|who| *who != actor)
        .map(str::to_owned)
        .collect();
    for who in others {
        items.push(PageElement::Form {
            id: format!("start-{who}"),
            action: PageAction {
                method: "POST".into(),
                url: "/dms".into(),
                fields: [("to".to_string(), who.clone())].into_iter().collect(),
            },
            children: vec![PageElement::Button {
                id: format!("start-{who}-submit"),
                text: format!("Message {who}"),
                action: PageAction {
                    method: "POST".into(),
                    url: "/dms".into(),
                    fields: [("to".to_string(), who.clone())].into_iter().collect(),
                },
            }],
        });
    }
    web::card(
        "sidebar",
        web::style()
            .background(look.sidebar)
            .padding(12)
            .width(240)
            .flex(0),
        items,
    )
}
fn reactions(id: &str, channel_id: &str, message: &crate::Message, look: &Look) -> PageElement {
    let mut chips: Vec<PageElement> = message
        .reactions
        .iter()
        .map(|(name, who)| {
            web::badge(
                &format!("{id}-has-{name}"),
                format!(":{name}: {}", who.len()),
                web::style()
                    .background(look.surface)
                    .border(look.accent)
                    .color(look.accent)
                    .radius(10)
                    .padding(3)
                    .size(12),
            )
        })
        .collect();
    // Quick reactions are buttons with a real route, not decoration.
    for name in ["+1", "tada", "eyes"] {
        chips.push(PageElement::Button {
            id: format!("{id}-react-{name}"),
            text: format!(":{name}:"),
            action: PageAction {
                method: "POST".into(),
                url: format!("/channels/{channel_id}/messages/{}/reactions", message.id),
                fields: [("reaction".to_string(), name.to_string())]
                    .into_iter()
                    .collect(),
            },
        });
    }
    web::styled_row(&format!("{id}-reactions"), 6, "center", web::style(), chips)
}
fn message_block(
    channel_id: &str,
    message: &crate::Message,
    replies: Vec<&crate::Message>,
    look: &Look,
) -> PageElement {
    let id = message.id.clone();
    let mut body = vec![
        web::styled_row(
            &format!("{id}-head"),
            8,
            "center",
            web::style(),
            vec![
                web::styled(
                    &format!("{id}-author"),
                    &message.author,
                    web::style().size(14).bold().color(look.ink),
                ),
                web::styled(
                    &format!("{id}-time"),
                    format!("tick {}", message.time),
                    web::style().size(11).color(look.muted),
                ),
            ],
        ),
        web::styled(
            &format!("{id}-text"),
            &message.text,
            web::style().size(14).color(look.ink),
        ),
        reactions(&id, channel_id, message, look),
    ];
    for reply in &replies {
        body.push(web::card(
            &format!("{}-in-thread", reply.id),
            web::style()
                .padding(8)
                .background(look.surface)
                .border(look.muted)
                .radius(6),
            vec![
                web::styled(
                    &format!("{}-author", reply.id),
                    format!("{} replied", reply.author),
                    web::style().size(12).bold().color(look.muted),
                ),
                web::styled(
                    &format!("{}-text", reply.id),
                    &reply.text,
                    web::style().size(13).color(look.ink),
                ),
                reactions(&reply.id, channel_id, reply, look),
            ],
        ));
    }
    let action = PageAction {
        method: "POST".into(),
        url: format!("/channels/{channel_id}/messages"),
        fields: [
            ("text".to_string(), format!("${id}-reply-text")),
            ("parent".to_string(), id.clone()),
        ]
        .into_iter()
        .collect(),
    };
    body.push(PageElement::Form {
        id: format!("{id}-reply"),
        action: action.clone(),
        children: vec![
            PageElement::Input {
                id: format!("{id}-reply-text"),
                label: format!("Reply in thread ({} replies)", replies.len()),
                value: String::new(),
                placeholder: String::new(),
            },
            PageElement::Button {
                id: format!("{id}-reply-submit"),
                text: "Reply".into(),
                action,
            },
        ],
    });
    web::styled_row(
        &format!("{id}-row"),
        10,
        "start",
        web::style().padding(8),
        vec![
            avatar(&format!("{id}-avatar"), &message.author, look),
            web::card(&format!("{id}-body"), web::style().flex(1), body),
        ],
    )
}
fn transcript(id: &str, channel: &Channel, look: &Look) -> Vec<PageElement> {
    let mut out = vec![];
    for message in channel.messages.iter().filter(|m| m.parent.is_none()) {
        let replies: Vec<_> = channel
            .messages
            .iter()
            .filter(|m| m.parent.as_deref() == Some(message.id.as_str()))
            .collect();
        out.push(message_block(id, message, replies, look));
        out.push(web::divider(&format!("{}-rule", message.id)));
    }
    out
}
/// The whole workspace: sidebar, open conversation, composer.
pub fn workspace(
    state: &ChatState,
    actor: &str,
    open: Option<&str>,
    look: &Look,
) -> SimResult<HttpResponse> {
    let mut main = vec![];
    let mut title = state.workspace.clone();
    match open {
        None => {
            main.push(web::styled(
                "empty",
                "Pick a channel or start a direct message.",
                web::style().size(16).color(look.muted),
            ));
        }
        Some(id) => match state.channel(actor, id) {
            Err(e) => return web::error(403, e),
            Ok(channel) => {
                let heading = if state.channels.contains_key(id) {
                    format!("# {id}")
                } else {
                    partner(id, actor)
                };
                title = format!("{heading} · {}", look.brand);
                main.push(web::styled_row(
                    "channel-head",
                    8,
                    "center",
                    web::style().padding(10),
                    vec![
                        web::styled(
                            "channel-title",
                            &heading,
                            web::style().size(18).bold().color(look.ink),
                        ),
                        web::badge(
                            "channel-members",
                            format!("{} members", channel.members.len()),
                            web::style()
                                .background(look.surface)
                                .border(look.muted)
                                .color(look.muted)
                                .radius(10)
                                .padding(3)
                                .size(12),
                        ),
                    ],
                ));
                main.push(web::divider("channel-rule"));
                main.extend(transcript(id, channel, look));
                main.push(web::form(
                    "send",
                    &format!("/channels/{id}/messages"),
                    &[("text", format!("Message {heading}").as_str(), "")],
                ));
            }
        },
    }
    if title.is_empty() {
        title = look.brand.to_owned();
    }
    let rail = web::card(
        "rail",
        web::style()
            .background(look.rail)
            .width(64)
            .flex(0)
            .padding(8),
        vec![web::thumbnail(
            "rail-mark",
            look.brand,
            web::style()
                .width(48)
                .height(48)
                .radius(16)
                .background(look.accent)
                .color("#ffffff")
                .size(11)
                .align("center"),
        )],
    );
    web::themed_page(
        &title,
        theme(state, look),
        vec![web::styled_row(
            "shell",
            0,
            "stretch",
            web::style(),
            vec![
                rail,
                sidebar(state, actor, open, look),
                web::card(
                    "main",
                    web::style().background(look.surface).padding(12).flex(3),
                    main,
                ),
            ],
        )],
    )
}
