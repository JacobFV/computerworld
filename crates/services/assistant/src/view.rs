//! Page rendering as HTML. Both products serve identical routes; the skin decides whether
//! the page is ChatGPT's dark shell (history sidebar, centred pill composer, a narrow
//! message column) or Claude's warm paper (serif greeting, the composer card, user turns
//! in a soft bubble and replies as plain text). `base.css` holds the structure both
//! share, `chatgpt.css` and `claude.css` the two looks; the seeded palette goes on
//! `<html>` as custom properties so the sheets stay static files.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `shell`,
//! `sidebar`, `main`, `side-brand`, `side-logo`, `side-brand-name`, `side-new`,
//! `side-label`, `side-<conv>` (a link, with `side-<conv>-title` inside), `side-empty`,
//! `home-greeting`, `suggestions`, `suggestion-<n>`, `composer` (the form),
//! `composer-message`, `composer-submit`, `home-model`, `head`, `head-title`,
//! `head-model`, `msg-<n>` with `-avatar`, `-card`, `-text`, `-sources` and
//! `-cite-<k>`, `turn-controls`, `regenerate`, `rename` (the form), `rename-title`,
//! `rename-submit` and `delete`. A button that posted with no inputs is now a
//! one-button form around it: `suggestion-<n>-form`, `regenerate-form`, `delete-form`.
//!
//! Everything the page draws that looks pressable is one of those: the composer's model
//! name and the product name in the top bar are readouts, drawn as plain text with no
//! hover and no pointer, because this world has one model per site and no page script to
//! open a menu with.
use crate::{AssistantState, Message};
use cw_protocol::{HttpResponse, Result as SimResult};
use cw_service_common as web;
use cw_service_common::html::{
    button, div, el, form, hidden, link, span, text_input, Document, Html as Node,
};

const BASE: &str = include_str!("base.css");
const CHATGPT: &str = include_str!("chatgpt.css");
const CLAUDE: &str = include_str!("claude.css");

/// Skin and palette resolved once per render.
struct Chrome {
    skin: &'static str,
    root: String,
}
impl Chrome {
    fn read(s: &AssistantState) -> Self {
        let skin = match s.skin.as_str() {
            "claude" => "claude",
            "chatgpt" => "chatgpt",
            _ if s.brand.to_ascii_lowercase().contains("claude") => "claude",
            _ => "chatgpt",
        };
        let claude = skin == "claude";
        let pick = |v: &Option<String>, d: &str| {
            v.clone()
                .filter(|c| rgb(c).is_some())
                .unwrap_or_else(|| d.to_owned())
        };
        let accent = pick(&s.theme.accent, if claude { "#c15f3c" } else { "#10a37f" });
        let paper = pick(
            &s.theme.background,
            if claude { "#faf9f5" } else { "#212121" },
        );
        let surface = pick(&s.theme.surface, if claude { "#f0eee6" } else { "#2f2f2f" });
        let ink = pick(&s.theme.ink, if claude { "#1f1e1d" } else { "#ececec" });
        let muted = pick(&s.theme.muted, if claude { "#6b6a65" } else { "#9b9b9b" });
        let (pr, pg, pb) = rgb(&paper).unwrap_or((255, 255, 255));
        let dark = u32::from(pr) * 3 + u32::from(pg) * 6 + u32::from(pb) < 1280;
        // A dark shell puts its history on a darker band and its cards on the surface;
        // a light one puts the history on the surface and its cards on white.
        let (side, card) = if dark {
            (
                format!("#{:02x}{:02x}{:02x}", pr / 10 * 7, pg / 10 * 7, pb / 10 * 7),
                surface.clone(),
            )
        } else {
            (surface.clone(), "#ffffff".to_owned())
        };
        let (ir, ig, ib) = rgb(&ink).unwrap_or((0, 0, 0));
        let content = s.theme.content_width.unwrap_or(760).clamp(320, 1200);
        Self {
            skin,
            root: format!(
                "--accent: {accent}; --paper: {paper}; --surface: {surface}; --ink: {ink}; --muted: {muted}; \
                 --side: {side}; --card: {card}; --rule: rgba({ir},{ig},{ib},.14); --hover: rgba({ir},{ig},{ib},.07); \
                 --content: {content}px"
            ),
        }
    }
    fn document(&self, title: &str, page_class: &str, body: Node) -> Document {
        Document::new(title)
            .lang("en")
            .stylesheet(BASE)
            .stylesheet(if self.skin == "claude" {
                CLAUDE
            } else {
                CHATGPT
            })
            .root_style(&self.root)
            .body_class(&format!("skin-{} {page_class}", self.skin))
            .body([body])
    }
}
/// `#rrggbb` or `#rgb`.
fn rgb(colour: &str) -> Option<(u8, u8, u8)> {
    let hex = colour.strip_prefix('#')?;
    let at = |i: usize, n: usize| u8::from_str_radix(hex.get(i..i + n)?, 16).ok();
    match hex.len() {
        6 | 8 => Some((at(0, 2)?, at(2, 2)?, at(4, 2)?)),
        3 => Some((at(0, 1)? * 17, at(1, 1)? * 17, at(2, 1)? * 17)),
        _ => None,
    }
}
fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}
fn model(s: &AssistantState) -> &str {
    if s.model_label.is_empty() {
        &s.brand
    } else {
        &s.model_label
    }
}

/// Sidebar + main column, the layout both products share.
fn shell(
    s: &AssistantState,
    chrome: &Chrome,
    actor: &str,
    active: Option<&str>,
    title: &str,
    page_class: &str,
    main: Vec<Node>,
) -> SimResult<HttpResponse> {
    let chats = el("nav")
        .class("chats")
        .attr("aria-label", "Chats")
        .each(s.mine(actor), |c| {
            let id = format!("side-{}", c.id);
            el("a")
                .id(id.as_str())
                .class(if active == Some(c.id.as_str()) {
                    "chat on"
                } else {
                    "chat"
                })
                .attr("href", format!("/c/{}", c.id))
                .child(
                    span("title")
                        .id(format!("{id}-title"))
                        .text(c.title.as_str()),
                )
        });
    let side = el("aside")
        .id("sidebar")
        .class("sidebar")
        .child(
            div("brand")
                .id("side-brand")
                .child(
                    span("logo")
                        .id("side-logo")
                        .attr("aria-hidden", "true")
                        .text(initial(&s.brand)),
                )
                .child(span("name").id("side-brand-name").text(s.brand.as_str())),
        )
        .child(link("side-new", "/", "New chat").class("new"))
        .child(
            el("p")
                .id("side-label")
                .class("label")
                .text(if chrome.skin == "claude" {
                    "Recents"
                } else {
                    "Chats"
                }),
        )
        .child(chats)
        .when(s.mine(actor).next().is_none(), |n| {
            n.child(
                el("p")
                    .id("side-empty")
                    .class("empty")
                    .text("No conversations yet."),
            )
        })
        .child(
            div("account")
                .id("side-account")
                .child(
                    span("avatar")
                        .attr("aria-hidden", "true")
                        .text(initial(actor)),
                )
                .child(span("who").id("side-account-name").text(actor)),
        );
    let body = div("shell")
        .id("shell")
        .child(side)
        .child(el("main").id("main").class("main").children(main));
    web::html::page(&chrome.document(title, page_class, body))
}
/// The message box. One line, so Enter sends, which is what both products do.
fn composer(s: &AssistantState, chrome: &Chrome, action: &str) -> Node {
    let placeholder = if chrome.skin == "claude" {
        "How can I help you today?"
    } else {
        "Ask anything"
    };
    form("composer", action, "post").class("composer").child(
        div("card")
            .child(
                text_input("composer-message", "message", "")
                    .attr("aria-label", "Message")
                    .attr("placeholder", placeholder)
                    .attr("autocomplete", "off"),
            )
            .child(
                // No attach and no microphone: this world has neither, and a "+" that
                // swallowed the click would be a lie. What is left is the model the
                // reply will come from, as a readout, and the send button.
                div("tools")
                    .child(span("grow"))
                    .child(span("picker").text(model(s)))
                    .child(
                        button("composer-submit", "")
                            .class("send")
                            .attr("aria-label", "Send message")
                            .child(span("arrow").child(el("i"))),
                    ),
            ),
    )
}
pub(crate) fn home(s: &AssistantState, actor: &str) -> SimResult<HttpResponse> {
    let chrome = Chrome::read(s);
    let greeting = match s.greeting.as_str() {
        "" => "What can I help with?",
        g => g,
    };
    let hero = div("hero")
        .child(
            el("h1")
                .id("home-greeting")
                .class("greeting")
                .child(
                    span("spark")
                        .attr("aria-hidden", "true")
                        .child(el("i"))
                        .child(el("i")),
                )
                .text(greeting),
        )
        .child(composer(s, &chrome, "/conversations"))
        .when(!s.suggestions.is_empty(), |n| {
            n.child(div("suggestions").id("suggestions").each(
                s.suggestions.iter().enumerate(),
                |(i, text)| {
                    form(&format!("suggestion-{i}-form"), "/conversations", "post")
                        .child(hidden("message", text))
                        .child(button(&format!("suggestion-{i}"), text.as_str()))
                },
            ))
        });
    let main = vec![
        // The product's name, not a menu: there is one model here, so there is nothing
        // for a caret to open.
        el("header")
            .class("topbar")
            .child(span("product").id("top-product").text(s.brand.as_str())),
        hero,
        if s.model_label.is_empty() {
            web::html::empty()
        } else {
            el("p").id("home-model").class("fine").text(format!(
                "{} · deterministic replies, citations you can check",
                s.model_label
            ))
        },
    ];
    shell(s, &chrome, actor, None, &s.brand, "page-home", main)
}
fn turn(index: usize, message: &Message, actor: &str) -> Node {
    let mine = message.role == "user";
    let id = format!("msg-{index}");
    el("article")
        .id(id.as_str())
        .class(if mine { "msg user" } else { "msg assistant" })
        .child(
            span("avatar")
                .id(format!("{id}-avatar"))
                .attr("aria-hidden", "true")
                .text(if mine {
                    initial(actor)
                } else {
                    "AI".to_owned()
                }),
        )
        .child(
            div("bubble")
                .id(format!("{id}-card"))
                .child(
                    el("p")
                        .id(format!("{id}-text"))
                        .class("text")
                        .text(message.text.as_str()),
                )
                .when(!message.citations.is_empty(), |n| {
                    n.child(
                        div("sources")
                            .child(span("label").id(format!("{id}-sources")).text("Sources"))
                            .each(message.citations.iter().enumerate(), |(k, c)| {
                                link(
                                    &format!("{id}-cite-{k}"),
                                    c.url.as_str(),
                                    format!("[{}] {}", k + 1, c.label),
                                )
                                .class("cite")
                            }),
                    )
                }),
        )
}
pub(crate) fn conversation(s: &AssistantState, actor: &str, id: &str) -> SimResult<HttpResponse> {
    let c = match s.open(actor, id) {
        Ok(c) => c,
        Err(e) => return web::error(404, e),
    };
    let chrome = Chrome::read(s);
    let head = el("header")
        .id("head")
        .class("topbar")
        .child(
            el("h1")
                .id("head-title")
                .class("title")
                .text(c.title.as_str()),
        )
        .child(span("badge").id("head-model").text(model(s)))
        .child(span("grow"))
        .child(
            form("rename", format!("/conversations/{id}/rename"), "post")
                .class("rename")
                .child(
                    text_input("rename-title", "title", &c.title)
                        .attr("aria-label", "Rename conversation"),
                )
                .child(button("rename-submit", "Rename")),
        )
        .child(
            form("delete-form", format!("/conversations/{id}/delete"), "post").child(
                button("delete", "Delete")
                    .class("danger")
                    .attr("aria-label", "Delete conversation"),
            ),
        );
    let thread = div("thread")
        .id("thread")
        .each(c.messages.iter().enumerate(), |(i, m)| turn(i, m, actor))
        .child(
            div("controls").id("turn-controls").child(
                form(
                    "regenerate-form",
                    format!("/conversations/{id}/regenerate"),
                    "post",
                )
                .child(button("regenerate", "Regenerate").class("ghost")),
            ),
        );
    let dock = div("dock")
        .child(composer(
            s,
            &chrome,
            &format!("/conversations/{id}/messages"),
        ))
        .child(el("p").id("dock-note").class("fine").text(format!(
            "{} can make mistakes. Open the sources to check.",
            s.brand
        )));
    let title = format!("{} - {}", c.title, s.brand);
    shell(
        s,
        &chrome,
        actor,
        Some(id),
        &title,
        "page-chat",
        vec![head, thread, dock],
    )
}
