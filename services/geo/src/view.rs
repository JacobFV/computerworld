//! The pages as HTML. One dataset, three looks: `gmaps` is Google Maps (a full-viewport
//! map with a floating search pill, category chips, a side rail and a white place panel),
//! `osm` is OpenStreetMap (a white header with the green button group, a sidebar and the
//! map beside it), `weather` is a forecast site (a blue header and sub-navigation, the
//! current-conditions card, day parts, details and daily cards). Each skin has its own
//! stylesheet next to this file; the seed's palette rides on `<html>` as custom
//! properties.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `chrome`,
//! `wordmark`, `hdr-search` (`hdr-q`, `hdr-search-go`), `nav-home`, `nav-saved`,
//! `nav-notes`, `nav-today`, `nav-tenday`, `home-tile`, `place-tile`, `dir-tile`,
//! `dir-form` (`dir-from`, `dir-to`, `dir-mode`, `dir-form-go`), `home-place-<n>`,
//! `home-saved-<n>`, `saved-<n>`, `r-<n>`, `n-<n>` (each card one link, with `-tile`,
//! `-name`, `-addr`, `-kind`, `-rating` inside), `place-*`, `save-form`, `place-dir`,
//! `note-form`, `note-<n>`, `mode-<mode>`, `step-<n>`, `dir-to-place`, `alert-<n>`
//! (`alert-<n>-ack`), `now-*`, `d-<n>-*`, `wx-today`, `wx-ten`, `locations-form`,
//! `units-form`, `wx-place`, `foot`, and the place page's `zoom-in` / `zoom-out`.
//!
//! Nothing here is drawn as pressable unless it is: the map's `+` and `−` are links to
//! this page at the next zoom level and disappear into plain furniture at the ends of the
//! range, and the app launcher, the account menu and the layer and locate buttons the real
//! sites hang off their maps are not drawn at all, because this world has nothing behind
//! them. A tab that leads back to the page it is on says so with `aria-current="page"`.
use crate::{
    coord, distance_label, duration_label, encode, stars, temp, Day, Forecast, GeoState, Place, Route,
    MAP_ZOOM_MAX, TRAVEL,
};
use cw_protocol::{HttpResponse, Result};
use cw_service_common as web;
use cw_service_common::html::{
    button, div, el, empty, form, fragment, href, link, span, text_input, Document, Html as Node,
};

const GMAPS_CSS: &str = include_str!("gmaps.css");
const OSM_CSS: &str = include_str!("osm.css");
const WEATHER_CSS: &str = include_str!("weather.css");

/// The looks a seed may name with `skin`; absent, the mode and the brand decide.
pub(crate) const SKINS: &[&str] = &["", "gmaps", "osm", "weather"];

/// The map raster a maps page asks for: 8:5 for Google's full viewport, 4:3 for the pane
/// beside OpenStreetMap's sidebar. Both are inside `MAP_LIMIT` and scaled by the sheet.
fn map_size(skin: &str) -> (u32, u32) {
    if skin == "osm" {
        (640, 480)
    } else {
        (768, 480)
    }
}

/// Everything a page needs that is the same on every page of a site.
pub(crate) struct Chrome<'a> {
    s: &'a GeoState,
    actor: &'a str,
    skin: &'static str,
}
impl<'a> Chrome<'a> {
    pub(crate) fn new(s: &'a GeoState, actor: &'a str) -> Self {
        let skin = match s.skin.as_str() {
            "gmaps" => "gmaps",
            "osm" => "osm",
            "weather" => "weather",
            _ if !s.maps() => "weather",
            _ if s.brand.to_ascii_lowercase().contains("openstreetmap") => "osm",
            _ => "gmaps",
        };
        Self { s, actor, skin }
    }
    fn imperial(&self) -> bool {
        self.s.imperial(self.actor)
    }
    fn document(&self, title: &str, page: &str, body: Vec<Node>) -> Result<HttpResponse> {
        let t = &self.s.theme;
        let or = |v: &Option<String>, d: &str| v.clone().unwrap_or_else(|| d.to_owned());
        let (accent, paper) = match self.skin {
            "osm" => ("#7ebc6f", "#ffffff"),
            "weather" => ("#1b4de4", "#ffffff"),
            _ => ("#1a73e8", "#ffffff"),
        };
        let doc = Document::new(title)
            .lang("en")
            .stylesheet(match self.skin {
                "osm" => OSM_CSS,
                "weather" => WEATHER_CSS,
                _ => GMAPS_CSS,
            })
            .root_style(&format!(
                "--accent: {}; --ink: {}; --muted: {}; --surface: {}; --paper: {}",
                or(&t.accent, accent),
                or(&t.ink, "#202124"),
                or(&t.muted, "#5f6368"),
                or(&t.surface, "#e8eaed"),
                paper
            ))
            .body_class(&format!("skin-{} page-{page}", self.skin))
            .body(body);
        web::html::page(&doc)
    }
    /// The one search box: a GET form, so the results URL is the query.
    fn search(&self, query: &str) -> Node {
        let label = if self.s.maps() { "Search places" } else { "Search a city" };
        let placeholder = match self.skin {
            "gmaps" => format!("Search {}", self.s.brand),
            "osm" => "Search".to_owned(),
            _ => "Search City or Zip Code".to_owned(),
        };
        form("hdr-search", "/search", "get")
            .class("search")
            .attr("role", "search")
            .child(
                text_input("hdr-q", "q", query)
                    .attr("aria-label", label)
                    .attr("placeholder", placeholder)
                    .attr("autocomplete", "off"),
            )
            .child(
                button("hdr-search-go", "")
                    .class("go")
                    .attr("aria-label", "Search")
                    .child(span("mag").attr("aria-hidden", "true"))
                    .child(span("word").text(if self.skin == "osm" { "Go" } else { "Search" })),
            )
    }
    /// A tab in the site's nav. The current one still links to itself, the way every real
    /// tab strip does, and says so with `aria-current` so the agent is not misled.
    fn nav_link(id: &str, url: String, label: &str, icon: &str, on: bool) -> Node {
        el("a")
            .id(id)
            .class(if on { "nav on" } else { "nav" })
            .attr("href", url)
            .when(on, |n| n.attr("aria-current", "page"))
            .child(span(&format!("ico ico-{icon}")).attr("aria-hidden", "true"))
            .child(span("lbl").text(label))
    }
    /// Brand, the search box and the account routes: on every page, so the nav is real.
    fn header(&self, query: &str, page: &str) -> Node {
        let s = self.s;
        let brand = div("brand")
            .id("wordmark")
            .child(span("logo").attr("aria-hidden", "true").child(el("i")))
            .child(span("name").text(s.brand.as_str()));
        let nav = if s.maps() {
            el("nav")
                .class("navs")
                .child(Self::nav_link("nav-home", "/".into(), "Home", "home", page == "home"))
                .child(Self::nav_link("nav-saved", "/maps/saved".into(), "Your places", "saved", page == "saved"))
                .child(Self::nav_link("nav-notes", "/maps/notes".into(), "Notes", "notes", page == "notes"))
        } else {
            let city = s.default_city(self.actor);
            el("nav")
                .class("navs")
                .child(Self::nav_link("nav-home", "/".into(), "Home", "home", page == "home"))
                .child(Self::nav_link("nav-today", format!("/weather/today/l/{city}"), "Today", "today", page == "today"))
                .child(Self::nav_link("nav-tenday", format!("/weather/tenday/l/{city}"), "10 day", "ten", page == "tenday"))
                .child(Self::nav_link("nav-saved", "/maps/saved".into(), "Saved places", "saved", page == "saved"))
        };
        // Category chips (Google) are searches for a kind of place: real links to `/search`.
        let mut kinds: Vec<&str> = s.places.values().map(|p| p.kind.as_str()).filter(|k| !k.is_empty()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        let chips = if self.skin == "gmaps" {
            div("chips").id("chips").each(kinds.iter().take(7).enumerate(), |(i, kind)| {
                let on = query.eq_ignore_ascii_case(kind);
                el("a")
                    .id(format!("chip-{i}"))
                    .class(if on { "chip on" } else { "chip" })
                    .attr("href", href("/search", &[("q", kind)]))
                    .when(on, |n| n.attr("aria-current", "page"))
                    .child(span(&format!("dot k-{kind}")).attr("aria-hidden", "true"))
                    .child(span("lbl").text(plural(kind)))
            })
        } else {
            empty()
        };
        // Who is signed in, as an initial. The real sites hang an app launcher and an
        // account menu off it; neither exists in this world, so neither is drawn.
        let initial = self.actor.chars().next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default();
        let account = div("account")
            .attr("aria-hidden", "true")
            .child(span("avatar").text(initial));
        let top = el("header").id("chrome").class("top");
        if self.skin == "weather" {
            // The bar is centred over a full-width band; the sub-navigation is its own band.
            return top
                .child(div("bar").child(brand).child(self.search(query)).child(self.units_form()).child(account))
                .child(div("sub").child(nav));
        }
        if self.skin == "osm" {
            // The search row is fixed at the top of the sidebar, and the engine does not
            // render a fixed box nested in the fixed header, so it follows the header.
            return fragment([top.child(brand).child(nav).child(account), self.search(query)]);
        }
        top.child(brand).child(self.search(query)).child(nav).child(chips).child(account)
    }
    /// The map behind (Google) or beside (OpenStreetMap) the panel.
    ///
    /// `zoom` is the level the image is drawn at and the path the `+` and `−` controls
    /// step through; they are real links to the same page at the next level, and at the
    /// end of the range the control is drawn as plain furniture rather than as a link
    /// that would only redraw the picture it is already showing. A page whose frame is
    /// fixed by its data — every place at once, or both ends of a route — has no level
    /// to step, so it draws no zoom control at all.
    fn map(&self, id: &str, alt: &str, query: &str, zoom: Option<(u32, String)>) -> Node {
        let (w, h) = map_size(self.skin);
        let picture = div("map").id("map").child(
            el("img")
                .id(id)
                .attr("src", format!("/map.rgba?w={w}&h={h}{query}"))
                .attr("alt", alt)
                .attr("width", w.to_string())
                .attr("height", h.to_string()),
        );
        let Some((level, base)) = zoom else {
            return picture;
        };
        let step = |id: &str, class: &str, label: &str, glyph: &str, to: Option<u32>| match to {
            Some(level) => el("a")
                .id(id)
                .class(&format!("ctl {class}"))
                .attr("href", href(&base, &[("zoom", &level.to_string())]))
                .attr("aria-label", label)
                .text(glyph),
            None => span(&format!("ctl {class} off")).attr("aria-hidden", "true").text(glyph),
        };
        picture.child(
            div("controls")
                .child(step("zoom-in", "plus", "Zoom in", "+", (level < MAP_ZOOM_MAX).then(|| level + 1)))
                .child(step("zoom-out", "minus", "Zoom out", "−", level.checked_sub(1))),
        )
    }
    fn footer(&self) -> Node {
        el("footer").id("foot").class("foot").text(
            "Simulated geography in a training world. Every address, coordinate and forecast here \
             is invented, and no real mapping or weather service is contacted.",
        )
    }
    fn maps_page(&self, title: &str, page: &str, query: &str, map: Node, panel: Vec<Node>) -> Result<HttpResponse> {
        // In weather mode the place pages have no map to sit on: the panel is a card.
        if self.skin == "weather" {
            return self.weather_doc(title, page, query, vec![el("section").id("panel").class("card panel").children(panel)]);
        }
        let panel = el("main").id("panel").class("panel").children(panel).child(self.footer());
        self.document(title, page, vec![map, div("searchback").attr("aria-hidden", "true"), self.header(query, page), panel])
    }
    fn weather_doc(&self, title: &str, page: &str, query: &str, main: Vec<Node>) -> Result<HttpResponse> {
        let main = el("main").id("main").class("main").children(main);
        self.document(title, page, vec![self.header(query, page), main, self.footer()])
    }
    fn units_form(&self) -> Node {
        let metric = self.s.units_for(self.actor) == "metric";
        form("units-form", "/api/units", "post")
            .class("units")
            .child(
                text_input("units-value", "units", if metric { "c" } else { "f" })
                    .attr("aria-label", "Units (f or c)")
                    .attr("maxlength", "1"),
            )
            .child(button("units-form-go", "Switch units"))
    }
}

/// "cafe" -> "Cafes": the chip reads as a category.
fn plural(kind: &str) -> String {
    let mut chars = kind.chars();
    let head = chars.next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default();
    format!("{head}{}s", chars.as_str())
}
/// Five stars with the rated share in gold: a grey row under a clipped gold row.
fn star_row(tenths: i64) -> Node {
    span("stars")
        .attr("aria-hidden", "true")
        .text("★★★★★")
        .child(el("i").style(&format!("width: {}%", (tenths * 2).clamp(0, 100))).text("★★★★★"))
}
/// A place as one link: name, rating, kind and address on the left, a tinted tile on the
/// right, the way a results list reads.
fn place_card(id: &str, place: &Place) -> Node {
    let text = span("text")
        .child(span("name").id(format!("{id}-name")).text(place.name.as_str()))
        .child(
            span("meta")
                .id(format!("{id}-meta"))
                .when(place.rating > 0, |m| {
                    m.child(span("rating").id(format!("{id}-rating")).text(stars(place.rating)).child(span("glyph").text(" ★")))
                        .child(star_row(place.rating))
                        .child(span("count").text(format!("({})", place.reviews)))
                })
                .child(span("kind").id(format!("{id}-kind")).text(place.kind.as_str())),
        )
        .child(span("addr").id(format!("{id}-addr")).text(place.address.as_str()))
        .when(!place.hours.is_empty(), |t| t.child(span("hours").text(place.hours.as_str())));
    el("a")
        .id(id)
        .class("place")
        .attr("href", format!("/maps/place/{}", place.id))
        .child(text)
        .child(
            span(&format!("tile k-{}", place.kind))
                .id(format!("{id}-tile"))
                .attr("aria-label", format!("{} · map", place.name))
                .child(span("pin")),
        )
}
fn city_card(id: &str, f: &Forecast, imperial: bool) -> Node {
    el("a")
        .id(id)
        .class("city")
        .attr("href", format!("/weather/today/l/{}", f.id))
        .child(icon("", &f.cond))
        .child(span("name").id(format!("{id}-city")).text(f.city.as_str()))
        .child(span("now").id(format!("{id}-now")).text(format!("{} · {}", temp(f.now_f, imperial), f.cond)))
}
/// The two ends and the travel mode, as a GET form, so the route's URL is the question.
/// Whatever was already typed comes back in the fields rather than being thrown away.
fn dir_form(id: &str, prefix: &str, from: &str, to: &str, mode: &str, mode_label: &str, submit: &str) -> Node {
    let field = |name: &str, label: &str, value: &str, dot: &str| {
        let fid = format!("{prefix}-{name}");
        div("field")
            .child(span(&format!("dot {dot}")).attr("aria-hidden", "true"))
            .child(el("label").attr("for", fid.as_str()).text(label))
            .child(text_input(&fid, name, value).attr("placeholder", label))
    };
    form(id, "/maps/dir", "get")
        .class("dirform")
        .child(field("from", "From (place id)", from, "start"))
        .child(field("to", "To (place id)", to, "end"))
        .child(field("mode", mode_label, mode, "how"))
        .child(button(&format!("{id}-go"), submit).class("primary"))
}

/// What the mode field is called where there is room to name the four modes; the place
/// panel, which has less, just says "Mode".
const MODE_LABEL: &str = "Mode (driving, transit, cycling, walking)";

/// `/maps/dir` with an end still blank. The form's own action has to lead somewhere a
/// person can act, so it leads back to the form with what was filled in kept, rather than
/// to an error page.
pub(crate) fn directions_prompt(s: &GeoState, actor: &str, from: &str, to: &str, mode: &str) -> Result<HttpResponse> {
    let c = Chrome::new(s, actor);
    let panel = vec![
        el("h1").id("title").class("title").text("Directions"),
        el("p").id("dir-hint").class("hint").text(
            "Name both ends to see the route. A place id is the last part of its address, \
             which every place page shows.",
        ),
        el("section").class("block").child(dir_form("dir-form", "dir", from, to, mode, MODE_LABEL, "Directions")),
    ];
    let map = c.map("map-tile", "Map of every place", "", None);
    c.maps_page("Directions", "dir", "", map, panel)
}

pub(crate) fn maps_home(s: &GeoState, actor: &str) -> Result<HttpResponse> {
    let c = Chrome::new(s, actor);
    let saved = s.saved_of(actor);
    let saved_places: Vec<&Place> = saved.iter().filter_map(|id| s.places.get(id)).collect();
    let mut panel = vec![
        el("h1").id("title").class("title").text(s.brand.as_str()),
        if s.tagline.is_empty() { empty() } else { el("p").id("tagline").class("lead").text(s.tagline.as_str()) },
        el("section").class("block").child(el("h2").class("h").text("Directions")).child(dir_form(
            "dir-form",
            "dir",
            saved.first().map_or("", String::as_str),
            "",
            "driving",
            MODE_LABEL,
            "Directions",
        )),
    ];
    if saved_places.is_empty() {
        panel.push(el("p").id("saved-empty").class("hint").text("Your places is empty. Open a place and save it."));
    } else {
        panel.push(
            el("section")
                .class("block")
                .child(el("h2").id("saved-title").class("h").text("Your places"))
                .child(div("list").id("saved-grid").each(saved_places.iter().enumerate(), |(i, p)| {
                    place_card(&format!("home-saved-{i}"), p)
                })),
        );
    }
    panel.push(
        el("section")
            .class("block")
            .child(el("h2").id("feat-title").class("h").text("Nearby"))
            .child(div("list").id("feat-grid").each(s.places.values().take(6).enumerate(), |(i, p)| {
                place_card(&format!("home-place-{i}"), p)
            })),
    );
    // The home map opens on the city: the person's first saved place, else the first seeded
    // one, at the widest zoom. With no places at all it is the empty world.
    let centre = saved_places.first().map(|p| p.id.as_str()).or_else(|| s.places.keys().next().map(String::as_str));
    let view = centre.map_or(String::new(), |id| format!("&center={}&zoom=0", encode(id)));
    let map = c.map("home-tile", "Map of every place · pick one below", &view, None);
    c.maps_page(&s.brand, "home", "", map, panel)
}

pub(crate) fn saved_page(s: &GeoState, actor: &str) -> Result<HttpResponse> {
    let c = Chrome::new(s, actor);
    let ids = s.saved_of(actor);
    let title = el("h1").id("title").class("title").text("Your places");
    if !s.maps() {
        let main = vec![el("section")
            .class("card")
            .child(title)
            .when(ids.is_empty(), |n| n.child(el("p").id("empty").class("hint").text("Nothing saved yet.")))
            .child(div("cities").each(
                ids.iter().enumerate().filter_map(|(i, id)| s.forecasts.get(id).map(|f| (i, f))),
                |(i, f)| city_card(&format!("saved-{i}"), f, c.imperial()),
            ))];
        return c.weather_doc("Your places", "saved", "", main);
    }
    let panel = vec![
        title,
        if ids.is_empty() {
            el("p").id("empty").class("hint").text("Nothing saved yet.")
        } else {
            div("list").id("saved-grid").each(ids.iter().filter_map(|id| s.places.get(id)).enumerate(), |(i, p)| {
                place_card(&format!("saved-{i}"), p)
            })
        },
    ];
    let map = c.map("map-tile", "Map of every place", "", None);
    c.maps_page("Your places", "saved", "", map, panel)
}

/// `zoom` is the level the map is framed at, which `?zoom=` on this page's own URL sets
/// and the `+` and `−` controls step; out of range it falls back to the default.
pub(crate) fn place_page(s: &GeoState, actor: &str, id: &str, zoom: u32) -> Result<HttpResponse> {
    let place = match s.place(id) {
        Ok(v) => v,
        Err(e) => return web::error(404, e),
    };
    let c = Chrome::new(s, actor);
    let saved = s.saved_of(actor).iter().any(|x| x == id);
    let zoom = zoom.min(MAP_ZOOM_MAX);
    let map = c.map(
        "place-tile",
        &format!("{} · {}, {}", place.name, coord(place.lat), coord(place.lon)),
        &format!("&center={id}&zoom={zoom}&sel={id}", id = encode(&place.id)),
        Some((zoom, format!("/maps/place/{}", encode(&place.id)))),
    );
    let fact = |fid: &str, ico: &str, body: Node| div("fact").child(span(&format!("ico ico-{ico}")).attr("aria-hidden", "true")).child(body.id(fid));
    let mut facts = div("facts")
        .id("place-card")
        .child(fact("place-addr", "pin", span("v").text(place.address.as_str())))
        .child(fact("place-coord", "grid", span("v dim").text(format!("{}, {}", coord(place.lat), coord(place.lon)))));
    if !place.hours.is_empty() {
        facts = facts.child(fact("place-hours", "clock", span("v").text(format!("Hours: {}", place.hours))));
    }
    if !place.phone.is_empty() {
        facts = facts.child(fact("place-phone", "phone", span("v").text(format!("Phone: {}", place.phone))));
    }
    if !place.website.is_empty() {
        facts = facts.child(fact("place-site", "globe", el("a").class("v").attr("href", place.website.as_str()).text(place.website.as_str())));
    }
    let notes = s.notes_at(id);
    let panel = vec![
        div(&format!("hero k-{}", place.kind)).attr("aria-hidden", "true").child(span("pin")),
        div("headline")
            .child(el("h1").id("place-name").class("title").text(place.name.as_str()))
            .child(
                div("meta")
                    .id("place-meta")
                    .when(place.rating > 0, |m| {
                        m.child(
                            span("rating")
                                .id("place-rating")
                                .text(format!("{} ★ · {} reviews", stars(place.rating), place.reviews)),
                        )
                        .child(star_row(place.rating))
                    })
                    .child(span("kind").id("place-kind").text(place.kind.as_str()))
                    .each(place.tags.iter().enumerate(), |(i, tag)| span("tag").id(format!("place-tag-{i}")).text(tag.as_str())),
            ),
        div("actions")
            .child(
                el("a")
                    .id("place-dir-jump")
                    .class("act primary")
                    .attr("href", "#place-dir")
                    .child(span("ico ico-dir").attr("aria-hidden", "true"))
                    .child(span("lbl").text("Directions")),
            )
            .child(
                form("save-form", format!("/api/places/{id}/save"), "post").class("act-form").child(
                    button("save-form-go", "")
                        .class(if saved { "act on" } else { "act" })
                        .child(span("ico ico-saved").attr("aria-hidden", "true"))
                        .child(span("lbl").text(if saved { "Remove from Your places" } else { "Save to Your places" })),
                ),
            ),
        if place.summary.is_empty() { empty() } else { el("p").id("place-summary").class("summary").text(place.summary.as_str()) },
        facts,
        el("section").class("block").child(el("h2").id("dir-title").class("h").text("Directions from here")).child(dir_form(
            "place-dir",
            "place-dir",
            id,
            "",
            "driving",
            "Mode",
            "Get directions",
        )),
        el("section")
            .class("block")
            .child(el("h2").id("note-title").class("h").text("Notes"))
            .when(notes.is_empty(), |n| n.child(el("p").id("note-empty").class("hint").text("No notes on this place.")))
            .each(notes.iter().enumerate(), |(i, n)| {
                div("note")
                    .id(format!("note-{i}"))
                    .child(span("who").id(format!("note-{i}-who")).text(format!("{} · tick {}", n.author, n.tick)))
                    .child(span("body").id(format!("note-{i}-text")).text(n.text.as_str()))
            })
            .child(
                form("note-form", "/api/notes", "post")
                    .class("noteform")
                    .child(el("label").attr("for", "note-place").text("Place"))
                    .child(text_input("note-place", "place", id))
                    .child(el("label").attr("for", "note-text").text("What is wrong here?"))
                    .child(text_input("note-text", "text", "").attr("placeholder", "What is wrong here?"))
                    .child(button("note-form-go", "Add a note").class("primary")),
            ),
    ];
    c.maps_page(&place.name, "place", "", map, panel)
}

pub(crate) fn notes_page(s: &GeoState, actor: &str) -> Result<HttpResponse> {
    let c = Chrome::new(s, actor);
    let panel = vec![
        el("h1").id("title").class("title").text("Map notes"),
        if s.notes.is_empty() { el("p").id("empty").class("hint").text("No notes yet.") } else { empty() },
        div("list").each(s.notes.iter().enumerate(), |(i, n)| {
            let name = s.places.get(&n.place).map_or(n.place.clone(), |x| x.name.clone());
            el("a")
                .id(format!("n-{i}"))
                .class("notecard")
                .attr("href", format!("/maps/place/{}", n.place))
                .child(span("marker").attr("aria-hidden", "true"))
                .child(
                    span("text")
                        .child(span("name").id(format!("n-{i}-where")).text(name))
                        .child(span("body").id(format!("n-{i}-text")).text(n.text.as_str()))
                        .child(span("who").id(format!("n-{i}-meta")).text(format!(
                            "{} · {}, {} · tick {}",
                            n.author,
                            coord(n.lat),
                            coord(n.lon),
                            n.tick
                        ))),
                )
        }),
    ];
    let map = c.map("map-tile", "Map of every place", "", None);
    c.maps_page("Map notes", "notes", "", map, panel)
}

pub(crate) fn directions_page(s: &GeoState, actor: &str, route: &Route) -> Result<HttpResponse> {
    let c = Chrome::new(s, actor);
    let from = s.place(&route.from).cloned().unwrap_or_default();
    let to = s.place(&route.to).cloned().unwrap_or_default();
    let map = c.map(
        "dir-tile",
        &format!("{} → {}", from.name, to.name),
        &format!(
            "&route={}%7C{}%7C{}&sel={}",
            encode(&route.from),
            encode(&route.to),
            encode(&route.mode),
            encode(&route.to)
        ),
        None,
    );
    let last = route.steps.len().saturating_sub(1);
    let panel = vec![
        div("dirhead")
            .child(el("nav").id("dir-modes").class("modes").each(TRAVEL.iter(), |(mode, _, label)| {
                el("a")
                    .id(format!("mode-{mode}"))
                    .class(if *mode == route.mode { "mode on" } else { "mode" })
                    .attr("href", format!("/maps/dir?from={}&to={}&mode={mode}", route.from, route.to))
                    .when(*mode == route.mode, |n| n.attr("aria-current", "page"))
                    .child(span(&format!("ico ico-{mode}")).attr("aria-hidden", "true"))
                    .child(span("lbl").text(*label))
            }))
            .child(
                div("ends")
                    .child(div("leg").child(span("dot start")).child(span("v").text(from.name.as_str())))
                    .child(div("leg").child(span("dot end")).child(span("v").text(to.name.as_str()))),
            ),
        el("h1").id("dir-title").class("title small").text(format!("{} to {}", from.name, to.name)),
        div("summary")
            .id("dir-summary")
            .child(span("min").id("dir-min").text(duration_label(route.minutes)))
            .child(span("dist").id("dir-dist").text(distance_label(route.metres, c.imperial())))
            .child(span("kind").id("dir-mode").text(route.mode.as_str())),
        el("ol").id("dir-rule").class("steps").each(route.steps.iter().enumerate(), |(i, step)| {
            el("li")
                .id(format!("step-{i}"))
                .class(if i == 0 || i == last { "step cap" } else { "step" })
                .child(span("n").id(format!("step-{i}-n")).text((i + 1).to_string()))
                .child(span("t").id(format!("step-{i}-text")).text(step.text.as_str()))
        }),
        link("dir-to-place", format!("/maps/place/{}", to.id), format!("Open {}", to.name)).class("more"),
    ];
    c.maps_page(&format!("{} to {}", from.name, to.name), "dir", "", map, panel)
}

pub(crate) fn search_page(s: &GeoState, actor: &str, q: &str) -> Result<HttpResponse> {
    let c = Chrome::new(s, actor);
    let title = el("h1").id("title").class("title").text(if q.is_empty() { "Search".to_owned() } else { format!("Results for “{q}”") });
    if s.maps() {
        let hits = s.find_places(q);
        let panel = vec![
            title,
            if hits.is_empty() { el("p").id("empty").class("hint").text("No places matched.") } else { empty() },
            div("list").id("results").each(hits.iter().enumerate(), |(i, p)| place_card(&format!("r-{i}"), p)),
        ];
        let map = c.map("map-tile", "Map of every place", "", None);
        return c.maps_page("Search", "search", q, map, panel);
    }
    let hits = s.find_cities(q);
    let main = vec![el("section")
        .class("card")
        .child(title)
        .when(hits.is_empty(), |n| n.child(el("p").id("empty").class("hint").text("No locations matched.")))
        .child(div("cities").id("results").each(hits.iter().enumerate(), |(i, f)| city_card(&format!("r-{i}"), f, c.imperial())))];
    c.weather_doc("Search", "search", q, main)
}

/// A CSS-drawn weather icon chosen from the condition's words; the words are its label.
fn icon(id: &str, cond: &str) -> Node {
    let c = cond.to_ascii_lowercase();
    let kind = if c.contains("thunder") || c.contains("storm") {
        "storm"
    } else if c.contains("snow") || c.contains("sleet") {
        "snow"
    } else if c.contains("rain") || c.contains("shower") || c.contains("drizzle") {
        "rain"
    } else if c.contains("partly") || c.contains("mostly") {
        "partly"
    } else if c.contains("cloud") || c.contains("overcast") {
        "cloud"
    } else if c.contains("fog") || c.contains("mist") || c.contains("haze") {
        "fog"
    } else {
        "sun"
    };
    let node = span(&format!("wx wx-{kind}"))
        .attr("role", "img")
        .attr("aria-label", cond)
        .child(span("sun"))
        .child(span("cloud"))
        .child(span("drops"));
    if id.is_empty() {
        node
    } else {
        node.id(id)
    }
}
/// A day as a column (the five-day strip) or a row (the ten-day list); the sheet decides.
fn day_card(id: &str, d: &Day, imperial: bool, first: bool) -> Node {
    div(if first { "day on" } else { "day" })
        .id(id)
        .child(span("name").child(span("dow").id(format!("{id}-day")).text(d.day.as_str())).when(first, |n| n.child(span("today").text(" · Today"))))
        .child(
            span("temps")
                .id(format!("{id}-t"))
                .child(span("hi").id(format!("{id}-hi")).text(temp(d.hi_f, imperial)))
                .child(span("lo").id(format!("{id}-lo")).text(temp(d.lo_f, imperial))),
        )
        .child(icon(&format!("{id}-icon"), &d.cond))
        .child(span("cond").id(format!("{id}-cond")).text(d.cond.as_str()))
        .child(span("precip").id(format!("{id}-precip")).child(span("drop").attr("aria-hidden", "true")).text(format!("{}% rain", d.precip_pct)))
}

pub(crate) fn weather_page(s: &GeoState, actor: &str, city: &str, ten: bool) -> Result<HttpResponse> {
    weather_view(s, actor, city, ten, false)
}
/// `home` is the bare `/`, which shows the same forecast as the Today page but at its own
/// address: the Home tab is the one you are standing on there, and the Today and 10 day
/// pills lead somewhere you are not, so neither is marked as the page you are reading.
fn weather_view(s: &GeoState, actor: &str, city: &str, ten: bool, home: bool) -> Result<HttpResponse> {
    let f = match s.forecast(city) {
        Ok(v) => v,
        Err(e) => return web::error(404, e),
    };
    let c = Chrome::new(s, actor);
    let imperial = c.imperial();
    let saved = s.saved_of(actor).iter().any(|x| x == city);
    let today = f.days.first().cloned().unwrap_or_default();
    let mut main: Vec<Node> = s
        .alerts_for(city, actor)
        .iter()
        .enumerate()
        .map(|(i, a)| {
            div("alert")
                .id(format!("alert-{i}"))
                .child(
                    div("head")
                        .id(format!("alert-{i}-head"))
                        .child(span("mark").attr("aria-hidden", "true").text("!"))
                        .child(span("sev").id(format!("alert-{i}-sev")).text(a.severity.as_str()))
                        .child(span("what").id(format!("alert-{i}-title")).text(a.title.as_str())),
                )
                .child(el("p").id(format!("alert-{i}-body")).class("body").text(a.body.as_str()))
                .child(
                    form(&format!("alert-{i}-ack"), format!("/api/alerts/{}/ack", a.id), "post")
                        .child(button(&format!("alert-{i}-ack-go"), "Got it")),
                )
        })
        .collect();
    let mut cards: Vec<Node> = Vec::new();
    // Current conditions: the city over a sky band, the big number, the day's range.
    cards.push(
        el("section")
            .class(&format!("card now sky-{}", if f.cond.to_ascii_lowercase().contains("sun") { "clear" } else { "grey" }))
            .child(
                div("band")
                    .child(el("h1").id("title").class("where").text(f.city.as_str()))
                    .child(span("asof").text("Current conditions")),
            )
            .child(
                div("body")
                    .id("now")
                    .child(
                        div("read")
                            .child(span("temp").id("now-temp").text(temp(f.now_f, imperial)))
                            .child(span("cond").text(f.cond.as_str()))
                            .child(span("range").id("now-range").text(format!(
                                "Day {} • Night {}",
                                temp(today.hi_f, imperial),
                                temp(today.lo_f, imperial)
                            ))),
                    )
                    .child(icon("now-icon", &f.cond)),
            )
            .child(el("p").id("now-meta").class("meta").text(format!(
                "{} · {}% humidity · {} mph wind",
                f.cond, f.humidity_pct, f.wind_mph
            ))),
    );
    if !ten {
        // The day in four parts, read off the day's high and low: integers, the same every replay.
        let (hi, lo) = (today.hi_f, today.lo_f);
        let parts = [
            ("Morning", lo + (hi - lo) / 3),
            ("Afternoon", hi),
            ("Evening", (hi + lo) / 2),
            ("Overnight", lo),
        ];
        cards.push(
            el("section")
                .class("card")
                .child(el("h2").class("h").text(format!("Today's Forecast for {}", f.city)))
                .child(div("parts").id("parts").each(parts.iter().enumerate(), |(i, (name, t))| {
                    div("part")
                        .id(format!("part-{i}"))
                        .child(span("name").text(*name))
                        .child(span("t").text(temp(*t, imperial)))
                        .child(icon("", &today.cond))
                        .child(span("precip").child(span("drop").attr("aria-hidden", "true")).text(format!("{}%", today.precip_pct)))
                })),
        );
        let detail = |id: &str, ico: &str, name: &str, value: String| {
            div("detail").id(id).child(span(&format!("ico ico-{ico}")).attr("aria-hidden", "true")).child(span("k").text(name)).child(span("v").text(value))
        };
        cards.push(
            el("section")
                .class("card")
                .child(el("h2").class("h").text(format!("Weather Today in {}", f.city)))
                .child(
                    div("details")
                        .id("details")
                        .child(detail("det-range", "temp", "High / Low", format!("{} / {}", temp(today.hi_f, imperial), temp(today.lo_f, imperial))))
                        .child(detail("det-humidity", "humid", "Humidity", format!("{}%", f.humidity_pct)))
                        .child(detail("det-wind", "wind", "Wind", format!("{} mph", f.wind_mph)))
                        .child(detail("det-precip", "rain", "Chance of rain", format!("{}%", today.precip_pct))),
                ),
        );
    }
    let days: Vec<&Day> = if ten { f.days.iter().collect() } else { f.days.iter().take(5).collect() };
    cards.push(
        el("section")
            .class(if ten { "card daily ten" } else { "card daily" })
            .child(el("h2").id("days-title").class("h").text(if ten { "10 day forecast" } else { "Daily Forecast" }))
            .child(div("days").id("days").each(days.iter().enumerate(), |(i, d)| day_card(&format!("d-{i}"), d, imperial, i == 0)))
            .child(
                div("pills")
                    .id("wx-nav")
                    .child(
                        link("wx-today", format!("/weather/today/l/{city}"), "Today")
                            .class(if !ten && !home { "pill on" } else { "pill" })
                            .when(!ten && !home, |n| n.attr("aria-current", "page")),
                    )
                    .child(
                        link("wx-ten", format!("/weather/tenday/l/{city}"), "10 day")
                            .class(if ten { "pill on" } else { "pill" })
                            .when(ten, |n| n.attr("aria-current", "page")),
                    ),
            ),
    );
    let side =
        el("section")
            .class("card tools")
            .child(el("h2").class("h").text("Your locations"))
            .child(
                form("locations-form", "/api/locations", "post")
                    .class("locform")
                    .child(el("label").attr("for", "loc-city").text("Save a location"))
                    .child(text_input("loc-city", "city", city))
                    .child(button("locations-form-go", if saved { "Remove this location" } else { "Save this location" }).class("primary")),
            )
            .maybe(s.places.get(&f.place).map(|place| {
                link("wx-place", format!("/maps/place/{}", place.id), format!("{} on the map", place.name)).class("more")
            }));
    main.push(div("cols").child(div("col-main").children(cards)).child(el("aside").class("col-side").child(side)));
    let page = match (home, ten) {
        (true, _) => "home",
        (false, true) => "tenday",
        (false, false) => "today",
    };
    c.weather_doc(&format!("{} weather", f.city), page, "", main)
}

pub(crate) fn weather_home(s: &GeoState, actor: &str) -> Result<HttpResponse> {
    let city = s.default_city(actor);
    if city.is_empty() {
        let c = Chrome::new(s, actor);
        let main = vec![el("section")
            .class("card")
            .child(el("h1").id("title").class("title").text(s.brand.as_str()))
            .child(el("p").id("empty").class("hint").text("No locations are seeded."))];
        return c.weather_doc(&s.brand, "home", "", main);
    }
    weather_view(s, actor, &city, false, true)
}

