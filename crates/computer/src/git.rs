//! Content-addressed synthetic Git. The transport exchanges explicit repository snapshots,
//! not the native Git pack protocol. Objects and refs remain independent of the worktree.
use crate::{CommandResult, Computer, ShellHost};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Repository {
    pub branch: String,
    pub refs: BTreeMap<String, String>,
    pub commits: BTreeMap<String, Commit>,
    pub index: BTreeMap<String, Vec<u8>>,
    pub remotes: BTreeMap<String, String>,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commit {
    pub parents: Vec<String>,
    pub files: BTreeMap<String, Vec<u8>>,
    pub message: String,
    pub author: String,
    pub tick: u64,
}
fn root(c: &Computer) -> Result<String, String> {
    let mut p = c.cwd.clone();
    loop {
        if c.vfs
            .read_as(&format!("{p}/.git/state.json"), &c.user)
            .is_ok()
        {
            return Ok(p);
        }
        if p == "/" {
            return Err("not a git repository".into());
        }
        p = p
            .rsplit_once('/')
            .map(|x| if x.0.is_empty() { "/" } else { x.0 })
            .unwrap_or("/")
            .into();
    }
}
fn load(c: &Computer, r: &str) -> Result<Repository, String> {
    let repository: Repository = serde_json::from_slice(
        &c.vfs
            .read_as(&format!("{r}/.git/state.json"), &c.user)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if repository
        .index
        .keys()
        .chain(repository.commits.values().flat_map(|cm| cm.files.keys()))
        .any(|p| !safe_tree_path(p))
    {
        return Err("unsafe repository tree path".into());
    }
    Ok(repository)
}
fn save(c: &mut Computer, r: &str, g: &Repository, t: u64) -> Result<(), String> {
    c.vfs
        .mkdir_all_as(&format!("{r}/.git"), &c.user, t)
        .map_err(|e| e.to_string())?;
    c.vfs
        .write_as(
            &format!("{r}/.git/state.json"),
            &serde_json::to_vec(g).map_err(|e| e.to_string())?,
            &c.user,
            t,
        )
        .map_err(|e| e.to_string())
}
fn files(c: &Computer, r: &str) -> BTreeMap<String, Vec<u8>> {
    let prefix = format!("{}/", r.trim_end_matches('/'));
    c.vfs
        .all_files()
        .into_iter()
        .filter_map(|(p, v)| {
            if c.vfs.check_access(&p, &c.user, true, false, false).is_err() {
                return None;
            }
            p.strip_prefix(&prefix)
                .filter(|s| !s.starts_with(".git/"))
                .map(|s| (s.into(), v))
        })
        .collect()
}
fn materialize(
    c: &mut Computer,
    r: &str,
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
    t: u64,
) -> Result<(), String> {
    for p in before.keys() {
        if !after.contains_key(p) {
            let _ = c.vfs.remove_as(&format!("{r}/{p}"), false, &c.user);
        }
    }
    for (p, v) in after {
        let path = format!("{r}/{p}");
        if let Some((parent, _)) = path.rsplit_once('/') {
            c.vfs
                .mkdir_all_as(parent, &c.user, t)
                .map_err(|e| e.to_string())?;
        }
        c.vfs
            .write_as(&path, v, &c.user, t)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn changes(a: &BTreeMap<String, Vec<u8>>, b: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, char> {
    let mut out = BTreeMap::new();
    for (p, v) in b {
        if a.get(p) != Some(v) {
            out.insert(p.clone(), if a.contains_key(p) { 'M' } else { 'A' });
        }
    }
    for p in a.keys() {
        if !b.contains_key(p) {
            out.insert(p.clone(), 'D');
        }
    }
    out
}
pub fn execute(
    c: &mut Computer,
    args: &[String],
    t: u64,
    host: &mut dyn ShellHost,
) -> CommandResult {
    if args.first().is_some_and(|s| s == "-C") {
        let Some(path) = args.get(1) else {
            return CommandResult::error("git: -C requires path");
        };
        let old = c.cwd.clone();
        let dir = c.resolve(path);
        if c.vfs.list_as(&dir, &c.user).is_err() {
            return CommandResult::error("git: invalid -C directory");
        }
        c.cwd = dir;
        let result = execute(c, &args[2..], t, host);
        c.cwd = old;
        return result;
    }
    match run(c, args, t, host) {
        Ok(s) => CommandResult::success(s),
        Err(e) => CommandResult::error(format!("git: {e}\n")),
    }
}
fn run(c: &mut Computer, a: &[String], t: u64, host: &mut dyn ShellHost) -> Result<String, String> {
    let command = a.first().map(String::as_str).unwrap_or("status");
    if command == "init" {
        let r = a.get(1).map(|s| c.resolve(s)).unwrap_or(c.cwd.clone());
        c.vfs
            .mkdir_all_as(&r, &c.user, t)
            .map_err(|e| e.to_string())?;
        if c.vfs.exists(&format!("{r}/.git/state.json")) {
            return Ok(format!("Reinitialized repository in {r}/.git\n"));
        }
        save(
            c,
            &r,
            &Repository {
                branch: "main".into(),
                ..Default::default()
            },
            t,
        )?;
        return Ok(format!("Initialized repository in {r}/.git\n"));
    }
    if command == "clone" {
        let url = a.get(1).ok_or("clone requires URL")?;
        let dest = a.get(2).cloned().unwrap_or_else(|| {
            url.rsplit('/')
                .next()
                .unwrap_or("repo")
                .trim_end_matches(".git")
                .into()
        });
        let r = c.resolve(&dest);
        if c.vfs.stat(&r).is_ok() {
            return Err("destination exists".into());
        }
        let mut g = fetch(host, url)?;
        g.remotes.insert("origin".into(), url.clone());
        c.vfs
            .mkdir_all_as(&r, &c.user, t)
            .map_err(|e| e.to_string())?;
        let head = g
            .refs
            .get(&g.branch)
            .and_then(|h| g.commits.get(h))
            .map(|c| c.files.clone())
            .unwrap_or_default();
        materialize(c, &r, &BTreeMap::new(), &head, t)?;
        g.index = head;
        save(c, &r, &g, t)?;
        return Ok(format!("Cloned into {dest}\n"));
    }
    let r = root(c)?;
    let mut g = load(c, &r)?;
    let head = g
        .refs
        .get(&g.branch)
        .and_then(|h| g.commits.get(h))
        .map(|c| c.files.clone())
        .unwrap_or_default();
    let mut work = files(c, &r);
    let ignore = work
        .iter()
        .filter(|(p, _)| p.as_str() == ".gitignore" || p.ends_with("/.gitignore"))
        .map(|(p, v)| {
            (
                p.rsplit_once('/')
                    .map(|(p, _)| format!("{p}/"))
                    .unwrap_or_default(),
                String::from_utf8_lossy(v).into_owned(),
            )
        })
        .collect::<Vec<_>>();
    work.retain(|path, _| {
        g.index.contains_key(path) || head.contains_key(path) || !ignored(path, &ignore)
    });
    let out = match command {
        "add" => {
            if a.len() < 2 {
                return Err("add requires path".into());
            }
            for p in &a[1..] {
                let resolved = if p == "-A" { r.clone() } else { c.resolve(p) };
                let prefix = format!("{r}/");
                let rel = if resolved == r {
                    ""
                } else {
                    resolved
                        .strip_prefix(&prefix)
                        .ok_or("path outside repository")?
                };
                let matches = |s: &str| {
                    rel.is_empty() || p == "-A" || s == rel || s.starts_with(&format!("{rel}/"))
                };
                g.index.retain(|s, _| !matches(s) || work.contains_key(s));
                for (s, v) in &work {
                    if matches(s) {
                        g.index.insert(s.clone(), v.clone());
                    }
                }
            }
            String::new()
        }
        "config" => {
            let key = a.get(1).ok_or("config requires key")?;
            if let Some(value) = a.get(2) {
                g.config.insert(key.clone(), value.clone());
                String::new()
            } else {
                format!("{}\n", g.config.get(key).ok_or("config key not found")?)
            }
        }
        "commit" => {
            if a.iter().any(|s| s == "-a" || s == "-am") {
                let tracked = g.index.keys().cloned().collect::<Vec<_>>();
                for p in tracked {
                    if let Some(v) = work.get(&p) {
                        g.index.insert(p, v.clone());
                    } else {
                        g.index.remove(&p);
                    }
                }
            }
            let i = a
                .iter()
                .position(|s| s == "-m" || s == "-am")
                .ok_or("commit requires -m MESSAGE")?;
            let message = a.get(i + 1).ok_or("missing commit message")?.clone();
            if g.index == head {
                return Err("nothing to commit".into());
            }
            let object = Commit {
                parents: g.refs.get(&g.branch).cloned().into_iter().collect(),
                files: g.index.clone(),
                message: message.clone(),
                author: g.config.get("user.name").cloned().unwrap_or(c.user.clone()),
                tick: t,
            };
            let hash = format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&wire_commit(&object)?).map_err(|e| e.to_string())?
                )
            );
            g.commits.insert(hash.clone(), object);
            g.refs.insert(g.branch.clone(), hash.clone());
            format!("[{} {}] {}\n", g.branch, &hash[..8], message)
        }
        "status" => {
            let staged = changes(&head, &g.index);
            let unstaged = changes(&g.index, &work);
            let mut names = staged
                .keys()
                .chain(unstaged.keys())
                .cloned()
                .collect::<Vec<_>>();
            names.sort();
            names.dedup();
            let mut s = format!("On branch {}\n", g.branch);
            for p in names {
                s.push_str(&format!(
                    "{}{} {p}\n",
                    staged.get(&p).copied().unwrap_or(' '),
                    unstaged.get(&p).copied().unwrap_or(' ')
                ));
            }
            s
        }
        "log" => {
            let mut h = g.refs.get(&g.branch).cloned();
            let mut s = String::new();
            let mut visited = std::collections::BTreeSet::new();
            while let Some(hash) = h {
                if !visited.insert(hash.clone()) {
                    return Err("cyclic commit ancestry".into());
                }
                let cm = g.commits.get(&hash).ok_or("missing commit object")?;
                s.push_str(&format!(
                    "commit {hash}\nAuthor: {}\nTick: {}\n\n    {}\n\n",
                    cm.author, cm.tick, cm.message
                ));
                h = cm.parents.first().cloned();
            }
            s
        }
        "diff" => {
            let (a0, b0) = if a.iter().any(|s| s == "--cached" || s == "--staged") {
                (&head, &g.index)
            } else {
                (&g.index, &work)
            };
            let mut s = String::new();
            for (p, _) in changes(a0, b0) {
                s.push_str(&format!("--- a/{p}\n+++ b/{p}\n"));
                if let Some(v) = a0.get(&p) {
                    for l in String::from_utf8_lossy(v).lines() {
                        s.push_str(&format!("-{l}\n"));
                    }
                }
                if let Some(v) = b0.get(&p) {
                    for l in String::from_utf8_lossy(v).lines() {
                        s.push_str(&format!("+{l}\n"));
                    }
                }
            }
            s
        }
        "branch" => {
            if let Some(name) = a.get(1) {
                if g.refs.contains_key(name) {
                    return Err("branch already exists".into());
                }
                let hash = g.refs.get(&g.branch).ok_or("no commits")?.clone();
                g.refs.insert(name.clone(), hash);
                String::new()
            } else {
                g.refs
                    .keys()
                    .map(|k| format!("{} {k}\n", if k == &g.branch { "*" } else { " " }))
                    .collect()
            }
        }
        "checkout" | "switch" => {
            let create = a.get(1).is_some_and(|s| s == "-b" || s == "-c");
            let name = a
                .get(if create { 2 } else { 1 })
                .ok_or("missing branch")?
                .clone();
            if work != g.index || g.index != head {
                return Err("commit or discard local changes before checkout".into());
            }
            if create {
                if g.refs.contains_key(&name) {
                    return Err("branch exists".into());
                }
                if let Some(h) = g.refs.get(&g.branch).cloned() {
                    g.refs.insert(name.clone(), h);
                }
            } else if !g.refs.contains_key(&name) {
                return Err("unknown branch".into());
            }
            g.branch = name;
            let after = g
                .refs
                .get(&g.branch)
                .and_then(|h| g.commits.get(h))
                .map(|c| c.files.clone())
                .unwrap_or_default();
            materialize(c, &r, &head, &after, t)?;
            g.index = after;
            format!("Switched to branch {}\n", g.branch)
        }
        "remote" => {
            if a.get(1).is_some_and(|s| s == "add") {
                g.remotes.insert(
                    a.get(2).ok_or("missing name")?.clone(),
                    a.get(3).ok_or("missing URL")?.clone(),
                );
                String::new()
            } else {
                g.remotes
                    .iter()
                    .map(|(k, v)| format!("{k}\t{v}\n"))
                    .collect()
            }
        }
        "push" => {
            let name = a.get(1).map(String::as_str).unwrap_or("origin");
            let url = g.remotes.get(name).ok_or("unknown remote")?;
            let request = cw_protocol::HttpRequest {
                method: "POST".into(),
                url: format!("{}/push", remote_url(url)?),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body: serde_json::to_vec(&serde_json::json!({"objects":g.commits.iter().map(|(id,c)|Ok((id.clone(),wire_commit(c)?))).collect::<Result<BTreeMap<_,_>,String>>()?,"refs":g.refs.iter().filter(|(b,_)|!b.contains('/')).map(|(b,h)|(format!("refs/heads/{b}"),h.clone())).collect::<BTreeMap<_,_>>(),"force":false})).map_err(|e|e.to_string())?,
            };
            let response = host.http(request)?;
            if response.status >= 400 {
                return Err(format!("remote HTTP {}", response.status));
            }
            format!("Pushed {} to {name}\n", g.branch)
        }
        "fetch" | "pull" => {
            let name = a.get(1).map(String::as_str).unwrap_or("origin");
            let remote = fetch(host, g.remotes.get(name).ok_or("unknown remote")?)?;
            for (h, c) in remote.commits {
                g.commits.insert(h, c);
            }
            for (b, h) in remote.refs {
                g.refs.insert(format!("{name}/{b}"), h.clone());
                if command == "pull" && b == g.branch {
                    if work != head || g.index != head {
                        return Err("local changes would be overwritten".into());
                    }
                    let mut ancestor = Some(h.clone());
                    let current = g.refs.get(&g.branch);
                    let mut compatible = current.is_none();
                    let mut visited = std::collections::BTreeSet::new();
                    while let Some(id) = ancestor {
                        if !visited.insert(id.clone()) {
                            return Err("cyclic commit ancestry".into());
                        }
                        if Some(&id) == current {
                            compatible = true;
                            break;
                        }
                        ancestor = g.commits.get(&id).and_then(|v| v.parents.first().cloned());
                    }
                    if !compatible {
                        return Err("non-fast-forward pull requires merge".into());
                    }
                    let after = g
                        .commits
                        .get(&h)
                        .ok_or("missing remote commit")?
                        .files
                        .clone();
                    materialize(c, &r, &head, &after, t)?;
                    g.index = after;
                    g.refs.insert(g.branch.clone(), h);
                }
            }
            "Fetched remote\n".into()
        }
        _ => return Err(format!("unsupported command: {command}")),
    };
    save(c, &r, &g, t)?;
    Ok(out)
}
#[derive(Serialize, Deserialize)]
struct WireCommit {
    parents: Vec<String>,
    files: BTreeMap<String, String>,
    message: String,
    author: String,
    tick: u64,
}
fn wire_commit(c: &Commit) -> Result<WireCommit, String> {
    Ok(WireCommit {
        parents: c.parents.clone(),
        files: c
            .files
            .iter()
            .map(|(k, v)| {
                String::from_utf8(v.clone())
                    .map(|v| (k.clone(), v))
                    .map_err(|_| "remote Git supports UTF-8 files".to_string())
            })
            .collect::<Result<_, _>>()?,
        message: c.message.clone(),
        author: c.author.clone(),
        tick: c.tick,
    })
}
fn remote_url(url: &str) -> Result<String, String> {
    let (scheme, rest) = url.split_once("://").ok_or("remote requires HTTP URL")?;
    if scheme != "http" && scheme != "https" {
        return Err("unsupported remote scheme".into());
    }
    let (host, path) = rest
        .split_once('/')
        .ok_or("remote requires repository path")?;
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let repo = path
        .strip_prefix("api/git/repos/")
        .or_else(|| path.strip_prefix("repos/"))
        .unwrap_or(path);
    Ok(format!("{scheme}://{host}/api/git/repos/{repo}"))
}
fn fetch(host: &mut dyn ShellHost, url: &str) -> Result<Repository, String> {
    let response = host.http(cw_protocol::HttpRequest {
        method: "GET".into(),
        url: remote_url(url)?,
        headers: BTreeMap::new(),
        body: vec![],
    })?;
    if response.status >= 400 {
        return Err(format!("remote HTTP {}", response.status));
    }
    #[derive(Deserialize)]
    struct Remote {
        refs: BTreeMap<String, String>,
        objects: BTreeMap<String, WireCommit>,
    }
    let remote: Remote = serde_json::from_slice(&response.body).map_err(|e| e.to_string())?;
    let mut g = Repository {
        branch: "main".into(),
        ..Default::default()
    };
    for (h, o) in remote.objects {
        let hash = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&o).map_err(|e| e.to_string())?)
        );
        if hash != h {
            return Err("remote object hash mismatch".into());
        }
        if o.files.keys().any(|p| !safe_tree_path(p)) {
            return Err("unsafe remote tree path".into());
        }
        g.commits.insert(
            h,
            Commit {
                parents: o.parents,
                files: o
                    .files
                    .into_iter()
                    .map(|(p, v)| (p, v.into_bytes()))
                    .collect(),
                message: o.message,
                author: o.author,
                tick: o.tick,
            },
        );
    }
    for (r, h) in remote.refs {
        if !g.commits.contains_key(&h) {
            return Err("remote ref has missing object".into());
        }
        if let Some(b) = r.strip_prefix("refs/heads/") {
            g.refs.insert(b.into(), h);
        }
    }
    Ok(g)
}

fn ignored(path: &str, rules: &[(String, String)]) -> bool {
    let mut ignored = false;
    for (base, text) in rules {
        let Some(relative) = path.strip_prefix(base) else {
            continue;
        };
        for line in text.lines() {
            let mut rule = line.trim();
            if rule.is_empty() || rule.starts_with('#') {
                continue;
            }
            let negate = rule.starts_with('!');
            if negate {
                rule = &rule[1..];
            }
            let directory = rule.ends_with('/');
            rule = rule.trim_matches('/');
            let matches = if rule.contains('/') {
                crate::shell::wildcard(rule, relative)
                    || directory && relative.starts_with(&format!("{rule}/"))
            } else {
                relative
                    .split('/')
                    .any(|part| crate::shell::wildcard(rule, part))
            };
            if matches {
                ignored = !negate
            }
        }
    }
    ignored
}

fn safe_tree_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && !path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.eq_ignore_ascii_case(".git"))
}
#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn git_tree_paths_cannot_escape_or_overwrite_metadata() {
        for path in [
            "/root",
            "../outside",
            "a/../b",
            "C:/outside",
            "a\\..\\outside",
            ".git/state.json",
            "a/.GIT/config",
            "a/./b",
            "a\0b",
        ] {
            assert!(!safe_tree_path(path), "{path}");
        }
        assert!(safe_tree_path("src/main.rs"));
    }
}
