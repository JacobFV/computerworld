//! The Google Calendar layout over the one event store. `plain` never reaches this module, so
//! the original page bytes cannot move; everything here is presentation plus real routes.
use crate::{civil, days_in_month, heading, CalendarState, Event, Nav, DAY_US, HOUR_US};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_service_common as web;

/// The product surface. Nothing here reaches a record: colours and labels only.
const ACCENT: &str = "#1a73e8";
const INK: &str = "#3c4043";
const MUTED: &str = "#5f6368";
const SURFACE: &str = "#f1f3f4";
const LINE: &str = "#dadce0";
const TODAY: &str = "#e8f0fe";
/// Google's own event colours, picked by owner so one person keeps one colour everywhere.
const CHIPS: [&str; 6] = [
    "#039be5", "#0b8043", "#d50000", "#f09300", "#8e24aa", "#3f51b5",
];
/// Mini-month column heads. Sunday first, as the product prints them.
const INITIALS: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];

fn palette() -> PageTheme {
    PageTheme {
        accent: Some(ACCENT.into()),
        background: Some("#ffffff".into()),
        surface: Some(SURFACE.into()),
        ink: Some(INK.into()),
        muted: Some(MUTED.into()),
        content_width: None,
        font: None,
    }
}
/// A vertical stack; `Grid` with one column is the page model's column primitive.
fn stack(id: &str, gap: u32, children: Vec<PageElement>) -> PageElement {
    web::grid(id, 1, gap, children)
}
/// A real control: one click, one route, no fields the reader has to fill in.
fn button(id: &str, text: &str, url: &str, fields: &[(&str, &str)]) -> PageElement {
    PageElement::Button {
        id: id.into(),
        text: text.into(),
        action: PageAction {
            method: "POST".into(),
            url: url.into(),
            fields: fields
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        },
        style: None,
    }
}
fn chip_colour(owner: &str) -> &'static str {
    CHIPS[owner.bytes().map(usize::from).sum::<usize>() % CHIPS.len()]
}
/// Weeks are anchored on the epoch day, not on Sunday: simulation time starts at day 0 and
/// cannot run backwards, so a Sunday-anchored grid would need days before the world existed.
fn anchor_of(day: u64) -> u64 {
    day - day % 7
}

/// Everything one render needs. Methods rather than free functions keep argument lists short.
struct View<'a> {
    s: &'a CalendarState,
    actor: &'a str,
    nav: &'a Nav,
    /// Day index of `ctx.tick`: the column the product would circle in blue.
    today: u64,
    /// First day of the week on screen.
    anchor: u64,
    brand: String,
    accent: String,
    ink: String,
    muted: String,
    surface: String,
}
/// A GET link into this calendar; both coordinates are in the query so a page is a permalink.
fn at(day: u64, event: Option<&str>) -> PageAction {
    match event {
        Some(id) => web::visit(format!("/?day={day}&event={id}")),
        None => web::visit(format!("/?day={day}")),
    }
}
impl View<'_> {
    /// "17 – 23 Sep 2026", or both month names when the week straddles a boundary.
    fn range(&self) -> String {
        let (a, b) = (
            civil(self.anchor * DAY_US),
            civil((self.anchor + 6) * DAY_US),
        );
        if (a.year, a.month) == (b.year, b.month) {
            format!("{} – {} {} {}", a.day, b.day, a.month_name(), a.year)
        } else {
            format!(
                "{} {} – {} {} {}",
                a.day,
                a.month_name(),
                b.day,
                b.month_name(),
                b.year
            )
        }
    }
    fn header(&self) -> PageElement {
        let chip = |id: &str, text: &str, day: u64| {
            web::card_action(
                id,
                web::style().border(LINE).radius(18).padding(8).width(64),
                at(day, None),
                vec![web::styled(
                    &format!("{id}-label"),
                    text,
                    web::style().size(13).color(&self.ink).align("center"),
                )],
            )
        };
        web::styled_row(
            "bar",
            12,
            "center",
            web::style().background("#ffffff").padding(12),
            vec![
                web::styled(
                    "wordmark",
                    &self.brand,
                    web::style()
                        .size(22)
                        .medium()
                        .color(&self.accent)
                        .width(150),
                ),
                chip("today", "Today", self.today),
                chip("prev", "‹", self.anchor.saturating_sub(7)),
                chip("next", "›", self.anchor + 7),
                web::styled(
                    "range",
                    self.range(),
                    web::style().size(18).medium().color(&self.ink).flex(5),
                ),
                web::badge(
                    "view-week",
                    "Week",
                    web::style()
                        .background(TODAY)
                        .color(&self.accent)
                        .radius(12)
                        .padding(6)
                        .size(12)
                        .width(64)
                        .align("center"),
                ),
                web::badge(
                    "account",
                    self.actor,
                    web::style()
                        .background(chip_colour(self.actor))
                        .color("#ffffff")
                        .radius(16)
                        .padding(8)
                        .size(12)
                        .width(80)
                        .align("center"),
                ),
            ],
        )
    }
    /// The little month grid in the corner. Every number is a real link to that day's week.
    fn mini_month(&self) -> PageElement {
        let at_anchor = civil(self.anchor * DAY_US);
        // Day 1 of this month, as a signed epoch offset: September 2026 begins before day 0.
        let first = self.anchor as i64 - (at_anchor.day as i64 - 1);
        let lead = (at_anchor.weekday as i64 - (at_anchor.day as i64 - 1)).rem_euclid(7);
        let mut cells: Vec<PageElement> = INITIALS
            .iter()
            .enumerate()
            .map(|(i, d)| {
                web::styled(
                    &format!("mini-head-{i}"),
                    *d,
                    web::style()
                        .size(11)
                        .color(&self.muted)
                        .align("center")
                        .height(20),
                )
            })
            .collect();
        for i in 0..lead {
            cells.push(web::spacer(&format!("mini-lead-{i}"), 24));
        }
        for d in 1..=days_in_month(at_anchor.year, at_anchor.month) {
            let index = first + d as i64 - 1;
            let id = format!("mini-{d}");
            // Days before the epoch have no week to open, so they are labels, not controls.
            let Ok(index) = u64::try_from(index) else {
                cells.push(web::styled(
                    &id,
                    d.to_string(),
                    web::style().size(11).color(LINE).align("center").height(24),
                ));
                continue;
            };
            let mut style = web::style().size(11).radius(12).height(24).align("center");
            style = if index == self.today {
                style.background(&self.accent).color("#ffffff")
            } else if anchor_of(index) == self.anchor {
                style.background(TODAY).color(&self.accent)
            } else {
                style.color(&self.ink)
            };
            cells.push(web::thumbnail_action(
                &id,
                d.to_string(),
                style,
                at(index, None),
            ));
        }
        stack(
            "mini",
            4,
            vec![
                web::styled(
                    "mini-title",
                    format!("{} {}", at_anchor.month_name(), at_anchor.year),
                    web::style().size(13).medium().color(&self.ink),
                ),
                web::grid("mini-grid", 7, 2, cells),
            ],
        )
    }
    fn sidebar(&self) -> PageElement {
        let mut owners: Vec<&str> = self
            .s
            .list(self.actor, 0, u64::MAX)
            .into_iter()
            .map(|e| e.owner.as_str())
            .collect();
        owners.sort_unstable();
        owners.dedup();
        let mut calendars = vec![web::styled(
            "calendars-title",
            "My calendars",
            web::style().size(13).medium().color(&self.ink),
        )];
        for owner in owners {
            calendars.push(web::styled_row(
                &format!("calendar-{owner}"),
                8,
                "center",
                web::style(),
                vec![
                    web::thumbnail(
                        &format!("calendar-{owner}-swatch"),
                        "",
                        web::style()
                            .background(chip_colour(owner))
                            .radius(4)
                            .width(14)
                            .height(14),
                    ),
                    web::styled(
                        &format!("calendar-{owner}-name"),
                        owner,
                        web::style().size(13).color(&self.muted).flex(6),
                    ),
                ],
            ));
        }
        web::card(
            "sidebar",
            web::style().width(220).flex(0).padding(12),
            vec![stack(
                "sidebar-items",
                14,
                vec![
                    web::card_action(
                        "create",
                        web::style()
                            .background("#ffffff")
                            .border(LINE)
                            .radius(24)
                            .padding(14),
                        at(self.anchor, None),
                        vec![web::styled(
                            "create-label",
                            "+ Create",
                            web::style().size(15).medium().color(&self.ink),
                        )],
                    ),
                    self.mini_month(),
                    web::divider("sidebar-rule"),
                    stack("calendars", 8, calendars),
                ],
            )],
        )
    }
    /// One day column: its heading, then every event the actor may see, earliest first.
    fn day(&self, d: u64) -> PageElement {
        let current = d == self.today;
        let mut children = vec![
            web::styled(
                &format!("day-{d}-head"),
                heading(d),
                web::style()
                    .size(12)
                    .medium()
                    .color(if current { &self.accent } else { &self.muted })
                    .align("center"),
            ),
            web::divider(&format!("day-{d}-rule")),
        ];
        for e in self.s.on_day(self.actor, d) {
            let open = self.nav.event.as_deref() == Some(e.id.as_str());
            let fill = chip_colour(&e.owner);
            // A continuation day shows no start time, because the event did not start there.
            let clock = if e.start / DAY_US == d {
                civil(e.start).clock()
            } else {
                "all day".into()
            };
            children.push(web::card_action(
                &format!("day-{d}-{}", e.id),
                web::style()
                    .background(fill)
                    .radius(6)
                    .padding(6)
                    .border(if open { INK } else { fill }),
                at(d, Some(&e.id)),
                vec![stack(
                    &format!("day-{d}-{}-text", e.id),
                    2,
                    vec![
                        web::styled(
                            &format!("day-{d}-{}-clock", e.id),
                            clock,
                            web::style().size(10).color("#ffffff").one_line(),
                        ),
                        web::styled(
                            &format!("day-{d}-{}-title", e.id),
                            &e.title,
                            web::style().size(12).medium().color("#ffffff").one_line(),
                        ),
                    ],
                )],
            ));
        }
        web::card(
            &format!("day-{d}"),
            web::style()
                .background(if current { TODAY } else { "#ffffff" })
                .border(LINE)
                .radius(8)
                .padding(6)
                .height(320),
            vec![stack(&format!("day-{d}-stack"), 6, children)],
        )
    }
    fn week_grid(&self) -> PageElement {
        web::card(
            "week",
            web::style().flex(4).background(&self.surface).padding(8),
            vec![web::grid(
                "week-grid",
                7,
                8,
                (self.anchor..self.anchor + 7)
                    .map(|d| self.day(d))
                    .collect(),
            )],
        )
    }
    /// The detail pane: the open event, or the compose form when nothing is selected.
    fn detail(&self) -> PageElement {
        let open = self
            .nav
            .event
            .as_deref()
            .and_then(|id| self.s.visible(self.actor, id));
        let body = match open {
            Some(e) => self.event(e),
            None => self.new_event(),
        };
        web::card(
            "detail",
            web::style().flex(2).padding(16),
            vec![stack("detail-stack", 12, body)],
        )
    }
    fn new_event(&self) -> Vec<PageElement> {
        // Prefilled with a sensible slot in the week on screen, so the form is one click from done.
        let start = (self.anchor * DAY_US + HOUR_US).to_string();
        let end = (self.anchor * DAY_US + 2 * HOUR_US).to_string();
        vec![
            web::styled(
                "detail-title",
                "New event",
                web::style().size(18).medium().color(&self.ink),
            ),
            web::styled(
                "detail-hint",
                "Pick an event in the grid, or fill this in to add one.",
                web::style().size(13).color(&self.muted),
            ),
            web::form(
                "event",
                "/events",
                &[
                    ("title", "Title", ""),
                    ("start", "Start (logical µs)", start.as_str()),
                    ("end", "End (logical µs)", end.as_str()),
                    ("attendees", "Guests", ""),
                ],
            ),
        ]
    }
    fn event(&self, e: &Event) -> Vec<PageElement> {
        let route = format!("/events/{}", e.id);
        let day = e.start / DAY_US;
        let mut out = vec![
            web::styled_row(
                "detail-head",
                10,
                "center",
                web::style(),
                vec![
                    web::thumbnail(
                        "detail-swatch",
                        "",
                        web::style()
                            .background(chip_colour(&e.owner))
                            .radius(6)
                            .width(16)
                            .height(16),
                    ),
                    web::styled(
                        "detail-title",
                        &e.title,
                        web::style().size(20).medium().color(&self.ink).flex(8),
                    ),
                ],
            ),
            web::styled(
                "detail-when",
                e.when(),
                web::style().size(13).color(&self.muted),
            ),
        ];
        if !e.location.is_empty() {
            out.push(web::badge(
                "detail-location",
                &e.location,
                web::style()
                    .size(12)
                    .color(&self.ink)
                    .background(&self.surface)
                    .radius(8)
                    .padding(6),
            ));
        }
        if !e.conference_url.is_empty() {
            out.push(web::link(
                "detail-join",
                format!("Join the meeting — {}", e.conference_url),
                &e.conference_url,
            ));
        }
        if !e.description.is_empty() {
            out.push(web::styled(
                "detail-description",
                &e.description,
                web::style().size(13).color(&self.ink),
            ));
            // URLs written in the prose become real navigation; this is how sites connect.
            let links = web::links("detail", &e.description);
            if !links.is_empty() {
                out.push(web::styled_row(
                    "detail-links",
                    10,
                    "center",
                    web::style(),
                    links,
                ));
            }
        }
        let mut guests = vec![web::styled(
            "guests-title",
            format!("{} guest(s) · organised by {}", e.attendees.len(), e.owner),
            web::style().size(12).medium().color(&self.muted),
        )];
        for (who, rsvp) in &e.attendees {
            guests.push(web::styled_row(
                &format!("guest-{who}"),
                8,
                "center",
                web::style(),
                vec![
                    web::styled(
                        &format!("guest-{who}-name"),
                        who,
                        web::style().size(13).color(&self.ink).flex(5),
                    ),
                    web::badge(
                        &format!("guest-{who}-rsvp"),
                        rsvp.label(),
                        web::style()
                            .size(11)
                            .color(&self.muted)
                            .border(LINE)
                            .radius(8)
                            .padding(4)
                            .width(110)
                            .align("center"),
                    ),
                ],
            ));
        }
        out.push(stack("guests", 6, guests));
        if e.attendees.contains_key(self.actor) {
            let rsvp = format!("{route}/rsvp");
            out.push(web::styled_row(
                "rsvp",
                10,
                "center",
                web::style(),
                vec![
                    web::styled(
                        "rsvp-label",
                        "Going?",
                        web::style().size(13).color(&self.muted).width(64),
                    ),
                    button("rsvp-yes", "Yes", &rsvp, &[("response", "accepted")]),
                    button("rsvp-no", "No", &rsvp, &[("response", "declined")]),
                    button("rsvp-maybe", "Maybe", &rsvp, &[("response", "tentative")]),
                ],
            ));
        }
        if e.owner == self.actor {
            let (start, end) = (e.start.to_string(), e.end.to_string());
            out.push(web::form(
                "edit",
                &route,
                &[
                    ("title", "Title", e.title.as_str()),
                    ("start", "Start (logical µs)", start.as_str()),
                    ("end", "End (logical µs)", end.as_str()),
                ],
            ));
            out.push(button(
                "delete",
                "Delete event",
                &format!("{route}/delete"),
                &[],
            ));
        }
        out.push(web::link("detail-permalink", "Permalink", &route));
        out.push(web::link(
            "detail-back",
            "Back to the week",
            format!("/?day={day}"),
        ));
        out
    }
}
pub(crate) fn week(s: &CalendarState, actor: &str, now: u64, nav: &Nav) -> Result<HttpResponse> {
    let theme = s.theme.clone().unwrap_or_else(palette);
    // A selected event wins over the day query, so an event permalink always opens its own week.
    let day = nav
        .event
        .as_deref()
        .and_then(|id| s.visible(actor, id))
        .map(|e| e.start / DAY_US)
        .or(nav.day)
        .unwrap_or(now / DAY_US);
    let view = View {
        s,
        actor,
        nav,
        today: now / DAY_US,
        anchor: anchor_of(day),
        brand: if s.brand.is_empty() {
            "Calendar".to_owned()
        } else {
            s.brand.clone()
        },
        accent: theme.accent.clone().unwrap_or_else(|| ACCENT.into()),
        ink: theme.ink.clone().unwrap_or_else(|| INK.into()),
        muted: theme.muted.clone().unwrap_or_else(|| MUTED.into()),
        surface: theme.surface.clone().unwrap_or_else(|| SURFACE.into()),
    };
    web::themed_page(
        "Google Calendar",
        theme,
        vec![
            view.header(),
            web::styled_row(
                "panes",
                12,
                "stretch",
                web::style().padding(12),
                vec![view.sidebar(), view.week_grid(), view.detail()],
            ),
        ],
    )
}
