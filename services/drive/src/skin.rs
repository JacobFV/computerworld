//! The branded drives: `gdrive` lays the same items out as a card gallery, `dropbox` as a list.
//!
//! Marks and covers are inert thumbnails. Every card, button and form on these pages carries a
//! route this crate serves, and every control that would be refused is simply not drawn.
use super::{DriveState, Node, NodeKind, Screen, TRASH};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_service_common as web;
/// Longer previews are still stored; drawing all of one would push the metadata off the page.
const PREVIEW_LINES: usize = 40;
struct Palette {
    accent: String,
    ink: String,
    muted: String,
    surface: String,
    line: String,
}
fn palette(theme: &PageTheme) -> Palette {
    Palette {
        accent: theme.accent.clone().unwrap_or_else(|| "#1a73e8".into()),
        ink: theme.ink.clone().unwrap_or_else(|| "#202124".into()),
        muted: theme.muted.clone().unwrap_or_else(|| "#5f6368".into()),
        surface: theme.surface.clone().unwrap_or_else(|| "#f8fafd".into()),
        line: "#dadce0".into(),
    }
}
/// What a node is, at a glance and before a word is read.
fn tint(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Folder => "#5f6368",
        NodeKind::File => "#188038",
        NodeKind::Shortcut => "#1a73e8",
    }
}
fn theme_of(s: &DriveState, p: &Palette) -> PageTheme {
    let mut t = s.theme.clone();
    t.accent.get_or_insert_with(|| p.accent.clone());
    t.background.get_or_insert_with(|| "#ffffff".into());
    t.surface.get_or_insert_with(|| p.surface.clone());
    t.ink.get_or_insert_with(|| p.ink.clone());
    t.muted.get_or_insert_with(|| p.muted.clone());
    t.content_width.get_or_insert(1000);
    t
}
fn post(url: String) -> PageAction {
    PageAction {
        method: "POST".into(),
        url,
        fields: Default::default(),
    }
}
/// Where a node opens. A shortcut leaves the site entirely, which is the point of a shortcut.
fn open(n: &Node) -> PageAction {
    match n.kind {
        NodeKind::Folder => web::visit(format!("/drive/folders/{}", n.id)),
        NodeKind::Shortcut if !n.target_url.is_empty() => web::visit(n.target_url.clone()),
        _ => web::visit(format!("/file/{}", n.id)),
    }
}
fn button(id: &str, label: &str, action: PageAction, width: u32, p: &Palette) -> PageElement {
    web::card_action(
        id,
        web::style()
            .background("#ffffff")
            .border(p.line.as_str())
            .radius(18)
            .padding(9)
            .width(width),
        action,
        vec![web::styled(
            &format!("{id}-label"),
            label,
            web::style()
                .size(13)
                .medium()
                .align("center")
                .color(p.ink.as_str()),
        )],
    )
}
fn chrome(s: &DriveState, p: &Palette) -> Vec<PageElement> {
    vec![
        web::styled_row(
            "chrome",
            14,
            "center",
            web::style(),
            vec![
                web::thumbnail(
                    "chrome-mark",
                    s.brand().chars().next().unwrap_or('D').to_string(),
                    web::style()
                        .background(p.accent.as_str())
                        .color("#ffffff")
                        .width(34)
                        .height(34)
                        .radius(8)
                        .size(16),
                ),
                web::styled(
                    "chrome-brand",
                    s.brand(),
                    web::style()
                        .size(20)
                        .medium()
                        .color(p.ink.as_str())
                        .width(150),
                ),
                web::link("nav-drive", "My files", "/"),
                web::link("nav-shared", "Shared with me", "/shared-with-me"),
                web::link("nav-starred", "Starred", "/starred"),
                web::link("nav-trash", "Trash", "/trash"),
            ],
        ),
        web::spacer("chrome-gap", 10),
        web::form("find", "/search", &[("q", "Search files", "")]),
        web::divider("chrome-rule"),
        web::spacer("chrome-gap2", 16),
    ]
}
fn crumbs(s: &DriveState, actor: &str, id: &str, p: &Palette) -> PageElement {
    let mut trail = vec![];
    for node in s.path_to(actor, id) {
        if !trail.is_empty() {
            trail.push(web::styled(
                &format!("crumb-sep-{}", node.id),
                "›",
                web::style().size(13).color(p.muted.as_str()).width(10),
            ));
        }
        trail.push(web::link(
            &format!("crumb-{}", node.id),
            node.name.as_str(),
            match node.kind {
                NodeKind::Folder => format!("/drive/folders/{}", node.id),
                _ => format!("/file/{}", node.id),
            },
        ));
    }
    web::styled_row("crumbs", 6, "center", web::style(), trail)
}
fn tags(n: &Node, s: &DriveState, actor: &str) -> Vec<PageElement> {
    let mut out = vec![web::badge(
        &format!("item-kind-{}", n.id),
        n.kind.label(),
        web::style().background(tint(n.kind)).size(10),
    )];
    if !n.shared_with.is_empty() {
        out.push(web::badge(
            &format!("item-shared-{}", n.id),
            format!("Shared · {}", n.shared_with.len()),
            web::style().background("#1a73e8").size(10),
        ));
    }
    if s.is_starred(actor, &n.id) {
        out.push(web::badge(
            &format!("item-star-{}", n.id),
            "Starred",
            web::style().background("#f9ab00").color("#202124").size(10),
        ));
    }
    out
}
/// One item, drawn either as a gallery tile or as a list row; the click target is the whole card.
fn item(n: &Node, s: &DriveState, actor: &str, p: &Palette, dense: bool) -> PageElement {
    let id = &n.id;
    let frame = web::style()
        .background("#ffffff")
        .border(p.line.as_str())
        .radius(10)
        .padding(12);
    let meta = web::styled(
        &format!("item-meta-{id}"),
        format!("{} · {}", n.owner, n.size_text()),
        web::style().size(12).color(p.muted.as_str()).one_line(),
    );
    let children = if dense {
        vec![web::styled_row(
            &format!("item-row-{id}"),
            12,
            "center",
            web::style(),
            vec![
                web::thumbnail(
                    &format!("item-cover-{id}"),
                    n.kind.label(),
                    web::style()
                        .background(tint(n.kind))
                        .color("#ffffff")
                        .width(36)
                        .height(36)
                        .radius(6)
                        .size(10),
                ),
                web::styled(
                    &format!("item-name-{id}"),
                    n.name.as_str(),
                    web::style()
                        .size(15)
                        .medium()
                        .color(p.ink.as_str())
                        .flex(5)
                        .one_line(),
                ),
                meta,
                web::styled_row(
                    &format!("item-tags-{id}"),
                    6,
                    "center",
                    web::style().flex(3),
                    tags(n, s, actor),
                ),
            ],
        )]
    } else {
        vec![
            web::thumbnail(
                &format!("item-cover-{id}"),
                n.name.as_str(),
                web::style()
                    .background(tint(n.kind))
                    .color("#ffffff")
                    .height(84)
                    .radius(6)
                    .size(13),
            ),
            web::styled(
                &format!("item-name-{id}"),
                n.name.as_str(),
                web::style()
                    .size(14)
                    .medium()
                    .color(p.ink.as_str())
                    .one_line(),
            ),
            meta,
            web::styled_row(
                &format!("item-tags-{id}"),
                6,
                "center",
                web::style(),
                tags(n, s, actor),
            ),
        ]
    };
    web::card_action(&format!("item-{id}"), frame, open(n), children)
}
fn items(nodes: &[&Node], s: &DriveState, actor: &str, p: &Palette, dense: bool) -> PageElement {
    web::grid(
        "items",
        if dense { 1 } else { 3 },
        if dense { 8 } else { 16 },
        nodes.iter().map(|n| item(n, s, actor, p, dense)).collect(),
    )
}
fn title(id: &str, text: impl Into<String>, p: &Palette) -> PageElement {
    web::styled(id, text, web::style().size(24).bold().color(p.ink.as_str()))
}
fn note(id: &str, text: impl Into<String>, p: &Palette) -> PageElement {
    web::styled(id, text, web::style().size(13).color(p.muted.as_str()))
}
fn section(id: &str, text: impl Into<String>, p: &Palette) -> PageElement {
    web::styled(
        id,
        text,
        web::style().size(15).medium().color(p.ink.as_str()),
    )
}
/// The star control, which every actor who can see a node may press for themselves.
fn star(id: &str, on: bool, p: &Palette) -> PageElement {
    web::card_action(
        "star",
        web::style()
            .background(if on { "#f9ab00" } else { "#ffffff" })
            .border(p.line.as_str())
            .radius(18)
            .padding(9)
            .width(110),
        post(format!("/nodes/{id}/star")),
        vec![web::styled(
            "star-label",
            if on { "Starred" } else { "Star" },
            web::style()
                .size(13)
                .medium()
                .align("center")
                .color("#202124"),
        )],
    )
}
fn head(id: &str, name: &str, kind: NodeKind, starred: bool, p: &Palette) -> PageElement {
    web::styled_row(
        "head",
        14,
        "center",
        web::style(),
        vec![
            web::thumbnail(
                "head-mark",
                kind.label(),
                web::style()
                    .background(tint(kind))
                    .color("#ffffff")
                    .width(42)
                    .height(42)
                    .radius(6)
                    .size(10),
            ),
            web::styled(
                "head-title",
                name,
                web::style()
                    .size(24)
                    .bold()
                    .color(p.ink.as_str())
                    .flex(6)
                    .one_line(),
            ),
            star(id, starred, p),
        ],
    )
}
/// The share link panel. A link that has not been minted is offered as a button, never faked.
fn sharing(n: &Node, s: &DriveState, actor: &str, p: &Palette) -> Vec<PageElement> {
    let id = &n.id;
    let mut e = vec![section("grant-title", "Sharing", p)];
    e.push(note(
        "grant-note",
        if n.shared_with.is_empty() {
            format!("Only {} can open this.", n.owner)
        } else {
            format!(
                "Shared with {}.",
                n.shared_with.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        },
        p,
    ));
    if n.link.is_empty() {
        if s.granted_to(actor, id) {
            e.push(button(
                "link-action",
                "Create a share link",
                post(format!("/nodes/{id}/link")),
                190,
                p,
            ));
        }
    } else {
        e.push(web::link(
            "link-url",
            format!("Share link: /s/{}", n.link),
            format!("/s/{}", n.link),
        ));
    }
    if n.owner == actor {
        e.push(web::form(
            "grant",
            &format!("/nodes/{id}/share"),
            &[("actor", "Share with (an actor id)", "")],
        ));
    }
    e
}
fn folder(s: &DriveState, actor: &str, node: &Node, p: &Palette) -> Vec<PageElement> {
    let id = &node.id;
    let kids = s.children(actor, id);
    let dense = s.skin.as_str() == "dropbox";
    let mut e = chrome(s, p);
    e.push(crumbs(s, actor, id, p));
    e.push(web::spacer("crumbs-gap", 8));
    e.push(head(id, &node.name, node.kind, s.is_starred(actor, id), p));
    e.push(note(
        "head-meta",
        format!(
            "{} item{} · owner {}",
            kids.len(),
            if kids.len() == 1 { "" } else { "s" },
            node.owner
        ),
        p,
    ));
    e.push(web::spacer("head-gap", 16));
    if kids.is_empty() {
        e.push(note("empty", "This folder is empty.", p));
    } else {
        e.push(items(&kids, s, actor, p, dense));
    }
    e.push(web::spacer("make-gap", 24));
    e.push(web::divider("make-rule"));
    e.extend(sharing(node, s, actor, p));
    // Only a direct grant may file something here, so the forms only appear when it exists.
    if s.granted_to(actor, id) {
        e.push(web::spacer("forms-gap", 18));
        e.push(section("make-title", "New folder", p));
        e.push(web::form(
            "make",
            "/folders",
            &[("name", "Folder name", ""), ("parent", "In folder", id)],
        ));
        e.push(section("upload-title", "Upload a text file", p));
        e.push(web::form(
            "upload",
            "/files",
            &[
                ("name", "File name", ""),
                ("parent", "In folder", id),
                ("content", "Contents", ""),
            ],
        ));
    }
    e
}
fn preview(n: &Node, p: &Palette) -> Vec<PageElement> {
    let mut inner: Vec<_> = n
        .content
        .lines()
        .take(PREVIEW_LINES)
        .enumerate()
        .map(|(i, line)| {
            if line.trim().is_empty() {
                web::spacer(&format!("preview-gap-{i}"), 8)
            } else {
                web::styled(
                    &format!("preview-line-{i}"),
                    line,
                    web::style().size(13).color(p.ink.as_str()),
                )
            }
        })
        .collect();
    if inner.is_empty() {
        inner.push(note("preview-empty", "No preview for this item.", p));
    }
    let mut e = vec![web::card(
        "preview",
        web::style()
            .background("#ffffff")
            .border(p.line.as_str())
            .radius(8)
            .padding(20),
        inner,
    )];
    if !n.target_url.is_empty() {
        e.push(web::link("target", &n.target_url, &n.target_url));
    }
    e.extend(web::links("content", &n.content));
    e
}
fn details(n: &Node, p: &Palette) -> PageElement {
    let rows = [
        ("kind", n.kind.label().to_owned()),
        ("owner", n.owner.clone()),
        ("size", n.size_text()),
        (
            "type",
            if n.mime.is_empty() {
                "—".to_owned()
            } else {
                n.mime.clone()
            },
        ),
        ("added", format!("tick {}", n.tick)),
    ];
    web::card(
        "details",
        web::style()
            .background(p.surface.as_str())
            .radius(8)
            .padding(16),
        rows.iter()
            .map(|(key, value)| {
                web::styled_row(
                    &format!("detail-{key}"),
                    10,
                    "center",
                    web::style(),
                    vec![
                        web::styled(
                            &format!("detail-key-{key}"),
                            *key,
                            web::style().size(12).color(p.muted.as_str()).width(90),
                        ),
                        web::styled(
                            &format!("detail-value-{key}"),
                            value.as_str(),
                            web::style().size(13).color(p.ink.as_str()).one_line(),
                        ),
                    ],
                )
            })
            .collect(),
    )
}
fn file(s: &DriveState, actor: &str, node: &Node, public: bool, p: &Palette) -> Vec<PageElement> {
    let id = &node.id;
    let mut e = chrome(s, p);
    if public {
        e.push(web::badge(
            "public",
            "Opened with a share link",
            web::style().background(p.accent.as_str()).size(11),
        ));
    } else {
        e.push(crumbs(s, actor, id, p));
    }
    e.push(web::spacer("crumbs-gap", 8));
    if public {
        e.push(title("head-title", node.name.as_str(), p));
    } else {
        e.push(head(id, &node.name, node.kind, s.is_starred(actor, id), p));
    }
    e.push(web::spacer("head-gap", 14));
    e.extend(preview(node, p));
    e.push(web::spacer("details-gap", 18));
    e.push(details(node, p));
    if public {
        return e;
    }
    e.push(web::spacer("sharing-gap", 18));
    e.extend(sharing(node, s, actor, p));
    if s.granted_to(actor, id) && node.parent.is_some() {
        e.push(web::spacer("rename-gap", 18));
        e.push(section("rename-title", "Rename or move", p));
        e.push(web::form(
            "rename",
            &format!("/nodes/{id}"),
            &[
                ("name", "Name", &node.name),
                (
                    "parent",
                    "Folder id",
                    node.parent.as_deref().unwrap_or_default(),
                ),
            ],
        ));
        if node.parent.as_deref() != Some(TRASH) {
            e.push(button(
                "trash-action",
                "Move to trash",
                post(format!("/nodes/{id}/trash")),
                160,
                p,
            ));
        }
    }
    e
}
fn listing(
    s: &DriveState,
    actor: &str,
    heading: &str,
    subtitle: String,
    nodes: Vec<&Node>,
    p: &Palette,
) -> Vec<PageElement> {
    let mut e = chrome(s, p);
    e.push(title("head-title", heading, p));
    e.push(note("head-meta", subtitle, p));
    e.push(web::spacer("head-gap", 16));
    if nodes.is_empty() {
        e.push(note("empty", "Nothing here.", p));
    } else {
        e.push(items(&nodes, s, actor, p, s.skin.as_str() == "dropbox"));
    }
    e
}
pub(crate) fn view(s: &DriveState, actor: &str, screen: Screen) -> Result<HttpResponse> {
    let p = palette(&s.theme);
    let brand = s.brand().to_owned();
    let count = |n: usize| format!("{n} item{}", if n == 1 { "" } else { "s" });
    let (heading, elements) = match screen {
        Screen::Folder(id) => match s.read(actor, id) {
            Ok(node) => (
                format!("{} · {brand}", node.name),
                folder(s, actor, node, &p),
            ),
            Err(e) => return web::error(403, e),
        },
        Screen::File(id) => match s.read(actor, id) {
            Ok(node) => (
                format!("{} · {brand}", node.name),
                file(s, actor, node, false, &p),
            ),
            Err(e) => return web::error(403, e),
        },
        Screen::Link(link) => match s.by_link(link) {
            Some(node) => (
                format!("{} · {brand}", node.name),
                file(s, actor, node, true, &p),
            ),
            None => return web::error(404, "no such share link"),
        },
        Screen::SharedWithMe => {
            let found = s.shared_with_me(actor);
            (
                format!("Shared with me · {brand}"),
                listing(
                    s,
                    actor,
                    "Shared with me",
                    format!("{} other people put here.", count(found.len())),
                    found,
                    &p,
                ),
            )
        }
        Screen::Starred => {
            let found = s.starred(actor);
            (
                format!("Starred · {brand}"),
                listing(
                    s,
                    actor,
                    "Starred",
                    "A star is yours alone; other people keep their own.".into(),
                    found,
                    &p,
                ),
            )
        }
        Screen::Trash => {
            let found = s.trash(actor);
            (
                format!("Trash · {brand}"),
                listing(
                    s,
                    actor,
                    "Trash",
                    format!(
                        "{} waiting here. Nothing is ever really gone.",
                        count(found.len())
                    ),
                    found,
                    &p,
                ),
            )
        }
        Screen::Search(q) => {
            let found = s.search(actor, q);
            (
                format!("{q} · {brand}"),
                listing(
                    s,
                    actor,
                    &format!("Results for {q}"),
                    count(found.len()),
                    found,
                    &p,
                ),
            )
        }
    };
    web::themed_page(&heading, theme_of(s, &p), elements)
}
