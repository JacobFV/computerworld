//! Gmail and Outlook layouts over the one mailbox. `plain` never reaches this module, so the
//! original page bytes cannot move; everything here is presentation plus real routes.
use crate::{stamp, MailState, Message, Nav};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_service_common as web;

/// One product's surface. Nothing here reaches a record: colours and labels only.
struct Look {
    brand: &'static str,
    title: &'static str,
    theme: PageTheme,
    /// Top bar fill and the ink that stays legible on it.
    bar: &'static str,
    bar_ink: &'static str,
    line: &'static str,
    unread: &'static str,
    read: &'static str,
    selected: &'static str,
    /// Compose button, the one saturated control on the page.
    chip: &'static str,
    chip_ink: &'static str,
    star: &'static str,
    radius: u32,
    folders: &'static [(&'static str, &'static str)],
}
fn theme(accent: &str, background: &str, surface: &str, ink: &str, muted: &str) -> PageTheme {
    PageTheme {
        accent: Some(accent.into()),
        background: Some(background.into()),
        surface: Some(surface.into()),
        ink: Some(ink.into()),
        muted: Some(muted.into()),
        content_width: None,
        font: None,
    }
}
fn gmail() -> Look {
    Look {
        brand: "Gmail",
        title: "Gmail",
        theme: theme("#c5221f", "#ffffff", "#f6f8fc", "#202124", "#5f6368"),
        bar: "#ffffff",
        bar_ink: "#202124",
        line: "#dadce0",
        unread: "#ffffff",
        read: "#f2f6fc",
        selected: "#c2dbff",
        chip: "#c2e7ff",
        chip_ink: "#001d35",
        star: "#f4b400",
        radius: 16,
        folders: &[
            ("inbox", "Inbox"),
            ("starred", "Starred"),
            ("sent", "Sent"),
            ("archive", "All Mail"),
        ],
    }
}
fn outlook() -> Look {
    Look {
        brand: "Outlook",
        title: "Mail - Outlook",
        theme: theme("#0f6cbd", "#f5f5f5", "#ffffff", "#242424", "#616161"),
        bar: "#0f6cbd",
        bar_ink: "#ffffff",
        line: "#e1dfdd",
        unread: "#ffffff",
        read: "#faf9f8",
        selected: "#cfe4fa",
        chip: "#0f6cbd",
        chip_ink: "#ffffff",
        star: "#c19c00",
        radius: 4,
        folders: &[
            ("inbox", "Inbox"),
            ("starred", "Favourites"),
            ("sent", "Sent Items"),
            ("archive", "Archive"),
        ],
    }
}
/// Six flat avatar fills, picked by name so one person keeps one colour across every render.
const AVATARS: [&str; 6] = [
    "#1a73e8", "#d93025", "#188038", "#e37400", "#9334e6", "#0f9d9d",
];
fn avatar(id: &str, name: &str, size: u32) -> PageElement {
    let fill = AVATARS[name.bytes().map(usize::from).sum::<usize>() % AVATARS.len()];
    let initials: String = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    web::thumbnail(
        id,
        initials,
        web::style()
            .background(fill)
            .color("#ffffff")
            .radius(size / 2)
            .width(size)
            .height(size)
            .size(13)
            .bold()
            .align("center"),
    )
}
/// A vertical stack; `Grid` with one column is the page model's column primitive.
fn stack(id: &str, gap: u32, children: Vec<PageElement>) -> PageElement {
    web::grid(id, 1, gap, children)
}
fn column(
    id: &str,
    gap: u32,
    style: cw_protocol::Style,
    children: Vec<PageElement>,
) -> PageElement {
    PageElement::Grid {
        id: id.into(),
        columns: 1,
        children,
        gap,
        style,
    }
}
/// A real control: one click, one route, no fields the reader has to fill in.
fn button(id: &str, text: &str, url: &str, fields: &[(&str, &str)]) -> PageElement {
    PageElement::Button {
        id: id.into(),
        text: text.into(),
        action: PageAction {
            method: "POST".into(),
            url: url.into(),
            fields: fields
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        },
        style: None,
    }
}
fn link_to(folder: &str, thread: Option<&str>, compose: bool) -> PageAction {
    let mut url = format!("/?folder={folder}");
    if let Some(thread) = thread {
        url.push_str(&format!("&thread={thread}"));
    }
    if compose {
        url.push_str("&compose=1");
    }
    web::visit(url)
}
fn snippet(body: &str, width: usize) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(width) {
        Some((cut, _)) => format!("{}…", &flat[..cut]),
        None => flat,
    }
}
pub(crate) fn mailbox(s: &MailState, actor: &str, nav: &Nav) -> Result<HttpResponse> {
    let look = match s.skin.as_str() {
        "outlook" => outlook(),
        _ => gmail(),
    };
    let palette = s.theme.clone().unwrap_or_else(|| look.theme.clone());
    let muted = palette.muted.clone().unwrap_or_else(|| "#5f6368".into());
    let ink = palette.ink.clone().unwrap_or_else(|| "#202124".into());
    let surface = palette.surface.clone().unwrap_or_else(|| "#f6f8fc".into());
    let accent = palette.accent.clone().unwrap_or_else(|| "#1a73e8".into());
    let brand = if s.brand.is_empty() {
        look.brand.to_owned()
    } else {
        s.brand.clone()
    };
    let folder = nav.folder().to_owned();
    let elements = vec![
        header(s, actor, &look, &brand, &accent, &muted, &folder),
        web::styled_row(
            "panes",
            12,
            "stretch",
            web::style().padding(12),
            vec![
                sidebar(s, actor, &look, &folder, &ink, &muted),
                list(s, actor, &look, &folder, nav, &ink, &muted, &surface),
                reading(s, actor, &look, &folder, nav, &ink, &muted, &accent),
            ],
        ),
    ];
    web::themed_page(look.title, palette, elements)
}
#[allow(clippy::too_many_arguments)]
fn header(
    s: &MailState,
    actor: &str,
    look: &Look,
    brand: &str,
    accent: &str,
    muted: &str,
    folder: &str,
) -> PageElement {
    let query = s.queries.get(actor).cloned().unwrap_or_default();
    let mut right = vec![web::form(
        "search",
        "/search",
        &[("q", "Search mail", query.as_str())],
    )];
    if !query.is_empty() {
        right.push(button(
            "search-clear",
            "Clear search",
            "/search",
            &[("q", "")],
        ));
    }
    let wordmark = if look.bar_ink == "#ffffff" {
        look.bar_ink
    } else {
        accent
    };
    web::styled_row(
        "bar",
        16,
        "center",
        web::style().background(look.bar).padding(12),
        vec![
            web::styled(
                "wordmark",
                brand,
                web::style().size(22).bold().color(wordmark).width(150),
            ),
            web::styled_row("search-box", 8, "center", web::style().flex(4), right),
            web::styled_row(
                "account",
                8,
                "center",
                web::style().flex(2).align("right"),
                vec![
                    web::styled(
                        "account-address",
                        s.address(actor),
                        web::style().size(13).color(muted).align("right").flex(3),
                    ),
                    avatar("account-avatar", &s.display(actor), 32),
                    web::badge(
                        "account-folder",
                        format!("{} unread", s.unread(actor, folder)),
                        web::style()
                            .background(look.selected)
                            .color(look.chip_ink)
                            .radius(10)
                            .padding(6)
                            .size(12)
                            .width(96),
                    ),
                ],
            ),
        ],
    )
}
fn sidebar(
    s: &MailState,
    actor: &str,
    look: &Look,
    folder: &str,
    ink: &str,
    muted: &str,
) -> PageElement {
    let mut items = vec![web::card_action(
        "compose",
        web::style()
            .background(look.chip)
            .radius(look.radius + 8)
            .padding(14),
        link_to(folder, None, true),
        vec![web::styled(
            "compose-label",
            "Compose",
            web::style().size(15).medium().color(look.chip_ink),
        )],
    )];
    for (key, label) in look.folders {
        let count = s.unread(actor, key);
        let current = *key == folder;
        // Only the folder being read is tinted; the rest inherit the sidebar's own fill.
        let mut style = web::style().radius(look.radius + 4).padding(8);
        if current {
            style = style.background(look.selected);
        }
        items.push(web::card_action(
            &format!("folder-{key}"),
            style,
            link_to(key, None, false),
            vec![web::styled_row(
                &format!("folder-{key}-row"),
                8,
                "center",
                web::style(),
                vec![
                    web::styled(
                        &format!("folder-{key}-label"),
                        *label,
                        web::style()
                            .size(14)
                            .color(if current { ink } else { muted })
                            .flex(4),
                    ),
                    web::badge(
                        &format!("folder-{key}-count"),
                        if count > 0 {
                            count.to_string()
                        } else {
                            String::new()
                        },
                        web::style().size(12).color(muted).width(28).align("right"),
                    ),
                ],
            )],
        ));
    }
    web::card(
        "sidebar",
        web::style().width(220).flex(0).padding(8),
        vec![stack("sidebar-items", 6, items)],
    )
}
#[allow(clippy::too_many_arguments)]
fn list(
    s: &MailState,
    actor: &str,
    look: &Look,
    folder: &str,
    nav: &Nav,
    ink: &str,
    muted: &str,
    surface: &str,
) -> PageElement {
    let label = look
        .folders
        .iter()
        .find(|(key, _)| *key == folder)
        .map_or("Mail", |(_, label)| *label);
    let conversations = s.conversations(actor, folder);
    let mut rows = vec![
        web::styled_row(
            "list-head",
            8,
            "center",
            web::style().padding(8),
            vec![
                web::styled(
                    "list-title",
                    label,
                    web::style().size(15).bold().color(ink).flex(4),
                ),
                web::styled(
                    "list-count",
                    format!("{} conversations", conversations.len()),
                    web::style().size(12).color(muted).align("right").flex(3),
                ),
            ],
        ),
        web::divider("list-rule"),
    ];
    if conversations.is_empty() {
        rows.push(web::styled(
            "list-empty",
            "Nothing here.",
            web::style().size(13).color(muted).padding(12),
        ));
    }
    for (m, count) in conversations {
        let box_ = &m.mailboxes[actor];
        let open = nav.thread.as_deref() == Some(m.thread());
        let line = vec![
            avatar(&format!("row-{}-avatar", m.id), &s.display(&m.sender), 32),
            column(
                &format!("row-{}-text", m.id),
                2,
                web::style().flex(6),
                vec![
                    web::styled_row(
                        &format!("row-{}-who", m.id),
                        6,
                        "center",
                        web::style(),
                        vec![
                            web::styled(
                                &format!("row-{}-sender", m.id),
                                s.display(&m.sender),
                                if box_.read {
                                    web::style().size(14).color(ink).flex(4).one_line()
                                } else {
                                    web::style().size(14).bold().color(ink).flex(4).one_line()
                                },
                            ),
                            web::badge(
                                &format!("row-{}-count", m.id),
                                if count > 1 {
                                    count.to_string()
                                } else {
                                    String::new()
                                },
                                web::style().size(11).color(muted).width(24),
                            ),
                        ],
                    ),
                    web::styled(
                        &format!("row-{}-subject", m.id),
                        &m.subject,
                        if box_.read {
                            web::style().size(13).color(ink).one_line()
                        } else {
                            web::style().size(13).bold().color(ink).one_line()
                        },
                    ),
                    web::styled(
                        &format!("row-{}-snippet", m.id),
                        snippet(&m.body, 64),
                        web::style().size(12).color(muted).one_line(),
                    ),
                ],
            ),
            column(
                &format!("row-{}-meta", m.id),
                2,
                web::style().width(72),
                vec![
                    web::styled(
                        &format!("row-{}-time", m.id),
                        stamp(m.time),
                        web::style().size(12).color(muted).align("right"),
                    ),
                    web::badge(
                        &format!("row-{}-star", m.id),
                        if box_.starred { "★" } else { "" },
                        web::style().size(13).color(look.star).align("right"),
                    ),
                ],
            ),
        ];
        rows.push(web::card_action(
            &format!("row-{}", m.id),
            web::style()
                .background(if open {
                    look.selected
                } else if box_.read {
                    look.read
                } else {
                    look.unread
                })
                .border(look.line)
                .radius(look.radius / 2)
                .padding(8),
            link_to(folder, Some(m.thread()), false),
            vec![web::styled_row(
                &format!("row-{}-line", m.id),
                10,
                "center",
                web::style(),
                line,
            )],
        ));
    }
    web::card(
        "list",
        web::style().flex(2).background(surface).padding(6),
        vec![stack("list-rows", 4, rows)],
    )
}
#[allow(clippy::too_many_arguments)]
fn reading(
    s: &MailState,
    actor: &str,
    look: &Look,
    folder: &str,
    nav: &Nav,
    ink: &str,
    muted: &str,
    accent: &str,
) -> PageElement {
    let body = if nav.compose {
        compose(s, actor, ink, muted)
    } else {
        match nav.thread.as_deref().map(|t| (t, s.thread(actor, t))) {
            Some((thread, messages)) if !messages.is_empty() => conversation(
                s, actor, look, folder, thread, &messages, ink, muted, accent,
            ),
            _ => vec![
                web::styled(
                    "reading-empty",
                    "Select a conversation",
                    web::style().size(16).medium().color(ink),
                ),
                web::styled(
                    "reading-hint",
                    "Pick a message on the left, or start a new one with Compose.",
                    web::style().size(13).color(muted),
                ),
            ],
        }
    };
    web::card(
        "reading",
        web::style().flex(3).padding(16),
        vec![stack("reading-body", 12, body)],
    )
}
fn compose(s: &MailState, actor: &str, ink: &str, muted: &str) -> Vec<PageElement> {
    vec![
        web::styled(
            "compose-title",
            "New message",
            web::style().size(18).medium().color(ink),
        ),
        web::styled(
            "compose-from",
            format!("From {}", s.address(actor)),
            web::style().size(12).color(muted),
        ),
        web::form(
            "new",
            "/send",
            &[
                ("to", "To", ""),
                ("cc", "Cc", ""),
                ("subject", "Subject", ""),
                ("body", "Message", ""),
            ],
        ),
    ]
}
#[allow(clippy::too_many_arguments)]
fn conversation(
    s: &MailState,
    actor: &str,
    look: &Look,
    folder: &str,
    thread: &str,
    messages: &[&Message],
    ink: &str,
    muted: &str,
    accent: &str,
) -> Vec<PageElement> {
    let last = messages[messages.len() - 1];
    let mut out = vec![web::styled_row(
        "thread-head",
        8,
        "center",
        web::style(),
        vec![
            web::styled(
                "thread-subject",
                &last.subject,
                web::style().size(20).medium().color(ink).flex(5),
            ),
            web::badge(
                "thread-size",
                format!("{} in thread", messages.len()),
                web::style()
                    .size(11)
                    .color(muted)
                    .border(look.line)
                    .radius(8)
                    .padding(4)
                    .width(110),
            ),
        ],
    )];
    for m in messages {
        let box_ = &m.mailboxes[actor];
        let mut card = vec![web::styled_row(
            &format!("read-{}-head", m.id),
            10,
            "center",
            web::style(),
            vec![
                avatar(&format!("read-{}-avatar", m.id), &s.display(&m.sender), 36),
                stack(
                    &format!("read-{}-who", m.id),
                    2,
                    vec![
                        web::styled(
                            &format!("read-{}-name", m.id),
                            s.display(&m.sender),
                            web::style().size(14).bold().color(ink),
                        ),
                        web::styled(
                            &format!("read-{}-line", m.id),
                            format!(
                                "{} → {}",
                                s.address(&m.sender),
                                m.to.iter()
                                    .chain(&m.cc)
                                    .map(|r| s.address(r))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            web::style().size(12).color(muted).one_line(),
                        ),
                    ],
                ),
                web::styled(
                    &format!("read-{}-time", m.id),
                    stamp(m.time),
                    web::style().size(12).color(muted).align("right").width(80),
                ),
            ],
        )];
        if !box_.labels.is_empty() {
            card.push(web::styled_row(
                &format!("read-{}-labels", m.id),
                6,
                "center",
                web::style(),
                box_.labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| {
                        web::badge(
                            &format!("read-{}-label-{i}", m.id),
                            label,
                            web::style()
                                .size(11)
                                .color(accent)
                                .border(look.line)
                                .radius(8)
                                .padding(4),
                        )
                    })
                    .collect(),
            ));
        }
        card.push(web::styled(
            &format!("read-{}-body", m.id),
            &m.body,
            web::style().size(14).color(ink),
        ));
        // Addresses written in the prose become real navigation; this is how sites connect.
        let links = web::links(&format!("read-{}", m.id), &m.body);
        if !links.is_empty() {
            card.push(web::styled_row(
                &format!("read-{}-links", m.id),
                10,
                "center",
                web::style(),
                links,
            ));
        }
        let route = format!("/messages/{}", m.id);
        card.push(web::styled_row(
            &format!("read-{}-actions", m.id),
            10,
            "center",
            web::style(),
            vec![
                button(
                    &format!("read-{}-star", m.id),
                    if box_.starred { "Unstar" } else { "Star" },
                    &route,
                    &[("star", "toggle"), ("folder", folder)],
                ),
                button(
                    &format!("read-{}-read", m.id),
                    if box_.read {
                        "Mark unread"
                    } else {
                        "Mark read"
                    },
                    &route,
                    &[
                        ("read", if box_.read { "false" } else { "true" }),
                        ("folder", folder),
                    ],
                ),
                button(
                    &format!("read-{}-archive", m.id),
                    "Archive",
                    &route,
                    &[("archive", "true"), ("folder", folder)],
                ),
                web::link(
                    &format!("read-{}-permalink", m.id),
                    "Permalink",
                    format!("/threads/{thread}"),
                ),
            ],
        ));
        card.push(web::form(
            &format!("read-{}-label", m.id),
            &route,
            &[("label", "Add label", "")],
        ));
        out.push(web::card(
            &format!("read-{}", m.id),
            web::style()
                .background(look.unread)
                .border(look.line)
                .radius(look.radius / 2)
                .padding(12),
            vec![stack(&format!("read-{}-stack", m.id), 8, card)],
        ));
    }
    let reply_to = if last.sender == actor {
        last.to.first().cloned().unwrap_or_else(|| actor.to_owned())
    } else {
        last.sender.clone()
    };
    let subject = if last.subject.to_lowercase().starts_with("re:") {
        last.subject.clone()
    } else {
        format!("Re: {}", last.subject)
    };
    out.push(web::form(
        "reply",
        "/send",
        &[
            ("to", "Reply to", s.address(&reply_to).as_str()),
            ("subject", "Subject", subject.as_str()),
            ("body", "Message", ""),
        ],
    ));
    out
}
