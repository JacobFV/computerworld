//! The notion.so skin, served as HTML: the grey sidebar of pages, the breadcrumb bar, and a
//! wide sans-serif page of blocks. The stylesheet is `notion.css` next to this file.
//!
//! notion.so wore `plain` before it had a look of its own, so this skin keeps the ids the
//! plain page exposed: `title`, one link per document whose id is the document id (now the
//! sidebar's page list), `document-title`, `document-body`, `body-link-<n>`, `cell-<A1>`,
//! `slide-<n>`, the `edit` form (`edit-revision`, `edit-body`, `edit-submit`), `comment-<n>`,
//! the `comment` form (`comment-text`, `comment-submit`) and the `create` form
//! (`create-title`, `create-body`, `create-readers`, `create-writers`, `create-submit`).
//! Everything the skin adds is prefixed (`side-`, `crumb-`, `recent-`, `star`), so a document
//! id can never collide with chrome. The `edit` form is drawn only for someone who may write
//! the page; a reader gets `document-readonly` instead of a form that could only answer 403.
use super::blocks::{self, Kind};
use super::{DocType, DocsState, Document, Screen};
use cw_protocol::{HttpResponse, Result};
use cw_service_common as web;
use cw_service_common::html::{button, div, el, form, label, link, span, text_input, Document as Page, Html};

const CSS: &str = include_str!("notion.css");

fn capital(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
fn glyph(kind: DocType) -> Html {
    span(match kind {
        DocType::Doc => "glyph",
        DocType::Sheet => "glyph table",
        DocType::Slides => "glyph deck",
    })
    .attr("aria-hidden", "true")
}
fn sidebar(s: &DocsState, actor: &str, kind: Option<DocType>, current: Option<&str>, starred: bool) -> Html {
    let name = capital(actor);
    el("aside")
        .class("side")
        .child(
            // The product's workspace switcher opens a menu; this world has one workspace and
            // no page script, so the row is the workspace's name, not a chevron that swallows
            // a click.
            div("workspace")
                .child(span("tile").attr("aria-hidden", "true").text(name.chars().next().map(String::from).unwrap_or_default()))
                .child(span("ws-name").text(format!("{name}'s Notion"))),
        )
        .child(
            el("nav")
                .class("side-nav")
                .attr("aria-label", "Workspace")
                .child(link("side-home", "/", "Home").class(if current.is_none() && !starred { "on" } else { "" }))
                .child(link("side-starred", "/starred", "Favorites").class(if starred { "on" } else { "" })),
        )
        .child(div("side-title").text("Workspace"))
        .child(el("nav").class("pages").attr("aria-label", "Pages").each(s.visible(actor, kind), |d| {
            el("a")
                .id(d.id.as_str())
                .class(if current == Some(d.id.as_str()) { "page-link on" } else { "page-link" })
                .attr("href", format!("/documents/{}", d.id))
                .child(glyph(d.doc_type))
                .child(span("t").text(d.title.as_str()))
        }))
}
/// The breadcrumb bar. `title` is the id the plain page put on its heading; on a page it is
/// the crumb above that page and so a link back to the workspace, and on the workspace itself
/// it names where you already are and stays text.
fn topbar(root: &str, d: Option<&Document>, actor: &str) -> Html {
    let mut bar = el("header").class("topbar").child(match d {
        Some(_) => link("title", "/", root).class("crumb root"),
        None => span("crumb root").id("title").text(root),
    });
    if let Some(d) = d {
        let on = d.starred.contains(actor);
        bar = bar
            .child(span("sep").attr("aria-hidden", "true").text("/"))
            .child(span("crumb").id("crumb-page").child(glyph(d.doc_type)).text(d.title.as_str()))
            .child(span("edited").id("crumb-meta").text(format!("Revision {} · owner {}", d.revision, d.owner)))
            .child(
                form("star-form", format!("/documents/{}/star", d.id), "post").child(
                    button("star", if on { "★" } else { "☆" })
                        .class(if on { "star on" } else { "star" })
                        .attr("aria-label", if on { "Remove from Favorites" } else { "Add to Favorites" })
                        .attr("aria-pressed", if on { "true" } else { "false" }),
                ),
            );
    }
    bar
}
fn block_field(form_id: &str, name: &str, text: &str, value: &str) -> Html {
    let id = format!("{form_id}-{name}");
    div("prop").child(label(&id, text)).child(text_input(&id, name, value).attr("autocomplete", "off").attr("placeholder", "Empty"))
}
fn block_area(form_id: &str, name: &str, text: &str, value: &str, rows: u32) -> Html {
    let id = format!("{form_id}-{name}");
    div("prop tall")
        .child(label(&id, text))
        .child(el("textarea").id(id.as_str()).attr("name", name).attr("rows", rows.to_string()).attr("placeholder", "Type something…").text(value))
}
fn body(d: &Document) -> Html {
    div("blocks").id("document-body").each(blocks::parse(&d.body).iter(), |b| {
        let depth = if b.depth > 0 { " deep" } else { "" };
        match &b.kind {
            Kind::Gap => div("block gap"),
            Kind::Heading => el(if b.line == 0 { "h2" } else { "h3" }).class("block").children(blocks::inline(&b.words)),
            Kind::Para => el("p").class("block").children(blocks::inline(&b.words)),
            Kind::Bullet => div(&format!("block item{depth}")).child(span("mark").text("•")).child(span("txt").children(blocks::inline(&b.words))),
            Kind::Todo(done) => div(&format!("block item todo{depth}"))
                .child(span(if *done { "box done" } else { "box" }).text(if *done { "✓" } else { "" }))
                .child(span(if *done { "txt struck" } else { "txt" }).children(blocks::inline(&b.words))),
            Kind::Numbered(n) => div(&format!("block item{depth}")).child(span("mark num").text(format!("{n}."))).child(span("txt").children(blocks::inline(&b.words))),
        }
    })
}
fn document(d: &Document, actor: &str) -> Html {
    let id = &d.id;
    let mut page = el("article")
        .class("page")
        .child(div("page-icon").attr("aria-hidden", "true").child(glyph(d.doc_type)))
        .child(el("h1").id("document-title").text(d.title.as_str()))
        .child(
            div("props")
                .child(div("prop-row").child(span("k").text("Owner")).child(span("v").text(capital(&d.owner))))
                .child(div("prop-row").child(span("k").text("Revision")).child(span("v").text(d.revision.to_string())))
                .child(div("prop-row").child(span("k").text("Comments")).child(span("v").text(d.comments.len().to_string()))),
        )
        .child(body(d));
    if !d.cells.is_empty() {
        page = page.child(
            el("table").class("database").child(el("thead").child(el("tr").child(el("th").text("Cell")).child(el("th").text("Value")))).child(
                el("tbody").each(&d.cells, |(cell, value)| {
                    el("tr").id(format!("cell-{cell}")).child(el("td").class("k").text(cell.as_str())).child(el("td").text(value.as_str()))
                }),
            ),
        );
    }
    page = page.each(d.slides.iter().enumerate(), |(i, slide)| {
        div("callout")
            .id(format!("slide-{i}"))
            .child(span("n").text((i + 1).to_string()))
            .child(div("c").child(el("strong").text(slide.title.as_str())).child(el("p").text(slide.body.as_str())))
    });
    if d.writable(actor) {
        page = page.child(
            el("section").class("editor").child(div("section-title").text("Edit this page")).child(
                form("edit", format!("/documents/{id}"), "post")
                    .child(block_field("edit", "revision", "Revision", &d.revision.to_string()))
                    .child(block_area("edit", "body", "Content", &d.body, 10))
                    .child(div("actions").child(button("edit-submit", "Save changes").class("primary"))),
            ),
        );
    } else {
        // No editor rather than one that could only ever answer 403; the page says why.
        page = page.child(el("p").id("document-readonly").class("block muted").text("You can read this page. Only its owner and the people it names as writers can change it."));
    }
    page.child(
        el("section")
            .class("discussion")
            .child(div("section-title").text("Comments"))
            .each(d.comments.iter().enumerate(), |(i, c)| {
                div("comment")
                    .id(format!("comment-{i}"))
                    .child(span("tile small").attr("aria-hidden", "true").text(c.author.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default()))
                    .child(div("c").child(span("by").text(capital(&c.author))).child(el("p").text(c.text.as_str())))
            })
            .child(
                form("comment", format!("/documents/{id}/comments"), "post")
                    .class("add-comment")
                    .child(text_input("comment-text", "text", "").attr("aria-label", "Comment").attr("placeholder", "Add a comment…").attr("autocomplete", "off"))
                    .child(button("comment-submit", "Send").class("primary")),
            ),
    )
}
fn cards(prefix: &str, docs: &[&Document]) -> Html {
    div("cards").each(docs, |d| {
        el("a")
            .id(format!("{prefix}-{}", d.id))
            .class("card")
            .attr("href", format!("/documents/{}", d.id))
            .child(div("card-top").child(glyph(d.doc_type)))
            .child(div("card-title").text(d.title.as_str()))
            .child(div("card-meta").text(format!("{} · rev {}", capital(&d.owner), d.revision)))
    })
}
fn home(s: &DocsState, actor: &str, kind: Option<DocType>) -> Html {
    let docs = s.visible(actor, kind);
    el("article")
        .class("page home")
        .child(el("h1").class("greeting").text(format!("Good morning, {}", capital(actor))))
        .child(div("section-title").text("Recently visited"))
        .child(if docs.is_empty() { el("p").class("block muted").text("No pages yet.") } else { cards("recent", &docs) })
        .child(
            el("section").class("editor").child(div("section-title").text("New page")).child(
                form("create", "/documents", "post")
                    .child(block_field("create", "title", "Title", ""))
                    .child(block_area("create", "body", "Content", "", 5))
                    .child(block_field("create", "readers", "Readers", ""))
                    .child(block_field("create", "writers", "Writers", ""))
                    .child(div("actions").child(button("create-submit", "Create page").class("primary"))),
            ),
        )
}
pub(crate) fn view(s: &DocsState, actor: &str, screen: Screen) -> Result<HttpResponse> {
    let (title, side, top, main) = match screen {
        Screen::Home(kind) => ("Notion".to_owned(), sidebar(s, actor, kind, None, false), topbar("Documents", None, actor), home(s, actor, kind)),
        Screen::Starred => {
            let docs = s.starred(actor);
            let main = el("article")
                .class("page home")
                .child(el("h1").class("greeting").text("Favorites"))
                .child(if docs.is_empty() {
                    el("p").class("block muted").text("Nothing in your Favorites yet.")
                } else {
                    cards("starred", &docs)
                });
            ("Favorites · Notion".to_owned(), sidebar(s, actor, None, None, true), topbar("Favorites", None, actor), main)
        }
        Screen::Doc(id) => match s.read(actor, id) {
            Ok(d) => (
                format!("{} · Notion", d.title),
                sidebar(s, actor, None, Some(id), false),
                topbar("Documents", Some(d), actor),
                document(d, actor),
            ),
            Err(e) => return web::error(403, e),
        },
    };
    let page = Page::new(title)
        .lang("en")
        .stylesheet(CSS)
        .body_class("skin-notion")
        .body([div("app").child(side).child(el("main").child(top).child(main))]);
    web::html::page(&page)
}
