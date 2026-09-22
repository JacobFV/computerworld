//! A Discord server: roles, categories of text and voice channels, members with a
//! nickname and roles, replies and reactions. Everyone in the server can read every
//! channel unless the channel is held to particular roles; voice channels have
//! occupants rather than messages.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub mod time;

/// Discord's clock runs `HISTORY` ahead of the world's: `Message::time` is microseconds
/// since seven days before the world's epoch, so a seed can hold the week before the
/// world starts, and a message sent at world tick `t` is stored at `t + HISTORY`.
pub const HISTORY: u64 = 7 * time::DAY_US;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DiscordState {
    pub server: Server,
    pub next_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<cw_protocol::PageTheme>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Server {
    /// The slug in `/channels/{server}/{channel}`.
    pub id: String,
    pub name: String,
    /// Role name to its look and rank; a higher position lists first.
    pub roles: BTreeMap<String, Role>,
    /// The sidebar's order: each category names its channels, text or voice.
    pub categories: Vec<Category>,
    pub channels: BTreeMap<String, TextChannel>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub voice: BTreeMap<String, VoiceChannel>,
    pub members: BTreeMap<String, Member>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Role {
    pub color: String,
    pub position: u32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Category {
    pub name: String,
    pub channels: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TextChannel {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub topic: String,
    /// Roles that may see the channel; empty means everyone in the server.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub roles: BTreeSet<String>,
    pub messages: Vec<Message>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VoiceChannel {
    pub occupants: BTreeSet<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Member {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub nick: String,
    pub roles: BTreeSet<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Message {
    pub id: String,
    pub author: String,
    pub text: String,
    pub time: u64,
    pub reactions: BTreeMap<String, BTreeSet<String>>,
    /// A reply quotes the message it answers and stays in the transcript.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
}

impl DiscordState {
    /// The name a member goes by in this server.
    pub fn display(&self, who: &str) -> String {
        self.server
            .members
            .get(who)
            .map(|m| m.nick.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| who.to_owned())
    }
    /// The member's highest role, for its colour.
    pub fn top_role(&self, who: &str) -> Option<(&str, &Role)> {
        let member = self.server.members.get(who)?;
        member
            .roles
            .iter()
            .filter_map(|r| self.server.roles.get_key_value(r))
            .max_by_key(|(_, role)| role.position)
            .map(|(name, role)| (name.as_str(), role))
    }
    pub fn can_see(&self, actor: &str, channel: &TextChannel) -> bool {
        let Some(member) = self.server.members.get(actor) else {
            return false;
        };
        channel.roles.is_empty() || member.roles.iter().any(|r| channel.roles.contains(r))
    }
    pub fn channel(&self, actor: &str, id: &str) -> Result<&TextChannel, String> {
        self.server
            .channels
            .get(id)
            .filter(|c| self.can_see(actor, c))
            .ok_or("channel unavailable".into())
    }
    /// Text channels the actor can see, in sidebar order, then any not in a category.
    pub fn visible(&self, actor: &str) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .server
            .categories
            .iter()
            .flat_map(|c| c.channels.iter().map(String::as_str))
            .filter(|id| self.channel(actor, id).is_ok())
            .collect();
        for id in self.server.channels.keys() {
            if !out.contains(&id.as_str()) && self.channel(actor, id).is_ok() {
                out.push(id);
            }
        }
        out
    }
    pub fn send(
        &mut self,
        actor: &str,
        id: &str,
        text: &str,
        time: u64,
        reply_to: Option<&str>,
    ) -> Result<Message, String> {
        let channel = self.channel(actor, id)?;
        if text.trim().is_empty() {
            return Err("message required".into());
        }
        let reply_to = match reply_to.filter(|p| !p.is_empty()) {
            None => None,
            Some(p) if channel.messages.iter().any(|m| m.id == p) => Some(p.to_owned()),
            Some(_) => return Err("the message replied to is not in this channel".into()),
        };
        self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        while self.server.channels.values().any(|c| {
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
            reply_to,
        };
        self.server
            .channels
            .get_mut(id)
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
            .server
            .channels
            .get_mut(id)
            .ok_or("channel unavailable")?
            .messages
            .iter_mut()
            .find(|m| m.id == message)
            .ok_or("message unavailable")?;
        // A reaction is a toggle: the same click that adds one takes it back.
        let who = m.reactions.entry(reaction.into()).or_default();
        if !who.remove(actor) {
            who.insert(actor.into());
        }
        if who.is_empty() {
            m.reactions.remove(reaction);
        }
        Ok(())
    }
    /// Join a voice channel; a member is in at most one, so joining leaves the last.
    pub fn join_voice(&mut self, actor: &str, id: &str) -> Result<(), String> {
        if !self.server.members.contains_key(actor) {
            return Err("server unavailable".into());
        }
        if !self.server.voice.contains_key(id) {
            return Err("voice channel unavailable".into());
        }
        for v in self.server.voice.values_mut() {
            v.occupants.remove(actor);
        }
        self.server
            .voice
            .get_mut(id)
            .ok_or("voice channel unavailable")?
            .occupants
            .insert(actor.into());
        Ok(())
    }
    pub fn leave_voice(&mut self, actor: &str) -> Result<bool, String> {
        if !self.server.members.contains_key(actor) {
            return Err("server unavailable".into());
        }
        let mut left = false;
        for v in self.server.voice.values_mut() {
            left |= v.occupants.remove(actor);
        }
        Ok(left)
    }
}

use cw_protocol::{HttpRequest, HttpResponse, Result as SimResult};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Value};
pub mod page;
pub struct DiscordService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(DiscordService)
}
impl Service for DiscordService {
    fn kind(&self) -> &str {
        "discord"
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
        let s: DiscordState = web::load(&initial)?;
        for category in &s.server.categories {
            for id in &category.channels {
                if !s.server.channels.contains_key(id) && !s.server.voice.contains_key(id) {
                    return Err(cw_protocol::SimError::invalid(format!(
                        "category {} lists unknown channel {id}",
                        category.name
                    )));
                }
            }
        }
        for (who, member) in &s.server.members {
            if let Some(role) = member
                .roles
                .iter()
                .find(|r| !s.server.roles.contains_key(*r))
            {
                return Err(cw_protocol::SimError::invalid(format!(
                    "member {who} has unknown role {role}"
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
        let mut s: DiscordState = web::load(state)?;
        let p = web::path(r);
        let path = p.strip_prefix("/api").unwrap_or(&p);
        // A voice channel can carry a space in its name, which a URL writes as `%20`.
        let path = path.replace("%20", " ");
        let mut parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        // `/channels/{server}/{channel}` is the real shape; `/channels/{channel}` is the
        // short form seeded prose and scenes use, and `@me` is the home view.
        if parts.len() >= 2
            && parts[0] == "channels"
            && (parts[1] == s.server.id && !s.server.id.is_empty() || parts[1] == "@me")
        {
            parts.remove(1);
        }
        let api = p.starts_with("/api/");
        let method = r.method.to_ascii_uppercase();
        let view = page::View {
            tick: c.tick.saturating_add(HISTORY),
            // The member list is open unless `?members=0` closes it.
            members: web::query(r, "members").is_none_or(|v| v != "0"),
            reply_to: web::query(r, "reply_to").filter(|v| !v.is_empty()),
        };
        if method == "GET" {
            return match parts.as_slice() {
                [""] | ["channels"] if !api => page::server(&s, &c.actor, None, &view),
                // The header's search box: every message of a visible channel that
                // holds the query.
                ["search"] if !api => {
                    page::search(&s, &c.actor, &web::query(r, "q").unwrap_or_default(), &view)
                }
                ["channels"] => HttpResponse::json(
                    200,
                    &s.visible(&c.actor)
                        .into_iter()
                        .map(|id| (id.to_owned(), id.to_owned()))
                        .collect::<BTreeMap<_, _>>(),
                ),
                ["server"] if api => {
                    if !s.server.members.contains_key(&c.actor) {
                        return web::error(403, "server unavailable");
                    }
                    let mut v = json!(s.server);
                    v["channels"] = json!(s.visible(&c.actor));
                    HttpResponse::json(200, &v)
                }
                ["channels", id] if !api => page::server(&s, &c.actor, Some(id), &view),
                ["channels", id] | ["channels", id, "messages"] => {
                    web::domain(s.channel(&c.actor, id).map(|ch| json!(ch)))
                }
                _ => web::error(404, "route not found"),
            };
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let b = web::body(r)?;
        let landing = parts.get(1).map(|id| (*id).to_owned());
        let result = match parts.as_slice() {
            ["channels", id, "messages"] => s
                .send(
                    &c.actor,
                    id,
                    &web::text(&b, "text"),
                    view.tick,
                    Some(web::text(&b, "reply_to")).as_deref(),
                )
                .map(|m| json!(m)),
            ["channels", id, "messages", message, "reactions"] => s
                .react(&c.actor, id, message, &web::text(&b, "reaction"))
                .map(|_| json!({"ok":true})),
            ["voice", id, "join"] => s.join_voice(&c.actor, id).map(|_| json!({"ok":true})),
            ["voice", _, "leave"] | ["voice", "leave"] => s
                .leave_voice(&c.actor)
                .map(|left| json!({"ok":true,"left":left})),
            _ => return web::error(404, "route not found"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !api && result.is_ok() {
            let open = match parts.first() {
                Some(&"voice") => s.visible(&c.actor).first().map(|id| (*id).to_owned()),
                _ => landing,
            };
            page::server(&s, &c.actor, open.as_deref(), &view)
        } else {
            web::domain(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> DiscordState {
        let mut s = DiscordState::default();
        s.server.id = "atlas".into();
        s.server.name = "Atlas Community".into();
        s.server.roles.insert(
            "Admin".into(),
            Role {
                color: "#f00".into(),
                position: 3,
            },
        );
        s.server.roles.insert(
            "Member".into(),
            Role {
                color: "#fff".into(),
                position: 1,
            },
        );
        s.server.members.insert(
            "admin".into(),
            Member {
                nick: String::new(),
                roles: ["Admin".into()].into(),
            },
        );
        s.server.members.insert(
            "alice".into(),
            Member {
                nick: "alice.chen".into(),
                roles: ["Member".into()].into(),
            },
        );
        s.server
            .channels
            .insert("general".into(), TextChannel::default());
        s.server.channels.insert(
            "mod-log".into(),
            TextChannel {
                roles: ["Admin".into()].into(),
                ..Default::default()
            },
        );
        s.server
            .voice
            .insert("Lounge".into(), VoiceChannel::default());
        s.server.categories.push(Category {
            name: "COMMUNITY".into(),
            channels: vec!["general".into(), "Lounge".into()],
        });
        s
    }
    #[test]
    fn roles_gate_channels_and_voice_has_one_seat_per_member() {
        let mut s = state();
        assert!(s.channel("alice", "general").is_ok());
        assert!(s.channel("alice", "mod-log").is_err());
        assert!(s.channel("admin", "mod-log").is_ok());
        assert!(s.channel("eve", "general").is_err());
        assert_eq!(s.visible("admin"), vec!["general", "mod-log"]);
        let m = s.send("alice", "general", "hi", 1, None).unwrap();
        assert!(s
            .send("alice", "general", "reply", 2, Some("chat-99"))
            .is_err());
        let reply = s.send("admin", "general", "hello", 2, Some(&m.id)).unwrap();
        assert_eq!(reply.reply_to.as_deref(), Some(m.id.as_str()));
        s.react("admin", "general", &m.id, "eyes").unwrap();
        s.react("alice", "general", &m.id, "eyes").unwrap();
        s.react("admin", "general", &m.id, "eyes").unwrap();
        assert_eq!(
            s.server.channels["general"].messages[0].reactions["eyes"],
            ["alice".to_owned()].into()
        );
        s.react("alice", "general", &m.id, "eyes").unwrap();
        assert!(s.server.channels["general"].messages[0]
            .reactions
            .is_empty());
        assert_eq!(s.top_role("admin").unwrap().0, "Admin");
        assert_eq!(s.display("alice"), "alice.chen");
        s.join_voice("alice", "Lounge").unwrap();
        assert!(s.join_voice("alice", "Nowhere").is_err());
        assert!(s.join_voice("eve", "Lounge").is_err());
        assert!(s.server.voice["Lounge"].occupants.contains("alice"));
        assert!(s.leave_voice("alice").unwrap());
        assert!(!s.leave_voice("alice").unwrap());
        let restored: DiscordState =
            serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(s, restored);
    }
}
