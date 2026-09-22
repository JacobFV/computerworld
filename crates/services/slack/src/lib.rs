//! A Slack workspace: channels with a topic and a purpose, direct messages between two
//! people or a small group, threads under a message, reactions, pins, a member directory
//! with display names, titles and statuses, an unread count per person per conversation,
//! and @-mentions. Membership scopes everything: a channel is only there for the people
//! in it.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub mod time;

/// Slack's clock runs `HISTORY` ahead of the world's: `Message::time` is microseconds
/// since seven days before the world's epoch, so a seed can hold the week before the
/// world starts, and a message sent at world tick `t` is stored at `t + HISTORY`.
pub const HISTORY: u64 = 7 * time::DAY_US;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SlackState {
    pub workspace: String,
    pub channels: BTreeMap<String, Channel>,
    pub next_id: u64,
    /// Direct messages, keyed by the participants sorted and joined with `|`; two people
    /// or a group of them name exactly one conversation whichever of them opens it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dms: BTreeMap<String, Channel>,
    /// The member directory. A person in a channel but not here is still a member; the
    /// directory only adds a display name, a title and a status.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub members: BTreeMap<String, Member>,
    /// Person to conversation to the number of top-level messages they had seen when
    /// they last opened it; what is beyond that count is unread.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub seen: BTreeMap<String, BTreeMap<String, usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<cw_protocol::PageTheme>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Channel {
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub topic: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    pub members: BTreeSet<String>,
    pub messages: Vec<Message>,
    /// A private channel shows a lock instead of a hash; membership scopes it the same.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
    /// Message ids pinned to the channel, in the order they were pinned.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Member {
    pub display_name: String,
    pub title: String,
    pub status: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Message {
    pub id: String,
    pub author: String,
    pub text: String,
    pub time: u64,
    pub reactions: BTreeMap<String, BTreeSet<String>>,
    /// Set on a threaded reply; the parent stays in the main transcript.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}
impl Message {
    /// Everyone this message @-mentions, as written.
    pub fn mentions(&self) -> Vec<&str> {
        self.text
            .split(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | ';' | '(' | ')'))
            .filter_map(|w| w.strip_prefix('@'))
            .map(|w| w.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-'))
            .filter(|w| !w.is_empty())
            .collect()
    }
}

/// A DM key is the participants sorted and joined, so both sides address the same conversation.
pub fn dm_key<'a>(people: impl IntoIterator<Item = &'a str>) -> String {
    let set: BTreeSet<&str> = people.into_iter().collect();
    set.into_iter().collect::<Vec<_>>().join("|")
}

impl SlackState {
    pub fn channel(&self, actor: &str, id: &str) -> Result<&Channel, String> {
        self.conversations()
            .into_iter()
            .find(|(key, c)| *key == id && c.members.contains(actor))
            .map(|(_, c)| c)
            .ok_or("channel unavailable".into())
    }
    /// Channels first, then DMs; one namespace so every route works for both.
    fn conversations(&self) -> Vec<(&str, &Channel)> {
        self.channels
            .iter()
            .chain(&self.dms)
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }
    fn conversation_mut(&mut self, id: &str) -> Option<&mut Channel> {
        match self.channels.contains_key(id) {
            true => self.channels.get_mut(id),
            false => self.dms.get_mut(id),
        }
    }
    /// How a person shows in the workspace: their display name, or their user name.
    pub fn display(&self, who: &str) -> String {
        self.members
            .get(who)
            .map(|m| m.display_name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| who.to_owned())
    }
    /// Opening a DM is idempotent: the people already have exactly one conversation or
    /// gain one. Two people make a DM; more make a group DM.
    pub fn open_dm(&mut self, actor: &str, others: &[String]) -> Result<String, String> {
        let mut people = BTreeSet::from([actor.to_owned()]);
        for other in others {
            let other = other.trim();
            if other.is_empty() || other == actor {
                continue;
            }
            if !self.people().any(|who| who == other) {
                return Err("person unavailable".into());
            }
            people.insert(other.to_owned());
        }
        if people.len() < 2 {
            return Err("a direct message needs another person".into());
        }
        let key = dm_key(people.iter().map(String::as_str));
        let title = people.iter().cloned().collect::<Vec<_>>().join(", ");
        self.dms.entry(key.clone()).or_insert_with(|| Channel {
            title,
            members: people,
            ..Default::default()
        });
        Ok(key)
    }
    /// Everyone in the workspace: the directory plus anyone in a conversation.
    pub fn people(&self) -> impl Iterator<Item = &str> {
        let mut all: Vec<&str> = self
            .channels
            .values()
            .chain(self.dms.values())
            .flat_map(|c| c.members.iter().map(String::as_str))
            .chain(self.members.keys().map(String::as_str))
            .collect();
        all.sort_unstable();
        all.dedup();
        all.into_iter()
    }
    pub fn send(
        &mut self,
        actor: &str,
        id: &str,
        text: &str,
        time: u64,
        parent: Option<&str>,
    ) -> Result<Message, String> {
        let channel = self.channel(actor, id)?;
        if text.trim().is_empty() {
            return Err("message required".into());
        }
        let parent = match parent.filter(|p| !p.is_empty()) {
            None => None,
            Some(p) if channel.messages.iter().any(|m| m.id == p) => Some(p.to_owned()),
            Some(_) => return Err("parent message is not in this conversation".into()),
        };
        self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        while self.channels.values().chain(self.dms.values()).any(|c| {
            c.messages
                .iter()
                .any(|m| m.id == format!("chat-{}", self.next_id))
        }) {
            self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        }
        let m = Message {
            id: format!("chat-{}", self.next_id),
            author: actor.into(),
            text: text.into(),
            time,
            reactions: BTreeMap::new(),
            parent,
        };
        self.conversation_mut(id)
            .ok_or("channel unavailable")?
            .messages
            .push(m.clone());
        // The author has seen their own message.
        self.mark_seen(actor, id);
        Ok(m)
    }
    pub fn react(
        &mut self,
        actor: &str,
        id: &str,
        message: &str,
        reaction: &str,
    ) -> Result<(), String> {
        self.channel(actor, id)?;
        if reaction.trim().is_empty() {
            return Err("reaction required".into());
        }
        let m = self
            .conversation_mut(id)
            .ok_or("channel unavailable")?
            .messages
            .iter_mut()
            .find(|m| m.id == message)
            .ok_or("message unavailable")?;
        m.reactions
            .entry(reaction.into())
            .or_default()
            .insert(actor.into());
        Ok(())
    }
    /// Pin a message to its channel, or unpin it if it is pinned already.
    pub fn pin(&mut self, actor: &str, id: &str, message: &str) -> Result<bool, String> {
        self.channel(actor, id)?;
        let c = self.conversation_mut(id).ok_or("channel unavailable")?;
        if !c.messages.iter().any(|m| m.id == message) {
            return Err("message unavailable".into());
        }
        if let Some(at) = c.pins.iter().position(|p| p == message) {
            c.pins.remove(at);
            Ok(false)
        } else {
            c.pins.push(message.into());
            Ok(true)
        }
    }
    /// Set the actor's own status line.
    pub fn set_status(&mut self, actor: &str, status: &str) {
        self.members.entry(actor.into()).or_default().status = status.trim().into();
    }
    /// The actor has now seen everything in the conversation.
    pub fn mark_seen(&mut self, actor: &str, id: &str) {
        let count = self
            .conversation_mut(id)
            .map(|c| c.messages.len())
            .unwrap_or_default();
        self.seen
            .entry(actor.into())
            .or_default()
            .insert(id.into(), count);
    }
    /// Messages in the conversation beyond the ones the actor has seen.
    pub fn unread(&self, actor: &str, id: &str) -> usize {
        let Ok(c) = self.channel(actor, id) else {
            return 0;
        };
        let seen = self
            .seen
            .get(actor)
            .and_then(|s| s.get(id))
            .copied()
            .unwrap_or(0);
        c.messages.len().saturating_sub(seen)
    }
    /// Every unread @-mention of the actor across their conversations.
    pub fn mentions(&self, actor: &str) -> Vec<(&str, &Message)> {
        self.conversations()
            .into_iter()
            .filter(|(_, c)| c.members.contains(actor))
            .flat_map(|(id, c)| {
                let seen = self
                    .seen
                    .get(actor)
                    .and_then(|s| s.get(id))
                    .copied()
                    .unwrap_or(0);
                c.messages
                    .iter()
                    .skip(seen)
                    .filter(|m| m.mentions().contains(&actor))
                    .map(move |m| (id, m))
            })
            .collect()
    }
}

use cw_protocol::{HttpRequest, HttpResponse, Result as SimResult};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Value};
pub mod page;
pub struct SlackService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(SlackService)
}
fn channel_json(s: &SlackState, actor: &str, id: &str, c: &Channel) -> Value {
    let mut v = json!(c);
    v["id"] = json!(id);
    v["unread"] = json!(s.unread(actor, id));
    v
}
impl Service for SlackService {
    fn kind(&self) -> &str {
        "slack"
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
        let s: SlackState = web::load(&initial)?;
        for (key, dm) in &s.dms {
            let expected = dm_key(dm.members.iter().map(String::as_str));
            if *key != expected {
                return Err(cw_protocol::SimError::invalid(format!(
                    "dm {key} must be keyed by its members: {expected}"
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
        let mut s: SlackState = web::load(state)?;
        let p = web::path(r);
        let path = p.strip_prefix("/api").unwrap_or(&p);
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let api = p.starts_with("/api/");
        let method = r.method.to_ascii_uppercase();
        let now = c.tick.saturating_add(HISTORY);
        let view = page::View {
            tick: now,
            thread: web::query(r, "thread"),
            members: web::query(r, "members").is_some(),
        };
        let open = |s: &SlackState, id: Option<&str>| page::workspace(s, &c.actor, id, &view);
        let listing = |set: &BTreeMap<String, Channel>| {
            set.iter()
                .filter(|(_, ch)| ch.members.contains(&c.actor))
                .map(|(id, ch)| (id.clone(), ch.title.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        if method == "GET" {
            return match parts.as_slice() {
                [""] => open(&s, None),
                ["channels"] if !api => open(&s, None),
                // The top bar's search box, and the rail's Activity tab.
                ["search"] if !api => {
                    page::search(&s, &c.actor, &web::query(r, "q").unwrap_or_default(), &view)
                }
                ["activity"] if !api => page::activity(&s, &c.actor, &view),
                ["channels"] => HttpResponse::json(200, &listing(&s.channels)),
                // Opening the DMs is opening the first of them, as Slack's DMs tab does.
                ["dms"] if !api => {
                    let first = s
                        .dms
                        .iter()
                        .find(|(_, dm)| dm.members.contains(&c.actor))
                        .map(|(key, _)| key.clone());
                    open(&s, first.as_deref())
                }
                ["dms"] => HttpResponse::json(200, &listing(&s.dms)),
                ["members"] if api => HttpResponse::json(200, &s.members),
                ["mentions"] if api => HttpResponse::json(
                    200,
                    &s.mentions(&c.actor)
                        .into_iter()
                        .map(|(id, m)| json!({"channel": id, "message": m}))
                        .collect::<Vec<_>>(),
                ),
                ["unread"] if api => HttpResponse::json(
                    200,
                    &s.channels
                        .keys()
                        .chain(s.dms.keys())
                        .map(|id| (id.clone(), s.unread(&c.actor, id)))
                        .filter(|(_, n)| *n > 0)
                        .collect::<BTreeMap<_, _>>(),
                ),
                // Slack permalinks are `/archives/<channel>`; seeded prose links them that way.
                ["archives", id] if !api => open(&s, Some(id)),
                ["channels", id] | ["dms", id] if !api => open(&s, Some(id)),
                ["channels", id] | ["dms", id] | ["channels", id, "messages"] => web::domain(
                    s.channel(&c.actor, id)
                        .map(|ch| channel_json(&s, &c.actor, id, ch)),
                ),
                ["channels", id, "pins"] => web::domain(s.channel(&c.actor, id).map(|ch| {
                    ch.messages
                        .iter()
                        .filter(|m| ch.pins.contains(&m.id))
                        .collect::<Vec<_>>()
                })),
                _ => web::error(404, "route not found"),
            };
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let b = web::body(r)?;
        // A browser POST lands on the conversation it changed, as a redirect would.
        let mut landing = parts.get(1).map(|id| (*id).to_owned());
        let result = match parts.as_slice() {
            ["channels", id, "messages"] => s
                .send(
                    &c.actor,
                    id,
                    &web::text(&b, "text"),
                    now,
                    Some(web::text(&b, "parent")).as_deref(),
                )
                .map(|m| json!(m)),
            ["channels", id, "messages", message, "reactions"] => s
                .react(&c.actor, id, message, &web::text(&b, "reaction"))
                .map(|_| json!({"ok":true})),
            ["channels", id, "messages", message, "pin"] => s
                .pin(&c.actor, id, message)
                .map(|pinned| json!({"ok":true,"pinned":pinned})),
            ["channels", id, "read"] => s.channel(&c.actor, id).map(|_| ()).map(|_| {
                s.mark_seen(&c.actor, id);
                json!({"ok":true})
            }),
            ["dms"] => {
                let mut to = web::strings(&b, "to");
                if to.is_empty() {
                    to = vec![web::text(&b, "to")];
                }
                match s.open_dm(&c.actor, &to) {
                    Ok(key) => {
                        landing = Some(key.clone());
                        Ok(json!({"conversation": key}))
                    }
                    Err(e) => Err(e),
                }
            }
            ["status"] => {
                s.set_status(&c.actor, &web::text(&b, "status"));
                landing = None;
                Ok(json!({"ok":true}))
            }
            _ => return web::error(404, "route not found"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !api && result.is_ok() {
            // A reply lands back in its thread; anything else on the conversation.
            let view = page::View {
                thread: Some(web::text(&b, "parent")).filter(|p| !p.is_empty()),
                ..view
            };
            page::workspace(&s, &c.actor, landing.as_deref(), &view)
        } else {
            web::domain(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> SlackState {
        SlackState {
            workspace: "Northstar".into(),
            channels: BTreeMap::from([(
                "general".into(),
                Channel {
                    title: "general".into(),
                    topic: "Company-wide".into(),
                    members: ["alice", "bob"].into_iter().map(str::to_owned).collect(),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        }
    }
    #[test]
    fn membership_threads_pins_and_mentions() {
        let mut s = state();
        assert!(s.send("eve", "general", "hi", 0, None).is_err());
        let m = s
            .send("alice", "general", "@bob can you look?", 9, None)
            .unwrap();
        assert_eq!(m.mentions(), vec!["bob"]);
        assert_eq!(s.unread("bob", "general"), 1);
        assert_eq!(s.unread("alice", "general"), 0);
        assert_eq!(s.mentions("bob").len(), 1);
        s.mark_seen("bob", "general");
        assert_eq!(s.unread("bob", "general"), 0);
        assert!(s.mentions("bob").is_empty());
        assert!(s.send("bob", "general", "reply", 10, Some("nope")).is_err());
        let reply = s.send("bob", "general", "on it", 10, Some(&m.id)).unwrap();
        assert_eq!(reply.parent.as_deref(), Some(m.id.as_str()));
        assert!(s.pin("alice", "general", &m.id).unwrap());
        assert!(!s.pin("alice", "general", &m.id).unwrap());
        assert!(s.pin("eve", "general", &m.id).is_err());
        s.react("bob", "general", &m.id, "+1").unwrap();
        s.react("bob", "general", &m.id, "+1").unwrap();
        assert_eq!(s.channels["general"].messages[0].reactions["+1"].len(), 1);
        let restored: SlackState =
            serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(s, restored);
    }
    #[test]
    fn group_dms_are_one_conversation_for_their_people() {
        let mut s = state();
        s.members.insert("carol".into(), Member::default());
        let key = s.open_dm("alice", &["bob".into(), "carol".into()]).unwrap();
        assert_eq!(key, "alice|bob|carol");
        assert_eq!(
            s.open_dm("carol", &["alice".into(), "bob".into()]).unwrap(),
            key
        );
        assert_eq!(s.dms.len(), 1);
        assert!(s.open_dm("alice", &["nobody".into()]).is_err());
        assert!(s.open_dm("alice", &[]).is_err());
    }
}
