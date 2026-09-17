//! Deterministic mail service: one authoritative message store, per-user mailbox metadata.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MailState {
    pub users: BTreeSet<String>,
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
    pub mailboxes: BTreeMap<String, Mailbox>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Mailbox {
    pub folders: BTreeSet<String>,
    pub labels: BTreeSet<String>,
    pub read: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SendMail {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
}
impl MailState {
    pub fn send(&mut self, actor: &str, time: u64, input: SendMail) -> Result<Message, String> {
        if !self.users.contains(actor) {
            return Err("unknown sender".into());
        }
        if input.to.is_empty() || input.subject.trim().is_empty() {
            return Err("recipient and subject required".into());
        }
        if input
            .to
            .iter()
            .chain(&input.cc)
            .any(|u| !self.users.contains(u))
        {
            return Err("unknown recipient".into());
        }
        let mut mailboxes = BTreeMap::<String, Mailbox>::new();
        for user in input.to.iter().chain(&input.cc) {
            mailboxes
                .entry(user.clone())
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
        let message = Message {
            id: format!("mail-{}", self.next_id),
            sender: actor.into(),
            to: input.to,
            cc: input.cc,
            subject: input.subject,
            body: input.body,
            time,
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
    pub fn metadata(
        &mut self,
        actor: &str,
        id: &str,
        read: Option<bool>,
        label: Option<String>,
        archive: bool,
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
        if archive {
            m.folders.remove("inbox");
            m.folders.insert("archive".into());
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
fn view(s: &MailState, actor: &str) -> SimResult<HttpResponse> {
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
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<HttpResponse> {
        let mut s: MailState = web::load(state)?;
        if !s.users.contains(&c.actor) {
            return web::error(403, "mailbox unavailable");
        }
        let p = web::path(r);
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            return match p.as_str() {
                "/" => view(&s, &c.actor),
                "/api/messages" => {
                    let f = web::query(r, "folder");
                    HttpResponse::json(
                        200,
                        &s.list(&c.actor, f.as_deref())
                            .into_iter()
                            .map(|m| {
                                let mut m = m.clone();
                                m.mailboxes.retain(|u, _| u == &c.actor);
                                m
                            })
                            .collect::<Vec<_>>(),
                    )
                }
                _ => web::error(404, "route not found"),
            };
        }
        let b = web::body(r)?;
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
            .map(|mut m| {
                m.mailboxes.retain(|u, _| u == &c.actor);
                json!(m)
            })
        } else if (method == "POST" || method == "PATCH")
            && (p.starts_with("/api/messages/") || p.starts_with("/messages/"))
        {
            let id = p.rsplit('/').next().unwrap();
            let read = b
                .get("read")
                .and_then(|v| v.as_bool().or_else(|| v.as_str()?.parse().ok()));
            s.metadata(
                &c.actor,
                id,
                read,
                b.get("label").and_then(Value::as_str).map(str::to_owned),
                b.get("archive").and_then(Value::as_bool).unwrap_or(false),
            )
            .map(|_| json!({"ok":true}))
        } else {
            return web::error(405, "unsupported route or method");
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !p.starts_with("/api/") && result.is_ok() {
            view(&s, &c.actor)
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
    fn input() -> SendMail {
        SendMail {
            to: vec!["bob".into()],
            subject: "Status".into(),
            body: "ready".into(),
            ..Default::default()
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
    fn sdk_authoritative_identity_restore_and_render_purity() {
        let c = ServiceContext {
            actor: "alice".into(),
            source: "pc".into(),
            tick: 3,
            seed: 1,
            instance: "mail".into(),
        };
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
}
