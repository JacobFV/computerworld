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
    /// GitHub-style namespace. Empty keeps the repository on the legacy `/repos/{name}` path only.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub stars: BTreeSet<String>,
    /// Who is subscribed to the repository's activity. Empty by default, so a world that
    /// never mentions watching serialises exactly as it did before the button was real.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub watchers: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub forks: u64,
    /// Issues and pull requests share one number space, as they do on the real thing.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub issues: BTreeMap<u64, Thread>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pull_requests: BTreeMap<u64, Thread>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub next_number: u64,
}
fn is_zero(n: &u64) -> bool {
    *n == 0
}
fn open_state() -> String {
    "open".into()
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Comment {
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tick: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Review {
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub decision: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tick: u64,
}
/// One issue or one pull request; `head`/`base`/`reviews` are the pull-request half.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Thread {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// `open` | `closed` | `merged`.
    #[serde(default = "open_state")]
    pub state: String,
    #[serde(default)]
    pub author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub assignee: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviews: Vec<Review>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub head: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub merged_by: String,
    /// A pull request opened as a draft: shown grey and not mergeable until marked ready.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub draft: bool,
    #[serde(default)]
    pub tick: u64,
}
impl Thread {
    pub fn new(number: u64, title: &str, body: &str, author: &str, tick: u64) -> Self {
        Self {
            number,
            title: title.into(),
            body: body.into(),
            state: open_state(),
            author: author.into(),
            assignee: String::new(),
            labels: vec![],
            comments: vec![],
            reviews: vec![],
            head: String::new(),
            base: String::new(),
            merged_by: String::new(),
            draft: false,
            tick,
        }
    }
}
/// A single-file-set paste. Gists are owned but never ACL'd; publishing one is the point.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Gist {
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub tick: u64,
}
/// Skins this instance may wear: `github` and `gitlab` are the branded layouts, `plain` is
/// git.internal (a gitweb-like index at `/`; its owner paths borrow the GitHub look).
pub const SKINS: &[&str] = &["plain", "github", "gitlab"];
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GitState {
    /// Presentation only. `plain` is the original rendering and is omitted from serialised
    /// state, so worlds and checkpoints written before skins existed stay byte-identical.
    #[serde(default, skip_serializing_if = "web::Skin::is_plain")]
    pub skin: web::Skin,
    #[serde(default)]
    pub repositories: BTreeMap<String, Repository>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub gists: BTreeMap<String, Gist>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<cw_protocol::PageTheme>,
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
    /// Issues and pull requests draw from one counter, so `#14` is unambiguous in a repository.
    fn allocate(&mut self) -> u64 {
        let highest = self
            .issues
            .keys()
            .chain(self.pull_requests.keys())
            .copied()
            .max()
            .unwrap_or(0);
        let number = self.next_number.max(highest + 1);
        self.next_number = number + 1;
        number
    }
    pub fn thread(&self, pull: bool, number: u64) -> Option<&Thread> {
        if pull {
            self.pull_requests.get(&number)
        } else {
            self.issues.get(&number)
        }
    }
    fn thread_mut(&mut self, pull: bool, number: u64) -> Result<&mut Thread, (u16, String)> {
        let found = if pull {
            self.pull_requests.get_mut(&number)
        } else {
            self.issues.get_mut(&number)
        };
        found.ok_or((404, "thread not found".into()))
    }
    /// Anyone who can read a public repository may file an issue; that is what makes it public.
    pub fn open_issue(
        &mut self,
        actor: &str,
        title: &str,
        body: &str,
        tick: u64,
    ) -> Result<u64, (u16, String)> {
        if !self.can_read(actor) {
            return Err((403, "repository read denied".into()));
        }
        let title = title.trim();
        if title.is_empty() {
            return Err((422, "issue title is required".into()));
        }
        let number = self.allocate();
        self.issues
            .insert(number, Thread::new(number, title, body, actor, tick));
        Ok(number)
    }
    pub fn open_pull(
        &mut self,
        actor: &str,
        title: &str,
        head: &str,
        base: &str,
        tick: u64,
    ) -> Result<u64, (u16, String)> {
        if !self.can_write(actor) {
            return Err((403, "repository write denied".into()));
        }
        if !self.refs.contains_key(head) {
            return Err((422, "head ref does not exist".into()));
        }
        let number = self.allocate();
        let mut thread = Thread::new(number, title, "", actor, tick);
        thread.head = head.into();
        thread.base = base.into();
        self.pull_requests.insert(number, thread);
        Ok(number)
    }
    pub fn comment(
        &mut self,
        pull: bool,
        number: u64,
        actor: &str,
        body: &str,
        tick: u64,
    ) -> Result<(), (u16, String)> {
        if !self.can_read(actor) {
            return Err((403, "repository read denied".into()));
        }
        let body = body.trim();
        if body.is_empty() {
            return Err((422, "comment body is required".into()));
        }
        self.thread_mut(pull, number)?.comments.push(Comment {
            author: actor.into(),
            body: body.into(),
            tick,
        });
        Ok(())
    }
    /// Closing and reopening are maintainer actions; commenting is not.
    pub fn set_state(
        &mut self,
        pull: bool,
        number: u64,
        actor: &str,
        state: &str,
    ) -> Result<(), (u16, String)> {
        if !self.can_write(actor) {
            return Err((403, "repository write denied".into()));
        }
        if !["open", "closed"].contains(&state) {
            return Err((422, "state must be open or closed".into()));
        }
        let thread = self.thread_mut(pull, number)?;
        if thread.state == "merged" {
            return Err((409, "a merged pull request cannot change state".into()));
        }
        thread.state = state.into();
        Ok(())
    }
    pub fn review(
        &mut self,
        number: u64,
        actor: &str,
        decision: &str,
        body: &str,
        tick: u64,
    ) -> Result<(), (u16, String)> {
        if !self.can_read(actor) {
            return Err((403, "repository read denied".into()));
        }
        if !["approve", "request_changes", "comment"].contains(&decision) {
            return Err((422, "invalid review decision".into()));
        }
        self.thread_mut(true, number)?.reviews.push(Review {
            author: actor.into(),
            decision: decision.into(),
            body: body.into(),
            tick,
        });
        Ok(())
    }
    /// Merging really moves the base ref onto the head commit, so a clone afterwards sees it.
    pub fn merge(&mut self, number: u64, actor: &str, tick: u64) -> Result<(), (u16, String)> {
        if !self.can_write(actor) {
            return Err((403, "repository write denied".into()));
        }
        let pull = self.thread_mut(true, number)?;
        if pull.state != "open" {
            return Err((409, format!("pull request is already {}", pull.state)));
        }
        let (head, base) = (pull.head.clone(), pull.base.clone());
        if !head.is_empty() {
            if base.is_empty() {
                return Err((422, "pull request has no base ref".into()));
            }
            let Some(id) = self.refs.get(&head).cloned() else {
                return Err((409, "head ref no longer exists".into()));
            };
            self.refs.insert(base, id);
        }
        let pull = self.thread_mut(true, number)?;
        pull.state = "merged".into();
        pull.merged_by = actor.into();
        pull.tick = tick;
        Ok(())
    }
    /// Toggling; returns whether the actor now stars the repository.
    pub fn star(&mut self, actor: &str) -> bool {
        if self.stars.remove(actor) {
            false
        } else {
            self.stars.insert(actor.into());
            true
        }
    }
    /// Toggling, as starring is; returns whether the actor now watches the repository.
    pub fn watch(&mut self, actor: &str) -> bool {
        if self.watchers.remove(actor) {
            false
        } else {
            self.watchers.insert(actor.into());
            true
        }
    }
    /// A draft pull request leaving draft. The author may do it, as may a maintainer.
    pub fn ready(&mut self, number: u64, actor: &str) -> Result<(), (u16, String)> {
        let writer = self.can_write(actor);
        let pull = self.thread_mut(true, number)?;
        if !writer && pull.author != actor {
            return Err((403, "only the author or a maintainer may do that".into()));
        }
        if !pull.draft {
            return Err((409, "this pull request is not a draft".into()));
        }
        pull.draft = false;
        Ok(())
    }
    /// Deleting the branch a merged pull request came from: the tidy-up the merge box
    /// offers. Only once it is merged, and only for a ref that is still there.
    pub fn delete_branch(&mut self, number: u64, actor: &str) -> Result<(), (u16, String)> {
        if !self.can_write(actor) {
            return Err((403, "repository write denied".into()));
        }
        let head = {
            let pull = self.thread_mut(true, number)?;
            if pull.state != "merged" {
                return Err((409, "the pull request is not merged".into()));
            }
            pull.head.clone()
        };
        if head.is_empty() || self.refs.remove(&head).is_none() {
            return Err((409, "the branch is already gone".into()));
        }
        Ok(())
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

use cw_protocol::{HttpRequest, HttpResponse, SimError};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
pub mod github;
mod plain;

// ---- History, diffs and tree helpers: everything a page derives from commits. ----

/// A branch tip: the ref's short name and the commit it names.
pub fn branch_tip<'a>(repository: &'a Repository, branch: &str) -> Option<(String, &'a Commit)> {
    let id = repository.refs.get(&format!("refs/heads/{branch}"))?;
    repository.objects.get(id).map(|c| (id.clone(), c))
}
/// The commit a revision names: a branch, or a commit id (in full or shortened), which is
/// what "browse the repository at this point in the history" hands the tree pages.
pub fn resolve<'a>(repository: &'a Repository, rev: &str) -> Option<(String, &'a Commit)> {
    if let Some(found) = branch_tip(repository, rev) {
        return Some(found);
    }
    if rev.len() < 4 || !rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let id = repository.objects.keys().find(|k| k.starts_with(rev))?;
    repository.objects.get(id).map(|c| (id.clone(), c))
}
/// The default branch: `main` when it exists, otherwise the first head in name order.
pub fn default_branch(repository: &Repository) -> String {
    if repository.refs.contains_key("refs/heads/main") {
        return "main".into();
    }
    repository
        .refs
        .keys()
        .filter_map(|r| r.strip_prefix("refs/heads/"))
        .next()
        .unwrap_or("main")
        .to_owned()
}
/// Short names of every branch, `main` first, the rest alphabetical.
pub fn branches(repository: &Repository) -> Vec<String> {
    let mut names: Vec<String> = repository
        .refs
        .keys()
        .filter_map(|r| r.strip_prefix("refs/heads/"))
        .map(str::to_owned)
        .collect();
    names.sort_by_key(|n| (n != "main", n.clone()));
    names
}
/// Every commit reachable from `start`, newest first (by tick, then id, so it is total).
pub fn log<'a>(repository: &'a Repository, start: &str) -> Vec<(String, &'a Commit)> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![start.to_owned()];
    let mut out = vec![];
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(commit) = repository.objects.get(&id) {
            pending.extend(commit.parents.iter().cloned());
            out.push((id, commit));
        }
    }
    out.sort_by(|a, b| b.1.tick.cmp(&a.1.tick).then_with(|| a.0.cmp(&b.0)));
    out
}
/// Commits reachable from `head` but not from `base`: what a pull request brings.
pub fn commits_between<'a>(
    repository: &'a Repository,
    base: &str,
    head: &str,
) -> Vec<(String, &'a Commit)> {
    let excluded: BTreeSet<String> = log(repository, base)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    log(repository, head)
        .into_iter()
        .filter(|(id, _)| !excluded.contains(id))
        .collect()
}
/// The newest commit both tips descend from: the point a pull request's diff starts at.
pub fn merge_base(repository: &Repository, a: &str, b: &str) -> Option<String> {
    let from_b: BTreeSet<String> = log(repository, b).into_iter().map(|(id, _)| id).collect();
    log(repository, a)
        .into_iter()
        .map(|(id, _)| id)
        .find(|id| from_b.contains(id))
}
/// The paths a commit changed against its first parent (every path, for a root commit).
pub fn changed_paths(repository: &Repository, commit: &Commit) -> Vec<String> {
    let parent = commit
        .parents
        .first()
        .and_then(|p| repository.objects.get(p))
        .map(|p| &p.files);
    let mut paths: Vec<String> = commit
        .files
        .iter()
        .filter(|(path, content)| parent.and_then(|p| p.get(*path)) != Some(content))
        .map(|(path, _)| path.clone())
        .collect();
    if let Some(parent) = parent {
        paths.extend(
            parent
                .keys()
                .filter(|p| !commit.files.contains_key(*p))
                .cloned(),
        );
    }
    paths.sort();
    paths
}
/// For every path in `start`'s tree, the newest commit that touched it.
pub fn last_commit_per_path<'a>(
    repository: &'a Repository,
    start: &str,
) -> BTreeMap<String, (String, &'a Commit)> {
    let mut out = BTreeMap::new();
    for (id, commit) in log(repository, start) {
        for path in changed_paths(repository, commit) {
            out.entry(path).or_insert_with(|| (id.clone(), commit));
        }
    }
    let Some(tip) = repository.objects.get(start) else {
        return out;
    };
    out.retain(|path, _| tip.files.contains_key(path));
    out
}
/// One entry of a directory listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub dir: bool,
}
/// The immediate children of `prefix` (`""` is the root), directories first, then files.
pub fn list_tree(files: &BTreeMap<String, String>, prefix: &str) -> Vec<TreeEntry> {
    let prefix = prefix.trim_matches('/');
    let mut dirs = BTreeSet::new();
    let mut plain = vec![];
    for path in files.keys() {
        let rest = if prefix.is_empty() {
            path.as_str()
        } else {
            match path.strip_prefix(prefix).and_then(|r| r.strip_prefix('/')) {
                Some(r) => r,
                None => continue,
            }
        };
        match rest.split_once('/') {
            Some((dir, _)) => {
                dirs.insert(dir.to_owned());
            }
            None => plain.push(rest.to_owned()),
        }
    }
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        }
    };
    dirs.iter()
        .map(|d| TreeEntry {
            name: d.clone(),
            path: join(d),
            dir: true,
        })
        .chain(plain.iter().map(|f| TreeEntry {
            name: f.clone(),
            path: join(f),
            dir: false,
        }))
        .collect()
}
/// One line of a unified diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Add(String),
    Remove(String),
}
/// A run of changed lines with `CONTEXT` lines of margin, as `@@ -a,b +c,d @@`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub lines: Vec<DiffLine>,
}
impl Hunk {
    pub fn header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_lines, self.new_start, self.new_lines
        )
    }
}
pub const CONTEXT: usize = 3;
/// Line diff by longest common subsequence: deterministic, and small enough for the
/// file sizes a world carries. Equal inputs give no hunks.
pub fn diff_lines(old: &str, new: &str) -> Vec<Hunk> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let (n, m) = (a.len(), b.len());
    let mut table = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if a[i] == b[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    // Every line, tagged, with its old and new numbers (1-based; 0 when absent).
    let mut ops: Vec<(DiffLine, usize, usize)> = vec![];
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((DiffLine::Context(a[i].to_owned()), i + 1, j + 1));
            i += 1;
            j += 1;
        } else if j < m && (i == n || table[i][j + 1] >= table[i + 1][j]) {
            ops.push((DiffLine::Add(b[j].to_owned()), 0, j + 1));
            j += 1;
        } else {
            ops.push((DiffLine::Remove(a[i].to_owned()), i + 1, 0));
            i += 1;
        }
    }
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, (op, _, _))| !matches!(op, DiffLine::Context(_)))
        .map(|(k, _)| k)
        .collect();
    let mut hunks: Vec<Hunk> = vec![];
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(CONTEXT);
        let mut end = changed[k] + CONTEXT;
        while k + 1 < changed.len() && changed[k + 1].saturating_sub(CONTEXT) <= end + 1 {
            k += 1;
            end = changed[k] + CONTEXT;
        }
        let end = end.min(ops.len() - 1);
        let slice = &ops[start..=end];
        let old_start = slice.iter().find(|o| o.1 > 0).map_or(1, |o| o.1);
        let new_start = slice.iter().find(|o| o.2 > 0).map_or(1, |o| o.2);
        hunks.push(Hunk {
            old_start,
            old_lines: slice.iter().filter(|o| o.1 > 0).count(),
            new_start,
            new_lines: slice.iter().filter(|o| o.2 > 0).count(),
            lines: slice.iter().map(|o| o.0.clone()).collect(),
        });
        k += 1;
    }
    hunks
}
/// One file of a diff between two trees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    /// `added` | `removed` | `modified`.
    pub status: String,
    pub hunks: Vec<Hunk>,
    pub additions: usize,
    pub deletions: usize,
}
/// Every file that differs between two trees, in path order.
pub fn diff_trees(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) -> Vec<FileDiff> {
    let paths: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    paths
        .into_iter()
        .filter_map(|path| {
            let (before, after) = (old.get(path), new.get(path));
            if before == after {
                return None;
            }
            let status = match (before, after) {
                (None, _) => "added",
                (_, None) => "removed",
                _ => "modified",
            };
            let hunks = diff_lines(before.map_or("", |s| s), after.map_or("", |s| s));
            let count = |f: fn(&DiffLine) -> bool| {
                hunks.iter().flat_map(|h| &h.lines).filter(|l| f(l)).count()
            };
            Some(FileDiff {
                path: path.clone(),
                status: status.into(),
                additions: count(|l| matches!(l, DiffLine::Add(_))),
                deletions: count(|l| matches!(l, DiffLine::Remove(_))),
                hunks,
            })
        })
        .collect()
}
/// GitHub's linguist, reduced: the language a path counts towards, or none for prose,
/// data and licences, which the languages bar leaves out.
pub fn language_of(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    let by_name = match lower.as_str() {
        "dockerfile" => Some("Dockerfile"),
        "makefile" => Some("Makefile"),
        _ => None,
    };
    if by_name.is_some() {
        return by_name;
    }
    match lower.rsplit_once('.').map(|(_, ext)| ext)? {
        "rs" => Some("Rust"),
        "py" => Some("Python"),
        "js" | "mjs" | "cjs" => Some("JavaScript"),
        "ts" | "tsx" => Some("TypeScript"),
        "go" => Some("Go"),
        "c" | "h" => Some("C"),
        "cpp" | "cc" | "hpp" => Some("C++"),
        "java" => Some("Java"),
        "rb" => Some("Ruby"),
        "sh" | "bash" | "zsh" => Some("Shell"),
        "html" => Some("HTML"),
        "css" => Some("CSS"),
        "toml" => Some("TOML"),
        "yml" | "yaml" => Some("YAML"),
        "lua" => Some("Lua"),
        "swift" => Some("Swift"),
        "kt" => Some("Kotlin"),
        _ => None,
    }
}
/// The colour GitHub paints a language's dot and bar segment.
pub fn language_color(language: &str) -> &'static str {
    match language {
        "Rust" => "#dea584",
        "Python" => "#3572a5",
        "JavaScript" => "#f1e05a",
        "TypeScript" => "#3178c6",
        "Go" => "#00add8",
        "C" => "#555555",
        "C++" => "#f34b7d",
        "Java" => "#b07219",
        "Ruby" => "#701516",
        "Shell" => "#89e051",
        "HTML" => "#e34c26",
        "CSS" => "#663399",
        "TOML" => "#9c4221",
        "YAML" => "#cb171e",
        "Dockerfile" => "#384d54",
        "Makefile" => "#427819",
        "Lua" => "#000080",
        "Swift" => "#f05138",
        "Kotlin" => "#a97bff",
        _ => "#ededed",
    }
}
/// Bytes per language across a tree, largest first, with the share in tenths of a percent.
pub fn language_stats(files: &BTreeMap<String, String>) -> Vec<(String, u64, u32)> {
    let mut bytes: BTreeMap<&str, u64> = BTreeMap::new();
    for (path, content) in files {
        if let Some(language) = language_of(path) {
            *bytes.entry(language).or_default() += content.len() as u64;
        }
    }
    let total: u64 = bytes.values().sum::<u64>().max(1);
    let mut out: Vec<(String, u64, u32)> = bytes
        .into_iter()
        .map(|(l, b)| (l.to_owned(), b, (b * 1000 / total) as u32))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}
/// The first seven characters, as every commit link shows.
pub fn short(id: &str) -> &str {
    id.get(..7).unwrap_or(id)
}

#[cfg(test)]
mod history_tests {
    use super::*;
    fn commit(parents: &[&str], files: &[(&str, &str)], message: &str, tick: u64) -> Commit {
        Commit {
            parents: parents.iter().map(|p| p.to_string()).collect(),
            files: files
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            message: message.into(),
            author: "ada".into(),
            tick,
        }
    }
    /// main: root -> second; feature branches off root with one commit; main then merges it.
    fn repo() -> (Repository, Vec<String>) {
        let root = commit(
            &[],
            &[("README.md", "# a\n"), ("src/lib.rs", "fn a() {}\n")],
            "root",
            1,
        );
        let root_id = object_id(&root);
        let second = commit(
            &[&root_id],
            &[("README.md", "# a\nmore\n"), ("src/lib.rs", "fn a() {}\n")],
            "readme",
            5,
        );
        let second_id = object_id(&second);
        let feature = commit(
            &[&root_id],
            &[
                ("README.md", "# a\n"),
                ("src/lib.rs", "fn a() {}\nfn b() {}\n"),
                ("Cargo.toml", "[package]\n"),
            ],
            "feature",
            3,
        );
        let feature_id = object_id(&feature);
        let merge = commit(
            &[&second_id, &feature_id],
            &[
                ("README.md", "# a\nmore\n"),
                ("src/lib.rs", "fn a() {}\nfn b() {}\n"),
                ("Cargo.toml", "[package]\n"),
            ],
            "merge",
            8,
        );
        let merge_id = object_id(&merge);
        let mut repository = Repository::default();
        for (id, c) in [
            (&root_id, root),
            (&second_id, second),
            (&feature_id, feature),
            (&merge_id, merge),
        ] {
            repository.objects.insert(id.clone(), c);
        }
        repository
            .refs
            .insert("refs/heads/main".into(), merge_id.clone());
        repository
            .refs
            .insert("refs/heads/feature".into(), feature_id.clone());
        (repository, vec![root_id, second_id, feature_id, merge_id])
    }
    #[test]
    fn log_walks_every_parent_newest_first() {
        let (repository, ids) = repo();
        let history: Vec<String> = log(&repository, &ids[3])
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(
            history,
            vec![
                ids[3].clone(),
                ids[1].clone(),
                ids[2].clone(),
                ids[0].clone()
            ]
        );
        let only_feature: Vec<String> = commits_between(&repository, &ids[1], &ids[2])
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(only_feature, vec![ids[2].clone()]);
        assert_eq!(
            merge_base(&repository, &ids[1], &ids[2]).as_deref(),
            Some(ids[0].as_str())
        );
        assert_eq!(branches(&repository), vec!["main", "feature"]);
        assert_eq!(default_branch(&repository), "main");
    }
    #[test]
    fn last_commit_per_path_follows_first_parents_and_merges() {
        let (repository, ids) = repo();
        let last = last_commit_per_path(&repository, &ids[3]);
        assert_eq!(last["README.md"].0, ids[1]);
        // The merge commit's first parent lacked b(), so the merge is what "touched" lib.rs on main.
        assert_eq!(last["src/lib.rs"].0, ids[3]);
        assert_eq!(last["Cargo.toml"].0, ids[3]);
        assert_eq!(
            changed_paths(&repository, &repository.objects[&ids[0]]),
            vec!["README.md", "src/lib.rs"]
        );
        let tree = list_tree(&repository.objects[&ids[3]].files, "");
        assert_eq!(
            tree.iter()
                .map(|e| (e.name.as_str(), e.dir))
                .collect::<Vec<_>>(),
            vec![("src", true), ("Cargo.toml", false), ("README.md", false)]
        );
        assert_eq!(
            list_tree(&repository.objects[&ids[3]].files, "src")[0].path,
            "src/lib.rs"
        );
    }
    #[test]
    fn line_diff_is_minimal_and_hunked() {
        assert!(diff_lines("a\nb\n", "a\nb\n").is_empty());
        let hunks = diff_lines(
            "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n",
            "a\nb\nc\nD\ne\nf\ng\nh\ni\nj\nk\n",
        );
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].header(), "@@ -1,10 +1,11 @@");
        assert_eq!(
            hunks[0]
                .lines
                .iter()
                .filter(|l| matches!(l, DiffLine::Add(_)))
                .count(),
            2
        );
        assert_eq!(
            hunks[0]
                .lines
                .iter()
                .filter(|l| matches!(l, DiffLine::Remove(_)))
                .count(),
            1
        );
        let far = diff_lines(
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n",
            "X\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\nY\n",
        );
        assert_eq!(far.len(), 2);
        assert_eq!(far[1].header(), "@@ -9,4 +9,4 @@");
        let (repository, ids) = repo();
        let files = diff_trees(
            &repository.objects[&ids[0]].files,
            &repository.objects[&ids[2]].files,
        );
        assert_eq!(
            files
                .iter()
                .map(|f| (f.path.as_str(), f.status.as_str(), f.additions, f.deletions))
                .collect::<Vec<_>>(),
            vec![
                ("Cargo.toml", "added", 1, 0),
                ("src/lib.rs", "modified", 1, 0)
            ]
        );
        // Deterministic: the same inputs give the same hunks every time.
        assert_eq!(
            diff_lines("x\ny\n", "y\nz\n"),
            diff_lines("x\ny\n", "y\nz\n")
        );
    }
    #[test]
    fn language_stats_skip_prose_and_sum_to_the_whole() {
        let files: BTreeMap<String, String> = [
            ("README.md", "# hello world\n"),
            ("src/a.rs", "fn a() {}\n"),
            ("build.sh", "ls\n"),
            ("LICENSE", "MIT"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let stats = language_stats(&files);
        assert_eq!(stats[0].0, "Rust");
        assert_eq!(stats[1].0, "Shell");
        assert_eq!(stats.len(), 2);
        assert_eq!((stats[0].2, stats[1].2), (10 * 1000 / 13, 3 * 1000 / 13));
        assert_eq!(language_of("Dockerfile"), Some("Dockerfile"));
        assert_eq!(language_of("notes.txt"), None);
        assert_eq!(short("abcdef0123"), "abcdef0");
    }
}
/// Owner names the router reserves; a repository owner may not shadow a fixed route.
const RESERVED: &[&str] = &["api", "repos", "gist", "gists"];
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
        state.skin.check(SKINS)?;
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
            if RESERVED.contains(&repo.owner.as_str()) {
                return Err(SimError::invalid(format!(
                    "repository {name}: owner may not be one of {}",
                    RESERVED.join(", ")
                )));
            }
            for (pull, threads) in [(false, &repo.issues), (true, &repo.pull_requests)] {
                for (number, thread) in threads {
                    if thread.number != *number {
                        return Err(SimError::invalid(format!(
                            "repository {name}: {} {number} carries number {}",
                            if pull { "pull request" } else { "issue" },
                            thread.number
                        )));
                    }
                    if !["open", "closed", "merged"].contains(&thread.state.as_str()) {
                        return Err(SimError::invalid(format!(
                            "repository {name}: unknown thread state {}",
                            thread.state
                        )));
                    }
                }
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
        // `/repos/*`, `/api/git/repos/*` and the plain landing page are the original surface
        // (the API byte for byte; the two pages as plain HTML, see `plain.rs`). Everything
        // else is the owner-namespaced surface in `github.rs`.
        let legacy = api
            || parts.first() == Some(&"repos")
            || (path.is_empty() && state["skin"].as_str().unwrap_or("plain") == "plain");
        if !legacy {
            return github::handle(state, ctx, req, path);
        }
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
            return plain::index(repos, &visible);
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
            ("GET", "page") => plain::repository(repo_name, repo),
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
