//! The docs.google.com skin, served as HTML: the file gallery, a document as a white page on
//! grey under the toolbar band, a spreadsheet whose formula bar is the cell form, and a deck
//! with its filmstrip. The stylesheet is `gdocs.css` next to this file; the seeded palette
//! goes on `<html>` as custom properties.
//!
//! Nothing here is decoration pretending to be a control: every link, button and form carries
//! a route this crate serves, and the menu words and toolbar glyphs are inert spans. Element
//! ids are the agent API and are the ones the `Page` version used: `nav-all`, `nav-doc`,
//! `nav-sheet`, `nav-slides`, `nav-starred`, `file-<id>` (the whole card is one link), the
//! `create` form (`create-title`, `create-type`, `create-body`, `create-readers`,
//! `create-writers`, `create-submit`), `star`, `head-title`, `doc-page`, `body-line-<n>`,
//! `body-link-<n>`, the `edit` form (`edit-revision`, `edit-body`, `edit-submit`), the `cell`
//! form (`cell-cell`, `cell-value`, `cell-submit`), `sheet-<A1>`, the `slide` form
//! (`slide-index`, `slide-title`, `slide-body`, `slide-submit`), `slide-<n>`, `note-<n>`, the
//! `comment` form (`comment-text`, `comment-submit`) and `history-<revision>`. The three
//! editors (`edit`, `cell`, `slide`) are drawn only for someone who may write the file; a
//! reader gets `doc-readonly`, `sheet-readonly` or `deck-readonly` instead of a form that
//! could only ever answer 403.
use super::blocks::{self, Kind};
use super::{parse_cell, DocType, DocsState, Document, Screen, SHEET_COLUMNS};
use cw_protocol::{HttpResponse, Result};
use cw_service_common as web;
use cw_service_common::html::{button, div, el, form, label, link, span, text_input, Document as Page, Html};

const CSS: &str = include_str!("gdocs.css");
/// Sheets taller than this are still stored; drawing every row would make the page unreadable.
const MAX_ROWS: u32 = 60;

fn product(kind: Option<DocType>) -> &'static str {
    match kind {
        Some(DocType::Sheet) => "Google Sheets",
        Some(DocType::Slides) => "Google Slides",
        _ => "Google Docs",
    }
}
fn kind_class(kind: DocType) -> &'static str {
    match kind {
        DocType::Doc => "kind-doc",
        DocType::Sheet => "kind-sheet",
        DocType::Slides => "kind-slides",
    }
}
/// The product's file icon: a tinted sheet of paper with its folded corner, drawn by the sheet.
fn icon(kind: DocType) -> Html {
    span(&format!("icon {}", kind_class(kind))).attr("aria-hidden", "true").child(el("i"))
}
fn avatar(name: &str) -> Html {
    let tint = name.bytes().map(usize::from).sum::<usize>() % 6;
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    span(&format!("avatar av{tint}")).attr("aria-hidden", "true").text(initial)
}
fn nav(current: Option<&str>) -> Html {
    el("nav").class("switch").attr("aria-label", "Files").each(
        [
            ("nav-all", "All files", "/"),
            ("nav-doc", "Docs", "/?type=doc"),
            ("nav-sheet", "Sheets", "/?type=sheet"),
            ("nav-slides", "Slides", "/?type=slides"),
            ("nav-starred", "Starred", "/starred"),
        ],
        |(id, text, url)| link(id, url, text).class(if current == Some(id) { "on" } else { "" }),
    )
}
/// The gallery pages' header: the product mark, the switcher and who is signed in.
fn chrome(brand: &str, kind: DocType, current: Option<&str>, actor: &str) -> Html {
    el("header")
        .id("chrome")
        .class("bar")
        .child(span("menu").attr("aria-hidden", "true").each(0..3, |_| el("i")))
        .child(el("a").class("brand").attr("href", "/").attr("aria-label", brand).child(icon(kind)).child(span("name").id("chrome-brand").text(brand)))
        .child(nav(current))
        .child(div("who").child(span("actor").text(actor)).child(avatar(actor)))
}
fn field(form_id: &str, name: &str, text: &str, value: &str) -> Html {
    let id = format!("{form_id}-{name}");
    div("field").child(label(&id, text)).child(text_input(&id, name, value).attr("autocomplete", "off"))
}
fn area(form_id: &str, name: &str, text: &str, value: &str, rows: u32) -> Html {
    let id = format!("{form_id}-{name}");
    div("field wide")
        .child(label(&id, text))
        .child(el("textarea").id(id.as_str()).attr("name", name).attr("rows", rows.to_string()).text(value))
}
/// What a file looks like from a distance: the top of the page, the corner of the grid, or
/// the title slide.
fn cover(d: &Document) -> Html {
    let id = format!("file-cover-{}", d.id);
    let sheet = div(&format!("thumb {}", kind_class(d.doc_type))).id(id);
    match d.doc_type {
        DocType::Doc => sheet.child(blocks::thumbnail(&d.body, 9)),
        DocType::Sheet => sheet.child(el("div").class("mini-grid").each(1..=6u32, |r| {
            div("r").each(0..4u8, |c| {
                let name = format!("{}{r}", char::from(b'A' + c));
                span(if r == 1 { "c h" } else { "c" }).text(d.cells.get(&name).cloned().unwrap_or_default())
            })
        })),
        DocType::Slides => {
            let first = d.slides.first();
            sheet.child(
                div("mini-slide")
                    .child(el("p").class("h").text(first.map_or(d.title.as_str(), |s| s.title.as_str())))
                    .child(el("p").text(first.map_or("", |s| s.body.as_str()))),
            )
        }
    }
}
fn file_card(d: &Document, actor: &str) -> Html {
    let id = &d.id;
    el("a")
        .id(format!("file-{id}"))
        .class("file")
        .attr("href", format!("/documents/{id}"))
        .child(cover(d))
        .child(
            div("caption")
                .child(div("name").text(d.title.as_str()))
                .child(
                    div("meta")
                        .id(format!("file-meta-{id}"))
                        .child(icon(d.doc_type))
                        .child(span("kind").id(format!("file-kind-{id}")).text(d.doc_type.label()))
                        .child(span("owner").id(format!("file-owner-{id}")).text(format!("{} · revision {}", d.owner, d.revision)))
                        .when(d.starred.contains(actor), |m| {
                            m.child(span("starred").id(format!("file-star-{id}")).attr("aria-label", "Starred").text("★"))
                        }),
                ),
        )
}
fn gallery(id: &str, docs: &[&Document], actor: &str) -> Html {
    div("files").id(id).each(docs, |d| file_card(d, actor))
}
fn home(s: &DocsState, actor: &str, kind: Option<DocType>) -> Vec<Html> {
    let docs = s.visible(actor, kind);
    let current = match kind {
        None => "nav-all",
        Some(DocType::Doc) => "nav-doc",
        Some(DocType::Sheet) => "nav-sheet",
        Some(DocType::Slides) => "nav-slides",
    };
    let pick = el("select").id("create-type").attr("name", "type").each(
        [(DocType::Doc, "Document"), (DocType::Sheet, "Spreadsheet"), (DocType::Slides, "Presentation")],
        |(k, text)| {
            el("option")
                .attr("value", k.as_str())
                .when(k == kind.unwrap_or_default(), |o| o.flag("selected"))
                .text(text)
        },
    );
    let start = el("section").class("start").child(
        div("inner")
            .child(el("h2").id("create-heading").text("Start a new file"))
            .child(
                div("start-row")
                    // The template card is drawn like the file tiles above it, so it acts
                    // like one: it opens the form beside it, which is what starting a
                    // blank document means here.
                    .child(
                        el("a")
                            .id("blank")
                            .class("blank")
                            .attr("href", "#create-title")
                            .attr("aria-label", "Blank document")
                            .child(span("plus").child(el("i")).child(el("b"))),
                    )
                    .child(
                        form("create", "/documents", "post")
                            .child(field("create", "title", "Title", ""))
                            .child(div("field").child(label("create-type", "Type")).child(pick))
                            .child(field("create", "readers", "Readers", ""))
                            .child(field("create", "writers", "Writers", ""))
                            .child(area("create", "body", "Content", "", 3))
                            .child(div("actions").child(button("create-submit", "Create").class("primary"))),
                    ),
            ),
    );
    let recent = el("section").class("recent").child(
        div("inner")
            .child(
                div("recent-head")
                    .child(el("h1").id("home-title").text(match kind {
                        Some(k) => k.plural(),
                        None => "All files",
                    }))
                    .child(span("count").id("home-count").text(format!(
                        "{} file{} shared with you",
                        docs.len(),
                        if docs.len() == 1 { "" } else { "s" }
                    ))),
            )
            .child(if docs.is_empty() {
                el("p").id("home-empty").class("none").text("Nothing here yet.")
            } else {
                gallery("files", &docs, actor)
            }),
    );
    vec![chrome(product(kind), kind.unwrap_or_default(), Some(current), actor), el("main").child(start).child(recent)]
}
fn starred(s: &DocsState, actor: &str) -> Vec<Html> {
    let docs = s.starred(actor);
    let body = el("section").class("recent").child(
        div("inner")
            .child(
                div("recent-head")
                    .child(el("h1").id("starred-title").text("Starred"))
                    .child(span("count").id("starred-note").text("A star is yours alone; other people keep their own.")),
            )
            .child(if docs.is_empty() {
                el("p").id("starred-empty").class("none").text("You have not starred anything yet.")
            } else {
                gallery("starred-files", &docs, actor)
            }),
    );
    vec![chrome("Google Docs", DocType::Doc, Some("nav-starred"), actor), el("main").child(body)]
}
/// The toolbar band. Every glyph is inert: the editor is the form under the page.
fn toolbar(kind: DocType) -> Html {
    let groups: &[&[&str]] = match kind {
        DocType::Doc => &[&["↶", "↷", "⎙"], &["100%"], &["Normal text"], &["Arial"], &["−", "11", "+"], &["B", "I", "U", "A"], &["≡", "☰", "⋮"]],
        DocType::Sheet => &[&["↶", "↷", "⎙"], &["100%"], &["$", "%", ".0", "123"], &["Arial"], &["−", "10", "+"], &["B", "I", "S", "A"], &["▦", "≡", "Σ"]],
        DocType::Slides => &[&["+", "↶", "↷", "⎙"], &["Fit"], &["▭", "◯", "╱"], &["Background"], &["Layout"], &["Theme"], &["Transition"]],
    };
    div("toolbar").attr("aria-hidden", "true").each(groups.iter(), |group| {
        span("group").each(group.iter(), |glyph| span(if glyph.chars().count() > 2 { "tool wide" } else { "tool" }).text(*glyph))
    })
}
/// Why a reader sees no editor. A form that could only ever answer 403 is a control in name
/// alone, so the page says what is missing instead of drawing one.
fn readonly(id: &str, what: &str) -> Html {
    el("p").id(id).class("readonly").text(format!("You can read this {what}. Only its owner and the people it names as writers can change it."))
}
fn prose(d: &Document, actor: &str) -> Html {
    let parsed = blocks::parse(&d.body);
    let page = el("article").id("doc-page").class("paper").each(parsed.iter(), |b| {
        let id = format!("body-line-{}", b.line);
        let depth = if b.depth > 0 { " deep" } else { "" };
        match &b.kind {
            Kind::Gap => el("p").class("gap"),
            Kind::Heading => el(if b.line == 0 { "h1" } else { "h2" }).id(id).children(blocks::inline(&b.words)),
            Kind::Para => el("p").id(id).children(blocks::inline(&b.words)),
            Kind::Bullet => div(&format!("item{depth}")).id(id).child(span("mark").text("●")).child(span("txt").children(blocks::inline(&b.words))),
            Kind::Todo(done) => div(&format!("item todo{depth}"))
                .id(id)
                .child(span(if *done { "box done" } else { "box" }).text(if *done { "✓" } else { "" }))
                .child(span("txt").children(blocks::inline(&b.words))),
            Kind::Numbered(n) => div(&format!("item{depth}")).id(id).child(span("mark num").text(format!("{n}."))).child(span("txt").children(blocks::inline(&b.words))),
        }
    });
    let mut column = div("column").child(page);
    if d.writable(actor) {
        column = column.child(
            el("section")
                .class("panel editor")
                .child(el("h2").id("edit-title").text("Edit"))
                .child(
                    form("edit", format!("/documents/{}", d.id), "post")
                        .child(field("edit", "revision", "Revision", &d.revision.to_string()))
                        .child(area("edit", "body", "Content", &d.body, 12))
                        .child(div("actions").child(button("edit-submit", "Save").class("primary"))),
                ),
        );
    } else {
        column = column.child(readonly("doc-readonly", "document"));
    }
    column
}
fn sheet(d: &Document, actor: &str) -> Html {
    let (mut columns, mut rows) = (4u8, 6u32);
    for cell in d.cells.keys() {
        if let Some((c, r)) = parse_cell(cell) {
            columns = columns.max(c + 1);
            rows = rows.max(r);
        }
    }
    let columns = columns.clamp(8, SHEET_COLUMNS);
    let rows = rows.clamp(18, MAX_ROWS);
    let letter = |c: u8| char::from(b'A' + c);
    let head = el("tr")
        .child(el("th").id("sheet-corner").class("corner"))
        .each(0..columns, |c| el("th").id(format!("sheet-col-{}", letter(c))).attr("scope", "col").text(letter(c).to_string()));
    let body = el("tbody").each(1..=rows, |r| {
        el("tr")
            .child(el("th").id(format!("sheet-row-{r}")).attr("scope", "row").text(r.to_string()))
            .each(0..columns, |c| {
                let name = format!("{}{r}", letter(c));
                let value = d.cells.get(&name).cloned().unwrap_or_default();
                let numeric = value.chars().next().is_some_and(|ch| ch.is_ascii_digit() || ch == '-' || ch == '$');
                el("td")
                    .id(format!("sheet-{name}"))
                    .class(if r == 1 { "h" } else if numeric { "n" } else { "" })
                    .text(value)
            })
    });
    // The formula bar is the only way to change a cell, so a reader is not shown one: a bar
    // that could only ever answer 403 is a control in name alone.
    div("column sheet-column")
        .when(d.writable(actor), |c| {
            c.child(el("h2").id("sheet-edit-title").class("sr").text("Edit a cell")).child(
                form("cell", format!("/documents/{}/cells", d.id), "post")
                    .class("formula")
                    .child(text_input("cell-cell", "cell", "").attr("aria-label", "Cell, for example B3").attr("placeholder", "A1").attr("autocomplete", "off"))
                    .child(span("fx").attr("aria-hidden", "true").text("fx"))
                    .child(text_input("cell-value", "value", "").attr("aria-label", "Value").attr("placeholder", "Value").attr("autocomplete", "off"))
                    .child(button("cell-submit", "Set cell")),
            )
        })
        .when(!d.writable(actor), |c| c.child(readonly("sheet-readonly", "sheet")))
        .child(div("grid-wrap").child(el("table").id("sheet").child(el("thead").child(head)).child(body)))
        .child(div("sheet-tabs").attr("aria-hidden", "true").child(span("add").text("+")).child(span("all").text("☰")).child(span("tab on").text("Sheet1")))
}
fn deck(d: &Document, actor: &str) -> Html {
    let strip = div("filmstrip").attr("aria-hidden", "true").each(d.slides.iter().enumerate(), |(i, slide)| {
        div(if i == 0 { "frame on" } else { "frame" })
            .child(span("no").text((i + 1).to_string()))
            .child(div("mini-slide").child(el("p").class("h").text(slide.title.as_str())).child(el("p").text(slide.body.as_str())))
    });
    let mut column = div("column deck-column");
    if d.slides.is_empty() {
        column = column.child(el("p").id("deck-empty").class("none").text("This deck has no slides yet."));
    } else {
        column = column.child(div("slides").id("deck").each(d.slides.iter().enumerate(), |(i, slide)| {
            el("section")
                .id(format!("slide-{i}"))
                .class("slide")
                .child(span("badge").id(format!("slide-no-{i}")).text(format!("Slide {}", i + 1)))
                .child(el("h2").id(format!("slide-title-{i}")).text(slide.title.as_str()))
                .child(el("p").id(format!("slide-body-{i}")).text(slide.body.as_str()))
        }));
    }
    // As with the prose editor and the formula bar: only a writer is offered the slide form.
    if d.writable(actor) {
        column = column.child(
            el("section")
                .class("panel editor")
                .child(el("h2").id("deck-edit-title").text("Add or replace a slide"))
                .child(
                    form("slide", format!("/documents/{}/slides", d.id), "post")
                        .child(field("slide", "index", "Slide number to replace, blank to add", ""))
                        .child(field("slide", "title", "Title", ""))
                        .child(area("slide", "body", "Body", "", 4))
                        .child(div("actions").child(button("slide-submit", "Save slide").class("primary"))),
                ),
        );
    } else {
        column = column.child(readonly("deck-readonly", "deck"));
    }
    div("deck-wrap").child(strip).child(column)
}
fn document(d: &Document, actor: &str) -> Vec<Html> {
    let id = &d.id;
    let is_starred = d.starred.contains(actor);
    let titlebar = el("header")
        .id("head")
        .class("docbar")
        .child(el("a").id("doc-home").class("home").attr("href", "/").attr("aria-label", format!("{} home", product(Some(d.doc_type)))).child(icon(d.doc_type)))
        .child(
            div("titles")
                .child(
                    div("title-row")
                        .child(el("h1").id("head-title").text(d.title.as_str()))
                        .child(
                            form("star-form", format!("/documents/{id}/star"), "post").child(
                                button("star", if is_starred { "Starred" } else { "Star" })
                                    .class(if is_starred { "star on" } else { "star" })
                                    .attr("aria-pressed", if is_starred { "true" } else { "false" }),
                            ),
                        )
                        .child(span("meta").id("head-meta").text(format!("{} · owner {} · revision {}", d.doc_type.label(), d.owner, d.revision))),
                )
                .child(div("menus").attr("aria-hidden", "true").each(
                    ["File", "Edit", "View", "Insert", "Format", "Tools", "Extensions", "Help"],
                    |m| span("").text(m),
                )),
        )
        .child(nav(None))
        .child(div("who").child(avatar(actor)));
    let notes = el("aside")
        .class("notes")
        .child(el("h2").id("notes-title").text(format!("Comments ({})", d.comments.len())))
        .each(d.comments.iter().enumerate(), |(i, c)| {
            div("note")
                .id(format!("note-{i}"))
                .child(div("note-head").child(avatar(&c.author)).child(span("by").id(format!("note-author-{i}")).text(format!("{} · tick {}", c.author, c.time))))
                .child(el("p").id(format!("note-text-{i}")).text(c.text.as_str()))
        })
        .child(
            form("comment", format!("/documents/{id}/comments"), "post")
                .class("note new")
                .child(el("textarea").id("comment-text").attr("name", "text").attr("rows", "3").attr("aria-label", "Comment").attr("placeholder", "Add a comment"))
                .child(div("actions").child(button("comment-submit", "Comment").class("primary"))),
        );
    let history = el("aside")
        .class("history")
        .child(el("h2").id("history-title").text("Revision history"))
        .each(d.history.iter().rev().take(10), |r| {
            div("rev")
                .id(format!("history-{}", r.revision))
                .text(format!("Revision {} · {} · tick {}", r.revision, r.author, r.time))
        });
    let center = match d.doc_type {
        DocType::Doc => prose(d, actor),
        DocType::Sheet => sheet(d, actor),
        DocType::Slides => deck(d, actor),
    };
    vec![
        titlebar,
        toolbar(d.doc_type),
        el("main").class("workspace").child(history).child(center).child(notes),
    ]
}
pub(crate) fn view(s: &DocsState, actor: &str, screen: Screen) -> Result<HttpResponse> {
    let (title, class, body) = match screen {
        Screen::Home(kind) => (product(kind).to_owned(), format!("page-home {}", kind_class(kind.unwrap_or_default())), home(s, actor, kind)),
        Screen::Starred => ("Starred · Google Docs".to_owned(), "page-home kind-doc".to_owned(), starred(s, actor)),
        Screen::Doc(id) => match s.read(actor, id) {
            Ok(d) => (
                format!("{} · {}", d.title, product(Some(d.doc_type))),
                format!("page-file {}", kind_class(d.doc_type)),
                document(d, actor),
            ),
            Err(e) => return web::error(403, e),
        },
    };
    let or = |value: &Option<String>, fallback: &str| value.clone().unwrap_or_else(|| fallback.to_owned());
    let page = Page::new(title)
        .lang("en")
        .stylesheet(CSS)
        .root_style(&format!(
            "--accent: {}; --ink: {}; --muted: {}; --surface: {}; --paper: {}",
            or(&s.theme.accent, "#1a73e8"),
            or(&s.theme.ink, "#202124"),
            or(&s.theme.muted, "#5f6368"),
            or(&s.theme.surface, "#f8fafd"),
            or(&s.theme.background, "#ffffff"),
        ))
        .body_class(&format!("skin-gdocs {class}"))
        .body(body);
    web::html::page(&page)
}
