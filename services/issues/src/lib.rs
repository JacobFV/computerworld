//! Project-scoped issues, pull requests, comments and reviews.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub tick: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    pub author: String,
    pub decision: String,
    pub body: String,
    pub tick: u64,
}
fn open() -> String {
    "open".into()
}
fn issue() -> String {
    "issue".into()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "open")]
    pub status: String,
    #[serde(default = "issue")]
    pub kind: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub reviews: Vec<Review>,
    #[serde(default)]
    pub source_ref: String,
    #[serde(default)]
    pub target_ref: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Project {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub issues: BTreeMap<u64, Issue>,
    #[serde(default)]
    pub readers: Vec<String>,
    #[serde(default)]
    pub writers: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IssueState {
    #[serde(default)]
    pub projects: BTreeMap<String, Project>,
}
impl Project {
    pub fn can_read(&self, actor: &str) -> bool {
        self.readers.is_empty() || self.readers.iter().chain(&self.writers).any(|a| a == actor)
    }
    pub fn can_write(&self, actor: &str) -> bool {
        self.writers.is_empty() || self.writers.iter().any(|a| a == actor)
    }
    pub fn create(&mut self, actor: &str, input: Value) -> Result<u64, (u16, String)> {
        if !self.can_write(actor) {
            return Err((403, "project write denied".into()));
        }
        let title = input["title"].as_str().unwrap_or("").trim();
        if title.is_empty() {
            return Err((422, "title is required".into()));
        }
        let kind = input["kind"].as_str().unwrap_or("issue");
        if !["issue", "pull_request"].contains(&kind) {
            return Err((422, "invalid issue kind".into()));
        }
        let id = self
            .issues
            .keys()
            .next_back()
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or((409, "issue namespace exhausted".into()))?;
        self.issues.insert(
            id,
            Issue {
                id,
                title: title.into(),
                body: input["body"].as_str().unwrap_or("").into(),
                status: open(),
                kind: kind.into(),
                author: actor.into(),
                assignee: input["assignee"].as_str().unwrap_or("").into(),
                labels: vec![],
                comments: vec![],
                reviews: vec![],
                source_ref: input["source_ref"].as_str().unwrap_or("").into(),
                target_ref: input["target_ref"].as_str().unwrap_or("").into(),
            },
        );
        Ok(id)
    }
    pub fn mutate(
        &mut self,
        id: u64,
        actor: &str,
        tick: u64,
        operation: &str,
        input: Value,
    ) -> Result<(), (u16, String)> {
        if !self.can_write(actor) {
            return Err((403, "project write denied".into()));
        }
        let issue = self
            .issues
            .get_mut(&id)
            .ok_or((404, "issue not found".into()))?;
        match operation {
            "comments" => {
                let body = input["body"].as_str().unwrap_or("").trim();
                if body.is_empty() {
                    return Err((422, "comment body is required".into()));
                }
                issue.comments.push(Comment {
                    author: actor.into(),
                    body: body.into(),
                    tick,
                });
            }
            "reviews" => {
                if issue.kind != "pull_request" {
                    return Err((422, "reviews require a pull request".into()));
                }
                let decision = input["decision"].as_str().unwrap_or("");
                if !["approve", "request_changes", "comment"].contains(&decision) {
                    return Err((422, "invalid review decision".into()));
                }
                issue.reviews.push(Review {
                    author: actor.into(),
                    decision: decision.into(),
                    body: input["body"].as_str().unwrap_or("").into(),
                    tick,
                });
            }
            "" | "update" => {
                if let Some(status) = input["status"].as_str() {
                    if !["open", "in_progress", "blocked", "closed"].contains(&status) {
                        return Err((422, "invalid status".into()));
                    }
                }
                if let Some(title) = input["title"].as_str() {
                    if title.trim().is_empty() {
                        return Err((422, "title is required".into()));
                    }
                }
                if let Some(status) = input["status"].as_str() {
                    issue.status = status.into();
                }
                if let Some(title) = input["title"].as_str() {
                    issue.title = title.trim().into();
                }
                if let Some(body) = input["body"].as_str() {
                    issue.body = body.into();
                }
                if let Some(assignee) = input["assignee"].as_str() {
                    issue.assignee = assignee.into();
                }
                if let Some(labels) = input["labels"].as_array() {
                    issue.labels = labels
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
            }
            _ => return Err((404, "operation not found".into())),
        }
        Ok(())
    }
}

use cw_protocol::{HttpRequest, HttpResponse, Page};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as wire;
pub struct IssuesService;
pub fn register(registry: &mut Registry) -> cw_protocol::Result<()> {
    registry.register(IssuesService)
}
impl Service for IssuesService {
    fn kind(&self) -> &str {
        "issues"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> cw_protocol::Result<Value> {
        let state: IssueState = wire::load(&initial)?;
        Ok(serde_json::to_value(state)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        req: &HttpRequest,
    ) -> cw_protocol::Result<HttpResponse> {
        let path = wire::path(req);
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let api = parts.first() == Some(&"api");
        let p = if api { &parts[1..] } else { &parts[..] };
        let projects = state["projects"]
            .as_object_mut()
            .ok_or_else(|| cw_protocol::SimError::invalid("issues state missing projects"))?;
        if path == "/" || path == "/api/projects" {
            if req.method != "GET" {
                return wire::error(405, "method not allowed");
            }
            let visible: Vec<_> = projects
                .iter()
                .filter(|(_, v)| access(v, "readers", &ctx.actor))
                .map(|(id, v)| json!({"id":id,"name":v["name"]}))
                .collect();
            if api {
                return HttpResponse::json(200, &visible);
            }
            return wire::page(
                "Projects",
                visible
                    .iter()
                    .map(|v| {
                        wire::link(
                            &format!("project-{}", v["id"].as_str().unwrap()),
                            v["name"].as_str().unwrap_or("Project"),
                            format!("/projects/{}", v["id"].as_str().unwrap()),
                        )
                    })
                    .collect(),
            );
        }
        if p.first() != Some(&"projects") || p.len() < 2 || p.len() > 5 {
            return wire::error(404, "route not found");
        }
        let key = p[1];
        let Some(project) = projects.get_mut(key) else {
            return wire::error(404, "project not found");
        };
        if !access(project, "readers", &ctx.actor) {
            return wire::error(403, "project read denied");
        }
        if p.len() == 2 && req.method == "GET" {
            if api {
                return HttpResponse::json(200, project);
            }
            let mut page = Page::new(format!("Project {key}"));
            page.elements.push(wire::heading(
                "project",
                project["name"].as_str().unwrap_or(key),
            ));
            if let Some(issues) = project["issues"].as_object() {
                for (id, item) in issues {
                    page.elements.push(wire::link(
                        &format!("issue-{id}"),
                        format!(
                            "{key}-{id}: {} [{}]",
                            item["title"].as_str().unwrap_or(""),
                            item["status"].as_str().unwrap_or("")
                        ),
                        format!("/projects/{key}/issues/{id}"),
                    ));
                }
            }
            page.elements.push(wire::form(
                "new-issue",
                &format!("/projects/{key}/issues"),
                &[("title", "Title", ""), ("body", "Description", "")],
            ));
            return HttpResponse::page(&page);
        }
        if p.get(2) != Some(&"issues") {
            return wire::error(404, "route not found");
        }
        if p.len() == 3 {
            match req.method.as_str() {
                "GET" => return HttpResponse::json(200, &project["issues"]),
                "POST" => {
                    let input = match wire::body(req) {
                        Ok(v) => v,
                        Err(_) => return wire::error(400, "malformed request body"),
                    };
                    let mut typed: Project = serde_json::from_value(project.clone())?;
                    return match typed.create(&ctx.actor, input) {
                        Ok(id) => {
                            *project = serde_json::to_value(&typed)?;
                            if api {
                                HttpResponse::json(201, &typed.issues[&id])
                            } else {
                                issue_page(key, &typed.issues[&id])
                            }
                        }
                        Err((code, msg)) => wire::error(code, msg),
                    };
                }
                _ => return wire::error(405, "method not allowed"),
            }
        }
        let id = match p[3].parse::<u64>() {
            Ok(id) => id,
            Err(_) => return wire::error(404, "issue not found"),
        };
        let operation = p.get(4).copied().unwrap_or("");
        if req.method == "GET" && operation.is_empty() {
            let Some(item) = project["issues"].get(id.to_string()) else {
                return wire::error(404, "issue not found");
            };
            return if api {
                HttpResponse::json(200, item)
            } else {
                issue_page(key, &serde_json::from_value(item.clone())?)
            };
        }
        if req.method != "POST" && req.method != "PATCH" {
            return wire::error(405, "method not allowed");
        }
        let input = match wire::body(req) {
            Ok(v) => v,
            Err(_) => return wire::error(400, "malformed request body"),
        };
        let mut typed: Project = serde_json::from_value(project.clone())?;
        match typed.mutate(id, &ctx.actor, ctx.tick, operation, input) {
            Ok(()) => {
                *project = serde_json::to_value(&typed)?;
                if api {
                    HttpResponse::json(200, &typed.issues[&id])
                } else {
                    issue_page(key, &typed.issues[&id])
                }
            }
            Err((code, msg)) => wire::error(code, msg),
        }
    }
}
fn access(v: &Value, field: &str, actor: &str) -> bool {
    v[field]
        .as_array()
        .is_none_or(|a| a.is_empty() || a.iter().any(|x| x.as_str() == Some(actor)))
        || (field == "readers"
            && v["writers"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(actor))))
}
fn issue_page(project: &str, issue: &Issue) -> cw_protocol::Result<HttpResponse> {
    let base = format!("/projects/{project}/issues/{}", issue.id);
    let mut elements = vec![
        wire::link("back", "Project", format!("/projects/{project}")),
        wire::heading("title", format!("{project}-{} {}", issue.id, issue.title)),
        wire::paragraph(
            "status",
            format!("Status: {} · Assigned: {}", issue.status, issue.assignee),
        ),
        wire::paragraph("body", &issue.body),
    ];
    for (i, c) in issue.comments.iter().enumerate() {
        elements.push(wire::paragraph(
            &format!("comment-{i}"),
            format!("{}: {}", c.author, c.body),
        ));
    }
    for (i, r) in issue.reviews.iter().enumerate() {
        elements.push(wire::paragraph(
            &format!("review-{i}"),
            format!("{}: {} {}", r.author, r.decision, r.body),
        ));
    }
    elements.push(wire::form(
        "comment",
        &format!("{base}/comments"),
        &[("body", "Comment", "")],
    ));
    elements.push(wire::form(
        "update",
        &base,
        &[
            ("status", "Status", &issue.status),
            ("assignee", "Assignee", &issue.assignee),
        ],
    ));
    if issue.kind == "pull_request" {
        elements.push(wire::form(
            "review",
            &format!("{base}/reviews"),
            &[("decision", "Decision", "approve"), ("body", "Review", "")],
        ));
    }
    wire::page(&issue.title, elements)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_ids_and_atomic_transitions() {
        let mut a = Project::default();
        let mut b = Project::default();
        assert_eq!(a.create("alice", json!({"title":"A"})).unwrap(), 1);
        assert_eq!(b.create("bob", json!({"title":"B"})).unwrap(), 1);
        a.mutate(1, "alice", 3, "comments", json!({"body":"hello"}))
            .unwrap();
        assert!(b.issues[&1].comments.is_empty());
        assert!(a
            .mutate(1, "alice", 4, "", json!({"status":"closed","title":""}))
            .is_err());
        assert_eq!(a.issues[&1].status, "open");
    }
    #[test]
    fn reviews_do_not_close_pr() {
        let mut p = Project::default();
        p.create("alice", json!({"title":"Change","kind":"pull_request"}))
            .unwrap();
        p.mutate(1, "bob", 1, "reviews", json!({"decision":"approve"}))
            .unwrap();
        assert_eq!(p.issues[&1].status, "open");
        assert_eq!(p.issues[&1].reviews[0].decision, "approve");
        p.mutate(1, "alice", 2, "", json!({"status":"closed"}))
            .unwrap();
        assert_eq!(p.issues[&1].status, "closed");
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    fn ctx(actor: &str) -> ServiceContext {
        ServiceContext {
            actor: actor.into(),
            source: format!("machine-{actor}"),
            tick: 20,
            seed: 7,
            instance: "tracker".into(),
        }
    }
    fn req(method: &str, path: &str, body: Value) -> HttpRequest {
        HttpRequest {
            method: method.into(),
            url: format!("http://issues.internal{path}"),
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&body).unwrap(),
        }
    }
    #[test]
    fn two_clients_mutate_read_and_render_same_issue() {
        let service = IssuesService;
        let mut state = service
            .initialize(
                json!({"projects":{"OPS":{"name":"Operations","writers":["alice","bob"]}}}),
                &ctx("alice"),
            )
            .unwrap();
        let create = service
            .handle(
                &mut state,
                &ctx("alice"),
                &req(
                    "POST",
                    "/api/projects/OPS/issues",
                    json!({"title":"Repair DNS","body":"Check resolver"}),
                ),
            )
            .unwrap();
        assert_eq!(create.status, 201);
        assert_eq!(
            service
                .handle(
                    &mut state,
                    &ctx("bob"),
                    &req(
                        "POST",
                        "/api/projects/OPS/issues/1/comments",
                        json!({"body":"resolver fixed"})
                    )
                )
                .unwrap()
                .status,
            200
        );
        assert_eq!(
            service
                .handle(
                    &mut state,
                    &ctx("guest"),
                    &req(
                        "POST",
                        "/api/projects/OPS/issues/1",
                        json!({"status":"closed"})
                    )
                )
                .unwrap()
                .status,
            403
        );
        assert_eq!(
            service
                .handle(
                    &mut state,
                    &ctx("alice"),
                    &req(
                        "PATCH",
                        "/api/projects/OPS/issues/1",
                        json!({"status":"closed"})
                    )
                )
                .unwrap()
                .status,
            200
        );
        let api = service
            .handle(
                &mut state,
                &ctx("alice"),
                &req("GET", "/api/projects/OPS/issues/1", Value::Null),
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&api.body).unwrap();
        assert_eq!(value["status"], "closed");
        assert_eq!(value["comments"][0]["author"], "bob");
        assert_eq!(value["comments"][0]["tick"], 20);
        let page = service
            .handle(
                &mut state,
                &ctx("bob"),
                &req("GET", "/projects/OPS/issues/1", Value::Null),
            )
            .unwrap();
        assert!(String::from_utf8(page.body)
            .unwrap()
            .contains("resolver fixed"));
        assert_eq!(
            service
                .handle(
                    &mut state,
                    &ctx("alice"),
                    &req("GET", "/api/projects/OPS/issues/999", Value::Null)
                )
                .unwrap()
                .status,
            404
        );
        let before = state.clone();
        let mut malformed = req("POST", "/api/projects/OPS/issues", Value::Null);
        malformed.body = b"{".to_vec();
        assert_eq!(
            service
                .handle(&mut state, &ctx("alice"), &malformed)
                .unwrap()
                .status,
            400
        );
        assert_eq!(before, state);
    }
}
