//! Versioned documents with explicit reader/writer grants and optimistic edits.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DocsState {
    pub documents: BTreeMap<String, Document>,
    pub next_id: u64,
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
impl Document {
    pub fn readable(&self, actor: &str) -> bool {
        self.owner == actor || self.readers.contains(actor) || self.writers.contains(actor)
    }
    pub fn writable(&self, actor: &str) -> bool {
        self.owner == actor || self.writers.contains(actor)
    }
}
impl DocsState {
    pub fn read(&self, actor: &str, id: &str) -> Result<&Document, String> {
        self.documents
            .get(id)
            .filter(|d| d.readable(actor))
            .ok_or("document unavailable".into())
    }
    pub fn create(
        &mut self,
        actor: &str,
        title: &str,
        body: &str,
        readers: BTreeSet<String>,
        writers: BTreeSet<String>,
        time: u64,
    ) -> Result<Document, String> {
        if title.trim().is_empty() {
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
            title: title.into(),
            owner: actor.into(),
            readers,
            writers,
            body: body.into(),
            body_variants: vec![],
            revision: 1,
            history: vec![Revision {
                revision: 1,
                author: actor.into(),
                body: body.into(),
                time,
            }],
            comments: vec![],
        };
        self.documents.insert(d.id.clone(), d.clone());
        Ok(d)
    }
    pub fn edit(
        &mut self,
        actor: &str,
        id: &str,
        expected: u64,
        body: &str,
        time: u64,
    ) -> Result<Document, String> {
        let d = self
            .documents
            .get_mut(id)
            .filter(|d| d.writable(actor))
            .ok_or("document not writable")?;
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
use cw_protocol::{HttpRequest, HttpResponse, Result as SimResult};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Value};
pub struct DocsService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(DocsService)
}
fn view(s: &DocsState, actor: &str, id: Option<&str>) -> SimResult<HttpResponse> {
    let mut e = vec![web::heading("title", "Documents")];
    for (id, d) in &s.documents {
        if d.readable(actor) {
            e.push(web::link(id, &d.title, format!("/documents/{id}")));
        }
    }
    if let Some(id) = id {
        let d = match s.read(actor, id) {
            Ok(d) => d,
            Err(e) => return web::error(403, e),
        };
        e.push(web::heading("document-title", &d.title));
        e.push(web::paragraph("document-body", &d.body));
        e.extend(web::links("body", &d.body));
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
        for d in s.documents.values_mut() {
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
            return match parts.as_slice() {
                [""] => view(&s, &c.actor, None),
                ["documents"] if !api => view(&s, &c.actor, None),
                ["documents"] => HttpResponse::json(
                    200,
                    &s.documents
                        .values()
                        .filter(|d| d.readable(&c.actor))
                        .collect::<Vec<_>>(),
                ),
                ["documents", id] if !api => view(&s, &c.actor, Some(id)),
                ["documents", id] => web::domain(s.read(&c.actor, id).map(|d| json!(d))),
                _ => web::error(404, "route not found"),
            };
        }
        let b = web::body(r)?;
        let result = match (method.as_str(), parts.as_slice()) {
            ("POST", ["documents"]) => s
                .create(
                    &c.actor,
                    &web::text(&b, "title"),
                    &web::text(&b, "body"),
                    web::strings(&b, "readers").into_iter().collect(),
                    web::strings(&b, "writers").into_iter().collect(),
                    c.tick,
                )
                .map(|d| json!(d)),
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
            _ => return web::error(405, "unsupported route or method"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !api && result.is_ok() {
            view(&s, &c.actor, parts.get(1).copied())
        } else {
            web::domain(result)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revisions_permissions_and_restore() {
        let mut s = DocsState::default();
        let d = s
            .create(
                "alice",
                "Plan",
                "old",
                ["bob".into()].into(),
                BTreeSet::new(),
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
        let c = ServiceContext {
            actor: "alice".into(),
            source: "pc".into(),
            tick: 3,
            seed: 1,
            instance: "docs".into(),
        };
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
}
#[cfg(test)]
mod browser_tests {
    use super::*;
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
