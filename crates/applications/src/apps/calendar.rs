//! Calendar backed by the `calendar` service. Events are real service records; the grid
//! is derived from simulation time only. Nothing here invents an appointment.
use super::look::{action, header, look, notice, FAINT, INK, LINE, MUTED};
use super::{push_bounded, Status};
use crate::desktop_scene::shared::{Align, CalendarDate, MONTHS, WEEKDAYS};
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

/// One simulated day, in the microseconds the world clock counts.
pub const DAY_US: u64 = 24 * 60 * 60 * 1_000_000;
const HOUR_US: u64 = 60 * 60 * 1_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Month,
    Day,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub owner: String,
    pub title: String,
    pub start: u64,
    pub end: u64,
}

/// Composer for a new or edited event. `editing` holds the service id when editing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub title: String,
    pub day: u64,
    pub hour: u64,
    pub length_hours: u64,
    pub editing: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calendar {
    pub base: String,
    pub view: View,
    /// Day index the grid is anchored on, counted from the world epoch.
    pub cursor_day: u64,
    pub events: Vec<Event>,
    pub selected: Option<String>,
    pub draft: Option<Draft>,
    pub status: Status,
}
impl Calendar {
    pub const KIND: &'static str = "calendar";
    pub fn launch(argument: &str, window: u64, clock_us: u64) -> (Self, Vec<AppEffect>) {
        // `<service>#<date>` opens on a day; a bare `<date>` is a day on the default service.
        let (base, place) = match argument.split_once('#') {
            Some((base, place)) => (base, place),
            None if day_of(argument).is_some() => ("", argument),
            None => (argument, ""),
        };
        let base = if base.is_empty() {
            "http://calendar.internal/".into()
        } else {
            base.to_owned()
        };
        let today = day_of(place).unwrap_or(clock_us / DAY_US);
        let opened_on_a_day = !place.is_empty() && day_of(place).is_some();
        let app = Self {
            base,
            // Opening on a particular day means looking at that day.
            view: if opened_on_a_day {
                View::Day
            } else {
                View::Month
            },
            cursor_day: today,
            events: vec![],
            selected: None,
            draft: None,
            status: Status::Loading,
        };
        let effects = app.fetch(window);
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, _theme: DesktopTheme) -> String {
        "Calendar".into()
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        let date = self.cursor_date();
        format!("{} {}", date.month_name(), date.year)
    }
    pub fn modified(&self) -> bool {
        self.draft.is_some()
    }
    fn url(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.base.trim_end_matches('/'))
    }
    /// Ask for the whole month around the cursor, so paging a month is one request.
    fn fetch(&self, window: u64) -> Vec<AppEffect> {
        let first = self.cursor_day.saturating_sub(45);
        vec![AppEffect::Http {
            window,
            tag: "events".into(),
            method: "GET".into(),
            url: self.url(&format!(
                "/api/events?start={}&end={}",
                first * DAY_US,
                (first + 120) * DAY_US
            )),
            body: String::new(),
        }]
    }
    pub fn cursor_date(&self) -> CalendarDate {
        CalendarDate::from_clock(self.cursor_day * DAY_US)
    }
    /// Events that overlap `day`, earliest first.
    pub fn events_on(&self, day: u64) -> Vec<&Event> {
        let (start, end) = (day * DAY_US, (day + 1) * DAY_US);
        let mut found: Vec<_> = self
            .events
            .iter()
            .filter(|e| e.start < end && e.end > start)
            .collect();
        found.sort_by_key(|e| (e.start, e.id.clone()));
        found
    }
    pub fn offline(&mut self, _tag: &str, reason: &str) {
        self.status = Status::Offline(reason.to_owned());
    }
    pub fn http(
        &mut self,
        window: u64,
        tag: &str,
        status: u16,
        body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        self.status = Status::from_status(status, body);
        if self.status != Status::Idle {
            return Ok(vec![]);
        }
        match tag {
            "events" => {
                self.events = serde_json::from_str(body).unwrap_or_default();
                self.events
                    .sort_by(|a, b| (a.start, &a.id).cmp(&(b.start, &b.id)));
                Ok(vec![])
            }
            // A mutation refetches, so the screen only ever shows committed state.
            "create" | "update" | "delete" | "rsvp" => {
                self.draft = None;
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            other => Err(format!("unexpected calendar reply {other}")),
        }
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        let draft = self.draft.as_mut().ok_or("no event is being edited")?;
        push_bounded(&mut draft.title, text, 160);
        Ok(())
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        match key {
            "Backspace" => {
                self.draft
                    .as_mut()
                    .ok_or("no event is being edited")?
                    .title
                    .pop();
                Ok(vec![])
            }
            "Enter" if self.draft.is_some() => self.click(window, "cal:save", clock_us),
            "Escape" => {
                self.draft = None;
                Ok(vec![])
            }
            other => Err(format!("unsupported calendar key {other}")),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("cal:")
            .ok_or("interaction does not belong to the calendar")?;
        match command {
            "prev" | "next" => {
                let date = self.cursor_date();
                let step = if self.view == View::Day {
                    1
                } else if command == "prev" {
                    date.day.max(1)
                } else {
                    date.days_in_month - date.day + 1
                };
                self.cursor_day = if command == "prev" {
                    self.cursor_day.saturating_sub(step)
                } else {
                    self.cursor_day + step
                };
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            "today" => {
                self.cursor_day = clock_us / DAY_US;
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            "month" | "day" => {
                self.view = if command == "month" {
                    View::Month
                } else {
                    View::Day
                };
                Ok(vec![])
            }
            "reload" => {
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            "new" => {
                self.draft = Some(Draft {
                    title: String::new(),
                    day: self.cursor_day,
                    hour: 9,
                    length_hours: 1,
                    editing: None,
                });
                Ok(vec![])
            }
            "cancel" => {
                self.draft = None;
                Ok(vec![])
            }
            "earlier" | "later" => {
                let draft = self.draft.as_mut().ok_or("no event is being edited")?;
                draft.hour = if command == "earlier" {
                    draft.hour.saturating_sub(1)
                } else {
                    (draft.hour + 1).min(23)
                };
                Ok(vec![])
            }
            "shorter" | "longer" => {
                let draft = self.draft.as_mut().ok_or("no event is being edited")?;
                draft.length_hours = if command == "shorter" {
                    draft.length_hours.saturating_sub(1).max(1)
                } else {
                    (draft.length_hours + 1).min(12)
                };
                Ok(vec![])
            }
            "save" => {
                let draft = self.draft.clone().ok_or("no event is being edited")?;
                if draft.title.trim().is_empty() {
                    return Err("an event needs a title".into());
                }
                let start = draft.day * DAY_US + draft.hour * HOUR_US;
                let end = start + draft.length_hours.max(1) * HOUR_US;
                let body = serde_json::json!({
                    "title": draft.title,
                    "start": start,
                    "end": end,
                })
                .to_string();
                self.status = Status::Loading;
                Ok(vec![AppEffect::Http {
                    window,
                    tag: if draft.editing.is_some() {
                        "update".into()
                    } else {
                        "create".into()
                    },
                    method: "POST".into(),
                    url: match &draft.editing {
                        Some(id) => self.url(&format!("/api/events/{id}")),
                        None => self.url("/api/events"),
                    },
                    body,
                }])
            }
            "delete" => {
                let id = self.selected.clone().ok_or("no event is selected")?;
                self.selected = None;
                self.status = Status::Loading;
                Ok(vec![AppEffect::Http {
                    window,
                    tag: "delete".into(),
                    method: "DELETE".into(),
                    url: self.url(&format!("/api/events/{id}")),
                    body: String::new(),
                }])
            }
            rest => {
                if let Some(day) = rest.strip_prefix("day:") {
                    self.cursor_day = day.parse().map_err(|_| "invalid day")?;
                    self.view = View::Day;
                    return Ok(vec![]);
                }
                if let Some(id) = rest.strip_prefix("event:") {
                    let event = self
                        .events
                        .iter()
                        .find(|e| e.id == id)
                        .ok_or("event not found")?;
                    self.selected = Some(event.id.clone());
                    self.cursor_day = event.start / DAY_US;
                    return Ok(vec![]);
                }
                if let Some(id) = rest.strip_prefix("edit:") {
                    let event = self
                        .events
                        .iter()
                        .find(|e| e.id == id)
                        .ok_or("event not found")?;
                    self.draft = Some(Draft {
                        title: event.title.clone(),
                        day: event.start / DAY_US,
                        hour: (event.start % DAY_US) / HOUR_US,
                        length_hours: ((event.end.saturating_sub(event.start)) / HOUR_US).max(1),
                        editing: Some(event.id.clone()),
                    });
                    return Ok(vec![]);
                }
                if let Some(rest) = rest.strip_prefix("rsvp:") {
                    let (id, response) = rest.split_once(':').ok_or("invalid RSVP")?;
                    self.status = Status::Loading;
                    return Ok(vec![AppEffect::Http {
                        window,
                        tag: "rsvp".into(),
                        method: "POST".into(),
                        url: self.url(&format!("/api/events/{id}/rsvp")),
                        body: serde_json::json!({ "response": response }).to_string(),
                    }]);
                }
                Err(format!("unknown calendar command {command}"))
            }
        }
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        let date = self.cursor_date();
        page.elements.push(E::Heading {
            id: "cal-title".into(),
            text: format!("{} {}", date.month_name(), date.year),
            level: 2,
        });
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "cal-status".into(),
                text: text.into(),
            });
        }
        for (id, label) in [
            ("cal:prev", "Previous"),
            ("cal:next", "Next"),
            ("cal:today", "Today"),
            ("cal:month", "Month"),
            ("cal:day", "Day"),
            ("cal:new", "New event"),
            ("cal:reload", "Reload"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
        for event in self.events_on(self.cursor_day) {
            page.elements.push(E::Button {
                id: format!("cal:event:{}", event.id),
                text: format!("{} — {}", clock_label(event.start), event.title),
                action: act(&format!("cal:event:{}", event.id)),
            });
            page.elements.push(E::Button {
                id: format!("cal:edit:{}", event.id),
                text: format!("Edit {}", event.title),
                action: act(&format!("cal:edit:{}", event.id)),
            });
        }
        if let Some(draft) = &self.draft {
            page.elements.push(E::Input {
                id: "cal-draft-title".into(),
                label: "Title".into(),
                value: draft.title.clone(),
                placeholder: "New event".into(),
            });
            for (id, label) in [
                ("cal:earlier", "Earlier"),
                ("cal:later", "Later"),
                ("cal:shorter", "Shorter"),
                ("cal:longer", "Longer"),
                ("cal:save", "Save"),
                ("cal:cancel", "Cancel"),
            ] {
                page.elements.push(E::Button {
                    id: id.into(),
                    text: label.into(),
                    action: act(id),
                });
            }
        }
        if self.selected.is_some() {
            page.elements.push(E::Button {
                id: "cal:delete".into(),
                text: "Delete".into(),
                action: act("cal:delete"),
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let date = self.cursor_date();
        let title = format!("{} {}", date.month_name(), date.year);
        let mut top = header(p, theme, &l, width, &title);
        // Toolbar: every control here moves real state.
        let bar = 38;
        p.box_(Rect::new(0, top, width, bar), l.chrome, 0);
        p.hline(0, top + bar as i32, width, LINE);
        // The New event button owns the right end; chips stop before it rather than
        // overlapping it on a narrow screen.
        let limit = width as i32 - 116;
        let mut x = 8;
        for (target, label) in [
            ("cal:prev", "‹"),
            ("cal:today", "Today"),
            ("cal:next", "›"),
            ("cal:month", "Month"),
            ("cal:day", "Day"),
            ("cal:reload", "Reload"),
        ] {
            let w = p.measure(label, 12, false) + 22;
            if x + w as i32 > limit {
                break;
            }
            let r = Rect::new(x, top + 6, w, 26);
            let on = (target == "cal:month" && self.view == View::Month)
                || (target == "cal:day" && self.view == View::Day);
            p.button(
                r,
                if on { l.selection } else { Color::TRANSPARENT },
                l.radius,
                target,
                label,
            );
            p.label(
                r.x,
                r.y + 4,
                r.width,
                label,
                12,
                if on { l.accent } else { INK },
                on,
                Align::Center,
            );
            x += w as i32 + 4;
        }
        let new = Rect::new(width as i32 - 106, top + 6, 96, 26);
        action(p, &l, new, "New event", "cal:new", true);
        top += bar as i32 + 1;
        if let Some(text) = self.status.notice() {
            notice(p, width, top + 10, text);
            top += 30;
        }
        match self.view {
            View::Month => self.month(p, theme, &l, width, height, top, date),
            View::Day => self.day(p, &l, width, height, top),
        }
        if let Some(draft) = &self.draft {
            self.composer(p, &l, width, height, draft);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn month(
        &self,
        p: &mut Painter,
        theme: DesktopTheme,
        l: &super::look::Look,
        width: u32,
        height: u32,
        top: i32,
        date: CalendarDate,
    ) {
        let columns = 7;
        let cell_w = width / columns;
        let head = 22;
        for (index, name) in WEEKDAYS.iter().enumerate() {
            let short = if theme.mobile() {
                &name[..1]
            } else {
                &name[..3]
            };
            p.label(
                index as i32 * cell_w as i32,
                top + 4,
                cell_w,
                short,
                11,
                MUTED,
                false,
                Align::Center,
            );
        }
        p.hline(0, top + head, width, LINE);
        // The grid always starts on the Sunday of the first week of this month. Day
        // indices count from the world's first day, so a cell earlier than that has no
        // index at all and is drawn inert rather than mislabelled.
        let first_day = self.cursor_day as i64 - (date.day as i64 - 1);
        let lead = date.first_weekday;
        let rows = (lead + date.days_in_month).div_ceil(7).max(1);
        let body = height.saturating_sub((top + head) as u32 + 2);
        let cell_h = (body / rows as u32).max(28);
        let today = self.cursor_day;
        for slot in 0..(rows * 7) {
            let column = slot % 7;
            let row = slot / 7;
            let r = Rect::new(
                column as i32 * cell_w as i32,
                top + head + 1 + row as i32 * cell_h as i32,
                cell_w,
                cell_h,
            );
            if slot < lead || slot >= lead + date.days_in_month {
                p.box_(r, Color(0, 0, 0, 8), 0);
                continue;
            }
            let day_number = slot - lead + 1;
            let index = first_day + (day_number as i64 - 1);
            let Ok(day) = u64::try_from(index) else {
                // Before the world began: label the date, offer nothing to click.
                p.vline(r.x, r.y, cell_h, LINE);
                p.hline(r.x, r.y, cell_w, LINE);
                p.label(
                    r.x + 4,
                    r.y + 3,
                    cell_w.saturating_sub(8),
                    &day_number.to_string(),
                    11,
                    FAINT,
                    false,
                    Align::Left,
                );
                continue;
            };
            p.button(
                r,
                if day == today {
                    l.selection
                } else {
                    Color::TRANSPARENT
                },
                0,
                &format!("cal:day:{day}"),
                &format!("{} {day_number}", date.month_name()),
            );
            p.vline(r.x, r.y, cell_h, LINE);
            p.hline(r.x, r.y, cell_w, LINE);
            p.label(
                r.x + 4,
                r.y + 3,
                cell_w.saturating_sub(8),
                &day_number.to_string(),
                11,
                if day == today { l.accent } else { INK },
                day == today,
                Align::Left,
            );
            for (index, event) in self.events_on(day).iter().take(3).enumerate() {
                let chip = Rect::new(
                    r.x + 3,
                    r.y + 18 + index as i32 * 15,
                    cell_w.saturating_sub(6),
                    13,
                );
                if chip.y + 13 > r.y + cell_h as i32 {
                    break;
                }
                p.button(
                    chip,
                    l.selection,
                    3,
                    &format!("cal:event:{}", event.id),
                    &event.title,
                );
                p.left(
                    chip.x + 4,
                    chip.y,
                    chip.width.saturating_sub(8),
                    &event.title,
                    10,
                    INK,
                );
            }
        }
    }
    fn day(&self, p: &mut Painter, l: &super::look::Look, width: u32, height: u32, top: i32) {
        let date = self.cursor_date();
        p.strong(
            14,
            top + 6,
            width.saturating_sub(28),
            &format!("{} {} {}", date.weekday_name(), date.day, date.month_name()),
            14,
            INK,
        );
        let mut y = top + 32;
        let events = self.events_on(self.cursor_day);
        if events.is_empty() {
            notice(p, width, y + 20, "No events");
            return;
        }
        for event in events {
            if y as u32 + l.row > height {
                break;
            }
            let r = Rect::new(8, y, width.saturating_sub(16), l.row);
            let selected = self.selected.as_deref() == Some(event.id.as_str());
            p.button(
                r,
                if selected {
                    l.selection
                } else {
                    Color(0, 0, 0, 8)
                },
                l.radius,
                &format!("cal:event:{}", event.id),
                &event.title,
            );
            p.box_(Rect::new(r.x, r.y, 3, r.height), l.accent, 0);
            p.left(r.x + 12, r.y + 5, 110, &clock_label(event.start), 12, MUTED);
            p.left(
                r.x + 120,
                r.y + 5,
                r.width.saturating_sub(220),
                &event.title,
                13,
                INK,
            );
            let edit = Rect::new(r.x + r.width as i32 - 96, r.y + 4, 44, r.height - 8);
            action(p, l, edit, "Edit", &format!("cal:edit:{}", event.id), false);
            if selected {
                let del = Rect::new(r.x + r.width as i32 - 48, r.y + 4, 44, r.height - 8);
                action(p, l, del, "Delete", "cal:delete", false);
            }
            y += l.row as i32 + 6;
        }
    }
    fn composer(
        &self,
        p: &mut Painter,
        l: &super::look::Look,
        width: u32,
        height: u32,
        draft: &Draft,
    ) {
        let h = 176.min(height.saturating_sub(20));
        let r = Rect::new(
            12,
            height as i32 - h as i32 - 10,
            width.saturating_sub(24),
            h,
        );
        p.drop_shadow(r, l.radius, 18, 60, 6);
        p.border(r, l.surface, l.radius, LINE);
        p.strong(
            r.x + 14,
            r.y + 12,
            r.width.saturating_sub(28),
            if draft.editing.is_some() {
                "Edit event"
            } else {
                "New event"
            },
            14,
            INK,
        );
        let field = Rect::new(r.x + 14, r.y + 38, r.width.saturating_sub(28), 30);
        p.border(field, Color::WHITE, 5, LINE);
        p.region(field, "cal:title", "Event title");
        p.left(
            field.x + 8,
            field.y + 7,
            field.width.saturating_sub(16),
            if draft.title.is_empty() {
                "Title"
            } else {
                &draft.title
            },
            13,
            if draft.title.is_empty() { FAINT } else { INK },
        );
        let date = CalendarDate::from_clock(draft.day * DAY_US);
        p.left(
            r.x + 14,
            r.y + 78,
            r.width.saturating_sub(28),
            &format!(
                "{} {} {} · {} for {} hour{}",
                date.weekday_name(),
                date.day,
                MONTHS[(date.month - 1) as usize],
                clock_label(draft.hour * HOUR_US),
                draft.length_hours,
                if draft.length_hours == 1 { "" } else { "s" }
            ),
            12,
            MUTED,
        );
        let mut x = r.x + 14;
        for (target, label) in [
            ("cal:earlier", "Earlier"),
            ("cal:later", "Later"),
            ("cal:shorter", "Shorter"),
            ("cal:longer", "Longer"),
        ] {
            let w = p.measure(label, 12, false) + 20;
            action(p, l, Rect::new(x, r.y + 102, w, 28), label, target, false);
            x += w as i32 + 6;
        }
        action(
            p,
            l,
            Rect::new(r.x + 14, r.y + 138, 92, 28),
            "Save",
            "cal:save",
            true,
        );
        action(
            p,
            l,
            Rect::new(r.x + 114, r.y + 138, 92, 28),
            "Cancel",
            "cal:cancel",
            false,
        );
    }
}
/// Day index, counted from the world's first day, of a `YYYY-MM-DD` date or a bare index.
/// `None` for anything else, and for a date before the world began, which has no index.
pub fn day_of(text: &str) -> Option<u64> {
    if let Ok(index) = text.parse::<u64>() {
        return Some(index);
    }
    let mut parts = text.split('-');
    let (year, month, day): (u64, u64, u64) = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    if parts.next().is_some() || !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    // Walk month by month from the world's start; bounded, so a far date is refused.
    let mut index = 0u64;
    for step in 0..1200u64 {
        let here = CalendarDate::from_clock(index * DAY_US);
        if (here.year, here.month) == (year, month) {
            if day > here.days_in_month {
                return None;
            }
            // `here` is the first representable day of this month.
            let offset = day.checked_sub(here.day)?;
            return Some(index + offset);
        }
        if (here.year, here.month) > (year, month) {
            return None;
        }
        index += here.days_in_month - here.day + 1;
        let _ = step;
    }
    None
}
/// 24-hour clock label for a world timestamp.
fn clock_label(us: u64) -> String {
    let minutes = (us % DAY_US) / 60_000_000;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> Calendar {
        let (mut app, effects) = Calendar::launch("http://calendar.internal/", 1, 0);
        assert!(matches!(effects.as_slice(), [AppEffect::Http { tag, .. }] if tag == "events"));
        app.http(
            1,
            "events",
            200,
            r#"[{"id":"event-1","owner":"alice","title":"Standup","start":32400000000,"end":34200000000}]"#,
        )
        .unwrap();
        app
    }
    #[test]
    fn events_come_from_the_service_and_land_on_the_right_day() {
        let app = app();
        assert_eq!(app.status, Status::Idle);
        assert_eq!(app.events_on(0).len(), 1);
        assert_eq!(app.events_on(1).len(), 0);
        assert_eq!(clock_label(32_400_000_000), "09:00");
    }
    #[test]
    fn creating_an_event_posts_real_fields_and_refetches() {
        let mut app = app();
        app.click(1, "cal:new", 0).unwrap();
        app.text("Design review").unwrap();
        app.click(1, "cal:later", 0).unwrap();
        app.click(1, "cal:longer", 0).unwrap();
        let effects = app.click(1, "cal:save", 0).unwrap();
        let AppEffect::Http {
            method,
            url,
            body,
            tag,
            ..
        } = &effects[0]
        else {
            panic!("expected a request");
        };
        assert_eq!((method.as_str(), tag.as_str()), ("POST", "create"));
        assert_eq!(url, "http://calendar.internal/api/events");
        let sent: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(sent["title"], "Design review");
        assert_eq!(sent["start"], 10 * HOUR_US);
        assert_eq!(sent["end"], 12 * HOUR_US);
        // The reply clears the draft and refetches rather than guessing the new state.
        let more = app.http(1, "create", 200, "{}").unwrap();
        assert!(app.draft.is_none());
        assert!(matches!(more.as_slice(), [AppEffect::Http { tag, .. }] if tag == "events"));
    }
    #[test]
    fn an_empty_title_is_refused_and_transport_failure_is_state() {
        let mut app = app();
        app.click(1, "cal:new", 0).unwrap();
        assert!(app.click(1, "cal:save", 0).is_err());
        app.offline("events", "network unreachable");
        assert_eq!(app.status, Status::Offline("network unreachable".into()));
        app.http(1, "events", 403, r#"{"error":"calendar unavailable"}"#)
            .unwrap();
        assert_eq!(app.status, Status::Denied("calendar unavailable".into()));
    }
    #[test]
    fn every_painted_control_is_one_the_model_accepts() {
        let app = app();
        let mut scene = Painter::themed(DesktopTheme::Macos, 900, 640, 0);
        app.render(
            &mut scene,
            &crate::AppEnv {
                theme: DesktopTheme::Macos,
                width: 900,
                height: 640,
                clock_us: 0,
                settings: &crate::SystemSettings::DEFAULT,
                clipboard: None,
                share_to: None,
                files: Default::default(),
            },
        );
        let targets: Vec<_> = scene
            .scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
            .collect();
        assert!(!targets.is_empty());
        for target in targets {
            let mut app = app.clone();
            if target == "cal:title" {
                continue; // The composer's field is only live while a draft exists.
            }
            assert!(
                app.click(1, &target, 0).is_ok(),
                "unhandled control {target}"
            );
        }
    }
    #[test]
    fn a_date_names_the_right_day_and_one_before_the_world_began_names_none() {
        // The world starts on Thursday 17 September 2026, which is day 0.
        assert_eq!(day_of("2026-09-17"), Some(0));
        assert_eq!(day_of("2026-09-18"), Some(1));
        assert_eq!(day_of("2026-09-30"), Some(13));
        assert_eq!(day_of("2026-10-01"), Some(14));
        assert_eq!(day_of("2027-01-01"), Some(106));
        // Leap day is real in 2028 and not in 2027.
        assert!(day_of("2028-02-29").is_some());
        assert_eq!(day_of("2027-02-29"), None);
        // Before the world began there is no day to open.
        assert_eq!(day_of("2026-09-16"), None);
        assert_eq!(day_of("2026-09-01"), None);
        for bad in [
            "",
            "2026-13-01",
            "2026-09-00",
            "2026-09-31",
            "yesterday",
            "2026-09",
        ] {
            assert_eq!(day_of(bad), None, "{bad}");
        }
        assert_eq!(day_of("40"), Some(40));
    }
    #[test]
    fn opening_on_a_date_keeps_the_service_and_shows_that_day() {
        let (app, effects) = Calendar::launch("http://calendar.google.com/#2026-10-01", 1, 0);
        assert_eq!(app.base, "http://calendar.google.com/");
        assert_eq!(app.cursor_day, 14);
        assert_eq!(app.view, View::Day);
        let AppEffect::Http { url, .. } = &effects[0] else {
            panic!("expected a fetch");
        };
        assert!(url.starts_with("http://calendar.google.com/"), "{url}");
        // A plain launch still opens on today's month.
        let (plain, _) = Calendar::launch("http://calendar.google.com/", 1, 0);
        assert_eq!((plain.cursor_day, plain.view), (0, View::Month));
    }
    #[test]
    fn month_paging_moves_whole_months_and_today_returns() {
        let mut app = app();
        assert_eq!(app.cursor_date().month, 9);
        app.click(1, "cal:next", 0).unwrap();
        assert_eq!(app.cursor_date().month, 10);
        app.click(1, "cal:prev", 0).unwrap();
        assert_eq!(app.cursor_date().month, 9);
        app.click(1, "cal:day:40", 0).unwrap();
        assert_eq!(app.view, View::Day);
        app.click(1, "cal:today", 0).unwrap();
        assert_eq!(app.cursor_day, 0);
    }
}
