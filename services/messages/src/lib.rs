//! iMessage/SMS-style messaging: people are reached by handle (an E.164 number or an
//! Apple-ID-style address), a conversation is the set of handles in it, and every
//! message carries the service it went over, who it was delivered to, who has read it
//! and the tapbacks it collected. This is texting, not a workspace: there are no
//! channels, no threads and no membership beyond being in the conversation.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The tapbacks iOS offers, in the order the picker shows them.
pub const TAPBACKS: &[&str] = &[
    "loved",
    "liked",
    "disliked",
    "laughed",
    "emphasized",
    "questioned",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MessagesState {
    /// Simulation user to the handles they answer to; the first handle is the one they
    /// send from.
    pub contacts: BTreeMap<String, Contact>,
    /// Keyed by the participant handles sorted and joined with `|`, so any participant
    /// opening the same set of people lands in the same thread.
    pub conversations: BTreeMap<String, Conversation>,
    pub next_id: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Contact {
    pub name: String,
    pub handles: Vec<String>,
    /// Whether the handle is registered with iMessage; without it, texts to this person
    /// fall back to SMS and carry no receipts.
    #[serde(default = "yes")]
    pub imessage: bool,
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Conversation {
    pub participants: BTreeSet<String>,
    /// A group can be named; a two-party thread never is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub messages: Vec<Message>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Message {
    pub id: String,
    /// The sender's handle.
    pub from: String,
    pub text: String,
    pub time: u64,
    /// `imessage` or `sms`.
    pub service: String,
    /// Handles the message reached. An iMessage is delivered to every other participant
    /// as it is sent; an SMS reports nothing.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub delivered: BTreeSet<String>,
    /// Handle to the tick they read it at.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub read: BTreeMap<String, u64>,
    /// Tapback to the handles that gave it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tapbacks: BTreeMap<String, BTreeSet<String>>,
}

/// A conversation key is its handles sorted and joined, so every participant addresses
/// the same thread.
pub fn conversation_key<'a>(handles: impl IntoIterator<Item = &'a str>) -> String {
    let set: BTreeSet<&str> = handles.into_iter().collect();
    set.into_iter().collect::<Vec<_>>().join("|")
}

impl MessagesState {
    /// The handle an actor sends from, or none when the actor is not in the contacts.
    pub fn handle_of(&self, actor: &str) -> Option<&str> {
        self.contacts
            .get(actor)
            .and_then(|c| c.handles.first())
            .map(String::as_str)
    }
    fn handle(&self, actor: &str) -> Result<String, String> {
        self.handle_of(actor)
            .map(str::to_owned)
            .ok_or_else(|| "handle unavailable: the actor has no number or address here".into())
    }
    /// The contact behind a handle, if the handle is known.
    pub fn contact_for(&self, handle: &str) -> Option<(&str, &Contact)> {
        self.contacts
            .iter()
            .find(|(_, c)| c.handles.iter().any(|h| h == handle))
            .map(|(user, c)| (user.as_str(), c))
    }
    /// The name a handle shows as: the contact's name, or the raw handle for a stranger.
    pub fn display(&self, handle: &str) -> String {
        self.contact_for(handle)
            .map(|(_, c)| c.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| handle.to_owned())
    }
    /// Resolve a person as typed: a handle, or a contact's user name.
    pub fn resolve(&self, who: &str) -> Option<String> {
        let who = who.trim();
        if who.is_empty() {
            return None;
        }
        if self.contact_for(who).is_some() {
            return Some(who.to_owned());
        }
        self.handle_of(who).map(str::to_owned)
    }
    /// The title a participant sees: the group's name, or the other people in it.
    pub fn title(&self, conversation: &Conversation, me: &str) -> String {
        if let Some(name) = conversation.name.as_deref().filter(|n| !n.is_empty()) {
            return name.to_owned();
        }
        let others: Vec<String> = conversation
            .participants
            .iter()
            .filter(|h| h.as_str() != me)
            .map(|h| self.display(h))
            .collect();
        if others.is_empty() {
            "Note to self".into()
        } else {
            others.join(", ")
        }
    }
    /// Which service a message to these people goes over: iMessage only when every
    /// other participant is a known iMessage contact.
    pub fn service_for(&self, participants: &BTreeSet<String>, me: &str) -> &'static str {
        let all_imessage = participants
            .iter()
            .filter(|h| h.as_str() != me)
            .all(|h| self.contact_for(h).is_some_and(|(_, c)| c.imessage));
        if all_imessage {
            "imessage"
        } else {
            "sms"
        }
    }
    /// A participant's view of a conversation; anyone else is refused.
    pub fn conversation(&self, actor: &str, id: &str) -> Result<(String, &Conversation), String> {
        let me = self.handle(actor)?;
        let c = self
            .conversations
            .get(id)
            .filter(|c| c.participants.contains(&me))
            .ok_or("conversation unavailable")?;
        Ok((me, c))
    }
    /// Every conversation the actor is in, newest activity first.
    pub fn inbox(&self, actor: &str) -> Vec<(&str, &Conversation)> {
        let Some(me) = self.handle_of(actor) else {
            return vec![];
        };
        let mut all: Vec<_> = self
            .conversations
            .iter()
            .filter(|(_, c)| c.participants.contains(me))
            .map(|(k, c)| (k.as_str(), c))
            .collect();
        all.sort_by_key(|(_, c)| std::cmp::Reverse(c.messages.last().map_or(0, |m| m.time)));
        all
    }
    /// Messages from other people that this handle has not read.
    pub fn unread(conversation: &Conversation, me: &str) -> usize {
        conversation
            .messages
            .iter()
            .filter(|m| m.from != me && !m.read.contains_key(me))
            .count()
    }
    /// Opening a thread with some people is idempotent: the set of handles already has
    /// exactly one conversation, or gains one. A group may be named.
    pub fn open(
        &mut self,
        actor: &str,
        to: &[String],
        name: Option<&str>,
    ) -> Result<String, String> {
        let me = self.handle(actor)?;
        let mut participants = BTreeSet::from([me.clone()]);
        for who in to {
            let handle = self
                .resolve(who)
                .ok_or_else(|| format!("person unavailable: {who}"))?;
            participants.insert(handle);
        }
        if participants.len() < 2 {
            return Err("a conversation needs someone else in it".into());
        }
        let key = conversation_key(participants.iter().map(String::as_str));
        let name = name.map(str::trim).filter(|n| !n.is_empty());
        let group = participants.len() > 2;
        let entry = self
            .conversations
            .entry(key.clone())
            .or_insert_with(|| Conversation {
                participants,
                name: None,
                messages: vec![],
            });
        if let (true, Some(name)) = (group, name) {
            entry.name = Some(name.to_owned());
        }
        Ok(key)
    }
    pub fn send(
        &mut self,
        actor: &str,
        id: &str,
        text: &str,
        time: u64,
    ) -> Result<Message, String> {
        let (me, c) = self.conversation(actor, id)?;
        if text.trim().is_empty() {
            return Err("message required".into());
        }
        let service = self.service_for(&c.participants, &me);
        let delivered: BTreeSet<String> = if service == "imessage" {
            c.participants
                .iter()
                .filter(|h| **h != me)
                .cloned()
                .collect()
        } else {
            BTreeSet::new()
        };
        self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        while self.conversations.values().any(|c| {
            c.messages
                .iter()
                .any(|m| m.id == format!("sms-{}", self.next_id))
        }) {
            self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        }
        let m = Message {
            id: format!("sms-{}", self.next_id),
            from: me,
            text: text.into(),
            time,
            service: service.into(),
            delivered,
            read: BTreeMap::new(),
            tapbacks: BTreeMap::new(),
        };
        self.conversations
            .get_mut(id)
            .ok_or("conversation unavailable")?
            .messages
            .push(m.clone());
        Ok(m)
    }
    /// Everything from other people in the thread is now read by the actor, at `time`.
    /// Only an iMessage carries a receipt; an SMS is read silently.
    pub fn mark_read(&mut self, actor: &str, id: &str, time: u64) -> Result<usize, String> {
        let (me, _) = self.conversation(actor, id)?;
        let c = self
            .conversations
            .get_mut(id)
            .ok_or("conversation unavailable")?;
        let mut newly = 0;
        for m in c.messages.iter_mut().filter(|m| m.from != me) {
            if m.service == "imessage" && !m.read.contains_key(&me) {
                m.read.insert(me.clone(), time);
                newly += 1;
            }
        }
        Ok(newly)
    }
    /// Give or take back a tapback; giving one already given takes it back, as on iOS.
    pub fn tapback(
        &mut self,
        actor: &str,
        id: &str,
        message: &str,
        tapback: &str,
    ) -> Result<bool, String> {
        let (me, _) = self.conversation(actor, id)?;
        if !TAPBACKS.contains(&tapback) {
            return Err(format!(
                "unknown tapback; expected one of {}",
                TAPBACKS.join(", ")
            ));
        }
        let m = self
            .conversations
            .get_mut(id)
            .ok_or("conversation unavailable")?
            .messages
            .iter_mut()
            .find(|m| m.id == message)
            .ok_or("message unavailable")?;
        let given = m.tapbacks.entry(tapback.into()).or_default();
        let added = given.insert(me.clone());
        if !added {
            given.remove(&me);
        }
        if given.is_empty() {
            m.tapbacks.remove(tapback);
        }
        Ok(added)
    }
}

use cw_protocol::{HttpRequest, HttpResponse, Result as SimResult};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Value};
pub mod page;
pub struct MessagesService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(MessagesService)
}

/// The API's view of a conversation for one participant: the thread plus the names to
/// show, the actor's own handle and the title, so a client needs no second lookup.
fn conversation_json(s: &MessagesState, id: &str, me: &str, c: &Conversation) -> Value {
    let people: BTreeMap<&str, String> = c
        .participants
        .iter()
        .map(|h| (h.as_str(), s.display(h)))
        .collect();
    json!({
        "id": id,
        "title": s.title(c, me),
        "name": c.name,
        "me": me,
        "participants": c.participants,
        "people": people,
        "service": s.service_for(&c.participants, me),
        "unread": MessagesState::unread(c, me),
        "messages": c.messages,
    })
}
fn inbox_json(s: &MessagesState, actor: &str) -> Value {
    let me = s.handle_of(actor).unwrap_or_default();
    Value::Array(
        s.inbox(actor)
            .into_iter()
            .map(|(id, c)| {
                let last = c.messages.last();
                json!({
                    "id": id,
                    "title": s.title(c, me),
                    "participants": c.participants,
                    "service": s.service_for(&c.participants, me),
                    "unread": MessagesState::unread(c, me),
                    "preview": last.map(|m| m.text.clone()).unwrap_or_default(),
                    "time": last.map_or(0, |m| m.time),
                })
            })
            .collect(),
    )
}
fn contacts_json(s: &MessagesState) -> Value {
    Value::Array(
        s.contacts
            .iter()
            .map(|(user, c)| {
                json!({"user": user, "name": c.name, "handles": c.handles, "imessage": c.imessage})
            })
            .collect(),
    )
}

impl Service for MessagesService {
    fn kind(&self) -> &str {
        "messages"
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
        let s: MessagesState = web::load(&initial)?;
        for (user, contact) in &s.contacts {
            if contact.handles.is_empty() {
                return Err(cw_protocol::SimError::invalid(format!(
                    "contact {user} has no handle"
                )));
            }
        }
        for (key, c) in &s.conversations {
            let expected = conversation_key(c.participants.iter().map(String::as_str));
            if *key != expected {
                return Err(cw_protocol::SimError::invalid(format!(
                    "conversation {key} must be keyed by its participants: {expected}"
                )));
            }
            if let Some(m) = c
                .messages
                .iter()
                .find(|m| !matches!(m.service.as_str(), "imessage" | "sms"))
            {
                return Err(cw_protocol::SimError::invalid(format!(
                    "message {} has service {:?}; expected imessage or sms",
                    m.id, m.service
                )));
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
        let mut s: MessagesState = web::load(state)?;
        let p = web::path(r);
        let path = p.strip_prefix("/api").unwrap_or(&p);
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let api = p.starts_with("/api/");
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            return match parts.as_slice() {
                [""] => page::inbox(&s, &c.actor),
                ["conversations"] if !api => page::inbox(&s, &c.actor),
                ["conversations"] => HttpResponse::json(200, &inbox_json(&s, &c.actor)),
                ["contacts"] if api => HttpResponse::json(200, &contacts_json(&s)),
                ["conversations", id] if !api => page::thread(&s, &c.actor, id),
                ["conversations", id] | ["conversations", id, "messages"] => web::domain(
                    s.conversation(&c.actor, id)
                        .map(|(me, conv)| conversation_json(&s, id, &me, conv)),
                ),
                _ => web::error(404, "route not found"),
            };
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let b = web::body(r)?;
        let mut landing = parts.get(1).map(|id| (*id).to_owned());
        let result = match parts.as_slice() {
            ["conversations"] => {
                let name = web::text(&b, "name");
                match s.open(
                    &c.actor,
                    &web::strings(&b, "to"),
                    (!name.is_empty()).then_some(name.as_str()),
                ) {
                    Ok(key) => {
                        landing = Some(key.clone());
                        Ok(json!({"conversation": key}))
                    }
                    Err(e) => Err(e),
                }
            }
            ["conversations", id, "messages"] => s
                .send(&c.actor, id, &web::text(&b, "text"), c.tick)
                .map(|m| json!(m)),
            // Reading marks the thread and answers with it, so a client that opens a
            // conversation does both in one round trip, as Messages does.
            ["conversations", id, "read"] => s.mark_read(&c.actor, id, c.tick).and_then(|_| {
                s.conversation(&c.actor, id)
                    .map(|(me, conv)| conversation_json(&s, id, &me, conv))
            }),
            ["conversations", id, "messages", message, "tapbacks"] => s
                .tapback(&c.actor, id, message, &web::text(&b, "tapback"))
                .map(|added| json!({"ok": true, "added": added})),
            _ => return web::error(404, "route not found"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        match (api, result) {
            (false, Ok(_)) => match landing {
                Some(id) => page::thread(&s, &c.actor, &id),
                None => page::inbox(&s, &c.actor),
            },
            (_, result) => web::domain(result),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> MessagesState {
        let mut s = MessagesState::default();
        for (user, name, handle, imessage) in [
            ("alice", "Alice Chen", "+14155550100", true),
            ("bob", "Bob Martinez", "+14155550101", true),
            ("carol", "Carol Okafor", "+14155550102", true),
            ("dentist", "Bayview Dental", "+14155550199", false),
        ] {
            s.contacts.insert(
                user.into(),
                Contact {
                    name: name.into(),
                    handles: vec![handle.into()],
                    imessage,
                },
            );
        }
        s
    }
    #[test]
    fn a_thread_is_the_people_in_it_and_only_they_can_read_it() {
        let mut s = state();
        let key = s.open("alice", &["bob".into()], None).unwrap();
        assert_eq!(key, "+14155550100|+14155550101");
        assert_eq!(s.open("bob", &["+14155550100".into()], None).unwrap(), key);
        assert_eq!(s.conversations.len(), 1);
        assert!(s.open("alice", &[], None).is_err());
        assert!(s.open("eve", &["bob".into()], None).is_err());
        let m = s.send("alice", &key, "lunch?", 5).unwrap();
        assert_eq!(m.service, "imessage");
        assert_eq!(m.delivered, BTreeSet::from(["+14155550101".to_owned()]));
        assert!(s.conversation("carol", &key).is_err());
        assert_eq!(
            MessagesState::unread(&s.conversations[&key], "+14155550101"),
            1
        );
        assert_eq!(s.mark_read("bob", &key, 7).unwrap(), 1);
        assert_eq!(s.conversations[&key].messages[0].read["+14155550101"], 7);
        assert_eq!(s.mark_read("bob", &key, 8).unwrap(), 0);
    }
    #[test]
    fn a_stranger_to_imessage_gets_sms_and_no_receipts() {
        let mut s = state();
        let key = s.open("alice", &["dentist".into()], None).unwrap();
        let m = s.send("alice", &key, "running late", 3).unwrap();
        assert_eq!(m.service, "sms");
        assert!(m.delivered.is_empty());
        assert_eq!(s.mark_read("dentist", &key, 4).unwrap(), 0);
        assert!(s.conversations[&key].messages[0].read.is_empty());
    }
    #[test]
    fn groups_are_named_and_tapbacks_toggle() {
        let mut s = state();
        let key = s
            .open(
                "alice",
                &["bob".into(), "carol".into()],
                Some("Launch crew"),
            )
            .unwrap();
        assert_eq!(s.conversations[&key].name.as_deref(), Some("Launch crew"));
        assert_eq!(
            s.title(&s.conversations[&key], "+14155550100"),
            "Launch crew"
        );
        let m = s.send("bob", &key, "pizza at 12?", 1).unwrap();
        assert!(s.tapback("alice", &key, &m.id, "loved").unwrap());
        assert!(s.tapback("eve", &key, &m.id, "loved").is_err());
        assert!(s.tapback("alice", &key, &m.id, "wow").is_err());
        assert!(!s.tapback("alice", &key, &m.id, "loved").unwrap());
        assert!(s.conversations[&key].messages[0].tapbacks.is_empty());
        let restored: MessagesState =
            serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(s, restored);
    }
}
