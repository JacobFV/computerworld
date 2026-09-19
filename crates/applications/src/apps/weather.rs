//! Weather over the `geo` service in weather mode — the dataset weather.com is built on.
//!
//! Cities, ten-day forecasts and alerts are service records; temperatures arrive as whole
//! Fahrenheit degrees and are converted with the same integer formula the service uses, so
//! a Celsius reading here and a Celsius reading on the site are the same number. A machine
//! that cannot reach the service shows that, and a world with no cities shows no cities.
use super::look::{action, chip, look, notice, screen, FAINT, INK, LINE, MUTED};
use super::Status;
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Retained bounds: a world may seed any number of cities, alerts or days.
pub const CITY_LIMIT: usize = 64;
pub const ALERT_LIMIT: usize = 32;
pub const DAY_LIMIT: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Day {
    pub day: String,
    pub hi_f: i64,
    pub lo_f: i64,
    pub cond: String,
    pub precip_pct: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Forecast {
    pub id: String,
    /// The place this city's weather is anchored to; maps and weather share one dataset.
    pub place: String,
    pub city: String,
    pub now_f: i64,
    pub cond: String,
    pub humidity_pct: i64,
    pub wind_mph: i64,
    pub days: Vec<Day>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alert {
    pub id: String,
    pub city: String,
    pub severity: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weather {
    pub base: String,
    pub cities: Vec<Forecast>,
    pub alerts: Vec<Alert>,
    /// City keys this person has saved, as the service reports them.
    pub saved: Vec<String>,
    /// `imperial` or `metric`, read from the service and changed through it.
    pub units: String,
    pub selected: Option<String>,
    /// Alerts this window acknowledged, recorded from the service's own reply. The alert
    /// list is not per-person on the wire, so this is what the app knows it did itself.
    pub acknowledged: Vec<String>,
    pub status: Status,
}
impl Weather {
    pub const KIND: &'static str = "weather";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let app = Self {
            base: if argument.is_empty() {
                "http://weather.internal/".into()
            } else {
                argument.to_owned()
            },
            cities: vec![],
            alerts: vec![],
            saved: vec![],
            units: "imperial".into(),
            selected: None,
            acknowledged: vec![],
            status: Status::Loading,
        };
        let effects = app.fetch(window);
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, _theme: DesktopTheme) -> String {
        "Weather".into()
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        match self.current() {
            Some(city) => format!(
                "{} · {} {}",
                city.city,
                temp(city.now_f, self.imperial()),
                city.cond
            ),
            None => String::new(),
        }
    }
    pub fn modified(&self) -> bool {
        false
    }
    fn imperial(&self) -> bool {
        self.units != "metric"
    }
    /// The city on screen: the chosen one, else the first the service listed.
    pub fn current(&self) -> Option<&Forecast> {
        match &self.selected {
            Some(id) => self.cities.iter().find(|c| c.id == *id),
            None => self.cities.first(),
        }
    }
    /// Alerts the service raised for the city on screen.
    pub fn current_alerts(&self) -> Vec<&Alert> {
        let Some(city) = self.current() else {
            return vec![];
        };
        self.alerts.iter().filter(|a| a.city == city.id).collect()
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
    fn post(&self, window: u64, tag: &str, suffix: &str, body: serde_json::Value) -> AppEffect {
        AppEffect::Http {
            window,
            tag: tag.into(),
            method: "POST".into(),
            url: self.url(suffix),
            body: body.to_string(),
        }
    }
    fn fetch(&self, window: u64) -> Vec<AppEffect> {
        vec![
            self.get(window, "forecasts", "/api/forecasts"),
            self.get(window, "alerts", "/api/alerts"),
            self.get(window, "saved", "/api/saved"),
            self.get(window, "units", "/api/units"),
        ]
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
            "forecasts" => {
                let cities: BTreeMap<String, Forecast> =
                    serde_json::from_str(body).unwrap_or_default();
                self.cities = cities
                    .into_values()
                    .take(CITY_LIMIT)
                    .map(|mut city| {
                        city.days.truncate(DAY_LIMIT);
                        city
                    })
                    .collect();
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|id| !self.cities.iter().any(|c| c.id == *id))
                {
                    self.selected = None;
                }
                Ok(vec![])
            }
            "alerts" => {
                self.alerts = serde_json::from_str::<Vec<Alert>>(body)
                    .unwrap_or_default()
                    .into_iter()
                    .take(ALERT_LIMIT)
                    .collect();
                Ok(vec![])
            }
            "saved" => {
                self.saved = serde_json::from_str::<Vec<String>>(body)
                    .unwrap_or_default()
                    .into_iter()
                    .take(CITY_LIMIT)
                    .collect();
                Ok(vec![])
            }
            // The service answers both the read and the write with the same shape, so one
            // arm is right for either.
            "units" => {
                self.units = serde_json::from_str::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("units")?.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "imperial".into());
                Ok(vec![])
            }
            "save" => {
                self.status = Status::Loading;
                Ok(vec![self.get(window, "saved", "/api/saved")])
            }
            "ack" => {
                if let Some(id) = serde_json::from_str::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("id")?.as_str().map(str::to_owned))
                {
                    if !self.acknowledged.contains(&id) {
                        self.acknowledged.push(id);
                        self.acknowledged.truncate(ALERT_LIMIT);
                    }
                }
                self.status = Status::Loading;
                Ok(vec![self.get(window, "alerts", "/api/alerts")])
            }
            other => Err(format!("unexpected weather reply {other}")),
        }
    }
    pub fn text(&mut self, _text: &str) -> Result<(), String> {
        Err("weather has no text field".into())
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        match key {
            "Ctrl+r" | "Meta+r" => self.click(window, "weather:reload", clock_us),
            other => Err(format!("unsupported weather key {other}")),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("weather:")
            .ok_or("interaction does not belong to weather")?;
        match command {
            "reload" => {
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            rest => {
                if let Some(id) = rest.strip_prefix("city:") {
                    if !self.cities.iter().any(|c| c.id == id) {
                        return Err("city not found".into());
                    }
                    self.selected = Some(id.to_owned());
                    return Ok(vec![]);
                }
                if let Some(id) = rest.strip_prefix("save:") {
                    if !self.cities.iter().any(|c| c.id == id) {
                        return Err("city not found".into());
                    }
                    self.status = Status::Loading;
                    return Ok(vec![self.post(
                        window,
                        "save",
                        "/api/locations",
                        serde_json::json!({ "city": id }),
                    )]);
                }
                if let Some(units) = rest.strip_prefix("units:") {
                    if !matches!(units, "imperial" | "metric") {
                        return Err(format!("unknown unit system {units}"));
                    }
                    self.status = Status::Loading;
                    return Ok(vec![self.post(
                        window,
                        "units",
                        "/api/units",
                        serde_json::json!({ "units": units }),
                    )]);
                }
                if let Some(id) = rest.strip_prefix("ack:") {
                    if !self.alerts.iter().any(|a| a.id == id) {
                        return Err("alert not found".into());
                    }
                    self.status = Status::Loading;
                    return Ok(vec![self.post(
                        window,
                        "ack",
                        &format!("/api/alerts/{id}/ack"),
                        serde_json::json!({}),
                    )]);
                }
                Err(format!("unknown weather command {command}"))
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
            id: "weather-title".into(),
            text: self.caption(),
            level: 2,
        });
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "weather-status".into(),
                text: text.into(),
            });
        }
        for (id, label) in [
            ("weather:reload", "Reload"),
            ("weather:units:imperial", "Fahrenheit"),
            ("weather:units:metric", "Celsius"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
                style: None,
            });
        }
        for city in &self.cities {
            page.elements.push(E::Button {
                id: format!("weather:city:{}", city.id),
                text: format!(
                    "{} — {} {}",
                    city.city,
                    temp(city.now_f, self.imperial()),
                    city.cond
                ),
                action: act(&format!("weather:city:{}", city.id)),
                style: None,
            });
            page.elements.push(E::Button {
                id: format!("weather:save:{}", city.id),
                text: format!(
                    "{} {}",
                    if self.saved.contains(&city.id) {
                        "Remove"
                    } else {
                        "Save"
                    },
                    city.city
                ),
                action: act(&format!("weather:save:{}", city.id)),
                style: None,
            });
        }
        if let Some(city) = self.current() {
            page.elements.push(E::Text {
                id: "weather-now".into(),
                text: format!(
                    "{} · humidity {}% · wind {} mph",
                    city.cond, city.humidity_pct, city.wind_mph
                ),
            });
            for (index, day) in city.days.iter().enumerate() {
                page.elements.push(E::Text {
                    id: format!("weather-day-{index}"),
                    text: format!(
                        "{} {} / {} {} {}%",
                        day.day,
                        temp(day.hi_f, self.imperial()),
                        temp(day.lo_f, self.imperial()),
                        day.cond,
                        day.precip_pct
                    ),
                });
            }
        }
        for alert in self.current_alerts() {
            page.elements.push(E::Text {
                id: format!("weather-alert-{}", alert.id),
                text: format!("{}: {} — {}", alert.severity, alert.title, alert.body),
            });
            page.elements.push(E::Button {
                id: format!("weather:ack:{}", alert.id),
                text: format!("Acknowledge {}", alert.title),
                action: act(&format!("weather:ack:{}", alert.id)),
                style: None,
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let screen = screen(p, theme, &l, width, height as i32, &self.title(theme));
        let mut top = screen.top;
        // Units and refresh: one toolbar on a desktop, one row of pills on a phone.
        let bar = if theme.mobile() { 44 } else { 36 };
        p.box_(Rect::new(0, top, width, bar), l.chrome, 0);
        p.hline(0, top + bar as i32, width, LINE);
        let mut x = 8;
        for (units, label) in [("imperial", "°F"), ("metric", "°C")] {
            chip(
                p,
                &l,
                Rect::new(x, top + 5, 46, bar - 10),
                label,
                &format!("weather:units:{units}"),
                self.units == units,
            );
            x += 50;
        }
        action(
            p,
            &l,
            Rect::new(width as i32 - 80, top + 5, 70, bar - 10),
            "Reload",
            "weather:reload",
            false,
        );
        top += bar as i32 + 1;
        if let Some(text) = self.status.notice() {
            notice(p, width, top + 8, text);
            top += 26;
        }
        if self.cities.is_empty() {
            notice(p, width, top + 24, "No locations");
            screen.end(p);
            return;
        }
        // A desktop keeps the city list in a sidebar; a phone puts it in a scrolling strip.
        let side = if theme.mobile() || width < 560 {
            0
        } else {
            210
        };
        if side > 0 {
            p.box_(
                Rect::new(
                    0,
                    top,
                    side as u32,
                    height.saturating_sub(top.max(0) as u32),
                ),
                l.chrome,
                0,
            );
            p.vline(side, top, height, LINE);
            let cities = screen.column(
                p,
                "cities",
                Rect::new(0, top, side as u32, (height as i32 - top).max(1) as u32),
            );
            self.sidebar(p, &l, cities.top, side as u32);
            cities.end(p);
        } else {
            top = self.strip(p, &l, top, width);
        }
        let body = Rect::new(
            side,
            top,
            width.saturating_sub(side as u32),
            (height as i32 - top).max(1) as u32,
        );
        let detail = screen.column(p, "detail", body);
        self.detail(
            p,
            theme,
            &l,
            Rect::new(body.x, detail.top, body.width, body.height),
        );
        detail.end(p);
        screen.end(p);
    }
    fn sidebar(&self, p: &mut Painter, l: &super::look::Look, top: i32, side: u32) {
        let mut y = top + 6;
        let current = self.current().map(|c| c.id.clone());
        for city in &self.cities {
            let on = current.as_deref() == Some(city.id.as_str());
            let r = Rect::new(5, y, side - 10, 42);
            p.button(
                r,
                if on { l.selection } else { Color::TRANSPARENT },
                l.radius,
                &format!("weather:city:{}", city.id),
                &city.city,
            );
            p.left(
                r.x + 10,
                r.y + 5,
                r.width.saturating_sub(84),
                &city.city,
                13,
                INK,
            );
            p.left(
                r.x + 10,
                r.y + 22,
                r.width.saturating_sub(84),
                &city.cond,
                11,
                MUTED,
            );
            p.right(
                r.x + r.width as i32 - 64,
                r.y + 12,
                54,
                &temp(city.now_f, self.imperial()),
                15,
                INK,
            );
            let starred = self.saved.contains(&city.id);
            let star = Rect::new(r.x + r.width as i32 - 32, r.y + 9, 26, 24);
            p.button(
                star,
                if starred {
                    l.selection
                } else {
                    Color::TRANSPARENT
                },
                l.radius,
                &format!("weather:save:{}", city.id),
                if starred { "Saved" } else { "Save" },
            );
            p.symbol(
                if starred { "star" } else { "star-outline" },
                star.x + 6,
                star.y + 5,
                14,
                if starred { l.accent } else { FAINT },
            );
            y += 44;
        }
    }
    /// The phone's cities: one pill per city, the chosen one engaged, wrapping onto as
    /// many rows as they need so every city stays reachable.
    fn strip(&self, p: &mut Painter, l: &super::look::Look, top: i32, width: u32) -> i32 {
        let current = self.current().map(|c| c.id.clone());
        let (mut x, mut y) = (12, top + 8);
        for city in &self.cities {
            let w = p.measure(&city.city, 12, false) + 26;
            if x > 12 && x + w as i32 > width as i32 - 12 {
                x = 12;
                y += 38;
            }
            chip(
                p,
                l,
                Rect::new(x, y, w, 30),
                &city.city,
                &format!("weather:city:{}", city.id),
                current.as_deref() == Some(city.id.as_str()),
            );
            x += w as i32 + 8;
        }
        y + 38
    }
    fn detail(&self, p: &mut Painter, theme: DesktopTheme, l: &super::look::Look, r: Rect) {
        let Some(city) = self.current() else {
            notice(p, r.width, r.y + 24, "No locations");
            return;
        };
        let mut y = r.y + 10;
        // The hero: the reading the service has right now, at the size the platform likes.
        let big = if theme.mobile() { 56 } else { 44 };
        p.symbol(
            condition_symbol(&city.cond),
            r.x + 16,
            y + 6,
            if theme.mobile() { 44 } else { 34 },
            l.accent,
        );
        p.label(
            r.x + 72,
            y,
            r.width.saturating_sub(96),
            &temp(city.now_f, self.imperial()),
            big,
            INK,
            true,
            Align::Left,
        );
        p.left(
            r.x + 16,
            y + big as i32 + 8,
            r.width.saturating_sub(32),
            &format!(
                "{} · {} · humidity {}% · wind {} mph",
                city.city, city.cond, city.humidity_pct, city.wind_mph
            ),
            12,
            MUTED,
        );
        y += big as i32 + 32;
        let starred = self.saved.contains(&city.id);
        action(
            p,
            l,
            Rect::new(r.x + 16, y, 118, 28),
            if starred { "Saved" } else { "Save city" },
            &format!("weather:save:{}", city.id),
            starred,
        );
        y += 38;
        for alert in self.current_alerts() {
            let acked = self.acknowledged.contains(&alert.id);
            let card = Rect::new(r.x + 12, y, r.width.saturating_sub(24), 48);
            p.box_(
                card,
                if acked {
                    Color(0, 0, 0, 10)
                } else {
                    Color(214, 92, 46, 34)
                },
                l.radius,
            );
            p.strong(
                card.x + 10,
                card.y + 6,
                card.width.saturating_sub(120),
                &format!("{}: {}", alert.severity, alert.title),
                12,
                INK,
            );
            p.left(
                card.x + 10,
                card.y + 24,
                card.width.saturating_sub(120),
                &alert.body,
                11,
                MUTED,
            );
            action(
                p,
                l,
                Rect::new(card.x + card.width as i32 - 104, card.y + 10, 94, 26),
                if acked { "Acknowledged" } else { "Acknowledge" },
                &format!("weather:ack:{}", alert.id),
                !acked,
            );
            y += 54;
        }
        if city.days.is_empty() {
            notice(p, r.width, y + 10, "No forecast");
            return;
        }
        p.strong(
            r.x + 16,
            y,
            r.width.saturating_sub(32),
            "Ten day",
            12,
            MUTED,
        );
        y += 20;
        let (lo, hi) = city.days.iter().fold((i64::MAX, i64::MIN), |(lo, hi), d| {
            (lo.min(d.lo_f), hi.max(d.hi_f))
        });
        let span = (hi - lo).max(1);
        for day in &city.days {
            p.left(r.x + 16, y + 4, 62, &day.day, 12, INK);
            p.left(r.x + 82, y + 5, 90, &day.cond, 11, MUTED);
            if day.precip_pct > 0 {
                p.left(
                    r.x + 176,
                    y + 5,
                    44,
                    &format!("{}%", day.precip_pct),
                    11,
                    l.accent,
                );
            }
            // The bar is the day's own range inside the ten-day range: real arithmetic,
            // integer all the way, on the numbers the service sent.
            let track = Rect::new(
                r.x + 228,
                y + 10,
                r.width.saturating_sub(228 + 104).max(16),
                6,
            );
            p.box_(track, Color(0, 0, 0, 16), 3);
            let from = (day.lo_f - lo) * i64::from(track.width) / span;
            let to = (day.hi_f - lo) * i64::from(track.width) / span;
            p.box_(
                Rect::new(track.x + from as i32, track.y, (to - from).max(2) as u32, 6),
                l.accent,
                3,
            );
            p.right(
                r.x + r.width as i32 - 96,
                y + 4,
                44,
                &temp(day.lo_f, self.imperial()),
                12,
                MUTED,
            );
            p.right(
                r.x + r.width as i32 - 46,
                y + 4,
                40,
                &temp(day.hi_f, self.imperial()),
                12,
                INK,
            );
            y += 26;
        }
    }
}

/// Integer Celsius, truncating toward zero — the service's own documented conversion.
fn temp(f: i64, imperial: bool) -> String {
    if imperial {
        format!("{f}°")
    } else {
        format!("{}°", (f - 32) * 5 / 9)
    }
}
/// Pick a bundled glyph from the condition the service wrote. The words are the service's;
/// only the choice of glyph is this application's.
fn condition_symbol(cond: &str) -> &'static str {
    let cond = cond.to_ascii_lowercase();
    const OVERCAST: &[&str] = &[
        "rain", "shower", "storm", "snow", "cloud", "overcast", "fog",
    ];
    if OVERCAST.iter().any(|word| cond.contains(word)) {
        "cloud"
    } else if cond.contains("night") {
        "moon"
    } else {
        "sun"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FORECASTS: &str = r#"{
        "seattle": {"id":"seattle","place":"devcon-center","city":"Seattle","now_f":54,
            "cond":"Light rain","humidity_pct":82,"wind_mph":7,
            "days":[{"day":"Thu","hi_f":58,"lo_f":48,"cond":"Rain","precip_pct":70},
                    {"day":"Fri","hi_f":63,"lo_f":50,"cond":"Cloudy","precip_pct":20}]},
        "austin": {"id":"austin","place":"live-oak","city":"Austin","now_f":88,
            "cond":"Sunny","humidity_pct":38,"wind_mph":11,"days":[]}
    }"#;
    const ALERTS: &str = r#"[{"id":"wx-1","city":"seattle","severity":"Advisory","title":"Wind advisory",
             "body":"Gusts to 40 mph through the evening."}]"#;
    fn app() -> Weather {
        let (mut app, effects) = Weather::launch("http://weather.com/", 1, 0);
        let tags: Vec<_> = effects
            .iter()
            .map(|e| match e {
                AppEffect::Http { tag, .. } => tag.clone(),
                _ => panic!("expected requests"),
            })
            .collect();
        assert_eq!(tags, vec!["forecasts", "alerts", "saved", "units"]);
        app.http(1, "forecasts", 200, FORECASTS).unwrap();
        app.http(1, "alerts", 200, ALERTS).unwrap();
        app.http(1, "saved", 200, r#"["seattle"]"#).unwrap();
        app.http(1, "units", 200, r#"{"units":"imperial"}"#)
            .unwrap();
        app
    }
    #[test]
    fn cities_forecasts_and_alerts_are_all_service_records() {
        let app = app();
        assert_eq!(app.cities.len(), 2);
        // BTreeMap order: austin before seattle, and the first city is what is shown.
        assert_eq!(app.current().unwrap().id, "austin");
        assert_eq!(app.caption(), "Austin · 88° Sunny");
        assert!(
            app.current_alerts().is_empty(),
            "the alert belongs to Seattle"
        );
        assert_eq!(app.saved, vec!["seattle".to_owned()]);
    }
    #[test]
    fn choosing_a_city_brings_its_forecast_and_its_alerts() {
        let mut app = app();
        assert!(app.click(1, "weather:city:nowhere", 0).is_err());
        app.click(1, "weather:city:seattle", 0).unwrap();
        let city = app.current().unwrap();
        assert_eq!(city.days.len(), 2);
        assert_eq!(city.days[0].hi_f, 58);
        assert_eq!(app.current_alerts().len(), 1);
    }
    #[test]
    fn units_are_changed_through_the_service_and_converted_its_way() {
        let mut app = app();
        assert_eq!(temp(88, true), "88°");
        let effects = app.click(1, "weather:units:metric", 0).unwrap();
        let AppEffect::Http {
            method, url, body, ..
        } = &effects[0]
        else {
            panic!("expected a request");
        };
        assert_eq!(
            (method.as_str(), url.as_str()),
            ("POST", "http://weather.com/api/units")
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap()["units"],
            "metric"
        );
        app.http(1, "units", 200, r#"{"units":"metric"}"#).unwrap();
        assert!(!app.imperial());
        assert_eq!(temp(88, false), "31°");
        assert_eq!(app.caption(), "Austin · 31° Sunny");
        assert!(app.click(1, "weather:units:kelvin", 0).is_err());
    }
    #[test]
    fn saving_and_acknowledging_post_real_routes_and_re_read() {
        let mut app = app();
        let effects = app.click(1, "weather:save:austin", 0).unwrap();
        let AppEffect::Http { url, body, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(url, "http://weather.com/api/locations");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap()["city"],
            "austin"
        );
        let more = app
            .http(1, "save", 200, r#"{"city":"austin","saved":true}"#)
            .unwrap();
        assert!(matches!(&more[0], AppEffect::Http { url, .. } if url.ends_with("/api/saved")));
        assert!(app.click(1, "weather:ack:nope", 0).is_err());
        let effects = app.click(1, "weather:ack:wx-1", 0).unwrap();
        assert!(
            matches!(&effects[0], AppEffect::Http { url, .. } if url == "http://weather.com/api/alerts/wx-1/ack")
        );
        app.http(
            1,
            "ack",
            200,
            r#"{"id":"wx-1","city":"seattle","acked":["alice"]}"#,
        )
        .unwrap();
        assert_eq!(app.acknowledged, vec!["wx-1".to_owned()]);
    }
    #[test]
    fn an_empty_world_and_an_unreachable_service_are_both_shown_plainly() {
        let (mut app, _) = Weather::launch("", 1, 0);
        app.http(1, "forecasts", 200, "{}").unwrap();
        assert!(app.cities.is_empty());
        assert_eq!(app.caption(), "");
        app.offline("forecasts", "network unreachable");
        assert_eq!(app.status, Status::Offline("network unreachable".into()));
        app.http(1, "forecasts", 403, r#"{"error":"forecast unavailable"}"#)
            .unwrap();
        assert_eq!(app.status, Status::Denied("forecast unavailable".into()));
        assert!(app.text("x").is_err());
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
            base.click(1, "weather:city:seattle", 0).unwrap();
            let mut scene = Painter::themed(theme, 900, 660, 0);
            base.render(
                &mut scene,
                &crate::AppEnv {
                    theme,
                    width: 900,
                    height: 660,
                    clock_us: 0,
                    settings: &crate::SystemSettings::DEFAULT,
                    clipboard: None,
                    share_to: None,
                    editor: None,
                    pointer: None,
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
