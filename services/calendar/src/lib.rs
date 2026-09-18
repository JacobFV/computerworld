//! Logical-microsecond calendars. No ambient date, timezone or host clock.
mod gcal;
mod time;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use time::{civil, days_in_month, heading, Civil, DAY_US, HOUR_US};
/// Skins this instance may wear; Google Calendar gets a branded layout, `plain` is calendar.internal.
pub const SKINS: &[&str] = &["plain", "gcal"];
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CalendarState {
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
    /// Where it happens, as prose; an empty location simply is not shown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub location: String,
    /// "Join the meeting" is a real link into the world, or nothing at all.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conference_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
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
impl Rsvp {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "Awaiting reply",
            Self::Accepted => "Going",
            Self::Declined => "Not going",
            Self::Tentative => "Maybe",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "accepted" => Some(Self::Accepted),
            "declined" => Some(Self::Declined),
            "tentative" => Some(Self::Tentative),
            "pending" => Some(Self::Pending),
            _ => None,
        }
    }
}
impl Event {
    /// Day indices this event touches, epoch-relative, first to last.
    pub fn days(&self) -> std::ops::RangeInclusive<u64> {
        self.start / DAY_US..=self.end.saturating_sub(1) / DAY_US
    }
    pub fn when(&self) -> String {
        let (from, to) = (civil(self.start), civil(self.end));
        if from.index == to.index {
            format!(
                "{} {} · {} – {}",
                from.weekday_name(),
                from.short(),
                from.clock(),
                to.clock()
            )
        } else {
            format!(
                "{} {} {} – {} {} {}",
                from.weekday_name(),
                from.short(),
                from.clock(),
                to.weekday_name(),
                to.short(),
                to.clock()
            )
        }
    }
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
            location: String::new(),
            conference_url: String::new(),
            description: String::new(),
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
    /// Visible events overlapping one epoch-relative day, earliest first.
    pub fn on_day(&self, actor: &str, day: u64) -> Vec<&Event> {
        let mut found = self.list(actor, day * DAY_US, (day + 1) * DAY_US);
        found.sort_by(|a, b| (a.start, &a.id).cmp(&(b.start, &b.id)));
        found
    }
    pub fn visible(&self, actor: &str, id: &str) -> Option<&Event> {
        self.events
            .get(id)
            .filter(|e| e.owner == actor || e.attendees.contains_key(actor))
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
        self.update(actor, id, None, Some(start), Some(end))
    }
    /// Owner-only edit. Every field is optional, so the same route renames an event, reschedules
    /// it, or does both; omitting the times keeps `move_event`'s behaviour for older callers.
    pub fn update(
        &mut self,
        actor: &str,
        id: &str,
        title: Option<&str>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<(), String> {
        if title.is_none() && start.is_none() && end.is_none() {
            return Err("nothing to update".into());
        }
        if title.is_some_and(|t| t.trim().is_empty()) {
            return Err("title required".into());
        }
        let current = self.events.get(id).ok_or("event unavailable")?;
        let (from, to) = (start.unwrap_or(current.start), end.unwrap_or(current.end));
        if to <= from {
            return Err("positive event duration required".into());
        }
        let e = self
            .events
            .get_mut(id)
            .filter(|e| e.owner == actor)
            .ok_or("event unavailable")?;
        if let Some(title) = title {
            e.title = title.trim().into();
        }
        e.start = from;
        e.end = to;
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
/// Where the reader is standing in a skinned calendar; `plain` ignores all of it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Nav {
    pub day: Option<u64>,
    pub event: Option<String>,
}
/// The original calendar page. Frozen: `calendar.internal` and its world data read these bytes.
fn plain(s: &CalendarState, actor: &str) -> SimResult<HttpResponse> {
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
fn view(s: &CalendarState, actor: &str, now: u64, nav: &Nav) -> SimResult<HttpResponse> {
    match s.skin.as_str() {
        "gcal" => gcal::week(s, actor, now, nav),
        _ => plain(s, actor),
    }
}
/// Optional integer field: absent means "leave this alone", not "zero".
fn optional(body: &Value, key: &str) -> SimResult<Option<u64>> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(_) => web::number(body, key).map(Some),
    }
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
        s.skin.check(SKINS)?;
        if let Some(theme) = &s.theme {
            cw_protocol::Page {
                version: 1,
                title: String::new(),
                elements: vec![],
                theme: Some(theme.clone()),
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
        let mut s: CalendarState = web::load(state)?;
        let p = web::path(r);
        let path = p.strip_prefix("/api").unwrap_or(&p);
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let api = p.starts_with("/api/");
        let skinned = !s.skin.is_plain();
        let method = r.method.to_ascii_uppercase();
        let nav = |event: Option<String>| Nav {
            day: web::query(r, "day").and_then(|d| d.parse().ok()),
            event: event.or_else(|| web::query(r, "event")),
        };
        if method == "GET" {
            return match parts.as_slice() {
                [""] => view(&s, &c.actor, c.tick, &nav(None)),
                ["events"] => {
                    let start = web::query(r, "start")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    let end = web::query(r, "end")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(u64::MAX);
                    HttpResponse::json(200, &s.list(&c.actor, start, end))
                }
                // Event permalinks exist only where a page links to them; `plain` keeps its
                // original route table, and therefore its original bytes.
                ["events", id] if skinned && !api => {
                    if s.visible(&c.actor, id).is_none() {
                        return web::error(404, "event unavailable");
                    }
                    view(&s, &c.actor, c.tick, &nav(Some((*id).to_owned())))
                }
                _ => web::error(404, "route not found"),
            };
        }
        let b = web::body(r)?;
        let mut land = Nav::default();
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
                .map(|e| {
                    land = Nav {
                        day: Some(e.start / DAY_US),
                        event: Some(e.id.clone()),
                    };
                    json!(e)
                }),
            ("POST", ["events", id, "rsvp"]) => {
                let Some(response) = Rsvp::parse(&web::text(&b, "response")) else {
                    return web::error(400, "invalid RSVP");
                };
                land = Nav {
                    day: s.visible(&c.actor, id).map(|e| e.start / DAY_US),
                    event: Some((*id).to_owned()),
                };
                s.respond(&c.actor, id, response)
                    .map(|_| json!({"ok":true}))
            }
            // Title, times, or both: a rename is an edit like any other.
            ("POST" | "PATCH", ["events", id]) => {
                let title = b
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty());
                let result = s.update(
                    &c.actor,
                    id,
                    title,
                    optional(&b, "start")?,
                    optional(&b, "end")?,
                );
                land = Nav {
                    day: s.visible(&c.actor, id).map(|e| e.start / DAY_US),
                    event: Some((*id).to_owned()),
                };
                result.map(|_| json!({"ok":true}))
            }
            // A page button can only POST, so a deletable event needs a POST spelling too.
            ("DELETE", ["events", id]) | ("POST", ["events", id, "delete"]) => {
                land = Nav {
                    day: s.visible(&c.actor, id).map(|e| e.start / DAY_US),
                    event: None,
                };
                s.delete(&c.actor, id).map(|_| json!({"ok":true}))
            }
            _ => return web::error(405, "unsupported route or method"),
        };
        if result.is_ok() {
            web::save(state, &s)?;
        }
        if !api && result.is_ok() {
            view(&s, &c.actor, c.tick, &land)
        } else {
            web::domain(result)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn context(actor: &str) -> ServiceContext {
        ServiceContext {
            actor: actor.into(),
            source: "pc".into(),
            tick: 3,
            seed: 1,
            instance: "calendar".into(),
        }
    }
    fn seeded() -> CalendarState {
        let mut s = CalendarState {
            skin: web::Skin("gcal".into()),
            ..Default::default()
        };
        s.create(
            "carol",
            "Atlas launch review",
            HOUR_US,
            2 * HOUR_US,
            vec!["alice".into(), "bob".into()],
            0,
        )
        .unwrap();
        let e = s.events.get_mut("event-1").unwrap();
        e.location = "Northstar HQ · Room 2".into();
        e.conference_url = "http://slack.com/archives/eng".into();
        s
    }
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
    fn a_title_can_be_edited_without_moving_the_event() {
        let mut s = seeded();
        let (start, end) = (s.events["event-1"].start, s.events["event-1"].end);
        s.update(
            "carol",
            "event-1",
            Some("Atlas launch review (final)"),
            None,
            None,
        )
        .unwrap();
        let e = &s.events["event-1"];
        assert_eq!(e.title, "Atlas launch review (final)");
        assert_eq!(
            (e.start, e.end),
            (start, end),
            "a rename must not reschedule"
        );
        // The old contract still holds for callers that send only times.
        s.move_event("carol", "event-1", 4 * HOUR_US, 5 * HOUR_US)
            .unwrap();
        assert_eq!(s.events["event-1"].title, "Atlas launch review (final)");
        assert!(s
            .update("alice", "event-1", Some("Hijacked"), None, None)
            .is_err());
        assert!(s
            .update("carol", "event-1", Some("  "), None, None)
            .is_err());
        assert!(s.update("carol", "event-1", None, None, None).is_err());
        assert!(s
            .update("carol", "event-1", None, Some(9 * HOUR_US), None)
            .is_err());
    }
    #[test]
    fn the_native_app_post_of_title_start_and_end_sticks() {
        // crates/applications/src/apps/calendar.rs sends exactly this body when saving an edit.
        let c = context("carol");
        let mut v = serde_json::to_value(seeded()).unwrap();
        let r = HttpRequest::json(
            "POST",
            "http://calendar/api/events/event-1",
            &json!({"title":"Launch review","start":DAY_US,"end":DAY_US + HOUR_US}),
        )
        .unwrap();
        assert_eq!(CalendarService.handle(&mut v, &c, &r).unwrap().status, 200);
        assert_eq!(v["events"]["event-1"]["title"], "Launch review");
        assert_eq!(v["events"]["event-1"]["start"], DAY_US);
    }
    #[test]
    fn page_pure() {
        let c = context("alice");
        let mut s = CalendarService.initialize(json!({}), &c).unwrap();
        let before = s.clone();
        let r = HttpRequest::get("http://calendar/");
        assert_eq!(
            CalendarService.handle(&mut s, &c, &r).unwrap(),
            CalendarService.handle(&mut s, &c, &r).unwrap()
        );
        assert_eq!(before, s);
    }
    #[test]
    fn plain_state_and_page_bytes_are_exactly_what_they_were_before_skins() {
        // The world file, existing checkpoints and the Playwright assertions read these bytes.
        let mut s = CalendarState::default();
        s.create(
            "carol",
            "Atlas launch review",
            3_600_000_000,
            5_400_000_000,
            vec!["alice".into()],
            0,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_string(&serde_json::to_value(&s).unwrap()).unwrap(),
            r#"{"events":{"event-1":{"attendees":{"alice":"pending"},"created":0,"end":5400000000,"id":"event-1","owner":"carol","start":3600000000,"title":"Atlas launch review"}},"next_id":1}"#
        );
        let page = plain(&s, "alice").unwrap();
        assert_eq!(
            String::from_utf8(page.body).unwrap(),
            r#"{"version":1,"title":"Calendar","elements":[{"kind":"heading","id":"title","text":"Calendar","level":1},{"kind":"form","id":"create","action":{"method":"POST","url":"/events","fields":{"attendees":"$create-attendees","end":"$create-end","start":"$create-start","title":"$create-title"}},"children":[{"kind":"input","id":"create-title","label":"Title","value":"","placeholder":""},{"kind":"input","id":"create-start","label":"Start (logical µs)","value":"0","placeholder":""},{"kind":"input","id":"create-end","label":"End (logical µs)","value":"1000","placeholder":""},{"kind":"input","id":"create-attendees","label":"Attendees","value":"","placeholder":""},{"kind":"button","id":"create-submit","text":"Submit","action":{"method":"POST","url":"/events","fields":{"attendees":"$create-attendees","end":"$create-end","start":"$create-start","title":"$create-title"}}}]},{"kind":"heading","id":"event-1","text":"Atlas launch review","level":1},{"kind":"text","id":"event-1-time","text":"3600000000–5400000000 · Owner carol · {\"alice\": Pending}"},{"kind":"form","id":"event-1-rsvp","action":{"method":"POST","url":"/events/event-1/rsvp","fields":{"response":"$event-1-rsvp-response"}},"children":[{"kind":"input","id":"event-1-rsvp-response","label":"Response","value":"accepted","placeholder":""},{"kind":"button","id":"event-1-rsvp-submit","text":"Submit","action":{"method":"POST","url":"/events/event-1/rsvp","fields":{"response":"$event-1-rsvp-response"}}}]}]}"#
        );
    }
    #[test]
    fn plain_route_table_is_unchanged() {
        let c = context("carol");
        let mut v = serde_json::to_value(CalendarState::default()).unwrap();
        let r = HttpRequest::get("http://calendar/events/event-1");
        assert_eq!(CalendarService.handle(&mut v, &c, &r).unwrap().status, 404);
    }
    #[test]
    fn skinned_week_renders_and_navigates() {
        let c = context("carol");
        let mut v = serde_json::to_value(seeded()).unwrap();
        let page = |v: &mut Value, url: &str| {
            let response = CalendarService
                .handle(v, &c, &HttpRequest::get(url))
                .unwrap();
            (response.status, String::from_utf8(response.body).unwrap())
        };
        let (status, body) = page(&mut v, "http://calendar/?day=0&event=event-1");
        assert_eq!(status, 200);
        assert!(body.contains("Atlas launch review"));
        assert!(body.contains("Thu 17 Sep") && body.contains("10:00"));
        // The conference link is navigation into the rest of the world, not decoration.
        assert!(body.contains("http://slack.com/archives/eng"));
        assert_eq!(page(&mut v, "http://calendar/events/event-1").0, 200);
        assert_eq!(page(&mut v, "http://calendar/events/event-9").0, 404);
        // Next week is a real link and shows no events, because the only one is this week.
        let (_, next) = page(&mut v, "http://calendar/?day=7");
        assert!(!next.contains("Atlas launch review"));
    }
    #[test]
    fn skinned_rsvp_and_edit_round_trip_through_the_page() {
        let alice = context("alice");
        let mut v = serde_json::to_value(seeded()).unwrap();
        let rsvp = HttpRequest::json(
            "POST",
            "http://calendar/events/event-1/rsvp",
            &json!({"response":"accepted"}),
        )
        .unwrap();
        let response = CalendarService.handle(&mut v, &alice, &rsvp).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(v["events"]["event-1"]["attendees"]["alice"], "accepted");
        assert!(String::from_utf8(response.body).unwrap().contains("Going"));
        let rename = HttpRequest::json(
            "POST",
            "http://calendar/events/event-1",
            &json!({"title":"Atlas launch review (final)"}),
        )
        .unwrap();
        assert_eq!(
            CalendarService
                .handle(&mut v, &context("carol"), &rename)
                .unwrap()
                .status,
            200
        );
        assert_eq!(
            v["events"]["event-1"]["title"],
            "Atlas launch review (final)"
        );
    }
    #[test]
    fn unknown_skin_is_a_seed_error() {
        assert!(CalendarService
            .initialize(json!({"skin":"fantastical"}), &context("alice"))
            .is_err());
    }
}
