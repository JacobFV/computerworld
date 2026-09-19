//! Places: maps.google.com and openstreetmap.org (`mode: maps`), weather.com (`mode: weather`).
//! One `places` dataset backs both, which is why they are one crate.
//!
//! Every coordinate is an integer micro-degree (47.606200 is `47_606_200`) and every distance,
//! duration and temperature is derived from those integers alone. No float ever enters state, so
//! a route computed on one replay is byte-identical to the same route on the next.
use cw_protocol::{HttpRequest, HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as web;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
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
const TRAVEL: &[(&str, i64, &str)] = &[
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
fn coord(v: i64) -> String {
    format!(
        "{}{}.{:06}",
        if v < 0 { "-" } else { "" },
        v.abs() / 1_000_000,
        v.abs() % 1_000_000
    )
}
fn stars(tenths: i64) -> String {
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
fn distance_label(metres: i64, imperial: bool) -> String {
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
fn duration_label(minutes: i64) -> String {
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
fn temp(f: i64, imperial: bool) -> String {
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
fn encode(value: &str) -> String {
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
    fn saved_of(&self, actor: &str) -> Vec<String> {
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
    fn imperial(&self, actor: &str) -> bool {
        self.units_for(actor) != "metric"
    }
    /// The city a bare `/` shows: the person's first saved location, else the first seeded one.
    fn default_city(&self, actor: &str) -> String {
        self.saved_of(actor)
            .into_iter()
            .find(|c| self.forecasts.contains_key(c))
            .or_else(|| self.forecasts.keys().next().cloned())
            .unwrap_or_default()
    }
    fn alerts_for(&self, city: &str, actor: &str) -> Vec<&Alert> {
        self.alerts
            .iter()
            .filter(|a| (city.is_empty() || a.city == city) && !a.acked.iter().any(|p| p == actor))
            .collect()
    }
    fn notes_at(&self, place: &str) -> Vec<&Note> {
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
/// Resolved palette: the seed theme with renderer-neutral fallbacks, so a partial theme still
/// produces a page that reads correctly.
struct Palette {
    accent: String,
    ink: String,
    muted: String,
    surface: String,
}
fn palette(t: &PageTheme) -> Palette {
    Palette {
        accent: t.accent.clone().unwrap_or_else(|| "#1a73e8".into()),
        ink: t.ink.clone().unwrap_or_else(|| "#202124".into()),
        muted: t.muted.clone().unwrap_or_else(|| "#5f6368".into()),
        surface: t.surface.clone().unwrap_or_else(|| "#e8eaed".into()),
    }
}
fn act(method: &str, url: &str, fields: &[(&str, &str)]) -> PageAction {
    PageAction {
        method: method.into(),
        url: url.into(),
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}
fn input(id: &str, label: &str, value: &str) -> PageElement {
    PageElement::Input {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        placeholder: String::new(),
    }
}
/// A form whose drawn controls are exactly the fields it submits.
fn form_el(
    id: &str,
    action: PageAction,
    mut children: Vec<PageElement>,
    submit: &str,
) -> PageElement {
    children.push(PageElement::Button {
        id: format!("{id}-go"),
        text: submit.into(),
        action: action.clone(),
        style: None,
    });
    PageElement::Form {
        id: id.into(),
        action,
        children,
    }
}
fn chip(id: &str, text: impl Into<String>, p: &Palette) -> PageElement {
    web::badge(
        id,
        text,
        web::style()
            .size(12)
            .medium()
            .color(p.accent.clone())
            .background(p.surface.clone())
            .radius(12)
            .padding(6),
    )
}
/// Widest and tallest map a page may ask for.
const MAP_LIMIT: (u32, u32) = (960, 480);
/// Micro-degrees of latitude a map centred on a place spans at zoom 0; each zoom level halves it.
const MAP_SPAN_AT_ZOOM_0: i64 = 64_000;
const MAP_ZOOM_DEFAULT: u32 = 3;
/// The map on a page: a picture this site serves from `GET /map.rgba`, drawn by `cw_map` from
/// the places alone, so the same streets appear here and in the native Maps app.
fn map_image(id: &str, alt: &str, width: u32, height: u32, query: &str) -> PageElement {
    web::image(
        id,
        format!("/map.rgba?w={width}&h={height}{query}"),
        alt,
        width,
        height,
    )
}
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
/// A flat stand-in labelled with what it stands for: a weather icon, or a place card's corner.
fn map_tile(id: &str, label: &str, height: u32, p: &Palette) -> PageElement {
    web::thumbnail(
        id,
        label,
        web::style()
            .height(height)
            .background(p.surface.clone())
            .color(p.muted.clone())
            .border(p.accent.clone())
            .radius(8)
            .align("center"),
    )
}
/// Brand bar, the one search box and the account routes — on every page, so the nav is real.
fn chrome(s: &GeoState, p: &Palette, actor: &str, query: &str) -> Vec<PageElement> {
    let search = form_el(
        "hdr-search",
        act("GET", "/search", &[("q", "$hdr-q")]),
        vec![input(
            "hdr-q",
            if s.maps() {
                "Search places"
            } else {
                "Search a city"
            },
            query,
        )],
        "Search",
    );
    let mut nav = vec![web::link("nav-home", "Home", "/")];
    if s.maps() {
        nav.push(web::link("nav-saved", "Your places", "/maps/saved"));
        nav.push(web::link("nav-notes", "Notes", "/maps/notes"));
    } else {
        let city = s.default_city(actor);
        nav.push(web::link(
            "nav-today",
            "Today",
            format!("/weather/today/l/{city}"),
        ));
        nav.push(web::link(
            "nav-tenday",
            "10 day",
            format!("/weather/tenday/l/{city}"),
        ));
        nav.push(web::link("nav-saved", "Saved places", "/maps/saved"));
    }
    vec![
        web::styled_row(
            "chrome",
            16,
            "center",
            web::style().background(p.ink.clone()).padding(12).radius(8),
            std::iter::once(web::styled(
                "wordmark",
                &s.brand,
                web::style().size(22).bold().color("#ffffff").width(190),
            ))
            .chain(std::iter::once(search))
            .chain(nav)
            .collect(),
        ),
        web::spacer("chrome-gap", 12),
    ]
}
fn footer(id: &str, p: &Palette) -> Vec<PageElement> {
    vec![
        web::spacer(&format!("{id}-gap"), 16),
        web::divider(&format!("{id}-rule")),
        web::styled(
            id,
            "Simulated geography in a training world. Every address, coordinate and forecast here \
             is invented, and no real mapping or weather service is contacted.",
            web::style().size(12).color(p.muted.clone()),
        ),
    ]
}
fn place_card(id: &str, place: &Place, p: &Palette) -> PageElement {
    let mut body = vec![
        map_tile(
            &format!("{id}-tile"),
            &format!("{} · map", place.name),
            88,
            p,
        ),
        web::styled(
            &format!("{id}-name"),
            &place.name,
            web::style().size(16).bold().color(p.ink.clone()),
        ),
        web::styled(
            &format!("{id}-addr"),
            &place.address,
            web::style().size(13).color(p.muted.clone()),
        ),
    ];
    let mut meta = vec![chip(&format!("{id}-kind"), &place.kind, p)];
    if place.rating > 0 {
        meta.push(web::badge(
            &format!("{id}-rating"),
            format!("{} ★", stars(place.rating)),
            web::style()
                .size(12)
                .bold()
                .color("#ffffff")
                .background(p.accent.clone())
                .radius(10)
                .padding(6),
        ));
    }
    body.push(web::row(&format!("{id}-meta"), 8, "center", meta));
    web::card_action(
        id,
        web::style()
            .background("#ffffff")
            .border(p.surface.clone())
            .radius(10)
            .padding(12),
        web::visit(format!("/maps/place/{}", place.id)),
        body,
    )
}
fn maps_home(s: &GeoState, p: &Palette, actor: &str) -> Result<HttpResponse> {
    let saved = s.saved_of(actor);
    let featured: Vec<PageElement> = s
        .places
        .values()
        .take(6)
        .enumerate()
        .map(|(i, place)| place_card(&format!("home-place-{i}"), place, p))
        .collect();
    let saved_cards: Vec<PageElement> = saved
        .iter()
        .filter_map(|id| s.places.get(id))
        .enumerate()
        .map(|(i, place)| place_card(&format!("home-saved-{i}"), place, p))
        .collect();
    let mut els = chrome(s, p, actor, "");
    els.push(web::heading("title", &s.brand));
    if !s.tagline.is_empty() {
        els.push(web::styled(
            "tagline",
            &s.tagline,
            web::style().size(14).color(p.muted.clone()),
        ));
    }
    els.push(map_image(
        "home-tile",
        "Map of every place · pick one below",
        720,
        280,
        "",
    ));
    els.push(web::spacer("home-gap", 16));
    els.push(form_el(
        "dir-form",
        act(
            "GET",
            "/maps/dir",
            &[
                ("from", "$dir-from"),
                ("to", "$dir-to"),
                ("mode", "$dir-mode"),
            ],
        ),
        vec![
            input(
                "dir-from",
                "From (place id)",
                saved.first().map_or("", String::as_str),
            ),
            input("dir-to", "To (place id)", ""),
            input(
                "dir-mode",
                "Mode (driving, transit, cycling, walking)",
                "driving",
            ),
        ],
        "Directions",
    ));
    els.push(web::spacer("dir-gap", 16));
    if saved_cards.is_empty() {
        els.push(web::styled(
            "saved-empty",
            "Your places is empty. Open a place and save it.",
            web::style().size(13).color(p.muted.clone()),
        ));
    } else {
        els.push(web::heading("saved-title", "Your places"));
        els.push(web::grid("saved-grid", 3, 12, saved_cards));
    }
    els.push(web::spacer("feat-gap", 16));
    els.push(web::heading("feat-title", "Nearby"));
    els.push(web::grid("feat-grid", 3, 12, featured));
    els.extend(footer("foot", p));
    web::themed_page(&s.brand, s.theme.clone(), els)
}
fn saved_page(s: &GeoState, p: &Palette, actor: &str) -> Result<HttpResponse> {
    let mut els = chrome(s, p, actor, "");
    els.push(web::heading("title", "Your places"));
    let ids = s.saved_of(actor);
    if ids.is_empty() {
        els.push(web::styled(
            "empty",
            "Nothing saved yet.",
            web::style().size(14).color(p.muted.clone()),
        ));
    } else if s.maps() {
        let cards: Vec<PageElement> = ids
            .iter()
            .filter_map(|id| s.places.get(id))
            .enumerate()
            .map(|(i, place)| place_card(&format!("saved-{i}"), place, p))
            .collect();
        els.push(web::grid("saved-grid", 3, 12, cards));
    } else {
        for (i, id) in ids.iter().enumerate() {
            let Some(f) = s.forecasts.get(id) else {
                continue;
            };
            els.push(web::card_action(
                &format!("saved-{i}"),
                web::style()
                    .background("#ffffff")
                    .border(p.surface.clone())
                    .radius(10)
                    .padding(12),
                web::visit(format!("/weather/today/l/{}", f.id)),
                vec![
                    web::styled(
                        &format!("saved-{i}-city"),
                        &f.city,
                        web::style().size(16).bold().color(p.ink.clone()),
                    ),
                    web::styled(
                        &format!("saved-{i}-now"),
                        format!("{} · {}", temp(f.now_f, s.imperial(actor)), f.cond),
                        web::style().size(13).color(p.muted.clone()),
                    ),
                ],
            ));
        }
    }
    els.extend(footer("foot", p));
    web::themed_page("Your places", s.theme.clone(), els)
}
fn place_page(s: &GeoState, p: &Palette, actor: &str, id: &str) -> Result<HttpResponse> {
    let place = match s.place(id) {
        Ok(v) => v,
        Err(e) => return web::error(404, e),
    };
    let saved = s.saved_of(actor).iter().any(|x| x == id);
    let mut els = chrome(s, p, actor, "");
    els.push(map_image(
        "place-tile",
        &format!(
            "{} · {}, {}",
            place.name,
            coord(place.lat),
            coord(place.lon)
        ),
        720,
        240,
        &format!(
            "&center={id}&zoom={MAP_ZOOM_DEFAULT}&sel={id}",
            id = encode(&place.id)
        ),
    ));
    els.push(web::spacer("place-gap", 12));
    els.push(web::heading("place-name", &place.name));
    let mut meta = vec![chip("place-kind", &place.kind, p)];
    if place.rating > 0 {
        meta.push(web::badge(
            "place-rating",
            format!("{} ★ · {} reviews", stars(place.rating), place.reviews),
            web::style()
                .size(12)
                .bold()
                .color("#ffffff")
                .background(p.accent.clone())
                .radius(10)
                .padding(6),
        ));
    }
    for (i, tag) in place.tags.iter().enumerate() {
        meta.push(chip(&format!("place-tag-{i}"), tag, p));
    }
    els.push(web::row("place-meta", 8, "center", meta));
    els.push(web::spacer("meta-gap", 12));
    let mut facts = vec![
        web::styled(
            "place-addr",
            &place.address,
            web::style().size(14).color(p.ink.clone()),
        ),
        web::styled(
            "place-coord",
            format!("{}, {}", coord(place.lat), coord(place.lon)),
            web::style().size(13).color(p.muted.clone()),
        ),
    ];
    if !place.hours.is_empty() {
        facts.push(web::styled(
            "place-hours",
            format!("Hours: {}", place.hours),
            web::style().size(13).color(p.ink.clone()),
        ));
    }
    if !place.phone.is_empty() {
        facts.push(web::styled(
            "place-phone",
            format!("Phone: {}", place.phone),
            web::style().size(13).color(p.ink.clone()),
        ));
    }
    if !place.website.is_empty() {
        facts.push(web::link("place-site", &place.website, &place.website));
    }
    if !place.summary.is_empty() {
        facts.push(web::styled(
            "place-summary",
            &place.summary,
            web::style().size(14).color(p.ink.clone()),
        ));
    }
    els.push(web::card(
        "place-card",
        web::style()
            .background("#ffffff")
            .border(p.surface.clone())
            .radius(10)
            .padding(16),
        facts,
    ));
    els.push(web::spacer("facts-gap", 12));
    els.push(form_el(
        "save-form",
        act("POST", &format!("/api/places/{id}/save"), &[]),
        vec![],
        if saved {
            "Remove from Your places"
        } else {
            "Save to Your places"
        },
    ));
    els.push(web::spacer("save-gap", 12));
    els.push(web::heading("dir-title", "Directions from here"));
    els.push(form_el(
        "place-dir",
        act(
            "GET",
            "/maps/dir",
            &[
                ("from", "$place-dir-from"),
                ("to", "$place-dir-to"),
                ("mode", "$place-dir-mode"),
            ],
        ),
        vec![
            input("place-dir-from", "From (place id)", id),
            input("place-dir-to", "To (place id)", ""),
            input("place-dir-mode", "Mode", "driving"),
        ],
        "Get directions",
    ));
    els.push(web::spacer("note-gap", 16));
    els.push(web::heading("note-title", "Notes"));
    let notes = s.notes_at(id);
    if notes.is_empty() {
        els.push(web::styled(
            "note-empty",
            "No notes on this place.",
            web::style().size(13).color(p.muted.clone()),
        ));
    }
    for (i, n) in notes.iter().enumerate() {
        els.push(web::card(
            &format!("note-{i}"),
            web::style()
                .background(p.surface.clone())
                .radius(8)
                .padding(10),
            vec![
                web::styled(
                    &format!("note-{i}-who"),
                    format!("{} · tick {}", n.author, n.tick),
                    web::style().size(12).color(p.muted.clone()),
                ),
                web::styled(
                    &format!("note-{i}-text"),
                    &n.text,
                    web::style().size(14).color(p.ink.clone()),
                ),
            ],
        ));
    }
    els.push(form_el(
        "note-form",
        act(
            "POST",
            "/api/notes",
            &[("place", "$note-place"), ("text", "$note-text")],
        ),
        vec![
            input("note-place", "Place", id),
            input("note-text", "What is wrong here?", ""),
        ],
        "Add a note",
    ));
    els.extend(footer("foot", p));
    web::themed_page(&place.name, s.theme.clone(), els)
}
fn notes_page(s: &GeoState, p: &Palette, actor: &str) -> Result<HttpResponse> {
    let mut els = chrome(s, p, actor, "");
    els.push(web::heading("title", "Map notes"));
    if s.notes.is_empty() {
        els.push(web::styled(
            "empty",
            "No notes yet.",
            web::style().size(14).color(p.muted.clone()),
        ));
    }
    for (i, n) in s.notes.iter().enumerate() {
        let name = s
            .places
            .get(&n.place)
            .map_or(n.place.clone(), |x| x.name.clone());
        els.push(web::card_action(
            &format!("n-{i}"),
            web::style()
                .background("#ffffff")
                .border(p.surface.clone())
                .radius(10)
                .padding(12),
            web::visit(format!("/maps/place/{}", n.place)),
            vec![
                web::styled(
                    &format!("n-{i}-where"),
                    name,
                    web::style().size(15).bold().color(p.ink.clone()),
                ),
                web::styled(
                    &format!("n-{i}-text"),
                    &n.text,
                    web::style().size(14).color(p.ink.clone()),
                ),
                web::styled(
                    &format!("n-{i}-meta"),
                    format!(
                        "{} · {}, {} · tick {}",
                        n.author,
                        coord(n.lat),
                        coord(n.lon),
                        n.tick
                    ),
                    web::style().size(12).color(p.muted.clone()),
                ),
            ],
        ));
    }
    els.extend(footer("foot", p));
    web::themed_page("Map notes", s.theme.clone(), els)
}
fn directions_page(s: &GeoState, p: &Palette, actor: &str, route: &Route) -> Result<HttpResponse> {
    let imperial = s.imperial(actor);
    let from = s.place(&route.from).cloned().unwrap_or_default();
    let to = s.place(&route.to).cloned().unwrap_or_default();
    let mut els = chrome(s, p, actor, "");
    els.push(map_image(
        "dir-tile",
        &format!("{} → {}", from.name, to.name),
        720,
        280,
        &format!(
            "&route={}%7C{}%7C{}&sel={}",
            encode(&route.from),
            encode(&route.to),
            encode(&route.mode),
            encode(&route.to)
        ),
    ));
    els.push(web::spacer("dir-gap", 12));
    els.push(web::heading(
        "dir-title",
        format!("{} to {}", from.name, to.name),
    ));
    els.push(web::row(
        "dir-summary",
        12,
        "center",
        vec![
            web::badge(
                "dir-min",
                duration_label(route.minutes),
                web::style()
                    .size(18)
                    .bold()
                    .color("#ffffff")
                    .background(p.accent.clone())
                    .radius(8)
                    .padding(8),
            ),
            web::styled(
                "dir-dist",
                distance_label(route.metres, imperial),
                web::style().size(16).medium().color(p.ink.clone()),
            ),
            chip("dir-mode", &route.mode, p),
        ],
    ));
    els.push(web::spacer("sum-gap", 12));
    let modes: Vec<PageElement> = TRAVEL
        .iter()
        .map(|(mode, _, label)| {
            web::link(
                &format!("mode-{mode}"),
                *label,
                format!("/maps/dir?from={}&to={}&mode={mode}", route.from, route.to),
            )
        })
        .collect();
    els.push(web::row("dir-modes", 12, "center", modes));
    els.push(web::spacer("modes-gap", 12));
    els.push(web::divider("dir-rule"));
    for (i, step) in route.steps.iter().enumerate() {
        els.push(web::row(
            &format!("step-{i}"),
            12,
            "center",
            vec![
                web::badge(
                    &format!("step-{i}-n"),
                    format!("{}", i + 1),
                    web::style()
                        .size(12)
                        .bold()
                        .color("#ffffff")
                        .background(p.muted.clone())
                        .radius(10)
                        .padding(6)
                        .width(28)
                        .align("center"),
                ),
                web::styled(
                    &format!("step-{i}-text"),
                    &step.text,
                    web::style().size(14).color(p.ink.clone()),
                ),
            ],
        ));
        els.push(web::divider(&format!("step-{i}-rule")));
    }
    els.push(web::spacer("steps-gap", 12));
    els.push(web::link(
        "dir-to-place",
        format!("Open {}", to.name),
        format!("/maps/place/{}", to.id),
    ));
    els.extend(footer("foot", p));
    web::themed_page(
        &format!("{} to {}", from.name, to.name),
        s.theme.clone(),
        els,
    )
}
fn search_page(s: &GeoState, p: &Palette, actor: &str, q: &str) -> Result<HttpResponse> {
    let mut els = chrome(s, p, actor, q);
    els.push(web::heading(
        "title",
        if q.is_empty() {
            "Search".into()
        } else {
            format!("Results for “{q}”")
        },
    ));
    if s.maps() {
        let hits = s.find_places(q);
        if hits.is_empty() {
            els.push(web::styled(
                "empty",
                "No places matched.",
                web::style().size(14).color(p.muted.clone()),
            ));
        }
        let cards: Vec<PageElement> = hits
            .iter()
            .enumerate()
            .map(|(i, place)| place_card(&format!("r-{i}"), place, p))
            .collect();
        els.push(web::grid("results", 3, 12, cards));
    } else {
        let hits = s.find_cities(q);
        if hits.is_empty() {
            els.push(web::styled(
                "empty",
                "No locations matched.",
                web::style().size(14).color(p.muted.clone()),
            ));
        }
        for (i, f) in hits.iter().enumerate() {
            els.push(web::card_action(
                &format!("r-{i}"),
                web::style()
                    .background("#ffffff")
                    .border(p.surface.clone())
                    .radius(10)
                    .padding(12),
                web::visit(format!("/weather/today/l/{}", f.id)),
                vec![
                    web::styled(
                        &format!("r-{i}-city"),
                        &f.city,
                        web::style().size(16).bold().color(p.ink.clone()),
                    ),
                    web::styled(
                        &format!("r-{i}-now"),
                        format!("{} · {}", temp(f.now_f, s.imperial(actor)), f.cond),
                        web::style().size(13).color(p.muted.clone()),
                    ),
                ],
            ));
        }
    }
    els.extend(footer("foot", p));
    web::themed_page("Search", s.theme.clone(), els)
}
fn day_card(id: &str, d: &Day, imperial: bool, p: &Palette) -> PageElement {
    web::card(
        id,
        web::style()
            .background("#ffffff")
            .border(p.surface.clone())
            .radius(10)
            .padding(12),
        vec![
            web::styled(
                &format!("{id}-day"),
                &d.day,
                web::style().size(15).bold().color(p.ink.clone()),
            ),
            map_tile(&format!("{id}-icon"), &d.cond, 56, p),
            web::row(
                &format!("{id}-t"),
                8,
                "center",
                vec![
                    web::styled(
                        &format!("{id}-hi"),
                        temp(d.hi_f, imperial),
                        web::style().size(18).bold().color(p.ink.clone()),
                    ),
                    web::styled(
                        &format!("{id}-lo"),
                        temp(d.lo_f, imperial),
                        web::style().size(16).color(p.muted.clone()),
                    ),
                ],
            ),
            web::badge(
                &format!("{id}-precip"),
                format!("{}% rain", d.precip_pct),
                web::style()
                    .size(12)
                    .medium()
                    .color(p.accent.clone())
                    .background(p.surface.clone())
                    .radius(10)
                    .padding(6),
            ),
        ],
    )
}
fn units_form(actor_units: &str) -> PageElement {
    form_el(
        "units-form",
        act("POST", "/api/units", &[("units", "$units-value")]),
        vec![input(
            "units-value",
            "Units (f or c)",
            if actor_units == "metric" { "c" } else { "f" },
        )],
        "Switch units",
    )
}
fn weather_page(
    s: &GeoState,
    p: &Palette,
    actor: &str,
    city: &str,
    ten: bool,
) -> Result<HttpResponse> {
    let f = match s.forecast(city) {
        Ok(v) => v,
        Err(e) => return web::error(404, e),
    };
    let imperial = s.imperial(actor);
    let saved = s.saved_of(actor).iter().any(|x| x == city);
    let mut els = chrome(s, p, actor, "");
    for (i, a) in s.alerts_for(city, actor).iter().enumerate() {
        els.push(web::card(
            &format!("alert-{i}"),
            web::style()
                .background("#fdecea")
                .border("#d93025")
                .radius(8)
                .padding(12),
            vec![
                web::row(
                    &format!("alert-{i}-head"),
                    8,
                    "center",
                    vec![
                        web::badge(
                            &format!("alert-{i}-sev"),
                            &a.severity,
                            web::style()
                                .size(12)
                                .bold()
                                .color("#ffffff")
                                .background("#d93025")
                                .radius(10)
                                .padding(6),
                        ),
                        web::styled(
                            &format!("alert-{i}-title"),
                            &a.title,
                            web::style().size(15).bold().color("#1b1b1b"),
                        ),
                    ],
                ),
                web::styled(
                    &format!("alert-{i}-body"),
                    &a.body,
                    web::style().size(13).color("#5a5a5a"),
                ),
                form_el(
                    &format!("alert-{i}-ack"),
                    act("POST", &format!("/api/alerts/{}/ack", a.id), &[]),
                    vec![],
                    "Got it",
                ),
            ],
        ));
        els.push(web::spacer(&format!("alert-{i}-gap"), 8));
    }
    els.push(web::heading("title", &f.city));
    els.push(web::row(
        "now",
        16,
        "center",
        vec![
            map_tile("now-icon", &f.cond, 96, p),
            web::styled(
                "now-temp",
                temp(f.now_f, imperial),
                web::style().size(44).bold().color(p.ink.clone()),
            ),
            web::styled(
                "now-meta",
                format!(
                    "{} · {}% humidity · {} mph wind",
                    f.cond, f.humidity_pct, f.wind_mph
                ),
                web::style().size(14).color(p.muted.clone()),
            ),
        ],
    ));
    els.push(web::spacer("now-gap", 16));
    let days: Vec<&Day> = if ten {
        f.days.iter().collect()
    } else {
        f.days.iter().take(1).collect()
    };
    els.push(web::heading(
        "days-title",
        if ten { "10 day forecast" } else { "Today" },
    ));
    els.push(web::grid(
        "days",
        if ten { 5 } else { 1 },
        12,
        days.iter()
            .enumerate()
            .map(|(i, d)| day_card(&format!("d-{i}"), d, imperial, p))
            .collect(),
    ));
    els.push(web::spacer("days-gap", 16));
    els.push(web::row(
        "wx-nav",
        12,
        "center",
        vec![
            web::link("wx-today", "Today", format!("/weather/today/l/{city}")),
            web::link("wx-ten", "10 day", format!("/weather/tenday/l/{city}")),
        ],
    ));
    els.push(web::spacer("nav-gap", 12));
    els.push(form_el(
        "locations-form",
        act("POST", "/api/locations", &[("city", "$loc-city")]),
        vec![input("loc-city", "Save a location", city)],
        if saved {
            "Remove this location"
        } else {
            "Save this location"
        },
    ));
    els.push(web::spacer("loc-gap", 12));
    els.push(units_form(&s.units_for(actor)));
    if let Some(place) = s.places.get(&f.place) {
        els.push(web::spacer("place-gap", 12));
        els.push(web::link(
            "wx-place",
            format!("{} on the map", place.name),
            format!("/maps/place/{}", place.id),
        ));
    }
    els.extend(footer("foot", p));
    web::themed_page(&format!("{} weather", f.city), s.theme.clone(), els)
}
fn weather_home(s: &GeoState, p: &Palette, actor: &str) -> Result<HttpResponse> {
    let city = s.default_city(actor);
    if city.is_empty() {
        let mut els = chrome(s, p, actor, "");
        els.push(web::heading("title", &s.brand));
        els.push(web::styled(
            "empty",
            "No locations are seeded.",
            web::style().size(14).color(p.muted.clone()),
        ));
        els.extend(footer("foot", p));
        return web::themed_page(&s.brand, s.theme.clone(), els);
    }
    weather_page(s, p, actor, &city, false)
}
impl Service for GeoService {
    fn kind(&self) -> &str {
        "geo"
    }
    fn initialize(&self, initial: Value, _: &ServiceContext) -> Result<Value> {
        let gated = web::shape(initial, OBJECTS, ARRAYS)?;
        let mode = web::variant(&gated, "mode", MODES)?;
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
        let p = palette(&s.theme);
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
                        directions_page(&s, &p, &c.actor, &route)
                    }
                    Err(e) => web::error(400, e),
                };
            }
            return match parts.as_slice() {
                [""] | ["maps"] if s.maps() => maps_home(&s, &p, &c.actor),
                [""] => weather_home(&s, &p, &c.actor),
                ["map.rgba"] if s.maps() => map_response(&s, r),
                ["maps", "place", id] => place_page(&s, &p, &c.actor, id),
                ["maps", "saved"] => saved_page(&s, &p, &c.actor),
                ["maps", "notes"] => notes_page(&s, &p, &c.actor),
                ["search"] => {
                    search_page(&s, &p, &c.actor, &web::query(r, "q").unwrap_or_default())
                }
                ["weather", "today", "l", city] => weather_page(&s, &p, &c.actor, city, false),
                ["weather", "tenday", "l", city] => weather_page(&s, &p, &c.actor, city, true),
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
            ["maps", "place", id] => place_page(&s, &p, &c.actor, id),
            ["weather", "today", "l", city] => weather_page(&s, &p, &c.actor, city, false),
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
            assert_eq!(page, get(&mut state, url), "{url} must be pure");
            assert_eq!(before, state, "{url} must not mutate state");
        }
        assert!(body(&get(
            &mut state,
            "http://maps.google.com/search?q=convention"
        ))
        .contains("Cascade Convention Center"));
        assert_eq!(
            get(&mut state, "http://maps.google.com/maps/place/nope").status,
            404
        );
    }
    #[test]
    fn pages_carry_a_served_map_and_the_map_endpoint_draws_it() {
        let mut state = init(maps_seed());
        let home = body(&get(&mut state, "http://maps.google.com/"));
        assert!(home.contains("/map.rgba?w=720&h=280"), "{home}");
        let place = body(&get(
            &mut state,
            "http://maps.google.com/maps/place/northstar-hq",
        ));
        assert!(
            place.contains("/map.rgba?w=720&h=240&center=northstar-hq&zoom=3&sel=northstar-hq"),
            "{place}"
        );
        let dir = body(&get(
            &mut state,
            "http://maps.google.com/maps/dir?from=northstar-hq&to=devcon-center&mode=driving",
        ));
        assert!(
            dir.contains("/map.rgba?w=720&h=280&route=northstar-hq%7Cdevcon-center%7Cdriving"),
            "{dir}"
        );
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
        assert!(body(&page).contains("14 min"), "{}", body(&page));
        assert!(body(&page).contains("3.4 mi"));
        assert!(body(&page).contains("Head north on Bayfront Ave — 2.7 mi"));
        assert!(body(&page).contains("Turn east onto Pike St — 0.7 mi"));
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
        assert!(body(&get(&mut state, "http://maps.google.com/maps/saved")).contains("Cascade"));
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
        assert!(body(&page).contains("Loading dock entrance"));
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
        let home = get(&mut state, "http://weather.com/");
        assert!(body(&home).contains("Seattle, WA"));
        assert!(body(&home).contains("61°"), "{}", body(&home));
        assert!(body(&home).contains("Wind advisory"));
        let ten = get(&mut state, "http://weather.com/weather/tenday/l/seattle");
        assert!(body(&ten).contains("Tue"));
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
        assert!(body(&get(
            &mut state,
            "http://weather.com/weather/today/l/seattle"
        ))
        .contains("16°"));
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
        post(&mut state, "http://weather.com/api/alerts/wx-1/ack", &[]);
        let s: GeoState = web::load(&state).unwrap();
        assert_eq!(s.alerts[0].acked, vec!["alice".to_string()]);
        assert!(!body(&get(
            &mut state,
            "http://weather.com/weather/today/l/seattle"
        ))
        .contains("Wind advisory"));
        assert_eq!(
            post(&mut state, "http://weather.com/api/alerts/nope/ack", &[]).status,
            400
        );
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
