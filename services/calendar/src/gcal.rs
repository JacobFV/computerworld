//! The Google Calendar layout over the one event store, served as HTML. `plain` never
//! reaches this module, so the original page bytes cannot move; everything here is
//! presentation plus real routes. The stylesheet is `gcal.css` next to this file; the
//! palette a seed carries goes on `<html>` as custom properties so the sheet stays static.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `wordmark`,
//! `today`, `prev`, `next`, `range`, `view-week` (and `view-month`), `account`, `create`,
//! `mini-title`, `mini-<n>`, `calendars-title`, `calendar-<owner>`, `day-<d>`,
//! `day-<d>-head`, `day-<d>-<event>` (one link, with `-clock` and `-title` inside),
//! `detail`, `detail-title`, `detail-hint`, the `event` form (`event-title`, `event-start`,
//! `event-end`, `event-attendees`, `event-submit`), `detail-when`, `detail-location`,
//! `detail-join`, `detail-description`, `detail-link-<i>`, `guests-title`, `guest-<who>`,
//! the `rsvp` form (`rsvp-yes`, `rsvp-no`, `rsvp-maybe`), the `edit` form, `delete` inside
//! `delete-form`, `detail-permalink` and `detail-back`.
use crate::{civil, days_in_month, heading, CalendarState, Event, Nav, DAY_US, HOUR_US};
use cw_protocol::{HttpResponse, Result};
use cw_service_common::html::{
    button, div, el, form, href, label, link, page, span, text_input, Document, Html,
};

const CSS: &str = include_str!("gcal.css");
const ACCENT: &str = "#1a73e8";
const INK: &str = "#3c4043";
const MUTED: &str = "#70757a";
const SURFACE: &str = "#f1f3f4";
/// Google's own event colours, picked by owner so one person keeps one colour everywhere.
const CHIPS: usize = 6;
/// Mini-month column heads. Sunday first, as the product prints them.
const INITIALS: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];
const WEEKDAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
    "November", "December",
];
/// The epoch is 09:00 on civil day 0, so a civil day begins nine hours before `d * DAY_US`.
const EPOCH_OFFSET: u64 = 9 * HOUR_US;
const MINUTE_US: u64 = 60_000_000;
/// Pixels per hour of the time grid.
const HOUR_PX: u64 = 48;
/// Events at least this long sit in the all-day row, as the product files them.
const ALL_DAY_US: u64 = DAY_US;
/// Chips a month cell shows before it says "n more".
const MONTH_CHIPS: usize = 3;

fn chip_class(owner: &str) -> String {
    // A rolling hash rather than a byte sum, so anagram-close names (bob, carol) still differ.
    let hash = owner.bytes().fold(0u32, |h, b| h.wrapping_mul(17).wrapping_add(u32::from(b)));
    format!("c{}", hash as usize % CHIPS)
}
/// First microsecond of civil day `d`, clamped at the epoch.
fn day_start(d: u64) -> u64 {
    (d * DAY_US).saturating_sub(EPOCH_OFFSET)
}
fn day_end(d: u64) -> u64 {
    (d + 1) * DAY_US - EPOCH_OFFSET
}
/// Minute of civil day `d` at which `us` falls, clamped into the day.
fn minute_of(d: u64, us: u64) -> u64 {
    ((us + EPOCH_OFFSET).saturating_sub(d * DAY_US) / MINUTE_US).min(1440)
}
/// Day of month, month (1-12), year and weekday of a signed civil day index. Days before
/// the epoch only ever appear as the greyed lead of the first week or month on screen.
fn date_of(index: i64) -> (u64, u64, u64, u64) {
    match u64::try_from(index) {
        Ok(d) => {
            let at = civil(d * DAY_US);
            (at.day, at.month, at.year, at.weekday)
        }
        Err(_) => {
            let weekday = (4 + index).rem_euclid(7) as u64;
            let day = 17 + index;
            if day >= 1 {
                (day as u64, 9, 2026, weekday)
            } else {
                ((31 + day).max(1) as u64, 8, 2026, weekday)
            }
        }
    }
}
fn month_name(month: u64) -> &'static str {
    MONTHS[(month as usize + 11) % 12]
}

/// Everything one render needs. Methods rather than free functions keep argument lists short.
struct View<'a> {
    s: &'a CalendarState,
    actor: &'a str,
    nav: &'a Nav,
    now: u64,
    /// Civil day index of `ctx.tick`: the column the product circles in blue.
    today: u64,
    /// The day the reader asked for; the week or month on screen is the one around it.
    focus: u64,
    month: bool,
    brand: String,
}
impl View<'_> {
    /// A GET link into this calendar; every coordinate is in the query so a page is a permalink.
    fn at(&self, day: u64, event: Option<&str>) -> String {
        let day = day.to_string();
        let mut params = Vec::new();
        if self.month {
            params.push(("view", "month"));
        }
        params.push(("day", day.as_str()));
        if let Some(id) = event {
            params.push(("event", id));
        }
        href("/", &params)
    }
    /// Sunday of the week on screen, as a signed index: the first week begins before day 0.
    fn week_start(&self) -> i64 {
        let weekday = civil(self.focus * DAY_US).weekday as i64;
        self.focus as i64 - weekday
    }
    /// Civil day index of the first of the focus month, signed for September 2026.
    fn month_start(&self) -> i64 {
        self.focus as i64 - (civil(self.focus * DAY_US).day as i64 - 1)
    }
    fn visible(&self, d: u64) -> Vec<&Event> {
        let mut found = self.s.list(self.actor, day_start(d), day_end(d));
        found.sort_by(|a, b| (a.start, &a.id).cmp(&(b.start, &b.id)));
        found
    }
    /// "September 2026", or "Sep – Oct 2026" when the week straddles a boundary.
    fn range(&self) -> String {
        if self.month {
            let at = civil(self.focus * DAY_US);
            return format!("{} {}", month_name(at.month), at.year);
        }
        let (_, m0, y0, _) = date_of(self.week_start());
        let (_, m1, y1, _) = date_of(self.week_start() + 6);
        if (m0, y0) == (m1, y1) {
            format!("{} {}", month_name(m0), y0)
        } else if y0 == y1 {
            format!("{} – {} {}", &month_name(m0)[..3], &month_name(m1)[..3], y1)
        } else {
            format!("{} {} – {} {}", &month_name(m0)[..3], y0, &month_name(m1)[..3], y1)
        }
    }
    fn header(&self) -> Html {
        let (prev, next) = if self.month {
            let at = civil(self.focus * DAY_US);
            let start = self.month_start();
            let before = if at.month == 1 { days_in_month(at.year - 1, 12) } else { days_in_month(at.year, at.month - 1) };
            (
                u64::try_from(start - before as i64).unwrap_or(0),
                (start + days_in_month(at.year, at.month) as i64).max(0) as u64,
            )
        } else {
            (self.focus.saturating_sub(7), self.focus + 7)
        };
        let today = civil(self.now);
        el("header")
            .id("bar")
            .class("bar")
            .child(span("burger").attr("aria-hidden", "true").each(0..3, |_| el("i")))
            .child(div("logo").attr("aria-hidden", "true").child(span("logo-day").text(today.day.to_string())))
            .child(span("wordmark").id("wordmark").text(self.brand.as_str()))
            .child(link("today", self.at(self.today, None), "Today").class("pill"))
            .child(link("prev", self.at(prev, None), "‹").class("arrow").attr("aria-label", if self.month { "Previous month" } else { "Previous week" }))
            .child(link("next", self.at(next, None), "›").class("arrow").attr("aria-label", if self.month { "Next month" } else { "Next week" }))
            .child(el("h1").id("range").class("range").text(self.range()))
            .child(
                div("views")
                    .child(
                        link("view-week", href("/", &[("day", &self.focus.to_string())]), "Week")
                            .class(if self.month { "view" } else { "view on" }),
                    )
                    .child(
                        link("view-month", href("/", &[("view", "month"), ("day", &self.focus.to_string())]), "Month")
                            .class(if self.month { "view on" } else { "view" }),
                    ),
            )
            .child(
                div("avatar")
                    .id("account")
                    .class(&chip_class(self.actor))
                    .attr("title", self.actor)
                    .attr("aria-label", format!("Account: {}", self.actor))
                    .text(self.actor.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()),
            )
    }
    /// The little month grid in the corner. Every number is a real link to that day's week.
    fn mini_month(&self) -> Html {
        let at = civil(self.focus * DAY_US);
        let first = self.month_start();
        let lead = date_of(first).3;
        let (week_from, week_to) = (self.week_start(), self.week_start() + 6);
        let mut grid = div("mini-grid").id("mini-grid").each(INITIALS, |d| span("mini-head").text(d));
        for _ in 0..lead {
            grid = grid.child(span("mini-blank"));
        }
        for n in 1..=days_in_month(at.year, at.month) {
            let index = first + n as i64 - 1;
            let id = format!("mini-{n}");
            // Days before the epoch have no week to open, so they are labels, not controls.
            grid = grid.child(match u64::try_from(index) {
                Err(_) => span("mini-day off").id(id).text(n.to_string()),
                Ok(day) => {
                    let class = if day == self.today {
                        "mini-day today"
                    } else if !self.month && (week_from..=week_to).contains(&index) {
                        "mini-day week"
                    } else {
                        "mini-day"
                    };
                    link(&id, self.at(day, None), n.to_string()).class(class)
                }
            });
        }
        div("mini")
            .id("mini")
            .child(div("mini-title").id("mini-title").text(format!("{} {}", month_name(at.month), at.year)))
            .child(grid)
    }
    fn sidebar(&self) -> Html {
        let mut owners: Vec<&str> = self.s.list(self.actor, 0, u64::MAX).into_iter().map(|e| e.owner.as_str()).collect();
        owners.sort_unstable();
        owners.dedup();
        el("aside")
            .id("sidebar")
            .class("sidebar")
            .child(
                el("a")
                    .id("create")
                    .class("create")
                    .attr("href", self.at(self.focus, None))
                    .child(span("plus").attr("aria-hidden", "true"))
                    .child(span("create-label").id("create-label").text("Create")),
            )
            .child(self.mini_month())
            .child(
                div("calendars")
                    .id("calendars")
                    .child(div("calendars-title").id("calendars-title").text("My calendars"))
                    .each(owners, |owner| {
                        div("calendar")
                            .id(format!("calendar-{owner}"))
                            .child(span("swatch").class(&chip_class(owner)).attr("aria-hidden", "true"))
                            .child(span("calendar-name").id(format!("calendar-{owner}-name")).text(owner))
                    }),
            )
    }
    /// One chip: the whole thing is the link that opens the event in the side panel.
    fn chip(&self, d: u64, e: &Event, clock: String, class: &str) -> Html {
        let open = self.nav.event.as_deref() == Some(e.id.as_str());
        let id = format!("day-{d}-{}", e.id);
        el("a")
            .id(id.as_str())
            .class("ev")
            .class(class)
            .class(&chip_class(&e.owner))
            .class(if open { "open" } else { "" })
            .attr("href", self.at(d, Some(&e.id)))
            .child(span("ev-title").id(format!("{id}-title")).text(e.title.as_str()))
            .child(span("ev-clock").id(format!("{id}-clock")).text(clock))
    }
    /// A day-long (or longer) event on day `d`: a bar whose ends square off where it continues
    /// into the neighbouring day, so the per-day links read as one strip.
    fn bar(&self, d: u64, e: &Event) -> Html {
        let class = match (e.start < day_start(d), e.end > day_end(d)) {
            (true, true) => "bar mid",
            (true, false) => "bar tail",
            (false, true) => "bar lead",
            (false, false) => "bar",
        };
        let clock = if e.start >= day_start(d) { civil(e.start).clock() } else { "all day".into() };
        self.chip(d, e, clock, class)
    }
    /// Timed events of one day with their lanes: (event, first minute, last minute, lane, lanes).
    fn lanes<'e>(&self, d: u64, events: &[&'e Event]) -> Vec<(&'e Event, u64, u64, usize, usize)> {
        let mut placed: Vec<(&Event, u64, u64, usize, usize)> = Vec::new();
        let mut cluster_from = 0;
        let mut cluster_end = 0;
        let mut ends: Vec<u64> = Vec::new();
        for e in events {
            let (from, to) = (minute_of(d, e.start.max(day_start(d))), minute_of(d, e.end.min(day_end(d))));
            let to = to.max(from + 1);
            if from >= cluster_end {
                let lanes = ends.len().max(1);
                for p in &mut placed[cluster_from..] {
                    p.4 = lanes;
                }
                cluster_from = placed.len();
                ends.clear();
            }
            let lane = match ends.iter().position(|end| *end <= from) {
                Some(i) => {
                    ends[i] = to;
                    i
                }
                None => {
                    ends.push(to);
                    ends.len() - 1
                }
            };
            cluster_end = cluster_end.max(to);
            placed.push((e, from, to, lane, 1));
        }
        let lanes = ends.len().max(1);
        for p in &mut placed[cluster_from..] {
            p.4 = lanes;
        }
        placed
    }
    fn week(&self) -> Html {
        let start = self.week_start();
        let days: Vec<i64> = (start..start + 7).collect();
        let per_day: Vec<(Vec<&Event>, Vec<&Event>)> = days
            .iter()
            .map(|i| match u64::try_from(*i) {
                Ok(d) => self.visible(d).into_iter().partition(|e| e.end - e.start >= ALL_DAY_US),
                Err(_) => (Vec::new(), Vec::new()),
            })
            .collect();
        // The hours on screen: the working day, widened to hold whatever the week contains.
        let (mut from, mut to) = (7 * 60, 20 * 60);
        for (i, (_, timed)) in days.iter().zip(&per_day) {
            let Ok(d) = u64::try_from(*i) else { continue };
            for e in timed {
                from = from.min(minute_of(d, e.start.max(day_start(d))) / 60 * 60);
                to = to.max(minute_of(d, e.end.min(day_end(d))).div_ceil(60) * 60);
            }
        }
        let hours = (to - from) / 60;
        let px = |minutes: u64| minutes * HOUR_PX / 60;
        let mut heads = div("heads").child(div("zone").text("GMT"));
        let mut allday = div("allday").child(div("zone"));
        let mut times = div("times").child(
            div("gutter").each(0..hours, |h| div("hour-label").child(span("").text(format!("{:02}:00", from / 60 + h)))),
        );
        for (i, (long, timed)) in days.iter().zip(&per_day) {
            let (number, _, _, weekday) = date_of(*i);
            let Ok(d) = u64::try_from(*i) else {
                heads = heads.child(
                    div("head off")
                        .child(span("dow").text(WEEKDAYS[weekday as usize]))
                        .child(span("num").text(number.to_string())),
                );
                allday = allday.child(div("allday-cell off"));
                times = times.child(div("col off").each(0..hours, |_| div("hour")));
                continue;
            };
            let current = d == self.today;
            heads = heads.child(
                div(if current { "head today" } else { "head" })
                    .id(format!("day-{d}-head"))
                    .attr("aria-label", heading(d))
                    .child(span("dow").text(WEEKDAYS[weekday as usize]))
                    .child(span("num").text(number.to_string())),
            );
            allday = allday.child(div("allday-cell").each(long, |e| self.bar(d, e)));
            let mut col = div(if current { "col today" } else { "col" }).id(format!("day-{d}")).each(0..hours, |_| div("hour"));
            for (e, first, last, lane, lanes) in self.lanes(d, timed) {
                let (top, bottom) = (first.max(from), last.min(to));
                let height = px(bottom - top).saturating_sub(2).max(20);
                let width = 100.0 / lanes as f64;
                let clock = if e.start >= day_start(d) {
                    format!("{} – {}", civil(e.start).clock(), civil(e.end).clock())
                } else {
                    format!("until {}", civil(e.end).clock())
                };
                col = col.child(
                    self.chip(d, e, clock, if bottom - top < 50 { "timed short" } else { "timed" }).style(&format!(
                        "top: {}px; height: {}px; left: {:.2}%; width: {:.2}%",
                        px(top - from),
                        height,
                        width * lane as f64,
                        width
                    )),
                );
            }
            if current {
                let minute = minute_of(d, self.now);
                if (from..to).contains(&minute) {
                    col = col.child(div("now").attr("aria-hidden", "true").style(&format!("top: {}px", px(minute - from))).child(el("i")));
                }
            }
            times = times.child(col);
        }
        el("main").id("week").class("main").child(heads).child(allday).child(times)
    }
    fn month_grid(&self) -> Html {
        let at = civil(self.focus * DAY_US);
        let first = self.month_start();
        let start = first - date_of(first).3 as i64;
        let last = first + days_in_month(at.year, at.month) as i64 - 1;
        let weeks = (last - start) / 7 + 1;
        let mut grid = div("month-grid");
        for i in start..start + weeks * 7 {
            let (number, month, _, weekday) = date_of(i);
            let label = if number == 1 { format!("{} 1", &month_name(month)[..3]) } else { number.to_string() };
            let dow = (i - start < 7).then(|| span("dow").text(WEEKDAYS[weekday as usize]));
            let Ok(d) = u64::try_from(i) else {
                grid = grid.child(div("cell off").maybe(dow).child(span("num").text(label)));
                continue;
            };
            let events = self.visible(d);
            let class = match (d == self.today, month == at.month) {
                (true, _) => "cell today",
                (false, true) => "cell",
                (false, false) => "cell other",
            };
            let more = events.len().saturating_sub(MONTH_CHIPS);
            grid = grid.child(
                div(class)
                    .id(format!("day-{d}"))
                    .maybe(dow)
                    .child(
                        link(&format!("day-{d}-head"), href("/", &[("day", &d.to_string())]), label)
                            .class("num")
                            .attr("aria-label", heading(d)),
                    )
                    .each(events.iter().take(MONTH_CHIPS), |e| {
                        if e.end - e.start >= ALL_DAY_US {
                            self.bar(d, e)
                        } else {
                            let clock = if e.start >= day_start(d) { civil(e.start).clock() } else { "all day".into() };
                            self.chip(d, e, clock, "dot")
                        }
                    })
                    .when(more > 0, |cell| {
                        cell.child(link(&format!("day-{d}-more"), href("/", &[("day", &d.to_string())]), format!("{more} more")).class("more"))
                    }),
            );
        }
        el("main").id("month").class("main").child(grid)
    }
    /// The side panel: the open event, or the new-event form when nothing is selected.
    fn detail(&self) -> Html {
        let open = self.nav.event.as_deref().and_then(|id| self.s.visible(self.actor, id));
        let panel = el("aside").id("detail").class("detail");
        match open {
            Some(e) => self.event(panel, e),
            None => self.new_event(panel),
        }
    }
    fn field(id: &str, name: &str, text: &str, value: &str) -> Html {
        div("field").child(label(id, text)).child(text_input(id, name, value).attr("autocomplete", "off"))
    }
    fn new_event(&self, panel: Html) -> Html {
        // Prefilled with a sensible slot on the day on screen, so the form is one click from done.
        let start = (self.focus * DAY_US + HOUR_US).to_string();
        let end = (self.focus * DAY_US + 2 * HOUR_US).to_string();
        panel
            .child(el("h2").id("detail-title").class("detail-title").text("New event"))
            .child(el("p").id("detail-hint").class("hint").text("Pick an event in the grid, or fill this in to add one."))
            .child(
                form("event", "/events", "post")
                    .class("fields")
                    .child(Self::field("event-title", "title", "Title", "").class("wide"))
                    .child(Self::field("event-start", "start", "Start (logical µs)", &start))
                    .child(Self::field("event-end", "end", "End (logical µs)", &end))
                    .child(Self::field("event-attendees", "attendees", "Guests", ""))
                    .child(button("event-submit", "Save").class("primary")),
            )
    }
    /// The description with every written address made a real link, numbered by word as
    /// `detail-link-<i>`; this is how sites connect.
    fn description(text: &str) -> Html {
        let mut out = el("p").id("detail-description").class("description");
        let (mut word, mut rest) = (0usize, text);
        while !rest.is_empty() {
            let space = rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
            if space > 0 {
                out = out.text(&rest[..space]);
                rest = &rest[space..];
                continue;
            }
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let token = &rest[..end];
            let url = token.trim_end_matches(['.', ',', ';', ')', ']']);
            let is_url = ["http://", "https://"].iter().any(|scheme| url.len() > scheme.len() && url.starts_with(scheme));
            out = if is_url {
                out.child(link(&format!("detail-link-{word}"), url, url)).text(&token[url.len()..])
            } else {
                out.text(token)
            };
            word += 1;
            rest = &rest[end..];
        }
        out
    }
    fn event(&self, panel: Html, e: &Event) -> Html {
        let route = format!("/events/{}", e.id);
        let day = civil(e.start).index;
        let mut panel = panel
            .child(
                div("detail-head")
                    .id("detail-head")
                    .child(span("swatch").id("detail-swatch").class(&chip_class(&e.owner)).attr("aria-hidden", "true"))
                    .child(el("h2").id("detail-title").class("detail-title").text(e.title.as_str())),
            )
            .child(el("p").id("detail-when").class("when").text(e.when()));
        if !e.location.is_empty() {
            panel = panel.child(el("p").id("detail-location").class("location").text(e.location.as_str()));
        }
        if !e.conference_url.is_empty() {
            panel = panel
                .child(link("detail-join", e.conference_url.as_str(), "Join the meeting").class("join"))
                .child(el("p").id("detail-join-url").class("join-url").text(e.conference_url.as_str()));
        }
        if !e.description.is_empty() {
            panel = panel.child(Self::description(&e.description));
        }
        panel = panel.child(
            div("guests")
                .id("guests")
                .child(
                    div("guests-title")
                        .id("guests-title")
                        .text(format!("{} guest(s) · organised by {}", e.attendees.len(), e.owner)),
                )
                .each(&e.attendees, |(who, rsvp)| {
                    div("guest")
                        .id(format!("guest-{who}"))
                        .child(span("guest-avatar").class(&chip_class(who)).attr("aria-hidden", "true").text(
                            who.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
                        ))
                        .child(span("guest-name").id(format!("guest-{who}-name")).text(who.as_str()))
                        .child(span("guest-rsvp").id(format!("guest-{who}-rsvp")).text(rsvp.label()))
                }),
        );
        if e.attendees.contains_key(self.actor) {
            let answer = |id: &str, text: &str, value: &str| button(id, text).attr("name", "response").attr("value", value);
            panel = panel.child(
                form("rsvp", format!("{route}/rsvp"), "post")
                    .class("rsvp")
                    .child(span("rsvp-label").id("rsvp-label").text("Going?"))
                    .child(answer("rsvp-yes", "Yes", "accepted"))
                    .child(answer("rsvp-no", "No", "declined"))
                    .child(answer("rsvp-maybe", "Maybe", "tentative")),
            );
        }
        if e.owner == self.actor {
            panel = panel
                .child(
                    form("edit", route.as_str(), "post")
                        .class("fields")
                        .child(Self::field("edit-title", "title", "Title", &e.title).class("wide"))
                        .child(Self::field("edit-start", "start", "Start (logical µs)", &e.start.to_string()))
                        .child(Self::field("edit-end", "end", "End (logical µs)", &e.end.to_string()))
                        .child(button("edit-submit", "Save").class("primary")),
                )
                .child(form("delete-form", format!("{route}/delete"), "post").child(button("delete", "Delete event").class("danger")));
        }
        panel.child(
            div("detail-links")
                .child(link("detail-permalink", route.as_str(), "Permalink"))
                .child(link("detail-back", self.at(day, None), if self.month { "Back to the month" } else { "Back to the week" })),
        )
    }
}
pub(crate) fn week(s: &CalendarState, actor: &str, now: u64, nav: &Nav) -> Result<HttpResponse> {
    let theme = s.theme.clone().unwrap_or_default();
    let today = civil(now).index;
    // A selected event wins over the day query, so an event permalink always opens its own week.
    let focus = nav
        .event
        .as_deref()
        .and_then(|id| s.visible(actor, id))
        .map(|e| civil(e.start).index)
        .or(nav.day)
        .unwrap_or(today);
    let view = View {
        s,
        actor,
        nav,
        now,
        today,
        focus,
        month: nav.month,
        brand: if s.brand.is_empty() { "Calendar".to_owned() } else { s.brand.clone() },
    };
    let or = |value: &Option<String>, fallback: &str| value.clone().unwrap_or_else(|| fallback.to_owned());
    let document = Document::new("Google Calendar")
        .lang("en")
        .stylesheet(CSS)
        .root_style(&format!(
            "--accent: {}; --ink: {}; --muted: {}; --surface: {}; --paper: {}",
            or(&theme.accent, ACCENT),
            or(&theme.ink, INK),
            or(&theme.muted, MUTED),
            or(&theme.surface, SURFACE),
            or(&theme.background, "#ffffff"),
        ))
        .body_class(if nav.month { "skin-gcal view-month" } else { "skin-gcal view-week" })
        .body([
            view.header(),
            div("shell")
                .id("panes")
                .child(view.sidebar())
                .child(if nav.month { view.month_grid() } else { view.week() })
                .child(view.detail()),
        ]);
    page(&document)
}
