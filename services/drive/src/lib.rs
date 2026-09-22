//! Cloud storage: drive.google.com and dropbox.com. One flat node map with parent links, so
//! a share is a set insertion that another actor sees on their very next request.
mod skin;
use cw_protocol::{HttpRequest, HttpResponse, PageTheme, Result as SimResult, SimError};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
/// Skins this instance may wear; `plain` is the flat rendering every drive had before a brand.
pub const SKINS: &[&str] = &["plain", "gdrive", "dropbox"];
/// The reserved parent a deleted node is moved under. It is not a stored node: nothing may be
/// filed *into* the trash by hand, and a hard delete would lose data a snapshot should still hold.
pub const TRASH: &str = "trash";
/// Deep enough for any plausible tree, shallow enough that a seeded cycle cannot hang a render.
const MAX_DEPTH: usize = 32;
/// A folder wider than this is a seeding mistake; drawing it would bury the page in cards.
const MAX_CHILDREN: usize = 240;
/// Documented seed keys whose container type is checked before the state is deserialised.
const OBJECTS: &[&str] = &["theme", "nodes", "starred"];
fn default_theme(theme: &PageTheme) -> bool {
    *theme == PageTheme::default()
}
fn is_zero(n: &u64) -> bool {
    *n == 0
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    #[default]
    Folder,
    File,
    /// A pointer at a page elsewhere in the world — how a drive holds a Google doc.
    Shortcut,
}
impl NodeKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::File => "File",
            Self::Shortcut => "Shortcut",
        }
    }
    /// Folders sort ahead of their contents, the way every file browser has always done it.
    fn rank(&self) -> u8 {
        match self {
            Self::Folder => 0,
            Self::Shortcut => 1,
            Self::File => 2,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    /// `None` only for the root; everything else is filed somewhere, including in the trash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub owner: String,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub shared_with: BTreeSet<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mime: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub size: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content: String,
    /// The public token minted by "create a share link"; empty until someone asks for one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub link: String,
    pub tick: u64,
}
impl Node {
    /// Human size, in integers: a float here would be a determinism hazard for no gain.
    pub fn size_text(&self) -> String {
        match self.size {
            0 => "—".into(),
            n if n < 1024 => format!("{n} bytes"),
            n => format!("{}.{} KB", n / 1024, (n % 1024) * 10 / 1024),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DriveState {
    /// Presentation only; the domain logic below is identical under all three skins.
    #[serde(skip_serializing_if = "web::Skin::is_plain")]
    pub skin: web::Skin,
    #[serde(skip_serializing_if = "default_theme")]
    pub theme: PageTheme,
    pub brand: String,
    pub root: String,
    pub nodes: BTreeMap<String, Node>,
    /// Actor to node ids. A star is one reader's opinion, so it lives outside the shared node.
    pub starred: BTreeMap<String, BTreeSet<String>>,
    pub next_id: u64,
}
/// A deterministic public token for a share link: FNV-1a over the id and the minting tick.
fn token(id: &str, tick: u64) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes().chain(tick.to_le_bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{:012x}", h & 0xffff_ffff_ffff)
}
/// How a listing is filtered and ordered. The Drive filter chips and the Dropbox column
/// headers set these on the listing route they already stand on, so no new route is involved
/// and an unsifted request renders exactly what it always did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sift {
    /// `folders` or `files` keeps that kind alone; anything else keeps everything.
    kind: String,
    /// `mine` keeps what this actor owns.
    people: String,
    /// `modified` orders newest first; otherwise folders first, then by name.
    sort: String,
}
impl Sift {
    fn read(r: &HttpRequest) -> Sift {
        Sift {
            kind: web::query(r, "type").unwrap_or_default(),
            people: web::query(r, "people").unwrap_or_default(),
            sort: web::query(r, "sort").unwrap_or_default(),
        }
    }
    pub(crate) fn kind(&self) -> &str {
        &self.kind
    }
    pub(crate) fn people(&self) -> &str {
        &self.people
    }
    pub(crate) fn sort(&self) -> &str {
        &self.sort
    }
    /// Whether anything is hiding rows, which is the difference between "empty" and "no match".
    pub(crate) fn filters(&self) -> bool {
        self.kind == "folders" || self.kind == "files" || self.people == "mine"
    }
}
impl DriveState {
    pub fn brand(&self) -> &str {
        if self.brand.is_empty() {
            "Drive"
        } else {
            &self.brand
        }
    }
    pub fn root_id(&self) -> &str {
        if self.root.is_empty() {
            "root"
        } else {
            &self.root
        }
    }
    /// Owned by, or shared directly with, this actor. The one rule every other check is built on.
    fn granted(&self, actor: &str, node: &Node) -> bool {
        node.owner == actor || node.shared_with.contains(actor)
    }
    /// Walk parents looking for `ancestor`; also how "is this in the trash" is answered.
    fn under(&self, id: &str, ancestor: &str) -> bool {
        let mut at = self.nodes.get(id).and_then(|n| n.parent.clone());
        for _ in 0..MAX_DEPTH {
            match at {
                Some(p) if p == ancestor => return true,
                Some(p) => at = self.nodes.get(&p).and_then(|n| n.parent.clone()),
                None => return false,
            }
        }
        false
    }
    pub fn trashed(&self, id: &str) -> bool {
        self.under(id, TRASH)
    }
    /// A leaf is visible when it is granted; a folder is visible when anything inside it is,
    /// so a share deep in a tree opens exactly the path to itself and nothing else.
    pub fn visible(&self, actor: &str, id: &str) -> bool {
        let Some(node) = self.nodes.get(id) else {
            return false;
        };
        self.granted(actor, node)
            || (node.kind == NodeKind::Folder
                && self
                    .nodes
                    .values()
                    .any(|n| self.granted(actor, n) && self.under(&n.id, id)))
    }
    pub fn read(&self, actor: &str, id: &str) -> Result<&Node, String> {
        self.nodes
            .get(id)
            .filter(|_| self.visible(actor, id))
            .ok_or("node unavailable".into())
    }
    /// Only a direct grant carries the right to change a node or to file something inside it;
    /// seeing a folder because one file in it was shared must not hand over the whole tree.
    fn editable(&self, actor: &str, id: &str) -> Result<&Node, String> {
        self.nodes
            .get(id)
            .filter(|n| self.granted(actor, n))
            .ok_or("node not writable".into())
    }
    /// Whether this actor holds the direct grant the mutating routes all require.
    pub fn granted_to(&self, actor: &str, id: &str) -> bool {
        self.editable(actor, id).is_ok()
    }
    pub fn children(&self, actor: &str, folder: &str) -> Vec<&Node> {
        let mut kids: Vec<_> = self
            .nodes
            .values()
            .filter(|n| {
                n.parent.as_deref() == Some(folder)
                    && self.visible(actor, &n.id)
                    && !self.trashed(&n.id)
            })
            .collect();
        kids.sort_by(|a, b| (a.kind.rank(), &a.name, &a.id).cmp(&(b.kind.rank(), &b.name, &b.id)));
        kids
    }
    /// Everything granted to this actor that someone else owns — the "Shared with me" screen.
    pub fn shared_with_me(&self, actor: &str) -> Vec<&Node> {
        self.sorted(|n| n.owner != actor && n.shared_with.contains(actor))
    }
    pub fn starred(&self, actor: &str) -> Vec<&Node> {
        let ids = self.starred.get(actor);
        self.sorted(|n| ids.is_some_and(|s| s.contains(&n.id)) && self.visible(actor, &n.id))
    }
    pub fn is_starred(&self, actor: &str, id: &str) -> bool {
        self.starred.get(actor).is_some_and(|s| s.contains(id))
    }
    pub fn trash(&self, actor: &str) -> Vec<&Node> {
        self.sorted(|n| n.parent.as_deref() == Some(TRASH) && self.granted(actor, n))
    }
    pub fn search(&self, actor: &str, query: &str) -> Vec<&Node> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return vec![];
        }
        self.sorted(|n| {
            self.visible(actor, &n.id)
                && !self.trashed(&n.id)
                && (n.name.to_lowercase().contains(&q) || n.content.to_lowercase().contains(&q))
        })
    }
    /// Narrow and order a listing the way the chips and column headers ask. Unset fields
    /// leave the list exactly as the screen built it.
    pub(crate) fn arrange<'a>(
        &self,
        actor: &str,
        mut nodes: Vec<&'a Node>,
        sift: &Sift,
    ) -> Vec<&'a Node> {
        match sift.kind() {
            "folders" => nodes.retain(|n| n.kind == NodeKind::Folder),
            "files" => nodes.retain(|n| n.kind != NodeKind::Folder),
            _ => (),
        }
        if sift.people() == "mine" {
            nodes.retain(|n| n.owner == actor);
        }
        if sift.sort() == "modified" {
            nodes.sort_by(|a, b| (b.tick, &a.name, &a.id).cmp(&(a.tick, &b.name, &b.id)));
        }
        nodes
    }
    fn sorted(&self, keep: impl Fn(&Node) -> bool) -> Vec<&Node> {
        let mut found: Vec<_> = self.nodes.values().filter(|n| keep(n)).collect();
        found.sort_by(|a, b| (a.kind.rank(), &a.name, &a.id).cmp(&(b.kind.rank(), &b.name, &b.id)));
        found
    }
    /// Breadcrumb from the root down to `id`, stopping at whatever the walk can still see.
    pub fn path_to(&self, actor: &str, id: &str) -> Vec<&Node> {
        let mut chain = vec![];
        let mut at = Some(id.to_owned());
        for _ in 0..MAX_DEPTH {
            let Some(current) = at else { break };
            let Some(node) = self
                .nodes
                .get(&current)
                .filter(|_| self.visible(actor, &current))
            else {
                break;
            };
            chain.push(node);
            at = node.parent.clone();
        }
        chain.reverse();
        chain
    }
    fn fresh_id(&mut self, prefix: &str) -> Result<String, String> {
        for _ in 0..1024 {
            self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
            let id = format!("{prefix}-{}", self.next_id);
            if !self.nodes.contains_key(&id) {
                return Ok(id);
            }
        }
        Err("ID space exhausted".into())
    }
    /// Two files of the same name in one folder would make the Dropbox/Drive parity check a lie.
    fn name_free(&self, parent: &str, name: &str, except: &str) -> Result<(), String> {
        let taken = self.nodes.values().any(|n| {
            n.id != except
                && n.parent.as_deref() == Some(parent)
                && n.name.eq_ignore_ascii_case(name)
        });
        if taken {
            Err(format!("{name} already exists in that folder"))
        } else {
            Ok(())
        }
    }
    fn place(&self, actor: &str, parent: &str, name: &str, except: &str) -> Result<(), String> {
        let folder = self.editable(actor, parent)?;
        if folder.kind != NodeKind::Folder {
            return Err("parent is not a folder".into());
        }
        if name.trim().is_empty() {
            return Err("name required".into());
        }
        if self.children(actor, parent).len() >= MAX_CHILDREN {
            return Err("folder is full".into());
        }
        self.name_free(parent, name.trim(), except)
    }
    pub fn create_folder(
        &mut self,
        actor: &str,
        name: &str,
        parent: &str,
        tick: u64,
    ) -> Result<Node, String> {
        self.place(actor, parent, name, "")?;
        let id = self.fresh_id("folder")?;
        Ok(self.insert(Node {
            id,
            kind: NodeKind::Folder,
            name: name.trim().into(),
            parent: Some(parent.into()),
            owner: actor.into(),
            tick,
            ..Node::default()
        }))
    }
    /// The "upload": content arrives as text, because a text file is the only file a
    /// deterministic world can honestly claim to hold.
    pub fn upload(
        &mut self,
        actor: &str,
        name: &str,
        parent: &str,
        content: &str,
        tick: u64,
    ) -> Result<Node, String> {
        self.place(actor, parent, name, "")?;
        let id = self.fresh_id("file")?;
        Ok(self.insert(Node {
            id,
            kind: NodeKind::File,
            name: name.trim().into(),
            parent: Some(parent.into()),
            owner: actor.into(),
            mime: "text/plain".into(),
            size: content.len() as u64,
            content: content.into(),
            tick,
            ..Node::default()
        }))
    }
    fn insert(&mut self, node: Node) -> Node {
        self.nodes.insert(node.id.clone(), node.clone());
        node
    }
    /// Rename, move, or both. An empty field leaves that half of the node alone.
    pub fn relocate(
        &mut self,
        actor: &str,
        id: &str,
        name: &str,
        parent: &str,
    ) -> Result<Node, String> {
        let node = self.editable(actor, id)?.clone();
        if node.parent.is_none() {
            return Err("the root cannot be renamed or moved".into());
        }
        let name = if name.trim().is_empty() {
            node.name.clone()
        } else {
            name.trim().to_owned()
        };
        let parent = if parent.trim().is_empty() {
            node.parent.clone().unwrap_or_default()
        } else {
            parent.trim().to_owned()
        };
        // A node filed inside itself would be unreachable and would make `under` loop forever.
        if parent == id || self.under(&parent, id) {
            return Err("a folder cannot contain itself".into());
        }
        self.place(actor, &parent, &name, id)?;
        let node = self
            .nodes
            .get_mut(id)
            .ok_or::<String>("node vanished".into())?;
        node.name = name;
        node.parent = Some(parent);
        Ok(node.clone())
    }
    /// Sharing is the cross-actor move: `with` sees the node on their very next request.
    pub fn share(&mut self, actor: &str, id: &str, with: &str) -> Result<Node, String> {
        let with = with.trim();
        if with.is_empty() {
            return Err("who to share with is required".into());
        }
        let node = self.nodes.get_mut(id).filter(|n| n.owner == actor);
        let node = node.ok_or::<String>("not writable: only the owner may share".into())?;
        if node.owner == with {
            return Err("the owner already has it".into());
        }
        node.shared_with.insert(with.into());
        Ok(node.clone())
    }
    pub fn star(&mut self, actor: &str, id: &str) -> Result<bool, String> {
        self.read(actor, id)?;
        let mine = self.starred.entry(actor.to_owned()).or_default();
        let on = mine.insert(id.to_owned());
        if !on {
            mine.remove(id);
        }
        if mine.is_empty() {
            self.starred.remove(actor);
        }
        Ok(on)
    }
    /// Deleting files a node under the reserved trash parent; nothing is ever dropped.
    pub fn delete(&mut self, actor: &str, id: &str) -> Result<Node, String> {
        let node = self.editable(actor, id)?.clone();
        if node.parent.is_none() {
            return Err("the root cannot be deleted".into());
        }
        if node.parent.as_deref() == Some(TRASH) {
            return Err("already in the trash".into());
        }
        let node = self
            .nodes
            .get_mut(id)
            .ok_or::<String>("node vanished".into())?;
        node.parent = Some(TRASH.into());
        Ok(node.clone())
    }
    /// Mint (or re-read) the public link. Anyone holding the token may open it — that is the
    /// whole point of a share link, and it is the one route that ignores the visibility rule.
    pub fn mint_link(&mut self, actor: &str, id: &str, tick: u64) -> Result<Node, String> {
        self.editable(actor, id)?;
        let link = token(id, tick);
        let node = self
            .nodes
            .get_mut(id)
            .ok_or::<String>("node vanished".into())?;
        if node.link.is_empty() {
            node.link = link;
        }
        Ok(node.clone())
    }
    pub fn by_link(&self, link: &str) -> Option<&Node> {
        self.nodes
            .values()
            .find(|n| !n.link.is_empty() && n.link == link)
    }
}
pub struct DriveService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(DriveService)
}
/// Which screen was asked for; the skins are three readings of the same seven.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Screen<'a> {
    Folder(&'a str),
    File(&'a str),
    SharedWithMe,
    Starred,
    Trash,
    Search(&'a str),
    Link(&'a str),
}
/// The unskinned rendering: one flat column of links and forms, no palette, no chrome.
fn view(s: &DriveState, actor: &str, screen: Screen, sift: &Sift) -> SimResult<HttpResponse> {
    let brand = s.brand().to_owned();
    let listing = |title: &str, nodes: Vec<&Node>| {
        let mut e = vec![web::heading("title", title)];
        for n in s.arrange(actor, nodes, sift) {
            e.push(web::link(
                &format!("item-{}", n.id),
                format!("{} · {}", n.name, n.kind.label()),
                match n.kind {
                    NodeKind::Folder => format!("/drive/folders/{}", n.id),
                    _ => format!("/file/{}", n.id),
                },
            ));
        }
        e
    };
    let mut e = match screen {
        Screen::SharedWithMe => listing("Shared with me", s.shared_with_me(actor)),
        Screen::Starred => listing("Starred", s.starred(actor)),
        Screen::Trash => listing("Trash", s.trash(actor)),
        // Nothing was asked for, so there is nothing to report: "Results for " is not a heading.
        Screen::Search(q) if q.trim().is_empty() => vec![
            web::heading("title", "Search"),
            web::paragraph(
                "hint",
                "Type a name, or a word from a file, in the box above.",
            ),
        ],
        Screen::Search(q) => listing(&format!("Results for {q}"), s.search(actor, q)),
        Screen::Folder(id) => {
            // A drive with nothing in it is an empty drive, not a forbidden one: the root
            // exists by definition even before anything has been filed in it.
            let name = match s.read(actor, id) {
                Ok(folder) => folder.name.clone(),
                Err(_) if id == s.root_id() => "My Drive".to_owned(),
                Err(e) => return web::error(if s.nodes.contains_key(id) { 403 } else { 404 }, e),
            };
            let mut e = listing(&name, s.children(actor, id));
            // Filing here takes a direct grant, so without one the forms are not offered:
            // a form whose only possible answer is 403 is not a control.
            if s.granted_to(actor, id) {
                e.push(web::form(
                    "folder",
                    "/folders",
                    &[("name", "Folder name", ""), ("parent", "Parent", id)],
                ));
                e.push(web::form(
                    "upload",
                    "/files",
                    &[
                        ("name", "File name", ""),
                        ("parent", "Parent", id),
                        ("content", "Contents", ""),
                    ],
                ));
            }
            e
        }
        Screen::File(id) | Screen::Link(id) => {
            let node = match screen {
                Screen::Link(link) => match s.by_link(link) {
                    Some(n) => n,
                    None => return web::error(404, "no such share link"),
                },
                _ => match s.read(actor, id) {
                    Ok(n) => n,
                    Err(e) => return web::error(403, e),
                },
            };
            let mut e = vec![
                web::heading("title", &node.name),
                web::paragraph(
                    "meta",
                    format!(
                        "{} · owner {} · {}",
                        node.kind.label(),
                        node.owner,
                        node.size_text()
                    ),
                ),
            ];
            if !node.target_url.is_empty() {
                e.push(web::link("target", &node.target_url, &node.target_url));
            }
            if !node.content.is_empty() {
                e.push(web::paragraph("content", &node.content));
                e.extend(web::links("content", &node.content));
            }
            // Only the owner may share, so only the owner is shown the form.
            if matches!(screen, Screen::File(_)) && node.owner == actor {
                e.push(web::form(
                    "share",
                    &format!("/nodes/{}/share", node.id),
                    &[("actor", "Share with", "")],
                ));
            }
            e
        }
    };
    // The chrome the plain skin can honestly carry: the drive itself, the screens beside it,
    // and the box that searches it. The screen one is already on is not offered again.
    let at_root = matches!(screen, Screen::Folder(id) if id == s.root_id());
    let mut chrome = vec![];
    if !at_root {
        chrome.push(web::link("home", &brand, "/"));
    }
    for (id, text, url, here) in [
        (
            "nav-shared",
            "Shared with me",
            "/shared-with-me",
            matches!(screen, Screen::SharedWithMe),
        ),
        (
            "nav-starred",
            "Starred",
            "/starred",
            matches!(screen, Screen::Starred),
        ),
        (
            "nav-trash",
            "Trash",
            "/trash",
            matches!(screen, Screen::Trash),
        ),
    ] {
        if !here {
            chrome.push(web::link(id, text, url));
        }
    }
    let query = match screen {
        Screen::Search(q) => q,
        _ => "",
    };
    chrome.push(web::form(
        "find",
        "/search",
        &[("q", "Search files", query)],
    ));
    e.splice(0..0, chrome);
    web::page(&brand, e)
}
fn render(s: &DriveState, actor: &str, screen: Screen, sift: &Sift) -> SimResult<HttpResponse> {
    if s.skin.is_plain() {
        view(s, actor, screen, sift)
    } else {
        skin::view(s, actor, screen, sift)
    }
}
impl Service for DriveService {
    fn kind(&self) -> &str {
        "drive"
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
    fn initialize(&self, initial: Value, _: &ServiceContext) -> SimResult<Value> {
        // A struct also deserialises from a JSON array, so the object check has to be explicit.
        let mut s: DriveState = web::load(&web::shape(initial, OBJECTS, &[])?)?;
        s.skin.check(SKINS)?;
        let root = s.root_id().to_owned();
        let ids: BTreeSet<_> = s.nodes.keys().cloned().collect();
        for (id, n) in &mut s.nodes {
            if n.id.is_empty() {
                n.id = id.clone();
            }
            if n.name.is_empty() {
                n.name = id.clone();
            }
            if n.kind == NodeKind::File && n.size == 0 {
                n.size = n.content.len() as u64;
            }
            // A dangling parent silently hides a whole subtree, so it is a load-time error.
            match n.parent.as_deref() {
                None if *id != root => {
                    return Err(SimError::invalid(format!("{id} has no parent")));
                }
                Some(p) if p != TRASH && !ids.contains(p) => {
                    return Err(SimError::invalid(format!("{id} names unknown parent {p}")));
                }
                _ => (),
            }
        }
        if !s.nodes.is_empty() && !s.nodes.contains_key(&root) {
            return Err(SimError::invalid(format!("root {root} is not a node")));
        }
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<HttpResponse> {
        let mut s: DriveState = web::load(state)?;
        let p = web::path(r);
        let path = p.strip_prefix("/api").unwrap_or(&p);
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let api = p.starts_with("/api/");
        let method = r.method.to_ascii_uppercase();
        let root = s.root_id().to_owned();
        // The chips and the column headers set these on the route they are already on.
        let sift = Sift::read(r);
        if method == "GET" {
            let query = web::query(r, "q").unwrap_or_default();
            return match parts.as_slice() {
                [""] => render(&s, &c.actor, Screen::Folder(&root), &sift),
                ["drive", "folders", id] if !api => render(&s, &c.actor, Screen::Folder(id), &sift),
                ["drive", "folders", id] => match s.read(&c.actor, id) {
                    Ok(_) => HttpResponse::json(200, &s.children(&c.actor, id)),
                    Err(e) => web::error(403, e),
                },
                ["file", id] if !api => render(&s, &c.actor, Screen::File(id), &sift),
                ["nodes"] => HttpResponse::json(
                    200,
                    &s.sorted(|n| s.visible(&c.actor, &n.id) && !s.trashed(&n.id)),
                ),
                ["file", id] | ["nodes", id] => web::domain(s.read(&c.actor, id).map(|n| json!(n))),
                ["shared-with-me"] if !api => render(&s, &c.actor, Screen::SharedWithMe, &sift),
                ["shared-with-me"] => HttpResponse::json(200, &s.shared_with_me(&c.actor)),
                ["starred"] if !api => render(&s, &c.actor, Screen::Starred, &sift),
                ["starred"] => HttpResponse::json(200, &s.starred(&c.actor)),
                ["trash"] if !api => render(&s, &c.actor, Screen::Trash, &sift),
                ["trash"] => HttpResponse::json(200, &s.trash(&c.actor)),
                ["search"] if !api => render(&s, &c.actor, Screen::Search(&query), &sift),
                ["search"] => HttpResponse::json(200, &s.search(&c.actor, &query)),
                ["s", link] => render(&s, &c.actor, Screen::Link(link), &sift),
                _ => web::error(404, "route not found"),
            };
        }
        let b = web::body(r)?;
        // The search box is a form, so the same results screen has to answer a POST.
        if method == "POST" && parts.as_slice() == ["search"] {
            let q = web::text(&b, "q");
            return if api {
                HttpResponse::json(200, &s.search(&c.actor, &q))
            } else {
                render(&s, &c.actor, Screen::Search(&q), &sift)
            };
        }
        let parent = |b: &Value| match web::text(b, "parent") {
            v if v.is_empty() => root.clone(),
            v => v,
        };
        let result = match (method.as_str(), parts.as_slice()) {
            ("POST", ["folders"]) => s
                .create_folder(&c.actor, &web::text(&b, "name"), &parent(&b), c.tick)
                .map(|n| json!(n)),
            ("POST", ["files"]) => s
                .upload(
                    &c.actor,
                    &web::text(&b, "name"),
                    &parent(&b),
                    &web::text(&b, "content"),
                    c.tick,
                )
                .map(|n| json!(n)),
            ("POST" | "PUT" | "PATCH", ["nodes", id]) => s
                .relocate(
                    &c.actor,
                    id,
                    &web::text(&b, "name"),
                    &web::text(&b, "parent"),
                )
                .map(|n| json!(n)),
            ("POST", ["nodes", id, "share"]) => s
                .share(&c.actor, id, &web::text(&b, "actor"))
                .map(|n| json!(n)),
            ("POST", ["nodes", id, "star"]) => {
                s.star(&c.actor, id).map(|on| json!({"id":id,"starred":on}))
            }
            ("POST", ["nodes", id, "link"]) => s.mint_link(&c.actor, id, c.tick).map(|n| json!(n)),
            ("DELETE", ["nodes", id]) | ("POST", ["nodes", id, "trash"]) => {
                s.delete(&c.actor, id).map(|n| json!(n))
            }
            _ => return web::error(405, "unsupported route or method"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if api || result.is_err() {
            return web::domain(result);
        }
        // A page form lands on the screen where its change is visible. The touched id comes from
        // the path, or from the record a creation just minted.
        let touched = match parts.as_slice() {
            ["nodes", id, ..] => (*id).to_owned(),
            // A creation lands in the folder that now holds it, not on the new item's own page.
            _ => result
                .as_ref()
                .map(|v| web::text(v, "parent"))
                .unwrap_or_default(),
        };
        match s.nodes.get(&touched) {
            _ if s.trashed(&touched) => render(&s, &c.actor, Screen::Trash, &sift),
            Some(n) if n.kind == NodeKind::Folder => {
                render(&s, &c.actor, Screen::Folder(&touched), &sift)
            }
            Some(_) => render(&s, &c.actor, Screen::File(&touched), &sift),
            None => render(&s, &c.actor, Screen::Folder(&root), &sift),
        }
    }
}
#[cfg(test)]
mod tests;
