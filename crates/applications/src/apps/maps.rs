//! Maps over the `geo` service in maps mode — the dataset maps.google.com is built on.
//!
//! Places arrive as integer micro-degrees and routes as integer metres and minutes, so the
//! canvas plots real coordinates and every label is arithmetic on what the service sent.
//! Nothing here rounds through a float, and no pin exists that the service did not name.
use super::look::{action, chip, header, inert, look, notice, FAINT, INK, LINE, MUTED};
use super::{push_bounded, Status};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Places retained from one reply, and steps retained from one route.
pub const PLACE_LIMIT: usize = 200;
pub const STEP_LIMIT: usize = 32;
const QUERY_LIMIT: usize = 120;
/// Travel modes the service prices, with the label each one wears.
const TRAVEL: &[(&str, &str)] = &[
    ("driving", "Drive"),
    ("transit", "Transit"),
    ("cycling", "Bike"),
    ("walking", "Walk"),
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub address: String,
    /// Micro-degrees, exactly as the service keeps them.
    pub lat: i64,
    pub lon: i64,
    /// Tenths of a star: 46 renders as 4.6.
    pub rating: i64,
    pub reviews: i64,
    pub hours: String,
    pub phone: String,
    pub summary: String,
    pub tags: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    pub text: String,
    pub metres: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Route {
    pub id: String,
    pub from: String,
    pub to: String,
    pub mode: String,
    pub metres: i64,
    pub minutes: i64,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Maps {
    pub base: String,
    pub query: String,
    /// True while the search field has the keyboard.
    pub typing: bool,
    pub places: Vec<Place>,
    /// Place ids this person has starred, as the service reports them.
    pub saved: Vec<String>,
    pub selected: Option<String>,
    pub origin: Option<String>,
    pub destination: Option<String>,
    pub travel: String,
    /// Boxed: a route carries its own steps, and `AppState` keeps every application
    /// side by side, so the largest one sets the size of them all.
    pub route: Option<Box<Route>>,
    /// `imperial` or `metric`, read from the service rather than assumed.
    pub units: String,
    pub status: Status,
}
impl Maps {
    pub const KIND: &'static str = "maps";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let app = Self {
            base: if argument.is_empty() {
                "http://maps.internal/".into()
            } else {
                argument.to_owned()
            },
            query: String::new(),
            typing: false,
            places: vec![],
            saved: vec![],
            selected: None,
            origin: None,
            destination: None,
            travel: "driving".into(),
            route: None,
            units: "imperial".into(),
            status: Status::Loading,
        };
        let effects = app.fetch(window);
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, _theme: DesktopTheme) -> String {
        "Maps".into()
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        match (&self.route, &self.selected) {
            (Some(route), _) => format!(
                "{} · {}",
                distance_label(route.metres, self.imperial()),
                duration_label(route.minutes)
            ),
            (None, Some(id)) => self
                .place(id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| id.clone()),
            (None, None) => format!("{} places", self.results().len()),
        }
    }
    pub fn modified(&self) -> bool {
        false
    }
    fn imperial(&self) -> bool {
        self.units != "metric"
    }
    fn url(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.base.trim_end_matches('/'))
    }
    fn get(&self, window: u64, tag: &str, suffix: &str) -> AppEffect {
        AppEffect::Http {
            window,
            tag: tag.into(),
            method: "GET".into(),
            url: self.url(suffix),
            body: String::new(),
        }
    }
    fn fetch(&self, window: u64) -> Vec<AppEffect> {
        vec![
            self.get(window, "places", "/api/places"),
            self.get(window, "saved", "/api/saved"),
            self.get(window, "units", "/api/units"),
        ]
    }
    pub fn place(&self, id: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.id == id)
    }
    /// The rows on screen: the service's own dataset, narrowed by what was typed. The
    /// filter only hides places the service sent; it never adds one.
    pub fn results(&self) -> Vec<&Place> {
        let needle = self.query.trim().to_ascii_lowercase();
        self.places
            .iter()
            .filter(|p| {
                needle.is_empty()
                    || [&p.name, &p.kind, &p.address, &p.summary]
                        .iter()
                        .any(|field| field.to_ascii_lowercase().contains(&needle))
                    || p.tags
                        .iter()
                        .any(|t| t.to_ascii_lowercase().contains(&needle))
            })
            .collect()
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
        let outcome = Status::from_status(status, body);
        if outcome != Status::Idle {
            self.status = outcome;
            return Ok(vec![]);
        }
        self.status = Status::Idle;
        match tag {
            "places" => {
                let places: BTreeMap<String, Place> =
                    serde_json::from_str(body).unwrap_or_default();
                self.places = places.into_values().take(PLACE_LIMIT).collect();
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|id| self.place(id).is_none())
                {
                    self.selected = None;
                }
                Ok(vec![])
            }
            "saved" => {
                self.saved = serde_json::from_str::<Vec<String>>(body)
                    .unwrap_or_default()
                    .into_iter()
                    .take(PLACE_LIMIT)
                    .collect();
                Ok(vec![])
            }
            "units" => {
                self.units = serde_json::from_str::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("units")?.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "imperial".into());
                Ok(vec![])
            }
            // Starring is a service toggle, so the starred list is re-read rather than
            // flipped locally.
            "save" => {
                self.status = Status::Loading;
                Ok(vec![self.get(window, "saved", "/api/saved")])
            }
            // Asking for directions is the one read that writes: the service computes the
            // route and caches it, and the cache is then read back for the record itself.
            "dir" => {
                self.status = Status::Loading;
                Ok(vec![self.get(window, "routes", "/api/routes")])
            }
            "routes" => {
                let routes: BTreeMap<String, Route> =
                    serde_json::from_str(body).unwrap_or_default();
                let key = match (&self.origin, &self.destination) {
                    (Some(from), Some(to)) => format!("{from}|{to}|{}", self.travel),
                    _ => String::new(),
                };
                self.route = routes.get(&key).cloned().map(|mut route| {
                    route.steps.truncate(STEP_LIMIT);
                    Box::new(route)
                });
                Ok(vec![])
            }
            other => Err(format!("unexpected maps reply {other}")),
        }
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        if !self.typing {
            return Err("no maps field is focused".into());
        }
        push_bounded(&mut self.query, text, QUERY_LIMIT);
        Ok(())
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        match key {
            "Backspace" if self.typing => {
                self.query.pop();
                Ok(vec![])
            }
            "Enter" if self.typing => {
                self.typing = false;
                Ok(vec![])
            }
            "Escape" => {
                self.typing = false;
                self.query.clear();
                Ok(vec![])
            }
            "Ctrl+r" | "Meta+r" => self.click(window, "maps:reload", clock_us),
            other => Err(format!("unsupported maps key {other}")),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("maps:")
            .ok_or("interaction does not belong to maps")?;
        match command {
            "reload" => {
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            "search-field" => {
                self.typing = true;
                Ok(vec![])
            }
            "clear" => {
                self.query.clear();
                Ok(vec![])
            }
            "clear-route" => {
                self.route = None;
                self.origin = None;
                self.destination = None;
                Ok(vec![])
            }
            "route" => {
                let from = self.origin.clone().ok_or("no starting place is chosen")?;
                let to = self.destination.clone().ok_or("no destination is chosen")?;
                if from == to {
                    return Err("a route needs two different places".into());
                }
                self.status = Status::Loading;
                Ok(vec![self.get(
                    window,
                    "dir",
                    &format!("/maps/dir?from={from}&to={to}&mode={}", self.travel),
                )])
            }
            rest => {
                if let Some(id) = rest.strip_prefix("place:") {
                    self.place(id).ok_or("place not found")?;
                    self.selected = Some(id.to_owned());
                    return Ok(vec![]);
                }
                if let Some(id) = rest.strip_prefix("save:") {
                    self.place(id).ok_or("place not found")?;
                    self.status = Status::Loading;
                    return Ok(vec![AppEffect::Http {
                        window,
                        tag: "save".into(),
                        method: "POST".into(),
                        url: self.url(&format!("/api/places/{id}/save")),
                        body: "{}".into(),
                    }]);
                }
                if let Some(id) = rest.strip_prefix("from:") {
                    self.place(id).ok_or("place not found")?;
                    self.origin = Some(id.to_owned());
                    self.route = None;
                    return Ok(vec![]);
                }
                if let Some(id) = rest.strip_prefix("to:") {
                    self.place(id).ok_or("place not found")?;
                    self.destination = Some(id.to_owned());
                    self.route = None;
                    return Ok(vec![]);
                }
                if let Some(mode) = rest.strip_prefix("mode:") {
                    if !TRAVEL.iter().any(|(m, _)| *m == mode) {
                        return Err(format!("unknown travel mode {mode}"));
                    }
                    self.travel = mode.to_owned();
                    self.route = None;
                    return Ok(vec![]);
                }
                Err(format!("unknown maps command {command}"))
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
        page.elements.push(E::Heading {
            id: "maps-title".into(),
            text: self.caption(),
            level: 2,
        });
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "maps-status".into(),
                text: text.into(),
            });
        }
        page.elements.push(E::Input {
            id: "maps-query".into(),
            label: "Search".into(),
            value: self.query.clone(),
            placeholder: "Search places".into(),
        });
        page.elements.push(E::Button {
            id: "maps:reload".into(),
            text: "Reload".into(),
            action: act("maps:reload"),
        });
        for (mode, label) in TRAVEL {
            page.elements.push(E::Button {
                id: format!("maps:mode:{mode}"),
                text: (*label).into(),
                action: act(&format!("maps:mode:{mode}")),
            });
        }
        for place in self.results() {
            page.elements.push(E::Button {
                id: format!("maps:place:{}", place.id),
                text: format!("{} — {}", place.name, place.address),
                action: act(&format!("maps:place:{}", place.id)),
            });
            page.elements.push(E::Button {
                id: format!("maps:save:{}", place.id),
                text: format!(
                    "{} {}",
                    if self.saved.contains(&place.id) {
                        "Unsave"
                    } else {
                        "Save"
                    },
                    place.name
                ),
                action: act(&format!("maps:save:{}", place.id)),
            });
        }
        if let Some(id) = &self.selected {
            for (prefix, label) in [("from", "Start here"), ("to", "End here")] {
                page.elements.push(E::Button {
                    id: format!("maps:{prefix}:{id}"),
                    text: label.into(),
                    action: act(&format!("maps:{prefix}:{id}")),
                });
            }
        }
        if self.origin.is_some() && self.destination.is_some() {
            page.elements.push(E::Button {
                id: "maps:route".into(),
                text: "Directions".into(),
                action: act("maps:route"),
            });
        }
        if let Some(route) = &self.route {
            page.elements.push(E::Text {
                id: "maps-route".into(),
                text: format!(
                    "{} · {}",
                    distance_label(route.metres, self.imperial()),
                    duration_label(route.minutes)
                ),
            });
            for (index, step) in route.steps.iter().enumerate() {
                page.elements.push(E::Text {
                    id: format!("maps-step-{index}"),
                    text: step.text.clone(),
                });
            }
            page.elements.push(E::Button {
                id: "maps:clear-route".into(),
                text: "Clear route".into(),
                action: act("maps:clear-route"),
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let mut top = header(p, theme, &l, width, &self.title(theme));
        // Search sits over the canvas on every platform, because a map is the content.
        let bar = if theme.mobile() { 48 } else { 38 };
        p.box_(Rect::new(0, top, width, bar), l.chrome, 0);
        p.hline(0, top + bar as i32, width, LINE);
        let field = Rect::new(10, top + 6, width.saturating_sub(96), bar - 12);
        p.border(field, l.surface, l.radius, LINE);
        p.region(field, "maps:search-field", "Search places");
        p.symbol(
            "search",
            field.x + 8,
            field.y + (field.height as i32 - 14) / 2,
            14,
            FAINT,
        );
        p.left(
            field.x + 28,
            field.y + (field.height as i32 - 16) / 2,
            field.width.saturating_sub(40),
            if self.query.is_empty() {
                "Search places"
            } else {
                &self.query
            },
            13,
            if self.query.is_empty() { FAINT } else { INK },
        );
        action(
            p,
            &l,
            Rect::new(width as i32 - 80, top + 6, 70, bar - 12),
            "Reload",
            "maps:reload",
            false,
        );
        top += bar as i32 + 1;
        if let Some(text) = self.status.notice() {
            notice(p, width, top + 8, text);
            top += 26;
        }
        // A phone stacks the canvas over the list; a desktop puts the list beside it.
        let list_w = if theme.mobile() || width < 560 {
            width
        } else {
            (width / 3).clamp(220, 320)
        };
        let stacked = list_w == width;
        let canvas = if stacked {
            Rect::new(
                0,
                top,
                width,
                height
                    .saturating_sub(top as u32)
                    .saturating_sub(220)
                    .max(80),
            )
        } else {
            Rect::new(
                list_w as i32,
                top,
                width - list_w,
                height.saturating_sub(top as u32),
            )
        };
        self.canvas(p, theme, &l, canvas);
        let list = if stacked {
            Rect::new(0, canvas.y + canvas.height as i32, width, height)
        } else {
            Rect::new(0, top, list_w, height)
        };
        self.list(p, theme, &l, list);
    }
    /// The map itself: every place the service sent, plotted from its own micro-degrees.
    fn canvas(&self, p: &mut Painter, theme: DesktopTheme, l: &super::look::Look, r: Rect) {
        p.box_(r, Color::rgb(232, 236, 230), 0);
        if !theme.mobile() {
            p.vline(r.x, r.y, r.height, LINE);
        }
        let places = self.results();
        let Some(frame) = Frame::around(&places) else {
            notice(p, r.width, r.y + r.height as i32 / 2, "No places to map");
            return;
        };
        let inner = Rect::new(
            r.x + 18,
            r.y + 18,
            r.width.saturating_sub(36).max(1),
            r.height.saturating_sub(36).max(1),
        );
        // Grid lines are the frame's own quarters: a scale that is real, not decoration.
        for step in 1..4 {
            let x = inner.x + (inner.width as i32 * step) / 4;
            let y = inner.y + (inner.height as i32 * step) / 4;
            p.vline(x, inner.y, inner.height, Color(0, 0, 0, 12));
            p.hline(inner.x, y, inner.width, Color(0, 0, 0, 12));
        }
        if let Some(route) = &self.route {
            if let (Some(a), Some(b)) = (self.place(&route.from), self.place(&route.to)) {
                let (ax, ay) = frame.project(a.lat, a.lon, inner);
                let (bx, by) = frame.project(b.lat, b.lon, inner);
                // The service walks latitude first, then longitude; the drawn line takes
                // the same two legs rather than a diagonal it never travels.
                p.line(vec![(ax, ay), (ax, by), (bx, by)], l.accent, 3);
            }
        }
        for place in &places {
            let (x, y) = frame.project(place.lat, place.lon, inner);
            let on = self.selected.as_deref() == Some(place.id.as_str());
            let radius = if on { 9 } else { 6 };
            p.circle(x, y, radius + 2, Color::WHITE);
            p.circle(
                x,
                y,
                radius,
                if self.saved.contains(&place.id) {
                    Color::rgb(214, 158, 46)
                } else {
                    l.accent
                },
            );
            p.region(
                Rect::new(x - 12, y - 12, 24, 24),
                &format!("maps:place:{}", place.id),
                &place.name,
            );
            if on {
                p.label(
                    x - 70,
                    y + 12,
                    140,
                    &place.name,
                    11,
                    INK,
                    true,
                    Align::Center,
                );
            }
        }
    }
    fn list(&self, p: &mut Painter, theme: DesktopTheme, l: &super::look::Look, r: Rect) {
        p.box_(Rect::new(r.x, r.y, r.width, r.height), l.surface, 0);
        if theme.mobile() {
            p.hline(r.x, r.y, r.width, LINE);
        }
        let mut y = r.y + 8;
        // Travel modes: four real prices the service quotes, one of which is engaged.
        let mut x = r.x + 8;
        let chip_w = (r.width.saturating_sub(24) / 4).clamp(44, 86);
        for (mode, label) in TRAVEL {
            chip(
                p,
                l,
                Rect::new(x, y, chip_w, 26),
                label,
                &format!("maps:mode:{mode}"),
                self.travel == *mode,
            );
            x += chip_w as i32 + 4;
        }
        y += 34;
        let endpoints = Rect::new(r.x + 8, y, r.width.saturating_sub(16), 26);
        let go = Rect::new(endpoints.x + endpoints.width as i32 - 92, y, 92, 26);
        p.left(
            endpoints.x,
            y + 5,
            endpoints.width.saturating_sub(100),
            &format!(
                "{} → {}",
                self.endpoint(&self.origin),
                self.endpoint(&self.destination)
            ),
            11,
            MUTED,
        );
        match (&self.origin, &self.destination) {
            (Some(from), Some(to)) if from != to => {
                action(p, l, go, "Directions", "maps:route", true)
            }
            // Without two different places there is no route to ask for.
            _ => inert(p, l, go, "Directions", "Choose a start and an end first"),
        }
        y += 34;
        if let Some(route) = &self.route {
            p.strong(
                r.x + 10,
                y,
                r.width.saturating_sub(110),
                &format!(
                    "{} · {}",
                    distance_label(route.metres, self.imperial()),
                    duration_label(route.minutes)
                ),
                13,
                INK,
            );
            action(
                p,
                l,
                Rect::new(r.x + r.width as i32 - 82, y - 4, 72, 24),
                "Clear",
                "maps:clear-route",
                false,
            );
            y += 22;
            for step in &route.steps {
                if y as u32 + 18 > r.y as u32 + r.height {
                    return;
                }
                p.left(
                    r.x + 10,
                    y,
                    r.width.saturating_sub(20),
                    &step.text,
                    11,
                    MUTED,
                );
                y += 17;
            }
            return;
        }
        let places = self.results();
        if places.is_empty() {
            notice(p, r.width, y + 12, "No places");
            return;
        }
        let row = l.row.max(44);
        for place in places {
            if y as u32 + row > r.y as u32 + r.height {
                break;
            }
            let on = self.selected.as_deref() == Some(place.id.as_str());
            let card = Rect::new(r.x + 6, y, r.width.saturating_sub(12), row);
            p.button(
                card,
                if on { l.selection } else { Color::TRANSPARENT },
                l.radius,
                &format!("maps:place:{}", place.id),
                &place.name,
            );
            p.hline(card.x, card.y + card.height as i32, card.width, LINE);
            p.left(
                card.x + 10,
                card.y + 5,
                card.width.saturating_sub(84),
                &place.name,
                13,
                INK,
            );
            p.left(
                card.x + 10,
                card.y + 22,
                card.width.saturating_sub(84),
                &format!(
                    "{}{}{}",
                    place.kind,
                    if place.rating > 0 { " · " } else { "" },
                    if place.rating > 0 {
                        format!(
                            "{}.{} ({})",
                            place.rating / 10,
                            place.rating % 10,
                            place.reviews
                        )
                    } else {
                        String::new()
                    }
                ),
                11,
                MUTED,
            );
            let starred = self.saved.contains(&place.id);
            let star = Rect::new(card.x + card.width as i32 - 34, card.y + 6, 28, 24);
            p.button(
                star,
                if starred {
                    l.selection
                } else {
                    Color::TRANSPARENT
                },
                l.radius,
                &format!("maps:save:{}", place.id),
                if starred { "Saved" } else { "Save" },
            );
            p.symbol(
                if starred { "star" } else { "star-outline" },
                star.x + 6,
                star.y + 5,
                14,
                if starred {
                    Color::rgb(214, 158, 46)
                } else {
                    FAINT
                },
            );
            y += row as i32 + 1;
            if on {
                if y as u32 + 30 > r.y as u32 + r.height {
                    break;
                }
                p.left(
                    card.x + 10,
                    y,
                    card.width.saturating_sub(20),
                    &place.address,
                    11,
                    MUTED,
                );
                for (index, (prefix, label)) in [("from", "Start here"), ("to", "End here")]
                    .iter()
                    .enumerate()
                {
                    action(
                        p,
                        l,
                        Rect::new(card.x + 10 + index as i32 * 96, y + 14, 90, 24),
                        label,
                        &format!("maps:{prefix}:{}", place.id),
                        false,
                    );
                }
                y += 44;
            }
        }
    }
    fn endpoint(&self, id: &Option<String>) -> String {
        match id {
            Some(id) => self
                .place(id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| id.clone()),
            None => "—".into(),
        }
    }
}

/// The bounding box of the places on screen, in micro-degrees. Projection is integer
/// arithmetic on the service's own coordinates, so the same dataset plots identically
/// on every machine and every replay.
struct Frame {
    min_lat: i64,
    min_lon: i64,
    span_lat: i64,
    span_lon: i64,
}
impl Frame {
    fn around(places: &[&Place]) -> Option<Self> {
        let first = places.first()?;
        let (mut min_lat, mut max_lat) = (first.lat, first.lat);
        let (mut min_lon, mut max_lon) = (first.lon, first.lon);
        for place in places {
            min_lat = min_lat.min(place.lat);
            max_lat = max_lat.max(place.lat);
            min_lon = min_lon.min(place.lon);
            max_lon = max_lon.max(place.lon);
        }
        Some(Self {
            min_lat,
            min_lon,
            // A single place, or a row of them, still needs a span to divide by.
            span_lat: (max_lat - min_lat).max(1),
            span_lon: (max_lon - min_lon).max(1),
        })
    }
    /// North is up and east is right, which is the only orientation a map may have.
    fn project(&self, lat: i64, lon: i64, r: Rect) -> (i32, i32) {
        let x = i64::from(r.x) + (lon - self.min_lon) * i64::from(r.width) / self.span_lon;
        let y = i64::from(r.y) + i64::from(r.height)
            - (lat - self.min_lat) * i64::from(r.height) / self.span_lat;
        (x as i32, y as i32)
    }
}

/// Integer distance labels, the same arithmetic the service uses for its own pages.
fn distance_label(metres: i64, imperial: bool) -> String {
    if imperial {
        let tenths = (metres * 10 + 804) / 1_609;
        if tenths < 2 {
            return format!("{} ft", (metres * 3_281 + 500) / 1_000);
        }
        format!("{}.{} mi", tenths / 10, tenths % 10)
    } else if metres < 1_000 {
        format!("{metres} m")
    } else {
        let tenths = (metres + 50) / 100;
        format!("{}.{} km", tenths / 10, tenths % 10)
    }
}
fn duration_label(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PLACES: &str = r#"{
        "devcon-center": {"id":"devcon-center","name":"Devcon Center","kind":"Conference centre",
            "address":"410 Bayfront Ave, Seattle WA","lat":47606200,"lon":-122332100,
            "rating":46,"reviews":812,"hours":"9-6","phone":"555-0100","summary":"Halls and stages",
            "tags":["events"]},
        "harbor-cafe": {"id":"harbor-cafe","name":"Harbor Cafe","kind":"Cafe",
            "address":"12 Pier St, Seattle WA","lat":47610400,"lon":-122340900,
            "rating":42,"reviews":210,"summary":"Coffee by the water","tags":["coffee"]}
    }"#;
    fn app() -> Maps {
        let (mut app, effects) = Maps::launch("http://maps.google.com/", 1, 0);
        let tags: Vec<_> = effects
            .iter()
            .map(|e| match e {
                AppEffect::Http { tag, .. } => tag.clone(),
                _ => panic!("expected requests"),
            })
            .collect();
        assert_eq!(tags, vec!["places", "saved", "units"]);
        app.http(1, "places", 200, PLACES).unwrap();
        app.http(1, "saved", 200, r#"["harbor-cafe"]"#).unwrap();
        app.http(1, "units", 200, r#"{"units":"metric"}"#).unwrap();
        app
    }
    #[test]
    fn places_saved_and_units_all_come_from_the_service() {
        let app = app();
        assert_eq!(app.places.len(), 2);
        assert_eq!(app.places[0].id, "devcon-center");
        assert_eq!(app.places[0].lat, 47_606_200);
        assert_eq!(app.saved, vec!["harbor-cafe".to_owned()]);
        assert_eq!(app.units, "metric");
        assert!(!app.imperial());
    }
    #[test]
    fn search_narrows_the_service_dataset_and_never_adds_to_it() {
        let mut app = app();
        app.click(1, "maps:search-field", 0).unwrap();
        app.text("coffee").unwrap();
        let hits = app.results();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "harbor-cafe");
        app.text(" nowhere").unwrap();
        assert!(app.results().is_empty());
        app.key(1, "Escape", 0).unwrap();
        assert_eq!(app.results().len(), 2);
    }
    #[test]
    fn a_route_is_asked_for_then_read_back_from_the_service_cache() {
        let mut app = app();
        assert!(app.click(1, "maps:route", 0).is_err(), "no endpoints yet");
        app.click(1, "maps:from:devcon-center", 0).unwrap();
        app.click(1, "maps:to:devcon-center", 0).unwrap();
        assert!(
            app.click(1, "maps:route", 0).is_err(),
            "one place is not a route"
        );
        app.click(1, "maps:to:harbor-cafe", 0).unwrap();
        app.click(1, "maps:mode:walking", 0).unwrap();
        let effects = app.click(1, "maps:route", 0).unwrap();
        let AppEffect::Http { url, tag, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(tag, "dir");
        assert_eq!(
            url,
            "http://maps.google.com/maps/dir?from=devcon-center&to=harbor-cafe&mode=walking"
        );
        // The directions page is the trigger; the record itself is read from the cache.
        let more = app.http(1, "dir", 200, "{}").unwrap();
        assert!(matches!(&more[0], AppEffect::Http { url, .. } if url.ends_with("/api/routes")));
        app.http(
            1,
            "routes",
            200,
            r#"{"devcon-center|harbor-cafe|walking":{"id":"devcon-center|harbor-cafe|walking",
                "from":"devcon-center","to":"harbor-cafe","mode":"walking","metres":1060,
                "minutes":13,"steps":[{"text":"Head north on Bayfront Ave","metres":466}]}}"#,
        )
        .unwrap();
        let route = app.route.as_ref().unwrap();
        assert_eq!((route.metres, route.minutes), (1060, 13));
        assert_eq!(app.caption(), "1.1 km · 13 min");
        assert_eq!(distance_label(1060, true), "0.7 mi");
        app.click(1, "maps:clear-route", 0).unwrap();
        assert!(app.route.is_none());
    }
    #[test]
    fn saving_toggles_through_the_service_and_re_reads_the_list() {
        let mut app = app();
        let effects = app.click(1, "maps:save:devcon-center", 0).unwrap();
        let AppEffect::Http { method, url, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(method, "POST");
        assert_eq!(url, "http://maps.google.com/api/places/devcon-center/save");
        let more = app
            .http(1, "save", 200, r#"{"place":"devcon-center","saved":true}"#)
            .unwrap();
        assert!(matches!(&more[0], AppEffect::Http { url, .. } if url.ends_with("/api/saved")));
        assert!(app.click(1, "maps:save:nowhere", 0).is_err());
    }
    #[test]
    fn an_empty_dataset_renders_empty_and_a_refusal_is_shown() {
        let (mut app, _) = Maps::launch("", 1, 0);
        app.http(1, "places", 200, "{}").unwrap();
        assert!(app.results().is_empty());
        assert_eq!(app.caption(), "0 places");
        app.offline("places", "network unreachable");
        assert_eq!(app.status, Status::Offline("network unreachable".into()));
        app.http(1, "places", 403, r#"{"error":"places unavailable"}"#)
            .unwrap();
        assert_eq!(app.status, Status::Denied("places unavailable".into()));
    }
    #[test]
    fn projection_is_integer_and_puts_north_up_and_east_right() {
        let north = Place {
            lat: 2_000_000,
            lon: 1_000_000,
            ..Place::default()
        };
        let south_east = Place {
            lat: 1_000_000,
            lon: 2_000_000,
            ..Place::default()
        };
        let places = vec![&north, &south_east];
        let frame = Frame::around(&places).unwrap();
        let r = Rect::new(0, 0, 100, 100);
        assert_eq!(frame.project(north.lat, north.lon, r), (0, 0));
        assert_eq!(frame.project(south_east.lat, south_east.lon, r), (100, 100));
    }
    #[test]
    fn every_painted_control_is_one_the_model_accepts() {
        for theme in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
            DesktopTheme::Ios,
            DesktopTheme::Android,
        ] {
            let mut base = app();
            base.click(1, "maps:place:devcon-center", 0).unwrap();
            base.click(1, "maps:from:devcon-center", 0).unwrap();
            base.click(1, "maps:to:harbor-cafe", 0).unwrap();
            let mut scene = Painter::themed(theme, 960, 680, 0);
            base.render(
                &mut scene,
                &crate::AppEnv {
                    theme,
                    width: 960,
                    height: 680,
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
                let mut app = base.clone();
                assert!(
                    app.click(1, &target, 0).is_ok(),
                    "unhandled control {target} on {theme:?}"
                );
            }
        }
    }
}
