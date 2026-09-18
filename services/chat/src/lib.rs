//! Membership-scoped persistent chat service.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
/// Skins this instance may wear; Slack and Discord get branded layouts, `plain` is chat.internal.
pub const SKINS: &[&str] = &["plain", "slack", "discord"];
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ChatState {
    /// Presentation only. `plain` is the original rendering and is omitted from serialised
    /// state, so worlds and checkpoints written before skins existed stay byte-identical.
    #[serde(default, skip_serializing_if = "web::Skin::is_plain")]
    pub skin: web::Skin,
    pub channels: BTreeMap<String, Channel>,
    pub next_id: u64,
    /// Direct messages, keyed by the participants sorted and joined with `|`, so the pair
    /// alice/bob names exactly one conversation whichever of them opens it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dms: BTreeMap<String, Channel>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub workspace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<cw_protocol::PageTheme>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Channel {
    pub title: String,
    pub members: BTreeSet<String>,
    pub messages: Vec<Message>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: String,
    pub author: String,
    pub text: String,
    pub time: u64,
    pub reactions: BTreeMap<String, BTreeSet<String>>,
    /// Set on a threaded reply; the parent stays in the main transcript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}
/// A DM key is the participants sorted and joined, so both sides address the same conversation.
pub fn dm_key(a: &str, b: &str) -> String {
    let mut pair = [a, b];
    pair.sort_unstable();
    pair.join("|")
}
impl ChatState {
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
    /// Opening a DM is idempotent: the pair already has exactly one conversation or gains one.
    pub fn open_dm(&mut self, actor: &str, other: &str) -> Result<String, String> {
        let other = other.trim();
        if other.is_empty() || other == actor {
            return Err("a direct message needs another person".into());
        }
        if !self.people().any(|who| who == other) {
            return Err("person unavailable".into());
        }
        let key = dm_key(actor, other);
        self.dms.entry(key.clone()).or_insert_with(|| Channel {
            title: format!("{actor} and {other}"),
            members: [actor, other].into_iter().map(str::to_owned).collect(),
            messages: vec![],
        });
        Ok(key)
    }
    /// Everyone who is a member of something on this instance; the DM directory.
    pub fn people(&self) -> impl Iterator<Item = &str> {
        let mut all: Vec<&str> = self
            .channels
            .values()
            .chain(self.dms.values())
            .flat_map(|c| c.members.iter().map(String::as_str))
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
    ) -> Result<Message, String> {
        self.send_reply(actor, id, text, time, None)
    }
    pub fn send_reply(
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
}
use cw_protocol::{HttpRequest, HttpResponse, Result as SimResult};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Value};
pub mod skins;
pub struct ChatService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(ChatService)
}
fn view(s: &ChatState, actor: &str, channel: Option<&str>) -> SimResult<HttpResponse> {
    let mut e = vec![web::heading("title", "Chat")];
    for (id, c) in &s.channels {
        if c.members.contains(actor) {
            e.push(web::link(id, &c.title, format!("/channels/{id}")));
        }
    }
    if let Some(id) = channel {
        match s.channel(actor, id) {
            Err(e) => return web::error(403, e),
            Ok(c) => {
                e.push(web::heading("channel", &c.title));
                for m in &c.messages {
                    e.push(web::paragraph(
                        &m.id,
                        format!("{}: {} {:?}", m.author, m.text, m.reactions),
                    ));
                    e.push(web::form(
                        &format!("{}-react", m.id),
                        &format!("/channels/{id}/messages/{}/reactions", m.id),
                        &[("reaction", "Reaction", "")],
                    ));
                }
                e.push(web::form(
                    "send",
                    &format!("/channels/{id}/messages"),
                    &[("text", "Message", "")],
                ));
            }
        }
    }
    web::page("Chat", e)
}
impl Service for ChatService {
    fn kind(&self) -> &str {
        "chat"
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
        let s: ChatState = web::load(&initial)?;
        s.skin.check(SKINS)?;
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<HttpResponse> {
        let mut s: ChatState = web::load(state)?;
        let p = web::path(r);
        let path = p.strip_prefix("/api").unwrap_or(&p);
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let api = p.starts_with("/api/");
        let method = r.method.to_ascii_uppercase();
        // The plain skin keeps the original flat page; a branded instance gets the workspace.
        let skinned = (!s.skin.is_plain()).then(|| skins::look(s.skin.as_str()));
        let render = |s: &ChatState, open: Option<&str>| match &skinned {
            Some(look) => skins::workspace(s, &c.actor, open, look),
            None => view(s, &c.actor, open),
        };
        if method == "GET" {
            return match parts.as_slice() {
                [""] => render(&s, None),
                ["channels"] => HttpResponse::json(
                    200,
                    &s.channels
                        .iter()
                        .filter(|(_, ch)| ch.members.contains(&c.actor))
                        .map(|(id, ch)| (id, &ch.title))
                        .collect::<BTreeMap<_, _>>(),
                ),
                ["dms"] if !api => render(&s, None),
                ["dms"] => HttpResponse::json(
                    200,
                    &s.dms
                        .iter()
                        .filter(|(_, ch)| ch.members.contains(&c.actor))
                        .map(|(id, ch)| (id, &ch.title))
                        .collect::<BTreeMap<_, _>>(),
                ),
                // Slack permalinks are `/archives/<channel>`; seeded prose links them that way.
                ["archives", id] if !api => render(&s, Some(id)),
                ["channels", id] | ["dms", id] if !api => render(&s, Some(id)),
                ["channels", id] | ["dms", id] | ["channels", id, "messages"] => {
                    web::domain(s.channel(&c.actor, id).map(|ch| json!(ch)))
                }
                _ => web::error(404, "route not found"),
            };
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let b = web::body(r)?;
        // A browser POST lands on the conversation it changed, as a redirect would.
        let mut landing = parts.get(1).copied();
        let opened;
        let result = match parts.as_slice() {
            ["channels", id, "messages"] => s
                .send_reply(
                    &c.actor,
                    id,
                    &web::text(&b, "text"),
                    c.tick,
                    Some(web::text(&b, "parent")).as_deref(),
                )
                .map(|m| json!(m)),
            ["channels", id, "messages", message, "reactions"] => s
                .react(&c.actor, id, message, &web::text(&b, "reaction"))
                .map(|_| json!({"ok":true})),
            ["dms"] => match s.open_dm(&c.actor, &web::text(&b, "to")) {
                Ok(key) => {
                    opened = key;
                    landing = Some(&opened);
                    Ok(json!({"conversation": opened}))
                }
                Err(e) => Err(e),
            },
            _ => return web::error(404, "route not found"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !api && result.is_ok() {
            render(&s, landing)
        } else {
            web::domain(result)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> ChatState {
        ChatState {
            channels: BTreeMap::from([(
                "general".into(),
                Channel {
                    title: "General".into(),
                    members: ["alice", "bob"].into_iter().map(str::to_owned).collect(),
                    messages: vec![],
                },
            )]),
            ..Default::default()
        }
    }
    #[test]
    fn membership_messages_reactions_restore() {
        let mut s = state();
        let before = s.clone();
        assert!(s.send("eve", "general", "hi", 0).is_err());
        assert_eq!(s, before);
        let m = s.send("alice", "general", "hello", 9).unwrap();
        assert_eq!(s.channel("bob", "general").unwrap().messages[0], m);
        assert!(s.react("eve", "general", &m.id, "like").is_err());
        s.react("bob", "general", &m.id, "like").unwrap();
        s.react("bob", "general", &m.id, "like").unwrap();
        assert_eq!(
            s.channel("bob", "general").unwrap().messages[0].reactions["like"].len(),
            1
        );
        let restored: ChatState = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(s, restored);
    }
    #[test]
    fn rendering_and_api_share_state() {
        let c = ServiceContext {
            actor: "alice".into(),
            source: "pc".into(),
            tick: 3,
            seed: 1,
            instance: "chat".into(),
        };
        let mut v = serde_json::to_value(state()).unwrap();
        ChatService
            .handle(
                &mut v,
                &c,
                &HttpRequest::json(
                    "POST",
                    "http://chat/api/channels/general/messages",
                    &json!({"text":"unique message"}),
                )
                .unwrap(),
            )
            .unwrap();
        let before = v.clone();
        let response = ChatService
            .handle(
                &mut v,
                &c,
                &HttpRequest::get("http://chat/channels/general"),
            )
            .unwrap();
        assert!(String::from_utf8(response.body)
            .unwrap()
            .contains("unique message"));
        assert_eq!(before, v);
    }
}
