use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum VfsError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    Exists(String),
    #[error("not a directory: {0}")]
    NotDirectory(String),
    #[error("is a directory: {0}")]
    IsDirectory(String),
    #[error("directory not empty: {0}")]
    NotEmpty(String),
    #[error("too many symbolic links")]
    LinkLoop,
    #[error("permission denied: {0}")]
    Permission(String),
    #[error("invalid path: {0}")]
    Invalid(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeKind {
    File(Arc<Vec<u8>>),
    Directory(BTreeMap<String, u64>),
    Symlink(String),
}
/// The group every node is born into when nothing says otherwise. The world models
/// users but not a group database, so a node's group is a name, not a membership set.
pub const DEFAULT_GROUP: &str = "users";
fn default_group() -> String {
    DEFAULT_GROUP.into()
}
/// The mask new nodes are created through: `0o666 & !umask` for a file, `0o777 & !umask`
/// for a directory, exactly as `open`/`mkdir` apply it. One mask per filesystem rather
/// than per process — this world runs one shell per computer.
fn default_umask() -> u16 {
    0o022
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Inode {
    pub id: u64,
    pub kind: NodeKind,
    pub owner: String,
    #[serde(default = "default_group")]
    pub group: String,
    pub mode: u16,
    pub links: u32,
    pub created: u64,
    /// Last read. The filesystem behaves as if mounted `noatime`: reading does not
    /// move it, because a read takes `&self`. `touch -a`, a copy with `-p` and a
    /// creation do.
    #[serde(default)]
    pub accessed: u64,
    pub modified: u64,
    /// Last metadata change: a chmod, a chown, a link count change or a write.
    #[serde(default)]
    pub changed: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Metadata {
    pub inode: u64,
    pub owner: String,
    #[serde(default = "default_group")]
    pub group: String,
    pub mode: u16,
    /// Hard links as a POSIX `stat` reports them: for a directory, `2` plus one per
    /// subdirectory (`.`, the parent's entry, and each child's `..`).
    pub links: u32,
    pub size: usize,
    pub is_dir: bool,
    pub is_symlink: bool,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub accessed: u64,
    pub modified: u64,
    #[serde(default)]
    pub changed: u64,
    /// The stored text of a symbolic link, as `readlink` gives it.
    #[serde(default)]
    pub target: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Vfs {
    pub case_sensitive: bool,
    next_inode: u64,
    nodes: Arc<BTreeMap<u64, Inode>>,
    #[serde(default = "default_umask")]
    umask: u16,
}
/// Canonical virtual path; Windows drive roots are represented as /C:/...
pub fn normalize_path(cwd: &str, path: &str) -> String {
    let p = path.replace('\\', "/");
    let absolute = p.starts_with('/') || p.as_bytes().get(1) == Some(&b':');
    let merged = if absolute { p } else { format!("{cwd}/{p}") };
    let mut parts = Vec::new();
    for part in merged.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}
impl Vfs {
    /// Validate a deserialized inode graph before accepting an external checkpoint.
    pub fn validate(&self) -> Result<(), VfsError> {
        if !matches!(
            self.nodes.get(&1).map(|n| &n.kind),
            Some(NodeKind::Directory(_))
        ) {
            return Err(VfsError::Invalid("missing root directory".into()));
        }
        let mut counts = BTreeMap::<u64, u32>::from([(1, 1)]);
        for (id, n) in self.nodes.iter() {
            if *id != n.id || *id >= self.next_inode {
                return Err(VfsError::Invalid("inode identity/counter".into()));
            }
            if let NodeKind::Directory(entries) = &n.kind {
                let mut names = BTreeSet::new();
                for (name, child) in entries {
                    if name.is_empty()
                        || name.contains('/')
                        || name == "."
                        || name == ".."
                        || !names.insert(self.key(name))
                        || !self.nodes.contains_key(child)
                    {
                        return Err(VfsError::Invalid("invalid directory entry".into()));
                    }
                    *counts.entry(*child).or_default() += 1;
                }
            }
        }
        for (id, n) in self.nodes.iter() {
            if counts.get(id) != Some(&n.links) {
                return Err(VfsError::Invalid("invalid link count".into()));
            }
        }
        fn visit(v: &Vfs, id: u64, seen: &mut BTreeSet<u64>) -> Result<(), VfsError> {
            if let NodeKind::Directory(entries) = &v.nodes[&id].kind {
                if !seen.insert(id) {
                    return Err(VfsError::Invalid("directory cycle or hardlink".into()));
                }
                for child in entries.values() {
                    visit(v, *child, seen)?
                }
            }
            Ok(())
        }
        let mut seen = BTreeSet::new();
        visit(self, 1, &mut seen)?;
        if self
            .nodes
            .values()
            .filter(|n| matches!(n.kind, NodeKind::Directory(_)))
            .count()
            != seen.len()
        {
            return Err(VfsError::Invalid("unreachable directory".into()));
        }
        Ok(())
    }
    pub fn new(case_sensitive: bool) -> Self {
        let root = Inode {
            id: 1,
            kind: NodeKind::Directory(BTreeMap::new()),
            owner: "root".into(),
            group: "root".into(),
            mode: 0o755,
            links: 1,
            created: 0,
            accessed: 0,
            modified: 0,
            changed: 0,
        };
        Self {
            case_sensitive,
            next_inode: 2,
            nodes: Arc::new(BTreeMap::from([(1, root)])),
            umask: default_umask(),
        }
    }
    /// The creation mask. `umask` reads it; `umask 027` sets it.
    pub fn umask(&self) -> u16 {
        self.umask
    }
    pub fn set_umask(&mut self, mask: u16) {
        self.umask = mask & 0o777;
    }
    fn key(&self, s: &str) -> String {
        if self.case_sensitive {
            s.into()
        } else {
            s.to_lowercase()
        }
    }
    fn child(&self, id: u64, name: &str) -> Result<u64, VfsError> {
        match &self.nodes[&id].kind {
            NodeKind::Directory(e) => e
                .iter()
                .find(|(k, _)| self.key(k) == self.key(name))
                .map(|(_, id)| *id)
                .ok_or_else(|| VfsError::NotFound(name.into())),
            _ => Err(VfsError::NotDirectory(name.into())),
        }
    }
    fn lookup(&self, path: &str, follow: bool, depth: usize) -> Result<u64, VfsError> {
        if depth > 40 {
            return Err(VfsError::LinkLoop);
        }
        let path = normalize_path("/", path);
        let parts: Vec<_> = path.split('/').filter(|p| !p.is_empty()).collect();
        let mut id = 1;
        let mut prefix = String::from("/");
        for (i, part) in parts.iter().enumerate() {
            id = self.child(id, part)?;
            if let NodeKind::Symlink(target) = &self.nodes[&id].kind {
                if follow || i + 1 < parts.len() {
                    let dest = normalize_path(&prefix, target);
                    let rest = parts[i + 1..].join("/");
                    return self.lookup(&format!("{dest}/{rest}"), follow, depth + 1);
                }
            }
            prefix = normalize_path(&prefix, part);
        }
        Ok(id)
    }
    fn parent(&self, path: &str) -> Result<(u64, String), VfsError> {
        let p = normalize_path("/", path);
        if p == "/" {
            return Err(VfsError::Invalid(p));
        }
        let (parent, name) = p.rsplit_once('/').unwrap();
        Ok((
            self.lookup(if parent.is_empty() { "/" } else { parent }, true, 0)?,
            name.into(),
        ))
    }
    fn insert(
        &mut self,
        path: &str,
        kind: NodeKind,
        owner: &str,
        tick: u64,
    ) -> Result<u64, VfsError> {
        self.insert_with_mode(path, kind, owner, tick, None)
    }
    /// `mode` overrides the umask-derived default, as `mkdir -m` and `install -m` do.
    /// A symbolic link always carries `0o777`: the mode of a link is never consulted.
    fn insert_with_mode(
        &mut self,
        path: &str,
        kind: NodeKind,
        owner: &str,
        tick: u64,
        mode: Option<u16>,
    ) -> Result<u64, VfsError> {
        let (parent, name) = self.parent(path)?;
        if self.child(parent, &name).is_ok() {
            return Err(VfsError::Exists(path.into()));
        }
        if !matches!(self.nodes[&parent].kind, NodeKind::Directory(_)) {
            return Err(VfsError::NotDirectory(path.into()));
        }
        let id = self.next_inode;
        self.next_inode += 1;
        let mode = match (mode, &kind) {
            (_, NodeKind::Symlink(_)) => 0o777,
            (Some(m), _) => m & 0o7777,
            (None, NodeKind::Directory(_)) => 0o777 & !self.umask,
            (None, _) => 0o666 & !self.umask,
        };
        // A new node carries the creator's own login group — the Debian convention of
        // one group per user — unless the parent is setgid, which is exactly what a
        // setgid directory is for: a shared tree where everything lands in one group.
        let group = if self.nodes[&parent].mode & 0o2000 != 0 {
            self.nodes[&parent].group.clone()
        } else {
            owner.to_string()
        };
        let nodes = Arc::make_mut(&mut self.nodes);
        nodes.insert(
            id,
            Inode {
                id,
                kind,
                owner: owner.into(),
                group,
                mode,
                links: 1,
                created: tick,
                accessed: tick,
                modified: tick,
                changed: tick,
            },
        );
        if let NodeKind::Directory(e) = &mut nodes.get_mut(&parent).unwrap().kind {
            e.insert(name, id);
        }
        Ok(id)
    }
    pub fn exists(&self, path: &str) -> bool {
        self.lookup(path, true, 0).is_ok()
    }
    pub fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        let id = self.lookup(path, true, 0)?;
        match &self.nodes[&id].kind {
            NodeKind::File(b) => Ok(b.as_ref().clone()),
            _ => Err(VfsError::IsDirectory(path.into())),
        }
    }
    pub fn read_shared(&self, path: &str) -> Result<Arc<Vec<u8>>, VfsError> {
        let id = self.lookup(path, true, 0)?;
        match &self.nodes[&id].kind {
            NodeKind::File(b) => Ok(b.clone()),
            _ => Err(VfsError::IsDirectory(path.into())),
        }
    }
    pub fn write(
        &mut self,
        path: &str,
        bytes: &[u8],
        owner: &str,
        tick: u64,
    ) -> Result<(), VfsError> {
        self.write_inner(path, bytes, owner, tick, 0)
    }
    fn write_inner(
        &mut self,
        path: &str,
        bytes: &[u8],
        owner: &str,
        tick: u64,
        depth: usize,
    ) -> Result<(), VfsError> {
        if depth > 40 {
            return Err(VfsError::LinkLoop);
        }
        if let Ok(id) = self.lookup(path, false, 0) {
            if let NodeKind::Symlink(target) = &self.nodes[&id].kind {
                let normalized = normalize_path("/", path);
                let parent = normalized.rsplit_once('/').unwrap().0;
                let target = normalize_path(parent, target);
                return self.write_inner(&target, bytes, owner, tick, depth + 1);
            }
        }
        match self.lookup(path, true, 0) {
            Ok(id) => {
                let n = Arc::make_mut(&mut self.nodes).get_mut(&id).unwrap();
                match &mut n.kind {
                    NodeKind::File(b) => {
                        *b = Arc::new(bytes.to_vec());
                        n.modified = tick;
                        n.changed = tick;
                        Ok(())
                    }
                    _ => Err(VfsError::IsDirectory(path.into())),
                }
            }
            Err(VfsError::NotFound(_)) => {
                self.insert(path, NodeKind::File(Arc::new(bytes.to_vec())), owner, tick)?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
    pub fn append(
        &mut self,
        path: &str,
        bytes: &[u8],
        owner: &str,
        tick: u64,
    ) -> Result<(), VfsError> {
        let mut content = match self.read(path) {
            Ok(b) => b,
            Err(VfsError::NotFound(_)) => Vec::new(),
            Err(e) => return Err(e),
        };
        content.extend_from_slice(bytes);
        self.write(path, &content, owner, tick)
    }
    pub fn mkdir(&mut self, path: &str, owner: &str, tick: u64) -> Result<(), VfsError> {
        self.insert(path, NodeKind::Directory(BTreeMap::new()), owner, tick)
            .map(|_| ())
    }
    pub fn mkdir_all(&mut self, path: &str, owner: &str, tick: u64) -> Result<(), VfsError> {
        let normalized = normalize_path("/", path);
        let mut current = String::from("/");
        for part in normalized.split('/').filter(|p| !p.is_empty()) {
            current = normalize_path(&current, part);
            match self.lookup(&current, true, 0) {
                Ok(id) => {
                    if !matches!(self.nodes[&id].kind, NodeKind::Directory(_)) {
                        return Err(VfsError::NotDirectory(current));
                    }
                }
                Err(VfsError::NotFound(_)) => self.mkdir(&current, owner, tick)?,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    pub fn list(&self, path: &str) -> Result<Vec<String>, VfsError> {
        let id = self.lookup(path, true, 0)?;
        match &self.nodes[&id].kind {
            NodeKind::Directory(e) => Ok(e.keys().cloned().collect()),
            _ => Err(VfsError::NotDirectory(path.into())),
        }
    }
    pub fn stat(&self, path: &str) -> Result<Metadata, VfsError> {
        self.metadata(path, true)
    }
    pub fn lstat(&self, path: &str) -> Result<Metadata, VfsError> {
        self.metadata(path, false)
    }
    fn metadata(&self, path: &str, follow: bool) -> Result<Metadata, VfsError> {
        let n = &self.nodes[&self.lookup(path, follow, 0)?];
        // A directory's link count is what `stat` reports: `.`, the entry in its
        // parent, and one `..` per subdirectory.
        let links = match &n.kind {
            NodeKind::Directory(e) => {
                2 + e
                    .values()
                    .filter(|id| matches!(self.nodes[id].kind, NodeKind::Directory(_)))
                    .count() as u32
            }
            _ => n.links,
        };
        Ok(Metadata {
            inode: n.id,
            owner: n.owner.clone(),
            group: n.group.clone(),
            mode: n.mode,
            links,
            size: match &n.kind {
                NodeKind::File(b) => b.len(),
                NodeKind::Directory(e) => e.len(),
                NodeKind::Symlink(s) => s.len(),
            },
            is_dir: matches!(n.kind, NodeKind::Directory(_)),
            is_symlink: matches!(n.kind, NodeKind::Symlink(_)),
            created: n.created,
            accessed: n.accessed,
            modified: n.modified,
            changed: n.changed,
            target: match &n.kind {
                NodeKind::Symlink(t) => Some(t.clone()),
                _ => None,
            },
        })
    }
    pub fn chmod(&mut self, path: &str, mode: u16) -> Result<(), VfsError> {
        let id = self.lookup(path, true, 0)?;
        let n = Arc::make_mut(&mut self.nodes).get_mut(&id).unwrap();
        n.mode = mode & 0o7777;
        Ok(())
    }
    /// `chown`/`chgrp`. Only root may hand a node to another user; the owner may change
    /// its group, because this world models user names rather than membership sets.
    pub fn chown_as(
        &mut self,
        path: &str,
        owner: Option<&str>,
        group: Option<&str>,
        user: &str,
        tick: u64,
        follow: bool,
    ) -> Result<(), VfsError> {
        let current = self.metadata(path, follow)?;
        if owner.is_some_and(|o| o != current.owner) && user != "root" {
            return Err(VfsError::Permission(path.into()));
        }
        if group.is_some() && user != "root" && current.owner != user {
            return Err(VfsError::Permission(path.into()));
        }
        let id = self.lookup(path, follow, 0)?;
        let n = Arc::make_mut(&mut self.nodes).get_mut(&id).unwrap();
        if let Some(o) = owner {
            n.owner = o.into();
        }
        if let Some(g) = group {
            n.group = g.into();
        }
        n.changed = tick;
        Ok(())
    }
    /// `utimensat`: set either timestamp, or both. Write access is the gate, as it is
    /// for `touch` on a file you do not own.
    pub fn set_times_as(
        &mut self,
        path: &str,
        accessed: Option<u64>,
        modified: Option<u64>,
        user: &str,
        tick: u64,
        follow: bool,
    ) -> Result<(), VfsError> {
        self.check_access(path, user, false, true, false)?;
        let id = self.lookup(path, follow, 0)?;
        let n = Arc::make_mut(&mut self.nodes).get_mut(&id).unwrap();
        if let Some(a) = accessed {
            n.accessed = a;
        }
        if let Some(m) = modified {
            n.modified = m;
        }
        n.changed = tick;
        Ok(())
    }
    /// Group membership, without a group database: every user is in the group that
    /// carries their own name and in the shared `users` group.
    pub fn in_group(user: &str, group: &str) -> bool {
        group == user || group == DEFAULT_GROUP
    }
    fn permission_bits(node: &Inode, user: &str) -> u16 {
        if node.owner == user {
            (node.mode >> 6) & 7
        } else if Self::in_group(user, &node.group) {
            (node.mode >> 3) & 7
        } else {
            node.mode & 7
        }
    }
    pub fn check_access(
        &self,
        path: &str,
        user: &str,
        read: bool,
        write: bool,
        execute: bool,
    ) -> Result<(), VfsError> {
        if user == "root" {
            return Ok(());
        }
        let p = normalize_path("/", path);
        let parts: Vec<_> = p.split('/').filter(|s| !s.is_empty()).collect();
        let mut current = String::from("/");
        for part in &parts[..parts.len().saturating_sub(1)] {
            current = normalize_path(&current, part);
            let n = &self.nodes[&self.lookup(&current, true, 0)?];
            if Self::permission_bits(n, user) & 1 == 0 {
                return Err(VfsError::Permission(current));
            }
        }
        let n = &self.nodes[&self.lookup(path, true, 0)?];
        let bits = Self::permission_bits(n, user);
        let need = (if read { 4 } else { 0 })
            | (if write { 2 } else { 0 })
            | (if execute { 1 } else { 0 });
        if bits & need == need {
            Ok(())
        } else {
            Err(VfsError::Permission(path.into()))
        }
    }
    fn check_parent_write(&self, path: &str, user: &str) -> Result<(), VfsError> {
        let p = normalize_path("/", path);
        let parent = p.rsplit_once('/').unwrap().0;
        self.check_access(
            if parent.is_empty() { "/" } else { parent },
            user,
            false,
            true,
            true,
        )
    }
    pub fn list_as(&self, path: &str, user: &str) -> Result<Vec<String>, VfsError> {
        self.check_access(path, user, true, false, true)?;
        self.list(path)
    }
    pub fn append_as(
        &mut self,
        path: &str,
        bytes: &[u8],
        user: &str,
        tick: u64,
    ) -> Result<(), VfsError> {
        if self.exists(path) {
            self.check_access(path, user, false, true, false)?;
        } else {
            self.check_parent_write(path, user)?;
        }
        self.append(path, bytes, user, tick)
    }
    pub fn mkdir_all_as(&mut self, path: &str, user: &str, tick: u64) -> Result<(), VfsError> {
        let normalized = normalize_path("/", path);
        let mut current = String::from("/");
        for part in normalized.split('/').filter(|p| !p.is_empty()) {
            current = normalize_path(&current, part);
            if self.exists(&current) {
                self.check_access(&current, user, false, false, true)?;
                if !self.stat(&current)?.is_dir {
                    return Err(VfsError::NotDirectory(current));
                }
            } else {
                self.check_parent_write(&current, user)?;
                self.mkdir(&current, user, tick)?;
            }
        }
        Ok(())
    }
    pub fn remove_as(&mut self, path: &str, recursive: bool, user: &str) -> Result<(), VfsError> {
        self.check_parent_write(path, user)?;
        let normalized = normalize_path("/", path);
        let parent = normalized.rsplit_once('/').unwrap().0;
        let parent = self.stat(if parent.is_empty() { "/" } else { parent })?;
        let own = self.lstat(path)?;
        if parent.mode & 0o1000 != 0 && user != "root" && parent.owner != user && own.owner != user
        {
            return Err(VfsError::Permission(path.into()));
        }
        self.remove(path, recursive)
    }
    pub fn rename_as(&mut self, from: &str, to: &str, user: &str) -> Result<(), VfsError> {
        self.check_parent_write(from, user)?;
        self.check_parent_write(to, user)?;
        for path in [from, to] {
            if let Ok(own) = self.lstat(path) {
                let normalized = normalize_path("/", path);
                let parent = normalized.rsplit_once('/').unwrap().0;
                let parent = self.stat(if parent.is_empty() { "/" } else { parent })?;
                if parent.mode & 0o1000 != 0
                    && user != "root"
                    && parent.owner != user
                    && own.owner != user
                {
                    return Err(VfsError::Permission(path.into()));
                }
            }
        }
        self.rename(from, to)
    }
    /// Copy a file, or a whole folder tree, as `user`. Every node goes through the same
    /// checks a read and a write would make, so copying is not a way around a mode a
    /// read would have refused. An existing destination is refused rather than merged,
    /// and a folder cannot be copied inside itself.
    pub fn copy_as(&mut self, from: &str, to: &str, user: &str, tick: u64) -> Result<(), VfsError> {
        let (from, to) = (normalize_path("/", from), normalize_path("/", to));
        if self.exists(&to) {
            return Err(VfsError::Exists(to));
        }
        if to == from || to.starts_with(&format!("{}/", from.trim_end_matches('/'))) {
            return Err(VfsError::Invalid(to));
        }
        if !self.lstat(&from)?.is_dir {
            let bytes = self.read_as(&from, user)?;
            return self.write_as(&to, &bytes, user, tick);
        }
        self.mkdir_all_as(&to, user, tick)?;
        for name in self.list_as(&from, user)? {
            self.copy_as(
                &format!("{}/{name}", from.trim_end_matches('/')),
                &format!("{}/{name}", to.trim_end_matches('/')),
                user,
                tick,
            )?;
        }
        Ok(())
    }
    /// `cp -p`: carry mode and timestamps across to the copy. Ownership follows only
    /// for root, exactly as coreutils gives up on `-p` ownership for an ordinary user.
    pub fn copy_attributes_as(&mut self, from: &str, to: &str, user: &str) -> Result<(), VfsError> {
        let source = self.lstat(from)?;
        let id = self.lookup(to, false, 0)?;
        let n = Arc::make_mut(&mut self.nodes).get_mut(&id).unwrap();
        n.mode = source.mode;
        n.accessed = source.accessed;
        n.modified = source.modified;
        n.changed = source.changed;
        if user == "root" {
            n.owner = source.owner;
            n.group = source.group;
        }
        Ok(())
    }
    /// `touch`: the one operation that moves a timestamp without touching the bytes.
    /// Write access is the gate, as it is for a real `utimensat(… UTIME_NOW)`.
    pub fn set_modified_as(&mut self, path: &str, tick: u64, user: &str) -> Result<(), VfsError> {
        self.check_access(path, user, false, true, false)?;
        let id = self.lookup(path, true, 0)?;
        Arc::make_mut(&mut self.nodes)
            .get_mut(&id)
            .unwrap()
            .modified = tick;
        Ok(())
    }
    pub fn chmod_as(&mut self, path: &str, mode: u16, user: &str) -> Result<(), VfsError> {
        if user != "root" && self.stat(path)?.owner != user {
            return Err(VfsError::Permission(path.into()));
        }
        self.chmod(path, mode)
    }
    /// `mkdir -m`: create with an explicit mode instead of `0o777 & !umask`.
    pub fn mkdir_mode_as(
        &mut self,
        path: &str,
        mode: u16,
        user: &str,
        tick: u64,
    ) -> Result<(), VfsError> {
        self.check_parent_write(path, user)?;
        self.insert_with_mode(
            path,
            NodeKind::Directory(BTreeMap::new()),
            user,
            tick,
            Some(mode),
        )
        .map(|_| ())
    }
    /// `truncate -s`: grow with zero bytes or cut, without rewriting the whole file.
    pub fn truncate_as(
        &mut self,
        path: &str,
        size: usize,
        user: &str,
        tick: u64,
    ) -> Result<(), VfsError> {
        if !self.exists(path) {
            return self.write_as(path, &vec![0u8; size], user, tick);
        }
        let mut bytes = self.read_as(path, user)?;
        bytes.resize(size, 0);
        self.write_as(path, &bytes, user, tick)
    }
    pub fn symlink_as(
        &mut self,
        target: &str,
        link: &str,
        user: &str,
        tick: u64,
    ) -> Result<(), VfsError> {
        self.check_parent_write(link, user)?;
        self.symlink(target, link, user, tick)
    }
    pub fn hard_link_as(&mut self, existing: &str, new: &str, user: &str) -> Result<(), VfsError> {
        self.check_access(existing, user, true, false, false)?;
        self.check_parent_write(new, user)?;
        self.hard_link(existing, new)
    }
    pub fn read_as(&self, path: &str, user: &str) -> Result<Vec<u8>, VfsError> {
        self.check_access(path, user, true, false, false)?;
        self.read(path)
    }
    pub fn write_as(
        &mut self,
        path: &str,
        bytes: &[u8],
        user: &str,
        tick: u64,
    ) -> Result<(), VfsError> {
        if self.exists(path) {
            self.check_access(path, user, false, true, false)?
        } else {
            let p = normalize_path("/", path);
            let parent = p.rsplit_once('/').unwrap().0;
            self.check_access(
                if parent.is_empty() { "/" } else { parent },
                user,
                false,
                true,
                true,
            )?
        }
        self.write(path, bytes, user, tick)
    }
    pub fn symlink(
        &mut self,
        target: &str,
        link: &str,
        owner: &str,
        tick: u64,
    ) -> Result<(), VfsError> {
        self.insert(link, NodeKind::Symlink(target.into()), owner, tick)
            .map(|_| ())
    }
    pub fn read_link(&self, path: &str) -> Result<String, VfsError> {
        match &self.nodes[&self.lookup(path, false, 0)?].kind {
            NodeKind::Symlink(t) => Ok(t.clone()),
            _ => Err(VfsError::Invalid(path.into())),
        }
    }
    pub fn hard_link(&mut self, existing: &str, new: &str) -> Result<(), VfsError> {
        let id = self.lookup(existing, true, 0)?;
        if matches!(self.nodes[&id].kind, NodeKind::Directory(_)) {
            return Err(VfsError::IsDirectory(existing.into()));
        }
        let (parent, name) = self.parent(new)?;
        if self.child(parent, &name).is_ok() {
            return Err(VfsError::Exists(new.into()));
        }
        let nodes = Arc::make_mut(&mut self.nodes);
        if let NodeKind::Directory(e) = &mut nodes.get_mut(&parent).unwrap().kind {
            e.insert(name, id);
        } else {
            return Err(VfsError::NotDirectory(new.into()));
        }
        nodes.get_mut(&id).unwrap().links += 1;
        Ok(())
    }
    fn unlink_inode(nodes: &mut BTreeMap<u64, Inode>, id: u64) {
        let n = nodes.get_mut(&id).unwrap();
        n.links -= 1;
        if n.links == 0 {
            let n = nodes.remove(&id).unwrap();
            if let NodeKind::Directory(e) = n.kind {
                for child in e.values() {
                    Self::unlink_inode(nodes, *child)
                }
            }
        }
    }
    pub fn remove(&mut self, path: &str, recursive: bool) -> Result<(), VfsError> {
        let (parent, name) = self.parent(path)?;
        let id = self.child(parent, &name)?;
        if let NodeKind::Directory(e) = &self.nodes[&id].kind {
            if !recursive && !e.is_empty() {
                return Err(VfsError::NotEmpty(path.into()));
            }
        }
        let key = match &self.nodes[&parent].kind {
            NodeKind::Directory(e) => e
                .keys()
                .find(|k| self.key(k) == self.key(&name))
                .unwrap()
                .clone(),
            _ => unreachable!(),
        };
        let nodes = Arc::make_mut(&mut self.nodes);
        if let NodeKind::Directory(e) = &mut nodes.get_mut(&parent).unwrap().kind {
            e.remove(&key);
        }
        Self::unlink_inode(nodes, id);
        Ok(())
    }
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), VfsError> {
        let a = normalize_path("/", from);
        let b = normalize_path("/", to);
        if a == b {
            return Ok(());
        }
        let (old_parent, old_name) = self.parent(&a)?;
        let id = self.child(old_parent, &old_name)?;
        if matches!(self.nodes[&id].kind, NodeKind::Directory(_))
            && self.key(&b).starts_with(&format!("{}/", self.key(&a)))
        {
            return Err(VfsError::Invalid(to.into()));
        }
        let (new_parent, new_name) = self.parent(&b)?;
        fn contains_dir(nodes: &BTreeMap<u64, Inode>, root: u64, needle: u64) -> bool {
            if root == needle {
                return true;
            }
            match &nodes[&root].kind {
                NodeKind::Directory(e) => e.values().any(|id| {
                    matches!(nodes[id].kind, NodeKind::Directory(_))
                        && contains_dir(nodes, *id, needle)
                }),
                _ => false,
            }
        }
        if contains_dir(&self.nodes, id, new_parent) {
            return Err(VfsError::Invalid(to.into()));
        }
        if let Ok(other) = self.child(new_parent, &new_name) {
            if other == id {
                if old_parent == new_parent
                    && old_name != new_name
                    && self.key(&old_name) == self.key(&new_name)
                {
                    let old_key = match &self.nodes[&old_parent].kind {
                        NodeKind::Directory(e) => e
                            .keys()
                            .find(|k| self.key(k) == self.key(&old_name))
                            .unwrap()
                            .clone(),
                        _ => unreachable!(),
                    };
                    if let NodeKind::Directory(e) = &mut Arc::make_mut(&mut self.nodes)
                        .get_mut(&old_parent)
                        .unwrap()
                        .kind
                    {
                        e.remove(&old_key);
                        e.insert(new_name, id);
                    }
                }
                return Ok(());
            }
            let src_dir = matches!(self.nodes[&id].kind, NodeKind::Directory(_));
            let dst_dir = matches!(self.nodes[&other].kind, NodeKind::Directory(_));
            if src_dir != dst_dir {
                return Err(VfsError::Invalid(to.into()));
            }
            self.remove(&b, false)?
        }
        let key = match &self.nodes[&old_parent].kind {
            NodeKind::Directory(e) => e
                .keys()
                .find(|k| self.key(k) == self.key(&old_name))
                .unwrap()
                .clone(),
            _ => unreachable!(),
        };
        let nodes = Arc::make_mut(&mut self.nodes);
        if let NodeKind::Directory(e) = &mut nodes.get_mut(&old_parent).unwrap().kind {
            e.remove(&key);
        }
        if let NodeKind::Directory(e) = &mut nodes.get_mut(&new_parent).unwrap().kind {
            e.insert(new_name, id);
        }
        Ok(())
    }
    pub fn all_files(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(
            v: &Vfs,
            id: u64,
            path: String,
            out: &mut BTreeMap<String, Vec<u8>>,
            seen: &mut BTreeSet<u64>,
        ) {
            match &v.nodes[&id].kind {
                NodeKind::File(b) => {
                    out.insert(path, b.as_ref().clone());
                }
                NodeKind::Directory(e) => {
                    if !seen.insert(id) {
                        return;
                    }
                    for (name, child) in e {
                        walk(v, *child, normalize_path(&path, name), out, seen)
                    }
                }
                NodeKind::Symlink(_) => {}
            }
        }
        let mut out = BTreeMap::new();
        walk(self, 1, "/".into(), &mut out, &mut BTreeSet::new());
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_rename_and_fork() {
        let mut v = Vfs::new(true);
        v.mkdir_all("/a/b", "u", 0).unwrap();
        v.write("/a/b/f", &[0, 255], "u", 1).unwrap();
        v.hard_link("/a/b/f", "/a/h").unwrap();
        let fork = v.clone();
        v.rename("/a/b/f", "/a/b/g").unwrap();
        v.append("/a/h", &[1], "u", 2).unwrap();
        assert_eq!(v.read("/a/b/g").unwrap(), [0, 255, 1]);
        assert_eq!(fork.read("/a/b/f").unwrap(), [0, 255]);
        v.symlink("b/g", "/a/s", "u", 2).unwrap();
        assert_eq!(v.read("/a/s").unwrap(), [0, 255, 1]);
        v.remove("/a/b", true).unwrap();
        assert_eq!(v.stat("/a/h").unwrap().links, 1);
    }
    #[test]
    fn windows_case_and_cycles() {
        let mut v = Vfs::new(false);
        v.mkdir_all("C:\\Users\\Alice", "alice", 0).unwrap();
        v.write("C:\\Users\\Alice\\Hi", b"hi", "alice", 0).unwrap();
        assert_eq!(v.read("c:/users/alice/hi").unwrap(), b"hi");
        v.symlink("/loop", "/loop", "u", 0).unwrap();
        assert_eq!(v.read("/loop"), Err(VfsError::LinkLoop));
    }
    #[test]
    fn dangling_write_and_symlink_cycle_rename() {
        let mut v = Vfs::new(true);
        v.symlink("/target", "/link", "u", 0).unwrap();
        v.write("/link", b"x", "u", 1).unwrap();
        assert_eq!(v.read("/target").unwrap(), b"x");
        v.mkdir_all("/tree/child", "u", 0).unwrap();
        v.symlink("/tree/child", "/alias", "u", 0).unwrap();
        assert!(v.rename("/tree", "/alias/tree").is_err());
    }
    #[test]
    fn permissions_and_restore() {
        let mut v = Vfs::new(true);
        v.write("/secret", b"x", "alice", 0).unwrap();
        v.chmod("/secret", 0o600).unwrap();
        assert!(v.read_as("/secret", "bob").is_err());
        assert!(v.read_as("/secret", "alice").is_ok());
        let encoded = serde_json::to_string(&v).unwrap();
        assert_eq!(v, serde_json::from_str(&encoded).unwrap());
    }
    proptest::proptest! {#[test]fn normalization_idempotent(s in "[a-z./]{0,80}"){let n=normalize_path("/home/a",&s);proptest::prop_assert_eq!(normalize_path("/",&n),n);}}
}
#[cfg(test)]
mod authorization_tests {
    use super::*;
    #[test]
    fn checked_mutations_and_sticky_directory() {
        let mut v = Vfs::new(true);
        v.mkdir_all("/tmp", "root", 0).unwrap();
        v.chmod("/tmp", 0o1777).unwrap();
        v.write_as("/tmp/alice", b"secret", "alice", 0).unwrap();
        v.chmod_as("/tmp/alice", 0o600, "alice").unwrap();
        assert!(v.read_as("/tmp/alice", "bob").is_err());
        assert!(v.remove_as("/tmp/alice", false, "bob").is_err());
        assert!(v.rename_as("/tmp/alice", "/tmp/stolen", "bob").is_err());
        assert!(v.chmod_as("/tmp/alice", 0o777, "bob").is_err());
        v.remove_as("/tmp/alice", false, "alice").unwrap();
    }
    #[test]
    fn case_only_rename_updates_display_name() {
        let mut v = Vfs::new(false);
        v.write("/hello", b"x", "u", 0).unwrap();
        let id = v.stat("/hello").unwrap().inode;
        v.rename("/hello", "/HELLO").unwrap();
        assert_eq!(v.list("/").unwrap(), ["HELLO"]);
        assert_eq!(v.stat("/hello").unwrap().inode, id);
    }
}
#[cfg(test)]
mod metadata_tests {
    use super::*;
    #[test]
    fn three_timestamps_move_independently() {
        let mut v = Vfs::new(true);
        v.write("/f", b"a", "u", 10).unwrap();
        let m = v.stat("/f").unwrap();
        assert_eq!(
            (m.created, m.accessed, m.modified, m.changed),
            (10, 10, 10, 10)
        );
        // A write moves modification and change, and leaves access where it was.
        v.write("/f", b"ab", "u", 20).unwrap();
        let m = v.stat("/f").unwrap();
        assert_eq!(
            (m.created, m.accessed, m.modified, m.changed),
            (10, 10, 20, 20)
        );
        // A chown moves change only.
        v.chown_as("/f", None, Some("staff"), "root", 30, true)
            .unwrap();
        let m = v.stat("/f").unwrap();
        assert_eq!((m.accessed, m.modified, m.changed), (10, 20, 30));
        assert_eq!(m.group, "staff");
        // `touch -a` moves access only.
        v.set_times_as("/f", Some(40), None, "u", 40, true).unwrap();
        let m = v.stat("/f").unwrap();
        assert_eq!((m.accessed, m.modified), (40, 20));
    }
    #[test]
    fn identity_targets_and_link_counts_are_reported() {
        let mut v = Vfs::new(true);
        v.mkdir_all("/d/sub1", "u", 0).unwrap();
        v.mkdir_all("/d/sub2", "u", 0).unwrap();
        v.write("/d/f", b"x", "u", 0).unwrap();
        // A directory reports `.`, its parent's entry and one `..` per subdirectory.
        assert_eq!(v.stat("/d").unwrap().links, 4);
        assert_eq!(v.stat("/d/sub1").unwrap().links, 2);
        // A hard link shares the inode and raises the count.
        v.hard_link("/d/f", "/d/g").unwrap();
        let (a, b) = (v.stat("/d/f").unwrap(), v.stat("/d/g").unwrap());
        assert_eq!(a.inode, b.inode);
        assert_eq!(a.links, 2);
        assert_ne!(a.inode, v.stat("/d").unwrap().inode);
        // A symlink carries its target, and lstat describes the link itself.
        v.symlink("/d/f", "/link", "u", 0).unwrap();
        assert_eq!(v.lstat("/link").unwrap().target.as_deref(), Some("/d/f"));
        assert!(v.lstat("/link").unwrap().is_symlink);
        assert!(v.stat("/link").unwrap().target.is_none());
        assert!(v.validate().is_ok());
    }
    #[test]
    fn the_umask_decides_a_new_nodes_mode() {
        let mut v = Vfs::new(true);
        v.chmod("/", 0o777).unwrap();
        assert_eq!(v.umask(), 0o022);
        v.write("/a", b"", "u", 0).unwrap();
        v.mkdir("/da", "u", 0).unwrap();
        assert_eq!(v.stat("/a").unwrap().mode, 0o644);
        assert_eq!(v.stat("/da").unwrap().mode, 0o755);
        v.set_umask(0o077);
        v.write("/b", b"", "u", 0).unwrap();
        v.mkdir("/db", "u", 0).unwrap();
        assert_eq!(v.stat("/b").unwrap().mode, 0o600);
        assert_eq!(v.stat("/db").unwrap().mode, 0o700);
        // `mkdir -m` overrides it outright.
        v.mkdir_mode_as("/dc", 0o2750, "u", 0).unwrap();
        assert_eq!(v.stat("/dc").unwrap().mode, 0o2750);
        // A setgid directory hands its group to what is created inside it.
        v.chown_as("/dc", None, Some("shared"), "root", 0, true)
            .unwrap();
        v.write("/dc/inner", b"", "u", 0).unwrap();
        assert_eq!(v.stat("/dc/inner").unwrap().group, "shared");
        // Elsewhere a node carries its creator's own login group.
        assert_eq!(v.stat("/b").unwrap().group, "u");
    }
    #[test]
    fn group_bits_sit_between_owner_and_other() {
        let mut v = Vfs::new(true);
        v.chmod("/", 0o777).unwrap();
        v.write("/shared", b"x", "alice", 0).unwrap();
        v.chown_as("/shared", None, Some(DEFAULT_GROUP), "root", 0, true)
            .unwrap();
        v.chmod("/shared", 0o640).unwrap();
        // Everyone is in `users`, so the group bits decide for bob, not the other bits.
        assert!(v.read_as("/shared", "bob").is_ok());
        assert!(v
            .check_access("/shared", "bob", false, true, false)
            .is_err());
        v.chown_as("/shared", None, Some("alice"), "root", 0, true)
            .unwrap();
        // Now bob is outside the group and the other bits, which are zero, decide.
        assert!(v.read_as("/shared", "bob").is_err());
        assert!(v.read_as("/shared", "alice").is_ok());
        // Only root hands a file to someone else; the owner may still set its group.
        assert!(v
            .chown_as("/shared", Some("bob"), None, "alice", 0, true)
            .is_err());
        assert!(v
            .chown_as("/shared", None, Some("dev"), "alice", 0, true)
            .is_ok());
        assert!(v
            .chown_as("/shared", Some("bob"), None, "root", 0, true)
            .is_ok());
    }
    #[test]
    fn copying_attributes_carries_mode_and_times_but_not_ownership() {
        let mut v = Vfs::new(true);
        v.chmod("/", 0o777).unwrap();
        v.write("/src", b"x", "alice", 5).unwrap();
        v.chmod("/src", 0o641).unwrap();
        v.set_times_as("/src", Some(7), Some(9), "alice", 9, true)
            .unwrap();
        v.write_as("/dst", b"x", "bob", 100).unwrap();
        v.copy_attributes_as("/src", "/dst", "bob").unwrap();
        let m = v.lstat("/dst").unwrap();
        assert_eq!((m.mode, m.accessed, m.modified), (0o641, 7, 9));
        assert_eq!(m.owner, "bob", "an ordinary user cannot give a file away");
        v.write_as("/dst2", b"x", "bob", 100).unwrap();
        v.copy_attributes_as("/src", "/dst2", "root").unwrap();
        assert_eq!(v.lstat("/dst2").unwrap().owner, "alice");
    }
    #[test]
    fn truncate_grows_with_zeroes_and_cuts() {
        let mut v = Vfs::new(true);
        v.chmod("/", 0o777).unwrap();
        v.truncate_as("/t", 3, "u", 0).unwrap();
        assert_eq!(v.read("/t").unwrap(), [0, 0, 0]);
        v.write("/t", b"abcdef", "u", 1).unwrap();
        v.truncate_as("/t", 2, "u", 2).unwrap();
        assert_eq!(v.read("/t").unwrap(), b"ab");
        v.truncate_as("/t", 4, "u", 3).unwrap();
        assert_eq!(v.read("/t").unwrap(), [b'a', b'b', 0, 0]);
    }
}
#[cfg(test)]
mod checkpoint_tests {
    use super::*;
    #[test]
    fn rejects_missing_inode_in_checkpoint() {
        let mut v = Vfs::new(true);
        v.write("/file", b"a", "u", 0).unwrap();
        assert!(v.validate().is_ok());
        let mut value = serde_json::to_value(v).unwrap();
        value["nodes"].as_object_mut().unwrap().remove("2");
        let broken: Vfs = serde_json::from_value(value).unwrap();
        assert!(broken.validate().is_err());
    }
}
