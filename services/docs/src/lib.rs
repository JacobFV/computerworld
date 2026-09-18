//! Versioned documents, sheets and decks with explicit grants and optimistic edits.
mod gdocs;
use cw_protocol::{HttpRequest, HttpResponse, PageTheme, Result as SimResult};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
/// Skins this instance may wear; Google Docs gets a branded layout, `plain` is docs.internal.
pub const SKINS: &[&str] = &["plain", "gdocs"];
/// Widest sheet the twelve-column page grid can draw once the row-number gutter is spent.
pub const SHEET_COLUMNS: u8 = 11;
/// Decks stay small enough that every slide is rendered; an unrendered slide is invisible data.
pub const MAX_SLIDES: usize = 64;
/// An absent palette is the only palette `plain` ever had, so it stays out of serialised state.
fn default_theme(theme: &PageTheme) -> bool {
    *theme == PageTheme::default()
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DocsState {
    /// Presentation only. `plain` is the original rendering and is omitted from serialised
    /// state, so worlds and checkpoints written before skins existed stay byte-identical.
    #[serde(default, skip_serializing_if = "web::Skin::is_plain")]
    pub skin: web::Skin,
    #[serde(default, skip_serializing_if = "default_theme")]
    pub theme: PageTheme,
    pub documents: BTreeMap<String, Document>,
    pub next_id: u64,
}
/// What a document *is*; the editor, the grid and the deck are three readings of one record.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum DocType {
    #[default]
    Doc,
    Sheet,
    Slides,
}
impl DocType {
    /// `doc` is the shape every pre-skin document had, so it is omitted from serialised state.
    pub fn is_doc(&self) -> bool {
        matches!(self, Self::Doc)
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Doc => "doc",
            Self::Sheet => "sheet",
            Self::Slides => "slides",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Doc => "Doc",
            Self::Sheet => "Sheet",
            Self::Slides => "Slides",
        }
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "" | "doc" => Ok(Self::Doc),
            "sheet" => Ok(Self::Sheet),
            "slides" => Ok(Self::Slides),
            other => Err(format!("unknown document type {other}")),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub owner: String,
    pub readers: BTreeSet<String>,
    pub writers: BTreeSet<String>,
    pub body: String,
    pub body_variants: Vec<String>,
    pub revision: u64,
    pub history: Vec<Revision>,
    pub comments: Vec<Comment>,
    #[serde(default, skip_serializing_if = "DocType::is_doc")]
    pub doc_type: DocType,
    /// A1 references to displayed values. Sheets hold no formulas: a stored value is the value.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cells: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slides: Vec<Slide>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub starred: BTreeSet<String>,
    /// The Drive folder id this document is filed under, so a drive can point at doc ids.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Slide {
    pub title: String,
    pub body: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Revision {
    pub revision: u64,
    pub author: String,
    pub body: String,
    pub time: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Comment {
    pub author: String,
    pub text: String,
    pub time: u64,
}
/// A new document before it has an id; keeps `create` to one argument per concept.
#[derive(Clone, Debug, Default)]
pub struct Draft {
    pub title: String,
    pub doc_type: DocType,
    pub body: String,
    pub readers: BTreeSet<String>,
    pub writers: BTreeSet<String>,
}
impl Document {
    pub fn readable(&self, actor: &str) -> bool {
        self.owner == actor || self.readers.contains(actor) || self.writers.contains(actor)
    }
    pub fn writable(&self, actor: &str) -> bool {
        self.owner == actor || self.writers.contains(actor)
    }
}
/// Split an A1 reference into zero-based column and one-based row, refusing anything the grid
/// cannot draw: a stored cell nobody can see would be data the reader is lied to about.
pub fn parse_cell(cell: &str) -> Option<(u8, u32)> {
    let mut chars = cell.chars();
    let column = chars.next()?;
    let index = u8::try_from(u32::from(column).checked_sub(u32::from('A'))?).ok()?;
    let row: u32 = chars.as_str().parse().ok()?;
    (index < SHEET_COLUMNS && (1..=999).contains(&row)).then_some((index, row))
}
impl DocsState {
    pub fn read(&self, actor: &str, id: &str) -> Result<&Document, String> {
        self.documents
            .get(id)
            .filter(|d| d.readable(actor))
            .ok_or("document unavailable".into())
    }
    /// Everything `actor` may open, optionally narrowed to one kind of document.
    pub fn visible(&self, actor: &str, kind: Option<DocType>) -> Vec<&Document> {
        self.documents
            .values()
            .filter(|d| d.readable(actor) && kind.is_none_or(|k| d.doc_type == k))
            .collect()
    }
    pub fn starred(&self, actor: &str) -> Vec<&Document> {
        self.documents
            .values()
            .filter(|d| d.readable(actor) && d.starred.contains(actor))
            .collect()
    }
    pub fn create(&mut self, actor: &str, draft: Draft, time: u64) -> Result<Document, String> {
        if draft.title.trim().is_empty() {
            return Err("title required".into());
        }
        self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        while self
            .documents
            .contains_key(&format!("doc-{}", self.next_id))
        {
            self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        }
        let d = Document {
            id: format!("doc-{}", self.next_id),
            title: draft.title,
            owner: actor.into(),
            readers: draft.readers,
            writers: draft.writers,
            body: draft.body.clone(),
            body_variants: vec![],
            revision: 1,
            history: vec![Revision {
                revision: 1,
                author: actor.into(),
                body: draft.body,
                time,
            }],
            comments: vec![],
            doc_type: draft.doc_type,
            cells: BTreeMap::new(),
            slides: vec![],
            starred: BTreeSet::new(),
            folder: String::new(),
        };
        self.documents.insert(d.id.clone(), d.clone());
        Ok(d)
    }
    fn writable(&mut self, actor: &str, id: &str) -> Result<&mut Document, String> {
        self.documents
            .get_mut(id)
            .filter(|d| d.writable(actor))
            .ok_or("document not writable".into())
    }
    pub fn edit(
        &mut self,
        actor: &str,
        id: &str,
        expected: u64,
        body: &str,
        time: u64,
    ) -> Result<Document, String> {
        let d = self.writable(actor, id)?;
        if d.revision != expected {
            return Err("revision conflict".into());
        }
        let revision = d.revision.checked_add(1).ok_or("revision exhausted")?;
        d.revision = revision;
        d.body = body.into();
        d.history.push(Revision {
            revision,
            author: actor.into(),
            body: body.into(),
            time,
        });
        Ok(d.clone())
    }
    /// One cell of a sheet. An empty value clears the cell rather than storing a blank string.
    pub fn set_cell(
        &mut self,
        actor: &str,
        id: &str,
        cell: &str,
        value: &str,
        time: u64,
    ) -> Result<Document, String> {
        let cell = cell.trim().to_ascii_uppercase();
        parse_cell(&cell).ok_or("cell must be A1 through K999")?;
        let d = self.writable(actor, id)?;
        if d.doc_type != DocType::Sheet {
            return Err("document is not a sheet".into());
        }
        let revision = d.revision.checked_add(1).ok_or("revision exhausted")?;
        d.revision = revision;
        if value.trim().is_empty() {
            d.cells.remove(&cell);
        } else {
            d.cells.insert(cell.clone(), value.into());
        }
        d.history.push(Revision {
            revision,
            author: actor.into(),
            body: format!("{cell} = {value}"),
            time,
        });
        Ok(d.clone())
    }
    /// Replace slide `index`, or append when it is absent.
    pub fn set_slide(
        &mut self,
        actor: &str,
        id: &str,
        index: Option<usize>,
        slide: Slide,
        time: u64,
    ) -> Result<Document, String> {
        if slide.title.trim().is_empty() {
            return Err("slide title required".into());
        }
        let d = self.writable(actor, id)?;
        if d.doc_type != DocType::Slides {
            return Err("document is not a deck".into());
        }
        match index {
            Some(i) if i >= d.slides.len() => return Err("slide not found".into()),
            None if d.slides.len() >= MAX_SLIDES => return Err("deck is full".into()),
            _ => (),
        }
        let revision = d.revision.checked_add(1).ok_or("revision exhausted")?;
        d.revision = revision;
        let body = format!(
            "Slide {}: {}",
            index.unwrap_or(d.slides.len()) + 1,
            slide.title
        );
        match index {
            Some(i) => d.slides[i] = slide,
            None => d.slides.push(slide),
        }
        d.history.push(Revision {
            revision,
            author: actor.into(),
            body,
            time,
        });
        Ok(d.clone())
    }
    /// Starring is per reader, so two actors disagree about the same document on purpose.
    pub fn star(&mut self, actor: &str, id: &str) -> Result<bool, String> {
        let d = self
            .documents
            .get_mut(id)
            .filter(|d| d.readable(actor))
            .ok_or::<String>("document unavailable".into())?;
        Ok(if d.starred.remove(actor) {
            false
        } else {
            d.starred.insert(actor.into());
            true
        })
    }
    pub fn comment(&mut self, actor: &str, id: &str, text: &str, time: u64) -> Result<(), String> {
        if text.trim().is_empty() {
            return Err("comment required".into());
        }
        let d = self
            .documents
            .get_mut(id)
            .filter(|d| d.readable(actor))
            .ok_or("document unavailable")?;
        d.comments.push(Comment {
            author: actor.into(),
            text: text.into(),
            time,
        });
        Ok(())
    }
}
pub struct DocsService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(DocsService)
}
/// Which page was asked for; the two skins are two readings of the same three screens.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Screen<'a> {
    Home(Option<DocType>),
    Starred,
    Doc(&'a str),
}
/// The original rendering, unchanged: one flat column of links, body, editor and comments.
fn view(s: &DocsState, actor: &str, screen: Screen) -> SimResult<HttpResponse> {
    let kind = match screen {
        Screen::Home(kind) => kind,
        Screen::Starred => {
            let mut e = vec![web::heading("title", "Starred")];
            for d in s.starred(actor) {
                e.push(web::link(&d.id, &d.title, format!("/documents/{}", d.id)));
            }
            return web::page("Documents", e);
        }
        Screen::Doc(_) => None,
    };
    let mut e = vec![web::heading("title", "Documents")];
    for (id, d) in &s.documents {
        if d.readable(actor) && kind.is_none_or(|k| d.doc_type == k) {
            e.push(web::link(id, &d.title, format!("/documents/{id}")));
        }
    }
    if let Screen::Doc(id) = screen {
        let d = match s.read(actor, id) {
            Ok(d) => d,
            Err(e) => return web::error(403, e),
        };
        e.push(web::heading("document-title", &d.title));
        e.push(web::paragraph("document-body", &d.body));
        e.extend(web::links("body", &d.body));
        // Sheets and decks carry their content outside `body`; an unskinned page still shows it.
        for (cell, value) in &d.cells {
            e.push(web::paragraph(
                &format!("cell-{cell}"),
                format!("{cell}: {value}"),
            ));
        }
        for (i, slide) in d.slides.iter().enumerate() {
            e.push(web::paragraph(
                &format!("slide-{i}"),
                format!("{}: {}", slide.title, slide.body),
            ));
        }
        if d.writable(actor) {
            e.push(web::form(
                "edit",
                &format!("/documents/{id}"),
                &[
                    ("revision", "Revision", &d.revision.to_string()),
                    ("body", "Content", &d.body),
                ],
            ));
        }
        for (i, c) in d.comments.iter().enumerate() {
            e.push(web::paragraph(
                &format!("comment-{i}"),
                format!("{}: {}", c.author, c.text),
            ));
        }
        e.push(web::form(
            "comment",
            &format!("/documents/{id}/comments"),
            &[("text", "Comment", "")],
        ));
    } else {
        e.push(web::form(
            "create",
            "/documents",
            &[
                ("title", "Title", ""),
                ("body", "Content", ""),
                ("readers", "Readers", ""),
                ("writers", "Writers", ""),
            ],
        ));
    }
    web::page("Documents", e)
}
fn render(s: &DocsState, actor: &str, screen: Screen) -> SimResult<HttpResponse> {
    if s.skin.is_plain() {
        view(s, actor, screen)
    } else {
        gdocs::view(s, actor, screen)
    }
}
impl Service for DocsService {
    fn kind(&self) -> &str {
        "docs"
    }
    fn handle_with_effects(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<cw_sdk::ServiceTransition> {
        let response = self.handle(state, c, r)?;
        let effects = if matches!(
            r.method.to_ascii_uppercase().as_str(),
            "POST" | "PUT" | "PATCH" | "DELETE"
        ) && (200..300).contains(&response.status)
        {
            vec![cw_sdk::ServiceEffect::Emit {
                name: format!("{}.mutated", self.kind()),
                data: json!({"actor":c.actor,"path":web::path(r),"method":r.method,"tick":c.tick}),
            }]
        } else {
            vec![]
        };
        Ok(cw_sdk::ServiceTransition { response, effects })
    }

    fn initialize(&self, initial: Value, c: &ServiceContext) -> SimResult<Value> {
        let mut s: DocsState = web::load(&initial)?;
        s.skin.check(SKINS)?;
        for (id, d) in &mut s.documents {
            if d.id.is_empty() {
                d.id = id.clone();
            }
            if !d.body_variants.is_empty() {
                d.body = d.body_variants[(c.seed % d.body_variants.len() as u64) as usize].clone();
                d.body_variants.clear();
            }
            if d.history.is_empty() {
                d.history.push(Revision {
                    revision: d.revision,
                    author: d.owner.clone(),
                    body: d.body.clone(),
                    time: c.tick,
                });
            }
            for cell in d.cells.keys() {
                parse_cell(cell).ok_or_else(|| {
                    cw_protocol::SimError::invalid(format!("cell {cell} is outside A1..K999"))
                })?;
            }
        }
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<HttpResponse> {
        let mut s: DocsState = web::load(state)?;
        let p = web::path(r);
        let path = p.strip_prefix("/api").unwrap_or(&p);
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let api = p.starts_with("/api/");
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            let kind = match web::query(r, "type") {
                Some(v) => match DocType::parse(&v) {
                    Ok(k) => Some(k),
                    Err(e) => return web::error(400, e),
                },
                None => None,
            };
            let home = Screen::Home(kind);
            return match parts.as_slice() {
                [""] => render(&s, &c.actor, home),
                ["documents"] if !api => render(&s, &c.actor, home),
                ["documents"] => HttpResponse::json(200, &s.visible(&c.actor, kind)),
                ["starred"] if !api => render(&s, &c.actor, Screen::Starred),
                ["starred"] => HttpResponse::json(200, &s.starred(&c.actor)),
                ["documents", id] if !api => render(&s, &c.actor, Screen::Doc(id)),
                ["documents", id] => web::domain(s.read(&c.actor, id).map(|d| json!(d))),
                _ => web::error(404, "route not found"),
            };
        }
        let b = web::body(r)?;
        // Slides are numbered from one on the page, so they are numbered from one on the wire.
        let slide_number = |raw: &str| -> Result<Option<usize>, String> {
            match raw.trim() {
                "" => Ok(None),
                v => v
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n >= 1)
                    .map(|n| Some(n - 1))
                    .ok_or_else(|| "slide numbers start at 1".into()),
            }
        };
        let slide = |b: &Value| Slide {
            title: web::text(b, "title"),
            body: web::text(b, "body"),
        };
        let result = match (method.as_str(), parts.as_slice()) {
            ("POST", ["documents"]) => match DocType::parse(&web::text(&b, "type")) {
                Ok(doc_type) => s
                    .create(
                        &c.actor,
                        Draft {
                            title: web::text(&b, "title"),
                            doc_type,
                            body: web::text(&b, "body"),
                            readers: web::strings(&b, "readers").into_iter().collect(),
                            writers: web::strings(&b, "writers").into_iter().collect(),
                        },
                        c.tick,
                    )
                    .map(|d| json!(d)),
                Err(e) => Err(e),
            },
            ("POST" | "PUT" | "PATCH", ["documents", id]) => s
                .edit(
                    &c.actor,
                    id,
                    web::number(&b, "revision")?,
                    &web::text(&b, "body"),
                    c.tick,
                )
                .map(|d| json!(d)),
            ("POST", ["documents", id, "comments"]) => s
                .comment(&c.actor, id, &web::text(&b, "text"), c.tick)
                .map(|_| json!({"ok":true})),
            ("POST", ["documents", id, "cells"]) => s
                .set_cell(
                    &c.actor,
                    id,
                    &web::text(&b, "cell"),
                    &web::text(&b, "value"),
                    c.tick,
                )
                .map(|d| json!(d)),
            // A blank slide number appends, so one form both adds and rewrites a slide.
            ("POST", ["documents", id, "slides"]) => slide_number(&web::text(&b, "index"))
                .and_then(|i| s.set_slide(&c.actor, id, i, slide(&b), c.tick))
                .map(|d| json!(d)),
            ("POST" | "PUT" | "PATCH", ["documents", id, "slides", n]) => slide_number(n)
                .and_then(|i| match i {
                    Some(i) => s.set_slide(&c.actor, id, Some(i), slide(&b), c.tick),
                    None => Err("slide number required".into()),
                })
                .map(|d| json!(d)),
            ("POST", ["documents", id, "star"]) => s
                .star(&c.actor, id)
                .map(|starred| json!({"id":id,"starred":starred})),
            _ => return web::error(405, "unsupported route or method"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !api && result.is_ok() {
            match parts.get(1) {
                Some(id) => render(&s, &c.actor, Screen::Doc(id)),
                None => render(&s, &c.actor, Screen::Home(None)),
            }
        } else {
            web::domain(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The docs.internal seed as world.json carries it: the shape the `plain` skin must preserve.
    const SEED: &str = r##"{"next_id":0,"documents":{"launch":{"id":"launch","title":"Atlas launch checklist","owner":"carol","readers":["alice","bob"],"writers":["alice","bob"],"body":"Release code: awaiting confirmation\nRepository: http://git.internal/repos/onboarding\n","revision":1,"history":[],"comments":[]}}}"##;
    const PLAIN_STATE: &str = r##"{"documents":{"launch":{"body":"Release code: awaiting confirmation\nRepository: http://git.internal/repos/onboarding\n","body_variants":[],"comments":[],"history":[{"author":"carol","body":"Release code: awaiting confirmation\nRepository: http://git.internal/repos/onboarding\n","revision":1,"time":7}],"id":"launch","owner":"carol","readers":["alice","bob"],"revision":1,"title":"Atlas launch checklist","writers":["alice","bob"]}},"next_id":0}"##;
    const PLAIN_HOME: &str = r##"{"version":1,"title":"Documents","elements":[{"kind":"heading","id":"title","text":"Documents","level":1},{"kind":"link","id":"launch","text":"Atlas launch checklist","url":"/documents/launch"},{"kind":"form","id":"create","action":{"method":"POST","url":"/documents","fields":{"body":"$create-body","readers":"$create-readers","title":"$create-title","writers":"$create-writers"}},"children":[{"kind":"input","id":"create-title","label":"Title","value":"","placeholder":""},{"kind":"input","id":"create-body","label":"Content","value":"","placeholder":""},{"kind":"input","id":"create-readers","label":"Readers","value":"","placeholder":""},{"kind":"input","id":"create-writers","label":"Writers","value":"","placeholder":""},{"kind":"button","id":"create-submit","text":"Submit","action":{"method":"POST","url":"/documents","fields":{"body":"$create-body","readers":"$create-readers","title":"$create-title","writers":"$create-writers"}}}]}]}"##;
    const PLAIN_DOCUMENT: &str = r##"{"version":1,"title":"Documents","elements":[{"kind":"heading","id":"title","text":"Documents","level":1},{"kind":"link","id":"launch","text":"Atlas launch checklist","url":"/documents/launch"},{"kind":"heading","id":"document-title","text":"Atlas launch checklist","level":1},{"kind":"text","id":"document-body","text":"Release code: awaiting confirmation\nRepository: http://git.internal/repos/onboarding\n"},{"kind":"link","id":"body-link-5","text":"http://git.internal/repos/onboarding","url":"http://git.internal/repos/onboarding"},{"kind":"form","id":"edit","action":{"method":"POST","url":"/documents/launch","fields":{"body":"$edit-body","revision":"$edit-revision"}},"children":[{"kind":"input","id":"edit-revision","label":"Revision","value":"1","placeholder":""},{"kind":"input","id":"edit-body","label":"Content","value":"Release code: awaiting confirmation\nRepository: http://git.internal/repos/onboarding\n","placeholder":""},{"kind":"button","id":"edit-submit","text":"Submit","action":{"method":"POST","url":"/documents/launch","fields":{"body":"$edit-body","revision":"$edit-revision"}}}]},{"kind":"form","id":"comment","action":{"method":"POST","url":"/documents/launch/comments","fields":{"text":"$comment-text"}},"children":[{"kind":"input","id":"comment-text","label":"Comment","value":"","placeholder":""},{"kind":"button","id":"comment-submit","text":"Submit","action":{"method":"POST","url":"/documents/launch/comments","fields":{"text":"$comment-text"}}}]}]}"##;
    fn ctx(actor: &str) -> ServiceContext {
        ServiceContext {
            actor: actor.into(),
            source: "pc".into(),
            tick: 7,
            seed: 1,
            instance: "docs".into(),
        }
    }
    fn body(response: HttpResponse) -> String {
        String::from_utf8(response.body).unwrap()
    }
    /// The whole point of a skin: world data, checkpoints and page bytes written before skins
    /// existed must still come out of this crate byte for byte.
    #[test]
    fn plain_state_and_pages_are_byte_identical() {
        let c = ctx("alice");
        let mut state = DocsService
            .initialize(serde_json::from_str(SEED).unwrap(), &c)
            .unwrap();
        assert_eq!(serde_json::to_string(&state).unwrap(), PLAIN_STATE);
        let home = DocsService
            .handle(&mut state, &c, &HttpRequest::get("http://docs.internal/"))
            .unwrap();
        assert_eq!(body(home), PLAIN_HOME);
        let document = DocsService
            .handle(
                &mut state,
                &c,
                &HttpRequest::get("http://docs.internal/documents/launch"),
            )
            .unwrap();
        assert_eq!(body(document), PLAIN_DOCUMENT);
    }
    #[test]
    fn revisions_permissions_and_restore() {
        let mut s = DocsState::default();
        let d = s
            .create(
                "alice",
                Draft {
                    title: "Plan".into(),
                    body: "old".into(),
                    readers: ["bob".into()].into(),
                    ..Draft::default()
                },
                0,
            )
            .unwrap();
        assert!(s.read("eve", &d.id).is_err());
        assert!(s.edit("bob", &d.id, 1, "bad", 1).is_err());
        s.edit("alice", &d.id, 1, "new", 1).unwrap();
        let before = s.clone();
        assert!(s.edit("alice", &d.id, 1, "stale", 2).is_err());
        assert_eq!(s, before);
        s.comment("bob", &d.id, "looks good", 3).unwrap();
        assert_eq!(s.read("bob", &d.id).unwrap().body, "new");
        assert_eq!(s.read("bob", &d.id).unwrap().history.len(), 2);
        let restored: DocsState = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(s, restored);
    }
    #[test]
    fn seeded_initialization_and_pure_render() {
        let c = ctx("alice");
        let initial = json!({"documents":{"one":{"id":"one","owner":"alice","title":"Plan","revision":1,"body_variants":["A","B"]}}});
        let mut s = DocsService.initialize(initial.clone(), &c).unwrap();
        assert_eq!(s["documents"]["one"]["body"], "B");
        let mut other = c.clone();
        other.seed = 2;
        assert_ne!(s, DocsService.initialize(initial, &other).unwrap());
        let before = s.clone();
        let r = HttpRequest::get("http://docs/documents/one");
        assert_eq!(
            DocsService.handle(&mut s, &c, &r).unwrap(),
            DocsService.handle(&mut s, &c, &r).unwrap()
        );
        assert_eq!(before, s);
    }
    #[test]
    fn sheets_take_cells_and_refuse_the_unrenderable() {
        let mut s = DocsState::default();
        let sheet = s
            .create(
                "carol",
                Draft {
                    title: "Q3 metrics".into(),
                    doc_type: DocType::Sheet,
                    writers: ["alice".into()].into(),
                    ..Draft::default()
                },
                1,
            )
            .unwrap();
        let doc = s
            .create(
                "carol",
                Draft {
                    title: "Notes".into(),
                    ..Draft::default()
                },
                1,
            )
            .unwrap();
        assert!(s.set_cell("bob", &sheet.id, "A1", "no", 2).is_err());
        assert!(s.set_cell("alice", &doc.id, "A1", "no", 2).is_err());
        assert!(s.set_cell("alice", &sheet.id, "Z9", "no", 2).is_err());
        assert!(s.set_cell("alice", &sheet.id, "A0", "no", 2).is_err());
        let after = s.set_cell("alice", &sheet.id, "b3", "1204", 2).unwrap();
        assert_eq!(after.cells["B3"], "1204");
        assert_eq!(after.revision, 2);
        assert_eq!(after.history.last().unwrap().body, "B3 = 1204");
        let cleared = s.set_cell("alice", &sheet.id, "B3", " ", 3).unwrap();
        assert!(cleared.cells.is_empty() && cleared.revision == 3);
    }
    #[test]
    fn decks_append_replace_and_refuse_a_missing_slide() {
        let mut s = DocsState::default();
        let deck = s
            .create(
                "carol",
                Draft {
                    title: "Atlas launch review".into(),
                    doc_type: DocType::Slides,
                    ..Draft::default()
                },
                1,
            )
            .unwrap();
        let slide = |t: &str| Slide {
            title: t.into(),
            body: "body".into(),
        };
        s.set_slide("carol", &deck.id, None, slide("One"), 2)
            .unwrap();
        assert!(s
            .set_slide("carol", &deck.id, Some(4), slide("Nope"), 3)
            .is_err());
        assert!(s
            .set_slide("carol", &deck.id, None, slide("  "), 3)
            .is_err());
        let after = s
            .set_slide("carol", &deck.id, Some(0), slide("Two"), 4)
            .unwrap();
        assert_eq!(after.slides.len(), 1);
        assert_eq!(after.slides[0].title, "Two");
        assert_eq!(after.revision, 3);
    }
    #[test]
    fn starring_is_per_reader_and_survives_a_round_trip() {
        let mut s = DocsState::default();
        let d = s
            .create(
                "carol",
                Draft {
                    title: "Plan".into(),
                    readers: ["alice".into()].into(),
                    ..Draft::default()
                },
                1,
            )
            .unwrap();
        assert!(s.star("alice", &d.id).unwrap());
        assert!(s.star("bob", &d.id).is_err());
        assert_eq!(s.starred("alice").len(), 1);
        assert_eq!(s.starred("carol").len(), 0);
        let restored: DocsState = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(s, restored);
        assert!(!s.star("alice", &d.id).unwrap());
        assert_eq!(s.starred("alice").len(), 0);
    }
    #[test]
    fn initialize_rejects_a_bad_skin_and_an_undrawable_cell() {
        let c = ctx("alice");
        assert!(DocsService
            .initialize(json!({"skin":"nonesuch"}), &c)
            .is_err());
        assert!(DocsService
            .initialize(
                json!({"documents":{"s":{"id":"s","doc_type":"sheet","cells":{"Z9":"x"}}}}),
                &c
            )
            .is_err());
        assert!(DocsService
            .initialize(
                json!({"skin":"gdocs","documents":{"s":{"id":"s","doc_type":"sheet","cells":{"A1":"x"}}}}),
                &c
            )
            .is_ok());
    }
    /// The skinned pages must be legal pages, and every control on them must carry a route.
    #[test]
    fn gdocs_pages_validate_and_only_promise_real_routes() {
        let c = ctx("alice");
        let seed = json!({"skin":"gdocs","theme":{"accent":"#1a73e8"},"documents":{
            "atlas-launch":{"id":"atlas-launch","title":"Atlas launch checklist","owner":"carol","readers":["alice"],"writers":["alice"],"body":"Release code: ATLAS-2026\nSee http://github.com/northstar/atlas\n","revision":1,"starred":["alice"]},
            "q3-metrics":{"id":"q3-metrics","title":"Q3 metrics","owner":"carol","doc_type":"sheet","writers":["alice"],"revision":1,"cells":{"A1":"Metric","B1":"Q3","A2":"Latency p95","B2":"184 ms"}},
            "atlas-launch-review":{"id":"atlas-launch-review","title":"Atlas launch review","owner":"carol","doc_type":"slides","readers":["alice"],"revision":1,"slides":[{"title":"Where we are","body":"Ship date locked."}]}}});
        let mut state = DocsService.initialize(seed, &c).unwrap();
        let mut grids = 0;
        for url in [
            "http://docs.google.com/",
            "http://docs.google.com/?type=sheet",
            "http://docs.google.com/starred",
            "http://docs.google.com/documents/atlas-launch",
            "http://docs.google.com/documents/q3-metrics",
            "http://docs.google.com/documents/atlas-launch-review",
        ] {
            let response = DocsService
                .handle(&mut state, &c, &HttpRequest::get(url))
                .unwrap();
            assert_eq!(response.status, 200, "{url}");
            let page: cw_protocol::Page = serde_json::from_slice(&response.body).unwrap();
            page.validate().expect(url);
            assert!(page.theme.is_some(), "{url} must carry the skin palette");
            grids += usize::from(
                page.elements
                    .iter()
                    .any(|e| matches!(e, cw_protocol::PageElement::Grid { .. })),
            );
        }
        assert_eq!(
            grids, 5,
            "both file galleries, starred, the sheet and the deck are grids"
        );
        let bad = DocsService
            .handle(
                &mut state,
                &c,
                &HttpRequest::get("http://docs.google.com/?type=nonesuch"),
            )
            .unwrap();
        assert_eq!(bad.status, 400);
    }
    /// A sheet edited through the skinned page is the same sheet the API serves.
    #[test]
    fn a_browser_can_drive_the_sheet_and_the_star() {
        let c = ctx("alice");
        let mut state = DocsService
            .initialize(
                json!({"skin":"gdocs","documents":{"q3":{"id":"q3","title":"Q3 metrics","owner":"alice","doc_type":"sheet","revision":1,"cells":{"A1":"Metric"}}}}),
                &c,
            )
            .unwrap();
        let mut browser = cw_browser::BrowserState::default();
        let mut transport = |r: HttpRequest| DocsService.handle(&mut state, &c, &r);
        browser
            .navigate("http://docs.google.com/documents/q3", &mut transport)
            .unwrap();
        browser.fill("cell-cell", "B2").unwrap();
        browser.fill("cell-value", "184 ms").unwrap();
        browser.click("cell-submit", &mut transport).unwrap();
        browser.click("star", &mut transport).unwrap();
        let response = DocsService
            .handle(
                &mut state,
                &c,
                &HttpRequest::get("http://docs.google.com/api/documents/q3"),
            )
            .unwrap();
        let document: Document = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(document.cells["B2"], "184 ms");
        assert_eq!(document.revision, 2);
        assert!(document.starred.contains("alice"));
    }
}
#[cfg(test)]
mod browser_tests {
    use super::*;
    /// The native Documents application saves `{revision, body}` and expects the committed
    /// document back; this is that contract, exercised through the browser's own form path.
    #[test]
    fn native_form_changes_same_api_document() {
        let context = ServiceContext {
            actor: "alice".into(),
            source: "workstation".into(),
            tick: 100,
            seed: 4,
            instance: "docs".into(),
        };
        let mut state=DocsService.initialize(json!({"documents":{"plan":{"id":"plan","title":"Plan","owner":"alice","body":"before","revision":1}}}),&context).unwrap();
        let mut browser = cw_browser::BrowserState::default();
        let mut transport =
            |request: HttpRequest| DocsService.handle(&mut state, &context, &request);
        browser
            .navigate("http://docs.internal/documents/plan", &mut transport)
            .unwrap();
        browser
            .fill("edit-body", "updated through browser")
            .unwrap();
        browser.click("edit-submit", &mut transport).unwrap();
        let response = DocsService
            .handle(
                &mut state,
                &context,
                &HttpRequest::get("http://docs.internal/api/documents/plan"),
            )
            .unwrap();
        let document: Document = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(document.body, "updated through browser");
        assert_eq!(document.revision, 2);
    }
}
