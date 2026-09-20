//! Deterministic mail service: one authoritative message store, per-user mailbox metadata.
mod skin;
mod time;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub use time::{stamp, Civil};

/// Skins this instance may wear; Gmail, Outlook and mail.com get branded HTML layouts, `plain`
/// is mail.internal.
pub const SKINS: &[&str] = &["plain", "gmail", "outlook", "mailcom"];
/// Folders a mailbox can be filtered by. `starred` and `all` are views, not stored folders.
pub const FOLDERS: &[&str] = &["inbox", "starred", "sent", "archive", "all"];
fn unset(v: &bool) -> bool {
    !*v
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MailState {
    /// Presentation only. `plain` is the original rendering and is omitted from serialised
    /// state, so worlds and checkpoints written before skins existed stay byte-identical.
    #[serde(default, skip_serializing_if = "web::Skin::is_plain")]
    pub skin: web::Skin,
    /// Wordmark for a skinned instance; the skin's own product name when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub brand: String,
    /// Seed palette override; the skin's own palette when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<cw_protocol::PageTheme>,
    pub users: BTreeSet<String>,
    /// Actor id → the address this instance delivers to. Absent means the actor id is the address.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub accounts: BTreeMap<String, String>,
    /// Default domain of the instance, shown in the account chip.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub domain: String,
    /// Display names, keyed by actor id or address; the local part is used where none is given.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<String, String>,
    /// Addresses that exist elsewhere in the world: deliverable, but only ever visible in Sent.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub external_contacts: BTreeSet<String>,
    /// Per-actor search box contents; a real, snapshot-surviving mutation of the mailbox view.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub queries: BTreeMap<String, String>,
    pub messages: BTreeMap<String, Message>,
    pub next_id: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: String,
    pub sender: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub time: u64,
    /// Conversation this message belongs to; empty means the message is its own thread, so a
    /// mailbox written before threads existed serialises exactly as it did before.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thread_id: String,
    pub mailboxes: BTreeMap<String, Mailbox>,
}
impl Message {
    pub fn thread(&self) -> &str {
        if self.thread_id.is_empty() {
            &self.id
        } else {
            &self.thread_id
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Mailbox {
    pub folders: BTreeSet<String>,
    pub labels: BTreeSet<String>,
    pub read: bool,
    #[serde(default, skip_serializing_if = "unset")]
    pub starred: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SendMail {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
}
/// Everything a mailbox page needs to know about where the reader is standing.
#[derive(Clone, Debug, Default)]
pub(crate) struct Nav {
    pub folder: String,
    pub thread: Option<String>,
    pub compose: bool,
}
impl Nav {
    fn folder(&self) -> &str {
        if self.folder.is_empty() {
            "inbox"
        } else {
            &self.folder
        }
    }
}
/// Drop the reply/forward prefixes so a reply lands in the conversation it answers.
fn root_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let cut = ["re:", "fwd:", "fw:"]
            .iter()
            .find(|p| s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p));
        match cut {
            Some(p) => s = s[p.len()..].trim_start(),
            None => return s.to_lowercase(),
        }
    }
}
impl MailState {
    /// Actors this instance serves: bare ids for the internal mailbox, `accounts` keys for a
    /// hosted one. Both forms coexist so `mail.internal` keeps working untouched.
    pub fn serves(&self, actor: &str) -> bool {
        self.users.contains(actor) || self.accounts.contains_key(actor)
    }
    /// The address to show for an actor, or the raw string for anyone outside this instance.
    pub fn address(&self, actor: &str) -> String {
        self.accounts
            .get(actor)
            .cloned()
            .unwrap_or_else(|| actor.to_owned())
    }
    /// Local mailbox a recipient names, whether they wrote the actor id or the address.
    pub fn local(&self, recipient: &str) -> Option<String> {
        if self.users.contains(recipient) || self.accounts.contains_key(recipient) {
            return Some(recipient.to_owned());
        }
        self.accounts
            .iter()
            .find(|(_, address)| address.as_str() == recipient)
            .map(|(actor, _)| actor.clone())
    }
    /// Display name for an actor or address: the seeded one, else the local part tidied up.
    pub fn display(&self, who: &str) -> String {
        let local = self.local(who);
        for key in [
            Some(who.to_owned()),
            local.clone(),
            local.map(|a| self.address(&a)),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(name) = self.names.get(&key) {
                return name.clone();
            }
        }
        let head = who.split('@').next().unwrap_or(who);
        let mut out = String::new();
        for word in head.split(['.', '-', '_']).filter(|w| !w.is_empty()) {
            if !out.is_empty() {
                out.push(' ');
            }
            let mut c = word.chars();
            if let Some(first) = c.next() {
                out.extend(first.to_uppercase());
                out.push_str(c.as_str());
            }
        }
        if out.is_empty() {
            who.to_owned()
        } else {
            out
        }
    }
    fn thread_for(&self, subject: &str) -> Option<String> {
        let root = root_subject(subject);
        let mut found: Vec<&Message> = self
            .messages
            .values()
            .filter(|m| root_subject(&m.subject) == root)
            .collect();
        found.sort_by_key(|m| (m.time, m.id.clone()));
        found.first().map(|m| m.thread().to_owned())
    }
    pub fn send(&mut self, actor: &str, time: u64, input: SendMail) -> Result<Message, String> {
        if !self.serves(actor) {
            return Err("unknown sender".into());
        }
        if input.to.is_empty() || input.subject.trim().is_empty() {
            return Err("recipient and subject required".into());
        }
        // Resolve every recipient before touching the store, so a bad address changes nothing.
        let mut local = Vec::new();
        for recipient in input.to.iter().chain(&input.cc) {
            match self.local(recipient) {
                Some(mailbox) => local.push(mailbox),
                // Deliverable outward: it reaches the sender's Sent folder and no local inbox.
                None if self.external_contacts.contains(recipient) => {}
                None => return Err("unknown recipient".into()),
            }
        }
        let mut mailboxes = BTreeMap::<String, Mailbox>::new();
        for user in local {
            mailboxes
                .entry(user)
                .or_default()
                .folders
                .insert("inbox".into());
        }
        let sender = mailboxes.entry(actor.into()).or_default();
        sender.folders.insert("sent".into());
        sender.read = true;
        self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        while self
            .messages
            .contains_key(&format!("mail-{}", self.next_id))
        {
            self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        }
        let id = format!("mail-{}", self.next_id);
        // Only a reply records a thread; a fresh message is its own, and stays byte-identical.
        let thread_id = self
            .thread_for(&input.subject)
            .filter(|t| t != &id)
            .unwrap_or_default();
        let message = Message {
            id,
            sender: actor.into(),
            to: input.to,
            cc: input.cc,
            subject: input.subject,
            body: input.body,
            time,
            thread_id,
            mailboxes,
        };
        self.messages.insert(message.id.clone(), message.clone());
        Ok(message)
    }
    pub fn list(&self, actor: &str, folder: Option<&str>) -> Vec<&Message> {
        self.messages
            .values()
            .filter(|m| {
                m.mailboxes
                    .get(actor)
                    .is_some_and(|b| folder.is_none_or(|f| b.folders.contains(f)))
            })
            .collect()
    }
    /// Messages of one conversation the actor can see, oldest first.
    pub fn thread(&self, actor: &str, thread: &str) -> Vec<&Message> {
        let mut found: Vec<&Message> = self
            .messages
            .values()
            .filter(|m| m.thread() == thread && m.mailboxes.contains_key(actor))
            .collect();
        found.sort_by_key(|m| (m.time, m.id.clone()));
        found
    }
    /// Folder view, newest first. `starred` and `all` are views over the same folders.
    pub(crate) fn folder(&self, actor: &str, folder: &str) -> Vec<&Message> {
        let query = self.queries.get(actor).map(|q| q.to_lowercase());
        let mut found: Vec<&Message> = self
            .messages
            .values()
            .filter(|m| {
                let Some(box_) = m.mailboxes.get(actor) else {
                    return false;
                };
                let placed = match folder {
                    "starred" => box_.starred,
                    "all" => true,
                    other => box_.folders.contains(other),
                };
                placed
                    && query.as_deref().is_none_or(|q| {
                        q.is_empty()
                            || m.subject.to_lowercase().contains(q)
                            || m.body.to_lowercase().contains(q)
                            || m.sender.to_lowercase().contains(q)
                    })
            })
            .collect();
        found.sort_by(|a, b| (b.time, &b.id).cmp(&(a.time, &a.id)));
        found
    }
    /// Newest message of every conversation in a folder, with that conversation's length.
    pub(crate) fn conversations(&self, actor: &str, folder: &str) -> Vec<(&Message, usize)> {
        let mut seen = BTreeSet::new();
        self.folder(actor, folder)
            .into_iter()
            .filter(|m| seen.insert(m.thread().to_owned()))
            .map(|m| (m, self.thread(actor, m.thread()).len()))
            .collect()
    }
    pub fn unread(&self, actor: &str, folder: &str) -> usize {
        self.folder(actor, folder)
            .iter()
            .filter(|m| !m.mailboxes[actor].read)
            .count()
    }
    pub fn metadata(
        &mut self,
        actor: &str,
        id: &str,
        read: Option<bool>,
        label: Option<String>,
        archive: bool,
    ) -> Result<(), String> {
        self.update(actor, id, read, label, archive, None)
    }
    /// `metadata` plus starring; a `None` star leaves the flag exactly as it was.
    pub fn update(
        &mut self,
        actor: &str,
        id: &str,
        read: Option<bool>,
        label: Option<String>,
        archive: bool,
        star: Option<bool>,
    ) -> Result<(), String> {
        let m = self
            .messages
            .get_mut(id)
            .and_then(|m| m.mailboxes.get_mut(actor))
            .ok_or("message unavailable")?;
        if let Some(v) = read {
            m.read = v;
        }
        if let Some(v) = label {
            if !v.trim().is_empty() {
                m.labels.insert(v);
            }
        }
        if let Some(v) = star {
            m.starred = v;
        }
        if archive {
            m.folders.remove("inbox");
            m.folders.insert("archive".into());
        }
        Ok(())
    }
    /// Store the search box for this actor; an empty query clears it rather than matching nothing.
    pub fn search(&mut self, actor: &str, query: &str) -> Result<(), String> {
        if !self.serves(actor) {
            return Err("mailbox unavailable".into());
        }
        match query.trim() {
            "" => {
                self.queries.remove(actor);
            }
            q => {
                self.queries.insert(actor.into(), q.to_owned());
            }
        }
        Ok(())
    }
}
use cw_protocol::{HttpRequest, HttpResponse, Result as SimResult};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Value};
pub struct MailService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(MailService)
}
/// The original mailbox page. Frozen: `mail.internal`, its world data and the Playwright
/// assertions all read these exact bytes, so nothing new may be added here.
fn plain(s: &MailState, actor: &str) -> SimResult<HttpResponse> {
    let mut e = vec![
        web::heading("title", "Mail"),
        web::form(
            "compose",
            "/send",
            &[
                ("to", "Recipients", ""),
                ("cc", "CC", ""),
                ("subject", "Subject", ""),
                ("body", "Message", ""),
            ],
        ),
    ];
    for m in s.list(actor, None) {
        e.push(web::heading(&m.id, &m.subject));
        e.push(web::paragraph(
            &format!("{}-body", m.id),
            format!("From: {}\n{}", m.sender, m.body),
        ));
        e.extend(web::links(&m.id, &m.body));
        e.push(web::form(
            &format!("{}-metadata", m.id),
            &format!("/messages/{}", m.id),
            &[("label", "Label", ""), ("read", "Read", "true")],
        ));
    }
    web::page("Mail", e)
}
fn view(s: &MailState, actor: &str, nav: &Nav) -> SimResult<HttpResponse> {
    match s.skin.as_str() {
        "gmail" | "outlook" | "mailcom" => skin::mailbox(s, actor, nav),
        _ => plain(s, actor),
    }
}
/// Redact other people's mailbox metadata from anything that leaves over the API.
fn mine(m: &Message, actor: &str) -> Message {
    let mut m = m.clone();
    m.mailboxes.retain(|u, _| u == actor);
    m
}
impl Service for MailService {
    fn kind(&self) -> &str {
        "mail"
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
        let s: MailState = web::load(&initial)?;
        s.skin.check(SKINS)?;
        if let Some(theme) = &s.theme {
            cw_protocol::Page {
                version: 1,
                title: String::new(),
                elements: vec![],
                theme: Some(theme.clone()),
                lang: None,
            }
            .validate()?;
        }
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<HttpResponse> {
        let mut s: MailState = web::load(state)?;
        if !s.serves(&c.actor) {
            return web::error(403, "mailbox unavailable");
        }
        let p = web::path(r);
        let skinned = !s.skin.is_plain();
        let method = r.method.to_ascii_uppercase();
        let nav = |thread: Option<String>| Nav {
            folder: web::query(r, "folder").unwrap_or_default(),
            thread: thread.or_else(|| web::query(r, "thread")),
            compose: web::query(r, "compose").is_some(),
        };
        if method == "GET" {
            return match p.as_str() {
                "/" => view(&s, &c.actor, &nav(None)),
                // Conversation permalinks exist only where a page links to them; `plain` keeps
                // its original route table, and therefore its original bytes.
                t if skinned && t.starts_with("/threads/") => {
                    let id = t.trim_start_matches("/threads/").to_owned();
                    view(&s, &c.actor, &nav(Some(id)))
                }
                "/api/messages" => {
                    let f = web::query(r, "folder");
                    HttpResponse::json(
                        200,
                        &s.list(&c.actor, f.as_deref())
                            .into_iter()
                            .map(|m| mine(m, &c.actor))
                            .collect::<Vec<_>>(),
                    )
                }
                t if t.starts_with("/api/threads/") => {
                    let id = t.trim_start_matches("/api/threads/");
                    let thread = s.thread(&c.actor, id);
                    if thread.is_empty() {
                        return web::error(404, "thread unavailable");
                    }
                    HttpResponse::json(
                        200,
                        &thread
                            .into_iter()
                            .map(|m| mine(m, &c.actor))
                            .collect::<Vec<_>>(),
                    )
                }
                _ => web::error(404, "route not found"),
            };
        }
        let b = web::body(r)?;
        let mut land = Nav {
            folder: web::text(&b, "folder"),
            thread: None,
            compose: false,
        };
        let result = if method == "POST" && (p == "/api/messages" || p == "/send") {
            s.send(
                &c.actor,
                c.tick,
                SendMail {
                    to: web::strings(&b, "to"),
                    cc: web::strings(&b, "cc"),
                    subject: web::text(&b, "subject"),
                    body: web::text(&b, "body"),
                },
            )
            .map(|m| {
                land.thread = Some(m.thread().to_owned());
                json!(mine(&m, &c.actor))
            })
        } else if method == "POST" && skinned && (p == "/search" || p == "/api/search") {
            s.search(&c.actor, &web::text(&b, "q"))
                .map(|_| json!({"ok":true}))
        } else if (method == "POST" || method == "PATCH")
            && (p.starts_with("/api/messages/") || p.starts_with("/messages/"))
        {
            let id = p.rsplit('/').next().unwrap();
            let read = b
                .get("read")
                .and_then(|v| v.as_bool().or_else(|| v.as_str()?.parse().ok()));
            // "toggle" is what a star button sends; it needs the current flag to mean anything.
            let star = match b.get("star").and_then(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .or_else(|| Some(v.as_bool()?.to_string()))
            }) {
                Some(v) if v == "toggle" => Some(
                    !s.messages
                        .get(id)
                        .and_then(|m| m.mailboxes.get(&c.actor))
                        .is_some_and(|b| b.starred),
                ),
                Some(v) => v.parse().ok(),
                None => None,
            };
            land.thread = s.messages.get(id).map(|m| m.thread().to_owned());
            s.update(
                &c.actor,
                id,
                read,
                b.get("label").and_then(Value::as_str).map(str::to_owned),
                b.get("archive")
                    .and_then(|v| v.as_bool().or_else(|| v.as_str()?.parse().ok()))
                    .unwrap_or(false),
                star,
            )
            .map(|_| json!({"ok":true}))
        } else {
            return web::error(405, "unsupported route or method");
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !p.starts_with("/api/") && result.is_ok() {
            view(&s, &c.actor, &land)
        } else {
            web::domain(result)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> MailState {
        MailState {
            users: ["alice", "bob", "eve"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ..Default::default()
        }
    }
    fn hosted() -> MailState {
        MailState {
            skin: web::Skin("gmail".into()),
            users: ["alice", "bob"].into_iter().map(str::to_owned).collect(),
            accounts: [
                ("alice", "alice@northstar.example"),
                ("bob", "bob@northstar.example"),
            ]
            .into_iter()
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .collect(),
            domain: "northstar.example".into(),
            external_contacts: ["tom.weber@theverge.com"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ..Default::default()
        }
    }
    fn input() -> SendMail {
        SendMail {
            to: vec!["bob".into()],
            subject: "Status".into(),
            body: "ready".into(),
            ..Default::default()
        }
    }
    fn context(actor: &str) -> ServiceContext {
        ServiceContext {
            actor: actor.into(),
            source: "pc".into(),
            tick: 3,
            seed: 1,
            instance: "mail".into(),
        }
    }
    #[test]
    fn delivery_and_private_metadata() {
        let mut s = state();
        let m = s.send("alice", 10, input()).unwrap();
        assert_eq!(s.list("bob", Some("inbox")).len(), 1);
        assert_eq!(s.list("alice", Some("sent")).len(), 1);
        assert!(s.list("eve", None).is_empty());
        assert!(s.metadata("eve", &m.id, Some(true), None, false).is_err());
        s.metadata("bob", &m.id, Some(true), Some("work".into()), true)
            .unwrap();
        assert!(s.list("bob", Some("inbox")).is_empty());
        assert!(!s.messages[&m.id].mailboxes["alice"].labels.contains("work"));
    }
    #[test]
    fn self_mail_both_folders_and_atomic_rejection() {
        let mut s = state();
        let mut m = input();
        m.to = vec!["alice".into()];
        s.send("alice", 1, m).unwrap();
        assert_eq!(s.list("alice", Some("inbox")).len(), 1);
        assert_eq!(s.list("alice", Some("sent")).len(), 1);
        let before = s.clone();
        let mut m = input();
        m.to.push("missing".into());
        assert!(s.send("alice", 1, m).is_err());
        assert_eq!(before, s);
    }
    #[test]
    fn addresses_resolve_and_external_mail_only_reaches_sent() {
        let mut s = hosted();
        let m = s
            .send(
                "alice",
                5,
                SendMail {
                    to: vec!["bob@northstar.example".into()],
                    subject: "Ship it".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(m.mailboxes.contains_key("bob"), "address resolved to bob");
        let out = s
            .send(
                "alice",
                6,
                SendMail {
                    to: vec!["tom.weber@theverge.com".into()],
                    subject: "Comment".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(out.mailboxes.keys().collect::<Vec<_>>(), ["alice"]);
        assert_eq!(s.list("alice", Some("sent")).len(), 2);
        let refused = s.send(
            "alice",
            7,
            SendMail {
                to: vec!["nobody@example.com".into()],
                subject: "Nope".into(),
                ..Default::default()
            },
        );
        assert!(refused.is_err());
    }
    #[test]
    fn replies_join_the_thread_and_fresh_mail_records_none() {
        let mut s = hosted();
        let first = s.send("alice", 1, input()).unwrap();
        assert!(first.thread_id.is_empty(), "a new mail owns its thread");
        let reply = s
            .send(
                "bob",
                2,
                SendMail {
                    to: vec!["alice".into()],
                    subject: "Re: Status".into(),
                    body: "on it".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(reply.thread_id, first.id);
        assert_eq!(s.thread("alice", first.thread()).len(), 2);
    }
    #[test]
    fn starring_and_search_are_per_actor_and_survive_serde() {
        let mut s = hosted();
        let m = s.send("alice", 1, input()).unwrap();
        s.update("bob", &m.id, None, None, false, Some(true))
            .unwrap();
        assert_eq!(s.folder("bob", "starred").len(), 1);
        assert!(s.folder("alice", "starred").is_empty());
        s.search("bob", "status").unwrap();
        assert_eq!(s.folder("bob", "inbox").len(), 1);
        s.search("bob", "invoice").unwrap();
        assert!(s.folder("bob", "inbox").is_empty());
        let restored: MailState = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(restored, s);
        s.search("bob", "  ").unwrap();
        assert!(restored.queries.contains_key("bob") && !s.queries.contains_key("bob"));
    }
    #[test]
    fn plain_state_and_page_bytes_are_exactly_what_they_were_before_skins() {
        // The world file, existing checkpoints and the Playwright assertions read these bytes.
        let mut s = state();
        s.send("alice", 10, input()).unwrap();
        let value = serde_json::to_value(&s).unwrap();
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"messages":{"mail-1":{"body":"ready","cc":[],"id":"mail-1","mailboxes":{"alice":{"folders":["sent"],"labels":[],"read":true},"bob":{"folders":["inbox"],"labels":[],"read":false}},"sender":"alice","subject":"Status","time":10,"to":["bob"]}},"next_id":1,"users":["alice","bob","eve"]}"#
        );
        let page = plain(&s, "alice").unwrap();
        assert_eq!(
            String::from_utf8(page.body).unwrap(),
            r#"{"version":1,"title":"Mail","elements":[{"kind":"heading","id":"title","text":"Mail","level":1},{"kind":"form","id":"compose","action":{"method":"POST","url":"/send","fields":{"body":"$compose-body","cc":"$compose-cc","subject":"$compose-subject","to":"$compose-to"}},"children":[{"kind":"input","id":"compose-to","label":"Recipients","value":"","placeholder":""},{"kind":"input","id":"compose-cc","label":"CC","value":"","placeholder":""},{"kind":"input","id":"compose-subject","label":"Subject","value":"","placeholder":""},{"kind":"input","id":"compose-body","label":"Message","value":"","placeholder":""},{"kind":"button","id":"compose-submit","text":"Submit","action":{"method":"POST","url":"/send","fields":{"body":"$compose-body","cc":"$compose-cc","subject":"$compose-subject","to":"$compose-to"}}}]},{"kind":"heading","id":"mail-1","text":"Status","level":1},{"kind":"text","id":"mail-1-body","text":"From: alice\nready"},{"kind":"form","id":"mail-1-metadata","action":{"method":"POST","url":"/messages/mail-1","fields":{"label":"$mail-1-metadata-label","read":"$mail-1-metadata-read"}},"children":[{"kind":"input","id":"mail-1-metadata-label","label":"Label","value":"","placeholder":""},{"kind":"input","id":"mail-1-metadata-read","label":"Read","value":"true","placeholder":""},{"kind":"button","id":"mail-1-metadata-submit","text":"Submit","action":{"method":"POST","url":"/messages/mail-1","fields":{"label":"$mail-1-metadata-label","read":"$mail-1-metadata-read"}}}]}]}"#
        );
    }
    #[test]
    fn plain_route_table_is_unchanged() {
        let c = context("alice");
        let mut v = serde_json::to_value(state()).unwrap();
        for url in [
            "http://mail/threads/mail-1",
            "http://mail/?folder=sent",
            "http://mail/search",
        ] {
            let r = HttpRequest::get(url);
            let status = MailService.handle(&mut v, &c, &r).unwrap().status;
            // `/?folder=sent` is still just the mailbox page; the other two are still absent.
            assert_eq!(status, if url.contains('?') { 200 } else { 404 }, "{url}");
        }
    }
    #[test]
    fn sdk_authoritative_identity_restore_and_render_purity() {
        let c = context("alice");
        let service = MailService;
        let mut v = serde_json::to_value(state()).unwrap();
        let r = HttpRequest::json(
            "POST",
            "http://mail/api/messages",
            &json!({"sender":"eve","to":["bob"],"subject":"x","body":"hello"}),
        )
        .unwrap();
        assert_eq!(service.handle(&mut v, &c, &r).unwrap().status, 200);
        assert_eq!(v["messages"]["mail-1"]["sender"], "alice");
        let before = v.clone();
        let page = service
            .handle(&mut v, &c, &HttpRequest::get("http://mail/"))
            .unwrap();
        assert_eq!(
            page,
            service
                .handle(&mut v, &c, &HttpRequest::get("http://mail/"))
                .unwrap()
        );
        assert_eq!(before, v);
        let restored: MailState = serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(restored.list("bob", None).len(), 1);
    }
    /// A parsed HTML response, strictly validated: every page a test fetches goes through the
    /// engine's strict pipeline, so unsupported CSS or a repeated id fails here.
    struct Dom(cw_web::dom::Document);
    impl Dom {
        fn of(response: &HttpResponse) -> Dom {
            assert_eq!(response.header("content-type"), Some(web::html::HTML_MEDIA_TYPE));
            let html = String::from_utf8(response.body.clone()).unwrap();
            web::html::validate_strict(&html).unwrap_or_else(|e| panic!("strict: {e:?}"));
            Dom(cw_web::html::parse(&html))
        }
        fn has(&self, id: &str) -> bool {
            !self.0.by_id(id).is_empty()
        }
        fn node(&self, id: &str) -> cw_web::dom::NodeId {
            *self.0.by_id(id).first().unwrap_or_else(|| panic!("no element #{id}"))
        }
        fn attr(&self, id: &str, name: &str) -> String {
            self.0.attr(self.node(id), name).unwrap_or_default().to_owned()
        }
        fn tag(&self, id: &str) -> String {
            self.0.tag(self.node(id)).unwrap_or_default().to_owned()
        }
        fn text(&self, id: &str) -> String {
            cw_web::paint::semantics::collapse(&self.0.text_content(self.node(id)))
        }
        fn body(&self) -> String {
            self.0.body().map(|b| self.0.text_content(b)).unwrap_or_default()
        }
        fn classes(&self, id: &str) -> Vec<String> {
            self.attr(id, "class").split_whitespace().map(str::to_owned).collect()
        }
    }
    fn get(v: &mut Value, actor: &str, url: &str) -> HttpResponse {
        MailService.handle(v, &context(actor), &HttpRequest::get(url)).unwrap()
    }
    /// What a browser sends when a form is submitted: urlencoded fields.
    fn post(v: &mut Value, actor: &str, url: &str, fields: &[(&str, &str)]) -> HttpResponse {
        let mut r = HttpRequest::get(url);
        r.method = "POST".into();
        r.headers.insert("content-type".into(), "application/x-www-form-urlencoded".into());
        r.body = web::html::href("", fields).trim_start_matches('?').as_bytes().to_vec();
        MailService.handle(v, &context(actor), &r).unwrap()
    }
    fn seeded(skin: &str) -> Value {
        let mut s = hosted();
        s.skin = web::Skin(skin.into());
        s.send(
            "alice",
            1,
            SendMail {
                to: vec!["bob@northstar.example".into()],
                subject: "Atlas launch checklist".into(),
                body: "Doc: http://docs.google.com/documents/atlas-launch.\n\nWalk it top to bottom.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        s.send(
            "bob",
            2,
            SendMail {
                to: vec!["alice".into()],
                subject: "Re: Atlas launch checklist".into(),
                body: "On it <today> & tomorrow".into(),
                ..Default::default()
            },
        )
        .unwrap();
        serde_json::to_value(s).unwrap()
    }
    #[test]
    fn every_page_of_every_skin_is_strict_html_with_the_documented_ids() {
        for (skin, brand) in [("gmail", "Gmail"), ("outlook", "Outlook"), ("mailcom", "mail.com")] {
            let mut v = seeded(skin);
            let home = Dom::of(&get(&mut v, "alice", "http://mail/"));
            assert_eq!(home.text("wordmark"), brand, "{skin}");
            // The search box is a POST form, as the Page form was, with the same field name.
            assert_eq!((home.tag("search"), home.attr("search", "action"), home.attr("search", "method")), ("form".into(), "/search".into(), "post".into()));
            assert_eq!((home.tag("search-q"), home.attr("search-q", "name"), home.attr("search-q", "aria-label")), ("input".into(), "q".into(), "Search mail".into()));
            assert_eq!(home.tag("search-submit"), "button");
            assert!(!home.has("search-clear"), "nothing to clear yet");
            assert_eq!(home.attr("compose", "href"), "/?folder=inbox&compose=1");
            assert_eq!(home.text("account-address"), "alice@northstar.example");
            assert_eq!(home.text("account-folder"), "1 unread");
            for (folder, href) in [("inbox", "/?folder=inbox"), ("starred", "/?folder=starred"), ("sent", "/?folder=sent"), ("archive", "/?folder=archive")] {
                assert_eq!((home.tag(&format!("folder-{folder}")), home.attr(&format!("folder-{folder}"), "href")), ("a".into(), href.into()), "{skin}");
            }
            assert!(home.classes("folder-inbox").contains(&"on".to_owned()));
            assert_eq!(home.text("folder-inbox-count"), "1");
            // The row is one link into the conversation; unread mail is marked so the sheet bolds it.
            assert_eq!((home.tag("row-mail-2"), home.attr("row-mail-2", "href")), ("a".into(), "/?folder=inbox&thread=mail-1".into()));
            assert!(home.classes("row-mail-2").contains(&"unread".to_owned()));
            assert_eq!(home.text("row-mail-2-sender"), "Bob");
            assert_eq!(home.text("row-mail-2-subject"), "Re: Atlas launch checklist");
            assert_eq!(home.text("row-mail-2-snippet"), "On it <today> & tomorrow", "seed text is escaped, not markup");
            assert_eq!(home.text("row-mail-2-count"), "2");
            assert!(home.has("row-mail-2-time") && home.has("row-mail-2-star") && home.has("row-mail-2-avatar"));
            assert!(!home.has("read-mail-1"), "no conversation is open");

            // The conversation: both messages, the doc link as a real link in the prose, the
            // three metadata buttons in one form, the label form and the reply form.
            let thread = Dom::of(&get(&mut v, "alice", "http://mail/?folder=inbox&thread=mail-1"));
            assert_eq!(thread.text("thread-subject"), "Re: Atlas launch checklist");
            assert_eq!(thread.text("thread-size"), "2 in thread");
            assert!(thread.classes("row-mail-2").contains(&"open".to_owned()));
            assert_eq!(thread.text("read-mail-1-name"), "Alice");
            assert!(thread.text("read-mail-1-line").contains("alice@northstar.example"));
            assert_eq!(thread.attr("read-mail-1-link-1", "href"), "http://docs.google.com/documents/atlas-launch");
            assert_eq!(thread.text("read-mail-1-link-1"), "http://docs.google.com/documents/atlas-launch");
            assert!(thread.text("read-mail-1-body").ends_with("Walk it top to bottom."));
            assert_eq!((thread.attr("read-mail-2-actions", "action"), thread.attr("read-mail-2-actions", "method")), ("/messages/mail-2".into(), "post".into()));
            assert_eq!((thread.attr("read-mail-2-star", "name"), thread.attr("read-mail-2-star", "value"), thread.text("read-mail-2-star")), ("star".into(), "toggle".into(), "Star".into()));
            assert_eq!((thread.attr("read-mail-2-read", "name"), thread.attr("read-mail-2-read", "value"), thread.text("read-mail-2-read")), ("read".into(), "true".into(), "Mark read".into()));
            assert_eq!((thread.attr("read-mail-2-archive", "name"), thread.attr("read-mail-2-archive", "value")), ("archive".into(), "true".into()));
            assert_eq!(thread.attr("read-mail-2-permalink", "href"), "/threads/mail-1");
            assert_eq!((thread.attr("read-mail-2-label", "action"), thread.attr("read-mail-2-label-label", "name")), ("/messages/mail-2".into(), "label".into()));
            assert_eq!(thread.tag("read-mail-2-label-submit"), "button");
            assert_eq!((thread.attr("reply", "action"), thread.attr("reply", "method")), ("/send".into(), "post".into()));
            assert_eq!(thread.attr("reply-to", "value"), "bob@northstar.example");
            assert_eq!(thread.attr("reply-subject", "value"), "Re: Atlas launch checklist");
            assert_eq!((thread.tag("reply-body"), thread.attr("reply-body", "name")), ("textarea".into(), "body".into()));
            assert_eq!(thread.tag("reply-submit"), "button");
            // The permalink route is the same conversation.
            let permalink = Dom::of(&get(&mut v, "alice", "http://mail/threads/mail-1"));
            assert_eq!(permalink.text("thread-subject"), "Re: Atlas launch checklist");

            // Compose: the same four fields, posted to the same route.
            let compose = Dom::of(&get(&mut v, "alice", "http://mail/?folder=sent&compose=1"));
            assert_eq!((compose.attr("new", "action"), compose.attr("new", "method")), ("/send".into(), "post".into()));
            for (id, name) in [("new-to", "to"), ("new-cc", "cc"), ("new-subject", "subject"), ("new-body", "body")] {
                assert_eq!(compose.attr(id, "name"), name, "{skin}");
            }
            assert_eq!(compose.tag("new-submit"), "button");
            assert_eq!(compose.text("compose-from"), "From alice@northstar.example");
            assert!(compose.classes("folder-sent").contains(&"on".to_owned()));
            // Empty folders say so.
            let empty = Dom::of(&get(&mut v, "alice", "http://mail/?folder=starred"));
            assert_eq!(empty.text("list-empty"), "Nothing here.");
            assert!(empty.has("reading-empty") && empty.has("reading-hint"));
        }
    }
    #[test]
    fn skinned_forms_send_search_star_and_label_the_way_a_browser_posts_them() {
        for skin in ["gmail", "outlook", "mailcom"] {
            let mut v = seeded(skin);
            // Reply through the form: the response is the conversation, now three long.
            let sent = Dom::of(&post(
                &mut v,
                "alice",
                "http://mail/send",
                &[("to", "bob@northstar.example"), ("subject", "Re: Atlas launch checklist"), ("body", "Ticked.\nAll of it.")],
            ));
            assert_eq!(sent.text("thread-size"), "3 in thread");
            assert_eq!(v["messages"]["mail-3"]["body"], "Ticked.\nAll of it.");
            // Search is a real mutation: it survives into the state and filters the next render.
            let found = Dom::of(&post(&mut v, "alice", "http://mail/search", &[("q", "nothing")]));
            assert_eq!(v["queries"]["alice"], "nothing");
            assert!(!found.has("row-mail-2") && found.has("list-empty"));
            assert_eq!(found.attr("search-q", "value"), "nothing");
            assert_eq!((found.tag("search-clear"), found.attr("search-clear-form", "action")), ("button".into(), "/search".into()));
            let cleared = Dom::of(&post(&mut v, "alice", "http://mail/search", &[("q", "")]));
            assert!(cleared.has("row-mail-2") && !cleared.has("search-clear"));
            // The star button posts `star=toggle` plus the folder it was pressed in.
            let starred = Dom::of(&post(&mut v, "alice", "http://mail/messages/mail-2", &[("folder", "inbox"), ("star", "toggle")]));
            assert_eq!(v["messages"]["mail-2"]["mailboxes"]["alice"]["starred"], true);
            assert_eq!(starred.text("read-mail-2-star"), "Unstar");
            assert!(starred.classes("row-mail-2-star").contains(&"on".to_owned()));
            let read = Dom::of(&post(&mut v, "alice", "http://mail/messages/mail-2", &[("folder", "inbox"), ("read", "true")]));
            assert_eq!(read.text("read-mail-2-read"), "Mark unread");
            assert!(read.classes("row-mail-2").contains(&"read".to_owned()));
            let labelled = Dom::of(&post(&mut v, "alice", "http://mail/messages/mail-2", &[("label", "Atlas")]));
            assert_eq!(labelled.text("read-mail-2-label-0"), "Atlas");
            let archived = Dom::of(&post(&mut v, "alice", "http://mail/messages/mail-2", &[("folder", "inbox"), ("archive", "true")]));
            assert!(!archived.has("row-mail-2"), "{skin}: archived mail leaves the inbox");
            assert!(Dom::of(&get(&mut v, "alice", "http://mail/?folder=archive")).has("row-mail-2"));
        }
    }
    #[test]
    fn skinned_pages_render_route_and_mutate() {
        let c = context("alice");
        let mut v = serde_json::to_value(hosted()).unwrap();
        let service = MailService;
        service
            .handle(
                &mut v,
                &c,
                &HttpRequest::json(
                    "POST",
                    "http://mail/api/messages",
                    &json!({"to":["bob@northstar.example"],"subject":"Atlas launch checklist","body":"see http://docs.google.com/documents/atlas-launch"}),
                )
                .unwrap(),
            )
            .unwrap();
        let sent = Dom::of(&get(&mut v, "alice", "http://mail/?folder=sent&thread=mail-1"));
        assert_eq!(sent.text("wordmark"), "Gmail");
        assert_eq!(sent.text("thread-subject"), "Atlas launch checklist");
        // The doc link in the body is a real navigation control, which is how sites connect.
        assert_eq!(sent.attr("read-mail-1-link-1", "href"), "http://docs.google.com/documents/atlas-launch");
        assert_eq!(get(&mut v, "alice", "http://mail/threads/mail-1").status, 200);
        // Search is a real mutation: it survives into the state and filters the next render.
        let search =
            HttpRequest::json("POST", "http://mail/search", &json!({"q":"nothing"})).unwrap();
        assert_eq!(service.handle(&mut v, &c, &search).unwrap().status, 200);
        assert_eq!(v["queries"]["alice"], "nothing");
        assert!(!Dom::of(&get(&mut v, "alice", "http://mail/?folder=sent")).body().contains("Atlas launch checklist"));
        let star = HttpRequest::json(
            "POST",
            "http://mail/messages/mail-1",
            &json!({"star":"toggle"}),
        )
        .unwrap();
        assert_eq!(service.handle(&mut v, &c, &star).unwrap().status, 200);
        assert_eq!(
            v["messages"]["mail-1"]["mailboxes"]["alice"]["starred"],
            true
        );
    }
    #[test]
    fn outlook_skin_renders_its_own_wordmark() {
        let mut s = hosted();
        s.skin = web::Skin("outlook".into());
        s.send("alice", 1, input()).unwrap();
        let page = Dom::of(&view(&s, "alice", &Nav::default()).unwrap());
        assert_eq!(page.text("wordmark"), "Outlook");
        assert!(page.body().contains("Outlook") && !page.body().contains("Gmail"));
        assert_eq!(page.text("folder-sent-label"), "Sent Items");
    }
    #[test]
    fn unknown_skin_is_a_seed_error() {
        assert!(MailService
            .initialize(json!({"skin":"proton"}), &context("alice"))
            .is_err());
    }
}
