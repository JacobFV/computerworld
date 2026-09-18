//! The docs.google.com skin: a file grid, a document, a spreadsheet and a deck.
//!
//! Nothing here is decoration pretending to be a control. Covers and marks are inert
//! thumbnails; every card, form and link carries a route this crate actually serves.
use super::{parse_cell, DocType, DocsState, Document, Screen, SHEET_COLUMNS};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_service_common as web;
/// Sheets taller than this are still stored; drawing every row would make the page unreadable.
const MAX_ROWS: u32 = 60;
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
/// The three products wear three colours; the colour is the fastest read of what a file is.
fn tint(kind: DocType) -> &'static str {
    match kind {
        DocType::Doc => "#1a73e8",
        DocType::Sheet => "#188038",
        DocType::Slides => "#e37400",
    }
}
fn product(kind: Option<DocType>) -> &'static str {
    match kind {
        Some(DocType::Sheet) => "Google Sheets",
        Some(DocType::Slides) => "Google Slides",
        _ => "Google Docs",
    }
}
fn theme_of(s: &DocsState, p: &Palette) -> PageTheme {
    let mut t = s.theme.clone();
    t.accent.get_or_insert_with(|| p.accent.clone());
    t.background.get_or_insert_with(|| "#ffffff".into());
    t.surface.get_or_insert_with(|| p.surface.clone());
    t.ink.get_or_insert_with(|| p.ink.clone());
    t.muted.get_or_insert_with(|| p.muted.clone());
    t.content_width.get_or_insert(980);
    t
}
/// A mutation a card performs; `web::visit` covers the navigating half of the same idea.
fn post(url: String) -> PageAction {
    PageAction {
        method: "POST".into(),
        url,
        fields: Default::default(),
    }
}
fn chrome(brand: &str, kind: DocType, p: &Palette) -> Vec<PageElement> {
    vec![
        web::styled_row(
            "chrome",
            14,
            "center",
            web::style(),
            vec![
                web::thumbnail(
                    "chrome-mark",
                    kind.label(),
                    web::style()
                        .background(tint(kind))
                        .color("#ffffff")
                        .width(34)
                        .height(34)
                        .radius(8)
                        .size(11),
                ),
                web::styled(
                    "chrome-brand",
                    brand,
                    web::style()
                        .size(20)
                        .medium()
                        .color(p.ink.as_str())
                        .width(170),
                ),
                web::link("nav-all", "All files", "/"),
                web::link("nav-doc", "Docs", "/?type=doc"),
                web::link("nav-sheet", "Sheets", "/?type=sheet"),
                web::link("nav-slides", "Slides", "/?type=slides"),
                web::link("nav-starred", "Starred", "/starred"),
            ],
        ),
        web::spacer("chrome-gap", 10),
        web::divider("chrome-rule"),
        web::spacer("chrome-gap2", 16),
    ]
}
fn file_card(d: &Document, actor: &str, p: &Palette) -> PageElement {
    let id = &d.id;
    let mut meta = vec![
        web::badge(
            &format!("file-kind-{id}"),
            d.doc_type.label(),
            web::style().background(tint(d.doc_type)).size(10),
        ),
        web::styled(
            &format!("file-owner-{id}"),
            format!("{} · revision {}", d.owner, d.revision),
            web::style().size(12).color(p.muted.as_str()).one_line(),
        ),
    ];
    if d.starred.contains(actor) {
        meta.push(web::badge(
            &format!("file-star-{id}"),
            "Starred",
            web::style().background("#f9ab00").color("#202124").size(10),
        ));
    }
    web::card_action(
        &format!("file-{id}"),
        web::style()
            .background("#ffffff")
            .border(p.line.as_str())
            .radius(10)
            .padding(12),
        web::visit(format!("/documents/{id}")),
        vec![
            web::thumbnail(
                &format!("file-cover-{id}"),
                d.title.as_str(),
                web::style()
                    .background(tint(d.doc_type))
                    .color("#ffffff")
                    .height(86)
                    .radius(6)
                    .size(13),
            ),
            web::styled_row(&format!("file-meta-{id}"), 8, "center", web::style(), meta),
        ],
    )
}
fn gallery(id: &str, docs: &[&Document], actor: &str, p: &Palette) -> PageElement {
    web::grid(
        id,
        3,
        16,
        docs.iter().map(|d| file_card(d, actor, p)).collect(),
    )
}
fn home(s: &DocsState, actor: &str, kind: Option<DocType>, p: &Palette) -> Vec<PageElement> {
    let docs = s.visible(actor, kind);
    let mut e = chrome(product(kind), kind.unwrap_or_default(), p);
    e.push(web::styled(
        "home-title",
        match kind {
            Some(k) => format!("{}s", k.label()),
            None => "All files".to_owned(),
        },
        web::style().size(24).bold().color(p.ink.as_str()),
    ));
    e.push(web::styled(
        "home-count",
        format!(
            "{} file{} shared with you",
            docs.len(),
            if docs.len() == 1 { "" } else { "s" }
        ),
        web::style().size(13).color(p.muted.as_str()),
    ));
    e.push(web::spacer("home-gap", 14));
    if docs.is_empty() {
        e.push(web::styled(
            "home-empty",
            "Nothing here yet.",
            web::style().size(14).color(p.muted.as_str()),
        ));
    } else {
        e.push(gallery("files", &docs, actor, p));
    }
    e.push(web::spacer("create-gap", 24));
    e.push(web::divider("create-rule"));
    // Not `create-title`: the form below owns that id for its Title input.
    e.push(web::styled(
        "create-heading",
        "Start a new file",
        web::style().size(15).medium().color(p.ink.as_str()),
    ));
    e.push(web::form(
        "create",
        "/documents",
        &[
            ("title", "Title", ""),
            ("type", "Type: doc, sheet or slides", "doc"),
            ("body", "Content", ""),
            ("readers", "Readers", ""),
            ("writers", "Writers", ""),
        ],
    ));
    e
}
fn starred(s: &DocsState, actor: &str, p: &Palette) -> Vec<PageElement> {
    let docs = s.starred(actor);
    let mut e = chrome("Google Docs", DocType::Doc, p);
    e.push(web::styled(
        "starred-title",
        "Starred",
        web::style().size(24).bold().color(p.ink.as_str()),
    ));
    e.push(web::styled(
        "starred-note",
        "A star is yours alone; other people keep their own.",
        web::style().size(13).color(p.muted.as_str()),
    ));
    e.push(web::spacer("starred-gap", 14));
    if docs.is_empty() {
        e.push(web::styled(
            "starred-empty",
            "You have not starred anything yet.",
            web::style().size(14).color(p.muted.as_str()),
        ));
    } else {
        e.push(gallery("starred-files", &docs, actor, p));
    }
    e
}
fn head_cell(id: &str, text: &str, p: &Palette) -> PageElement {
    web::card(
        id,
        web::style()
            .background("#f1f3f4")
            .border(p.line.as_str())
            .radius(2)
            .padding(6)
            .height(30),
        vec![web::styled(
            &format!("{id}-text"),
            text,
            web::style()
                .size(11)
                .medium()
                .color(p.muted.as_str())
                .align("center")
                .one_line(),
        )],
    )
}
fn sheet(d: &Document, p: &Palette) -> Vec<PageElement> {
    let (mut columns, mut rows) = (4u8, 6u32);
    for cell in d.cells.keys() {
        if let Some((c, r)) = parse_cell(cell) {
            columns = columns.max(c + 1);
            rows = rows.max(r);
        }
    }
    let columns = columns.min(SHEET_COLUMNS);
    let rows = rows.min(MAX_ROWS);
    let letter = |c: u8| char::from(b'A' + c);
    let mut cells = vec![head_cell("sheet-corner", "", p)];
    for c in 0..columns {
        let name = letter(c);
        cells.push(head_cell(
            &format!("sheet-col-{name}"),
            &name.to_string(),
            p,
        ));
    }
    for r in 1..=rows {
        cells.push(head_cell(&format!("sheet-row-{r}"), &r.to_string(), p));
        for c in 0..columns {
            let name = format!("{}{r}", letter(c));
            let value = d.cells.get(&name).cloned().unwrap_or_default();
            // Row one is the sheet's own header row in every seeded sheet; give it weight.
            let text = web::style()
                .size(12)
                .color(if r == 1 {
                    p.ink.as_str()
                } else {
                    p.muted.as_str()
                })
                .one_line();
            cells.push(web::card(
                &format!("sheet-{name}"),
                web::style()
                    .background("#ffffff")
                    .border(p.line.as_str())
                    .radius(2)
                    .padding(6)
                    .height(30),
                vec![web::styled(
                    &format!("sheet-{name}-text"),
                    value,
                    if r == 1 { text.medium() } else { text },
                )],
            ));
        }
    }
    vec![
        web::grid("sheet", u32::from(columns) + 1, 2, cells),
        web::spacer("sheet-gap", 18),
        web::styled(
            "sheet-edit-title",
            "Edit a cell",
            web::style().size(15).medium().color(p.ink.as_str()),
        ),
        web::form(
            "cell",
            &format!("/documents/{}/cells", d.id),
            &[("cell", "Cell, for example B3", ""), ("value", "Value", "")],
        ),
    ]
}
fn deck(d: &Document, p: &Palette) -> Vec<PageElement> {
    let cards: Vec<_> = d
        .slides
        .iter()
        .enumerate()
        .map(|(i, slide)| {
            web::card(
                &format!("slide-{i}"),
                web::style()
                    .background("#ffffff")
                    .border(p.line.as_str())
                    .radius(8)
                    .padding(16)
                    .height(210),
                vec![
                    web::badge(
                        &format!("slide-no-{i}"),
                        format!("Slide {}", i + 1),
                        web::style().background(tint(DocType::Slides)).size(10),
                    ),
                    web::styled(
                        &format!("slide-title-{i}"),
                        slide.title.as_str(),
                        web::style().size(18).bold().color(p.ink.as_str()),
                    ),
                    web::divider(&format!("slide-rule-{i}")),
                    web::styled(
                        &format!("slide-body-{i}"),
                        slide.body.as_str(),
                        web::style().size(13).color(p.muted.as_str()),
                    ),
                ],
            )
        })
        .collect();
    let mut e = vec![];
    if cards.is_empty() {
        e.push(web::styled(
            "deck-empty",
            "This deck has no slides yet.",
            web::style().size(14).color(p.muted.as_str()),
        ));
    } else {
        e.push(web::grid("deck", 2, 16, cards));
    }
    e.push(web::spacer("deck-gap", 18));
    e.push(web::styled(
        "deck-edit-title",
        "Add or replace a slide",
        web::style().size(15).medium().color(p.ink.as_str()),
    ));
    e.push(web::form(
        "slide",
        &format!("/documents/{}/slides", d.id),
        &[
            ("index", "Slide number to replace, blank to add", ""),
            ("title", "Title", ""),
            ("body", "Body", ""),
        ],
    ));
    e
}
fn prose(d: &Document, actor: &str, p: &Palette) -> Vec<PageElement> {
    let lines: Vec<_> = d
        .body
        .lines()
        .enumerate()
        .map(|(i, line)| {
            if line.trim().is_empty() {
                web::spacer(&format!("body-gap-{i}"), 10)
            } else {
                web::styled(
                    &format!("body-line-{i}"),
                    line,
                    web::style().size(14).color(p.ink.as_str()),
                )
            }
        })
        .collect();
    let mut e = vec![web::card(
        "doc-page",
        web::style()
            .background("#ffffff")
            .border(p.line.as_str())
            .radius(8)
            .padding(28),
        lines,
    )];
    let outbound = web::links("body", &d.body);
    if !outbound.is_empty() {
        e.push(web::spacer("links-gap", 16));
        e.push(web::styled(
            "links-title",
            "Links in this document",
            web::style().size(13).medium().color(p.muted.as_str()),
        ));
        e.extend(outbound);
    }
    if d.writable(actor) {
        e.push(web::spacer("edit-gap", 18));
        e.push(web::styled(
            "edit-title",
            "Edit",
            web::style().size(15).medium().color(p.ink.as_str()),
        ));
        e.push(web::form(
            "edit",
            &format!("/documents/{}", d.id),
            &[
                ("revision", "Revision", &d.revision.to_string()),
                ("body", "Content", &d.body),
            ],
        ));
    }
    e
}
fn document(d: &Document, actor: &str, p: &Palette) -> Vec<PageElement> {
    let id = &d.id;
    let starred = d.starred.contains(actor);
    let mut e = chrome(product(Some(d.doc_type)), d.doc_type, p);
    e.push(web::styled_row(
        "head",
        14,
        "center",
        web::style(),
        vec![
            web::thumbnail(
                "head-mark",
                d.doc_type.label(),
                web::style()
                    .background(tint(d.doc_type))
                    .color("#ffffff")
                    .width(42)
                    .height(42)
                    .radius(6)
                    .size(11),
            ),
            web::styled(
                "head-title",
                d.title.as_str(),
                web::style()
                    .size(24)
                    .bold()
                    .color(p.ink.as_str())
                    .flex(6)
                    .one_line(),
            ),
            web::card_action(
                "star",
                web::style()
                    .background(if starred { "#f9ab00" } else { "#ffffff" })
                    .border(p.line.as_str())
                    .radius(18)
                    .padding(9)
                    .width(120),
                post(format!("/documents/{id}/star")),
                vec![web::styled(
                    "star-label",
                    if starred { "Starred" } else { "Star" },
                    web::style()
                        .size(13)
                        .medium()
                        .align("center")
                        .color("#202124"),
                )],
            ),
        ],
    ));
    e.push(web::styled(
        "head-meta",
        format!(
            "{} · owner {} · revision {}",
            d.doc_type.label(),
            d.owner,
            d.revision
        ),
        web::style().size(12).color(p.muted.as_str()),
    ));
    e.push(web::spacer("head-gap", 16));
    e.extend(match d.doc_type {
        DocType::Doc => prose(d, actor, p),
        DocType::Sheet => sheet(d, p),
        DocType::Slides => deck(d, p),
    });
    e.push(web::spacer("notes-gap", 22));
    e.push(web::divider("notes-rule"));
    e.push(web::styled(
        "notes-title",
        format!("Comments ({})", d.comments.len()),
        web::style().size(15).medium().color(p.ink.as_str()),
    ));
    for (i, c) in d.comments.iter().enumerate() {
        e.push(web::card(
            &format!("note-{i}"),
            web::style()
                .background(p.surface.as_str())
                .radius(8)
                .padding(12),
            vec![
                web::styled(
                    &format!("note-author-{i}"),
                    format!("{} · tick {}", c.author, c.time),
                    web::style().size(12).medium().color(p.accent.as_str()),
                ),
                web::styled(
                    &format!("note-text-{i}"),
                    c.text.as_str(),
                    web::style().size(13).color(p.ink.as_str()),
                ),
            ],
        ));
    }
    e.push(web::form(
        "comment",
        &format!("/documents/{id}/comments"),
        &[("text", "Comment", "")],
    ));
    e.push(web::spacer("history-gap", 22));
    e.push(web::styled(
        "history-title",
        "Revision history",
        web::style().size(15).medium().color(p.ink.as_str()),
    ));
    for r in d.history.iter().rev().take(10) {
        e.push(web::styled(
            &format!("history-{}", r.revision),
            format!("Revision {} · {} · tick {}", r.revision, r.author, r.time),
            web::style().size(12).color(p.muted.as_str()),
        ));
    }
    e
}
pub(crate) fn view(s: &DocsState, actor: &str, screen: Screen) -> Result<HttpResponse> {
    let p = palette(&s.theme);
    let (title, elements) = match screen {
        Screen::Home(kind) => (product(kind).to_owned(), home(s, actor, kind, &p)),
        Screen::Starred => ("Starred · Google Docs".to_owned(), starred(s, actor, &p)),
        Screen::Doc(id) => match s.read(actor, id) {
            Ok(d) => (
                format!("{} · {}", d.title, product(Some(d.doc_type))),
                document(d, actor, &p),
            ),
            Err(e) => return web::error(403, e),
        },
    };
    web::themed_page(&title, theme_of(s, &p), elements)
}
