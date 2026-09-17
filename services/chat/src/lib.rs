//! Membership-scoped persistent chat service.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ChatState {
    pub channels: BTreeMap<String, Channel>,
    pub next_id: u64,
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
}
impl ChatState {
    pub fn channel(&self, actor: &str, id: &str) -> Result<&Channel, String> {
        self.channels
            .get(id)
            .filter(|c| c.members.contains(actor))
            .ok_or("channel unavailable".into())
    }
    pub fn send(
        &mut self,
        actor: &str,
        id: &str,
        text: &str,
        time: u64,
    ) -> Result<Message, String> {
        self.channel(actor, id)?;
        if text.trim().is_empty() {
            return Err("message required".into());
        }
        self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        while self.channels.values().any(|c| {
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
        };
        self.channels.get_mut(id).unwrap().messages.push(m.clone());
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
            .channels
            .get_mut(id)
            .unwrap()
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
        if method == "GET" {
            return match parts.as_slice() {
                [""] => view(&s, &c.actor, None),
                ["channels"] => HttpResponse::json(
                    200,
                    &s.channels
                        .iter()
                        .filter(|(_, ch)| ch.members.contains(&c.actor))
                        .map(|(id, ch)| (id, &ch.title))
                        .collect::<BTreeMap<_, _>>(),
                ),
                ["channels", id] if !api => view(&s, &c.actor, Some(id)),
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
        let result = match parts.as_slice() {
            ["channels", id, "messages"] => s
                .send(&c.actor, id, &web::text(&b, "text"), c.tick)
                .map(|m| json!(m)),
            ["channels", id, "messages", message, "reactions"] => s
                .react(&c.actor, id, message, &web::text(&b, "reaction"))
                .map(|_| json!({"ok":true})),
            _ => return web::error(404, "route not found"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !api && result.is_ok() {
            view(&s, &c.actor, parts.get(1).copied())
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
