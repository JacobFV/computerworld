//! Logical-microsecond calendars. No ambient date, timezone or host clock.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CalendarState {
    pub events: BTreeMap<String, Event>,
    pub next_id: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    pub owner: String,
    pub title: String,
    pub start: u64,
    pub end: u64,
    pub created: u64,
    pub attendees: BTreeMap<String, Rsvp>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Rsvp {
    #[default]
    Pending,
    Accepted,
    Declined,
    Tentative,
}
impl CalendarState {
    pub fn create(
        &mut self,
        actor: &str,
        title: &str,
        start: u64,
        end: u64,
        attendees: Vec<String>,
        time: u64,
    ) -> Result<Event, String> {
        if title.trim().is_empty() || end <= start {
            return Err("title and positive event duration required".into());
        }
        self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        while self.events.contains_key(&format!("event-{}", self.next_id)) {
            self.next_id = self.next_id.checked_add(1).ok_or("ID space exhausted")?;
        }
        let e = Event {
            id: format!("event-{}", self.next_id),
            owner: actor.into(),
            title: title.into(),
            start,
            end,
            created: time,
            attendees: attendees.into_iter().map(|a| (a, Rsvp::Pending)).collect(),
        };
        self.events.insert(e.id.clone(), e.clone());
        Ok(e)
    }
    pub fn list(&self, actor: &str, start: u64, end: u64) -> Vec<&Event> {
        self.events
            .values()
            .filter(|e| {
                (e.owner == actor || e.attendees.contains_key(actor))
                    && e.start < end
                    && e.end > start
            })
            .collect()
    }
    pub fn respond(&mut self, actor: &str, id: &str, response: Rsvp) -> Result<(), String> {
        let r = self
            .events
            .get_mut(id)
            .and_then(|e| e.attendees.get_mut(actor))
            .ok_or("invitation unavailable")?;
        *r = response;
        Ok(())
    }
    pub fn move_event(
        &mut self,
        actor: &str,
        id: &str,
        start: u64,
        end: u64,
    ) -> Result<(), String> {
        if end <= start {
            return Err("positive event duration required".into());
        }
        let e = self
            .events
            .get_mut(id)
            .filter(|e| e.owner == actor)
            .ok_or("event unavailable")?;
        e.start = start;
        e.end = end;
        Ok(())
    }
    pub fn delete(&mut self, actor: &str, id: &str) -> Result<(), String> {
        if !self.events.get(id).is_some_and(|e| e.owner == actor) {
            return Err("event unavailable".into());
        }
        self.events.remove(id);
        Ok(())
    }
}
use cw_protocol::{HttpRequest, HttpResponse, Result as SimResult};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde_json::{json, Value};
pub struct CalendarService;
pub fn register(registry: &mut Registry) -> SimResult<()> {
    registry.register(CalendarService)
}
fn view(s: &CalendarState, actor: &str) -> SimResult<HttpResponse> {
    let mut elements = vec![
        web::heading("title", "Calendar"),
        web::form(
            "create",
            "/events",
            &[
                ("title", "Title", ""),
                ("start", "Start (logical µs)", "0"),
                ("end", "End (logical µs)", "1000"),
                ("attendees", "Attendees", ""),
            ],
        ),
    ];
    for e in s.list(actor, 0, u64::MAX) {
        elements.push(web::heading(&e.id, &e.title));
        elements.push(web::paragraph(
            &format!("{}-time", e.id),
            format!(
                "{}–{} · Owner {} · {:?}",
                e.start, e.end, e.owner, e.attendees
            ),
        ));
        if e.attendees.contains_key(actor) {
            elements.push(web::form(
                &format!("{}-rsvp", e.id),
                &format!("/events/{}/rsvp", e.id),
                &[("response", "Response", "accepted")],
            ));
        }
        if e.owner == actor {
            elements.push(web::form(
                &format!("{}-move", e.id),
                &format!("/events/{}", e.id),
                &[
                    ("start", "Start", &e.start.to_string()),
                    ("end", "End", &e.end.to_string()),
                ],
            ));
        }
    }
    web::page("Calendar", elements)
}
impl Service for CalendarService {
    fn kind(&self) -> &str {
        "calendar"
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
        let s: CalendarState = web::load(&initial)?;
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> SimResult<HttpResponse> {
        let mut s: CalendarState = web::load(state)?;
        let p = web::path(r);
        let path = p.strip_prefix("/api").unwrap_or(&p);
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let api = p.starts_with("/api/");
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            return match parts.as_slice() {
                [""] => view(&s, &c.actor),
                ["events"] => {
                    let start = web::query(r, "start")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    let end = web::query(r, "end")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(u64::MAX);
                    HttpResponse::json(200, &s.list(&c.actor, start, end))
                }
                _ => web::error(404, "route not found"),
            };
        }
        let b = web::body(r)?;
        let result = match (method.as_str(), parts.as_slice()) {
            ("POST", ["events"]) => s
                .create(
                    &c.actor,
                    &web::text(&b, "title"),
                    web::number(&b, "start")?,
                    web::number(&b, "end")?,
                    web::strings(&b, "attendees"),
                    c.tick,
                )
                .map(|e| json!(e)),
            ("POST", ["events", id, "rsvp"]) => {
                let response = match web::text(&b, "response").as_str() {
                    "accepted" => Rsvp::Accepted,
                    "declined" => Rsvp::Declined,
                    "tentative" => Rsvp::Tentative,
                    "pending" => Rsvp::Pending,
                    _ => return web::error(400, "invalid RSVP"),
                };
                s.respond(&c.actor, id, response)
                    .map(|_| json!({"ok":true}))
            }
            ("POST" | "PATCH", ["events", id]) => s
                .move_event(
                    &c.actor,
                    id,
                    web::number(&b, "start")?,
                    web::number(&b, "end")?,
                )
                .map(|_| json!({"ok":true})),
            ("DELETE", ["events", id]) => s.delete(&c.actor, id).map(|_| json!({"ok":true})),
            _ => return web::error(405, "unsupported route or method"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !api && result.is_ok() {
            view(&s, &c.actor)
        } else {
            web::domain(result)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invitations_scope_time_and_restore() {
        let mut s = CalendarState::default();
        assert!(s.create("alice", "Bad", 10, 10, vec![], 0).is_err());
        assert!(s.events.is_empty());
        let e = s
            .create("alice", "Planning", 10, 20, vec!["bob".into()], 1)
            .unwrap();
        assert_eq!(s.list("bob", 0, 15).len(), 1);
        assert!(s.list("eve", 0, 30).is_empty());
        assert!(s.list("bob", 20, 30).is_empty());
        assert!(s.respond("eve", &e.id, Rsvp::Accepted).is_err());
        s.respond("bob", &e.id, Rsvp::Accepted).unwrap();
        assert!(s.move_event("bob", &e.id, 30, 40).is_err());
        s.move_event("alice", &e.id, 30, 40).unwrap();
        let restored: CalendarState =
            serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(s, restored);
        assert!(s.delete("bob", &e.id).is_err());
        s.delete("alice", &e.id).unwrap();
        assert!(s.events.is_empty());
    }
    #[test]
    fn page_pure() {
        let c = ServiceContext {
            actor: "alice".into(),
            source: "pc".into(),
            tick: 3,
            seed: 1,
            instance: "calendar".into(),
        };
        let mut s = CalendarService.initialize(json!({}), &c).unwrap();
        let before = s.clone();
        let r = HttpRequest::get("http://calendar/");
        assert_eq!(
            CalendarService.handle(&mut s, &c, &r).unwrap(),
            CalendarService.handle(&mut s, &c, &r).unwrap()
        );
        assert_eq!(before, s);
    }
}
