//! Synthetic Git hosting. Content-addressed commits and refs travel over HTTP;
//! local working trees are never shared with this service.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Canonical commit object. A full tree is deliberate: causal Git semantics
/// without packfiles, subprocesses or pretending to implement Git wire v2.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Commit {
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tick: u64,
}
pub fn object_id(commit: &Commit) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(commit).expect("commit serializes"))
    )
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Repository {
    #[serde(default)]
    pub objects: BTreeMap<String, Commit>,
    #[serde(default)]
    pub refs: BTreeMap<String, String>,
    #[serde(default)]
    pub readers: Vec<String>,
    #[serde(default)]
    pub writers: Vec<String>,
    /// Initialization convenience; consumed into the first commit.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GitState {
    #[serde(default)]
    pub repositories: BTreeMap<String, Repository>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Push {
    #[serde(default)]
    pub objects: BTreeMap<String, Commit>,
    #[serde(default)]
    pub refs: BTreeMap<String, String>,
    /// Compare-and-swap; empty old value asserts the ref does not exist.
    #[serde(default)]
    pub expected_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub force: bool,
}
impl Repository {
    pub fn initialize(&mut self) {
        if !self.files.is_empty() && self.refs.is_empty() {
            let commit = Commit {
                files: std::mem::take(&mut self.files),
                message: "Initial commit".into(),
                author: "system".into(),
                ..Commit::default()
            };
            let id = object_id(&commit);
            self.objects.insert(id.clone(), commit);
            self.refs.insert("refs/heads/main".into(), id);
        }
    }
    pub fn can_read(&self, actor: &str) -> bool {
        self.readers.is_empty() || self.readers.iter().chain(&self.writers).any(|s| s == actor)
    }
    pub fn can_write(&self, actor: &str) -> bool {
        self.writers.is_empty() || self.writers.iter().any(|s| s == actor)
    }
    pub fn push(&mut self, actor: &str, push: Push) -> Result<(), (u16, String)> {
        if !self.can_write(actor) {
            return Err((403, "repository write denied".into()));
        }
        for (name, old) in &push.expected_refs {
            if self.refs.get(name).map(String::as_str).unwrap_or("") != old {
                return Err((409, format!("stale ref {name}")));
            }
        }
        for (id, object) in &push.objects {
            if object_id(object) != *id {
                return Err((422, format!("object hash mismatch: {id}")));
            }
            for path in object.files.keys() {
                if path.starts_with('/')
                    || path.contains(['\\', ':', '\0'])
                    || path.split('/').any(|p| {
                        p == "." || p == ".." || p.is_empty() || p.eq_ignore_ascii_case(".git")
                    })
                {
                    return Err((422, "invalid tree path".into()));
                }
            }
        }
        let lookup = |id: &str| push.objects.get(id).or_else(|| self.objects.get(id));
        for object in push.objects.values() {
            if object.parents.iter().any(|id| lookup(id).is_none()) {
                return Err((422, "missing parent object".into()));
            }
        }
        for (name, id) in &push.refs {
            if !name.starts_with("refs/")
                || name.contains("..")
                || name.chars().any(char::is_whitespace)
            {
                return Err((422, "invalid ref name".into()));
            }
            if lookup(id).is_none() {
                return Err((422, "ref points to missing object".into()));
            }
            if !push.force {
                if let Some(old) = self.refs.get(name) {
                    let mut pending = vec![id.as_str()];
                    let mut seen = BTreeSet::new();
                    let mut found = false;
                    while let Some(at) = pending.pop() {
                        if at == old {
                            found = true;
                            break;
                        }
                        if seen.insert(at) {
                            if let Some(commit) = lookup(at) {
                                pending.extend(commit.parents.iter().map(String::as_str));
                            }
                        }
                    }
                    if !found {
                        return Err((409, "non-fast-forward update".into()));
                    }
                }
            }
        }
        self.objects.extend(push.objects);
        self.refs.extend(push.refs);
        Ok(())
    }
}

use cw_protocol::{HttpRequest, HttpResponse, Page, PageElement, SimError};
use cw_sdk::{Registry, Service, ServiceContext};
pub struct GitService;
pub fn register(registry: &mut Registry) -> cw_protocol::Result<()> {
    registry.register(GitService)
}
fn response_error(status: u16, message: impl Into<String>) -> cw_protocol::Result<HttpResponse> {
    HttpResponse::json(status, &json!({"error":message.into()}))
}
impl Service for GitService {
    fn kind(&self) -> &str {
        "git"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> cw_protocol::Result<Value> {
        let mut state: GitState = serde_json::from_value(if initial.is_null() {
            json!({})
        } else {
            initial
        })?;
        for (name, repo) in state.repositories.iter_mut() {
            if name.is_empty() || name.contains('/') || name == "." || name == ".." {
                return Err(SimError::invalid(
                    "repository names must be one nonempty path segment",
                ));
            }
            if !repo.files.is_empty() && !repo.refs.is_empty() {
                return Err(SimError::invalid(
                    "use initial files or explicit refs, not both",
                ));
            }
            repo.initialize();
            let mut validator = Repository::default();
            validator
                .push(
                    "",
                    Push {
                        objects: repo.objects.clone(),
                        refs: repo.refs.clone(),
                        force: true,
                        ..Push::default()
                    },
                )
                .map_err(|(_, message)| {
                    SimError::invalid(format!("repository {name}: {message}"))
                })?;
        }
        Ok(serde_json::to_value(state)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        req: &HttpRequest,
    ) -> cw_protocol::Result<HttpResponse> {
        let url = url::Url::parse(&req.url).map_err(|e| SimError::invalid(e.to_string()))?;
        let path = url.path().trim_matches('/');
        let parts: Vec<_> = path.split('/').collect();
        let api = parts.starts_with(&["api", "git", "repos"]);
        let repos = state
            .get_mut("repositories")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| SimError::invalid("git state missing repositories"))?;
        if path.is_empty() || path == "api/git/repos" {
            if req.method != "GET" {
                return response_error(405, "method not allowed");
            }
            let visible: Vec<_> = repos
                .iter()
                .filter(|(_, v)| allowed(v, "readers", &ctx.actor))
                .map(|(k, _)| k.clone())
                .collect();
            if api {
                return HttpResponse::json(200, &visible);
            }
            let mut page = Page::new("Git repositories");
            for name in visible {
                page.elements.push(PageElement::Link {
                    id: format!("repo-{name}"),
                    text: name.clone(),
                    url: format!("/repos/{name}"),
                });
            }
            return HttpResponse::page(&page);
        }
        let (repo_name, op) = if api && parts.len() >= 4 {
            (parts[3], parts.get(4).copied().unwrap_or(""))
        } else if parts.first() == Some(&"repos") && parts.len() == 2 {
            (parts[1], "page")
        } else {
            return response_error(404, "route not found");
        };
        if parts.len() > 5 {
            return response_error(404, "route not found");
        }
        let Some(repo) = repos.get_mut(repo_name) else {
            return response_error(404, "repository not found");
        };
        if !allowed(repo, "readers", &ctx.actor) {
            return response_error(403, "repository read denied");
        }
        match (req.method.as_str(), op) {
            ("GET", "refs") => HttpResponse::json(200, &repo["refs"]),
            ("GET", "objects") => HttpResponse::json(200, &repo["objects"]),
            ("GET", "") => {
                HttpResponse::json(200, &json!({"refs":repo["refs"],"objects":repo["objects"]}))
            }
            ("GET", "page") => {
                let mut page = Page::new(format!("Repository {repo_name}"));
                page.elements.push(PageElement::Heading {
                    id: "repo-title".into(),
                    text: repo_name.into(),
                    level: 1,
                });
                if let Some(refs) = repo["refs"].as_object() {
                    for (name, id) in refs {
                        page.elements.push(PageElement::Text {
                            id: format!("ref-{name}"),
                            text: format!("{name} {}", id.as_str().unwrap_or("")),
                        });
                        if let Some(files) = id
                            .as_str()
                            .and_then(|id| repo["objects"][id]["files"].as_object())
                        {
                            for (name, content) in files {
                                page.elements.push(PageElement::Text {
                                    id: format!("file-{name}"),
                                    text: format!("{name}\n{}", content.as_str().unwrap_or("")),
                                });
                            }
                        }
                    }
                }
                HttpResponse::page(&page)
            }
            ("POST", "push") => {
                let push: Push = match serde_json::from_slice(&req.body) {
                    Ok(p) => p,
                    Err(_) => return response_error(400, "malformed push JSON"),
                };
                let mut typed: Repository = serde_json::from_value(repo.clone())?;
                match typed.push(&ctx.actor, push) {
                    Ok(()) => {
                        *repo = serde_json::to_value(&typed)?;
                        HttpResponse::json(200, &json!({"refs":typed.refs}))
                    }
                    Err((code, msg)) => response_error(code, msg),
                }
            }
            (_, "refs" | "objects" | "push" | "page" | "") => {
                response_error(405, "method not allowed")
            }
            _ => response_error(404, "route not found"),
        }
    }
}
fn allowed(repo: &Value, field: &str, actor: &str) -> bool {
    let acl = repo[field].as_array();
    acl.is_none_or(|a| a.is_empty() || a.iter().any(|x| x.as_str() == Some(actor)))
        || (field == "readers"
            && repo["writers"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(actor))))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ctx(actor: &str) -> ServiceContext {
        ServiceContext {
            actor: actor.into(),
            source: actor.into(),
            tick: 100,
            seed: 42,
            instance: "git".into(),
        }
    }
    fn request(method: &str, path: &str, body: Value) -> HttpRequest {
        HttpRequest {
            method: method.into(),
            url: format!("http://git.internal{path}"),
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&body).unwrap(),
        }
    }
    #[test]
    fn two_clients_clone_push_and_conflict() {
        let service = GitService;
        let mut state = service
            .initialize(
                json!({"repositories":{"demo":{"files":{"README":"initial"},"writers":["alice"]}}}),
                &ctx("alice"),
            )
            .unwrap();
        let initial = service
            .handle(
                &mut state,
                &ctx("bob"),
                &request("GET", "/api/git/repos/demo", Value::Null),
            )
            .unwrap();
        let cloned: Value = serde_json::from_slice(&initial.body).unwrap();
        let old = cloned["refs"]["refs/heads/main"]
            .as_str()
            .unwrap()
            .to_string();
        let commit = Commit {
            parents: vec![old.clone()],
            files: BTreeMap::from([("README".into(), "changed".into())]),
            message: "edit".into(),
            author: "alice".into(),
            tick: 100,
        };
        let id = object_id(&commit);
        let push = json!({"objects":{id.clone():commit},"refs":{"refs/heads/main":id},"expected_refs":{"refs/heads/main":old}});
        let req = request("POST", "/api/git/repos/demo/push", push);
        assert_eq!(
            service
                .handle(&mut state, &ctx("bob"), &req)
                .unwrap()
                .status,
            403
        );
        assert_eq!(
            service
                .handle(&mut state, &ctx("alice"), &req)
                .unwrap()
                .status,
            200
        );
        assert_eq!(
            service
                .handle(&mut state, &ctx("alice"), &req)
                .unwrap()
                .status,
            409
        );
        let got = service
            .handle(
                &mut state,
                &ctx("bob"),
                &request("GET", "/api/git/repos/demo", Value::Null),
            )
            .unwrap();
        let fetched: Value = serde_json::from_slice(&got.body).unwrap();
        assert_eq!(fetched["objects"][&id]["files"]["README"], "changed");
        assert_ne!(cloned, fetched);
    }
    #[test]
    fn invalid_object_push_is_atomic() {
        let mut repo = Repository::default();
        let bad = Push {
            objects: BTreeMap::from([("bad".into(), Commit::default())]),
            ..Push::default()
        };
        assert_eq!(repo.push("alice", bad).unwrap_err().0, 422);
        assert!(repo.objects.is_empty());
    }
    #[test]
    fn non_fast_forward_rejected() {
        let mut r = Repository {
            files: BTreeMap::from([("a".into(), "a".into())]),
            ..Repository::default()
        };
        r.initialize();
        let c = Commit {
            message: "unrelated".into(),
            ..Commit::default()
        };
        let id = object_id(&c);
        assert_eq!(
            r.push(
                "a",
                Push {
                    objects: BTreeMap::from([(id.clone(), c)]),
                    refs: BTreeMap::from([("refs/heads/main".into(), id)]),
                    ..Push::default()
                }
            )
            .unwrap_err()
            .0,
            409
        );
    }
}

#[cfg(test)]
mod path_security_tests {
    use super::*;
    #[test]
    fn reject_portable_path_escapes_atomically() {
        for path in [
            "../x",
            "..\\x",
            "C:/x",
            "a/./x",
            ".git/config",
            "a/.GIT/config",
            "a\0b",
            "/x",
            "a//b",
        ] {
            let object = Commit {
                files: BTreeMap::from([(path.into(), "malicious".into())]),
                ..Commit::default()
            };
            let id = object_id(&object);
            let mut repo = Repository::default();
            assert_eq!(
                repo.push(
                    "alice",
                    Push {
                        objects: BTreeMap::from([(id.clone(), object)]),
                        refs: BTreeMap::from([("refs/heads/main".into(), id)]),
                        ..Push::default()
                    }
                )
                .unwrap_err()
                .0,
                422,
                "{path:?}"
            );
            assert!(repo.objects.is_empty());
            assert!(repo.refs.is_empty());
            let context = ServiceContext {
                actor: "alice".into(),
                source: "pc".into(),
                tick: 0,
                seed: 1,
                instance: "git".into(),
            };
            assert!(
                GitService
                    .initialize(
                        json!({"repositories":{"demo":{"files":{path:"bad"}}}}),
                        &context
                    )
                    .is_err(),
                "initial {path:?}"
            );
        }
    }
}
