//! The branded drives as HTML: `gdrive` lays the items out as Google Drive does (folder pills
//! and file cards in a rounded white panel), `dropbox` as Dropbox does (a file table beside a
//! white sidebar). One markup builder, two stylesheets (`gdrive.css`, `dropbox.css`); the seeded
//! palette rides on `<html>` as custom properties so each sheet stays a static file.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `chrome-brand`,
//! `nav-drive`, `nav-shared`, `nav-starred`, `nav-trash`, the `find` form (`find-q`,
//! `find-submit`), `crumbs`, `crumb-<id>`, `head-title`, `head-meta`, `star`, `items`,
//! `item-<id>` (with `item-name-`, `item-meta-`, `item-kind-`, `item-shared-`, `item-star-<id>`),
//! `empty`, `grant-title`, `grant-note`, `link-action`, `link-url`, the `grant`, `make`, `upload`
//! and `rename` forms (`<form>-<field>`, `<form>-submit`), `preview`, `preview-line-<i>`,
//! `target`, `content-link-<i>`, `details`, `detail-value-<key>`, `public`, `trash-action`.
//! Every control carries a route this crate serves; one that would be refused is not drawn.
use super::{DriveState, Node, NodeKind, Screen, TRASH};
use cw_protocol::{HttpResponse, Result};
use cw_service_common as web;
use cw_service_common::html::{
    button, div, el, empty, form, fragment, label, link, span, text_input, Document, Html,
};

const GDRIVE: &str = include_str!("gdrive.css");
const DROPBOX: &str = include_str!("dropbox.css");
/// Longer previews are still stored; drawing all of one would push the metadata off the page.
const PREVIEW_LINES: usize = 40;
/// Lines of a file a Drive card shows as its thumbnail.
const THUMB_LINES: usize = 7;
/// Cards in the "Suggested" strip of a root folder.
const SUGGESTED: usize = 4;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const US_PER_DAY: u64 = 86_400_000_000;

/// The calendar date of a logical tick: the world begins on 17 Sep 2026, as the calendar says.
fn date(tick: u64) -> String {
    let (mut year, mut month, mut day) = (2026u64, 8usize, 17 + tick / US_PER_DAY);
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let length = match month {
            1 if leap => 29,
            1 => 28,
            3 | 5 | 8 | 10 => 30,
            _ => 31,
        };
        if day <= length {
            return format!("{} {day}, {year}", MONTHS[month]);
        }
        day -= length;
        month += 1;
        if month == 12 {
            month = 0;
            year += 1;
        }
    }
}
/// What a node is, at a glance and before a word is read: the class of its CSS-drawn icon.
fn glyph(n: &Node) -> &'static str {
    let name = n.name.to_ascii_lowercase();
    let ends = |suffixes: &[&str]| suffixes.iter().any(|s| name.ends_with(s));
    match n.kind {
        NodeKind::Folder if n.shared_with.is_empty() => "folder",
        NodeKind::Folder => "folder shared",
        _ if n.mime.ends_with("apps.document") || ends(&[".doc", ".docx"]) => "doc",
        _ if n.mime.ends_with("apps.spreadsheet") || n.mime == "text/csv" || ends(&[".csv", ".xlsx"]) => "sheet",
        _ if n.mime.ends_with("apps.presentation") || ends(&[".ppt", ".pptx", ".key"]) => "slides",
        _ if n.mime == "application/pdf" || ends(&[".pdf"]) => "pdf",
        NodeKind::Shortcut => "shortcut",
        _ => "text",
    }
}
fn icon(id: Option<String>, n: &Node) -> Html {
    let mut i = span(&format!("ico {}", glyph(n)))
        .attr("role", "img")
        .attr("aria-label", n.kind.label())
        .child(el("i"));
    if let Some(id) = id {
        i = i.id(id);
    }
    i
}
/// Where a node opens. A shortcut leaves the site entirely, which is the point of a shortcut.
fn open(n: &Node) -> String {
    match n.kind {
        NodeKind::Folder => format!("/drive/folders/{}", n.id),
        NodeKind::Shortcut if !n.target_url.is_empty() => n.target_url.clone(),
        _ => format!("/file/{}", n.id),
    }
}
fn initials(name: &str) -> String {
    name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
}
/// A form field with its visible label; `<form>-<key>` is the id the `Page` form gave it.
fn field(form_id: &str, key: &str, text: &str, value: &str) -> Html {
    let id = format!("{form_id}-{key}");
    div("field").child(label(&id, text)).child(text_input(&id, key, value))
}
/// A button that posts with no fields: a one-button form, the id on the button.
fn action(id: &str, text: &str, url: String, class: &str) -> Html {
    form(&format!("{id}-form"), url, "post").class("one").child(button(id, text).class(class))
}

/// Everything one render needs; methods keep the argument lists short.
struct View<'a> {
    s: &'a DriveState,
    actor: &'a str,
    dropbox: bool,
    /// Which sidebar entry is current: `drive`, `shared`, `starred`, `trash` or none.
    at: &'a str,
    query: &'a str,
    /// Whether this screen carries the new-folder and upload forms the New button jumps to.
    can_new: bool,
}
impl View<'_> {
    fn document(&self, title: &str, screen: &str, main: Vec<Html>) -> Document {
        let t = &self.s.theme;
        let or = |v: &Option<String>, d: &str| v.clone().unwrap_or_else(|| d.to_owned());
        let (accent, surface, ink, muted) = if self.dropbox {
            ("#0061ff", "#f7f5f2", "#1e1919", "#637282")
        } else {
            ("#1a73e8", "#f8fafd", "#202124", "#5f6368")
        };
        Document::new(title)
            .lang("en")
            .stylesheet(if self.dropbox { DROPBOX } else { GDRIVE })
            .root_style(&format!(
                "--accent: {}; --paper: {}; --surface: {}; --ink: {}; --muted: {}",
                or(&t.accent, accent),
                or(&t.background, "#ffffff"),
                or(&t.surface, surface),
                or(&t.ink, ink),
                or(&t.muted, muted)
            ))
            .body_class(&format!(
                "skin-{} screen-{screen}",
                if self.dropbox { "dropbox" } else { "gdrive" }
            ))
            .body(if self.dropbox {
                vec![self.sidebar(), div("stage").child(self.top()).child(el("main").id("main").children(main))]
            } else {
                vec![
                    self.top(),
                    div("shell").child(self.sidebar()).child(el("main").id("main").children(main)),
                ]
            })
    }
    /// The wordmark: the tri-colour triangle for Drive, the box of diamonds for Dropbox.
    fn brand(&self) -> Html {
        let mark = span("mark").id("chrome-mark").attr("aria-hidden", "true");
        let mark = if self.dropbox { mark.each(0..5, |_| el("i")) } else { mark.each(0..3, |_| el("i")) };
        let name = match self.s.brand() {
            "Google Drive" => "Drive",
            other => other,
        };
        el("a")
            .class("brand")
            .attr("href", "/")
            .attr("aria-label", self.s.brand())
            .child(mark)
            .child(span("name").id("chrome-brand").text(name))
    }
    /// The search box. It posts, as the `Page` form did; `/search` answers both verbs.
    fn find(&self) -> Html {
        let hint = if self.dropbox { "Search" } else { "Search in Drive" };
        form("find", "/search", "post")
            .class("search")
            .attr("role", "search")
            .child(span("mag").attr("aria-hidden", "true"))
            .child(
                text_input("find-q", "q", self.query)
                    .attr("aria-label", "Search files")
                    .attr("placeholder", hint)
                    .attr("autocomplete", "off"),
            )
            .child(button("find-submit", "Search"))
    }
    fn account(&self) -> Html {
        div("account")
            .child(span("who").id("account-name").text(self.actor))
            .child(span("avatar").id("account").attr("aria-label", self.actor).text(initials(self.actor)))
    }
    fn top(&self) -> Html {
        let bar = el("header").id("chrome").class("top");
        if self.dropbox {
            bar.child(self.find()).child(self.account())
        } else {
            bar.child(self.brand()).child(self.find()).child(self.account())
        }
    }
    fn sidebar(&self) -> Html {
        let entry = |id: &str, key: &str, text: &str, url: &str| {
            el("a")
                .id(id)
                .class(if self.at == key { "entry on" } else { "entry" })
                .attr("href", url)
                .child(span(&format!("nav-ico {key}")).attr("aria-hidden", "true").child(el("i")))
                .child(span("nav-text").text(text))
        };
        let used: u64 = self.s.nodes.values().filter(|n| n.owner == self.actor).map(|n| n.size).sum();
        let used = match used {
            n if n < 1024 => format!("{n} bytes"),
            n => format!("{}.{} KB", n / 1024, (n % 1024) * 10 / 1024),
        };
        let nav = el("nav").id("nav").class("side");
        if self.dropbox {
            nav.child(self.brand())
                .child(entry("nav-drive", "drive", "All files", "/"))
                .child(entry("nav-shared", "shared", "Shared", "/shared-with-me"))
                .child(entry("nav-starred", "starred", "Starred", "/starred"))
                .child(entry("nav-trash", "trash", "Deleted files", "/trash"))
                .child(div("storage").id("storage").child(div("meter").child(el("i"))).child(span("").text(format!("{used} of 2 GB used"))))
        } else {
            nav.child(
                el("a").id("new").class("new").attr("href", if self.can_new { "#make-title" } else { "/" }).child(span("plus").attr("aria-hidden", "true")).child(span("").text("New")),
            )
            .child(entry("nav-drive", "drive", "My Drive", "/"))
            .child(entry("nav-shared", "shared", "Shared with me", "/shared-with-me"))
            .child(entry("nav-starred", "starred", "Starred", "/starred"))
            .child(entry("nav-trash", "trash", "Trash", "/trash"))
            .child(div("storage").id("storage").child(div("meter").child(el("i"))).child(span("").text(format!("{used} of 15 GB used"))))
        }
    }
    /// The folder path. Drive's path *is* its title, so there the last crumb is the `<h1>`;
    /// Dropbox prints a small path above a heading of its own.
    fn crumbs(&self, id: &str) -> Html {
        let trail = self.s.path_to(self.actor, id);
        let last = trail.len().saturating_sub(1);
        el("nav").id("crumbs").class("crumbs").attr("aria-label", "Folder path").each(trail.iter().enumerate(), |(i, node)| {
            let crumb = link(
                &format!("crumb-{}", node.id),
                match node.kind {
                    NodeKind::Folder => format!("/drive/folders/{}", node.id),
                    _ => format!("/file/{}", node.id),
                },
                node.name.as_str(),
            )
            .class(if i == last { "crumb here" } else { "crumb" });
            fragment([
                if i > 0 { span("sep").attr("aria-hidden", "true").text("›") } else { empty() },
                if i == last && !self.dropbox { el("h1").id("head-title").child(crumb) } else { crumb },
            ])
        })
    }
    fn star(&self, id: &str, on: bool) -> Html {
        form("star-form", format!("/nodes/{id}/star"), "post").class("one").child(
            el("button")
                .id("star")
                .attr("type", "submit")
                .class(if on { "pill star on" } else { "pill star" })
                .child(span("glyph").attr("aria-hidden", "true").text(if on { "★" } else { "☆" }))
                .child(span("").id("star-label").text(if on { "Starred" } else { "Star" })),
        )
    }
    fn head(&self, n: &Node) -> Html {
        let star = self.star(&n.id, self.s.is_starred(self.actor, &n.id));
        if self.dropbox {
            return fragment([
                self.crumbs(&n.id),
                div("head")
                    .id("head")
                    .child(icon(Some("head-mark".into()), n))
                    .child(el("h1").id("head-title").text(n.name.as_str()))
                    .child(star),
            ]);
        }
        div("head").id("head").child(self.crumbs(&n.id)).child(star)
    }
    /// `owner · size`, or the owner alone for what has no size of its own.
    fn meta(&self, n: &Node) -> String {
        match n.size {
            0 => n.owner.clone(),
            _ => format!("{} · {}", n.owner, n.size_text()),
        }
    }
    fn access(&self, n: &Node) -> String {
        match n.shared_with.len() {
            0 if n.owner == self.actor => "Only you".into(),
            0 => format!("Only {}", n.owner),
            count => format!("{} members", count + 1),
        }
    }
    /// A Drive tile: a folder pill, or a file card with a thumbnail of what is inside.
    fn tile(&self, n: &Node, prefix: &str) -> Html {
        let id = &n.id;
        let own = prefix == "item";
        let part = |part: &str| format!("{prefix}-{part}-{id}");
        let starred = self.s.is_starred(self.actor, id);
        let name = span("name").id(part("name")).text(n.name.as_str());
        let star = if starred { span("starred").id(part("star")).attr("aria-label", "Starred").text("★") } else { empty() };
        let meta = span("meta")
            .child(span("").id(part("meta")).text(self.meta(n)))
            .when(!n.shared_with.is_empty(), |m| {
                m.child(span("shared").id(part("shared")).text(format!("Shared · {}", n.shared_with.len())))
            });
        let card = el("a").id(if own { format!("item-{id}") } else { format!("{prefix}-{id}") }).attr("href", open(n));
        if n.kind == NodeKind::Folder {
            return card
                .class("tile folder-tile")
                .child(icon(Some(part("kind")), n))
                .child(span("words").child(name).child(meta))
                .child(star);
        }
        let mut thumb = div("thumb").id(part("cover"));
        let lines: Vec<&str> = n.content.lines().filter(|l| !l.trim().is_empty()).take(THUMB_LINES).collect();
        thumb = if lines.is_empty() {
            thumb.class("blank").child(icon(None, n))
        } else {
            thumb.child(div("sheet-of-paper").each(lines, |l| el("p").text(l)))
        };
        card.class("tile file-tile")
            .child(div("tile-head").child(icon(Some(part("kind")), n)).child(name).child(star))
            .child(thumb)
            .child(meta)
    }
    /// The Dropbox file table; the row's link carries `item-<id>`.
    fn table(&self, nodes: &[&Node]) -> Html {
        let head = el("thead").child(
            el("tr")
                .child(el("th").class("c-name").text("Name"))
                .child(el("th").class("c-access").text("Who can access"))
                .child(el("th").class("c-when").text("Modified"))
                // `item-meta-<id>` is the owner and, for a file, its size: the column says so
                // rather than promising a size a folder does not have.
                .child(el("th").class("c-size").text("Owner")),
        );
        let body = el("tbody").each(nodes, |n| {
            let id = &n.id;
            let starred = self.s.is_starred(self.actor, id);
            el("tr")
                .id(format!("item-row-{id}"))
                .child(
                    el("td").class("c-name").child(
                        div("cell")
                            .child(icon(Some(format!("item-kind-{id}")), n))
                            .child(
                                el("a").id(format!("item-{id}")).class("open").attr("href", open(n)).child(
                                    span("name").id(format!("item-name-{id}")).text(n.name.as_str()),
                                ),
                            )
                            .when(starred, |c| {
                                c.child(span("starred").id(format!("item-star-{id}")).attr("aria-label", "Starred").text("★"))
                            }),
                    ),
                )
                .child(el("td").class("c-access").child(span("").id(format!("item-shared-{id}")).text(self.access(n))))
                .child(el("td").class("c-when").text(date(n.tick)))
                .child(el("td").class("c-size").id(format!("item-meta-{id}")).text(self.meta(n)))
        });
        el("table").id("items").class("files").child(head).child(body)
    }
    fn items(&self, nodes: &[&Node]) -> Html {
        if self.dropbox {
            return self.table(nodes);
        }
        let (folders, files): (Vec<&Node>, Vec<&Node>) = nodes.iter().partition(|n| n.kind == NodeKind::Folder);
        div("items")
            .id("items")
            .when(!folders.is_empty(), |d| {
                d.child(el("h2").class("section").text("Folders"))
                    .child(div("tiles folders").each(folders, |n| self.tile(n, "item")))
            })
            .when(!files.is_empty(), |d| {
                d.child(el("h2").class("section").text("Files"))
                    .child(div("tiles files").each(files, |n| self.tile(n, "item")))
            })
    }
    /// Recent files from anywhere the actor can see, offered on the root the way both products
    /// open with suggestions. View-only: each card is the same link the item has in its folder.
    fn suggested(&self, folder: &str) -> Html {
        let shown: Vec<String> = self.s.children(self.actor, folder).iter().map(|n| n.id.clone()).collect();
        let mut found: Vec<&Node> = self
            .s
            .nodes
            .values()
            .filter(|n| {
                n.kind != NodeKind::Folder
                    && !shown.contains(&n.id)
                    && !self.s.trashed(&n.id)
                    && self.s.visible(self.actor, &n.id)
            })
            .collect();
        found.sort_by(|a, b| (b.tick, &a.name).cmp(&(a.tick, &b.name)));
        found.truncate(SUGGESTED);
        if found.is_empty() {
            return empty();
        }
        el("section")
            .id("suggested")
            .class("suggested")
            .child(el("h2").class("section").text(if self.dropbox { "Suggested from your activity" } else { "Suggested files" }))
            .child(div("tiles files").each(found, |n| self.tile(n, "suggest")))
    }
    /// The share panel. A link that has not been minted is offered as a button, never faked.
    fn sharing(&self, n: &Node) -> Html {
        let id = &n.id;
        let note = if n.shared_with.is_empty() {
            format!("Only {} can open this.", n.owner)
        } else {
            format!("Shared with {}.", n.shared_with.iter().cloned().collect::<Vec<_>>().join(", "))
        };
        let mut panel = el("section")
            .class("card sharing")
            .child(el("h2").id("grant-title").text("Sharing"))
            .child(el("p").id("grant-note").class("note").text(note));
        if n.link.is_empty() {
            if self.s.granted_to(self.actor, id) {
                panel = panel.child(action("link-action", "Create a share link", format!("/nodes/{id}/link"), "pill"));
            }
        } else {
            panel = panel.child(
                el("p").class("share-link").child(span("chain").attr("aria-hidden", "true")).child(link(
                    "link-url",
                    format!("/s/{}", n.link),
                    format!("Share link: /s/{}", n.link),
                )),
            );
        }
        if n.owner == self.actor {
            panel = panel.child(
                form("grant", format!("/nodes/{id}/share"), "post")
                    .child(field("grant", "actor", "Share with (an actor id)", ""))
                    .child(button("grant-submit", "Share").class("primary")),
            );
        }
        panel
    }
    fn folder(&self, node: &Node) -> Vec<Html> {
        let id = &node.id;
        let kids = self.s.children(self.actor, id);
        let granted = self.s.granted_to(self.actor, id);
        let mut e = vec![self.head(node)];
        e.push(el("p").id("head-meta").class("note").text(format!(
            "{} item{} · owner {}",
            kids.len(),
            if kids.len() == 1 { "" } else { "s" },
            node.owner
        )));
        if self.dropbox {
            e.push(
                div("actions")
                    .when(granted, |d| {
                        d.child(link("do-upload", "#upload-title", "Upload").class("btn primary"))
                            .child(link("do-make", "#make-title", "Create folder").class("btn"))
                    })
                    .child(link("do-share", "#grant-title", "Share").class("btn")),
            );
        } else {
            e.push(
                div("chips")
                    .attr("aria-hidden", "true")
                    .each(["Type", "People", "Modified"], |c| span("chip").text(c)),
            );
        }
        if id == self.s.root_id() {
            e.push(self.suggested(id));
        }
        if kids.is_empty() {
            e.push(el("p").id("empty").class("empty").text("This folder is empty."));
        } else {
            e.push(self.items(&kids));
        }
        let mut panels = div("panels").child(self.sharing(node));
        // Only a direct grant may file something here, so the forms only appear when it exists.
        if granted {
            panels = panels
                .child(
                    el("section").class("card").child(el("h2").id("make-title").text("New folder")).child(
                        form("make", "/folders", "post")
                            .child(field("make", "name", "Folder name", ""))
                            .child(field("make", "parent", "In folder", id))
                            .child(button("make-submit", "Create").class("primary")),
                    ),
                )
                .child(
                    el("section").class("card").child(el("h2").id("upload-title").text("Upload a text file")).child(
                        form("upload", "/files", "post")
                            .child(field("upload", "name", "File name", ""))
                            .child(field("upload", "parent", "In folder", id))
                            .child(
                                div("field").child(label("upload-content", "Contents")).child(
                                    el("textarea").id("upload-content").attr("name", "content").attr("rows", "3"),
                                ),
                            )
                            .child(button("upload-submit", "Upload").class("primary")),
                    ),
                );
        }
        e.push(panels);
        e
    }
    fn preview(&self, n: &Node) -> Html {
        let mut paper = div("paper").id("preview");
        let mut any = false;
        for (i, line) in n.content.lines().take(PREVIEW_LINES).enumerate() {
            any = true;
            paper = paper.child(if line.trim().is_empty() {
                div("gap")
            } else {
                el("p").id(format!("preview-line-{i}")).text(line)
            });
        }
        if !any {
            paper = paper.child(el("p").id("preview-empty").class("note").text("No preview for this item."));
        }
        let mut links = div("outbound");
        if !n.target_url.is_empty() {
            links = links.child(link("target", n.target_url.as_str(), n.target_url.as_str()));
        }
        // Addresses written in the file become real navigation; this is how sites connect.
        for (i, word) in n.content.split_whitespace().enumerate() {
            let url = word.trim_end_matches(['.', ',', ';', ')', ']']);
            if url.starts_with("http://") || url.starts_with("https://") {
                links = links.child(link(&format!("content-link-{i}"), url, url));
            }
        }
        div("viewer").child(paper).child(links)
    }
    fn details(&self, n: &Node) -> Html {
        let rows = [
            ("kind", n.kind.label().to_owned()),
            ("owner", n.owner.clone()),
            ("size", n.size_text()),
            ("type", if n.mime.is_empty() { "—".to_owned() } else { n.mime.clone() }),
            ("added", date(n.tick)),
        ];
        el("section").id("details").class("card details").child(el("h2").text("Details")).child(
            el("dl").each(rows, |(key, value)| {
                div("row")
                    .id(format!("detail-{key}"))
                    .child(el("dt").id(format!("detail-key-{key}")).text(key))
                    .child(el("dd").id(format!("detail-value-{key}")).text(value))
            }),
        )
    }
    fn file(&self, node: &Node, public: bool) -> Vec<Html> {
        let id = &node.id;
        let mut e = vec![];
        if public {
            e.push(span("badge").id("public").text("Opened with a share link"));
            e.push(div("head").id("head").child(icon(Some("head-mark".into()), node)).child(el("h1").id("head-title").text(node.name.as_str())));
        } else {
            e.push(self.head(node));
        }
        let mut aside = el("aside").class("info").child(self.details(node));
        if !public {
            aside = aside.child(self.sharing(node));
            if self.s.granted_to(self.actor, id) && node.parent.is_some() {
                aside = aside.child(
                    el("section").class("card").child(el("h2").id("rename-title").text("Rename or move")).child(
                        form("rename", format!("/nodes/{id}"), "post")
                            .child(field("rename", "name", "Name", &node.name))
                            .child(field("rename", "parent", "Folder id", node.parent.as_deref().unwrap_or_default()))
                            .child(button("rename-submit", "Save").class("primary")),
                    ),
                );
                if node.parent.as_deref() != Some(TRASH) {
                    aside = aside.child(action("trash-action", "Move to trash", format!("/nodes/{id}/trash"), "pill danger"));
                }
            }
        }
        e.push(div("split").child(self.preview(node)).child(aside));
        e
    }
    fn listing(&self, heading: &str, subtitle: String, nodes: Vec<&Node>) -> Vec<Html> {
        vec![
            div("head").id("head").child(el("h1").id("head-title").text(heading)),
            el("p").id("head-meta").class("note").text(subtitle),
            if nodes.is_empty() {
                el("p").id("empty").class("empty").text("Nothing here.")
            } else {
                self.items(&nodes)
            },
        ]
    }
}
pub(crate) fn view(s: &DriveState, actor: &str, screen: Screen) -> Result<HttpResponse> {
    let brand = s.brand().to_owned();
    let count = |n: usize| format!("{n} item{}", if n == 1 { "" } else { "s" });
    let mut v = View {
        s,
        actor,
        dropbox: s.skin.as_str() == "dropbox",
        at: "",
        query: "",
        can_new: false,
    };
    let (title, class, main) = match screen {
        Screen::Folder(id) => match s.read(actor, id) {
            Ok(node) => {
                v.at = if s.trashed(id) { "trash" } else { "drive" };
                v.can_new = s.granted_to(actor, id);
                (format!("{} · {brand}", node.name), "folder", v.folder(node))
            }
            Err(e) => return web::error(403, e),
        },
        Screen::File(id) => match s.read(actor, id) {
            Ok(node) => {
                v.at = if s.trashed(id) { "trash" } else { "drive" };
                (format!("{} · {brand}", node.name), "file", v.file(node, false))
            }
            Err(e) => return web::error(403, e),
        },
        Screen::Link(link) => match s.by_link(link) {
            Some(node) => (format!("{} · {brand}", node.name), "file", v.file(node, true)),
            None => return web::error(404, "no such share link"),
        },
        Screen::SharedWithMe => {
            v.at = "shared";
            let found = s.shared_with_me(actor);
            (
                format!("Shared with me · {brand}"),
                "list",
                v.listing("Shared with me", format!("{} other people put here.", count(found.len())), found),
            )
        }
        Screen::Starred => {
            v.at = "starred";
            (
                format!("Starred · {brand}"),
                "list",
                v.listing("Starred", "A star is yours alone; other people keep their own.".into(), s.starred(actor)),
            )
        }
        Screen::Trash => {
            v.at = "trash";
            let found = s.trash(actor);
            (
                format!("Trash · {brand}"),
                "list",
                v.listing(
                    if v.dropbox { "Deleted files" } else { "Trash" },
                    format!("{} waiting here. Nothing is ever really gone.", count(found.len())),
                    found,
                ),
            )
        }
        Screen::Search(q) => {
            v.query = q;
            let found = s.search(actor, q);
            (format!("{q} · {brand}"), "list", v.listing(&format!("Results for {q}"), count(found.len()), found))
        }
    };
    web::html::page(&v.document(&title, class, main))
}
