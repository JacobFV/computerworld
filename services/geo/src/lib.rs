//! Places: maps.google.com and openstreetmap.org (`mode: maps`), weather.com (`mode: weather`).
//! One `places` dataset backs both, which is why they are one crate.
//!
//! Every coordinate is an integer micro-degree (47.606200 is `47_606_200`) and every distance,
//! duration and temperature is derived from those integers alone. No float ever enters state, so
//! a route computed on one replay is byte-identical to the same route on the next.
use cw_protocol::{HttpRequest, HttpResponse, PageTheme, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
mod view;
pub struct GeoService;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(GeoService)
}
/// Documented seed keys, checked by container type only — the crate that fills them owns the rest.
const OBJECTS: &[&str] = &[
    "theme",
    "places",
    "routes",
    "saved",
    "forecasts",
    "unit_prefs",
];
const ARRAYS: &[&str] = &["alerts", "notes"];
/// `mode` is the documented discriminant; an unlisted value is a seed typo, not a fallback.
const MODES: &[&str] = &["maps", "weather"];
/// Metres in 1000 micro-degrees of latitude. Fixed, so the same pair of places always measures
/// the same; a real geodesic would need floats and would not survive a replay bit for bit.
const METRES_PER_KILO_MICRODEG: i64 = 111;
/// cos(latitude) at the world's working latitude, in parts per thousand — longitude degrees are
/// shorter than latitude degrees, and this is that correction without trigonometry.
const LON_SCALE_PER_MIL: i64 = 674;
/// Travel modes and their fixed speeds in metres per minute: urban driving, bus, bike, foot.
pub(crate) const TRAVEL: &[(&str, i64, &str)] = &[
    ("driving", 400, "Drive"),
    ("transit", 250, "Transit"),
    ("cycling", 220, "Bike"),
    ("walking", 80, "Walk"),
];
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeoState {
    pub mode: String,
    pub brand: String,
    pub tagline: String,
    /// The look: `gmaps`, `osm` or `weather`. Absent, the mode and the brand decide.
    pub skin: String,
    /// Seed default, `imperial` or `metric`; a person's own choice lives in `unit_prefs`.
    pub units: String,
    pub theme: PageTheme,
    pub places: BTreeMap<String, Place>,
    /// Cache keyed `from|to|mode`, filled on first lookup so a repeat is visibly instant.
    pub routes: BTreeMap<String, Route>,
    /// actor -> place ids (maps) or forecast city keys (weather). Saving is per person.
    pub saved: BTreeMap<String, Vec<String>>,
    pub unit_prefs: BTreeMap<String, String>,
    pub forecasts: BTreeMap<String, Forecast>,
    pub alerts: Vec<Alert>,
    pub notes: Vec<Note>,
    pub next_note: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub address: String,
    /// Micro-degrees. Integers all the way down.
    pub lat: i64,
    pub lon: i64,
    /// Tenths of a star: 46 renders as 4.6.
    pub rating: i64,
    pub reviews: i64,
    pub hours: String,
    pub phone: String,
    pub website: String,
    pub summary: String,
    pub tags: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
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
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Step {
    pub text: String,
    pub metres: i64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Forecast {
    pub id: String,
    /// The place this city's weather is anchored to, so maps and weather share one dataset.
    pub place: String,
    pub city: String,
    pub now_f: i64,
    pub cond: String,
    pub humidity_pct: i64,
    pub wind_mph: i64,
    pub days: Vec<Day>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Day {
    pub day: String,
    pub hi_f: i64,
    pub lo_f: i64,
    pub cond: String,
    pub precip_pct: i64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Alert {
    pub id: String,
    pub city: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub tick: u64,
    /// Acknowledging is per person, so one actor dismissing it does not hide it from another.
    pub acked: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Note {
    pub id: String,
    pub author: String,
    pub place: String,
    pub lat: i64,
    pub lon: i64,
    pub text: String,
    pub tick: u64,
}
/// `47_606_200` -> `47.606200`; the sign belongs to the whole number, not to the fraction.
pub(crate) fn coord(v: i64) -> String {
    format!(
        "{}{}.{:06}",
        if v < 0 { "-" } else { "" },
        v.abs() / 1_000_000,
        v.abs() % 1_000_000
    )
}
pub(crate) fn stars(tenths: i64) -> String {
    format!("{}.{}", tenths / 10, tenths % 10)
}
/// Manhattan distance on the integer grid: city blocks, not crow flight, and no square roots.
fn distance_m(a: &Place, b: &Place) -> i64 {
    let lat = (a.lat - b.lat).abs() * METRES_PER_KILO_MICRODEG / 1_000;
    let lon = (a.lon - b.lon).abs() * METRES_PER_KILO_MICRODEG * LON_SCALE_PER_MIL / 1_000_000;
    lat + lon
}
fn leg_m(delta: i64, scaled: bool) -> i64 {
    let base = delta.abs() * METRES_PER_KILO_MICRODEG;
    if scaled {
        base * LON_SCALE_PER_MIL / 1_000_000
    } else {
        base / 1_000
    }
}
fn speed_of(mode: &str) -> Option<i64> {
    TRAVEL
        .iter()
        .find(|(m, _, _)| *m == mode)
        .map(|(_, s, _)| *s)
}
/// Half-up rounding, and never zero: a trip you can see on a map takes at least a minute.
fn minutes_for(metres: i64, speed: i64) -> i64 {
    ((metres + speed / 2) / speed).max(1)
}
pub(crate) fn distance_label(metres: i64, imperial: bool) -> String {
    if imperial {
        let tenths = (metres * 10 + 804) / 1_609;
        // Under a fifth of a mile a driver thinks in feet, not in a leading zero.
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
pub(crate) fn duration_label(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}
/// Integer Celsius, truncating toward zero — documented, and the same on every machine.
fn to_c(f: i64) -> i64 {
    (f - 32) * 5 / 9
}
pub(crate) fn temp(f: i64, imperial: bool) -> String {
    if imperial {
        format!("{f}°")
    } else {
        format!("{}°", to_c(f))
    }
}
/// "410 Bayfront Ave, Seattle WA" -> "Bayfront Ave": the street a turn instruction names.
fn street(address: &str) -> String {
    let head = address.split(',').next().unwrap_or(address).trim();
    let without_number = head
        .split_once(' ')
        .filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit()))
        .map_or(head, |(_, rest)| rest);
    if without_number.is_empty() {
        "the main road".into()
    } else {
        without_number.into()
    }
}
/// Percent-encoding for a query value: place ids are slugs, but nothing here assumes so.
pub(crate) fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
fn tokens(q: &str) -> Vec<String> {
    q.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}
impl Place {
    fn score(&self, terms: &[String]) -> u64 {
        let name = self.name.to_ascii_lowercase();
        let body = format!(
            "{} {} {} {}",
            self.address,
            self.kind,
            self.summary,
            self.tags.join(" ")
        )
        .to_ascii_lowercase();
        terms
            .iter()
            .map(|t| 8 * u64::from(name.contains(t)) + u64::from(body.contains(t)))
            .sum()
    }
}
impl GeoState {
    pub fn maps(&self) -> bool {
        self.mode != "weather"
    }
    pub fn place(&self, id: &str) -> std::result::Result<&Place, String> {
        self.places.get(id).ok_or_else(|| "unknown place".into())
    }
    pub fn forecast(&self, city: &str) -> std::result::Result<&Forecast, String> {
        self.forecasts
            .get(city)
            .ok_or_else(|| "unknown location".into())
    }
    /// Highest score first, then by id, so equal matches never depend on map iteration order.
    pub fn find_places(&self, q: &str) -> Vec<&Place> {
        let terms = tokens(q);
        let mut hits: Vec<(u64, &Place)> = self
            .places
            .values()
            .map(|p| (if terms.is_empty() { 1 } else { p.score(&terms) }, p))
            .filter(|(s, _)| *s > 0)
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        hits.into_iter().map(|(_, p)| p).collect()
    }
    pub fn find_cities(&self, q: &str) -> Vec<&Forecast> {
        let needle = q.trim().to_ascii_lowercase();
        self.forecasts
            .values()
            .filter(|f| {
                needle.is_empty()
                    || f.city.to_ascii_lowercase().contains(&needle)
                    || f.id.contains(&needle)
            })
            .collect()
    }
    pub(crate) fn saved_of(&self, actor: &str) -> Vec<String> {
        self.saved.get(actor).cloned().unwrap_or_default()
    }
    /// Seed default unless this person has chosen otherwise.
    pub fn units_for(&self, actor: &str) -> String {
        match self.unit_prefs.get(actor) {
            Some(u) => u.clone(),
            None if self.units.is_empty() => "imperial".into(),
            None => self.units.clone(),
        }
    }
    pub(crate) fn imperial(&self, actor: &str) -> bool {
        self.units_for(actor) != "metric"
    }
    /// The city a bare `/` shows: the person's first saved location, else the first seeded one.
    pub(crate) fn default_city(&self, actor: &str) -> String {
        self.saved_of(actor)
            .into_iter()
            .find(|c| self.forecasts.contains_key(c))
            .or_else(|| self.forecasts.keys().next().cloned())
            .unwrap_or_default()
    }
    pub(crate) fn alerts_for(&self, city: &str, actor: &str) -> Vec<&Alert> {
        self.alerts
            .iter()
            .filter(|a| (city.is_empty() || a.city == city) && !a.acked.iter().any(|p| p == actor))
            .collect()
    }
    pub(crate) fn notes_at(&self, place: &str) -> Vec<&Note> {
        self.notes.iter().filter(|n| n.place == place).collect()
    }
    /// Toggle: the second save removes it, which is what every "Your places" star really does.
    pub fn toggle_saved(&mut self, actor: &str, id: &str) -> std::result::Result<bool, String> {
        if self.maps() {
            self.place(id)?;
        } else {
            self.forecast(id)?;
        }
        let list = self.saved.entry(actor.to_owned()).or_default();
        let on = match list.iter().position(|x| x == id) {
            Some(at) => {
                list.remove(at);
                false
            }
            None => {
                list.push(id.to_owned());
                true
            }
        };
        if list.is_empty() {
            self.saved.remove(actor);
        }
        Ok(on)
    }
    /// Steps are generated from the sign of the two deltas, latitude leg first — a fixed order,
    /// so the same pair of places always yields the same words.
    fn steps_between(&self, from: &Place, to: &Place) -> Vec<Step> {
        let imperial = self.units != "metric";
        let mut steps = vec![Step {
            text: format!("Start at {}, {}", from.name, from.address),
            metres: 0,
        }];
        let lat = leg_m(to.lat - from.lat, false);
        if lat > 0 {
            let heading = if to.lat > from.lat { "north" } else { "south" };
            steps.push(Step {
                text: format!(
                    "Head {heading} on {} — {}",
                    street(&from.address),
                    distance_label(lat, imperial)
                ),
                metres: lat,
            });
        }
        let lon = leg_m(to.lon - from.lon, true);
        if lon > 0 {
            let heading = if to.lon > from.lon { "east" } else { "west" };
            steps.push(Step {
                text: format!(
                    "Turn {heading} onto {} — {}",
                    street(&to.address),
                    distance_label(lon, imperial)
                ),
                metres: lon,
            });
        }
        steps.push(Step {
            text: format!("Arrive at {}, {}", to.name, to.address),
            metres: 0,
        });
        steps
    }
    /// Cached on first lookup. The cache is the same value the computation produces, so a hit and
    /// a miss are indistinguishable except in speed.
    pub fn directions(
        &mut self,
        from: &str,
        to: &str,
        mode: &str,
    ) -> std::result::Result<Route, String> {
        let speed = speed_of(mode).ok_or_else(|| format!("unknown travel mode {mode}"))?;
        if from == to {
            return Err("from and to are the same place".into());
        }
        let key = format!("{from}|{to}|{mode}");
        if let Some(hit) = self.routes.get(&key) {
            return Ok(hit.clone());
        }
        let a = self.place(from)?.clone();
        let b = self.place(to)?.clone();
        let metres = distance_m(&a, &b);
        let route = Route {
            id: key.clone(),
            from: from.to_owned(),
            to: to.to_owned(),
            mode: mode.to_owned(),
            metres,
            minutes: minutes_for(metres, speed),
            steps: self.steps_between(&a, &b),
        };
        self.routes.insert(key, route.clone());
        Ok(route)
    }
    /// An OSM note: a real report pinned to a real place, kept in seeded order.
    pub fn add_note(
        &mut self,
        actor: &str,
        place: &str,
        body: &str,
        tick: u64,
    ) -> std::result::Result<Note, String> {
        let text = body.trim();
        if text.is_empty() {
            return Err("note text is required".into());
        }
        let at = self.place(place)?;
        let note = Note {
            id: format!("note-{}", self.next_note.max(1)),
            author: actor.to_owned(),
            place: place.to_owned(),
            lat: at.lat,
            lon: at.lon,
            text: text.to_owned(),
            tick,
        };
        self.next_note = self.next_note.max(1) + 1;
        self.notes.push(note.clone());
        Ok(note)
    }
    /// `f`/`c` are what the control sends; the stored names stay `imperial`/`metric`.
    pub fn set_units(&mut self, actor: &str, want: &str) -> std::result::Result<String, String> {
        let units = match want.trim().to_ascii_lowercase().as_str() {
            "f" | "imperial" => "imperial",
            "c" | "metric" => "metric",
            other => return Err(format!("unknown units {other}")),
        };
        self.unit_prefs.insert(actor.to_owned(), units.to_owned());
        Ok(units.to_owned())
    }
    pub fn ack_alert(&mut self, actor: &str, id: &str) -> std::result::Result<Alert, String> {
        let alert = self
            .alerts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| "unknown alert".to_string())?;
        if !alert.acked.iter().any(|p| p == actor) {
            alert.acked.push(actor.to_owned());
        }
        Ok(alert.clone())
    }
}
/// Widest and tallest map a page may ask for.
const MAP_LIMIT: (u32, u32) = (960, 480);
/// Micro-degrees of latitude a map centred on a place spans at zoom 0; each zoom level halves it.
const MAP_SPAN_AT_ZOOM_0: i64 = 64_000;
pub(crate) const MAP_ZOOM_DEFAULT: u32 = 3;
/// `GET /map.rgba?w=&h=&center=<place>&zoom=<n>&route=<from>|<to>|<mode>&sel=<place>`: the map
/// as a page image. A route frames both ends; otherwise `center` frames one place at `zoom`;
/// otherwise every place is in view.
fn map_response(s: &GeoState, r: &HttpRequest) -> Result<HttpResponse> {
    let number = |k: &str| web::query(r, k).and_then(|v| v.parse::<u32>().ok());
    let width = number("w").unwrap_or(640).clamp(16, MAP_LIMIT.0);
    let height = number("h").unwrap_or(320).clamp(16, MAP_LIMIT.1);
    let places: Vec<cw_map::Place> = s
        .places
        .values()
        .map(|p| cw_map::Place {
            id: p.id.clone(),
            name: p.name.clone(),
            kind: p.kind.clone(),
            lat: p.lat,
            lon: p.lon,
            street: street(&p.address),
        })
        .collect();
    let world = cw_map::Bbox::around(places.iter().map(|p| (p.lat, p.lon)))
        .unwrap_or_else(|| cw_map::Bbox::centred(0, 0, MAP_SPAN_AT_ZOOM_0));
    let route = web::query(r, "route").and_then(|spec| {
        let mut parts = spec.split('|');
        let from = s.places.get(parts.next()?)?;
        let to = s.places.get(parts.next()?)?;
        Some(cw_map::Route {
            from: (from.lat, from.lon),
            to: (to.lat, to.lon),
        })
    });
    let center = web::query(r, "center").and_then(|id| s.places.get(&id));
    let bbox = match (route, center) {
        (Some(route), _) => cw_map::Bbox::around([route.from, route.to])
            .unwrap_or(world)
            .padded(25, 6_000),
        (None, Some(place)) => {
            let zoom = number("zoom").unwrap_or(MAP_ZOOM_DEFAULT).min(8);
            cw_map::Bbox::centred(place.lat, place.lon, MAP_SPAN_AT_ZOOM_0 >> zoom)
        }
        (None, None) => world.padded(10, 4_000),
    };
    let selected = web::query(r, "sel");
    let scene = cw_map::Scene {
        view: cw_map::View::new(bbox, width, height),
        places: &places,
        selected: selected.as_deref(),
        route,
        world,
    };
    web::rgba_response(width, height, &cw_map::render(&scene))
}
impl Service for GeoService {
    fn kind(&self) -> &str {
        "geo"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        let mode = web::variant(&gated, "mode", MODES)?;
        web::variant(&gated, "skin", view::SKINS)?;
        web::theme(&gated)?;
        let mut s: GeoState = web::load(&gated)?;
        s.mode = mode;
        if s.brand.is_empty() {
            s.brand = "Geo".into();
        }
        if s.units.is_empty() {
            s.units = "imperial".into();
        }
        if s.units != "imperial" && s.units != "metric" {
            return Err(cw_protocol::SimError::invalid(format!(
                "unknown units {}; expected imperial or metric",
                s.units
            )));
        }
        // Ids are the map keys; a seed that leaves them blank still renders links that resolve.
        for (id, place) in s.places.iter_mut() {
            if place.id.is_empty() {
                place.id = id.clone();
            }
        }
        for (id, forecast) in s.forecasts.iter_mut() {
            if forecast.id.is_empty() {
                forecast.id = id.clone();
            }
            if forecast.city.is_empty() {
                forecast.city = id.clone();
            }
        }
        s.next_note = s.next_note.max(s.notes.len() as u64 + 1);
        Ok(serde_json::to_value(s)?)
    }
    fn handle(
        &self,
        state: &mut Value,
        c: &ServiceContext,
        r: &HttpRequest,
    ) -> Result<HttpResponse> {
        let mut s: GeoState = web::load(state)?;
        let path = web::path(r);
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        let method = r.method.to_ascii_uppercase();
        if method == "GET" {
            // Directions are the one GET that writes: the computed route is cached under
            // `from|to|mode`, and a cache hit returns the identical value.
            if let ["maps", "dir"] = parts.as_slice() {
                let from = web::query(r, "from").unwrap_or_default();
                let to = web::query(r, "to").unwrap_or_default();
                let mode = web::query(r, "mode").unwrap_or_else(|| "driving".into());
                return match s.directions(&from, &to, &mode) {
                    Ok(route) => {
                        web::save(state, &s)?;
                        view::directions_page(&s, &c.actor, &route)
                    }
                    Err(e) => web::error(400, e),
                };
            }
            return match parts.as_slice() {
                [""] | ["maps"] if s.maps() => view::maps_home(&s, &c.actor),
                [""] => view::weather_home(&s, &c.actor),
                ["map.rgba"] if s.maps() => map_response(&s, r),
                ["maps", "place", id] => view::place_page(&s, &c.actor, id),
                ["maps", "saved"] => view::saved_page(&s, &c.actor),
                ["maps", "notes"] => view::notes_page(&s, &c.actor),
                ["search"] => {
                    view::search_page(&s, &c.actor, &web::query(r, "q").unwrap_or_default())
                }
                ["weather", "today", "l", city] => view::weather_page(&s, &c.actor, city, false),
                ["weather", "tenday", "l", city] => view::weather_page(&s, &c.actor, city, true),
                ["api", "places"] => HttpResponse::json(200, &s.places),
                ["api", "places", id] => web::domain(s.place(id).map(|x| json!(x))),
                ["api", "forecasts"] => HttpResponse::json(200, &s.forecasts),
                ["api", "forecasts", city] => web::domain(s.forecast(city).map(|x| json!(x))),
                ["api", "routes"] => HttpResponse::json(200, &s.routes),
                ["api", "saved"] => HttpResponse::json(200, &s.saved_of(&c.actor)),
                ["api", "notes"] => HttpResponse::json(200, &s.notes),
                ["api", "alerts"] => HttpResponse::json(200, &s.alerts),
                ["api", "units"] => {
                    HttpResponse::json(200, &json!({"units": s.units_for(&c.actor)}))
                }
                _ => web::error(404, "route not found"),
            };
        }
        if method != "POST" {
            return web::error(405, "method not allowed");
        }
        let b = web::body(r)?;
        let text = |k: &str| web::text(&b, k);
        let outcome: std::result::Result<(Value, String), String> = match parts.as_slice() {
            ["api", "places", id, "save"] => s.toggle_saved(&c.actor, id).map(|on| {
                (
                    json!({"place": id, "saved": on}),
                    format!("/maps/place/{id}"),
                )
            }),
            ["api", "notes"] => s
                .add_note(&c.actor, &text("place"), &text("text"), c.tick)
                .map(|n| {
                    let url = format!("/maps/place/{}", n.place);
                    (json!(n), url)
                }),
            ["api", "locations"] => {
                let city = text("city");
                s.toggle_saved(&c.actor, &city).map(|on| {
                    (
                        json!({"city": city.clone(), "saved": on}),
                        format!("/weather/today/l/{city}"),
                    )
                })
            }
            ["api", "units"] => s.set_units(&c.actor, &text("units")).map(|u| {
                let city = s.default_city(&c.actor);
                (json!({"units": u}), format!("/weather/today/l/{city}"))
            }),
            ["api", "alerts", id, "ack"] => s.ack_alert(&c.actor, id).map(|a| {
                let url = format!("/weather/today/l/{}", a.city);
                (json!(a), url)
            }),
            _ => return web::error(404, "route not found"),
        };
        let (value, next) = match outcome {
            Ok(v) => v,
            Err(e) => return web::domain::<Value>(Err(e)),
        };
        web::save(state, &s)?;
        // A browser form wants the page it just changed; an API client wants the record.
        let submitted = r
            .header("content-type")
            .is_some_and(|h| h.starts_with("application/x-www-form-urlencoded"));
        if !submitted {
            return HttpResponse::json(200, &value);
        }
        let parts: Vec<&str> = next.trim_matches('/').split('/').collect();
        match parts.as_slice() {
            ["maps", "place", id] => view::place_page(&s, &c.actor, id),
            ["weather", "today", "l", city] => view::weather_page(&s, &c.actor, city, false),
            _ => HttpResponse::json(200, &value),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "alice".into(),
            source: "alice-mac".into(),
            tick: 7,
            seed: 1,
            instance: "geo".into(),
        }
    }
    fn maps_seed() -> Value {
        json!({
            "mode": "maps", "brand": "Testmaps", "units": "imperial",
            "theme": {"accent": "#1a73e8"},
            "places": {
                "northstar-hq": {"name": "Northstar HQ", "kind": "office",
                    "address": "410 Bayfront Ave, Seattle WA",
                    "lat": 47572600, "lon": -122348000, "rating": 46, "reviews": 128,
                    "hours": "Mon-Fri 8:00-18:00", "phone": "+1 206 555 0148",
                    "website": "http://northstar.example/", "summary": "Bayfront campus.",
                    "tags": ["office"]},
                "devcon-center": {"name": "Cascade Convention Center", "kind": "venue",
                    "address": "800 Pike St, Seattle WA",
                    "lat": 47611400, "lon": -122333000, "rating": 43, "reviews": 900,
                    "hours": "Varies", "summary": "Convention hall.", "tags": ["venue"]}
            }
        })
    }
    fn weather_seed() -> Value {
        json!({
            "mode": "weather", "brand": "Testweather", "units": "imperial",
            "forecasts": {
                "seattle": {"place": "northstar-hq", "city": "Seattle, WA", "now_f": 57,
                    "cond": "Rain", "humidity_pct": 84, "wind_mph": 9,
                    "days": [{"day": "Mon", "hi_f": 61, "lo_f": 48, "cond": "Rain", "precip_pct": 80},
                             {"day": "Tue", "hi_f": 64, "lo_f": 50, "cond": "Cloudy", "precip_pct": 20}]}
            },
            "alerts": [{"id": "wx-1", "city": "seattle", "severity": "Advisory",
                        "title": "Wind advisory", "body": "Gusts to 40 mph.", "tick": 0}]
        })
    }
    fn init(seed: Value) -> Value {
        GeoService.initialize(seed, &ctx()).unwrap()
    }
    fn get(state: &mut Value, url: &str) -> HttpResponse {
        GeoService
            .handle(state, &ctx(), &HttpRequest::get(url))
            .unwrap()
    }
    fn post(state: &mut Value, url: &str, body: &[(&str, &str)]) -> HttpResponse {
        let mut r = HttpRequest::get(url);
        r.method = "POST".into();
        r.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        r.body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(body)
            .finish()
            .into_bytes();
        GeoService.handle(state, &ctx(), &r).unwrap()
    }
    fn body(r: &HttpResponse) -> String {
        String::from_utf8(r.body.clone()).unwrap()
    }
    /// A page response, parsed: it must be HTML and pass the engine's strict validator.
    struct Dom(cw_web::dom::Document);
    impl Dom {
        fn of(r: &HttpResponse) -> Dom {
            assert_eq!(r.status, 200);
            assert_eq!(
                r.headers.get("content-type").map(String::as_str),
                Some(web::html::HTML_MEDIA_TYPE)
            );
            let html = body(r);
            web::html::validate_strict(&html).unwrap_or_else(|e| panic!("{e:?}"));
            Dom(cw_web::html::parse(&html))
        }
        fn has(&self, id: &str) -> bool {
            !self.0.by_id(id).is_empty()
        }
        fn node(&self, id: &str) -> cw_web::dom::NodeId {
            *self.0.by_id(id).first().unwrap_or_else(|| panic!("no element #{id}"))
        }
        fn text(&self, id: &str) -> String {
            self.0.text_content(self.node(id))
        }
        fn attr(&self, id: &str, name: &str) -> String {
            self.0.attr(self.node(id), name).unwrap_or_default().to_owned()
        }
        fn tag(&self, id: &str) -> String {
            self.0.tag(self.node(id)).unwrap_or_default().to_owned()
        }
        /// `<body class>`: the skin the sheet keys its variants on.
        fn body_class(&self) -> String {
            let d = &self.0;
            d.descendants(cw_web::dom::Document::ROOT)
                .find(|n| d.is(*n, "body"))
                .and_then(|n| d.attr(n, "class"))
                .unwrap_or_default()
                .to_owned()
        }
        /// `(name, value)` of every named control inside a form, in document order.
        fn fields(&self, form: &str) -> Vec<(String, String)> {
            let d = &self.0;
            d.descendants(self.node(form))
                .filter(|n| d.is(*n, "input"))
                .filter_map(|n| Some((d.attr(n, "name")?.to_owned(), d.attr(n, "value").unwrap_or_default().to_owned())))
                .collect()
        }
    }
    fn has_float(v: &Value) -> bool {
        match v {
            Value::Number(n) => n.as_i64().is_none() && n.as_u64().is_none(),
            Value::Array(a) => a.iter().any(has_float),
            Value::Object(o) => o.values().any(has_float),
            _ => false,
        }
    }
    #[test]
    fn seed_shape_is_gated_at_load() {
        assert!(GeoService.initialize(json!([]), &ctx()).is_err());
        assert!(GeoService
            .initialize(json!({"places": []}), &ctx())
            .is_err());
        assert!(GeoService
            .initialize(json!({"mode": "nonesuch"}), &ctx())
            .is_err());
        assert!(GeoService
            .initialize(json!({"units": "furlongs"}), &ctx())
            .is_err());
        assert!(GeoService.initialize(json!({"skin": "yahoo"}), &ctx()).is_err());
        assert!(GeoService.initialize(Value::Null, &ctx()).is_ok());
    }
    #[test]
    fn pages_render_and_do_not_mutate() {
        let mut state = init(maps_seed());
        for url in [
            "http://maps.google.com/",
            "http://maps.google.com/maps",
            "http://maps.google.com/maps/place/northstar-hq",
            "http://maps.google.com/maps/saved",
            "http://maps.google.com/maps/notes",
            "http://maps.google.com/search?q=convention",
        ] {
            let before = state.clone();
            let page = get(&mut state, url);
            assert_eq!(page.status, 200, "{url}");
            let dom = Dom::of(&page);
            // The chrome is on every page: the search form is a GET of `q`, the nav is real.
            assert_eq!(dom.tag("hdr-search"), "form");
            assert_eq!(dom.attr("hdr-search", "action"), "/search");
            assert_eq!(dom.attr("hdr-search", "method"), "get");
            assert_eq!(dom.attr("hdr-q", "name"), "q");
            assert_eq!(dom.tag("hdr-search-go"), "button");
            assert_eq!(dom.attr("nav-home", "href"), "/");
            assert_eq!(dom.attr("nav-saved", "href"), "/maps/saved");
            assert_eq!(dom.attr("nav-notes", "href"), "/maps/notes");
            assert_eq!(dom.text("wordmark"), "Testmaps");
            assert!(dom.has("chrome") && dom.has("foot"), "{url}");
            assert_eq!(page, get(&mut state, url), "{url} must be pure");
            assert_eq!(before, state, "{url} must not mutate state");
        }
        let results = Dom::of(&get(&mut state, "http://maps.google.com/search?q=convention"));
        assert_eq!(results.attr("hdr-q", "value"), "convention");
        assert_eq!(results.tag("r-0"), "a");
        assert_eq!(results.attr("r-0", "href"), "/maps/place/devcon-center");
        assert_eq!(results.text("r-0-name"), "Cascade Convention Center");
        assert_eq!(results.text("r-0-addr"), "800 Pike St, Seattle WA");
        assert_eq!(results.text("r-0-kind"), "venue");
        assert_eq!(results.text("r-0-rating"), "4.3 ★");
        assert!(results.has("r-0-tile") && !results.has("r-1"));
        assert_eq!(Dom::of(&get(&mut state, "http://maps.google.com/search?q=zzz")).text("empty"), "No places matched.");
        // The home page: the directions form is a GET of from, to and mode; cards are links.
        let home = Dom::of(&get(&mut state, "http://maps.google.com/"));
        assert_eq!(home.text("title"), "Testmaps");
        assert_eq!(home.attr("dir-form", "action"), "/maps/dir");
        assert_eq!(home.attr("dir-form", "method"), "get");
        assert_eq!(
            home.fields("dir-form"),
            [("from", ""), ("to", ""), ("mode", "driving")].map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
        for id in ["dir-from", "dir-to", "dir-mode"] {
            assert_eq!(home.tag(id), "input", "{id}");
        }
        assert_eq!(home.text("dir-form-go"), "Directions");
        assert_eq!(home.attr("home-place-0", "href"), "/maps/place/devcon-center");
        assert!(home.has("saved-empty") && home.has("feat-title") && home.has("feat-grid"));
        // The place page: facts, the save form, directions from here, and the note form.
        let place = Dom::of(&get(&mut state, "http://maps.google.com/maps/place/northstar-hq"));
        assert_eq!(place.text("place-name"), "Northstar HQ");
        assert_eq!(place.text("place-rating"), "4.6 ★ · 128 reviews");
        assert_eq!(place.text("place-kind"), "office");
        assert_eq!(place.text("place-tag-0"), "office");
        assert_eq!(place.text("place-addr"), "410 Bayfront Ave, Seattle WA");
        assert_eq!(place.text("place-coord"), "47.572600, -122.348000");
        assert_eq!(place.text("place-hours"), "Hours: Mon-Fri 8:00-18:00");
        assert_eq!(place.text("place-phone"), "Phone: +1 206 555 0148");
        assert_eq!(place.attr("place-site", "href"), "http://northstar.example/");
        assert_eq!(place.text("place-summary"), "Bayfront campus.");
        assert_eq!(place.attr("save-form", "action"), "/api/places/northstar-hq/save");
        assert_eq!(place.attr("save-form", "method"), "post");
        assert_eq!(place.text("save-form-go"), "Save to Your places");
        assert_eq!(place.attr("place-dir", "action"), "/maps/dir");
        assert_eq!(
            place.fields("place-dir"),
            [("from", "northstar-hq"), ("to", ""), ("mode", "driving")].map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
        assert_eq!(place.text("place-dir-go"), "Get directions");
        assert_eq!(place.attr("note-form", "action"), "/api/notes");
        assert_eq!(place.attr("note-form", "method"), "post");
        assert_eq!(
            place.fields("note-form"),
            [("place", "northstar-hq"), ("text", "")].map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
        assert_eq!(place.text("note-form-go"), "Add a note");
        assert_eq!(place.text("note-empty"), "No notes on this place.");
        assert_eq!(
            get(&mut state, "http://maps.google.com/maps/place/nope").status,
            404
        );
    }
    #[test]
    fn pages_carry_a_served_map_and_the_map_endpoint_draws_it() {
        let mut state = init(maps_seed());
        let home = Dom::of(&get(&mut state, "http://maps.google.com/"));
        assert_eq!(home.tag("home-tile"), "img");
        assert_eq!(home.attr("home-tile", "src"), "/map.rgba?w=768&h=480&center=devcon-center&zoom=0");
        assert_eq!(home.attr("home-tile", "alt"), "Map of every place · pick one below");
        let place = Dom::of(&get(
            &mut state,
            "http://maps.google.com/maps/place/northstar-hq",
        ));
        assert_eq!(
            place.attr("place-tile", "src"),
            "/map.rgba?w=768&h=480&center=northstar-hq&zoom=3&sel=northstar-hq"
        );
        let dir = Dom::of(&get(
            &mut state,
            "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=driving",
        ));
        assert_eq!(
            dir.attr("dir-tile", "src"),
            "/map.rgba?w=768&h=480&route=northstar-hq%7Cdevcon-center%7Cdriving&sel=devcon-center"
        );
        assert_eq!(dir.attr("dir-tile", "alt"), "Northstar HQ → Cascade Convention Center");
        // OpenStreetMap's pane beside the sidebar is 4:3.
        let mut osm_seed = maps_seed();
        osm_seed["skin"] = json!("osm");
        let mut osm = init(osm_seed);
        assert_eq!(Dom::of(&get(&mut osm, "http://openstreetmap.org/")).attr("home-tile", "src"), "/map.rgba?w=640&h=480&center=devcon-center&zoom=0");
        let url = "http://maps.google.com/map.rgba?w=96&h=64&route=northstar-hq%7Cdevcon-center%7Cdriving&sel=devcon-center";
        let before = state.clone();
        let picture = get(&mut state, url);
        assert_eq!(picture.status, 200);
        assert_eq!(
            picture.headers.get("content-type").map(String::as_str),
            Some(cw_protocol::RGBA_MEDIA_TYPE)
        );
        let asset: Value = serde_json::from_slice(&picture.body).unwrap();
        assert_eq!(asset["width"], 96);
        assert_eq!(asset["height"], 64);
        assert_eq!(asset["rgba"].as_array().unwrap().len(), 96 * 64 * 4);
        assert_eq!(picture, get(&mut state, url), "the map must be pure");
        assert_eq!(before, state);
        // A place-centred map is a different picture, and sizes are clamped, never refused.
        let centred = get(
            &mut state,
            "http://maps.google.com/map.rgba?w=96&h=64&center=northstar-hq&zoom=4",
        );
        assert_ne!(centred.body, picture.body);
        let huge: Value = serde_json::from_slice(
            &get(&mut state, "http://maps.google.com/map.rgba?w=5000&h=5000").body,
        )
        .unwrap();
        assert_eq!(
            (huge["width"].as_u64(), huge["height"].as_u64()),
            (Some(960), Some(480))
        );
        let mut weather = init(weather_seed());
        assert_eq!(get(&mut weather, "http://weather.com/map.rgba").status, 404);
    }
    #[test]
    fn directions_are_integer_only_and_cached_on_first_lookup() {
        let mut state = init(maps_seed());
        let url = "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=driving";
        let page = get(&mut state, url);
        assert_eq!(page.status, 200);
        // 38 800 µdeg north + 15 000 µdeg east -> 5 428 m, 400 m/min -> 14 min. Storyline 4.
        let s: GeoState = web::load(&state).unwrap();
        let route = &s.routes["northstar-hq|devcon-center|driving"];
        assert_eq!(route.metres, 5_428);
        assert_eq!(route.minutes, 14);
        let dom = Dom::of(&page);
        assert_eq!(dom.text("dir-title"), "Northstar HQ to Cascade Convention Center");
        assert_eq!(dom.text("dir-min"), "14 min");
        assert_eq!(dom.text("dir-dist"), "3.4 mi");
        assert_eq!(dom.text("dir-mode"), "driving");
        assert_eq!(dom.text("step-0-n"), "1");
        assert_eq!(dom.text("step-1-text"), "Head north on Bayfront Ave — 2.7 mi");
        assert_eq!(dom.text("step-2-text"), "Turn east onto Pike St — 0.7 mi");
        assert!(dom.has("step-3") && !dom.has("step-4"));
        for (mode, label) in [("driving", "Drive"), ("transit", "Transit"), ("cycling", "Bike"), ("walking", "Walk")] {
            let id = format!("mode-{mode}");
            assert_eq!(dom.text(&id), label);
            assert_eq!(
                dom.attr(&id, "href"),
                format!("/maps/dir?from=northstar-hq&to=devcon-center&mode={mode}")
            );
        }
        assert_eq!(dom.attr("dir-to-place", "href"), "/maps/place/devcon-center");
        assert_eq!(dom.text("dir-to-place"), "Open Cascade Convention Center");
        // A cache hit is the identical value, so the second page is byte-identical.
        let cached = state.clone();
        assert_eq!(page, get(&mut state, url));
        assert_eq!(cached, state);
        // Walking shares the distance and only the speed differs: 5 428 / 80 -> 68 min.
        get(
            &mut state,
            "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=walking",
        );
        let s: GeoState = web::load(&state).unwrap();
        assert_eq!(s.routes["northstar-hq|devcon-center|walking"].minutes, 68);
        // The determinism requirement, asserted rather than assumed: no float reaches state.
        assert!(
            !has_float(&state),
            "state must hold no floating-point number"
        );
    }
    #[test]
    fn directions_are_refused_when_they_should_be() {
        let mut state = init(maps_seed());
        for url in [
            "http://maps.google.com/maps/dir?from=northstar-hq&to=nope&mode=driving",
            "http://maps.google.com/maps/dir?from=northstar-hq&to=northstar-hq&mode=driving",
            "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=teleport",
        ] {
            assert_eq!(get(&mut state, url).status, 400, "{url}");
        }
        let s: GeoState = web::load(&state).unwrap();
        assert!(
            s.routes.is_empty(),
            "a refused lookup must not cache anything"
        );
    }
    #[test]
    fn saving_a_place_and_adding_a_note_survive_a_round_trip() {
        let mut state = init(maps_seed());
        post(
            &mut state,
            "http://maps.google.com/api/places/devcon-center/save",
            &[],
        );
        let s: GeoState = web::load(&state).unwrap();
        assert_eq!(s.saved["alice"], vec!["devcon-center".to_string()]);
        let saved = Dom::of(&get(&mut state, "http://maps.google.com/maps/saved"));
        assert_eq!(saved.text("saved-0-name"), "Cascade Convention Center");
        assert_eq!(saved.attr("saved-0", "href"), "/maps/place/devcon-center");
        let home = Dom::of(&get(&mut state, "http://maps.google.com/"));
        assert_eq!(home.text("saved-title"), "Your places");
        assert_eq!(home.attr("home-saved-0", "href"), "/maps/place/devcon-center");
        assert_eq!(home.attr("dir-from", "value"), "devcon-center");
        let place = Dom::of(&get(&mut state, "http://maps.google.com/maps/place/devcon-center"));
        assert_eq!(place.text("save-form-go"), "Remove from Your places");
        // Saving again is the "unsave" every star really performs.
        post(
            &mut state,
            "http://maps.google.com/api/places/devcon-center/save",
            &[],
        );
        let s: GeoState = web::load(&state).unwrap();
        assert!(!s.saved.contains_key("alice"));
        let page = post(
            &mut state,
            "http://maps.google.com/api/notes",
            &[
                ("place", "devcon-center"),
                ("text", "Loading dock entrance is on 9th."),
            ],
        );
        let dom = Dom::of(&page);
        assert_eq!(dom.text("place-name"), "Cascade Convention Center");
        assert_eq!(dom.text("note-0-text"), "Loading dock entrance is on 9th.");
        assert_eq!(dom.text("note-0-who"), "alice · tick 7");
        let notes = Dom::of(&get(&mut state, "http://maps.google.com/maps/notes"));
        assert_eq!(notes.attr("n-0", "href"), "/maps/place/devcon-center");
        assert_eq!(notes.text("n-0-where"), "Cascade Convention Center");
        assert_eq!(notes.text("n-0-text"), "Loading dock entrance is on 9th.");
        assert_eq!(notes.text("n-0-meta"), "alice · 47.611400, -122.333000 · tick 7");
        let s: GeoState = web::load(&state).unwrap();
        assert_eq!(s.notes.len(), 1);
        assert_eq!(s.notes[0].author, "alice");
        assert_eq!(s.notes[0].lat, 47_611_400);
        let round_tripped: GeoState =
            serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(s, round_tripped);
        assert_eq!(
            post(
                &mut state,
                "http://maps.google.com/api/notes",
                &[("place", "devcon-center"), ("text", "  ")]
            )
            .status,
            400
        );
        assert_eq!(
            post(
                &mut state,
                "http://maps.google.com/api/places/nope/save",
                &[]
            )
            .status,
            400
        );
    }
    #[test]
    fn weather_reads_a_forecast_switches_units_and_acks_an_alert() {
        let mut state = init(weather_seed());
        let home = Dom::of(&get(&mut state, "http://weather.com/"));
        assert_eq!(home.text("title"), "Seattle, WA");
        assert_eq!(home.text("wordmark"), "Testweather");
        assert_eq!(home.text("now-temp"), "57°");
        assert_eq!(home.text("now-meta"), "Rain · 84% humidity · 9 mph wind");
        assert_eq!(home.attr("now-icon", "aria-label"), "Rain");
        assert_eq!(home.text("d-0-hi"), "61°");
        assert_eq!(home.text("d-0-lo"), "48°");
        assert_eq!(home.text("d-0-precip"), "80% rain");
        assert_eq!(home.text("days-title"), "Daily Forecast");
        assert_eq!(home.text("alert-0-sev"), "Advisory");
        assert_eq!(home.text("alert-0-title"), "Wind advisory");
        assert_eq!(home.text("alert-0-body"), "Gusts to 40 mph.");
        assert_eq!(home.attr("alert-0-ack", "action"), "/api/alerts/wx-1/ack");
        assert_eq!(home.attr("alert-0-ack", "method"), "post");
        assert_eq!(home.text("alert-0-ack-go"), "Got it");
        assert_eq!(home.attr("nav-today", "href"), "/weather/today/l/seattle");
        assert_eq!(home.attr("nav-tenday", "href"), "/weather/tenday/l/seattle");
        assert_eq!(home.attr("nav-saved", "href"), "/maps/saved");
        assert_eq!(home.attr("wx-today", "href"), "/weather/today/l/seattle");
        assert_eq!(home.attr("wx-ten", "href"), "/weather/tenday/l/seattle");
        assert_eq!(home.attr("locations-form", "action"), "/api/locations");
        assert_eq!(home.attr("locations-form", "method"), "post");
        assert_eq!(home.fields("locations-form"), [("city".to_owned(), "seattle".to_owned())]);
        assert_eq!(home.text("locations-form-go"), "Save this location");
        assert_eq!(home.attr("units-form", "action"), "/api/units");
        assert_eq!(home.attr("units-form", "method"), "post");
        assert_eq!(home.fields("units-form"), [("units".to_owned(), "f".to_owned())]);
        assert_eq!(home.text("units-form-go"), "Switch units");
        assert!(!home.has("wx-place"), "the test seed has no places");
        assert_eq!(home.attr("hdr-search", "action"), "/search");
        let ten = Dom::of(&get(&mut state, "http://weather.com/weather/tenday/l/seattle"));
        assert_eq!(ten.text("days-title"), "10 day forecast");
        assert_eq!(ten.text("d-1-day"), "Tue");
        assert_eq!(ten.text("d-1-cond"), "Cloudy");
        let found = Dom::of(&get(&mut state, "http://weather.com/search?q=seattle"));
        assert_eq!(found.attr("r-0", "href"), "/weather/today/l/seattle");
        assert_eq!(found.text("r-0-city"), "Seattle, WA");
        assert_eq!(found.text("r-0-now"), "57° · Rain");
        assert_eq!(Dom::of(&get(&mut state, "http://weather.com/search?q=zzz")).text("empty"), "No locations matched.");
        assert_eq!(Dom::of(&get(&mut state, "http://weather.com/maps/saved")).text("empty"), "Nothing saved yet.");
        assert_eq!(
            get(&mut state, "http://weather.com/weather/today/l/nope").status,
            404
        );
        // (61 - 32) * 5 / 9 == 16, in integers, on every machine.
        post(
            &mut state,
            "http://weather.com/api/units",
            &[("units", "c")],
        );
        let s: GeoState = web::load(&state).unwrap();
        assert_eq!(s.unit_prefs["alice"], "metric");
        let metric = Dom::of(&get(&mut state, "http://weather.com/weather/today/l/seattle"));
        assert_eq!(metric.text("d-0-hi"), "16°");
        assert_eq!(metric.attr("units-value", "value"), "c");
        assert_eq!(
            post(
                &mut state,
                "http://weather.com/api/units",
                &[("units", "kelvin")]
            )
            .status,
            400
        );
        post(
            &mut state,
            "http://weather.com/api/locations",
            &[("city", "seattle")],
        );
        let s: GeoState = web::load(&state).unwrap();
        assert_eq!(s.saved["alice"], vec!["seattle".to_string()]);
        let saved = Dom::of(&get(&mut state, "http://weather.com/maps/saved"));
        assert_eq!(saved.attr("saved-0", "href"), "/weather/today/l/seattle");
        assert_eq!(saved.text("saved-0-city"), "Seattle, WA");
        assert_eq!(
            Dom::of(&get(&mut state, "http://weather.com/weather/today/l/seattle")).text("locations-form-go"),
            "Remove this location"
        );
        post(&mut state, "http://weather.com/api/alerts/wx-1/ack", &[]);
        let s: GeoState = web::load(&state).unwrap();
        assert_eq!(s.alerts[0].acked, vec!["alice".to_string()]);
        assert!(!Dom::of(&get(&mut state, "http://weather.com/weather/today/l/seattle")).has("alert-0"));
        assert_eq!(
            post(&mut state, "http://weather.com/api/alerts/nope/ack", &[]).status,
            400
        );
    }
    /// Every route of every skin, through the strict validator: an unknown property, a
    /// repeated id or a selector the engine cannot match fails here rather than in a still.
    #[test]
    fn every_page_of_every_skin_passes_the_strict_validator() {
        for skin in ["", "gmaps", "osm"] {
            let mut seed = maps_seed();
            seed["skin"] = json!(skin);
            let mut state = init(seed);
            post(
                &mut state,
                "http://maps.google.com/api/places/devcon-center/save",
                &[],
            );
            post(
                &mut state,
                "http://maps.google.com/api/notes",
                &[("place", "northstar-hq"), ("text", "Side door is locked.")],
            );
            let want = if skin.is_empty() { "gmaps" } else { skin };
            for url in [
                "http://maps.google.com/",
                "http://maps.google.com/maps",
                "http://maps.google.com/maps/saved",
                "http://maps.google.com/maps/notes",
                "http://maps.google.com/maps/place/northstar-hq",
                "http://maps.google.com/maps/place/devcon-center",
                "http://maps.google.com/search?q=convention",
                "http://maps.google.com/search?q=zzz",
                "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=driving",
                "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=walking",
            ] {
                let dom = Dom::of(&get(&mut state, url));
                assert!(dom.body_class().starts_with(&format!("skin-{want} ")), "{url}");
                assert!(dom.has("chrome") && dom.has("hdr-search") && dom.has("foot"), "{url}");
            }
        }
        let mut seed = weather_seed();
        seed["skin"] = json!("weather");
        let mut state = init(seed);
        post(&mut state, "http://weather.com/api/locations", &[("city", "seattle")]);
        for url in [
            "http://weather.com/",
            "http://weather.com/weather/today/l/seattle",
            "http://weather.com/weather/tenday/l/seattle",
            "http://weather.com/search?q=seattle",
            "http://weather.com/search?q=zzz",
            "http://weather.com/maps/saved",
            "http://weather.com/maps/notes",
        ] {
            let dom = Dom::of(&get(&mut state, url));
            assert!(dom.body_class().starts_with("skin-weather "), "{url}");
            assert!(dom.has("chrome") && dom.has("hdr-search") && dom.has("foot"), "{url}");
        }
        // A weather instance with nothing seeded is still a page, not a blank.
        let mut bare = init(json!({"mode": "weather", "brand": "Testweather"}));
        assert_eq!(Dom::of(&get(&mut bare, "http://weather.com/")).text("empty"), "No locations are seeded.");
    }
    #[test]
    fn maps_routes_are_absent_in_weather_mode_and_the_reverse() {
        let mut state = init(weather_seed());
        assert_eq!(get(&mut state, "http://weather.com/maps").status, 404);
        let mut state = init(maps_seed());
        assert_eq!(
            get(&mut state, "http://maps.google.com/weather/today/l/seattle").status,
            404
        );
    }
    #[test]
    fn non_get_post_methods_are_rejected() {
        let mut state = init(maps_seed());
        let mut r = HttpRequest::get("http://maps.google.com/api/places/devcon-center/save");
        r.method = "DELETE".into();
        assert_eq!(
            GeoService.handle(&mut state, &ctx(), &r).unwrap().status,
            405
        );
    }
}
