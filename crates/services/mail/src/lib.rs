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
    /// Attached files: name → the file's bytes in standard base64. Absent when there are
    /// none, so a message without attachments serialises exactly as it did before them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attachments: BTreeMap<String, String>,
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
    /// Files to attach, name → base64, as [`Message::attachments`] holds them.
    pub attachments: BTreeMap<String, String>,
}
/// The most one message may carry in attachments, decoded. The whole mailbox rides in the
/// world's state, so a message is a letter and not a file share.
pub const ATTACHMENT_BYTES: usize = 8 * 1024 * 1024;
/// Refuse attachments a mail client would: a name that is a path, or is empty or hidden,
/// bytes that are not base64, and more than [`ATTACHMENT_BYTES`] in all.
pub fn check_attachments(attachments: &BTreeMap<String, String>) -> Result<(), String> {
    let mut total = 0usize;
    for (name, data) in attachments {
        if name.trim().is_empty()
            || name.starts_with('.')
            || name.contains(['/', '\\', '\0'])
            || name.chars().count() > 128
        {
            return Err(format!("{name:?} is not an attachment name"));
        }
        total += cw_protocol::decode_base64(data)
            .map_err(|e| format!("attachment {name:?} is not base64: {e}"))?
            .len();
    }
    if total > ATTACHMENT_BYTES {
        return Err(format!(
            "attachments total {total} bytes; a message holds at most {ATTACHMENT_BYTES}"
        ));
    }
    Ok(())
}
/// The media type a file's extension names, for serving it and for its chip.
pub fn media_type(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("txt" | "log") => "text/plain; charset=utf-8",
        Some("md") => "text/markdown; charset=utf-8",
        Some("csv") => "text/csv; charset=utf-8",
        Some("ics") => "text/calendar; charset=utf-8",
        Some("json") => "application/json",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}
/// How many bytes a base64 attachment holds, without decoding it.
pub fn attachment_size(data: &str) -> usize {
    let data = data.trim_end();
    let pad = data.bytes().rev().take_while(|b| *b == b'=').count();
    (data.len() / 4 * 3).saturating_sub(pad)
}
/// A name as it appears in an attachment's URL: everything but unreserved characters
/// percent-encoded, so a name with spaces is still one path segment.
pub fn url_name(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn from_url_name(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
/// Everything a mailbox page needs to know about where the reader is standing.
#[derive(Clone, Debug, Default)]
pub(crate) struct Nav {
    pub folder: String,
    pub thread: Option<String>,
    pub compose: bool,
    /// One of the reader's own labels, when the rail is filtering the folder by it.
    pub label: String,
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
        check_attachments(&input.attachments)?;
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
            attachments: input.attachments,
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
                            || m.attachments.keys().any(|n| n.to_lowercase().contains(q))
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
/// A message as it leaves over the API: other people's mailbox metadata redacted, and each
/// attachment described — name, size, type and where to fetch it — rather than inlined.
fn mine(m: &Message, actor: &str) -> Value {
    let mut out = m.clone();
    out.mailboxes.retain(|u, _| u == actor);
    let files: Vec<Value> = std::mem::take(&mut out.attachments)
        .iter()
        .map(|(name, data)| {
            json!({
                "name": name,
                "size": attachment_size(data),
                "type": media_type(name).split(';').next().unwrap_or_default(),
                "url": format!("/attachments/{}/{}", m.id, url_name(name)),
            })
        })
        .collect();
    let mut value = serde_json::to_value(out).unwrap_or(Value::Null);
    if !files.is_empty() {
        value["attachments"] = Value::Array(files);
    }
    value
}
/// One attachment's bytes, to someone whose mailbox holds the message. Served as an
/// attachment, so a browser saves it to Downloads rather than showing it.
fn attachment(s: &MailState, actor: &str, rest: &str) -> SimResult<HttpResponse> {
    let found = rest.split_once('/').and_then(|(id, name)| {
        let m = s.messages.get(id)?;
        m.mailboxes.get(actor)?;
        let name = from_url_name(name)?;
        let data = m.attachments.get(&name)?;
        Some((name, data))
    });
    let Some((name, data)) = found else {
        return web::error(404, "attachment unavailable");
    };
    let body = cw_protocol::decode_base64(data).map_err(cw_protocol::SimError::invalid)?;
    let quoted: String = name
        .chars()
        .map(|c| if c == '"' || c.is_control() { '_' } else { c })
        .collect();
    Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::from([
            ("content-type".into(), media_type(&name).into()),
            (
                "content-disposition".into(),
                format!("attachment; filename=\"{quoted}\""),
            ),
        ]),
        body,
    })
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
        for m in s.messages.values() {
            check_attachments(&m.attachments)
                .map_err(|e| cw_protocol::SimError::invalid(format!("{}: {e}", m.id)))?;
        }
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
            label: web::query(r, "label").unwrap_or_default(),
        };
        if method == "GET" {
            return match p.as_str() {
                "/" => view(&s, &c.actor, &nav(None)),
                t if t.starts_with("/attachments/") => {
                    attachment(&s, &c.actor, t.trim_start_matches("/attachments/"))
                }
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
        // Where a form says it was pressed. `filter` is the label the list was narrowed to;
        // `label` is never read here, because on `/messages/<id>` it is the label being added.
        let mut land = Nav {
            folder: web::text(&b, "folder"),
            thread: None,
            compose: false,
            label: web::text(&b, "filter"),
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
                    attachments: b
                        .get("attachments")
                        .and_then(Value::as_object)
                        .map(|files| {
                            files
                                .iter()
                                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned()))
                                .collect()
                        })
                        .unwrap_or_default(),
                },
            )
            .map(|m| {
                land.thread = Some(m.thread().to_owned());
                mine(&m, &c.actor)
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
            // A row's star says which conversation was open when it was pressed, so the page
            // comes back as it was; the reading pane sends no `thread` and opens the one it
            // acted on, as it always has.
            land.thread = match b.get("thread") {
                Some(_) => Some(web::text(&b, "thread")).filter(|t| !t.is_empty()),
                None => s.messages.get(id).map(|m| m.thread().to_owned()),
            };
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
            assert_eq!(
                response.header("content-type"),
                Some(web::html::HTML_MEDIA_TYPE)
            );
            let html = String::from_utf8(response.body.clone()).unwrap();
            web::html::validate_strict(&html).unwrap_or_else(|e| panic!("strict: {e:?}"));
            Dom(cw_web::html::parse(&html))
        }
        fn has(&self, id: &str) -> bool {
            !self.0.by_id(id).is_empty()
        }
        fn node(&self, id: &str) -> cw_web::dom::NodeId {
            *self
                .0
                .by_id(id)
                .first()
                .unwrap_or_else(|| panic!("no element #{id}"))
        }
        fn attr(&self, id: &str, name: &str) -> String {
            self.0
                .attr(self.node(id), name)
                .unwrap_or_default()
                .to_owned()
        }
        fn tag(&self, id: &str) -> String {
            self.0.tag(self.node(id)).unwrap_or_default().to_owned()
        }
        fn text(&self, id: &str) -> String {
            cw_web::paint::semantics::collapse(&self.0.text_content(self.node(id)))
        }
        fn body(&self) -> String {
            self.0
                .body()
                .map(|b| self.0.text_content(b))
                .unwrap_or_default()
        }
        fn classes(&self, id: &str) -> Vec<String> {
            self.attr(id, "class")
                .split_whitespace()
                .map(str::to_owned)
                .collect()
        }
    }
    fn get(v: &mut Value, actor: &str, url: &str) -> HttpResponse {
        MailService
            .handle(v, &context(actor), &HttpRequest::get(url))
            .unwrap()
    }
    /// What a browser sends when a form is submitted: urlencoded fields.
    fn post(v: &mut Value, actor: &str, url: &str, fields: &[(&str, &str)]) -> HttpResponse {
        let mut r = HttpRequest::get(url);
        r.method = "POST".into();
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        r.body = web::html::href("", fields)
            .trim_start_matches('?')
            .as_bytes()
            .to_vec();
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
                body:
                    "Doc: http://docs.google.com/documents/atlas-launch.\n\nWalk it top to bottom."
                        .into(),
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
        for (skin, brand) in [
            ("gmail", "Gmail"),
            ("outlook", "Outlook"),
            ("mailcom", "mail.com"),
        ] {
            let mut v = seeded(skin);
            let home = Dom::of(&get(&mut v, "alice", "http://mail/"));
            assert_eq!(home.text("wordmark"), brand, "{skin}");
            // The search box is a POST form, as the Page form was, with the same field name.
            assert_eq!(
                (
                    home.tag("search"),
                    home.attr("search", "action"),
                    home.attr("search", "method")
                ),
                ("form".into(), "/search".into(), "post".into())
            );
            assert_eq!(
                (
                    home.tag("search-q"),
                    home.attr("search-q", "name"),
                    home.attr("search-q", "aria-label")
                ),
                ("input".into(), "q".into(), "Search mail".into())
            );
            assert_eq!(home.tag("search-submit"), "button");
            assert!(!home.has("search-clear"), "nothing to clear yet");
            assert_eq!(home.attr("compose", "href"), "/?folder=inbox&compose=1");
            assert_eq!(home.text("account-address"), "alice@northstar.example");
            assert_eq!(home.text("account-folder"), "1 unread");
            for (folder, href) in [
                ("inbox", "/?folder=inbox"),
                ("starred", "/?folder=starred"),
                ("sent", "/?folder=sent"),
                ("archive", "/?folder=archive"),
            ] {
                assert_eq!(
                    (
                        home.tag(&format!("folder-{folder}")),
                        home.attr(&format!("folder-{folder}"), "href")
                    ),
                    ("a".into(), href.into()),
                    "{skin}"
                );
            }
            assert!(home.classes("folder-inbox").contains(&"on".to_owned()));
            assert_eq!(home.text("folder-inbox-count"), "1");
            // The row is one link into the conversation; unread mail is marked so the sheet bolds it.
            assert_eq!(
                (home.tag("row-mail-2"), home.attr("row-mail-2", "href")),
                ("a".into(), "/?folder=inbox&thread=mail-1".into())
            );
            assert!(home.classes("row-mail-2").contains(&"unread".to_owned()));
            assert_eq!(home.text("row-mail-2-sender"), "Bob");
            assert_eq!(
                home.text("row-mail-2-subject"),
                "Re: Atlas launch checklist"
            );
            assert_eq!(
                home.text("row-mail-2-snippet"),
                "On it <today> & tomorrow",
                "seed text is escaped, not markup"
            );
            assert_eq!(home.text("row-mail-2-count"), "2");
            assert!(
                home.has("row-mail-2-time")
                    && home.has("row-mail-2-star")
                    && home.has("row-mail-2-avatar")
            );
            assert!(!home.has("read-mail-1"), "no conversation is open");
            // The row's star is a button of its own form, not an icon inside the row link: it
            // posts the same toggle the reading pane posts and says where it was pressed.
            assert_eq!(home.tag("row-mail-2-star"), "button", "{skin}");
            assert_eq!(
                (
                    home.attr("row-mail-2-star", "name"),
                    home.attr("row-mail-2-star", "value"),
                    home.attr("row-mail-2-star", "aria-label")
                ),
                ("star".into(), "toggle".into(), "Star".into()),
                "{skin}"
            );
            assert_eq!(
                (
                    home.attr("row-mail-2-star-form", "action"),
                    home.attr("row-mail-2-star-form", "method")
                ),
                ("/messages/mail-2".into(), "post".into()),
                "{skin}"
            );
            // The one toolbar icon is a link to this same view, which is what refresh means.
            assert_eq!(
                (home.tag("list-refresh"), home.attr("list-refresh", "href")),
                ("a".into(), "/?folder=inbox".into()),
                "{skin}"
            );
            // Nothing is drawn that cannot be pressed: no category strip, no select-all box.
            assert!(
                !home.body().contains("Promotions") && !home.body().contains("Focused"),
                "{skin}"
            );
            // Gmail's rail used to call this "All Mail" while showing only what was archived.
            assert_eq!(home.text("folder-archive-label"), "Archive", "{skin}");

            // The conversation: both messages, the doc link as a real link in the prose, the
            // three metadata buttons in one form, the label form and the reply form.
            let thread = Dom::of(&get(
                &mut v,
                "alice",
                "http://mail/?folder=inbox&thread=mail-1",
            ));
            assert_eq!(thread.text("thread-subject"), "Re: Atlas launch checklist");
            assert_eq!(thread.text("thread-size"), "2 in thread");
            assert!(thread.classes("row-mail-2").contains(&"open".to_owned()));
            assert_eq!(thread.text("read-mail-1-name"), "Alice");
            assert!(thread
                .text("read-mail-1-line")
                .contains("alice@northstar.example"));
            assert_eq!(
                thread.attr("read-mail-1-link-1", "href"),
                "http://docs.google.com/documents/atlas-launch"
            );
            assert_eq!(
                thread.text("read-mail-1-link-1"),
                "http://docs.google.com/documents/atlas-launch"
            );
            assert!(thread
                .text("read-mail-1-body")
                .ends_with("Walk it top to bottom."));
            assert_eq!(
                (
                    thread.attr("read-mail-2-actions", "action"),
                    thread.attr("read-mail-2-actions", "method")
                ),
                ("/messages/mail-2".into(), "post".into())
            );
            assert_eq!(
                (
                    thread.attr("read-mail-2-star", "name"),
                    thread.attr("read-mail-2-star", "value"),
                    thread.text("read-mail-2-star")
                ),
                ("star".into(), "toggle".into(), "Star".into())
            );
            assert_eq!(
                (
                    thread.attr("read-mail-2-read", "name"),
                    thread.attr("read-mail-2-read", "value"),
                    thread.text("read-mail-2-read")
                ),
                ("read".into(), "true".into(), "Mark read".into())
            );
            assert_eq!(
                (
                    thread.attr("read-mail-2-archive", "name"),
                    thread.attr("read-mail-2-archive", "value")
                ),
                ("archive".into(), "true".into())
            );
            assert_eq!(
                thread.attr("read-mail-2-permalink", "href"),
                "/threads/mail-1"
            );
            assert_eq!(
                (
                    thread.attr("read-mail-2-label", "action"),
                    thread.attr("read-mail-2-label-label", "name")
                ),
                ("/messages/mail-2".into(), "label".into())
            );
            assert_eq!(thread.tag("read-mail-2-label-submit"), "button");
            assert_eq!(
                (
                    thread.attr("reply", "action"),
                    thread.attr("reply", "method")
                ),
                ("/send".into(), "post".into())
            );
            assert_eq!(thread.attr("reply-to", "value"), "bob@northstar.example");
            assert_eq!(
                thread.attr("reply-subject", "value"),
                "Re: Atlas launch checklist"
            );
            assert_eq!(
                (thread.tag("reply-body"), thread.attr("reply-body", "name")),
                ("textarea".into(), "body".into())
            );
            assert_eq!(thread.tag("reply-submit"), "button");
            // The permalink route is the same conversation.
            let permalink = Dom::of(&get(&mut v, "alice", "http://mail/threads/mail-1"));
            assert_eq!(
                permalink.text("thread-subject"),
                "Re: Atlas launch checklist"
            );

            // Compose: the same four fields, posted to the same route.
            let compose = Dom::of(&get(&mut v, "alice", "http://mail/?folder=sent&compose=1"));
            assert_eq!(
                (compose.attr("new", "action"), compose.attr("new", "method")),
                ("/send".into(), "post".into())
            );
            for (id, name) in [
                ("new-to", "to"),
                ("new-cc", "cc"),
                ("new-subject", "subject"),
                ("new-body", "body"),
            ] {
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
                &[
                    ("to", "bob@northstar.example"),
                    ("subject", "Re: Atlas launch checklist"),
                    ("body", "Ticked.\nAll of it."),
                ],
            ));
            assert_eq!(sent.text("thread-size"), "3 in thread");
            assert_eq!(v["messages"]["mail-3"]["body"], "Ticked.\nAll of it.");
            // Search is a real mutation: it survives into the state and filters the next render.
            let found = Dom::of(&post(
                &mut v,
                "alice",
                "http://mail/search",
                &[("q", "nothing")],
            ));
            assert_eq!(v["queries"]["alice"], "nothing");
            assert!(!found.has("row-mail-2") && found.has("list-empty"));
            assert_eq!(found.attr("search-q", "value"), "nothing");
            assert_eq!(
                (
                    found.tag("search-clear"),
                    found.attr("search-clear-form", "action")
                ),
                ("button".into(), "/search".into())
            );
            let cleared = Dom::of(&post(&mut v, "alice", "http://mail/search", &[("q", "")]));
            assert!(cleared.has("row-mail-2") && !cleared.has("search-clear"));
            // The star button posts `star=toggle` plus the folder it was pressed in.
            let starred = Dom::of(&post(
                &mut v,
                "alice",
                "http://mail/messages/mail-2",
                &[("folder", "inbox"), ("star", "toggle")],
            ));
            assert_eq!(
                v["messages"]["mail-2"]["mailboxes"]["alice"]["starred"],
                true
            );
            assert_eq!(starred.text("read-mail-2-star"), "Unstar");
            assert!(starred
                .classes("row-mail-2-star")
                .contains(&"on".to_owned()));
            let read = Dom::of(&post(
                &mut v,
                "alice",
                "http://mail/messages/mail-2",
                &[("folder", "inbox"), ("read", "true")],
            ));
            assert_eq!(read.text("read-mail-2-read"), "Mark unread");
            assert!(read.classes("row-mail-2").contains(&"read".to_owned()));
            let labelled = Dom::of(&post(
                &mut v,
                "alice",
                "http://mail/messages/mail-2",
                &[("label", "Atlas")],
            ));
            assert_eq!(labelled.text("read-mail-2-label-0"), "Atlas");
            // The rail's labels are links into the mail that carries them, and the view they
            // open says so in its heading.
            assert_eq!(
                (labelled.tag("label-0"), labelled.attr("label-0", "href")),
                ("a".into(), "/?folder=all&label=Atlas".into()),
                "{skin}"
            );
            let by_label = Dom::of(&get(&mut v, "alice", "http://mail/?folder=all&label=Atlas"));
            assert_eq!(by_label.text("list-title"), "Atlas");
            // A label belongs to the conversation: the label was typed on mail-2 and the row
            // that stands for the exchange in `all` is its newest message, mail-3.
            assert!(
                by_label.has("row-mail-3")
                    && by_label.classes("label-0").contains(&"on".to_owned()),
                "{skin}"
            );
            // A star pressed in the list comes back to the list, not to some other conversation.
            let from_list = Dom::of(&post(
                &mut v,
                "alice",
                "http://mail/messages/mail-2",
                &[
                    ("folder", "inbox"),
                    ("filter", ""),
                    ("thread", ""),
                    ("star", "toggle"),
                ],
            ));
            assert!(
                !from_list.has("read-mail-2"),
                "{skin}: the list star does not open the thread"
            );
            assert_eq!(
                from_list.text("row-mail-2-star"),
                "☆",
                "{skin}: and it unstarred the row"
            );
            let archived = Dom::of(&post(
                &mut v,
                "alice",
                "http://mail/messages/mail-2",
                &[("folder", "inbox"), ("archive", "true")],
            ));
            assert!(
                !archived.has("row-mail-2"),
                "{skin}: archived mail leaves the inbox"
            );
            assert!(
                Dom::of(&get(&mut v, "alice", "http://mail/?folder=archive")).has("row-mail-2")
            );
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
        let sent = Dom::of(&get(
            &mut v,
            "alice",
            "http://mail/?folder=sent&thread=mail-1",
        ));
        assert_eq!(sent.text("wordmark"), "Gmail");
        assert_eq!(sent.text("thread-subject"), "Atlas launch checklist");
        // The doc link in the body is a real navigation control, which is how sites connect.
        assert_eq!(
            sent.attr("read-mail-1-link-1", "href"),
            "http://docs.google.com/documents/atlas-launch"
        );
        assert_eq!(
            get(&mut v, "alice", "http://mail/threads/mail-1").status,
            200
        );
        // Search is a real mutation: it survives into the state and filters the next render.
        let search =
            HttpRequest::json("POST", "http://mail/search", &json!({"q":"nothing"})).unwrap();
        assert_eq!(service.handle(&mut v, &c, &search).unwrap().status, 200);
        assert_eq!(v["queries"]["alice"], "nothing");
        assert!(!Dom::of(&get(&mut v, "alice", "http://mail/?folder=sent"))
            .body()
            .contains("Atlas launch checklist"));
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
    fn with_files(files: &[(&str, &[u8])]) -> BTreeMap<String, String> {
        files
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), base64(bytes)))
            .collect()
    }
    fn base64(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                out.push(if i <= chunk.len() {
                    A[(n >> (18 - 6 * i)) as usize & 63] as char
                } else {
                    '='
                });
            }
        }
        out
    }
    #[test]
    fn attachments_are_served_only_to_a_mailbox_that_holds_them() {
        let mut s = hosted();
        s.users.insert("eve".into());
        s.send(
            "alice",
            1,
            SendMail {
                to: vec!["bob".into()],
                subject: "Plan".into(),
                attachments: with_files(&[
                    ("rollback plan.md", b"# Roll back\n"),
                    ("b.zip", &[0x50, 0x4b, 0, 0xff]),
                ]),
                ..Default::default()
            },
        )
        .unwrap();
        let mut v = serde_json::to_value(s).unwrap();
        let zip = get(&mut v, "bob", "http://mail/attachments/mail-1/b.zip");
        assert_eq!(zip.status, 200);
        assert_eq!(zip.body, [0x50, 0x4b, 0, 0xff]);
        assert_eq!(zip.header("content-type"), Some("application/zip"));
        assert_eq!(
            zip.header("content-disposition"),
            Some("attachment; filename=\"b.zip\"")
        );
        let spaced = get(
            &mut v,
            "alice",
            "http://mail/attachments/mail-1/rollback%20plan.md",
        );
        assert_eq!(spaced.body, b"# Roll back\n");
        assert_eq!(
            get(&mut v, "eve", "http://mail/attachments/mail-1/b.zip").status,
            404
        );
        assert_eq!(
            get(&mut v, "bob", "http://mail/attachments/mail-1/c.zip").status,
            404
        );
    }
    #[test]
    fn the_api_describes_attachments_rather_than_inlining_them() {
        let mut v = serde_json::to_value(hosted()).unwrap();
        let sent = MailService
            .handle(
                &mut v,
                &context("alice"),
                &HttpRequest::json(
                    "POST",
                    "http://mail/api/messages",
                    &json!({"to":["bob"],"subject":"Owners","attachments":{"owners.csv": base64(b"section,owner\nci,bob\n")}}),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(sent.status, 200);
        let listed: Value =
            serde_json::from_slice(&get(&mut v, "bob", "http://mail/api/messages").body).unwrap();
        assert_eq!(
            listed[0]["attachments"],
            json!([{"name":"owners.csv","size":21,"type":"text/csv","url":"/attachments/mail-1/owners.csv"}])
        );
        let bad = MailService
            .handle(
                &mut v,
                &context("alice"),
                &HttpRequest::json(
                    "POST",
                    "http://mail/api/messages",
                    &json!({"to":["bob"],"subject":"Sneaky","attachments":{"../x": "aGk="}}),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(bad.status, 400);
    }
    #[test]
    fn a_message_without_attachments_serialises_as_before_and_bad_seeds_are_refused() {
        let mut s = state();
        s.send("alice", 1, input()).unwrap();
        assert!(!serde_json::to_string(&s).unwrap().contains("attachments"));
        let mut v = serde_json::to_value(&s).unwrap();
        v["messages"]["mail-1"]["attachments"] = json!({"x.txt": "not base64!"});
        assert!(MailService.initialize(v, &context("alice")).is_err());
    }
    #[test]
    fn every_skin_shows_a_messages_files_as_links_that_download() {
        for skin in ["gmail", "outlook", "mailcom"] {
            let mut s = hosted();
            s.skin = web::Skin(skin.into());
            s.send(
                "alice",
                1,
                SendMail {
                    to: vec!["bob".into()],
                    subject: "Plan".into(),
                    attachments: with_files(&[("plan.md", &[b'x'; 2048])]),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut v = serde_json::to_value(s).unwrap();
            let page = Dom::of(&get(&mut v, "bob", "http://mail/?thread=mail-1"));
            assert!(page.has("read-mail-1-files"), "{skin}");
            assert_eq!(
                page.attr("read-mail-1-file-0", "href"),
                "/attachments/mail-1/plan.md"
            );
            assert_eq!(page.attr("read-mail-1-file-0", "download"), "plan.md");
            assert!(page.text("read-mail-1-file-0").contains("2 KB"), "{skin}");
        }
    }
}
